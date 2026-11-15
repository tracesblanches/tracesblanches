//! Registration of a plan on the map.
//!
//! 1. Labels ↔ OSM landmarks (same folded name, same printed elevation when both have
//!    one) give candidate correspondences; an exhaustive RANSAC over pairs of them finds
//!    the similarity (rotation ≤ 45°, no reflection) with the most labels within
//!    15 page points (~5 mm on paper), refined as an affine map on its inliers.
//! 2. Non-rigid ICP: points sampled on the drawn lines are paired with the nearest OSM
//!    piste point of a compatible colour, one pair per cell of a plan grid, and a
//!    thin-plate spline is refitted with a shrinking search radius and smoothing.
//! 3. Tie points: junctions of the drawing (lines of one ink branching) paired with OSM
//!    junctions when they are mutual nearest neighbours, unambiguous (second candidate at
//!    least twice as far) and of a compatible colour.
//!
//! Plan coordinates are page points with y down; models work on `[x, −y]` so that a
//! north-up plan maps to Lambert-93 without reflection.

use rayon::prelude::*;
use rstar::{RTree, primitives::GeomWithData};
use serde::Serialize;

use crate::input::TextBox;
use crate::labels::{fold, similarity, split_elevation};
use crate::osmref::{Landmark, OsmRef};
use crate::skeleton::SkelGraph;
use crate::spec::PlanSpec;
use crate::transcript::PisteKind;
use crate::warp::{Affine, ModelKind, P2, Tps, dist, leave_one_out, quantile};

/// Page point (y down) to model frame (y up).
pub fn up(p: P2) -> P2 {
    [p[0], -p[1]]
}

/// Model frame to page point.
pub fn down(q: P2) -> P2 {
    [q[0], -q[1]]
}

/// A label ↔ landmark candidate.
#[derive(Debug, Clone, Serialize)]
pub struct LabelMatch {
    /// Label text.
    pub label: String,
    /// Index of the label.
    pub label_idx: usize,
    /// Index of the landmark in `OsmRef::landmarks`.
    pub landmark: usize,
    /// Label centre, page points.
    pub plan: P2,
    /// Landmark, Lambert-93.
    pub map: P2,
    /// Name similarity, 0 to 1.
    pub sim: f64,
    /// Elevation printed on the label, m.
    pub ele: Option<f64>,
}

/// Candidate correspondences between labels and landmarks: folded names with a
/// similarity of at least `min_sim`, rejected when both elevations are known and differ
/// by more than `ele_tol` metres (`None`: elevations not compared, OCR digits are
/// unreliable).
pub fn label_candidates(
    labels: &[TextBox],
    lms: &[Landmark],
    min_sim: f64,
    ele_tol: Option<f64>,
) -> Vec<LabelMatch> {
    let mut out = Vec::new();
    for (i, l) in labels.iter().enumerate() {
        let (name, ele) = split_elevation(&l.text);
        let f = fold(&name);
        if f.chars().filter(|c| c.is_alphabetic()).count() < 3 {
            continue;
        }
        for (k, m) in lms.iter().enumerate() {
            let s = similarity(&f, &m.name);
            if s < min_sim {
                continue;
            }
            if let (Some(a), Some(b), Some(tol)) = (ele, m.ele, ele_tol)
                && (a - b).abs() > tol
            {
                continue;
            }
            out.push(LabelMatch {
                label: l.text.clone(),
                label_idx: i,
                landmark: k,
                plan: l.center(),
                map: m.xy,
                sim: s,
                ele,
            });
        }
    }
    out
}

/// Result of the label RANSAC.
#[derive(Debug, Clone)]
pub struct Coarse {
    /// Affine map plan (model frame) → Lambert-93.
    pub affine: Affine,
    /// Inlier candidates (one per label).
    pub inliers: Vec<LabelMatch>,
    /// Metres per page point.
    pub scale: f64,
}

