//! Registration of a plan and transcription of its pistes onto our geometries.

use std::collections::BTreeSet;
use std::time::Instant;

use serde::Serialize;
use serde_json::{Value, json};
use skimap_common::download::{Downloader, Fetched};
use skimap_common::layout::DataLayout;
use skimap_common::meta::{FileMeta, LayerMeta, SourceRef};
use skimap_common::output;
use skimap_osm::extract::Planet;

use crate::align::{
    self, Coarse, IcpStep, LabelMatch, LooStats, Pair, Tie, Warp, compare_models, held_out,
};
use crate::error::{Error, Result};
use crate::input::{self, PlanInput, TextBox};
use crate::labels::{self, LegendEntry};
use crate::ocr;
use crate::osmref::{self, OsmRef};
use crate::segment;
use crate::skeleton::SkelGraph;
use crate::spec::{LabelSource, PlanSpec};
use crate::transcribe;
use crate::transcript::{GeometrySource, MatchMethod, PisteKind, PlanLandmark, PlanPiste};
use crate::warp::{ModelKind, quantile};

/// ICP schedule: (search radius m, spline smoothing).
pub const ICP_SCHEDULE: &[(f64, f64)] = &[
    (600.0, 1.0),
    (400.0, 0.3),
    (250.0, 0.1),
    (150.0, 0.03),
    (100.0, 0.01),
    (70.0, 0.01),
];

/// Grid cell of the ICP pairs, page points.
pub const ICP_CELL_PT: f64 = 10.0;

/// OCR tile and overlap, pixels.
pub const OCR_TILE: usize = 800;
/// OCR tile overlap, pixels.
pub const OCR_OVERLAP: usize = 200;
/// Top-hat window radius (pixels) and gain of the text enhancement of picture plans.
pub const OCR_TOPHAT_R: usize = 6;
/// Gain of the top-hat.
pub const OCR_TOPHAT_GAIN: f32 = 4.0;

/// RANSAC tolerance, page points (~5 mm on paper).
pub const RANSAC_TOL_PT: f64 = 15.0;

/// Largest distance between a drawn junction (after the warp) and its OSM junction, page
/// points (~1.4 mm on paper).
pub const TIE_MAX_PT: f64 = 4.0;

/// Everything learnt about a plan.
pub struct Registration {
    /// The plan.
    pub spec: &'static PlanSpec,
    /// Input (rendering, text layer).
    pub input: PlanInput,
    /// All text lines used (text layer or OCR), page points.
    pub lines: Vec<TextBox>,
    /// OCR lines (also on plans with a text layer, to measure the OCR).
    pub ocr_lines: Vec<TextBox>,
    /// Labels on the map.
    pub map_labels: Vec<TextBox>,
    /// Legend of the pistes.
    pub legend: Vec<LegendEntry>,
    /// Ink per pixel.
    pub classes: Vec<u8>,
    /// Skeleton graph per ink.
    pub graphs: Vec<(usize, SkelGraph)>,
    /// Skeleton graph of all inks together.
    pub union: SkelGraph,
    /// OSM reference.
    pub osm: OsmRef,
    /// Overpass answer of the landmarks.
    pub landmarks_fetched: Fetched,
    /// Label ↔ landmark candidates.
    pub candidates: Vec<LabelMatch>,
    /// RANSAC result.
    pub coarse: Coarse,
    /// Final warp.
    pub warp: Warp,
    /// ICP pairs of the final warp.
    pub pairs: Vec<Pair>,
    /// ICP iterations.
    pub steps: Vec<IcpStep>,
    /// Junction tie points.
    pub ties: Vec<Tie>,
    /// Leave-one-out comparison of models on the tie points.
    pub loo: Vec<LooStats>,
    /// Same on the label ↔ landmark pairs of the RANSAC (label offsets included).
    pub loo_labels: Vec<LooStats>,
    /// Held-out error of the final warp at the tie points, m.
    pub held_out_m: Vec<f64>,
    /// Timings, s.
    pub timings: Vec<(String, f64)>,
}

