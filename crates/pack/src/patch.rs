//! Tile-by-tile update of a PMTiles archive (#160).
//!
//! A new build of a basemap, the snow of the day or any other PMTiles layer changes a few
//! tiles of an archive and leaves the others byte for byte. The update bundle
//! ([`crate::update`], format 3) then carries, instead of the whole new file, a *patch*:
//! the new archive cut along its own layout (header, root directory, metadata, each leaf
//! directory, each distinct tile content, by offset), each region either copied from the
//! old archive (the same bytes are there: an unchanged tile, unchanged metadata) or sent
//! as is (a tile added or changed, a directory whose offsets moved). The phone rebuilds
//! the new file by copying from the old one and appending the bytes sent ([`apply`]); the
//! update checks the SHA-256 of the result against the manifest, so the file installed
//! is the server's byte for byte or nothing.
//!
//! The patch does not rebuild the archive from tile ids: that would need the phone to
//! write directories exactly as the writer of the server did (ordering, clustering, leaf
//! sizes, gzip of each directory). Copying regions only needs the old file, whatever
//! wrote either archive. The tiles added, changed and removed (by tile id) are counted
//! when the patch is made ([`Diff`]).
//!
//! Format (`SKMPTCH1`), all integers unsigned LEB128 varints:
//!
//! ```text
//! "SKMPTCH1"                       8 bytes
//! n                                number of operations
//! n times: (len << 1) | kind       kind 0: copy, 1: bytes sent
//!          offset                  copy only: where the bytes are in the old file
//! the bytes sent, in order         sum of the lengths of the kind-1 operations
//! ```
//!
//! The operations, in order, write the new file from its first byte to its last.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::io::{Read, Seek, SeekFrom, Write};

use serde::Serialize;

use crate::error::{Error, Result};
use crate::pmtiles_v3::{HEADER_LEN, Header, decompress, parse_directory, parse_header};

/// First bytes of a patch.
pub const PATCH_MAGIC: &[u8; 8] = b"SKMPTCH1";
/// Most operations a patch may hold (a bad patch is refused before any allocation).
const MAX_OPS: u64 = 1 << 24;
/// Deepest leaf directory followed (the specification has one level; writers use one).
const MAX_DEPTH: u8 = 4;

/// Tiles of one zoom level that changed.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
pub struct ZoomDiff {
    /// Tile ids only in the new archive.
    pub added: u64,
    /// Tile ids in both, with other contents.
    pub changed: u64,
    /// Tile ids only in the old archive.
    pub removed: u64,
    /// Bytes of the tile contents of this zoom that the old archive does not have (each
    /// distinct content once, at the zoom of its first tile).
    pub new_bytes: u64,
}

/// What a patch holds.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct Diff {
    /// Tile ids only in the new archive.
    pub tiles_added: u64,
    /// Tile ids in both archives, with other contents.
    pub tiles_changed: u64,
    /// Tile ids only in the old archive.
    pub tiles_removed: u64,
    /// Tile ids in both, same contents.
    pub tiles_kept: u64,
    /// Bytes of the new file copied from the old one.
    pub copied_bytes: u64,
    /// Bytes of the new file sent in the patch (tiles, directories, header).
    pub sent_bytes: u64,
    /// Operations of the patch.
    pub ops: u64,
    /// The tile changes by zoom.
    pub by_zoom: BTreeMap<u8, ZoomDiff>,
}

/// A region of an archive: start and length.
type Span = (u64, u64);

/// The layout of an archive: every region (header, directories, metadata, distinct tile
/// contents) and every tile entry.
struct Layout {
    spans: Vec<Span>,
    /// First tile id, run length, contents.
    tiles: Vec<(u64, u64, Span)>,
}

fn region(b: &[u8], (start, len): Span) -> Result<&[u8]> {
    let end = start
        .checked_add(len)
        .filter(|&e| e <= b.len() as u64)
        .ok_or_else(|| Error::Pack(format!("region {start}+{len} beyond the archive")))?;
    Ok(&b[start as usize..end as usize])
}

