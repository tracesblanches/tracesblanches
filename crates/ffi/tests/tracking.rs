//! The morning, the validated plan and the tracking session on the real packs of both
//! sites (`full_data_*`, `--ignored`: needs `data/out/<site>/pack/` and the pipeline
//! outputs), against the core's tracker run directly on the same inputs.
//!
//! ```sh
//! cargo test --release -p skimap-ffi --test tracking -- --ignored --nocapture
//! ```

#![allow(
    clippy::disallowed_methods,
    reason = "tests write their inputs directly"
)]
use std::path::{Path, PathBuf};
use std::time::Instant;

use chrono::NaiveTime;
use skimap_common::layout::DataLayout;
use skimap_common::site::Site;
/// Evaluation site of `data/sites.json`, test data.
static CERVIERES: std::sync::LazyLock<skimap_common::site::Site> =
    std::sync::LazyLock::new(|| *skimap_common::site::site("cervieres").expect("evaluation site"));
/// Evaluation site of `data/sites.json`, test data.
static CHAPELLE_DES_BOIS: std::sync::LazyLock<skimap_common::site::Site> =
    std::sync::LazyLock::new(|| {
        *skimap_common::site::site("chapelle-des-bois").expect("evaluation site")
    });
use skimap_ffi::tracking::{
    BatchOut, GpsModeView, PhaseView, StepReport, TrackActivity, TrackEvent, TrackingSession,
    load_graph, load_layers, open_tracking, pack_fixes,
};
use skimap_ffi::tracking_day::{
    DayRequest, GroupSpec, day_starts, group_default, group_learn, group_new, open_day_planner,
    read_day_plan,
};
use skimap_plan::gps::GpsRequest;
use skimap_plan::phrases::{Voice, never_safe};
use skimap_plan::plan::DayPlan;
use skimap_plan::presets;
use skimap_plan::scenario::{self, Behaviour, Scenario, Step};
use skimap_plan::session::Activity;
use skimap_plan::track::TrackView;
use skimap_plan::tracker::{Input, Output, Tracker, TrackerParams};
use skimap_voice::pack::VoiceManifest;

fn data() -> PathBuf {
    let root = std::env::var("SKIMAP_DATA_DIR")
        .unwrap_or_else(|_| format!("{}/../../data", env!("CARGO_MANIFEST_DIR")));
    let p = PathBuf::from(root);
    assert!(
        p.join("out/cervieres/pack/manifest.json").is_file(),
        "full data missing: {}",
        p.display()
    );
    p
}

fn pack(site: &Site) -> PathBuf {
    data().join("out").join(site.id).join("pack")
}

fn event(i: &Input) -> Option<TrackEvent> {
    let t = |x: &chrono::DateTime<chrono::Utc>| x.timestamp_millis();
    Some(match i {
        Input::Fix { .. } => return None,
        Input::Activity { t: x, activity } => TrackEvent::Activity {
            t_ms: t(x),
            activity: match activity {
                Activity::Still => TrackActivity::Still,
                Activity::OnFoot => TrackActivity::OnFoot,
                Activity::InVehicle => TrackActivity::InVehicle,
                Activity::Unknown => TrackActivity::Unknown,
                Activity::SignificantMotion => TrackActivity::SignificantMotion,
            },
        },
        Input::Screen { t: x, on } => TrackEvent::Screen {
            t_ms: t(x),
            on: *on,
        },
        Input::Closed { t: x, ways } => TrackEvent::Closed {
            t_ms: t(x),
            ways: ways.clone(),
        },
        Input::Refuse { t: x } => TrackEvent::Refuse { t_ms: t(x) },
        Input::ProfileAnswer { t: x, accept } => TrackEvent::ProfileAnswer {
            t_ms: t(x),
            accept: *accept,
        },
        Input::Steps { t: x, steps } => TrackEvent::Steps {
            t_ms: t(x),
            steps: *steps,
        },
        Input::Answer { t: x, accept } => TrackEvent::Answer {
            t_ms: t(x),
            accept: *accept,
        },
    })
}

/// What one call gave, as comparable values.
#[derive(Debug, Default, PartialEq)]
struct Said {
    spoken: Vec<String>,
    gps_last: Option<(GpsModeView, i64, i64)>,
    switches: usize,
    proposals: usize,
    profiles: usize,
    finished: bool,
}

fn from_session(b: &BatchOut) -> Said {
    Said {
        spoken: b.spoken.iter().map(|s| s.text.clone()).collect(),
        gps_last: b.gps.map(|g| (g.mode, g.interval_ms, g.max_delay_ms)),
        switches: b.switches.len(),
        proposals: b.proposals.len(),
        profiles: b.profile_proposals.len(),
        finished: b.recap.is_some(),
    }
}

fn gps_key(r: &GpsRequest) -> (GpsModeView, i64, i64) {
    let mode = match r.mode {
        skimap_plan::gps::GpsMode::Idle => GpsModeView::Idle,
        skimap_plan::gps::GpsMode::Pocket => GpsModeView::Pocket,
        skimap_plan::gps::GpsMode::Approach => GpsModeView::Approach,
        skimap_plan::gps::GpsMode::Deviation => GpsModeView::Deviation,
        skimap_plan::gps::GpsMode::Interaction => GpsModeView::Interaction,
        skimap_plan::gps::GpsMode::Pause => GpsModeView::Pause,
        skimap_plan::gps::GpsMode::Refuge => GpsModeView::Refuge,
        skimap_plan::gps::GpsMode::Waking => GpsModeView::Waking,
    };
    (
        mode,
        (r.interval_s * 1000.0).round() as i64,
        (r.max_delay_s * 1000.0).round() as i64,
    )
}

