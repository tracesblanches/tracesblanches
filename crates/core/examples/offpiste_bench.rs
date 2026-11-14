//! Off-piste routing (C3, #35): pipeline-side preparation, proof routes, and the benchmark
//! of the dominating case (C4, #36: 10 × 10 km at 5 m, on the host and on the Pixel 8).
//!
//! ```sh
//! # Pipeline: static rasters + snow of the given snow layer directories (default: the
//! # site's data/out/<site>/snow) -> data/out/<site>/offpiste/
//! cargo run --release -p skimap-core --example offpiste_bench -- prepare <site> [snow_dir...]
//! # Proof routes on both sites -> data/out/<site>/routes/offpiste_*.geojson (asserts)
//! cargo run --release -p skimap-core --example offpiste_bench -- proofs
//! # One route
//! cargo run --release -p skimap-core --example offpiste_bench -- route <site> <lon,lat> \
//!     <lon,lat> <RFC 3339 instant> <snow file> <name>
//! # Benchmark (also on the phone): load, zone policy, A*, Dijkstra, exhaustive search
//! cargo run --release -p skimap-core --example offpiste_bench -- bench <site> <snow file> \
//!     [--window-km 10] [--iters 3] [--at <RFC 3339>] [--power]
//! ```
//!
//! Data directory: `$SKIMAP_DATA_DIR` or `./data`. Snow files are named
//! `snow_<YYYY-MM-DD>.skr` in the site's `offpiste/` directory.
//!
//! `prepare` is the pipeline function of C3 (the phone only reads its output):
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
//!   taken from `snow.meta.json` (`valid_for`).
//!
//! `--power` samples `/sys/class/power_supply/battery/{current_now,voltage_now}` (or
//! `$SKIMAP_POWER_SUPPLY`) every 100 ms: 5 s idle baseline, then the workload.
//!
//! # Pixel 8 (C4)
//!
//! ```sh
//! export ANDROID_NDK_HOME=~/Android/Sdk/ndk/28.2.13676358
//! cargo ndk -t arm64-v8a --platform 34 build --release -p skimap-core --example offpiste_bench
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

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use serde_json::{Value, json};
use skimap_common::crs::{self, Crs};
use skimap_common::geom::LonLat;
use skimap_common::grid::{GeoTransform, Grid};
use skimap_common::layout::DataLayout;
use skimap_common::site::site;
use skimap_core::offpiste::{
    self, CostParams, ELEVATION_NODATA, Heuristic, LAYER, OffpisteGrid, PISTE_GROOMED,
    PISTE_MARKED, PISTE_NONE, Router, SnowGrid, Window, ZoneClass, ZonePolicy,
};
use skimap_core::zone_eval::{self, Verdict};
use skimap_terrain::geotiff;
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
            prepare(&layout, s.id, &snow_dirs)
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
// prepare
// ---------------------------------------------------------------------------------------

fn same_grid<A, B>(a: &Grid<A>, b: &Grid<B>) -> bool {
    a.width() == b.width()
        && a.height() == b.height()
        && a.transform() == b.transform()
        && a.crs() == b.crs()
}

/// Densifies a WGS84 ring (at most 0.0005° per edge, about 40 to 55 m) and projects it to
/// Lambert-93: a long edge is straight in longitude / latitude (as for
/// `ZoneLayer::containing`), not in Lambert-93 (a 14 km edge along a parallel bulges by
/// several metres).
fn project_ring(points: &[(f64, f64)]) -> Res<Vec<(f64, f64)>> {
    let mut dense = Vec::with_capacity(points.len());
    for e in points.windows(2) {
        let ((x1, y1), (x2, y2)) = (e[0], e[1]);
        let n = (((x2 - x1).abs().max((y2 - y1).abs())) / 0.0005)
            .ceil()
            .max(1.0) as usize;
        for k in 0..n {
            let f = k as f64 / n as f64;
            dense.push((x1 + (x2 - x1) * f, y1 + (y2 - y1) * f));
        }
    }
    if let Some(last) = points.last() {
        dense.push(*last);
    }
    project(&dense)
}

fn project(points: &[(f64, f64)]) -> Res<Vec<(f64, f64)>> {
    let mut p = points.to_vec();
    crs::transform_points(Crs::Wgs84, Crs::Lambert93, &mut p)?;
    Ok(p)
}

