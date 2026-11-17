//! OCR of the plan with ocrs (pure Rust, rten runtime). The two models (detection and
//! recognition, ONNX) are downloaded once through the shared downloader (source
//! `ocrs-models`, cached a year). The ocrs alphabet has no accents: names are compared
//! without accents anyway ([`crate::labels::fold`]).

use std::time::Duration;

use ocrs::{ImageSource, OcrEngine, OcrEngineParams, TextItem};
use rten::Model;
use skimap_common::download::{Downloader, Request};

use crate::error::{Error, Result};
use crate::image::Rgb;
use crate::input::TextBox;

/// Detection model.
pub const DETECTION_URL: &str =
    "https://ocrs-models.s3-accelerate.amazonaws.com/text-detection.onnx";
/// Recognition model.
pub const RECOGNITION_URL: &str =
    "https://ocrs-models.s3-accelerate.amazonaws.com/text-recognition.onnx";
/// Downloader source of the models.
pub const SOURCE: &str = "ocrs-models";

/// Loads the OCR engine (models from the cache, downloaded on first use).
pub fn engine(dl: &Downloader) -> Result<OcrEngine> {
    let year = Duration::from_secs(365 * 86_400);
    let load = |url: &str| -> Result<Model> {
        let f = dl.fetch(&Request::get(SOURCE, url).max_age(year))?;
        // The cached body has no `.onnx` extension: the format is detected from the bytes.
        Model::load(f.bytes()?).map_err(|e| Error::Data(format!("OCR model {url}: {e}")))
    };
    OcrEngine::new(OcrEngineParams {
        detection_model: Some(load(DETECTION_URL)?),
        recognition_model: Some(load(RECOGNITION_URL)?),
        ..Default::default()
    })
    .map_err(|e| Error::Data(format!("OCR engine: {e}")))
}

/// Reads the text lines of an image region `[x0, y0, x1, y1]` (pixels). Returns lines
/// with their bounding box in page points.
pub fn read_lines(
    engine: &OcrEngine,
    img: &Rgb,
    region: [usize; 4],
    pt_per_px: f64,
) -> Result<Vec<TextBox>> {
    let [x0, y0, x1, y1] = region;
    let (x1, y1) = (x1.min(img.w), y1.min(img.h));
    if x1 <= x0 || y1 <= y0 {
        return Ok(Vec::new());
    }
    let (w, h) = (x1 - x0, y1 - y0);
    let mut buf = Vec::with_capacity(w * h * 3);
    for y in y0..y1 {
        let s = (y * img.w + x0) * 3;
        buf.extend_from_slice(&img.px[s..s + w * 3]);
    }
    let src = ImageSource::from_bytes(&buf, (w as u32, h as u32))
        .map_err(|e| Error::Data(format!("OCR input: {e}")))?;
    let input = engine
        .prepare_input(src)
        .map_err(|e| Error::Data(format!("OCR input: {e}")))?;
    let words = engine
        .detect_words(&input)
        .map_err(|e| Error::Data(format!("OCR detection: {e}")))?;
    let lines = engine.find_text_lines(&input, &words);
    let texts = engine
        .recognize_text(&input, &lines)
        .map_err(|e| Error::Data(format!("OCR recognition: {e}")))?;
    let mut out = Vec::new();
    for line in texts.into_iter().flatten() {
        let text = line.to_string().trim().to_owned();
        if text.chars().filter(|c| c.is_alphanumeric()).count() < 2 {
            continue;
        }
        let r = line.bounding_rect();
        out.push(TextBox {
            text,
            rect: [
                (x0 as f64 + r.left() as f64) * pt_per_px,
                (y0 as f64 + r.top() as f64) * pt_per_px,
                (x0 as f64 + r.right() as f64) * pt_per_px,
                (y0 as f64 + r.bottom() as f64) * pt_per_px,
            ],
        });
    }
    Ok(out)
}