fn count_inliers(a: &Affine, cands: &[LabelMatch], tol_m: f64) -> (usize, Vec<usize>) {
    // Best candidate per label within tolerance.
    let mut best: std::collections::BTreeMap<usize, (f64, usize)> = Default::default();
    for (i, c) in cands.iter().enumerate() {
        let r = dist(a.apply(up(c.plan)), c.map);
        if r <= tol_m {
            let e = best.entry(c.label_idx).or_insert((f64::MAX, i));
            if r < e.0 {
                *e = (r, i);
            }
        }
    }
    // One landmark per label and one label per landmark.
    let mut used = std::collections::BTreeSet::new();
    let mut idx: Vec<(f64, usize)> = best.values().copied().collect();
    idx.sort_by(|a, b| a.0.total_cmp(&b.0));
    let mut out = Vec::new();
    for (_, i) in idx {
        if used.insert(cands[i].landmark) {
            out.push(i);
        }
    }
    (out.len(), out)
}

/// Exhaustive RANSAC over pairs of candidates (similarity), then affine refinement.
pub fn coarse(cands: &[LabelMatch], tol_pt: f64) -> Option<Coarse> {
    let n = cands.len();
    if n < 2 {
        return None;
    }
    let pairs: Vec<(usize, usize)> = (0..n)
        .flat_map(|i| (i + 1..n).map(move |j| (i, j)))
        .filter(|&(i, j)| {
            cands[i].label_idx != cands[j].label_idx
                && cands[i].landmark != cands[j].landmark
                && dist(cands[i].plan, cands[j].plan) > 40.0
        })
        .collect();
    let best = pairs
        .par_iter()
        .filter_map(|&(i, j)| {
            let a = Affine::fit_similarity(
                &[up(cands[i].plan), up(cands[j].plan)],
                &[cands[i].map, cands[j].map],
            )?;
            let s = a.scale();
            let theta = a.m[1][1].atan2(a.m[0][1]).to_degrees();
            if theta.abs() > 45.0 || !(0.5..500.0).contains(&s) {
                return None;
            }
            let (k, _) = count_inliers(&a, cands, tol_pt * s);
            Some((k, i, j, a))
        })
        .max_by(|x, y| x.0.cmp(&y.0).then((y.1, y.2).cmp(&(x.1, x.2))))?;
    let (_, _, _, mut a) = best;
    let mut inl = Vec::new();
    for round in 0..6 {
        let s = a.scale();
        let (_, idx) = count_inliers(&a, cands, tol_pt * s);
        if idx.len() < 3 {
            break;
        }
        let src: Vec<P2> = idx.iter().map(|&i| up(cands[i].plan)).collect();
        let dst: Vec<P2> = idx.iter().map(|&i| cands[i].map).collect();
        let next = if round < 2 || idx.len() < 6 {
            Affine::fit_similarity(&src, &dst)
        } else {
            Affine::fit(&src, &dst)
        };
        inl = idx;
        match next {
            Some(n) => a = n,
            None => break,
        }
    }
    let s = a.scale();
    let (_, idx) = count_inliers(&a, cands, tol_pt * s);
    if idx.len() >= inl.len() {
        inl = idx;
    }
    Some(Coarse {
        affine: a,
        inliers: inl.iter().map(|&i| cands[i].clone()).collect(),
        scale: s,
    })
}

/// A point of a drawn line, page points, with its ink.
#[derive(Debug, Clone, Copy)]
pub struct DrawnPoint {
    /// Position, page points.
    pub p: P2,
    /// Ink index.
    pub ink: usize,
}

