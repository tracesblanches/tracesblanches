//! Reading sheet of a piste plan: where it is published, which part of the page is the
//! map, the inks of the pistes (from the plan's legend) and the geographic extent drawn.
//! The sheets of the two sites were written by hand ([`cervieres`], [`chapelle_des_bois`]);
//! [`crate::autospec`] detects them from the plan alone.
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

impl Ink {
    fn manual(
        name: &str,
        refs: &[[u8; 3]],
        max_de: f32,
        difficulty: Option<&str>,
        kind: PisteKind,
    ) -> Ink {
        Ink {
            name: name.to_owned(),
            refs: refs.to_vec(),
            max_de,
            difficulty: difficulty.map(str::to_owned),
            kind,
        }
    }
}

/// Where the labels of the plan come from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum LabelSource {
    /// Text layer of the PDF (`pdftotext -bbox-layout`); OCR is still run to measure it.
    TextLayer,
    /// The plan is a picture: labels are read by OCR (ocrs).
    Ocr,
}

/// Where a plan is published (the catalogue entry of the collector, or the hand-written
/// sheets of the two sites).
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

/// Converts a rectangle given in pixels of a rendering at `dpi` to page points.
const fn px(r: [f64; 4], dpi: f64) -> PageRect {
    let k = 72.0 / dpi;
    [r[0] * k, r[1] * k, r[2] * k, r[3] * k]
}

/// Cervières: « Plan des pistes - Cervières », Canva, 27/03/2024, linked as the current
/// plan by Nordic Alpes du Sud (page of the domain, season 2025-2026).
pub fn cervieres_source() -> PlanSource {
    PlanSource {
        site: "cervieres".into(),
        id: "cervieres-izoard-2024".into(),
        title: "Plan des pistes - Cervières (dépliant « L'Izoard, hiver »)".into(),
        publisher: "Commune de Cervières / Office de tourisme des Hautes Vallées (via Nordic France)".into(),
        url: "https://www.nordicfrance.fr/content/uploads/2018/12/Plan-des-pistes-Cervieres.pdf".into(),
        page_url: "https://www.nordicalpesdusud.com/domaines/cervieres".into(),
        source: "nordicfrance".into(),
        edition: "PDF créé le 27/03/2024 (Canva, « Plan des pistes - Cervières (210 x 210 mm) »)".into(),
        rights: "aucune licence affichée ; crédits photo et « Conception graphique : Emilie Chenier » sur le plan : propriété de la station, usage personnel".into(),
    }
}

