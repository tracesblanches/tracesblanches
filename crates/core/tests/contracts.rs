//! Contracts of the core that the existing tests left open (Q4, #104): each test pins a
//! behaviour that a mutant surviving `cargo mutants -p skimap-core` changed without any
//! test failing (style factors, route properties, graph reading, time breakdown,
//! Munter / Tobler boundaries, turnaround arithmetic, deviation hysteresis).

use jiff::civil::date;
#[path = "../../../fixtures/support/mod.rs"]
mod fixtures;

use jiff::Timestamp;
use serde_json::{Map, json};
use skimap_common::geom::LonLat;
use skimap_core::eta::{Breakdown, EtaModel, FLAT_GRADE, SnowClass, friction, munter_s, tobler_s};
use skimap_core::graph::{Difficulty, EdgeKind, PisteGraph};
use skimap_core::matching::{
    AlertEvent, CorridorMonitor, DeviationDetector, Fix, Hysteresis, MatchParams, ROUTE_HYSTERESIS,
    RouteMonitor,
};
use skimap_core::routing::{CostModel, Distance, Piece, Router, Style, StylePolicy, partial_cost};
use skimap_core::turnaround::{TurnaroundParams, Urgency, deadline, out_and_back, status};

/// Metres per degree of latitude used to lay out the synthetic graph.
const M_LAT: f64 = 111_195.0;

/// Four nodes on a line at 46.6° N (1 km apart) and a spur:
///
/// ```text
/// 0 ──e0 A (classic+skating, novice)── 1 ──e1 B (skating only)── 2 ──e2 C (classic only)── 3
///                                        └──e3 D (snowshoe, one-way 1 → 4)── 4
/// ```
fn synthetic() -> PisteGraph {
    let lon = |x_m: f64| 6.1 + x_m / (M_LAT * 46.6f64.to_radians().cos());
    let lat = |y_m: f64| 46.6 + y_m / M_LAT;
    let p = |x: f64, y: f64| json!([lon(x), lat(y)]);
    let node = |i: usize, x: f64, y: f64| {
        json!({"type": "Feature", "geometry": {"type": "Point", "coordinates": p(x, y)},
               "properties": {"node": i, "osm_node_id": 100 + i}})
    };
    let nodes = json!({"type": "FeatureCollection", "features": [
        node(0, 0.0, 0.0), node(1, 1000.0, 0.0), node(2, 2000.0, 0.0), node(3, 3000.0, 0.0),
        node(4, 1000.0, -800.0),
    ]});
    let edge = |i: usize,
                from: usize,
                to: usize,
                pts: Vec<serde_json::Value>,
                props: serde_json::Value| {
        let mut m = props.as_object().unwrap().clone();
        m.insert("edge".into(), json!(i));
        m.insert("from".into(), json!(from));
        m.insert("to".into(), json!(to));
        json!({"type": "Feature", "geometry": {"type": "LineString", "coordinates": pts},
               "properties": m})
    };
    let edges = json!({"type": "FeatureCollection", "features": [
        edge(0, 0, 1, vec![p(0.0, 0.0), p(400.0, 0.0), p(1000.0, 0.0)],
             json!({"kind": "piste", "name": "A", "uses": ["nordic"], "classic": true, "skating": true,
                    "difficulty": "novice", "difficulty_max": "expert", "osm_way_id": 7})),
        edge(1, 1, 2, vec![p(1000.0, 0.0), p(2000.0, 0.0)],
             json!({"kind": "piste", "name": "B", "uses": ["nordic"], "classic": false, "skating": true,
                    "difficulty": "intermediate"})),
        edge(2, 2, 3, vec![p(2000.0, 0.0), p(3000.0, 0.0)],
             json!({"kind": "piste", "name": "C", "uses": ["nordic"], "classic": true, "skating": false,
                    "difficulty": "freeride"})),
        edge(3, 1, 4, vec![p(1000.0, 0.0), p(1000.0, -800.0)],
             json!({"kind": "piste", "name": "D", "uses": ["snowshoe"], "oneway": true,
                    "difficulty": "unknown"})),
    ]});
    PisteGraph::from_geojson("synthetic", &edges.to_string(), &nodes.to_string()).unwrap()
}

