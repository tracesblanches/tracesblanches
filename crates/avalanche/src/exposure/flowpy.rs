//! Avalanche runout: a Rust port of the Flow-Py routing model (D'Amboise et al., 2022,
//! « Flow-Py v1.0: a customizable, open-source simulation tool to estimate runout and
//! intensity of gravitational mass flows », GMD 15, 2423–2439), as implemented by
//! com4FlowPy in AvaFrame (`avaframe/com4FlowPy/flowCoreNumba.py`, commit d0451ce of
//! 2026-11-12), with the forest friction and detrainment module used by AutoATES v3.0.
//!
//! Why Flow-Py rather than a bare alpha-angle cone: the cone (report 07 pre-filter) is a
//! majorant that ignores the terrain between the release and the point, so it marks the
//! whole of Cervières as reachable at α = 18°. Flow-Py routes mass down the DEM
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
//! the additions (Float64 here, Float32 in the reference). Paths run in parallel (rayon).

use std::sync::Mutex;
use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};

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

/// Per-thread workspace.
struct Work {
    pending: Vec<u32>,
    visited: Vec<u32>,
    stamp: u32,
    queue: Vec<Entry>,
    modified: Vec<u32>,
}

#[derive(Clone, Copy)]
struct Entry {
    r: u32,
    c: u32,
    zdelta: f64,
    flux: f64,
    is_start: bool,
    first_parent_start: bool,
    n_parents: u8,
    pdir: [u8; MAXP],
    pzd: [f64; MAXP],
    pmd: [f64; MAXP],
    pmd3d: [f64; MAXP],
}

const NONE: u32 = u32::MAX;

impl Work {
    fn new(n: usize) -> Self {
        Self {
            pending: vec![NONE; n],
            visited: vec![0; n],
            stamp: 0,
            queue: Vec::with_capacity(1 << 14),
            modified: Vec::with_capacity(1 << 14),
        }
    }
}

/// Shared outputs, updated with atomics.
struct Acc {
    zdelta: Vec<AtomicU32>,
    flux: Vec<AtomicU64>,
    fp: Vec<AtomicU32>,
    count: Vec<AtomicU32>,
}

/// Order-preserving map of an `f32` to a `u32` (for an atomic maximum of signed floats).
#[inline]
fn f32_key(v: f32) -> u32 {
    let b = v.to_bits();
    if b >> 31 == 0 { b | 0x8000_0000 } else { !b }
}

#[inline]
fn key_f32(k: u32) -> f32 {
    if k >> 31 == 1 {
        f32::from_bits(k & 0x7fff_ffff)
    } else {
        f32::from_bits(!k)
    }
}

#[inline]
fn atomic_add_f64(a: &AtomicU64, v: f64) {
    let mut cur = a.load(Ordering::Relaxed);
    loop {
        let new = (f64::from_bits(cur) + v).to_bits();
        match a.compare_exchange_weak(cur, new, Ordering::Relaxed, Ordering::Relaxed) {
            Ok(_) => return,
            Err(x) => cur = x,
        }
    }
}

/// Routes every release cell of `release` over `dem` (m, `NaN` no-data) with the
/// forest structure index `fsi` (0–1, the canopy cover / 100 of the reference).
pub fn run(dem: &Raster<f32>, release: &Raster<u8>, fsi: &Raster<f32>, p: &FlowParams) -> Runout {
    assert!(dem.same_grid(release) && dem.same_grid(fsi), "runout grids");
    let (w, h) = (dem.w, dem.h);
    let n = w * h;
    let z: Vec<f64> = dem.data.iter().map(|v| f64::from(*v)).collect();
    let forest: Vec<f64> = fsi
        .data
        .iter()
        .map(|v| if v.is_finite() { f64::from(*v) } else { 0.0 })
        .collect();
    let starts: Vec<u32> = (0..n)
        .filter(|i| release.data[*i] > 0)
        .map(|i| i as u32)
        .collect();
    let acc = Acc {
        zdelta: (0..n).map(|_| AtomicU32::new(0f32.to_bits())).collect(),
        flux: (0..n).map(|_| AtomicU64::new(0f64.to_bits())).collect(),
        fp: (0..n).map(|_| AtomicU32::new(f32_key(-9999.0))).collect(),
        count: (0..n).map(|_| AtomicU32::new(0)).collect(),
    };
    let k = Kernel::new(&z, &forest, w, h, dem.cell, p);
    let pool: Mutex<Vec<Work>> = Mutex::new(Vec::new());
    let (paths, visits) = starts
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
    let count: Vec<u32> = acc
        .count
        .iter()
        .map(|a| a.load(Ordering::Relaxed))
        .collect();
    let zdelta: Vec<f32> = acc
        .zdelta
        .iter()
        .map(|a| f32::from_bits(a.load(Ordering::Relaxed)))
        .collect();
    let flux: Vec<f32> = acc
        .flux
        .iter()
        .map(|a| f64::from_bits(a.load(Ordering::Relaxed)) as f32)
        .collect();
    let fp: Vec<f32> = acc
        .fp
        .iter()
        .zip(&count)
        .map(|(a, c)| {
            if *c == 0 {
                f32::NAN
            } else {
                key_f32(a.load(Ordering::Relaxed))
            }
        })
        .collect();
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
    z: &'a [f64],
    forest: &'a [f64],
    w: usize,
    h: usize,
    cell: f64,
    p: FlowParams,
    ds: [f64; 9],
    distance: [f64; 9],
    tan_alpha: f64,
    no_fr_z: f64,
    no_de_z: f64,
    detrain: bool,
}

