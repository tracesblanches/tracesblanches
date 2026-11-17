//! RGB raster of the rendered plan (pipeline input only, never written to the outputs)
//! and a few drawing helpers for diagnostic images.

use std::path::Path;

use crate::error::{Error, Result};

/// 8-bit RGB image, row-major.
#[derive(Debug, Clone, PartialEq)]
pub struct Rgb {
    /// Width, pixels.
    pub w: usize,
    /// Height, pixels.
    pub h: usize,
    /// `3 · w · h` bytes.
    pub px: Vec<u8>,
}

impl Rgb {
    /// Blank image of one colour.
    pub fn new(w: usize, h: usize, c: [u8; 3]) -> Rgb {
        let mut px = Vec::with_capacity(w * h * 3);
        for _ in 0..w * h {
            px.extend_from_slice(&c);
        }
        Rgb { w, h, px }
    }

    /// Pixel at (x, y).
    pub fn get(&self, x: usize, y: usize) -> [u8; 3] {
        let i = (y * self.w + x) * 3;
        [self.px[i], self.px[i + 1], self.px[i + 2]]
    }

    /// Sets a pixel (ignored outside the image).
    pub fn put(&mut self, x: i64, y: i64, c: [u8; 3]) {
        if x >= 0 && y >= 0 && (x as usize) < self.w && (y as usize) < self.h {
            let i = (y as usize * self.w + x as usize) * 3;
            self.px[i..i + 3].copy_from_slice(&c);
        }
    }

    /// Reads a binary PPM (`P6`, maxval 255), as written by `pdftoppm`.
    pub fn read_ppm(path: &Path) -> Result<Rgb> {
        let bytes = std::fs::read(path).map_err(|e| Error::io(path, e))?;
        Rgb::parse_ppm(&bytes).map_err(|m| Error::Data(format!("{}: {m}", path.display())))
    }

    /// Parses a binary PPM.
    pub fn parse_ppm(bytes: &[u8]) -> std::result::Result<Rgb, String> {
        let mut fields = Vec::new();
        let mut i = 0;
        while fields.len() < 4 {
            while i < bytes.len() && bytes[i].is_ascii_whitespace() {
                i += 1;
            }
            if i < bytes.len() && bytes[i] == b'#' {
                while i < bytes.len() && bytes[i] != b'\n' {
                    i += 1;
                }
                continue;
            }
            let start = i;
            while i < bytes.len() && !bytes[i].is_ascii_whitespace() {
                i += 1;
            }
            if start == i {
                return Err("truncated header".into());
            }
            fields.push(String::from_utf8_lossy(&bytes[start..i]).into_owned());
        }
        i += 1; // single whitespace after maxval
        if fields[0] != "P6" {
            return Err(format!("not a binary PPM ({})", fields[0]));
        }
        let num = |s: &str| s.parse::<usize>().map_err(|e| format!("{s}: {e}"));
        let (w, h, max) = (num(&fields[1])?, num(&fields[2])?, num(&fields[3])?);
        if max != 255 {
            return Err(format!("maxval {max} unsupported"));
        }
        let n = w * h * 3;
        if bytes.len() < i + n {
            return Err("truncated pixel data".into());
        }
        Ok(Rgb {
            w,
            h,
            px: bytes[i..i + n].to_vec(),
        })
    }