#[test]
fn reading_the_osm_layer() {
    let g = synthetic();
    let a = &g.edge(0).attrs;
    assert_eq!(a.kind, EdgeKind::Piste);
    assert_eq!(
        (a.difficulty, a.difficulty_max),
        (Some(Difficulty::Novice), Some(Difficulty::Advanced))
    );
    assert_eq!(g.edge(1).attrs.difficulty, Some(Difficulty::Intermediate));
    assert_eq!(g.edge(2).attrs.difficulty, Some(Difficulty::Advanced));
    assert_eq!(g.edge(3).attrs.difficulty, None);
    // A snowshoe trail is a hike-only trail.
    assert!(g.edge(3).attrs.hike && !g.edge(3).attrs.nordic && g.edge(3).attrs.hike_only());
    assert!(g.edge(3).oneway && !g.edge(0).oneway);
    // Cumulative distances: 0, 400, 1000 m along e0.
    let c = g.cumulative_m(0);
    assert_eq!(c.len(), 3);
    assert_eq!(c[0], 0.0);
    assert!((c[1] - 400.0).abs() < 1.0 && (c[2] - g.length_m(0)).abs() < 1e-9);
    // Node and edge indices must be complete.
    let sparse_nodes = json!({"type": "FeatureCollection", "features": [
        {"type": "Feature", "geometry": {"type": "Point", "coordinates": [6.1, 46.6]},
         "properties": {"node": 2}}]});
    let no_edges = json!({"type": "FeatureCollection", "features": []});
    let e = PisteGraph::from_geojson("x", &no_edges.to_string(), &sparse_nodes.to_string());
    assert!(e.unwrap_err().to_string().contains("node 0 missing"));
}

#[test]
fn stations_and_sub_polylines_are_strictly_between_their_ends() {
    let g = synthetic();
    let c = g.cumulative_m(0).to_vec();
    // From the middle vertex (400 m) to 1000 m: the vertex is the first station, once.
    let s = g.stations(0, c[1], c[2]);
    assert_eq!(s.len(), 2, "{s:?}");
    assert_eq!((s[0].0, s[1].0), (c[1], c[2]));
    assert!(s.iter().all(|(_, z)| z.is_none()));
    // Across the vertex, both directions.
    let s = g.stations(0, 100.0, 900.0);
    assert_eq!(
        s.iter().map(|x| x.0).collect::<Vec<_>>(),
        vec![100.0, c[1], 900.0]
    );
    let r = g.stations(0, 900.0, 100.0);
    assert_eq!(
        r.iter().map(|x| x.0).collect::<Vec<_>>(),
        vec![900.0, c[1], 100.0]
    );
    let p = g.sub_polyline(0, c[1], c[2]);
    assert_eq!(p.len(), 2, "{p:?}");
    let p = g.sub_polyline(0, 900.0, 100.0);
    assert_eq!(p.len(), 3);
    assert!(p[0].lon > p[2].lon);
    // Clamped positions.
    assert_eq!(g.point_at(0, -5.0), g.point_at(0, 0.0));
    assert_eq!(g.point_at(0, 1e9), g.point_at(0, c[2]));
}

