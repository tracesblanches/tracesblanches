//! Avalanche runout: a Rust port of the Flow-Py routing model (D'Amboise et al., 2022,
//! « Flow-Py v1.0: a customizable, open-source simulation tool to estimate runout and
//! intensity of gravitational mass flows », GMD 15, 2423–2439), as implemented by
//! com4FlowPy in AvaFrame (`avaframe/com4FlowPy/flowCoreNumba.py`, commit d0451ce of
//! 2026-11-12), with the forest friction and detrainment module used by AutoATES v3.0.
//!
//! Why Flow-Py rather than a bare alpha-angle cone: the cone (report 07 pre-filter) is a
//! majorant that ignores the terrain between the release and the point, so it marks a
//! whole high-mountain site as reachable at α = 18°. Flow-Py routes mass down the DEM
//! (spreading controlled by an exponent, persistence of the flow direction), stops it on
//! the energy line of angle α (the alpha-beta concept of Lied & Bakkehøi 1980, as a
//! Coulomb friction), caps the energy-line height (velocity limit) and slows it in forest.
//! It is the runout model of AutoATES v2.0 and v3.0, so porting it keeps our classes
//! comparable to the reference cell by cell.
//!
//! Each release cell is routed independently (breadth-first over the 3 × 3 neighbours);
//! the outputs aggregate all paths: maximum energy-line height `zDelta`, sum of the
//! routed flux `routFluxSum`, maximum flow-path travel angle `fpTravelAngleMax`, number
//! of paths `cellCounts`. The per-path arithmetic follows the Numba kernel operation for
//! operation in Float64, so maxima are bit-identical; sums differ only by the order of
//! the additions (Float64 here, Float32 in the reference). Paths run in parallel (rayon):
//! each path aggregates its outputs per cell, then merges them once into the shared
//! accumulators, the flux as a fixed-point integer sum, so the result does not depend on
//! the scheduling of the threads.

use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock};

use rayon::prelude::*;

use super::raster::Raster;

const SQRT2: f64 = std::f64::consts::SQRT_2;
const HALF_PI: f64 = std::f64::consts::PI / 2.0;
const DEG_PER_RAD: f64 = 180.0 / std::f64::consts::PI;
const G: f64 = 9.81;
/// Maximum parents recorded per queued cell (as the reference workspace).
const MAXP: usize = 8;

/// Forest interaction (`forestDetrainment` module of com4FlowPy).
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize)]
pub struct ForestParams {
    /// Maximum added friction on fully forested cells, degrees.
    pub max_added_friction: f64,
    /// Minimum added friction on forested cells, degrees.
    pub min_added_friction: f64,
    /// Velocity threshold of the friction effect (the reference uses `v² / (√2 g)`).
    pub vel_th_friction: f64,
    /// Maximum detrainment (flux removed per cell) on fully forested cells.
    pub max_detrainment: f64,
    /// Minimum detrainment.
    pub min_detrainment: f64,
    /// Velocity threshold of the detrainment effect.
    pub vel_th_detrainment: f64,
    /// No added friction over the first metres (3-D) of a path.
    pub skip_forest_dist: f64,
}

/// AutoATES v3.0 ISSW defaults (`[FlowPy]` of `autoATESCfg.ini`).
pub const AUTOATES_FOREST: ForestParams = ForestParams {
    max_added_friction: 52.0,
    min_added_friction: 5.0,
    vel_th_friction: 270.0,
    max_detrainment: 0.003,
    min_detrainment: 0.00001,
    vel_th_detrainment: 270.0,
    skip_forest_dist: 0.0,
};

/// Flow-Py parameters of one scenario.
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize)]
pub struct FlowParams {
    /// Scenario name.
    pub name: &'static str,
    /// Angle of reach α, degrees.
    pub alpha: f64,
    /// Spreading exponent.
    pub exp: f64,
    /// Flux below which a neighbour receives nothing.
    pub flux_threshold: f64,
    /// Energy-line height limit, m.
    pub max_z: f64,
    /// Forest interaction (`None`: no forest).
    pub forest: Option<ForestParams>,
}

/// Typical scenario: α = 30° (`frequent_alpha`).
pub const FREQUENT: FlowParams = FlowParams {
    name: "frequent",
    alpha: 30.0,
    exp: 8.0,
    flux_threshold: 0.003,
    max_z: 270.0,
    forest: Some(AUTOATES_FOREST),
};

/// Infrequent scenario: α = 18° (`extreme_alpha`).
pub const EXTREME: FlowParams = FlowParams {
    name: "extreme",
    alpha: 18.0,
    ..FREQUENT
};

/// Aggregated runout of one scenario.
#[derive(Debug, Clone)]
pub struct Runout {
    /// Maximum energy-line height, m (0 where not reached).
    pub zdelta: Raster<f32>,
    /// Sum over all paths and visits of the routed flux (0 where not reached).
    pub rout_flux_sum: Raster<f32>,
    /// Maximum flow-path travel angle, degrees (`NaN` where not reached; release cells
    /// reached only by their own path have 0).
    pub fp_travel_angle: Raster<f32>,
    /// Number of paths reaching the cell.
    pub cell_counts: Raster<u32>,
    /// Release cells routed (border and no-data neighbourhoods excluded).
    pub paths: usize,
    /// Cell visits (queue entries processed), the unit of work.
    pub visits: u64,
}

impl Runout {
    /// Whether the cell is reached by at least one path.
    pub fn reached(&self, i: usize) -> bool {
        self.cell_counts.data[i] > 0
    }
}

/// Side of the square tiles of [`Cells::tb_exp`], cells.
const TILE: usize = 64;
/// Rows of release cells routed together before the workspaces are dropped and the
/// tiles behind are evicted (#172).
const BAND_ROWS: usize = 4 * TILE;
/// Rows kept above a band: paths of a band that run uphill in the grid (north) reach
/// these tiles again without recomputing them.
const KEEP_ROWS: usize = 4 * TILE;

/// Routing weights of a grid, shared by the scenarios that route over the same DEM and
/// forest index with the same spreading exponent (the two AutoATES scenarios differ only
/// by α): the `tan β^exp` tables are then computed once per tile for both runs.
pub struct Weights<'a> {
    cells: Cells<'a>,
}

