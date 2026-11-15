//! Plans drawn in perspective (panoramas): the point of view is estimated on the relief.
//!
//! A panorama is a view of the terrain from a point in the air: no similarity carries it
//! onto the map. The labels are paired with landmarks whose height is read on the LiDAR
//! HD terrain model (`skimap-terrain`, 5 m, averaged to [`DEM_CELL_M`]); a RANSAC over six
//! pairs fits a projective camera (DLT: position, heading, tilt, focal length and the
//! skew of a drawing in one 3 × 4 matrix), kept when at least [`MIN_INLIERS`] labels
//! agree within the tolerance and the camera stands above the terrain, every inlier in
//! front of it. A page point is then carried onto the map by casting its ray onto the
//! terrain (first crossing); the non-rigid ICP of the flat plans starts from that mapping
//! and corrects the artist's liberties as it does for a flat plan.

use rayon::prelude::*;
use serde::Serialize;
use skimap_common::grid::Grid;

use crate::align::LabelMatch;
use crate::warp::P2;

/// Cell of the terrain model used to cast the rays, metres.
pub const DEM_CELL_M: f64 = 25.0;
/// Fewest labels agreeing with a camera (six determine it).
pub const MIN_INLIERS: usize = 8;
/// RANSAC samples.
pub const SAMPLES: usize = 40_000;
/// Step of the ray march, metres along the ray.
pub const RAY_STEP_M: f64 = 12.5;
/// Longest ray, metres.
pub const RAY_MAX_M: f64 = 80_000.0;

/// Terrain heights for the rays (Lambert-93, IGN69).
pub struct Dem {
    grid: Grid<f32>,
}

impl Dem {
    /// Terrain model of an extent at [`DEM_CELL_M`] (5 m tiles averaged).
    pub fn new(grid5: &Grid<f32>) -> Dem {
        let f = (DEM_CELL_M / skimap_terrain::dem::CELL).round().max(1.0) as usize;
        Dem {
            grid: skimap_terrain::dem::block_mean(grid5, f),
        }
    }

    /// Height at a point (bilinear), `None` outside or on no-data.
    pub fn z(&self, x: f64, y: f64) -> Option<f64> {
        let (c, r) = self.grid.transform().world_to_pixel(x, y);
        let (c, r) = (c - 0.5, r - 0.5);
        if c < 0.0 || r < 0.0 {
            return None;
        }
        let (c0, r0) = (c.floor() as usize, r.floor() as usize);
        let (fc, fr) = (c - c0 as f64, r - r0 as f64);
        let g = |cc: usize, rr: usize| {
            self.grid
                .get(cc, rr)
                .copied()
                .filter(|v| v.is_finite())
                .map(f64::from)
        };
        let (a, b, cc, d) = (
            g(c0, r0)?,
            g(c0 + 1, r0)?,
            g(c0, r0 + 1)?,
            g(c0 + 1, r0 + 1)?,
        );
        Some((a * (1.0 - fc) + b * fc) * (1.0 - fr) + (cc * (1.0 - fc) + d * fc) * fr)
    }

    /// Lowest height of the model.
    pub fn min_z(&self) -> f64 {
        self.grid
            .data()
            .iter()
            .filter(|v| v.is_finite())
            .fold(f64::INFINITY, |m, v| m.min(f64::from(*v)))
    }
}

/// Eigenvector of the smallest eigenvalue of a symmetric matrix (cyclic Jacobi).
#[allow(clippy::needless_range_loop)] // Jacobi rotations index two columns at once.
pub fn smallest_eigenvector<const N: usize>(mut a: [[f64; N]; N]) -> [f64; N] {
    let mut v = [[0.0; N]; N];
    for (i, row) in v.iter_mut().enumerate() {
        row[i] = 1.0;
    }
    for _ in 0..60 {
        let off: f64 = (0..N)
            .flat_map(|i| (0..N).filter(move |&j| j != i).map(move |j| (i, j)))
            .map(|(i, j)| a[i][j] * a[i][j])
            .sum();
        if off < 1e-22 {
            break;
        }
        for p in 0..N {
            for q in p + 1..N {
                if a[p][q].abs() < 1e-300 {
                    continue;
                }
                let theta = (a[q][q] - a[p][p]) / (2.0 * a[p][q]);
                let t = theta.signum() / (theta.abs() + (theta * theta + 1.0).sqrt());
                let t = if theta == 0.0 { 1.0 } else { t };
                let c = 1.0 / (t * t + 1.0).sqrt();
                let s = t * c;
                for k in 0..N {
                    let (akp, akq) = (a[k][p], a[k][q]);
                    a[k][p] = c * akp - s * akq;
                    a[k][q] = s * akp + c * akq;
                }
                for k in 0..N {
                    let (apk, aqk) = (a[p][k], a[q][k]);
                    a[p][k] = c * apk - s * aqk;
                    a[q][k] = s * apk + c * aqk;
                }
                for row in v.iter_mut() {
                    let (vkp, vkq) = (row[p], row[q]);
                    row[p] = c * vkp - s * vkq;
                    row[q] = s * vkp + c * vkq;
                }
            }
        }
    }
    let k = (0..N)
        .min_by(|&i, &j| a[i][i].total_cmp(&a[j][j]))
        .unwrap_or(0);
    let mut out = [0.0; N];
    for (i, o) in out.iter_mut().enumerate() {
        *o = v[i][k];
    }
    out
}

