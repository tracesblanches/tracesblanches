//! Settings of the estimated traces (G1, #117) measured on the sites given: each site's
//! plan is registered once, then the estimation runs for every combination of settings
//! (radius of the OSM explanation, of the map label, of the departure box, smallest share
//! followed by paths). Prints, per combination, the traces written and their method.
//!
//! `SKIMAP_PERSONAL_SOURCES=1 cargo run --release -p skimap-pisteplan --features pipeline --example estimate_sweep <site...>`

use skimap_common::config::Config;
use skimap_common::download::Downloader;
use skimap_pisteplan::build::register_site;
use skimap_pisteplan::estimate::{Params, Path, estimate_with, paths_ign};
use skimap_pisteplan::transcript::MatchMethod;
use skimap_pisteplan::{Error, learn, ocr, transcribe};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let cfg = Config::from_env()?;
    if !cfg.personal_sources {
        return Err(Error::PersonalSourcesDisabled.into());
    }
    let layout = cfg.layout();
    let dl = Downloader::from_config(&cfg)?;
    let registry = skimap_common::site::Registry::load(&layout)?;
    let planet = skimap_osm::extract::load_planet(&dl)?;
    let engine = ocr::engine(&dl)?;
    let model = learn::Model::read(&layout.france_layer_dir("plan_training")?.join("model.json"));
    for id in std::env::args().skip(1) {
        let site = registry.get(&id)?;
        let (reg, _) = register_site(site, &layout, &dl, &planet, &engine)?;
        let entries = transcribe::entries(&reg);
        let dmap = transcribe::InkDistance::new(&reg);
        let (mut decisions, _) = transcribe::decide(&reg, &dmap, &entries);
        if let Some(m) = &model {
            learn::apply_model(&reg, &dmap, &mut decisions, m);
        }
        let mut cache: Option<Vec<Path>> = None;
        for explained_pt in [4.0, 6.0, 8.0] {
            for label_radius_pt in [12.0, 20.0, 30.0] {
                for box_radius_pt in [30.0, 45.0] {
                    for min_covered in [0.4, 0.6] {
                        let prm = Params {
                            explained_pt,
                            label_radius_pt,
                            box_radius_pt,
                            min_covered,
                            ..Params::default()
                        };
                        let out = estimate_with(&reg, &decisions, &prm, &mut |_b| {
                            if cache.is_none() {
                                cache = Some(paths_ign(&dl, &reg.spec.extent)?.0);
                            }
                            Ok(cache.clone().unwrap_or_default())
                        })?;
                        let paths = out
                            .estimated
                            .iter()
                            .filter(|e| e.method == MatchMethod::EstimatedPaths)
                            .count();
                        let names: Vec<String> = out
                            .estimated
                            .iter()
                            .map(|e| {
                                format!(
                                    "{}:{}:{:.0}%:{:.0}m",
                                    decisions[e.decision].entry.name_read,
                                    if e.method == MatchMethod::EstimatedPaths {
                                        "P"
                                    } else {
                                        "D"
                                    },
                                    e.estimate.drawn_covered * 100.0,
                                    e.length_m
                                )
                            })
                            .collect();
                        println!(
                            "{id}\texplained {explained_pt}\tlabel {label_radius_pt}\tbox {box_radius_pt}\tcovered {min_covered}\testimated {}\tpaths {paths}\tdrawn_lines {}\t{}",
                            out.estimated.len(),
                            out.drawn_lines,
                            names.join(" | ")
                        );
                    }
                }
            }
        }
    }
    Ok(())
}