/// Samples the edges of the skeleton graphs every `step_pt` page points.
pub fn sample_lines(
    graphs: &[(usize, SkelGraph)],
    pt_per_px: f64,
    step_pt: f64,
) -> Vec<DrawnPoint> {
    let mut out = Vec::new();
    for (ink, g) in graphs {
        for e in &g.edges {
            let (mut next, mut done) = (0.0, 0.0);
            for w in e.pts.windows(2) {
                let a = [w[0][0] * pt_per_px, w[0][1] * pt_per_px];
                let b = [w[1][0] * pt_per_px, w[1][1] * pt_per_px];
                let l = dist(a, b);
                if l <= 0.0 {
                    continue;
                }
                while next <= done + l {
                    let f = (next - done) / l;
                    out.push(DrawnPoint {
                        p: [a[0] + (b[0] - a[0]) * f, a[1] + (b[1] - a[1]) * f],
                        ink: *ink,
                    });
                    next += step_pt;
                }
                done += l;
            }
        }
    }
    out
}

/// OSM piste points (Lambert-93, every `step_m`), with the way index.
pub type OsmPoint = GeomWithData<[f64; 2], usize>;

/// R-tree of OSM piste points densified every `step_m` metres.
pub fn osm_tree(osm: &OsmRef, step_m: f64) -> RTree<OsmPoint> {
    let mut pts = Vec::new();
    for (wi, w) in osm.ways.iter().enumerate() {
        for s in w.xy.windows(2) {
            let l = dist(s[0], s[1]);
            let k = (l / step_m).ceil().max(1.0) as usize;
            for j in 0..k {
                let f = j as f64 / k as f64;
                pts.push(GeomWithData::new(
                    [
                        s[0][0] + (s[1][0] - s[0][0]) * f,
                        s[0][1] + (s[1][1] - s[0][1]) * f,
                    ],
                    wi,
                ));
            }
        }
        if let Some(&l) = w.xy.last() {
            pts.push(GeomWithData::new(l, wi));
        }
    }
    RTree::bulk_load(pts)
}

/// Whether an OSM way can be the line of an ink.
pub fn compatible(spec: &PlanSpec, ink: usize, osm: &OsmRef, way: usize) -> bool {
    // Ski inks follow nordic ways only; walkers' and snowshoe inks the other ways.
    let nordic = osm.ways[way].uses.iter().any(|u| u == "nordic");
    let ski = matches!(
        spec.inks[ink].kind,
        PisteKind::Ski | PisteKind::Liaison | PisteKind::MultiActivity
    );
    if ski != nordic {
        return false;
    }
    match (spec.inks[ink].difficulty, osm.ways[way].colour.as_deref()) {
        (Some(d), Some(c)) => {
            d == c
                || osm.ways[way]
                    .relations
                    .iter()
                    .any(|&r| osm.relations[r].colour.as_deref() == Some(d))
        }
        _ => true,
    }
}

/// A plan ↔ map pair.
#[derive(Debug, Clone, Copy, Serialize)]
pub struct Pair {
    /// Page point.
    pub plan: P2,
    /// Lambert-93.
    pub map: P2,
}

/// A fitted plan → map warp and its inverse.
#[derive(Debug, Clone)]
pub struct Warp {
    /// Plan (model frame) → Lambert-93.
    pub fwd: Tps,
    /// Lambert-93 → plan (model frame).
    pub inv: Tps,
}

impl Warp {
    /// Page point → Lambert-93.
    pub fn to_map(&self, p: P2) -> P2 {
        self.fwd.apply(up(p))
    }

    /// Lambert-93 → page point.
    pub fn to_plan(&self, q: P2) -> P2 {
        down(self.inv.apply(q))
    }

    /// Fits both directions on pairs.
    pub fn fit(pairs: &[Pair], lambda: f64) -> Option<Warp> {
        let src: Vec<P2> = pairs.iter().map(|p| up(p.plan)).collect();
        let dst: Vec<P2> = pairs.iter().map(|p| p.map).collect();
        Some(Warp {
            fwd: Tps::fit(&src, &dst, lambda)?,
            inv: Tps::fit(&dst, &src, lambda)?,
        })
    }
}