/// A projective camera: page point = P · (X − origin, 1), Lambert-93 metres and heights.
#[derive(Debug, Clone, Serialize)]
pub struct Camera {
    /// 3 × 4 matrix on world coordinates relative to `origin`.
    pub p: [[f64; 4]; 3],
    /// World origin (Lambert-93 x, y, height).
    pub origin: [f64; 3],
    /// Camera centre (Lambert-93 x, y, height).
    pub centre: [f64; 3],
}

impl Camera {
    /// Page point and depth sign of a world point (`None` behind the camera).
    pub fn project(&self, w: [f64; 3]) -> Option<P2> {
        let x = [
            w[0] - self.origin[0],
            w[1] - self.origin[1],
            w[2] - self.origin[2],
        ];
        let r = |i: usize| {
            self.p[i][0] * x[0] + self.p[i][1] * x[1] + self.p[i][2] * x[2] + self.p[i][3]
        };
        let h = r(2);
        (h > 1e-12).then(|| [r(0) / h, r(1) / h])
    }

    /// Direct linear transform on at least six world ↔ page pairs (Hartley
    /// normalisation); the sign makes the first point in front.
    pub fn fit(world: &[[f64; 3]], page: &[P2]) -> Option<Camera> {
        let n = world.len();
        if n < 6 || page.len() != n {
            return None;
        }
        let mean3 = |i: usize| world.iter().map(|w| w[i]).sum::<f64>() / n as f64;
        let origin = [mean3(0), mean3(1), mean3(2)];
        let sw = (world
            .iter()
            .map(|w| {
                ((w[0] - origin[0]).powi(2)
                    + (w[1] - origin[1]).powi(2)
                    + (w[2] - origin[2]).powi(2))
                .sqrt()
            })
            .sum::<f64>()
            / n as f64)
            .max(1e-9)
            / 3f64.sqrt();
        let pc = [
            page.iter().map(|p| p[0]).sum::<f64>() / n as f64,
            page.iter().map(|p| p[1]).sum::<f64>() / n as f64,
        ];
        let sp = (page
            .iter()
            .map(|p| ((p[0] - pc[0]).powi(2) + (p[1] - pc[1]).powi(2)).sqrt())
            .sum::<f64>()
            / n as f64)
            .max(1e-9)
            / 2f64.sqrt();
        let mut ata = [[0.0; 12]; 12];
        for (w, p) in world.iter().zip(page) {
            let x = [
                (w[0] - origin[0]) / sw,
                (w[1] - origin[1]) / sw,
                (w[2] - origin[2]) / sw,
                1.0,
            ];
            let (u, v) = ((p[0] - pc[0]) / sp, (p[1] - pc[1]) / sp);
            let mut r1 = [0.0; 12];
            let mut r2 = [0.0; 12];
            for k in 0..4 {
                r1[k] = x[k];
                r1[8 + k] = -u * x[k];
                r2[4 + k] = x[k];
                r2[8 + k] = -v * x[k];
            }
            for i in 0..12 {
                for j in 0..12 {
                    ata[i][j] += r1[i] * r1[j] + r2[i] * r2[j];
                }
            }
        }
        let h = smallest_eigenvector(ata);
        // Denormalise: P = Tp⁻¹ · H · Tw.
        let mut p = [[0.0; 4]; 3];
        for i in 0..3 {
            for k in 0..4 {
                let hk = h[i * 4 + k];
                let hk = if k < 3 { hk / sw } else { hk };
                p[i][k] = hk;
            }
        }
        // Tp⁻¹: u = sp·u' + pc.
        let row2 = p[2];
        for i in 0..2 {
            for k in 0..4 {
                p[i][k] = sp * p[i][k] + pc[i] * row2[k];
            }
        }
        // Centre: M · C = −p4.
        let m = vec![
            p[0][0], p[0][1], p[0][2], p[1][0], p[1][1], p[1][2], p[2][0], p[2][1], p[2][2],
        ];
        let c = crate::linalg::solve(m, 3, vec![-p[0][3], -p[1][3], -p[2][3]], 1)?;
        let mut cam = Camera {
            p,
            origin,
            centre: [c[0] + origin[0], c[1] + origin[1], c[2] + origin[2]],
        };
        let depth = |cam: &Camera, w: [f64; 3]| {
            (0..3)
                .map(|k| cam.p[2][k] * (w[k] - cam.origin[k]))
                .sum::<f64>()
                + cam.p[2][3]
        };
        if depth(&cam, world[0]) < 0.0 {
            for row in &mut cam.p {
                for v in row.iter_mut() {
                    *v = -*v;
                }
            }
        }
        cam.p.iter().flatten().all(|v| v.is_finite()).then_some(cam)
    }

