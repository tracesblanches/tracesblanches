//! Raster layers: pipeline GeoTIFF (Float32, any projected CRS of `common::crs`) to
//! classified Web Mercator tiles, 8-bit palette PNG in a PMTiles archive.
//!
//! The palette index of a pixel is its class, so the tile drawn by the viewer is also
//! the value read by the phone core ([`crate::reader::Pack::raster_value_at`]). The
//! highest zoom samples the source by nearest cell at each pixel centre; each lower zoom
//! takes the most frequent class of the 2 × 2 pixels below (ties: the more cautious
//! class, see [`RasterSpec::tie_high`]).
//!
//! Any GeoTIFF Float32 layer of the pipeline slots in by adding a [`RasterSpec`] to
//! [`catalog`]: today the 5 m slope of `terrain` and the 25 m snow rasters of `snow`.

use std::collections::HashMap;
use std::fs::File;
use std::path::Path;

use pmtiles::{PmTilesWriter, TileCoord, TileId, TileType};
use rayon::prelude::*;
use serde_json::{Value, json};
use skimap_common::crs::{Crs, transform_points};
use skimap_common::download::Downloader;
use skimap_common::geom::BBox;
use skimap_common::grid::Grid;

use crate::build::layers::{EntryParts, Outcome, copy_meta, layer_entry, read_meta};
use crate::build::source;
use crate::error::{Error, Result};
use crate::manifest::{Layer, LayerKind, LegendEntry, Missing, RasterInfo, ValueEncoding};
use crate::png;
use crate::tiles::{TILE_SIZE, TileData, TileKey, pixel_lat, pixel_lon, tiles_covering};

/// One class: values below `upper` (and at or above the previous class's `upper`).
#[derive(Debug, Clone)]
pub struct Class {
    /// Exclusive upper bound; `f32::INFINITY` for the last class.
    pub upper: f32,
    /// Label, French.
    pub label: &'static str,
    /// RGBA colour (alpha 0: not drawn).
    pub color: [u8; 4],
}

/// A raster layer of the pipeline.
#[derive(Debug, Clone)]
pub struct RasterSpec {
    /// Pack layer identifier.
    pub id: &'static str,
    /// Display title, French.
    pub title: &'static str,
    /// Pipeline layer directory.
    pub origin: &'static str,
    /// GeoTIFF file in that directory.
    pub file: &'static str,
    /// `<couche>.meta.json`.
    pub meta: &'static str,
    /// Classes, increasing.
    pub classes: Vec<Class>,
    /// Lowest zoom.
    pub minzoom: u8,
    /// Highest zoom (values are read there).
    pub maxzoom: u8,
    /// Downsampling tie-break: `true` keeps the higher class (slopes: steeper is the
    /// cautious side), `false` the lower one (snow: less snow is the cautious side).
    pub tie_high: bool,
    /// Shown when the viewer opens.
    pub visible: bool,
    /// Notes, French.
    pub notes: &'static [&'static str],
}

impl RasterSpec {
    /// Palette: one entry per class, then a transparent "no data" entry.
    pub fn palette(&self) -> Vec<[u8; 4]> {
        let mut p: Vec<[u8; 4]> = self.classes.iter().map(|c| c.color).collect();
        p.push([0, 0, 0, 0]);
        p
    }

    /// Palette index of "no data".
    pub fn nodata(&self) -> u8 {
        self.classes.len() as u8
    }

    /// Class of a value (`nodata` for NaN).
    pub fn classify(&self, v: f32) -> u8 {
        if v.is_nan() {
            return self.nodata();
        }
        self.classes
            .iter()
            .position(|c| v < c.upper)
            .unwrap_or(self.classes.len() - 1) as u8
    }
}

fn hex(c: [u8; 4]) -> String {
    format!("#{:02x}{:02x}{:02x}{:02x}", c[0], c[1], c[2], c[3])
}

/// Slope classes of the IGN slope map, same edges and colours, for comparison by eye.
pub fn slope_classes() -> Vec<Class> {
    let e = skimap_terrain::ign_slopes::CLASS_EDGES;
    let c = skimap_terrain::ign_slopes::CLASS_COLOURS;
    let rgba = |k: usize| [c[k][0], c[k][1], c[k][2], 255];
    vec![
        Class {
            upper: e[0],
            label: "moins de 30°",
            color: [0, 0, 0, 0],
        },
        Class {
            upper: e[1],
            label: "30 à 35°",
            color: rgba(0),
        },
        Class {
            upper: e[2],
            label: "35 à 40°",
            color: rgba(1),
        },
        Class {
            upper: e[3],
            label: "40 à 45°",
            color: rgba(2),
        },
        Class {
            upper: f32::INFINITY,
            label: "45° et plus",
            color: rgba(3),
        },
    ]
}