impl<'a> Weights<'a> {
    /// Weights of `dem` (m, `NaN` no-data) with the forest index `fsi` for the
    /// spreading exponent `exp`. Nothing is computed until a run reaches a tile.
    pub fn new(dem: &'a Raster<f32>, fsi: &'a Raster<f32>, exp: f64) -> Self {
        assert!(dem.same_grid(fsi), "runout grids");
        let dt0 = [SQRT2, 1.0, SQRT2, 1.0, 1.0, 1.0, SQRT2, 1.0, SQRT2];
        Self {
            cells: Cells::new(dem, fsi, dt0.map(|d| d * dem.cell), exp),
        }
    }
}

/// Read-only cell data of one run, shared by all paths.
///
/// What a visit needs that does not depend on the path is computed once per cell here
/// instead of once per visit (a high-mountain site: 5.8 billion visits of 11.5 million
/// cells, about 500 visits per cell): the routing weights `tan β^exp` towards the eight neighbours
/// (two `powf`, one `atan` and one `tan` per open neighbour and visit in the reference
/// kernel) and whether a cell can be entered at all. The weights are computed by tiles of
/// [`TILE`] × [`TILE`] cells the first time a path reaches the tile, so that a site where
/// few cells are reached (low relief) does not pay for the whole grid. The values
/// are those the kernel computed, by the same operations: the routing is unchanged bit
/// for bit.
struct Cells<'a> {
    dem: &'a Raster<f32>,
    w: usize,
    h: usize,
    /// Elevation as read (Float32; widened exactly to Float64 on use).
    z: &'a [f32],
    /// Forest structure index (0 where not finite).
    fsi: &'a [f32],
    /// Interior cell without no-data in its 3 × 3 window: the only cells a path enters.
    routable: Vec<bool>,
    /// Horizontal distance to the neighbour `k` of the 3 × 3 window, m.
    distance: [f64; 9],
    /// Spreading exponent.
    exp: f64,
    /// Tiles across.
    tiles_w: usize,
    /// Per tile, row-major inside the tile: `tan β^exp` towards the neighbours `k ≠ 4`
    /// (slot [`slot`]), 0 where `tan β ≤ 0` and on cells that are not routable.
    tb_exp: Vec<OnceLock<Box<[[f64; 8]]>>>,
    /// Tile rows evicted by [`Cells::evict_above`] during the current run.
    evicted_rows: usize,
}

/// Slot of the neighbour `k ≠ 4` of the 3 × 3 window in [`Cells::tb_exp`].
#[inline]
fn slot(k: usize) -> usize {
    if k < 4 { k } else { k - 1 }
}

impl<'a> Cells<'a> {
    fn new(dem: &'a Raster<f32>, fsi: &'a Raster<f32>, distance: [f64; 9], exp: f64) -> Self {
        let (w, h) = (dem.w, dem.h);
        let z = &dem.data;
        let routable: Vec<bool> = (0..w * h)
            .into_par_iter()
            .map(|i| {
                let (r, c) = (i / w, i % w);
                r >= 1
                    && r < h - 1
                    && c >= 1
                    && c < w - 1
                    && (r - 1..=r + 1).all(|rr| (c - 1..=c + 1).all(|cc| !z[rr * w + cc].is_nan()))
            })
            .collect();
        let tiles_w = w.div_ceil(TILE);
        let tiles = tiles_w * h.div_ceil(TILE);
        Self {
            dem,
            w,
            h,
            z,
            fsi: &fsi.data,
            routable,
            distance,
            exp,
            tiles_w,
            tb_exp: (0..tiles).map(|_| OnceLock::new()).collect(),
            evicted_rows: 0,
        }
    }

    /// Forest structure index of a cell, 0 where unknown.
    #[inline]
    fn forest(&self, i: usize) -> f64 {
        let v = self.fsi[i];
        if v.is_finite() { f64::from(v) } else { 0.0 }
    }

    /// `tan β^exp` towards the eight neighbours of the cell `(r, c)`.
    #[inline]
    fn tb_exp(&self, r: usize, c: usize) -> &[f64; 8] {
        let t = (r / TILE) * self.tiles_w + c / TILE;
        let tile = self.tb_exp[t].get_or_init(|| self.tile(t));
        &tile[(r % TILE) * TILE + c % TILE]
    }

    /// Drops the weight tiles that lie entirely above `row`: a path that reaches one of
    /// them again recomputes it.
    fn evict_above(&mut self, row: usize) {
        let rows = (row / TILE).min(self.h.div_ceil(TILE));
        for t in self.evicted_rows * self.tiles_w..rows * self.tiles_w {
            self.tb_exp[t].take();
        }
        self.evicted_rows = self.evicted_rows.max(rows);
    }

    /// Weights of one tile (calc_tanbeta of the kernel: β = atan(Δz / d) + π/2, then
    /// tan(β / 2), raised to the exponent where positive).
    fn tile(&self, t: usize) -> Box<[[f64; 8]]> {
        let w = self.w;
        let (r0, c0) = ((t / self.tiles_w) * TILE, (t % self.tiles_w) * TILE);
        let mut out = vec![[0f64; 8]; TILE * TILE].into_boxed_slice();
        for r in r0..(r0 + TILE).min(self.h) {
            for c in c0..(c0 + TILE).min(w) {
                let i = r * w + c;
                if !self.routable[i] {
                    continue;
                }
                let o = &mut out[(r - r0) * TILE + (c - c0)];
                let altitude = f64::from(self.z[i]);
                for k in (0..9).filter(|k| *k != 4) {
                    let n = f64::from(self.z[(r + k / 3 - 1) * w + (c + k % 3 - 1)]);
                    let beta = ((altitude - n) / self.distance[k]).atan() + HALF_PI;
                    let tb = (beta / 2.0).tan();
                    if tb > 0.0 {
                        o[slot(k)] = tb.powf(self.exp);
                    }
                }
            }
        }
        out
    }
}

