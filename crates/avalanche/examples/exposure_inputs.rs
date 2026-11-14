//! Writes the model inputs of a site (terrain model and canopy cover at 5 m and 10 m)
//! into `data/out/<site>/avalanche-exposure/inputs/`, for the reference implementation:
//! `cargo run --release -p skimap-avalanche --example exposure_inputs -- <site>`.

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
    for id in std::env::args().skip(1) {
        let s = site::site(&id)?;
        let t = Instant::now();
        let i5 = exposure::inputs_5m(s, &layout, &dl)?;
        let t5 = t.elapsed().as_secs_f64();
        let i10 = exposure::resample(&i5, 2);
        let dir = layout.layer_dir(s, exposure::LAYER)?.join("inputs");
        let mut files = exposure::write_inputs(&dir, &i5)?;
        files.extend(exposure::write_inputs(&dir, &i10)?);
        let dem_valid = i5.dem.data.iter().filter(|v| v.is_finite()).count();
        let no_canopy = i5
            .dem
            .data
            .iter()
            .zip(&i5.canopy.data)
            .filter(|(d, c)| d.is_finite() && !c.is_finite())
            .count();
        let forested = i5
            .canopy
            .data
            .iter()
            .filter(|c| c.is_finite() && **c > 30.0)
            .count();
        println!(
            "{id}: {}x{} cells, DEM valid {dem_valid}, valid DEM without canopy {no_canopy}, canopy > 30 %: {:.1} % of valid DEM; inputs in {t5:.1} s",
            i5.dem.w,
            i5.dem.h,
            100.0 * forested as f64 / dem_valid as f64
        );
        for f in files {
            println!("  {}", f.display());
        }
    }
    Ok(())
}
