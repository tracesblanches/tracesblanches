//! Q9 scenarios for the app's instrumented test (K4, K5, K6, E1): real outings simulated on
//! the plan of the archived day of an evaluation site (`skimap_plan::scenario`, as in
//! `crates/plan/tests/scenarios.rs`), written with what the core says for them, so that
//! the phone replays the same inputs through its location service and checks that it says,
//! switches and sets the GPS exactly as the core does.
//!
//! ```sh
//! cargo run --release -p skimap-ffi --example q9_export -- OUT_DIR [SITE]
//! adb push OUT_DIR/. /data/local/tmp/skimap-q9/
//! ```
//!
//! Per scenario, `OUT_DIR/<name>/`: `plan.json`, `plan.trk` (the plan and its flat table),
//! `scenario.jsonl` (the inputs, `skimap_plan::tracker::Input` per line) and
//! `expected.json` (spoken texts with their kinds and whether every clip is in the pack,
//! alerts, switches, proposals, session decisions, GPS requests, each with its time).
//! SITE: an evaluation site (archived day and plan requests in `data/sites.json`), default
//! the first one. Reads `data/out/<site>/` (pipeline outputs, archived day, pack).

#![allow(
    clippy::disallowed_methods,
    reason = "developer tool, not a pipeline output"
)]
use std::path::PathBuf;

use jiff::civil::Time;
use serde_json::{Value, json};
use skimap_common::layout::DataLayout;
use skimap_plan::plan::{self, KeyKind};
use skimap_plan::presets;
use skimap_plan::scenario::{self, Behaviour, Scenario, Step};
use skimap_plan::track::TrackView;
use skimap_plan::tracker::{Output, Tracker, TrackerParams};
use skimap_voice::pack::VoiceManifest;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let out = PathBuf::from(
        std::env::args()
            .nth(1)
            .ok_or("usage: q9_export OUT_DIR [SITE]")?,
    );
    let site = match std::env::args().nth(2) {
        Some(id) => skimap_common::site::site(&id)?,
        None => &skimap_common::site::SITES[0],
    };
    let root = std::env::var("SKIMAP_DATA_DIR")
        .unwrap_or_else(|_| format!("{}/../../data", env!("CARGO_MANIFEST_DIR")));
    let layout = DataLayout::new(&root);
    let pack = layout.out_dir(site).join("pack");
    let vdir = pack.join("voice");
    let manifest = VoiceManifest::read(&vdir)?;
    let voice = skimap_plan::phrases::Voice::new(&manifest, "pack");
    let (mut data, ctx) = presets::load(&layout, site)?;
    let date = presets::past_day(site);
    let base = presets::requests(site, date).remove(0);
    let mut family = base.clone();
    family.start_time = Time::new(10, 0, 0, 0).ok();
    for (req, scenarios) in [
        (base, vec!["refuge", "closure_refused", "wrong_turn"]),
        (family, vec!["family"]),
    ] {
        let m = plan::morning(&data, &req);
        let p = plan::build(&mut data, &m, 0, &ctx)?;
        let g = data.graph.as_ref().ok_or("graph")?;
        let trk = plan::track(&p, g)?.to_bytes();
        for name in scenarios {
            let mut sc = Scenario {
                name: name.into(),
                start: scenario::departure_utc(&p),
                slowdown: 1.0,
                steps: Vec::new(),
                sigma_m: 10.0,
                forest: true,
                seed: 7,
                tail_s: 300.0,
                behaviour: None,
                step_counter: false,
            };
            match name {
                "refuge" => {
                    let s = p
                        .key_points
                        .iter()
                        .find(|k| k.kind == KeyKind::Refuge)
                        .ok_or("refuge")?
                        .s;
                    sc.steps = vec![Step::Stop {
                        at_s: s,
                        secs: 1200.0,
                    }];
                }
                "closure_refused" => {
                    let (ways, s_div) = p
                        .plans_b
                        .iter()
                        .filter(|x| x.fallback.exists() && x.fallback.s_div > 60.0)
                        .find_map(|x| match &x.fallback.kind {
                            skimap_plan::fallback::FallbackKind::Detour { ways, .. } => {
                                Some((ways.clone(), x.fallback.s_div))
                            }
                            _ => None,
                        })
                        .ok_or("detour")?;
                    sc.steps = vec![
                        Step::Close {
                            at_s: (s_div - 800.0).max(s_div * 0.3),
                            ways,
                        },
                        Step::RefuseNext { after_s: 20.0 },
                    ];
                }
                "wrong_turn" => {
                    // The first junction after 1 km with a trail the loop never takes (a
                    // branch it takes later is not a wrong turn).
                    let j = (0..p.junctions.len())
                        .find(|&k| {
                            p.junctions[k].s > 1000.0
                                && p.junctions[k].branches > 0
                                && scenario::wrong_turn(&p, g, k, 250.0, true).is_ok()
                        })
                        .ok_or("junction")?;
                    sc.steps = vec![Step::WrongTurn {
                        junction: j,
                        length_m: 250.0,
                        back: true,
                    }];
                }
                _ => {
                    sc.steps = vec![Step::WaitAtStart { secs: 60.0 }];
                    sc.behaviour = Some(Behaviour::family());
                    // The phone counts the steps (X10): reports go with the fixes.
                    sc.step_counter = true;
                }
            }
            let run = scenario::run(&sc, &p, TrackView::new(&trk)?, g, TrackerParams::default())?;
            // What the phone's tracker says: the same tracker with the pack's clips.
            let mut tr = Tracker::new(
                &p,
                TrackView::new(&trk)?,
                g,
                Some(&voice),
                TrackerParams::default(),
            )?;
            let mut outs = Vec::new();
            for i in &run.inputs {
                tr.push(i, &mut outs);
            }
            let dir = out.join(name);
            std::fs::create_dir_all(&dir)?;
            std::fs::write(dir.join("plan.json"), p.to_json()?)?;
            std::fs::write(dir.join("plan.trk"), &trk)?;
            let lines: Vec<String> = run
                .inputs
                .iter()
                .map(serde_json::to_string)
                .collect::<Result<_, _>>()?;
            std::fs::write(dir.join("scenario.jsonl"), lines.join("\n") + "\n")?;
            let expected = expected(&outs, &manifest);
            std::fs::write(
                dir.join("expected.json"),
                serde_json::to_string_pretty(&expected)?,
            )?;
            println!(
                "{name}: {} inputs ({} fixes, {} batches), {} said, {} switches, {} GPS requests, {} proposals → {}",
                run.inputs.len(),
                run.fixes,
                run.batches,
                expected["spoken"].as_array().map_or(0, Vec::len),
                expected["switches"].as_array().map_or(0, Vec::len),
                expected["gps"].as_array().map_or(0, Vec::len),
                expected["proposals"].as_array().map_or(0, Vec::len),
                dir.display()
            );
        }
    }
    Ok(())
}

