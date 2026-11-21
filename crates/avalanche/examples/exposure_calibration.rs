//! Calibration of the infrequent runout against the known avalanches (#31, #142), model
//! alone (no CLPA fusion), personal use only:
//!
//! ```sh
//! SKIMAP_PERSONAL_SOURCES=1 cargo run --release -p skimap-avalanche --example exposure_calibration -- <site>...
//! ```
//!
//! The CLPA (DDT 05) and the EPA paths (INRAE WFS, `siavalanches:epa_site_geom`) are read
//! only as test truth: nothing is written, no layer is built. For every site and every
//! variant of the infrequent runout forest parameters (the typical scenario is unchanged)
//! it prints: the share of the CLPA area classed « peu exposé » (target 0), the nordic
//! piste length inside the CLPA classed « peu exposé », the EPA path length classed
//! « peu exposé », what the classes still leave « peu exposé » (share of the valid cells,
//! nordic network of the commune) and the stage timings, as one JSON line per variant.

use std::time::Instant;

use skimap_avalanche::exposure::{self, class, flowpy, segments, validation};
use skimap_avalanche::historic::{self, pistes::Network};
use skimap_common::config::Config;
use skimap_common::crs;
use skimap_common::download::{Downloader, Request};
use skimap_common::geom::LonLat;
use skimap_common::site;

const WFS: &str = "https://carto-service.inrae.fr/geoserver/siavalanches/ows";

/// Parts of one EPA path, Lambert-93.
type EpaPath = Vec<Vec<(f64, f64)>>;