fn snow_depth_classes() -> Vec<Class> {
    vec![
        Class {
            upper: 1.0,
            label: "moins de 1 cm",
            color: [150, 110, 70, 70],
        },
        Class {
            upper: 10.0,
            label: "1 à 10 cm",
            color: [215, 232, 255, 150],
        },
        Class {
            upper: 20.0,
            label: "10 à 20 cm",
            color: [165, 202, 250, 170],
        },
        Class {
            upper: 40.0,
            label: "20 à 40 cm",
            color: [105, 160, 240, 185],
        },
        Class {
            upper: 60.0,
            label: "40 à 60 cm",
            color: [45, 115, 220, 200],
        },
        Class {
            upper: 100.0,
            label: "60 à 100 cm",
            color: [20, 65, 175, 210],
        },
        Class {
            upper: f32::INFINITY,
            label: "100 cm et plus",
            color: [70, 20, 140, 220],
        },
    ]
}

fn snow_practice_classes() -> Vec<Class> {
    vec![
        Class {
            upper: 0.5,
            label: "insuffisant",
            color: [215, 48, 39, 140],
        },
        Class {
            upper: 1.5,
            label: "limite",
            color: [250, 170, 30, 150],
        },
        Class {
            upper: f32::INFINITY,
            label: "suffisant",
            color: [40, 160, 70, 150],
        },
    ]
}

/// Raster layers, in display order.
pub fn catalog() -> Vec<RasterSpec> {
    vec![
        RasterSpec {
            id: "snow-depth",
            title: "Neige estimée (hauteur)",
            origin: "snow",
            file: "snow_depth.tif",
            meta: "snow.meta.json",
            classes: snow_depth_classes(),
            minzoom: 9,
            maxzoom: 13,
            tie_high: false,
            visible: false,
            notes: &[
                "Valeur centrale de la simulation Météo-France projetée sur le relief (pixel de 25 m) ; pas une mesure.",
            ],
        },
        RasterSpec {
            id: "snow-groomed",
            title: "Neige pour piste damée (classes)",
            origin: "snow",
            file: "class_groomed.tif",
            meta: "snow.meta.json",
            classes: snow_practice_classes(),
            minzoom: 9,
            maxzoom: 13,
            tie_high: false,
            visible: false,
            notes: &[
                "Seuils proposés : moins de 20 cm insuffisant, 20 à 40 cm limite, 40 cm et plus suffisant.",
            ],
        },
        RasterSpec {
            id: "snow-srn",
            title: "Neige pour ski hors piste (classes)",
            origin: "snow",
            file: "class_srn.tif",
            meta: "snow.meta.json",
            classes: snow_practice_classes(),
            minzoom: 9,
            maxzoom: 13,
            tie_high: false,
            visible: false,
            notes: &[
                "Seuils proposés : moins de 30 cm insuffisant, 30 à 60 cm limite, 60 cm et plus suffisant.",
            ],
        },
        RasterSpec {
            id: "slope",
            title: "Pentes (LiDAR HD 5 m)",
            origin: "terrain",
            file: "slope_5m.tif",
            meta: "terrain.meta.json",
            classes: slope_classes(),
            minzoom: 10,
            maxzoom: 15,
            tie_high: true,
            visible: true,
            notes: &["Classes et couleurs de la carte des pentes IGN ; moins de 30° transparent."],
        },
    ]
}

/// Classified tiles: `(z, x, y)` → palette indices (256 × 256, row-major).
pub type ClassTiles = HashMap<(u8, u32, u32), Vec<u8>>;

/// WGS84 bounds of a projected grid (densified edges).
pub fn grid_bbox_wgs84(grid: &Grid<f32>) -> Result<BBox> {
    let (x0, y0, x1, y1) = grid.bounds();
    let n = 64;
    let mut pts = Vec::with_capacity(4 * (n + 1));
    for i in 0..=n {
        let t = i as f64 / n as f64;
        let x = x0 + t * (x1 - x0);
        let y = y0 + t * (y1 - y0);
        pts.extend([(x, y0), (x, y1), (x0, y), (x1, y)]);
    }
    transform_points(grid.crs(), Crs::Wgs84, &mut pts)?;
    let (mut w, mut s, mut e, mut nn) = (f64::MAX, f64::MAX, f64::MIN, f64::MIN);
    for (lon, lat) in pts {
        w = w.min(lon);
        e = e.max(lon);
        s = s.min(lat);
        nn = nn.max(lat);
    }
    Ok(BBox::new(s, w, nn, e))
}

