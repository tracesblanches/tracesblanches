//! Property tests of the common geometry (Q3, #103): grid georeferencing, bilinear
//! sampling, projections, tiles, great-circle distances; GeoTIFF round trip with the
//! `geotiff` feature.

use proptest::prelude::*;
use skimap_common::crs::{self, Crs};
use skimap_common::geom::{BBox, LonLat};
use skimap_common::grid::{GeoTransform, Grid};

/// Mainland France and its borders, where Lambert-93 is used.
fn france() -> impl Strategy<Value = LonLat> {
    (-5.0f64..9.5, 41.3f64..51.1).prop_map(|(lon, lat)| LonLat::new(lon, lat))
}

fn transform() -> impl Strategy<Value = GeoTransform> {
    (
        -1e6f64..1e7,
        -1e6f64..1e7,
        prop_oneof![Just(5.0f64), Just(25.0), 0.01f64..1000.0],
    )
        .prop_map(|(x, y, c)| GeoTransform::north_up(x, y, c))
}

proptest! {
    /// Pixel ↔ world round trip, cell of a cell centre, and cells outside the grid.
    #[test]
    fn grid_georeferencing(
        gt in transform(),
        (w, h) in (1usize..300, 1usize..300),
        (c, r) in (0usize..300, 0usize..300),
        (fc, fr) in (0.0f64..1.0, 0.0f64..1.0),
    ) {
        let g = Grid::filled(w, h, gt, Crs::Lambert93, 0u8);
        let (x, y) = gt.pixel_to_world(c as f64 + fc, r as f64 + fr);
        let (pc, pr) = gt.world_to_pixel(x, y);
        let tol = 1e-6 * (1.0 + c.max(r) as f64);
        prop_assert!((pc - (c as f64 + fc)).abs() < tol && (pr - (r as f64 + fr)).abs() < tol);
        let (cx, cy) = gt.cell_center(c, r);
        match g.cell_at(cx, cy) {
            Some(cell) => prop_assert_eq!(cell, (c, r)),
            None => prop_assert!(c >= w || r >= h),
        }
        let (x0, y0, x1, y1) = g.bounds();
        prop_assert!(x0 < x1 && y0 < y1);
        prop_assert!(g.cell_at(x0 - gt.pixel_width, y1 - 0.5 * gt.pixel_width).is_none());
        prop_assert!(g.cell_at(f64::NAN, y0).is_none());
        prop_assert!(GeoTransform::from_gdal(gt.to_gdal()).is_ok_and(|t| t == gt));
    }

    /// Bilinear sampling returns the cell value at a centre and stays between the four
    /// neighbouring values elsewhere; a no-data neighbour gives no value.
    #[test]
    fn bilinear_sampling(
        (w, h) in (2usize..20, 2usize..20),
        values in prop::collection::vec(-1000.0f32..1000.0, 400),
        (c, r) in (0usize..19, 0usize..19),
        (fc, fr) in (0.0f64..1.0, 0.0f64..1.0),
    ) {
        let (c, r) = (c % (w - 1), r % (h - 1));
        let gt = GeoTransform::north_up(900_000.0, 6_500_000.0, 5.0);
        let data: Vec<f32> = values[..w * h].to_vec();
        let g = Grid::from_vec(w, h, gt, Crs::Lambert93, data.clone()).expect("shape");
        let (cx, cy) = gt.cell_center(c, r);
        let at = g.sample_bilinear(cx, cy).expect("inside");
        prop_assert!((at - f64::from(data[r * w + c])).abs() < 1e-3);
        let (x, y) = gt.pixel_to_world(c as f64 + 0.5 + fc, r as f64 + 0.5 + fr);
        let v = g.sample_bilinear(x, y).expect("inside the centre lattice");
        let four = [data[r * w + c], data[r * w + c + 1], data[(r + 1) * w + c], data[(r + 1) * w + c + 1]];
        let lo = four.iter().copied().fold(f32::INFINITY, f32::min);
        let hi = four.iter().copied().fold(f32::NEG_INFINITY, f32::max);
        prop_assert!(v >= f64::from(lo) - 1e-3 && v <= f64::from(hi) + 1e-3, "{v} not in [{lo}, {hi}]");
        let nd = g.clone().with_nodata(data[(r + 1) * w + c + 1]);
        if fc > 0.0 && fr > 0.0 {
            prop_assert!(nd.sample_bilinear(x, y).is_none());
        }
    }

    /// WGS84 → Lambert-93 → WGS84 and WGS84 → Web Mercator → WGS84 within 1e-9° (0.1 mm).
    #[test]
    fn projections_round_trip(p in france()) {
        let (x, y) = crs::to_lambert93(p).expect("project");
        let q = crs::from_lambert93(x, y).expect("unproject");
        prop_assert!((q.lon - p.lon).abs() < 1e-9 && (q.lat - p.lat).abs() < 1e-9, "{p:?} -> {q:?}");
        let (x, y) = crs::to_web_mercator(p).expect("project");
        let q = crs::from_web_mercator(x, y).expect("unproject");
        prop_assert!((q.lon - p.lon).abs() < 1e-9 && (q.lat - p.lat).abs() < 1e-9);
        prop_assert!(crs::transform_point(Crs::Wgs84, Crs::Lambert93, f64::NAN, p.lat).is_err());
    }

    /// Lambert-93 is conformal: the scale of a 1 m step is the same in every direction, and
    /// between 0.9990 (46.5° N) and 1.0060 (Corsica) over the country. Ground lengths on
    /// the GRS80 ellipsoid (radii of curvature at the point).
    #[test]
    fn lambert93_is_conformal(p in france(), bearing in 0.0f64..360.0) {
        let (a, e2) = (6_378_137.0f64, 0.006_694_380_022_90f64);
        let phi = p.lat.to_radians();
        let w = (1.0 - e2 * phi.sin().powi(2)).sqrt();
        let (m, n) = (a * (1.0 - e2) / w.powi(3), a / w);
        let scale = |bearing: f64| {
            let b = bearing.to_radians();
            let (dn, de) = (b.cos(), b.sin());
            let q = LonLat::new(p.lon + (de / (n * phi.cos())).to_degrees(), p.lat + (dn / m).to_degrees());
            let (x0, y0) = crs::to_lambert93(p).expect("project");
            let (x1, y1) = crs::to_lambert93(q).expect("project");
            (x1 - x0).hypot(y1 - y0)
        };
        let k = scale(bearing);
        prop_assert!((0.9990..1.0060).contains(&k), "scale {k} at {p:?}");
        prop_assert!((scale(bearing + 90.0) - k).abs() < 1e-5, "not conformal: {k} vs {}", scale(bearing + 90.0));
    }

    /// The projected box of a WGS84 box contains every projected point of the box.
    #[test]
    fn projected_bbox_contains_the_box(
        p in france(),
        (dn, de) in (0.01f64..1.0, 0.01f64..1.0),
        (u, v) in (0.0f64..=1.0, 0.0f64..=1.0),
        steps in 1usize..40,
    ) {
        let b = BBox::new(p.lat, p.lon, p.lat + dn, p.lon + de);
        let (x0, y0, x1, y1) = crs::project_bbox(b, Crs::Lambert93, steps).expect("project");
        // Points on the edges are inside; interior points of a conformal conic box are too.
        let q = LonLat::new(b.west + u * de, b.south + v * dn);
        let (x, y) = crs::to_lambert93(q).expect("project");
        let slack = if steps >= 10 { 1.0 } else { 300.0 };
        prop_assert!(x >= x0 - slack && x <= x1 + slack && y >= y0 - slack && y <= y1 + slack,
            "({x}, {y}) outside ({x0}, {y0}, {x1}, {y1})");
    }

    /// A position lies in the bounds of its tile, at every zoom.
    #[test]
    fn tile_contains_position(lon in -179.999f64..179.999, lat in -85.0f64..85.0, z in 0u8..=22) {
        let p = LonLat::new(lon, lat);
        let (x, y) = crs::tile_for(p, z);
        prop_assert!(x < 1u32 << z && y < 1u32 << z);
        let b = crs::tile_bounds(z, x, y);
        let eps = 1e-9;
        prop_assert!(b.west - eps <= lon && lon <= b.east + eps, "{lon} not in {b:?}");
        prop_assert!(b.south - eps <= lat && lat <= b.north + eps, "{lat} not in {b:?}");
    }

    /// Great-circle distances: zero to itself, symmetric, triangle inequality.
    #[test]
    fn haversine_is_a_metric(a in france(), b in france(), c in france()) {
        prop_assert!(a.haversine_m(a).abs() < 1e-6);
        prop_assert!((a.haversine_m(b) - b.haversine_m(a)).abs() < 1e-6);
        prop_assert!(a.haversine_m(c) <= a.haversine_m(b) + b.haversine_m(c) + 1e-6);
        prop_assert!(a.haversine_m(b) >= 0.0);
    }
}

