//! Geometry along a planned route: cumulative distance, position and heading at a
//! distance, projection of a position onto a window of the route.
//!
//! Distances are haversine sums, like the trail graph (`skimap_core::graph`); projections
//! use the graph's local equirectangular plane (error below 0.1 % over a site).

use skimap_common::geom::LonLat;
use skimap_core::graph::LocalPlane;

/// A polyline with its cumulative distances.
#[derive(Debug, Clone, PartialEq)]
pub struct RouteLine {
    pts: Vec<LonLat>,
    cum: Vec<f64>,
    plane: LocalPlane,
}

/// Bearing from `a` to `b`, degrees clockwise from north, in [0, 360).
pub fn bearing_deg(a: LonLat, b: LonLat) -> f64 {
    let (p1, p2) = (a.lat.to_radians(), b.lat.to_radians());
    let dl = (b.lon - a.lon).to_radians();
    let y = dl.sin() * p2.cos();
    let x = p1.cos() * p2.sin() - p1.sin() * p2.cos() * dl.cos();
    y.atan2(x).to_degrees().rem_euclid(360.0)
}

/// Signed angle from bearing `from` to bearing `to`, degrees in (−180, 180]: positive to
/// the right (clockwise).
pub fn turn_deg(from: f64, to: f64) -> f64 {
    let d = (to - from).rem_euclid(360.0);
    if d > 180.0 { d - 360.0 } else { d }
}

impl RouteLine {
    /// Line through `pts` (consecutive duplicates removed); at least one point.
    pub fn new(pts: Vec<LonLat>) -> Self {
        let mut clean: Vec<LonLat> = Vec::with_capacity(pts.len());
        for p in pts {
            if clean.last() != Some(&p) {
                clean.push(p);
            }
        }
        let mut cum = Vec::with_capacity(clean.len());
        let mut acc = 0.0;
        for (i, p) in clean.iter().enumerate() {
            if i > 0 {
                acc += clean[i - 1].haversine_m(*p);
            }
            cum.push(acc);
        }
        let origin = clean[clean.len() / 2];
        Self {
            pts: clean,
            cum,
            plane: LocalPlane::new(origin),
        }
    }

    /// Points.
    pub fn points(&self) -> &[LonLat] {
        &self.pts
    }

    /// Cumulative distance at each point, metres.
    pub fn cumulative(&self) -> &[f64] {
        &self.cum
    }

    /// Length, metres.
    pub fn length_m(&self) -> f64 {
        *self.cum.last().unwrap_or(&0.0)
    }

    /// Position at a distance (clamped).
    pub fn point_at(&self, s: f64) -> LonLat {
        if self.pts.len() == 1 {
            return self.pts[0];
        }
        let s = s.clamp(0.0, self.length_m());
        let i = self
            .cum
            .partition_point(|&c| c <= s)
            .clamp(1, self.pts.len() - 1);
        let span = self.cum[i] - self.cum[i - 1];
        let t = if span > 0.0 {
            (s - self.cum[i - 1]) / span
        } else {
            0.0
        };
        let (a, b) = (self.pts[i - 1], self.pts[i]);
        LonLat::new(a.lon + (b.lon - a.lon) * t, a.lat + (b.lat - a.lat) * t)
    }

    /// Heading of the route over `[s0, s1]` (chord), degrees; `None` when the chord is
    /// shorter than a metre.
    pub fn heading(&self, s0: f64, s1: f64) -> Option<f64> {
        let (a, b) = (self.point_at(s0), self.point_at(s1));
        (a.haversine_m(b) >= 1.0).then(|| bearing_deg(a, b))
    }

