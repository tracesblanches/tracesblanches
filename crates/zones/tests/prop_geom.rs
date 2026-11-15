//! Property tests of the zone geometry helpers (Q3, #103): ring orientation, geodesic
//! areas independent of orientation and of the first vertex, clipping to the site box.

use proptest::prelude::*;
use skimap_common::geom::BBox;
use skimap_zones::geo::orient::{Direction, Orient};
use skimap_zones::geo::{BoundingRect, Coord, LineString, MultiPolygon, Polygon, Winding};
use skimap_zones::geom::{area_ha, bbox_polygon, clip_to_bbox, geodesic_area_m2, normalize};

/// A star-shaped (hence simple) ring around a centre, closed, counter-clockwise, with an
/// optional hole well inside the smallest radius.
fn star(cx: f64, cy: f64, radii: &[f64], hole: Option<f64>) -> Polygon<f64> {
    let n = radii.len();
    let kx = 1.0 / cy.to_radians().cos();
    let ring: Vec<Coord<f64>> = (0..=n)
        .map(|k| {
            let a = std::f64::consts::TAU * (k % n) as f64 / n as f64;
            let r = radii[k % n];
            Coord {
                x: cx + r * a.cos() * kx,
                y: cy + r * a.sin(),
            }
        })
        .collect();
    let rmin = radii.iter().copied().fold(f64::INFINITY, f64::min);
    let holes = hole
        .map(|f| {
            let r = rmin * f * 0.5;
            let mut h: Vec<Coord<f64>> = (0..=8)
                .map(|k| {
                    let a = std::f64::consts::TAU * (k % 8) as f64 / 8.0;
                    Coord {
                        x: cx + r * a.cos() * kx,
                        y: cy + r * a.sin(),
                    }
                })
                .collect();
            h.reverse();
            vec![LineString::new(h)]
        })
        .unwrap_or_default();
    Polygon::new(LineString::new(ring), holes)
}

/// Polygons of 0.2 to 1.3 km radius with 8 to 40 vertices (edges under 1 km, like the
/// zone sources).
fn polygon() -> impl Strategy<Value = Polygon<f64>> {
    (
        6.0f64..6.2,
        44.8f64..44.9,
        prop::collection::vec(0.002f64..0.012, 8..40),
        prop::option::of(0.1f64..0.9),
    )
        .prop_map(|(cx, cy, radii, hole)| star(cx, cy, &radii, hole))
}

/// Reverses every ring and rotates its start vertex by `shift`.
fn scramble(p: &Polygon<f64>, shift: usize) -> Polygon<f64> {
    let ring = |ls: &LineString<f64>| {
        let mut v: Vec<Coord<f64>> = ls.0[..ls.0.len() - 1].to_vec();
        v.reverse();
        let k = shift % v.len();
        v.rotate_left(k);
        v.push(v[0]);
        LineString::new(v)
    };
    Polygon::new(ring(p.exterior()), p.interiors().iter().map(ring).collect())
}

fn close(a: f64, b: f64, rel: f64) -> bool {
    (a - b).abs() <= rel * a.abs().max(b.abs()).max(1.0)
}

