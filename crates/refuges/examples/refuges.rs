//! Builds the huts and shelters layers (P11) of both sites and prints the figures compared
//! with research report 06.
//!
//! ```sh
//! cargo run --release -p skimap-refuges --example refuges
//! ```

use std::time::Instant;

use skimap_common::config::Config;
use skimap_common::download::Downloader;
use skimap_common::site::SITES;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .with_writer(std::io::stderr)
        .init();
    let cfg = Config::from_env()?;
    let layout = cfg.layout();
    let dl = Downloader::from_config(&cfg)?;
    for site in SITES {
        let t = Instant::now();
        let out = skimap_refuges::build(site, &layout, &dl)?;
        println!(
            "== {} -> {} ({:.1} s)\n{}",
            site.name,
            out.dir.display(),
            t.elapsed().as_secs_f64(),
            serde_json::to_string_pretty(&out.stats)?
        );
    }
    Ok(())
}
