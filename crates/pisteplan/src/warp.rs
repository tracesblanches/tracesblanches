//! Plane-to-plane models fitted on tie points: similarity, affine, homography (global) and
//! thin-plate spline (piecewise smooth). Phone side: no pipeline dependency.

use serde::{Deserialize, Serialize};

use crate::linalg::{least_squares, solve};

/// A point of the plane.
pub type P2 = [f64; 2];

/// Euclidean distance.
pub fn dist(a: P2, b: P2) -> f64 {
    (a[0] - b[0]).hypot(a[1] - b[1])
}

/// Normalisation of a point set: `(p - centre) / scale`, with the RMS distance to the
/// centre as scale (conditioning of the fits).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Norm {
    /// Centre.
    pub c: P2,
    /// Scale.
    pub s: f64,
}

impl Norm {
    /// Normalisation of `pts` (identity scale for a single point).
    pub fn of(pts: &[P2]) -> Norm {
        let n = pts.len().max(1) as f64;
        let c = [
            pts.iter().map(|p| p[0]).sum::<f64>() / n,
            pts.iter().map(|p| p[1]).sum::<f64>() / n,
        ];
        let rms = (pts
            .iter()
            .map(|p| (p[0] - c[0]).powi(2) + (p[1] - c[1]).powi(2))
            .sum::<f64>()
            / n)
            .sqrt();
        Norm {
            c,
            s: if rms > 0.0 && rms.is_finite() {
                rms
            } else {
                1.0
            },
        }
    }

    /// Into normalised coordinates.
    pub fn fwd(&self, p: P2) -> P2 {
        [(p[0] - self.c[0]) / self.s, (p[1] - self.c[1]) / self.s]
    }

    /// Back from normalised coordinates.
    pub fn back(&self, q: P2) -> P2 {
        [q[0] * self.s + self.c[0], q[1] * self.s + self.c[1]]
    }
}

/// Affine map `x' = m[0][0] + m[0][1] x + m[0][2] y`, `y' = m[1][0] + m[1][1] x + m[1][2] y`.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Affine {
    /// Coefficients, one row per output coordinate: constant, x, y.
    pub m: [[f64; 3]; 2],
}

impl Affine {
    /// Applies the map.
    pub fn apply(&self, p: P2) -> P2 {
        let m = &self.m;
        [
            m[0][0] + m[0][1] * p[0] + m[0][2] * p[1],
            m[1][0] + m[1][1] * p[0] + m[1][2] * p[1],
        ]
    }

    /// Mean linear scale (square root of the determinant's absolute value).
    pub fn scale(&self) -> f64 {
        (self.m[0][1] * self.m[1][2] - self.m[0][2] * self.m[1][1])
            .abs()
            .sqrt()
    }

    /// Least-squares affine fit (at least 3 non-collinear points).
    pub fn fit(src: &[P2], dst: &[P2]) -> Option<Affine> {
        if src.len() < 3 || src.len() != dst.len() {
            return None;
        }
        let (ns, nd) = (Norm::of(src), Norm::of(dst));
        let a: Vec<f64> = src
            .iter()
            .flat_map(|&p| {
                let q = ns.fwd(p);
                [1.0, q[0], q[1]]
            })
            .collect();
        let b: Vec<f64> = dst.iter().flat_map(|&p| nd.fwd(p)).collect();
        let x = least_squares(&a, src.len(), 3, &b, 2)?;
        // x is 3 × 2: rows constant, x, y; columns output.
        let norm_map = Affine {
            m: [[x[0], x[2], x[4]], [x[1], x[3], x[5]]],
        };
        Some(compose_norm(&ns, &norm_map, &nd))
    }