#[test]
fn style_factors() {
    let g = synthetic();
    let p = StylePolicy::default();
    let f = |e: usize, s: Style| p.factor(&g.edge(e).attrs, s);
    assert_eq!(f(0, Style::Classic), 1.0);
    assert_eq!(f(0, Style::Skating), 1.0);
    assert_eq!(f(1, Style::Classic), p.classic_on_skating_only);
    assert_eq!(f(1, Style::Skating), 1.0);
    assert_eq!(f(2, Style::Skating), p.skating_on_classic_only);
    assert_eq!(f(2, Style::Classic), 1.0);
    assert_eq!(f(3, Style::Classic), p.hike_only);
    // The time model routes with the same factors.
    let m = EtaModel::new(Style::Classic);
    assert_eq!(m.edge_factor(&g.edge(1).attrs), p.classic_on_skating_only);
    assert_eq!(m.edge_factor(&g.edge(0).attrs), 1.0);
    // A forbidding factor, or a one-way edge travelled backwards, costs infinity.
    let forbid = Distance {
        style: Style::Skating,
        policy: StylePolicy {
            skating_on_classic_only: f64::INFINITY,
            ..p
        },
    };
    assert_eq!(partial_cost(&g, &forbid, 2, 0.0, 100.0), f64::INFINITY);
    let d = Distance {
        style: Style::Classic,
        policy: p,
    };
    assert_eq!(partial_cost(&g, &d, 3, 500.0, 100.0), f64::INFINITY);
    assert!((partial_cost(&g, &d, 3, 100.0, 500.0) - 400.0 * p.hike_only).abs() < 1.0);
    assert!((partial_cost(&g, &d, 0, 500.0, 100.0) - 400.0).abs() < 1.0);
}

#[test]
fn route_properties() {
    let g = synthetic();
    let d = Distance {
        style: Style::Classic,
        policy: StylePolicy::default(),
    };
    let start = g.point_at(0, 100.0);
    let end = g.point_at(2, 500.0);
    let r = Router::new(&g, d).route(start, end, 50.0).unwrap();
    assert_eq!(r.names(&g), vec!["A", "B", "C"]);
    assert!((r.length_m() - 2400.0).abs() < 2.0, "{}", r.length_m());
    // No profile: no coverage, no climb.
    assert_eq!(r.profile_coverage(&g), 0.0);
    let mut props = Map::new();
    props.insert("label".into(), json!("t"));
    let f = r.to_feature(&g, props);
    let pr = &f["properties"];
    assert_eq!(pr["label"], "t");
    assert_eq!(pr["names"], json!(["A", "B", "C"]));
    assert_eq!(pr["edges"], json!([0, 1, 2]));
    assert_eq!(pr["profile_coverage"], 0.0);
    let len = pr["length_m"].as_f64().unwrap();
    assert_eq!(len, (r.length_m() * 10.0).round() / 10.0);
    assert_eq!(
        (pr["gain_m"].as_f64(), pr["loss_m"].as_f64()),
        (Some(0.0), Some(0.0))
    );
    let coords = f["geometry"]["coordinates"].as_array().unwrap();
    // Start, the vertices at 400, 1000 and 2000 m, end; seven decimals.
    assert_eq!(coords.len(), 5, "{coords:?}");
    let lon = coords[0][0].as_f64().unwrap();
    assert_eq!(lon, (start.lon * 1e7).round() / 1e7);
    assert!((lon - start.lon).abs() < 1e-7);
    assert_eq!(f["geometry"]["type"], "LineString");
    assert!(pr["snap_start_m"].as_f64().unwrap() < 1.0);
}

