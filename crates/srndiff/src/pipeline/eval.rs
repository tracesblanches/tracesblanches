//! Agreement with published ratings: confusion matrices, calibration of a few rule
//! parameters on a small grid, validation leaving one route out at a time (all the
//! references of the same route, whatever the source, leave together).

use rayon::prelude::*;
use serde::{Deserialize, Serialize};

use crate::analysis::{self, Direction, Measure, Metrics};
use crate::pipeline::refs::RefRoute;
use crate::profile::Profile;
use crate::rules::{self, Effort, Level, Params};

/// One rated reference with its profile.
pub struct Case<'a> {
    /// Reference.
    pub route: &'a RefRoute,
    /// Profile.
    pub profile: &'a Profile,
    /// Group (same route across sources).
    pub group: usize,
}

/// Agreement figures of one scale.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Agreement {
    /// Scale.
    pub scale: String,
    /// Number of ratings compared.
    pub n: usize,
    /// Exact agreement.
    pub exact: f64,
    /// At most one level apart.
    pub within_one: f64,
    /// Mean absolute difference, levels.
    pub mae: f64,
    /// Linearly weighted Cohen kappa.
    pub kappa_w: f64,
    /// `matrix[published - 1][predicted - 1]` (predicted folded on the scale).
    pub matrix: Vec<Vec<usize>>,
}

/// Number of levels of a scale.
pub fn scale_levels(scale: &str) -> usize {
    if scale == "srn_com" { 3 } else { 4 }
}

/// Predicted level folded on a scale (« Difficile » of skirandonneenordique.com covers
/// difficile and très difficile).
pub fn fold(scale: &str, level: u8) -> u8 {
    level.min(scale_levels(scale) as u8)
}

/// Agreement from (scale, published, predicted) triples of one scale.
pub fn agreement(scale: &str, pairs: &[(u8, u8)]) -> Agreement {
    let k = scale_levels(scale);
    let mut m = vec![vec![0usize; k]; k];
    for (p, q) in pairs {
        m[usize::from(*p) - 1][usize::from(fold(scale, *q)) - 1] += 1;
    }
    let n = pairs.len();
    let nf = n.max(1) as f64;
    let exact = (0..k).map(|i| m[i][i]).sum::<usize>() as f64 / nf;
    let within = (0..k)
        .flat_map(|i| (0..k).map(move |j| (i, j)))
        .filter(|(i, j)| i.abs_diff(*j) <= 1)
        .map(|(i, j)| m[i][j])
        .sum::<usize>() as f64
        / nf;
    let mae = pairs
        .iter()
        .map(|(p, q)| f64::from(p.abs_diff(fold(scale, *q))))
        .sum::<f64>()
        / nf;
    // Linear weights w = 1 - |i - j| / (k - 1).
    let w = |i: usize, j: usize| 1.0 - i.abs_diff(j) as f64 / (k - 1) as f64;
    let rows: Vec<f64> = (0..k).map(|i| m[i].iter().sum::<usize>() as f64).collect();
    let cols: Vec<f64> = (0..k)
        .map(|j| (0..k).map(|i| m[i][j]).sum::<usize>() as f64)
        .collect();
    let (mut po, mut pe) = (0.0, 0.0);
    for i in 0..k {
        for j in 0..k {
            po += w(i, j) * m[i][j] as f64 / nf;
            pe += w(i, j) * rows[i] * cols[j] / (nf * nf);
        }
    }
    let kappa_w = if (1.0 - pe).abs() > 1e-12 {
        (po - pe) / (1.0 - pe)
    } else {
        0.0
    };
    Agreement {
        scale: scale.to_owned(),
        n,
        exact,
        within_one: within,
        mae,
        kappa_w,
        matrix: m,
    }
}

