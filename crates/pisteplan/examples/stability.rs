//! Stability of the exact matching under realistic changes of the automatic sheet (X4):
//! the plan is read once, its automatic sheet perturbed (map area a few points larger or
//! smaller, ink thresholds ± ΔE 2, line width ± 10 %, ink references a little lighter or
//! darker), the plan registered again with each sheet and its pistes matched. Prints the
//! pistes transcribed by each sheet and the spread, for the current rule and for the
//! coverage tolerant to a local offset (`transcribe::tolerant_test`: one offset for the
//! whole geometry, or one per piece of 200 m, at a few thresholds).
//!
//! `SKIMAP_PERSONAL_SOURCES=1 cargo run --release -p skimap-pisteplan --features pipeline --example stability -- <catalogue id>...`

use std::collections::{BTreeMap, BTreeSet};

use skimap_common::config::Config;
use skimap_pisteplan::autospec::{self, EXTENT_MARGIN_M, line_params, read_plan};
use skimap_pisteplan::build::register_input;
use skimap_pisteplan::collect::{self, Collected};
use skimap_pisteplan::spec::{LabelSource, PlanSpec};
use skimap_pisteplan::transcribe::{self, InkDistance};
use skimap_pisteplan::{Error, ocr};

/// A change of the sheet.
type Perturb = Box<dyn Fn(&mut PlanSpec)>;

