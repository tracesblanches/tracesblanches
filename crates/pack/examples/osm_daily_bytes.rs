//! Bytes downloaded from the Protomaps builds by one day of OSM basemaps (X26, #164): for
//! each site, the build in use is set when absent (`--in-use AAAAMMJJ`), then the latest
//! build (`--latest AAAAMMJJ`) is compared with it and adopted or not, as in a pack build.
//! Prints, per site, the decision, the tiles compared, the requests and bytes that reached
//! the downloader cache of `protomaps` (new entries), and the files left in that cache.
//!
//! Run on a copy of the data directory (hard links are enough: every write of the cache is
//! a temporary file then a rename):
//!
//! ```sh
//! SKIMAP_DATA_DIR=<copy> cargo run --release -p skimap-pack --features pipeline \
//!   --example osm_daily_bytes -- --in-use 20261115 --latest 20261116 <site>...
//! ```

use std::collections::BTreeMap;
use std::path::Path;
use std::time::Instant;

use skimap_common::config::Config;
use skimap_common::download::Downloader;
use skimap_common::site::Registry;
use skimap_pack::build::{OSM_MAXZOOM, osm};

/// Cache entries of a directory: metadata file name -> (fetched at, body size).
fn inventory(dir: &Path) -> BTreeMap<String, (u64, u64)> {
    let mut out = BTreeMap::new();
    for e in std::fs::read_dir(dir).into_iter().flatten().flatten() {
        let p = e.path();
        if p.extension().is_none_or(|x| x != "json") || p.ends_with("ratelimit.json") {
            continue;
        }
        let Ok(b) = std::fs::read(&p) else { continue };
        let Ok(v) = serde_json::from_slice::<serde_json::Value>(&b) else {
            continue;
        };
        out.insert(
            e.file_name().to_string_lossy().into_owned(),
            (
                v["fetched_at_unix"].as_u64().unwrap_or(0),
                v["size"].as_u64().unwrap_or(0),
            ),
        );
    }
    out
}

/// Files and bytes under a directory (recursive).
fn du(dir: &Path) -> (u64, u64) {
    let (mut n, mut b) = (0, 0);
    for e in std::fs::read_dir(dir).into_iter().flatten().flatten() {
        let p = e.path();
        if p.is_dir() {
            let (n2, b2) = du(&p);
            n += n2;
            b += b2;
        } else if let Ok(m) = p.metadata() {
            n += 1;
            b += m.len();
        }
    }
    (n, b)
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_env_filter(std::env::var("RUST_LOG").unwrap_or_else(|_| "warn".to_owned()))
        .init();
    let mut args = std::env::args().skip(1);
    let (mut in_use, mut latest, mut ids) = (None, None, Vec::new());
    while let Some(a) = args.next() {
        match a.as_str() {
            "--in-use" => in_use = args.next(),
            "--latest" => latest = args.next(),
            _ => ids.push(a),
        }
    }
    let cfg = Config::from_env()?;
    let layout = cfg.layout();
    let dl = Downloader::from_config(&cfg)?;
    let registry = Registry::load(&layout)?;
    let latest = osm::select_build(&dl, latest.as_deref())?;
    let in_use = in_use
        .map(|k| osm::select_build(&dl, Some(&k)))
        .transpose()?;
    let cache = layout.cache_dir(osm::SOURCE)?;
    let (mut total_req, mut total_bytes) = (0usize, 0u64);
    for id in &ids {
        let site = registry.get(id)?;
        if let Some(b) = &in_use {
            // `extracts/<site>-z<zoom>-<box>.current.json`, as `osm::current_extract` names it.
            let bx = site.bbox;
            let state = cache.join("extracts").join(format!(
                "{}-z{OSM_MAXZOOM}-{}_{}_{}_{}.current.json",
                site.id, bx.west, bx.south, bx.east, bx.north
            ));
            if !state.is_file() {
                skimap_common::output::write_bytes(&state, &serde_json::to_vec_pretty(b)?)?;
            }
        }
        let before = inventory(&cache);
        let t = Instant::now();
        let chosen = osm::current_extract(site, &layout, &dl, &latest, OSM_MAXZOOM)?;
        let after = inventory(&cache);
        let new: Vec<u64> = after
            .iter()
            .filter(|(k, v)| before.get(*k) != Some(v))
            .map(|(_, v)| v.1)
            .collect();
        let bytes: u64 = new.iter().sum();
        total_req += new.len();
        total_bytes += bytes;
        println!(
            "{id:<34} {:?} en usage {} ; changées {:?} ; {} requêtes, {bytes} octets ; {:.1} s",
            chosen.decision,
            chosen.build.key,
            chosen.change.map(|c| (c.changed, c.tiles)),
            new.len(),
            t.elapsed().as_secs_f64()
        );
    }
    let (files, bytes) = du(&cache);
    println!(
        "total {} sites, build {} : {total_req} requêtes, {total_bytes} octets ; cache protomaps : {files} fichiers, {bytes} octets",
        ids.len(),
        latest.key
    );
    Ok(())
}
