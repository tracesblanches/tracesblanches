//! Registration of a plan and transcription of its pistes onto our geometries.

use std::collections::{BTreeMap, BTreeSet};
use std::time::Instant;

use rayon::prelude::*;
use serde::Serialize;
use serde_json::{Value, json};
use skimap_common::download::Downloader;
use skimap_common::layout::DataLayout;
use skimap_common::meta::{FileMeta, LayerMeta, SourceRef};
use skimap_common::output;
use skimap_osm::extract::Planet;
use skimap_osm::pistes::Oneway;

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
use crate::transcript::{
    Direction, GeometrySource, MatchMethod, OnewayStatus, PisteKind, PlanLandmark, PlanPiste,
    PlanWay,
};
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

/// Largest distance between an arrow and the way it is counted on, page points.
pub const ARROW_RADIUS_PT: f64 = 4.0;

/// Everything learnt about a plan.
pub struct Registration {
    /// The plan.
    pub spec: PlanSpec,
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
    /// Source of the landmarks (Overpass answer or BD TOPO toponyms).
    pub landmarks_source: osmref::LandmarkSource,
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
    /// Camera of a plan drawn in perspective (panorama), placed on the relief.
    pub camera: Option<crate::perspective::Camera>,
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

/// OCR of the rendering, raw or text-enhanced (top-hat), in tiles of 800 px, cached in
/// `data/cache/pisteplan/` by PDF, resolution, tiling and enhancement.
pub fn ocr_page(
    engine: &ocrs::OcrEngine,
    inp: &PlanInput,
    enhanced: bool,
    layout: &DataLayout,
) -> Result<Vec<TextBox>> {
    let dir = layout.cache_dir(input::CACHE)?;
    DataLayout::ensure(&dir)?;
    let dpi = (72.0 / inp.pt_per_px).round() as u32;
    let key = if enhanced {
        format!("tophat-r{OCR_TOPHAT_R}-g{OCR_TOPHAT_GAIN}")
    } else {
        "raw".to_owned()
    };
    let path = dir.join(format!(
        "{}-{dpi}-ocr-{key}-t{OCR_TILE}-o{OCR_OVERLAP}.json",
        &inp.fetched.meta.sha256[..16]
    ));
    let img = if enhanced {
        ocr::enhance_text(&inp.image, OCR_TOPHAT_R, OCR_TOPHAT_GAIN)
    } else {
        inp.image.clone()
    };
    ocr::read_lines_cached(engine, &img, OCR_TILE, OCR_OVERLAP, inp.pt_per_px, &path)
}

/// Registers a plan on OSM.
pub fn register(
    spec: PlanSpec,
    layout: &DataLayout,
    dl: &Downloader,
    planet: &Planet,
    ocr_engine: Option<&ocrs::OcrEngine>,
) -> Result<Registration> {
    let t = Instant::now();
    let inp = input::load(&spec, layout, dl)?;
    let load_s = t.elapsed().as_secs_f64();
    register_input(spec, inp, load_s, layout, dl, planet, ocr_engine, None)
}

/// Geographic extent of a page rectangle carried onto the map by `fit` (page point →
/// Lambert-93; a 5 × 5 grid of its points), grown by `margin_m`.
pub fn extent_of(
    area: &crate::spec::PageRect,
    fit: &dyn Fn(crate::warp::P2) -> crate::warp::P2,
    margin_m: f64,
) -> Result<skimap_common::geom::BBox> {
    use skimap_common::crs::from_lambert93;
    let (mut x0, mut y0, mut x1, mut y1) = (f64::MAX, f64::MAX, f64::MIN, f64::MIN);
    for i in 0..5 {
        for j in 0..5 {
            let q = fit([
                area[0] + (area[2] - area[0]) * i as f64 / 4.0,
                area[1] + (area[3] - area[1]) * j as f64 / 4.0,
            ]);
            x0 = x0.min(q[0]);
            y0 = y0.min(q[1]);
            x1 = x1.max(q[0]);
            y1 = y1.max(q[1]);
        }
    }
    let m = margin_m;
    let c = [
        from_lambert93(x0 - m, y0 - m)?,
        from_lambert93(x1 + m, y1 + m)?,
        from_lambert93(x0 - m, y1 + m)?,
        from_lambert93(x1 + m, y0 - m)?,
    ];
    Ok(skimap_common::geom::BBox::new(
        c.iter().map(|p| p.lat).fold(f64::MAX, f64::min),
        c.iter().map(|p| p.lon).fold(f64::MAX, f64::min),
        c.iter().map(|p| p.lat).fold(f64::MIN, f64::max),
        c.iter().map(|p| p.lon).fold(f64::MIN, f64::max),
    ))
}

/// Registers a plan already loaded (`load_s`: time taken by the input step). With
/// `refine_extent_m`, the sheet's extent is a hint: once the labels place the plan, the
/// OSM pistes are taken on the extent drawn, grown by that margin.
#[allow(clippy::too_many_arguments)]
pub fn register_input(
    mut spec: PlanSpec,
    inp: PlanInput,
    load_s: f64,
    layout: &DataLayout,
    dl: &Downloader,
    planet: &Planet,
    ocr_engine: Option<&ocrs::OcrEngine>,
    refine_extent_m: Option<f64>,
) -> Result<Registration> {
    let mut timings = vec![("input".to_owned(), load_s)];
    let mut t = Instant::now();
    let mut lap = |name: &str, t: &mut Instant| {
        timings.push((name.to_owned(), t.elapsed().as_secs_f64()));
        *t = Instant::now();
    };
    let k = inp.pt_per_px;
    // OCR in tiles of 800 px. Picture plans: the map is read on the text-enhanced image
    // (top-hat: letters over textured backgrounds), the legend panels (plain background)
    // on the image itself.
    let raw_ocr = match ocr_engine {
        Some(e) => ocr_page(e, &inp, false, layout)?,
        None => Vec::new(),
    };
    let ocr_lines: Vec<TextBox> = match (ocr_engine, spec.labels) {
        (Some(e), LabelSource::Ocr) => {
            let on_map = |l: &TextBox| {
                let c = l.center();
                spec.on_map(c[0], c[1])
            };
            ocr_page(e, &inp, true, layout)?
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
        LabelSource::TextLayer => labels::legend(&spec, &lines, &inp.image, k),
        LabelSource::Ocr => labels::legend_ocr(&spec, &lines, &inp.image, k),
    };
    let classes = segment::classify(&spec, &inp.image, k);
    let graphs: Vec<(usize, SkelGraph)> = (0..spec.inks.len())
        .into_par_iter()
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
    let (lms, landmarks_source) = osmref::landmarks_any(dl, &spec.extent)?;
    osm.landmarks = lms;
    lap("osm", &mut t);
    let (min_sim, ele_tol) = match spec.labels {
        LabelSource::TextLayer => (0.92, Some(30.0)),
        LabelSource::Ocr => (0.75, None),
    };
    let candidates = align::label_candidates(&map_labels, &osm.landmarks, min_sim, ele_tol);
    // Placement: the RANSAC allows a rotation of 45° at most (north up, give or take);
    // an automatic sheet may be any plan, drawn rotated to fit its page: when the first
    // placement fails (no consistent set, or an ICP without pairs), any rotation is tried,
    // with five consistent labels at least.
    let rotations: &[f64] = if refine_extent_m.is_some() {
        &[45.0, 180.0]
    } else {
        &[45.0]
    };
    let hint = spec.extent;
    let mut placed = None;
    let mut failure = format!(
        "{}: no consistent label ↔ landmark set ({} candidates)",
        spec.id,
        candidates.len()
    );
    // Flat placements (one per rotation bound), then, for an automatic sheet, the
    // perspective placement of a panorama (camera on the relief).
    let attempts = rotations.len() + usize::from(refine_extent_m.is_some());
    let mut camera_used = None;
    for attempt in 0..attempts {
        let (coarse, camera) = if let Some(&max_rot) = rotations.get(attempt) {
            let Some(coarse) = align::coarse(&candidates, RANSAC_TOL_PT, max_rot) else {
                continue;
            };
            if max_rot > 45.0 && coarse.inliers.len() < 5 {
                continue;
            }
            (coarse, None)
        } else {
            match perspective_placement(&hint, &candidates, dl)? {
                Some(p) => p,
                None => continue,
            }
        };
        let to_map = |p: crate::warp::P2| match &camera {
            Some((cam, dem)) => crate::perspective::Camera::ground(cam, dem, p),
            None => Some(coarse.affine.apply(align::up(p))),
        };
        // Automatic sheets: the extent is a hint around the domains; once the plan is
        // placed, the OSM pistes are those of the extent actually drawn (map area carried
        // onto the map by the coarse fit, plus the margin).
        if let Some(margin) = refine_extent_m {
            let Some(drawn_extent) = extent_seen(&spec.map_area, &to_map, margin)? else {
                failure = format!("{}: no ray of the map area meets the terrain", spec.id);
                continue;
            };
            let overlaps = drawn_extent.south < hint.north
                && drawn_extent.north > hint.south
                && drawn_extent.west < hint.east
                && drawn_extent.east > hint.west;
            if !overlaps {
                failure = format!(
                    "{}: the plan placed by its labels ({drawn_extent:?}) is outside the hint",
                    spec.id
                );
                continue;
            }
            spec.extent = drawn_extent;
            let lms = std::mem::take(&mut osm.landmarks);
            osm = osmref::pistes(planet, &spec.extent)?;
            osm.landmarks = lms;
        }
        let drawn = align::sample_lines(&graphs, k, 3.0);
        let tree = align::osm_tree(&osm, 10.0);
        match align::icp(
            &spec,
            &drawn,
            &osm,
            &tree,
            &to_map,
            ICP_SCHEDULE,
            ICP_CELL_PT,
        ) {
            Some(r) => {
                camera_used = camera.map(|c| c.0);
                placed = Some((coarse, r));
                break;
            }
            None => failure = format!("{}: ICP found too few pairs", spec.id),
        }
    }
    let Some((coarse, (warp, pairs, steps))) = placed else {
        return Err(crate::Error::Fit(failure));
    };
    lap("ransac_icp", &mut t);
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
        landmarks_source,
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
        camera: camera_used,
    })
}