fn from_direct(outs: &[Output]) -> Said {
    let mut s = Said::default();
    for o in outs {
        match o {
            Output::Spoken { spoken, .. } => s.spoken.push(spoken.announcement.text()),
            Output::Gps { request, .. } => s.gps_last = Some(gps_key(request)),
            Output::Switched { .. } | Output::Reverted { .. } => s.switches += 1,
            Output::Proposal { .. } => s.proposals += 1,
            Output::ProfileProposal { .. } => s.profiles += 1,
            Output::Recap { .. } => s.finished = true,
            _ => {}
        }
    }
    s
}

/// Feeds `inputs` to the session the way the phone does (fixes by batches of the GPS
/// delay in force, events alone) and to the core's tracker directly, and checks that each
/// call says the same. Returns the number of calls and the session's results.
fn follow(
    session: &TrackingSession,
    direct: &mut Tracker<'_>,
    inputs: &[Input],
) -> (usize, Vec<BatchOut>) {
    let mut calls = 0;
    let mut results = Vec::new();
    let mut delay_ms: i64 = 0;
    let mut i = 0;
    while i < inputs.len() {
        let mut outs = Vec::new();
        let b = match event(&inputs[i]) {
            Some(e) => {
                direct.push(&inputs[i], &mut outs);
                i += 1;
                session.push_event(e).unwrap()
            }
            None => {
                let first = inputs[i].t().timestamp_millis();
                let mut fixes = Vec::new();
                while i < inputs.len() {
                    let Input::Fix { t, pos } = &inputs[i] else {
                        break;
                    };
                    if t.timestamp_millis() - first > delay_ms && !fixes.is_empty() {
                        break;
                    }
                    fixes.push((t.timestamp_millis(), pos.lon, pos.lat));
                    direct.push(&inputs[i], &mut outs);
                    i += 1;
                }
                session.push_fixes(pack_fixes(&fixes)).unwrap()
            }
        };
        let want = from_direct(&outs);
        let got = from_session(&b);
        assert_eq!(got, want, "call {calls} at input {i}");
        if let Some(g) = b.gps {
            delay_ms = g.max_delay_ms;
        }
        calls += 1;
        results.push(b);
    }
    (calls, results)
}

fn group(children: &[u8]) -> String {
    group_new(
        GroupSpec {
            name: "test".into(),
            adults: 2,
            children: children.to_vec(),
            skating: false,
            level: "intermediate".into(),
        },
        skimap_ffi::card::Lang::Fr,
    )
    .unwrap()
    .model_json
}

/// The morning of today's pack, « Valider la journée » with a family, then a whole outing
/// of that family through the session: identical to the core, restart in the middle,
/// recap, GPX, the group learns.
fn morning_to_recap(site: &'static Site, date: &str, budget_min: f64, return_before: Option<&str>) {
    let work = tempfile::tempdir().unwrap();
    let t0 = Instant::now();
    let planner = open_day_planner(
        pack(site).display().to_string(),
        work.path().join("view").display().to_string(),
        skimap_ffi::card::Lang::Fr,
    )
    .unwrap();
    let start = day_starts(site.id.into(), skimap_ffi::card::Lang::Fr).remove(0);
    let req = DayRequest {
        label: "sortie en famille".into(),
        start,
        date: date.into(),
        start_time: Some("10:00".into()),
        earliest: "08:00".into(),
        budget_min,
        skating: false,
        return_before: return_before.map(str::to_owned),
        max_difficulty: None,
    };
    let fam = group(&[6, 9]);
    // Validated below: the last morning computed, the family's.
    let alone = planner.morning(req.clone(), None).unwrap();
    let m = planner.morning(req, Some(fam.clone())).unwrap();
    println!(
        "{}: layers {} ms, morning {} ms ({} loops), first: {} · {} · {} · {:?}",
        site.id,
        planner.load_ms(),
        m.compute_ms,
        m.loops.len(),
        m.loops[0].label,
        m.loops[0].time_text,
        m.loops[0].back_text,
        m.loops[0].warning
    );
    // The family's time is a range (no outing learned) and longer than the model's.
    assert!(m.loops[0].range, "{:?}", m.loops[0]);
    assert!(
        m.loops[0].time_text.contains(" à "),
        "{}",
        m.loops[0].time_text
    );
    assert!(alone.loops[0].time_text.contains("modèle"));
    assert!(never_safe(&m.never_safe));
    let plan_dir = work.path().join("plan");
    let v = planner
        .validate(0, Some(fam.clone()), plan_dir.display().to_string())
        .unwrap();
    println!(
        "  plan: {} km, départ {}, retour prévu {}, limite {:?}, {} points clés, {} plans B ({} ms from opening)",
        v.length_km,
        v.departure,
        v.expected_back,
        v.limit,
        v.key_points.len(),
        v.plans_b.len(),
        t0.elapsed().as_millis()
    );
    println!("  SMS: {}", v.sms);
    println!("  pace: {:?}", v.pace_text);
    assert!(v.sms.contains("112"), "{}", v.sms);
    assert!(v.pace_text.as_deref().unwrap_or("").contains("a priori"));
    if let Some(r) = return_before {
        assert_eq!(v.limit.as_deref(), Some(r.replace(':', " h ").as_str()));
    }
    assert!(v.line.len() > 20);
    assert_eq!(
        read_day_plan(v.plan_dir.clone(), skimap_ffi::card::Lang::Fr).unwrap(),
        v
    );

    // A family outing on the plan (C11 behaviour), simulated by the core's bench.
    let plan =
        DayPlan::from_json(&std::fs::read_to_string(plan_dir.join("plan.json")).unwrap()).unwrap();
    let trk = std::fs::read(plan_dir.join("plan.trk")).unwrap();
    let g = load_graph(&pack(site)).unwrap();
    let sc = Scenario {
        name: "famille".into(),
        start: scenario::departure_utc(&plan),
        slowdown: 1.0,
        steps: vec![Step::WaitAtStart { secs: 60.0 }],
        sigma_m: 10.0,
        forest: true,
        seed: 11,
        tail_s: 300.0,
        behaviour: Some(Behaviour::family()),
        step_counter: false,
    };
    let run = scenario::run(
        &sc,
        &plan,
        TrackView::new(&trk).unwrap(),
        &g,
        TrackerParams::default(),
    )
    .unwrap();
    let vdir = pack(site).join("voice");
    let manifest = VoiceManifest::read(&vdir).unwrap();
    let voice = Voice::new(&manifest, "pack");
    let mut direct = Tracker::new(
        &plan,
        TrackView::new(&trk).unwrap(),
        &g,
        Some(&voice),
        TrackerParams::default(),
    )
    .unwrap();

    // First half, then the process « dies »: a new session replays the recorded inputs.
    let half = run.inputs.len() / 2;
    let s1 = open_tracking(v.plan_dir.clone(), None, "fr".into()).unwrap();
    assert_eq!(s1.replayed(), 0);
    let ta = Instant::now();
    let (calls1, _) = follow(&s1, &mut direct, &run.inputs[..half]);
    let g1 = s1.guidance().unwrap();
    drop(s1);
    let s2 = open_tracking(v.plan_dir.clone(), None, "fr".into()).unwrap();
    assert_eq!(s2.replayed() as usize, half);
    assert_eq!(s2.guidance().unwrap(), g1, "state restored by the replay");
    let (calls2, results) = follow(&s2, &mut direct, &run.inputs[half..]);
    let el = ta.elapsed();
    let said: usize = results.iter().map(|b| b.spoken.len()).sum();
    let clips: usize = results
        .iter()
        .flat_map(|b| &b.spoken)
        .filter(|s| s.all_clips)
        .count();
    let recap = results.iter().find_map(|b| b.recap.clone()).expect("recap");
    println!(
        "  outing: {} inputs in {} calls, {:.1} ms in all ({:.1} µs per input); 2nd half said {said} ({clips} from the pack's clips); {}",
        run.inputs.len(),
        calls1 + calls2,
        el.as_secs_f64() * 1e3,
        el.as_secs_f64() * 1e6 / run.inputs.len() as f64,
        recap.text
    );
    assert!(recap.arrived);
    let last = s2.guidance().unwrap();
    assert_eq!(last.phase, PhaseView::Finished);
    // Every spoken item of the pack's announcements resolves to a file of the pack.
    for s in results.iter().flat_map(|b| &b.spoken) {
        for c in &s.segments {
            if let Some(f) = &c.file {
                assert!(Path::new(f).is_file(), "{f}");
            }
        }
    }
    let gpx = s2.write_gpx().unwrap();
    let text = std::fs::read_to_string(&gpx).unwrap();
    let pts = text.matches("<trkpt").count();
    assert_eq!(pts, run.fixes, "every fix in the GPX");
    let learned = group_learn(fam, s2.observation().unwrap(), skimap_ffi::card::Lang::Fr).unwrap();
    assert_eq!(learned.outings, 1);
    println!("  GPX {pts} points; group: {}", learned.learned);
}

