//! Photos of the reports: JPEG only, small, without metadata (V8 #71, V9 #72).
//!
//! The phone already re-encodes the photo (scaled down, no EXIF); the server does not trust
//! it and rewrites the file keeping only the segments needed to decode the image: every
//! `APPn` segment but the JFIF header (`APP0`: EXIF and XMP are `APP1`, ICC `APP2`, maker
//! notes...) and every comment (`COM`) is dropped.

/// Largest photo accepted, bytes (the server reads request bodies of at most 64 KiB).
pub const MAX_BYTES: usize = 60 * 1024;
/// Largest side accepted, pixels.
pub const MAX_SIDE: u16 = 1280;

/// Why a photo is refused.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum PhotoError {
    /// Not a JPEG, or truncated.
    #[error("not a JPEG image: {0}")]
    Invalid(&'static str),
    /// Over [`MAX_BYTES`].
    #[error("photo over {MAX_BYTES} bytes")]
    TooLarge,
    /// A side over [`MAX_SIDE`].
    #[error("photo of {0}×{1} pixels, at most {MAX_SIDE} per side")]
    TooBig(u16, u16),
}

/// A photo cleaned of its metadata.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Clean {
    /// The JPEG without metadata.
    pub bytes: Vec<u8>,
    /// Width, pixels.
    pub width: u16,
    /// Height, pixels.
    pub height: u16,
    /// Bytes of metadata removed.
    pub removed: usize,
}

/// Checks a JPEG and removes its metadata segments.
pub fn clean(data: &[u8]) -> Result<Clean, PhotoError> {
    if data.len() > MAX_BYTES {
        return Err(PhotoError::TooLarge);
    }
    if data.len() < 4 || data[0] != 0xFF || data[1] != 0xD8 {
        return Err(PhotoError::Invalid("no start of image"));
    }
    let mut out = Vec::with_capacity(data.len());
    out.extend_from_slice(&data[..2]);
    let mut i = 2;
    let mut size = None;
    loop {
        // Fill bytes 0xFF before a marker are allowed.
        while i < data.len() && data[i] == 0xFF && data.get(i + 1) == Some(&0xFF) {
            i += 1;
        }
        if i + 1 >= data.len() || data[i] != 0xFF {
            return Err(PhotoError::Invalid("bad marker"));
        }
        let marker = data[i + 1];
        match marker {
            0xD9 => {
                // End of image before any scan.
                return Err(PhotoError::Invalid("no image data"));
            }
            0x01 | 0xD0..=0xD7 => {
                out.extend_from_slice(&data[i..i + 2]);
                i += 2;
                continue;
            }
            _ => {}
        }
        if i + 4 > data.len() {
            return Err(PhotoError::Invalid("truncated segment"));
        }
        let len = usize::from(u16::from_be_bytes([data[i + 2], data[i + 3]]));
        if len < 2 || i + 2 + len > data.len() {
            return Err(PhotoError::Invalid("truncated segment"));
        }
        let seg = &data[i..i + 2 + len];
        match marker {
            // Start of frame (baseline, extended, progressive, lossless...): the size.
            0xC0..=0xC3 | 0xC5..=0xC7 | 0xC9..=0xCB | 0xCD..=0xCF => {
                if len < 7 {
                    return Err(PhotoError::Invalid("short frame header"));
                }
                let h = u16::from_be_bytes([seg[5], seg[6]]);
                let w = u16::from_be_bytes([seg[7], seg[8]]);
                if w == 0 || h == 0 {
                    return Err(PhotoError::Invalid("empty frame"));
                }
                if w > MAX_SIDE || h > MAX_SIDE {
                    return Err(PhotoError::TooBig(w, h));
                }
                size = Some((w, h));
                out.extend_from_slice(seg);
            }
            // JFIF header kept only if it really is one.
            0xE0 if seg.get(4..9) == Some(b"JFIF\0") => out.extend_from_slice(seg),
            // Metadata: EXIF, XMP, ICC, maker notes, comments.
            0xE0..=0xEF | 0xFE => {}
            // Start of scan: the entropy-coded data follows, up to the end of image.
            0xDA => {
                let Some((width, height)) = size else {
                    return Err(PhotoError::Invalid("scan before frame header"));
                };
                let rest = &data[i..];
                // The end of image is the last 0xFFD9 (trailing garbage is dropped too).
                let end = rest
                    .windows(2)
                    .rposition(|w| w == [0xFF, 0xD9])
                    .ok_or(PhotoError::Invalid("no end of image"))?;
                out.extend_from_slice(&rest[..end + 2]);
                return Ok(Clean {
                    removed: data.len() - out.len(),
                    bytes: out,
                    width,
                    height,
                });
            }
            _ => out.extend_from_slice(seg),
        }
        i += 2 + len;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn seg(marker: u8, payload: &[u8]) -> Vec<u8> {
        let mut v = vec![0xFF, marker];
        v.extend_from_slice(&u16::try_from(payload.len() + 2).unwrap().to_be_bytes());
        v.extend_from_slice(payload);
        v
    }

    /// A JPEG-shaped file: SOI, JFIF, EXIF with GPS, comment, frame, scan, EOI.
    fn sample(w: u16, h: u16) -> Vec<u8> {
        let mut v = vec![0xFF, 0xD8];
        v.extend(seg(0xE0, b"JFIF\0\x01\x01\0\0\x01\0\x01\0\0"));
        v.extend(seg(0xE1, b"Exif\0\0GPSLatitude 44.85 GPSLongitude 6.77"));
        v.extend(seg(0xFE, b"taken by someone"));
        v.extend(seg(0xDB, &[0u8; 65]));
        let mut sof = vec![8];
        sof.extend_from_slice(&h.to_be_bytes());
        sof.extend_from_slice(&w.to_be_bytes());
        sof.extend_from_slice(&[1, 1, 0x11, 0]);
        v.extend(seg(0xC0, &sof));
        v.extend(seg(0xDA, &[1, 1, 0, 0, 63, 0]));
        v.extend_from_slice(&[0x12, 0x34, 0xFF, 0x00, 0x56, 0xFF, 0xD0, 0x78]);
        v.extend_from_slice(&[0xFF, 0xD9]);
        v
    }

    #[test]
    fn metadata_removed_image_kept() {
        let raw = sample(640, 480);
        let c = clean(&raw).unwrap();
        assert_eq!((c.width, c.height), (640, 480));
        let s = String::from_utf8_lossy(&c.bytes);
        assert!(!s.contains("Exif") && !s.contains("GPS") && !s.contains("someone"));
        assert!(s.contains("JFIF"));
        assert!(c.bytes.ends_with(&[0x56, 0xFF, 0xD0, 0x78, 0xFF, 0xD9]));
        assert_eq!(c.removed, raw.len() - c.bytes.len());
        // Idempotent.
        assert_eq!(clean(&c.bytes).unwrap().bytes, c.bytes);
    }

    #[test]
    fn refusals() {
        assert_eq!(
            clean(b"GIF89a....").unwrap_err(),
            PhotoError::Invalid("no start of image")
        );
        assert_eq!(
            clean(&sample(4000, 3000)).unwrap_err(),
            PhotoError::TooBig(4000, 3000)
        );
        let mut big = sample(100, 100);
        big.resize(MAX_BYTES + 1, 0);
        assert_eq!(clean(&big).unwrap_err(), PhotoError::TooLarge);
        let s = sample(100, 100);
        assert!(clean(&s[..s.len() - 2]).is_err(), "no end of image");
        assert!(clean(&s[..30]).is_err(), "truncated");
    }
}