/// Pairs drawn points with the nearest compatible OSM point within `radius_m`, one per
/// `cell_pt` grid cell (the pair of smallest residual), dropping residuals above three
/// times the median or half the radius, whichever is larger (a part of the plan drawn far
/// from OSM is still pulled in while the radius shrinks).
pub fn icp_pairs(
    spec: &PlanSpec,
    drawn: &[DrawnPoint],
    osm: &OsmRef,
    tree: &RTree<OsmPoint>,
    to_map: &(dyn Fn(P2) -> P2 + Sync),
    radius_m: f64,
    cell_pt: f64,
) -> Vec<Pair> {
    let found: Vec<(i64, i64, f64, Pair)> = drawn
        .par_iter()
        .filter_map(|d| {
            let q = to_map(d.p);
            let r2 = radius_m * radius_m;
            let hit = tree
                .nearest_neighbor_iter(&q)
                .take_while(|o| {
                    let g = o.geom();
                    (g[0] - q[0]).powi(2) + (g[1] - q[1]).powi(2) <= r2
                })
                .find(|o| compatible(spec, d.ink, osm, o.data))?;
            let g = *hit.geom();
            Some((
                (d.p[0] / cell_pt).floor() as i64,
                (d.p[1] / cell_pt).floor() as i64,
                dist(g, q),
                Pair { plan: d.p, map: g },
            ))
        })
        .collect();
    let mut best: std::collections::HashMap<(i64, i64), (f64, Pair)> = Default::default();
    for (cx, cy, r, p) in found {
        let e = best.entry((cx, cy)).or_insert((f64::MAX, p));
        if r < e.0 {
            *e = (r, p);
        }
    }
    let rs: Vec<f64> = best.values().map(|v| v.0).collect();
    let med = quantile(&rs, 0.5).unwrap_or(0.0);
    let mut out: Vec<(i64, i64, Pair)> = best
        .into_iter()
        .filter(|(_, (r, _))| *r <= (3.0 * med).max(0.5 * radius_m))
        .map(|(k, (_, p))| (k.0, k.1, p))
        .collect();
    out.sort_by_key(|(x, y, _)| (*y, *x));
    out.into_iter().map(|(_, _, p)| p).collect()
}

/// One ICP iteration record.
#[derive(Debug, Clone, Serialize)]
pub struct IcpStep {
    /// Search radius, m.
    pub radius_m: f64,
    /// Smoothing of the spline.
    pub lambda: f64,
    /// Pairs used.
    pub pairs: usize,
    /// Median residual of the pairs before the fit, m.
    pub median_m: f64,
}

/// Non-rigid ICP from the coarse affine map. Returns the final warp, its pairs and the
/// iterations.
pub fn icp(
    spec: &PlanSpec,
    drawn: &[DrawnPoint],
    osm: &OsmRef,
    tree: &RTree<OsmPoint>,
    coarse: &Affine,
    schedule: &[(f64, f64)],
    cell_pt: f64,
) -> Option<(Warp, Vec<Pair>, Vec<IcpStep>)> {
    let mut warp: Option<Warp> = None;
    let mut steps = Vec::new();
    let mut last_pairs = Vec::new();
    for &(radius_m, lambda) in schedule {
        for _ in 0..2 {
            let pairs = {
                let f = |p: P2| match &warp {
                    Some(w) => w.to_map(p),
                    None => coarse.apply(up(p)),
                };
                icp_pairs(spec, drawn, osm, tree, &f, radius_m, cell_pt)
            };
            if pairs.len() < 10 {
                return None;
            }
            let res: Vec<f64> = pairs
                .iter()
                .map(|p| match &warp {
                    Some(w) => dist(w.to_map(p.plan), p.map),
                    None => dist(coarse.apply(up(p.plan)), p.map),
                })
                .collect();
            steps.push(IcpStep {
                radius_m,
                lambda,
                pairs: pairs.len(),
                median_m: quantile(&res, 0.5).unwrap_or(f64::NAN),
            });
            warp = Some(Warp::fit(&pairs, lambda)?);
            last_pairs = pairs;
        }
    }
    Some((warp?, last_pairs, steps))
}

