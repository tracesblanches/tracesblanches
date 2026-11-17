//! Pack reader: manifest, layers, tiles, raster values. No network, no SQLite: this is
//! the part the phone core links.

use std::fs::File;
use std::io::Read;
use std::path::{Component, Path, PathBuf};

use pmtiles::{NoCache, PmTilesReader, TileCoord, TileType};
use sha2::{Digest, Sha256};
use skimap_common::geom::LonLat;

use crate::error::{Error, Result};
use crate::manifest::{FORMAT, FORMAT_VERSION, Layer, MANIFEST_FILE, Manifest, ValueEncoding};
use crate::png;
use crate::tiles;

/// An opened pack directory.
#[derive(Debug, Clone)]
pub struct Pack {
    root: PathBuf,
    manifest: Manifest,
}

/// Checks that a manifest path is relative and stays inside the pack.
pub fn check_relative(rel: &str) -> Result<()> {
    let p = Path::new(rel);
    let ok = !rel.is_empty()
        && !rel.contains('\\')
        && p.components().all(|c| matches!(c, Component::Normal(_)));
    if ok {
        Ok(())
    } else {
        Err(Error::Pack(format!(
            "path {rel:?} is not a relative path inside the pack"
        )))
    }
}

impl Pack {
    /// Opens the pack at `root` (directory containing `manifest.json`).
    pub fn open(root: impl Into<PathBuf>) -> Result<Self> {
        let root = root.into();
        let path = root.join(MANIFEST_FILE);
        let text = std::fs::read_to_string(&path).map_err(|e| Error::io(&path, e))?;
        let manifest: Manifest =
            serde_json::from_str(&text).map_err(|e| Error::json(path.display().to_string(), e))?;
        if manifest.format != FORMAT {
            return Err(Error::Pack(format!(
                "format {:?}, expected {FORMAT:?}",
                manifest.format
            )));
        }
        if manifest.format_version == 0 || manifest.format_version > FORMAT_VERSION {
            return Err(Error::Pack(format!(
                "format version {} not supported (this reader reads 1 to {FORMAT_VERSION})",
                manifest.format_version
            )));
        }
        Ok(Self { root, manifest })
    }

    /// Pack root directory.
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// The manifest.
    pub fn manifest(&self) -> &Manifest {
        &self.manifest
    }

    /// Layer by identifier.
    pub fn layer(&self, id: &str) -> Result<&Layer> {
        self.manifest
            .layer(id)
            .ok_or_else(|| Error::Pack(format!("no layer {id:?} in the pack")))
    }

    /// Absolute path of a file of the pack.
    pub fn path(&self, rel: &str) -> Result<PathBuf> {
        check_relative(rel)?;
        Ok(self.root.join(rel))
    }

    /// Raw bytes of a pack file.
    pub fn read_file(&self, rel: &str) -> Result<Vec<u8>> {
        let p = self.path(rel)?;
        std::fs::read(&p).map_err(|e| Error::io(&p, e))
    }

    /// Main file of a JSON or GeoJSON layer, parsed.
    pub fn read_json(&self, id: &str) -> Result<serde_json::Value> {
        let l = self.layer(id)?;
        if l.format != "json" && l.format != "geojson" {
            return Err(Error::Pack(format!(
                "layer {id:?} is {}, not JSON",
                l.format
            )));
        }
        let bytes = self.read_file(&l.file)?;
        serde_json::from_slice(&bytes).map_err(|e| Error::json(&l.file, e))
    }

