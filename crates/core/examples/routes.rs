//! Proof run of C1 (routing) on both sites.
//!
//! ```sh
//! cargo run --release -p skimap-core --features pipeline --example routes [site ...]
//! ```
//!
//! For each site: builds the graph with elevation profiles (`routes/piste_graph.json`),
//! routes real point pairs in both directions (shortest distance), compares the route
//! length with the OSM lengths of the edges and the elevations with refuges.info, writes
//! `routes/routes.geojson`, then times 2 000 random routes on the network.

#[path = "common/mod.rs"]
mod common;

use std::collections::BTreeMap;
use std::time::Instant;

use serde_json::{Map, Value, json};
use skimap_common::config::Config;
use skimap_common::geom::LonLat;
use skimap_common::layout::DataLayout;
use skimap_common::site::{Site, site};
use skimap_core::graph::{PisteGraph, Snap};
use skimap_core::routing::{self, Distance, Route, Router, Style, StylePolicy};

const MAX_SNAP_M: f64 = 500.0;

fn osm_lengths(dir: &std::path::Path) -> Vec<f64> {
    let s = std::fs::read_to_string(dir.join("graph_edges.geojson")).expect("graph_edges");
    let v: Value = serde_json::from_str(&s).expect("json");
    let feats = v["features"].as_array().expect("features");
    let mut out = vec![0.0; feats.len()];
    for f in feats {
        let p = &f["properties"];
        out[p["edge"].as_u64().unwrap() as usize] = p["length_m"].as_f64().unwrap();
    }
    out
}

fn named_lengths(dir: &std::path::Path) -> BTreeMap<String, f64> {
    let s = std::fs::read_to_string(dir.join("pistes.geojson")).expect("pistes");
    let v: Value = serde_json::from_str(&s).expect("json");
    let mut m = BTreeMap::new();
    for f in v["features"].as_array().expect("features") {
        let p = &f["properties"];
        if let Some(n) = p["name"].as_str() {
            *m.entry(n.to_owned()).or_insert(0.0) += p["length_m"].as_f64().unwrap_or(0.0);
        }
    }
    m
}

struct Eval {
    props: Map<String, Value>,
    line: String,
}

fn evaluate(g: &PisteGraph, r: &Route, osm: &[f64]) -> Eval {
    let osm_sum: f64 = r
        .legs
        .iter()
        .map(|l| l.length_m() / g.length_m(l.edge) * osm[l.edge])
        .sum();
    let (up, down) = r.gain_loss(g);
    let mut props = Map::new();
    props.insert(
        "osm_length_sum_m".into(),
        json!((osm_sum * 10.0).round() / 10.0),
    );
    let line = format!(
        "{:>7.2} km (OSM {:>7.2} km, Δ {:+.2} %) D+ {:>4.0} m D- {:>4.0} m",
        r.length_m() / 1000.0,
        osm_sum / 1000.0,
        (r.length_m() / osm_sum - 1.0) * 100.0,
        up,
        down,
    );
    Eval { props, line }
}

fn refuge_altitudes(dir: &std::path::Path) -> BTreeMap<String, f64> {
    let mut m = BTreeMap::new();
    let Ok(s) = std::fs::read_to_string(dir.join("refuges_info.geojson")) else {
        return m;
    };
    let v: Value = serde_json::from_str(&s).expect("json");
    for f in v["features"].as_array().into_iter().flatten() {
        let p = &f["properties"];
        if let (Some(n), Some(a)) = (p["name"].as_str(), p["alt"].as_f64()) {
            m.insert(n.to_owned(), a);
        }
    }
    m
}

/// (distance along the route, elevation) at every profile station of the route.
fn route_series(g: &PisteGraph, r: &Route) -> Vec<(f64, Option<f64>)> {
    let mut out = Vec::new();
    let mut base = 0.0;
    for l in &r.legs {
        let st = g.stations(l.edge, l.from_m, l.to_m);
        let skip = usize::from(!out.is_empty());
        for &(d, z) in &st[skip..] {
            out.push((base + (d - l.from_m).abs(), z));
        }
        base += l.length_m();
    }
    out
}

/// Climb and descent keeping one station in `k` (profile step 10 m × k).
fn gain_every(series: &[(f64, Option<f64>)], k: usize) -> (f64, f64) {
    let zs: Vec<f64> = series.iter().filter_map(|(_, z)| *z).collect();
    let mut kept: Vec<f64> = zs.iter().step_by(k).copied().collect();
    if let Some(last) = zs.last()
        && !(zs.len() - 1).is_multiple_of(k)
    {
        kept.push(*last);
    }
    kept.windows(2).fold((0.0, 0.0), |(u, d), w| {
        let dz = w[1] - w[0];
        if dz > 0.0 { (u + dz, d) } else { (u, d - dz) }
    })
}