/// Summary statistics of a sample of errors.
#[derive(Debug, Clone, Serialize)]
pub struct ErrStats {
    /// Count.
    pub n: usize,
    /// Median, m.
    pub median_m: Option<f64>,
    /// 90th percentile, m.
    pub p90_m: Option<f64>,
}

impl ErrStats {
    /// Statistics of `v`.
    pub fn of(v: &[f64]) -> ErrStats {
        ErrStats {
            n: v.len(),
            median_m: quantile(v, 0.5),
            p90_m: quantile(v, 0.9),
        }
    }
}

/// Registers a plan on OSM.
pub fn register(
    spec: &'static PlanSpec,
    layout: &DataLayout,
    dl: &Downloader,
    planet: &Planet,
    ocr_engine: Option<&ocrs::OcrEngine>,
) -> Result<Registration> {
    let mut timings = Vec::new();
    let mut t = Instant::now();
    let mut lap = |name: &str, t: &mut Instant| {
        timings.push((name.to_owned(), t.elapsed().as_secs_f64()));
        *t = Instant::now();
    };
    let inp = input::load(spec, layout, dl)?;
    lap("input", &mut t);
    let k = inp.pt_per_px;
    // OCR in tiles of 800 px. Picture plans: the map is read on the text-enhanced image
    // (top-hat: letters over textured backgrounds), the legend panels (plain background)
    // on the image itself.
    let raw_ocr = match ocr_engine {
        Some(e) => ocr::read_lines_tiled(e, &inp.image, OCR_TILE, OCR_OVERLAP, k)?,
        None => Vec::new(),
    };
    let ocr_lines: Vec<TextBox> = match (ocr_engine, spec.labels) {
        (Some(e), LabelSource::Ocr) => {
            let enhanced = ocr::enhance_text(&inp.image, OCR_TOPHAT_R, OCR_TOPHAT_GAIN);
            let on_map = |l: &TextBox| {
                let c = l.center();
                spec.on_map(c[0], c[1])
            };
            ocr::read_lines_tiled(e, &enhanced, OCR_TILE, OCR_OVERLAP, k)?
                .into_iter()
                .filter(|l| on_map(l))
                .chain(raw_ocr.into_iter().filter(|l| !on_map(l)))
                .collect()
        }
        _ => raw_ocr,
    };
    lap("ocr", &mut t);
    let lines: Vec<TextBox> = match spec.labels {
        LabelSource::TextLayer => inp.text.lines.iter().map(|(l, _)| l.clone()).collect(),
        LabelSource::Ocr => ocr_lines.clone(),
    };
    let map_labels: Vec<TextBox> = lines
        .iter()
        .filter(|l| {
            let c = l.center();
            spec.on_map(c[0], c[1])
        })
        .cloned()
        .collect();
    let legend = match spec.labels {
        LabelSource::TextLayer => labels::legend(spec, &lines, &inp.image, k),
        LabelSource::Ocr => labels::legend_ocr(spec, &lines),
    };
    let classes = segment::classify(spec, &inp.image, k);
    let graphs: Vec<(usize, SkelGraph)> = (0..spec.inks.len())
        .map(|i| {
            let (_, g) =
                segment::ink_lines(&classes, inp.image.w, inp.image.h, i as u8, &spec.line);
            (i, g)
        })
        .collect();
    let union = {
        let mut m = segment::Mask::new(inp.image.w, inp.image.h);
        for (i, c) in classes.iter().enumerate() {
            m.v[i] = u8::from(*c != segment::NONE);
        }
        let m = m.close(spec.line.close_r).fill_holes(spec.line.max_hole_px);
        let (_, g) = segment::lines_of_mask(&m, &spec.line);
        g
    };
    lap("segmentation", &mut t);
    let mut osm = osmref::pistes(planet, &spec.extent)?;
    let (lms, landmarks_fetched) = osmref::landmarks(dl, &spec.extent)?;
    osm.landmarks = lms;
    lap("osm", &mut t);
    let (min_sim, ele_tol) = match spec.labels {
        LabelSource::TextLayer => (0.92, Some(30.0)),
        LabelSource::Ocr => (0.75, None),
    };
    let candidates = align::label_candidates(&map_labels, &osm.landmarks, min_sim, ele_tol);
    let coarse = align::coarse(&candidates, RANSAC_TOL_PT).ok_or_else(|| {
        crate::Error::Fit(format!(
            "{}: no consistent label ↔ landmark set ({} candidates)",
            spec.id,
            candidates.len()
        ))
    })?;
    lap("ransac", &mut t);
    let drawn = align::sample_lines(&graphs, k, 3.0);
    let tree = align::osm_tree(&osm, 10.0);
    let (warp, pairs, steps) = align::icp(
        spec,
        &drawn,
        &osm,
        &tree,
        &coarse.affine,
        ICP_SCHEDULE,
        ICP_CELL_PT,
    )
    .ok_or_else(|| crate::Error::Fit(format!("{}: ICP found too few pairs", spec.id)))?;
    lap("icp", &mut t);
    let ties = align::junction_ties(&union, k, &osm, &warp, TIE_MAX_PT * coarse.scale);
    let tie_pairs: Vec<_> = ties.iter().map(|t| (t.plan, t.map)).collect();
    let kinds = [
        ModelKind::Similarity,
        ModelKind::Affine,
        ModelKind::Homography,
        ModelKind::Tps(0.0),
        ModelKind::Tps(0.01),
        ModelKind::Tps(0.1),
        ModelKind::Tps(1.0),
    ];
    // Labels kept by the RANSAC: written beside their feature (offset included).
    let label_pairs: Vec<_> = coarse.inliers.iter().map(|c| (c.plan, c.map)).collect();
    let loo_labels = compare_models(&label_pairs, &kinds);
    let loo = compare_models(&tie_pairs, &kinds);
    let final_lambda = ICP_SCHEDULE.last().map_or(0.01, |s| s.1);
    let held_out_m: Vec<f64> = held_out(&pairs, &ties, final_lambda, 2.0 * ICP_CELL_PT)
        .into_iter()
        .flatten()
        .collect();
    lap("ties", &mut t);
    Ok(Registration {
        spec,
        input: inp,
        lines,
        ocr_lines,
        map_labels,
        legend,
        classes,
        graphs,
        union,
        osm,
        landmarks_fetched,
        candidates,
        coarse,
        warp,
        pairs,
        steps,
        ties,
        loo,
        loo_labels,
        held_out_m,
        timings,
    })
}

