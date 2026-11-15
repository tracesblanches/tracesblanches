//! Off-piste routing (C3, #35): pipeline-side preparation, proof routes, and the benchmark
//! of the dominating case (C4, #36: 10 × 10 km at 5 m, on the host and on the Pixel 8).
//!
//! ```sh
//! # Pipeline: static rasters + snow of the given snow layer directories (default: the
//! # site's data/out/<site>/snow) -> data/out/<site>/offpiste/
//! cargo run --release -p skimap-core --features pipeline --example offpiste_bench -- prepare <site> [snow_dir...]
//! # Proof routes on both sites -> data/out/<site>/routes/offpiste_*.geojson (asserts)
//! cargo run --release -p skimap-core --features pipeline --example offpiste_bench -- proofs
//! # One route
//! cargo run --release -p skimap-core --features pipeline --example offpiste_bench -- route <site> <lon,lat> \
//!     <lon,lat> <RFC 3339 instant> <snow file> <name>
//! # Benchmark (also on the phone): load, zone policy, A*, Dijkstra, exhaustive search
//! cargo run --release -p skimap-core --features pipeline --example offpiste_bench -- bench <site> <snow file> \
//!     [--window-km 10] [--iters 3] [--at <RFC 3339>] [--power]
//! ```
//!
//! Data directory: `$SKIMAP_DATA_DIR` or `./data`. Snow files are named
//! `snow_<YYYY-MM-DD>.skr` in the site's `offpiste/` directory.
//!
//! `prepare` runs `skimap_core::offpiste::prepare::prepare`, the pipeline function of C3
//! (feature `pipeline`; the phone only reads its output):
//!
//! - elevation and slope from `terrain/dem_5m.tif`, `terrain/slope_5m.tif` (Float32,
//!   Lambert-93, 5 m), quantised to decimetres (`u16`) and half degrees (`u8`, floor);
//! - zones of `zones/zones.geojson` rasterised by scanline at the cell centres (rings
//!   densified to 0.0005° then projected to Lambert-93, even-odd rule per polygon, holes
//!   included); cells whose centre lies within 1 m of a polygon edge are decided by
//!   `ZoneLayer::containing` itself (boundary included, as `zone_eval`); one class per
//!   distinct set of containing zones; each class gets a representative cell whose
//!   `ZoneLayer::containing` set is checked to be the class set, and 20 000 random cells
//!   are checked the same way (`offpiste.meta.json`);
//! - pistes of `pistes/graph_edges.geojson` rasterised by grid traversal (every cell a
//!   segment passes through): groomed nordic pistes (`kind = piste`, not `backcountry`,
//!   `uses` contains `nordic`) and other marked itineraries;
//! - snow classes of each snow layer (`class_srn.tif`, `class_groomed.tif`, 25 m), date
//!   taken from `snow.meta.json` (`valid_from`).
//!
//! `--power` samples `/sys/class/power_supply/battery/{current_now,voltage_now}` (or
//! `$SKIMAP_POWER_SUPPLY`) every 100 ms: 5 s idle baseline, then the workload.
//!
//! # Pixel 8 (C4)
//!
//! ```sh
//! export ANDROID_NDK_HOME=~/Android/Sdk/ndk/28.2.13676358
//! cargo ndk -t arm64-v8a --platform 34 build --release -p skimap-core --features pipeline --example offpiste_bench
//! adb devices
//! adb shell getprop ro.product.model
//! adb push target/aarch64-linux-android/release/examples/offpiste_bench /data/local/tmp/
//! adb shell chmod 755 /data/local/tmp/offpiste_bench
//! adb shell mkdir -p /data/local/tmp/skimap-data/out/cervieres/zones \
//!     /data/local/tmp/skimap-data/out/chapelle-des-bois/zones
//! adb push data/out/cervieres/offpiste /data/local/tmp/skimap-data/out/cervieres/
//! adb push data/out/cervieres/zones/zones.geojson data/out/cervieres/zones/rules.json \
//!     /data/local/tmp/skimap-data/out/cervieres/zones/
//! adb push data/out/chapelle-des-bois/offpiste /data/local/tmp/skimap-data/out/chapelle-des-bois/
//! adb push data/out/chapelle-des-bois/zones/zones.geojson \
//!     data/out/chapelle-des-bois/zones/rules.json /data/local/tmp/skimap-data/out/chapelle-des-bois/zones/
//! # Time and memory (USB connected), dominating case: Cervières 10 x 10 km
//! adb shell 'cd /data/local/tmp && SKIMAP_DATA_DIR=/data/local/tmp/skimap-data ./offpiste_bench \
//!     bench cervieres skimap-data/out/cervieres/offpiste/snow_2026-02-15.skr --iters 5' \
//!     > bench_pixel8_cervieres_10km.json
//! # Same on the X3 big core only (cpu 8) and on a little A510 core (cpu 0)
//! adb shell 'cd /data/local/tmp && SKIMAP_DATA_DIR=/data/local/tmp/skimap-data taskset 100 \
//!     ./offpiste_bench bench cervieres skimap-data/out/cervieres/offpiste/snow_2026-02-15.skr \
//!     --iters 5' > bench_pixel8_cervieres_10km_x3.json
//! adb shell 'cd /data/local/tmp && SKIMAP_DATA_DIR=/data/local/tmp/skimap-data taskset 1 \
//!     ./offpiste_bench bench cervieres skimap-data/out/cervieres/offpiste/snow_2026-02-15.skr \
//!     --iters 3' > bench_pixel8_cervieres_10km_a510.json
//! # Whole site (16.6 km window, 11 M cells) and Chapelle-des-Bois 10 x 10 km
//! adb shell 'cd /data/local/tmp && SKIMAP_DATA_DIR=/data/local/tmp/skimap-data ./offpiste_bench \
//!     bench cervieres skimap-data/out/cervieres/offpiste/snow_2026-02-15.skr --iters 3 \
//!     --window-km 20' > bench_pixel8_cervieres_full.json
//! adb shell 'cd /data/local/tmp && SKIMAP_DATA_DIR=/data/local/tmp/skimap-data ./offpiste_bench \
//!     bench chapelle-des-bois skimap-data/out/chapelle-des-bois/offpiste/snow_2026-02-15.skr \
//!     --iters 5' > bench_pixel8_chapelle_10km.json
//! # Energy: the phone must run on battery. Wireless debugging (Developer options >
//! # Wireless debugging > Pair device with pairing code), then unplug the USB cable.
//! adb pair <ip>:<pairing port> <code>
//! adb connect <ip>:<port>
//! adb shell dumpsys battery | grep -E 'powered|status|level'   # all "powered: false"
//! adb shell cat /sys/class/power_supply/battery/current_now /sys/class/power_supply/battery/voltage_now
//! adb shell 'cd /data/local/tmp && SKIMAP_DATA_DIR=/data/local/tmp/skimap-data ./offpiste_bench \
//!     bench cervieres skimap-data/out/cervieres/offpiste/snow_2026-02-15.skr --iters 30 --power' \
//!     > bench_pixel8_cervieres_10km_power.json
//! ```
//!
//! Energy of the work = (mean power of `workload` − mean power of `idle`) × duration of
//! `workload` (`energy` in the output; current in µA, voltage in µV, negative current =
//! discharging on Pixel phones). If `current_now` is not readable by the shell user, the
//! output says so and `dumpsys batterystats` (reset before, `--charged` after, uid 2000)
//! is the fallback.

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use serde_json::{Value, json};
use skimap_common::geom::LonLat;
use skimap_common::geotiff;
use skimap_common::grid::Grid;
use skimap_common::layout::DataLayout;
use skimap_common::site::site;
use skimap_core::offpiste::prepare::{load_pistes, segment_distance};
use skimap_core::offpiste::{
    self, CostParams, Heuristic, LAYER, OffpisteGrid, PISTE_GROOMED, PISTE_MARKED, PISTE_NONE,
    Router, SnowGrid, Window, ZonePolicy,
};
use skimap_core::zone_eval::{self, Verdict};
use skimap_zones::chrono::{DateTime, Utc};
use skimap_zones::layer::ZoneLayer;
use skimap_zones::rules::{RuleSet, Topic};