/// Per-thread workspace.
struct Work {
    /// Per cell: queue index + 1 while the cell waits in the queue (0 otherwise) in the
    /// low 32 bits, the last path that counted the cell in the high 32 bits. Zero at
    /// start, so the allocation is lazily zeroed: the pages of the cells no path of the
    /// thread reaches are never written.
    slots: Vec<u64>,
    /// Per cell: index in [`Work::touched`] for the path stamped in [`Work::slots`]
    /// (meaningful only while the stamp is the current path's). Lazily zeroed too.
    touch: Vec<u32>,
    stamp: u32,
    queue: Vec<Entry>,
    modified: Vec<u32>,
    /// Outputs of the current path, one per distinct cell it reached, in order of first
    /// visit; merged into the shared accumulators once per path.
    touched: Vec<Touch>,
}

/// Outputs of one path on one cell, aggregated over the visits of the path.
#[derive(Clone, Copy)]
struct Touch {
    cell: u32,
    /// Maximum of the bits of the Float32 energy-line heights (as the shared maximum).
    zdelta: u32,
    /// Maximum of `Δh / d` (key of [`f64_key`]).
    gamma: u64,
    /// Sum of the routed flux over the visits, in visit order (Float64).
    flux: f64,
}

/// One queued cell. Its parents are earlier queue entries: their `zdelta` and, once they
/// are processed, their distances from the start are read there (the reference copies
/// them into the child).
#[derive(Clone, Copy)]
struct Entry {
    r: u32,
    c: u32,
    zdelta: f64,
    flux: f64,
    /// Projected shortest distance from the start, set when the entry is processed.
    min_distance: f64,
    /// 3-D shortest distance from the start, set when the entry is processed.
    min_dist_xyz: f64,
    is_start: bool,
    first_parent_start: bool,
    n_parents: u8,
    parents: [u32; MAXP],
}

const LOW: u64 = 0xffff_ffff;

impl Work {
    fn new(n: usize) -> Self {
        Self {
            slots: vec![0; n],
            touch: vec![0; n],
            stamp: 0,
            queue: Vec::with_capacity(1 << 14),
            modified: Vec::with_capacity(1 << 14),
            touched: Vec::with_capacity(1 << 14),
        }
    }

    /// Queue index of the cell if it waits in the queue.
    #[inline]
    fn pending(&self, i: usize) -> Option<usize> {
        match self.slots[i] & LOW {
            0 => None,
            q => Some(q as usize - 1),
        }
    }

    #[inline]
    fn set_pending(&mut self, i: usize, q: usize) {
        self.slots[i] = (self.slots[i] & !LOW) | (q as u64 + 1);
    }

    #[inline]
    fn clear_pending(&mut self, i: usize) {
        self.slots[i] &= !LOW;
    }

    /// Index in [`Work::touched`] of the cell for the current path, adding it on the
    /// path's first visit of the cell.
    #[inline]
    fn touch(&mut self, i: usize) -> usize {
        let s = self.slots[i];
        if (s >> 32) as u32 == self.stamp {
            return self.touch[i] as usize;
        }
        self.slots[i] = (s & LOW) | (u64::from(self.stamp) << 32);
        let t = self.touched.len();
        self.touch[i] = t as u32;
        self.touched.push(Touch {
            cell: i as u32,
            zdelta: 0f32.to_bits(),
            gamma: 0,
            flux: 0.0,
        });
        t
    }
}

/// Shared outputs of one cell, merged once per path that reaches it.
struct CellAcc {
    zdelta: AtomicU32,
    count: AtomicU32,
    /// Maximum over the visits of `Δh / d` (key of [`f64_key`]): the travel angle is its
    /// `atan`, taken once per cell at the end (`atan` is monotonic, so the angle of the
    /// maximum is the maximum of the angles the kernel computed at every visit).
    gamma: AtomicU64,
    /// Sum of the routed flux, fixed point (units of [`FLUX_UNIT`]): integer additions
    /// commute, so the sum does not depend on the order in which the threads finish
    /// their paths (a Float64 atomic sum did).
    flux: AtomicU64,
}

/// Cells per chunk of [`Acc`]: 96 MB, above the largest mmap threshold of glibc (32 MB),
/// so that a freed chunk goes back to the system instead of staying in an arena.
const ACC_CHUNK: usize = 1 << 22;

/// Shared accumulators of a run, by chunks of [`ACC_CHUNK`] cells so that the outputs
/// can be built while the chunks are freed one after the other.
struct Acc {
    chunks: Vec<Box<[CellAcc]>>,
}

impl Acc {
    fn new(n: usize) -> Self {
        Self {
            chunks: (0..n.div_ceil(ACC_CHUNK))
                .into_par_iter()
                .map(|k| {
                    (k * ACC_CHUNK..((k + 1) * ACC_CHUNK).min(n))
                        .map(|_| CellAcc {
                            zdelta: AtomicU32::new(0f32.to_bits()),
                            count: AtomicU32::new(0),
                            gamma: AtomicU64::new(0),
                            flux: AtomicU64::new(0),
                        })
                        .collect()
                })
                .collect(),
        }
    }

    #[inline]
    fn get(&self, i: usize) -> &CellAcc {
        &self.chunks[i / ACC_CHUNK][i % ACC_CHUNK]
    }
}

/// Resolution of the fixed-point flux sum: 2⁻⁴⁰ (about 9 × 10⁻¹³). The per-path sum of a
/// cell (Float64, at least the minimum flux 0.0003 when positive) is rounded to it once,
/// far below the Float32 precision of the output; the largest sum it holds is 2²⁴ (a
/// cell would need sixteen million full-flux visits), checked on every addition.
const FLUX_SCALE: f64 = (1u64 << 40) as f64;
/// One unit of the fixed-point flux sum.
const FLUX_UNIT: f64 = 1.0 / FLUX_SCALE;

/// Fixed-point value of a non-negative per-path flux sum (round to nearest).
#[inline]
fn flux_fixed(v: f64) -> u64 {
    (v * FLUX_SCALE).round() as u64
}

/// Order-preserving map of an `f64` to a `u64` (total order: `-0 < +0`), for an atomic
/// maximum of signed floats.
#[inline]
fn f64_key(v: f64) -> u64 {
    let b = v.to_bits();
    if b >> 63 == 0 { b | (1 << 63) } else { !b }
}

#[inline]
fn key_f64(k: u64) -> f64 {
    if k >> 63 == 1 {
        f64::from_bits(k & !(1 << 63))
    } else {
        f64::from_bits(!k)
    }
}

