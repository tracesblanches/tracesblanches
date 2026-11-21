//! Nordic areas entered along a route (H2 #133): each area said once where the route
//! enters it; the start's area and an area entered again (the way back) are not said; a
//! route that grazes the neighbour's outline does not change area.

use geo::{LineString, MultiPolygon, Polygon};
use skimap_assist::layers::{Area, Domain};
use skimap_common::geom::LonLat;
use skimap_plan::plan::{DOMAIN_HOLD_M, domain_changes};

/// A square area from `lon0` to `lon1` (latitudes 46.0 to 46.1).
fn area(id: &str, lon0: f64, lon1: f64) -> Domain {
    let ring = LineString::from(vec![
        (lon0, 46.0),
        (lon1, 46.0),
        (lon1, 46.1),
        (lon0, 46.1),
        (lon0, 46.0),
    ]);
    Domain {
        id: id.into(),
        name: format!("Domaine {id}"),
        km_nordic: 10.0,
        point: LonLat::new((lon0 + lon1) / 2.0, 46.05),
        area: Area::new(id, MultiPolygon(vec![Polygon::new(ring, vec![])])),
        ways: vec![],
    }
}

fn line(lons: &[f64]) -> Vec<LonLat> {
    lons.iter().map(|l| LonLat::new(*l, 46.05)).collect()
}

#[test]
fn out_and_back_says_the_neighbour_once() {
    let d = [area("site-a", 0.0, 0.1), area("site-b", 0.1, 0.2)];
    // A to B and back to A: B said once, at the border (0.1° ≈ 7.7 km at 46°).
    let c = domain_changes(&line(&[0.05, 0.15, 0.05]), &d);
    assert_eq!(c.len(), 1, "{c:?}");
    assert_eq!(c[0].id, "site-b");
    let border = LonLat::new(0.05, 46.05).haversine_m(LonLat::new(0.1, 46.05));
    assert!((c[0].s - border).abs() < 30.0, "{} vs {border}", c[0].s);
}

#[test]
fn grazing_the_border_is_not_a_change() {
    let d = [area("site-a", 0.0, 0.1), area("site-b", 0.1, 0.2)];
    // 100 m into B and back: 200 m of route in B, less than the hold.
    let into = 100.0 / LonLat::new(0.1, 46.05).haversine_m(LonLat::new(0.2, 46.05)) * 0.1;
    const { assert!(2.0 * 100.0 < DOMAIN_HOLD_M) };
    let c = domain_changes(&line(&[0.05, 0.1 + into, 0.05]), &d);
    assert!(c.is_empty(), "{c:?}");
}

#[test]
fn a_site_inside_a_network_is_the_area() {
    // The network covers both; the site inside it is the area of its points.
    let d = [
        area("net-n", 0.0, 0.2),
        area("site-a", 0.0, 0.1),
        area("site-b", 0.1, 0.2),
    ];
    let c = domain_changes(&line(&[0.05, 0.15]), &d);
    assert_eq!(
        c.iter().map(|x| x.id.as_str()).collect::<Vec<_>>(),
        ["site-b"]
    );
    // Without areas, or in one area, nothing.
    assert!(domain_changes(&line(&[0.05, 0.15]), &[]).is_empty());
    assert!(domain_changes(&line(&[0.01, 0.09]), &d).is_empty());
}
