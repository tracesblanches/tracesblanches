//! Cost of the plan (workstation) and of the following per fix (phone) on both sites
//! (E1 #125): a normal outing of each request of each site replayed through the tracker,
//! fix by fix and by batches of 60 s; on each plan's largest table, an outing switched
//! early to a plan B (a closed trail: the plan B read through the loop's block, X13
//! #147); then a slow family outing on the first request (C11 #130: stops, detours, the
//! family's profile on the plan, proposals on).
//!
//! ```sh
//! cargo run --release -p skimap-plan --example track_bench
//! ```

use std::time::Instant;

use skimap_common::layout::DataLayout;
use skimap_common::site::SITES;
use skimap_core::calibration::{GroupProfile, ProfileModel};
use skimap_core::graph::PisteGraph;
use skimap_plan::fallback::FallbackKind;
use skimap_plan::pace;
use skimap_plan::plan::{self, DayPlan};
use skimap_plan::presets;
use skimap_plan::scenario::{self, Behaviour, Scenario, Step};
use skimap_plan::track::TrackView;
use skimap_plan::tracker::{Input, Output, Tracker, TrackerParams};

fn pct(v: &mut [f64], p: f64) -> f64 {
    v.sort_by(f64::total_cmp);
    v[((v.len() as f64 - 1.0) * p).round() as usize]
}

/// A closed trail ahead of a detour of the morning, 600 m before it leaves the loop.
fn closure(p: &DayPlan) -> Option<Step> {
    p.plans_b
        .iter()
        .filter(|b| b.fallback.exists() && b.fallback.s_div > 1500.0)
        .find_map(|b| match &b.fallback.kind {
            FallbackKind::Detour { ways, .. } if !ways.is_empty() => Some(Step::Close {
                at_s: b.fallback.s_div - 600.0,
                ways: ways.clone(),
            }),
            _ => None,
        })
}

fn bench(
    label: &str,
    p: &DayPlan,
    g: &PisteGraph,
    steps: Vec<Step>,
    behaviour: Option<Behaviour>,
) -> Result<(), Box<dyn std::error::Error>> {
    let t = Instant::now();
    let bytes = plan::track(p, g)?.to_bytes();
    let t_track = t.elapsed().as_secs_f64();
    let t = Instant::now();
    let view = TrackView::new(&bytes)?;
    let t_view = t.elapsed().as_secs_f64();
    let sc = Scenario {
        name: "bench".into(),
        start: scenario::departure_utc(p),
        slowdown: 1.0,
        steps,
        sigma_m: 10.0,
        forest: true,
        seed: 11,
        tail_s: 300.0,
        behaviour,
    };
    let run = scenario::run(&sc, p, view, g, TrackerParams::default())?;
    let switches = run
        .outputs
        .iter()
        .filter(|o| matches!(o, Output::Switched { .. }))
        .count();
    let fixes = run
        .inputs
        .iter()
        .filter(|i| matches!(i, Input::Fix { .. }))
        .count();
    let mut per = Vec::with_capacity(run.inputs.len() * 20);
    let mut out: Vec<Output> = Vec::with_capacity(256);
    let reps = 20;
    let t = Instant::now();
    for _ in 0..reps {
        let mut tr = Tracker::new(p, view, g, None, TrackerParams::default())?;
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
    // Batches of 60 s through push_batch; the fastest pass (the least disturbed by the
    // other processes of the machine) too.
    let t = Instant::now();
    let mut best = f64::INFINITY;
    for _ in 0..reps {
        let t1 = Instant::now();
        let mut tr = Tracker::new(p, view, g, None, TrackerParams::default())?;
        for chunk in run.inputs.chunks(12) {
            out.clear();
            tr.push_batch(chunk, &mut out);
        }
        best = best.min(t1.elapsed().as_secs_f64());
    }
    let batched = t.elapsed().as_secs_f64() / reps as f64;
    let mean = per.iter().sum::<f64>() / per.len() as f64;
    let (p50, p99, max) = (pct(&mut per, 0.5), pct(&mut per, 0.99), pct(&mut per, 1.0));
    println!(
        "  {label} : table {:.3} s ({} Ko, {} itinéraires), lecture de la table {:.1} µs ; sortie de {:.0} min, {} bascule(s) : {} entrées dont {fixes} fixes ; tout le suivi {:.2} ms ({:.2} µs par fix), par lots de 12 {:.2} ms (meilleure passe {:.2} ms)",
        t_track,
        bytes.len() / 1024,
        view.len(),
        t_view * 1e6,
        scenario::rel(&run, run.t1) / 60.0,
        switches,
        run.inputs.len(),
        total * 1e3,
        total * 1e6 / fixes as f64,
        batched * 1e3,
        best * 1e3
    );
    println!(
        "    fix sans rien à dire : moyenne {mean:.2} µs, médiane {p50:.2} µs, p99 {p99:.2} µs, max {max:.1} µs ({} mesures)",
        per.len()
    );
    Ok(())
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let layout =
        DataLayout::new(std::env::var("SKIMAP_DATA_DIR").unwrap_or_else(|_| "data".into()));
    for site in SITES.iter() {
        let (mut data, ctx) = presets::load(&layout, site)?;
        let mut plans = Vec::new();
        for req in presets::requests(site, presets::past_day(site)) {
            let t = Instant::now();
            let m = plan::morning(&data, &req);
            let t_morning = t.elapsed().as_secs_f64();
            if m.suggestions.suggestions.is_empty() {
                continue;
            }
            let t = Instant::now();
            let p = plan::build(&mut data, &m, 0, &ctx)?;
            println!(
                "{} {} : matin {:.3} s, plan {:.3} s",
                site.id,
                req.label,
                t_morning,
                t.elapsed().as_secs_f64()
            );
            plans.push((req, p));
        }
        let g = data.graph.take().ok_or("graph")?;
        for (req, p) in &plans {
            bench(
                &format!("{} {}, sortie normale", site.id, req.label),
                p,
                &g,
                vec![Step::WaitAtStart { secs: 60.0 }],
                None,
            )?;
        }
        // The largest table: an outing on a plan B.
        let big = plans
            .iter()
            .max_by_key(|(_, p)| p.plans_b.len())
            .ok_or("plan")?;
        if let Some(close) = closure(&big.1) {
            bench(
                &format!("{} {}, piste fermée (plan B)", site.id, big.0.label),
                &big.1,
                &g,
                vec![Step::WaitAtStart { secs: 60.0 }, close],
                None,
            )?;
        }
        let (req, plain) = &plans[0];
        let mut fam = plain.clone();
        let t = Instant::now();
        let model = ProfileModel::new(GroupProfile::family(2, &[6, 9], req.style));
        pace::with_pace(&mut fam, &g, &model)?;
        println!(
            "{} : temps estimé du groupe sur le plan (C11) {:.2} ms",
            site.id,
            t.elapsed().as_secs_f64() * 1e3
        );
        bench(
            &format!("{} {}, famille", site.id, req.label),
            &fam,
            &g,
            vec![Step::WaitAtStart { secs: 60.0 }],
            Some(Behaviour::family()),
        )?;
    }
    Ok(())
}
