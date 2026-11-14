//! Clipping of polylines by a union of polygons with holes, in a projected plane.
//!
//! Each segment is cut at every crossing with a polygon edge; each sub-segment is then
//! inside or outside the union according to its midpoint (even-odd rule per polygon, so
//! holes are honoured). Overlapping polygons are therefore counted once. Consecutive inside
//! sub-segments form a run whose length and containing polygons are reported.

/// A ring of vertices (projected or geographic coordinates).
pub type Ring = Vec<(f64, f64)>;

/// A polygon part: outer ring and its holes.
pub type Part = (Ring, Vec<Ring>);

/// A polygon with holes (rings in any orientation) and its bounding box.
#[derive(Debug, Clone, PartialEq)]
pub struct Poly {
    /// Rings (closed or not; the closing edge is implied).
    pub rings: Vec<Vec<(f64, f64)>>,
    /// `(min_x, min_y, max_x, max_y)`.
    pub bbox: (f64, f64, f64, f64),
}

impl Poly {
    /// Polygon from rings.
    pub fn new(rings: Vec<Vec<(f64, f64)>>) -> Self {
        let mut b = (
            f64::INFINITY,
            f64::INFINITY,
            f64::NEG_INFINITY,
            f64::NEG_INFINITY,
        );
        for &(x, y) in rings.iter().flatten() {
            b.0 = b.0.min(x);
            b.1 = b.1.min(y);
            b.2 = b.2.max(x);
            b.3 = b.3.max(y);
        }
        Self { rings, bbox: b }
    }

    /// Even-odd point-in-polygon test over all rings.
    pub fn contains(&self, (px, py): (f64, f64)) -> bool {
        let (x0, y0, x1, y1) = self.bbox;
        if px < x0 || px > x1 || py < y0 || py > y1 {
            return false;
        }
        let mut inside = false;
        for ring in &self.rings {
            let n = ring.len();
            if n < 3 {
                continue;
            }
            let mut j = n - 1;
            for i in 0..n {
                let (xi, yi) = ring[i];
                let (xj, yj) = ring[j];
                if (yi > py) != (yj > py) && px < (xj - xi) * (py - yi) / (yj - yi) + xi {
                    inside = !inside;
                }
                j = i;
            }
        }
        inside
    }

    /// Area by the shoelace formula (holes subtracted when oriented opposite to the outer
    /// ring, as in shapefiles).
    pub fn area(&self) -> f64 {
        self.rings
            .iter()
            .map(|r| {
                let n = r.len();
                (0..n)
                    .map(|i| {
                        let (x1, y1) = r[i];
                        let (x2, y2) = r[(i + 1) % n];
                        x1 * y2 - x2 * y1
                    })
                    .sum::<f64>()
                    / 2.0
            })
            .sum::<f64>()
            .abs()
    }

    /// Groups the rings into polygons `(outer, holes)`: rings with a negative signed area
    /// (clockwise, the shapefile convention for outer rings) are outers, the others are
    /// holes assigned to the smallest outer containing their first vertex. Returned rings
    /// follow RFC 7946: outer counter-clockwise, holes clockwise.
    pub fn parts(&self) -> Vec<Part> {
        let (outers, holes): (Vec<&Ring>, Vec<&Ring>) =
            self.rings.iter().partition(|r| signed_area(r) < 0.0);
        let mut parts: Vec<Part> = outers
            .iter()
            .map(|o| {
                let mut o = (*o).clone();
                o.reverse();
                (o, Vec::new())
            })
            .collect();
        for h in holes {
            let Some(&probe) = h.first() else { continue };
            let host = (0..parts.len())
                .filter(|&i| Poly::new(vec![parts[i].0.clone()]).contains(probe))
                .min_by(|&a, &b| {
                    signed_area(&parts[a].0)
                        .abs()
                        .total_cmp(&signed_area(&parts[b].0).abs())
                });
            let mut h = h.clone();
            h.reverse();
            match host {
                Some(i) => parts[i].1.push(h),
                // A counter-clockwise ring outside every outer: treat it as an outer.
                None => {
                    h.reverse();
                    parts.push((h, Vec::new()));
                }
            }
        }
        parts
    }

    fn edges(&self) -> impl Iterator<Item = ((f64, f64), (f64, f64))> + '_ {
        self.rings.iter().flat_map(|r| {
            let n = r.len();
            (0..n).map(move |i| (r[i], r[(i + 1) % n]))
        })
    }
}

