//! Checks that the département discovery (`jurisdiction::departments_in_bbox`, which asks
//! only the grid points outside the communes already found) gives the same départements
//! as asking every point of the 11 × 11 grid, on every site of the registry whose 121
//! point answers are in the cache. Offline: no request.
//!
//! ```sh
//! SKIMAP_DATA_DIR=data/public cargo run --release -p skimap-zones --features pipeline \
//!   --example departments_check
//! ```

use std::collections::BTreeSet;

use skimap_common::config::Config;
use skimap_common::download::{CachePolicy, Downloader, Request};
use skimap_common::site::Registry;
use skimap_zones::sources::jurisdiction::{API, departments_in_bbox};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let cfg = Config::from_env()?;
    let layout = cfg.layout();
    let dl = Downloader::builder(layout.cache_root())
        .offline(true)
        .build()?;
    let reg = Registry::load(&layout)?;
    let (mut same, mut differ, mut skipped, mut asked) = (0, 0, 0, 0usize);
    'site: for s in reg.all() {
        let b = s.bbox;
        let mut all = BTreeSet::new();
        for i in 0..11 {
            for j in 0..11 {
                let lat = b.south + (b.north - b.south) * i as f64 / 10.0;
                let lon = b.west + (b.east - b.west) * j as f64 / 10.0;
                let url = format!(
                    "{API}/communes?lat={lat:.5}&lon={lon:.5}&fields=code,codeDepartement&format=json"
                );
                let Ok(f) = dl.fetch(&Request::get("geo-api", &url).cache(CachePolicy::Offline))
                else {
                    skipped += 1;
                    continue 'site;
                };
                let v: serde_json::Value = f.json()?;
                for c in v.as_array().into_iter().flatten() {
                    if let Some(d) = c["codeDepartement"].as_str() {
                        all.insert(d.to_owned());
                    }
                }
            }
        }
        let new = departments_in_bbox(&dl, b)?;
        if new == all {
            same += 1;
        } else {
            differ += 1;
            println!(
                "{} : grille complète {all:?}, nouvelle méthode {new:?}",
                s.id
            );
        }
        asked += all.len();
    }
    println!(
        "{same} sites identiques, {differ} différents, {skipped} sans les 121 réponses en cache ({asked} départements au total)"
    );
    Ok(())
}