#[test]
fn route_profile_coverage_on_the_real_graphs() {
    let load = |site: &str| {
        let p = fixtures::layer_file(site, "routes", "piste_graph.json");
        PisteGraph::from_json(&std::fs::read_to_string(p).unwrap()).unwrap()
    };
    let d = Distance {
        style: Style::Classic,
        policy: StylePolicy::default(),
    };
    let g = load("cervieres");
    let r = Router::new(&g, d)
        .route(
            LonLat::new(6.725_006, 44.858_940),
            LonLat::new(6.8157, 44.84465),
            500.0,
        )
        .unwrap();
    assert_eq!(r.profile_coverage(&g), 1.0);
    // Chapelle-des-Bois: from an edge with a profile to a neighbouring edge on the Swiss
    // side, without terrain model.
    let g = load("chapelle-des-bois");
    let router = Router::new(&g, d);
    let ends = |e: usize| [g.edge(e).from, g.edge(e).to];
    let r = (0..g.edges().len())
        .filter(|&e| g.edge(e).profile.is_none())
        .find_map(|e| {
            (0..g.edges().len())
                .filter(|&f| {
                    g.edge(f).profile.is_some() && ends(f).iter().any(|n| ends(e).contains(n))
                })
                .find_map(|f| {
                    let from = g.point_at(f, g.length_m(f) / 2.0);
                    let to = g.point_at(e, g.length_m(e) / 2.0);
                    router.route(from, to, 5.0).ok()
                })
        })
        .expect("an edge without profile next to one with");
    let c = r.profile_coverage(&g);
    assert!(c > 0.0 && c < 1.0, "{c}");
}

fn piece(attrs: &skimap_core::graph::EdgeAttrs, dist_m: f64, dz_m: f64) -> Piece<'_> {
    Piece {
        attrs,
        dist_m,
        dz_m,
        elev_m: Some(1200.0),
    }
}

#[test]
fn time_breakdown_by_terrain() {
    let g = synthetic();
    let a = &g.edge(0).attrs;
    let m = EtaModel::new(Style::Classic);
    let pieces = [
        piece(a, 100.0, 10.0),
        piece(a, 100.0, 100.0 * FLAT_GRADE),
        piece(a, 100.0, -10.0),
        piece(a, 100.0, -100.0 * FLAT_GRADE),
        piece(a, 0.0, 0.0),
    ];
    let t: Vec<f64> = pieces
        .iter()
        .map(|p| m.piece_time_s(p.attrs, p.dist_m, p.dz_m, p.elev_m))
        .collect();
    let b = m.time_s(&pieces);
    assert_eq!(t[4], 0.0);
    assert!((b.total_s - t.iter().sum::<f64>()).abs() < 1e-9);
    assert!((b.uphill_s - t[0]).abs() < 1e-9, "{b:?}");
    assert!((b.downhill_s - t[2]).abs() < 1e-9, "{b:?}");
    // Exactly ±2 %: flat.
    assert!((b.flat_s - (t[1] + t[3])).abs() < 1e-9, "{b:?}");
    assert_eq!((b.dist_m, b.uphill_m, b.downhill_m), (400.0, 100.0, 100.0));
    assert!(
        (b.gain_m - 12.0).abs() < 1e-9 && (b.loss_m - 12.0).abs() < 1e-9,
        "{b:?}"
    );
    assert!((b.mean_kmh() - 400.0 / b.total_s * 3.6).abs() < 1e-9);
    assert_eq!(Breakdown::default().mean_kmh(), 0.0);
    // Uphill slower than flat, flat slower than downhill.
    assert!(t[0] > t[1] && t[1] > t[2]);
}

#[test]
fn munter_tobler_and_friction() {
    let g = synthetic();
    let a = &g.edge(0).attrs;
    // Munter: −2 % exactly is walked at the uphill rate, below at the downhill rate.
    let flat = [piece(a, 1000.0, -20.0)];
    assert!((munter_s(&flat, 4.0, 10.0) - (1.0 + 0.2) / 4.0 * 3600.0).abs() < 1e-9);
    let down = [piece(a, 1000.0, -21.0)];
    assert!((munter_s(&down, 4.0, 10.0) - (1.0 + 0.21) / 10.0 * 3600.0).abs() < 1e-9);
    // A zero-length piece adds its climb only (Munter) and nothing to Tobler.
    let zero = [piece(a, 0.0, 0.0), piece(a, 1000.0, 0.0)];
    let tob = tobler_s(&zero);
    assert!(tob.is_finite());
    let kmh = 6.0 * (-3.5f64 * 0.05).exp();
    assert!((tob - 1000.0 / (kmh / 3.6)).abs() < 1e-9);
    assert!((munter_s(&zero, 4.0, 10.0) - 900.0).abs() < 1e-9);
    // New snow brakes 1.4 times more than groomed dry snow.
    let dry = friction(SnowClass::Groomed, -10.0, false);
    assert!((friction(SnowClass::NewSnow, -10.0, false) - 1.4 * dry).abs() < 1e-12);
    assert_eq!(friction(SnowClass::Groomed, -10.0, true), dry.max(0.08));
}

