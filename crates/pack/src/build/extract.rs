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
use crate::tiles::tiles_covering;

/// Bytes read first: header and, in every archive written by the reference tools, the
/// root directory.
pub const FIRST_READ: u64 = 16_384;
/// Largest gap between two needed tile ranges that is read rather than split into two
/// requests (unused bytes are dropped).
pub const MAX_GAP: u64 = 256 * 1024;
/// PMTiles v3 header length.
const HEADER_LEN: usize = 127;

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

/// The fields of a PMTiles v3 header used by the extract.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Header {
    /// Root directory offset and length.
    pub root: (u64, u64),
    /// Metadata offset and length.
    pub metadata: (u64, u64),
    /// Leaf directories offset and length.
    pub leaves: (u64, u64),
    /// Tile data offset and length.
    pub data: (u64, u64),
    /// Compression of the directories and metadata.
    pub internal_compression: u8,
    /// Compression of the tiles.
    pub tile_compression: u8,
    /// Tile type.
    pub tile_type: u8,
    /// Zoom range.
    pub zooms: (u8, u8),
}

/// Parses the header (spec v3, little-endian).
pub fn parse_header(b: &[u8]) -> Result<Header> {
    if b.len() < HEADER_LEN || &b[..7] != b"PMTiles" || b[7] != 3 {
        return Err(Error::Pack("not a PMTiles v3 archive".into()));
    }
    let u = |o: usize| u64::from_le_bytes(b[o..o + 8].try_into().expect("8 bytes"));
    Ok(Header {
        root: (u(8), u(16)),
        metadata: (u(24), u(32)),
        leaves: (u(40), u(48)),
        data: (u(56), u(64)),
        internal_compression: b[97],
        tile_compression: b[98],
        tile_type: b[99],
        zooms: (b[100], b[101]),
    })
}

/// A directory entry: tiles `tile_id..tile_id + run_length` (a leaf directory when
/// `run_length == 0`), at `offset` (from the data or leaf section), `length` bytes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Entry {
    /// First tile id.
    pub tile_id: u64,
    /// Offset in its section.
    pub offset: u64,
    /// Length.
    pub length: u64,
    /// Run length (0: leaf directory).
    pub run_length: u64,
}

fn varint(b: &[u8], pos: &mut usize) -> Result<u64> {
    let mut v = 0u64;
    for shift in (0..64).step_by(7) {
        let byte = *b
            .get(*pos)
            .ok_or_else(|| Error::Pack("truncated directory".into()))?;
        *pos += 1;
        v |= u64::from(byte & 0x7f) << shift;
        if byte < 0x80 {
            return Ok(v);
        }
    }
    Err(Error::Pack("varint too long".into()))
}

fn decompress(b: Vec<u8>, compression: u8) -> Result<Vec<u8>> {
    match compression {
        // Unknown (0) is read as none, like the reference readers.
        0 | 1 => Ok(b),
        2 => {
            let mut out = Vec::new();
            flate2::read::GzDecoder::new(&b[..])
                .read_to_end(&mut out)
                .map_err(|e| Error::Pack(format!("gzip directory: {e}")))?;
            Ok(out)
        }
        c => Err(Error::Pack(format!(
            "internal compression {c} not supported (gzip or none)"
        ))),
    }
}

