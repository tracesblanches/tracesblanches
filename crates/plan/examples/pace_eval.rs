//! C11 (#130): error of the arrival time learned per group against C5 alone, on synthetic
//! outings of both sites (a family with children of 6 and 9, a normal adult, a fast
//! adult), in the morning and at 25, 50 and 75 % of the route; leave-future-out
//! cross-validation of the learning (learn on the N outings before, test on the next); cost
//! of the learning on the phone (digest of an outing, fit).
//!
//! ```sh
//! cargo run --release -p skimap-plan --example pace_eval            # both sites, 8 days
//! cargo run --release -p skimap-plan --example pace_eval -- 12      # 12 days
//! ```

use std::time::Instant;

use skimap_common::layout::DataLayout;
use skimap_common::site::SITES;
use skimap_core::calibration::{self, GroupProfile, Level};
use skimap_core::routing::Style;
use skimap_plan::pace_eval::{self, Case};
use skimap_plan::plan;
use skimap_plan::presets;
use skimap_plan::scenario::Behaviour;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let days: u64 = std::env::args()
        .nth(1)
        .and_then(|a| a.parse().ok())
        .unwrap_or(8);
    let layout =
        DataLayout::new(std::env::var("SKIMAP_DATA_DIR").unwrap_or_else(|_| "data".into()));
    let mut all = Vec::new();
    for site in SITES {
        let (mut data, ctx) = presets::load(&layout, site)?;
        let base = presets::requests(site, presets::past_day(site))[0].clone();
        let fam = presets::family_request(site, presets::past_day(site));
        let mut cases = Vec::new();
        for (name, req, bh, profile) in [
            (
                "famille (6 et 9 ans)",
                fam,
                Behaviour::family(),
                GroupProfile::family(2, &[6, 9], Style::Classic),
            ),
            (
                "adulte normal",
                base.clone(),
                Behaviour::normal_adult(),
                GroupProfile::solo(base.style, Level::Intermediate),
            ),
            (
                "adulte rapide",
                base.clone(),
                Behaviour::fast_adult(),
                GroupProfile::solo(base.style, Level::Intermediate),
            ),
        ] {
            let m = plan::morning(&data, &req);
            if m.suggestions.suggestions.is_empty() {
                println!(
                    "{} {name} : aucune boucle ({})",
                    site.id,
                    m.notes.join(" ; ")
                );
                continue;
            }
            let p = plan::build(&mut data, &m, 0, &ctx)?;
            cases.push((name, p, bh, profile));
        }
        let g = data.graph.take().ok_or("graph")?;
        for (name, p, bh, profile) in cases {
            let bytes = plan::track(&p, &g)?.to_bytes();
            let case = Case {
                name: format!("{} — {name}", site.id),
                plan: &p,
                track: &bytes,
                graph: &g,
                behaviour: bh,
                profile: profile.clone(),
                days,
                seed: 500,
            };
            let t = Instant::now();
            let res = pace_eval::evaluate(&case)?;
            let secs = t.elapsed().as_secs_f64();
            println!(
                "\n{} : boucle {} ({:.1} km, modèle {:.0} min), {days} jours simulés et rejoués en {secs:.1} s",
                case.name, p.route.label, p.route.length_km, p.route.model_min
            );
            for l in pace_eval::table(&case.name, &res, 0) {
                println!("{l}");
            }
            for l in pace_eval::table(&case.name, &res, 3) {
                println!("{l}");
            }
            println!(
                "  validation croisée (apprendre sur les N sorties précédentes, tester sur la suivante) :"
            );
            println!(
                "    N | réel (min) | matin : C5 / a priori / appris [fourchette appris] | à 50 % : C5 / rythme / appris"
            );
            for d in &res {
                let m = &d.samples[0];
                let half = d.samples.iter().find(|s| (s.frac - 0.5).abs() < 1e-9);
                println!(
                    "    {} | {:.0} | {:+.0} / {:+.0} / {:+.0} [{}] | {}{}",
                    d.learned_on,
                    d.actual_s / 60.0,
                    (m.c5_s - d.actual_s) / 60.0,
                    (m.prior_s - d.actual_s) / 60.0,
                    (m.learned_s - d.actual_s) / 60.0,
                    if m.learned_in { "dedans" } else { "dehors" },
                    half.map_or("—".to_owned(), |h| format!(
                        "{:+.0} / {:+.0} / {:+.0}",
                        (h.c5_s - d.actual_s) / 60.0,
                        (h.pace_s - d.actual_s) / 60.0,
                        (h.learned_s - d.actual_s) / 60.0
                    )),
                    if d.switched {
                        " (itinéraire changé, non compté)"
                    } else {
                        ""
                    }
                );
            }
            for d in res
                .iter()
                .filter(|d| [0, 3, res.len() - 1].contains(&d.learned_on))
            {
                println!(
                    "  matin après {} sorties : {}",
                    d.learned_on, d.morning_text
                );
            }
            // Cost on the phone (E2): digest of the outings (replay through the tracker)
            // and the fit.
            let obs: Vec<_> = res.iter().map(|d| d.obs.clone()).collect();
            let t = Instant::now();
            let reps = 2000;
            for _ in 0..reps {
                std::hint::black_box(calibration::fit(profile.clone(), &obs));
            }
            let fit_us = t.elapsed().as_secs_f64() / reps as f64 * 1e6;
            let dig: f64 = res.iter().map(|d| d.digest_ms).sum::<f64>() / res.len() as f64;
            let inputs: usize = res.iter().map(|d| d.inputs).sum::<usize>() / res.len();
            println!(
                "  coût (poste, release) : résumé d'une sortie (rejeu de ses {inputs} entrées dans le suivi) {dig:.1} ms en moyenne, ajustement sur {} sorties {fit_us:.1} µs",
                obs.len()
            );
            all.push((case.name.clone(), res));
        }
    }
    // Overall, days with at least 3 outings learned.
    println!("\nEnsemble (au moins 3 sorties apprises) :");
    for (name, res) in &all {
        let s = pace_eval::score(res, 3);
        let at = |f: f64| {
            s.iter()
                .find(|x| (x.frac - f).abs() < 1e-9)
                .copied()
                .unwrap_or_default()
        };
        println!(
            "  {name} : matin C5 {:.0} min, appris {:.0} min ; 50 % C5 {:.0}, rythme {:.0}, appris {:.0} min",
            at(0.0).mae_min[0],
            at(0.0).mae_min[4],
            at(0.5).mae_min[0],
            at(0.5).mae_min[2],
            at(0.5).mae_min[4]
        );
    }
    Ok(())
}