/// One tile at the highest zoom: nearest source cell at each pixel centre.
fn sample_tile(
    grid: &Grid<f32>,
    spec: &RasterSpec,
    z: u8,
    x: u32,
    y: u32,
) -> Result<Option<Vec<u8>>> {
    let ts = TILE_SIZE as usize;
    let lons: Vec<f64> = (0..ts)
        .map(|c| pixel_lon(f64::from(x * TILE_SIZE) + c as f64 + 0.5, z))
        .collect();
    let lats: Vec<f64> = (0..ts)
        .map(|r| pixel_lat(f64::from(y * TILE_SIZE) + r as f64 + 0.5, z))
        .collect();
    let mut pts = Vec::with_capacity(ts * ts);
    for lat in &lats {
        for lon in &lons {
            pts.push((*lon, *lat));
        }
    }
    transform_points(Crs::Wgs84, grid.crs(), &mut pts)?;
    let nodata = spec.nodata();
    let mut any = false;
    let idx: Vec<u8> = pts
        .iter()
        .map(|&(px, py)| {
            let v = grid.value_at(px, py).copied().unwrap_or(f32::NAN);
            let c = spec.classify(v);
            any |= c != nodata;
            c
        })
        .collect();
    Ok(any.then_some(idx))
}

/// Most frequent class of four pixels, ignoring no data.
fn mode4(v: [u8; 4], nodata: u8, n_classes: usize, tie_high: bool) -> u8 {
    let mut counts = [0u8; 256];
    for &c in &v {
        if c != nodata {
            counts[usize::from(c)] += 1;
        }
    }
    let mut best = nodata;
    let mut best_n = 0;
    for (c, &k) in counts.iter().enumerate().take(n_classes) {
        if k > best_n || (k == best_n && k > 0 && tie_high) {
            best = c as u8;
            best_n = k;
        }
    }
    best
}

/// Parent tile from its four children (absent child = no data).
fn downsample(children: [Option<&Vec<u8>>; 4], spec: &RasterSpec) -> Option<Vec<u8>> {
    let ts = TILE_SIZE as usize;
    let half = ts / 2;
    let nodata = spec.nodata();
    let mut out = vec![nodata; ts * ts];
    let mut any = false;
    for py in 0..ts {
        for px in 0..ts {
            let q = usize::from(py >= half) * 2 + usize::from(px >= half);
            let Some(child) = children[q] else { continue };
            let (cx, cy) = (2 * (px % half), 2 * (py % half));
            let at = |dx: usize, dy: usize| child[(cy + dy) * ts + cx + dx];
            let m = mode4(
                [at(0, 0), at(1, 0), at(0, 1), at(1, 1)],
                nodata,
                spec.classes.len(),
                spec.tie_high,
            );
            any |= m != nodata;
            out[py * ts + px] = m;
        }
    }
    any.then_some(out)
}

/// Classified tiles of a grid for `spec.minzoom..=spec.maxzoom`.
pub fn render(grid: &Grid<f32>, spec: &RasterSpec) -> Result<ClassTiles> {
    let bbox = grid_bbox_wgs84(grid)?;
    let top: Vec<(u8, u32, u32)> = tiles_covering(bbox, spec.maxzoom, spec.maxzoom);
    let sampled: Vec<(TileKey, Option<Vec<u8>>)> = top
        .par_iter()
        .map(|&(z, x, y)| sample_tile(grid, spec, z, x, y).map(|t| ((z, x, y), t)))
        .collect::<Result<_>>()?;
    let mut all: ClassTiles = sampled
        .into_iter()
        .filter_map(|(k, t)| t.map(|t| (k, t)))
        .collect();
    for z in (spec.minzoom..spec.maxzoom).rev() {
        let mut parents: Vec<(u32, u32)> = all
            .keys()
            .filter(|(cz, _, _)| *cz == z + 1)
            .map(|&(_, x, y)| (x / 2, y / 2))
            .collect();
        parents.sort_unstable();
        parents.dedup();
        let made: Vec<((u8, u32, u32), Vec<u8>)> = parents
            .par_iter()
            .filter_map(|&(x, y)| {
                let c = |dx: u32, dy: u32| all.get(&(z + 1, 2 * x + dx, 2 * y + dy));
                downsample([c(0, 0), c(1, 0), c(0, 1), c(1, 1)], spec).map(|t| ((z, x, y), t))
            })
            .collect();
        all.extend(made);
    }
    Ok(all)
}