/// Decodes a (decompressed) directory.
pub fn parse_directory(b: &[u8]) -> Result<Vec<Entry>> {
    let mut pos = 0;
    let n = varint(b, &mut pos)? as usize;
    let mut e = vec![
        Entry {
            tile_id: 0,
            offset: 0,
            length: 0,
            run_length: 0
        };
        n
    ];
    let mut last = 0u64;
    for x in &mut e {
        last += varint(b, &mut pos)?;
        x.tile_id = last;
    }
    for x in &mut e {
        x.run_length = varint(b, &mut pos)?;
    }
    for x in &mut e {
        x.length = varint(b, &mut pos)?;
    }
    for i in 0..n {
        let v = varint(b, &mut pos)?;
        e[i].offset = if v == 0 && i > 0 {
            e[i - 1].offset + e[i - 1].length
        } else {
            v.saturating_sub(1)
        };
    }
    Ok(e)
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

/// Extracts the tiles of `bbox` up to `maxzoom` from `src` into `out`.
pub fn extract(src: &dyn RangeSource, bbox: BBox, maxzoom: u8, out: &Path) -> Result<ExtractStats> {
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

    let mut stats = ExtractStats::default();
    let maxzoom = maxzoom.min(h.zooms.1);
    let mut ids: Vec<u64> = tiles_covering(bbox, h.zooms.0, maxzoom)
        .into_iter()
        .map(|(z, x, y)| {
            TileCoord::new(z, x, y)
                .map(|c| TileId::from(c).value())
                .map_err(|e| Error::Pack(format!("tile {z}/{x}/{y}: {e}")))
        })
        .collect::<Result<_>>()?;
    ids.sort_unstable();
    // Tile id -> (data offset, length), walking leaf directories (cached by offset).
    let mut leaves: HashMap<u64, Vec<Entry>> = HashMap::new();
    let mut found: Vec<(u64, u64, u64)> = Vec::new();
    for &id in &ids {
        let mut dir_entry = find(&root, id);
        let mut depth = 0;
        while let Some(e) = dir_entry.filter(|e| e.run_length == 0) {
            depth += 1;
            if depth > 8 {
                return Err(Error::Pack("leaf directories nested too deep".into()));
            }
            let key = h.leaves.0 + e.offset;
            let leaf = match leaves.entry(key) {
                std::collections::hash_map::Entry::Occupied(o) => o.into_mut(),
                std::collections::hash_map::Entry::Vacant(v) => {
                    stats.leaf_directories += 1;
                    v.insert(parse_directory(&decompress(
                        src.read(key, e.length)?,
                        h.internal_compression,
                    )?)?)
                }
            };
            dir_entry = find(leaf, id);
        }
        if let Some(e) = dir_entry {
            found.push((id, e.offset, e.length));
        }
    }
    stats.tiles = found.len();

    // Distinct contents sorted by offset, read in merged ranges.
    let mut spans: Vec<(u64, u64)> = found.iter().map(|&(_, o, l)| (o, l)).collect();
    spans.sort_unstable();
    spans.dedup();
    stats.contents = spans.len();
    let mut data: HashMap<(u64, u64), Vec<u8>> = HashMap::new();
    let mut i = 0;
    while i < spans.len() {
        let start = spans[i].0;
        let mut end = spans[i].0 + spans[i].1;
        let mut j = i + 1;
        while j < spans.len() && spans[j].0 <= end + MAX_GAP {
            end = end.max(spans[j].0 + spans[j].1);
            j += 1;
        }
        let block = src.read(h.data.0 + start, end - start)?;
        if block.len() as u64 != end - start {
            return Err(Error::Pack(format!(
                "short read at {}: {} of {} bytes",
                h.data.0 + start,
                block.len(),
                end - start
            )));
        }
        stats.data_reads += 1;
        stats.data_bytes_read += end - start;
        for &(o, l) in &spans[i..j] {
            let a = (o - start) as usize;
            data.insert((o, l), block[a..a + l as usize].to_vec());
        }
        i = j;
    }

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
    let f = File::create(out).map_err(|e| Error::io(out, e))?;
    let c = bbox.center();
    let mut w = PmTilesWriter::new(tile_type)
        .precompressed(compression)
        .min_zoom(h.zooms.0)
        .max_zoom(maxzoom)
        .bounds(bbox.west, bbox.south, bbox.east, bbox.north)
        .center(c.lon, c.lat)
        .center_zoom(maxzoom.min(12).max(h.zooms.0))
        .metadata(metadata)
        .create(f)
        .map_err(|e| Error::pmtiles(out, e))?;
    for (id, o, l) in &found {
        let coord: TileCoord = TileId::new(*id).map_err(|e| Error::pmtiles(out, e))?.into();
        w.add_tile(coord, &data[&(*o, *l)])
            .map_err(|e| Error::pmtiles(out, e))?;
    }
    w.finalize().map_err(|e| Error::pmtiles(out, e))?;
    stats.bytes = std::fs::metadata(out).map_err(|e| Error::io(out, e))?.len();
    Ok(stats)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::build::raster::write_archive;
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
        let s = extract(&src, small, 13, &out).unwrap();
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
        let s = extract(&File::open(&src_path).unwrap(), small, 15, &out).unwrap();
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
