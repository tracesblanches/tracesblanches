//! ATES-like classes 0–4: a port of the AutoATES v3.0 « dev8 » classifier
//! (`autoates/comAutoATES/ates/dev8.py`, `driverStack.py`, `atesPostProcess.py`,
//! `atesValidation/forestGaps.py` of the ISSW 2026 snapshot, commit 5558c16).
//!
//! dev8 averages ordinal votes (start-zone propensity, start-zone size, travel angle,
//! routed flux area, slope over 300 m, forest-gap area), raises the result to floors
//! (reached by the infrequent runout → 1, by the typical runout → 2, typical flux area ≥
//! 500 m² at 21.34 m → 3, open slope > 45° → 4, 300 m slope ≥ 40° / 45° → 3 / 4), then sets
//! class 0 where a « non-avalanche » score is at least 0.6. Small patches are then
//! dissolved into their neighbours (minimum mapping unit per class).
//!
//! These classes are an intermediate product kept for the comparison with the oracle:
//! the reference itself calls its maps « a research/tutorial product, not an operational
//! ATES map », and its class 0 can fall inside a modelled runout (report 07). The layer
//! shown to users is [`super::exposure_classes`], derived from these classes and the raw runout.

use rayon::prelude::*;

use super::flowpy::Runout;
use super::pra::{self, Pra, ProximityBand};
use super::raster::Raster;

/// Cell size the dev8 area cuts were calibrated on, m.
pub const CUT_REF_CELL: f64 = 21.34;
const FLOOR3_FLUX_AREA: f64 = 500.0;
const FLOOR4_SLOPE: f32 = 45.0;
const FLOOR4_CANOPY: f32 = 20.0;
const COARSE_CUTS: [f32; 3] = [8.029, 17.933, 31.419];
const COARSE_WINDOW_M: f64 = 300.0;
const GAP_CUTS: [f32; 3] = [0.0, 54.9, 100.0];
const GAP_WEIGHT: f64 = 2.0;
const GAP_CONTEXT_PCT: f32 = 30.0;
const GAP_OPEN_MAX_PCT: f32 = 30.0;
const GAP_MAX_AREA_HA: f32 = 100.0;
/// Class-0 score threshold.
pub const CLASS0_T: f32 = 0.60;
/// Minimum mapping unit per class, m².
pub const MMU_M2: [f64; 5] = [10000.0, 10000.0, 5000.0, 5000.0, 2500.0];

/// Odd cell count spanning `window_m`, at least 3 (`_win_cells`).
pub fn win_cells(window_m: f64, cell: f64) -> usize {
    ((window_m / cell).round() as usize | 1).max(3)
}

/// `scipy.ndimage.uniform_filter` (mode `reflect`) on a Float32 raster, as SciPy 1.17
/// computes it: one pass per axis (rows index first), each a running sum in Float64
/// divided by the size, cast to Float32 between the passes.
pub fn uniform_filter(src: &[f32], w: usize, h: usize, size: usize) -> Vec<f32> {
    let s1 = size / 2;
    let s2 = size - s1 - 1;
    let line = |input: &[f64], out: &mut [f32]| {
        let n = input.len();
        // Reflect: d c b a | a b c d | d c b a
        let at = |i: i64| -> f64 {
            let n = n as i64;
            let mut i = i;
            loop {
                if i < 0 {
                    i = -i - 1;
                } else if i >= n {
                    i = 2 * n - i - 1;
                } else {
                    return input[i as usize];
                }
            }
        };
        let ext: Vec<f64> = (-(s1 as i64)..(n + s2) as i64).map(at).collect();
        let mut tmp = 0.0f64;
        for v in &ext[..size] {
            tmp += *v;
        }
        out[0] = (tmp / size as f64) as f32;
        for ll in 1..n {
            tmp += ext[ll - 1 + size] - ext[ll - 1];
            out[ll] = (tmp / size as f64) as f32;
        }
    };
    // Axis 0 (along columns).
    let mut cols = vec![0f32; w * h];
    let col_out: Vec<Vec<f32>> = (0..w)
        .into_par_iter()
        .map(|c| {
            let input: Vec<f64> = (0..h).map(|r| f64::from(src[r * w + c])).collect();
            let mut o = vec![0f32; h];
            line(&input, &mut o);
            o
        })
        .collect();
    for (c, col) in col_out.iter().enumerate() {
        for (r, v) in col.iter().enumerate() {
            cols[r * w + c] = *v;
        }
    }
    // Axis 1 (along rows).
    let mut out = vec![0f32; w * h];
    out.par_chunks_mut(w)
        .zip(cols.par_chunks(w))
        .for_each(|(o, row)| {
            let input: Vec<f64> = row.iter().map(|v| f64::from(*v)).collect();
            line(&input, o);
        });
    out
}

