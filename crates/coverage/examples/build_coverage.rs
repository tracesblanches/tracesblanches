//! Builds the coverage layer of the given sites (all sites by default).
//!
//! ```sh
//! cargo run --release -p skimap-coverage --example build_coverage -- \
//!     [--archives <dir with the .7z>] [--release 2026_T2] [site...]
//! ```
//!
//! Stand-in for the `skimap` sub-command until the orchestrator wires it in `crates/cli`.

use skimap_common::config::Config;
use skimap_common::download::Downloader;
use skimap_common::site::{SITES, site};
use skimap_coverage::{CoverageOptions, Release};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .with_writer(std::io::stderr)
        .init();
    let cfg = Config::from_env()?;
    let dl = Downloader::from_config(&cfg)?;
    let layout = cfg.layout();
    let mut opts = CoverageOptions::default();
    let mut ids = Vec::new();
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        match a.as_str() {
            "--archives" => opts.archive_dir = Some(args.next().ok_or("--archives <dir>")?.into()),
            "--release" => {
                let v = args.next().ok_or("--release YYYY_TQ")?;
                let (y, q) = v.split_once("_T").ok_or("--release YYYY_TQ")?;
                opts.release = Release {
                    year: y.parse()?,
                    quarter: q.parse()?,
                };
            }
            _ => ids.push(a),
        }
    }
    let sites: Vec<_> = if ids.is_empty() {
        SITES.iter().collect()
    } else {
        ids.iter().map(|a| site(a)).collect::<Result<_, _>>()?
    };
    for s in sites {
        let r = skimap_coverage::build(s, &layout, &dl, &opts)?;
        println!("{}", serde_json::to_string_pretty(&r)?);
    }
    Ok(())
}