    /// Decodes a PNG or JPEG picture (by its magic bytes) to RGB: grey expanded, alpha
    /// composed over white.
    pub fn decode_picture(bytes: &[u8]) -> std::result::Result<Rgb, String> {
        let (w, h, ch, data) = if bytes.starts_with(&[0x89, b'P', b'N', b'G']) {
            let mut dec = png::Decoder::new(std::io::Cursor::new(bytes));
            dec.set_transformations(png::Transformations::normalize_to_color8());
            let mut r = dec.read_info().map_err(|e| format!("PNG: {e}"))?;
            let mut buf = vec![0; r.output_buffer_size().ok_or("PNG: size")?];
            let info = r.next_frame(&mut buf).map_err(|e| format!("PNG: {e}"))?;
            buf.truncate(info.buffer_size());
            let ch = info.color_type.samples();
            (info.width as usize, info.height as usize, ch, buf)
        } else if bytes.starts_with(&[0xFF, 0xD8]) {
            let mut dec = zune_jpeg::JpegDecoder::new(std::io::Cursor::new(bytes));
            let data = dec.decode().map_err(|e| format!("JPEG: {e:?}"))?;
            let info = dec.info().ok_or("JPEG: no header")?;
            let (w, h) = (info.width as usize, info.height as usize);
            let ch = data.len() / (w * h).max(1);
            (w, h, ch, data)
        } else {
            return Err("neither PNG nor JPEG".into());
        };
        if !(1..=4).contains(&ch) || data.len() < w * h * ch {
            return Err(format!("{ch} channels, {} bytes for {w} × {h}", data.len()));
        }
        let mut px = Vec::with_capacity(w * h * 3);
        for p in data.chunks_exact(ch).take(w * h) {
            let (rgb, a) = match ch {
                1 => ([p[0]; 3], 255),
                2 => ([p[0]; 3], p[1]),
                3 => ([p[0], p[1], p[2]], 255),
                _ => ([p[0], p[1], p[2]], p[3]),
            };
            let over = |v: u8| ((v as u32 * a as u32 + 255 * (255 - a as u32)) / 255) as u8;
            px.extend_from_slice(&[over(rgb[0]), over(rgb[1]), over(rgb[2])]);
        }
        Ok(Rgb { w, h, px })
    }

    /// Resampled by `k` (bilinear; pixel centres aligned).
    pub fn scaled(&self, k: f64) -> Rgb {
        let (w, h) = (
            ((self.w as f64 * k).round() as usize).max(1),
            ((self.h as f64 * k).round() as usize).max(1),
        );
        let mut px = vec![0u8; w * h * 3];
        px.chunks_mut(w * 3).enumerate().for_each(|(y, row)| {
            let sy = ((y as f64 + 0.5) / k - 0.5).clamp(0.0, (self.h - 1) as f64);
            let (y0, fy) = (sy.floor() as usize, sy.fract());
            let y1 = (y0 + 1).min(self.h - 1);
            for x in 0..w {
                let sx = ((x as f64 + 0.5) / k - 0.5).clamp(0.0, (self.w - 1) as f64);
                let (x0, fx) = (sx.floor() as usize, sx.fract());
                let x1 = (x0 + 1).min(self.w - 1);
                let (a, b, c, d) = (
                    self.get(x0, y0),
                    self.get(x1, y0),
                    self.get(x0, y1),
                    self.get(x1, y1),
                );
                for i in 0..3 {
                    let top = a[i] as f64 * (1.0 - fx) + b[i] as f64 * fx;
                    let bot = c[i] as f64 * (1.0 - fx) + d[i] as f64 * fx;
                    row[x * 3 + i] = (top * (1.0 - fy) + bot * fy).round() as u8;
                }
            }
        });
        Rgb { w, h, px }
    }

    /// Writes a binary PPM (the cached rendering).
    pub fn write_ppm(&self, path: &Path) -> Result<()> {
        let mut b = format!("P6\n{} {}\n255\n", self.w, self.h).into_bytes();
        b.extend_from_slice(&self.px);
        Ok(skimap_common::output::write_bytes(path, &b)?)
    }

    /// Writes a PNG (diagnostics and demo images).
    pub fn write_png(&self, path: &Path) -> Result<()> {
        let mut f = skimap_common::output::AtomicFile::create(path)?;
        let mut enc = png::Encoder::new(&mut f, self.w as u32, self.h as u32);
        enc.set_color(png::ColorType::Rgb);
        enc.set_depth(png::BitDepth::Eight);
        let mut w = enc
            .write_header()
            .map_err(|e| Error::Data(format!("PNG {}: {e}", path.display())))?;
        w.write_image_data(&self.px)
            .map_err(|e| Error::Data(format!("PNG {}: {e}", path.display())))?;
        w.finish()
            .map_err(|e| Error::Data(format!("PNG {}: {e}", path.display())))?;
        f.commit()?;
        Ok(())
    }

    /// Filled disc.
    pub fn disc(&mut self, cx: f64, cy: f64, r: f64, c: [u8; 3]) {
        let ri = r.ceil() as i64;
        for dy in -ri..=ri {
            for dx in -ri..=ri {
                if (dx * dx + dy * dy) as f64 <= r * r {
                    self.put(cx.round() as i64 + dx, cy.round() as i64 + dy, c);
                }
            }
        }
    }