/// Largest strongly connected component (one-way edges respected), nodes.
fn largest_scc(g: &PisteGraph) -> usize {
    let n = g.nodes().len();
    // Kosaraju, iterative: finish order on the graph, then DFS on the reverse graph.
    let mut seen = vec![false; n];
    let mut order = Vec::with_capacity(n);
    for s in 0..n {
        if seen[s] {
            continue;
        }
        seen[s] = true;
        let mut stack = vec![(s, 0usize)];
        while let Some((u, i)) = stack.pop() {
            let out = g.out_edges(u);
            if i < out.len() {
                stack.push((u, i + 1));
                let v = g.head(out[i]);
                if !seen[v] {
                    seen[v] = true;
                    stack.push((v, 0));
                }
            } else {
                order.push(u);
            }
        }
    }
    let mut rev = vec![Vec::new(); n];
    for u in 0..n {
        for &h in g.out_edges(u) {
            rev[g.head(h)].push(u);
        }
    }
    let mut comp = vec![usize::MAX; n];
    let mut best = 0;
    for (k, &s) in order.iter().rev().enumerate() {
        if comp[s] != usize::MAX {
            continue;
        }
        comp[s] = k;
        let (mut size, mut stack) = (0, vec![s]);
        while let Some(u) = stack.pop() {
            size += 1;
            for &v in &rev[u] {
                if comp[v] == usize::MAX {
                    comp[v] = k;
                    stack.push(v);
                }
            }
        }
        best = best.max(size);
    }
    best
}

fn point_feature(p: LonLat, name: &str, snap: &Snap) -> Value {
    json!({
        "type": "Feature",
        "geometry": {"type": "Point", "coordinates": [p.lon, p.lat]},
        "properties": {"kind": "endpoint", "name": name, "snap_m": (snap.dist_m * 10.0).round() / 10.0},
    })
}