fn ms(t: jiff::Timestamp) -> i64 {
    t.as_millisecond()
}

fn expected(outs: &[Output], m: &VoiceManifest) -> Value {
    let has = |id: &str, text: &str| m.get(id).is_some_and(|e| e.text == text);
    let mut spoken = Vec::new();
    let mut alerts = Vec::new();
    let mut switches = Vec::new();
    let mut gps = Vec::new();
    let mut session = Vec::new();
    let mut proposals = Vec::new();
    for o in outs {
        match o {
            Output::Spoken { t, spoken: s } => spoken.push(json!({
                "t_ms": ms(*t),
                "text": s.announcement.text(),
                "kinds": s.items.iter().map(|c| c.kind.clone()).collect::<Vec<_>>(),
                "all_clips": s.announcement.segments.iter().all(|x| has(&x.clip.id, &x.clip.text)),
            })),
            Output::Alert { t, alert } => alerts.push(json!({"t_ms": ms(*t), "alert": alert})),
            Output::Switched { t, to, .. } => {
                switches.push(json!({"t_ms": ms(*t), "to": to, "reverted": false}));
            }
            Output::Reverted { t, to, .. } => {
                switches.push(json!({"t_ms": ms(*t), "to": to, "reverted": true}));
            }
            Output::Gps { t, request } => gps.push(json!({
                "t_ms": ms(*t),
                "mode": request.mode,
                "gnss": request.gnss,
                "interval_ms": (request.interval_s * 1000.0).round() as i64,
                "max_delay_ms": (request.max_delay_s * 1000.0).round() as i64,
            })),
            Output::Session { t, event } => session.push(json!({"t_ms": ms(*t), "event": event})),
            Output::Proposal { t, proposal } => {
                proposals.push(json!({"t_ms": ms(*t), "text": proposal.text}));
            }
            _ => {}
        }
    }
    json!({
        "spoken": spoken,
        "alerts": alerts,
        "switches": switches,
        "gps": gps,
        "session": session,
        "proposals": proposals,
    })
}
