//! Property tests of the polyline clipping used for the historic avalanche crossings
//! (Q3, #103): signed areas and ring orientation, grouping of shapefile rings, length of
//! the runs inside a union of polygons.

use proptest::prelude::*;
use skimap_avalanche::historic::clip::{Poly, inside_runs, length, signed_area};

/// A star-shaped ring around `(cx, cy)` (simple), counter-clockwise, not closed.
fn star(cx: f64, cy: f64, radii: &[f64]) -> Vec<(f64, f64)> {
    let n = radii.len();
    (0..n)
        .map(|k| {
            let a = std::f64::consts::TAU * k as f64 / n as f64;
            (cx + radii[k] * a.cos(), cy + radii[k] * a.sin())
        })
        .collect()
}

fn ring() -> impl Strategy<Value = Vec<(f64, f64)>> {
    (
        -500.0f64..500.0,
        -500.0f64..500.0,
        prop::collection::vec(50.0f64..400.0, 3..30),
    )
        .prop_map(|(cx, cy, r)| star(cx, cy, &r))
}

fn polyline() -> impl Strategy<Value = Vec<(f64, f64)>> {
    prop::collection::vec((-1200.0f64..1200.0, -1200.0f64..1200.0), 2..12)
}

proptest! {
    /// Reversing a ring negates its signed area; rotating its first vertex or closing it
    /// changes nothing; `Poly::area` is the absolute value.
    #[test]
    fn signed_area_orientation(r in ring(), k in 0usize..30) {
        let a = signed_area(&r);
        prop_assert!(a > 0.0, "star rings are counter-clockwise");
        let mut rev = r.clone();
        rev.reverse();
        prop_assert!((signed_area(&rev) + a).abs() <= 1e-9 * a);
        let mut rot = r.clone();
        rot.rotate_left(k % r.len());
        prop_assert!((signed_area(&rot) - a).abs() <= 1e-9 * a);
        let mut closed = r.clone();
        closed.push(r[0]);
        prop_assert!((signed_area(&closed) - a).abs() <= 1e-9 * a);
        prop_assert!((Poly::new(vec![rev]).area() - a).abs() <= 1e-9 * a);
    }

    /// Shapefile rings (outer clockwise, hole counter-clockwise) are grouped into RFC 7946
    /// parts: outer counter-clockwise, hole clockwise, hole attached to its outer; the
    /// area is outer minus hole.
    #[test]
    fn parts_follow_rfc7946(
        (cx, cy, radii) in (-500.0f64..500.0, -500.0f64..500.0, prop::collection::vec(50.0f64..400.0, 3..30)),
        f in 0.05f64..0.45,
    ) {
        let r = star(cx, cy, &radii);
        let a = signed_area(&r);
        // Hole: a square around the centre of the star, inside its smallest radius (the
        // star is star-shaped from its centre, but its inscribed disc around the centre
        // is only guaranteed for the square's corners when they stay within the smallest
        // radius times the cosine of the half angle between two vertices).
        let n = radii.len() as f64;
        let rmin = radii.iter().copied().fold(f64::INFINITY, f64::min);
        let h = rmin * f * (std::f64::consts::PI / n).cos() / std::f64::consts::SQRT_2;
        let (x0, y0) = (cx, cy);
        let hole = vec![(x0 - h, y0 - h), (x0 + h, y0 - h), (x0 + h, y0 + h), (x0 - h, y0 + h)];
        let mut outer = r.clone();
        outer.reverse(); // shapefile outer: clockwise
        let p = Poly::new(vec![outer, hole.clone()]);
        let parts = p.parts();
        prop_assert_eq!(parts.len(), 1);
        prop_assert!(signed_area(&parts[0].0) > 0.0, "outer counter-clockwise");
        prop_assert_eq!(parts[0].1.len(), 1);
        prop_assert!(signed_area(&parts[0].1[0]) < 0.0, "hole clockwise");
        let want = a - 4.0 * h * h;
        prop_assert!((p.area() - want).abs() <= 1e-9 * a, "{} vs {want}", p.area());
        prop_assert!(!p.contains((x0, y0)), "centre is in the hole");
    }

    /// Runs inside a union: never longer than the line, never more than one run per
    /// sub-segment change, each run's length equals its geometry's length; a line fully
    /// inside a convex polygon is one run of its whole length; splitting the line at a
    /// vertex does not change the total inside length.
    #[test]
    fn inside_runs_lengths(
        rings in prop::collection::vec(ring(), 1..4),
        line in polyline(),
        split in any::<prop::sample::Index>(),
    ) {
        let polys: Vec<Poly> = rings.iter().map(|r| Poly::new(vec![r.clone()])).collect();
        let runs = inside_runs(&line, &polys);
        let total: f64 = runs.iter().map(|r| r.length).sum();
        let l = length(&line);
        prop_assert!(total <= l * (1.0 + 1e-12) + 1e-9, "{total} > {l}");
        for r in &runs {
            prop_assert!((length(&r.coords) - r.length).abs() <= 1e-6 * r.length.max(1.0));
            prop_assert!(!r.polygons.is_empty());
            prop_assert!(r.polygons.windows(2).all(|w| w[0] < w[1]), "sorted, unique");
            // Every run midpoint is inside one of its polygons.
            let k = r.coords.len() / 2;
            let (a, b) = (r.coords[k - 1], r.coords[k]);
            let mid = ((a.0 + b.0) / 2.0, (a.1 + b.1) / 2.0);
            prop_assert!(r.polygons.iter().any(|&i| polys[i].contains(mid)));
        }
        // Splitting the line at a vertex.
        let k = 1 + split.index(line.len().saturating_sub(1).max(1));
        if k < line.len() {
            let (a, b) = (line[..=k].to_vec(), line[k..].to_vec());
            let t2: f64 = inside_runs(&a, &polys).iter().chain(inside_runs(&b, &polys).iter()).map(|r| r.length).sum();
            prop_assert!((t2 - total).abs() <= 1e-6 * l.max(1.0), "{t2} vs {total}");
        }
    }

    /// A segment between two points of a convex polygon lies inside it: one run, whole
    /// length.
    #[test]
    fn chord_of_a_convex_polygon_is_inside(
        (cx, cy, rad) in (-500.0f64..500.0, -500.0f64..500.0, 50.0f64..400.0),
        n in 3usize..40,
        (s, t) in (0.0f64..1.0, 0.0f64..1.0),
        (u, v) in (0.05f64..0.95, 0.05f64..0.95),
    ) {
        let r = star(cx, cy, &vec![rad; n]);
        let p = Poly::new(vec![r.clone()]);
        // Two interior points: convex combinations towards the centre.
        let at = |k: f64, w: f64| {
            let i = (k * n as f64) as usize % n;
            ((cx + (r[i].0 - cx) * w), (cy + (r[i].1 - cy) * w))
        };
        let line = vec![at(s, u), at(t, v)];
        let runs = inside_runs(&line, std::slice::from_ref(&p));
        let l = length(&line);
        if l > 1e-6 {
            prop_assert_eq!(runs.len(), 1);
            prop_assert!((runs[0].length - l).abs() <= 1e-9 * l.max(1.0));
        }
    }
}
