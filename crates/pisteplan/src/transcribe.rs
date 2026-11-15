//! Transcription of the plan's pistes onto OSM geometries, with an exact matching rule:
//! a piste of the legend is carried onto an OSM relation (or onto named OSM ways) only
//! when the names are certain and the plan draws the piste's ink over that geometry once
//! registered (drawn share above a threshold validated on negative pairs); otherwise it is
//! reported, never guessed.

use std::collections::{BTreeMap, BTreeSet, HashMap};

use rstar::primitives::{GeomWithData, Line};
use rstar::{PointDistance, RTree};
use serde::Serialize;
use skimap_common::names::name_key;

use crate::build::Registration;
use crate::labels::{fold, similarity};
use crate::osmref::OsmRef;
use crate::spec::LabelSource;
use crate::transcript::{LandmarkKind, MatchMethod, OsmStatus, PisteKind};
use crate::warp::{P2, dist, quantile};

/// Spacing of the samples along the OSM geometry, metres.
pub const SHARE_STEP_M: f64 = 10.0;
/// Smallest name similarity of an OCR reading.
pub const OCR_MIN_SIM: f64 = 0.6;
/// Smallest margin between the best and the second OCR name candidates.
pub const OCR_MARGIN: f64 = 0.15;

/// A piste of the legend (entries of the same name and ink merged).
#[derive(Debug, Clone, Serialize)]
pub struct PlanEntry {
    /// Name as read.
    pub name_read: String,
    /// Ink index.
    pub ink: Option<usize>,
    /// Lengths printed, km.
    pub lengths_km: Vec<f64>,
    /// Departures listing it.
    pub departures: Vec<String>,
    /// Box of the legend line, page points (where its departure is drawn).
    pub rect: [f64; 4],
}

/// One entry per legend line: two departures may list the same piste (« Fabrice Guy »
/// from Le Pré-Poncet and Chez Liadet) or two homonyms (« Piste débutant » at Mouthe and
/// at Chez Liadet); the geometry decides ([`candidates`]), then [`merge`] joins the
/// entries carried onto the same relations.
pub fn entries(reg: &Registration) -> Vec<PlanEntry> {
    reg.legend
        .iter()
        .map(|e| PlanEntry {
            name_read: e.name.clone(),
            ink: e.ink,
            lengths_km: e.length_km.into_iter().collect(),
            departures: e.departure.clone().into_iter().collect(),
            rect: e.rect,
        })
        .collect()
}

/// Joins the decisions of the same name, ink and OSM geometry (one piste listed by
/// several departures).
pub fn merge(ds: Vec<Decision>) -> Vec<Decision> {
    let mut out: Vec<Decision> = Vec::new();
    for d in ds {
        let same = out.iter_mut().find(|o| {
            fold(&o.entry.name_read) == fold(&d.entry.name_read)
                && o.ink == d.ink
                && o.relations == d.relations
                && o.ways == d.ways
                && o.status.is_some() == d.status.is_some()
        });
        match same {
            Some(o) => {
                o.entry.lengths_km.extend(d.entry.lengths_km);
                o.entry.departures.extend(d.entry.departures);
            }
            None => out.push(d),
        }
    }
    out
}

/// Groups relations whose ways share a node (one connected piste per group).
fn connected_groups(osm: &OsmRef, rels: &[usize]) -> Vec<Vec<usize>> {
    let nodes: Vec<std::collections::HashSet<i64>> = rels
        .iter()
        .map(|&r| {
            osm.relations[r]
                .ways
                .iter()
                .flat_map(|&w| osm.ways[w].nodes.iter().copied())
                .collect()
        })
        .collect();
    let mut group: Vec<usize> = (0..rels.len()).collect();
    for i in 0..rels.len() {
        for j in i + 1..rels.len() {
            if !nodes[i].is_disjoint(&nodes[j]) {
                let (a, b) = (group[i], group[j]);
                for g in group.iter_mut() {
                    if *g == b {
                        *g = a;
                    }
                }
            }
        }
    }
    let mut out: BTreeMap<usize, Vec<usize>> = BTreeMap::new();
    for (i, g) in group.into_iter().enumerate() {
        out.entry(g).or_default().push(rels[i]);
    }
    out.into_values().collect()
}

/// Largest angle between an OSM segment and a drawn line counted as the same line.
pub const MAX_ANGLE_DEG: f64 = 30.0;
/// Largest search radius, page points.
const MAX_RADIUS_PT: f64 = 12.0;