/// Calibration grid: window of the sustained angle, forest rule, side hill, length of a
/// coarse segment, effort, shift of the slope bounds of open terrain, floor of « très
/// exposé ».
pub fn grid() -> Vec<Params> {
    let forests: [(Option<f32>, [f32; 2]); 4] = [
        (None, [8.0, 15.0]),
        (Some(70.0), [8.0, 15.0]),
        (Some(90.0), [8.0, 15.0]),
        (Some(70.0), [12.0, 20.0]),
    ];
    let mut out = Vec::new();
    for sustain in [30.0, 50.0, 100.0, 200.0] {
        for (forest, forest_deg) in forests {
            for sidehill in [None, Some([25.0, 30.0]), Some([28.0, 33.0])] {
                for coarse in [40.0, 60.0, 100.0, f64::INFINITY] {
                    for effort in [
                        Effort::Scale,
                        Effort::Scaled(1.5),
                        Effort::Scaled(2.0),
                        Effort::RaidOnly,
                    ] {
                        for shift in [-1.5f32, 0.0, 1.5, 3.0, 4.5, 6.0] {
                            for high in [Level::Moyen, Level::Difficile] {
                                let d = Params::scale();
                                out.push(Params {
                                    high_exposure_floor: high,
                                    sustain_m: sustain,
                                    forest_cover_pct: forest,
                                    forest_deg,
                                    sidehill_deg: sidehill,
                                    coarse_m: coarse,
                                    effort,
                                    open_deg: [
                                        d.open_deg[0] + shift,
                                        d.open_deg[1] + shift,
                                        d.open_deg[2] + shift,
                                    ],
                                    ..d
                                });
                            }
                        }
                    }
                }
            }
        }
    }
    out
}

type Key = (u64, u64, u64, u64);

/// Measurement setting of a parameter set (what the metrics depend on).
fn measure_key(p: &Params) -> Key {
    (
        p.sustain_m.to_bits(),
        f64::from(p.forest_cover_pct.unwrap_or(-1.0)).to_bits(),
        f64::from(p.sidehill_deg.map_or(-1.0, |s| s[0])).to_bits(),
        p.coarse_m.to_bits(),
    )
}

/// Forward metrics of every case for every distinct measurement setting of the grid.
pub struct MetricCache {
    keys: Vec<Key>,
    metrics: Vec<Vec<Metrics>>,
}

impl MetricCache {
    /// Measures every case once per setting.
    pub fn new(cases: &[Case<'_>], params: &[Params]) -> Self {
        let mut keys: Vec<Key> = Vec::new();
        let mut settings: Vec<Measure> = Vec::new();
        for p in params {
            let k = measure_key(p);
            if !keys.contains(&k) {
                keys.push(k);
                settings.push(p.measure());
            }
        }
        let metrics = settings
            .par_iter()
            .map(|m| {
                cases
                    .iter()
                    .map(|c| analysis::measure(c.profile, Direction::Forward, m))
                    .collect()
            })
            .collect();
        Self { keys, metrics }
    }

    /// Metrics of case `i` for the setting of `p`.
    pub fn get(&self, p: &Params, i: usize) -> &Metrics {
        let k = self
            .keys
            .iter()
            .position(|k| *k == measure_key(p))
            .expect("setting measured");
        &self.metrics[k][i]
    }
}

/// Published level of a case on its first mapped scale.
pub fn published(c: &Case<'_>) -> Option<(String, u8)> {
    c.route
        .ratings
        .iter()
        .find_map(|r| r.level.map(|l| (r.scale.clone(), l)))
}

/// What the calibration minimises.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Objective {
    /// Mean absolute level difference (exact misses break ties).
    Mae,
    /// Linearly weighted kappa on 3 levels, all scales together (rewards agreement
    /// beyond the majority class).
    Kappa,
}

