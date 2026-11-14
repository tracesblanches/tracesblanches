//! Minimal PNG codec for 8-bit palette images (raster class tiles).
//!
//! The workspace has no PNG crate. Class rasters only need colour type 3 (palette), bit
//! depth 8, no interlacing: the palette index is the class value, so the same tile is
//! drawn by MapLibre and read back by the phone core.

use std::io::{Read, Write};

use flate2::Compression;
use flate2::read::ZlibDecoder;
use flate2::write::ZlibEncoder;

use crate::error::{Error, Result};

const SIGNATURE: [u8; 8] = [0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];

/// A decoded palette image.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Indexed {
    /// Width in pixels.
    pub width: u32,
    /// Height in pixels.
    pub height: u32,
    /// Palette RGBA (alpha from `tRNS`, 255 when absent).
    pub palette: Vec<[u8; 4]>,
    /// Palette index of each pixel, row-major.
    pub indices: Vec<u8>,
}

impl Indexed {
    /// Palette index at `(x, y)`.
    pub fn index_at(&self, x: u32, y: u32) -> Option<u8> {
        if x >= self.width || y >= self.height {
            return None;
        }
        self.indices
            .get(y as usize * self.width as usize + x as usize)
            .copied()
    }
}

fn chunk(out: &mut Vec<u8>, kind: &[u8; 4], data: &[u8]) {
    out.extend_from_slice(&(data.len() as u32).to_be_bytes());
    out.extend_from_slice(kind);
    out.extend_from_slice(data);
    let mut crc = flate2::Crc::new();
    crc.update(kind);
    crc.update(data);
    out.extend_from_slice(&crc.sum().to_be_bytes());
}

/// Encodes a palette image. `palette` has at most 256 entries; every index must be in it.
pub fn encode_indexed(
    width: u32,
    height: u32,
    palette: &[[u8; 4]],
    indices: &[u8],
) -> Result<Vec<u8>> {
    if palette.is_empty() || palette.len() > 256 {
        return Err(Error::Png(format!("palette of {} entries", palette.len())));
    }
    if indices.len() != width as usize * height as usize {
        return Err(Error::Png(format!(
            "{} indices for {width}x{height}",
            indices.len()
        )));
    }
    if let Some(i) = indices.iter().find(|&&i| usize::from(i) >= palette.len()) {
        return Err(Error::Png(format!("index {i} outside the palette")));
    }
    let mut out = Vec::with_capacity(256 + indices.len() / 8);
    out.extend_from_slice(&SIGNATURE);
    let mut ihdr = Vec::with_capacity(13);
    ihdr.extend_from_slice(&width.to_be_bytes());
    ihdr.extend_from_slice(&height.to_be_bytes());
    ihdr.extend_from_slice(&[8, 3, 0, 0, 0]);
    chunk(&mut out, b"IHDR", &ihdr);
    let plte: Vec<u8> = palette.iter().flat_map(|c| [c[0], c[1], c[2]]).collect();
    chunk(&mut out, b"PLTE", &plte);
    // tRNS up to the last non-opaque entry.
    if let Some(last) = palette.iter().rposition(|c| c[3] != 255) {
        let trns: Vec<u8> = palette.iter().take(last + 1).map(|c| c[3]).collect();
        chunk(&mut out, b"tRNS", &trns);
    }
    let mut z = ZlibEncoder::new(Vec::new(), Compression::best());
    let w = width as usize;
    let io = |e: std::io::Error| Error::Png(e.to_string());
    for row in indices.chunks(w.max(1)) {
        z.write_all(&[0]).map_err(io)?; // filter: none (recommended for palette images)
        z.write_all(row).map_err(io)?;
    }
    let idat = z.finish().map_err(io)?;
    chunk(&mut out, b"IDAT", &idat);
    chunk(&mut out, b"IEND", &[]);
    Ok(out)
}

