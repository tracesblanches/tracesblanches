//! Reading sheet of a piste plan: where it is published, which part of the page is the
//! map, the inks of the pistes (from the plan's legend) and the geographic extent drawn.
//! [`crate::autospec`] detects it from the plan alone: no sheet is written by hand, no
//! site has a reading of its own.
//!
//! Coordinates on the page are PDF points (1/72 in), origin top left, y down: the frame
//! of `pdftotext -bbox`, independent of the rendering resolution.

use serde::Serialize;
use skimap_common::geom::BBox;

use crate::segment::LineParams;
use crate::transcript::PisteKind;

/// A rectangle on the page, PDF points: `[x0, y0, x1, y1]`.
pub type PageRect = [f64; 4];

/// One ink of the plan (a colour of the legend).
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Ink {
    /// Name of the ink (`green`, `blue`, `red`, `black`, `orange`, `yellow`, `purple`).
    pub name: String,
    /// Reference colours, sampled on the drawn lines of the plan (a thin line takes the
    /// tint of the background: one reference per background when needed).
    pub refs: Vec<[u8; 3]>,
    /// Largest CIELAB distance to the reference for a pixel of this ink.
    pub max_de: f32,
    /// Difficulty colour meant by the ink (French scale: green, blue, red, black), if any.
    pub difficulty: Option<String>,
    /// What the legend says lines of this ink are.
    pub kind: PisteKind,
}

/// Where the labels of the plan come from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum LabelSource {
    /// Text layer of the PDF (`pdftotext -bbox-layout`); OCR is still run to measure it.
    TextLayer,
    /// The plan is a picture: labels are read by OCR (ocrs).
    Ocr,
}

/// Where a plan is published (its catalogue entry, [`crate::collect::plan_source`]).
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct PlanSource {
    /// Site identifier (`common::site`), or census domain identifier.
    pub site: String,
    /// Plan identifier.
    pub id: String,
    /// Title of the plan.
    pub title: String,
    /// Publisher (owner of the plan).
    pub publisher: String,
    /// URL of the PDF.
    pub url: String,
    /// Page linking the PDF.
    pub page_url: String,
    /// Downloader source (cache directory, rate limits).
    pub source: String,
    /// Edition read on the plan or in the PDF metadata.
    pub edition: String,
    /// Rights displayed on the plan.
    pub rights: String,
}

/// Reading sheet of an official piste plan.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct PlanSpec {
    /// Site identifier (`common::site`), or census domain identifier.
    pub site: String,
    /// Plan identifier.
    pub id: String,
    /// Title of the plan.
    pub title: String,
    /// Publisher (owner of the plan).
    pub publisher: String,
    /// URL of the PDF.
    pub url: String,
    /// Page linking the PDF.
    pub page_url: String,
    /// Downloader source (cache directory, rate limits).
    pub source: String,
    /// Edition read on the plan or in the PDF metadata.
    pub edition: String,
    /// Rights displayed on the plan.
    pub rights: String,
    /// Rendering resolution, dots per inch.
    pub dpi: u32,
    /// The map on the page (legend panels, title and advertisements outside).
    pub map_area: PageRect,
    /// Parts of the map area that are not map (insets, logos, advertisements, boxes).
    pub masks: Vec<PageRect>,
    /// Geographic extent drawn on the plan, with a margin (OSM queries).
    pub extent: BBox,
    /// Inks of the piste lines.
    pub inks: Vec<Ink>,
    /// Source of the labels.
    pub labels: LabelSource,
    /// Line extraction parameters (pixels of the rendering).
    pub line: LineParams,
    /// Whether the plan draws direction arrows on the pistes.
    pub arrows: bool,
    /// Sectors of the legend (columns listing the pistes of one area), page points.
    pub sectors: Vec<(PageRect, String)>,
}

/// Points per pixel of a rendering at `dpi`.
pub fn pt_per_px(dpi: u32) -> f64 {
    72.0 / dpi as f64
}

impl PlanSpec {
    /// A sheet from a source and the reading parameters.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        src: &PlanSource,
        dpi: u32,
        map_area: PageRect,
        masks: Vec<PageRect>,
        extent: BBox,
        inks: Vec<Ink>,
        labels: LabelSource,
        line: LineParams,
        arrows: bool,
        sectors: Vec<(PageRect, String)>,
    ) -> PlanSpec {
        PlanSpec {
            site: src.site.clone(),
            id: src.id.clone(),
            title: src.title.clone(),
            publisher: src.publisher.clone(),
            url: src.url.clone(),
            page_url: src.page_url.clone(),
            source: src.source.clone(),
            edition: src.edition.clone(),
            rights: src.rights.clone(),
            dpi,
            map_area,
            masks,
            extent,
            inks,
            labels,
            line,
            arrows,
            sectors,
        }
    }

    /// The source part of the sheet.
    pub fn plan_source(&self) -> PlanSource {
        PlanSource {
            site: self.site.clone(),
            id: self.id.clone(),
            title: self.title.clone(),
            publisher: self.publisher.clone(),
            url: self.url.clone(),
            page_url: self.page_url.clone(),
            source: self.source.clone(),
            edition: self.edition.clone(),
            rights: self.rights.clone(),
        }
    }
}

impl PlanSpec {
    /// Whether a page point is on the map (inside the map area, outside every mask).
    pub fn on_map(&self, x: f64, y: f64) -> bool {
        let inside = |r: &PageRect| x >= r[0] && x < r[2] && y >= r[1] && y < r[3];
        inside(&self.map_area) && !self.masks.iter().any(inside)
    }
}
