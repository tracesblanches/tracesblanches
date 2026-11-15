//! Validation against the EPA avalanche paths (Enquête permanente sur les avalanches,
//! INRAE), personal use only:
//!
//! ```sh
//! SKIMAP_PERSONAL_SOURCES=1 cargo run --release -p skimap-avalanche --example epa_validation -- cervieres
//! ```
//!
//! The EPA terms (avalanches.fr, checked 15/11/2026) allow reproduction only for a
//! « usage personnel excluant tout usage à des fins publicitaires et/ou commerciales
//! et/ou d'information »: the paths are read from the public INRAE WFS
//! (`siavalanches:epa_site_geom`) through the shared downloader, only to compute these
//! statistics, and nothing is written into the layer. Without `personal_sources` the
//! example refuses to run.
//!
//! Classes: the model alone (`exposure_model_5m.tif` when present, else
//! `exposure_5m.tif`; with `--product`, `exposure_5m.tif`), sampled along each EPA path
//! with the 10 m buffer of the piste runs.

use skimap_avalanche::exposure::{self, class, raster, segments};
use skimap_common::config::Config;
use skimap_common::crs;
use skimap_common::download::{Downloader, Request};
use skimap_common::geom::LonLat;
use skimap_common::site;

const WFS: &str = "https://carto-service.inrae.fr/geoserver/siavalanches/ows";

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let s = site::site(
        &std::env::args()
            .nth(1)
            .ok_or("usage: epa_validation <site>")?,
    )?;
    let cfg = Config::from_env()?;
    if !cfg.personal_sources {
        return Err("EPA data are for personal use only: set SKIMAP_PERSONAL_SOURCES=1".into());
    }
    let layout = cfg.layout();
    let dl = Downloader::from_config(&cfg)?;
    let dir = layout.layer_dir(s, exposure::LAYER)?;
    // `--product`: the user classes (after the CLPA fusion) instead of the model alone.
    let product = std::env::args().any(|a| a == "--product");
    let names: &[&str] = if product {
        &[exposure::build::files::EXPOSURE]
    } else {
        &[
            exposure::build::files::EXPOSURE_MODEL,
            exposure::build::files::EXPOSURE,
        ]
    };
    let path = names
        .iter()
        .map(|f| dir.join(f))
        .find(|p| p.exists())
        .ok_or("build the avalanche-exposure layer first")?;
    let rr = raster::read(&path)?;
    let classes = rr.with_data(
        rr.data
            .iter()
            .map(|v| if v.is_nan() { class::NODATA } else { *v as u8 })
            .collect(),
    );
    let b = s.bbox;
    let url = format!(
        "{WFS}?service=WFS&version=1.0.0&request=GetFeature&typeName=siavalanches%3Aepa_site_geom\
         &outputFormat=application%2Fjson&bbox={},{},{},{},EPSG:4326",
        b.west, b.south, b.east, b.north
    );
    let f = dl.fetch(&Request::get("inrae-carto", &url))?;
    let j: serde_json::Value = serde_json::from_slice(&f.bytes()?)?;
    let feats = j["features"].as_array().cloned().unwrap_or_default();
    let (mut sites, mut sites_low, mut sites_all_low) = (0usize, 0usize, 0usize);
    let mut by = [0f64; 4];
    for ft in &feats {
        let g = &ft["geometry"];
        let parts: Vec<Vec<LonLat>> = match g["type"].as_str() {
            Some("MultiLineString") => g["coordinates"]
                .as_array()
                .into_iter()
                .flatten()
                .map(|l| {
                    l.as_array()
                        .into_iter()
                        .flatten()
                        .filter_map(|p| Some(LonLat::new(p[0].as_f64()?, p[1].as_f64()?)))
                        .collect()
                })
                .collect(),
            Some("LineString") => vec![
                g["coordinates"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(|p| Some(LonLat::new(p[0].as_f64()?, p[1].as_f64()?)))
                    .collect(),
            ],
            _ => continue,
        };
        let mut site_by = [0f64; 4];
        for part in parts {
            let pts: Vec<(f64, f64)> = part
                .iter()
                .filter_map(|p| crs::to_lambert93(*p).ok())
                .collect();
            let r = segments::runs(&pts, &classes);
            let l = segments::length_by_class(&r);
            for k in 0..4 {
                site_by[k] += l[k];
            }
        }
        let inside: f64 = site_by[1..].iter().sum();
        if inside == 0.0 {
            continue; // outside the grid or no data
        }
        sites += 1;
        if site_by[1] > 0.0 {
            sites_low += 1;
        }
        if site_by[2] == 0.0 && site_by[3] == 0.0 {
            sites_all_low += 1;
        }
        for k in 0..4 {
            by[k] += site_by[k];
        }
    }
    let total: f64 = by[1..].iter().sum();
    println!(
        "{}: classes {} ; EPA features {} ; paths with data {sites}",
        s.id,
        path.display(),
        feats.len()
    );
    println!(
        "length with data {:.2} km: peu exposé {:.3} km ({:.2} %), exposé {:.2} km, très exposé {:.2} km; no data {:.2} km",
        total / 1000.0,
        by[1] / 1000.0,
        100.0 * by[1] / total.max(1.0),
        by[2] / 1000.0,
        by[3] / 1000.0,
        by[0] / 1000.0
    );
    println!(
        "paths touching « peu exposé »: {sites_low} of {sites}; entirely « peu exposé »: {sites_all_low}"
    );
    Ok(())
}