/// Encodes class tiles to PNG, sorted by `(z, x, y)`.
pub fn encode_tiles(tiles: &ClassTiles, palette: &[[u8; 4]]) -> Result<Vec<TileData>> {
    let mut keys: Vec<(u8, u32, u32)> = tiles.keys().copied().collect();
    keys.sort_unstable();
    keys.par_iter()
        .map(|k| {
            let png = png::encode_indexed(TILE_SIZE, TILE_SIZE, palette, &tiles[k])?;
            Ok((*k, png))
        })
        .collect()
}

/// Writes tiles to a PMTiles archive, sorted by tile id. MVT tiles are gzipped by the
/// writer unless `precompressed` says they already are; PNG tiles are stored as they are.
/// Returns the archive size.
#[allow(clippy::too_many_arguments)]
pub fn write_archive(
    path: &Path,
    tile_type: TileType,
    precompressed: Option<pmtiles::Compression>,
    tiles: &[TileData],
    metadata: &Value,
    bbox: BBox,
    minzoom: u8,
    maxzoom: u8,
) -> Result<u64> {
    if let Some(p) = path.parent() {
        crate::build::create_dir(p)?;
    }
    let mut sorted: Vec<(u64, &TileKey, &Vec<u8>)> = tiles
        .iter()
        .map(|(k, t)| {
            let c = TileCoord::new(k.0, k.1, k.2).map_err(|e| Error::pmtiles(path, e))?;
            Ok((TileId::from(c).value(), k, t))
        })
        .collect::<Result<_>>()?;
    sorted.sort_by_key(|(id, _, _)| *id);
    let f = File::create(path).map_err(|e| Error::io(path, e))?;
    let c = bbox.center();
    let mut builder = PmTilesWriter::new(tile_type);
    if let Some(c) = precompressed {
        builder = builder.precompressed(c);
    }
    let mut w = builder
        .min_zoom(minzoom)
        .max_zoom(maxzoom)
        .bounds(bbox.west, bbox.south, bbox.east, bbox.north)
        .center(c.lon, c.lat)
        .center_zoom(maxzoom.clamp(minzoom, 12.max(minzoom)))
        .metadata(metadata.to_string())
        .create(f)
        .map_err(|e| Error::pmtiles(path, e))?;
    for (_, k, t) in sorted {
        let coord = TileCoord::new(k.0, k.1, k.2).map_err(|e| Error::pmtiles(path, e))?;
        w.add_tile(coord, t).map_err(|e| Error::pmtiles(path, e))?;
    }
    w.finalize().map_err(|e| Error::pmtiles(path, e))?;
    Ok(std::fs::metadata(path)
        .map_err(|e| Error::io(path, e))?
        .len())
}

fn legend(spec: &RasterSpec) -> Vec<LegendEntry> {
    spec.classes
        .iter()
        .enumerate()
        .map(|(i, c)| LegendEntry {
            value: i as u8,
            label: c.label.to_owned(),
            color: hex(c.color),
        })
        .collect()
}