fn layout(b: &[u8]) -> Result<(Header, Layout)> {
    let h = parse_header(b)?;
    let mut out = Layout {
        spans: vec![(0, HEADER_LEN as u64), h.root, h.metadata],
        tiles: Vec::new(),
    };
    region(b, h.metadata)?;
    let mut dirs = vec![(h.root, 0u8)];
    while let Some((span, depth)) = dirs.pop() {
        let raw = region(b, span)?.to_vec();
        for e in parse_directory(&decompress(raw, h.internal_compression)?)? {
            if e.run_length == 0 {
                if depth >= MAX_DEPTH {
                    return Err(Error::Pack("leaf directories nested too deep".into()));
                }
                let leaf = (h.leaves.0.saturating_add(e.offset), e.length);
                out.spans.push(leaf);
                dirs.push((leaf, depth + 1));
            } else {
                let t = (h.data.0.saturating_add(e.offset), e.length);
                region(b, t)?;
                out.tiles.push((e.tile_id, e.run_length, t));
            }
        }
    }
    out.spans.extend(out.tiles.iter().map(|t| t.2));
    out.spans.retain(|s| s.1 > 0);
    out.spans.sort_unstable();
    out.spans.dedup();
    Ok((h, out))
}

/// `spans` (sorted) and the bytes between them up to `len`, in order. Overlapping spans
/// are kept as they are (the caller refuses them).
fn with_gaps(spans: &[Span], len: u64) -> Vec<Span> {
    let mut out = Vec::with_capacity(spans.len() * 2 + 1);
    let mut at = 0u64;
    for &(start, l) in spans {
        if start > at {
            out.push((at, start - at));
        }
        out.push((start, l));
        at = at.max(start + l);
    }
    if len > at {
        out.push((at, len - at));
    }
    out
}

/// Zoom of a tile id (Hilbert order: the ids of zoom `z` start at `(4^z - 1) / 3`).
pub fn tile_zoom(id: u64) -> u8 {
    let mut z = 0u8;
    let mut first_next = 1u64;
    while id >= first_next && z < 31 {
        z += 1;
        first_next = first_next.saturating_mul(4).saturating_add(1);
    }
    z
}

/// Writes `v` as an unsigned LEB128 varint.
fn put_varint(out: &mut Vec<u8>, mut v: u64) {
    while v >= 0x80 {
        out.push((v as u8 & 0x7f) | 0x80);
        v >>= 7;
    }
    out.push(v as u8);
}

