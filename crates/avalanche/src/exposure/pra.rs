//! Potential release areas (PRA), as in AutoATES v3.0 (`autoates/comAutoATES/pra/`).
//!
//! The reference combines three fuzzy memberships (Veitinger et al. 2016, as adapted by
//! AutoATES v2.0, Toft et al. 2024, NHESS 24, 1779–1793) with an ordered-weighted AND:
//!
//! - slope: Cauchy bell `1 / (1 + ((s - c) / a)^(2b))` on the Horn slope in degrees;
//! - wind shelter: same bell on the median of `atan((z_n - z_0) / d)` over the cells `n`
//!   within 60 m (a concavity / curvature index: positive in hollows that fill with
//!   wind-drifted snow, negative on ridges);
//! - forest: same bell on the canopy cover in percent;
//! - `pra = (1 - m) m + m · mean` with `m` the minimum membership.
//!
//! The cells at or above the scenario threshold form the binary PRA; 4-connected
//! components smaller than 1000 m² are removed (GDAL sieve). Two scenarios: « frequent »
//! (typical, Flow-Py α = 30°) and « extreme » (infrequent, α = 18°), with the ISSW 2026
//! default parameters of `autoATESCfg.ini`. Ruggedness is off, as in the reference.
//!
//! Floating-point operations follow the reference order and precision (Float32 for the
//! memberships as in NumPy, Float64 for the wind shelter as in its Numba kernel, GDAL's
//! own Horn slope), so that the layers match the oracle cell for cell.

use rayon::prelude::*;

use super::raster::Raster;

/// Parameters of one PRA scenario.
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize)]
pub struct PraParams {
    /// Scenario name (`frequent`, `extreme`).
    pub name: &'static str,
    /// Binary PRA threshold on the continuous PRA.
    pub threshold: f32,
    /// Slope membership `(a, b, c)`.
    pub slope: (f32, f32, f32),
    /// Forest membership `(a, b, c)` on canopy cover (%).
    pub forest: (f32, f32, f32),
    /// Wind shelter membership `(a, b, c)` (radians).
    pub wind: (f32, f32, f32),
    /// Wind shelter search radius, m.
    pub wind_radius_m: f64,
    /// Quantile of the wind shelter angles (0.5 = median).
    pub wind_probability: f64,
    /// Minimum PRA component area kept by the sieve, m².
    pub sieve_m2: f64,
}

/// Typical scenario (`[PRA] frequent_*` of the ISSW defaults).
pub const FREQUENT: PraParams = PraParams {
    name: "frequent",
    threshold: 0.25,
    slope: (12.0, 4.0, 42.5),
    forest: (50.0, 3.0, 0.0),
    wind: (3.0, 10.0, 3.0),
    wind_radius_m: 60.0,
    wind_probability: 0.5,
    sieve_m2: 1000.0,
};

/// Infrequent scenario (`[PRA] extreme_*`).
pub const EXTREME: PraParams = PraParams {
    name: "extreme",
    threshold: 0.05,
    slope: (14.0, 4.0, 40.0),
    ..FREQUENT
};

/// Horn slope in degrees exactly as GDAL `DEMProcessing("slope")` computes it
/// (`gdaldem_lib.cpp`, GDAL 3.12): Float32 sums of the 3 × 3 window, Float64 gradient.
///
/// With `compute_edges` (as in the reference PRA step), the edge windows are completed by
/// linear extrapolation `2 a - b` and no-data neighbours take the centre value; without
/// (as in the reference ATES step), edges and cells next to no-data are no-data.
/// No-data (`NaN`) in, `NaN` out.
pub fn gdal_slope(z: &Raster<f32>, compute_edges: bool) -> Raster<f32> {
    let (w, h) = (z.w, z.h);
    let res = z.cell;
    let at = |r: usize, c: usize| z.data[r * w + c];
    let interp = |a: f32, b: f32| {
        if a.is_nan() || b.is_nan() {
            f32::NAN
        } else {
            2.0 * a - b
        }
    };
    let out: Vec<f32> = (0..h)
        .into_par_iter()
        .flat_map_iter(|r| {
            (0..w).map(move |c| {
                let centre = at(r, c);
                if centre.is_nan() || w < 2 || h < 2 {
                    return f32::NAN;
                }
                let edge = r == 0 || c == 0 || r == h - 1 || c == w - 1;
                if edge && !compute_edges {
                    return f32::NAN;
                }
                let mut win = [0f32; 9];
                if r == 0 || r == h - 1 {
                    // First or last line: columns clamped, missing line extrapolated.
                    let cs = [c.saturating_sub(1), c, (c + 1).min(w - 1)];
                    let (near, far) = if r == 0 { (0, 1) } else { (h - 1, h - 2) };
                    let (lo, hi) = if r == 0 { (0, 3) } else { (6, 3) };
                    for (k, &cc) in cs.iter().enumerate() {
                        win[lo + k] = interp(at(near, cc), at(far, cc));
                        win[hi + k] = at(near, cc);
                    }
                    let other = if r == 0 { 6 } else { 0 };
                    let other_row = if r == 0 { 1 } else { h - 2 };
                    for (k, &cc) in cs.iter().enumerate() {
                        win[other + k] = at(other_row, cc);
                    }
                } else if c == 0 || c == w - 1 {
                    // First or last column of an inner line: missing column extrapolated.
                    for (k, rr) in [r - 1, r, r + 1].into_iter().enumerate() {
                        if c == 0 {
                            win[3 * k] = interp(at(rr, 0), at(rr, 1));
                            win[3 * k + 1] = at(rr, 0);
                            win[3 * k + 2] = at(rr, 1);
                        } else {
                            win[3 * k] = at(rr, c - 1);
                            win[3 * k + 1] = at(rr, c);
                            win[3 * k + 2] = interp(at(rr, c), at(rr, c - 1));
                        }
                    }
                } else {
                    for (k, rr) in [r - 1, r, r + 1].into_iter().enumerate() {
                        win[3 * k] = at(rr, c - 1);
                        win[3 * k + 1] = at(rr, c);
                        win[3 * k + 2] = at(rr, c + 1);
                    }
                }
                for v in &mut win {
                    if v.is_nan() {
                        if compute_edges {
                            *v = centre;
                        } else {
                            return f32::NAN;
                        }
                    }
                }
                horn(&win, res)
            })
        })
        .collect();
    z.with_data(out)
}