type Res<T> = Result<T, Box<dyn std::error::Error>>;

fn main() -> Res<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let layout =
        DataLayout::new(std::env::var("SKIMAP_DATA_DIR").unwrap_or_else(|_| "data".into()));
    match args.first().map(String::as_str) {
        Some("prepare") => {
            let s = site(args.get(1).ok_or("prepare <site> [snow_dir...]")?)?;
            let snow_dirs: Vec<PathBuf> = if args.len() > 2 {
                args[2..].iter().map(PathBuf::from).collect()
            } else {
                vec![layout.layer_dir(s, "snow")?]
            };
            let meta = offpiste::prepare::prepare(s, &layout, &snow_dirs)?;
            println!("{}", serde_json::to_string_pretty(&meta)?);
            let bad = meta.stats["zones"]["check_mismatches"]
                .as_u64()
                .unwrap_or(0);
            if bad > 0 {
                eprintln!("warning: {bad} sampled cells disagree with ZoneLayer::containing");
            }
            Ok(())
        }
        Some("proofs") => proofs(&layout),
        Some("route") => {
            if args.len() != 7 {
                return Err("route <site> <lon,lat> <lon,lat> <instant> <snow file> <name>".into());
            }
            let req = Request {
                site: site(&args[1])?.id,
                name: args[6].clone(),
                from: parse_lonlat(&args[2])?,
                to: parse_lonlat(&args[3])?,
                at: args[4].parse()?,
                snow: PathBuf::from(&args[5]),
            };
            let ctx = SiteCtx::load(&layout, req.site)?;
            let out = run_request(&layout, &ctx, &req)?;
            println!("{}", serde_json::to_string_pretty(&out.summary)?);
            Ok(())
        }
        Some("bench") => bench(&layout, &args[1..]),
        _ => Err("usage: prepare | proofs | route | bench (see the example documentation)".into()),
    }
}

fn parse_lonlat(s: &str) -> Res<LonLat> {
    let (a, b) = s.split_once(',').ok_or("expected lon,lat")?;
    Ok(LonLat::new(a.trim().parse()?, b.trim().parse()?))
}

fn now_rfc3339() -> String {
    Utc::now().to_rfc3339_opts(skimap_zones::chrono::SecondsFormat::Secs, true)
}