fn be_u32(b: &[u8], at: usize) -> Result<u32> {
    b.get(at..at + 4)
        .map(|s| u32::from_be_bytes([s[0], s[1], s[2], s[3]]))
        .ok_or_else(|| Error::Png("truncated".into()))
}

fn paeth(a: u8, b: u8, c: u8) -> u8 {
    let (ia, ib, ic) = (i16::from(a), i16::from(b), i16::from(c));
    let p = ia + ib - ic;
    let (pa, pb, pc) = ((p - ia).abs(), (p - ib).abs(), (p - ic).abs());
    if pa <= pb && pa <= pc {
        a
    } else if pb <= pc {
        b
    } else {
        c
    }
}

/// Decodes an 8-bit palette PNG (any filter, no interlacing).
pub fn decode_indexed(bytes: &[u8]) -> Result<Indexed> {
    if bytes.get(..8) != Some(&SIGNATURE[..]) {
        return Err(Error::Png("not a PNG".into()));
    }
    let mut at = 8;
    let (mut width, mut height) = (0u32, 0u32);
    let mut palette: Vec<[u8; 4]> = Vec::new();
    let mut idat = Vec::new();
    let mut seen_ihdr = false;
    while at + 8 <= bytes.len() {
        let len = be_u32(bytes, at)? as usize;
        let kind = &bytes[at + 4..at + 8];
        let data = bytes
            .get(at + 8..at + 8 + len)
            .ok_or_else(|| Error::Png("truncated chunk".into()))?;
        let crc_stored = be_u32(bytes, at + 8 + len)?;
        let mut crc = flate2::Crc::new();
        crc.update(kind);
        crc.update(data);
        if crc.sum() != crc_stored {
            return Err(Error::Png(format!(
                "bad CRC in chunk {}",
                String::from_utf8_lossy(kind)
            )));
        }
        match kind {
            b"IHDR" => {
                if data.len() != 13 {
                    return Err(Error::Png("bad IHDR".into()));
                }
                width = be_u32(data, 0)?;
                height = be_u32(data, 4)?;
                let (depth, colour, interlace) = (data[8], data[9], data[12]);
                if depth != 8 || colour != 3 || interlace != 0 {
                    return Err(Error::Png(format!(
                        "unsupported PNG (bit depth {depth}, colour type {colour}, interlace {interlace}): \
                         only 8-bit palette images are read"
                    )));
                }
                seen_ihdr = true;
            }
            b"PLTE" => {
                palette = data
                    .as_chunks::<3>()
                    .0
                    .iter()
                    .map(|c| [c[0], c[1], c[2], 255])
                    .collect();
            }
            b"tRNS" => {
                for (p, a) in palette.iter_mut().zip(data) {
                    p[3] = *a;
                }
            }
            b"IDAT" => idat.extend_from_slice(data),
            b"IEND" => break,
            _ => {}
        }
        at += 12 + len;
    }
    if !seen_ihdr || palette.is_empty() {
        return Err(Error::Png("missing IHDR or PLTE".into()));
    }
    let (w, h) = (width as usize, height as usize);
    let mut raw = Vec::with_capacity((w + 1) * h);
    ZlibDecoder::new(&idat[..])
        .read_to_end(&mut raw)
        .map_err(|e| Error::Png(format!("IDAT: {e}")))?;
    if raw.len() != (w + 1) * h {
        return Err(Error::Png(format!(
            "IDAT holds {} bytes, expected {}",
            raw.len(),
            (w + 1) * h
        )));
    }
    let mut indices = vec![0u8; w * h];
    for y in 0..h {
        let filter = raw[y * (w + 1)];
        let src = &raw[y * (w + 1) + 1..(y + 1) * (w + 1)];
        let (done, rest) = indices.split_at_mut(y * w);
        let prev = if y == 0 {
            None
        } else {
            Some(&done[(y - 1) * w..])
        };
        let cur = &mut rest[..w];
        for x in 0..w {
            let a = if x == 0 { 0 } else { cur[x - 1] };
            let b = prev.map_or(0, |p| p[x]);
            let c = if x == 0 {
                0
            } else {
                prev.map_or(0, |p| p[x - 1])
            };
            cur[x] = match filter {
                0 => src[x],
                1 => src[x].wrapping_add(a),
                2 => src[x].wrapping_add(b),
                3 => src[x].wrapping_add(((u16::from(a) + u16::from(b)) / 2) as u8),
                4 => src[x].wrapping_add(paeth(a, b, c)),
                f => return Err(Error::Png(format!("unknown filter {f}"))),
            };
        }
    }
    Ok(Indexed {
        width,
        height,
        palette,
        indices,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip() {
        let palette = [[0, 0, 0, 0], [242, 229, 0, 255], [240, 0, 0, 200]];
        let (w, h) = (7u32, 5u32);
        let indices: Vec<u8> = (0..w * h).map(|i| (i % 3) as u8).collect();
        let png = encode_indexed(w, h, &palette, &indices).unwrap();
        let back = decode_indexed(&png).unwrap();
        assert_eq!(back.width, w);
        assert_eq!(back.height, h);
        assert_eq!(back.indices, indices);
        assert_eq!(back.palette, palette.to_vec());
        assert_eq!(back.index_at(1, 0), Some(1));
        assert_eq!(back.index_at(7, 0), None);
    }

    #[test]
    fn decodes_all_filters() {
        // Same image re-encoded by hand with filters 1..=4 on successive rows.
        let (w, h) = (4usize, 5usize);
        let img: Vec<u8> = vec![0, 1, 2, 3, 3, 2, 1, 0, 1, 1, 2, 2, 0, 3, 0, 3, 2, 2, 2, 2];
        let mut raw = Vec::new();
        for y in 0..h {
            let f = (y % 5) as u8;
            raw.push(f);
            for x in 0..w {
                let v = img[y * w + x];
                let a = if x == 0 { 0 } else { img[y * w + x - 1] };
                let b = if y == 0 { 0 } else { img[(y - 1) * w + x] };
                let c = if x == 0 || y == 0 {
                    0
                } else {
                    img[(y - 1) * w + x - 1]
                };
                let pred = match f {
                    0 => 0,
                    1 => a,
                    2 => b,
                    3 => ((u16::from(a) + u16::from(b)) / 2) as u8,
                    _ => paeth(a, b, c),
                };
                raw.push(v.wrapping_sub(pred));
            }
        }
        let mut z = ZlibEncoder::new(Vec::new(), Compression::default());
        z.write_all(&raw).unwrap();
        let idat = z.finish().unwrap();
        let mut png = SIGNATURE.to_vec();
        let mut ihdr = (w as u32).to_be_bytes().to_vec();
        ihdr.extend_from_slice(&(h as u32).to_be_bytes());
        ihdr.extend_from_slice(&[8, 3, 0, 0, 0]);
        chunk(&mut png, b"IHDR", &ihdr);
        chunk(&mut png, b"PLTE", &[0; 12]);
        chunk(&mut png, b"IDAT", &idat);
        chunk(&mut png, b"IEND", &[]);
        assert_eq!(decode_indexed(&png).unwrap().indices, img);
    }

    #[test]
    fn rejects_bad_input() {
        assert!(decode_indexed(b"not a png").is_err());
        assert!(encode_indexed(2, 2, &[[0; 4]], &[0, 0, 0, 1]).is_err());
        assert!(encode_indexed(2, 2, &[[0; 4]], &[0, 0, 0]).is_err());
        let mut png = encode_indexed(1, 1, &[[1, 2, 3, 255]], &[0]).unwrap();
        let n = png.len();
        png[n - 20] ^= 0xFF; // corrupt IDAT
        assert!(decode_indexed(&png).is_err());
    }
}