/// The perturbations: name and change of the sheet.
fn perturbations(width_px: f64, k: f64) -> Vec<(String, Perturb)> {
    let area = |d: f64| -> Perturb {
        Box::new(move |s: &mut PlanSpec| {
            s.map_area = [
                s.map_area[0] - d,
                s.map_area[1] - d,
                s.map_area[2] + d,
                s.map_area[3] + d,
            ];
        })
    };
    let de = |d: f32| -> Perturb {
        Box::new(move |s: &mut PlanSpec| {
            for i in &mut s.inks {
                i.max_de = (i.max_de + d).max(4.0);
            }
        })
    };
    let width = |f: f64| -> Perturb {
        Box::new(move |s: &mut PlanSpec| {
            if width_px > 0.0 {
                s.line = line_params(width_px * f, k);
            }
        })
    };
    let shade = |d: i16| -> Perturb {
        Box::new(move |s: &mut PlanSpec| {
            for i in &mut s.inks {
                for r in &mut i.refs {
                    for c in r.iter_mut() {
                        *c = (*c as i16 + d).clamp(0, 255) as u8;
                    }
                }
            }
        })
    };
    vec![
        ("fiche".to_owned(), Box::new(|_: &mut PlanSpec| {})),
        ("zone −3 pt".to_owned(), area(-3.0)),
        ("zone +3 pt".to_owned(), area(3.0)),
        ("seuils ΔE −2".to_owned(), de(-2.0)),
        ("seuils ΔE +2".to_owned(), de(2.0)),
        ("trait −10 %".to_owned(), width(0.9)),
        ("trait +10 %".to_owned(), width(1.1)),
        ("encres −6".to_owned(), shade(-6)),
        ("encres +6".to_owned(), shade(6)),
    ]
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .with_writer(std::io::stderr)
        .init();
    let ids: Vec<String> = std::env::args().skip(1).collect();
    let cfg = Config::from_env()?;
    if !cfg.personal_sources {
        return Err(Error::PersonalSourcesDisabled.into());
    }
    let layout = cfg.layout();
    let dl = collect::downloader(&cfg)?;
    let census = collect::load_census(&layout)?;
    let cat: Vec<Collected> = serde_json::from_slice(&std::fs::read(
        layout
            .france_layer_dir(collect::LAYER)?
            .join("catalogue.json"),
    )?)?;
    let planet = skimap_osm::extract::load_planet(&dl)?;
    let engine = ocr::engine(&dl)?;
    for id in &ids {
        let c = cat
            .iter()
            .find(|c| c.candidate.id == *id)
            .ok_or("not in the catalogue")?;
        let src = collect::plan_source(c);
        let hint = collect::hint(&c.candidate.domains, &census).ok_or("no domain")?;
        let (probe, inp, lines) = read_plan(&src, &layout, &dl, &engine)?;
        let auto = autospec::detect(&src, &probe, &inp.image, inp.pt_per_px, &lines, hint);
        let k = inp.pt_per_px;
        // Rules compared: the current one (radius validated per plan, half the geometry),
        // and the coverage tolerant to a local offset at a few thresholds.
        let rules: Vec<(String, Option<(bool, f64)>)> =
            std::iter::once(("règle actuelle".to_owned(), None))
                .chain(
                    [0.4, 0.5, 0.6]
                        .iter()
                        .map(|&t| (format!("décalage global ≥ {t:.1}"), Some((false, t)))),
                )
                .chain(
                    [0.4, 0.5, 0.6, 0.7]
                        .iter()
                        .map(|&t| (format!("décalage par morceau ≥ {t:.1}"), Some((true, t)))),
                )
                .collect();
        let mut runs: Vec<(String, Option<Vec<BTreeSet<String>>>)> = Vec::new();
        for (name, f) in perturbations(auto.report.line_width_px, k) {
            let mut spec = auto.spec.clone();
            f(&mut spec);
            let reg = register_input(
                spec,
                inp.clone(),
                0.0,
                &layout,
                &dl,
                &planet,
                (auto.spec.labels == LabelSource::Ocr).then_some(&engine),
                Some(EXTENT_MARGIN_M),
            );
            let got = match reg {
                Ok(reg) => {
                    let dmap = InkDistance::new(&reg);
                    let plan = transcribe::entries(&reg);
                    let (ds, v) = transcribe::decide(&reg, &dmap, &plan);
                    // The coverage tolerant to a local offset of every name candidate.
                    let tol = transcribe::local_tolerance_pt(&reg);
                    let tts: Vec<Option<transcribe::TolerantTest>> = ds
                        .iter()
                        .map(|d| match (d.entry.ink, d.ways.is_empty()) {
                            (Some(ink), false) => Some(transcribe::tolerant_test(
                                &dmap,
                                &transcribe::TestSamples::new(&reg, &d.ways),
                                ink,
                                tol,
                            )),
                            _ => None,
                        })
                        .collect();
                    let sets: Vec<BTreeSet<String>> = rules
                        .iter()
                        .map(|(_, t)| {
                            ds.iter()
                                .zip(&tts)
                                .filter(|(d, tt)| match t {
                                    None => d.status.is_some(),
                                    Some((local, t)) => tt.as_ref().is_some_and(|tt| {
                                        let (s, c) = if *local {
                                            (tt.local, &tt.local_copies)
                                        } else {
                                            (tt.share, &tt.copies)
                                        };
                                        s >= *t && c.iter().all(|x| s > *x)
                                    }),
                                })
                                .map(|(d, _)| d.entry.name_read.clone())
                                .collect()
                        })
                        .collect();
                    println!(
                        "{id} {name:14} : {} (rayon {} pt, tolérance {:.1} pt, {} candidats)",
                        sets.iter()
                            .map(|s| s.len().to_string())
                            .collect::<Vec<_>>()
                            .join(" / "),
                        v.radius_pt,
                        transcribe::local_tolerance_pt(&reg),
                        v.candidates
                    );
                    Some(sets)
                }
                Err(e) => {
                    println!("{id} {name:14} : échec {e}");
                    None
                }
            };
            runs.push((name, got));
        }
        for (ri, (rule, _)) in rules.iter().enumerate() {
            let sets: Vec<&BTreeSet<String>> = runs
                .iter()
                .filter_map(|r| r.1.as_ref().map(|s| &s[ri]))
                .collect();
            let counts: Vec<usize> = sets.iter().map(|s| s.len()).collect();
            let (lo, hi) = (
                counts.iter().min().copied().unwrap_or(0),
                counts.iter().max().copied().unwrap_or(0),
            );
            // Pistes that some sheets transcribe and others do not.
            let mut seen: BTreeMap<&str, usize> = BTreeMap::new();
            for s in &sets {
                for n in s.iter() {
                    *seen.entry(n.as_str()).or_default() += 1;
                }
            }
            let flips: Vec<String> = seen
                .iter()
                .filter(|(_, n)| **n < counts.len())
                .map(|(n, c)| format!("{n} ({c}/{})", counts.len()))
                .collect();
            println!(
                "{id} [{rule}] : {} fiches calées sur {}, exactes de {lo} à {hi} (écart {}), pistes instables : {}",
                counts.len(),
                runs.len(),
                hi - lo,
                if flips.is_empty() {
                    "aucune".to_owned()
                } else {
                    flips.join(", ")
                }
            );
        }
    }
    Ok(())
}
