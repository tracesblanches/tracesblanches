//! SRN areas of France (G5): builds `data/out/france/srn/` and prints the key figures.
//!
//! ```sh
//! cargo run --release -p skimap-srn --example srn
//! ```

use std::time::Instant;

use skimap_common::config::Config;
use skimap_common::download::Downloader;
use skimap_osm::extract::load_planet;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .with_writer(std::io::stderr)
        .init();
    let cfg = Config::from_env()?;
    let layout = cfg.layout();
    let dl = Downloader::from_config(&cfg)?;
    let t = Instant::now();
    let planet = load_planet(&dl)?;
    let t_planet = t.elapsed().as_secs_f64();
    let out = skimap_srn::build_from(&layout, &dl, &planet)?;
    println!(
        "planet {t_planet:.1} s, srn {:.1} s -> {}",
        t.elapsed().as_secs_f64() - t_planet,
        out.dir.display()
    );
    for a in &out.areas {
        println!(
            "{:<14} {:<9} {:<10} {:>6.1} km ({:>5.1} att.) {:>7.1} km² {:<28} {} [{}]{}",
            a.massif,
            a.kind.as_str(),
            a.evidence,
            a.km_routes,
            a.km_attached,
            a.area_km2,
            a.commune.name,
            a.name,
            a.providers.join(","),
            if a.stations.is_empty() {
                String::new()
            } else {
                format!(
                    " stations: {}",
                    a.stations
                        .iter()
                        .map(|s| format!("{} {:.0}%", s.name, s.area_share * 100.0))
                        .collect::<Vec<_>>()
                        .join(", ")
                )
            }
        );
    }
    let mut s = out.stats.clone();
    if let Some(o) = s.as_object_mut() {
        o.remove("hints_outside_areas");
    }
    println!("{}", serde_json::to_string_pretty(&s)?);
    Ok(())
}
