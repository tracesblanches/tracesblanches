//! Input step: the PDF through the shared downloader, its rendering (`pdftoppm`) and its
//! text layer (`pdftotext -bbox-layout`), both poppler tools called as subprocesses.
//! Everything after this step is Rust. The rendering stays in the cache
//! (`data/cache/pisteplan/`): the plan belongs to the resort and is never written to the
//! outputs.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use skimap_common::download::{Downloader, Fetched, Request};
use skimap_common::layout::DataLayout;

use crate::error::{Error, Result};
use crate::image::Rgb;
use crate::spec::{PlanSpec, pt_per_px};

/// Cache directory of the renderings and text layers.
pub const CACHE: &str = "pisteplan";

/// Lifetime of the cached PDF (the servers send neither `ETag` nor `Last-Modified`).
pub const PDF_MAX_AGE: Duration = Duration::from_secs(30 * 86_400);

/// A word or line of the text layer, page points.
#[derive(Debug, Clone, PartialEq)]
pub struct TextBox {
    /// Text.
    pub text: String,
    /// `[x0, y0, x1, y1]`, page points.
    pub rect: [f64; 4],
}

impl TextBox {
    /// Centre of the box.
    pub fn center(&self) -> [f64; 2] {
        [
            (self.rect[0] + self.rect[2]) / 2.0,
            (self.rect[1] + self.rect[3]) / 2.0,
        ]
    }
}

/// Text layer of a page: lines (with their words).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct TextLayer {
    /// Page size, points.
    pub page: [f64; 2],
    /// Lines, each with its words.
    pub lines: Vec<(TextBox, Vec<TextBox>)>,
}

/// The plan ready for analysis.
pub struct PlanInput {
    /// Downloaded PDF.
    pub fetched: Fetched,
    /// Rendering of page 1 at the plan's resolution.
    pub image: Rgb,
    /// Page points per pixel of [`PlanInput::image`].
    pub pt_per_px: f64,
    /// Text layer (empty for a picture-only PDF).
    pub text: TextLayer,
}

/// Downloads the PDF of a plan.
pub fn fetch(spec: &PlanSpec, dl: &Downloader) -> Result<Fetched> {
    Ok(dl.fetch(&Request::get(spec.source, spec.url).max_age(PDF_MAX_AGE))?)
}

/// Downloads, renders and reads the text layer of a plan (cached by the PDF's SHA-256).
pub fn load(spec: &PlanSpec, layout: &DataLayout, dl: &Downloader) -> Result<PlanInput> {
    let fetched = fetch(spec, dl)?;
    let dir = layout.cache_dir(CACHE)?;
    DataLayout::ensure(&dir)?;
    let stem = format!("{}-{}", &fetched.meta.sha256[..16], spec.dpi);
    let ppm = dir.join(format!("{stem}.ppm"));
    if !ppm.exists() {
        render(&fetched.path, spec.dpi, &ppm)?;
    }
    let image = Rgb::read_ppm(&ppm)?;
    let txt = dir.join(format!("{}.bbox.html", &fetched.meta.sha256[..16]));
    if !txt.exists() {
        text_layer_file(&fetched.path, &txt)?;
    }
    let html = std::fs::read_to_string(&txt).map_err(|e| Error::io(&txt, e))?;
    let text = parse_bbox_layout(&html);
    Ok(PlanInput {
        fetched,
        image,
        pt_per_px: pt_per_px(spec.dpi),
        text,
    })
}

fn run(tool: &str, args: &[&std::ffi::OsStr]) -> Result<()> {
    let out = Command::new(tool)
        .args(args)
        .output()
        .map_err(|e| Error::Tool {
            tool: tool.to_owned(),
            message: format!("{e} (poppler-utils installed?)"),
        })?;
    if !out.status.success() {
        return Err(Error::Tool {
            tool: tool.to_owned(),
            message: format!(
                "{}: {}",
                out.status,
                String::from_utf8_lossy(&out.stderr).trim()
            ),
        });
    }
    Ok(())
}

/// Renders page 1 of `pdf` to a binary PPM (`pdftoppm -r dpi -singlefile`).
pub fn render(pdf: &Path, dpi: u32, out: &Path) -> Result<()> {
    let tmp: PathBuf = out.with_extension("part");
    let dpi_s = dpi.to_string();
    run(
        "pdftoppm",
        &[
            "-r".as_ref(),
            dpi_s.as_ref(),
            "-f".as_ref(),
            "1".as_ref(),
            "-l".as_ref(),
            "1".as_ref(),
            "-singlefile".as_ref(),
            pdf.as_os_str(),
            tmp.as_os_str(),
        ],
    )?;
    // pdftoppm appends `.ppm` to the prefix.
    let written = PathBuf::from(format!("{}.ppm", tmp.display()));
    std::fs::rename(&written, out).map_err(|e| Error::io(out, e))
}

