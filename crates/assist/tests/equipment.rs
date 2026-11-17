//! A6 equipment list on the reference layers, rule by rule (Q4, #104: survivors of
//! `cargo mutants -p skimap-assist` in `equipment.rs`): the distances along the route
//! (exposed terrain, no network) against an independent count, the return time, the
//! headlamp before sunrise and after sunset, the no-network thresholds (1 km or a
//! quarter of the route), long and off-piste outings, unreliable kick wax. No weather:
//! the weather items never appear.

#[path = "../../../fixtures/support/mod.rs"]
mod fixtures;

use chrono::{Duration, NaiveDate, NaiveDateTime};
use skimap_assist::equipment::{self, Kind, Outing};
use skimap_assist::inputs::{Paths, SiteData};
use skimap_common::geom::LonLat;
use skimap_common::layout::DataLayout;
use skimap_common::site::Site;
/// Evaluation site of `data/sites.json`, test data.
static CERVIERES: std::sync::LazyLock<skimap_common::site::Site> =
    std::sync::LazyLock::new(|| *skimap_common::site::site("cervieres").expect("evaluation site"));
/// Evaluation site of `data/sites.json`, test data.
static CHAPELLE_DES_BOIS: std::sync::LazyLock<skimap_common::site::Site> =
    std::sync::LazyLock::new(|| {
        *skimap_common::site::site("chapelle-des-bois").expect("evaluation site")
    });
use skimap_core::eta::EtaModel;
use skimap_core::routing::{Router, Style};
use skimap_core::turnaround::TurnaroundParams;

fn data(site: &Site) -> SiteData {
    SiteData::load(&DataLayout::new(fixtures::data()), site, &Paths::default()).unwrap()
}

fn at(s: &str) -> NaiveDateTime {
    NaiveDateTime::parse_from_str(s, "%Y-%m-%dT%H:%M").unwrap()
}

fn outing(route: Vec<LonLat>, start: &str, minutes: f64) -> Outing {
    Outing {
        label: "test".into(),
        start: at(start),
        duration_min: minutes,
        route,
        offpiste: false,
        kick_unreliable: false,
    }
}

fn kinds(e: &equipment::Equipment) -> Vec<Kind> {
    e.items.iter().map(|i| i.kind).collect()
}

/// Length along `route` (10 m steps) where `pred` holds, km.
fn km_where(route: &[LonLat], pred: impl Fn(LonLat) -> bool) -> (f64, f64) {
    let (mut yes, mut all) = (0.0, 0.0);
    for w in route.windows(2) {
        let d = w[0].haversine_m(w[1]);
        let n = (d / 10.0).ceil().max(1.0) as usize;
        for k in 0..n {
            let t = (k as f64 + 0.5) / n as f64;
            let p = LonLat::new(
                w[0].lon + (w[1].lon - w[0].lon) * t,
                w[0].lat + (w[1].lat - w[0].lat) * t,
            );
            all += d / n as f64;
            if pred(p) {
                yes += d / n as f64;
            }
        }
    }
    (yes / 1000.0, all / 1000.0)
}

fn chapelle_route(d: &SiteData) -> Vec<LonLat> {
    let g = d.graph.as_ref().unwrap();
    Router::new(g, EtaModel::new(Style::Classic))
        .route(
            LonLat::new(6.1118, 46.5952),
            LonLat::new(6.12167, 46.56318),
            500.0,
        )
        .unwrap()
        .polyline(g)
}

#[test]
fn distances_return_time_and_avalanche_terrain() {
    let d = data(&CHAPELLE_DES_BOIS);
    assert!(d.weather.is_none() && d.exposure.is_some());
    let route = chapelle_route(&d);
    let e = equipment::equipment(&d, &outing(route.clone(), "2027-01-15T10:00", 120.0));
    let (exposed, total) = km_where(&route, |p| d.exposure_at(p).is_some_and(|c| c >= 2));
    println!("{}", e.text);
    assert!(
        (e.facts.route_km - total).abs() < 0.02,
        "{:?} vs {total}",
        e.facts
    );
    assert!(
        (e.facts.exposed_km - exposed).abs() < 0.05,
        "{:?} vs {exposed}",
        e.facts
    );
    assert!(exposed >= 0.05);
    assert!(kinds(&e).contains(&Kind::AvalancheKit));
    assert!(e.text.contains("en terrain exposé aux avalanches (modèle)"));
    assert_eq!(e.facts.clpa_km, 0.0);
    // Back at start + duration × the turnaround factor.
    let f = TurnaroundParams::default().eta_factor;
    let back = at("2027-01-15T10:00") + Duration::seconds((120.0 * 60.0 * f) as i64);
    assert_eq!(e.facts.back, back);
    assert!(e.facts.sunset.is_some());
    // No weather: no weather item.
    for k in [
        Kind::WarmLayer,
        Kind::FaceProtection,
        Kind::Compass,
        Kind::SunProtection,
    ] {
        assert!(!kinds(&e).contains(&k), "{k:?}");
    }
    assert_eq!((e.facts.fog_hours, e.facts.cloud_mean_pct), (0, None));
    // 1.5 km without network (report 06 coverage): power bank, tell someone.
    let areas = d.no_network.as_ref().unwrap();
    let (none, _) = km_where(&route, |p| areas.find(p).is_some());
    assert!(
        (e.facts.no_network_km - none).abs() < 0.05,
        "{:?} vs {none}",
        e.facts
    );
    assert!(none >= 1.0);
    assert!(kinds(&e).contains(&Kind::PowerBank) && kinds(&e).contains(&Kind::TellSomeone));
    // Midday, two hours: no headlamp, no waxless skis.
    for k in [Kind::Headlamp, Kind::WaxlessSkis] {
        assert!(!kinds(&e).contains(&k), "{k:?}");
    }
}

