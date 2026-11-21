//! Golden fingerprints of the core on the reference data set (Q2, #102): trail graph
//! (C1), reference routes with their climb and estimated times (C1, C5), turnaround
//! (C7), map matching under noise (C2) and the zone sentences (C8) of both sites.
//!
//! References: `fixtures/golden/core/<site>.json`; update with
//! `SKIMAP_BLESS=1 cargo test -p skimap-core --test golden` and review the diff.
//!
//! Tolerances: every input is fixed (versioned graph, zone layer, seeded noise) and every
//! computation is deterministic, so numbers get `Tol::FLOAT` (floating-point noise only)
//! and counts and sentences are exact; instants are compared to the second (`Abs(1.0)`:
//! a last-bit change of a duration in seconds may move its rounding by one second).
//! Cross-checks with the research: Le Laus → Les Fonts 11.36 km, D+ 462 m, D− 145 m,
//! estimated 2 h 15 (docs/architecture/compiler.md); refuges.info altitudes of the Refuge
//! des Fonts (2040 m) and the Chalet Gaillard (1232 m); col d'Izoard 2360 m.

use jiff::civil::date;
#[path = "../../../fixtures/support/mod.rs"]
mod fixtures;

use fixtures::golden::{Golden, Tol};
use jiff::Timestamp;
use skimap_common::geom::LonLat;
use skimap_core::eta::{EtaModel, munter_s, tobler_s};
use skimap_core::graph::{HalfEdge, PisteGraph};
use skimap_core::matching::synth::{Noise, Rng, add_noise, sample_polyline};
use skimap_core::matching::{AlertEvent, MatchParams, MatchState, ROUTE_HYSTERESIS, RouteMonitor};
use skimap_core::routing::{Distance, Route, Router, Style, StylePolicy};
use skimap_core::turnaround::{TurnaroundParams, deadline, out_and_back};
use skimap_core::zone_eval::evaluate;
use skimap_zones::{RuleSet, ZoneLayer};

fn graph(site: &str) -> PisteGraph {
    let p = fixtures::layer_file(site, "routes", "piste_graph.json");
    PisteGraph::from_json(&std::fs::read_to_string(p).unwrap()).unwrap()
}

fn zones(site: &str) -> (ZoneLayer, RuleSet) {
    let read = |n: &str| std::fs::read_to_string(fixtures::layer_file(site, "zones", n)).unwrap();
    (
        ZoneLayer::from_geojson_str(&read("zones.geojson")).unwrap(),
        RuleSet::from_json_str(&read("rules.json")).unwrap(),
    )
}

fn distance() -> Distance {
    Distance {
        style: Style::Classic,
        policy: StylePolicy::default(),
    }
}

fn instant(s: &str) -> Timestamp {
    s.parse::<Timestamp>().unwrap()
}

fn seconds(t: Timestamp) -> f64 {
    t.as_second() as f64
}

fn graph_figures(g: &PisteGraph, k: &mut Golden) {
    let oneway_m: f64 = (0..g.edges().len())
        .filter(|&e| g.edge(e).oneway)
        .map(|e| g.length_m(e))
        .fold(0.0, |a, b| a + b);
    let (mut up, mut down) = (0.0, 0.0);
    for e in 0..g.edges().len() {
        let (u, d) = g.gain_loss(HalfEdge {
            edge: e,
            forward: true,
        });
        up += u;
        down += d;
    }
    let with_profile = g.edges().iter().filter(|e| e.profile.is_some()).count();
    k.count("graph.nodes", g.nodes().len())
        .count("graph.edges", g.edges().len())
        .count("graph.components", g.components().1)
        .count("graph.edges_with_profile", with_profile)
        .num("graph.length_m", g.total_length_m(), Tol::FLOAT)
        .num("graph.oneway_m", oneway_m, Tol::FLOAT)
        .num("graph.gain_forward_m", up, Tol::FLOAT)
        .num("graph.loss_forward_m", down, Tol::FLOAT);
}