type InkSegment = GeomWithData<Line<[f64; 2]>, P2>;

/// The drawn lines of each ink (skeleton segments, page points, with their unit
/// direction): a sample of an OSM geometry counts as drawn only where a line of the ink
/// runs nearby *in the same direction* (crossing another line is not following it).
pub struct InkDistance {
    trees: Vec<RTree<InkSegment>>,
}

impl InkDistance {
    /// Segment trees of the inks of a registered plan.
    pub fn new(reg: &Registration) -> InkDistance {
        let k = reg.input.pt_per_px;
        let mut per_ink: Vec<Vec<InkSegment>> = vec![Vec::new(); reg.spec.inks.len()];
        for (ink, g) in &reg.graphs {
            for e in &g.edges {
                // Directions over ~3 pt to smooth the pixel staircase.
                let pts: Vec<P2> = e.pts.iter().map(|p| [p[0] * k, p[1] * k]).collect();
                let step = ((3.0 / k).round() as usize).max(1);
                let mut i = 0;
                while i + 1 < pts.len() {
                    let j = (i + step).min(pts.len() - 1);
                    let (a, b) = (pts[i], pts[j]);
                    let l = dist(a, b);
                    if l > 0.0 {
                        let u = [(b[0] - a[0]) / l, (b[1] - a[1]) / l];
                        per_ink[*ink].push(GeomWithData::new(Line::new(a, b), u));
                    }
                    i = j;
                }
            }
        }
        InkDistance {
            trees: per_ink.into_iter().map(RTree::bulk_load).collect(),
        }
    }

    /// Distance (page points) from `p` to the nearest drawn segment of `ink` whose
    /// direction is within [`MAX_ANGLE_DEG`] of `t` (unit), infinite beyond 12 pt.
    pub fn at(&self, ink: usize, p: P2, t: P2) -> f64 {
        let cos_max = MAX_ANGLE_DEG.to_radians().cos();
        self.trees[ink]
            .locate_within_distance(p, MAX_RADIUS_PT * MAX_RADIUS_PT)
            .filter(|s| (s.data[0] * t[0] + s.data[1] * t[1]).abs() >= cos_max)
            .map(|s| s.geom().distance_2(&p).sqrt())
            .fold(f64::INFINITY, f64::min)
    }

    /// Unit direction of the drawn segment of `ink` nearest to `p` within `r_pt`.
    pub fn nearest_dir(&self, ink: usize, p: P2, r_pt: f64) -> Option<P2> {
        self.trees[ink]
            .locate_within_distance(p, r_pt * r_pt)
            .min_by(|a, b| a.geom().distance_2(&p).total_cmp(&b.geom().distance_2(&p)))
            .map(|s| s.data)
    }
}

/// Share of a Lambert-93 polyline set drawn with `ink` within `r_pt` on the plan, and the
/// number of samples (every [`SHARE_STEP_M`], on the map part of the plan only).
pub fn drawn_share(
    reg: &Registration,
    d: &InkDistance,
    lines: &[&[P2]],
    ink: usize,
    r_pt: f64,
) -> (f64, usize) {
    let s = drawn_samples(reg, d, lines, ink);
    (share_within(&s, r_pt), s.len())
}

/// Share of samples within `r_pt` of the ink.
pub fn share_within(samples: &[(P2, f64)], r_pt: f64) -> f64 {
    if samples.is_empty() {
        return 0.0;
    }
    samples.iter().filter(|s| s.1 <= r_pt).count() as f64 / samples.len() as f64
}

/// Samples of [`drawn_share`]: page point and distance to the ink, page points.
pub fn drawn_samples(
    reg: &Registration,
    d: &InkDistance,
    lines: &[&[P2]],
    ink: usize,
) -> Vec<(P2, f64)> {
    let mut out = Vec::new();
    for line in lines {
        for s in line.windows(2) {
            let l = dist(s[0], s[1]);
            let steps = (l / SHARE_STEP_M).ceil().max(1.0) as usize;
            for j in 0..steps {
                let f = j as f64 / steps as f64;
                let q = [
                    s[0][0] + (s[1][0] - s[0][0]) * f,
                    s[0][1] + (s[1][1] - s[0][1]) * f,
                ];
                let p = reg.warp.to_plan(q);
                // Off the map (outside the page, legend, boxes, advertisements): the plan
                // says nothing there.
                if !reg.spec.on_map(p[0], p[1]) {
                    continue;
                }
                // Direction of the segment on the plan.
                let p2 = reg.warp.to_plan([
                    q[0] + (s[1][0] - s[0][0]) / l.max(1e-9),
                    q[1] + (s[1][1] - s[0][1]) / l.max(1e-9),
                ]);
                let tl = dist(p, p2);
                if tl <= 0.0 {
                    continue;
                }
                let t = [(p2[0] - p[0]) / tl, (p2[1] - p[1]) / tl];
                out.push((p, d.at(ink, p, t)));
            }
        }
    }
    out
}