/// Builds a raster layer from the pipeline GeoTIFF, if present.
pub fn include(spec: &RasterSpec, out: &Path, root: &Path) -> Result<Outcome> {
    let dir = out.join(spec.origin);
    let src = dir.join(spec.file);
    if !src.is_file() {
        return Ok(Outcome::Missing(Missing {
            id: spec.id.to_owned(),
            reason: format!(
                "{}/{} absent des sorties du pipeline",
                spec.origin, spec.file
            ),
        }));
    }
    let grid = skimap_common::geotiff::read_f32(&src)?;
    let tiles = render(&grid, spec)?;
    let png_tiles = encode_tiles(&tiles, &spec.palette())?;
    let bbox = grid_bbox_wgs84(&grid)?;
    let file = format!("rasters/{}.pmtiles", spec.id);
    let meta = read_meta(&dir.join(spec.meta))?;
    let attribution: Vec<String> = meta
        .as_ref()
        .map(|m| {
            m.meta
                .sources
                .iter()
                .map(|s| s.attribution.clone())
                .filter(|a| !a.is_empty())
                .collect()
        })
        .unwrap_or_default();
    let metadata = json!({
        "name": spec.title,
        "type": "overlay",
        "format": "png",
        "attribution": attribution.join(" ; "),
        "skimap:legend": legend(spec),
        "skimap:nodata_index": spec.nodata(),
    });
    let bytes = write_archive(
        &root.join(&file),
        TileType::Png,
        None,
        &png_tiles,
        &metadata,
        bbox,
        spec.minzoom,
        spec.maxzoom,
    )?;
    let meta_rel = copy_meta(&dir, spec.meta, root)?;
    let (gx, gy) = (grid.width(), grid.height());
    let mut l = layer_entry(
        EntryParts {
            id: spec.id,
            title: spec.title,
            kind: LayerKind::Raster,
            format: "pmtiles".to_owned(),
            file,
            origin: format!("{}/{}", spec.origin, spec.file),
            meta_rel,
            bytes,
            visible: spec.visible,
            notes: spec.notes,
        },
        meta.as_ref(),
    );
    l.raster = Some(RasterInfo {
        minzoom: spec.minzoom,
        maxzoom: spec.maxzoom,
        tile_size: TILE_SIZE,
        tile_count: png_tiles.len() as u64,
        encoding: ValueEncoding::PaletteIndex,
        nodata_index: Some(spec.nodata()),
        legend: legend(spec),
        source: format!(
            "{}/{} : GeoTIFF Float32, EPSG:{}, {gx} × {gy} cellules de {} m",
            spec.origin,
            spec.file,
            grid.crs().epsg(),
            grid.transform().pixel_width
        ),
        downsampling: format!(
            "zoom {} : cellule la plus proche du centre de chaque pixel ; zooms inférieurs : classe la plus fréquente des 2 × 2 pixels (égalité : classe la plus {})",
            spec.maxzoom,
            if spec.tie_high { "haute" } else { "basse" }
        ),
    });
    Ok(Outcome::Included(Box::new(l)))
}

/// Zooms of the IGN slope map copied into the pack.
pub const IGN_SLOPES_ZOOMS: (u8, u8) = (11, 15);