/// Signed area of a ring (positive when counter-clockwise).
pub fn signed_area(r: &[(f64, f64)]) -> f64 {
    let n = r.len();
    (0..n)
        .map(|i| {
            let (x1, y1) = r[i];
            let (x2, y2) = r[(i + 1) % n];
            x1 * y2 - x2 * y1
        })
        .sum::<f64>()
        / 2.0
}

fn bbox_overlap(a: (f64, f64, f64, f64), b: (f64, f64, f64, f64)) -> bool {
    a.0 <= b.2 && b.0 <= a.2 && a.1 <= b.3 && b.1 <= a.3
}

fn cross(a: (f64, f64), b: (f64, f64)) -> f64 {
    a.0 * b.1 - a.1 * b.0
}

/// Parameter `t` on segment `p→p2` of its proper crossing with segment `q→q2`, if any.
fn crossing(p: (f64, f64), p2: (f64, f64), q: (f64, f64), q2: (f64, f64)) -> Option<f64> {
    let r = (p2.0 - p.0, p2.1 - p.1);
    let s = (q2.0 - q.0, q2.1 - q.1);
    let denom = cross(r, s);
    if denom.abs() < 1e-18 {
        // Parallel or collinear: the midpoint tests of the neighbouring sub-segments
        // decide; a collinear overlap with an edge has zero area effect.
        return None;
    }
    let qp = (q.0 - p.0, q.1 - p.1);
    let t = cross(qp, s) / denom;
    let u = cross(qp, r) / denom;
    ((0.0..=1.0).contains(&t) && (0.0..=1.0).contains(&u)).then_some(t)
}

/// A maximal run of a polyline inside the union.
#[derive(Debug, Clone, PartialEq)]
pub struct Run {
    /// Vertices of the run (projected coordinates).
    pub coords: Vec<(f64, f64)>,
    /// Indices (into the polygon slice) of the polygons containing part of the run,
    /// sorted, without duplicates.
    pub polygons: Vec<usize>,
    /// Length of the run, projected units.
    pub length: f64,
}

/// Length of a polyline, projected units.
pub fn length(line: &[(f64, f64)]) -> f64 {
    line.windows(2)
        .map(|w| ((w[1].0 - w[0].0).powi(2) + (w[1].1 - w[0].1).powi(2)).sqrt())
        .sum()
}

/// Runs of `line` inside the union of `polys`.
pub fn inside_runs(line: &[(f64, f64)], polys: &[Poly]) -> Vec<Run> {
    if line.len() < 2 {
        return Vec::new();
    }
    let mut lb = (
        f64::INFINITY,
        f64::INFINITY,
        f64::NEG_INFINITY,
        f64::NEG_INFINITY,
    );
    for &(x, y) in line {
        lb.0 = lb.0.min(x);
        lb.1 = lb.1.min(y);
        lb.2 = lb.2.max(x);
        lb.3 = lb.3.max(y);
    }
    let candidates: Vec<usize> = (0..polys.len())
        .filter(|&i| bbox_overlap(polys[i].bbox, lb))
        .collect();
    if candidates.is_empty() {
        return Vec::new();
    }
    let mut runs: Vec<Run> = Vec::new();
    let mut current: Option<Run> = None;
    for w in line.windows(2) {
        let (a, b) = (w[0], w[1]);
        let sb = (a.0.min(b.0), a.1.min(b.1), a.0.max(b.0), a.1.max(b.1));
        let mut ts = vec![0.0, 1.0];
        for &i in &candidates {
            if !bbox_overlap(polys[i].bbox, sb) {
                continue;
            }
            for (q, q2) in polys[i].edges() {
                let eb = (q.0.min(q2.0), q.1.min(q2.1), q.0.max(q2.0), q.1.max(q2.1));
                if !bbox_overlap(eb, sb) {
                    continue;
                }
                if let Some(t) = crossing(a, b, q, q2) {
                    ts.push(t);
                }
            }
        }
        ts.sort_by(f64::total_cmp);
        ts.dedup_by(|x, y| (*x - *y).abs() < 1e-12);
        let at = |t: f64| (a.0 + (b.0 - a.0) * t, a.1 + (b.1 - a.1) * t);
        for tw in ts.windows(2) {
            let (t0, t1) = (tw[0], tw[1]);
            let mid = at((t0 + t1) / 2.0);
            let inside: Vec<usize> = candidates
                .iter()
                .copied()
                .filter(|&i| polys[i].contains(mid))
                .collect();
            let (p0, p1) = (at(t0), at(t1));
            let seg_len = ((p1.0 - p0.0).powi(2) + (p1.1 - p0.1).powi(2)).sqrt();
            if inside.is_empty() {
                if let Some(r) = current.take() {
                    runs.push(r);
                }
                continue;
            }
            let r = current.get_or_insert_with(|| Run {
                coords: vec![p0],
                polygons: Vec::new(),
                length: 0.0,
            });
            r.coords.push(p1);
            r.length += seg_len;
            for i in inside {
                if !r.polygons.contains(&i) {
                    r.polygons.push(i);
                }
            }
        }
    }
    if let Some(r) = current.take() {
        runs.push(r);
    }
    for r in &mut runs {
        r.polygons.sort_unstable();
    }
    runs
}