/// Name similarity between a reading and an OSM name: exact key equality for a text
/// layer (1 or 0), best of folded and key similarities for OCR.
pub fn name_sim(src: LabelSource, read: &str, osm: &str) -> f64 {
    let (ka, kb) = (name_key(read), name_key(osm));
    match src {
        LabelSource::TextLayer => {
            if (!ka.is_empty() && ka == kb) || fold(read) == fold(osm) {
                1.0
            } else {
                0.0
            }
        }
        // OCR: the reading may carry garbage after the name (« lzoord b iom caller »):
        // best similarity of its leading words, with or without articles.
        LabelSource::Ocr => {
            let words: Vec<&str> = ka.iter().map(String::as_str).collect();
            let (fo, ko) = (fold(osm), kb.join(" "));
            let mut best = similarity(read, osm);
            for n in 1..=words.len() {
                let lead = words[..n].join(" ");
                best = best.max(similarity(&lead, &ko)).max(similarity(&lead, &fo));
            }
            best
        }
    }
}

/// Lambert-93 polylines of OSM ways.
fn way_lines<'a>(osm: &'a OsmRef, ways: &[usize]) -> Vec<&'a [P2]> {
    ways.iter().map(|&w| osm.ways[w].xy.as_slice()).collect()
}

/// Whether ways form closed chains: every end node used an even number of times.
pub fn is_loop(osm: &OsmRef, ways: &[usize]) -> bool {
    let mut ends: HashMap<i64, usize> = HashMap::new();
    for &w in ways {
        let n = &osm.ways[w].nodes;
        if let (Some(a), Some(b)) = (n.first(), n.last()) {
            *ends.entry(*a).or_default() += 1;
            *ends.entry(*b).or_default() += 1;
        }
    }
    !ends.is_empty() && ends.values().all(|c| c % 2 == 0)
}

/// Decision on one piste of the plan.
#[derive(Debug, Clone, Serialize)]
pub struct Decision {
    /// The piste of the plan.
    pub entry: PlanEntry,
    /// Ink name.
    pub ink: Option<String>,
    /// Kind of line.
    pub kind: Option<PisteKind>,
    /// Plan colour (difficulty).
    pub colour: Option<String>,
    /// Name used (plan name, or the OSM name for an OCR reading).
    pub name: String,
    /// Status against OSM; `None` when not transcribed.
    pub status: Option<OsmStatus>,
    /// Method.
    pub method: Option<MatchMethod>,
    /// Drawn share of the matched geometry.
    pub score: f64,
    /// Best drawn share of the shifted copies (permutation test).
    pub shift_max: f64,
    /// Name similarity of the chosen candidate.
    pub name_sim: f64,
    /// Matched relations (indices).
    pub relations: Vec<usize>,
    /// Ways of the geometry (indices).
    pub ways: Vec<usize>,
    /// The ways carry the name, no relation does.
    pub named_ways_only: bool,
    /// Why it was not transcribed, or remarks.
    pub reason: String,
}

/// Offsets of the shifted geometries of the permutation test, metres.
pub const NEGATIVE_SHIFTS_M: &[f64] = &[400.0, 800.0];

/// Search radii tried, page points.
pub const RADII_PT: &[f64] = &[3.0, 5.0, 8.0, 12.0];

/// Smallest drawn share of an exact match: the plan must draw at least half the
/// geometry in the piste's ink.
pub const MIN_SHARE: f64 = 0.5;

