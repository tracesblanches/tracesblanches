//! Runs the station validation (P7) and prints the results.
//!
//! ```sh
//! cargo run --release -p skimap-snow --example snow_validate [first_season last_season] [site_id...]
//! ```
//!
//! Seasons are start years: `2020 2025` = winters 2020-2021 to 2025-2026 (default).

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
    let mut args: Vec<String> = std::env::args().skip(1).collect();
    let (first, last) = match (
        args.first().and_then(|a| a.parse::<i32>().ok()),
        args.get(1).and_then(|a| a.parse::<i32>().ok()),
    ) {
        (Some(a), Some(b)) => {
            args.drain(..2);
            (a, b)
        }
        _ => (2020, 2025),
    };
    let sites: Vec<_> = if args.is_empty() {
        SITES.iter().collect()
    } else {
        args.iter().map(|a| site(a)).collect::<Result<_, _>>()?
    };
    for s in sites {
        let v = skimap_snow::validate::validate(s, &layout, &dl, first, last)?;
        println!("{}", serde_json::to_string_pretty(&v)?);
    }
    Ok(())
}