#[cfg(test)]
mod tests {
    use super::*;
    use approx::assert_relative_eq;

    fn square(x0: f64, y0: f64, s: f64) -> Vec<(f64, f64)> {
        vec![
            (x0, y0),
            (x0, y0 + s),
            (x0 + s, y0 + s),
            (x0 + s, y0),
            (x0, y0),
        ]
    }

    #[test]
    fn line_through_square_with_hole() {
        // 10 x 10 square with a 2 x 2 hole in the middle (hole counter-clockwise).
        let mut hole = square(4.0, 4.0, 2.0);
        hole.reverse();
        let p = Poly::new(vec![square(0.0, 0.0, 10.0), hole]);
        assert_relative_eq!(p.area(), 96.0);
        assert!(p.contains((1.0, 1.0)));
        assert!(!p.contains((5.0, 5.0)));
        let line = [(-5.0, 5.0), (15.0, 5.0)];
        let runs = inside_runs(&line, std::slice::from_ref(&p));
        assert_eq!(runs.len(), 2);
        assert_relative_eq!(runs[0].length, 4.0, epsilon = 1e-9);
        assert_relative_eq!(runs[1].length, 4.0, epsilon = 1e-9);
        assert_relative_eq!(runs[0].coords[0].0, 0.0, epsilon = 1e-9);
        assert_relative_eq!(length(&line), 20.0);
        // Shapefile orientation: outer clockwise (`square` is clockwise), hole
        // counter-clockwise.
        let outer = square(0.0, 0.0, 10.0);
        assert!(signed_area(&outer) < 0.0);
        let mut hole = square(4.0, 4.0, 2.0);
        hole.reverse();
        let sq2 = square(20.0, 0.0, 1.0);
        let p = Poly::new(vec![outer, hole, sq2]);
        let parts = p.parts();
        assert_eq!(parts.len(), 2);
        assert_eq!(parts[0].1.len(), 1);
        assert!(signed_area(&parts[0].0) > 0.0, "outer counter-clockwise");
        assert!(signed_area(&parts[0].1[0]) < 0.0, "hole clockwise");
        assert!(parts[1].1.is_empty());
    }

    #[test]
    fn overlapping_polygons_counted_once() {
        let a = Poly::new(vec![square(0.0, 0.0, 10.0)]);
        let b = Poly::new(vec![square(5.0, 0.0, 10.0)]);
        let line = [(-1.0, 5.0), (3.0, 5.0), (20.0, 5.0)];
        let runs = inside_runs(&line, &[a, b]);
        assert_eq!(runs.len(), 1);
        assert_relative_eq!(runs[0].length, 15.0, epsilon = 1e-9);
        assert_eq!(runs[0].polygons, [0, 1]);
        assert_eq!(runs[0].coords.len(), 5, "cut at x=0, 3 (vertex), 5, 10, 15");
    }

    #[test]
    fn outside_and_degenerate() {
        let a = Poly::new(vec![square(0.0, 0.0, 1.0)]);
        assert!(inside_runs(&[(5.0, 5.0), (6.0, 6.0)], std::slice::from_ref(&a)).is_empty());
        assert!(inside_runs(&[(0.5, 0.5)], std::slice::from_ref(&a)).is_empty());
        let r = inside_runs(&[(0.2, 0.2), (0.8, 0.8)], std::slice::from_ref(&a));
        assert_relative_eq!(r[0].length, 0.6 * 2f64.sqrt(), epsilon = 1e-12);
    }
}