/// Atomic maximum that skips the read-modify-write when the stored value is already at
/// least `v`.
#[inline]
fn atomic_max_u64(a: &AtomicU64, v: u64) {
    if v > a.load(Ordering::Relaxed) {
        a.fetch_max(v, Ordering::Relaxed);
    }
}

/// Travel angle of the reference (degrees, Float32) from `Δh / d`.
#[inline]
fn travel_angle(x: f64) -> f32 {
    (x.atan() * DEG_PER_RAD) as f32
}

/// Adds a per-path flux to the fixed-point sum of a cell.
#[inline]
fn add_flux(a: &AtomicU64, v: f64) {
    let q = flux_fixed(v);
    let old = a.fetch_add(q, Ordering::Relaxed);
    assert!(old.checked_add(q).is_some(), "flux sum overflow");
}

/// Atomic maximum that skips the read-modify-write when the stored value is already at
/// least `v` (the maximum does not depend on the order of the updates).
#[inline]
fn atomic_max_u32(a: &AtomicU32, v: u32) {
    if v > a.load(Ordering::Relaxed) {
        a.fetch_max(v, Ordering::Relaxed);
    }
}

/// Key of `f64::total_cmp`, as an integer.
#[inline]
fn total_key(v: f64) -> i64 {
    let b = v.to_bits() as i64;
    b ^ ((((b >> 63) as u64) >> 1) as i64)
}

/// Routes every release cell of `release` over `dem` (m, `NaN` no-data) with the
/// forest structure index `fsi` (0–1, the canopy cover / 100 of the reference).
pub fn run(dem: &Raster<f32>, release: &Raster<u8>, fsi: &Raster<f32>, p: &FlowParams) -> Runout {
    run_with(&mut Weights::new(dem, fsi, p.exp), release, p)
}

/// [`run`] over precomputed [`Weights`] (same DEM, forest index and exponent as `p`).
pub fn run_with(weights: &mut Weights<'_>, release: &Raster<u8>, p: &FlowParams) -> Runout {
    let dem = weights.cells.dem;
    assert!(dem.same_grid(release), "runout grids");
    assert!(
        weights.cells.exp.to_bits() == p.exp.to_bits(),
        "weights computed for another exponent"
    );
    let n = dem.w * dem.h;
    let starts: Vec<u32> = release
        .data
        .par_iter()
        .enumerate()
        .filter(|(_, v)| **v > 0)
        .map(|(i, _)| i as u32)
        .collect();
    let acc = Acc::new(n);
    weights.cells.evicted_rows = 0;
    let t0 = std::time::Instant::now();
    // Release cells by bands of rows, in order: the per-thread workspaces are dropped
    // after each band and the weight tiles above the next band (less a margin) are
    // evicted, so that memory follows the band and not the grid (#172). A path is
    // independent of every other and an evicted tile is recomputed by the same operations
    // when a later path reaches it again: the outputs are those of a single pass.
    let (mut paths, mut visits) = (0usize, 0u64);
    let mut lo = 0usize;
    while lo < starts.len() {
        let band = starts[lo] as usize / dem.w / BAND_ROWS;
        let hi = lo + starts[lo..].partition_point(|s| *s as usize / dem.w / BAND_ROWS == band);
        let k = Kernel::new(&weights.cells, p);
        let pool: Mutex<Vec<Work>> = Mutex::new(Vec::new());
        let (bp, bv) = starts[lo..hi]
            .par_chunks(256)
            .map(|chunk| {
                let mut work = pool
                    .lock()
                    .expect("workspace pool")
                    .pop()
                    .unwrap_or_else(|| Work::new(n));
                let (mut paths, mut visits) = (0usize, 0u64);
                for &s in chunk {
                    let v = k.path(s as usize, &mut work, &acc);
                    if v > 0 {
                        paths += 1;
                        visits += v;
                    }
                }
                pool.lock().expect("workspace pool").push(work);
                (paths, visits)
            })
            .reduce(|| (0, 0), |a, b| (a.0 + b.0, a.1 + b.1));
        drop(pool);
        paths += bp;
        visits += bv;
        lo = hi;
        if let Some(next) = starts.get(lo) {
            let top = next / dem.w as u32 / BAND_ROWS as u32 * BAND_ROWS as u32;
            weights
                .cells
                .evict_above((top as usize).saturating_sub(KEEP_ROWS));
        }
    }
    // The tiles left at the bottom of the grid are freed before the outputs are
    // gathered: the next scenario reaches them only at its own end, and recomputes them.
    weights.cells.evict_above(usize::MAX);
    tracing::debug!(
        scenario = p.name,
        paths_s = t0.elapsed().as_secs_f64(),
        paths,
        visits,
        "runout"
    );
    // Outputs filled chunk by chunk while the accumulator chunks are freed: the two
    // never both cover the whole grid (#172). The output vectors are reserved, not
    // written ahead, so their pages become resident only as they are filled.
    let (mut count, mut zdelta, mut flux, mut fp) = (
        Vec::with_capacity(n),
        Vec::with_capacity(n),
        Vec::with_capacity(n),
        Vec::with_capacity(n),
    );
    for chunk in acc.chunks {
        for a in chunk.iter() {
            let c = a.count.load(Ordering::Relaxed);
            count.push(c);
            zdelta.push(f32::from_bits(a.zdelta.load(Ordering::Relaxed)));
            flux.push((a.flux.load(Ordering::Relaxed) as f64 * FLUX_UNIT) as f32);
            fp.push(if c == 0 {
                f32::NAN
            } else {
                travel_angle(key_f64(a.gamma.load(Ordering::Relaxed)))
            });
        }
    }
    Runout {
        zdelta: dem.with_data(zdelta),
        rout_flux_sum: dem.with_data(flux),
        fp_travel_angle: dem.with_data(fp),
        cell_counts: dem.with_data(count),
        paths,
        visits,
    }
}

struct Kernel<'a> {
    cells: &'a Cells<'a>,
    w: usize,
    p: FlowParams,
    ds: [f64; 9],
    /// Projected distance to the neighbour `k` of the 3 × 3 window, m (`calcDistMin`).
    dist2d: [f64; 9],
    /// Squared horizontal distance to the neighbour `k`, m² (3-D distance of `calcDistMin`).
    h2: [f64; 9],
    /// Offset of the neighbour `k` of the 3 × 3 window from its north-west cell.
    offset: [usize; 9],
    tan_alpha: f64,
    no_fr_z: f64,
    no_de_z: f64,
    detrain: bool,
}