    /// Days of the hourly forecast the pack carries (`meteo-hourly`, first point), those
    /// with 8 hours or more, `AAAA-MM-JJ`, in order; empty without it.
    pub fn forecast_days(&self) -> Vec<String> {
        let Ok(v) = self.read_json("meteo-hourly") else {
            return Vec::new();
        };
        let mut count = std::collections::BTreeMap::<String, usize>::new();
        for p in v["points"].as_array().into_iter().flatten().take(1) {
            for h in p["hours"].as_array().into_iter().flatten() {
                if let Some(d) = h["time"].as_str().and_then(|s| s.get(..10)) {
                    *count.entry(d.to_owned()).or_default() += 1;
                }
            }
        }
        count
            .into_iter()
            .filter(|(_, n)| *n >= 8)
            .map(|(d, _)| d)
            .collect()
    }

    /// Features of a GeoJSON layer.
    pub fn read_geojson(&self, id: &str) -> Result<geojson::FeatureCollection> {
        let l = self.layer(id)?;
        if l.format != "geojson" {
            return Err(Error::Pack(format!(
                "layer {id:?} is {}, not GeoJSON",
                l.format
            )));
        }
        let bytes = self.read_file(&l.file)?;
        let text = String::from_utf8(bytes).map_err(|e| Error::GeoJson {
            context: l.file.clone(),
            message: e.to_string(),
        })?;
        text.parse::<geojson::FeatureCollection>()
            .map_err(|e| Error::GeoJson {
                context: l.file.clone(),
                message: e.to_string(),
            })
    }

    /// Tile archive of a raster layer or of a basemap.
    pub fn tiles(&self, id: &str) -> Result<TileArchive> {
        let rel = if let Some(b) = self.manifest.basemap(id) {
            b.tiles.clone()
        } else {
            let l = self.layer(id)?;
            if l.format != "pmtiles" {
                return Err(Error::Pack(format!("layer {id:?} has no tiles")));
            }
            l.file.clone()
        };
        TileArchive::open(self.path(&rel)?)
    }

    /// Class value of a palette raster layer at a position, read at the layer's highest
    /// zoom. `Ok(None)`: outside the tiles or no data there.
    pub fn raster_value_at(&self, id: &str, p: LonLat) -> Result<Option<u8>> {
        let l = self.layer(id)?;
        let r = l
            .raster
            .as_ref()
            .ok_or_else(|| Error::Pack(format!("layer {id:?} is not a raster")))?;
        if r.encoding != ValueEncoding::PaletteIndex {
            return Err(Error::Pack(format!(
                "layer {id:?} is a third-party RGBA raster: no values to read"
            )));
        }
        let archive = self.tiles(id)?;
        archive.palette_index_at(p, r.maxzoom, r.nodata_index)
    }

    /// Checks every file of the manifest (size and SHA-256). Returns the problems found.
    pub fn verify(&self) -> Result<Vec<String>> {
        let mut problems = Vec::new();
        let mut total = 0u64;
        for f in &self.manifest.files {
            let p = self.path(&f.path)?;
            match sha256_file(&p) {
                Ok((bytes, sha)) => {
                    total += bytes;
                    if bytes != f.bytes {
                        problems.push(format!(
                            "{}: {} bytes, manifest says {}",
                            f.path, bytes, f.bytes
                        ));
                    }
                    if sha != f.sha256 {
                        problems.push(format!("{}: SHA-256 differs from the manifest", f.path));
                    }
                }
                Err(e) => problems.push(format!("{}: {e}", f.path)),
            }
        }
        if total != self.manifest.total_bytes {
            problems.push(format!(
                "total {total} bytes, manifest says {}",
                self.manifest.total_bytes
            ));
        }
        for l in &self.manifest.layers {
            if !self.manifest.files.iter().any(|f| f.path == l.file) {
                problems.push(format!("layer {}: file {} not listed", l.id, l.file));
            }
        }
        Ok(problems)
    }
}

/// Size and SHA-256 (lowercase hex) of a file.
pub fn sha256_file(path: &Path) -> Result<(u64, String)> {
    let mut f = File::open(path).map_err(|e| Error::io(path, e))?;
    let mut h = Sha256::new();
    let mut buf = vec![0u8; 1 << 16];
    let mut n = 0u64;
    loop {
        let k = f.read(&mut buf).map_err(|e| Error::io(path, e))?;
        if k == 0 {
            break;
        }
        h.update(&buf[..k]);
        n += k as u64;
    }
    Ok((n, hex::encode(h.finalize())))
}