/// EPA path polylines of a site, Lambert-93.
fn epa_paths(s: &site::Site, dl: &Downloader) -> Result<Vec<EpaPath>, Box<dyn std::error::Error>> {
    let b = s.bbox;
    let url = format!(
        "{WFS}?service=WFS&version=1.0.0&request=GetFeature&typeName=siavalanches%3Aepa_site_geom\
         &outputFormat=application%2Fjson&bbox={},{},{},{},EPSG:4326",
        b.west, b.south, b.east, b.north
    );
    let f = dl.fetch(&Request::get("inrae-carto", &url))?;
    let j: serde_json::Value = serde_json::from_slice(&f.bytes()?)?;
    let pts = |l: &serde_json::Value| -> Vec<(f64, f64)> {
        l.as_array()
            .into_iter()
            .flatten()
            .filter_map(|p| Some(LonLat::new(p[0].as_f64()?, p[1].as_f64()?)))
            .filter_map(|p| crs::to_lambert93(p).ok())
            .collect()
    };
    let mut out = Vec::new();
    for ft in j["features"].as_array().into_iter().flatten() {
        let g = &ft["geometry"];
        match g["type"].as_str() {
            Some("MultiLineString") => out.push(
                g["coordinates"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .map(pts)
                    .collect(),
            ),
            Some("LineString") => out.push(vec![pts(&g["coordinates"])]),
            _ => {}
        }
    }
    Ok(out)
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let cfg = Config::from_env()?;
    if !cfg.personal_sources {
        return Err("CLPA and EPA are for personal use only: set SKIMAP_PERSONAL_SOURCES=1".into());
    }
    let layout = cfg.layout();
    let dl = Downloader::from_config(&cfg)?;
    let registry = site::Registry::load(&layout)?;
    let only: Option<String> = std::env::var("CALIB_VARIANT").ok();
    let margins: Vec<f64> = std::env::var("CALIB_MARGINS")
        .ok()
        .map(|v| v.split(',').filter_map(|m| m.parse().ok()).collect())
        .unwrap_or_else(|| vec![exposure::LOW_MARGIN_M]);
    let f = flowpy::AUTOATES_FOREST;
    let scaled = |k: f64| flowpy::ForestParams {
        max_added_friction: f.max_added_friction * k,
        min_added_friction: f.min_added_friction * k,
        ..f
    };
    let vth = |v: f64| flowpy::ForestParams {
        vel_th_friction: v,
        vel_th_detrainment: v,
        ..f
    };
    let with = |name: &'static str, forest: Option<flowpy::ForestParams>| flowpy::FlowParams {
        name,
        forest,
        ..flowpy::AUTOATES_EXTREME
    };
    let variants: Vec<(&str, flowpy::FlowParams)> = vec![
        ("autoates", flowpy::AUTOATES_EXTREME),
        ("layer", flowpy::EXTREME),
        ("friction_x0.5", with("x0.5", Some(scaled(0.5)))),
        ("friction_x0.25", with("x0.25", Some(scaled(0.25)))),
        ("friction_0", with("x0", Some(scaled(0.0)))),
        ("vth_50", with("vth50", Some(vth(50.0)))),
        ("vth_30", with("vth30", Some(vth(30.0)))),
        ("vth_20", with("vth20", Some(vth(20.0)))),
        ("vth_10", with("vth10", Some(vth(10.0)))),
        ("no_forest", with("none", None)),
    ];
    for id in std::env::args().skip(1) {
        let s = registry.get(&id)?;
        let t = Instant::now();
        let inputs = exposure::inputs_5m(s, &layout, &dl)?;
        eprintln!(
            "{id}: inputs {:.1} s, {} x {}",
            t.elapsed().as_secs_f64(),
            inputs.dem.w,
            inputs.dem.h
        );
        let body = std::fs::read(layout.layer_dir(s, "pistes")?.join("pistes.geojson"))?;
        let lines = historic::pistes::from_pistes_geojson(&body)?;
        let commune = historic::read_commune(&dl.fetch(&historic::commune_request(s))?.bytes()?)?;
        let nordic: Vec<_> = lines
            .iter()
            .filter(|l| {
                let v = l.vertex_mean();
                l.network == Network::Nordic && commune.contains((v.lon, v.lat))
            })
            .collect();
        let all: Vec<_> = lines.iter().collect();
        let clpa = validation::clpa_footprints(s, &dl)?.map(|(p, _)| p);
        let epa = epa_paths(s, &dl)?;
        let mut inside: Option<Vec<bool>> = None;
        for (name, ext) in &variants {
            if only
                .as_deref()
                .is_some_and(|o| !o.split(',').any(|v| v == *name))
            {
                continue;
            }
            let p = exposure::ModelParams {
                frequent: flowpy::FREQUENT,
                extreme: *ext,
            };
            let t = Instant::now();
            let m = exposure::model_with(&inputs, &p.frequent, &p.extreme);
            let secs = t.elapsed().as_secs_f64();
            let layer = m.layer();
            drop(m.runout_frequent);
            for margin in margins.iter().copied() {
                let ex = &exposure::exposure_classes_with_margin(
                    &layer.ates_classes,
                    &layer.reached_extreme,
                    &m.pra_extreme.binary.data,
                    margin,
                );
                let valid = ex.data.iter().filter(|v| **v != class::NODATA).count();
                let low = ex.data.iter().filter(|v| **v == class::LOW).count();
                let lx = segments::lines(&lines, ex)?;
                let nordic_by = segments::length_by_class(
                    lx.iter()
                        .filter(|l| nordic.iter().any(|n| std::ptr::eq(*n, l.line)))
                        .flat_map(|l| l.runs.iter()),
                );
                let clpa_v = match &clpa {
                    Some(polys) => {
                        let ins = inside.get_or_insert_with(|| {
                            validation::rasterize(ex, &polys.iter().collect::<Vec<_>>())
                        });
                        Some(validation::validate(&layer, ex, polys, ins, &nordic, &all)?)
                    }
                    None => None,
                };
                let (mut epa_by, mut epa_paths_low, mut epa_paths) = ([0f64; 4], 0usize, 0usize);
                for path in &epa {
                    let mut by = [0f64; 4];
                    for part in path {
                        let l = segments::length_by_class(&segments::runs(part, ex));
                        for k in 0..4 {
                            by[k] += l[k];
                        }
                    }
                    if by[1..].iter().sum::<f64>() == 0.0 {
                        continue;
                    }
                    epa_paths += 1;
                    epa_paths_low += usize::from(by[1] > 0.0);
                    for k in 0..4 {
                        epa_by[k] += by[k];
                    }
                }
                let epa_len: f64 = epa_by[1..].iter().sum();
                let j = serde_json::json!({
                    "site": id,
                    "variant": name,
                    "margin_m": margin,
                    "params": ext,
                    "clpa_low_share": clpa_v.as_ref().map(|v| v.area.low),
                    "clpa_cells": clpa_v.as_ref().map(|v| v.area.cells),
                    "clpa_nordic_low_km": clpa_v.as_ref().map(|v| v.nordic_crossings.low_km),
                    "clpa_nordic_inside_km": clpa_v.as_ref().map(|v| v.nordic_crossings.inside_km),
                    "clpa_all_low_km": clpa_v.as_ref().map(|v| v.all_crossings.low_km),
                    "epa_paths": epa_paths,
                    "epa_paths_low": epa_paths_low,
                    "epa_km": epa_len / 1000.0,
                    "epa_low_km": epa_by[1] / 1000.0,
                    "epa_low_share": if epa_len > 0.0 { Some(epa_by[1] / epa_len) } else { None },
                    "valid_cells": valid,
                    "low_share_valid": low as f64 / valid.max(1) as f64,
                    "nordic_km": nordic_by.iter().sum::<f64>() / 1000.0,
                    "nordic_low_km": nordic_by[1] / 1000.0,
                    "paths": layer.paths,
                    "visits": layer.visits,
                    "seconds": secs,
                    "timings": layer.timings,
                });
                println!("{j}");
            }
        }
    }
    Ok(())
}