/// IGN slope map tiles (WMTS `SLOPES.MOUNTAIN`) of the site box, as served (RGBA PNG).
pub fn ign_slopes(bbox: BBox, dl: &Downloader, root: &Path) -> Result<Layer> {
    let (minz, maxz) = IGN_SLOPES_ZOOMS;
    let list = tiles_covering(bbox, minz, maxz);
    let mut tiles = Vec::with_capacity(list.len());
    for (z, x, y) in list {
        tiles.push((
            (z, x, y),
            skimap_terrain::ign_slopes::fetch_tile(dl, z, x, y)?,
        ));
    }
    let id = "ign-slopes";
    let file = format!("rasters/{id}.pmtiles");
    let classes = slope_classes();
    let legend: Vec<LegendEntry> = classes
        .iter()
        .enumerate()
        .map(|(i, c)| LegendEntry {
            value: i as u8,
            label: c.label.to_owned(),
            color: hex(c.color),
        })
        .collect();
    let src = source(
        "IGN, carte des pentes (WMTS GEOGRAPHICALGRIDSYSTEMS.SLOPES.MOUNTAIN, RGE ALTI 5 m)",
        "https://data.geopf.fr/wmts?LAYER=GEOGRAPHICALGRIDSYSTEMS.SLOPES.MOUNTAIN",
        "Licence Ouverte 2.0 (Etalab) ; usage hors ligne à confirmer (features.md n° 5)",
        "© IGN – carte des pentes",
    );
    let metadata = json!({
        "name": "Carte des pentes IGN",
        "type": "overlay",
        "format": "png",
        "attribution": "© IGN – carte des pentes",
    });
    let bytes = write_archive(
        &root.join(&file),
        TileType::Png,
        None,
        &tiles,
        &metadata,
        bbox,
        minz,
        maxz,
    )?;
    Ok(Layer {
        id: id.to_owned(),
        title: "Pentes IGN (comparaison)".to_owned(),
        kind: LayerKind::Raster,
        format: "pmtiles".to_owned(),
        file,
        meta: None,
        origin: "WMTS IGN (cache du téléchargeur, source ign)".to_owned(),
        generated_at: Some(chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true)),
        valid_from: None,
        valid_until: None,
        validity: Some("relief permanent (RGE ALTI 5 m)".to_owned()),
        sources: vec![src],
        personal_use_only: false,
        bytes,
        features: None,
        geometry_types: Vec::new(),
        visible: false,
        raster: Some(RasterInfo {
            minzoom: minz,
            maxzoom: maxz,
            tile_size: TILE_SIZE,
            tile_count: tiles.len() as u64,
            encoding: ValueEncoding::RgbaLegend,
            nodata_index: None,
            legend,
            source: "tuiles PNG RGBA du WMTS, copiées telles quelles".to_owned(),
            downsampling: "rendu par l'IGN à chaque zoom".to_owned(),
        }),
        notes: vec![
            "Couche de comparaison avec notre pente LiDAR HD (rapport de la crate terrain : 76,9 % / 97,8 % de pixels dans la même classe).".to_owned(),
        ],
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use skimap_common::grid::GeoTransform;

    fn spec() -> RasterSpec {
        RasterSpec {
            id: "t",
            title: "t",
            origin: "terrain",
            file: "x.tif",
            meta: "m.json",
            classes: slope_classes(),
            minzoom: 12,
            maxzoom: 14,
            tie_high: true,
            visible: true,
            notes: &[],
        }
    }

    #[test]
    fn classify_edges() {
        let s = spec();
        assert_eq!(s.classify(f32::NAN), 5);
        assert_eq!(s.classify(0.0), 0);
        assert_eq!(s.classify(29.99), 0);
        assert_eq!(s.classify(30.0), 1);
        assert_eq!(s.classify(44.9), 3);
        assert_eq!(s.classify(45.0), 4);
        assert_eq!(s.classify(89.0), 4);
        assert_eq!(s.palette().len(), 6);
        assert_eq!(s.palette()[5][3], 0);
    }

    #[test]
    fn mode_and_ties() {
        assert_eq!(mode4([1, 1, 2, 5], 5, 5, true), 1);
        assert_eq!(mode4([1, 2, 5, 5], 5, 5, true), 2);
        assert_eq!(mode4([1, 2, 5, 5], 5, 5, false), 1);
        assert_eq!(mode4([5, 5, 5, 5], 5, 5, true), 5);
        assert_eq!(mode4([0, 0, 4, 4], 5, 5, true), 4);
    }

    /// A Lambert-93 grid: west half 10°, east half 40°, last row no data. The tile
    /// pixels must take the value of the cell under their centre.
    #[test]
    fn render_samples_and_downsamples() {
        let (w, h) = (400usize, 400usize);
        let gt = GeoTransform::north_up(990_000.0, 6_420_000.0, 5.0);
        let mut data = vec![10.0f32; w * h];
        for r in 0..h {
            for c in w / 2..w {
                data[r * w + c] = 40.0;
            }
        }
        for c in 0..w {
            data[(h - 1) * w + c] = f32::NAN;
        }
        let grid = Grid::from_vec(w, h, gt, Crs::Lambert93, data).unwrap();
        let s = spec();
        let tiles = render(&grid, &s).unwrap();
        assert!(tiles.keys().any(|k| k.0 == 14));
        assert!(tiles.keys().any(|k| k.0 == 12));
        // Check every pixel centre of the top zoom against the grid.
        let mut checked = 0;
        for (&(z, x, y), t) in tiles.iter().filter(|(k, _)| k.0 == 14) {
            for r in (0..256).step_by(17) {
                for c in (0..256).step_by(13) {
                    let lon = pixel_lon(f64::from(x * 256) + c as f64 + 0.5, z);
                    let lat = pixel_lat(f64::from(y * 256) + r as f64 + 0.5, z);
                    let (px, py) =
                        skimap_common::crs::transform_point(Crs::Wgs84, Crs::Lambert93, lon, lat)
                            .unwrap();
                    let want = s.classify(grid.value_at(px, py).copied().unwrap_or(f32::NAN));
                    assert_eq!(t[r * 256 + c], want);
                    checked += 1;
                }
            }
        }
        assert!(checked > 100);
        // PNG round trip through the encoder.
        let png = encode_tiles(&tiles, &s.palette()).unwrap();
        assert_eq!(png.len(), tiles.len());
        let (k, bytes) = &png[0];
        assert_eq!(png::decode_indexed(bytes).unwrap().indices, tiles[k]);
    }
}