// ---------------------------------------------------------------------------------------
// routes and proofs
// ---------------------------------------------------------------------------------------

struct SiteCtx {
    id: &'static str,
    grid: OffpisteGrid,
    /// Default overlays: avalanche exposure when prepared (`exposure_5m.skr`).
    overlays: Vec<offpiste::Overlay>,
    layer: ZoneLayer,
    rules: RuleSet,
    slope_f32: Grid<f32>,
    piste_segments: Vec<((f64, f64), (f64, f64))>,
}

impl SiteCtx {
    fn load(layout: &DataLayout, id: &str) -> Res<Self> {
        let s = site(id)?;
        let grid = OffpisteGrid::load(&layout.layer_dir(s, LAYER)?)?;
        let overlays = default_overlays(&layout.layer_dir(s, LAYER)?)?;
        let zdir = layout.layer_dir(s, "zones")?;
        let layer =
            ZoneLayer::from_geojson_str(&std::fs::read_to_string(zdir.join("zones.geojson"))?)?;
        let rules = RuleSet::from_json_str(&std::fs::read_to_string(zdir.join("rules.json"))?)?;
        let slope_f32 = geotiff::read_f32(&layout.layer_dir(s, "terrain")?.join("slope_5m.tif"))?;
        let mut piste_segments = Vec::new();
        for (_, line) in load_pistes(&layout.layer_dir(s, "pistes")?.join("graph_edges.geojson"))? {
            for seg in line.windows(2) {
                piste_segments.push((seg[0], seg[1]));
            }
        }
        Ok(Self {
            id: s.id,
            grid,
            overlays,
            layer,
            rules,
            slope_f32,
            piste_segments,
        })
    }

    fn piste_distance(&self, p: (f64, f64)) -> f64 {
        self.piste_segments
            .iter()
            .map(|&(a, b)| segment_distance(p, a, b))
            .fold(f64::INFINITY, f64::min)
    }
}

/// The router's default overlays: the avalanche exposure of the prepared directory with
/// [`offpiste::ExposureFactors::default`] (« exposé » ×3, « très exposé » blocked).
/// `SKIMAP_OFFPISTE_EXPOSURE=off` drops it; `SKIMAP_EXPOSURE_HIGH=<factor>` lets the user
/// allow « très exposé » at that factor; `SKIMAP_EXPOSURE_EXPOSED=<factor>` changes ×3.
fn default_overlays(dir: &std::path::Path) -> Res<Vec<offpiste::Overlay>> {
    if std::env::var("SKIMAP_OFFPISTE_EXPOSURE").is_ok_and(|v| v == "off") {
        return Ok(Vec::new());
    }
    let mut f = offpiste::ExposureFactors::default();
    if let Ok(v) = std::env::var("SKIMAP_EXPOSURE_HIGH") {
        f.high = v.parse()?;
    }
    if let Ok(v) = std::env::var("SKIMAP_EXPOSURE_EXPOSED") {
        f.exposed = v.parse()?;
    }
    Ok(offpiste::load_exposure_overlay(dir, &f)?
        .into_iter()
        .collect())
}

struct Request {
    site: &'static str,
    name: String,
    from: LonLat,
    to: LonLat,
    at: DateTime<Utc>,
    snow: PathBuf,
}

struct RequestOut {
    summary: Value,
    cost_s: f64,
    /// Off-piste cells inside each zone id.
    offpiste_in_zone: BTreeMap<String, usize>,
}

fn hm(s: f64) -> String {
    let m = (s / 60.0).round() as i64;
    format!("{}h{:02}", m / 60, m % 60)
}