#[test]
fn classic_speed_balances_the_power() {
    let m = EtaModel::new(Style::Classic);
    let k = m.skier;
    for grade in [0.0, 0.05, 0.15, 0.3] {
        let v = m.classic_speed_ms(grade, 1200.0, false);
        let mu = friction(m.conditions.snow, m.conditions.temperature_c, false) * k.friction_factor;
        let alpha = f64::atan(grade);
        let resist = k.mass_kg * skimap_core::eta::G * (alpha.sin() + mu * alpha.cos());
        let drag =
            0.5 * skimap_core::eta::air_density(1200.0, m.conditions.temperature_c) * k.cda_m2;
        let power = v * resist + drag * v.powi(3);
        assert!(
            (power - k.power_w).abs() < 1e-6,
            "grade {grade}: {v} m/s, {power} W"
        );
    }
    // Steep descents are capped.
    assert_eq!(m.classic_speed_ms(-0.3, 1200.0, false), k.max_speed_ms);
}

fn instant(s: &str) -> Timestamp {
    s.parse::<Timestamp>().unwrap()
}

#[test]
fn turnaround_uses_the_whole_budget_and_turns_on_the_route() {
    let p = fixtures::layer_file("chapelle-des-bois", "routes", "piste_graph.json");
    let g = PisteGraph::from_json(&std::fs::read_to_string(p).unwrap()).unwrap();
    let village = LonLat::new(6.1118, 46.5952);
    let model = EtaModel::new(Style::Classic);
    let r = Router::new(&g, model)
        .route(village, LonLat::new(6.12167, 46.56318), 500.0)
        .unwrap();
    let date = date(2027, 1, 15);
    let params = TurnaroundParams {
        eta_factor: 1.3,
        ..TurnaroundParams::default()
    };
    let dl = deadline(date, village, &params).unwrap();
    let start = instant("2027-01-15T13:30:00Z");
    let ob = out_and_back(&g, &r, &model, start, &dl, &params).unwrap();
    assert!(!ob.whole_route && ob.turn_at_m > 0.0 && ob.turn_at_m < r.length_m());
    // The arrival is the deadline (the budget is used up), within a second.
    assert!(
        (ob.arrival - dl.deadline).as_secs().abs() <= 1,
        "{ob:?} {dl:?}"
    );
    assert!(start < ob.turn_time && ob.turn_time < ob.arrival);
    assert!(ob.whole_arrival > dl.deadline);
    // Time out to the turnaround point, with the factor: the model time along the route
    // up to turn_at_m.
    let mut out = 0.0;
    let mut d = 0.0;
    for pc in r.pieces(&g) {
        let t = model.piece_time_s(pc.attrs, pc.dist_m, pc.dz_m, pc.elev_m);
        if d + pc.dist_m >= ob.turn_at_m {
            out += t * (ob.turn_at_m - d) / pc.dist_m;
            break;
        }
        out += t;
        d += pc.dist_m;
    }
    let got = (ob.turn_time - start).as_millis() as i64 as f64 / 1000.0;
    assert!(
        (got - out * params.eta_factor).abs() < 2.0,
        "{got} vs {}",
        out * params.eta_factor
    );
    // The turnaround point lies on the route, at turn_at_m from the start.
    let line = r.polyline(&g);
    let mut along = 0.0;
    let mut best = f64::INFINITY;
    for w in line.windows(2) {
        let seg = w[0].haversine_m(w[1]);
        if along + seg >= ob.turn_at_m {
            let t = (ob.turn_at_m - along) / seg;
            let q = LonLat::new(
                w[0].lon + (w[1].lon - w[0].lon) * t,
                w[0].lat + (w[1].lat - w[0].lat) * t,
            );
            best = q.haversine_m(ob.turn_pos);
            break;
        }
        along += seg;
    }
    assert!(best < 2.0, "turnaround point {best} m off the route");
    // A start after the deadline: no turnaround possible.
    assert!(
        out_and_back(
            &g,
            &r,
            &model,
            dl.deadline + jiff::SignedDuration::from_mins(1),
            &dl,
            &params
        )
        .is_none()
    );
}

