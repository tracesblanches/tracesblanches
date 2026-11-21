//! X10 (#144): false pauses and stops found on synthetic outings of every evaluation site
//! (test data, archived day of `data/sites.json`), every
//! request: a skier at the C5 pace and slower (the pace divided by 2, 3, 4: climbs of 0.2 to
//! 0.7 m/s), a family and an adult with random stops (C11 `Behaviour`), four GPS (σ 10 and
//! 15 m, forest); the fixes alone with the thresholds of before X10 (`avant`: the flat's on
//! climbs), with the fixed windows of X10 (`X10`: 180 s under 0.08 m/s on climbs), now
//! (`GPS seul`: the window sized on the skier's expected speed, #144), and the steps counted
//! by the phone (`pas`). `PAUSE_SPAN_M`, `PAUSE_SHARE`, `PAUSE_MIN_MS`, `PAUSE_MAX_S`
//! override the settings of `GPS seul`. A pause is false when it meets no stop of the skier; a stop
//! of 90 s or more is found when a pause meets it.
//!
//! ```sh
//! cargo run --release -p skimap-plan --example pause_eval
//! ```
use skimap_common::layout::DataLayout;
use skimap_plan::plan;
use skimap_plan::presets;
use skimap_plan::scenario::{self, Behaviour, Run, Scenario, Step};
use skimap_plan::session::SessionEvent;
use skimap_plan::track::TrackView;
use skimap_plan::tracker::{Output, TrackerParams};

/// (false pauses, real stops ≥ 90 s, detected, pauses).
fn score(run: &Run) -> (usize, usize, usize, usize) {
    let t0 = run.t0.as_second() as f64;
    let mut pauses: Vec<(f64, f64)> = Vec::new();
    let mut fin = f64::INFINITY;
    for o in &run.outputs {
        if let Output::Session { event, .. } = o {
            match event {
                SessionEvent::Paused { t } => pauses.push((t - t0, f64::INFINITY)),
                SessionEvent::Resumed { t, .. } => {
                    if let Some(p) = pauses.last_mut() {
                        p.1 = t - t0;
                    }
                }
                SessionEvent::Finished { t, .. } => fin = fin.min(t - t0),
                _ => {}
            }
        }
    }
    let pauses: Vec<(f64, f64)> = pauses.into_iter().filter(|p| p.0 < fin - 1.0).collect();
    let overlaps = |p: &(f64, f64), s: &(f64, f64)| p.0 < s.0 + s.1 + 30.0 && p.1 > s.0 - 30.0;
    let false_p = pauses
        .iter()
        .filter(|p| !run.stops.iter().any(|s| overlaps(p, s)))
        .count();
    let real: Vec<&(f64, f64)> = run
        .stops
        .iter()
        .filter(|s| s.1 >= 90.0 && s.0 < fin)
        .collect();
    let found = real
        .iter()
        .filter(|s| pauses.iter().any(|p| overlaps(p, s)))
        .count();
    (false_p, real.len(), found, pauses.len())
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let l = DataLayout::new(std::env::var("SKIMAP_DATA_DIR").unwrap_or_else(|_| "data".into()));
    let mut tot = std::collections::BTreeMap::new();
    for site in skimap_common::site::SITES {
        let (mut data, ctx) = presets::load(&l, site)?;
        for req in presets::requests(site, presets::past_day(site)) {
            let m = plan::morning(&data, &req);
            if m.suggestions.suggestions.is_empty() {
                continue;
            }
            let p = plan::build(&mut data, &m, 0, &ctx)?;
            let g = data.graph.as_ref().ok_or("graph")?;
            let trk = plan::track(&p, g)?.to_bytes();
            let view = TrackView::new(&trk)?;
            for (who, slow, bh) in [
                ("C5", 1.0, None),
                ("lent x2", 2.0, None),
                ("lent x3", 3.0, None),
                ("lent x4", 4.0, None),
                ("famille", 1.0, Some(Behaviour::family())),
                ("adulte", 1.0, Some(Behaviour::normal_adult())),
            ] {
                for (mode, steps) in [
                    ("avant", false),
                    ("X10", false),
                    ("GPS seul", false),
                    ("pas", true),
                ] {
                    for (sigma, seed) in [(10.0, 7u64), (15.0, 99), (10.0, 11), (10.0, 23)] {
                        let sc = Scenario {
                            name: "probe".into(),
                            start: scenario::departure_utc(&p),
                            slowdown: slow,
                            steps: vec![Step::WaitAtStart { secs: 60.0 }],
                            sigma_m: sigma,
                            forest: true,
                            seed,
                            tail_s: 300.0,
                            behaviour: bh.map(|b| b.day(seed)),
                            step_counter: steps,
                        };
                        let mut params = TrackerParams::default();
                        let env = |k: &str| std::env::var(k).ok().and_then(|v| v.parse().ok());
                        let ps = &mut params.session;
                        ps.pause_span_m = env("PAUSE_SPAN_M").unwrap_or(ps.pause_span_m);
                        ps.pause_share = env("PAUSE_SHARE").unwrap_or(ps.pause_share);
                        ps.pause_min_ms = env("PAUSE_MIN_MS").unwrap_or(ps.pause_min_ms);
                        ps.pause_max_s = env("PAUSE_MAX_S").unwrap_or(ps.pause_max_s);
                        if mode != "GPS seul" && mode != "pas" {
                            params.session.pause_span_m = 0.0;
                        }
                        if mode == "avant" {
                            params.session.climb_pause_s = params.session.pause_s;
                            params.session.climb_pause_fix_ms = params.session.pause_fix_ms;
                        }
                        let run = scenario::run(&sc, &p, view, g, params)?;
                        let (f, r, d, n) = score(&run);
                        let hours = (run.t1 - run.t0).as_secs() as f64 / 3600.0;
                        let e = tot
                            .entry((who, mode))
                            .or_insert((0usize, 0usize, 0usize, 0usize, 0.0f64));
                        e.0 += f;
                        e.1 += r;
                        e.2 += d;
                        e.3 += n;
                        e.4 += hours;
                        println!(
                            "{} {} — {who}, {mode}, σ {sigma} m, graine {seed} : {hours:.1} h, {f} fausses pauses, {d} arrêts de 90 s ou plus trouvés sur {r} ({n} pauses)",
                            site.id, req.label
                        );
                    }
                }
            }
        }
    }
    println!("\nEnsemble (deux sites, toutes les demandes, quatre GPS) :");
    for ((who, mode), (f, r, d, n, h)) in &tot {
        println!(
            "  {who}, {mode} : {h:.0} h, {f} fausses pauses, {d} arrêts trouvés sur {r} ({n} pauses)"
        );
    }
    Ok(())
}