fn run_request(layout: &DataLayout, ctx: &SiteCtx, req: &Request) -> Res<RequestOut> {
    let params = CostParams::default();
    let snow = SnowGrid::load(&req.snow)?;
    let t0 = Instant::now();
    let policy = offpiste::zone_policy(&ctx.grid.classes, &ctx.layer, &ctx.rules, req.at);
    let policy_s = t0.elapsed().as_secs_f64();
    let router = Router {
        grid: &ctx.grid,
        snow: Some(&snow),
        overlays: &ctx.overlays,
        policy: &policy,
        params: &params,
    };
    let route = router
        .route(req.from, req.to)
        .map_err(|e| format!("{}: {e}", req.name))?;
    let g = &ctx.grid;

    // Independent checks: float slope raster, zone_eval at every cell centre and every
    // segment midpoint, distance to the piste geometry for piste cells.
    let mut evaluations = 0usize;
    let mut violations: Vec<String> = Vec::new();
    let mut offpiste_in_zone: BTreeMap<String, usize> = BTreeMap::new();
    let mut max_slope_f32 = 0f32;
    let mut piste_cells_in_restricted = 0usize;
    let mut max_piste_dist = 0f64;
    let mut snow_counts = [0usize; 4];
    let mut zones_crossed: BTreeSet<String> = BTreeSet::new();
    let mut rules_crossed: BTreeSet<String> = BTreeSet::new();
    let mut in_places: BTreeSet<String> = BTreeSet::new();
    let mut uncovered = 0usize;
    for (k, &(c, r)) in route.cells.iter().enumerate() {
        let sl = *ctx
            .slope_f32
            .get(c, r)
            .ok_or("route outside the slope raster")?;
        if !(sl.is_finite() && sl < params.max_slope_deg) {
            violations.push(format!("cell {c},{r}: slope {sl}"));
        }
        max_slope_f32 = max_slope_f32.max(sl);
        let p = route.points[k];
        let ev = zone_eval::evaluate(&ctx.layer, &ctx.rules, p, req.at);
        evaluations += 1;
        let off = route.piste[k] == PISTE_NONE;
        if ev.verdict(Topic::Access).verdict == Verdict::Forbidden {
            violations.push(format!("cell {c},{r}: access forbidden"));
        }
        let leave = ev.verdict(Topic::LeaveRoutes);
        if leave.verdict == Verdict::Forbidden {
            if off {
                violations.push(format!(
                    "cell {c},{r}: off-piste where leaving routes is forbidden {:?}",
                    leave.rules
                ));
            } else {
                piste_cells_in_restricted += 1;
                let d = ctx.piste_distance(g.center_l93(c, r));
                max_piste_dist = max_piste_dist.max(d);
                // Supercover: the piste passes through the cell, so its centre is within
                // half a diagonal (3.54 m) of the line.
                if d > g.cell_size() * std::f64::consts::FRAC_1_SQRT_2 + 1e-6 {
                    violations.push(format!("cell {c},{r}: piste cell {d:.2} m from any piste"));
                }
            }
        }
        if !ev.covered {
            uncovered += 1;
        }
        for z in &ev.zones {
            zones_crossed.insert(z.id.clone());
            if off {
                *offpiste_in_zone.entry(z.id.clone()).or_default() += 1;
            }
        }
        let pol = &policy.classes[usize::from(*g.zones.get(c, r).unwrap_or(&0))];
        rules_crossed.extend(pol.rules.iter().cloned());
        in_places.extend(pol.in_places.iter().cloned());
        if off {
            let (x, y) = g.center_l93(c, r);
            snow_counts[usize::from(SnowGrid::srn(snow.at(x, y)).min(3))] += 1;
        }
        if k > 0 {
            let a = route.points[k - 1];
            let mid = LonLat::new((a.lon + p.lon) / 2.0, (a.lat + p.lat) / 2.0);
            let ev = zone_eval::evaluate(&ctx.layer, &ctx.rules, mid, req.at);
            evaluations += 1;
            if ev.verdict(Topic::Access).verdict == Verdict::Forbidden {
                violations.push(format!("segment {k}: access forbidden at the midpoint"));
            }
            let both_off = off && route.piste[k - 1] == PISTE_NONE;
            if both_off && ev.verdict(Topic::LeaveRoutes).verdict == Verdict::Forbidden {
                violations.push(format!(
                    "segment {k}: off-piste midpoint where leaving routes is forbidden"
                ));
            }
        }
    }
    let n_off = route.piste.iter().filter(|v| **v == PISTE_NONE).count();
    // Exposure class of every route cell (overlay grid sampled at the cell centre).
    let exposure_cells: Option<BTreeMap<&str, usize>> = ctx.overlays.first().map(|o| {
        let mut m = BTreeMap::new();
        for &(c, r) in &route.cells {
            let (x, y) = ctx.grid.elevation.transform().cell_center(c, r);
            let k = match o.grid.value_at(x, y).copied() {
                Some(1) => "terrain peu exposé (modèle)",
                Some(2) => "exposé",
                Some(3) => "très exposé",
                _ => "sans donnée",
            };
            *m.entry(k).or_insert(0) += 1;
        }
        m
    });
    let n_marked = route.piste.iter().filter(|v| **v == PISTE_MARKED).count();
    let n_groomed = route.piste.iter().filter(|v| **v == PISTE_GROOMED).count();
    let snow_date = req
        .snow
        .file_stem()
        .and_then(|s| s.to_str())
        .and_then(|s| s.strip_prefix("snow_"))
        .unwrap_or("?")
        .to_owned();

    let summary = json!({
        "name": req.name,
        "site": ctx.id,
        "at": req.at.to_rfc3339(),
        "snow_date": snow_date,
        "from": [req.from.lon, req.from.lat], "to": [req.to.lon, req.to.lat],
        "from_snap_m": route.from_snap_m, "to_snap_m": route.to_snap_m,
        "length_m": route.length_m, "cost_s": route.cost_s, "cost_hm": hm(route.cost_s),
        "ascent_m": route.ascent_m, "descent_m": route.descent_m,
        "max_slope_deg_stored": route.max_slope_deg, "max_slope_deg_terrain": max_slope_f32,
        "cells": route.cells.len(), "cells_offpiste": n_off, "cells_marked": n_marked, "cells_groomed": n_groomed,
        "offpiste_srn_snow_cells": {"insufficient": snow_counts[0], "limit": snow_counts[1], "sufficient": snow_counts[2], "unknown": snow_counts[3]},
        "zones_crossed": zones_crossed, "offpiste_cells_per_zone": offpiste_in_zone,
        "restricting_rules_crossed": rules_crossed, "forbidden_in_places_rules": in_places,
        "cells_outside_covered_territory": uncovered,
        "piste_cells_where_leaving_routes_forbidden": piste_cells_in_restricted,
        "max_distance_of_those_cells_to_a_piste_m": max_piste_dist,
        "checks": {"zone_evaluations": evaluations, "violations": violations.len()},
        "search": {"window_cells": route.stats.window_cells, "passable_cells": route.stats.passable_cells,
                   "expanded": route.stats.expanded, "pushed": route.stats.pushed, "max_open": route.stats.max_open,
                   "bytes": route.stats.bytes, "zone_policy_s": policy_s, "prepare_s": route.stats.prepare_s, "search_s": route.stats.search_s},
        "params": {"max_slope_deg": params.max_slope_deg, "flat_speed_mps": params.flat_speed_mps, "climb_rate_mps": params.climb_rate_mps,
                   "downhill": params.downhill, "steep_from_deg": params.steep_from_deg, "steep_factor_at_max": params.steep_factor_at_max,
                   "snow_factor": params.snow_factor, "snow_unknown_factor": params.snow_unknown_factor,
                   "groomed_factor": params.groomed_factor, "marked_factor": params.marked_factor, "avoid_factor": params.avoid_factor},
    });
    let mut summary = summary;
    summary["exposure_cells"] = json!(exposure_cells);
    if !violations.is_empty() {
        return Err(format!(
            "{}: {} violations, first: {:?}",
            req.name,
            violations.len(),
            &violations[..violations.len().min(5)]
        )
        .into());
    }

    // GeoJSON: the whole route, then its piste / off-piste stretches, then the ends.
    let coords = |a: usize, b: usize| -> Vec<[f64; 2]> {
        route.points[a..b]
            .iter()
            .map(|p| [round7(p.lon), round7(p.lat)])
            .collect()
    };
    let mut features = vec![json!({
        "type": "Feature",
        "geometry": {"type": "LineString", "coordinates": coords(0, route.points.len())},
        "properties": {"kind": "route", "summary": summary},
    })];
    let mut a = 0;
    for k in 1..=route.cells.len() {
        if k == route.cells.len() || route.piste[k] != route.piste[a] {
            let b = (k + 1).min(route.cells.len());
            let len: f64 = (a + 1..b)
                .map(|j| {
                    let (p, q) = (
                        g.center_l93(route.cells[j - 1].0, route.cells[j - 1].1),
                        g.center_l93(route.cells[j].0, route.cells[j].1),
                    );
                    ((p.0 - q.0).powi(2) + (p.1 - q.1).powi(2)).sqrt()
                })
                .sum();
            features.push(json!({
                "type": "Feature",
                "geometry": {"type": "LineString", "coordinates": coords(a, b)},
                "properties": {"kind": "stretch", "network": match route.piste[a] { PISTE_GROOMED => "groomed", PISTE_MARKED => "marked", _ => "offpiste" }, "length_m": len},
            }));
            a = k;
        }
    }
    for (kind, p) in [("requested_from", req.from), ("requested_to", req.to)] {
        features.push(json!({"type": "Feature", "geometry": {"type": "Point", "coordinates": [p.lon, p.lat]}, "properties": {"kind": kind}}));
    }
    let fc = json!({
        "type": "FeatureCollection",
        "properties": {
            "generated_at": now_rfc3339(), "crs": "EPSG:4326",
            "description": "Itinéraire hors piste (C3, #35) : centres des cellules de 5 m du chemin optimal (A*), tronçons piste / hors piste, points demandés.",
            "attribution": "IGN – MNT LiDAR HD (Licence Ouverte 2.0) ; © les contributeurs d'OpenStreetMap (ODbL) ; Météo-France (simulation nivologique, Licence Ouverte 2.0) ; zones : voir zones.meta.json",
        },
        "features": features,
    });
    let dir = layout.layer_dir(site(ctx.id)?, "routes")?;
    std::fs::create_dir_all(&dir)?;
    let path = dir.join(format!("offpiste_{}.geojson", req.name));
    std::fs::write(&path, serde_json::to_string(&fc)?)?;
    eprintln!("wrote {}", path.display());
    Ok(RequestOut {
        summary,
        cost_s: route.cost_s,
        offpiste_in_zone,
    })
}

