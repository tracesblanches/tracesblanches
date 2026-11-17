//! Runs the infrequent runout of a site on a contiguous `1 / <step>` of its release cells (X25, #159), so
//! that the routing kernel can be profiled (Callgrind) on the real terrain of a site in a
//! bounded time:
//!
//! ```sh
//! cargo run --release -p skimap-avalanche --example runout_sample -- <site> <step> [<cache>]
//! ```

use std::time::Instant;

use skimap_avalanche::exposure::{self, flowpy, pra};
use skimap_common::config::Config;
use skimap_common::download::Downloader;
use skimap_common::site;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args().skip(1);
    let id = args.next().ok_or("site")?;
    let step: usize = args.next().map_or(Ok(1000), |s| s.parse())?;
    let cfg = Config::from_env()?;
    let layout = cfg.layout();
    let dl = Downloader::from_config(&cfg)?;
    let registry = site::Registry::load(&layout)?;
    let inputs = exposure::inputs_5m(registry.get(&id)?, &layout, &dl)?;
    // Release cells of the infrequent scenario, cached in `<cache>` when given (the wind
    // shelter dominates a run under Callgrind otherwise).
    let cache = args.next().map(std::path::PathBuf::from);
    let mut release = match cache.as_deref().filter(|p| p.is_file()) {
        Some(p) => inputs.dem.with_data(std::fs::read(p)?),
        None => {
            let slope = pra::gdal_slope(&inputs.dem, true);
            let ws = pra::windshelter(
                &inputs.dem,
                pra::FREQUENT.wind_radius_m,
                pra::FREQUENT.wind_probability,
            );
            let r = pra::pra(&slope, &ws, &inputs.canopy, &pra::EXTREME).release;
            if let Some(p) = &cache {
                std::fs::write(p, &r.data)?;
            }
            r
        }
    };
    // A contiguous run of release cells in the middle of the site, as a thread routes them
    // (a sparse sample would be dominated by the first-touch weight tables).
    let n = release.data.iter().filter(|v| **v > 0).count();
    let (lo, hi) = (n / 2, n / 2 + n / step);
    let mut k = 0usize;
    for v in &mut release.data {
        if *v > 0 {
            if k < lo || k >= hi {
                *v = 0;
            }
            k += 1;
        }
    }
    let fsi = exposure::forest_index(&inputs.canopy);
    let t = Instant::now();
    let r = flowpy::run(&inputs.dem, &release, &fsi, &flowpy::EXTREME);
    println!(
        "{id}: {} paths, {} visits, {:.2} s",
        r.paths,
        r.visits,
        t.elapsed().as_secs_f64()
    );
    Ok(())
}
