//! Extract of a PMTiles v3 archive over range reads (`pmtiles extract` in Rust).
//!
//! Reads the header and the root directory (first 16 KiB, as go-pmtiles does), walks the
//! root and leaf directories only for the tiles of the box (every zoom up to `maxzoom`),
//! then reads the tile data in a few merged ranges (sorted by offset, gaps up to
//! [`MAX_GAP`] read and dropped) and writes a new archive with the crate writer, tiles as
//! they are (no recompression), metadata of the source. Over HTTP, each read is a range
//! request through the shared downloader ([`HttpRanges`]): cached per range, rate
//! limited, retried.

use std::collections::HashMap;
use std::fs::File;
use std::io::Read;
use std::path::Path;

use pmtiles::{Compression, PmTilesWriter, TileCoord, TileId, TileType};
use serde::Serialize;
use skimap_common::download::{Downloader, Request};
use skimap_common::geom::BBox;

use crate::error::{Error, Result};
use crate::pmtiles_v3::decompress;
pub use crate::pmtiles_v3::{Entry, Header, parse_directory, parse_header};
use crate::tiles::tiles_covering;

/// Bytes read first: header and, in every archive written by the reference tools, the
/// root directory.
pub const FIRST_READ: u64 = 16_384;
/// Largest gap between two needed tile ranges that is read rather than split into two
/// requests (unused bytes are dropped).
pub const MAX_GAP: u64 = 256 * 1024;

/// Range reads of an archive.
pub trait RangeSource {
    /// `len` bytes from `offset` (fewer at the end of the archive).
    fn read(&self, offset: u64, len: u64) -> Result<Vec<u8>>;
}

/// A local archive.
impl RangeSource for File {
    fn read(&self, offset: u64, len: u64) -> Result<Vec<u8>> {
        use std::io::{Seek, SeekFrom};
        let mut f = self;
        f.seek(SeekFrom::Start(offset))
            .map_err(|e| Error::Pack(format!("seek: {e}")))?;
        let mut buf = Vec::with_capacity(len as usize);
        f.take(len)
            .read_to_end(&mut buf)
            .map_err(|e| Error::Pack(format!("read: {e}")))?;
        Ok(buf)
    }
}

/// A remote archive read by range requests through the shared downloader.
pub struct HttpRanges<'a> {
    /// Downloader.
    pub dl: &'a Downloader,
    /// Downloader source (cache directory, rate limit).
    pub source: &'a str,
    /// Archive URL.
    pub url: &'a str,
}

impl RangeSource for HttpRanges<'_> {
    fn read(&self, offset: u64, len: u64) -> Result<Vec<u8>> {
        Ok(self
            .dl
            .fetch(&Request::get(self.source, self.url).range(offset, len))?
            .bytes()?)
    }
}

/// Entry covering `id` in a sorted directory: tile entry or leaf directory.
fn find(dir: &[Entry], id: u64) -> Option<Entry> {
    let i = dir.partition_point(|e| e.tile_id <= id).checked_sub(1)?;
    let e = dir[i];
    (e.run_length == 0 || id < e.tile_id + e.run_length).then_some(e)
}

/// Figures of an extract.
#[derive(Debug, Clone, Default, Serialize, PartialEq, Eq)]
pub struct ExtractStats {
    /// Tiles of the box present in the source (addressed).
    pub tiles: usize,
    /// Distinct tile contents read.
    pub contents: usize,
    /// Leaf directories read.
    pub leaf_directories: usize,
    /// Range reads of tile data (after merging).
    pub data_reads: usize,
    /// Bytes of tile data read (gaps included).
    pub data_bytes_read: u64,
    /// Size of the written archive.
    pub bytes: u64,
}

/// Extracts the tiles of `bbox` at zooms `zooms.0..=zooms.1` (within those of the
/// source) from `src` into `out`.
pub fn extract(
    src: &dyn RangeSource,
    bbox: BBox,
    zooms: (u8, u8),
    out: &Path,
) -> Result<ExtractStats> {
    Archive::open(src)?.extract(bbox, zooms, out)
}