/// Driver layers of the classifier for one scenario, on the DEM grid (`driverStack`).
struct Scenario<'a> {
    pra: &'a Pra,
    run: &'a Runout,
}

impl Scenario<'_> {
    /// Aligned zDelta (0 where not reached).
    fn zdelta(&self, i: usize) -> f32 {
        if self.run.reached(i) {
            self.run.zdelta.data[i]
        } else {
            0.0
        }
    }
    /// Aligned travel angle (0 where not reached).
    fn travel(&self, i: usize) -> f32 {
        if self.run.reached(i) {
            self.run.fp_travel_angle.data[i]
        } else {
            0.0
        }
    }
    /// routFluxSum × cell area, m² (0 where not reached).
    fn flux_area(&self, i: usize, cell_area: f32) -> f32 {
        if self.run.reached(i) {
            self.run.rout_flux_sum.data[i] * cell_area
        } else {
            0.0
        }
    }
}

/// Classifier inputs and intermediate layers kept for diagnostics.
#[derive(Debug, Clone)]
pub struct Ates {
    /// Valid terrain (driver-stack slope defined: no edge, no no-data neighbour).
    pub valid: Vec<bool>,
    /// Classes before the minimum mapping unit (`merge_2`), 255 = invalid.
    pub raw: Raster<u8>,
    /// Final classes 0–4, 255 = invalid.
    pub classes: Raster<u8>,
    /// Slope averaged over 300 m (degrees).
    pub coarse_slope: Raster<f32>,
    /// Class-0 score.
    pub class0_score: Raster<f32>,
}