/// Writes the text layer of page 1 (`pdftotext -bbox-layout`, XHTML).
pub fn text_layer_file(pdf: &Path, out: &Path) -> Result<()> {
    let tmp = out.with_extension("part");
    run(
        "pdftotext",
        &[
            "-bbox-layout".as_ref(),
            "-f".as_ref(),
            "1".as_ref(),
            "-l".as_ref(),
            "1".as_ref(),
            pdf.as_os_str(),
            tmp.as_os_str(),
        ],
    )?;
    std::fs::rename(&tmp, out).map_err(|e| Error::io(out, e))
}

fn attr(tag: &str, name: &str) -> Option<f64> {
    let key = format!("{name}=\"");
    let start = tag.find(&key)? + key.len();
    let end = tag[start..].find('"')? + start;
    tag[start..end].parse().ok()
}

fn unescape(s: &str) -> String {
    s.replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&apos;", "'")
        .replace("&#39;", "'")
        .replace("&amp;", "&")
}

fn rect_of(tag: &str) -> Option<[f64; 4]> {
    Some([
        attr(tag, "xMin")?,
        attr(tag, "yMin")?,
        attr(tag, "xMax")?,
        attr(tag, "yMax")?,
    ])
}

/// Parses the XHTML of `pdftotext -bbox-layout` (first page).
pub fn parse_bbox_layout(html: &str) -> TextLayer {
    let mut layer = TextLayer::default();
    if let Some(i) = html.find("<page ") {
        let tag = &html[i..i + html[i..].find('>').unwrap_or(0)];
        layer.page = [
            attr(tag, "width").unwrap_or(0.0),
            attr(tag, "height").unwrap_or(0.0),
        ];
    }
    let mut rest = html;
    while let Some(i) = rest.find("<line ") {
        rest = &rest[i..];
        let Some(end) = rest.find("</line>") else {
            break;
        };
        let block = &rest[..end];
        rest = &rest[end + 7..];
        let Some(line_rect) = rect_of(&block[..block.find('>').unwrap_or(0)]) else {
            continue;
        };
        let mut words = Vec::new();
        let mut w = block;
        while let Some(j) = w.find("<word ") {
            w = &w[j..];
            let Some(gt) = w.find('>') else { break };
            let Some(close) = w.find("</word>") else {
                break;
            };
            if let Some(r) = rect_of(&w[..gt]) {
                words.push(TextBox {
                    text: unescape(&w[gt + 1..close]),
                    rect: r,
                });
            }
            w = &w[close + 7..];
        }
        if words.is_empty() {
            continue;
        }
        let text = words
            .iter()
            .map(|w| w.text.as_str())
            .collect::<Vec<_>>()
            .join(" ");
        layer.lines.push((
            TextBox {
                text,
                rect: line_rect,
            },
            words,
        ));
    }
    layer
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_bbox_layout() {
        let html = r#"<doc><page width="1417.32" height="1190.55"><flow><block xMin="1" yMin="2" xMax="3" yMax="4">
<line xMin="831.1" yMin="908.5" xMax="870.1" yMax="917.7">
<word xMin="831.1" yMin="908.5" xMax="849.1" yMax="917.7">Greffier</word>
<word xMin="850.4" yMin="908.5" xMax="861.2" yMax="917.7">14,5</word>
<word xMin="862.4" yMin="908.5" xMax="870.1" yMax="917.7">km</word>
</line>
<line xMin="10" yMin="20" xMax="30" yMax="40"><word xMin="10" yMin="20" xMax="30" yMax="40">Ch&amp;co</word></line>
</block></flow></page></doc>"#;
        let t = parse_bbox_layout(html);
        assert_eq!(t.page, [1417.32, 1190.55]);
        assert_eq!(t.lines.len(), 2);
        assert_eq!(t.lines[0].0.text, "Greffier 14,5 km");
        assert_eq!(t.lines[0].1.len(), 3);
        assert_eq!(t.lines[1].0.text, "Ch&co");
        assert_eq!(t.lines[1].0.center(), [20.0, 30.0]);
    }
}