#[test]
#[ignore = "full-data: needs data/out/<site>/pack"]
fn full_data_morning_to_recap_cervieres() {
    // Pistes closed in November: the only loop of 3 h (status known, pack of 15/11).
    morning_to_recap(&CERVIERES, "2026-11-16", 180.0, None);
}

#[test]
#[ignore = "full-data: needs data/out/<site>/pack"]
fn full_data_morning_to_recap_chapelle() {
    morning_to_recap(&CHAPELLE_DES_BOIS, "2026-11-16", 120.0, Some("16:00"));
}

/// The Q9 scenarios of the archived day (the plan built by the pipeline, the pack's
/// graph and clips): the session says, switches and sets the GPS as the core.
#[test]
#[ignore = "full-data: needs data/out/<site>/ and the archived day"]
fn full_data_q9_through_the_session() {
    let layout = DataLayout::new(data());
    let (mut d, ctx) = presets::load(&layout, &CERVIERES).unwrap();
    let mut req = presets::requests(&CERVIERES, presets::past_day(&CERVIERES)).remove(0);
    req.start_time = NaiveTime::from_hms_opt(10, 0, 0);
    let m = skimap_plan::plan::morning(&d, &req);
    let plan = skimap_plan::plan::build(&mut d, &m, 0, &ctx).unwrap();
    let g = d.graph.take().unwrap();
    let trk = skimap_plan::plan::track(&plan, &g).unwrap().to_bytes();
    let work = tempfile::tempdir().unwrap();
    std::fs::write(work.path().join("plan.json"), plan.to_json().unwrap()).unwrap();
    std::fs::write(work.path().join("plan.trk"), &trk).unwrap();
    std::fs::write(
        work.path().join("pack.txt"),
        pack(&CERVIERES).display().to_string(),
    )
    .unwrap();
    let manifest = VoiceManifest::read(&pack(&CERVIERES).join("voice")).unwrap();
    let voice = Voice::new(&manifest, "pack");
    let refuge = plan
        .key_points
        .iter()
        .find(|k| k.kind == skimap_plan::plan::KeyKind::Refuge)
        .unwrap()
        .s;
    for (name, steps, bh) in [
        (
            "refuge",
            vec![Step::Stop {
                at_s: refuge,
                secs: 1200.0,
            }],
            None,
        ),
        (
            "family",
            vec![Step::WaitAtStart { secs: 60.0 }],
            Some(Behaviour::family()),
        ),
    ] {
        let _ = std::fs::remove_file(work.path().join("inputs.jsonl"));
        let sc = Scenario {
            name: name.into(),
            start: scenario::departure_utc(&plan),
            slowdown: 1.0,
            steps,
            sigma_m: 10.0,
            forest: true,
            seed: 7,
            tail_s: 300.0,
            behaviour: bh,
            step_counter: false,
        };
        let run = scenario::run(
            &sc,
            &plan,
            TrackView::new(&trk).unwrap(),
            &g,
            TrackerParams::default(),
        )
        .unwrap();
        let mut direct = Tracker::new(
            &plan,
            TrackView::new(&trk).unwrap(),
            &g,
            Some(&voice),
            TrackerParams::default(),
        )
        .unwrap();
        let s = open_tracking(work.path().display().to_string(), None, "fr".into()).unwrap();
        let (calls, results) = follow(&s, &mut direct, &run.inputs);
        let modes: std::collections::BTreeSet<String> = results
            .iter()
            .filter_map(|b| b.gps.map(|x| format!("{:?}", x.mode)))
            .collect();
        println!(
            "{name}: {} inputs, {calls} calls ({} batches in the bench), GPS modes {modes:?}",
            run.inputs.len(),
            run.batches
        );
        assert!(modes.contains("Pocket") && modes.contains("Approach"));
        if name == "refuge" {
            // GNSS off at the pause, back on with the significant motion (the refuge is
            // recognised at the same input: the core does not ask for its own mode then).
            assert!(
                modes.contains("Pause") && modes.contains("Waking"),
                "{modes:?}"
            );
        } else {
            assert!(results.iter().any(|b| !b.proposals.is_empty()));
        }
    }
}