/// Layer name (`data/out/<site>/plan_pistes/`).
pub const LAYER: &str = "plan_pistes";

/// What a build produced.
#[derive(Debug, Clone, Serialize)]
pub struct BuildReport {
    /// Output directory.
    pub dir: std::path::PathBuf,
    /// Key figures (also in `plan_pistes.meta.json`).
    pub stats: serde_json::Value,
    /// Decisions on every piste of the legend.
    pub decisions: Vec<transcribe::Decision>,
}

/// OCR read rate against the text layer: share of the words of the map (three letters
/// at least) found by the OCR in a line overlapping the word, with a folded similarity of
/// at least `min_sim`.
pub fn ocr_rate(reg: &Registration, min_sim: f64) -> Option<(usize, usize)> {
    if reg.spec.labels != LabelSource::TextLayer {
        return None;
    }
    let words: Vec<&TextBox> = reg
        .input
        .text
        .lines
        .iter()
        .flat_map(|(_, ws)| ws.iter())
        .filter(|w| {
            let c = w.center();
            reg.spec.on_map(c[0], c[1])
                && labels::fold(&w.text)
                    .chars()
                    .filter(|c| c.is_alphabetic())
                    .count()
                    >= 3
        })
        .collect();
    let overlaps = |a: &[f64; 4], b: &[f64; 4]| {
        a[0] < b[2] + 2.0 && b[0] < a[2] + 2.0 && a[1] < b[3] + 2.0 && b[1] < a[3] + 2.0
    };
    let read = words
        .iter()
        .filter(|w| {
            let f = labels::fold(&w.text);
            reg.ocr_lines.iter().any(|l| {
                overlaps(&l.rect, &w.rect)
                    && labels::fold(&l.text)
                        .split(' ')
                        .any(|t| labels::similarity(t, &f) >= min_sim)
            })
        })
        .count();
    Some((read, words.len()))
}