/// GDAL `GDALSlopeHornAlg` (degrees).
fn horn(a: &[f32; 9], res: f64) -> f32 {
    let dx = f64::from((a[0] + a[3] + a[3] + a[6]) - (a[2] + a[5] + a[5] + a[8])) / res;
    let dy = f64::from((a[6] + a[7] + a[7] + a[8]) - (a[0] + a[1] + a[1] + a[2])) / (-res);
    let key = dx * dx + dy * dy;
    ((key.sqrt() / 8.0).atan() * (180.0 / std::f64::consts::PI)) as f32
}

/// Wind shelter index (radians) as `compute_windshelter_numba` of the reference: the
/// `prob` quantile (linear interpolation) of `atan((z_n - z_0) / d_n)` over the valid
/// cells `n` of the disc of radius `max(2, round(radius_m / cell))` cells. Cells closer
/// than the radius to the raster edge are `NaN` (the reference writes the DEM no-data
/// there); a no-data centre gives `NaN` (never used: its slope is no-data).
pub fn windshelter(z: &Raster<f32>, radius_m: f64, prob: f64) -> Raster<f32> {
    let (w, h) = (z.w, z.h);
    let rad = ((radius_m / z.cell).round() as usize).max(2);
    let mut offsets = Vec::new();
    for di in 0..=2 * rad {
        for dj in 0..=2 * rad {
            let (y, x) = (di as i64 - rad as i64, dj as i64 - rad as i64);
            if (x * x + y * y) as usize <= rad * rad && !(x == 0 && y == 0) {
                let d = (((di as f64 - rad as f64).powi(2) + (dj as f64 - rad as f64).powi(2))
                    .sqrt())
                    * z.cell;
                offsets.push((y, x, d));
            }
        }
    }
    let out: Vec<f32> = (0..h)
        .into_par_iter()
        .flat_map_iter(|r| {
            let offsets = &offsets;
            let mut vals: Vec<f64> = Vec::with_capacity(offsets.len());
            (0..w)
                .map(move |c| {
                    if r < rad || r >= h - rad || c < rad || c >= w - rad {
                        return f32::NAN;
                    }
                    let centre = f64::from(z.data[r * w + c]);
                    if centre.is_nan() {
                        return f32::NAN;
                    }
                    vals.clear();
                    for &(y, x, d) in offsets {
                        let v = z.data[(r as i64 + y) as usize * w + (c as i64 + x) as usize];
                        // The reference also drops cells whose elevation is exactly 0.
                        if v.is_nan() || v == 0.0 {
                            continue;
                        }
                        vals.push((f64::from(v) - centre) / d);
                    }
                    if vals.is_empty() {
                        return 0.0;
                    }
                    // The quantile only needs the order statistics i and i + 1 of the
                    // angles; `atan` is monotonic, so they are the `atan` of the same order
                    // statistics of the slopes: select them instead of sorting the ~450
                    // angles of the disc, and take two `atan` instead of all of them.
                    let p = prob * (vals.len() - 1) as f64;
                    let i = p.floor() as usize;
                    let frac = p - i as f64;
                    let (_, lo, above) = vals.select_nth_unstable_by(i, f64::total_cmp);
                    let lo = lo.atan();
                    let v = match above.iter().copied().min_by(f64::total_cmp) {
                        Some(hi) => {
                            let hi = hi.atan();
                            lo + frac * (hi - lo)
                        }
                        None => lo,
                    };
                    v as f32
                })
                .collect::<Vec<_>>()
        })
        .collect();
    z.with_data(out)
}