    /// Least-squares similarity (rotation, uniform scale, translation, no reflection;
    /// at least 2 distinct points).
    pub fn fit_similarity(src: &[P2], dst: &[P2]) -> Option<Affine> {
        if src.len() < 2 || src.len() != dst.len() {
            return None;
        }
        let (ns, nd) = (Norm::of(src), Norm::of(dst));
        let (mut re, mut im, mut den) = (0.0, 0.0, 0.0);
        for (&s, &d) in src.iter().zip(dst) {
            let (a, b) = (ns.fwd(s), nd.fwd(d));
            // b · conj(a)
            re += b[0] * a[0] + b[1] * a[1];
            im += b[1] * a[0] - b[0] * a[1];
            den += a[0] * a[0] + a[1] * a[1];
        }
        if den <= 0.0 {
            return None;
        }
        let (re, im) = (re / den, im / den);
        let norm_map = Affine {
            m: [[0.0, re, -im], [0.0, im, re]],
        };
        Some(compose_norm(&ns, &norm_map, &nd))
    }
}

/// `nd⁻¹ ∘ m ∘ ns` as one affine map.
fn compose_norm(ns: &Norm, m: &Affine, nd: &Norm) -> Affine {
    // q = (p - cs) / ss ; r = m(q) ; out = r · sd + cd
    let k = nd.s / ns.s;
    let row = |i: usize| {
        let [a0, a1, a2] = m.m[i];
        [
            nd.c[i] + nd.s * a0 - k * (a1 * ns.c[0] + a2 * ns.c[1]),
            a1 * k,
            a2 * k,
        ]
    };
    Affine {
        m: [row(0), row(1)],
    }
}

/// Projective map (perspective), `h` row-major 3 × 3 with `h[8] = 1`, between normalised
/// frames.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Homography {
    /// Normalisation of the source.
    pub src: Norm,
    /// Normalisation of the destination.
    pub dst: Norm,
    /// Coefficients in the normalised frames.
    pub h: [f64; 9],
}

impl Homography {
    /// Applies the map (`None` on the line at infinity).
    pub fn apply(&self, p: P2) -> Option<P2> {
        let q = self.src.fwd(p);
        let h = &self.h;
        let d = h[6] * q[0] + h[7] * q[1] + h[8];
        if d.abs() < 1e-12 {
            return None;
        }
        Some(self.dst.back([
            (h[0] * q[0] + h[1] * q[1] + h[2]) / d,
            (h[3] * q[0] + h[4] * q[1] + h[5]) / d,
        ]))
    }

    /// Fit: linear (DLT, `h33 = 1`) then Gauss–Newton on the transfer error (at least 4
    /// points in general position).
    pub fn fit(src: &[P2], dst: &[P2]) -> Option<Homography> {
        let n = src.len();
        if n < 4 || n != dst.len() {
            return None;
        }
        let (ns, nd) = (Norm::of(src), Norm::of(dst));
        let s: Vec<P2> = src.iter().map(|&p| ns.fwd(p)).collect();
        let d: Vec<P2> = dst.iter().map(|&p| nd.fwd(p)).collect();
        let mut a = Vec::with_capacity(2 * n * 8);
        let mut b = Vec::with_capacity(2 * n);
        for (p, q) in s.iter().zip(&d) {
            let (x, y, u, v) = (p[0], p[1], q[0], q[1]);
            a.extend_from_slice(&[x, y, 1.0, 0.0, 0.0, 0.0, -u * x, -u * y]);
            b.push(u);
            a.extend_from_slice(&[0.0, 0.0, 0.0, x, y, 1.0, -v * x, -v * y]);
            b.push(v);
        }
        let mut hv = least_squares(&a, 2 * n, 8, &b, 1)?;
        // Gauss–Newton on the geometric (transfer) error.
        let cost = |hv: &[f64]| -> f64 {
            s.iter()
                .zip(&d)
                .map(|(p, q)| {
                    let den = hv[6] * p[0] + hv[7] * p[1] + 1.0;
                    let u = (hv[0] * p[0] + hv[1] * p[1] + hv[2]) / den;
                    let v = (hv[3] * p[0] + hv[4] * p[1] + hv[5]) / den;
                    (u - q[0]).powi(2) + (v - q[1]).powi(2)
                })
                .sum()
        };
        let mut c0 = cost(&hv);
        for _ in 0..20 {
            let mut jac = Vec::with_capacity(2 * n * 8);
            let mut r = Vec::with_capacity(2 * n);
            for (p, q) in s.iter().zip(&d) {
                let (x, y) = (p[0], p[1]);
                let den = hv[6] * x + hv[7] * y + 1.0;
                let u = (hv[0] * x + hv[1] * y + hv[2]) / den;
                let v = (hv[3] * x + hv[4] * y + hv[5]) / den;
                jac.extend_from_slice(&[
                    x / den,
                    y / den,
                    1.0 / den,
                    0.0,
                    0.0,
                    0.0,
                    -u * x / den,
                    -u * y / den,
                ]);
                r.push(q[0] - u);
                jac.extend_from_slice(&[
                    0.0,
                    0.0,
                    0.0,
                    x / den,
                    y / den,
                    1.0 / den,
                    -v * x / den,
                    -v * y / den,
                ]);
                r.push(q[1] - v);
            }
            let Some(step) = least_squares(&jac, 2 * n, 8, &r, 1) else {
                break;
            };
            let trial: Vec<f64> = hv.iter().zip(&step).map(|(h, s)| h + s).collect();
            let c1 = cost(&trial);
            if c1 < c0 {
                let done = (c0 - c1) < 1e-12 * c0.max(1e-30);
                hv = trial;
                c0 = c1;
                if done {
                    break;
                }
            } else {
                break;
            }
        }
        let mut h = [0.0; 9];
        h[..8].copy_from_slice(&hv);
        h[8] = 1.0;
        Some(Homography {
            src: ns,
            dst: nd,
            h,
        })
    }
}