/// Length, climb, end elevation and the four time models of a route.
fn route_figures(g: &PisteGraph, r: &Route, key: &str, k: &mut Golden) {
    let (up, down) = r.gain_loss(g);
    let p = r.pieces(g);
    let end = g
        .edge(r.end.edge)
        .profile
        .as_ref()
        .map(|pr| pr.elevation_at(r.end.along_m));
    let top = r
        .legs
        .iter()
        .flat_map(|l| g.stations(l.edge, l.from_m, l.to_m))
        .filter_map(|(_, z)| z)
        .fold(f64::NEG_INFINITY, f64::max);
    k.num(&format!("{key}.length_m"), r.length_m(), Tol::FLOAT)
        .count(&format!("{key}.legs"), r.legs.len())
        .num(&format!("{key}.gain_m"), up, Tol::FLOAT)
        .num(&format!("{key}.loss_m"), down, Tol::FLOAT)
        .num(&format!("{key}.top_m"), top, Tol::FLOAT)
        .num(
            &format!("{key}.eta_classic_s"),
            EtaModel::new(Style::Classic).time_s(&p).total_s,
            Tol::FLOAT,
        )
        .num(
            &format!("{key}.eta_skating_s"),
            EtaModel::new(Style::Skating).time_s(&p).total_s,
            Tol::FLOAT,
        )
        .num(
            &format!("{key}.munter_s"),
            munter_s(&p, 4.0, 10.0),
            Tol::FLOAT,
        )
        .num(&format!("{key}.tobler_s"), tobler_s(&p), Tol::FLOAT);
    if let Some(z) = end {
        k.num(&format!("{key}.end_elevation_m"), z, Tol::FLOAT);
    }
}

/// C8 sentence at a point and an instant.
fn zone_sentence(
    (layer, rules): &(ZoneLayer, RuleSet),
    key: &str,
    p: LonLat,
    at: &str,
    k: &mut Golden,
) {
    let e = evaluate(layer, rules, p, instant(at));
    k.lines(&format!("zones.{key}"), &e.describe_fr());
}

#[test]
fn cervieres() {
    let g = graph("cervieres");
    let mut k = Golden::new("core/cervieres");
    graph_figures(&g, &mut k);
    let laus = LonLat::new(6.725_006, 44.858_940);
    let fonts = LonLat::new(6.8157, 44.84465);
    let shortest = Router::new(&g, distance())
        .route(laus, fonts, 500.0)
        .unwrap();
    route_figures(&g, &shortest, "laus_fonts.shortest", &mut k);
    let fastest = Router::new(&g, EtaModel::new(Style::Classic))
        .route(laus, fonts, 500.0)
        .unwrap();
    route_figures(&g, &fastest, "laus_fonts.fastest_classic", &mut k);
    let izoard = Router::new(&g, distance())
        .route(
            LonLat::new(6.725_143, 44.859_064),
            LonLat::new(6.7172, 44.7994),
            500.0,
        )
        .unwrap();
    route_figures(&g, &izoard, "laus_brunissard", &mut k);
    // C7 on Le Laus → Les Fonts, 15 January 2027 (docs: start 13:00, back by 17:19).
    let date = date(2027, 1, 15);
    let p = TurnaroundParams::default();
    let dl = deadline(date, laus, &p).unwrap();
    k.num(
        "c7.2027-01-15.deadline",
        seconds(dl.deadline),
        Tol::Abs(1.0),
    );
    let ob = out_and_back(
        &g,
        &fastest,
        &EtaModel::new(Style::Classic),
        instant("2027-01-15T12:00:00Z"),
        &dl,
        &p,
    )
    .unwrap();
    k.text("c7.start_13h.whole_route", ob.whole_route.to_string())
        .num("c7.start_13h.turn_at_m", ob.turn_at_m, Tol::FLOAT)
        .num("c7.start_13h.arrival", seconds(ob.arrival), Tol::Abs(1.0));
    let z = zones("cervieres");
    zone_sentence(
        &z,
        "village_january",
        LonLat::new(6.7230, 44.8640),
        "2027-01-05T12:00:00Z",
        &mut k,
    );
    zone_sentence(
        &z,
        "casse_deserte_july_evening",
        LonLat::new(6.7381, 44.8119),
        "2027-07-05T19:00:00Z",
        &mut k,
    );
    zone_sentence(
        &z,
        "arvieux_july_evening",
        LonLat::new(6.75, 44.79),
        "2027-07-05T19:00:00Z",
        &mut k,
    );
    zone_sentence(
        &z,
        "italy",
        LonLat::new(6.85, 44.90),
        "2027-01-05T12:00:00Z",
        &mut k,
    );
    k.check();
}