/// Cauchy membership in Float32, operation for operation as NumPy evaluates
/// `1 / (1 + ((x - c) / a) ** (2 * b))` on a Float32 array.
#[inline]
pub fn cauchy(x: f32, (a, b, c): (f32, f32, f32)) -> f32 {
    1.0 / (1.0 + ((x - c) / a).powf(2.0 * b))
}

/// Continuous PRA (0–1) of a scenario. `slope` with edges computed, `ws` from
/// [`windshelter`], `canopy` in percent (`NaN` = open, as the reference maps the forest
/// no-data to 0). Cells without slope are 0 (the reference leaves a value below 1e-20
/// there, never above any threshold, on cells that are not valid terrain).
pub fn continuous(
    slope: &Raster<f32>,
    ws: &Raster<f32>,
    canopy: &Raster<f32>,
    p: &PraParams,
) -> Raster<f32> {
    let out: Vec<f32> = slope
        .data
        .par_iter()
        .zip(&ws.data)
        .zip(&canopy.data)
        .map(|((&s, &wsv), &f)| {
            if s.is_nan() {
                return 0.0;
            }
            let sc = cauchy(s, p.slope);
            // The border of the wind shelter raster is no-data (-9999 in the reference):
            // its membership underflows to 0.
            let wc = if wsv.is_nan() {
                0.0
            } else {
                cauchy(wsv, p.wind)
            };
            let f = if f.is_nan() { 0.0 } else { f };
            let mut fc = cauchy(f, p.forest);
            if fc <= 0.0 {
                fc = 1.0;
            }
            let m = sc.min(wc).min(fc);
            let mean = (sc + wc + fc) / 3.0;
            let v = (1.0 - m) * m + m * mean;
            if v <= 0.0 { 0.0 } else { v }
        })
        .collect();
    slope.with_data(out)
}

/// Binary PRA: `continuous >= threshold`.
pub fn binary(cont: &Raster<f32>, threshold: f32) -> Raster<u8> {
    cont.with_data(
        cont.data
            .iter()
            .map(|v| u8::from(*v >= threshold))
            .collect(),
    )
}

/// Connected-component labels of the non-zero cells (`conn8`: 8-connectivity, else 4).
/// Returns the label of each cell (0 = background) and the size of each label (index 0
/// unused).
pub fn components(mask: &[u8], w: usize, h: usize, conn8: bool) -> (Vec<u32>, Vec<u32>) {
    let mut label = vec![0u32; w * h];
    let mut sizes = vec![0u32];
    let mut stack = Vec::new();
    for start in 0..w * h {
        if mask[start] == 0 || label[start] != 0 {
            continue;
        }
        let id = sizes.len() as u32;
        sizes.push(0);
        label[start] = id;
        stack.push(start);
        while let Some(i) = stack.pop() {
            sizes[id as usize] += 1;
            let (r, c) = ((i / w) as i64, (i % w) as i64);
            for dr in -1i64..=1 {
                for dc in -1i64..=1 {
                    if (dr == 0 && dc == 0) || (!conn8 && dr != 0 && dc != 0) {
                        continue;
                    }
                    let (rr, cc) = (r + dr, c + dc);
                    if rr < 0 || cc < 0 || rr >= h as i64 || cc >= w as i64 {
                        continue;
                    }
                    let j = rr as usize * w + cc as usize;
                    if mask[j] != 0 && label[j] == 0 {
                        label[j] = id;
                        stack.push(j);
                    }
                }
            }
        }
    }
    (label, sizes)
}

