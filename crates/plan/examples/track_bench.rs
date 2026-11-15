//! Cost of the plan (workstation) and of the following per fix (phone) on both sites
//! (E1 #125): a normal outing of each site's first request replayed through the tracker,
//! fix by fix and by batches of 60 s; then a slow family outing (C11 #130: stops,
//! detours, the family's profile on the plan, proposals on).
//!
//! ```sh
//! cargo run --release -p skimap-plan --example track_bench
//! ```

use std::time::Instant;

use skimap_common::layout::DataLayout;
use skimap_common::site::SITES;
use skimap_core::calibration::{GroupProfile, ProfileModel};
use skimap_plan::pace;
use skimap_plan::plan;
use skimap_plan::presets;
use skimap_plan::scenario::{self, Behaviour, Scenario, Step};
use skimap_plan::track::TrackView;
use skimap_plan::tracker::{Input, Output, Tracker, TrackerParams};

fn pct(v: &mut [f64], p: f64) -> f64 {
    v.sort_by(f64::total_cmp);
    v[((v.len() as f64 - 1.0) * p).round() as usize]
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let layout =
        DataLayout::new(std::env::var("SKIMAP_DATA_DIR").unwrap_or_else(|_| "data".into()));
    for site in SITES.iter() {
        let (mut data, ctx) = presets::load(&layout, site)?;
        let req = presets::requests(site, presets::past_day(site))[0].clone();
        let t = Instant::now();
        let m = plan::morning(&data, &req);
        let t_morning = t.elapsed().as_secs_f64();
        let t = Instant::now();
        let plain = plan::build(&mut data, &m, 0, &ctx)?;
        let t_build = t.elapsed().as_secs_f64();
        let g = data.graph.take().ok_or("graph")?;
        let mut fam = plain.clone();
        let t = Instant::now();
        let model = ProfileModel::new(GroupProfile::family(2, &[6, 9], req.style));
        pace::with_pace(&mut fam, &g, &model)?;
        let t_pace = t.elapsed().as_secs_f64();
        println!(
            "{} : temps estimé du groupe sur le plan (C11) {:.2} ms",
            site.id,
            t_pace * 1e3
        );
        for (p, behaviour) in [(&plain, None), (&fam, Some(Behaviour::family()))] {
            let t = Instant::now();
            let bytes = plan::track(p, &g)?.to_bytes();
            let t_track = t.elapsed().as_secs_f64();
            let t = Instant::now();
            let view = TrackView::new(&bytes)?;
            let t_view = t.elapsed().as_secs_f64();
            let sc = Scenario {
                name: "bench".into(),
                start: scenario::departure_utc(p),
                slowdown: 1.0,
                steps: vec![Step::WaitAtStart { secs: 60.0 }],
                sigma_m: 10.0,
                forest: true,
                seed: 11,
                tail_s: 300.0,
                behaviour,
            };
            let run = scenario::run(&sc, p, view, &g, TrackerParams::default())?;
            // Every fix at 1 Hz too (the approach mode everywhere): the worst case per hour.
            let fixes: Vec<&Input> = run
                .inputs
                .iter()
                .filter(|i| matches!(i, Input::Fix { .. }))
                .collect();
            let mut per = Vec::with_capacity(run.inputs.len());
            let mut out: Vec<Output> = Vec::with_capacity(256);
            let reps = 20;
            let t = Instant::now();
            for _ in 0..reps {
                let mut tr = Tracker::new(p, view, &g, None, TrackerParams::default())?;
                for i in &run.inputs {
                    out.clear();
                    let t0 = Instant::now();
                    tr.push(i, &mut out);
                    let dt = t0.elapsed().as_secs_f64() * 1e6;
                    if matches!(i, Input::Fix { .. }) && out.is_empty() {
                        per.push(dt);
                    }
                }
            }
            let total = t.elapsed().as_secs_f64() / reps as f64;
            // Batches of 60 s through push_batch.
            let t = Instant::now();
            for _ in 0..reps {
                let mut tr = Tracker::new(p, view, &g, None, TrackerParams::default())?;
                for chunk in run.inputs.chunks(12) {
                    out.clear();
                    tr.push_batch(chunk, &mut out);
                }
            }
            let batched = t.elapsed().as_secs_f64() / reps as f64;
            let mean = per.iter().sum::<f64>() / per.len() as f64;
            let (p50, p99, max) = (pct(&mut per, 0.5), pct(&mut per, 0.99), pct(&mut per, 1.0));
            println!(
                "{} : matin {:.3} s, plan {:.3} s, table {:.3} s ({} Ko, {} itinéraires), lecture de la table {:.1} µs",
                site.id,
                t_morning,
                t_build,
                t_track,
                bytes.len() / 1024,
                view.len(),
                t_view * 1e6
            );
            println!(
                "  {}sortie de {:.0} min : {} entrées dont {} fixes ; tout le suivi {:.2} ms ({:.2} µs par fix), par lots de 12 {:.2} ms",
                if behaviour.is_some() { "famille, " } else { "" },
                scenario::rel(&run, run.t1) / 60.0,
                run.inputs.len(),
                fixes.len(),
                total * 1e3,
                total * 1e6 / fixes.len() as f64,
                batched * 1e3
            );
            println!(
                "  fix sans rien à dire : moyenne {mean:.2} µs, médiane {p50:.2} µs, p99 {p99:.2} µs, max {max:.1} µs ({} mesures)",
                per.len()
            );
        }
    }
    Ok(())
}