/// Calls `set(index)` for every cell whose centre is inside the polygon (rings in
/// Lambert-93, even-odd rule).
fn fill_polygon(
    rings: &[Vec<(f64, f64)>],
    t: &GeoTransform,
    w: usize,
    h: usize,
    mut set: impl FnMut(usize),
) {
    let row_of = |y: f64| (y - t.origin_y) / t.pixel_height - 0.5;
    let col_of = |x: f64| (x - t.origin_x) / t.pixel_width - 0.5;
    let yc = |r: i64| t.origin_y + (r as f64 + 0.5) * t.pixel_height;
    let (mut ymin, mut ymax) = (f64::INFINITY, f64::NEG_INFINITY);
    for ring in rings {
        for p in ring {
            ymin = ymin.min(p.1);
            ymax = ymax.max(p.1);
        }
    }
    if !ymin.is_finite() {
        return;
    }
    let r_from = (row_of(ymax).floor() as i64).max(0);
    let r_to = (row_of(ymin).ceil() as i64).min(h as i64 - 1);
    if r_from > r_to {
        return;
    }
    let mut buckets: Vec<Vec<f64>> = vec![Vec::new(); (r_to - r_from + 1) as usize];
    for ring in rings {
        for e in ring.windows(2) {
            let ((x1, y1), (x2, y2)) = (e[0], e[1]);
            if y1 == y2 {
                continue;
            }
            let (ylo, yhi) = (y1.min(y2), y1.max(y2));
            let a = (row_of(yhi).floor() as i64).max(r_from);
            let b = (row_of(ylo).ceil() as i64).min(r_to);
            for r in a..=b {
                let y = yc(r);
                if y >= ylo && y < yhi {
                    buckets[(r - r_from) as usize].push(x1 + (y - y1) * (x2 - x1) / (y2 - y1));
                }
            }
        }
    }
    for (k, xs) in buckets.iter_mut().enumerate() {
        xs.sort_by(f64::total_cmp);
        let r = r_from as usize + k;
        for pair in xs.as_chunks::<2>().0 {
            let c0 = (col_of(pair[0]).ceil() as i64).max(0);
            let c1 = (col_of(pair[1]).floor() as i64).min(w as i64 - 1);
            for c in c0..=c1 {
                set(r * w + c as usize);
            }
        }
    }
}

/// Distance from a point to a segment.
fn segment_distance(p: (f64, f64), a: (f64, f64), b: (f64, f64)) -> f64 {
    let (vx, vy) = (b.0 - a.0, b.1 - a.1);
    let l2 = vx * vx + vy * vy;
    let u = if l2 > 0.0 {
        (((p.0 - a.0) * vx + (p.1 - a.1) * vy) / l2).clamp(0.0, 1.0)
    } else {
        0.0
    };
    ((a.0 + u * vx - p.0).powi(2) + (a.1 + u * vy - p.1).powi(2)).sqrt()
}

/// Visits every cell a segment passes through (fractional pixel coordinates).
fn traverse(p0: (f64, f64), p1: (f64, f64), mut visit: impl FnMut(i64, i64)) {
    let (mut cx, mut cy) = (p0.0.floor() as i64, p0.1.floor() as i64);
    let (ex, ey) = (p1.0.floor() as i64, p1.1.floor() as i64);
    let (dx, dy) = (p1.0 - p0.0, p1.1 - p0.1);
    let (sx, sy) = (dx.signum() as i64, dy.signum() as i64);
    let tdx = if dx != 0.0 {
        1.0 / dx.abs()
    } else {
        f64::INFINITY
    };
    let tdy = if dy != 0.0 {
        1.0 / dy.abs()
    } else {
        f64::INFINITY
    };
    let mut tmx = if dx > 0.0 {
        ((cx + 1) as f64 - p0.0) / dx
    } else if dx < 0.0 {
        (cx as f64 - p0.0) / dx
    } else {
        f64::INFINITY
    };
    let mut tmy = if dy > 0.0 {
        ((cy + 1) as f64 - p0.1) / dy
    } else if dy < 0.0 {
        (cy as f64 - p0.1) / dy
    } else {
        f64::INFINITY
    };
    visit(cx, cy);
    let n = (ex - cx).abs() + (ey - cy).abs();
    for _ in 0..n {
        if tmx < tmy {
            cx += sx;
            tmx += tdx;
        } else {
            cy += sy;
            tmy += tdy;
        }
        visit(cx, cy);
    }
}

/// A piste edge: raster value and Lambert-93 polyline.
type PisteLine = (u8, Vec<(f64, f64)>);