/// dev8 classification. `dem` m, `canopy` % (`NaN` = no data), PRA and runout of the
/// typical (`freq`) and infrequent (`ext`) scenarios.
pub fn classify(
    dem: &Raster<f32>,
    canopy: &Raster<f32>,
    freq: (&Pra, &Runout),
    ext: (&Pra, &Runout),
) -> Ates {
    let (w, h) = (dem.w, dem.h);
    let n = w * h;
    let cell = dem.cell;
    let cell_area = (cell * cell) as f32;
    let f = Scenario {
        pra: freq.0,
        run: freq.1,
    };
    let e = Scenario {
        pra: ext.0,
        run: ext.1,
    };

    // Terrain: GDAL slope without edges, cast to Int16 (truncation) as the driver stack.
    let slope_raw = pra::gdal_slope(dem, false);
    let valid: Vec<bool> = slope_raw.data.iter().map(|v| !v.is_nan()).collect();
    let slope: Vec<f32> = slope_raw
        .data
        .iter()
        .map(|v| if v.is_nan() { 0.0 } else { v.trunc() })
        .collect();
    // Canopy: rint of the aligned canopy, no-data → 0.
    let canopy_pct: Vec<f32> = canopy
        .data
        .iter()
        .map(|v| if v.is_nan() { 0.0 } else { v.round_ties_even() })
        .collect();

    let coarse = uniform_filter(&slope, w, h, win_cells(COARSE_WINDOW_M, cell));
    let neighbourhood = uniform_filter(&canopy_pct, w, h, win_cells(COARSE_WINDOW_M, cell));

    // Forest gaps: area (ha) of the 8-connected open component (canopy ≤ 30 %).
    let open: Vec<u8> = (0..n)
        .map(|i| u8::from(valid[i] && canopy_pct[i] <= GAP_OPEN_MAX_PCT))
        .collect();
    let (glabel, gsize) = pra::components(&open, w, h, true);
    let gap_ha: Vec<f32> = glabel
        .iter()
        .map(|&l| {
            if l == 0 {
                0.0
            } else {
                ((f64::from(gsize[l as usize]) * cell * cell / 10000.0) as f32).min(GAP_MAX_AREA_HA)
            }
        })
        .collect();

    // Start-zone sizes (ha), see `pra::component_area_m2`.
    let size_e = pra::component_area_m2(&e.pra.release);
    let size_f = pra::component_area_m2(&f.pra.release);
    // Proximities with the reference's GDAL calls.
    let reach_e = dem.with_data(
        (0..n)
            .map(|i| u8::from(e.run.reached(i) && e.travel(i) > 0.0))
            .collect(),
    );
    let runout_prox = pra::gdal_proximity(&reach_e, ProximityBand::Float32);
    let pra_prox = pra::gdal_proximity(&e.pra.binary, ProximityBand::Int16);

    let area_factor = (cell * cell) / (CUT_REF_CELL * CUT_REF_CELL);
    let flux_cuts = [
        0.0 * area_factor,
        10.299 * area_factor,
        375.18 * area_factor,
    ];
    let floor3_flux = (FLOOR3_FLUX_AREA * area_factor) as f32;

    let ord = |v: f32, cuts: &[f32; 3]| cuts.iter().filter(|c| v > **c).count() as f64;
    let dist_layer = |v: f32| {
        if !v.is_finite() || v < 0.0 || v == -9999.0 {
            1.0e4f32
        } else {
            v
        }
    };

    let mut raw = vec![255u8; n];
    let mut score_out = vec![f32::NAN; n];
    raw.par_iter_mut()
        .zip(score_out.par_iter_mut())
        .enumerate()
        .for_each(|(i, (out, sc))| {
            if !valid[i] {
                return;
            }
            let (mut num, mut den) = (0.0f64, 0.0f64);
            // Split votes: (lo, c1, c2, hi, c3); extreme owns the low steps.
            let mut vote = |lo: f32, c1: f32, c2: f32, hi: f32, c3: f32| {
                if hi > 0.0 || lo > 0.0 {
                    let mut v = 0.0;
                    if lo > c1 {
                        v = 1.0;
                    }
                    if lo > c2 {
                        v = 2.0;
                    }
                    if hi > c3 {
                        v = 3.0;
                    }
                    num += v;
                    den += 1.0;
                }
            };
            vote(
                e.pra.continuous.data[i],
                0.0,
                0.0087,
                f.pra.continuous.data[i],
                0.1613,
            );
            vote(
                size_e.data[i] / 10000.0,
                0.0,
                0.0136,
                size_f.data[i] / 10000.0,
                0.3086,
            );
            vote(e.travel(i), 0.0, 24.005, f.travel(i), 34.270);
            vote(
                e.flux_area(i, cell_area),
                flux_cuts[0] as f32,
                flux_cuts[1] as f32,
                f.flux_area(i, cell_area),
                flux_cuts[2] as f32,
            );
            num += ord(coarse[i], &COARSE_CUTS);
            den += 1.0;
            if neighbourhood[i] >= GAP_CONTEXT_PCT {
                num += GAP_WEIGHT * ord(gap_ha[i], &GAP_CUTS);
                den += GAP_WEIGHT;
            }
            let tier = if den > 0.0 { num / den.max(1e-9) } else { 0.0 };
            let mut class = tier.round_ties_even().clamp(0.0, 4.0) as u8;
            // Floors.
            let mut floor = 0u8;
            if e.zdelta(i) > 0.0 {
                floor = floor.max(1);
            }
            if f.zdelta(i) > 0.0 {
                floor = floor.max(2);
            }
            if f.flux_area(i, cell_area) >= floor3_flux {
                floor = floor.max(3);
            }
            if slope[i] > FLOOR4_SLOPE && canopy_pct[i] <= FLOOR4_CANOPY {
                floor = floor.max(4);
            }
            class = class.max(floor);
            if coarse[i] >= 40.0 && class < 3 {
                class = 3;
            }
            if coarse[i] >= 45.0 && class < 4 {
                class = 4;
            }
            // Class-0 score (Float32 as NumPy), applied last.
            let f1 = ((14.0f32 - coarse[i]) / 14.0).clamp(0.0, 1.0);
            let f2 = ((20.0f32 - slope[i]) / 20.0).clamp(0.0, 1.0);
            let f3 = (canopy_pct[i] / 60.0).clamp(0.0, 1.0);
            let f4 = (dist_layer(runout_prox.data[i]) / 200.0).clamp(0.0, 1.0);
            let f5 = (dist_layer(pra_prox.data[i]) / 500.0).clamp(0.0, 1.0);
            let score = ((((f1 + f2) + f3) + f4) + f5) / 5.0;
            *sc = score;
            if score >= CLASS0_T {
                class = 0;
            }
            *out = class;
        });
    let raw = dem.with_data(raw);
    let classes = mmu(&raw, &valid);
    Ates {
        valid,
        raw,
        classes,
        coarse_slope: dem.with_data(coarse),
        class0_score: dem.with_data(score_out),
    }
}

