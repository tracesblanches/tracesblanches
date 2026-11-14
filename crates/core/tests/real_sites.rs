//! Real-data checks of C1 and C5 on the graph written by the pipeline
//! (`data/out/<site>/routes/piste_graph.json`, built by
//! `cargo run --release -p skimap-core --features pipeline --example routes`).
//!
//! Run with `cargo test -p skimap-core --test real_sites -- --ignored`.

use skimap_common::geom::LonLat;
use skimap_core::eta::{EtaModel, munter_s, tobler_s};
use skimap_core::graph::{PisteGraph, Snap};
use skimap_core::routing::{Distance, Route, Router, Style, StylePolicy};

fn graph(site: &str) -> PisteGraph {
    let root = std::env::var("SKIMAP_DATA_DIR")
        .unwrap_or_else(|_| concat!(env!("CARGO_MANIFEST_DIR"), "/../../data").to_owned());
    let p = std::path::Path::new(&root)
        .join("out")
        .join(site)
        .join("routes/piste_graph.json");
    PisteGraph::from_json(&std::fs::read_to_string(&p).unwrap_or_else(|e| panic!("{p:?}: {e}")))
        .unwrap()
}

fn shortest(g: &PisteGraph, a: LonLat, b: LonLat) -> Route {
    Router::new(
        g,
        Distance {
            style: Style::Classic,
            policy: StylePolicy::default(),
        },
    )
    .route(a, b, 500.0)
    .unwrap()
}

fn elev(g: &PisteGraph, s: &Snap) -> f64 {
    g.edge(s.edge)
        .profile
        .as_ref()
        .unwrap()
        .elevation_at(s.along_m)
}

fn max_elev(g: &PisteGraph, r: &Route) -> f64 {
    r.legs
        .iter()
        .flat_map(|l| g.stations(l.edge, l.from_m, l.to_m))
        .filter_map(|(_, z)| z)
        .fold(f64::NEG_INFINITY, f64::max)
}

/// Physical classic and skating, Munter and Tobler, seconds.
fn times(g: &PisteGraph, r: &Route) -> [f64; 4] {
    let p = r.pieces(g);
    [
        EtaModel::new(Style::Classic).time_s(&p).total_s,
        EtaModel::new(Style::Skating).time_s(&p).total_s,
        munter_s(&p, 4.0, 10.0),
        tobler_s(&p),
    ]
}

#[test]
#[ignore = "needs data/out/cervieres/routes/piste_graph.json"]
fn cervieres_laus_fonts_and_izoard() {
    let g = graph("cervieres");
    let laus = LonLat::new(6.725_006, 44.858_940);
    let fonts = LonLat::new(6.8157, 44.84465);
    let r = shortest(&g, laus, fonts);
    let (up, down) = r.gain_loss(&g);
    println!(
        "Laus → Fonts {:.2} km, D+ {up:.0} m, D- {down:.0} m",
        r.length_m() / 1000.0
    );
    assert!((r.length_m() - 11_360.0).abs() < 100.0);
    assert!((up - 462.0).abs() < 20.0 && (down - 145.0).abs() < 20.0);
    // refuges.info: Refuge des Fonts de Cervières, 2040 m.
    assert!((elev(&g, &r.end) - 2040.0).abs() < 10.0);
    let [c, s, m, t] = times(&g, &r);
    assert!(s < c && c < t && t < m, "{c} {s} {m} {t}");
    assert!((0.09..0.14).contains(&(c / s - 1.0)));
    // Le Laus → Brunissard over the Col d'Izoard (2360 m).
    let r = shortest(
        &g,
        LonLat::new(6.725_143, 44.859_064),
        LonLat::new(6.7172, 44.7994),
    );
    let top = max_elev(&g, &r);
    println!("Izoard: {:.2} km, top {top:.0} m", r.length_m() / 1000.0);
    assert!((top - 2360.0).abs() < 10.0, "{top}");
    // The two Le Laus trail ends are 18 m apart, beyond the 10 m connectors of P2.
    assert!(
        Router::new(
            &g,
            Distance {
                style: Style::Classic,
                policy: StylePolicy::default()
            }
        )
        .route(laus, LonLat::new(6.7172, 44.7994), 500.0)
        .is_err()
    );
}

#[test]
#[ignore = "needs data/out/chapelle-des-bois/routes/piste_graph.json"]
fn chapelle_village_chalet_gaillard() {
    let g = graph("chapelle-des-bois");
    let village = LonLat::new(6.1118, 46.5952);
    let gaillard = LonLat::new(6.12167, 46.56318);
    let r = shortest(&g, village, gaillard);
    println!("village → Chalet Gaillard {:.2} km", r.length_m() / 1000.0);
    assert!((r.length_m() - 12_900.0).abs() < 200.0);
    // refuges.info: Chalet Gaillard, 1232 m.
    assert!((elev(&g, &r.end) - 1232.0).abs() < 10.0);
    // Time routing avoids part of the climb for a slightly longer route.
    let rt = Router::new(&g, EtaModel::new(Style::Classic))
        .route(village, gaillard, 500.0)
        .unwrap();
    assert!(rt.length_m() >= r.length_m());
    assert!(rt.gain_loss(&g).0 < r.gain_loss(&g).0);
    let [c, s, m, t] = times(&g, &r);
    assert!(s < c && c < t && t < m, "{c} {s} {m} {t}");
}
