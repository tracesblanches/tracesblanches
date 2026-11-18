//! PMTiles v3 layout (header, directories), read without the `pmtiles` crate: the
//! extract of the Protomaps build (`build::extract`, pipeline only) and the tile-by-tile
//! update of an archive ([`crate::patch`]) walk the directories themselves.

use std::io::Read;

use crate::error::{Error, Result};

/// PMTiles v3 header length.
pub const HEADER_LEN: usize = 127;

/// The fields of a PMTiles v3 header.
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

pub(crate) fn varint(b: &[u8], pos: &mut usize) -> Result<u64> {
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

pub(crate) fn decompress(b: Vec<u8>, compression: u8) -> Result<Vec<u8>> {
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