fn round7(v: f64) -> f64 {
    (v * 1e7).round() / 1e7
}

fn proofs(layout: &DataLayout) -> Res<()> {
    let snow = |id: &str, date: &str| -> Res<PathBuf> {
        Ok(layout
            .layer_dir(site(id)?, LAYER)?
            .join(format!("snow_{date}.skr")))
    };
    let feb: DateTime<Utc> = "2026-02-15T09:00:00Z".parse()?;
    let aug: DateTime<Utc> = "2026-08-15T09:00:00Z".parse()?;
    // Places (OpenStreetMap, Overpass): Le Laus (hamlet), Col d'Izoard (saddle, 2361 m),
    // Plaine du Bourget (1897 m), Col de Bousson (2160 m, French-Italian border).
    let cervieres = [
        Request {
            site: "cervieres",
            name: "laus_izoard".into(),
            from: LonLat::new(6.72687, 44.8552163),
            to: LonLat::new(6.734975, 44.8196595),
            at: feb,
            snow: snow("cervieres", "2026-02-15")?,
        },
        Request {
            site: "cervieres",
            name: "bourget_italie".into(),
            from: LonLat::new(6.7682745, 44.8750496),
            to: LonLat::new(6.7838257, 44.8920871),
            at: feb,
            snow: snow("cervieres", "2026-02-15")?,
        },
    ];
    let mut summaries = Vec::new();
    let ctx = SiteCtx::load(layout, "cervieres")?;
    for r in &cervieres {
        let out = run_request(layout, &ctx, r)?;
        summaries.push(out.summary);
    }
    // Chapelle-des-Bois: from the south end to the north end of the Risoux forest along the
    // Swiss border; the straight line runs through the APPB Haut-Doubs (inpn:FR3801178),
    // where leaving the marked routes is forbidden from 15/12 to 30/06. Same request and
    // same snow in February and in August.
    let appb = "inpn:FR3801178";
    let ctx = SiteCtx::load(layout, "chapelle-des-bois")?;
    let mk = |name: &str, at| Request {
        site: "chapelle-des-bois",
        name: name.into(),
        from: LonLat::new(6.138, 46.598),
        to: LonLat::new(6.185, 46.663),
        at,
        snow: PathBuf::new(),
    };
    let mut winter = mk("risoux_fevrier", feb);
    winter.snow = snow("chapelle-des-bois", "2026-02-15")?;
    let mut summer = mk("risoux_aout", aug);
    summer.snow = winter.snow.clone();
    let w = run_request(layout, &ctx, &winter)?;
    let s = run_request(layout, &ctx, &summer)?;
    let w_in = w.offpiste_in_zone.get(appb).copied().unwrap_or(0);
    let s_in = s.offpiste_in_zone.get(appb).copied().unwrap_or(0);
    assert_eq!(w_in, 0, "February route goes off-piste in the APPB");
    assert!(s_in > 0, "August route does not cut through the APPB");
    assert!(s.cost_s < w.cost_s, "August route not cheaper");
    summaries.push(w.summary);
    summaries.push(s.summary);
    let table: Vec<Value> = summaries
        .iter()
        .map(|v| {
            json!({
                "name": v["name"], "cost_hm": v["cost_hm"], "length_m": v["length_m"].as_f64().map(|x| x.round()),
                "ascent_m": v["ascent_m"].as_f64().map(|x| x.round()), "max_slope_deg_terrain": v["max_slope_deg_terrain"],
                "cells": v["cells"], "offpiste": v["cells_offpiste"], "marked": v["cells_marked"], "groomed": v["cells_groomed"],
                "offpiste_cells_in_appb": v["offpiste_cells_per_zone"][appb].as_u64().unwrap_or(0), "violations": v["checks"]["violations"],
                "expanded": v["search"]["expanded"], "search_s": v["search"]["search_s"],
            })
        })
        .collect();
    println!(
        "{}",
        serde_json::to_string_pretty(&json!({"routes": summaries, "table": table}))?
    );
    println!("APPB {appb}: off-piste cells February {w_in}, August {s_in}");
    Ok(())
}