/// GDAL sieve of the binary PRA as the reference applies it (`gdal_sieve`, 4-connected,
/// threshold `round(min_m2 / cell²)` cells, at least 1), then `original AND sieved`:
/// holes are never filled.
///
/// GDAL (`alg/gdalsievefilter.cpp`, GDAL 3.12) labels the 4-connected polygons of both
/// values, gives each polygon its largest neighbour (first met in raster order on ties,
/// comparing each cell with the cell above, then the cell to the left), and replaces a
/// small polygon by the value of the first polygon at least the threshold reached by
/// following largest neighbours. A small PRA patch next to a small gap of a large PRA
/// patch therefore survives (its largest neighbour is the gap, whose largest neighbour is
/// the large patch): simply removing small components would differ there.
pub fn sieve(bin: &Raster<u8>, min_m2: f64) -> Raster<u8> {
    let min_cells = ((min_m2 / (bin.cell * bin.cell)).round() as u32).max(1);
    let (w, h) = (bin.w, bin.h);
    // 4-connected polygons of equal value, ids in raster order of their first cell.
    let mut label = vec![u32::MAX; w * h];
    let mut sizes: Vec<u32> = Vec::new();
    let mut values: Vec<u8> = Vec::new();
    let mut stack = Vec::new();
    for start in 0..w * h {
        if label[start] != u32::MAX {
            continue;
        }
        let id = sizes.len() as u32;
        let v = bin.data[start];
        sizes.push(0);
        values.push(v);
        label[start] = id;
        stack.push(start);
        while let Some(i) = stack.pop() {
            sizes[id as usize] += 1;
            let (r, c) = (i / w, i % w);
            let mut visit = |j: usize| {
                if label[j] == u32::MAX && bin.data[j] == v {
                    label[j] = id;
                    stack.push(j);
                }
            };
            if r > 0 {
                visit(i - w);
            }
            if r + 1 < h {
                visit(i + w);
            }
            if c > 0 {
                visit(i - 1);
            }
            if c + 1 < w {
                visit(i + 1);
            }
        }
    }
    let mut big = vec![-1i64; sizes.len()];
    let mut compare = |a: u32, b: u32| {
        if a == b {
            return;
        }
        let (a, b) = (a as usize, b as usize);
        if big[a] == -1 || sizes[big[a] as usize] < sizes[b] {
            big[a] = b as i64;
        }
        if big[b] == -1 || sizes[big[b] as usize] < sizes[a] {
            big[b] = a as i64;
        }
    };
    for r in 0..h {
        for c in 0..w {
            let i = r * w + c;
            if r > 0 {
                compare(label[i], label[i - w]);
            }
            if c > 0 {
                compare(label[i], label[i - 1]);
            }
        }
    }
    let mut target = vec![-1i64; sizes.len()];
    for p in 0..sizes.len() {
        if sizes[p] >= min_cells || big[p] == -1 {
            continue;
        }
        let mut seen = std::collections::HashSet::from([p]);
        let mut cur = p;
        let found = loop {
            let nb = big[cur];
            if nb < 0 {
                break None;
            }
            let nb = nb as usize;
            if sizes[nb] >= min_cells {
                break Some(nb);
            }
            if !seen.insert(nb) {
                break None;
            }
            cur = nb;
        };
        if let Some(f) = found {
            target[p] = f as i64;
        }
    }
    bin.with_data(
        (0..w * h)
            .map(|i| {
                let p = label[i] as usize;
                let sieved = if target[p] >= 0 {
                    values[target[p] as usize]
                } else {
                    values[p]
                };
                u8::from(bin.data[i] != 0 && sieved != 0)
            })
            .collect(),
    )
}

/// Area (m²) of the 8-connected component of each non-zero cell, 0 elsewhere.
///
/// Stands for the `area_ha` of the reference start-zone polygons
/// (`praSegmentation.py`), which the dev8 size vote reads: the reference splits each
/// component by watershed, then merges every piece below 5000 m² (typical) or
/// 15000 m² (infrequent) into a touching piece, without a size cap; a piece below the
/// vote cut (0.3086 ha) therefore survives only as a whole isolated component. The two
/// cuts of the vote (0.0136 ha and 0.3086 ha) give the same result on component areas
/// and on polygon areas, except for components split into pieces that are all larger
/// than the merge threshold, which do not change the vote either.
pub fn component_area_m2(mask: &Raster<u8>) -> Raster<f32> {
    let (label, sizes) = components(&mask.data, mask.w, mask.h, true);
    let a = (mask.cell * mask.cell) as f32;
    mask.with_data(
        label
            .iter()
            .map(|&l| {
                if l == 0 {
                    0.0
                } else {
                    sizes[l as usize] as f32 * a
                }
            })
            .collect(),
    )
}

/// Exact Euclidean distance (m) from each cell centre to the nearest non-zero cell
/// centre (Felzenszwalb & Huttenlocher 2012 separable transform). `f32::INFINITY` when
/// the mask is empty. GDAL `ComputeProximity`, used by the reference, is a two-pass
/// approximation of the same distance.
pub fn distance_to(mask: &Raster<u8>) -> Raster<f32> {
    let (w, h) = (mask.w, mask.h);
    const INF: f64 = 1e20;
    // Columns first.
    let mut g = vec![INF; w * h];
    let cols: Vec<Vec<f64>> = (0..w)
        .into_par_iter()
        .map(|c| {
            let f: Vec<f64> = (0..h)
                .map(|r| if mask.data[r * w + c] != 0 { 0.0 } else { INF })
                .collect();
            edt_1d(&f)
        })
        .collect();
    for (c, col) in cols.iter().enumerate() {
        for (r, v) in col.iter().enumerate() {
            g[r * w + c] = *v;
        }
    }
    let out: Vec<f32> = g
        .par_chunks(w)
        .flat_map_iter(|row| {
            edt_1d(row).into_iter().map(|d2| {
                if d2 >= INF / 2.0 {
                    f32::INFINITY
                } else {
                    (d2.sqrt() * mask.cell) as f32
                }
            })
        })
        .collect();
    mask.with_data(out)
}

/// Output band type of [`gdal_proximity`]: it changes the result.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProximityBand {
    /// Float32 band (reference runout proximity): the first sweep is kept in Float32.
    Float32,
    /// Int16 band (reference PRA proximity): GDAL uses the output band as its work band,
    /// so the first sweep is stored rounded to whole *pixels* before the second sweep,
    /// and the final metres are rounded again. Cells without target get 32767.
    Int16,
}

