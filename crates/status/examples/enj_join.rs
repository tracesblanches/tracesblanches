//! Measures the attachment of the ENJ itineraries to the OSM ways of the `pistes` layer
//! (X12, #146) on any site whose box crosses ENJ itineraries, whether or not its ENJ city
//! is known: `cargo run --release -p skimap-status --example enj_join -- <site>…`.
//!
//! For every itinerary crossing the site box (`trek/?in_bbox=`), the points every 10 m
//! inside the box are attached when matched to one of the itinerary's ways
//! ([`skimap_status::join::ItineraryGraphs::match_itinerary`]). The former rule
//! ([`skimap_status::join::ways_along`]: ways with 80 % of their points within 20 m) is
//! measured on the same points (attached when within 20 m of one of its ways). Prints the
//! rates per site and practice, then every gap of at least 50 m.

use std::collections::BTreeMap;

use serde_json::{Value, json};
use skimap_common::config::Config;
use skimap_common::download::Downloader;
use skimap_common::geom::LonLat;
use skimap_common::site;
use skimap_status::enj;
use skimap_status::join::{self, ItineraryGraphs, OsmWay, Practice};

/// Points of the line geometry inside the box, every 10 m.
fn points(geometry: &Value, b: skimap_common::geom::BBox) -> Vec<LonLat> {
    let lines: Vec<Value> = match geometry["type"].as_str() {
        Some("LineString") => vec![geometry["coordinates"].clone()],
        Some("MultiLineString") => geometry["coordinates"]
            .as_array()
            .cloned()
            .unwrap_or_default(),
        _ => Vec::new(),
    };
    let mut out = Vec::new();
    for l in lines {
        let pts: Vec<LonLat> = l
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|p| Some(LonLat::new(p.get(0)?.as_f64()?, p.get(1)?.as_f64()?)))
            .collect();
        for s in pts.windows(2) {
            let n = (s[0].haversine_m(s[1]) / 10.0).ceil().max(1.0) as usize;
            for k in 0..n {
                let t = k as f64 / n as f64;
                out.push(LonLat::new(
                    s[0].lon + t * (s[1].lon - s[0].lon),
                    s[0].lat + t * (s[1].lat - s[0].lat),
                ));
            }
        }
        out.extend(pts.last());
    }
    out.retain(|p| b.contains(*p));
    out
}

/// Distance (m) from a point to a way, local plane.
fn dist_to_way(p: LonLat, w: &OsmWay) -> f64 {
    let k = p.lat.to_radians().cos();
    let xy = |q: LonLat| ((q.lon - p.lon) * 111_320.0 * k, (q.lat - p.lat) * 110_540.0);
    let pts: Vec<(f64, f64)> = w
        .coords
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|c| Some(xy(LonLat::new(c.get(0)?.as_f64()?, c.get(1)?.as_f64()?))))
        .collect();
    pts.windows(2)
        .map(|s| {
            let (a, b) = (s[0], s[1]);
            let (dx, dy) = (b.0 - a.0, b.1 - a.1);
            let l2 = dx * dx + dy * dy;
            let t = if l2 == 0.0 {
                0.0
            } else {
                (-(a.0 * dx + a.1 * dy) / l2).clamp(0.0, 1.0)
            };
            (a.0 + t * dx).hypot(a.1 + t * dy)
        })
        .fold(f64::INFINITY, f64::min)
}

#[derive(Default)]
struct Tally {
    itineraries: usize,
    points: usize,
    attached: usize,
    within_20: usize,
    reachable: usize,
    old: usize,
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let cfg = Config::from_env()?;
    let dl = Downloader::from_config(&cfg)?;
    let layout = cfg.layout();
    let registry = site::Registry::load(&layout)?;
    let mut total: BTreeMap<String, Tally> = BTreeMap::new();
    let mut gaps = Vec::new();
    let mut offsets = Vec::new();
    for id in std::env::args().skip(1) {
        let site = registry.get(&id)?;
        let dir = layout.layer_dir(site, "pistes")?;
        let t0 = std::time::Instant::now();
        let graphs = ItineraryGraphs::load(site.id, &dir)?;
        let ways = join::read_pistes(&dir.join("pistes.geojson"))?;
        let treks = enj::fetch_in_bbox(&dl, site.bbox)?;
        let mut per: BTreeMap<String, Tally> = BTreeMap::new();
        let mut t_match = std::time::Duration::ZERO;
        for t in &treks {
            let pts = points(&t.geometry, site.bbox);
            if pts.is_empty() {
                continue;
            }
            let practice = join::practice_of(t.practice.as_deref().unwrap_or(""));
            let tm = std::time::Instant::now();
            let m = graphs.match_itinerary(&t.geometry, practice, Some(site.bbox));
            t_match += tm.elapsed();
            let old_ways = join::ways_along(&t.geometry, practice, &ways);
            let old = pts
                .iter()
                .filter(|p| old_ways.iter().any(|w| dist_to_way(**p, w) <= 20.0))
                .count();
            let group = match practice {
                Practice::Nordic => "ski",
                Practice::Foot => "raquettes",
                Practice::Any => "multi",
            };
            for key in [group.to_owned(), "toutes".to_owned()] {
                for tally in [
                    per.entry(key.clone()).or_default(),
                    total.entry(key).or_default(),
                ] {
                    tally.itineraries += 1;
                    tally.points += m.points;
                    tally.attached += m.attached;
                    tally.within_20 += m.attached_within_along;
                    tally.reachable += m.reachable;
                    tally.old += old;
                }
            }
            for w in m.way_matches.iter().filter(|w| w.median_offset_m > 20.0) {
                offsets.push(json!({"site": site.id, "itinerary": t.id, "name": t.name,
                    "practice": group, "way": w}));
            }
            for g in &m.gaps {
                gaps.push(json!({"site": site.id, "itinerary": t.id, "name": t.name,
                    "practice": group, "gap": g}));
            }
        }
        println!(
            "{} : {} itinéraires dans l'emprise, graphe et voies lus + appariement {:.2} s (appariement seul {:.2} s)",
            site.id,
            per.get("toutes").map_or(0, |t| t.itineraries),
            t0.elapsed().as_secs_f64(),
            t_match.as_secs_f64()
        );
        for (k, t) in &per {
            print_tally(k, t);
        }
    }
    println!("TOTAL");
    for (k, t) in &total {
        print_tally(k, t);
    }
    println!("VOIES À PLUS DE 20 M (médiane)");
    for o in &offsets {
        println!("OFFSET {o}");
    }
    println!("ÉCARTS (≥ {} m)", join::GAP_MIN_M);
    for g in &gaps {
        println!("{g}");
    }
    Ok(())
}

fn print_tally(k: &str, t: &Tally) {
    let pc = |n: usize| 100.0 * n as f64 / t.points.max(1) as f64;
    println!(
        "  {k:<10} {:>3} itin. {:>6} pts ({:>6.1} km), voie de la pratique à ≤ 50 m {:>5.1} % : HMM {:>5.1} % des points ({:>5.1} % des points atteignables ; à ≤ 20 m {:>5.1} %), règle 80 %/20 m {:>5.1} %",
        t.itineraries,
        t.points,
        t.points as f64 / 100.0,
        pc(t.reachable),
        pc(t.attached),
        100.0 * t.attached as f64 / t.reachable.max(1) as f64,
        pc(t.within_20),
        pc(t.old)
    );
}