/// The phone's batches: fixes of the GPS delay in force with the step reports over the
/// same time, one `push_batch` per batch; other events alone. Checks each call against
/// the core's tracker on the same inputs.
fn follow_batched(
    session: &TrackingSession,
    direct: &mut Tracker<'_>,
    inputs: &[Input],
) -> Vec<BatchOut> {
    let mut results = Vec::new();
    let mut delay_ms: i64 = 0;
    let mut i = 0;
    while i < inputs.len() {
        let mut outs = Vec::new();
        let b = match (&inputs[i], event(&inputs[i])) {
            (Input::Steps { .. } | Input::Fix { .. }, _) => {
                let first = inputs[i].t().timestamp_millis();
                let (mut fixes, mut steps) = (Vec::new(), Vec::new());
                while i < inputs.len() {
                    let t = inputs[i].t().timestamp_millis();
                    if t - first > delay_ms && !(fixes.is_empty() && steps.is_empty()) {
                        break;
                    }
                    match &inputs[i] {
                        Input::Fix { pos, .. } => fixes.push((t, pos.lon, pos.lat)),
                        Input::Steps { steps: n, .. } => {
                            steps.push(StepReport { t_ms: t, steps: *n })
                        }
                        _ => break,
                    }
                    direct.push(&inputs[i], &mut outs);
                    i += 1;
                }
                session.push_batch(pack_fixes(&fixes), steps).unwrap()
            }
            (_, Some(e)) => {
                direct.push(&inputs[i], &mut outs);
                i += 1;
                session.push_event(e).unwrap()
            }
            (_, None) => unreachable!(),
        };
        assert_eq!(from_session(&b), from_direct(&outs), "call at input {i}");
        if let Some(g) = b.gps {
            delay_ms = g.max_delay_ms;
        }
        results.push(b);
    }
    results
}

/// X11 and X10 through the session: Q9-33 (Cervières, 1 h 30 from Les Fonts, a wrong turn
/// at the third junction, the route computed then crosses a zone without any network and
/// a historic avalanche path), the phone counting the steps. `open_tracking` gives the
/// tracker the pack's layers: the new route announces them, as the core with the same
/// layers does call for call; the steps go with the fixes in one call per batch.
#[test]
#[ignore = "full-data: needs data/out/cervieres/ and the archived day"]
fn full_data_rerouted_route_says_the_hazards_through_the_session() {
    let layout = DataLayout::new(data());
    let (mut d, ctx) = presets::load(&layout, &CERVIERES).unwrap();
    let req = presets::requests(&CERVIERES, presets::past_day(&CERVIERES))[1].clone();
    let m = skimap_plan::plan::morning(&d, &req);
    let plan = skimap_plan::plan::build(&mut d, &m, 0, &ctx).unwrap();
    let g = d.graph.take().unwrap();
    let trk = skimap_plan::plan::track(&plan, &g).unwrap().to_bytes();
    let work = tempfile::tempdir().unwrap();
    std::fs::write(work.path().join("plan.json"), plan.to_json().unwrap()).unwrap();
    std::fs::write(work.path().join("plan.trk"), &trk).unwrap();
    std::fs::write(
        work.path().join("pack.txt"),
        pack(&CERVIERES).display().to_string(),
    )
    .unwrap();
    let sc = Scenario {
        name: "q9-33 session".into(),
        start: scenario::departure_utc(&plan),
        slowdown: 1.0,
        steps: vec![
            Step::WaitAtStart { secs: 60.0 },
            Step::WrongTurn {
                junction: 2,
                length_m: 600.0,
                back: false,
            },
        ],
        sigma_m: 10.0,
        forest: true,
        seed: 7,
        tail_s: 300.0,
        behaviour: None,
        step_counter: true,
    };
    let run = scenario::run_with_layers(
        &sc,
        &plan,
        TrackView::new(&trk).unwrap(),
        &g,
        TrackerParams::default(),
        Some(&d),
    )
    .unwrap();
    let n_steps = run
        .inputs
        .iter()
        .filter(|i| matches!(i, Input::Steps { .. }))
        .count();
    assert!(n_steps > 100, "{n_steps} step reports");
    let t0 = Instant::now();
    let s = open_tracking(work.path().display().to_string(), None, "fr".into()).unwrap();
    println!(
        "session opened with the pack's layers in {} ms: {:?}",
        t0.elapsed().as_millis(),
        s.layers()
    );
    assert_eq!(s.layers(), vec!["no_network", "avalanche_paths", "zones"]);
    let pack_layers = load_layers(&pack(&CERVIERES), &work.path().join("direct")).unwrap();
    let manifest = VoiceManifest::read(&pack(&CERVIERES).join("voice")).unwrap();
    let voice = Voice::new(&manifest, "pack");
    let mut direct = Tracker::new(
        &plan,
        TrackView::new(&trk).unwrap(),
        &g,
        Some(&voice),
        TrackerParams::default(),
    )
    .unwrap()
    .with_layers(&pack_layers);
    let results = follow_batched(&s, &mut direct, &run.inputs);
    let kinds: Vec<(String, Vec<String>)> = results
        .iter()
        .flat_map(|b| b.spoken.iter())
        .map(|x| (x.text.clone(), x.kinds.clone()))
        .collect();
    let has = |k: &str| kinds.iter().any(|(_, ks)| ks.iter().any(|x| x == k));
    for (t, k) in &kinds {
        if k.iter()
            .any(|x| x.starts_with("no_network") || x == "avalanche_path")
        {
            println!("{k:?}: {t}");
        }
    }
    println!(
        "{} inputs ({n_steps} step reports) in {} calls",
        run.inputs.len(),
        results.len()
    );
    for k in ["rerouted", "no_network", "avalanche_path", "no_network_end"] {
        assert!(has(k), "{k} not said: {kinds:?}");
    }
    // Said along the route computed on the phone (after « Nouvel itinéraire calculé. »),
    // which only the layers given by `open_tracking` make it say.
    let rr = kinds
        .iter()
        .position(|(_, ks)| ks.iter().any(|x| x == "rerouted"))
        .unwrap();
    let after: Vec<&String> = kinds[rr..].iter().flat_map(|(_, ks)| ks.iter()).collect();
    println!("after the new route: {after:?}");
    for k in ["no_network", "avalanche_path", "no_network_end"] {
        assert!(
            after.iter().any(|x| *x == k),
            "{k} not said on the new route"
        );
    }
    assert!(results.iter().any(|b| b.recap.is_some()));
}