/// Name candidates of every piste of the legend (no geometric test yet): relations of
/// the same name (text layer: same key; OCR: best reading, unambiguous), else named ways.
pub fn candidates(reg: &Registration, plan: &[PlanEntry]) -> Vec<Decision> {
    let src = reg.spec.labels;
    let min_sim = match src {
        LabelSource::TextLayer => 1.0,
        LabelSource::Ocr => OCR_MIN_SIM,
    };
    plan.iter()
        .map(|e| {
            let ink = e.ink.map(|i| &reg.spec.inks[i]);
            let mut d = Decision {
                entry: e.clone(),
                ink: ink.map(|i| i.name.to_owned()),
                kind: ink.map(|i| i.kind),
                colour: ink.and_then(|i| i.difficulty.clone()),
                name: e.name_read.clone(),
                status: None,
                method: None,
                score: 0.0,
                shift_max: 0.0,
                name_sim: 0.0,
                relations: Vec::new(),
                ways: Vec::new(),
                named_ways_only: false,
                reason: String::new(),
            };
            let Some(ink_i) = e.ink else {
                d.reason = "encre de la légende non lue".into();
                return d;
            };
            let mut cands: Vec<(f64, usize)> = reg
                .osm
                .relations
                .iter()
                .enumerate()
                .filter_map(|(i, r)| {
                    let s = name_sim(src, &e.name_read, r.name.as_deref()?);
                    (s >= min_sim && !r.ways.is_empty()).then_some((s, i))
                })
                .collect();
            cands.sort_by(|a, b| b.0.total_cmp(&a.0));
            let key = |i: usize| name_key(reg.osm.relations[i].name.as_deref().unwrap_or(""));
            let chosen: Vec<(f64, usize)> = match src {
                // Same key: every relation of that name (a piste split in several).
                LabelSource::TextLayer => cands.clone(),
                LabelSource::Ocr => {
                    let best = cands.first().copied();
                    // The runner-up is the best candidate of another name.
                    let second = cands
                        .iter()
                        .find(|c| best.is_some_and(|b| key(c.1) != key(b.1)));
                    match (best, second) {
                        (Some(b), Some(s)) if b.0 - s.0 < OCR_MARGIN => {
                            d.reason = format!(
                                "lecture OCR ambiguë : « {} » ≈ « {} » ({:.2}) et « {} » ({:.2})",
                                e.name_read,
                                reg.osm.relations[b.1].name.as_deref().unwrap_or(""),
                                b.0,
                                reg.osm.relations[s.1].name.as_deref().unwrap_or(""),
                                s.0
                            );
                            return d;
                        }
                        (Some(b), _) => cands
                            .iter()
                            .copied()
                            .filter(|c| key(c.1) == key(b.1))
                            .collect(),
                        _ => Vec::new(),
                    }
                }
            };
            // A ski piste is a nordic relation when there is one of that name (« Izoard »
            // piste, not the « Route du Col d'Izoard » walking route).
            let ski = matches!(
                reg.spec.inks[ink_i].kind,
                PisteKind::Ski | PisteKind::Liaison | PisteKind::MultiActivity
            );
            let nordic = |i: usize| reg.osm.relations[i].uses.iter().any(|u| u == "nordic");
            let chosen: Vec<(f64, usize)> = if ski && chosen.iter().any(|c| nordic(c.1)) {
                chosen.into_iter().filter(|c| nordic(c.1)).collect()
            } else {
                chosen
            };
            // Homonyms: several unconnected pistes of the name; the one nearest to the
            // legend box (drawn by its departure) is this line's.
            let chosen: Vec<(f64, usize)> = {
                let rels: Vec<usize> = chosen.iter().map(|c| c.1).collect();
                let groups = connected_groups(&reg.osm, &rels);
                if groups.len() > 1 {
                    let c = reg.warp.to_map([
                        (e.rect[0] + e.rect[2]) / 2.0,
                        (e.rect[1] + e.rect[3]) / 2.0,
                    ]);
                    let gdist = |g: &Vec<usize>| {
                        g.iter()
                            .flat_map(|&r| reg.osm.relations[r].ways.iter())
                            .flat_map(|&w| reg.osm.ways[w].xy.iter())
                            .map(|p| dist(*p, c))
                            .fold(f64::INFINITY, f64::min)
                    };
                    let best = groups
                        .iter()
                        .min_by(|a, b| gdist(a).total_cmp(&gdist(b)))
                        .cloned()
                        .unwrap_or_default();
                    d.reason = format!(
                        "{} pistes OSM homonymes non reliées : celle la plus proche de l'encadré du départ",
                        groups.len()
                    );
                    chosen.into_iter().filter(|c| best.contains(&c.1)).collect()
                } else {
                    chosen
                }
            };
            if !chosen.is_empty() {
                d.relations = chosen.iter().map(|c| c.1).collect();
                let ways: BTreeSet<usize> = d
                    .relations
                    .iter()
                    .flat_map(|&r| reg.osm.relations[r].ways.iter().copied())
                    .collect();
                d.ways = ways.into_iter().collect();
                d.name_sim = chosen[0].0;
                if src == LabelSource::Ocr {
                    d.name = reg.osm.relations[chosen[0].1]
                        .name
                        .clone()
                        .unwrap_or_default();
                }
                return d;
            }
            // Named ways without relation.
            d.ways = reg
                .osm
                .ways
                .iter()
                .enumerate()
                .filter(|(_, w)| {
                    w.name
                        .as_deref()
                        .is_some_and(|n| name_sim(src, &e.name_read, n) >= min_sim.max(0.9))
                })
                .map(|(i, _)| i)
                .collect();
            d.named_ways_only = !d.ways.is_empty();
            if d.ways.is_empty() {
                d.reason = "aucune piste OSM de ce nom dans l'emprise du plan".into();
            }
            d
        })
        .collect()
}

