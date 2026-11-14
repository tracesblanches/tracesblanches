//! Minimal PNG decoder (8-bit, non-interlaced) for the IGN `SLOPES.MOUNTAIN` WMTS tiles.
//!
//! The workspace has no PNG crate; this decoder covers what the Géoplateforme serves
//! (8-bit RGBA, RGB, palette with `tRNS`, grey, grey + alpha) and rejects the rest.
//! Decompression uses `flate2` (zlib, Adler-32 checked).

use std::io::Read;

use crate::error::{Error, Result};

/// A decoded image, RGBA 8-bit, row-major.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rgba {
    /// Width in pixels.
    pub width: usize,
    /// Height in pixels.
    pub height: usize,
    /// `width * height * 4` bytes.
    pub data: Vec<u8>,
}

impl Rgba {
    /// Pixel `(x, y)` as `[r, g, b, a]`.
    pub fn pixel(&self, x: usize, y: usize) -> [u8; 4] {
        let i = (y * self.width + x) * 4;
        [
            self.data[i],
            self.data[i + 1],
            self.data[i + 2],
            self.data[i + 3],
        ]
    }
}

const SIGNATURE: [u8; 8] = [0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];

fn be32(b: &[u8]) -> u32 {
    u32::from_be_bytes([b[0], b[1], b[2], b[3]])
}

fn paeth(a: u8, b: u8, c: u8) -> u8 {
    let (a16, b16, c16) = (i16::from(a), i16::from(b), i16::from(c));
    let p = a16 + b16 - c16;
    let (pa, pb, pc) = ((p - a16).abs(), (p - b16).abs(), (p - c16).abs());
    if pa <= pb && pa <= pc {
        a
    } else if pb <= pc {
        b
    } else {
        c
    }
}