/// Thin-plate spline `f(p) = A p + Σ wᵢ U(|p − cᵢ|)`, `U(r) = r² ln r`, fitted with a
/// smoothing (regularisation) parameter `lambda` in the normalised source frame:
/// `lambda = 0` interpolates the tie points, larger values tend to the affine fit.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Tps {
    /// Normalisation of the source.
    pub src: Norm,
    /// Normalisation of the destination.
    pub dst: Norm,
    /// Smoothing parameter.
    pub lambda: f64,
    /// Control points, normalised source frame.
    pub centers: Vec<P2>,
    /// Kernel weights (normalised destination frame).
    pub weights: Vec<P2>,
    /// Affine part (normalised frames).
    pub affine: Affine,
}

fn kernel(r2: f64) -> f64 {
    if r2 <= 1e-300 {
        0.0
    } else {
        0.5 * r2 * r2.ln()
    }
}

impl Tps {
    /// Fits the spline (at least 3 non-collinear points).
    pub fn fit(src: &[P2], dst: &[P2], lambda: f64) -> Option<Tps> {
        let n = src.len();
        if n < 3 || n != dst.len() || lambda.is_nan() || lambda < 0.0 {
            return None;
        }
        let (ns, nd) = (Norm::of(src), Norm::of(dst));
        let c: Vec<P2> = src.iter().map(|&p| ns.fwd(p)).collect();
        let m = n + 3;
        let mut a = vec![0.0; m * m];
        for i in 0..n {
            for j in 0..n {
                let r2 = (c[i][0] - c[j][0]).powi(2) + (c[i][1] - c[j][1]).powi(2);
                a[i * m + j] = kernel(r2) + if i == j { lambda } else { 0.0 };
            }
            let row = [1.0, c[i][0], c[i][1]];
            for k in 0..3 {
                a[i * m + n + k] = row[k];
                a[(n + k) * m + i] = row[k];
            }
        }
        let mut b = vec![0.0; m * 2];
        for (i, &p) in dst.iter().enumerate() {
            let q = nd.fwd(p);
            b[i * 2] = q[0];
            b[i * 2 + 1] = q[1];
        }
        let x = solve(a, m, b, 2)?;
        let weights = (0..n).map(|i| [x[i * 2], x[i * 2 + 1]]).collect();
        let k = |r: usize, col: usize| x[(n + r) * 2 + col];
        let affine = Affine {
            m: [[k(0, 0), k(1, 0), k(2, 0)], [k(0, 1), k(1, 1), k(2, 1)]],
        };
        Some(Tps {
            src: ns,
            dst: nd,
            lambda,
            centers: c,
            weights,
            affine,
        })
    }