/// Shares of one candidate geometry at every radius of [`RADII_PT`]: the geometry itself
/// and, for the permutation test, the best of its copies shifted by
/// [`NEGATIVE_SHIFTS_M`] in eight directions (unrelated to the drawing, crossing the same
/// network).
#[derive(Debug, Clone, Serialize)]
pub struct PisteTest {
    /// Share of the geometry, per radius.
    pub share: Vec<f64>,
    /// Best share of the shifted copies, per radius.
    pub shift_max: Vec<f64>,
    /// Samples of the geometry on the map part of the plan.
    pub samples: usize,
}

/// Runs the test of a candidate.
pub fn piste_test(reg: &Registration, d: &InkDistance, ways: &[usize], ink: usize) -> PisteTest {
    let truth = drawn_samples(reg, d, &way_lines(&reg.osm, ways), ink);
    let mut shift_max = vec![0.0f64; RADII_PT.len()];
    for k in 0..8 * NEGATIVE_SHIFTS_M.len() {
        let a = k as f64 * std::f64::consts::FRAC_PI_4;
        let m = NEGATIVE_SHIFTS_M[k / 8];
        let (dx, dy) = (m * a.cos(), m * a.sin());
        let shifted: Vec<Vec<P2>> = ways
            .iter()
            .map(|&w| {
                reg.osm.ways[w]
                    .xy
                    .iter()
                    .map(|p| [p[0] + dx, p[1] + dy])
                    .collect()
            })
            .collect();
        let refs: Vec<&[P2]> = shifted.iter().map(Vec::as_slice).collect();
        let s = drawn_samples(reg, d, &refs, ink);
        for (i, &r) in RADII_PT.iter().enumerate() {
            shift_max[i] = shift_max[i].max(share_within(&s, r));
        }
    }
    PisteTest {
        share: RADII_PT.iter().map(|&r| share_within(&truth, r)).collect(),
        shift_max,
        samples: truth.len(),
    }
}

/// Whether a test passes at radius index `i`: drawn share of at least [`MIN_SHARE`] and
/// above every shifted copy (rank 1 of 17: p ≤ 1/17 under the hypothesis that the
/// geometry is unrelated to the drawing).
pub fn passes(t: &PisteTest, i: usize) -> bool {
    t.samples > 0 && t.share[i] >= MIN_SHARE && t.share[i] > t.shift_max[i]
}

/// One radius of the validation.
#[derive(Debug, Clone, Serialize)]
pub struct RadiusRow {
    /// Radius, page points.
    pub radius_pt: f64,
    /// Share of the name candidates passing the test.
    pub accepted: f64,
    /// Median share of the candidates.
    pub share_median: f64,
    /// Median best share of their shifted copies.
    pub shift_median: f64,
}

/// Validated test: the radius accepting the most name candidates (each acceptance is a
/// permutation test at p ≤ 1/17).
#[derive(Debug, Clone, Serialize)]
pub struct Validation {
    /// Chosen radius, page points.
    pub radius_pt: f64,
    /// Candidates tested.
    pub candidates: usize,
    /// Every radius tried.
    pub rows: Vec<RadiusRow>,
}