/// Chapelle-des-Bois: « Plan des pistes ski de fond, Lacs et Montagnes du Haut-Doubs
/// (Sud) : Mouthe - Chez Liadet, Le Pré Poncet, Chapelle-des-Bois », Espace Nordique
/// Jurassien, décembre 2025 (PDF du 17/02/2026).
pub fn chapelle_des_bois_source() -> PlanSource {
    PlanSource {
        site: "chapelle-des-bois".into(),
        id: "enj-val-de-mouthe-ski-2025".into(),
        title: "Plan des pistes ski de fond - Lacs et Montagnes du Haut-Doubs (Sud)".into(),
        publisher: "Espace Nordique Jurassien / Communauté de communes des Lacs et Montagnes du Haut-Doubs".into(),
        url: "https://www.espacenordiquejurassien.com/csx/scripts/downloader2.php?filename=ENJ_MEDIAS/media/3d/7a/n2kw275c0y&mime=application%2Fpdf&originalname=ENJ_Plans_CCLMHD_VALdeMOUTHE_2511_WEB_ski.pdf&moid=130".into(),
        page_url: "https://www.espacenordiquejurassien.com/haut-doubs.html".into(),
        source: "enj".into(),
        edition: "décembre 2025 (mention du plan), PDF du 17/02/2026 (InDesign)".into(),
        rights: "aucune licence affichée ; « Cartographie : Latitude Cartagène », « © les contributeurs d'OpenStreetMap », « © Gdem Aster pour le relief » sur le plan : propriété de l'ENJ, usage personnel".into(),
    }
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

/// Hand-written sheet of Cervières.
pub fn cervieres() -> PlanSpec {
    PlanSpec::new(
        &cervieres_source(),
        300,
        // Map: left 83 % and top 66 % of the page (rendering at 150 dpi: 1030 × 825 px).
        px([0.0, 0.0, 1030.0, 825.0], 150.0),
        vec![
            px([30.0, 30.0, 215.0, 135.0], 150.0), // « L'Izoard Hiver » logo
            px([840.0, 30.0, 1035.0, 310.0], 150.0), // legend
            px([725.0, 30.0, 840.0, 135.0], 150.0), // compass
            px([826.0, 415.0, 1035.0, 830.0], 150.0), // « Cervières, station village »
            px([0.0, 215.0, 85.0, 265.0], 150.0),  // « Briançon » arrow
        ],
        BBox::new(44.73, 6.58, 44.97, 6.93),
        vec![
            Ink::manual(
                "green",
                &[[163, 204, 93]],
                12.0,
                Some("green"),
                PisteKind::Ski,
            ),
            Ink::manual("blue", &[[95, 190, 225]], 8.0, Some("blue"), PisteKind::Ski),
            Ink::manual("red", &[[140, 45, 52]], 20.0, Some("red"), PisteKind::Ski),
            Ink::manual(
                "black",
                &[[31, 22, 25], [10, 42, 48]],
                14.0,
                Some("black"),
                PisteKind::Ski,
            ),
            Ink::manual("purple", &[[104, 72, 92]], 10.0, None, PisteKind::Ski),
            Ink::manual("orange", &[[205, 172, 76]], 12.0, None, PisteKind::Liaison),
            Ink::manual(
                "yellow",
                &[[222, 221, 97]],
                12.0,
                None,
                PisteKind::Pedestrian,
            ),
        ],
        LabelSource::Ocr,
        LineParams {
            close_r: 1,
            min_len_px: 60.0,
            max_width_px: 14.0,
            min_width_px: 2.0,
            max_hole_px: 40,
            spur_px: 10.0,
        },
        false,
        vec![
            (
                px([620.0, 825.0, 828.0, 1010.0], 150.0),
                "Secteur le Laus / l'Izoard".to_owned(),
            ),
            (
                px([828.0, 825.0, 1035.0, 1010.0], 150.0),
                "Secteur Plaine du Bourget / Vallée des Fonts".to_owned(),
            ),
        ],
    )
}

/// Hand-written sheet of Chapelle-des-Bois.
pub fn chapelle_des_bois() -> PlanSpec {
    PlanSpec::new(
        &chapelle_des_bois_source(),
        200,
        // Map: right of the left panel, left of the advertisement column (100 dpi pixels).
        px([395.0, 0.0, 1700.0, 1654.0], 100.0),
        vec![
            px([395.0, 0.0, 672.0, 128.0], 100.0),       // title
            px([410.0, 165.0, 768.0, 342.0], 100.0),     // « Environnement » box
            px([1330.0, 1160.0, 1700.0, 1282.0], 100.0), // Montnoir advertisement
            px([1198.0, 1305.0, 1700.0, 1450.0], 100.0), // Mouthe / photo advertisements
            px([1385.0, 1020.0, 1700.0, 1160.0], 100.0), // Chez Liadet advertisement
            px([1488.0, 892.0, 1700.0, 1022.0], 100.0),  // Auberge du grand git
            px([1198.0, 1452.0, 1700.0, 1654.0], 100.0), // Fromagerie, Transju
            px([1290.0, 1080.0, 1385.0, 1160.0], 100.0), // Swiss flag
            px([1495.0, 780.0, 1590.0, 820.0], 100.0),   // French flag
            px([400.0, 880.0, 520.0, 990.0], 100.0),     // north arrow and scale
        ],
        BBox::new(46.50, 5.88, 46.76, 6.27),
        vec![
            Ink::manual(
                "green",
                &[[57, 164, 69]],
                18.0,
                Some("green"),
                PisteKind::Ski,
            ),
            Ink::manual("blue", &[[4, 131, 196]], 18.0, Some("blue"), PisteKind::Ski),
            Ink::manual("red", &[[226, 6, 28]], 22.0, Some("red"), PisteKind::Ski),
            Ink::manual("black", &[[3, 3, 3]], 14.0, Some("black"), PisteKind::Ski),
            Ink::manual("orange", &[[238, 115, 44]], 16.0, None, PisteKind::Liaison),
            Ink::manual(
                "yellow",
                &[[255, 237, 0]],
                18.0,
                None,
                PisteKind::MultiActivity,
            ),
        ],
        LabelSource::TextLayer,
        LineParams {
            close_r: 2,
            min_len_px: 80.0,
            max_width_px: 16.0,
            min_width_px: 3.2,
            max_hole_px: 160,
            spur_px: 12.0,
        },
        true,
        Vec::new(),
    )
}

/// Hand-written sheets of the two sites.
pub fn manual_plans() -> Vec<PlanSpec> {
    vec![cervieres(), chapelle_des_bois()]
}

/// Hand-written sheet of a site.
pub fn plan_for(site: &str) -> Option<PlanSpec> {
    manual_plans().into_iter().find(|p| p.site == site)
}

impl PlanSpec {
    /// Whether a page point is on the map (inside the map area, outside every mask).
    pub fn on_map(&self, x: f64, y: f64) -> bool {
        let inside = |r: &PageRect| x >= r[0] && x < r[2] && y >= r[1] && y < r[3];
        inside(&self.map_area) && !self.masks.iter().any(inside)
    }
}
