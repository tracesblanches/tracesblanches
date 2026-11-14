//! Runs the snow layer build (P6) and prints the summaries with timings.
//!
//! ```sh
//! cargo run --release -p skimap-snow --example snow_build [YYYY-MM-DD] [site_id...]
//! ```
//!
//! Without a date: latest date of the current-year simulation file (J-1).
//! `SKIMAP_OFFLINE=1`: cache only (times the computation without the network).
//! Data directory: `$SKIMAP_DATA_DIR` or `./data`.

use std::time::Instant;

use chrono::NaiveDate;
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
    // SKIMAP_OFFLINE=1: cache only, to time the computation without the network.
    let offline = std::env::var("SKIMAP_OFFLINE").is_ok_and(|v| v == "1");
    let dl = Downloader::builder(layout.cache_root())
        .rate_limit_overrides(cfg.rate_limits.clone())
        .offline(offline)
        .build()?;
    let mut args: Vec<String> = std::env::args().skip(1).collect();
    let date = match args
        .first()
        .map(|a| NaiveDate::parse_from_str(a, "%Y-%m-%d"))
    {
        Some(Ok(d)) => {
            args.remove(0);
            Some(d)
        }
        _ => None,
    };
    let sites: Vec<_> = if args.is_empty() {
        SITES.iter().collect()
    } else {
        args.iter().map(|a| site(a)).collect::<Result<_, _>>()?
    };
    for s in sites {
        let t0 = Instant::now();
        let b = skimap_snow::build(s, &layout, &dl, date)?;
        println!("{}", serde_json::to_string_pretty(&b)?);
        println!(
            "total {} {}: {:.3} s",
            s.id,
            b.date,
            t0.elapsed().as_secs_f64()
        );
    }
    Ok(())
}