#[cfg(feature = "geotiff")]
mod geotiff {
    use super::*;
    use skimap_common::geotiff::{decode_any, decode_f32, encode_f32};

    proptest! {
        /// Float32 GeoTIFF round trip: same size, georeferencing, CRS and bits (NaN as
        /// no-data); the tolerant reader gives the same values.
        #[test]
        fn f32_round_trip(
            gt in transform(),
            (w, h) in (1usize..40, 1usize..40),
            values in prop::collection::vec(prop_oneof![9 => -1e6f32..1e6, 1 => Just(f32::NAN)], 1600),
            crs in prop_oneof![Just(Crs::Lambert93), Just(Crs::Wgs84), Just(Crs::WebMercator)],
        ) {
            let g = Grid::from_vec(w, h, gt, crs, values[..w * h].to_vec()).expect("shape");
            let mut buf = std::io::Cursor::new(Vec::new());
            encode_f32(&mut buf, &g).expect("encode");
            let bytes = buf.into_inner();
            let back = decode_f32(&bytes).expect("decode");
            prop_assert_eq!(back.width(), w);
            prop_assert_eq!(back.height(), h);
            prop_assert_eq!(back.crs(), crs);
            prop_assert_eq!(back.transform(), &gt);
            for (a, b) in g.data().iter().zip(back.data()) {
                prop_assert!(a.to_bits() == b.to_bits() || (a.is_nan() && b.is_nan()));
            }
            let any = decode_any(&bytes).expect("tolerant decode");
            prop_assert_eq!(any.width, w);
            for (a, b) in g.data().iter().zip(&any.data) {
                prop_assert!(a.to_bits() == b.to_bits() || (a.is_nan() && b.is_nan()));
            }
        }
    }
}