/// « Ramène-moi » in the middle of an outing: the way back along the trace skied, its
/// turns said with the pack's clips, the arrival at the start, the GPS mode of the core's
/// rule on the way back.
#[test]
#[ignore = "full-data: needs data/out/<site>/pack"]
fn full_data_bring_me_back() {
    let work = tempfile::tempdir().unwrap();
    let planner = open_day_planner(
        pack(&CERVIERES).display().to_string(),
        work.path().join("view").display().to_string(),
        skimap_ffi::card::Lang::Fr,
    )
    .unwrap();
    let req = DayRequest {
        label: "3 h".into(),
        start: day_starts("cervieres".into(), skimap_ffi::card::Lang::Fr).remove(0),
        date: "2026-11-16".into(),
        start_time: Some("10:00".into()),
        earliest: "08:00".into(),
        budget_min: 180.0,
        skating: false,
        return_before: None,
        max_difficulty: None,
    };
    planner.morning(req, None).unwrap();
    let dir = work.path().join("plan");
    let v = planner
        .validate(0, None, dir.display().to_string())
        .unwrap();
    let plan =
        DayPlan::from_json(&std::fs::read_to_string(dir.join("plan.json")).unwrap()).unwrap();
    let trk = std::fs::read(dir.join("plan.trk")).unwrap();
    let g = load_graph(&pack(&CERVIERES)).unwrap();
    let sc = Scenario {
        name: "aller".into(),
        start: scenario::departure_utc(&plan),
        slowdown: 1.0,
        steps: vec![Step::WaitAtStart { secs: 60.0 }],
        sigma_m: 10.0,
        forest: false,
        seed: 5,
        tail_s: 0.0,
        behaviour: None,
        step_counter: false,
    };
    let run = scenario::run(
        &sc,
        &plan,
        TrackView::new(&trk).unwrap(),
        &g,
        TrackerParams::default(),
    )
    .unwrap();
    let s = open_tracking(v.plan_dir.clone(), None, "fr".into()).unwrap();
    // The first 40 minutes of the outing.
    let t_start = run.inputs[0].t();
    let out: Vec<(i64, f64, f64)> = run
        .inputs
        .iter()
        .filter_map(|i| match i {
            Input::Fix { t, pos } if (*t - t_start).num_seconds() < 2400 => {
                Some((t.timestamp_millis(), pos.lon, pos.lat))
            }
            _ => None,
        })
        .collect();
    for c in out.chunks(12) {
        s.push_fixes(pack_fixes(c)).unwrap();
    }
    let before = s.guidance().unwrap();
    let t_back = out.last().unwrap().0 + 1000;
    let b = s.back_start(t_back).unwrap();
    assert_eq!(b.guidance.route, "back");
    assert!(
        b.spoken[0].text.starts_with("Il reste"),
        "{}",
        b.spoken[0].text
    );
    println!(
        "back after {:.1} km along the plan: {:.1} km back, {} ; said « {} »",
        before.progress_m / 1000.0,
        b.guidance.remaining_m / 1000.0,
        b.guidance.remaining_text,
        b.spoken[0].text
    );
    assert!(b.guidance.remaining_m > 0.5 * before.progress_m);
    assert!(b.guidance.remaining_m < 1.3 * before.progress_m);
    // Ski back along the way out, at the same pace, one fix every 2 s.
    let mut said = Vec::new();
    let mut modes = std::collections::BTreeSet::new();
    let mut t = t_back;
    let mut arrived = false;
    let back: Vec<_> = out.iter().rev().step_by(2).copied().collect();
    for c in back.chunks(5) {
        let fixes: Vec<_> = c
            .iter()
            .map(|(_, lon, lat)| {
                t += 2000;
                (t, *lon, *lat)
            })
            .collect();
        let r = s.push_fixes(pack_fixes(&fixes)).unwrap();
        said.extend(
            r.spoken
                .iter()
                .map(|x| (x.kinds.clone(), x.text.clone(), x.all_clips)),
        );
        if let Some(gq) = r.gps {
            modes.insert(format!("{:?}", gq.mode));
        }
        if r.guidance.route != "back" {
            arrived = true;
            break;
        }
    }
    for x in &said {
        println!("  {:?}", x);
    }
    assert!(arrived, "arrived at the start");
    assert!(said.iter().any(|x| x.0 == ["arrive"]));
    assert!(
        said.iter().filter(|x| x.0 == ["turn"]).count() >= 1,
        "{said:?}"
    );
    assert!(
        said.iter().filter(|x| x.0 == ["turn"]).all(|x| x.2),
        "turn clips from the pack"
    );
    assert!(
        modes.contains("Approach") || modes.contains("Pocket"),
        "{modes:?}"
    );
}