proptest! {
    /// The geodesic area does not depend on the orientation of the rings nor on their first
    /// vertex; a hole is subtracted whatever its orientation.
    #[test]
    fn area_ignores_orientation_and_start(p in polygon(), shift in 0usize..30) {
        let a = geodesic_area_m2(&MultiPolygon::new(vec![p.clone()]));
        let b = geodesic_area_m2(&MultiPolygon::new(vec![scramble(&p, shift)]));
        prop_assert!(a > 0.0);
        prop_assert!(close(a, b, 1e-9), "{a} vs {b}");
        let outer = geodesic_area_m2(&MultiPolygon::new(vec![Polygon::new(p.exterior().clone(), vec![])]));
        prop_assert!(a <= outer * (1.0 + 1e-12));
        prop_assert!(close(area_ha(&MultiPolygon::new(vec![p])), (a.round()) / 10_000.0, 1e-12));
    }

    /// Normalisation orients exteriors counter-clockwise and holes clockwise, keeps the area,
    /// and merges overlapping copies (an invalid multipolygon counts once).
    #[test]
    fn normalize_orients_and_merges(p in polygon(), shift in 0usize..30) {
        let a = geodesic_area_m2(&MultiPolygon::new(vec![p.clone()]));
        let n = normalize(&MultiPolygon::new(vec![scramble(&p, shift)]));
        for q in &n.0 {
            prop_assert!(q.exterior().is_ccw(), "exterior not counter-clockwise");
            for h in q.interiors() {
                prop_assert!(h.is_cw(), "hole not clockwise");
            }
        }
        prop_assert!(close(geodesic_area_m2(&n), a, 1e-6), "{} vs {a}", geodesic_area_m2(&n));
        let twice = normalize(&MultiPolygon::new(vec![p.clone(), scramble(&p, shift)]));
        prop_assert!(close(geodesic_area_m2(&twice), a, 1e-6));
        // `Orient` agrees.
        let o = p.orient(Direction::Default);
        prop_assert!(o.exterior().is_ccw());
    }

    /// Clipping to a box: the result lies inside the box, is no larger than the polygon or
    /// the box, equals the polygon when it is inside, and the clips of the two halves of a
    /// box add up to the clip of the box.
    #[test]
    fn clip_is_inside_and_additive(
        p in polygon(),
        (s, w) in (44.78f64..44.9, 5.98f64..6.2),
        (dn, de) in (0.005f64..0.08, 0.005f64..0.08),
        cut in 0.2f64..0.8,
    ) {
        let b = BBox::new(s, w, s + dn, w + de);
        let mp = MultiPolygon::new(vec![p.clone()]);
        let a = geodesic_area_m2(&mp);
        let boxa = geodesic_area_m2(&MultiPolygon::new(vec![bbox_polygon(b, 1)]));
        let clip = |b: BBox| clip_to_bbox(&mp, b).map_or(0.0, |c| {
            let r = c.geometry.bounding_rect().expect("non-empty");
            assert!(r.min().x >= b.west - 1e-9 && r.max().x <= b.east + 1e-9, "{r:?} outside {b:?}");
            assert!(r.min().y >= b.south - 1e-9 && r.max().y <= b.north + 1e-9, "{r:?} outside {b:?}");
            geodesic_area_m2(&c.geometry)
        });
        let whole = clip(b);
        prop_assert!(whole <= a * (1.0 + 1e-6) + 1.0, "clip {whole} > polygon {a}");
        prop_assert!(whole <= boxa * (1.0 + 1e-6) + 1.0, "clip {whole} > box {boxa}");
        let mid = w + de * cut;
        let left = clip(BBox::new(s, w, s + dn, mid));
        let right = clip(BBox::new(s, mid, s + dn, w + de));
        // Pieces under 1 m² are dropped by the clip. The cut point of an edge is computed in
        // the lon/lat plane while the area treats the edge as a geodesic, which bulges
        // poleward by L² tan φ / 8R: each cut edge moves the area by at most the sliver
        // L³ tan φ / 12R (found by this test: 17 m² on 39 ha, 26 m² on 21 ha, for 1 km
        // edges; 56 m² on 75 ha with the box side).
        let sliver: f64 = p
            .exterior()
            .lines()
            .chain(p.interiors().iter().flat_map(|h| h.lines()))
            .map(|l| {
                let kx = 111_195.0 * l.start.y.to_radians().cos();
                let len = ((l.end.x - l.start.x) * kx).hypot((l.end.y - l.start.y) * 111_195.0);
                len.powi(3) * l.start.y.to_radians().tan() / (12.0 * 6_371_000.0)
            })
            .sum::<f64>()
            // The north and south sides of the box are parallels, not geodesics: their
            // part inside the polygon (at most its width) is cut in two as well.
            + {
                let r = p.bounding_rect().expect("non-empty");
                let lat = r.max().y;
                let len = (r.max().x - r.min().x) * 111_195.0 * lat.to_radians().cos();
                2.0 * len.powi(3) * lat.to_radians().tan() / (12.0 * 6_371_000.0)
            };
        prop_assert!(
            (left + right - whole).abs() <= 1e-9 * whole + 2.0 * sliver + 2.0,
            "{left} + {right} vs {whole} (sliver bound {sliver})"
        );
        // A box around the whole polygon returns it unchanged.
        let r = p.bounding_rect().expect("non-empty");
        let around = BBox::new(r.min().y - 0.01, r.min().x - 0.01, r.max().y + 0.01, r.max().x + 0.01);
        let c = clip_to_bbox(&mp, around).expect("inside");
        prop_assert!(!c.clipped);
        prop_assert!(close(geodesic_area_m2(&c.geometry), a, 1e-6));
    }
}