// ---------------------------------------------------------------------------------------
// bench
// ---------------------------------------------------------------------------------------

fn proc_status(key: &str) -> Option<u64> {
    let s = std::fs::read_to_string("/proc/self/status").ok()?;
    s.lines()
        .find(|l| l.starts_with(key))
        .and_then(|l| l.split_whitespace().nth(1))
        .and_then(|v| v.parse().ok())
}

fn mem() -> Value {
    json!({"rss_kib": proc_status("VmRSS:"), "peak_rss_kib": proc_status("VmHWM:")})
}

struct PowerSampler {
    stop: Arc<AtomicBool>,
    handle: std::thread::JoinHandle<Vec<(f64, f64, f64)>>,
    t0: Instant,
}

impl PowerSampler {
    fn start(dir: PathBuf) -> Option<Self> {
        let read = |f: &str| -> Option<f64> {
            std::fs::read_to_string(dir.join(f))
                .ok()?
                .trim()
                .parse()
                .ok()
        };
        read("current_now")?;
        read("voltage_now")?;
        let stop = Arc::new(AtomicBool::new(false));
        let s2 = stop.clone();
        let t0 = Instant::now();
        let handle = std::thread::spawn(move || {
            let read = |f: &str| -> Option<f64> {
                std::fs::read_to_string(dir.join(f))
                    .ok()?
                    .trim()
                    .parse()
                    .ok()
            };
            let mut v = Vec::new();
            while !s2.load(Ordering::Relaxed) {
                if let (Some(i), Some(u)) = (read("current_now"), read("voltage_now")) {
                    v.push((t0.elapsed().as_secs_f64(), i, u));
                }
                std::thread::sleep(Duration::from_millis(100));
            }
            v
        });
        Some(Self { stop, handle, t0 })
    }

    fn now(&self) -> f64 {
        self.t0.elapsed().as_secs_f64()
    }