/// Minimum mapping unit (`_remove_small_polygons`): 8-connected patches of a class
/// smaller than its MMU take the class of the nearest surviving cell (Euclidean).
pub fn mmu(raw: &Raster<u8>, valid: &[bool]) -> Raster<u8> {
    let (w, h) = (raw.w, raw.h);
    let n = w * h;
    let cell_area = raw.cell * raw.cell;
    let mut to_fill = vec![false; n];
    for (c, m2) in MMU_M2.iter().enumerate() {
        let min_cells = ((m2 / cell_area).round() as u32).max(1);
        if min_cells <= 1 {
            continue;
        }
        let mask: Vec<u8> = raw.data.iter().map(|v| u8::from(*v == c as u8)).collect();
        let (label, size) = pra::components(&mask, w, h, true);
        for i in 0..n {
            let l = label[i];
            if l != 0 && size[l as usize] < min_cells {
                to_fill[i] = true;
            }
        }
    }
    let survivors: Vec<u8> = (0..n).map(|i| u8::from(valid[i] && !to_fill[i])).collect();
    let nearest = nearest_feature(&survivors, w, h);
    let out: Vec<u8> = (0..n)
        .map(|i| {
            if !valid[i] {
                255
            } else if to_fill[i] {
                nearest[i].map_or(raw.data[i], |j| raw.data[j])
            } else {
                raw.data[i]
            }
        })
        .collect();
    raw.with_data(out)
}