    /// Ground point seen at a page point: first crossing of its ray with the terrain.
    pub fn ground(&self, dem: &Dem, q: P2) -> Option<P2> {
        // Ray direction: M⁻¹ (u, v, 1), oriented so that depth grows.
        let m = vec![
            self.p[0][0],
            self.p[0][1],
            self.p[0][2],
            self.p[1][0],
            self.p[1][1],
            self.p[1][2],
            self.p[2][0],
            self.p[2][1],
            self.p[2][2],
        ];
        let d = crate::linalg::solve(m, 3, vec![q[0], q[1], 1.0], 1)?;
        let depth_rate: f64 = (0..3).map(|k| self.p[2][k] * d[k]).sum();
        let s = if depth_rate < 0.0 { -1.0 } else { 1.0 };
        let norm = (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt();
        if norm <= 0.0 {
            return None;
        }
        let dir = [s * d[0] / norm, s * d[1] / norm, s * d[2] / norm];
        let at = |t: f64| {
            [
                self.centre[0] + t * dir[0],
                self.centre[1] + t * dir[1],
                self.centre[2] + t * dir[2],
            ]
        };
        let above = |t: f64| -> Option<bool> {
            let x = at(t);
            dem.z(x[0], x[1]).map(|z| x[2] > z)
        };
        let mut t = 0.0;
        let mut seen = false;
        let mut prev = t;
        while t < RAY_MAX_M {
            match above(t) {
                Some(true) => seen = true,
                Some(false) if seen => {
                    // Bisection between the last point above and this one.
                    let (mut lo, mut hi) = (prev, t);
                    for _ in 0..20 {
                        let mid = 0.5 * (lo + hi);
                        if above(mid) == Some(false) {
                            hi = mid;
                        } else {
                            lo = mid;
                        }
                    }
                    let x = at(hi);
                    return Some([x[0], x[1]]);
                }
                Some(false) => return None,
                None => {}
            }
            prev = t;
            t += RAY_STEP_M;
        }
        None
    }
}

/// A perspective placement.
#[derive(Debug, Clone)]
pub struct Placement {
    /// The camera.
    pub camera: Camera,
    /// Label ↔ landmark pairs agreeing with it (one per label and per landmark).
    pub inliers: Vec<LabelMatch>,
    /// Median reprojection error of the inliers, page points.
    pub median_px_pt: f64,
}

fn inliers(cam: &Camera, cands: &[(LabelMatch, f64)], tol_pt: f64) -> Vec<usize> {
    let mut best: std::collections::BTreeMap<usize, (f64, usize)> = Default::default();
    for (i, (c, z)) in cands.iter().enumerate() {
        let Some(q) = cam.project([c.map[0], c.map[1], *z]) else {
            continue;
        };
        let r = crate::warp::dist(q, c.plan);
        if r <= tol_pt {
            let e = best.entry(c.label_idx).or_insert((f64::MAX, i));
            if r < e.0 {
                *e = (r, i);
            }
        }
    }
    let mut used = std::collections::BTreeSet::new();
    let mut idx: Vec<(f64, usize)> = best.values().copied().collect();
    idx.sort_by(|a, b| a.0.total_cmp(&b.0));
    idx.into_iter()
        .filter(|(_, i)| used.insert(cands[*i].0.landmark))
        .map(|(_, i)| i)
        .collect()
}

/// RANSAC of a projective camera on label ↔ landmark candidates whose landmark lies on
/// the terrain model (height from it). Deterministic (fixed seeds). The camera must stand
/// above the terrain under it (or above its lowest point outside the model).
pub fn place(cands: &[LabelMatch], dem: &Dem, tol_pt: f64) -> Option<Placement> {
    let cz: Vec<(LabelMatch, f64)> = cands
        .iter()
        .filter_map(|c| Some((c.clone(), dem.z(c.map[0], c.map[1])?)))
        .collect();
    let labels: std::collections::BTreeSet<usize> = cz.iter().map(|c| c.0.label_idx).collect();
    if labels.len() < MIN_INLIERS {
        return None;
    }
    let n = cz.len();
    let min_z = dem.min_z();
    let plausible = |cam: &Camera| {
        let c = cam.centre;
        c.iter().all(|v| v.is_finite()) && c[2] > dem.z(c[0], c[1]).unwrap_or(min_z) + 50.0
    };
    let fit = |idx: &[usize]| {
        let w: Vec<[f64; 3]> = idx
            .iter()
            .map(|&i| [cz[i].0.map[0], cz[i].0.map[1], cz[i].1])
            .collect();
        let p: Vec<P2> = idx.iter().map(|&i| cz[i].0.plan).collect();
        let cam = Camera::fit(&w, &p)?;
        let front = w.iter().all(|x| cam.project(*x).is_some());
        (front && plausible(&cam)).then_some(cam)
    };
    let best = (0..SAMPLES)
        .into_par_iter()
        .filter_map(|s| {
            // Six candidates of distinct labels and landmarks (xorshift, seed per sample).
            let mut x = (s as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1;
            let mut next = || {
                x ^= x << 13;
                x ^= x >> 7;
                x ^= x << 17;
                (x % n as u64) as usize
            };
            let mut idx: Vec<usize> = Vec::with_capacity(6);
            for _ in 0..60 {
                let i = next();
                if idx.iter().all(|&j| {
                    cz[j].0.label_idx != cz[i].0.label_idx
                        && cz[j].0.landmark != cz[i].0.landmark
                        && crate::warp::dist(cz[j].0.plan, cz[i].0.plan) > 20.0
                }) {
                    idx.push(i);
                    if idx.len() == 6 {
                        break;
                    }
                }
            }
            if idx.len() < 6 {
                return None;
            }
            let cam = fit(&idx)?;
            let k = inliers(&cam, &cz, tol_pt).len();
            Some((k, s, cam))
        })
        .max_by(|a, b| a.0.cmp(&b.0).then(b.1.cmp(&a.1)))?;
    let (_, _, mut cam) = best;
    let mut inl = inliers(&cam, &cz, tol_pt);
    for _ in 0..4 {
        if inl.len() < MIN_INLIERS {
            return None;
        }
        let Some(next) = fit(&inl) else { break };
        let ni = inliers(&next, &cz, tol_pt);
        if ni.len() < inl.len() {
            break;
        }
        cam = next;
        inl = ni;
    }
    if inl.len() < MIN_INLIERS {
        return None;
    }
    let mut res: Vec<f64> = inl
        .iter()
        .filter_map(|&i| {
            let q = cam.project([cz[i].0.map[0], cz[i].0.map[1], cz[i].1])?;
            Some(crate::warp::dist(q, cz[i].0.plan))
        })
        .collect();
    res.sort_by(f64::total_cmp);
    Some(Placement {
        camera: cam,
        inliers: inl.iter().map(|&i| cz[i].0.clone()).collect(),
        median_px_pt: res.get(res.len() / 2).copied().unwrap_or(f64::NAN),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use skimap_common::crs::Crs;
    use skimap_common::grid::GeoTransform;

    /// A synthetic valley: z = 1000 + 0.2·|x − 5000| over 10 × 10 km at 25 m.
    fn dem() -> Dem {
        let n = 400;
        let gt = GeoTransform::north_up(0.0, 10_000.0, 25.0);
        let mut g = Grid::filled(n, n, gt, Crs::Lambert93, 0.0f32);
        for r in 0..n {
            for c in 0..n {
                let (x, _) = gt.cell_center(c, r);
                g.data_mut()[r * n + c] = (1000.0 + 0.2 * (x - 5000.0).abs()) as f32;
            }
        }
        Dem { grid: g }
    }

    /// A pinhole camera at (5000, −3000, 3000) looking north and down.
    fn camera() -> Camera {
        let c = [5000.0, -3000.0, 3000.0];
        let f = 800.0;
        // Axes: right = +x; forward = (0, cos a, −sin a); down = forward × right.
        let a: f64 = 0.35;
        let fw = [0.0, a.cos(), -a.sin()];
        let right = [1.0, 0.0, 0.0];
        let down = [
            fw[1] * right[2] - fw[2] * right[1],
            fw[2] * right[0] - fw[0] * right[2],
            fw[0] * right[1] - fw[1] * right[0],
        ];
        let rows = [right, down, fw];
        let mut p = [[0.0; 4]; 3];
        for i in 0..3 {
            let k = if i < 2 { f } else { 1.0 };
            let off = if i < 2 { 500.0 } else { 0.0 };
            for j in 0..3 {
                p[i][j] = k * rows[i][j] + off * rows[2][j];
            }
            p[i][3] = -(0..3).map(|j| p[i][j] * c[j]).sum::<f64>();
        }
        Camera {
            p,
            origin: [0.0; 3],
            centre: c,
        }
    }

    #[test]
    fn jacobi_finds_the_null_vector() {
        // A = vvᵀ-free matrix with known smallest eigenvector (1, −1, 0)/√2.
        let a = [[2.0, 1.0, 0.0], [1.0, 2.0, 0.0], [0.0, 0.0, 5.0]];
        let v = smallest_eigenvector(a);
        assert!((v[0] + v[1]).abs() < 1e-9 && v[2].abs() < 1e-9);
    }

    #[test]
    fn dlt_recovers_a_camera_and_rays_hit_the_ground() {
        let d = dem();
        let truth = camera();
        let world: Vec<[f64; 3]> = [
            (2000.0, 3000.0),
            (8000.0, 3500.0),
            (5000.0, 6000.0),
            (3000.0, 8000.0),
            (7000.0, 9000.0),
            (4500.0, 2500.0),
            (6000.0, 5000.0),
            (2500.0, 6500.0),
        ]
        .iter()
        .map(|&(x, y)| [x, y, d.z(x, y).unwrap()])
        .collect();
        let page: Vec<P2> = world.iter().map(|w| truth.project(*w).unwrap()).collect();
        let cam = Camera::fit(&world, &page).unwrap();
        for k in 0..3 {
            assert!(
                (cam.centre[k] - truth.centre[k]).abs() < 1.0,
                "{:?}",
                cam.centre
            );
        }
        for (w, p) in world.iter().zip(&page) {
            let q = cam.project(*w).unwrap();
            assert!(crate::warp::dist(q, *p) < 1e-6);
            // The ray of the page point lands on the world point.
            let g = cam.ground(&d, *p).unwrap();
            assert!(crate::warp::dist(g, [w[0], w[1]]) < 2.0, "{g:?} {w:?}");
        }
    }

    #[test]
    fn ransac_places_with_outliers() {
        let d = dem();
        let truth = camera();
        let mut cands = Vec::new();
        let pts = [
            (2000.0, 3000.0),
            (8000.0, 3500.0),
            (5000.0, 6000.0),
            (3000.0, 8000.0),
            (7000.0, 9000.0),
            (4500.0, 2500.0),
            (6000.0, 5000.0),
            (2500.0, 6500.0),
            (7500.0, 7000.0),
            (3500.0, 4500.0),
        ];
        for (i, &(x, y)) in pts.iter().enumerate() {
            let q = truth.project([x, y, d.z(x, y).unwrap()]).unwrap();
            let mk = |lm: usize, map: P2| LabelMatch {
                label: format!("l{i}"),
                label_idx: i,
                landmark: lm,
                plan: q,
                map,
                sim: 1.0,
                ele: None,
            };
            cands.push(mk(i, [x, y]));
            // A homonym elsewhere for each label.
            cands.push(mk(100 + i, [y, x]));
        }
        let p = place(&cands, &d, 5.0).unwrap();
        assert_eq!(p.inliers.len(), pts.len());
        assert!(p.inliers.iter().all(|c| c.landmark < 100));
        assert!(p.median_px_pt < 1.0);
    }
}