/// Text enhancement for picture plans: black top-hat of the luminance (closing minus
/// image, square window `2r+1`): thin dark strokes (letters) stand out, large dark areas
/// (forests, shading) vanish. Returns a white image with dark text.
pub fn enhance_text(img: &Rgb, r: usize, gain: f32) -> Rgb {
    let (w, h) = (img.w, img.h);
    let lum: Vec<f32> = img
        .px
        .as_chunks::<3>()
        .0
        .iter()
        .map(|c| 0.299 * c[0] as f32 + 0.587 * c[1] as f32 + 0.114 * c[2] as f32)
        .collect();
    let filt = |src: &[f32], horizontal: bool, max: bool| -> Vec<f32> {
        let mut out = vec![0.0f32; w * h];
        for y in 0..h {
            for x in 0..w {
                let mut acc = if max { f32::MIN } else { f32::MAX };
                for d in -(r as i64)..=(r as i64) {
                    let (xx, yy) = if horizontal {
                        (x as i64 + d, y as i64)
                    } else {
                        (x as i64, y as i64 + d)
                    };
                    if xx < 0 || yy < 0 || xx as usize >= w || yy as usize >= h {
                        continue;
                    }
                    let v = src[yy as usize * w + xx as usize];
                    acc = if max { acc.max(v) } else { acc.min(v) };
                }
                out[y * w + x] = acc;
            }
        }
        out
    };
    let dil = filt(&filt(&lum, true, true), false, true);
    let close = filt(&filt(&dil, true, false), false, false);
    let mut out = Rgb::new(w, h, [255, 255, 255]);
    for i in 0..w * h {
        let t = ((close[i] - lum[i]) * gain).clamp(0.0, 255.0);
        let v = (255.0 - t) as u8;
        out.px[i * 3..i * 3 + 3].copy_from_slice(&[v, v, v]);
    }
    out
}

/// OCR over overlapping tiles of `tile` pixels (a large page holds more small labels than
/// the detector resolves at once); a line found twice (centres closer than half its size)
/// is kept once, the longer reading winning (the other was cut by a tile edge).
pub fn read_lines_tiled(
    engine: &OcrEngine,
    img: &Rgb,
    tile: usize,
    overlap: usize,
    pt_per_px: f64,
) -> Result<Vec<TextBox>> {
    use rayon::prelude::*;
    let step = tile.saturating_sub(overlap).max(1);
    let starts = |len: usize| {
        let mut v = vec![0];
        while v[v.len() - 1] + tile < len {
            let next = v[v.len() - 1] + step;
            v.push(next);
        }
        v
    };
    // Tiles in row-major order, read in parallel (the engine is shared read-only), then
    // merged in that order: the result does not depend on the scheduling.
    let tiles: Vec<(usize, usize)> = starts(img.h)
        .into_iter()
        .flat_map(|y| starts(img.w).into_iter().map(move |x| (x, y)))
        .collect();
    let read: Vec<Vec<TextBox>> = tiles
        .par_iter()
        .map(|&(x, y)| read_lines(engine, img, [x, y, x + tile, y + tile], pt_per_px))
        .collect::<Result<_>>()?;
    let mut out: Vec<TextBox> = Vec::new();
    for l in read.into_iter().flatten() {
        let c = l.center();
        let hgt = (l.rect[3] - l.rect[1]).max(1.0);
        let dup = out.iter().position(|o| {
            let d = o.center();
            (d[0] - c[0]).abs() < (o.rect[2] - o.rect[0]).max(hgt) / 2.0
                && (d[1] - c[1]).abs() < hgt / 2.0
        });
        match dup {
            Some(i) if out[i].text.len() < l.text.len() => out[i] = l,
            Some(_) => {}
            None => out.push(l),
        }
    }
    Ok(out)
}

/// [`read_lines_tiled`] cached as JSON in `path` (the OCR of a large page takes a minute;
/// the reading only depends on the image, the models and the tiling, all in the key the
/// caller puts in the file name).
pub fn read_lines_cached(
    engine: &OcrEngine,
    img: &Rgb,
    tile: usize,
    overlap: usize,
    pt_per_px: f64,
    path: &std::path::Path,
) -> Result<Vec<TextBox>> {
    if let Ok(bytes) = std::fs::read(path)
        && let Ok(v) = serde_json::from_slice::<Vec<TextBox>>(&bytes)
    {
        return Ok(v);
    }
    let v = read_lines_tiled(engine, img, tile, overlap, pt_per_px)?;
    let json = serde_json::to_vec(&v).map_err(|e| Error::Json {
        context: "OCR cache".into(),
        source: e,
    })?;
    skimap_common::output::write_bytes(path, &json)?;
    Ok(v)
}