/// Distance (m) to the nearest non-zero cell exactly as GDAL `ComputeProximity` with
/// `DISTUNITS=GEO` computes it (`alg/gdalproximity.cpp`, GDAL 3.12): a top-down and a
/// bottom-up sweep, each scanning every line both ways and keeping the nearest target
/// among those of the cell above (below), the previous cell and the next cell. With a
/// Float32 band it is close to the exact Euclidean distance; with an Int16 band it is
/// coarser (see [`ProximityBand`]). The reference ATES classifier uses both, so the dev8
/// port does too. `NaN` where no target exists (Float32).
pub fn gdal_proximity(mask: &Raster<u8>, band: ProximityBand) -> Raster<f32> {
    let (w, h) = (mask.w, mask.h);
    let max_dist = (w + h) as f64;
    let mut prox = vec![-1.0f32; w * h];
    let mut near_x = vec![-1i64; w];
    let mut near_y = vec![-1i64; w];
    let line =
        |row: usize, forward: bool, prox: &mut [f32], near_x: &mut [i64], near_y: &mut [i64]| {
            let src = &mask.data[row * w..(row + 1) * w];
            let iline = row as i64;
            let cols: Box<dyn Iterator<Item = usize>> = if forward {
                Box::new(0..w)
            } else {
                Box::new((0..w).rev())
            };
            for p in cols {
                if src[p] != 0 {
                    prox[p] = 0.0;
                    near_x[p] = p as i64;
                    near_y[p] = iline;
                    continue;
                }
                let sq = |nx: i64, ny: i64| {
                    let (dx, dy) = ((nx - p as i64) as f64, (ny - iline) as f64);
                    dx * dx + dy * dy
                };
                let mut best = max_dist.max(w as f64) * max_dist.max(w as f64) * 2.0;
                if near_x[p] != -1 {
                    let d = sq(near_x[p], near_y[p]);
                    if d < best {
                        best = d;
                    } else {
                        near_x[p] = -1;
                        near_y[p] = -1;
                    }
                }
                let last = if forward {
                    p.checked_sub(1)
                } else {
                    Some(p + 1).filter(|q| *q < w)
                };
                if let Some(l) = last
                    && near_x[l] != -1
                {
                    let d = sq(near_x[l], near_y[l]);
                    if d < best {
                        best = d;
                        near_x[p] = near_x[l];
                        near_y[p] = near_y[l];
                    }
                }
                let next = if forward {
                    Some(p + 1).filter(|q| *q < w)
                } else {
                    p.checked_sub(1)
                };
                if let Some(n) = next
                    && near_x[n] != -1
                {
                    let d = sq(near_x[n], near_y[n]);
                    if d < best {
                        best = d;
                        near_x[p] = near_x[n];
                        near_y[p] = near_y[n];
                    }
                }
                if near_x[p] != -1
                    && best <= max_dist * max_dist
                    && (prox[p] < 0.0 || best < f64::from(prox[p]) * f64::from(prox[p]))
                {
                    prox[p] = best.sqrt() as f32;
                }
            }
        };
    for row in 0..h {
        let pl = &mut prox[row * w..(row + 1) * w];
        line(row, true, pl, &mut near_x, &mut near_y);
        line(row, false, pl, &mut near_x, &mut near_y);
        if band == ProximityBand::Int16 {
            // Written to (and read back from) the Int16 output band.
            for v in pl.iter_mut() {
                *v = v.round();
            }
        }
    }
    near_x.fill(-1);
    near_y.fill(-1);
    for row in (0..h).rev() {
        let pl = &mut prox[row * w..(row + 1) * w];
        line(row, false, pl, &mut near_x, &mut near_y);
        line(row, true, pl, &mut near_x, &mut near_y);
    }
    let mult = mask.cell as f32;
    mask.with_data(
        prox.into_iter()
            .map(|v| match band {
                ProximityBand::Float32 if v < 0.0 => f32::NAN,
                ProximityBand::Float32 => v * mult,
                // GDAL no-data 65535 saturates to the Int16 maximum.
                ProximityBand::Int16 if v < 0.0 => 32767.0,
                ProximityBand::Int16 => (v * mult).round().min(32767.0),
            })
            .collect(),
    )
}

