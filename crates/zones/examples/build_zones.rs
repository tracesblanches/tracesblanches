//! Builds the zones layer of the given sites (all sites by default).
//!
//! ```sh
//! cargo run --release -p skimap-zones --features pipeline --example build_zones -- <site>
//! ```
//!
//! Same as `skimap zones <site>` (several sites at once).

use skimap_common::config::Config;
use skimap_common::download::Downloader;
use skimap_common::site::{SITES, site};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber_init();
    let cfg = Config::from_env()?;
    let dl = Downloader::from_config(&cfg)?;
    let layout = cfg.layout();
    let args: Vec<String> = std::env::args().skip(1).collect();
    let sites: Vec<_> = if args.is_empty() {
        SITES.iter().collect()
    } else {
        args.iter().map(|a| site(a)).collect::<Result<_, _>>()?
    };
    for s in sites {
        let r = skimap_zones::build(s, &layout, &dl)?;
        println!("{}", serde_json::to_string_pretty(&r)?);
    }
    Ok(())
}

fn tracing_subscriber_init() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .with_writer(std::io::stderr)
        .init();
}
