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
    // Axis 0 (along columns), written column-major: one flat buffer, no per-column
    // allocation left behind in the allocator (#172).
    let mut cols_t = vec![0f32; w * h];
    cols_t
        .par_chunks_mut(h.max(1))
        .enumerate()
        .for_each(|(c, o)| {
            let input: Vec<f64> = (0..h).map(|r| f64::from(src[r * w + c])).collect();
            line(&input, o);
        });
    // Axis 1 (along rows).
    let mut out = vec![0f32; w * h];
    out.par_chunks_mut(w.max(1)).enumerate().for_each(|(r, o)| {
        let input: Vec<f64> = (0..w).map(|c| f64::from(cols_t[c * h + r])).collect();
        line(&input, o);
    });
    out
}

/// No gap vote (neighbourhood canopy below [`GAP_CONTEXT_PCT`]).
const NO_GAP_VOTE: u8 = u8::MAX;

/// Split vote of the classifier from its four comparisons: the infrequent scenario owns
/// the low steps (`lo > c1`, `lo > c2`), the typical one the high step (`hi > c3`); 0–3,
/// or -1 when neither scenario has a value (`lo > 0`, `hi > 0`). The low cut `c1` is 0
/// for every vote of dev8 (start zone, size, travel angle, flux area), so `lo > c1` is
/// `lo > 0`.
fn split_vote(lo_pos: bool, lo_c2: bool, hi_pos: bool, hi_c3: bool) -> i8 {
    if hi_pos || lo_pos {
        let mut v = 0;
        if lo_pos {
            v = 1;
        }
        if lo_c2 {
            v = 2;
        }
        if hi_c3 {
            v = 3;
        }
        v
    } else {
        -1
    }
}

/// The two scenarios of the classifier.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Scenario {
    /// Typical (α = 30°): the high step of the split votes and the floors 2 and 3.
    Frequent,
    /// Infrequent (α = 18°): the low steps of the split votes and the floor 1.
    Extreme,
}

/// Vote cuts of one scenario: continuous PRA, start-zone size (ha), travel angle (°).
/// The infrequent scenario owns `c2`, the typical one `c3` (`split_vote`).
fn cuts(s: Scenario) -> (f32, f32, f32) {
    match s {
        Scenario::Extreme => (0.0087, 0.0136, 24.005),
        Scenario::Frequent => (0.1613, 0.3086, 34.270),
    }
}

/// Flux-area cuts of dev8 at the cell size: (low cut, `c2`, `c3`, floor 3), m².
fn flux_cuts(cell: f64) -> (f32, f32, f32, f32) {
    let area_factor = (cell * cell) / (CUT_REF_CELL * CUT_REF_CELL);
    (
        (0.0 * area_factor) as f32,
        (10.299 * area_factor) as f32,
        (375.18 * area_factor) as f32,
        (FLOOR3_FLUX_AREA * area_factor) as f32,
    )
}

/// `m` when `b`, else 0.
fn flag(b: bool, m: u8) -> u8 {
    if b { m } else { 0 }
}

/// Bits of [`pra_bits`], per scenario: continuous PRA above 0, above its cut; start-zone
/// size above 0, above its cut.
const PRA_POS: u8 = 1;
const PRA_CUT: u8 = 2;
const SIZE_POS: u8 = 4;
const SIZE_CUT: u8 = 8;

/// What the classifier reads of the start zones of one scenario, one byte per cell
/// (four bits): the comparisons of the continuous PRA and of the start-zone size (ha,
/// `pra::component_area_m2` of the release cells) with 0 and with the vote cut of the
/// scenario. Computed once the PRA is known, so that the continuous PRA (4 bytes per
/// cell and scenario) need not be kept through the runout (#173).
pub fn pra_bits(p: &Pra, s: Scenario) -> Vec<u8> {
    let (pra_cut, size_cut, _) = cuts(s);
    let size = pra::component_area_m2(&p.release);
    p.continuous
        .data
        .par_iter()
        .zip(&size.data)
        .map(|(c, a)| {
            let ha = a / 10000.0;
            flag(*c > 0.0, PRA_POS)
                | flag(*c > pra_cut, PRA_CUT)
                | flag(ha > 0.0, SIZE_POS)
                | flag(ha > size_cut, SIZE_CUT)
        })
        .collect()
}

/// Bits of [`runout_bits`].
const REACHED: u8 = 1;
const ZDELTA_POS: u8 = 2;
const TRAVEL_POS: u8 = 4;
const TRAVEL_CUT: u8 = 8;
const FLUX_POS: u8 = 16;
const FLUX_CUT: u8 = 32;
const FLUX_FLOOR3: u8 = 64;