    /// Thick segment.
    pub fn segment(&mut self, a: [f64; 2], b: [f64; 2], width: f64, c: [u8; 3]) {
        let len = (b[0] - a[0]).hypot(b[1] - a[1]);
        let steps = (len * 2.0).ceil().max(1.0) as usize;
        for k in 0..=steps {
            let t = k as f64 / steps as f64;
            self.disc(
                a[0] + (b[0] - a[0]) * t,
                a[1] + (b[1] - a[1]) * t,
                width / 2.0,
                c,
            );
        }
    }

    /// Thick polyline.
    pub fn polyline(&mut self, pts: &[[f64; 2]], width: f64, c: [u8; 3]) {
        for w in pts.windows(2) {
            self.segment(w[0], w[1], width, c);
        }
    }

    /// Blends towards white (`k` in 0..1), to draw over a faded background.
    pub fn fade(&mut self, k: f64) {
        for v in &mut self.px {
            *v = (*v as f64 + (255.0 - *v as f64) * k).round() as u8;
        }
    }
}

/// CIE L*a*b* (D65) of an sRGB colour.
pub fn lab(c: [u8; 3]) -> [f32; 3] {
    let lin = |v: u8| {
        let v = v as f32 / 255.0;
        if v <= 0.04045 {
            v / 12.92
        } else {
            ((v + 0.055) / 1.055).powf(2.4)
        }
    };
    let (r, g, b) = (lin(c[0]), lin(c[1]), lin(c[2]));
    let x = (0.4124 * r + 0.3576 * g + 0.1805 * b) / 0.95047;
    let y = 0.2126 * r + 0.7152 * g + 0.0722 * b;
    let z = (0.0193 * r + 0.1192 * g + 0.9505 * b) / 1.08883;
    let f = |t: f32| {
        if t > 0.008856 {
            t.cbrt()
        } else {
            7.787 * t + 16.0 / 116.0
        }
    };
    let (fx, fy, fz) = (f(x), f(y), f(z));
    [116.0 * fy - 16.0, 500.0 * (fx - fy), 200.0 * (fy - fz)]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_ppm_with_comment() {
        let mut b = b"P6\n# c\n2 1\n255\n".to_vec();
        b.extend_from_slice(&[1, 2, 3, 4, 5, 6]);
        let img = Rgb::parse_ppm(&b).unwrap();
        assert_eq!((img.w, img.h), (2, 1));
        assert_eq!(img.get(1, 0), [4, 5, 6]);
        assert!(Rgb::parse_ppm(b"P3\n1 1\n255\n").is_err());
    }

    #[test]
    fn decodes_png_and_scales() {
        // 2 × 1 RGBA PNG: opaque red, transparent (over white).
        let mut bytes = Vec::new();
        {
            let mut enc = png::Encoder::new(&mut bytes, 2, 1);
            enc.set_color(png::ColorType::Rgba);
            enc.set_depth(png::BitDepth::Eight);
            let mut w = enc.write_header().unwrap();
            w.write_image_data(&[255, 0, 0, 255, 0, 0, 0, 0]).unwrap();
        }
        let img = Rgb::decode_picture(&bytes).unwrap();
        assert_eq!((img.w, img.h), (2, 1));
        assert_eq!(img.get(0, 0), [255, 0, 0]);
        assert_eq!(img.get(1, 0), [255, 255, 255]);
        assert!(Rgb::decode_picture(b"%PDF-1.4").is_err());
        let s = img.scaled(2.0);
        assert_eq!((s.w, s.h), (4, 2));
        assert_eq!(s.get(0, 0), [255, 0, 0]);
        assert_eq!(s.get(3, 1), [255, 255, 255]);
        // A quarter of the way from red to white.
        assert_eq!(s.get(1, 0)[1], 64);
        // Round trip through the cached PPM.
        let mut ppm = format!("P6\n{} {}\n255\n", s.w, s.h).into_bytes();
        ppm.extend_from_slice(&s.px);
        assert_eq!(Rgb::parse_ppm(&ppm).unwrap(), s);
    }

    #[test]
    fn lab_of_reference_colours() {
        let w = lab([255, 255, 255]);
        assert!((w[0] - 100.0).abs() < 0.1 && w[1].abs() < 0.1 && w[2].abs() < 0.1);
        let r = lab([255, 0, 0]);
        // Reference: L 53.24, a 80.09, b 67.20.
        assert!(
            (r[0] - 53.24).abs() < 0.2 && (r[1] - 80.09).abs() < 0.3 && (r[2] - 67.2).abs() < 0.3
        );
    }
}