#[test]
fn turnaround_status_thresholds() {
    let village = LonLat::new(6.1118, 46.5952);
    let p = TurnaroundParams::default();
    let dl = deadline(date(2027, 1, 15), village, &p).unwrap();
    let at = |margin_s: i64| {
        dl.deadline
            - jiff::SignedDuration::from_secs(margin_s)
            - jiff::SignedDuration::from_secs(600)
    };
    let s = status(at(1800), 600.0 / p.eta_factor, &dl, &p);
    assert!((s.margin_s - 1800.0).abs() < 1e-3, "{s:?}");
    assert_eq!(s.urgency, Urgency::Ok);
    assert_eq!(
        status(at(1799), 600.0 / p.eta_factor, &dl, &p).urgency,
        Urgency::Soon
    );
    assert_eq!(
        status(at(0), 600.0 / p.eta_factor, &dl, &p).urgency,
        Urgency::Now
    );
    assert!((s.time_back_s - 600.0).abs() < 1e-9);
    assert_eq!(
        s.latest_turnaround,
        dl.deadline - jiff::SignedDuration::from_secs(600)
    );
}

#[test]
fn deviation_hysteresis_boundaries() {
    let h = Hysteresis {
        enter_m: 40.0,
        enter_s: 10.0,
        exit_m: 15.0,
        exit_s: 5.0,
    };
    let mut d = DeviationDetector::new(h);
    assert!(!d.alerting());
    // Exactly at the entry distance: not beyond.
    for t in 0..20 {
        assert_eq!(d.push(f64::from(t), 40.0), None);
    }
    assert!(d.push(20.0, 40.1).is_none());
    assert_eq!(d.push(30.0, 41.0), Some(AlertEvent::Raised(30.0)));
    assert!(d.alerting());
    // Exactly at the exit distance: still alerting.
    for t in 31..50 {
        assert_eq!(d.push(f64::from(t), 15.0), None);
    }
    assert!(d.push(50.0, 14.9).is_none());
    assert_eq!(d.push(55.0, 14.0), Some(AlertEvent::Cleared(55.0)));
    assert!(!d.alerting());
}

#[test]
fn monitors_report_their_alert_state() {
    let g = synthetic();
    let plan = vec![
        g.point_at(0, 0.0),
        g.point_at(0, 1000.0),
        g.point_at(1, 1000.0),
    ];
    let mut route = RouteMonitor::new(&g, &plan, MatchParams::default(), ROUTE_HYSTERESIS);
    let mut corridor = CorridorMonitor::new(&plan, skimap_core::matching::CORRIDOR_HYSTERESIS);
    assert!(!route.alerting() && !corridor.alerting());
    assert_eq!(route.matcher().params(), &MatchParams::default());
    // Ten minutes 300 m south of the plan, off any trail.
    let off = |t: f64| Fix {
        t_s: t,
        pos: LonLat::new(
            g.point_at(0, 500.0).lon,
            g.point_at(0, 500.0).lat - 300.0 / M_LAT,
        ),
    };
    for i in 0..600 {
        route.push(off(f64::from(i)));
        corridor.push(off(f64::from(i)));
    }
    assert!(route.alerting() && corridor.alerting());
    assert!(route.matcher().breaks() <= 600);
}