/// A tie point: a junction of the drawing paired with an OSM junction.
#[derive(Debug, Clone, Serialize)]
pub struct Tie {
    /// Page point.
    pub plan: P2,
    /// Lambert-93.
    pub map: P2,
    /// OSM node id.
    pub osm_node: i64,
    /// Ink of the drawn junction.
    pub ink: String,
    /// Distance after the warp, m.
    pub residual_m: f64,
    /// Distance to the second nearest OSM junction, m.
    pub second_m: f64,
}

/// Junction tie points: junctions (degree ≥ 3) of the skeleton of all the piste inks
/// together, and OSM junctions (nodes of degree ≥ 3, those closer than 15 m merged), that
/// are mutual nearest neighbours within `max_m` after the warp, with the second OSM
/// candidate at least twice as far (and 30 m away at least).
pub fn junction_ties(
    union: &SkelGraph,
    pt_per_px: f64,
    osm: &OsmRef,
    warp: &Warp,
    max_m: f64,
) -> Vec<Tie> {
    let d = union.degrees();
    let drawn: Vec<P2> = union
        .nodes
        .iter()
        .enumerate()
        .filter(|(n, _)| d[*n] >= 3)
        .map(|(_, p)| [p[0] * pt_per_px, p[1] * pt_per_px])
        .collect();
    // OSM junction clusters (15 m).
    let mut junc: Vec<(i64, P2, usize)> = Vec::new();
    for (&id, &p) in &osm.junctions {
        match junc.iter_mut().find(|(_, q, _)| dist(*q, p) < 15.0) {
            Some(c) => {
                let k = c.2 as f64;
                c.1 = [
                    (c.1[0] * k + p[0]) / (k + 1.0),
                    (c.1[1] * k + p[1]) / (k + 1.0),
                ];
                c.2 += 1;
                c.0 = c.0.min(id);
            }
            None => junc.push((id, p, 1)),
        }
    }
    if junc.is_empty() {
        return Vec::new();
    }
    let mapped: Vec<P2> = drawn.iter().map(|p| warp.to_map(*p)).collect();
    let mut ties = Vec::new();
    for (i, q) in mapped.iter().enumerate() {
        let mut dd: Vec<(f64, usize)> = junc
            .iter()
            .enumerate()
            .map(|(k, (_, p, _))| (dist(*p, *q), k))
            .collect();
        dd.sort_by(|a, b| a.0.total_cmp(&b.0));
        let (d1, k) = dd[0];
        let d2 = dd.get(1).map_or(f64::MAX, |x| x.0);
        if d1 > max_m || d2 < (2.0 * d1).max(30.0) {
            continue;
        }
        let p = junc[k].1;
        let mutual = mapped
            .iter()
            .enumerate()
            .all(|(j, m)| j == i || dist(*m, p) > d1);
        if !mutual {
            continue;
        }
        ties.push(Tie {
            plan: drawn[i],
            map: p,
            osm_node: junc[k].0,
            ink: "all".to_owned(),
            residual_m: d1,
            second_m: d2,
        });
    }
    ties
}

/// Leave-one-out error statistics of one model kind on tie points.
#[derive(Debug, Clone, Serialize)]
pub struct LooStats {
    /// Model.
    pub model: String,
    /// Tie points.
    pub n: usize,
    /// Median error, m.
    pub median_m: Option<f64>,
    /// 90th percentile, m.
    pub p90_m: Option<f64>,
    /// Maximum, m.
    pub max_m: Option<f64>,
}

