//! Downloads the Geofabrik extracts, builds (or reads) the local OSM index (X5) and prints
//! its figures and the huts of the given sites (default: the evaluation sites, test data).
//!
//! ```sh
//! RUST_LOG=info cargo run --release -p skimap-osm --example local_index [site ...]
//! ```

use std::time::Instant;

use skimap_common::config::Config;
use skimap_common::download::Downloader;
use skimap_osm::local::{self, Kind};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .with_writer(std::io::stderr)
        .init();
    let cfg = Config::from_env()?;
    let layout = cfg.layout();
    let dl = Downloader::from_config(&cfg)?;
    let t = Instant::now();
    let idx = local::load(&layout, &dl)?;
    println!(
        "index {} : {} éléments en {:.1} s ; données OSM du {}",
        idx.path.display(),
        idx.elements.len(),
        t.elapsed().as_secs_f64(),
        idx.valid_at.as_deref().unwrap_or("?")
    );
    for k in [Kind::Node, Kind::Way, Kind::Relation] {
        let n = idx.elements.iter().filter(|e| e.kind == k).count();
        let huts = idx
            .elements
            .iter()
            .filter(|e| e.kind == k && local::is_hut(|x| e.get(x)))
            .count();
        println!("  {} : {n} (abris {huts})", k.as_str());
    }
    let registry = skimap_common::site::Registry::load(&layout)?;
    let ids: Vec<String> = std::env::args().skip(1).collect();
    let sites: Vec<&skimap_common::site::Site> = if ids.is_empty() {
        skimap_common::site::SITES.iter().collect()
    } else {
        ids.iter()
            .map(|id| registry.get(id))
            .collect::<Result<_, _>>()?
    };
    for s in sites {
        let t = Instant::now();
        let v = idx.query(&[s.bbox], |e| local::is_hut(|k| e.get(k)));
        println!(
            "{} : {} abris OSM ({:.3} s)",
            s.id,
            v["elements"].as_array().map_or(0, Vec::len),
            t.elapsed().as_secs_f64()
        );
    }
    Ok(())
}