/// Squared distance transform of a 1-D sampled function (lower envelope of parabolas).
fn edt_1d(f: &[f64]) -> Vec<f64> {
    let n = f.len();
    let mut d = vec![0.0; n];
    if n == 0 {
        return d;
    }
    let mut v = vec![0usize; n];
    let mut z = vec![0.0f64; n + 1];
    let mut k = 0usize;
    z[0] = f64::NEG_INFINITY;
    z[1] = f64::INFINITY;
    let inter = |q: usize, p: usize| {
        ((f[q] + (q * q) as f64) - (f[p] + (p * p) as f64)) / (2.0 * (q - p) as f64)
    };
    for q in 1..n {
        let mut s = inter(q, v[k]);
        // z[0] is -inf, so k never goes below 0.
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
    for (q, out) in d.iter_mut().enumerate() {
        while z[k + 1] < q as f64 {
            k += 1;
        }
        let p = v[k];
        let dq = q as f64 - p as f64;
        *out = dq * dq + f[p];
    }
    d
}

/// PRA layers of one scenario.
#[derive(Debug, Clone)]
pub struct Pra {
    /// Scenario parameters.
    pub params: PraParams,
    /// Continuous PRA (0–1).
    pub continuous: Raster<f32>,
    /// Binary PRA before the sieve.
    pub binary: Raster<u8>,
    /// Release cells (binary after the sieve).
    pub release: Raster<u8>,
}

/// PRA of one scenario from the shared terrain layers.
pub fn pra(
    slope_edges: &Raster<f32>,
    ws: &Raster<f32>,
    canopy: &Raster<f32>,
    p: &PraParams,
) -> Pra {
    let continuous = continuous(slope_edges, ws, canopy, p);
    let binary = binary(&continuous, p.threshold);
    let release = sieve(&binary, p.sieve_m2);
    Pra {
        params: *p,
        continuous,
        binary,
        release,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn r<T: Clone>(w: usize, h: usize, data: Vec<T>) -> Raster<T> {
        Raster {
            w,
            h,
            cell: 5.0,
            origin_x: 1000.0,
            origin_y: 2000.0,
            data,
        }
    }

    // DEM and GDAL 3.12.3 outputs of `scratchpad/scripts/gdal_slope_probe.py` (seeded
    // random surface, one no-data cell at row 2, column 4).
    #[allow(clippy::excessive_precision)]
    const Z: [[f32; 7]; 6] = [
        [
            1015.3546752929688,
            1032.513916015625,
            1012.3247680664062,
            1040.45947265625,
            1025.35498046875,
            1032.6998291015625,
            1048.8310546875,
        ],
        [
            1019.2760009765625,
            1027.48779296875,
            1015.8267822265625,
            1041.6053466796875,
            1039.144287109375,
            1036.8919677734375,
            1054.65283203125,
        ],
        [
            1023.0958251953125,
            1031.60498046875,
            1026.021240234375,
            1038.0933837890625,
            f32::NAN,
            1041.869384765625,
            1060.510986328125,
        ],
        [
            1029.4122314453125,
            1039.5557861328125,
            1058.422119140625,
            1061.8497314453125,
            1058.74365234375,
            1057.23681640625,
            1053.3067626953125,
        ],
        [
            1032.819580078125,
            1061.0977783203125,
            1051.4820556640625,
            1043.4759521484375,
            1062.7047119140625,
            1071.300537109375,
            1070.39013671875,
        ],
        [
            1062.5189208984375,
            1040.187744140625,
            1058.857666015625,
            1060.780029296875,
            1052.8704833984375,
            1074.2398681640625,
            1084.5789794921875,
        ],
    ];
    #[allow(clippy::excessive_precision)]
    const GDAL_EDGES: [[f32; 7]; 6] = [
        [
            60.23645782470703,
            18.27777671813965,
            38.99922180175781,
            58.46964645385742,
            61.207763671875,
            69.9041519165039,
            62.76511764526367,
        ],
        [
            65.96605682373047,
            27.510629653930664,
            50.78192901611328,
            63.2508430480957,
            42.69538116455078,
            65.67277526855469,
            74.89045715332031,
        ],
        [
            63.86988067626953,
            64.27965545654297,
            72.57698822021484,
            70.54788208007812,
            f32::NAN,
            62.11945724487305,
            68.6574935913086,
        ],
        [
            71.64962768554688,
            72.02765655517578,
            66.52174377441406,
            56.49100875854492,
            50.333885192871094,
            61.67485046386719,
            47.83864974975586,
        ],
        [
            75.91382598876953,
            60.840293884277344,
            10.83376407623291,
            24.7152099609375,
            58.246864318847656,
            61.12917709350586,
            72.311767578125,
        ],
        [
            76.21421813964844,
            23.570249557495117,
            64.88478088378906,
            59.759037017822266,
            53.395721435546875,
            72.70630645751953,
            68.19219207763672,
        ],
    ];

    #[test]
    fn slope_matches_gdal_bit_for_bit() {
        let z = r(7, 6, Z.iter().flatten().copied().collect());
        let s = gdal_slope(&z, true);
        for (i, (a, b)) in s.data.iter().zip(GDAL_EDGES.iter().flatten()).enumerate() {
            assert!(
                (a.is_nan() && b.is_nan()) || a.to_bits() == b.to_bits(),
                "cell {i}: {a} vs GDAL {b}"
            );
        }
        // Without edges: GDAL keeps only inner cells whose window has no no-data.
        let s = gdal_slope(&z, false);
        let kept: Vec<usize> = (0..42).filter(|i| !s.data[*i].is_nan()).collect();
        assert_eq!(kept, vec![8, 9, 15, 16, 22, 23, 29, 30, 31, 32, 33]);
        for i in kept {
            assert_eq!(s.data[i].to_bits(), GDAL_EDGES[i / 7][i % 7].to_bits());
        }
    }

    #[test]
    fn sieve_matches_gdal() {
        // `gdal_sieve` threshold 3, 4-connected (same probe script).
        let m: Vec<u8> = [
            [1, 0, 1, 1, 0, 0, 0],
            [0, 0, 0, 0, 0, 0, 0],
            [1, 1, 1, 0, 0, 0, 0],
            [0, 0, 0, 0, 0, 0, 0],
            [1, 0, 0, 0, 1, 0, 0],
            [1, 1, 0, 0, 0, 1, 0],
        ]
        .iter()
        .flatten()
        .copied()
        .collect();
        let expected: Vec<u8> = [
            [0, 0, 0, 0, 0, 0, 0],
            [0, 0, 0, 0, 0, 0, 0],
            [1, 1, 1, 0, 0, 0, 0],
            [0, 0, 0, 0, 0, 0, 0],
            [1, 0, 0, 0, 0, 0, 0],
            [1, 1, 0, 0, 0, 0, 0],
        ]
        .iter()
        .flatten()
        .copied()
        .collect();
        // 3 cells at 5 m = 75 m².
        assert_eq!(sieve(&r(7, 6, m), 75.0).data, expected);
        // A one-cell PRA island inside an 8-cell gap of a large PRA patch: GDAL merges
        // the island into the gap's largest neighbour (PRA), so it survives (gdal_sieve
        // threshold 10, checked with GDAL 3.12.3).
        let mut m = vec![1u8; 81];
        for rr in 3..6 {
            for cc in 3..6 {
                m[rr * 9 + cc] = 0;
            }
        }
        m[4 * 9 + 4] = 1;
        for i in [8, 72, 73, 74, 63] {
            m[i] = 0;
        }
        assert_eq!(sieve(&r(9, 9, m.clone()), 250.0).data, m);
    }

    #[test]
    fn distance_matches_gdal_proximity() {
        let mut m = vec![0u8; 42];
        m[8] = 1;
        m[4 * 7 + 5] = 1;
        m[4 * 7 + 6] = 1;
        let mr = r(7, 6, m);
        let d = gdal_proximity(&mr, ProximityBand::Int16);
        // GDAL ComputeProximity into an Int16 band, same probe script: cell (2, 0) is
        // 7.07 m from the target (1, 1) but GDAL says 5 (first sweep rounded to pixels).
        let gdal: [[i32; 7]; 6] = [
            [7, 5, 7, 11, 16, 20, 20],
            [5, 0, 5, 10, 15, 15, 15],
            [5, 5, 5, 10, 11, 10, 10],
            [10, 10, 10, 11, 7, 5, 5],
            [15, 15, 15, 10, 5, 0, 0],
            [20, 20, 15, 10, 5, 5, 5],
        ];
        for (i, g) in gdal.iter().flatten().enumerate() {
            assert_eq!(d.data[i].round() as i32, *g, "cell {i}");
        }
        assert!(gdal_proximity(&r(2, 1, vec![0, 0]), ProximityBand::Float32).data[0].is_nan());
        assert_eq!(
            gdal_proximity(&r(2, 1, vec![0, 0]), ProximityBand::Int16).data[0],
            32767.0
        );
        // Float32 band, one target at (1, 1): GDAL 3.12.3 gives the exact distance here.
        let mut one = vec![0u8; 42];
        one[8] = 1;
        let f = gdal_proximity(&r(7, 6, one), ProximityBand::Float32);
        assert!((f.data[14] - 7.071068).abs() < 1e-5);
        assert!((f.data[41] - 32.01562).abs() < 1e-4);
        let i16 = gdal_proximity(&r(7, 6, vec![0u8; 42]), ProximityBand::Int16);
        assert!(i16.data.iter().all(|v| *v == 32767.0));
        let mut one = vec![0u8; 42];
        one[8] = 1;
        let g = gdal_proximity(&r(7, 6, one), ProximityBand::Int16);
        // GDAL Int16 output of the same single target (debug run above).
        let expected: [[i32; 7]; 6] = [
            [7, 5, 7, 11, 16, 21, 25],
            [5, 0, 5, 10, 15, 20, 25],
            [5, 5, 5, 10, 15, 20, 25],
            [10, 10, 10, 15, 20, 20, 25],
            [15, 15, 15, 20, 20, 25, 30],
            [20, 20, 20, 20, 25, 30, 30],
        ];
        for (i, v) in expected.iter().flatten().enumerate() {
            assert_eq!(g.data[i] as i32, *v, "single target, cell {i}");
        }
        // The exact transform.
        let e = distance_to(&mr);
        for row in 0..6 {
            for col in 0..7 {
                let exact = [(1, 1), (4, 5), (4, 6)]
                    .iter()
                    .map(|&(rr, cc): &(i32, i32)| {
                        5.0 * f64::from((rr - row).pow(2) + (cc - col).pow(2)).sqrt()
                    })
                    .fold(f64::INFINITY, f64::min);
                assert!((f64::from(e.data[(row * 7 + col) as usize]) - exact).abs() < 1e-4);
            }
        }
        assert!(distance_to(&r(2, 1, vec![0, 0])).data[0].is_infinite());
    }

    #[test]
    fn components_and_areas() {
        // Diagonal neighbours: one 8-connected component, two 4-connected ones.
        let m = vec![1u8, 0, 0, 1];
        let (_, s4) = components(&m, 2, 2, false);
        let (_, s8) = components(&m, 2, 2, true);
        assert_eq!(s4.len() - 1, 2);
        assert_eq!(s8, vec![0, 2]);
        let a = component_area_m2(&r(2, 2, m));
        assert_eq!(a.data, vec![50.0, 0.0, 0.0, 50.0]);
    }

    #[test]
    fn memberships() {
        // Flat terrain: wind shelter 0 gives 0.5; open slope at the centre gives 1.
        assert_eq!(cauchy(0.0, FREQUENT.wind), 0.5);
        assert_eq!(cauchy(42.5, FREQUENT.slope), 1.0);
        assert!(cauchy(0.0, FREQUENT.forest) == 1.0);
        assert!(cauchy(100.0, FREQUENT.forest) < 0.02);
        let s = r(2, 1, vec![42.5f32, f32::NAN]);
        let ws = r(2, 1, vec![0.0f32, 0.0]);
        let f = r(2, 1, vec![f32::NAN, 0.0]);
        let c = continuous(&s, &ws, &f, &FREQUENT);
        // m = 0.5, mean = (1 + 0.5 + 1) / 3.
        assert!((c.data[0] - (0.25 + 0.5 * 2.5 / 3.0)).abs() < 1e-6);
        assert_eq!(c.data[1], 0.0);
    }

    #[test]
    fn windshelter_selection_matches_the_sorted_quantile() {
        // Rough terrain with no-data holes and exact zeros: the selection of two order
        // statistics gives the quantile of the fully sorted angles, bit for bit.
        let (w, h) = (61usize, 47usize);
        let mut seed = 0x2545_f491_4f6c_dd1du64;
        let mut z = Vec::with_capacity(w * h);
        for i in 0..w * h {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            let v =
                1800.0 + (i % w) as f32 * 3.7 - (i / w) as f32 * 2.1 + (seed % 4000) as f32 / 97.0;
            z.push(match seed % 53 {
                0 => f32::NAN,
                1 => 0.0,
                _ => v,
            });
        }
        let dem = r(w, h, z.clone());
        for prob in [0.5, 0.25, 0.9, 1.0, 0.0] {
            let fast = windshelter(&dem, 60.0, prob);
            let rad = 12usize;
            for row in rad..h - rad {
                for col in rad..w - rad {
                    let centre = f64::from(z[row * w + col]);
                    if centre.is_nan() {
                        continue;
                    }
                    let mut vals = Vec::new();
                    for di in 0..=2 * rad {
                        for dj in 0..=2 * rad {
                            let (y, x) = (di as i64 - rad as i64, dj as i64 - rad as i64);
                            if (x * x + y * y) as usize > rad * rad || (x == 0 && y == 0) {
                                continue;
                            }
                            let d = (((di as f64 - rad as f64).powi(2)
                                + (dj as f64 - rad as f64).powi(2))
                            .sqrt())
                                * 5.0;
                            let v = z[(row as i64 + y) as usize * w + (col as i64 + x) as usize];
                            if v.is_nan() || v == 0.0 {
                                continue;
                            }
                            vals.push(((f64::from(v) - centre) / d).atan());
                        }
                    }
                    vals.sort_by(f64::total_cmp);
                    let p = prob * (vals.len() - 1) as f64;
                    let i = p.floor() as usize;
                    let frac = p - i as f64;
                    let v = if i + 1 < vals.len() {
                        vals[i] + frac * (vals[i + 1] - vals[i])
                    } else {
                        vals[i]
                    };
                    assert_eq!(
                        fast.data[row * w + col].to_bits(),
                        (v as f32).to_bits(),
                        "{row} {col} {prob}"
                    );
                }
            }
        }
    }

    #[test]
    fn windshelter_hollow_is_positive() {
        // A bowl: the centre is lower than everything around it.
        let n = 31;
        let mut z = Vec::new();
        for r in 0..n {
            for c in 0..n {
                let (y, x) = (r as f32 - 15.0, c as f32 - 15.0);
                z.push(1000.0 + 0.5 * (x * x + y * y).sqrt() * 5.0);
            }
        }
        let ws = windshelter(&r(n, n, z), 60.0, 0.5);
        assert!(ws.data[15 * n + 15] > 0.0);
        assert!(ws.data[0].is_nan()); // border frame (radius 12 cells)
        assert!(ws.data[11 * n + 15].is_nan());
        assert!(!ws.data[12 * n + 15].is_nan());
    }
}
