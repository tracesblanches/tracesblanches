//! Builds the stations layer of France (H1, H2) and prints the key figures.
//!
//! ```sh
//! SKIMAP_PERSONAL_SOURCES=1 cargo run --release -p skimap-stations --example stations
//! ```

use std::time::Instant;

use skimap_common::config::Config;
use skimap_common::download::Downloader;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .with_writer(std::io::stderr)
        .init();
    let cfg = Config::from_env()?;
    let layout = cfg.layout();
    let dl = Downloader::from_config(&cfg)?;
    let t = Instant::now();
    let out = skimap_stations::build(&layout, &dl, cfg.personal_sources)?;
    println!(
        "== stations -> {} ({:.1} s, personal_sources = {})\n{}",
        out.dir.display(),
        t.elapsed().as_secs_f64(),
        cfg.personal_sources,
        serde_json::to_string_pretty(&out.stats)?
    );
    Ok(())
}