fn props_of(reg: &Registration, d: &transcribe::Decision) -> Option<PlanPiste> {
    let status = d.status?;
    let osm = &reg.osm;
    let relation_names: BTreeSet<&str> = d
        .relations
        .iter()
        .filter_map(|&r| osm.relations[r].name.as_deref())
        .collect();
    let relation_colours: BTreeSet<&str> = d
        .relations
        .iter()
        .filter_map(|&r| osm.relations[r].colour.as_deref())
        .collect();
    let join =
        |s: BTreeSet<&str>| (!s.is_empty()).then(|| s.into_iter().collect::<Vec<_>>().join(";"));
    Some(PlanPiste {
        name: d.name.clone(),
        name_read: d.entry.name_read.clone(),
        number: None,
        colour: d.colour.clone(),
        ink: d.ink.clone().unwrap_or_default(),
        kind: d.kind.unwrap_or(PisteKind::Ski),
        // OCR digits cannot be checked (« 7,5 km » read « 75km »): not transcribed.
        lengths_km_plan: if reg.spec.labels == LabelSource::Ocr {
            Vec::new()
        } else {
            d.entry.lengths_km.clone()
        },
        departures: d.entry.departures.clone(),
        length_m: d
            .ways
            .iter()
            .map(|&w| osm.ways[w].length_m)
            .sum::<f64>()
            .round(),
        is_loop: transcribe::is_loop(osm, &d.ways),
        direction: None,
        method: d.method.unwrap_or(MatchMethod::Exact),
        score: (d.score * 1000.0).round() / 1000.0,
        geometry_source: GeometrySource::Osm,
        osm_status: status,
        osm_name: join(relation_names),
        osm_colour: join(relation_colours),
        osm_relation_ids: d.relations.iter().map(|&r| osm.relations[r].id).collect(),
        osm_way_ids: d.ways.iter().map(|&w| osm.ways[w].id).collect(),
        plan: reg.spec.id.to_owned(),
    })
}