/// `pack_dir` as the pipeline writes it now: a pack written before P163 (#163) does not
/// carry the record of its site, so the phone plans nothing on it but the evaluation
/// sites; its view under `view` (links to its files, read only) gets the manifest with the
/// record the pack build writes (`skimap_pack::build::starts::record`).
fn with_site_record(pack_dir: &Path, view: &Path) -> PathBuf {
    let text = std::fs::read_to_string(pack_dir.join("manifest.json")).unwrap();
    let mut m: skimap_pack::Manifest = serde_json::from_str(&text).unwrap();
    if m.site.record.is_some() {
        return pack_dir.to_path_buf();
    }
    let layout = DataLayout::new(data());
    let reg = skimap_common::site::Registry::load(&layout).unwrap();
    let Ok(site) = reg.get(&m.site.id) else {
        return pack_dir.to_path_buf();
    };
    m.site.record = Some(skimap_pack::build::starts::record(site, &layout, false));
    std::fs::create_dir_all(view).unwrap();
    for e in std::fs::read_dir(pack_dir).unwrap() {
        let e = e.unwrap();
        if e.file_name() != "manifest.json" {
            std::os::unix::fs::symlink(e.path(), view.join(e.file_name())).unwrap();
        }
    }
    std::fs::write(
        view.join("manifest.json"),
        serde_json::to_string(&m).unwrap(),
    )
    .unwrap();
    view.to_path_buf()
}

/// One « ramène-moi » on the pack in `pack_dir`: 40 minutes of a 3 h plan from its first
/// start, then back along the way out to where the outing started; then the same outing
/// and the arrow of the screen followed to the target. `Err` when no plan
/// could be made there, or when the arrival was not said (`not arrived…`).
fn bring_me_back_on(pack_dir: &Path) -> Result<String, String> {
    use skimap_common::geom::LonLat;
    let work = tempfile::tempdir().unwrap();
    let pack_dir = &with_site_record(pack_dir, &work.path().join("pack"));
    let starts = skimap_ffi::tracking_day::day_pack_starts(
        pack_dir.display().to_string(),
        skimap_ffi::card::Lang::Fr,
    );
    let start = starts.into_iter().next().ok_or("no start")?;
    let planner = open_day_planner(
        pack_dir.display().to_string(),
        work.path().join("view").display().to_string(),
        skimap_ffi::card::Lang::Fr,
    )
    .map_err(|e| format!("planner: {e}"))?;
    let req = DayRequest {
        label: "3 h".into(),
        start,
        date: "2026-11-16".into(),
        start_time: Some("10:00".into()),
        earliest: "08:00".into(),
        budget_min: 180.0,
        skating: false,
        return_before: None,
        max_difficulty: None,
    };
    planner
        .morning(req, None)
        .map_err(|e| format!("morning: {e}"))?;
    let dir = work.path().join("plan");
    let v = planner
        .validate(0, None, dir.display().to_string())
        .map_err(|e| format!("validate: {e}"))?;
    let plan =
        DayPlan::from_json(&std::fs::read_to_string(dir.join("plan.json")).unwrap()).unwrap();
    let trk = std::fs::read(dir.join("plan.trk")).unwrap();
    let g = load_graph(pack_dir).unwrap();
    let sc = Scenario {
        name: "aller".into(),
        start: scenario::departure_utc(&plan),
        slowdown: 1.0,
        steps: vec![Step::WaitAtStart { secs: 60.0 }],
        sigma_m: 10.0,
        forest: false,
        seed: 5,
        tail_s: 0.0,
        behaviour: None,
        step_counter: false,
    };
    let run = scenario::run(
        &sc,
        &plan,
        TrackView::new(&trk).unwrap(),
        &g,
        TrackerParams::default(),
    )
    .unwrap();
    let s = open_tracking(v.plan_dir.clone(), None, "fr".into()).unwrap();
    let t_start = run.inputs[0].t();
    let out: Vec<(i64, f64, f64)> = run
        .inputs
        .iter()
        .filter_map(|i| match i {
            Input::Fix { t, pos } if (*t - t_start).num_seconds() < 2400 => {
                Some((t.timestamp_millis(), pos.lon, pos.lat))
            }
            _ => None,
        })
        .collect();
    for c in out.chunks(12) {
        s.push_fixes(pack_fixes(c)).unwrap();
    }
    let before = s.guidance().unwrap();
    let t_back = out.last().unwrap().0 + 1000;
    let b = s.back_start(t_back).unwrap();
    assert_eq!(b.guidance.route, "back", "{}", pack_dir.display());
    let view = b.guidance.back.clone().expect("way back");
    let target = LonLat::new(view.target_lon, view.target_lat);
    let pos = |x: &(i64, f64, f64)| LonLat::new(x.1, x.2);
    let first = pos(&out[0]);
    let asked = pos(out.last().unwrap()).haversine_m(target);
    // Ski back along the way out, one fix every 2 s.
    let mut t = t_back;
    let mut arrived_at = None;
    let mut left = (b.guidance.remaining_m, false, 0);
    let mut turns = 0;
    let back: Vec<_> = out.iter().rev().step_by(2).copied().collect();
    for c in back.chunks(5) {
        let fixes: Vec<_> = c
            .iter()
            .map(|(_, lon, lat)| {
                t += 2000;
                (t, *lon, *lat)
            })
            .collect();
        let r = s.push_fixes(pack_fixes(&fixes)).unwrap();
        turns += r.spoken.iter().filter(|x| x.kinds == ["turn"]).count();
        left = (
            r.guidance.remaining_m,
            r.guidance.back.as_ref().is_some_and(|x| x.off_way),
            left.2 + r.spoken.iter().filter(|x| x.kinds == ["off_route"]).count(),
        );
        if r.guidance.route != "back" {
            assert!(
                r.spoken.iter().any(|x| x.kinds == ["arrive"]),
                "{}: back over without the arrival said",
                pack_dir.display()
            );
            arrived_at = fixes.last().map(pos);
            break;
        }
    }
    let at = arrived_at.ok_or_else(|| {
        format!(
            "not arrived: target {} {:?} {:.0} m from the first fix, way {}, {:.0} m left at \
             the first fix (off the way: {}, said {} times)",
            view.target,
            view.target_name,
            target.haversine_m(first),
            view.way,
            left.0,
            left.1,
            left.2
        )
    })?;
    // The same outing again, then the arrow of the screen followed (the next useful point
    // of the way, 6 m a fix every 2 s): it leads to the target.
    let s = open_tracking(v.plan_dir.clone(), None, "fr".into()).unwrap();
    for c in out.chunks(12) {
        s.push_fixes(pack_fixes(c)).unwrap();
    }
    let b2 = s.back_start(t_back).unwrap();
    let mut g = b2.guidance.back.clone().expect("way back");
    let (mut p, mut t, mut skied) = (pos(out.last().unwrap()), t_back, 0.0);
    let mut followed = None;
    let mut off = 0;
    let limit = 3.0 * b2.guidance.remaining_m + 1000.0;
    while followed.is_none() && skied < limit {
        let mut fixes = Vec::new();
        for _ in 0..5 {
            let to = LonLat::new(g.guide_lon, g.guide_lat);
            let d = p.haversine_m(to);
            let f = if d > 6.0 { 6.0 / d } else { 1.0 };
            p = LonLat::new(p.lon + f * (to.lon - p.lon), p.lat + f * (to.lat - p.lat));
            skied += d.min(6.0);
            t += 2000;
            fixes.push((t, p.lon, p.lat));
        }
        let r = s.push_fixes(pack_fixes(&fixes)).unwrap();
        off += r.spoken.iter().filter(|x| x.kinds == ["off_route"]).count();
        match r.guidance.back {
            Some(x) if r.guidance.route == "back" => g = x,
            _ => {
                assert!(r.spoken.iter().any(|x| x.kinds == ["arrive"]));
                followed = Some(p);
            }
        }
    }
    let end = followed.ok_or_else(|| {
        format!(
            "not arrived: the arrow followed for {skied:.0} m, {:.0} m from the target",
            p.haversine_m(target)
        )
    })?;
    let row = format!(
        "{:>4.1} km out, back {:>4.1} km by {:<12} to the {:<5} ({:>3.0} m from the first fix, \
         {asked:>5.0} m away when asked); the way out skied back: arrived {:>3.0} m from the \
         first fix, {:>3.0} m from the target, {turns} turns said; the arrow followed: arrived \
         after {:>4.1} km, {:>3.0} m from the target, {off} times off the way",
        before.progress_m / 1000.0,
        b.guidance.remaining_m / 1000.0,
        view.way,
        view.target,
        target.haversine_m(first),
        at.haversine_m(first),
        at.haversine_m(target),
        skied / 1000.0,
        end.haversine_m(target),
    );
    assert!(
        end.haversine_m(target) <= 60.0 || end.haversine_m(first) <= 60.0,
        "{}: {row}",
        pack_dir.display()
    );
    assert_eq!(off, 0, "{}: {row}", pack_dir.display());
    // Arrived where the outing started (where the car was recorded) or at the target.
    assert!(
        at.haversine_m(first) <= 60.0 || at.haversine_m(target) <= 60.0,
        "{}: {row}",
        pack_dir.display()
    );
    Ok(row)
}