/// Leave-one-out errors of several models on the tie points.
pub fn compare_models(ties: &[(P2, P2)], kinds: &[ModelKind]) -> Vec<LooStats> {
    let src: Vec<P2> = ties.iter().map(|t| up(t.0)).collect();
    let dst: Vec<P2> = ties.iter().map(|t| t.1).collect();
    kinds
        .par_iter()
        .map(|&k| {
            let e: Vec<f64> = leave_one_out(k, &src, &dst).into_iter().flatten().collect();
            LooStats {
                model: k.label(),
                n: e.len(),
                median_m: quantile(&e, 0.5),
                p90_m: quantile(&e, 0.9),
                max_m: quantile(&e, 1.0),
            }
        })
        .collect()
}

/// Held-out error of the dense warp at each tie point: the spline is refitted without the
/// ICP pairs within `hole_pt` of the tie point, and its prediction compared with the OSM
/// junction.
pub fn held_out(pairs: &[Pair], ties: &[Tie], lambda: f64, hole_pt: f64) -> Vec<Option<f64>> {
    ties.par_iter()
        .map(|t| {
            let kept: Vec<Pair> = pairs
                .iter()
                .copied()
                .filter(|p| dist(p.plan, t.plan) > hole_pt)
                .collect();
            let src: Vec<P2> = kept.iter().map(|p| up(p.plan)).collect();
            let dst: Vec<P2> = kept.iter().map(|p| p.map).collect();
            Tps::fit(&src, &dst, lambda).map(|f| dist(f.apply(up(t.plan)), t.map))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cand(i: usize, k: usize, plan: P2, map: P2) -> LabelMatch {
        LabelMatch {
            label: format!("l{i}"),
            label_idx: i,
            landmark: k,
            plan,
            map,
            sim: 1.0,
            ele: None,
        }
    }

    #[test]
    fn ransac_recovers_the_similarity_despite_outliers() {
        // Plan in page points (y down), map rotated by 10°, 17 m/pt.
        let (c, s) = (10f64.to_radians().cos(), 10f64.to_radians().sin());
        let f = |p: P2| {
            let q = up(p);
            [
                900_000.0 + 17.0 * (c * q[0] - s * q[1]),
                6_600_000.0 + 17.0 * (s * q[0] + c * q[1]),
            ]
        };
        let mut cands = Vec::new();
        for i in 0..12 {
            let p = [100.0 + 70.0 * (i % 4) as f64, 80.0 + 90.0 * (i / 4) as f64];
            // Labels written beside their feature: a few points of offset.
            let off = [((i * 7) % 5) as f64 - 2.0, ((i * 3) % 5) as f64 - 2.0];
            cands.push(cand(i, i, [p[0] + off[0], p[1] + off[1]], f(p)));
        }
        // Homonyms far away: wrong candidates of the same labels.
        for i in 0..6 {
            let m = [910_000.0 + 3000.0 * i as f64, 6_590_000.0];
            cands.push(cand(i, 100 + i, [100.0, 80.0], m));
        }
        let co = coarse(&cands, 15.0).unwrap();
        assert_eq!(co.inliers.len(), 12);
        assert!(co.inliers.iter().all(|c| c.landmark < 100));
        assert!((co.scale - 17.0).abs() < 0.5, "{}", co.scale);
        let e = dist(co.affine.apply(up([250.0, 200.0])), f([250.0, 200.0]));
        assert!(e < 60.0, "{e}");
    }

    #[test]
    fn line_samples_are_evenly_spaced() {
        let g = SkelGraph {
            nodes: vec![[0.0, 0.0], [100.0, 0.0]],
            edges: vec![crate::skeleton::SkelEdge {
                a: 0,
                b: 1,
                pts: vec![[0.0, 0.0], [40.0, 0.0], [100.0, 0.0]],
            }],
        };
        let s = sample_lines(&[(2, g)], 0.5, 10.0);
        // 100 px × 0.5 pt/px = 50 pt: samples at 0, 10, …, 50.
        assert_eq!(s.len(), 6);
        assert!(s.iter().all(|d| d.ink == 2));
        assert!((s[3].p[0] - 30.0).abs() < 1e-9);
    }
}