/// Transcribes a registered plan and writes `data/out/<site>/plan_pistes/`.
pub fn transcribe_and_write(
    reg: &Registration,
    layout: &DataLayout,
    planet_source: &SourceRef,
) -> Result<BuildReport> {
    let site = skimap_common::site::site(reg.spec.site)?;
    let dir = layout.layer_dir(site, LAYER)?;
    DataLayout::ensure(&dir)?;
    let plan = transcribe::entries(reg);
    let dmap = transcribe::InkDistance::new(reg);
    let (decisions, validation) = transcribe::decide(reg, &dmap, &plan);
    let lms = transcribe::landmarks(reg);
    // Pistes.
    let mut features = Vec::new();
    for d in &decisions {
        let Some(p) = props_of(reg, d) else { continue };
        let lines: Vec<Value> = d
            .ways
            .iter()
            .map(|&w| {
                Value::Array(
                    reg.osm.ways[w]
                        .ll
                        .iter()
                        .map(|&q| output::position(q))
                        .collect(),
                )
            })
            .collect();
        let geom = json!({"type": "MultiLineString", "coordinates": lines});
        features.push(output::feature(
            geom,
            serde_json::to_value(&p).map_err(|e| Error::Json {
                context: "plan piste".into(),
                source: e,
            })?,
        ));
    }
    output::write_geojson(&dir.join("plan_pistes.geojson"), &features)?;
    // Landmarks.
    let mut lfeat = Vec::new();
    for m in &lms {
        let lm = &reg.osm.landmarks[m.landmark];
        let (name, ele) = labels::split_elevation(&m.label);
        let p = PlanLandmark {
            name: if reg.spec.labels == LabelSource::Ocr {
                lm.name.clone()
            } else {
                name
            },
            kind: m.kind,
            ele_plan: m.ele.or(ele),
            osm: lm.osm.clone(),
            osm_name: lm.name.clone(),
            residual_m: if m.residual_m.is_finite() {
                m.residual_m.round()
            } else {
                -1.0
            },
            plan: reg.spec.id.to_owned(),
        };
        lfeat.push(output::feature(
            output::point(lm.ll),
            serde_json::to_value(&p).map_err(|e| Error::Json {
                context: "plan landmark".into(),
                source: e,
            })?,
        ));
    }
    output::write_geojson(&dir.join("plan_landmarks.geojson"), &lfeat)?;
    // Statistics.
    let transcribed = decisions.iter().filter(|d| d.status.is_some()).count();
    let tie_pairs: Vec<f64> = reg.ties.iter().map(|t| t.residual_m).collect();
    let ocr = ocr_rate(reg, 0.8);
    let ocr_exact = ocr_rate(reg, 1.0);
    let stats = json!({
        "legend_lines": plan.len(),
        "plan_pistes": decisions.len(),
        "transcribed": transcribed,
        "transcribed_share": if decisions.is_empty() { 0.0 } else { transcribed as f64 / decisions.len() as f64 },
        "with_osm_name_candidate": decisions.iter().filter(|d| !d.ways.is_empty()).count(),
        "status": transcribe::status_counts(&decisions),
        "validation": validation,
        "not_transcribed": decisions.iter().filter(|d| d.status.is_none())
            .map(|d| json!({"name": d.entry.name_read, "ink": d.ink, "score": d.score, "shift_max": d.shift_max, "reason": d.reason}))
            .collect::<Vec<_>>(),
        "landmarks": lms.len(),
        "registration": {
            "labels_on_map": reg.map_labels.len(),
            "label_candidates": reg.candidates.len(),
            "ransac_inliers": reg.coarse.inliers.len(),
            "scale_m_per_pt": reg.coarse.scale,
            "icp": reg.steps,
            "icp_pairs": reg.pairs.len(),
            "tie_points": reg.ties.len(),
            "tie_residual_m": ErrStats::of(&tie_pairs),
            "loo": reg.loo,
            "loo_labels": reg.loo_labels,
            "held_out_dense_warp_m": ErrStats::of(&reg.held_out_m),
        },
        "ocr": {
            "lines": reg.ocr_lines.len(),
            "read_rate_sim08": ocr.map(|(a, b)| json!({"read": a, "words": b, "rate": a as f64 / b.max(1) as f64})),
            "read_rate_exact": ocr_exact.map(|(a, b)| json!({"read": a, "words": b, "rate": a as f64 / b.max(1) as f64})),
        },
        "osm": {"ways": reg.osm.ways.len(), "relations": reg.osm.relations.len(),
                "landmarks": reg.osm.landmarks.len(), "junctions": reg.osm.junctions.len()},
        "timings_s": reg.timings,
    });
    let spec = reg.spec;
    let mut meta = LayerMeta::new(LAYER, site.id);
    meta.tickets = vec!["P22 (#121)".to_owned()];
    meta.crs = Some("EPSG:4326 (WGS84, longitude, latitude)".to_owned());
    meta.validity = Some(format!("édition du plan : {}", spec.edition));
    meta.sources = vec![
        SourceRef::from_fetched(spec.title, spec.rights, spec.publisher, &reg.input.fetched)
            .personal_use_only()
            .producer(spec.publisher)
            .edition(spec.edition)
            .note(format!(
                "plan officiel de la station, lien publié sur {} ; image du plan jamais redistribuée (entrée locale du pipeline seulement)",
                spec.page_url
            )),
        planet_source.clone(),
        SourceRef::from_fetched(
            "OpenStreetMap, repères nommés (Overpass)",
            "ODbL 1.0",
            "© les contributeurs d'OpenStreetMap (ODbL)",
            &reg.landmarks_fetched,
        ),
    ];
    meta.files = vec![
        FileMeta::new(
            "plan_pistes.geojson",
            "pistes du plan officiel transcrites sur les voies OSM (une MultiLineString par piste du plan), seulement celles appariées de façon exacte",
        )
        .licence("géométrie ODbL 1.0 (OSM) ; attributs du plan : usage personnel")
        .count(features.len())
        .properties([
            ("name", "nom de la piste sur le plan (plan lu par OCR : nom de la relation OSM appariée)"),
            ("name_read", "nom tel que lu sur le plan (couche texte ou OCR)"),
            ("number", "numéro sur le plan, si le plan numérote"),
            ("colour", "couleur de difficulté du plan (green, blue, red, black)"),
            ("ink", "encre du tracé sur le plan"),
            ("kind", "ski, liaison, multi_activity, pedestrian, snowshoe (légende)"),
            ("lengths_km_plan", "longueurs imprimées, une par départ"),
            ("departures", "départs (encadrés ou secteurs de la légende)"),
            ("length_m", "longueur de la géométrie OSM"),
            ("loop", "géométrie fermée (boucle)"),
            ("direction", "sens des flèches du plan (non lu au 15/11/2026 : absent)"),
            ("method", "exact (noms certains et encre dessinée sur la géométrie) ou ml"),
            ("score", "part de la géométrie où le plan dessine l'encre de la piste"),
            ("geometry_source", "osm ou estimated_from_plan"),
            ("osm_status", "match, colour_differs, colour_missing, name_missing, missing"),
            ("osm_name", "nom des relations OSM appariées"),
            ("osm_colour", "couleur des relations OSM"),
            ("osm_relation_ids", "relations OSM"),
            ("osm_way_ids", "voies OSM de la géométrie"),
            ("plan", "identifiant du plan"),
        ]),
        FileMeta::new(
            "plan_landmarks.geojson",
            "repères du plan (villages, sommets, cols, refuges, départs) appariés aux éléments OSM nommés ; géométrie OSM",
        )
        .licence("ODbL 1.0 (OSM)")
        .count(lfeat.len())
        .properties([
            ("name", "nom sur le plan"),
            ("kind", "place, summit, pass, refuge, departure, lake, other"),
            ("ele_plan", "altitude imprimée"),
            ("osm", "élément OSM"),
            ("osm_name", "nom OSM"),
            ("residual_m", "distance entre l'étiquette reportée sur la carte et l'élément (−1 : départ sans étiquette sur la carte)"),
            ("plan", "identifiant du plan"),
        ]),
    ];
    meta.parameters = json!({
        "dpi": spec.dpi,
        "labels": format!("{:?}", spec.labels),
        "ransac_tol_pt": RANSAC_TOL_PT,
        "icp_schedule_m_lambda": ICP_SCHEDULE,
        "icp_cell_pt": ICP_CELL_PT,
        "tie_max_pt": TIE_MAX_PT,
        "share_radii_pt": transcribe::RADII_PT,
        "min_share": transcribe::MIN_SHARE,
        "negative_shifts_m": transcribe::NEGATIVE_SHIFTS_M,
        "max_angle_deg": transcribe::MAX_ANGLE_DEG,
        "share_step_m": transcribe::SHARE_STEP_M,
        "ocr_min_sim": transcribe::OCR_MIN_SIM,
        "ocr_margin": transcribe::OCR_MARGIN,
        "extent": [spec.extent.south, spec.extent.west, spec.extent.north, spec.extent.east],
    });
    meta.stats = stats.clone();
    meta.notes = vec![
        "Le plan reste la propriété de la station : usage personnel (personal_sources), jamais redistribué ; seules des géométries OSM portent les attributs transcrits.".to_owned(),
        "Une piste n'est transcrite que si son nom est certain et si le plan dessine son encre sur la géométrie OSM appariée (part au-dessus du seuil validé sur des paires négatives) ; sinon elle est listée dans stats.not_transcribed avec la raison.".to_owned(),
    ];
    meta.write(&dir)?;
    Ok(BuildReport {
        dir,
        stats,
        decisions,
    })
}

/// Builds the transcription of the site's plan: personal sources required (the plan is
/// the resort's), planet extract and OCR models from the cache.
pub fn build(
    site: &skimap_common::site::Site,
    layout: &DataLayout,
    dl: &Downloader,
    personal_sources: bool,
) -> Result<BuildReport> {
    if !personal_sources {
        return Err(Error::PersonalSourcesDisabled);
    }
    let spec = crate::spec::plan_for(site.id)
        .ok_or_else(|| Error::Data(format!("no official plan known for {}", site.id)))?;
    let planet = skimap_osm::extract::load_planet(dl)
        .map_err(|e| Error::Data(format!("planet extract: {e}")))?;
    let engine = ocr::engine(dl)?;
    let reg = register(spec, layout, dl, &planet, Some(&engine))?;
    transcribe_and_write(&reg, layout, &planet.source)
}