/// « Ramène-moi » on every pack of the data: the way back from 40 minutes out leads to
/// the car (or to where the outing started) and the arrival is said there.
#[test]
#[ignore = "full-data: needs data/out/<site>/pack"]
fn full_data_bring_me_back_on_every_pack() {
    let mut packs: Vec<PathBuf> = std::fs::read_dir(data().join("out"))
        .unwrap()
        .filter_map(Result::ok)
        .map(|e| e.path().join("pack"))
        .filter(|p| p.join("manifest.json").is_file())
        .collect();
    packs.sort();
    let (mut ok, mut failed, mut no_plan) = (0, Vec::new(), Vec::new());
    for p in &packs {
        let name = p
            .parent()
            .and_then(Path::file_name)
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        match std::panic::catch_unwind(|| bring_me_back_on(p)) {
            Ok(Ok(row)) => {
                ok += 1;
                println!("{name:<44} {row}");
            }
            Ok(Err(e)) if e.starts_with("not arrived") => {
                println!("{name:<44} FAILED {e}");
                failed.push(name);
            }
            Ok(Err(e)) => {
                println!("{name:<44} no plan: {e}");
                no_plan.push(name);
            }
            Err(_) => {
                println!("{name:<44} FAILED (panic)");
                failed.push(name);
            }
        }
    }
    println!(
        "{} packs: {ok} brought back, {} failed, {} without a plan",
        packs.len(),
        failed.len(),
        no_plan.len()
    );
    assert!(failed.is_empty(), "{failed:?}");
    assert!(ok >= 10, "{ok} packs brought back only");
}

/// One outing of a group on a loop of `model_min` minutes of model time, `slow` times the
/// model's pace when moving, four stops of five minutes: the observation the tracking
/// session writes at the end (`TrackingSession::observation`).
fn outing_observation(model_min: f64, length_km: f64, slow: f64) -> String {
    let model = model_min * 60.0;
    let model_s = [0.4 * model, 0.35 * model, 0.25 * model];
    let moving_s = model_s.map(|s| s * slow);
    let stops_s = vec![300.0; 4];
    let elapsed_s = moving_s.iter().sum::<f64>() + stops_s.iter().sum::<f64>();
    serde_json::json!({
        "model_s": model_s,
        "moving_s": moving_s,
        "progress_m": length_km * 1000.0,
        "travelled_m": length_km * 1050.0,
        "stops_s": stops_s,
        "elapsed_s": elapsed_s,
    })
    .to_string()
}