    /// Nearest point of the route between distances `lo` and `hi`: (distance along,
    /// distance from `p` in metres).
    pub fn project(&self, p: LonLat, lo: f64, hi: f64) -> Option<(f64, f64)> {
        if self.pts.len() == 1 {
            return Some((
                0.0,
                self.plane
                    .xy(p)
                    .iter()
                    .zip(self.plane.xy(self.pts[0]))
                    .map(|(a, b)| (a - b).powi(2))
                    .sum::<f64>()
                    .sqrt(),
            ));
        }
        let lo = lo.max(0.0);
        let hi = hi.min(self.length_m());
        if hi < lo {
            return None;
        }
        let q = self.plane.xy(p);
        let first = self.cum.partition_point(|&c| c < lo).saturating_sub(1);
        let mut best: Option<(f64, f64)> = None;
        for i in first..self.pts.len() - 1 {
            if self.cum[i] > hi {
                break;
            }
            let (a, b) = (self.plane.xy(self.pts[i]), self.plane.xy(self.pts[i + 1]));
            let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
            let l2 = dx * dx + dy * dy;
            let mut t = if l2 > 0.0 {
                (((q[0] - a[0]) * dx + (q[1] - a[1]) * dy) / l2).clamp(0.0, 1.0)
            } else {
                0.0
            };
            let seg = self.cum[i + 1] - self.cum[i];
            // Keep the projection inside the window.
            if seg > 0.0 {
                let s = self.cum[i] + t * seg;
                if s < lo {
                    t = (lo - self.cum[i]) / seg;
                } else if s > hi {
                    t = (hi - self.cum[i]) / seg;
                }
            }
            let x = [a[0] + t * dx, a[1] + t * dy];
            let d = ((q[0] - x[0]).powi(2) + (q[1] - x[1]).powi(2)).sqrt();
            let s = self.cum[i] + t * seg;
            if best.is_none_or(|(_, bd)| d < bd) {
                best = Some((s, d));
            }
        }
        best
    }

    /// Side of the route as seen by a skier at `p` heading `heading_deg`: the bearing to
    /// the nearest point of the route relative to the heading (degrees, positive right).
    pub fn relative_bearing(&self, p: LonLat, target_s: f64, heading_deg: f64) -> f64 {
        turn_deg(heading_deg, bearing_deg(p, self.point_at(target_s)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn line() -> RouteLine {
        let plane = LocalPlane::new(LonLat::new(6.1, 46.6));
        // East 1 km, then north 1 km.
        RouteLine::new(vec![
            plane.lonlat([0.0, 0.0]),
            plane.lonlat([1000.0, 0.0]),
            plane.lonlat([1000.0, 0.0]),
            plane.lonlat([1000.0, 1000.0]),
        ])
    }

    #[test]
    fn length_point_heading_and_projection() {
        let l = line();
        assert_eq!(l.points().len(), 3, "duplicate removed");
        assert!((l.length_m() - 2000.0).abs() < 2.0, "{}", l.length_m());
        let h = l.heading(0.0, 100.0).unwrap();
        assert!((h - 90.0).abs() < 0.5, "{h}");
        let h = l.heading(1100.0, 1200.0).unwrap();
        assert!(h < 0.5 || h > 359.5, "{h}");
        let plane = LocalPlane::new(LonLat::new(6.1, 46.6));
        let (s, d) = l.project(plane.lonlat([500.0, 30.0]), 0.0, 2000.0).unwrap();
        assert!((s - 500.0).abs() < 1.5 && (d - 30.0).abs() < 0.5, "{s} {d}");
        // Window forces the second leg.
        let (s, _) = l
            .project(plane.lonlat([500.0, 30.0]), 1200.0, 2000.0)
            .unwrap();
        assert!((s - 1200.0).abs() < 1.0, "{s}");
        assert!(
            l.project(plane.lonlat([0.0, 0.0]), 1500.0, 1000.0)
                .is_none()
        );
    }

    #[test]
    fn turn_angles() {
        assert_eq!(turn_deg(90.0, 0.0), -90.0);
        assert_eq!(turn_deg(350.0, 10.0), 20.0);
        assert_eq!(turn_deg(0.0, 180.0), 180.0);
        let l = line();
        let plane = LocalPlane::new(LonLat::new(6.1, 46.6));
        // Heading east at (500, 100): the route (y = 0) is on the right.
        let rb = l.relative_bearing(plane.lonlat([500.0, 100.0]), 500.0, 90.0);
        assert!((rb - 90.0).abs() < 1.0, "{rb}");
    }
}