/// The start of `route` up to its first point without network.
fn with_network(d: &SiteData, route: &[LonLat]) -> Vec<LonLat> {
    let areas = d.no_network.as_ref().unwrap();
    let end = route
        .iter()
        .position(|p| areas.find(*p).is_some())
        .unwrap_or(route.len());
    let r = route[..end.saturating_sub(1)].to_vec();
    assert!(r.len() >= 2);
    assert_eq!(km_where(&r, |p| areas.find(p).is_some()).0, 0.0);
    r
}

#[test]
fn headlamp_long_offpiste_and_wax() {
    let d = data(&CHAPELLE_DES_BOIS);
    let route = chapelle_route(&d);
    let date = NaiveDate::from_ymd_opt(2027, 1, 15).unwrap();
    let (rise, set) = skimap_core::ephemeris::sun_times(date, route[0]).unwrap();
    let (rise, set) = (
        skimap_core::ephemeris::local_time(rise),
        skimap_core::ephemeris::local_time(set),
    );
    let f = TurnaroundParams::default().eta_factor;
    // Back less than an hour before sunset: headlamp, with both times.
    let minutes = 60.0;
    let start = set - Duration::minutes(90) - Duration::seconds((minutes * 60.0 * f) as i64);
    let o = Outing {
        start,
        ..outing(route.clone(), "2027-01-15T10:00", minutes)
    };
    let e = equipment::equipment(&d, &o);
    assert!(!kinds(&e).contains(&Kind::Headlamp), "{}", e.text);
    let o = Outing {
        start: start + Duration::minutes(31),
        ..o
    };
    let e = equipment::equipment(&d, &o);
    assert!(kinds(&e).contains(&Kind::Headlamp), "{}", e.text);
    assert!(e.text.contains("coucher du soleil à"), "{}", e.text);
    // Start before sunrise.
    let o = Outing {
        start: rise - Duration::minutes(1),
        ..outing(route.clone(), "2027-01-15T10:00", 60.0)
    };
    let e = equipment::equipment(&d, &o);
    assert!(kinds(&e).contains(&Kind::Headlamp));
    assert!(e.text.contains("avant le lever du soleil"), "{}", e.text);
    let o = Outing {
        start: rise + Duration::minutes(1),
        ..o
    };
    assert!(!kinds(&equipment::equipment(&d, &o)).contains(&Kind::Headlamp));
    // More than three hours, off the pistes, unreliable kick (on a stretch with network:
    // the no-network rule says « tell someone » on its own).
    let route = with_network(&d, &route);
    let long = equipment::equipment(&d, &outing(route.clone(), "2027-01-15T09:00", 181.0));
    assert!(kinds(&long).contains(&Kind::TellSomeone));
    assert!(
        long.text
            .contains("sortie de plus de 3 h (181 min estimées)"),
        "{}",
        long.text
    );
    let three = equipment::equipment(&d, &outing(route.clone(), "2027-01-15T09:00", 180.0));
    assert!(!kinds(&three).contains(&Kind::TellSomeone));
    let o = Outing {
        offpiste: true,
        kick_unreliable: true,
        ..outing(route, "2027-01-15T10:00", 60.0)
    };
    let e = equipment::equipment(&d, &o);
    assert!(e.text.contains("sortie hors des pistes"), "{}", e.text);
    assert!(kinds(&e).contains(&Kind::WaxlessSkis));
}

#[test]
fn no_network_thresholds() {
    let d = data(&CERVIERES);
    let areas = d.no_network.as_ref().unwrap();
    // A point inside a no-network area and a short route around it.
    let inside = (0..2000)
        .map(|k| {
            LonLat::new(
                6.70 + (k % 50) as f64 * 0.003,
                44.80 + (k / 50) as f64 * 0.002,
            )
        })
        .find(|p| {
            areas.find(*p).is_some() && areas.find(LonLat::new(p.lon + 0.0026, p.lat)).is_some()
        })
        .expect("a 200 m stretch without network");
    let short = vec![inside, LonLat::new(inside.lon + 0.0026, inside.lat)];
    let e = equipment::equipment(&d, &outing(short.clone(), "2027-01-15T10:00", 10.0));
    // 0.2 km, all of it without network: a quarter of the route or more.
    assert!(
        e.facts.no_network_km < 1.0 && e.facts.no_network_km > 0.15,
        "{:?}",
        e.facts
    );
    assert!(kinds(&e).contains(&Kind::PowerBank), "{}", e.text);
    assert!(e.text.contains("(100 % de l'itinéraire)"), "{}", e.text);
    // The same stretch within a long route with network: below a quarter, under 1 km.
    let g = d.graph.as_ref().unwrap();
    let mut long: Vec<LonLat> = Router::new(g, EtaModel::new(Style::Classic))
        .route(
            LonLat::new(6.725_143, 44.859_064),
            LonLat::new(6.7172, 44.7994),
            500.0,
        )
        .unwrap()
        .polyline(g);
    let (none, total) = km_where(&long, |p| areas.find(p).is_some());
    long.splice(0..0, short);
    let e = equipment::equipment(&d, &outing(long, "2027-01-15T10:00", 60.0));
    let share = e.facts.no_network_km / e.facts.route_km;
    println!("{none} of {total} km without network; {:?}", e.facts);
    if e.facts.no_network_km < 1.0 && share < 0.25 {
        assert!(!kinds(&e).contains(&Kind::PowerBank), "{}", e.text);
    } else {
        assert!(kinds(&e).contains(&Kind::PowerBank), "{}", e.text);
    }
}
