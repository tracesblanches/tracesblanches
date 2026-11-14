//! Sensitivity of the « peu exposé » misses inside the CLPA (P18) to the runout and
//! margin parameters, model alone (no CLPA fusion):
//! `cargo run --release -p skimap-avalanche --example exposure_sensitivity -- <site>`.
//!
//! Inputs: `data/out/<site>/avalanche-exposure/inputs/{dem,canopy}_5m.tif` and the
//! `pistes` layer. Prints one line per variant: share of the CLPA area and length of
//! nordic piste inside the CLPA classed « peu exposé », and the length of the whole
//! nordic network (commune rule) classed « peu exposé ».

use skimap_avalanche::exposure::{self, Inputs, class, flowpy, raster, segments, validation};
use skimap_avalanche::historic::{self, pistes::Network};
use skimap_common::config::Config;
use skimap_common::download::Downloader;
use skimap_common::site;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let s = site::site(
        &std::env::args()
            .nth(1)
            .ok_or("usage: exposure_sensitivity <site>")?,
    )?;
    let cfg = Config::from_env()?;
    let layout = cfg.layout();
    let dl = Downloader::from_config(&cfg)?;
    let dir = layout.layer_dir(s, exposure::LAYER)?.join("inputs");
    let inputs = Inputs {
        dem: raster::read_any(&dir.join("dem_5m.tif"))?.raster,
        canopy: raster::read_any(&dir.join("canopy_5m.tif"))?.raster,
    };
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
    let no_forest = flowpy::FlowParams {
        forest: None,
        ..flowpy::EXTREME
    };
    let a15 = flowpy::FlowParams {
        alpha: 15.0,
        ..flowpy::EXTREME
    };
    let variants: [(&str, flowpy::FlowParams); 3] = [
        ("défaut (α 18°, forêt)", flowpy::EXTREME),
        ("rare sans forêt", no_forest),
        ("rare α 15°, forêt", a15),
    ];
    for (name, ext) in variants {
        let m = exposure::model_with(&inputs, &flowpy::FREQUENT, &ext);
        let reached: Vec<bool> = (0..m.exposure.data.len())
            .map(|i| m.runout_extreme.reached(i))
            .collect();
        for margin in [10.0, 25.0, 50.0] {
            let ex = exposure::exposure_classes_with_margin(
                &m.ates.classes,
                &reached,
                &m.pra_extreme.binary.data,
                margin,
            );
            let lx = segments::lines(&lines, &ex)?;
            let nordic_low: f64 = lx
                .iter()
                .filter(|l| nordic.iter().any(|n| std::ptr::eq(*n, l.line)))
                .flat_map(|l| l.runs.iter())
                .filter(|r| r.class == class::LOW)
                .map(|r| r.length_m)
                .sum();
            let low_cells = ex.data.iter().filter(|v| **v == class::LOW).count();
            match &clpa {
                Some(p) => {
                    let v = validation::validate(&m, &ex, p, &nordic, &all)?;
                    println!(
                        "{name:<24} marge {margin:>4} m | CLPA peu exposé {:.4} (recall {:.4}), atteint rare {:.4} | pistes CLPA peu exposé {:.3} / {:.3} km | réseau nordique peu exposé {:.2} km | cellules peu exposé {low_cells} | runout {:.1} s",
                        v.area.low,
                        v.area.recall_exposed,
                        v.reached_extreme,
                        v.nordic_crossings.low_km,
                        v.nordic_crossings.inside_km,
                        nordic_low / 1000.0,
                        m.timings.runout_extreme
                    );
                }
                None => println!(
                    "{name:<24} marge {margin:>4} m | réseau nordique peu exposé {:.2} km | cellules peu exposé {low_cells} | runout {:.1} s",
                    nordic_low / 1000.0,
                    m.timings.runout_extreme
                ),
            }
        }
    }
    Ok(())
}
