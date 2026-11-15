//! Direction arrows of the plan (ENJ plans: white triangles outlined in the piste's ink,
//! on the line). An arrow is a hole of the ink's mask (white interior enclosed by the
//! outline) inside a kept line component; its direction is the sign of the skewness of
//! its pixels along the line: a triangle is wide at its base and thin at its apex, so the
//! long tail of the distribution points forward.

use serde::Serialize;

use crate::build::Registration;
use crate::segment::{Mask, components, ink_mask, lines_of_mask};
use crate::transcribe::InkDistance;
use crate::warp::P2;

/// An arrow of the plan.
#[derive(Debug, Clone, Serialize)]
pub struct Arrow {
    /// Centre, page points.
    pub p: P2,
    /// Unit direction (page frame, y down).
    pub dir: P2,
    /// Ink index.
    pub ink: usize,
    /// Interior area, pixels.
    pub area: usize,
    /// Normalised skewness along the line (|value| = confidence).
    pub skew: f64,
}

/// Smallest normalised skewness of an arrow (a symmetric hole is not an arrow).
pub const MIN_SKEW: f64 = 0.3;

/// Arrows of every ink. Interior areas between `min_px` and `max_px` pixels.
pub fn detect(reg: &Registration, lines: &InkDistance, min_px: usize, max_px: usize) -> Vec<Arrow> {
    let (w, h) = (reg.input.image.w, reg.input.image.h);
    let k = reg.input.pt_per_px;
    let p = &reg.spec.line;
    let mut out = Vec::new();
    for ink in 0..reg.spec.inks.len() {
        let raw = ink_mask(&reg.classes, w, h, ink as u8);
        let filled = raw.close(p.close_r).fill_holes(p.max_hole_px.max(max_px));
        let (kept, _) = lines_of_mask(&filled, p);
        // Holes of the raw mask (the closing would shrink the triangles) that are enclosed
        // by the outline and belong to a kept line.
        let raw_filled = raw.fill_holes(max_px);
        let mut holes = Mask::new(w, h);
        for i in 0..w * h {
            holes.v[i] = u8::from(kept.v[i] != 0 && raw_filled.v[i] != 0 && raw.v[i] == 0);
        }
        let (lab, sizes) = components(&holes);
        let mut pix: Vec<Vec<usize>> = vec![Vec::new(); sizes.len()];
        for (i, &l) in lab.iter().enumerate() {
            if l != 0 && (min_px..=max_px).contains(&sizes[l as usize]) {
                pix[l as usize].push(i);
            }
        }
        for ps in pix.into_iter().filter(|v| !v.is_empty()) {
            let n = ps.len() as f64;
            let cx = ps.iter().map(|&i| (i % w) as f64 + 0.5).sum::<f64>() / n;
            let cy = ps.iter().map(|&i| (i / w) as f64 + 0.5).sum::<f64>() / n;
            let c = [cx * k, cy * k];
            let Some(t) = lines.nearest_dir(ink, c, 4.0) else {
                continue;
            };
            // Moments along the line.
            let s: Vec<f64> = ps
                .iter()
                .map(|&i| ((i % w) as f64 + 0.5 - cx) * t[0] + ((i / w) as f64 + 0.5 - cy) * t[1])
                .collect();
            let m2 = s.iter().map(|v| v * v).sum::<f64>() / n;
            let m3 = s.iter().map(|v| v * v * v).sum::<f64>() / n;
            if m2 <= 0.0 {
                continue;
            }
            let skew = m3 / m2.powf(1.5);
            if skew.abs() < MIN_SKEW {
                continue;
            }
            let sg = skew.signum();
            out.push(Arrow {
                p: c,
                dir: [t[0] * sg, t[1] * sg],
                ink,
                area: ps.len(),
                skew,
            });
        }
    }
    out
}

/// Votes of the arrows of `ink` near a polyline (page points): +1 per arrow pointing
/// along the polyline's order, −1 against. An arrow counts when it lies within `r_pt` of
/// a segment whose direction is within 30° of its own (either way).
pub fn votes(arrows: &[Arrow], ink: usize, line: &[P2], r_pt: f64) -> (usize, usize) {
    let cos_max = 30f64.to_radians().cos();
    let (mut fwd, mut back) = (0, 0);
    for a in arrows.iter().filter(|a| a.ink == ink) {
        let mut best: Option<(f64, f64)> = None;
        for s in line.windows(2) {
            let d = [s[1][0] - s[0][0], s[1][1] - s[0][1]];
            let l2 = d[0] * d[0] + d[1] * d[1];
            if l2 <= 0.0 {
                continue;
            }
            let u = (((a.p[0] - s[0][0]) * d[0] + (a.p[1] - s[0][1]) * d[1]) / l2).clamp(0.0, 1.0);
            let q = [s[0][0] + d[0] * u, s[0][1] + d[1] * u];
            let dist = (a.p[0] - q[0]).hypot(a.p[1] - q[1]);
            let l = l2.sqrt();
            let cos = (a.dir[0] * d[0] + a.dir[1] * d[1]) / l;
            if dist <= r_pt && cos.abs() >= cos_max && best.is_none_or(|b| dist < b.0) {
                best = Some((dist, cos));
            }
        }
        match best {
            Some((_, c)) if c > 0.0 => fwd += 1,
            Some(_) => back += 1,
            None => {}
        }
    }
    (fwd, back)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn votes_follow_the_polyline_order() {
        let a = |p: P2, dir: P2| Arrow {
            p,
            dir,
            ink: 1,
            area: 50,
            skew: 0.8,
        };
        let line = [[0.0, 0.0], [100.0, 0.0], [100.0, 100.0]];
        let arrows = [
            a([50.0, 1.0], [1.0, 0.0]),   // along the first segment
            a([101.0, 50.0], [0.0, 1.0]), // along the second
            a([30.0, -1.0], [-1.0, 0.0]), // against
            a([50.0, 30.0], [1.0, 0.0]),  // too far
            a([70.0, 0.0], [0.0, 1.0]),   // across the line
        ];
        assert_eq!(votes(&arrows, 1, &line, 4.0), (2, 1));
        assert_eq!(votes(&arrows, 2, &line, 4.0), (0, 0));
    }
}