/// Loss of a parameter set on cases (lower is better).
pub fn loss(
    cache: &MetricCache,
    cases: &[Case<'_>],
    idx: &[usize],
    p: &Params,
    obj: Objective,
) -> (f64, usize) {
    let mut l = 0.0;
    let mut miss = 0;
    let mut pairs = Vec::new();
    for &i in idx {
        for r in &cases[i].route.ratings {
            let Some(pubd) = r.level else { continue };
            let pred = rules::rate(cache.get(p, i), p, None).level.index();
            let d = pubd.abs_diff(fold(&r.scale, pred));
            l += f64::from(d);
            miss += usize::from(d > 0);
            pairs.push((pubd.min(3), fold(&r.scale, pred).min(3)));
        }
    }
    match obj {
        Objective::Mae => (l, miss),
        Objective::Kappa => (-agreement("srn_com", &pairs).kappa_w, miss),
    }
}

/// Best parameters of the grid on the cases `idx`.
pub fn best(
    cache: &MetricCache,
    cases: &[Case<'_>],
    idx: &[usize],
    grid: &[Params],
    obj: Objective,
) -> Params {
    let scored: Vec<((f64, usize), usize)> = grid
        .par_iter()
        .enumerate()
        .map(|(k, p)| (loss(cache, cases, idx, p, obj), k))
        .collect();
    let (_, k) = scored
        .into_iter()
        .min_by(|a, b| {
            a.0.0
                .total_cmp(&b.0.0)
                .then(a.0.1.cmp(&b.0.1))
                .then(a.1.cmp(&b.1))
        })
        .expect("non-empty grid");
    grid[k]
}

/// Prediction of every case with one parameter set (forward direction).
pub fn predict_all(cache: &MetricCache, cases: &[Case<'_>], p: &Params) -> Vec<u8> {
    (0..cases.len())
        .map(|i| rules::rate(cache.get(p, i), p, None).level.index())
        .collect()
}

/// Leave-one-route-out: for each group, the best parameters on the other groups predict
/// the group. Returns the predictions and the parameters chosen per group.
pub fn leave_one_route_out(
    cache: &MetricCache,
    cases: &[Case<'_>],
    grid: &[Params],
    obj: Objective,
) -> (Vec<u8>, Vec<Params>) {
    let mut groups: Vec<usize> = cases.iter().map(|c| c.group).collect();
    groups.sort_unstable();
    groups.dedup();
    let mut pred = vec![0u8; cases.len()];
    let mut chosen = Vec::new();
    for g in groups {
        let train: Vec<usize> = (0..cases.len()).filter(|i| cases[*i].group != g).collect();
        let p = best(cache, cases, &train, grid, obj);
        for i in (0..cases.len()).filter(|i| cases[*i].group == g) {
            pred[i] = rules::rate(cache.get(&p, i), &p, None).level.index();
        }
        chosen.push(p);
    }
    (pred, chosen)
}

/// Agreement per scale of predictions.
pub fn agreements(cases: &[Case<'_>], pred: &[u8]) -> Vec<Agreement> {
    let mut by: std::collections::BTreeMap<String, Vec<(u8, u8)>> = Default::default();
    for (i, c) in cases.iter().enumerate() {
        for r in &c.route.ratings {
            if let Some(l) = r.level {
                by.entry(r.scale.clone()).or_default().push((l, pred[i]));
            }
        }
    }
    let mut out: Vec<Agreement> = by.iter().map(|(s, v)| agreement(s, v)).collect();
    let all: Vec<(u8, u8)> = by
        .iter()
        .flat_map(|(s, v)| v.iter().map(move |(p, q)| (*p, fold(s, *q))))
        .collect();
    // All scales together, on 3 levels (« difficile » and « très difficile » merged).
    let all3: Vec<(u8, u8)> = all
        .iter()
        .map(|(p, q)| ((*p).min(3), (*q).min(3)))
        .collect();
    out.push(agreement("srn_com", &all3));
    if let Some(last) = out.last_mut() {
        last.scale = "toutes (3 niveaux)".into();
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn agreement_figures() {
        let a = agreement("geotrek_vercors", &[(1, 1), (2, 2), (3, 4), (4, 4), (1, 3)]);
        assert_eq!(a.n, 5);
        assert!((a.exact - 0.6).abs() < 1e-9);
        assert!((a.within_one - 0.8).abs() < 1e-9);
        assert!((a.mae - 0.6).abs() < 1e-9);
        assert_eq!(a.matrix[2][3], 1);
        let b = agreement("srn_com", &[(3, 4), (1, 1)]);
        assert!((b.exact - 1.0).abs() < 1e-9);
        assert!((b.kappa_w - 1.0).abs() < 1e-9);
    }

    #[test]
    fn grid_size() {
        assert_eq!(grid().len(), 4 * 4 * 3 * 4 * 4 * 6 * 2);
    }
}
