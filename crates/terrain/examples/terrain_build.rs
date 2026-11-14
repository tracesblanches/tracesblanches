//! Runs the terrain layer build (P5) on the evaluation sites and prints the summaries.
//!
//! ```sh
//! cargo run --release -p skimap-terrain --example terrain_build [site_id...]
//! ```
//!
//! Data directory: `$SKIMAP_DATA_DIR` or `./data`.

use std::time::Instant;

use skimap_common::config::Config;
use skimap_common::download::Downloader;
use skimap_common::site::{SITES, site};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .with_writer(std::io::stderr)
        .init();
    let cfg = Config::from_env()?;
    let layout = cfg.layout();
    let dl = Downloader::from_config(&cfg)?;
    let args: Vec<String> = std::env::args().skip(1).collect();
    let sites: Vec<_> = if args.is_empty() {
        SITES.iter().collect()
    } else {
        args.iter().map(|a| site(a)).collect::<Result<_, _>>()?
    };
    for s in sites {
        let t0 = Instant::now();
        let b = skimap_terrain::build(s, &layout, &dl)?;
        println!("{}", serde_json::to_string_pretty(&b)?);
        println!("total {}: {:.2} s", s.id, t0.elapsed().as_secs_f64());
    }
    Ok(())
}
