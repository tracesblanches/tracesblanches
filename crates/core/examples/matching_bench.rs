//! Proof run of C2 (map matching, deviation detection) on synthetic noisy traces
//! generated from real routes of both sites.
//!
//! ```sh
//! cargo run --release -p skimap-core --features pipeline --example matching_bench [site ...]
//! ```
//!
//! Routes: the proof pairs of `routes` in both directions plus random routes of at least
//! 5 km in the main component. Truth: positions every `dt` seconds at the speed of the
//! C5 classic model on each piece. Noise scenarios: first-order Gauss–Markov error per
//! axis (σ 5, 10, 15 m) and forest multipath episodes (30 m offsets lasting 2–10 s, one
//! every 50 s on average). Measures: matched edge and position accuracy (online filter
//! and Viterbi), false alerts per hour on traces that follow the plan (route and
//! corridor modes), detection delay on traces that take a wrong turn or leave the
//! corridor, time per GPS fix.

#[path = "common/mod.rs"]
mod common;

use std::time::Instant;

use serde_json::{Value, json};
use skimap_common::config::Config;
use skimap_common::geom::LonLat;
use skimap_common::layout::DataLayout;
use skimap_common::site::{Site, site};
use skimap_core::eta::EtaModel;
use skimap_core::graph::{LocalPlane, PisteGraph};
use skimap_core::matching::synth::{Noise, Rng, Truth, add_noise, sample_polyline};
use skimap_core::matching::{
    AlertEvent, CORRIDOR_HYSTERESIS, CorridorMonitor, MatchParams, MatchState, PolylineIndex,
    ROUTE_HYSTERESIS, RouteMonitor,
};
use skimap_core::routing::{self, Distance, Route, Router, Style, StylePolicy};

struct Scenario {
    name: &'static str,
    noise: Noise,
    dt_s: f64,
}

fn scenarios() -> Vec<Scenario> {
    let gm = |sigma, tau| Noise {
        sigma_m: sigma,
        tau_s: tau,
        outlier_rate_hz: 0.0,
        outlier_m: 0.0,
        outlier_len_s: (0.0, 0.0),
    };
    let forest = |sigma, tau| Noise {
        outlier_rate_hz: 0.02,
        outlier_m: 30.0,
        outlier_len_s: (2.0, 10.0),
        ..gm(sigma, tau)
    };
    vec![
        Scenario {
            name: "σ 5 m",
            noise: gm(5.0, 10.0),
            dt_s: 1.0,
        },
        Scenario {
            name: "σ 10 m",
            noise: gm(10.0, 20.0),
            dt_s: 1.0,
        },
        Scenario {
            name: "σ 15 m",
            noise: gm(15.0, 30.0),
            dt_s: 1.0,
        },
        Scenario {
            name: "σ 10 m + forêt 30 m",
            noise: forest(10.0, 20.0),
            dt_s: 1.0,
        },
        Scenario {
            name: "σ 15 m + forêt 30 m",
            noise: forest(15.0, 30.0),
            dt_s: 1.0,
        },
        Scenario {
            name: "σ 15 m + forêt 30 m, 1 fix / 5 s",
            noise: forest(15.0, 30.0),
            dt_s: 5.0,
        },
    ]
}

/// Edge and distance along it of a point at `along` metres along a route.
fn locate(g: &PisteGraph, r: &Route, along: f64) -> (usize, f64) {
    let mut base = 0.0;
    for l in &r.legs {
        let len = l.length_m();
        if along <= base + len || std::ptr::eq(l, r.legs.last().unwrap()) {
            let t = (along - base).clamp(0.0, len);
            let on = if l.to_m >= l.from_m {
                l.from_m + t
            } else {
                l.from_m - t
            };
            return (l.edge, on.clamp(0.0, g.length_m(l.edge)));
        }
        base += len;
    }
    unreachable!("route has legs")
}