/// What the classifier and the layer read of the runout of one scenario, one byte per
/// cell instead of 16 (#173): reached, zDelta above 0, travel angle and routed flux area
/// (0 where not reached, as the driver stack aligns them) above 0 and above the vote cut
/// of the scenario, flux area at least the floor-3 area.
pub fn runout_bits(run: &Runout, cell: f64, s: Scenario) -> Vec<u8> {
    let (_, _, travel_cut) = cuts(s);
    let (_, flux_c2, flux_c3, floor3) = flux_cuts(cell);
    let flux_cut = match s {
        Scenario::Extreme => flux_c2,
        Scenario::Frequent => flux_c3,
    };
    let cell_area = (cell * cell) as f32;
    (0..run.cell_counts.data.len())
        .into_par_iter()
        .map(|i| {
            let reached = run.reached(i);
            let (zdelta, travel, flux) = if reached {
                (
                    run.zdelta.data[i],
                    run.fp_travel_angle.data[i],
                    run.rout_flux_sum.data[i] * cell_area,
                )
            } else {
                (0.0, 0.0, 0.0)
            };
            flag(reached, REACHED)
                | flag(zdelta > 0.0, ZDELTA_POS)
                | flag(travel > 0.0, TRAVEL_POS)
                | flag(travel > travel_cut, TRAVEL_CUT)
                | flag(flux > 0.0, FLUX_POS)
                | flag(flux > flux_cut, FLUX_CUT)
                | flag(flux >= floor3, FLUX_FLOOR3)
        })
        .collect()
}

/// Drivers of the classifier from the PRA and the runout of both scenarios, three bytes
/// per cell ([`pra_bits`], [`runout_bits`]): what the classifier reads of 44 bytes per
/// cell of PRA and runout layers (#173).
#[derive(Debug, Clone)]
pub struct Drivers {
    /// [`pra_bits`] of the typical scenario.
    pub pra_frequent: Vec<u8>,
    /// [`pra_bits`] of the infrequent scenario.
    pub pra_extreme: Vec<u8>,
    /// [`runout_bits`] of the typical scenario.
    pub runout_frequent: Vec<u8>,
    /// [`runout_bits`] of the infrequent scenario.
    pub runout_extreme: Vec<u8>,
}

impl Drivers {
    /// Drivers of the PRA and runout of both scenarios on a grid of `cell` m.
    pub fn new(cell: f64, freq: (&Pra, &Runout), ext: (&Pra, &Runout)) -> Self {
        Self {
            pra_frequent: pra_bits(freq.0, Scenario::Frequent),
            pra_extreme: pra_bits(ext.0, Scenario::Extreme),
            runout_frequent: runout_bits(freq.1, cell, Scenario::Frequent),
            runout_extreme: runout_bits(ext.1, cell, Scenario::Extreme),
        }
    }

    /// Whether the infrequent runout reaches cell `i`.
    pub fn reached_extreme(&self, i: usize) -> bool {
        self.runout_extreme[i] & REACHED != 0
    }