/// Decompresses a tile stored with the tile compression `compression` of a header (1:
/// none, 2: gzip); other compressions are not read.
pub fn decompress_tile(b: &[u8], compression: u8) -> Result<Vec<u8>> {
    match compression {
        0 | 1 => Ok(b.to_vec()),
        2 => {
            let mut out = Vec::new();
            flate2::read::GzDecoder::new(b)
                .read_to_end(&mut out)
                .map_err(|e| Error::Pack(format!("gzip tile: {e}")))?;
            Ok(out)
        }
        c => Err(Error::Pack(format!(
            "tile compression {c} not supported (gzip or none)"
        ))),
    }
}

/// Tile address `(z, x, y)` -> tile id.
fn tile_id(z: u8, x: u32, y: u32) -> Result<u64> {
    TileCoord::new(z, x, y)
        .map(|c| TileId::from(c).value())
        .map_err(|e| Error::Pack(format!("tile {z}/{x}/{y}: {e}")))
}

/// An archive read by ranges: header, root directory and metadata read at
/// [`open`](Self::open); leaf directories and tile data read when first needed and kept,
/// so that the tiles of one zoom read for a comparison ([`tiles_at`](Self::tiles_at)) are
/// not read again by the [`extract`](Self::extract) that follows.
pub struct Archive<'a> {
    src: &'a dyn RangeSource,
    header: Header,
    root: Vec<Entry>,
    metadata: String,
    /// Leaf directories by absolute offset.
    leaves: HashMap<u64, Vec<Entry>>,
    /// Tile contents by (data offset, length).
    data: HashMap<(u64, u64), Vec<u8>>,
    /// Reads so far: leaf directories, data ranges and bytes.
    reads: ExtractStats,
}

impl<'a> Archive<'a> {
    /// Reads the header, the root directory and the metadata (first [`FIRST_READ`] bytes).
    pub fn open(src: &'a dyn RangeSource) -> Result<Self> {
        let first = src.read(0, FIRST_READ)?;
        let h = parse_header(&first)?;
        let read_section = |(off, len): (u64, u64)| -> Result<Vec<u8>> {
            if off + len <= first.len() as u64 {
                Ok(first[off as usize..(off + len) as usize].to_vec())
            } else {
                src.read(off, len)
            }
        };
        let root = parse_directory(&decompress(read_section(h.root)?, h.internal_compression)?)?;
        let metadata = if h.metadata.1 > 0 {
            String::from_utf8(decompress(
                read_section(h.metadata)?,
                h.internal_compression,
            )?)
            .map_err(|e| Error::Pack(format!("metadata: {e}")))?
        } else {
            "{}".to_owned()
        };
        Ok(Self {
            src,
            header: h,
            root,
            metadata,
            leaves: HashMap::new(),
            data: HashMap::new(),
            reads: ExtractStats::default(),
        })
    }

    /// The header.
    pub fn header(&self) -> &Header {
        &self.header
    }

    /// Reads so far: leaf directories, data ranges and bytes of tile data.
    pub fn reads(&self) -> &ExtractStats {
        &self.reads
    }

    /// Tiles of `bbox` at zooms `minzoom..=maxzoom` present in the archive, sorted by
    /// tile id: `(tile id, data offset, length)`. Walks (and keeps) the leaf directories.
    fn locate(&mut self, bbox: BBox, minzoom: u8, maxzoom: u8) -> Result<Vec<(u64, u64, u64)>> {
        let mut ids: Vec<u64> = tiles_covering(bbox, minzoom, maxzoom)
            .into_iter()
            .map(|(z, x, y)| tile_id(z, x, y))
            .collect::<Result<_>>()?;
        ids.sort_unstable();
        let mut found = Vec::new();
        for &id in &ids {
            let mut dir_entry = find(&self.root, id);
            let mut depth = 0;
            while let Some(e) = dir_entry.filter(|e| e.run_length == 0) {
                depth += 1;
                if depth > 8 {
                    return Err(Error::Pack("leaf directories nested too deep".into()));
                }
                let key = self.header.leaves.0 + e.offset;
                let leaf = match self.leaves.entry(key) {
                    std::collections::hash_map::Entry::Occupied(o) => o.into_mut(),
                    std::collections::hash_map::Entry::Vacant(v) => {
                        self.reads.leaf_directories += 1;
                        v.insert(parse_directory(&decompress(
                            self.src.read(key, e.length)?,
                            self.header.internal_compression,
                        )?)?)
                    }
                };
                dir_entry = find(leaf, id);
            }
            if let Some(e) = dir_entry {
                found.push((id, e.offset, e.length));
            }
        }
        Ok(found)
    }