/// Classic-model speed along a route, per piece.
fn speed_profile(g: &PisteGraph, r: &Route) -> impl Fn(f64) -> f64 {
    let m = EtaModel::new(Style::Classic);
    let mut ends = Vec::new();
    let mut speeds = Vec::new();
    let mut acc = 0.0;
    for p in r.pieces(g) {
        acc += p.dist_m;
        ends.push(acc);
        let t = m.piece_time_s(p.attrs, p.dist_m, p.dz_m, p.elev_m);
        speeds.push(p.dist_m / t);
    }
    move |d: f64| {
        let i = ends.partition_point(|&e| e < d).min(speeds.len() - 1);
        speeds[i]
    }
}

#[derive(Default)]
struct Acc {
    points: usize,
    on_edge_online: usize,
    on_edge_viterbi: usize,
    pos10_online: usize,
    pos10_viterbi: usize,
    off_online: usize,
    err_raw: Vec<f64>,
    err_online: Vec<f64>,
    err_viterbi: Vec<f64>,
    hours: f64,
    route_alerts: usize,
    corridor_alerts: usize,
    breaks: usize,
    push_us: Vec<f64>,
    loop_s: f64,
}

fn pct(v: &mut [f64], q: f64) -> f64 {
    if v.is_empty() {
        return f64::NAN;
    }
    v.sort_by(f64::total_cmp);
    v[((v.len() - 1) as f64 * q).round() as usize]
}

/// Wrong turn: plan prefix up to a junction past a third of the route, then a greedy walk
/// away from the plan for 1.5 km.
fn wrong_turn(g: &PisteGraph, r: &Route, plan: &PolylineIndex) -> Option<(Vec<LonLat>, f64)> {
    let total = r.length_m();
    let mut base = 0.0;
    for (k, l) in r.legs.iter().enumerate() {
        base += l.length_m();
        if base < total / 3.0 || k + 1 >= r.legs.len() {
            continue;
        }
        let e = g.edge(l.edge);
        let j = if l.to_m >= l.from_m { e.to } else { e.from };
        let next = r.legs[k + 1].edge;
        let options: Vec<_> = g
            .incident_edges(j)
            .iter()
            .filter(|h| h.edge != l.edge && h.edge != next)
            .copied()
            .collect();
        if options.is_empty() {
            continue;
        }
        let mut pts = Vec::new();
        for leg in &r.legs[..=k] {
            let p = g.sub_polyline(leg.edge, leg.from_m, leg.to_m);
            let skip = usize::from(pts.last() == p.first());
            pts.extend_from_slice(&p[skip..]);
        }
        let mut walked = 0.0;
        let (mut node, mut came) = (j, l.edge);
        let mut first = true;
        while walked < 1500.0 {
            let cand: Vec<_> = g
                .incident_edges(node)
                .iter()
                .filter(|h| h.edge != came && (!first || h.edge != next))
                .copied()
                .collect();
            let Some(h) = cand.into_iter().max_by(|a, b| {
                let da = plan.distance_m(g.nodes()[g.head(*a)].pos);
                let db = plan.distance_m(g.nodes()[g.head(*b)].pos);
                da.total_cmp(&db)
            }) else {
                break;
            };
            first = false;
            let len = g.length_m(h.edge);
            let p = if h.forward {
                g.sub_polyline(h.edge, 0.0, len)
            } else {
                g.sub_polyline(h.edge, len, 0.0)
            };
            pts.extend_from_slice(&p[1..]);
            walked += len;
            came = h.edge;
            node = g.head(h);
        }
        return Some((pts, base));
    }
    None
}