fn run(site: &Site, layout: &DataLayout) -> Result<(), Box<dyn std::error::Error>> {
    println!("\n=== {} ===", site.name);
    let (g, summary) = routing::build(site, layout)?;
    println!(
        "graphe : {} nœuds, {} arêtes, {:.1} km ({:.1} km à sens unique), {} composantes ; profils {} / {} arêtes ({:.1} / {:.1} km), {} stations dont {} interpolées ; D+ total des arêtes (sens direct) {:.0} m ; lecture {:.2} s, échantillonnage {:.3} s, écriture {:.3} s, {} octets",
        summary.nodes,
        summary.edges,
        summary.length_km,
        summary.oneway_km,
        summary.components,
        summary.profiles.edges_with_profile,
        summary.edges,
        summary.profiles.length_with_m / 1000.0,
        summary.length_km,
        summary.profiles.stations,
        summary.profiles.stations_filled,
        summary.gain_forward_m,
        summary.seconds[0],
        summary.seconds[1],
        summary.seconds[2],
        summary.graph_bytes,
    );
    // Phone side: load the graph from the JSON written by the pipeline.
    let t = Instant::now();
    let json = std::fs::read_to_string(&summary.graph_path)?;
    let g2 = PisteGraph::from_json(&json)?;
    println!(
        "chargement du graphe JSON (côté téléphone) : {:.1} ms",
        t.elapsed().as_secs_f64() * 1e3
    );
    assert_eq!(g2.data(), g.data());

    let pistes_dir = layout.layer_dir(site, "pistes")?;
    let osm = osm_lengths(&pistes_dir);
    let named = named_lengths(&pistes_dir);
    let dist = Router::new(
        &g,
        Distance {
            style: Style::Classic,
            policy: StylePolicy::default(),
        },
    );

    let refuge_alt = refuge_altitudes(&layout.layer_dir(site, "refuges")?);
    let mut features = Vec::new();
    for (pa, pb) in common::pairs(site.id) {
        for (from, to) in [(pa, pb), (pb, pa)] {
            println!("\n{} → {}", from.label, to.label);
            for (model, route) in [("distance", dist.route(from.pos, to.pos, MAX_SNAP_M))] {
                let r = match route {
                    Ok(r) => r,
                    Err(e) => {
                        println!("  {model:<16} : {e}");
                        continue;
                    }
                };
                let ev = evaluate(&g, &r, &osm);
                println!(
                    "  {model:<16} : {} | rattachement {:.0} m / {:.0} m",
                    ev.line, r.start.dist_m, r.end.dist_m
                );
                let mut props = ev.props;
                props.insert("kind".into(), json!("route"));
                props.insert("from".into(), json!(from.label));
                props.insert("to".into(), json!(to.label));
                props.insert("model".into(), json!(model));
                if model == "distance" {
                    let names = r.names(&g);
                    let mut known: Vec<String> = Vec::new();
                    for n in &names {
                        if let Some(l) = named.get(n) {
                            known.push(format!("{n} ({:.1} km)", l / 1000.0));
                        }
                    }
                    println!("    pistes : {}", known.join(", "));
                    let series = route_series(&g, &r);
                    let zs: Vec<f64> = series.iter().filter_map(|(_, z)| *z).collect();
                    if !zs.is_empty() {
                        let (lo, hi) = zs
                            .iter()
                            .fold((f64::INFINITY, f64::NEG_INFINITY), |(a, b), z| {
                                (a.min(*z), b.max(*z))
                            });
                        let gains: Vec<String> = [1usize, 2, 5]
                            .iter()
                            .map(|&k| {
                                let (u, d) = gain_every(&series, k);
                                format!("{} m : {u:.0} / {d:.0} m", 10 * k)
                            })
                            .collect();
                        println!(
                            "    altitude {lo:.0} à {hi:.0} m ; D+ / D- selon le pas du profil : {}",
                            gains.join(", ")
                        );
                    }
                    for (place, snap) in [(from, r.start), (to, r.end)] {
                        if let (Some(name), Some(prof)) =
                            (place.refuge, g.edge(snap.edge).profile.as_ref())
                            && let Some(alt) = refuge_alt.get(name)
                        {
                            println!(
                                "    {name} : altitude refuges.info {alt:.0} m, MNT au point rattaché {:.0} m",
                                prof.elevation_at(snap.along_m)
                            );
                        }
                    }
                }
                features.push(r.to_feature(&g, props));
                if model == "distance" {
                    features.push(point_feature(from.pos, from.label, &r.start));
                    features.push(point_feature(to.pos, to.label, &r.end));
                }
            }
        }
    }
    let dir = layout.layer_dir(site, routing::LAYER)?;
    let out = dir.join("routes.geojson");
    std::fs::write(
        &out,
        serde_json::to_string(&json!({"type": "FeatureCollection", "features": features}))?,
    )?;
    println!("\nécrit {}", out.display());

    // Timing: 2 000 random routes between random points of the largest component.
    let (comp, k) = g.components();
    let mut size = vec![0.0; k];
    for (e, edge) in g.edges().iter().enumerate() {
        size[comp[edge.from]] += g.length_m(e);
    }
    let big = (0..k).max_by(|a, b| size[*a].total_cmp(&size[*b])).unwrap();
    let edges: Vec<usize> = (0..g.edges().len())
        .filter(|&e| comp[g.edge(e).from] == big)
        .collect();
    let mut rng = common::SplitMix(42);
    let mut pick = || {
        let e = edges[(rng.unit() * edges.len() as f64) as usize];
        let along = rng.unit() * g.length_m(e);
        let p = g.point_at(e, along);
        g.snap(p, 1.0).unwrap()
    };
    let queries: Vec<(Snap, Snap)> = (0..2000).map(|_| (pick(), pick())).collect();
    let main_nodes = (0..g.nodes().len()).filter(|&u| comp[u] == big).count();
    println!(
        "composante principale : {main_nodes} nœuds, dont {} dans la plus grande composante fortement connexe (sens uniques respectés)",
        largest_scc(&g)
    );
    for (name, f) in [(
        "distance",
        &(|s, t| dist.route_snaps(s, t).map(|r| r.length_m()))
            as &dyn Fn(Snap, Snap) -> skimap_core::error::Result<f64>,
    )] {
        let mut times = Vec::with_capacity(queries.len());
        let (mut ok, mut longest) = (0, 0.0f64);
        for &(s, t) in &queries {
            let t0 = Instant::now();
            let r = f(s, t);
            times.push(t0.elapsed().as_secs_f64() * 1e6);
            if let Ok(l) = r {
                ok += 1;
                longest = longest.max(l);
            }
        }
        times.sort_by(f64::total_cmp);
        let mean = times.iter().sum::<f64>() / times.len() as f64;
        println!(
            "routage {name} : 2000 requêtes aléatoires (composante principale, {:.1} km), {ok} trouvées (sens uniques), plus longue {:.1} km ; moyenne {mean:.1} µs, médiane {:.1} µs, p99 {:.1} µs, max {:.1} µs",
            size[big] / 1000.0,
            longest / 1000.0,
            times[times.len() / 2],
            times[times.len() * 99 / 100],
            times[times.len() - 1],
        );
    }
    let t = Instant::now();
    let mut n = 0;
    for &(s, _) in &queries {
        n += usize::from(g.snap(s.pos, 50.0).is_some());
    }
    println!(
        "rattachement au plus proche : {:.2} µs par point ({n} points)",
        t.elapsed().as_secs_f64() * 1e6 / queries.len() as f64
    );

    routing::write_meta(site, layout, &summary)?;
    Ok(())
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let cfg = Config::from_env()?;
    let layout = DataLayout::new(cfg.data_dir);
    let args: Vec<String> = std::env::args().skip(1).collect();
    let ids: Vec<&str> = if args.is_empty() {
        vec!["cervieres", "chapelle-des-bois"]
    } else {
        args.iter().map(String::as_str).collect()
    };
    for id in ids {
        run(site(id)?, &layout)?;
    }
    Ok(())
}