/// Extent of the ground seen in a page rectangle (a 9 × 9 grid of its points carried by
/// `to_map`; points without an image skipped), grown by `margin_m`; `None` when no point
/// has an image.
pub fn extent_seen(
    area: &crate::spec::PageRect,
    to_map: &dyn Fn(crate::warp::P2) -> Option<crate::warp::P2>,
    margin_m: f64,
) -> Result<Option<skimap_common::geom::BBox>> {
    use skimap_common::crs::from_lambert93;
    let (mut x0, mut y0, mut x1, mut y1) = (f64::MAX, f64::MAX, f64::MIN, f64::MIN);
    let mut any = false;
    for i in 0..9 {
        for j in 0..9 {
            let Some(q) = to_map([
                area[0] + (area[2] - area[0]) * i as f64 / 8.0,
                area[1] + (area[3] - area[1]) * j as f64 / 8.0,
            ]) else {
                continue;
            };
            any = true;
            x0 = x0.min(q[0]);
            y0 = y0.min(q[1]);
            x1 = x1.max(q[0]);
            y1 = y1.max(q[1]);
        }
    }
    if !any {
        return Ok(None);
    }
    let m = margin_m;
    let c = [
        from_lambert93(x0 - m, y0 - m)?,
        from_lambert93(x1 + m, y1 + m)?,
        from_lambert93(x0 - m, y1 + m)?,
        from_lambert93(x1 + m, y0 - m)?,
    ];
    Ok(Some(skimap_common::geom::BBox::new(
        c.iter().map(|p| p.lat).fold(f64::MAX, f64::min),
        c.iter().map(|p| p.lon).fold(f64::MAX, f64::min),
        c.iter().map(|p| p.lat).fold(f64::MIN, f64::max),
        c.iter().map(|p| p.lon).fold(f64::MIN, f64::max),
    )))
}