impl<'a> Kernel<'a> {
    fn new(cells: &'a Cells<'a>, p: &FlowParams) -> Self {
        let (w, cell) = (cells.dem.w, cells.dem.cell);
        let ds0 = [SQRT2, 1.0, SQRT2, 1.0, 0.0, 1.0, SQRT2, 1.0, SQRT2];
        let (no_fr_z, no_de_z, detrain) = match p.forest {
            Some(f) => {
                let nfz = if f.vel_th_friction != 0.0 {
                    (f.vel_th_friction * f.vel_th_friction) / (SQRT2 * G)
                } else {
                    0.0
                };
                let ndz = if f.vel_th_detrainment != 0.0 {
                    (f.vel_th_detrainment * f.vel_th_detrainment) / (SQRT2 * G)
                } else {
                    0.0
                };
                let detrain = !(f.max_detrainment == 0.0
                    && f.min_detrainment == 0.0
                    && f.vel_th_detrainment == 0.0);
                (nfz, ndz, detrain)
            }
            None => (0.0, 0.0, false),
        };
        // Same operations as the reference's calcDistMin, per neighbour direction.
        let mut dist2d = [0f64; 9];
        let mut h2 = [0f64; 9];
        for k in 0..9 {
            let dxp = (k % 3) as i64 - 1;
            let dyp = (k / 3) as i64 - 1;
            let (fx, fy) = (dxp as f64 * cell, dyp as f64 * cell);
            dist2d[k] = (fx * fx + fy * fy).sqrt();
            let ady = dyp.abs() as f64 * cell;
            let adx = dxp.abs() as f64 * cell;
            h2[k] = adx * adx + ady * ady;
        }
        Self {
            cells,
            w,
            p: *p,
            ds: ds0.map(|d| d * cell),
            dist2d,
            h2,
            offset: std::array::from_fn(|k| (k / 3) * w + k % 3),
            tan_alpha: p.alpha.to_radians().tan(),
            no_fr_z,
            no_de_z,
            detrain,
        }
    }