fn read_varint(r: &mut impl Read) -> Result<u64> {
    let mut v = 0u64;
    for shift in (0..64).step_by(7) {
        let mut byte = [0u8];
        r.read_exact(&mut byte)
            .map_err(|e| Error::Pack(format!("patch truncated: {e}")))?;
        v |= u64::from(byte[0] & 0x7f) << shift;
        if byte[0] < 0x80 {
            return Ok(v);
        }
    }
    Err(Error::Pack("patch: varint too long".into()))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Op {
    /// Bytes of the old file: offset, length.
    Copy(u64, u64),
    /// Bytes of the new file sent: offset in the new file, length.
    Send(u64, u64),
}

fn push(ops: &mut Vec<Op>, op: Op) {
    match (ops.last_mut(), op) {
        (_, Op::Copy(_, 0) | Op::Send(_, 0)) => {}
        (Some(Op::Copy(o, l)), Op::Copy(off, len)) if *o + *l == off => *l += len,
        (Some(Op::Send(o, l)), Op::Send(off, len)) if *o + *l == off => *l += len,
        _ => ops.push(op),
    }
}

/// The patch from the archive `old` to the archive `new` (both PMTiles v3, whole files),
/// with what it holds. Fails on what is not a readable PMTiles v3 archive (the update
/// then sends the whole file).
pub fn diff(old: &[u8], new: &[u8]) -> Result<(Vec<u8>, Diff)> {
    let (_, o) = layout(old)?;
    let (_, n) = layout(new)?;
    // Every region of the old archive by its bytes: where to copy it from; and its
    // longest run of zeros (the padding a writer leaves after the root directory, whose
    // length changes with the directory).
    let old_regions = with_gaps(&o.spans, old.len() as u64);
    let mut have: HashMap<&[u8], u64> = HashMap::with_capacity(old_regions.len());
    let mut zeros = (0u64, 0u64);
    for &s in &old_regions {
        let bytes = region(old, s)?;
        have.entry(bytes).or_insert(s.0);
        if s.1 > zeros.1 && bytes.iter().all(|&b| b == 0) {
            zeros = s;
        }
    }
    let mut ops = Vec::new();
    let mut at = 0u64;
    for (start, len) in with_gaps(&n.spans, new.len() as u64) {
        if start != at {
            return Err(Error::Pack(format!(
                "regions overlap at byte {start} of the new archive"
            )));
        }
        let bytes = region(new, (start, len))?;
        push(
            &mut ops,
            match have.get(bytes) {
                Some(&off) => Op::Copy(off, len),
                None if len <= zeros.1 && bytes.iter().all(|&b| b == 0) => Op::Copy(zeros.0, len),
                None => Op::Send(start, len),
            },
        );
        at = start + len;
    }
    if at != new.len() as u64 {
        return Err(Error::Pack("regions beyond the new archive".into()));
    }

    let mut patch = PATCH_MAGIC.to_vec();
    put_varint(&mut patch, ops.len() as u64);
    let mut d = Diff {
        ops: ops.len() as u64,
        ..Diff::default()
    };
    for op in &ops {
        match *op {
            Op::Copy(off, len) => {
                put_varint(&mut patch, len << 1);
                put_varint(&mut patch, off);
                d.copied_bytes += len;
            }
            Op::Send(_, len) => {
                put_varint(&mut patch, (len << 1) | 1);
                d.sent_bytes += len;
            }
        }
    }
    for op in &ops {
        if let Op::Send(off, len) = *op {
            patch.extend_from_slice(region(new, (off, len))?);
        }
    }
    count_tiles(old, &o, new, &n, &mut d)?;
    Ok((patch, d))
}

/// Tile ids of an archive with their contents.
fn by_id<'a>(b: &'a [u8], l: &Layout) -> Result<HashMap<u64, &'a [u8]>> {
    let mut out = HashMap::new();
    for &(id, run, span) in &l.tiles {
        let bytes = region(b, span)?;
        for i in 0..run {
            out.insert(id.saturating_add(i), bytes);
        }
    }
    Ok(out)
}

fn count_tiles(old: &[u8], o: &Layout, new: &[u8], n: &Layout, d: &mut Diff) -> Result<()> {
    let before = by_id(old, o)?;
    let after = by_id(new, n)?;
    let old_contents: HashSet<&[u8]> = before.values().copied().collect();
    let mut sent: HashSet<&[u8]> = HashSet::new();
    let mut ids: Vec<(&u64, &&[u8])> = after.iter().collect();
    ids.sort_unstable();
    for (id, bytes) in ids {
        let z = d.by_zoom.entry(tile_zoom(*id)).or_default();
        match before.get(id) {
            None => {
                d.tiles_added += 1;
                z.added += 1;
            }
            Some(b) if b == bytes => d.tiles_kept += 1,
            Some(_) => {
                d.tiles_changed += 1;
                z.changed += 1;
            }
        }
        if !old_contents.contains(bytes) && sent.insert(bytes) {
            z.new_bytes += bytes.len() as u64;
        }
    }
    for id in before.keys().filter(|id| !after.contains_key(id)) {
        d.tiles_removed += 1;
        d.by_zoom.entry(tile_zoom(*id)).or_default().removed += 1;
    }
    Ok(())
}