    /// Whether the typical runout reaches cell `i`.
    pub fn reached_frequent(&self, i: usize) -> bool {
        self.runout_frequent[i] & REACHED != 0
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
/// typical (`freq`) and infrequent (`ext`) scenarios; every intermediate layer kept.
pub fn classify(
    dem: &Raster<f32>,
    canopy: &Raster<f32>,
    freq: (&Pra, &Runout),
    ext: (&Pra, &Runout),
) -> Ates {
    let d = Drivers::new(dem.cell, freq, ext);
    let (valid, raw, coarse, score) = votes(dem, canopy, &d, &ext.0.binary, true);
    let classes = mmu(&raw, &valid);
    Ates {
        valid,
        raw,
        classes,
        coarse_slope: dem.with_data(coarse),
        class0_score: dem.with_data(score.unwrap_or_default()),
    }
}

/// Final dev8 classes 0–4 (255 = invalid) from the [`Drivers`] and the infrequent binary
/// PRA (before the sieve), as [`classify`] computes them, without keeping its
/// intermediate layers: what the layer build reads (#173).
pub fn classes(
    dem: &Raster<f32>,
    canopy: &Raster<f32>,
    d: &Drivers,
    pra_extreme_binary: &Raster<u8>,
) -> Raster<u8> {
    let (valid, raw, coarse, _) = votes(dem, canopy, d, pra_extreme_binary, false);
    drop(coarse);
    mmu(&raw, &valid)
}

/// Classes before the minimum mapping unit: (valid terrain, raw classes, 300 m slope,
/// class-0 score when `keep_score`).
#[allow(clippy::type_complexity)]
fn votes(
    dem: &Raster<f32>,
    canopy: &Raster<f32>,
    d: &Drivers,
    pra_extreme_binary: &Raster<u8>,
    keep_score: bool,
) -> (Vec<bool>, Raster<u8>, Vec<f32>, Option<Vec<f32>>) {
    let (w, h) = (dem.w, dem.h);
    let n = w * h;
    let cell = dem.cell;

    // Terrain: GDAL slope without edges, cast to Int16 (truncation) as the driver stack.
    let slope_raw = pra::gdal_slope(dem, false);
    let valid: Vec<bool> = slope_raw.data.iter().map(|v| !v.is_nan()).collect();
    let slope: Vec<f32> = slope_raw
        .data
        .iter()
        .map(|v| if v.is_nan() { 0.0 } else { v.trunc() })
        .collect();
    // Every full-grid layer below is freed as soon as it is no longer read (#172).
    drop(slope_raw);
    // Canopy: rint of the aligned canopy, no-data → 0.
    let canopy_pct: Vec<f32> = canopy
        .data
        .iter()
        .map(|v| if v.is_nan() { 0.0 } else { v.round_ties_even() })
        .collect();

    let ord = |v: f32, cuts: &[f32; 3]| cuts.iter().filter(|c| v > **c).count() as f64;
    let coarse = uniform_filter(&slope, w, h, win_cells(COARSE_WINDOW_M, cell));

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
    drop((open, glabel, gsize));
    // Gap vote where the 300 m neighbourhood is forested enough, `NO_GAP_VOTE` elsewhere.
    let gap_vote: Vec<u8> = {
        let neighbourhood = uniform_filter(&canopy_pct, w, h, win_cells(COARSE_WINDOW_M, cell));
        neighbourhood
            .iter()
            .zip(&gap_ha)
            .map(|(nb, g)| {
                if *nb >= GAP_CONTEXT_PCT {
                    ord(*g, &GAP_CUTS) as u8
                } else {
                    NO_GAP_VOTE
                }
            })
            .collect()
    };
    drop(gap_ha);
    // Proximities with the reference's GDAL calls.
    let reach_e = dem.with_data(
        d.runout_extreme
            .iter()
            .map(|b| u8::from(b & REACHED != 0 && b & TRAVEL_POS != 0))
            .collect(),
    );
    let runout_prox = pra::gdal_proximity(&reach_e, ProximityBand::Float32);
    drop(reach_e);
    let pra_prox = pra::gdal_proximity(pra_extreme_binary, ProximityBand::Int16);

    let dist_layer = |v: f32| {
        if !v.is_finite() || v < 0.0 || v == -9999.0 {
            1.0e4f32
        } else {
            v
        }
    };
    let bit = |b: u8, m: u8| b & m != 0;

    // Class and class-0 score of a valid cell.
    let cell_class = |i: usize| -> (u8, f32) {
        let (pf, pe) = (d.pra_frequent[i], d.pra_extreme[i]);
        let (rf, re) = (d.runout_frequent[i], d.runout_extreme[i]);
        let (mut num, mut den) = (0.0f64, 0.0f64);
        // Split votes (see `split_vote`), -1 = no vote.
        let mut vote = |v: i8| {
            if v >= 0 {
                num += f64::from(v);
                den += 1.0;
            }
        };
        vote(split_vote(
            bit(pe, PRA_POS),
            bit(pe, PRA_CUT),
            bit(pf, PRA_POS),
            bit(pf, PRA_CUT),
        ));
        vote(split_vote(
            bit(pe, SIZE_POS),
            bit(pe, SIZE_CUT),
            bit(pf, SIZE_POS),
            bit(pf, SIZE_CUT),
        ));
        vote(split_vote(
            bit(re, TRAVEL_POS),
            bit(re, TRAVEL_CUT),
            bit(rf, TRAVEL_POS),
            bit(rf, TRAVEL_CUT),
        ));
        vote(split_vote(
            bit(re, FLUX_POS),
            bit(re, FLUX_CUT),
            bit(rf, FLUX_POS),
            bit(rf, FLUX_CUT),
        ));
        num += ord(coarse[i], &COARSE_CUTS);
        den += 1.0;
        if gap_vote[i] != NO_GAP_VOTE {
            num += GAP_WEIGHT * f64::from(gap_vote[i]);
            den += GAP_WEIGHT;
        }
        let tier = if den > 0.0 { num / den.max(1e-9) } else { 0.0 };
        let mut class = tier.round_ties_even().clamp(0.0, 4.0) as u8;
        // Floors.
        let mut floor = 0u8;
        if bit(re, ZDELTA_POS) {
            floor = floor.max(1);
        }
        if bit(rf, ZDELTA_POS) {
            floor = floor.max(2);
        }
        if bit(rf, FLUX_FLOOR3) {
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
        if score >= CLASS0_T {
            class = 0;
        }
        (class, score)
    };

    let mut raw = vec![255u8; n];
    let score = if keep_score {
        let mut score_out = vec![f32::NAN; n];
        raw.par_iter_mut()
            .zip(score_out.par_iter_mut())
            .enumerate()
            .filter(|(i, _)| valid[*i])
            .for_each(|(i, (out, sc))| (*out, *sc) = cell_class(i));
        Some(score_out)
    } else {
        raw.par_iter_mut()
            .enumerate()
            .filter(|(i, _)| valid[*i])
            .for_each(|(i, out)| *out = cell_class(i).0);
        None
    };
    drop((slope, canopy_pct, gap_vote));
    drop((runout_prox, pra_prox));
    (valid, dem.with_data(raw), coarse, score)
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
    drop(survivors);
    let out: Vec<u8> = (0..n)
        .map(|i| {
            if !valid[i] {
                255
            } else if to_fill[i] {
                match nearest[i] {
                    NO_FEATURE => raw.data[i],
                    j => raw.data[j as usize],
                }
            } else {
                raw.data[i]
            }
        })
        .collect();
    raw.with_data(out)
}

/// No feature in [`nearest_feature`] (empty mask).
pub const NO_FEATURE: u32 = u32::MAX;

/// Index of the nearest non-zero cell (exact Euclidean feature transform, two separable
/// passes keeping the argmin), [`NO_FEATURE`] when the mask is empty. Indices are `u32`
/// (a grid has fewer than 2³² − 1 cells) and the column pass is one flat column-major
/// buffer: 8 bytes per cell instead of 32 (#172).
pub fn nearest_feature(mask: &[u8], w: usize, h: usize) -> Vec<u32> {
    const INF: f64 = 1e20;
    assert!(
        w * h < NO_FEATURE as usize,
        "grid too large for u32 indices"
    );
    // Pass 1: per column, nearest feature row (column-major, NO_FEATURE = none).
    let mut col_near = vec![NO_FEATURE; w * h];
    col_near
        .par_chunks_mut(h.max(1))
        .enumerate()
        .for_each(|(c, out)| {
            let mut last = NO_FEATURE;
            for (r, o) in out.iter_mut().enumerate() {
                if mask[r * w + c] != 0 {
                    last = r as u32;
                }
                *o = last;
            }
            let mut next = NO_FEATURE;
            for r in (0..h).rev() {
                if mask[r * w + c] != 0 {
                    next = r as u32;
                }
                let best = match (out[r], next) {
                    (NO_FEATURE, b) => b,
                    (a, NO_FEATURE) => a,
                    (a, b) => {
                        if r - a as usize <= b as usize - r {
                            a
                        } else {
                            b
                        }
                    }
                };
                out[r] = best;
            }
        });
    // Pass 2: per row, lower envelope of parabolas over the columns.
    let mut nearest = vec![NO_FEATURE; w * h];
    nearest
        .par_chunks_mut(w.max(1))
        .enumerate()
        .for_each(|(r, o)| {
            let near = |c: usize| col_near[c * h + r];
            let f: Vec<f64> = (0..w)
                .map(|c| match near(c) {
                    NO_FEATURE => INF,
                    rr => ((rr as f64) - r as f64).powi(2),
                })
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
            for (q, out) in o.iter_mut().enumerate() {
                while z[k + 1] < q as f64 {
                    k += 1;
                }
                let p = v[k];
                *out = match near(p) {
                    NO_FEATURE => NO_FEATURE,
                    rr => rr * w as u32 + p as u32,
                };
            }
        });
    nearest
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
                let j = nf[r * w + c] as usize;
                assert_ne!(nf[r * w + c], NO_FEATURE);
                let d = |i: usize| {
                    let (rr, cc) = (i / w, i % w);
                    (rr as i64 - r as i64).pow(2) + (cc as i64 - c as i64).pow(2)
                };
                let best = (0..w * h).filter(|i| m[*i] != 0).map(d).min().unwrap();
                assert_eq!(d(j), best, "cell ({r}, {c})");
            }
        }
        assert!(
            nearest_feature(&[0, 0], 2, 1)
                .iter()
                .all(|j| *j == NO_FEATURE)
        );
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