fn run(site: &Site, layout: &DataLayout) -> Result<(), Box<dyn std::error::Error>> {
    println!("\n=== {} ===", site.name);
    let (g, summary) = routing::build(site, layout)?;
    let router = Router::new(
        &g,
        Distance {
            style: Style::Classic,
            policy: StylePolicy::default(),
        },
    );
    // Routes: proof pairs (both directions) and random routes ≥ 5 km.
    let mut routes: Vec<(String, Route)> = Vec::new();
    for (a, b) in common::pairs(site.id) {
        for (x, y) in [(a, b), (b, a)] {
            if let Ok(r) = router.route(x.pos, y.pos, 500.0) {
                routes.push((format!("{} → {}", x.label, y.label), r));
            }
        }
    }
    let mut rng = Rng(2026);
    let (comp, k) = g.components();
    let mut size = vec![0.0; k];
    for (e, edge) in g.edges().iter().enumerate() {
        size[comp[edge.from]] += g.length_m(e);
    }
    let big = (0..k).max_by(|a, b| size[*a].total_cmp(&size[*b])).unwrap();
    let edges: Vec<usize> = (0..g.edges().len())
        .filter(|&e| comp[g.edge(e).from] == big)
        .collect();
    let mut tries = 0;
    while routes.len() < common::pairs(site.id).len() * 2 + 6 && tries < 1000 {
        tries += 1;
        let mut pick = || {
            let e = edges[(rng.unit() * edges.len() as f64) as usize];
            g.snap(g.point_at(e, rng.unit() * g.length_m(e)), 1.0)
                .unwrap()
        };
        let (s, t) = (pick(), pick());
        if let Ok(r) = router.route_snaps(s, t)
            && r.length_m() >= 5000.0
        {
            routes.push((format!("aléatoire {}", routes.len()), r));
        }
    }
    let km: f64 = routes.iter().map(|(_, r)| r.length_m()).sum::<f64>() / 1000.0;
    println!("{} itinéraires, {km:.1} km au total", routes.len());

    // Keep the whole trace for the Viterbi decoding (the phone keeps the default hour).
    let params = MatchParams {
        history: usize::MAX,
        ..MatchParams::default()
    };
    let mut example: Option<Value> = None;
    let mut all_push = Vec::new();
    for sc in scenarios() {
        let mut acc = Acc::default();
        let mut seed = Rng(7 + sc.dt_s as u64);
        for (ri, (_, r)) in routes.iter().enumerate() {
            let plan = r.polyline(&g);
            let speed = speed_profile(&g, r);
            let truth: Vec<Truth> = sample_polyline(&plan, sc.dt_s, &speed);
            let fixes = add_noise(&truth, &sc.noise, &mut seed);
            let mut mon = RouteMonitor::new(&g, &plan, params, ROUTE_HYSTERESIS);
            let mut cor = CorridorMonitor::new(&plan, CORRIDOR_HYSTERESIS);
            let mut online = Vec::with_capacity(fixes.len());
            let mut alerts = Vec::new();
            for f in &fixes {
                let out = mon.push(*f);
                online.push(out.state);
                if let Some(AlertEvent::Raised(t)) = out.event {
                    acc.route_alerts += 1;
                    alerts.push((t, f.pos));
                }
                if let Some(AlertEvent::Raised(_)) = cor.push(*f).event {
                    acc.corridor_alerts += 1;
                }
            }
            // Timing in the phone configuration (default history of one hour).
            let mut phone = RouteMonitor::new(&g, &plan, MatchParams::default(), ROUTE_HYSTERESIS);
            let loop_start = Instant::now();
            for f in &fixes {
                let t0 = Instant::now();
                std::hint::black_box(phone.push(*f));
                acc.push_us.push(t0.elapsed().as_secs_f64() * 1e6);
            }
            acc.loop_s += loop_start.elapsed().as_secs_f64();
            let viterbi = mon.matcher().decode();
            assert_eq!(viterbi.len(), fixes.len());
            acc.breaks += mon.matcher().breaks();
            acc.hours += truth.last().unwrap().t_s / 3600.0;
            for ((tr, f), (on, vi)) in truth.iter().zip(&fixes).zip(online.iter().zip(&viterbi)) {
                acc.points += 1;
                let (edge, _) = locate(&g, r, tr.along_m);
                acc.err_raw.push(f.pos.haversine_m(tr.pos));
                let eval = |s: &MatchState, ok: &mut usize, p10: &mut usize, err: &mut Vec<f64>| {
                    let pos = match s {
                        MatchState::OnTrail(c) => {
                            if c.edge == edge {
                                *ok += 1;
                            }
                            c.pos
                        }
                        MatchState::OffTrail => f.pos,
                    };
                    let e = pos.haversine_m(tr.pos);
                    if e <= 10.0 {
                        *p10 += 1;
                    }
                    err.push(e);
                };
                eval(
                    on,
                    &mut acc.on_edge_online,
                    &mut acc.pos10_online,
                    &mut acc.err_online,
                );
                eval(
                    vi,
                    &mut acc.on_edge_viterbi,
                    &mut acc.pos10_viterbi,
                    &mut acc.err_viterbi,
                );
                if *on == MatchState::OffTrail {
                    acc.off_online += 1;
                }
            }
            if ri == 0 && sc.name == "σ 15 m + forêt 30 m" {
                let ll = |p: &LonLat| [p.lon, p.lat];
                let mut feats = vec![
                    json!({"type": "Feature", "properties": {"kind": "plan", "noise": sc.name},
                        "geometry": {"type": "LineString", "coordinates": plan.iter().map(ll).collect::<Vec<_>>()}}),
                    json!({"type": "Feature", "properties": {"kind": "truth", "noise": sc.name},
                        "geometry": {"type": "LineString", "coordinates": truth.iter().map(|t| ll(&t.pos)).collect::<Vec<_>>()}}),
                ];
                for (f, (on, vi)) in fixes.iter().zip(online.iter().zip(&viterbi)) {
                    feats.push(
                        json!({"type": "Feature", "properties": {"kind": "gps", "t_s": f.t_s},
                        "geometry": {"type": "Point", "coordinates": ll(&f.pos)}}),
                    );
                    for (kind, s) in [("online", on), ("viterbi", vi)] {
                        if let MatchState::OnTrail(c) = s {
                            feats.push(json!({"type": "Feature", "properties": {"kind": kind, "t_s": f.t_s, "edge": c.edge},
                                "geometry": {"type": "Point", "coordinates": ll(&c.pos)}}));
                        }
                    }
                }
                for (t, p) in &alerts {
                    feats.push(
                        json!({"type": "Feature", "properties": {"kind": "alert", "t_s": t},
                        "geometry": {"type": "Point", "coordinates": ll(p)}}),
                    );
                }
                example = Some(json!({"type": "FeatureCollection", "features": feats}));
            }
        }
        let n = acc.points as f64;
        let mut us = acc.push_us.clone();
        let mean_us = us.iter().sum::<f64>() / us.len() as f64;
        println!(
            "\n[{}] {} fixes, {:.1} h ; arête juste : en ligne {:.1} %, Viterbi {:.1} % ; à ≤ 10 m : en ligne {:.1} %, Viterbi {:.1} % ; hors réseau (en ligne) {:.2} % ; ruptures HMM {}",
            sc.name,
            acc.points,
            acc.hours,
            100.0 * acc.on_edge_online as f64 / n,
            100.0 * acc.on_edge_viterbi as f64 / n,
            100.0 * acc.pos10_online as f64 / n,
            100.0 * acc.pos10_viterbi as f64 / n,
            100.0 * acc.off_online as f64 / n,
            acc.breaks,
        );
        println!(
            "  erreur de position médiane / p95 : GPS {:.1} / {:.1} m, en ligne {:.1} / {:.1} m, Viterbi {:.1} / {:.1} m",
            pct(&mut acc.err_raw, 0.5),
            pct(&mut acc.err_raw, 0.95),
            pct(&mut acc.err_online, 0.5),
            pct(&mut acc.err_online, 0.95),
            pct(&mut acc.err_viterbi, 0.5),
            pct(&mut acc.err_viterbi, 0.95),
        );
        println!(
            "  fausses alertes (trace sur le plan) : itinéraire {} ({:.2} / h), couloir {} ({:.2} / h)",
            acc.route_alerts,
            acc.route_alerts as f64 / acc.hours,
            acc.corridor_alerts,
            acc.corridor_alerts as f64 / acc.hours,
        );
        println!(
            "  temps par fix (rattachement + suivi) : moyenne {mean_us:.1} µs, p99 {:.1} µs, max {:.1} µs ; boucle complète (horloge comprise) {:.1} µs / fix",
            pct(&mut us, 0.99),
            pct(&mut us, 1.0),
            acc.loop_s * 1e6 / n,
        );
        all_push.extend_from_slice(&acc.push_us);

        // Deviations: wrong turn on the network, exit of the corridor.
        let (mut detected, mut missed, mut undetectable, mut early) = (0, 0, 0, 0);
        let mut delays_s = Vec::new();
        let mut delays_m = Vec::new();
        let (mut c_det, mut c_miss) = (0, 0);
        let mut c_delays = Vec::new();
        for (_, r) in &routes {
            let plan = r.polyline(&g);
            let idx = PolylineIndex::new(&plan);
            if let Some((pts, split)) = wrong_turn(&g, r, &idx) {
                let truth = sample_polyline(&pts, sc.dt_s, |_| 3.5);
                let fixes = add_noise(&truth, &sc.noise, &mut seed);
                let t_turn = truth.iter().find(|p| p.along_m >= split).unwrap().t_s;
                let max_away = truth
                    .iter()
                    .filter(|p| p.along_m > split)
                    .map(|p| idx.distance_m(p.pos))
                    .fold(0.0, f64::max);
                let mut mon = RouteMonitor::new(&g, &plan, params, ROUTE_HYSTERESIS);
                let mut first = None;
                for f in &fixes {
                    if let Some(AlertEvent::Raised(t)) = mon.push(*f).event
                        && first.is_none()
                    {
                        first = Some(t);
                    }
                }
                if max_away < 60.0 {
                    undetectable += 1;
                } else {
                    match first {
                        Some(t) if t < t_turn => early += 1,
                        Some(t) => {
                            detected += 1;
                            delays_s.push(t - t_turn);
                            let at = truth.iter().find(|p| p.t_s >= t).unwrap();
                            delays_m.push(at.along_m - split);
                        }
                        None => missed += 1,
                    }
                }
            }
            // Corridor: follow the plan for half its length, then 600 m perpendicular.
            let half = r.length_m() / 2.0;
            let mut pts: Vec<LonLat> = Vec::new();
            let mut acc_d = 0.0;
            for w in plan.windows(2) {
                pts.push(w[0]);
                acc_d += w[0].haversine_m(w[1]);
                if acc_d >= half {
                    pts.push(w[1]);
                    let plane = LocalPlane::new(w[1]);
                    let [dx, dy] = plane.xy(w[0]);
                    let n = dx.hypot(dy).max(1e-9);
                    // Left normal of the direction of travel (w0 → w1 is (-dx, -dy)).
                    let (nx, ny) = (dy / n, -dx / n);
                    for k in 1..=60 {
                        let s = 10.0 * k as f64;
                        pts.push(plane.lonlat([nx * s, ny * s]));
                    }
                    break;
                }
            }
            let truth = sample_polyline(&pts, sc.dt_s, |_| 3.5);
            let fixes = add_noise(&truth, &sc.noise, &mut seed);
            let t_out = truth
                .iter()
                .find(|p| idx.distance_m(p.pos) > CORRIDOR_HYSTERESIS.enter_m)
                .map(|p| p.t_s);
            let mut cm = CorridorMonitor::new(&plan, CORRIDOR_HYSTERESIS);
            let mut first = None;
            for f in &fixes {
                if let Some(AlertEvent::Raised(t)) = cm.push(*f).event
                    && first.is_none()
                {
                    first = Some(t);
                }
            }
            match (t_out, first) {
                (Some(to), Some(t)) => {
                    c_det += 1;
                    c_delays.push(t - to);
                }
                _ => c_miss += 1,
            }
        }
        println!(
            "  mauvais embranchement : {detected} détectés, {missed} manqués, {early} alertes avant l'embranchement, {undetectable} non détectables (piste parallèle à moins de 60 m) ; délai médian / max {:.0} / {:.0} s, {:.0} / {:.0} m après l'embranchement",
            pct(&mut delays_s, 0.5),
            pct(&mut delays_s, 1.0),
            pct(&mut delays_m, 0.5),
            pct(&mut delays_m, 1.0),
        );
        println!(
            "  sortie du couloir (50 m) : {c_det} détectées, {c_miss} manquées ; délai après la sortie réelle médian / min / max {:.0} / {:.0} / {:.0} s",
            pct(&mut c_delays, 0.5),
            pct(&mut c_delays, 0.0),
            pct(&mut c_delays, 1.0),
        );
    }

    // Threshold and delay against false alerts: sweep on the harshest 1 Hz scenario.
    let harsh = scenarios()
        .into_iter()
        .find(|s| s.name == "σ 15 m + forêt 30 m")
        .unwrap();
    println!(
        "\nbalayage seuil / délai (mode itinéraire, {}) :",
        harsh.name
    );
    for (enter_m, enter_s) in [
        (20.0, 10.0),
        (30.0, 10.0),
        (30.0, 15.0),
        (30.0, 20.0),
        (40.0, 15.0),
        (40.0, 20.0),
        (50.0, 20.0),
    ] {
        let hyst = skimap_core::matching::Hysteresis {
            enter_m,
            enter_s,
            ..ROUTE_HYSTERESIS
        };
        let mut seed = Rng(99);
        let (mut fa, mut hours) = (0usize, 0.0);
        let (mut det, mut miss, mut early) = (0, 0, 0);
        let mut delays = Vec::new();
        for (_, r) in &routes {
            let plan = r.polyline(&g);
            let idx = PolylineIndex::new(&plan);
            let truth = sample_polyline(&plan, 1.0, speed_profile(&g, r));
            let fixes = add_noise(&truth, &harsh.noise, &mut seed);
            let mut mon = RouteMonitor::new(&g, &plan, params, hyst);
            for f in &fixes {
                if let Some(AlertEvent::Raised(_)) = mon.push(*f).event {
                    fa += 1;
                }
            }
            hours += truth.last().unwrap().t_s / 3600.0;
            if let Some((pts, split)) = wrong_turn(&g, r, &idx) {
                let truth = sample_polyline(&pts, 1.0, |_| 3.5);
                let max_away = truth
                    .iter()
                    .filter(|p| p.along_m > split)
                    .map(|p| idx.distance_m(p.pos))
                    .fold(0.0, f64::max);
                if max_away < 60.0 {
                    continue;
                }
                let t_turn = truth.iter().find(|p| p.along_m >= split).unwrap().t_s;
                let fixes = add_noise(&truth, &harsh.noise, &mut seed);
                let mut mon = RouteMonitor::new(&g, &plan, params, hyst);
                let first = fixes
                    .iter()
                    .filter_map(|f| match mon.push(*f).event {
                        Some(AlertEvent::Raised(t)) => Some(t),
                        _ => None,
                    })
                    .next();
                match first {
                    Some(t) if t < t_turn => early += 1,
                    Some(t) => {
                        det += 1;
                        delays.push(t - t_turn);
                    }
                    None => miss += 1,
                }
            }
        }
        println!(
            "  {enter_m:.0} m pendant {enter_s:.0} s : {fa} fausses alertes en {hours:.1} h ({:.2} / h) ; embranchement {det} détectés, {miss} manqués, {early} alertes avant ; délai médian / max {:.0} / {:.0} s",
            fa as f64 / hours,
            pct(&mut delays, 0.5),
            pct(&mut delays, 1.0),
        );
    }
    let mut all = all_push;
    println!(
        "\ntemps par fix, tous scénarios : {} fixes, moyenne {:.1} µs, médiane {:.1} µs, p99 {:.1} µs, max {:.1} µs",
        all.len(),
        all.iter().sum::<f64>() / all.len() as f64,
        pct(&mut all, 0.5),
        pct(&mut all, 0.99),
        pct(&mut all, 1.0),
    );
    if let Some(ex) = example {
        let out = layout
            .layer_dir(site, routing::LAYER)?
            .join("matching_example.geojson");
        std::fs::write(&out, serde_json::to_string(&ex)?)?;
        println!("écrit {}", out.display());
    }
    routing::write_meta(site, layout, &summary)?;
    Ok(())
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let cfg = Config::from_env()?;
    let layout = DataLayout::new(cfg.data_dir);
    let args: Vec<String> = std::env::args().skip(1).collect();
    let ids: Vec<&str> = if args.is_empty() {
        skimap_common::site::SITES.iter().map(|s| s.id).collect()
    } else {
        args.iter().map(String::as_str).collect()
    };
    for id in ids {
        run(site(id)?, &layout)?;
    }
    Ok(())
}