    /// Applies the spline.
    pub fn apply(&self, p: P2) -> P2 {
        let q = self.src.fwd(p);
        let mut out = self.affine.apply(q);
        for (c, w) in self.centers.iter().zip(&self.weights) {
            let u = kernel((q[0] - c[0]).powi(2) + (q[1] - c[1]).powi(2));
            out[0] += w[0] * u;
            out[1] += w[1] * u;
        }
        self.dst.back(out)
    }
}

/// Kinds of models compared on the tie points.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ModelKind {
    /// Rotation, uniform scale, translation.
    Similarity,
    /// General affine map.
    Affine,
    /// Perspective (projective) map.
    Homography,
    /// Thin-plate spline with the given smoothing.
    Tps(f64),
}

impl ModelKind {
    /// Minimum number of tie points.
    pub fn min_points(self) -> usize {
        match self {
            ModelKind::Similarity => 2,
            ModelKind::Affine | ModelKind::Tps(_) => 3,
            ModelKind::Homography => 4,
        }
    }

    /// Short label.
    pub fn label(self) -> String {
        match self {
            ModelKind::Similarity => "similitude".to_owned(),
            ModelKind::Affine => "affine".to_owned(),
            ModelKind::Homography => "homographie (perspective)".to_owned(),
            ModelKind::Tps(l) => format!("spline plaque mince (lambda {l})"),
        }
    }
}

/// A fitted model.
#[derive(Debug, Clone, PartialEq)]
pub enum Model {
    /// Global affine (or similarity) map.
    Affine(Affine),
    /// Perspective map.
    Homography(Homography),
    /// Thin-plate spline.
    Tps(Tps),
}

impl Model {
    /// Fits a model of `kind`.
    pub fn fit(kind: ModelKind, src: &[P2], dst: &[P2]) -> Option<Model> {
        match kind {
            ModelKind::Similarity => Affine::fit_similarity(src, dst).map(Model::Affine),
            ModelKind::Affine => Affine::fit(src, dst).map(Model::Affine),
            ModelKind::Homography => Homography::fit(src, dst).map(Model::Homography),
            ModelKind::Tps(l) => Tps::fit(src, dst, l).map(Model::Tps),
        }
    }

    /// Applies the model.
    pub fn apply(&self, p: P2) -> Option<P2> {
        match self {
            Model::Affine(a) => Some(a.apply(p)),
            Model::Homography(h) => h.apply(p),
            Model::Tps(t) => Some(t.apply(p)),
        }
    }
}

/// Leave-one-out errors of a model kind: for each tie point, the model is fitted on all
/// the others and the distance between its prediction and the true destination is
/// returned (`None` where the fit failed).
pub fn leave_one_out(kind: ModelKind, src: &[P2], dst: &[P2]) -> Vec<Option<f64>> {
    let n = src.len();
    (0..n)
        .map(|i| {
            if n - 1 < kind.min_points() {
                return None;
            }
            let s: Vec<P2> = (0..n).filter(|&j| j != i).map(|j| src[j]).collect();
            let d: Vec<P2> = (0..n).filter(|&j| j != i).map(|j| dst[j]).collect();
            Model::fit(kind, &s, &d)
                .and_then(|m| m.apply(src[i]))
                .map(|p| dist(p, dst[i]))
        })
        .collect()
}