/// A PMTiles archive opened for reading (local file, positioned reads).
pub struct TileArchive {
    path: PathBuf,
    reader: PmTilesReader<File, NoCache>,
}

impl std::fmt::Debug for TileArchive {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TileArchive")
            .field("path", &self.path)
            .finish()
    }
}

impl TileArchive {
    /// Opens a PMTiles v3 file.
    pub fn open(path: impl Into<PathBuf>) -> Result<Self> {
        let path = path.into();
        let reader = PmTilesReader::new_with_path(&path).map_err(|e| Error::pmtiles(&path, e))?;
        Ok(Self { path, reader })
    }

    /// Path of the archive.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Lowest and highest zoom of the header.
    pub fn zoom_range(&self) -> (u8, u8) {
        let h = self.reader.get_header();
        (h.min_zoom, h.max_zoom)
    }

    /// Bounds `[west, south, east, north]` of the header.
    pub fn bounds(&self) -> [f64; 4] {
        let h = self.reader.get_header();
        [
            h.min_longitude,
            h.min_latitude,
            h.max_longitude,
            h.max_latitude,
        ]
    }

    /// Tile type of the header.
    pub fn tile_type(&self) -> TileType {
        self.reader.get_header().tile_type
    }

    /// Number of addressed tiles of the header.
    pub fn addressed_tiles(&self) -> Option<u64> {
        self.reader.get_header().n_addressed_tiles().map(u64::from)
    }

    /// JSON metadata of the archive.
    pub fn metadata(&self) -> Result<String> {
        self.reader
            .get_metadata()
            .map_err(|e| Error::pmtiles(&self.path, e))
    }

    /// Tile bytes, decompressed (gzip vector tiles are inflated); `None` if absent.
    pub fn tile(&self, z: u8, x: u32, y: u32) -> Result<Option<Vec<u8>>> {
        let coord = TileCoord::new(z, x, y).map_err(|e| Error::pmtiles(&self.path, e))?;
        self.reader
            .get_tile_decompressed(coord)
            .map(|t| t.map(|b| b.to_vec()))
            .map_err(|e| Error::pmtiles(&self.path, e))
    }

    /// Tile bytes as stored (compressed as the header says); `None` if absent.
    pub fn raw_tile(&self, z: u8, x: u32, y: u32) -> Result<Option<Vec<u8>>> {
        let coord = TileCoord::new(z, x, y).map_err(|e| Error::pmtiles(&self.path, e))?;
        self.reader
            .get_tile(coord)
            .map(|t| t.map(|b| b.to_vec()))
            .map_err(|e| Error::pmtiles(&self.path, e))
    }

    /// Tile compression of the header.
    pub fn tile_compression(&self) -> pmtiles::Compression {
        self.reader.get_header().tile_compression
    }

    /// Palette index at a position at zoom `z`; `None` outside the tiles or on `nodata`.
    pub fn palette_index_at(&self, p: LonLat, z: u8, nodata: Option<u8>) -> Result<Option<u8>> {
        let (tx, ty, px, py) = tiles::tile_pixel(p, z);
        let Some(bytes) = self.tile(z, tx, ty)? else {
            return Ok(None);
        };
        let img = png::decode_indexed(&bytes)?;
        Ok(img.index_at(px, py).filter(|i| Some(*i) != nodata))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn relative_paths() {
        assert!(check_relative("layers/pistes.geojson").is_ok());
        assert!(check_relative("../x").is_err());
        assert!(check_relative("/etc/passwd").is_err());
        assert!(check_relative("./a").is_err());
        assert!(check_relative("").is_err());
        assert!(check_relative("a\\b").is_err());
    }
}
