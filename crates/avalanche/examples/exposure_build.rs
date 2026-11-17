//! Builds the `avalanche-exposure` layer (P17, P18) of the given sites and prints the
//! summary: `cargo run --release -p skimap-avalanche --example exposure_build -- <site>...`
//!
//! Needs the `terrain` and `pistes` layers of the site.

use std::time::Instant;

use skimap_avalanche::exposure;
use skimap_common::config::Config;
use skimap_common::download::Downloader;
use skimap_common::site;

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
    let registry = site::Registry::load(&layout)?;
    for id in std::env::args().skip(1) {
        let s = registry.get(&id)?;
        let t = Instant::now();
        let sum = exposure::build(s, &layout, &dl)?;
        println!("{}", serde_json::to_string_pretty(&sum)?);
        println!(
            "total {id}: {:.1} s ({} threads)",
            t.elapsed().as_secs_f64(),
            rayon::current_num_threads()
        );
    }
    Ok(())
}