    fn finish(self, phases: &[(&str, f64, f64)]) -> Value {
        self.stop.store(true, Ordering::Relaxed);
        let samples = self.handle.join().unwrap_or_default();
        let mut out = serde_json::Map::new();
        for (name, a, b) in phases {
            let s: Vec<_> = samples.iter().filter(|x| x.0 >= *a && x.0 <= *b).collect();
            if s.is_empty() {
                out.insert((*name).into(), json!({"samples": 0}));
                continue;
            }
            let n = s.len() as f64;
            let i = s.iter().map(|x| x.1).sum::<f64>() / n;
            let u = s.iter().map(|x| x.2).sum::<f64>() / n;
            // current_now (µA) × voltage_now (µV) -> W; the sign convention depends on the
            // device (negative = discharging on Pixel phones).
            let p = s.iter().map(|x| x.1 * x.2 * 1e-12).sum::<f64>() / n;
            out.insert(
                (*name).into(),
                json!({"samples": s.len(), "duration_s": b - a, "mean_current_ua": i, "mean_voltage_uv": u, "mean_power_w": p}),
            );
        }
        // Net energy of the workload: (|P_workload| - |P_idle|) x duration.
        let pw = |k: &str| {
            out.get(k)
                .and_then(|v| v["mean_power_w"].as_f64())
                .map(f64::abs)
        };
        if let (Some(work), Some(idle), Some(d)) = (
            pw("workload"),
            pw("idle"),
            out.get("workload").and_then(|v| v["duration_s"].as_f64()),
        ) {
            out.insert("net_energy_j".into(), json!((work - idle) * d));
        }
        Value::Object(out)
    }
}