/// Chooses the radius.
pub fn validate(tests: &[PisteTest]) -> (usize, Validation) {
    let rows: Vec<RadiusRow> = RADII_PT
        .iter()
        .enumerate()
        .map(|(i, &r)| {
            let sh: Vec<f64> = tests.iter().map(|t| t.share[i]).collect();
            let sm: Vec<f64> = tests.iter().map(|t| t.shift_max[i]).collect();
            RadiusRow {
                radius_pt: r,
                accepted: tests.iter().filter(|t| passes(t, i)).count() as f64
                    / tests.len().max(1) as f64,
                share_median: quantile(&sh, 0.5).unwrap_or(0.0),
                shift_median: quantile(&sm, 0.5).unwrap_or(0.0),
            }
        })
        .collect();
    let best = (0..rows.len())
        .max_by(|&a, &b| {
            rows[a]
                .accepted
                .total_cmp(&rows[b].accepted)
                .then(rows[b].radius_pt.total_cmp(&rows[a].radius_pt))
        })
        .unwrap_or(0);
    (
        best,
        Validation {
            radius_pt: RADII_PT[best],
            candidates: tests.len(),
            rows,
        },
    )
}

/// Matches every piste of the legend: name candidates, tests, validated radius, status.
pub fn decide(
    reg: &Registration,
    dmap: &InkDistance,
    plan: &[PlanEntry],
) -> (Vec<Decision>, Validation) {
    let mut ds = candidates(reg, plan);
    let tests: Vec<Option<PisteTest>> = ds
        .iter()
        .map(|d| match (d.entry.ink, d.ways.is_empty()) {
            (Some(ink), false) => Some(piste_test(reg, dmap, &d.ways, ink)),
            _ => None,
        })
        .collect();
    let all: Vec<PisteTest> = tests.iter().flatten().cloned().collect();
    let (ri, v) = validate(&all);
    for (d, t) in ds.iter_mut().zip(&tests) {
        let Some(t) = t else { continue };
        d.score = t.share[ri];
        d.shift_max = t.shift_max[ri];
        if passes(t, ri) {
            d.method = Some(MatchMethod::Exact);
            d.status = Some(if d.named_ways_only {
                OsmStatus::NameMissing
            } else {
                let osm_colours: BTreeSet<&str> = d
                    .relations
                    .iter()
                    .filter_map(|&r| reg.osm.relations[r].colour.as_deref())
                    .collect();
                match (d.colour.as_deref(), osm_colours.len()) {
                    (None, _) => OsmStatus::Match,
                    (Some(_), 0) => OsmStatus::ColourMissing,
                    (Some(c), _) if osm_colours.contains(c) => OsmStatus::Match,
                    _ => OsmStatus::ColourDiffers,
                }
            });
        } else {
            d.reason = format!(
                "géométrie OSM du même nom : le plan y dessine l'encre {} sur {:.0} % (à {} pt, même sens), copies décalées jusqu'à {:.0} % : test non concluant (au moins {:.0} % et plus que toutes les copies)",
                d.ink.as_deref().unwrap_or("?"),
                100.0 * t.share[ri],
                RADII_PT[ri],
                100.0 * t.shift_max[ri],
                100.0 * MIN_SHARE
            );
        }
    }
    (merge(ds), v)
}

/// A landmark of the plan matched to OSM.
#[derive(Debug, Clone, Serialize)]
pub struct LandmarkMatch {
    /// Label as read.
    pub label: String,
    /// Landmark index.
    pub landmark: usize,
    /// Kind (departures from the legend's box titles).
    pub kind: LandmarkKind,
    /// Elevation printed.
    pub ele: Option<f64>,
    /// Label centre carried onto the map vs landmark, m.
    pub residual_m: f64,
}