/// Piste edges of `graph_edges.geojson`.
fn load_pistes(path: &Path) -> Res<Vec<PisteLine>> {
    let v: Value = serde_json::from_str(&std::fs::read_to_string(path)?)?;
    let mut out = Vec::new();
    for f in v["features"].as_array().ok_or("pistes: no features")? {
        let p = &f["properties"];
        let groomed = p["kind"] == "piste"
            && p["backcountry"] != true
            && p["uses"]
                .as_array()
                .is_some_and(|u| u.iter().any(|x| x == "nordic"));
        let coords: Vec<(f64, f64)> = f["geometry"]["coordinates"]
            .as_array()
            .ok_or("pistes: not a LineString")?
            .iter()
            .map(|c| {
                (
                    c[0].as_f64().unwrap_or(f64::NAN),
                    c[1].as_f64().unwrap_or(f64::NAN),
                )
            })
            .collect();
        out.push((
            if groomed { PISTE_GROOMED } else { PISTE_MARKED },
            project(&coords)?,
        ));
    }
    Ok(out)
}

struct XorShift(u64);
impl XorShift {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
}

fn containing_ids(layer: &ZoneLayer, p: LonLat) -> Vec<String> {
    let mut v: Vec<String> = layer.containing(p).map(|z| z.id().to_owned()).collect();
    v.sort();
    v
}