    /// One path; returns the number of cell visits (0 if the release cell is skipped).
    ///
    /// The arithmetic is the reference kernel's, operation for operation and in the same
    /// order for every sum; only the bookkeeping differs (3 × 3 window read once, loops
    /// without data-dependent exits, per-direction constants in tables).
    fn path(&self, start: usize, work: &mut Work, acc: &Acc) -> u64 {
        let w = self.w;
        let cells = &self.cells;
        if !cells.routable[start] {
            return 0;
        }
        let (rs, cs) = (start / w, start % w);
        let p = &self.p;
        let (max_z, thr) = (p.max_z, p.flux_threshold);
        let forest_on = p.forest.is_some();
        let fp_ = p.forest.unwrap_or(AUTOATES_FOREST);
        let altitude_start = f64::from(cells.z[start]);
        work.stamp = work.stamp.wrapping_add(1);
        if work.stamp == 0 {
            // No cell waits between two paths: the slots are all clear.
            work.slots.fill(0);
            work.stamp = 1;
        }
        work.queue.clear();
        work.modified.clear();
        work.touched.clear();
        work.queue.push(Entry {
            r: rs as u32,
            c: cs as u32,
            zdelta: 0.0,
            flux: 1.0,
            min_distance: 0.0,
            min_dist_xyz: 0.0,
            is_start: true,
            first_parent_start: false,
            n_parents: 0,
            parents: [0; MAXP],
        });
        work.set_pending(start, 0);
        work.modified.push(start as u32);
        let mut head = 0usize;
        while head < work.queue.len() {
            let me = head as u32;
            let e = work.queue[head];
            head += 1;
            let (r, c) = (e.r as usize, e.c as usize);
            let idx = r * w + c;
            // Index of the north-west cell of the 3 × 3 window.
            let base = idx - w - 1;
            work.clear_pending(idx);
            let ti = work.touch(idx);
            let z_delta = e.zdelta;
            let mut flux = e.flux;
            let row = |o: usize| {
                let s = &cells.z[base + o..base + o + 3];
                [f64::from(s[0]), f64::from(s[1]), f64::from(s[2])]
            };
            let (zt, zm, zb) = (row(0), row(w), row(2 * w));
            let dem_ng = [
                zt[0], zt[1], zt[2], zm[0], zm[1], zm[2], zb[0], zb[1], zb[2],
            ];
            let altitude = dem_ng[4];
            let fsi = if forest_on { cells.forest(idx) } else { 0.0 };

            // Parents: direction (3 × 3 index of the parent seen from this cell), energy
            // line height and distances from the start, read from their queue entries.
            let np = e.n_parents as usize;
            let mut pdir = [0usize; MAXP];
            let mut pzd = [0f64; MAXP];
            let mut pmd = [0f64; MAXP];
            let mut pmd3d = [0f64; MAXP];
            for k in 0..np {
                let pe = &work.queue[e.parents[k] as usize];
                let dyp = pe.r as usize + 1 - r;
                let dxp = pe.c as usize + 1 - c;
                pdir[k] = dyp * 3 + dxp;
                pzd[k] = pe.zdelta;
                pmd[k] = pe.min_distance;
                pmd3d[k] = pe.min_dist_xyz;
            }

            // calcDistMin: projected and 3-D shortest distance from the start.
            let (mut min_distance, mut min_dist_xyz) = (0.0f64, 0.0f64);
            if !e.is_start {
                let (mut best, mut best3d) = (1.0e30f64, 1.0e30f64);
                for k in 0..np {
                    let d = pdir[k];
                    let dd = self.dist2d[d] + pmd[k];
                    if dd < best {
                        best = dd;
                    }
                    if forest_on {
                        let dz = (dem_ng[d] - altitude).abs();
                        let dd3 = (self.h2[d] + dz * dz).sqrt() + pmd3d[k];
                        if dd3 < best3d {
                            best3d = dd3;
                        }
                    }
                }
                min_distance = best;
                if forest_on {
                    min_dist_xyz = best3d;
                }
            }
            {
                let me_e = &mut work.queue[head - 1];
                me_e.min_distance = min_distance;
                me_e.min_dist_xyz = min_dist_xyz;
            }

            // calc_z_delta with forest friction.
            let apply_for = forest_on && !e.is_start && fp_.skip_forest_dist < min_dist_xyz;
            let tan_alpha = if apply_for && fsi > 0.0 {
                let alpha_calc = if z_delta < self.no_fr_z {
                    let rest = fp_.max_added_friction * fsi;
                    let slope = (rest - fp_.min_added_friction) / (0.0 - self.no_fr_z);
                    let friction = fp_.min_added_friction.max(slope * z_delta + rest);
                    p.alpha + friction.max(0.0)
                } else {
                    p.alpha + fp_.min_added_friction
                };
                alpha_calc.to_radians().tan()
            } else {
                self.tan_alpha
            };
            // calc_persistence (all 1 on the start cell and its children).
            let mut persistence = [1f64; 9];
            if !(e.is_start || e.first_parent_start) {
                persistence = [0.0; 9];
                let mut no_flow = [1f64; 9];
                for k in 0..np {
                    let d = pdir[k];
                    let mw = pzd[k];
                    no_flow[d] = 0.0;
                    if let Some((pa, pb, pc)) = PERSIST[d] {
                        persistence[pa] += mw;
                        persistence[pb] += 0.707 * mw;
                        persistence[pc] += 0.707 * mw;
                    }
                }
                for i in 0..9 {
                    persistence[i] *= no_flow[i];
                }
            }

            // calc_tanbeta / routing weights, from the per-cell `tan β^exp`. Terms of the
            // reference with `tan β ≤ 0` are 0 in the table: adding them (or the 0 of a
            // closed direction) to the non-negative sum does not change it, and their
            // weight stays 0.
            // The energy line heights `zdn` (calc_z_delta, clamped to 0..max_z) are computed
            // in the same pass: they do not depend on the persistence.
            let t8 = cells.tb_exp(r, c);
            let tb = [t8[0], t8[1], t8[2], t8[3], 0.0, t8[4], t8[5], t8[6], t8[7]];
            let mut zdn = [0f64; 9];
            // Weights of the closed directions are +0: dividing them gives the +0 of the
            // reference, so the division runs on every direction (branch-free).
            let mut tb_open = [0f64; 9];
            let mut tb_sum = 0.0;
            for i in 0..9 {
                let v = z_delta + (altitude - dem_ng[i]) - self.ds[i] * tan_alpha;
                zdn[i] = select(v < 0.0, 0.0, select(v > max_z, max_z, v));
                let open = (zdn[i] > 0.0) & (persistence[i] > 0.0);
                tb_open[i] = select(open, tb[i], 0.0);
                tb_sum += tb_open[i];
            }
            let mut r_t = [0f64; 9];
            if tb_sum > 0.0 {
                for i in 0..9 {
                    r_t[i] = tb_open[i] / tb_sum;
                }
            }

            // Travel angles and forest detrainment (not on the start cell). The kernel's
            // travel angle is atan(Δh / d) in degrees (0 on the start cell): keep Δh / d
            // (0 gives the angle 0); the angle is taken once per cell at the end.
            let mut gamma_ratio = 0.0f64;
            if !e.is_start {
                let dh = altitude_start - altitude;
                if min_distance > 0.0 {
                    gamma_ratio = dh / min_distance;
                }
                if forest_on && self.detrain {
                    let rest_d = fp_.max_detrainment * fsi;
                    let slope_d = (rest_d - fp_.min_detrainment) / (0.0 - self.no_de_z);
                    let detr = fp_.min_detrainment.max(slope_d * z_delta + rest_d);
                    flux = 0.0003f64.max(flux - detr);
                }
            }

            // calc_distribution (fluxDistOldVersion = False).
            // The reference tests the sum of `r_t` (non-negative terms, at most 1 each): it is
            // positive exactly when one term is.
            let mut dist = [0f64; 9];
            if r_t.iter().any(|v| *v > 0.0) {
                let mut pr = [0f64; 9];
                let mut pr_sum = 0.0;
                for i in 0..9 {
                    pr[i] = persistence[i] * r_t[i];
                    pr_sum += pr[i];
                }
                if pr_sum > 0.0 {
                    for i in 0..9 {
                        // The reference gives +0 to a zero product: a product of
                        // non-negative factors is +0, and +0 / pr_sum × flux is +0 too.
                        dist[i] = pr[i] / pr_sum * flux;
                    }
                }
            }
            let mut count = 0usize;
            let mut mass_below = 0.0;
            for d in &dist {
                // Adding +0 for the kept terms leaves the (non-negative) sum unchanged.
                let keep = *d >= thr;
                count += usize::from(keep);
                mass_below += select(keep, 0.0, *d);
            }
            if mass_below > 0.0 && count > 0 {
                let add = mass_below / count as f64;
                for d in dist.iter_mut() {
                    *d = select(*d >= thr, *d + add, 0.0);
                }
            }
            let mut dist_sum = 0.0;
            for d in &dist {
                dist_sum += *d;
            }
            if dist_sum != flux && count > 0 {
                let corr = (flux - dist_sum) / count as f64;
                for d in dist.iter_mut() {
                    *d = select(*d >= thr, *d + corr, *d);
                }
            }
            // With count == 0 the reference deposits the flux (depFluxSum, not used here):
            // no child is created.

            // Children sorted ascending by (zdelta, flux, row, col); (row, col) ascends
            // with the 3 × 3 index, so the key is (total order of zdelta, of flux, index).
            let mut ch: [(i64, i64, usize); 9] = [(0, 0, 0); 9];
            let mut nch = 0;
            for i in 0..9 {
                if dist[i] >= thr {
                    let key = (total_key(zdn[i]), total_key(dist[i]), i);
                    // Insertion sort (at most 8 children, all keys distinct).
                    let mut j = nch;
                    while j > 0 && ch[j - 1] > key {
                        ch[j] = ch[j - 1];
                        j -= 1;
                    }
                    ch[j] = key;
                    nch += 1;
                }
            }
            for &(_, _, i) in &ch[..nch] {
                let czd = zdn[i];
                let cflux = dist[i];
                let cidx = base + self.offset[i];
                if let Some(pq) = work.pending(cidx) {
                    let q = &mut work.queue[pq];
                    q.flux += cflux;
                    if czd > q.zdelta {
                        q.zdelta = czd;
                    }
                    let k = q.n_parents as usize;
                    if k < MAXP {
                        q.parents[k] = me;
                        q.n_parents += 1;
                    }
                } else {
                    if !cells.routable[cidx] {
                        continue;
                    }
                    let mut parents = [0u32; MAXP];
                    parents[0] = me;
                    work.set_pending(cidx, work.queue.len());
                    work.modified.push(cidx as u32);
                    work.queue.push(Entry {
                        r: (r + i / 3 - 1) as u32,
                        c: (c + i % 3 - 1) as u32,
                        zdelta: czd,
                        flux: cflux,
                        min_distance: 0.0,
                        min_dist_xyz: 0.0,
                        is_start: false,
                        first_parent_start: e.is_start,
                        n_parents: 1,
                        parents,
                    });
                }
            }

            // Accumulate the outputs of the processed cell for this path.
            let t = &mut work.touched[ti];
            t.zdelta = t.zdelta.max((z_delta as f32).to_bits());
            t.gamma = t.gamma.max(f64_key(gamma_ratio));
            t.flux += flux;
        }
        for i in 0..work.modified.len() {
            let m = work.modified[i] as usize;
            work.clear_pending(m);
        }
        // Merge the path into the shared outputs: one count, two maxima (independent of
        // the order) and one integer sum per reached cell.
        for t in &work.touched {
            let a = acc.get(t.cell as usize);
            a.count.fetch_add(1, Ordering::Relaxed);
            atomic_max_u32(&a.zdelta, t.zdelta);
            atomic_max_u64(&a.gamma, t.gamma);
            add_flux(&a.flux, t.flux);
        }
        head as u64
    }
}

