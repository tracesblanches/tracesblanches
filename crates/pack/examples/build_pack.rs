//! Builds the pack of one or both sites into `data/out/<site>/pack/` (P14).
//!
//! ```sh
//! cargo run --release -p skimap-pack --features pipeline --example build_pack -- [site...]
//! ```
//!
//! Environment: `SKIMAP_PERSONAL_SOURCES=1`
//! (include personal-use-only layers: trail status, hourly weather),
//! `SKIMAP_PLAN_IGN_MAXZOOM`, `SKIMAP_PROTOMAPS_BUILD` (`AAAAMMJJ`),
//! `SKIMAP_PACK_IGN_SLOPES=1` (IGN slope tiles for comparison),
//! `SKIMAP_PACK_SKIP=plan-ign,osm,voice` (development).

use std::time::Instant;

use skimap_common::config::Config;
use skimap_common::download::Downloader;
use skimap_common::site::{self, SITES};
use skimap_pack::Pack;
use skimap_pack::build::{BuildOptions, build};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_env_filter(std::env::var("RUST_LOG").unwrap_or_else(|_| "warn".to_owned()))
        .init();
    let ids: Vec<String> = std::env::args().skip(1).collect();
    let sites: Vec<_> = if ids.is_empty() {
        SITES.iter().collect()
    } else {
        ids.iter()
            .map(|i| site::site(i))
            .collect::<Result<_, _>>()?
    };
    let cfg = Config::from_env()?;
    let layout = cfg.layout();
    let dl = Downloader::from_config(&cfg)?;
    let mut opts = BuildOptions {
        personal_sources: cfg.personal_sources,
        protomaps_build: std::env::var("SKIMAP_PROTOMAPS_BUILD").ok(),
        ..BuildOptions::default()
    };
    if let Ok(z) = std::env::var("SKIMAP_PLAN_IGN_MAXZOOM") {
        opts.plan_ign_maxzoom = z.parse()?;
    }
    // Development: SKIMAP_PACK_SKIP=plan-ign,osm,voice
    let skip = std::env::var("SKIMAP_PACK_SKIP").unwrap_or_default();
    let skip: Vec<&str> = skip.split(',').map(str::trim).collect();
    opts.plan_ign &= !skip.contains(&"plan-ign");
    opts.osm &= !skip.contains(&"osm");
    opts.ign_slopes = std::env::var("SKIMAP_PACK_IGN_SLOPES").is_ok_and(|v| v == "1");
    opts.voice &= !skip.contains(&"voice");
    for s in sites {
        let t = Instant::now();
        let r = build(s, &layout, &dl, &opts)?;
        println!(
            "== {} : {} ({:.1} s)",
            s.name,
            r.pack_dir.display(),
            t.elapsed().as_secs_f64()
        );
        println!(
            "pack_version {} ; total {:.2} MB",
            r.pack_version,
            r.total_bytes as f64 / 1e6
        );
        for (g, b) in &r.bytes_by_group {
            println!("  {g:<28} {:>9.3} MB", *b as f64 / 1e6);
        }
        println!("layers: {}", r.layers.join(", "));
        for m in &r.missing {
            println!("  missing {}: {}", m.id, m.reason);
        }
        for (step, secs) in &r.timings {
            println!("  time {step:<24} {secs:>7.2} s");
        }
        // Read back with the phone-side reader and check every file.
        let pack = Pack::open(&r.pack_dir)?;
        let problems = pack.verify()?;
        println!(
            "reader: {} files, {} layers, {} basemaps, verify: {}",
            pack.manifest().files.len(),
            pack.manifest().layers.len(),
            pack.manifest().basemaps.len(),
            if problems.is_empty() {
                "OK".to_owned()
            } else {
                problems.join("; ")
            }
        );
    }
    Ok(())
}