fn prepare(layout: &DataLayout, site_id: &str, snow_dirs: &[PathBuf]) -> Res<()> {
    let t_all = Instant::now();
    let s = site(site_id)?;
    let out = layout.layer_dir(s, LAYER)?;
    std::fs::create_dir_all(&out)?;
    let terrain = layout.layer_dir(s, "terrain")?;
    let t0 = Instant::now();
    let dem = geotiff::read_f32(&terrain.join("dem_5m.tif"))?;
    let slope = geotiff::read_f32(&terrain.join("slope_5m.tif"))?;
    if !same_grid(&dem, &slope) || dem.crs() != Crs::Lambert93 {
        return Err("dem and slope grids differ or are not Lambert-93".into());
    }
    let (w, h) = (dem.width(), dem.height());
    let n = w * h;
    let t = *dem.transform();
    let read_s = t0.elapsed().as_secs_f64();

    let t0 = Instant::now();
    let mut elevation = dem.map(|v| offpiste::elevation_raw(*v).unwrap_or(ELEVATION_NODATA));
    let mut slope_q = slope.map(|v| offpiste::slope_raw(*v));
    let mut missing = 0usize;
    for i in 0..n {
        if elevation.data()[i] == ELEVATION_NODATA || slope_q.data()[i] == offpiste::SLOPE_NODATA {
            elevation.data_mut()[i] = ELEVATION_NODATA;
            slope_q.data_mut()[i] = offpiste::SLOPE_NODATA;
            missing += 1;
        }
    }
    let steep = slope_q
        .data()
        .iter()
        .filter(|q| **q != offpiste::SLOPE_NODATA && **q >= 60)
        .count();
    let quantise_s = t0.elapsed().as_secs_f64();

    // Zones.
    let t0 = Instant::now();
    let zdir = layout.layer_dir(s, "zones")?;
    let layer = ZoneLayer::from_geojson_str(&std::fs::read_to_string(zdir.join("zones.geojson"))?)?;
    if layer.zones.len() > 128 {
        return Err(format!("{} zones, at most 128 supported", layer.zones.len()).into());
    }
    let mut mask = vec![0u128; n];
    let mut near: Vec<usize> = Vec::new();
    for (k, z) in layer.zones.iter().enumerate() {
        for poly in &z.geometry.0 {
            let mut rings = vec![project_ring(
                &poly
                    .exterior()
                    .coords()
                    .map(|c| (c.x, c.y))
                    .collect::<Vec<_>>(),
            )?];
            for hole in poly.interiors() {
                rings.push(project_ring(
                    &hole.coords().map(|c| (c.x, c.y)).collect::<Vec<_>>(),
                )?);
            }
            fill_polygon(&rings, &t, w, h, |i| mask[i] |= 1u128 << k);
            // Cells whose centre is within 1 m of an edge: decided below by
            // ZoneLayer::containing itself (boundary included), as zone_eval does.
            for ring in &rings {
                for e in ring.windows(2) {
                    let a = t.world_to_pixel(e[0].0, e[0].1);
                    let b = t.world_to_pixel(e[1].0, e[1].1);
                    traverse(a, b, |c, r| {
                        if c >= 0 && r >= 0 && (c as usize) < w && (r as usize) < h {
                            let ctr = t.cell_center(c as usize, r as usize);
                            if segment_distance(ctr, e[0], e[1]) < 1.0 {
                                near.push(r as usize * w + c as usize);
                            }
                        }
                    });
                }
            }
        }
    }
    near.sort_unstable();
    near.dedup();
    let index_of: HashMap<&str, usize> = layer
        .zones
        .iter()
        .enumerate()
        .map(|(k, z)| (z.id(), k))
        .collect();
    let mut near_changed = 0usize;
    for &i in &near {
        let (x, y) = t.cell_center(i % w, i / w);
        let p = crs::from_lambert93(x, y)?;
        let m = layer
            .containing(p)
            .fold(0u128, |m, z| m | (1u128 << index_of[z.id()]));
        if m != mask[i] {
            near_changed += 1;
            mask[i] = m;
        }
    }
    let mut class_of: HashMap<u128, u8> = HashMap::new();
    let mut masks: Vec<u128> = Vec::new();
    let mut zones_q = Grid::from_vec(w, h, t, Crs::Lambert93, vec![0u8; n])?;
    for (i, m) in mask.iter().enumerate() {
        let c = match class_of.get(m) {
            Some(c) => *c,
            None => {
                let c = u8::try_from(masks.len()).map_err(|_| "more than 256 zone classes")?;
                class_of.insert(*m, c);
                masks.push(*m);
                c
            }
        };
        zones_q.data_mut()[i] = c;
    }
    let ids_of = |m: u128| -> Vec<String> {
        let mut v: Vec<String> = (0..layer.zones.len())
            .filter(|k| m & (1u128 << k) != 0)
            .map(|k| layer.zones[k].id().to_owned())
            .collect();
        v.sort();
        v
    };
    let mut counts = vec![0u64; masks.len()];
    // Candidate representative cells: interior cells (8 neighbours of the same class)
    // first, then any cell.
    let mut interior: Vec<Vec<usize>> = vec![Vec::new(); masks.len()];
    let mut any: Vec<Vec<usize>> = vec![Vec::new(); masks.len()];
    for r in 0..h {
        for c in 0..w {
            let i = r * w + c;
            let k = usize::from(zones_q.data()[i]);
            counts[k] += 1;
            if any[k].len() < 64 {
                any[k].push(i);
            }
            if interior[k].len() < 64 && r > 0 && c > 0 && r + 1 < h && c + 1 < w {
                let same = [
                    i - w - 1,
                    i - w,
                    i - w + 1,
                    i - 1,
                    i + 1,
                    i + w - 1,
                    i + w,
                    i + w + 1,
                ]
                .iter()
                .all(|j| zones_q.data()[*j] == zones_q.data()[i]);
                if same && (counts[k] % 97 == 1 || interior[k].is_empty()) {
                    interior[k].push(i);
                }
            }
        }
    }
    let mut classes = Vec::with_capacity(masks.len());
    for (k, m) in masks.iter().enumerate() {
        let ids = ids_of(*m);
        let mut rep = None;
        for &i in interior[k].iter().chain(any[k].iter()) {
            let p =
                crs::from_lambert93(t.cell_center(i % w, i / w).0, t.cell_center(i % w, i / w).1)?;
            if containing_ids(&layer, p) == ids {
                rep = Some(p);
                break;
            }
        }
        if rep.is_none() {
            for &i in interior[k].iter().chain(any[k].iter()).take(5) {
                let (x, y) = t.cell_center(i % w, i / w);
                let p = crs::from_lambert93(x, y)?;
                eprintln!(
                    "class {k} ({} cells) cell {} {}: {:?} vs {:?}",
                    counts[k],
                    i % w,
                    i / w,
                    p,
                    containing_ids(&layer, p)
                );
            }
        }
        let rep = rep.ok_or_else(|| format!("zone class {k} {ids:?}: no representative cell"))?;
        classes.push(ZoneClass {
            zone_ids: ids,
            representative: rep,
            cells: counts[k],
        });
    }
    // Random check of the rasterisation against ZoneLayer::containing.
    let mut rng = XorShift(0x9E37_79B9_7F4A_7C15);
    let sample = 20_000usize;
    let mut mismatches = Vec::new();
    for _ in 0..sample {
        let i = (rng.next() % n as u64) as usize;
        let (x, y) = t.cell_center(i % w, i / w);
        let p = crs::from_lambert93(x, y)?;
        let expect = &classes[usize::from(zones_q.data()[i])].zone_ids;
        let got = containing_ids(&layer, p);
        if &got != expect {
            mismatches.push(json!({"cell": [i % w, i / w], "lon": p.lon, "lat": p.lat, "raster": expect, "containing": got}));
        }
    }
    let zones_s = t0.elapsed().as_secs_f64();
    drop(mask);

    // Pistes.
    let t0 = Instant::now();
    let edges = load_pistes(&layout.layer_dir(s, "pistes")?.join("graph_edges.geojson"))?;
    let mut pistes = Grid::from_vec(w, h, t, Crs::Lambert93, vec![PISTE_NONE; n])?;
    for (v, line) in &edges {
        for seg in line.windows(2) {
            let a = t.world_to_pixel(seg[0].0, seg[0].1);
            let b = t.world_to_pixel(seg[1].0, seg[1].1);
            traverse(a, b, |c, r| {
                if c >= 0 && r >= 0 && (c as usize) < w && (r as usize) < h {
                    let cell = &mut pistes.data_mut()[r as usize * w + c as usize];
                    *cell = (*cell).max(*v);
                }
            });
        }
    }
    let piste_counts =
        [PISTE_MARKED, PISTE_GROOMED].map(|v| pistes.data().iter().filter(|x| **x == v).count());
    let pistes_s = t0.elapsed().as_secs_f64();

    let t0 = Instant::now();
    let grid = OffpisteGrid::new(elevation, slope_q, zones_q, pistes, classes)?;
    let files = grid.write(&out)?;
    let write_s = t0.elapsed().as_secs_f64();

    // Snow layers.
    let mut snow_files = Vec::new();
    for dir in snow_dirs {
        let meta: Value =
            serde_json::from_str(&std::fs::read_to_string(dir.join("snow.meta.json"))?)?;
        let valid = meta["valid_for"]
            .as_str()
            .ok_or("snow.meta.json: no valid_for")?;
        let date = valid.get(0..10).ok_or("snow.meta.json: bad valid_for")?;
        let srn = geotiff::read_f32(&dir.join("class_srn.tif"))?;
        let groomed = geotiff::read_f32(&dir.join("class_groomed.tif"))?;
        if !same_grid(&srn, &groomed) || srn.crs() != Crs::Lambert93 {
            return Err("snow class grids differ".into());
        }
        let cls = |v: f32| v.is_finite().then_some(v as u8);
        let packed: Vec<u8> = srn
            .data()
            .iter()
            .zip(groomed.data())
            .map(|(a, b)| SnowGrid::pack(cls(*a), cls(*b)))
            .collect();
        let snow = SnowGrid {
            grid: Grid::from_vec(
                srn.width(),
                srn.height(),
                *srn.transform(),
                Crs::Lambert93,
                packed,
            )?,
        };
        let name = format!("snow_{date}.skr");
        let bytes = snow.write(&out.join(&name))?;
        let mut k = [0usize; 4];
        for v in snow.grid.data() {
            k[usize::from(SnowGrid::srn(*v).min(3))] += 1;
        }
        snow_files.push(json!({
            "name": name, "bytes": bytes, "source_dir": dir.display().to_string(),
            "valid_for": valid, "size_cells": [srn.width(), srn.height()], "cell_size": srn.transform().pixel_width,
            "srn_cells": {"insufficient": k[0], "limit": k[1], "sufficient": k[2], "unknown": k[3]},
        }));
    }

    let meta = json!({
        "layer": LAYER,
        "site": s.id,
        "tickets": ["C3 (#35)", "C4 (#36)"],
        "generated_at": now_rfc3339(),
        "valid_for": "relief permanent ; zones et pistes à la date de leurs couches ; neige au jour de chaque fichier snow_<date>.skr",
        "crs": "EPSG:2154 (Lambert-93), grille 5 m de skimap-terrain ; neige : grille 25 m de skimap-snow",
        "format": "SKR1 : en-tête de 72 octets little-endian (magic SKR1, type 1 = u8 / 2 = u16, 3 octets réservés, largeur u32, hauteur u32, EPSG u32, valeur absente u32, géotransformation GDAL 6 × f64), puis les cellules ligne par ligne (skimap_core::offpiste::encode_raster)",
        "sources": [
            {"layer": "terrain", "files": ["dem_5m.tif", "slope_5m.tif"], "licence": "Licence Ouverte 2.0 (IGN, MNT LiDAR HD)", "attribution": "IGN – MNT LiDAR HD"},
            {"layer": "zones", "files": ["zones.geojson"], "note": "sources et licences : zones/zones.meta.json"},
            {"layer": "pistes", "files": ["graph_edges.geojson"], "licence": "ODbL 1.0", "attribution": "© les contributeurs d'OpenStreetMap (ODbL), extrait OpenSnowMap.org"},
            {"layer": "snow", "files": ["class_srn.tif", "class_groomed.tif"], "licence": "Licence Ouverte 2.0 (Météo-France, simulation nivologique)", "note": "voir snow/snow.meta.json"},
        ],
        "files": files.iter().map(|(name, bytes)| json!({"name": name, "bytes": bytes})).collect::<Vec<_>>(),
        "file_descriptions": {
            offpiste::FILE_ELEVATION: "altitude, décimètres (u16), 65535 = absente",
            offpiste::FILE_SLOPE: "pente, floor(2 × degrés) (u8), 255 = absente ; bloquée si (q + 1) / 2 > pente maximale",
            offpiste::FILE_ZONES: "classe de zones (u8), indice dans zone_classes.tsv",
            offpiste::FILE_ZONE_CLASSES: "classe, lon, lat du point représentatif, nombre de cellules, identifiants des zones",
            offpiste::FILE_PISTES: "0 hors réseau, 1 itinéraire balisé non damé, 2 piste nordique damée",
            "snow_<date>.skr": "u8 : classe SRN (4 bits bas) | classe piste damée (4 bits hauts) ; 0 insuffisant, 1 limite, 2 suffisant, 15 inconnu",
        },
        "snow": snow_files,
        "grid": {"size_cells": [w, h], "cell_size": t.pixel_width, "origin": [t.origin_x, t.origin_y],
                 "cells_without_terrain": missing, "share_without_terrain": missing as f64 / n as f64,
                 "cells_slope_ge_30": steep, "share_slope_ge_30_of_valid": steep as f64 / (n - missing).max(1) as f64},
        "zones": {"zones": layer.zones.len(), "classes": grid.classes.len(),
                  "boundary_cells_decided_by_containing": near.len(), "boundary_cells_changed": near_changed,
                  "check_sample_cells": sample, "check_mismatches": mismatches.len(), "mismatch_examples": mismatches.iter().take(10).collect::<Vec<_>>()},
        "pistes": {"edges": edges.len(), "cells_marked": piste_counts[0], "cells_groomed": piste_counts[1]},
        "timings_s": {"read_geotiff": read_s, "quantise": quantise_s, "zones": zones_s, "pistes": pistes_s, "write": write_s, "total": t_all.elapsed().as_secs_f64()},
        "peak_rss_kib": proc_status("VmHWM:"),
        "notes": [
            "Les rasters gardent les entrées du coût (altitude, pente, classes de zones, pistes, neige) : la pente maximale, les poids et la date sont des paramètres de la requête sur le téléphone.",
            "Itinéraires balisés = graphe des pistes OSM (ski de fond, raquettes, randonnée) : approximation des itinéraires autorisés des arrêtés (annexe 5 de l'APPB Haut-Jura), non vérifiée.",
            "Sans couche forêt ni exposition aux avalanches pour l'instant (crochet Overlay prévu).",
        ],
    });
    std::fs::write(
        out.join("offpiste.meta.json"),
        serde_json::to_string_pretty(&meta)?,
    )?;
    println!("{}", serde_json::to_string_pretty(&meta)?);
    if !mismatches.is_empty() {
        eprintln!(
            "warning: {} of {sample} sampled cells disagree with ZoneLayer::containing",
            mismatches.len()
        );
    }
    Ok(())
}

// ---------------------------------------------------------------------------------------
// routes and proofs
// ---------------------------------------------------------------------------------------

struct SiteCtx {
    id: &'static str,
    grid: OffpisteGrid,
    layer: ZoneLayer,
    rules: RuleSet,
    slope_f32: Grid<f32>,
    piste_segments: Vec<((f64, f64), (f64, f64))>,
}

impl SiteCtx {
    fn load(layout: &DataLayout, id: &str) -> Res<Self> {
        let s = site(id)?;
        let grid = OffpisteGrid::load(&layout.layer_dir(s, LAYER)?)?;
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
        overlays: &[],
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
        overlays: &[],
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