#[test]
fn chapelle_des_bois() {
    let g = graph("chapelle-des-bois");
    let mut k = Golden::new("core/chapelle-des-bois");
    graph_figures(&g, &mut k);
    let village = LonLat::new(6.1118, 46.5952);
    let gaillard = LonLat::new(6.12167, 46.56318);
    let shortest = Router::new(&g, distance())
        .route(village, gaillard, 500.0)
        .unwrap();
    route_figures(&g, &shortest, "village_gaillard.shortest", &mut k);
    let fastest = Router::new(&g, EtaModel::new(Style::Classic))
        .route(village, gaillard, 500.0)
        .unwrap();
    route_figures(&g, &fastest, "village_gaillard.fastest_classic", &mut k);
    let skating = Router::new(&g, EtaModel::new(Style::Skating))
        .route(village, gaillard, 500.0)
        .unwrap();
    route_figures(&g, &skating, "village_gaillard.fastest_skating", &mut k);
    // C7: out and back from the village, 15 January 2027, start 13:00 and 14:00 local.
    let date = date(2027, 1, 15);
    let p = TurnaroundParams::default();
    let dl = deadline(date, village, &p).unwrap();
    k.num(
        "c7.2027-01-15.deadline",
        seconds(dl.deadline),
        Tol::Abs(1.0),
    )
    .num("c7.2027-01-15.sunset", seconds(dl.sunset), Tol::Abs(1.0));
    for (key, start) in [
        ("start_13h", "2027-01-15T12:00:00Z"),
        ("start_14h", "2027-01-15T13:00:00Z"),
    ] {
        let ob = out_and_back(
            &g,
            &fastest,
            &EtaModel::new(Style::Classic),
            instant(start),
            &dl,
            &p,
        )
        .unwrap();
        k.text(&format!("c7.{key}.whole_route"), ob.whole_route.to_string())
            .num(&format!("c7.{key}.turn_at_m"), ob.turn_at_m, Tol::FLOAT)
            .num(
                &format!("c7.{key}.arrival"),
                seconds(ob.arrival),
                Tol::Abs(1.0),
            );
    }
    // C2: Chalet Gaillard → north of the village under forest GPS noise (seed 5).
    let r = Router::new(&g, distance())
        .route(gaillard, LonLat::new(6.11442, 46.64122), 500.0)
        .unwrap();
    let plan = r.polyline(&g);
    let truth = sample_polyline(&plan, 1.0, |_| 3.5);
    let noise = Noise {
        sigma_m: 15.0,
        tau_s: 30.0,
        outlier_rate_hz: 0.02,
        outlier_m: 30.0,
        outlier_len_s: (2.0, 10.0),
    };
    let fixes = add_noise(&truth, &noise, &mut Rng(5));
    let mut mon = RouteMonitor::new(&g, &plan, MatchParams::default(), ROUTE_HYSTERESIS);
    let (mut near, mut alerts) = (0, 0);
    for (f, t) in fixes.iter().zip(&truth) {
        let out = mon.push(*f);
        if let Some(AlertEvent::Raised(_)) = out.event {
            alerts += 1;
        }
        if let MatchState::OnTrail(s) = out.state
            && s.pos.haversine_m(t.pos) < 25.0
        {
            near += 1;
        }
    }
    k.count("c2.fixes", fixes.len())
        .count("c2.matched_within_25m", near)
        .count("c2.alerts", alerts);
    let z = zones("chapelle-des-bois");
    let risoux = LonLat::new(6.10, 46.56);
    zone_sentence(
        &z,
        "risoux_february",
        risoux,
        "2027-02-10T10:00:00Z",
        &mut k,
    );
    zone_sentence(&z, "risoux_august", risoux, "2027-08-10T10:00:00Z", &mut k);
    zone_sentence(
        &z,
        "lac_des_mortes_january",
        LonLat::new(6.087, 46.588),
        "2027-01-10T10:00:00Z",
        &mut k,
    );
    zone_sentence(
        &z,
        "swiss_risoud_january",
        LonLat::new(6.15, 46.60),
        "2027-01-10T10:00:00Z",
        &mut k,
    );
    k.check();
}