    /// Reads the contents of `found` not read yet, sorted by offset, in merged ranges.
    fn fetch(&mut self, found: &[(u64, u64, u64)]) -> Result<()> {
        let mut spans: Vec<(u64, u64)> = found
            .iter()
            .map(|&(_, o, l)| (o, l))
            .filter(|s| !self.data.contains_key(s))
            .collect();
        spans.sort_unstable();
        spans.dedup();
        let base = self.header.data.0;
        let mut i = 0;
        while i < spans.len() {
            let start = spans[i].0;
            let mut end = spans[i].0 + spans[i].1;
            let mut j = i + 1;
            while j < spans.len() && spans[j].0 <= end + MAX_GAP {
                end = end.max(spans[j].0 + spans[j].1);
                j += 1;
            }
            let block = self.src.read(base + start, end - start)?;
            if block.len() as u64 != end - start {
                return Err(Error::Pack(format!(
                    "short read at {}: {} of {} bytes",
                    base + start,
                    block.len(),
                    end - start
                )));
            }
            self.reads.data_reads += 1;
            self.reads.data_bytes_read += end - start;
            for &(o, l) in &spans[i..j] {
                let a = (o - start) as usize;
                self.data.insert((o, l), block[a..a + l as usize].to_vec());
            }
            i = j;
        }
        Ok(())
    }

    /// The tiles of `bbox` at `zoom` present in the archive, as stored (tile compression
    /// of the header), by address. Reads only their leaf directories and data.
    pub fn tiles_at(&mut self, bbox: BBox, zoom: u8) -> Result<HashMap<(u8, u32, u32), Vec<u8>>> {
        let found = self.locate(bbox, zoom, zoom)?;
        self.fetch(&found)?;
        let by_id: HashMap<u64, (u64, u64)> =
            found.iter().map(|&(id, o, l)| (id, (o, l))).collect();
        let mut out = HashMap::new();
        for (z, x, y) in tiles_covering(bbox, zoom, zoom) {
            if let Some(span) = by_id.get(&tile_id(z, x, y)?) {
                out.insert((z, x, y), self.data[span].clone());
            }
        }
        Ok(out)
    }