/// Index of the nearest non-zero cell (exact Euclidean feature transform, two separable
/// passes keeping the argmin). `None` when the mask is empty.
pub fn nearest_feature(mask: &[u8], w: usize, h: usize) -> Vec<Option<usize>> {
    const INF: f64 = 1e20;
    // Pass 1: per column, nearest feature row.
    let col_near: Vec<Vec<Option<usize>>> = (0..w)
        .into_par_iter()
        .map(|c| {
            let mut out = vec![None; h];
            let mut last: Option<usize> = None;
            for (r, o) in out.iter_mut().enumerate() {
                if mask[r * w + c] != 0 {
                    last = Some(r);
                }
                *o = last;
            }
            let mut next: Option<usize> = None;
            for r in (0..h).rev() {
                if mask[r * w + c] != 0 {
                    next = Some(r);
                }
                let best = match (out[r], next) {
                    (Some(a), Some(b)) => Some(if r - a <= b - r { a } else { b }),
                    (a, b) => a.or(b),
                };
                out[r] = best;
            }
            out
        })
        .collect();
    // Pass 2: per row, lower envelope of parabolas over the columns.
    (0..h)
        .into_par_iter()
        .flat_map_iter(|r| {
            let f: Vec<f64> = (0..w)
                .map(|c| col_near[c][r].map_or(INF, |rr| ((rr as f64) - r as f64).powi(2)))
                .collect();
            let mut v = vec![0usize; w];
            let mut z = vec![0f64; w + 1];
            let mut k = 0usize;
            z[0] = f64::NEG_INFINITY;
            z[1] = f64::INFINITY;
            let inter = |q: usize, p: usize| {
                ((f[q] + (q * q) as f64) - (f[p] + (p * p) as f64)) / (2.0 * (q - p) as f64)
            };
            for q in 1..w {
                let mut s = inter(q, v[k]);
                while s <= z[k] {
                    k -= 1;
                    s = inter(q, v[k]);
                }
                k += 1;
                v[k] = q;
                z[k] = s;
                z[k + 1] = f64::INFINITY;
            }
            k = 0;
            let col_near = &col_near;
            (0..w)
                .map(move |q| {
                    while z[k + 1] < q as f64 {
                        k += 1;
                    }
                    let p = v[k];
                    col_near[p][r].map(|rr| rr * w + p)
                })
                .collect::<Vec<_>>()
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn uniform_filter_reflects_like_scipy() {
        // scipy.ndimage.uniform_filter(np.arange(12, dtype='float32').reshape(3, 4), 3)
        // = [[1.6666666, 2.3333333, 3.3333333, 4.], [4.3333335, 5., 6., 6.6666665],
        //    [7., 7.6666665, 8.666667, 9.333333]]
        let src: Vec<f32> = (0..12).map(|v| v as f32).collect();
        let out = uniform_filter(&src, 4, 3, 3);
        let expected = [
            1.6666666269302368f64,
            2.3333332538604736,
            3.3333332538604736,
            4.0,
            4.333333492279053,
            5.0,
            6.0,
            6.666666507720947,
            7.0,
            7.666666507720947,
            8.666666984558105,
            9.333333969116211,
        ];
        for (a, b) in out.iter().zip(expected) {
            assert_eq!(a.to_bits(), (b as f32).to_bits(), "{a} vs {b}");
        }
        assert_eq!(win_cells(300.0, 5.0), 61);
        assert_eq!(win_cells(300.0, 10.0), 31);
        assert_eq!(win_cells(300.0, 21.34), 15);
    }

    #[test]
    fn nearest_feature_is_exact() {
        let (w, h) = (9usize, 7usize);
        let mut m = vec![0u8; w * h];
        for &(r, c) in &[(0usize, 0usize), (6, 8), (3, 4)] {
            m[r * w + c] = 1;
        }
        let nf = nearest_feature(&m, w, h);
        for r in 0..h {
            for c in 0..w {
                let j = nf[r * w + c].unwrap();
                let d = |i: usize| {
                    let (rr, cc) = (i / w, i % w);
                    (rr as i64 - r as i64).pow(2) + (cc as i64 - c as i64).pow(2)
                };
                let best = (0..w * h).filter(|i| m[*i] != 0).map(d).min().unwrap();
                assert_eq!(d(j), best, "cell ({r}, {c})");
            }
        }
        assert!(nearest_feature(&[0, 0], 2, 1).iter().all(Option::is_none));
    }

    #[test]
    fn small_patches_are_dissolved() {
        // 30 × 30 at 10 m: MMU of class 1 is 100 cells; a 3 × 3 patch of class 1 inside
        // class 0 becomes 0; a 10 × 10 patch survives.
        let (w, h) = (30usize, 30usize);
        let mut data = vec![0u8; w * h];
        for r in 2..5 {
            for c in 2..5 {
                data[r * w + c] = 1;
            }
        }
        for r in 15..25 {
            for c in 15..25 {
                data[r * w + c] = 1;
            }
        }
        // Class 0 covers 900 - 9 - 100 = 791 cells ≥ 100: survives.
        let raw = Raster {
            w,
            h,
            cell: 10.0,
            origin_x: 0.0,
            origin_y: 0.0,
            data,
        };
        let valid = vec![true; w * h];
        let out = mmu(&raw, &valid);
        assert_eq!(out.data[3 * w + 3], 0);
        assert_eq!(out.data[20 * w + 20], 1);
    }
}