/// `if c { a } else { b }` without a branch (the routing tests are data-dependent and
/// mispredicted); bit for bit the selected value, signed zeros included.
#[inline(always)]
fn select(c: bool, a: f64, b: f64) -> f64 {
    let m = u64::from(c).wrapping_neg();
    f64::from_bits((a.to_bits() & m) | (b.to_bits() & !m))
}

/// Persistence targets of a parent in the direction `d` of the 3 × 3 window
/// (calc_persistence): the cell opposite the parent gets its zdelta, the two cells next
/// to that one 0.707 of it.
const PERSIST: [Option<(usize, usize, usize)>; 9] = [
    Some((8, 7, 5)), // dx -1, dy -1
    Some((7, 6, 8)), // dx 0, dy -1
    Some((6, 3, 7)), // dx 1, dy -1
    Some((5, 8, 2)), // dx -1, dy 0
    None,
    Some((3, 0, 6)), // dx 1, dy 0
    Some((2, 1, 5)), // dx -1, dy 1
    Some((1, 0, 2)), // dx 0, dy 1
    Some((0, 1, 3)), // dx 1, dy 1
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn float_key_orders_signed_values() {
        let vals = [-9999.0f64, -1.5, -1e-300, -0.0, 0.0, 1e-300, 2.5, 89.9];
        for w in vals.windows(2) {
            assert!(f64_key(w[0]) < f64_key(w[1]), "{w:?}");
            assert_eq!(f64_key(w[0]).cmp(&f64_key(w[1])), w[0].total_cmp(&w[1]));
        }
        for v in vals {
            assert_eq!(key_f64(f64_key(v)).to_bits(), v.to_bits());
        }
        // The zero-initialised accumulator is below every key.
        assert!(vals.iter().all(|v| f64_key(*v) > 0));
    }

    #[test]
    fn travel_angle_of_the_maximum_ratio_is_the_maximum_angle() {
        // The kernel kept the maximum of atan(Δh / d) in degrees, as Float32, over the
        // visits; we keep the maximum of Δh / d and take the angle once. Same value as
        // long as the angle is monotonic in the ratio, which holds on a dense sweep.
        let mut prev = f32::NEG_INFINITY;
        let mut x = -50.0f64;
        while x < 50.0 {
            let a = travel_angle(x);
            assert!(a >= prev, "{x}: {a} < {prev}");
            prev = a;
            x += 1e-3 + x.abs() * 1e-6;
        }
        assert_eq!(travel_angle(0.0).to_bits(), 0f32.to_bits());
    }

    #[test]
    fn total_key_orders_as_total_cmp() {
        let vals = [-3.0f64, -0.0, 0.0, 1e-310, 0.003, 270.0];
        for a in vals {
            for b in vals {
                assert_eq!(total_key(a).cmp(&total_key(b)), a.total_cmp(&b), "{a} {b}");
            }
        }
    }

    fn plane(w: usize, h: usize, cell: f64, slope_deg: f64) -> Raster<f32> {
        // Tilted plane falling towards the south (increasing rows).
        let t = slope_deg.to_radians().tan();
        let data = (0..w * h)
            .map(|i| (2000.0 - (i / w) as f64 * cell * t) as f32)
            .collect();
        Raster {
            w,
            h,
            cell,
            origin_x: 0.0,
            origin_y: 0.0,
            data,
        }
    }

    #[test]
    fn stops_on_the_energy_line() {
        // A 40° slope over rows 0..40 then flat: the path from row 2 must stop where the
        // energy line of α = 30° meets the flat ground: z_start - α-line height.
        let (w, h, cell) = (21usize, 200usize, 5.0f64);
        let mut dem = plane(w, h, cell, 40.0);
        let flat = dem.data[40 * w];
        for r in 40..h {
            for c in 0..w {
                dem.data[r * w + c] = flat;
            }
        }
        let mut rel = Raster::filled_like(&dem, 0u8);
        rel.data[2 * w + 10] = 1;
        let fsi = Raster::filled_like(&dem, 0f32);
        let p = FlowParams {
            forest: None,
            ..FREQUENT
        };
        let out = run(&dem, &rel, &fsi, &p);
        assert_eq!(out.paths, 1);
        let last_row = (0..h)
            .filter(|r| (0..w).any(|c| out.reached(r * w + c)))
            .max()
            .unwrap();
        // Drop 38 rows × 5 m × tan 40° = 159.4 m; horizontal reach at 30°: 276 m = 55
        // rows from row 2 (the energy line ignores the lateral spreading).
        let reach =
            (f64::from(dem.data[2 * w + 10] - flat) / 30f64.to_radians().tan() / cell) as usize + 2;
        assert!(
            last_row.abs_diff(reach) <= 2,
            "last reached row {last_row}, energy line {reach}"
        );
        // Travel angle on the last reached row is close to α.
        let gamma = (0..w)
            .filter_map(|c| {
                let v = out.fp_travel_angle.data[last_row * w + c];
                v.is_finite().then_some(v)
            })
            .fold(f32::NEG_INFINITY, f32::max);
        assert!((gamma - 30.0).abs() < 1.5, "gamma {gamma}");
        // The start cell itself has travel angle 0 and flux 1.
        assert_eq!(out.fp_travel_angle.data[2 * w + 10], 0.0);
        assert!((out.rout_flux_sum.data[2 * w + 10] - 1.0).abs() < 1e-6);
    }

    #[test]
    fn select_keeps_the_bits() {
        for (a, b) in [
            (1.5, -0.0),
            (-0.0, 0.0),
            (0.0, 270.0),
            (f64::MIN_POSITIVE, 3.0),
        ] {
            assert_eq!(select(true, a, b).to_bits(), a.to_bits());
            assert_eq!(select(false, a, b).to_bits(), b.to_bits());
        }
    }

    #[test]
    fn shared_weights_and_any_scheduling_give_the_same_bits() {
        // Many overlapping paths on a forested slope: the flux sums of a cell come from
        // many threads. Every output must be identical between runs, whatever order the
        // threads finish in, and with weights shared between the two scenarios.
        let (w, h, cell) = (41usize, 160usize, 5.0f64);
        let mut dem = plane(w, h, cell, 38.0);
        for (i, z) in dem.data.iter_mut().enumerate() {
            *z += ((i * 7919) % 13) as f32 * 0.37;
        }
        let mut rel = Raster::filled_like(&dem, 0u8);
        for r in 2..30 {
            for c in 2..w - 2 {
                rel.data[r * w + c] = 1;
            }
        }
        let mut fsi = Raster::filled_like(&dem, 0f32);
        for (i, v) in fsi.data.iter_mut().enumerate() {
            *v = ((i % 11) as f32) / 20.0;
        }
        let bits = |o: &Runout| {
            (
                o.rout_flux_sum
                    .data
                    .iter()
                    .map(|v| v.to_bits())
                    .collect::<Vec<_>>(),
                o.zdelta
                    .data
                    .iter()
                    .map(|v| v.to_bits())
                    .collect::<Vec<_>>(),
                o.fp_travel_angle
                    .data
                    .iter()
                    .map(|v| v.to_bits())
                    .collect::<Vec<_>>(),
                o.cell_counts.data.clone(),
                o.visits,
            )
        };
        let mut shared = Weights::new(&dem, &fsi, EXTREME.exp);
        let reference = bits(&run(&dem, &rel, &fsi, &EXTREME));
        for threads in [1, 3, 8] {
            let pool = rayon::ThreadPoolBuilder::new()
                .num_threads(threads)
                .build()
                .unwrap();
            let again = pool.install(|| bits(&run_with(&mut shared, &rel, &EXTREME)));
            assert!(again == reference, "{threads} threads");
        }
        let _ = run_with(&mut shared, &rel, &FREQUENT);
        assert!(bits(&run_with(&mut shared, &rel, &EXTREME)) == reference);
    }

    /// The weight tiles of a run do not outlive it, and routing by bands with eviction
    /// gives the bits of the unbanded run (#172: on a 21-million-cell zone the tiles
    /// kept for the whole grid weighed 1.4 GB).
    #[test]
    fn weight_tiles_are_freed_by_bands() {
        let (w, h, cell) = (40usize, 1300usize, 5.0f64);
        let dem = plane(w, h, cell, 35.0);
        let fsi = Raster::filled_like(&dem, 0f32);
        let mut rel = Raster::filled_like(&dem, 0u8);
        for r in [5usize, 600, 1200] {
            for c in [10usize, 20, 30] {
                rel.data[r * w + c] = 1;
            }
        }
        let mut weights = Weights::new(&dem, &fsi, EXTREME.exp);
        let banded = run_with(&mut weights, &rel, &EXTREME);
        let cached = weights
            .cells
            .tb_exp
            .iter()
            .filter(|t| t.get().is_some())
            .count();
        assert_eq!(cached, 0, "weight tiles left after the run");
        // Each band alone, on fresh weights, sums to the same outputs.
        let mut counts = vec![0u32; w * h];
        for r in [5usize, 600, 1200] {
            let mut one = Raster::filled_like(&dem, 0u8);
            for c in [10usize, 20, 30] {
                one.data[r * w + c] = 1;
            }
            let o = run(&dem, &one, &fsi, &EXTREME);
            for (a, b) in counts.iter_mut().zip(&o.cell_counts.data) {
                *a += b;
            }
        }
        assert_eq!(banded.cell_counts.data, counts);
        assert!(banded.reached((h - 10) * w + 20), "paths reach the bottom");
    }

    #[test]
    fn forest_shortens_the_runout() {
        let (w, h, cell) = (21usize, 200usize, 5.0f64);
        let dem = plane(w, h, cell, 35.0);
        let mut rel = Raster::filled_like(&dem, 0u8);
        rel.data[2 * w + 10] = 1;
        let open = Raster::filled_like(&dem, 0f32);
        let mut forest = open.clone();
        for r in 20..h {
            for c in 0..w {
                forest.data[r * w + c] = 0.8;
            }
        }
        let far = |o: &Runout| {
            (0..h)
                .filter(|r| (0..w).any(|c| o.reached(r * w + c)))
                .max()
                .unwrap()
        };
        let a = run(&dem, &rel, &open, &EXTREME);
        let b = run(&dem, &rel, &forest, &EXTREME);
        // On a 35° plane α = 18° never stops in the open (until the max_z cap and the
        // raster edge); the forest friction (+≈42° at FSI 0.8) stops it soon after row 20.
        assert!(far(&a) >= h - 3, "open runout {}", far(&a));
        assert!(far(&b) < 40, "forested runout {}", far(&b));
    }
}