    /// Writes the tiles of `bbox` at zooms `zooms.0..=zooms.1` (within those of the
    /// archive) into `out` (tiles as they are, metadata of the source); reads only what was
    /// not read before. The read figures are those of the whole archive session
    /// (comparison included).
    pub fn extract(&mut self, bbox: BBox, zooms: (u8, u8), out: &Path) -> Result<ExtractStats> {
        let h = self.header;
        let maxzoom = zooms.1.min(h.zooms.1);
        let minzoom = zooms.0.max(h.zooms.0);
        if minzoom > maxzoom {
            return Err(Error::Pack(format!(
                "extract: zooms {}-{} outside the archive zooms {}-{}",
                zooms.0, zooms.1, h.zooms.0, h.zooms.1
            )));
        }
        let found = self.locate(bbox, minzoom, maxzoom)?;
        self.fetch(&found)?;
        let mut stats = self.reads.clone();
        stats.tiles = found.len();
        let mut contents: Vec<(u64, u64)> = found.iter().map(|&(_, o, l)| (o, l)).collect();
        contents.sort_unstable();
        contents.dedup();
        stats.contents = contents.len();

        let tile_type = match h.tile_type {
            1 => TileType::Mvt,
            2 => TileType::Png,
            3 => TileType::Jpeg,
            4 => TileType::Webp,
            5 => TileType::Avif,
            t => return Err(Error::Pack(format!("tile type {t} not supported"))),
        };
        let compression = match h.tile_compression {
            1 => Compression::None,
            2 => Compression::Gzip,
            3 => Compression::Brotli,
            4 => Compression::Zstd,
            c => return Err(Error::Pack(format!("tile compression {c} not supported"))),
        };
        if let Some(p) = out.parent() {
            crate::build::create_dir(p)?;
        }
        let mut f = skimap_common::output::AtomicFile::create(out)?;
        let c = bbox.center();
        let mut w = PmTilesWriter::new(tile_type)
            .precompressed(compression)
            .min_zoom(minzoom)
            .max_zoom(maxzoom)
            .bounds(bbox.west, bbox.south, bbox.east, bbox.north)
            .center(c.lon, c.lat)
            .center_zoom(maxzoom.min(12).max(minzoom))
            .metadata(self.metadata.clone())
            .create(&mut f)
            .map_err(|e| Error::pmtiles(out, e))?;
        for (id, o, l) in &found {
            let coord: TileCoord = TileId::new(*id).map_err(|e| Error::pmtiles(out, e))?.into();
            w.add_tile(coord, &self.data[&(*o, *l)])
                .map_err(|e| Error::pmtiles(out, e))?;
        }
        w.finalize().map_err(|e| Error::pmtiles(out, e))?;
        stats.bytes = f.commit()?;
        Ok(stats)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::build::raster::write_archive;
    use crate::pmtiles_v3::varint;
    use crate::reader::TileArchive;
    use std::cell::Cell;

    /// Counts the reads of a local archive.
    struct Counted(File, Cell<usize>);

    impl RangeSource for Counted {
        fn read(&self, offset: u64, len: u64) -> Result<Vec<u8>> {
            self.1.set(self.1.get() + 1);
            self.0.read(offset, len)
        }
    }

    #[test]
    fn varints_and_directories() {
        // 2 entries: ids 5 and 7 (delta 2), run lengths 1 and 0 (leaf), lengths 10 and 20,
        // offsets 0 (encoded 1) and "follows the previous one" (0).
        let d = parse_directory(&[2, 5, 2, 1, 0, 10, 20, 1, 0]).unwrap();
        assert_eq!(
            d,
            vec![
                Entry {
                    tile_id: 5,
                    offset: 0,
                    length: 10,
                    run_length: 1
                },
                Entry {
                    tile_id: 7,
                    offset: 10,
                    length: 20,
                    run_length: 0
                },
            ]
        );
        assert_eq!(find(&d, 4), None);
        assert_eq!(find(&d, 5).unwrap().tile_id, 5);
        assert_eq!(find(&d, 6), None);
        assert_eq!(find(&d, 1000).unwrap().tile_id, 7, "leaf covers the rest");
        let mut pos = 0;
        assert_eq!(varint(&[0xac, 0x02], &mut pos).unwrap(), 300);
        assert!(parse_directory(&[3, 1]).is_err());
    }

    /// A source archive over a larger box, zooms 10 to 14: the extract keeps exactly the
    /// tiles of the small box up to zoom 13, byte for byte, with the source metadata.
    #[test]
    fn extract_keeps_the_box_tiles_byte_for_byte() {
        let dir = tempfile::tempdir().unwrap();
        let big = BBox::new(44.70, 6.55, 45.00, 6.95);
        let small = BBox::new(44.80, 6.70, 44.85, 6.78);
        let tiles: Vec<crate::tiles::TileData> = tiles_covering(big, 10, 14)
            .into_iter()
            .map(|k| (k, format!("{}/{}/{}", k.0, k.1, k.2).into_bytes()))
            .collect();
        let src_path = dir.path().join("src.pmtiles");
        write_archive(
            &src_path,
            TileType::Png,
            None,
            &tiles,
            &serde_json::json!({"name": "source"}),
            big,
            10,
            14,
        )
        .unwrap();
        let out = dir.path().join("out.pmtiles");
        let src = Counted(File::open(&src_path).unwrap(), Cell::new(0));
        let s = extract(&src, small, (0, 13), &out).unwrap();
        let want = tiles_covering(small, 10, 13);
        assert_eq!(s.tiles, want.len());
        assert_eq!(s.contents, want.len());
        assert_eq!(s.data_reads, 1, "contiguous tiles: one merged read");
        assert!(
            src.1.get() <= 3 + s.leaf_directories,
            "{} reads",
            src.1.get()
        );
        let a = TileArchive::open(&out).unwrap();
        assert_eq!(a.zoom_range(), (10, 13));
        assert_eq!(a.addressed_tiles().unwrap(), want.len() as u64);
        for (z, x, y) in want {
            let t = a.tile(z, x, y).unwrap().unwrap();
            assert_eq!(t, format!("{z}/{x}/{y}").into_bytes());
        }
        assert!(a.tile(14, 8600, 5900).unwrap().is_none());
        assert!(a.metadata().unwrap().contains("source"));

        // From zoom 12 only: the tiles of the lower zooms are neither read nor written.
        let high = dir.path().join("high.pmtiles");
        let src = Counted(File::open(&src_path).unwrap(), Cell::new(0));
        let s = extract(&src, small, (12, 13), &high).unwrap();
        let want = tiles_covering(small, 12, 13);
        assert_eq!(s.tiles, want.len());
        let a = TileArchive::open(&high).unwrap();
        assert_eq!(a.zoom_range(), (12, 13));
        assert_eq!(a.addressed_tiles().unwrap(), want.len() as u64);
        for (z, x, y) in tiles_covering(small, 10, 11) {
            assert!(a.tile(z, x, y).unwrap().is_none(), "{z}/{x}/{y}");
        }
        for (z, x, y) in want {
            assert_eq!(
                a.tile(z, x, y).unwrap().unwrap(),
                format!("{z}/{x}/{y}").into_bytes()
            );
        }
        assert!(
            extract(&src, small, (15, 16), &high).is_err(),
            "above the source"
        );
    }

    /// A source with leaf directories (more than 16 384 entries): they are walked and
    /// read once each.
    #[test]
    fn extract_walks_leaf_directories() {
        let dir = tempfile::tempdir().unwrap();
        let big = BBox::new(44.0, 6.0, 45.0, 7.5);
        let small = BBox::new(44.80, 6.70, 44.85, 6.78);
        let tiles: Vec<crate::tiles::TileData> = tiles_covering(big, 0, 15)
            .into_iter()
            .map(|k| (k, format!("{}/{}/{}", k.0, k.1, k.2).into_bytes()))
            .collect();
        assert!(tiles.len() > 20_000, "{}", tiles.len());
        let src_path = dir.path().join("src.pmtiles");
        write_archive(
            &src_path,
            TileType::Png,
            None,
            &tiles,
            &serde_json::json!({}),
            big,
            0,
            15,
        )
        .unwrap();
        let first = std::fs::read(&src_path).unwrap();
        assert!(
            parse_header(&first).unwrap().leaves.1 > 0,
            "source must have leaves"
        );
        let out = dir.path().join("out.pmtiles");
        let s = extract(&File::open(&src_path).unwrap(), small, (0, 15), &out).unwrap();
        assert!(s.leaf_directories >= 1);
        let want = tiles_covering(small, 0, 15);
        assert_eq!(s.tiles, want.len());
        let a = TileArchive::open(&out).unwrap();
        for (z, x, y) in want {
            assert_eq!(
                a.tile(z, x, y).unwrap().unwrap(),
                format!("{z}/{x}/{y}").into_bytes()
            );
        }
    }
}