/// Decodes a PNG into RGBA.
pub fn decode(bytes: &[u8]) -> Result<Rgba> {
    let err = |m: &str| Error::Png(m.to_owned());
    if bytes.len() < 8 || bytes[..8] != SIGNATURE {
        return Err(err("missing PNG signature"));
    }
    let mut pos = 8;
    let (mut width, mut height, mut color_type) = (0usize, 0usize, 0u8);
    let mut header = false;
    let mut palette: Vec<[u8; 4]> = Vec::new();
    let mut trns: Vec<u8> = Vec::new();
    let mut idat = Vec::new();
    while pos + 8 <= bytes.len() {
        let len = be32(&bytes[pos..]) as usize;
        let kind = &bytes[pos + 4..pos + 8];
        let start = pos + 8;
        let end = start
            .checked_add(len)
            .filter(|e| e + 4 <= bytes.len())
            .ok_or_else(|| err("truncated chunk"))?;
        let body = &bytes[start..end];
        match kind {
            b"IHDR" => {
                if len != 13 {
                    return Err(err("bad IHDR"));
                }
                width = be32(body) as usize;
                height = be32(&body[4..]) as usize;
                let depth = body[8];
                color_type = body[9];
                let interlace = body[12];
                if depth != 8 {
                    return Err(Error::Png(format!("unsupported bit depth {depth}")));
                }
                if interlace != 0 {
                    return Err(err("interlaced PNG not supported"));
                }
                if !matches!(color_type, 0 | 2 | 3 | 4 | 6) {
                    return Err(Error::Png(format!("bad colour type {color_type}")));
                }
                header = true;
            }
            b"PLTE" => {
                palette = body
                    .as_chunks::<3>()
                    .0
                    .iter()
                    .map(|c| [c[0], c[1], c[2], 255])
                    .collect();
            }
            b"tRNS" => trns = body.to_vec(),
            b"IDAT" => idat.extend_from_slice(body),
            b"IEND" => break,
            _ => {}
        }
        pos = end + 4; // skip CRC
    }
    if !header || width == 0 || height == 0 {
        return Err(err("missing IHDR"));
    }
    for (p, a) in palette.iter_mut().zip(&trns) {
        p[3] = *a;
    }
    let channels = match color_type {
        0 | 3 => 1,
        4 => 2,
        2 => 3,
        _ => 4,
    };
    let stride = width * channels;
    let mut raw = Vec::with_capacity((stride + 1) * height);
    flate2::read::ZlibDecoder::new(&idat[..])
        .read_to_end(&mut raw)
        .map_err(|e| Error::Png(format!("zlib: {e}")))?;
    if raw.len() < (stride + 1) * height {
        return Err(err("image data too short"));
    }
    let bpp = channels;
    let mut prev = vec![0u8; stride];
    let mut cur = vec![0u8; stride];
    let mut out = Vec::with_capacity(width * height * 4);
    for y in 0..height {
        let line = &raw[y * (stride + 1)..(y + 1) * (stride + 1)];
        let filter = line[0];
        cur.copy_from_slice(&line[1..]);
        for i in 0..stride {
            let a = if i >= bpp { cur[i - bpp] } else { 0 };
            let b = prev[i];
            let c = if i >= bpp { prev[i - bpp] } else { 0 };
            let pred = match filter {
                0 => 0,
                1 => a,
                2 => b,
                3 => ((u16::from(a) + u16::from(b)) / 2) as u8,
                4 => paeth(a, b, c),
                f => return Err(Error::Png(format!("bad filter type {f}"))),
            };
            cur[i] = cur[i].wrapping_add(pred);
        }
        for px in cur.chunks_exact(channels) {
            let rgba = match color_type {
                0 => {
                    let a = if trns.len() >= 2
                        && u16::from(px[0]) == u16::from_be_bytes([trns[0], trns[1]])
                    {
                        0
                    } else {
                        255
                    };
                    [px[0], px[0], px[0], a]
                }
                4 => [px[0], px[0], px[0], px[1]],
                2 => [px[0], px[1], px[2], 255],
                3 => *palette
                    .get(usize::from(px[0]))
                    .ok_or_else(|| err("palette index out of range"))?,
                _ => [px[0], px[1], px[2], px[3]],
            };
            out.extend_from_slice(&rgba);
        }
        std::mem::swap(&mut prev, &mut cur);
    }
    Ok(Rgba {
        width,
        height,
        data: out,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn chunk(out: &mut Vec<u8>, kind: &[u8], body: &[u8]) {
        out.extend_from_slice(&(body.len() as u32).to_be_bytes());
        out.extend_from_slice(kind);
        out.extend_from_slice(body);
        out.extend_from_slice(&[0, 0, 0, 0]); // CRC not checked
    }

    fn png(
        width: u32,
        height: u32,
        color_type: u8,
        extra: &[(&[u8], Vec<u8>)],
        raw: &[u8],
    ) -> Vec<u8> {
        let mut out = SIGNATURE.to_vec();
        let mut ihdr = Vec::new();
        ihdr.extend_from_slice(&width.to_be_bytes());
        ihdr.extend_from_slice(&height.to_be_bytes());
        ihdr.extend_from_slice(&[8, color_type, 0, 0, 0]);
        chunk(&mut out, b"IHDR", &ihdr);
        for (k, b) in extra {
            chunk(&mut out, k, b);
        }
        let mut z = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
        z.write_all(raw).unwrap();
        chunk(&mut out, b"IDAT", &z.finish().unwrap());
        chunk(&mut out, b"IEND", &[]);
        out
    }

    #[test]
    fn rgba_with_all_filters() {
        // 2 x 5 RGBA image; each row uses another filter type. Expected pixels:
        let px = [
            [[10, 20, 30, 255], [40, 50, 60, 0]],
            [[11, 21, 31, 255], [41, 51, 61, 128]],
            [[12, 22, 32, 255], [42, 52, 62, 255]],
            [[13, 23, 33, 255], [43, 53, 63, 255]],
            [[14, 24, 34, 255], [44, 54, 64, 255]],
        ];
        let mut raw = Vec::new();
        let mut prev = [0u8; 8];
        for (filter, row) in px.iter().enumerate() {
            let cur: Vec<u8> = row.iter().flatten().copied().collect();
            raw.push(filter as u8);
            for i in 0..8 {
                let a = if i >= 4 { cur[i - 4] } else { 0 };
                let b = prev[i];
                let c = if i >= 4 { prev[i - 4] } else { 0 };
                let pred = match filter {
                    0 => 0,
                    1 => a,
                    2 => b,
                    3 => ((u16::from(a) + u16::from(b)) / 2) as u8,
                    _ => paeth(a, b, c),
                };
                raw.push(cur[i].wrapping_sub(pred));
            }
            prev.copy_from_slice(&cur);
        }
        let img = decode(&png(2, 5, 6, &[], &raw)).unwrap();
        assert_eq!((img.width, img.height), (2, 5));
        for (y, row) in px.iter().enumerate() {
            for (x, p) in row.iter().enumerate() {
                assert_eq!(img.pixel(x, y), *p, "({x},{y})");
            }
        }
    }

    #[test]
    fn palette_with_transparency() {
        let plte = vec![255, 0, 0, 0, 255, 0];
        let trns = vec![0];
        let raw = [0, 0, 1];
        let img = decode(&png(2, 1, 3, &[(b"PLTE", plte), (b"tRNS", trns)], &raw)).unwrap();
        assert_eq!(img.pixel(0, 0), [255, 0, 0, 0]);
        assert_eq!(img.pixel(1, 0), [0, 255, 0, 255]);
    }

    #[test]
    fn rejects_garbage() {
        assert!(decode(b"not a png").is_err());
        assert!(decode(&SIGNATURE).is_err());
    }
}