/// Rebuilds the new file into `out` from the patch read from `patch` and the old file
/// `old` (`old_len` bytes). Everything is checked before a byte is written: the magic,
/// every copy inside the old file, the total equal to `new_len`; then that the patch
/// holds exactly the bytes its operations send. Returns the bytes written.
pub fn apply<R: Read, O: Read + Seek, W: Write>(
    patch: &mut R,
    old: &mut O,
    old_len: u64,
    out: &mut W,
    new_len: u64,
) -> Result<u64> {
    let mut magic = [0u8; 8];
    patch
        .read_exact(&mut magic)
        .map_err(|e| Error::Pack(format!("patch truncated: {e}")))?;
    if &magic != PATCH_MAGIC {
        return Err(Error::Pack("not a skimap patch".into()));
    }
    let n = read_varint(patch)?;
    if n > MAX_OPS {
        return Err(Error::Pack(format!("patch: {n} operations")));
    }
    let mut ops = Vec::with_capacity(n as usize);
    let mut total = 0u64;
    for _ in 0..n {
        let v = read_varint(patch)?;
        let len = v >> 1;
        let op = if v & 1 == 0 {
            let off = read_varint(patch)?;
            if off.checked_add(len).is_none_or(|e| e > old_len) {
                return Err(Error::Pack(format!(
                    "patch copies {off}+{len}, the old file has {old_len} bytes"
                )));
            }
            Op::Copy(off, len)
        } else {
            Op::Send(0, len)
        };
        total = total
            .checked_add(len)
            .ok_or_else(|| Error::Pack("patch: length overflow".into()))?;
        ops.push(op);
    }
    if total != new_len {
        return Err(Error::Pack(format!(
            "patch writes {total} bytes, the file has {new_len}"
        )));
    }
    let io = |e: std::io::Error| Error::Pack(format!("patch: {e}"));
    for op in ops {
        let (src, len): (&mut dyn Read, u64) = match op {
            Op::Copy(off, len) => {
                old.seek(SeekFrom::Start(off)).map_err(io)?;
                (&mut *old, len)
            }
            Op::Send(_, len) => (&mut *patch, len),
        };
        let k = std::io::copy(&mut src.take(len), out).map_err(io)?;
        if k != len {
            return Err(Error::Pack(format!("patch truncated: {k} bytes of {len}")));
        }
    }
    if patch.read(&mut [0u8; 1]).map_err(io)? != 0 {
        return Err(Error::Pack("patch: bytes after the last operation".into()));
    }
    Ok(total)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zooms_of_tile_ids() {
        assert_eq!(tile_zoom(0), 0);
        assert_eq!(tile_zoom(1), 1);
        assert_eq!(tile_zoom(4), 1);
        assert_eq!(tile_zoom(5), 2);
        assert_eq!(tile_zoom(20), 2);
        assert_eq!(tile_zoom(21), 3);
        // First id of zoom 15: (4^15 - 1) / 3.
        let z15 = (4u64.pow(15) - 1) / 3;
        assert_eq!(tile_zoom(z15 - 1), 14);
        assert_eq!(tile_zoom(z15), 15);
    }

    #[test]
    fn varints_round_trip() {
        for v in [0u64, 1, 127, 128, 300, u64::from(u32::MAX), u64::MAX] {
            let mut b = Vec::new();
            put_varint(&mut b, v);
            assert_eq!(read_varint(&mut b.as_slice()).unwrap(), v);
        }
    }

    #[test]
    fn ops_merge() {
        let mut ops = Vec::new();
        push(&mut ops, Op::Copy(10, 5));
        push(&mut ops, Op::Copy(15, 5));
        push(&mut ops, Op::Send(0, 0));
        push(&mut ops, Op::Copy(30, 1));
        push(&mut ops, Op::Send(40, 2));
        push(&mut ops, Op::Send(42, 3));
        assert_eq!(ops, [Op::Copy(10, 10), Op::Copy(30, 1), Op::Send(40, 5)]);
    }
}