/// Landmarks: label ↔ landmark pairs kept by the RANSAC (one per label, consistent with
/// the registration), plus the departures of the legend matched by name.
pub fn landmarks(reg: &Registration) -> Vec<LandmarkMatch> {
    let mut out: Vec<LandmarkMatch> = reg
        .coarse
        .inliers
        .iter()
        .map(|c| {
            let lm = &reg.osm.landmarks[c.landmark];
            LandmarkMatch {
                label: c.label.clone(),
                landmark: c.landmark,
                kind: lm.kind,
                ele: c.ele,
                residual_m: dist(reg.warp.to_map(c.plan), lm.xy),
            }
        })
        .collect();
    let departures: BTreeSet<String> = reg
        .legend
        .iter()
        .filter_map(|e| e.departure.clone())
        .collect();
    let min_sim = match reg.spec.labels {
        LabelSource::TextLayer => 0.92,
        LabelSource::Ocr => 0.8,
    };
    for dep in departures {
        // The departure's own label on the map, if any, else the nearest landmark of the
        // name to the plan's centre is not guessed: only a unique name match is kept.
        let m: Vec<usize> = reg
            .osm
            .landmarks
            .iter()
            .enumerate()
            .filter(|(_, l)| similarity(&dep, &l.name) >= min_sim)
            .map(|(i, _)| i)
            .collect();
        let places: Vec<usize> = m
            .iter()
            .copied()
            .filter(|&i| reg.osm.landmarks[i].kind == LandmarkKind::Place)
            .collect();
        let pick = if places.len() == 1 {
            Some(places[0])
        } else if m.len() == 1 {
            Some(m[0])
        } else {
            None
        };
        if let Some(i) = pick {
            if let Some(o) = out.iter_mut().find(|o| o.landmark == i) {
                o.kind = LandmarkKind::Departure;
            } else {
                out.push(LandmarkMatch {
                    label: dep.clone(),
                    landmark: i,
                    kind: LandmarkKind::Departure,
                    ele: None,
                    residual_m: f64::NAN,
                });
            }
        }
    }
    out
}

/// Counts per status, for the report.
pub fn status_counts(ds: &[Decision]) -> BTreeMap<String, usize> {
    let mut m = BTreeMap::new();
    for d in ds {
        let k = match d.status {
            Some(s) => serde_json::to_value(s)
                .ok()
                .and_then(|v| v.as_str().map(str::to_owned))
                .unwrap_or_default(),
            None => "non_transcrite".to_owned(),
        };
        *m.entry(k).or_default() += 1;
    }
    m
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::osmref::OsmWay;
    use skimap_osm::pistes::Oneway;

    fn way(id: i64, nodes: &[i64]) -> OsmWay {
        OsmWay {
            id,
            nodes: nodes.to_vec(),
            ll: Vec::new(),
            xy: Vec::new(),
            uses: vec!["nordic".into()],
            name: None,
            relations: Vec::new(),
            colour: None,
            oneway: Oneway::Unknown,
            length_m: 0.0,
        }
    }

    #[test]
    fn loops_are_closed_chains() {
        let osm = OsmRef {
            ways: vec![way(1, &[1, 2, 3]), way(2, &[3, 4, 1]), way(3, &[3, 5])],
            ..Default::default()
        };
        assert!(is_loop(&osm, &[0, 1]));
        assert!(!is_loop(&osm, &[0, 1, 2]));
        assert!(!is_loop(&osm, &[2]));
    }

    #[test]
    fn names_are_exact_on_a_text_layer_and_tolerant_on_ocr() {
        let t = LabelSource::TextLayer;
        assert_eq!(name_sim(t, "Le Pré d’Haut", "Le Pré d'Haut"), 1.0);
        assert_eq!(name_sim(t, "La Loge", "La Loge"), 1.0);
        assert_eq!(name_sim(t, "La Loge", "Les Loges"), 0.0);
        let o = LabelSource::Ocr;
        assert!(name_sim(o, "la pinatelle", "La Pinatelle") > 0.99);
        assert!(name_sim(o, "lzoord b iom caller", "Izoard") >= OCR_MIN_SIM);
        assert!(name_sim(o, "le chemindu sioun", "Le Chemin du Sioux") > 0.8);
        assert!(name_sim(o, "le cancel", "Le Gibera") < OCR_MIN_SIM);
    }

    #[test]
    fn the_permutation_test_needs_coverage_and_rank_one() {
        let t = PisteTest {
            share: vec![0.3, 0.6, 0.8, 0.9],
            shift_max: vec![0.1, 0.4, 0.85, 0.95],
            samples: 50,
        };
        assert!(!passes(&t, 0), "below half drawn");
        assert!(passes(&t, 1));
        assert!(!passes(&t, 2), "a shifted copy does better");
        let (best, v) = validate(&[t.clone(), t]);
        assert_eq!(best, 1);
        assert_eq!(v.radius_pt, RADII_PT[1]);
        assert_eq!(v.rows[1].accepted, 1.0);
        assert_eq!(
            share_within(&[([0.0, 0.0], 1.0), ([0.0, 0.0], 9.0)], 5.0),
            0.5
        );
    }
}
