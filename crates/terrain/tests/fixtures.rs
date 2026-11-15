//! P5 on the reference data set: Horn slope and aspect of the 1 km terrain windows of
//! `fixtures/inputs/<site>/dem_window_5m.tif` (cut from the 5 m LiDAR HD model of each
//! site; `skimap fixtures` checks that their slope and aspect equal the terrain layer's).

#[path = "../../../fixtures/support/mod.rs"]
mod fixtures;

use skimap_common::crs::Crs;
use skimap_common::site::SITES;
use skimap_terrain::geotiff;
use skimap_terrain::slope::{FLAT_ASPECT, horn};

#[test]
fn horn_on_the_windows() {
    for site in SITES {
        let dem =
            geotiff::read_f32(&fixtures::input(&format!("{}/dem_window_5m.tif", site.id))).unwrap();
        assert_eq!(
            (dem.width(), dem.height(), dem.crs()),
            (200, 200, Crs::Lambert93)
        );
        assert!(dem.data().iter().all(|z| z.is_finite()));
        let sa = horn(&dem);
        let (w, h) = (dem.width(), dem.height());
        let mut steep = 0;
        for r in 0..h {
            for c in 0..w {
                let (s, a) = (*sa.slope.get(c, r).unwrap(), *sa.aspect.get(c, r).unwrap());
                if r == 0 || c == 0 || r == h - 1 || c == w - 1 {
                    assert!(s.is_nan() && a.is_nan(), "border ({c}, {r})");
                    continue;
                }
                assert!((0.0..90.0).contains(&s), "slope {s} at ({c}, {r})");
                assert!(a == FLAT_ASPECT || (0.0..360.0).contains(&a), "aspect {a}");
                steep += usize::from(s >= 30.0);
            }
        }
        let share = steep as f64 / ((w - 2) * (h - 2)) as f64;
        println!(
            "{}: {:.1} % of the window at 30° or more",
            site.id,
            share * 100.0
        );
        // The windows were chosen for the widest spread of slopes: steep terrain present.
        assert!(share > 0.1, "{share}");
    }
}