fn bench(layout: &DataLayout, args: &[String]) -> Res<()> {
    let usage = "bench <site> <snow file> [--window-km 10] [--iters 3] [--at <RFC 3339>] [--power]";
    let s = site(args.first().ok_or(usage)?)?;
    let snow_path = PathBuf::from(args.get(1).ok_or(usage)?);
    let mut window_km = 10.0f64;
    let mut iters = 3usize;
    let mut at: DateTime<Utc> = "2026-02-15T09:00:00Z".parse()?;
    let mut power = false;
    let mut k = 2;
    while k < args.len() {
        match args[k].as_str() {
            "--window-km" => {
                window_km = args.get(k + 1).ok_or(usage)?.parse()?;
                k += 1;
            }
            "--iters" => {
                iters = args.get(k + 1).ok_or(usage)?.parse()?;
                k += 1;
            }
            "--at" => {
                at = args.get(k + 1).ok_or(usage)?.parse()?;
                k += 1;
            }
            "--power" => power = true,
            a => return Err(format!("unknown argument {a}; {usage}").into()),
        }
        k += 1;
    }
    let sampler = if power {
        let dir = std::env::var("SKIMAP_POWER_SUPPLY")
            .map(PathBuf::from)
            .unwrap_or_else(|_| PathBuf::from("/sys/class/power_supply/battery"));
        let p = PowerSampler::start(dir.clone());
        if p.is_none() {
            eprintln!(
                "power: {} not readable, no energy measurement",
                dir.display()
            );
        }
        p
    } else {
        None
    };
    let mut phases = Vec::new();
    if let Some(p) = &sampler {
        let a = p.now();
        std::thread::sleep(Duration::from_secs(5));
        phases.push(("idle", a, p.now()));
    }
    let work_start = sampler.as_ref().map(PowerSampler::now);
    let mem0 = mem();

    let t0 = Instant::now();
    let grid = OffpisteGrid::load(&layout.layer_dir(s, LAYER)?)?;
    let overlays = default_overlays(&layout.layer_dir(s, LAYER)?)?;
    let load_grid_s = t0.elapsed().as_secs_f64();
    let mem_grid = mem();
    let t0 = Instant::now();
    let zdir = layout.layer_dir(s, "zones")?;
    let layer = ZoneLayer::from_geojson_str(&std::fs::read_to_string(zdir.join("zones.geojson"))?)?;
    let rules = RuleSet::from_json_str(&std::fs::read_to_string(zdir.join("rules.json"))?)?;
    let load_zones_s = t0.elapsed().as_secs_f64();
    let t0 = Instant::now();
    let snow = SnowGrid::load(&snow_path)?;
    let load_snow_s = t0.elapsed().as_secs_f64();
    let t0 = Instant::now();
    let policy: ZonePolicy = offpiste::zone_policy(&grid.classes, &layer, &rules, at);
    let policy_s = t0.elapsed().as_secs_f64();

    let params = CostParams::default();
    let router = Router {
        grid: &grid,
        snow: Some(&snow),
        overlays: &overlays,
        policy: &policy,
        params: &params,
    };
    let size = ((window_km * 1000.0 / grid.cell_size()).round() as usize)
        .min(grid.width())
        .min(grid.height());
    let win = Window {
        col0: (grid.width() - size) / 2,
        row0: (grid.height() - size) / 2,
        width: size,
        height: size,
    };
    // Ends: in the largest connected component of passable cells (8 neighbours, no corner
    // cutting, as in the search), the cells nearest to the south-west and north-east
    // corners: the longest diagonal request of the window.
    let t0 = Instant::now();
    let ok = router.passable(win);
    let (ww, wh) = (win.width, win.height);
    let mut label = vec![u32::MAX; ww * wh];
    let mut sizes: Vec<usize> = Vec::new();
    let mut stack = Vec::new();
    for seed in 0..ww * wh {
        if !ok[seed] || label[seed] != u32::MAX {
            continue;
        }
        let id = sizes.len() as u32;
        label[seed] = id;
        stack.push(seed);
        let mut size = 0usize;
        while let Some(i) = stack.pop() {
            size += 1;
            let (c, r) = ((i % ww) as i64, (i / ww) as i64);
            for (dc, dr) in [
                (1, 0),
                (-1, 0),
                (0, 1),
                (0, -1),
                (1, 1),
                (1, -1),
                (-1, 1),
                (-1, -1),
            ] {
                let (cj, rj) = (c + dc, r + dr);
                if cj < 0 || rj < 0 || cj >= ww as i64 || rj >= wh as i64 {
                    continue;
                }
                let j = rj as usize * ww + cj as usize;
                if label[j] != u32::MAX || !ok[j] {
                    continue;
                }
                if dc != 0
                    && dr != 0
                    && !(ok[r as usize * ww + cj as usize] && ok[rj as usize * ww + c as usize])
                {
                    continue;
                }
                label[j] = id;
                stack.push(j);
            }
        }
        sizes.push(size);
    }
    let main = (0..sizes.len())
        .max_by_key(|k| sizes[*k])
        .ok_or("no passable cell in the window")? as u32;
    let component = sizes[main as usize];
    let (mut best_sw, mut best_ne) = ((i64::MAX, 0usize), (i64::MAX, 0usize));
    for (i, l) in label.iter().enumerate() {
        if *l != main {
            continue;
        }
        let (c, r) = ((i % ww) as i64, (i / ww) as i64);
        best_sw = best_sw.min((c * c + (wh as i64 - 1 - r).pow(2), i));
        best_ne = best_ne.min(((ww as i64 - 1 - c).pow(2) + r * r, i));
    }
    let to_cell = |i: usize| (win.col0 + i % ww, win.row0 + i / ww);
    let (start, goal) = (to_cell(best_sw.1), to_cell(best_ne.1));
    let (d_start, d_goal) = (
        (best_sw.0 as f64).sqrt() * grid.cell_size(),
        (best_ne.0 as f64).sqrt() * grid.cell_size(),
    );
    let ends_s = t0.elapsed().as_secs_f64();
    let passable_cells = ok.iter().filter(|v| **v).count();
    let components = sizes.len();
    drop(ok);
    drop(label);

    let mut runs = Vec::new();
    let mut astar_s = Vec::new();
    for _ in 0..iters {
        let t0 = Instant::now();
        let r = router.route_cells(win, start, goal, Heuristic::AStar)?;
        let total = t0.elapsed().as_secs_f64();
        astar_s.push(total);
        runs.push(json!({"heuristic": "astar", "total_s": total, "prepare_s": r.stats.prepare_s, "search_s": r.stats.search_s,
                         "expanded": r.stats.expanded, "pushed": r.stats.pushed, "max_open": r.stats.max_open, "bytes": r.stats.bytes,
                         "length_m": r.length_m, "cost_s": r.cost_s, "cells": r.cells.len()}));
    }
    let t0 = Instant::now();
    let d = router.route_cells(win, start, goal, Heuristic::Dijkstra)?;
    let dijkstra_s = t0.elapsed().as_secs_f64();
    runs.push(json!({"heuristic": "dijkstra", "total_s": dijkstra_s, "prepare_s": d.stats.prepare_s, "search_s": d.stats.search_s,
                     "expanded": d.stats.expanded, "pushed": d.stats.pushed, "max_open": d.stats.max_open, "bytes": d.stats.bytes,
                     "length_m": d.length_m, "cost_s": d.cost_s}));
    let t0 = Instant::now();
    let e = router.explore(win, start);
    let explore_s = t0.elapsed().as_secs_f64();
    runs.push(json!({"heuristic": "exhaustive (unreachable destination)", "total_s": explore_s, "prepare_s": e.prepare_s, "search_s": e.search_s,
                     "expanded": e.expanded, "pushed": e.pushed, "max_open": e.max_open, "bytes": e.bytes, "passable_cells": e.passable_cells}));
    let work_end = sampler.as_ref().map(PowerSampler::now);
    if let (Some(a), Some(b)) = (work_start, work_end) {
        phases.push(("workload", a, b));
    }
    let energy = sampler.map(|p| p.finish(&phases));
    let mut sorted = astar_s.clone();
    sorted.sort_by(f64::total_cmp);
    let out = json!({
        "site": s.id,
        "target": std::env::consts::ARCH, "os": std::env::consts::OS,
        "at": at.to_rfc3339(), "snow": snow_path.display().to_string(),
        "grid_cells": [grid.width(), grid.height()],
        "window": {"col0": win.col0, "row0": win.row0, "cells": [win.width, win.height], "km": window_km, "total_cells": win.cells()},
        "start": {"cell": [start.0, start.1], "distance_to_corner_m": d_start}, "goal": {"cell": [goal.0, goal.1], "distance_to_corner_m": d_goal},
        "passable_cells": passable_cells, "components": components, "largest_component_cells": component, "choose_ends_s": ends_s,
        "straight_distance_m": (((start.0 as f64 - goal.0 as f64).powi(2) + (start.1 as f64 - goal.1 as f64).powi(2)).sqrt() * grid.cell_size()),
        "load_s": {"grid": load_grid_s, "zones_rules": load_zones_s, "snow": load_snow_s}, "zone_policy_s": policy_s,
        "zone_classes": grid.classes.len(),
        "runs": runs,
        "astar_median_s": sorted[sorted.len() / 2],
        "memory": {"start": mem0, "after_grid_load": mem_grid, "end": mem()},
        "energy": energy,
    });
    println!("{}", serde_json::to_string_pretty(&out)?);
    Ok(())
}