/// Quantile (linear interpolation) of a sample; `None` when empty.
pub fn quantile(values: &[f64], q: f64) -> Option<f64> {
    let mut v: Vec<f64> = values.iter().copied().filter(|x| x.is_finite()).collect();
    if v.is_empty() {
        return None;
    }
    v.sort_by(f64::total_cmp);
    let pos = q.clamp(0.0, 1.0) * (v.len() - 1) as f64;
    let (lo, hi) = (pos.floor() as usize, pos.ceil() as usize);
    Some(v[lo] + (v[hi] - v[lo]) * (pos - lo as f64))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn grid() -> Vec<P2> {
        let mut v = Vec::new();
        for i in 0..6 {
            for j in 0..5 {
                v.push([i as f64 * 100.0 + (j as f64) * 7.0, j as f64 * 80.0]);
            }
        }
        v
    }

    #[test]
    fn similarity_recovers_rotation_scale_translation() {
        let (c, s) = (0.3_f64.cos(), 0.3_f64.sin());
        let f = |p: P2| {
            [
                1000.0 + 2.5 * (c * p[0] - s * p[1]),
                -50.0 + 2.5 * (s * p[0] + c * p[1]),
            ]
        };
        let src = grid();
        let dst: Vec<P2> = src.iter().map(|&p| f(p)).collect();
        let a = Affine::fit_similarity(&src, &dst).unwrap();
        for &p in &src {
            assert!(dist(a.apply(p), f(p)) < 1e-6);
        }
        assert!((a.scale() - 2.5).abs() < 1e-9);
    }

    #[test]
    fn affine_and_homography_recover_their_maps() {
        let src = grid();
        let fa = |p: P2| [3.0 + 1.2 * p[0] - 0.4 * p[1], 7.0 + 0.3 * p[0] + 0.9 * p[1]];
        let dst: Vec<P2> = src.iter().map(|&p| fa(p)).collect();
        let a = Affine::fit(&src, &dst).unwrap();
        for &p in &src {
            assert!(dist(a.apply(p), fa(p)) < 1e-6);
        }
        let fh = |p: P2| {
            let d = 1.0 + 0.0004 * p[0] - 0.0002 * p[1];
            [
                (2.0 * p[0] + 0.5 * p[1] + 10.0) / d,
                (0.1 * p[0] + 1.5 * p[1] - 4.0) / d,
            ]
        };
        let dst: Vec<P2> = src.iter().map(|&p| fh(p)).collect();
        let h = Homography::fit(&src, &dst).unwrap();
        for &p in &src {
            assert!(dist(h.apply(p).unwrap(), fh(p)) < 1e-6, "{p:?}");
        }
        // An affine map cannot represent the perspective: residuals stay large.
        let a = Affine::fit(&src, &dst).unwrap();
        let worst = src
            .iter()
            .map(|&p| dist(a.apply(p), fh(p)))
            .fold(0.0, f64::max);
        assert!(worst > 5.0, "{worst}");
    }

    #[test]
    fn tps_interpolates_and_reproduces_affine_maps() {
        let src = grid();
        let f = |p: P2| {
            [
                p[0] + 30.0 * (p[1] / 200.0).sin(),
                p[1] + 0.001 * p[0] * p[0],
            ]
        };
        let dst: Vec<P2> = src.iter().map(|&p| f(p)).collect();
        let t = Tps::fit(&src, &dst, 0.0).unwrap();
        for (&p, &q) in src.iter().zip(&dst) {
            assert!(dist(t.apply(p), q) < 1e-6);
        }
        // Smooth enough to predict between the nodes better than the affine fit.
        let mid = [250.0, 120.0];
        let a = Affine::fit(&src, &dst).unwrap();
        assert!(dist(t.apply(mid), f(mid)) < dist(a.apply(mid), f(mid)));
        // Pure affine data: the spline is exactly that affine map, even with smoothing.
        let fa = |p: P2| [3.0 + 1.2 * p[0] - 0.4 * p[1], 7.0 + 0.3 * p[0] + 0.9 * p[1]];
        let dst: Vec<P2> = src.iter().map(|&p| fa(p)).collect();
        let t = Tps::fit(&src, &dst, 5.0).unwrap();
        assert!(dist(t.apply([33.0, 444.0]), fa([33.0, 444.0])) < 1e-6);
    }

    #[test]
    fn leave_one_out_is_zero_for_an_exact_model() {
        let src = grid();
        let fa = |p: P2| [3.0 + 1.2 * p[0] - 0.4 * p[1], 7.0 + 0.3 * p[0] + 0.9 * p[1]];
        let dst: Vec<P2> = src.iter().map(|&p| fa(p)).collect();
        let e = leave_one_out(ModelKind::Affine, &src, &dst);
        assert!(e.iter().all(|x| x.unwrap() < 1e-6));
        assert_eq!(quantile(&[3.0, 1.0, 2.0], 0.5), Some(2.0));
        assert_eq!(quantile(&[0.0, 10.0], 0.9), Some(9.0));
    }
}