/// « 2 h 35 » or « 45 min », minutes.
fn minutes(s: &str) -> f64 {
    match s.split_once(" h ") {
        Some((h, m)) => h.parse::<f64>().unwrap() * 60.0 + m.parse::<f64>().unwrap(),
        None => s.trim_end_matches(" min").parse().unwrap(),
    }
}

/// The range and the median of the group's time in the validated plan's time
/// (« … (3 sorties apprises) : 1 h 05 à 2 h 10, médiane 1 h 30, prévu sur … »), minutes.
fn range_and_median(pace: &str) -> (f64, f64, f64) {
    let rest = pace.split_once(") : ").expect("the group's time").1;
    let (range, rest) = rest
        .split_once(", médiane ")
        .expect("a range and its median");
    let (lo, hi) = range.split_once(" à ").expect("a range");
    let median = rest.split(',').next().unwrap();
    (minutes(lo), minutes(hi), minutes(median))
}

/// V2 (#65), « calibration visible après 3 sorties »: the app's default family learns its
/// outings one by one (`group_learn`, as the app does at the end of each outing). After
/// each one the group says how many it learned, the morning says it for its loops, and the
/// time of the same loop (the first of the morning a priori, validated with the group)
/// narrows around what the outings showed; after the third, « 3 sorties apprises », the
/// range much narrower than a priori, the median at the pace the outings showed. It stays
/// a range (no figure « environ » yet): with three outings its 80 % interval is still
/// wider than ±15 % (`pace::FIGURE_SPREAD`), so no figure is claimed.
fn calibration_visible_after_three_outings(site: &'static Site, date: &str, budget_min: f64) {
    let work = tempfile::tempdir().unwrap();
    let lang = skimap_ffi::card::Lang::Fr;
    let planner = open_day_planner(
        pack(site).display().to_string(),
        work.path().join("view").display().to_string(),
        lang,
    )
    .unwrap();
    let start = day_starts(site.id.into(), lang).remove(0);
    let req = DayRequest {
        label: "sortie en famille".into(),
        start,
        date: date.into(),
        start_time: Some("10:00".into()),
        earliest: "08:00".into(),
        budget_min,
        skating: false,
        return_before: None,
        max_difficulty: None,
    };
    let mut g = group_default(lang).unwrap();
    assert_eq!(g.learned, "a priori, aucune sortie apprise");
    let prior_json = g.model_json.clone();
    // The time of the first loop of the morning a priori, validated with `group`.
    let time_of = |group: &str, n: u32| {
        let m = planner
            .morning(req.clone(), Some(prior_json.clone()))
            .unwrap();
        let v = planner
            .validate(
                0,
                Some(group.to_owned()),
                work.path().join(format!("plan-{n}")).display().to_string(),
            )
            .unwrap();
        (m.loops[0].clone(), v.pace_text.unwrap_or_default())
    };
    let (l0, pace0) = time_of(&prior_json, 0);
    println!(
        "{} · {}: {:.1} km, model {:.0} min; a priori: {pace0}",
        site.id, l0.label, l0.length_km, l0.model_min
    );
    assert!(l0.range, "{l0:?}");
    assert!(
        pace0.contains("(a priori, aucune sortie apprise)"),
        "{pace0}"
    );
    let (lo0, hi0, _) = range_and_median(&pace0);
    // The family is half again as slow as the model when moving, every outing alike, with
    // four stops of 5 min.
    let slow = 1.5;
    let shown = l0.model_min.mul_add(slow, 20.0);
    let mut width = hi0 - lo0;
    let mut median = 0.0;
    for n in 1..=3u32 {
        g = group_learn(
            g.model_json,
            outing_observation(l0.model_min, l0.length_km, slow),
            lang,
        )
        .unwrap();
        assert_eq!(g.outings, n);
        // The morning for the group says what it learned.
        let m = planner
            .morning(req.clone(), Some(g.model_json.clone()))
            .unwrap();
        assert!(
            m.loops[0].reasons[0].contains(&g.learned),
            "{}: {:?}",
            g.learned,
            m.loops[0].reasons
        );
        let (l, pace) = time_of(&g.model_json, n);
        assert_eq!(l.label, l0.label);
        let (lo, hi, med) = range_and_median(&pace);
        println!("  {}: {pace}", g.learned);
        assert!(pace.contains(&format!("({})", g.learned)), "{pace}");
        assert!(hi - lo < width, "the range narrows: {pace}");
        assert!(lo <= shown && shown <= hi, "{shown} min in {pace}");
        width = hi - lo;
        median = med;
    }
    assert_eq!(g.learned, "3 sorties apprises");
    println!(
        "  width {:.0} min a priori, {width:.0} min after 3 outings; outings {shown:.0} min, median {median:.0} min",
        hi0 - lo0
    );
    assert!(width <= 0.65 * (hi0 - lo0), "{width} vs {}", hi0 - lo0);
    assert!(
        (median - shown).abs() <= 0.1 * shown,
        "median {median} min, outings {shown:.0} min"
    );
    assert!(
        0.5 * width / median > skimap_plan::pace::FIGURE_SPREAD,
        "a figure would be due at ±{:.0} %",
        50.0 * width / median
    );
}

#[test]
#[ignore = "full-data: needs data/out/<site>/pack"]
fn full_data_calibration_visible_after_three_outings_cervieres() {
    calibration_visible_after_three_outings(&CERVIERES, "2026-11-16", 180.0);
}

#[test]
#[ignore = "full-data: needs data/out/<site>/pack"]
fn full_data_calibration_visible_after_three_outings_chapelle() {
    calibration_visible_after_three_outings(&CHAPELLE_DES_BOIS, "2026-11-16", 120.0);
}