impl<'a> Kernel<'a> {
    fn new(z: &'a [f64], forest: &'a [f64], w: usize, h: usize, cell: f64, p: &FlowParams) -> Self {
        let ds0 = [SQRT2, 1.0, SQRT2, 1.0, 0.0, 1.0, SQRT2, 1.0, SQRT2];
        let dt0 = [SQRT2, 1.0, SQRT2, 1.0, 1.0, 1.0, SQRT2, 1.0, SQRT2];
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
        Self {
            z,
            forest,
            w,
            h,
            cell,
            p: *p,
            ds: ds0.map(|d| d * cell),
            distance: dt0.map(|d| d * cell),
            tan_alpha: p.alpha.to_radians().tan(),
            no_fr_z,
            no_de_z,
            detrain,
        }
    }

    #[inline]
    fn nodata_around(&self, r: usize, c: usize) -> bool {
        (r - 1..=r + 1).any(|rr| (c - 1..=c + 1).any(|cc| self.z[rr * self.w + cc].is_nan()))
    }

    /// One path; returns the number of cell visits (0 if the release cell is skipped).
    fn path(&self, start: usize, work: &mut Work, acc: &Acc) -> u64 {
        let (w, h) = (self.w, self.h);
        let (rs, cs) = (start / w, start % w);
        if rs < 1 || rs >= h - 1 || cs < 1 || cs >= w - 1 || self.nodata_around(rs, cs) {
            return 0;
        }
        let p = &self.p;
        let forest_on = p.forest.is_some();
        let fp_ = p.forest.unwrap_or(AUTOATES_FOREST);
        let altitude_start = self.z[start];
        work.stamp = work.stamp.wrapping_add(1);
        if work.stamp == 0 {
            work.visited.fill(0);
            work.stamp = 1;
        }
        let stamp = work.stamp;
        work.queue.clear();
        work.modified.clear();
        work.queue.push(Entry {
            r: rs as u32,
            c: cs as u32,
            zdelta: 0.0,
            flux: 1.0,
            is_start: true,
            first_parent_start: false,
            n_parents: 0,
            pdir: [0; MAXP],
            pzd: [0.0; MAXP],
            pmd: [0.0; MAXP],
            pmd3d: [0.0; MAXP],
        });
        work.pending[start] = 0;
        work.modified.push(start as u32);
        let mut head = 0usize;
        while head < work.queue.len() {
            let e = work.queue[head];
            head += 1;
            let (r, c) = (e.r as usize, e.c as usize);
            let idx = r * w + c;
            work.pending[idx] = NONE;
            if work.visited[idx] != stamp {
                work.visited[idx] = stamp;
                acc.count[idx].fetch_add(1, Ordering::Relaxed);
            }
            let z_delta = e.zdelta;
            let mut flux = e.flux;
            let mut dem_ng = [0f64; 9];
            for (k, v) in dem_ng.iter_mut().enumerate() {
                *v = self.z[(r + k / 3 - 1) * w + (c + k % 3 - 1)];
            }
            let altitude = dem_ng[4];
            let fsi = if forest_on { self.forest[idx] } else { 0.0 };

            // calcDistMin: projected and 3-D shortest distance from the start.
            let (mut min_distance, mut min_dist_xyz) = (0.0f64, 0.0f64);
            if !e.is_start {
                let (mut best, mut best3d) = (1.0e30f64, 1.0e30f64);
                for k in 0..e.n_parents as usize {
                    let pdir = e.pdir[k] as i64;
                    let dxp = pdir % 3 - 1;
                    let dyp = pdir / 3 - 1;
                    let (fx, fy) = (dxp as f64 * self.cell, dyp as f64 * self.cell);
                    let dd = (fx * fx + fy * fy).sqrt() + e.pmd[k];
                    if dd < best {
                        best = dd;
                    }
                    if forest_on {
                        let palt =
                            self.z[(r as i64 + dyp) as usize * w + (c as i64 + dxp) as usize];
                        let dz = (palt - altitude).abs();
                        let ady = dyp.abs() as f64 * self.cell;
                        let adx = dxp.abs() as f64 * self.cell;
                        let dd3 = (adx * adx + ady * ady + dz * dz).sqrt() + e.pmd3d[k];
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
            let mut zdn = [0f64; 9];
            for i in 0..9 {
                let mut v = z_delta + (altitude - dem_ng[i]) - self.ds[i] * tan_alpha;
                if v < 0.0 {
                    v = 0.0;
                } else if v > p.max_z {
                    v = p.max_z;
                }
                zdn[i] = v;
            }

            // calc_persistence.
            let mut persistence = [0f64; 9];
            let mut no_flow = [1f64; 9];
            if e.is_start || e.first_parent_start {
                persistence = [1.0; 9];
            } else {
                for k in 0..e.n_parents as usize {
                    let pdir = e.pdir[k] as usize;
                    let mw = e.pzd[k];
                    no_flow[pdir] = 0.0;
                    let dx = (pdir % 3) as i64 - 1;
                    let dy = (pdir / 3) as i64 - 1;
                    let (a, b, cc) = match (dx, dy) {
                        (-1, -1) => (8, 7, 5),
                        (-1, 0) => (5, 8, 2),
                        (-1, 1) => (2, 1, 5),
                        (0, -1) => (7, 6, 8),
                        (0, 1) => (1, 0, 2),
                        (1, -1) => (6, 3, 7),
                        (1, 0) => (3, 0, 6),
                        (1, 1) => (0, 1, 3),
                        _ => continue,
                    };
                    persistence[a] += mw;
                    persistence[b] += 0.707 * mw;
                    persistence[cc] += 0.707 * mw;
                }
            }
            for i in 0..9 {
                persistence[i] *= no_flow[i];
            }

            // calc_tanbeta / routing weights.
            let mut tan_beta = [0f64; 9];
            for i in 0..9 {
                if i != 4 && zdn[i] > 0.0 && persistence[i] > 0.0 {
                    let beta = ((altitude - dem_ng[i]) / self.distance[i]).atan() + HALF_PI;
                    tan_beta[i] = (beta / 2.0).tan();
                }
            }
            let mut tb_sum = 0.0;
            for t in &tan_beta {
                if *t > 0.0 {
                    tb_sum += t.powf(p.exp);
                }
            }
            let mut r_t = [0f64; 9];
            if tb_sum > 0.0 {
                for i in 0..9 {
                    if tan_beta[i] > 0.0 {
                        r_t[i] = tan_beta[i].powf(p.exp) / tb_sum;
                    }
                }
            }

            // Travel angles and forest detrainment (not on the start cell).
            let mut max_gamma = 0.0f64;
            if !e.is_start {
                let dh = altitude_start - altitude;
                if min_distance > 0.0 {
                    max_gamma = (dh / min_distance).atan() * DEG_PER_RAD;
                }
                if forest_on && self.detrain {
                    let rest_d = fp_.max_detrainment * fsi;
                    let slope_d = (rest_d - fp_.min_detrainment) / (0.0 - self.no_de_z);
                    let detr = fp_.min_detrainment.max(slope_d * z_delta + rest_d);
                    flux = 0.0003f64.max(flux - detr);
                }
            }

            // calc_distribution (fluxDistOldVersion = False).
            let thr = p.flux_threshold;
            let mut dist = [0f64; 9];
            let rt_sum: f64 = r_t.iter().sum();
            if rt_sum > 0.0 {
                let mut pr_sum = 0.0;
                for i in 0..9 {
                    pr_sum += persistence[i] * r_t[i];
                }
                if pr_sum > 0.0 {
                    for i in 0..9 {
                        dist[i] = persistence[i] * r_t[i] / pr_sum * flux;
                    }
                }
            }
            let count = dist.iter().filter(|d| **d >= thr).count();
            let mut mass_below = 0.0;
            for d in &dist {
                if *d < thr {
                    mass_below += *d;
                }
            }
            if mass_below > 0.0 && count > 0 {
                let add = mass_below / count as f64;
                for d in dist.iter_mut() {
                    if *d >= thr {
                        *d += add;
                    } else {
                        *d = 0.0;
                    }
                }
            }
            let mut dist_sum = 0.0;
            for d in &dist {
                dist_sum += *d;
            }
            if dist_sum != flux && count > 0 {
                let corr = (flux - dist_sum) / count as f64;
                for d in dist.iter_mut() {
                    if *d >= thr {
                        *d += corr;
                    }
                }
            }
            // With count == 0 the reference deposits the flux (depFluxSum, not used here):
            // no child is created.

            // Children sorted ascending by (zdelta, flux, row, col).
            let mut ch: [(f64, f64, usize, usize); 9] = [(0.0, 0.0, 0, 0); 9];
            let mut nch = 0;
            for i in 0..9 {
                if dist[i] >= thr {
                    ch[nch] = (zdn[i], dist[i], r + i / 3 - 1, c + i % 3 - 1);
                    nch += 1;
                }
            }
            let ch = &mut ch[..nch];
            ch.sort_by(|a, b| {
                a.0.total_cmp(&b.0)
                    .then(a.1.total_cmp(&b.1))
                    .then(a.2.cmp(&b.2))
                    .then(a.3.cmp(&b.3))
            });
            for &(czd, cflux, cr, cc) in ch.iter() {
                let cidx = cr * w + cc;
                let pq = work.pending[cidx];
                let dxp = c as i64 - cc as i64;
                let dyp = r as i64 - cr as i64;
                let pdir = ((dyp + 1) * 3 + (dxp + 1)) as u8;
                if pq != NONE {
                    let q = &mut work.queue[pq as usize];
                    q.flux += cflux;
                    if czd > q.zdelta {
                        q.zdelta = czd;
                    }
                    let k = q.n_parents as usize;
                    if k < MAXP {
                        q.pdir[k] = pdir;
                        q.pzd[k] = z_delta;
                        q.pmd[k] = min_distance;
                        q.pmd3d[k] = min_dist_xyz;
                        q.n_parents += 1;
                    }
                } else {
                    if cr < 1 || cr >= h - 1 || cc < 1 || cc >= w - 1 || self.nodata_around(cr, cc)
                    {
                        continue;
                    }
                    let mut q = Entry {
                        r: cr as u32,
                        c: cc as u32,
                        zdelta: czd,
                        flux: cflux,
                        is_start: false,
                        first_parent_start: e.is_start,
                        n_parents: 1,
                        pdir: [0; MAXP],
                        pzd: [0.0; MAXP],
                        pmd: [0.0; MAXP],
                        pmd3d: [0.0; MAXP],
                    };
                    q.pdir[0] = pdir;
                    q.pzd[0] = z_delta;
                    q.pmd[0] = min_distance;
                    q.pmd3d[0] = min_dist_xyz;
                    work.pending[cidx] = work.queue.len() as u32;
                    work.modified.push(cidx as u32);
                    work.queue.push(q);
                }
            }

            // Accumulate the outputs of the processed cell.
            acc.zdelta[idx].fetch_max((z_delta as f32).to_bits(), Ordering::Relaxed);
            atomic_add_f64(&acc.flux[idx], flux);
            acc.fp[idx].fetch_max(f32_key(max_gamma as f32), Ordering::Relaxed);
        }
        for &m in &work.modified {
            work.pending[m as usize] = NONE;
        }
        head as u64
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn float_key_orders_signed_values() {
        let vals = [-9999.0f32, -1.5, -0.0, 0.0, 1e-30, 2.5, 89.9];
        for w in vals.windows(2) {
            assert!(f32_key(w[0]) <= f32_key(w[1]), "{w:?}");
        }
        for v in vals {
            assert_eq!(key_f32(f32_key(v)).to_bits(), v.to_bits());
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