/// Margin of the terrain model around the domains of a panorama, km (the hint is the
/// domains grown by [`crate::collect::HINT_MARGIN_KM`]).
pub const PANORAMA_DEM_MARGIN_KM: f64 = 3.0;

/// Tolerance of the camera RANSAC, page points (twice the flat one: a painter bends the
/// relief).
pub const PERSPECTIVE_TOL_PT: f64 = 2.0 * RANSAC_TOL_PT;

/// Perspective placement of a panorama: terrain model of the domains (LiDAR HD), camera
/// RANSAC on the label candidates. `None` when no camera gathers enough labels. The coarse
/// fit returned beside the camera is the similarity of its inliers (scale for the tie
/// tolerance, report).
#[allow(clippy::type_complexity)]
pub fn perspective_placement(
    hint: &skimap_common::geom::BBox,
    candidates: &[LabelMatch],
    dl: &Downloader,
) -> Result<
    Option<(
        Coarse,
        Option<(crate::perspective::Camera, crate::perspective::Dem)>,
    )>,
> {
    let labels: BTreeSet<usize> = candidates.iter().map(|c| c.label_idx).collect();
    if labels.len() < crate::perspective::MIN_INLIERS {
        return Ok(None);
    }
    let area = crate::collect::grown(
        *hint,
        PANORAMA_DEM_MARGIN_KM - crate::collect::HINT_MARGIN_KM,
    );
    let e = skimap_terrain::dem::Extent::from_bbox(area)
        .map_err(|e| Error::Data(format!("terrain extent: {e}")))?;
    let grid = skimap_terrain::dem::fetch_dem(&e, dl)
        .map_err(|e| Error::Data(format!("terrain model: {e}")))?;
    let dem = crate::perspective::Dem::new(&grid);
    let placed = crate::perspective::place(candidates, &dem, PERSPECTIVE_TOL_PT);
    tracing::info!(
        labels = labels.len(),
        candidates = candidates.len(),
        inliers = placed.as_ref().map_or(0, |p| p.inliers.len()),
        median_pt = placed.as_ref().map(|p| p.median_px_pt),
        centre = ?placed.as_ref().map(|p| p.camera.centre),
        "perspective placement"
    );
    let Some(p) = placed else {
        return Ok(None);
    };
    let src: Vec<crate::warp::P2> = p.inliers.iter().map(|c| align::up(c.plan)).collect();
    let dst: Vec<crate::warp::P2> = p.inliers.iter().map(|c| c.map).collect();
    let Some(affine) = crate::warp::Affine::fit_similarity(&src, &dst) else {
        return Ok(None);
    };
    let coarse = Coarse {
        scale: affine.scale(),
        affine,
        inliers: p.inliers,
    };
    Ok(Some((coarse, Some((p.camera, dem)))))
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
    site: &skimap_common::site::Site,
    considered: &[PlanTried],
    layout: &DataLayout,
    planet_source: &SourceRef,
) -> Result<BuildReport> {
    let dir = layout.layer_dir(site, LAYER)?;
    DataLayout::ensure(&dir)?;
    let plan = transcribe::entries(reg);
    let dmap = transcribe::InkDistance::new(reg);
    let (mut decisions, validation) = transcribe::decide(reg, &dmap, &plan);
    // Learnt matches for the lines the exact matching left out, when a model trained on
    // the other domains is there (`data/out/france/plan_training/model.json`).
    let model_path = layout.france_layer_dir("plan_training")?.join("model.json");
    let model = crate::learn::Model::read(&model_path);
    let ml = match &model {
        Some(m) => crate::learn::apply_model(reg, &dmap, &mut decisions, m),
        None => 0,
    };
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
    // Direction arrows on the ways of the transcribed pistes.
    let arrows = if reg.spec.arrows {
        let s = reg.spec.dpi as f64 / 100.0;
        crate::arrows::detect(
            reg,
            &dmap,
            (12.0 * s * s) as usize,
            (120.0 * s * s) as usize,
        )
    } else {
        Vec::new()
    };
    let mut per_way: BTreeMap<usize, (Vec<String>, usize, usize)> = BTreeMap::new();
    for d in decisions.iter().filter(|d| d.status.is_some()) {
        let Some(ink) = d.entry.ink else { continue };
        for &w in &d.ways {
            let line: Vec<[f64; 2]> = reg.osm.ways[w]
                .xy
                .iter()
                .map(|&q| reg.warp.to_plan(q))
                .collect();
            let (f, b) = crate::arrows::votes(&arrows, ink, &line, ARROW_RADIUS_PT);
            let e = per_way.entry(w).or_default();
            e.0.push(d.name.clone());
            e.1 += f;
            e.2 += b;
        }
    }
    let mut wfeat = Vec::new();
    let mut oneway_counts: BTreeMap<String, usize> = BTreeMap::new();
    for (w, (pistes, f, b)) in &per_way {
        if f + b == 0 {
            continue;
        }
        let way = &reg.osm.ways[*w];
        let direction = match (*f > 0, *b > 0) {
            (true, false) => Direction::Forward,
            (false, true) => Direction::Backward,
            _ => Direction::Both,
        };
        let osm_oneway = match way.oneway {
            Oneway::Forward => "forward",
            Oneway::Backward => "backward",
            Oneway::No => "no",
            Oneway::Unknown => "unknown",
        };
        let oneway_status = match (direction, way.oneway) {
            (Direction::Forward, Oneway::Forward) | (Direction::Backward, Oneway::Backward) => {
                OnewayStatus::Match
            }
            (Direction::Forward, Oneway::Backward) | (Direction::Backward, Oneway::Forward) => {
                OnewayStatus::Opposite
            }
            (Direction::Both, Oneway::Forward | Oneway::Backward) => OnewayStatus::PlanBothWays,
            (Direction::Both, _) => OnewayStatus::BothWays,
            _ => OnewayStatus::MissingInOsm,
        };
        let key = serde_json::to_value(oneway_status)
            .ok()
            .and_then(|v| v.as_str().map(str::to_owned))
            .unwrap_or_default();
        *oneway_counts.entry(key).or_default() += 1;
        let mut names = pistes.clone();
        names.sort();
        names.dedup();
        let p = PlanWay {
            osm_way_id: way.id,
            pistes: names,
            direction,
            arrows_forward: *f,
            arrows_backward: *b,
            osm_oneway: osm_oneway.to_owned(),
            oneway_status,
            plan: reg.spec.id.to_owned(),
        };
        wfeat.push(output::feature(
            output::line_string(way.ll.iter().copied()),
            serde_json::to_value(&p).map_err(|e| Error::Json {
                context: "plan way".into(),
                source: e,
            })?,
        ));
    }
    output::write_geojson(&dir.join("plan_ways.geojson"), &wfeat)?;
    // Statistics.
    let transcribed = decisions.iter().filter(|d| d.status.is_some()).count();
    let tie_pairs: Vec<f64> = reg.ties.iter().map(|t| t.residual_m).collect();
    let ocr = ocr_rate(reg, 0.8);
    let ocr_exact = ocr_rate(reg, 1.0);
    let stats = json!({
        "plan": reg.spec.id,
        "plans_considered": considered,
        "legend_lines": plan.len(),
        "plan_pistes": decisions.len(),
        "transcribed": transcribed,
        "transcribed_ml": ml,
        "model": model.as_ref().map(|m| json!({"file": model_path.display().to_string(), "threshold": m.threshold, "trees": m.model.trees.len()})),
        "transcribed_share": if decisions.is_empty() { 0.0 } else { transcribed as f64 / decisions.len() as f64 },
        "with_osm_name_candidate": decisions.iter().filter(|d| !d.ways.is_empty()).count(),
        "status": transcribe::status_counts(&decisions),
        "validation": validation,
        "not_transcribed": decisions.iter().filter(|d| d.status.is_none())
            .map(|d| json!({"name": d.entry.name_read, "ink": d.ink, "score": d.score, "shift_max": d.shift_max, "reason": d.reason}))
            .collect::<Vec<_>>(),
        "landmarks": lms.len(),
        "arrows": {"detected": arrows.len(), "ways_with_arrows": wfeat.len(), "oneway_status": oneway_counts},
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
    let spec = &reg.spec;
    let mut meta = LayerMeta::new(LAYER, site.id);
    meta.tickets = vec!["P22 (#121)".to_owned()];
    meta.crs = Some("EPSG:4326 (WGS84, longitude, latitude)".to_owned());
    meta.validity = Some(format!("édition du plan : {}", spec.edition));
    meta.sources = vec![
        SourceRef::from_fetched(&spec.title, &spec.rights, &spec.publisher, &reg.input.fetched)
            .personal_use_only()
            .producer(&spec.publisher)
            .edition(&spec.edition)
            .note(format!(
                "plan officiel de la station, lien publié sur {} ; image du plan jamais redistribuée (entrée locale du pipeline seulement)",
                spec.page_url
            )),
        planet_source.clone(),
        SourceRef::from_fetched(
            reg.landmarks_source.name,
            reg.landmarks_source.licence,
            reg.landmarks_source.attribution,
            &reg.landmarks_source.fetched,
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
            "plan_ways.geojson",
            "voies OSM des pistes transcrites portant des flèches de sens sur le plan (géométrie OSM, dans l'ordre de la voie)",
        )
        .licence("géométrie ODbL 1.0 (OSM) ; sens du plan : usage personnel")
        .count(wfeat.len())
        .properties([
            ("osm_way_id", "voie OSM"),
            ("pistes", "pistes transcrites qui l'empruntent"),
            ("direction", "forward / backward (par rapport à l'ordre de la voie), both : flèches dans les deux sens"),
            ("arrows_forward", "flèches dans l'ordre de la voie"),
            ("arrows_backward", "flèches à contresens de l'ordre de la voie"),
            ("osm_oneway", "piste:oneway OSM (forward, backward, no, unknown)"),
            ("oneway_status", "match, missing_in_osm, opposite, plan_both_ways, both_ways"),
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

/// A collected plan tried for a site, and its outcome.
#[derive(Debug, Clone, Serialize)]
pub struct PlanTried {
    /// Catalogue identifier.
    pub id: String,
    /// Registered on the map.
    pub registered: bool,
    /// Drawn extent meets the site.
    pub draws_site: bool,
    /// Pistes transcribed by the exact matching.
    pub exact: usize,
    /// Failure, if any.
    pub error: Option<String>,
}

/// Registers the plans of the catalogue that may draw the site (census domains meeting
/// its extent, [`crate::collect::plans_for_area`]), each with an automatic sheet, and
/// keeps the one whose drawn extent meets the site with the most pistes transcribed by
/// the exact matching. The same rule for every site: no plan, sheet or threshold of its
/// own.
pub fn register_site(
    site: &skimap_common::site::Site,
    layout: &DataLayout,
    dl: &Downloader,
    planet: &Planet,
    engine: &ocrs::OcrEngine,
) -> Result<(Registration, Vec<PlanTried>)> {
    let census = crate::collect::load_census(layout)?;
    let cat = crate::collect::load_catalogue(layout)?;
    let mut plans = crate::collect::plans_for_area(&site.bbox, &cat, &census);
    plans.sort_by(|a, b| a.candidate.id.cmp(&b.candidate.id));
    let mut tried = Vec::new();
    let mut best: Option<(usize, Registration)> = None;
    for c in plans {
        let Some(hint) = crate::collect::hint(&c.candidate.domains, &census) else {
            continue;
        };
        let src = crate::collect::plan_source(c);
        match crate::autospec::register_auto(&src, hint, layout, dl, planet, engine) {
            Ok(a) => {
                let reg = a.reg;
                let draws = crate::collect::intersects(&reg.spec.extent, &site.bbox);
                let dmap = transcribe::InkDistance::new(&reg);
                let entries = transcribe::entries(&reg);
                let (ds, _) = transcribe::decide(&reg, &dmap, &entries);
                let exact = ds.iter().filter(|d| d.status.is_some()).count();
                tried.push(PlanTried {
                    id: c.candidate.id.clone(),
                    registered: true,
                    draws_site: draws,
                    exact,
                    error: None,
                });
                if draws && best.as_ref().is_none_or(|(n, _)| exact > *n) {
                    best = Some((exact, reg));
                }
            }
            Err(e) => tried.push(PlanTried {
                id: c.candidate.id.clone(),
                registered: false,
                draws_site: false,
                exact: 0,
                error: Some(e.to_string()),
            }),
        }
    }
    match best {
        Some((_, reg)) => Ok((reg, tried)),
        None => Err(Error::Data(format!(
            "no collected plan registers on {}: {}",
            site.id,
            serde_json::to_string(&tried).unwrap_or_default()
        ))),
    }
}

/// Builds the transcription of the site's plan: personal sources required (the plan is
/// the resort's), catalogue of the collected plans, planet extract and OCR models from
/// the cache.
pub fn build(
    site: &skimap_common::site::Site,
    layout: &DataLayout,
    dl: &Downloader,
    personal_sources: bool,
) -> Result<BuildReport> {
    if !personal_sources {
        return Err(Error::PersonalSourcesDisabled);
    }
    let planet = skimap_osm::extract::load_planet(dl)
        .map_err(|e| Error::Data(format!("planet extract: {e}")))?;
    let engine = ocr::engine(dl)?;
    let (reg, tried) = register_site(site, layout, dl, &planet, &engine)?;
    transcribe_and_write(&reg, site, &tried, layout, &planet.source)
}
