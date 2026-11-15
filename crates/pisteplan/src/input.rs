//! Input step: the PDF through the shared downloader, its rendering (`pdftoppm`) and its
//! text layer (`pdftotext -bbox-layout`), both poppler tools called as subprocesses.
//! Everything after this step is Rust. The rendering stays in the cache
//! (`data/cache/pisteplan/`): the plan belongs to the resort and is never written to the
//! outputs.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use serde::Serialize;
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
#[derive(Debug, Clone, PartialEq, Serialize, serde::Deserialize)]
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
    Ok(dl.fetch(&Request::get(&spec.source, &spec.url).max_age(PDF_MAX_AGE))?)
}

/// Downloads, renders and reads the text layer of a plan (cached by the PDF's SHA-256).
pub fn load(spec: &PlanSpec, layout: &DataLayout, dl: &Downloader) -> Result<PlanInput> {
    load_fetched(fetch(spec, dl)?, spec.dpi, layout)
}

/// Renders (at `dpi`) and reads the text layer of a downloaded plan (cached by the PDF's
/// SHA-256).
pub fn load_fetched(fetched: Fetched, dpi: u32, layout: &DataLayout) -> Result<PlanInput> {
    let dir = layout.cache_dir(CACHE)?;
    DataLayout::ensure(&dir)?;
    let stem = format!("{}-{}", &fetched.meta.sha256[..16], dpi);
    let ppm = dir.join(format!("{stem}.ppm"));
    if !ppm.exists() {
        render(&fetched.path, dpi, &ppm)?;
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
        pt_per_px: pt_per_px(dpi),
        text,
    })
}

/// An image drawn on page 1 of a PDF (`pdfimages -list`).
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct PdfImage {
    /// Width, pixels.
    pub w: u32,
    /// Height, pixels.
    pub h: u32,
    /// Resolution on the page, pixels per inch (horizontal).
    pub ppi: f64,
}

/// What a PDF holds, before any reading: pages, page size, images and text of page 1.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Probe {
    /// Number of pages.
    pub pages: usize,
    /// Size of page 1, points.
    pub page_pt: [f64; 2],
    /// Images of page 1 (masks excluded).
    pub images: Vec<PdfImage>,
    /// Words of the text layer of page 1.
    pub words: usize,
    /// Creation date (`pdfinfo`), as printed.
    pub created: Option<String>,
    /// Producer software (`pdfinfo` `Creator`, else `Producer`).
    pub creator: Option<String>,
}

impl Probe {
    /// The largest image of page 1, if it covers at least 80 % of the page width.
    pub fn page_image(&self) -> Option<PdfImage> {
        self.images
            .iter()
            .copied()
            .filter(|i| i.ppi > 0.0 && i.w as f64 / i.ppi * 72.0 >= 0.8 * self.page_pt[0])
            .max_by_key(|i| i.w as u64 * i.h as u64)
    }

    /// A picture saved as PDF: almost no text, one image covering the page.
    pub fn picture_only(&self) -> bool {
        self.words < 20 && self.page_image().is_some()
    }
}

/// Parses `pdfinfo` output: pages, page size, creation date, creator.
pub fn parse_pdfinfo(s: &str) -> (usize, [f64; 2], Option<String>, Option<String>) {
    let field = |k: &str| {
        s.lines()
            .find_map(|l| l.strip_prefix(k))
            .map(|v| v.trim().to_owned())
            .filter(|v| !v.is_empty())
    };
    let pages = field("Pages:").and_then(|v| v.parse().ok()).unwrap_or(0);
    let size = field("Page size:")
        .and_then(|v| {
            let t: Vec<&str> = v.split_whitespace().collect();
            Some([t.first()?.parse().ok()?, t.get(2)?.parse().ok()?])
        })
        .unwrap_or([0.0, 0.0]);
    let creator = field("Creator:").or_else(|| field("Producer:"));
    (pages, size, field("CreationDate:"), creator)
}

/// Parses `pdfimages -list` output (images only, soft masks skipped).
pub fn parse_pdfimages(s: &str) -> Vec<PdfImage> {
    s.lines()
        .skip(2)
        .filter_map(|l| {
            let t: Vec<&str> = l.split_whitespace().collect();
            if t.get(2) != Some(&"image") {
                return None;
            }
            Some(PdfImage {
                w: t.get(3)?.parse().ok()?,
                h: t.get(4)?.parse().ok()?,
                ppi: t.get(12)?.parse().ok()?,
            })
        })
        .collect()
}

/// Probes a PDF (`pdfinfo`, `pdfimages -list` on page 1) with its text layer.
pub fn probe(pdf: &Path, text: &TextLayer) -> Result<Probe> {
    let info = run_capture("pdfinfo", &[pdf.as_os_str()])?;
    let (pages, page_pt, created, creator) = parse_pdfinfo(&info);
    let list = run_capture(
        "pdfimages",
        &[
            "-list".as_ref(),
            "-f".as_ref(),
            "1".as_ref(),
            "-l".as_ref(),
            "1".as_ref(),
            pdf.as_os_str(),
        ],
    )?;
    Ok(Probe {
        pages,
        page_pt,
        images: parse_pdfimages(&list),
        words: text.lines.iter().map(|(_, w)| w.len()).sum(),
        created,
        creator,
    })
}

fn run_capture(tool: &str, args: &[&std::ffi::OsStr]) -> Result<String> {
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
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
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

/// Text layer of page 1 of a downloaded PDF (cached by its SHA-256).
pub fn text_layer(fetched: &Fetched, layout: &DataLayout) -> Result<TextLayer> {
    let dir = layout.cache_dir(CACHE)?;
    DataLayout::ensure(&dir)?;
    let txt = dir.join(format!("{}.bbox.html", &fetched.meta.sha256[..16]));
    if !txt.exists() {
        text_layer_file(&fetched.path, &txt)?;
    }
    let html = std::fs::read_to_string(&txt).map_err(|e| Error::io(&txt, e))?;
    Ok(parse_bbox_layout(&html))
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

    #[test]
    fn parses_pdfinfo_and_pdfimages() {
        let info = "Creator:         Canva\nProducer:        Canva\nCreationDate:    Wed Mar 27 10:12:01 2024 CET\nPages:           1\nPage size:       595.5 x 595.5 pts\n";
        let (pages, size, created, creator) = parse_pdfinfo(info);
        assert_eq!(pages, 1);
        assert_eq!(size, [595.5, 595.5]);
        assert_eq!(created.as_deref(), Some("Wed Mar 27 10:12:01 2024 CET"));
        assert_eq!(creator.as_deref(), Some("Canva"));
        let list = "page   num  type   width height color comp bpc  enc interp  object ID x-ppi y-ppi size ratio\n--------------------------------------------------------------------------------------------\n   1     0 image    1280  1255  rgb     3   8  image  no        11  0   155   155 2176K  46%\n   1     2 smask      69    79  gray    1   8  jpeg   no       671  0   146   145 1379B  25%\n";
        let imgs = parse_pdfimages(list);
        assert_eq!(
            imgs,
            vec![PdfImage {
                w: 1280,
                h: 1255,
                ppi: 155.0
            }]
        );
    }
}
