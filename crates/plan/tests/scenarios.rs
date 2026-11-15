//! Q9 (#109): outings replayed on the real graphs of both sites with a simulated clock
//! (`skimap_plan::scenario`), expected announcements, alerts, switches and decisions of
//! the outing in order, with their time windows.
//!
//! Reads `data/out/<site>/` (pipeline outputs and archived past days, `skimap history`):
//! fails if absent. To be pointed at `fixtures/` (Q1, #101) once it is merged.
//!
//! ```sh
//! cargo test --release -p skimap-plan --test scenarios -- --nocapture
//! ```

use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};

use chrono::NaiveTime;
use skimap_assist::inputs::SiteData;
use skimap_common::layout::DataLayout;
use skimap_common::site::{CERVIERES, CHAPELLE_DES_BOIS, Site};
use skimap_core::calibration::{self, GroupProfile};
use skimap_core::graph::PisteGraph;
use skimap_core::routing::Style;
use skimap_plan::gps::GpsMode;
use skimap_plan::pace::{self, PaceReason, ProposalKind};
use skimap_plan::phrases::never_safe;
use skimap_plan::plan::{self, DayPlan, PlanRequest};
use skimap_plan::presets;
use skimap_plan::scenario::{self, Behaviour, Run, Scenario, Step};
use skimap_plan::session::{End, SessionEvent};
use skimap_plan::speech::Priority;
use skimap_plan::track::TrackView;
use skimap_plan::tracker::{Alert, Cause, Condition, Input, Output, RouteRef, TrackerParams};

struct Built {
    plan: DayPlan,
    track: Vec<u8>,
    graph: PisteGraph,
    /// Layers of the day (without the graph) and the inputs of the plan (voice of the
    /// pack).
    layers: Arc<(SiteData, plan::Context)>,
}

fn layout() -> DataLayout {
    let root = std::env::var("SKIMAP_DATA_DIR")
        .unwrap_or_else(|_| format!("{}/../../data", env!("CARGO_MANIFEST_DIR")));
    let l = DataLayout::new(root);
    for s in [&CERVIERES, &CHAPELLE_DES_BOIS] {
        let g = l.out_dir(s).join("routes/piste_graph.json");
        assert!(
            g.is_file(),
            "données absentes : {} (sorties du pipeline des deux sites, ou fixtures/ une fois Q1 fusionné)",
            g.display()
        );
    }
    l
}

/// The plan of a request (built once per process).
fn built(site: &'static Site, key: &str, edit: impl FnOnce(&mut PlanRequest)) -> Arc<Built> {
    static CACHE: OnceLock<Mutex<HashMap<String, Arc<Built>>>> = OnceLock::new();
    let cache = CACHE.get_or_init(|| Mutex::new(HashMap::new()));
    let id = format!("{}/{key}", site.id);
    if let Some(b) = cache.lock().unwrap().get(&id) {
        return b.clone();
    }
    let l = layout();
    let (mut data, ctx) = presets::load(&l, site).expect("layers");
    let mut req = presets::requests(site, presets::past_day(site))[0].clone();
    edit(&mut req);
    let m = plan::morning(&data, &req);
    assert!(
        !m.suggestions.suggestions.is_empty(),
        "{id}: no loop: {:?}",
        m.notes
    );
    let p = plan::build(&mut data, &m, 0, &ctx).expect("plan");
    let graph = data.graph.take().expect("graph");
    let track = plan::track(&p, &graph).expect("track").to_bytes();
    let b = Arc::new(Built {
        plan: p,
        track,
        graph,
        layers: Arc::new((data, ctx)),
    });
    cache.lock().unwrap().insert(id, b.clone());
    b
}

fn cervieres() -> Arc<Built> {
    built(&CERVIERES, "3h", |_| {})
}

fn chapelle() -> Arc<Built> {
    built(&CHAPELLE_DES_BOIS, "2h", |_| {})
}

fn scen(b: &Built, name: &str, steps: Vec<Step>) -> Scenario {
    Scenario {
        name: name.into(),
        start: scenario::departure_utc(&b.plan),
        slowdown: 1.0,
        steps,
        sigma_m: 10.0,
        forest: true,
        seed: 7,
        tail_s: 300.0,
        behaviour: None,
        step_counter: false,
    }
}

fn play(b: &Built, sc: &Scenario, params: TrackerParams) -> Run {
    let view = TrackView::new(&b.track).expect("track");
    let run = scenario::run(sc, &b.plan, view, &b.graph, params.clone()).expect("run");
    println!("== {} ({} fixes, {} lots)", sc.name, run.fixes, run.batches);
    for l in scenario::timeline(&run) {
        println!("{l}");
    }
    common_checks(b, &run, &params);
    run
}

/// Checks of every scenario: safety never lost, never « sans danger », deterministic.
fn common_checks(b: &Built, run: &Run, params: &TrackerParams) {
    for o in &run.outputs {
        if let Output::Spoken { spoken, .. } = o {
            let text = spoken.announcement.text();
            assert!(never_safe(&text), "{}: {text}", run.scenario);
        }
        if let Output::Recap { recap, .. } = o {
            assert!(never_safe(&recap.text));
        }
    }
    // Replay: same outputs.
    let view = TrackView::new(&b.track).unwrap();
    let again = scenario::replay(&run.inputs, &b.plan, view, &b.graph, params.clone()).unwrap();
    assert_eq!(again, run.outputs, "{}: not deterministic", run.scenario);
}

#[derive(Debug, Clone)]
enum M {
    Started,
    Paused,
    Refuge,
    Resumed,
    NearFinish,
    Finished(End),
    /// A spoken group with this item kind or clip id.
    Said(&'static str),
    Alert(Alert),
    /// Switched for a cause (`closure`, `late`, `conditions`, `deviation`, `no_time`).
    Switched(&'static str),
    Reverted(bool),
    Gps(GpsMode),
    /// A proposal (C11), of this kind if given.
    Proposal(Option<ProposalKind>),
    /// The answer to a proposal: accepted and applied (true) or refused.
    Answered(bool),
}

fn cause_tag(c: &Cause) -> &'static str {
    match c {
        Cause::Closure { .. } => "closure",
        Cause::Late { .. } => "late",
        Cause::Conditions(_) => "conditions",
        Cause::Deviation => "deviation",
        Cause::NoTime => "no_time",
        Cause::Pace => "pace",
    }
}

fn is(o: &Output, m: &M) -> bool {
    match (o, m) {
        (Output::Session { event, .. }, M::Started) => {
            matches!(event, SessionEvent::Started { .. })
        }
        (Output::Session { event, .. }, M::Paused) => matches!(event, SessionEvent::Paused { .. }),
        (Output::Session { event, .. }, M::Refuge) => matches!(event, SessionEvent::Refuge { .. }),
        (Output::Session { event, .. }, M::Resumed) => {
            matches!(event, SessionEvent::Resumed { .. })
        }
        (Output::Session { event, .. }, M::NearFinish) => {
            matches!(event, SessionEvent::NearFinish { .. })
        }
        (Output::Session { event, .. }, M::Finished(e)) => {
            matches!(event, SessionEvent::Finished { end, .. } if end == e)
        }
        (Output::Spoken { spoken, .. }, M::Said(k)) => {
            spoken.items.iter().any(|c| c.kind == *k) || spoken.announcement.clip_ids().contains(k)
        }
        (Output::Alert { alert, .. }, M::Alert(a)) => alert == a,
        (Output::Switched { cause, .. }, M::Switched(c)) => cause_tag(cause) == *c,
        (Output::Reverted { implicit, .. }, M::Reverted(i)) => implicit == i,
        (Output::Gps { request, .. }, M::Gps(g)) => request.mode == *g,
        (Output::Proposal { proposal, .. }, M::Proposal(k)) => k.is_none_or(|k| proposal.kind == k),
        (
            Output::Answered {
                accept, applied, ..
            },
            M::Answered(a),
        ) => (*accept && *applied) == *a,
        _ => false,
    }
}

/// Expected outputs in order (a subsequence), each in a window of seconds from the start
/// of the run; returns the times found.
fn expect(run: &Run, want: &[(M, f64, f64)]) -> Vec<f64> {
    let mut found = Vec::new();
    let mut from = 0;
    for (m, lo, hi) in want {
        let k = run.outputs[from..]
            .iter()
            .position(|o| is(o, m))
            .unwrap_or_else(|| panic!("{}: {m:?} not found after output {from}", run.scenario));
        let o = &run.outputs[from + k];
        let s = event_time(run, o);
        assert!(
            (*lo..=*hi).contains(&s),
            "{}: {m:?} at {s:.0} s, expected in [{lo}, {hi}]",
            run.scenario
        );
        found.push(s);
        from += k + 1;
    }
    found
}

/// Seconds from the start of a run: the time a session decision refers to (the start of
/// the movement, of the pause…), else the time of the output.
fn event_time(run: &Run, o: &Output) -> f64 {
    let t0 = run.t0.timestamp() as f64;
    match o {
        Output::Session { event, .. } => match event {
            SessionEvent::Started { t, .. }
            | SessionEvent::Paused { t }
            | SessionEvent::Refuge { t, .. }
            | SessionEvent::Resumed { t, .. }
            | SessionEvent::NearFinish { t }
            | SessionEvent::Finished { t, .. } => t - t0,
        },
        _ => scenario::rel(run, o.t()),
    }
}

fn never(run: &Run, ms: &[M]) {
    for m in ms {
        if let Some(o) = run.outputs.iter().find(|o| is(o, m)) {
            panic!("{}: unexpected {m:?}: {o:?}", run.scenario);
        }
    }
}

fn count(run: &Run, m: &M) -> usize {
    run.outputs.iter().filter(|o| is(o, m)).count()
}

/// Spoken groups per hour of outing, and those with a safety part.
fn rate(run: &Run) -> (f64, usize) {
    let spoken: Vec<&Output> = run
        .outputs
        .iter()
        .filter(|o| matches!(o, Output::Spoken { .. }))
        .collect();
    let hours = (scenario::rel(run, run.t1) / 3600.0).max(0.1);
    let safety = spoken
        .iter()
        .filter(|o| matches!(o, Output::Spoken { spoken, .. } if spoken.items.iter().any(|c| c.priority == Priority::Safety)))
        .count();
    (spoken.len() as f64 / hours, safety)
}

fn no_manipulation(run: &Run) {
    assert!(
        run.inputs.iter().all(|i| matches!(
            i,
            Input::Fix { .. } | Input::Activity { .. } | Input::Steps { .. } | Input::Closed { .. }
        )),
        "{}: an input needs the skier",
        run.scenario
    );
}

/// When the skier starts moving (from the simulation's truth), seconds.
fn t_move(run: &Run) -> f64 {
    run.truth
        .iter()
        .find(|x| x.1 > 0.5)
        .map_or(0.0, |x| x.0 - 10.0)
}

/// Checks that the start was detected within 60 s of the first movement on skis.
fn started_on_time(run: &Run) -> f64 {
    let tm = t_move(run);
    let s = expect(run, &[(M::Started, tm, tm + 60.0)])[0];
    println!(
        "{} : premier mouvement {tm:.0} s, départ décidé daté de {s:.0} s",
        run.scenario
    );
    s
}

fn model_s(b: &Built) -> f64 {
    b.plan.route.model_min * 60.0
}

// ---------------------------------------------------------------------------------------
// Normal outings: nothing to touch, silence when all goes well.

fn normal(b: &Built, name: &str) -> Run {
    let sc = scen(b, name, vec![Step::WaitAtStart { secs: 120.0 }]);
    let run = play(b, &sc, TrackerParams::default());
    no_manipulation(&run);
    let total = model_s(b);
    let turns = b
        .plan
        .announcements
        .iter()
        .filter(|a| a.kind == "turn")
        .count();
    let t0 = started_on_time(&run);
    let t = expect(
        &run,
        &[
            (M::Started, t0, t0),
            (M::Said("loop_summary"), t0, t0 + 120.0),
            (M::NearFinish, t0 + 0.8 * total, t0 + total + 60.0),
            (M::Said("arrive"), t0 + 0.8 * total, t0 + total + 70.0),
            (
                M::Finished(End::Arrived),
                t0 + total - 60.0,
                t0 + total + 300.0,
            ),
        ],
    );
    never(
        &run,
        &[
            M::Alert(Alert::OffRoute),
            M::Switched("closure"),
            M::Switched("deviation"),
            M::Switched("late"),
            M::Proposal(None),
        ],
    );
    let said_turns = count(&run, &M::Said("turn"));
    assert!(
        said_turns + 1 >= turns,
        "{name}: {said_turns} turns said of {turns}"
    );
    let (per_hour, _) = rate(&run);
    println!(
        "{name}: départ {:.0} s, arrivée {:.0} s, {said_turns} virages dits sur {} carrefours ({} évidents), {per_hour:.1} annonces / h",
        t[0],
        t[4],
        b.plan.junctions.len(),
        b.plan
            .junctions
            .iter()
            .filter(|j| j.direction.is_none())
            .count()
    );
    run
}

#[test]
fn q9_01_cervieres_normal() {
    let b = cervieres();
    normal(&b, "Cervières, sortie normale");
}

#[test]
fn q9_02_chapelle_normal() {
    let b = chapelle();
    normal(&b, "Chapelle-des-Bois, sortie normale");
}

#[test]
fn q9_03_cervieres_normal_harsh_gps() {
    // σ 15 m and forest outliers, another seed: still nothing to touch, no false alert.
    let b = cervieres();
    let mut sc = scen(
        &b,
        "Cervières, GPS dégradé (σ 15 m, forêt)",
        vec![Step::WaitAtStart { secs: 60.0 }],
    );
    sc.sigma_m = 15.0;
    sc.seed = 99;
    let run = play(&b, &sc, TrackerParams::default());
    no_manipulation(&run);
    let t0 = started_on_time(&run);
    expect(
        &run,
        &[(
            M::Finished(End::Arrived),
            t0 + model_s(&b) - 60.0,
            t0 + model_s(&b) + 400.0,
        )],
    );
    never(&run, &[M::Alert(Alert::OffRoute)]);
}

#[test]
fn q9_04_cervieres_arrival_by_car() {
    // Drives 3 km to the start, waits 2 min, skis: the start is the first movement on skis.
    let b = cervieres();
    let sc = scen(
        &b,
        "Cervières, arrivée en voiture",
        vec![Step::ArriveByCar { from_m: 3000.0 }],
    );
    let run = play(&b, &sc, TrackerParams::default());
    no_manipulation(&run);
    // 200 s of driving, 120 s of wait: never started in the car.
    let t0 = started_on_time(&run);
    assert!(t0 >= 320.0, "{t0}");
}

// ---------------------------------------------------------------------------------------
// Deviation.

#[test]
fn q9_05_cervieres_wrong_turn_and_back() {
    let b = cervieres();
    let j = b
        .plan
        .junctions
        .iter()
        .position(|j| j.s > 1000.0 && j.branches > 0)
        .expect("junction");
    // 250 m on the wrong branch and back; the new route would come after 10 min only
    // (user setting), so that the way back is the skier's own.
    let sc = scen(
        &b,
        "Cervières, écart puis retour",
        vec![Step::WrongTurn {
            junction: j,
            length_m: 250.0,
            back: true,
        }],
    );
    let p = TrackerParams {
        reroute_after_s: 600.0,
        ..TrackerParams::default()
    };
    let run = play(&b, &sc, p);
    no_manipulation(&run);
    expect(
        &run,
        &[
            (M::Alert(Alert::OffRoute), 0.0, 1e6),
            (M::Said("off_route"), 0.0, 1e6),
            (M::Alert(Alert::BackOnRoute), 0.0, 1e6),
            (M::Finished(End::Arrived), 0.0, 1e6),
        ],
    );
    let off = run
        .outputs
        .iter()
        .find(|o| is(o, &M::Alert(Alert::OffRoute)))
        .unwrap();
    let back = run
        .outputs
        .iter()
        .find(|o| is(o, &M::Alert(Alert::BackOnRoute)))
        .unwrap();
    // 250 m out and back at the model pace: off for a few minutes at most.
    assert!((back.t() - off.t()).num_seconds() < 600);
    assert_eq!(count(&run, &M::Alert(Alert::OffRoute)), 1);
    never(&run, &[M::Switched("deviation")]);
}

#[test]
fn q9_06_chapelle_wrong_turn_rerouted() {
    let b = chapelle();
    let j = b
        .plan
        .junctions
        .iter()
        .position(|j| j.s > 1000.0 && j.branches > 0)
        .expect("junction");
    let sc = scen(
        &b,
        "Chapelle-des-Bois, écart puis nouvel itinéraire",
        vec![Step::WrongTurn {
            junction: j,
            length_m: 3000.0,
            back: false,
        }],
    );
    let run = play(&b, &sc, TrackerParams::default());
    no_manipulation(&run);
    let t = expect(
        &run,
        &[
            (M::Alert(Alert::OffRoute), 0.0, 1e6),
            (M::Switched("deviation"), 0.0, 1e6),
            (M::Said("rerouted"), 0.0, 1e6),
            (M::Finished(End::Arrived), 0.0, 1e6),
        ],
    );
    // New route at least 60 s after the alert (TrackerParams::reroute_after_s).
    assert!(t[1] - t[0] >= 60.0 - 1e-9, "{t:?}");
}

/// Spoken kinds after an instant, in order.
fn said_after(run: &Run, t: f64) -> Vec<(f64, &'static str, Priority)> {
    let mut v = Vec::new();
    for o in &run.outputs {
        let s = scenario::rel(run, o.t());
        if let Output::Spoken { spoken, .. } = o
            && s >= t
        {
            for c in &spoken.items {
                for k in [
                    "no_network",
                    "no_network_end",
                    "avalanche_path",
                    "protected_zone",
                ] {
                    if c.kind == k {
                        v.push((s, k, c.priority));
                    }
                }
            }
        }
    }
    v
}

#[test]
fn q9_33_cervieres_rerouted_route_says_the_hazards() {
    // X11: Cervières, 1 h 30 from Les Fonts at 14 h 30; a wrong turn at the third junction
    // and skiing on: the route computed then crosses a zone without any network and a
    // historic avalanche path. With the layers of the day (the pack's), it announces them
    // as the plan's routes do (the same function); without, as before, nothing.
    let b = built(&CERVIERES, "1h30", |r| {
        *r = presets::requests(&CERVIERES, presets::past_day(&CERVIERES))[1].clone()
    });
    let sc = scen(
        &b,
        "Cervières, écart puis nouvel itinéraire à travers une zone sans réseau et un couloir",
        vec![
            Step::WaitAtStart { secs: 60.0 },
            Step::WrongTurn {
                junction: 2,
                length_m: 600.0,
                back: false,
            },
        ],
    );
    let params = TrackerParams::default();
    let view = TrackView::new(&b.track).expect("track");
    let run = scenario::run_with_layers(
        &sc,
        &b.plan,
        view,
        &b.graph,
        params.clone(),
        Some(&b.layers.0),
    )
    .expect("run");
    println!("== {} ({} fixes, {} lots)", sc.name, run.fixes, run.batches);
    for l in scenario::timeline(&run) {
        println!("{l}");
    }
    // Deterministic with the same layers.
    let again = scenario::replay_with_layers(
        &run.inputs,
        &b.plan,
        view,
        &b.graph,
        params.clone(),
        Some(&b.layers.0),
    )
    .unwrap();
    assert_eq!(again, run.outputs);
    no_manipulation(&run);
    let t = expect(
        &run,
        &[
            (M::Switched("deviation"), 0.0, 1e6),
            (M::Said("rerouted"), 0.0, 1e6),
            (M::Said("no_network"), 0.0, 1e6),
            (M::Said("avalanche_path"), 0.0, 1e6),
            (M::Said("no_network_end"), 0.0, 1e6),
            (M::Finished(End::Arrived), 0.0, 1e6),
        ],
    );
    let live = run.outputs.iter().find_map(|o| match o {
        Output::Switched {
            to: RouteRef::Live(n),
            cause: Cause::Deviation,
            ..
        } => Some(*n),
        _ => None,
    });
    assert!(live.is_some(), "a route computed on the phone");
    let after = said_after(&run, t[0]);
    println!(
        "{} : après le nouvel itinéraire ({:.0} s) : {after:?}",
        run.scenario, t[0]
    );
    assert!(
        after
            .iter()
            .filter(|(_, k, _)| *k != "no_network_end")
            .all(|(_, _, p)| *p == Priority::Safety),
        "safety announcements: {after:?}"
    );
    for o in &run.outputs {
        if let Output::Spoken { spoken, .. } = o {
            assert!(never_safe(&spoken.announcement.text()));
        }
    }
    // The same outing without the layers: the same new route, nothing said of them (the
    // bug of X11).
    let bare = scenario::run(&sc, &b.plan, view, &b.graph, params).expect("run");
    let tb = expect(&bare, &[(M::Switched("deviation"), t[0], t[0])])[0];
    assert!(
        said_after(&bare, tb).is_empty(),
        "{:?}",
        said_after(&bare, tb)
    );
}

#[test]
fn x11_protected_zones_of_a_route_said_by_the_common_function() {
    // The Chapelle-des-Bois loop of 2 h crosses the winter quiet zone of the capercaillie:
    // told in the morning for the loop (A7), not along it; a route that crosses it without
    // the loop (`zones_told` without it) says it at its entry, with its rule, clips of the
    // pack.
    let b = chapelle();
    let c = skimap_plan::course::Course::new(&b.graph, &b.plan.route.legs, &b.plan.eta).unwrap();
    let zones = plan::protected_zones(
        c.line().points(),
        (0.0, f64::INFINITY),
        &b.layers.0,
        b.plan.date,
    );
    println!("{zones:?}");
    let z = zones.first().expect("a protected zone on the loop");
    assert!(z.name.contains("tétras"), "{}", z.name);
    let anns = |told: &std::collections::BTreeSet<String>| {
        let h = plan::Hazards {
            data: &b.layers.0,
            date: b.plan.date,
            zones_told: Some(told),
            zones_range: (0.0, f64::INFINITY),
        };
        let j = skimap_plan::guidance::junctions(&b.graph, &c);
        plan::announcements(
            &b.graph,
            &c,
            &j,
            b.layers.1.voice.as_ref(),
            Some(&h),
            b.plan.departure,
        )
    };
    let all: std::collections::BTreeSet<String> = zones.iter().map(|z| z.id.clone()).collect();
    assert!(anns(&all).iter().all(|a| a.kind != "protected_zone"));
    // The plan's loop: none along it (said in the morning).
    assert!(
        b.plan
            .announcements
            .iter()
            .all(|a| a.kind != "protected_zone")
    );
    let none = std::collections::BTreeSet::new();
    let said: Vec<_> = anns(&none)
        .into_iter()
        .filter(|a| a.kind == "protected_zone")
        .collect();
    assert_eq!(said.len(), zones.len(), "{said:?}");
    let a = &said[0];
    println!("{:?} : « {} »", a.trigger, a.text);
    assert!(
        matches!(a.trigger, plan::Trigger::At { s, .. } if (s - (z.runs[0].0 - plan::ZONE_LEAD_M).max(0.0)).abs() < 1e-6)
    );
    assert!(
        a.text.contains("tétras") && never_safe(&a.text),
        "{}",
        a.text
    );
    let mut check = skimap_plan::phrases::ClipCheck::default();
    b.layers
        .1
        .voice
        .as_ref()
        .expect("voice of the pack")
        .check(&a.announcement, &mut check);
    assert!(check.missing.is_empty(), "{check:?}");
}

// ---------------------------------------------------------------------------------------
// Closed trails.

/// A detour of the plan that comes back to the loop, its ways and divergence.
fn detour(b: &Built) -> (usize, Vec<i64>, f64) {
    b.plan
        .plans_b
        .iter()
        .enumerate()
        .filter(|(_, x)| x.fallback.exists() && x.fallback.s_div > 60.0)
        .find_map(|(i, x)| match &x.fallback.kind {
            skimap_plan::fallback::FallbackKind::Detour { ways, .. } => {
                Some((i, ways.clone(), x.fallback.s_div))
            }
            _ => None,
        })
        .expect("a detour after 60 m")
}

fn closure(b: &Built, name: &str, extra: Vec<Step>) -> (Run, usize) {
    let (i, ways, s_div) = detour(b);
    let mut steps = vec![Step::Close {
        at_s: (s_div - 800.0).max(s_div * 0.3),
        ways,
    }];
    steps.extend(extra);
    let sc = scen(b, name, steps);
    (play(b, &sc, TrackerParams::default()), i)
}

#[test]
fn q9_07_cervieres_closed_trail() {
    let b = cervieres();
    let (run, i) = closure(&b, "Cervières, piste fermée", vec![]);
    expect(
        &run,
        &[
            (M::Alert(Alert::Closed), 0.0, 1e6),
            (M::Switched("closure"), 0.0, 1e6),
            (M::Said("fix.plan-closed-ahead"), 0.0, 1e6),
            (M::Finished(End::Arrived), 0.0, 1e6),
        ],
    );
    let to = run.outputs.iter().find_map(|o| match o {
        Output::Switched { to, .. } => Some(*to),
        _ => None,
    });
    assert_eq!(to, Some(RouteRef::PlanB(i)), "the plan B of the morning");
    never(
        &run,
        &[
            M::Alert(Alert::OffRoute),
            M::Reverted(true),
            M::Reverted(false),
        ],
    );
}

#[test]
fn q9_08_chapelle_closed_trail() {
    let b = chapelle();
    let (run, i) = closure(&b, "Chapelle-des-Bois, piste fermée", vec![]);
    expect(
        &run,
        &[
            (M::Switched("closure"), 0.0, 1e6),
            (M::Said("fix.plan-closed-ahead"), 0.0, 1e6),
            (M::Finished(End::Arrived), 0.0, 1e6),
        ],
    );
    assert!(
        run.outputs
            .iter()
            .any(|o| matches!(o, Output::Switched { to: RouteRef::PlanB(k), .. } if *k == i))
    );
    never(&run, &[M::Alert(Alert::OffRoute)]);
}

#[test]
fn q9_09_chapelle_closed_trail_refused() {
    let b = chapelle();
    let (run, _) = closure(
        &b,
        "Chapelle-des-Bois, piste fermée, refus d'un geste",
        vec![Step::RefuseNext { after_s: 20.0 }],
    );
    let t = expect(
        &run,
        &[
            (M::Switched("closure"), 0.0, 1e6),
            (M::Reverted(false), 0.0, 1e6),
            (M::Said("fix.plan-kept"), 0.0, 1e6),
            (M::Finished(End::Arrived), 0.0, 1e6),
        ],
    );
    assert!(
        (t[1] - t[0] - 20.0).abs() <= 1.0,
        "refusal 20 s after the switch: {t:?}"
    );
    assert_eq!(
        count(&run, &M::Switched("closure")),
        1,
        "not proposed again"
    );
}

#[test]
fn q9_10_chapelle_closed_trail_ignored() {
    let b = chapelle();
    let (run, _) = closure(
        &b,
        "Chapelle-des-Bois, piste fermée, refus implicite",
        vec![Step::IgnoreNext],
    );
    expect(
        &run,
        &[
            (M::Switched("closure"), 0.0, 1e6),
            (M::Reverted(true), 0.0, 1e6),
            (M::Said("fix.plan-kept"), 0.0, 1e6),
            (M::Finished(End::Arrived), 0.0, 1e6),
        ],
    );
    assert_eq!(count(&run, &M::Switched("closure")), 1);
    no_manipulation(&run);
}

// ---------------------------------------------------------------------------------------
// Late.

#[test]
fn q9_11_chapelle_late_for_the_return_time() {
    // 2 h skating from 13:30, back before 16 h, skiing at 1/1.4 of the model pace.
    let b = built(&CHAPELLE_DES_BOIS, "2h-13h30", |r| {
        r.start_time = NaiveTime::from_hms_opt(13, 30, 0)
    });
    let mut sc = scen(
        &b,
        "Chapelle-des-Bois, retard sur l'heure de retour",
        vec![],
    );
    sc.slowdown = 1.4;
    let run = play(&b, &sc, TrackerParams::default());
    no_manipulation(&run);
    expect(
        &run,
        &[
            (M::Alert(Alert::TurnaroundAdvanced), 1200.0, 1e6),
            (M::Switched("late"), 1200.0, 1e6),
            (M::Said("fix.plan-shortened"), 1200.0, 1e6),
            (M::Finished(End::Arrived), 0.0, 1e6),
        ],
    );
    let fin = run
        .outputs
        .iter()
        .find(|o| is(o, &M::Finished(End::Arrived)))
        .unwrap();
    let back = scenario::local(fin.t());
    let rb = b.plan.turnaround.return_before.unwrap();
    assert!(
        back <= rb + chrono::Duration::minutes(10),
        "back at {back}, asked {rb}"
    );
}

#[test]
fn q9_12_cervieres_late_before_night() {
    // 3 h classic from 13 h at half the model pace: the C7 deadline (17:13) would be missed;
    // the exit of the morning still ahead (1.7 km) shortens the loop.
    let b = built(&CERVIERES, "3h-13h", |r| {
        r.start_time = NaiveTime::from_hms_opt(13, 0, 0)
    });
    let mut sc = scen(
        &b,
        "Cervières, retard avant la nuit, boucle écourtée",
        vec![],
    );
    sc.slowdown = 2.0;
    let run = play(&b, &sc, TrackerParams::default());
    no_manipulation(&run);
    expect(
        &run,
        &[
            (M::Alert(Alert::TurnaroundAdvanced), 1200.0, 1e6),
            (M::Switched("late"), 1200.0, 1e6),
            (M::Said("fix.plan-shortened"), 1200.0, 1e6),
            (M::Finished(End::Arrived), 0.0, 1e6),
        ],
    );
    let fin = run
        .outputs
        .iter()
        .find(|o| is(o, &M::Finished(End::Arrived)))
        .unwrap();
    let dl = b.plan.turnaround.deadline.unwrap();
    assert!(
        scenario::local(fin.t()) <= dl,
        "back {} after {dl}",
        scenario::local(fin.t())
    );
    never(&run, &[M::Said("turnaround_advanced")]);
}

#[test]
fn q9_22_cervieres_late_start() {
    // The 13 h plan started at 14 h: the loop no longer ends before the C7 deadline (17:13)
    // even at the model pace; shortened at the departure.
    let b = built(&CERVIERES, "3h-13h", |r| {
        r.start_time = NaiveTime::from_hms_opt(13, 0, 0)
    });
    let mut sc = scen(&b, "Cervières, départ en retard", vec![]);
    sc.start = scenario::start_at(&b.plan, 14, 0);
    let run = play(&b, &sc, TrackerParams::default());
    no_manipulation(&run);
    let t0 = started_on_time(&run);
    expect(
        &run,
        &[
            (M::Alert(Alert::TurnaroundAdvanced), t0, t0 + 120.0),
            (M::Switched("late"), t0, t0 + 120.0),
            (M::Said("fix.plan-shortened"), t0, t0 + 130.0),
            (M::Finished(End::Arrived), 0.0, 1e6),
        ],
    );
}

#[test]
fn q9_23_cervieres_no_time_left() {
    // Started late and refusing the shortened loop, at 1/1.3 of the model pace: « turn
    // around now » (safety) when no other way ends before the deadline, and the way home.
    let b = built(&CERVIERES, "3h-13h", |r| {
        r.start_time = NaiveTime::from_hms_opt(13, 0, 0)
    });
    let mut sc = scen(
        &b,
        "Cervières, plus le temps : rentrer",
        vec![Step::RefuseNext { after_s: 15.0 }],
    );
    sc.start = scenario::start_at(&b.plan, 14, 0);
    sc.slowdown = 1.3;
    let run = play(&b, &sc, TrackerParams::default());
    expect(
        &run,
        &[
            (M::Switched("late"), 0.0, 1e6),
            (M::Reverted(false), 0.0, 1e6),
            (M::Alert(Alert::TurnaroundNow), 1200.0, 1e6),
            (M::Switched("no_time"), 1200.0, 1e6),
            (M::Finished(End::Arrived), 0.0, 1e6),
        ],
    );
    // « Turn around now » is safety: said at the minute it is raised.
    let a = run
        .outputs
        .iter()
        .find(|o| is(o, &M::Alert(Alert::TurnaroundNow)))
        .unwrap();
    let s = run
        .outputs
        .iter()
        .find(|o| matches!(o, Output::Spoken { spoken, .. } if spoken.items.iter().any(|c| c.key == "turnaround_now")))
        .expect("said");
    assert_eq!(a.t(), s.t());
    let fin = run
        .outputs
        .iter()
        .find(|o| is(o, &M::Finished(End::Arrived)))
        .unwrap();
    let dl = b.plan.turnaround.deadline.unwrap();
    println!("retour {}, limite C7 {dl}", scenario::local(fin.t()));
    assert!(scenario::local(fin.t()) <= dl + chrono::Duration::minutes(30));
}

// ---------------------------------------------------------------------------------------
// Conditions.

fn with_weather(b: &Built, f: impl Fn(&mut skimap_assist::layers::WeatherHour)) -> Built {
    let mut plan = b.plan.clone();
    let w = plan.weather.as_mut().expect("weather of the day");
    let from = plan.departure + chrono::Duration::minutes(60);
    for h in &mut w.hours {
        if h.time >= from - chrono::Duration::minutes(59) {
            f(h);
        }
    }
    Built {
        plan,
        track: b.track.clone(),
        graph: b.graph.clone(),
        layers: b.layers.clone(),
    }
}

#[test]
fn q9_13_chapelle_fog_takes_the_shortcut() {
    let b = with_weather(&chapelle(), |h| {
        if h.time.time() >= NaiveTime::from_hms_opt(9, 0, 0).unwrap() {
            h.fog = Some(true);
        }
    });
    let sc = scen(&b, "Chapelle-des-Bois, brouillard", vec![]);
    let run = play(&b, &sc, TrackerParams::default());
    no_manipulation(&run);
    let t = expect(
        &run,
        &[
            (M::Alert(Alert::Condition(Condition::Fog)), 600.0, 3600.0),
            (M::Switched("conditions"), 600.0, 3600.0),
            (M::Said("Fog"), 600.0, 3600.0),
            (M::Finished(End::Arrived), 0.0, 1e6),
        ],
    );
    assert!(t[2] - t[0] < 1e-9, "safety at once");
    // The fog and the shortcut in one group, the fog first.
    let g = run
        .outputs
        .iter()
        .find_map(|o| match o {
            Output::Spoken { spoken, .. } if spoken.items.iter().any(|c| c.kind == "Fog") => {
                Some(spoken.clone())
            }
            _ => None,
        })
        .unwrap();
    let ids = g.announcement.clip_ids();
    assert_eq!(ids.first(), Some(&"fix.fog-coming"), "{ids:?}");
    assert!(ids.contains(&"fix.plan-shortcut"), "{ids:?}");
    let fin = t[3];
    assert!(
        fin < model_s(&b),
        "back before the end of the loop: {fin} s"
    );
}

#[test]
fn q9_14_cervieres_wind_rising() {
    let b = with_weather(&cervieres(), |h| {
        h.gust_kmh = Some(60.0);
        h.wind_kmh = Some(35.0);
    });
    let sc = scen(&b, "Cervières, le vent se lève", vec![]);
    let run = play(&b, &sc, TrackerParams::default());
    no_manipulation(&run);
    expect(
        &run,
        &[
            (
                M::Alert(Alert::Condition(Condition::WindRising)),
                600.0,
                3600.0,
            ),
            (M::Said("WindRising"), 600.0, 3600.0),
            (M::Finished(End::Arrived), 0.0, 1e6),
        ],
    );
    assert_eq!(
        count(&run, &M::Alert(Alert::Condition(Condition::WindRising))),
        1,
        "said once"
    );
}

// ---------------------------------------------------------------------------------------
// Pauses, refuge, abandon.

#[test]
fn q9_15_cervieres_pause_at_the_refuge() {
    let b = cervieres();
    let r = b.plan.refuges.first().expect("a refuge near the loop");
    let s = b
        .plan
        .key_points
        .iter()
        .find(|k| k.kind == plan::KeyKind::Refuge)
        .map(|k| k.s)
        .unwrap();
    // Stops on the trail next to the refuge for 20 min.
    let sc = scen(
        &b,
        "Cervières, pause au refuge",
        vec![Step::Stop {
            at_s: s,
            secs: 1200.0,
        }],
    );
    let run = play(&b, &sc, TrackerParams::default());
    no_manipulation(&run);
    let t = expect(
        &run,
        &[
            (M::Paused, 0.0, 1e6),
            (M::Gps(GpsMode::Pause), 0.0, 1e6),
            (M::Refuge, 0.0, 1e6),
            (M::Resumed, 0.0, 1e6),
            (M::Finished(End::Arrived), 0.0, 1e6),
        ],
    );
    println!(
        "refuge {:?} : pause {:.0} s, refuge {:.0} s, reprise {:.0} s",
        r.name, t[0], t[2], t[3]
    );
    // Pause after 90 s at a stop, refuge 5 min into it, resumed after 20 min.
    let paused = run.outputs.iter().find_map(|o| match o {
        Output::Session {
            event: SessionEvent::Paused { t },
            ..
        } => Some(*t),
        _ => None,
    });
    let refuge = run.outputs.iter().find_map(|o| match o {
        Output::Session {
            event: SessionEvent::Refuge { t, .. },
            ..
        } => Some(*t),
        _ => None,
    });
    assert!((refuge.unwrap() - paused.unwrap() - 300.0).abs() < 1e-6);
    // Resumed within 90 s of the end of the 20 min stop (40 m to leave the pause).
    assert!((t[3] - t[0] - 1200.0).abs() < 90.0, "{t:?}");
    // Nothing but safety while paused.
    let (p0, p1) = (t[0], t[3]);
    for o in &run.outputs {
        if let Output::Spoken { spoken, .. } = o {
            let s = scenario::rel(&run, o.t());
            if s > p0 + 90.0 && s < p1 {
                assert!(
                    spoken.items.iter().all(|c| c.priority == Priority::Safety),
                    "said in the pause: {spoken:?}"
                );
            }
        }
    }
}

/// The steepest stretch of 300 m in the first `within_m` metres of the plan's loop (its
/// start, metres along: a late skier is offered a shorter loop).
fn steepest(b: &Built, within_m: f64) -> f64 {
    let c = skimap_plan::course::Course::new(&b.graph, &b.plan.route.legs, &b.plan.eta).unwrap();
    let p = c.profile();
    let mut best = (0.0, 0.0);
    for i in 0..p.dist_m.len() {
        let j = p.dist_m.partition_point(|d| *d < p.dist_m[i] + 300.0);
        if j >= p.dist_m.len() || p.dist_m[j] / c.profile_at(1.0) > within_m {
            break;
        }
        // Slower out than back: climbing.
        let r = (p.out_s[j] - p.out_s[i]) / (p.back_s[j] - p.back_s[i]).max(1.0);
        if r > best.1 {
            best = (p.dist_m[i] / c.profile_at(1.0), r);
        }
    }
    best.0
}

/// Pauses of a run before its end (start, resumed), seconds from the start.
fn pauses(run: &Run) -> Vec<(f64, f64)> {
    let t0 = run.t0.timestamp() as f64;
    let mut v: Vec<(f64, f64)> = Vec::new();
    let mut fin = f64::INFINITY;
    for o in &run.outputs {
        if let Output::Session { event, .. } = o {
            match event {
                SessionEvent::Paused { t } => v.push((t - t0, f64::INFINITY)),
                SessionEvent::Resumed { t, .. } => {
                    if let Some(p) = v.last_mut() {
                        p.1 = t - t0;
                    }
                }
                SessionEvent::Finished { t, .. } => fin = fin.min(t - t0),
                _ => {}
            }
        }
    }
    v.retain(|p| p.0 < fin - 1.0);
    v
}

/// Speeds of the skier along its course over 60 s while moving (the truth), m/s.
fn speeds(run: &Run) -> Vec<f64> {
    let mut v: Vec<f64> = run
        .truth
        .windows(7)
        .map(|w| (w[6].1 - w[0].1) / (w[6].0 - w[0].0))
        .filter(|v| *v > 0.05 && *v < 10.0)
        .collect();
    v.sort_by(f64::total_cmp);
    v
}

/// A slow skier (the C5 pace divided by `slowdown`: climbs at 0.3 to 0.5 m/s, hairpins of
/// the Izoard road at Cervières) stopping 3 min on the steepest climb: with the steps
/// counted by the phone (X10), the one pause is that stop; with the fixes alone, the
/// thresholds of a climb (180 s under 0.08 m/s) against those of before (90 s under 0.15
/// m/s).
fn slow_climb(b: &Built, name: &str, slowdown: f64, within_m: f64) {
    let at_s = steepest(b, within_m);
    let mut counts = Vec::new();
    for (seed, sigma) in [(7u64, 10.0), (99, 15.0), (11, 10.0)] {
        let mut sc = scen(
            b,
            &format!("{name}, montée lente, arrêt de 3 min (σ {sigma} m)"),
            vec![
                Step::WaitAtStart { secs: 60.0 },
                Step::Stop { at_s, secs: 180.0 },
            ],
        );
        sc.slowdown = slowdown;
        sc.seed = seed;
        sc.sigma_m = sigma;
        sc.step_counter = true;
        let run = play(b, &sc, TrackerParams::default());
        no_manipulation(&run);
        let v = speeds(&run);
        let q = |f: f64| v[((v.len() - 1) as f64 * f) as usize];
        let slow = v.iter().filter(|x| (0.3..=0.5).contains(*x)).count() as f64 / v.len() as f64;
        let (stop_t, stop_s) = run.stops[0];
        let ps = pauses(&run);
        println!(
            "{} : vitesse 10 / 50 / 90 % {:.2} / {:.2} / {:.2} m/s, {:.0} % du temps en mouvement de 0,3 à 0,5 m/s ; arrêt de {stop_s:.0} s à {stop_t:.0} s ({at_s:.0} m) ; pauses {ps:?}",
            run.scenario,
            q(0.1),
            q(0.5),
            q(0.9),
            slow * 100.0
        );
        assert!(slow >= 0.25, "{}: {slow}", run.scenario);
        // With the steps: the stop, and nothing else.
        assert_eq!(ps.len(), 1, "{}: {ps:?}", run.scenario);
        let (p0, p1) = ps[0];
        assert!(
            (p0 - stop_t).abs() <= 40.0 && (p1 - (stop_t + stop_s)).abs() <= 90.0,
            "{}: pause {ps:?}, stop at {stop_t} for {stop_s}",
            run.scenario
        );
        // The fixes alone, now and before X10 (the thresholds of the flat on climbs).
        sc.step_counter = false;
        let view = TrackView::new(&b.track).unwrap();
        let gps = scenario::run(&sc, &b.plan, view, &b.graph, TrackerParams::default()).unwrap();
        let mut before = TrackerParams::default();
        before.session.climb_pause_s = before.session.pause_s;
        before.session.climb_pause_fix_ms = before.session.pause_fix_ms;
        let old = scenario::run(&sc, &b.plan, view, &b.graph, before).unwrap();
        let false_of = |r: &Run| {
            let (st, sd) = r.stops[0];
            pauses(r)
                .iter()
                .filter(|p| !(p.0 < st + sd + 30.0 && p.1 > st - 30.0))
                .count()
        };
        counts.push((false_of(&gps), false_of(&old)));
    }
    let (now, old): (usize, usize) = counts.iter().fold((0, 0), |a, c| (a.0 + c.0, a.1 + c.1));
    println!(
        "{name} : fausses pauses avec les seules positions {now} (avant X10 : {old}) ; avec les pas 0"
    );
    assert!(now < old, "{now} vs {old}");
}

#[test]
fn q9_34_cervieres_slow_climb_no_false_pause() {
    let b = cervieres();
    // Before the divergence of the shortened loop it is offered at the start (late).
    slow_climb(&b, "Cervières", 2.0, 1500.0);
}

#[test]
fn q9_35_chapelle_slow_climb_no_false_pause() {
    let b = chapelle();
    slow_climb(&b, "Chapelle-des-Bois", 3.0, 2500.0);
}

#[test]
fn q9_16_chapelle_short_pause() {
    let b = chapelle();
    let sc = scen(
        &b,
        "Chapelle-des-Bois, pause de 8 min",
        vec![Step::Stop {
            at_s: 5000.0,
            secs: 480.0,
        }],
    );
    let run = play(&b, &sc, TrackerParams::default());
    no_manipulation(&run);
    let t = expect(
        &run,
        &[
            (M::Paused, 0.0, 1e6),
            (M::Gps(GpsMode::Pause), 0.0, 1e6),
            (M::Resumed, 0.0, 1e6),
            (M::Finished(End::Arrived), 0.0, 1e6),
        ],
    );
    assert!((t[2] - t[0] - 480.0).abs() < 60.0, "{t:?}");
    never(&run, &[M::Refuge, M::Alert(Alert::OffRoute)]);
}

#[test]
fn q9_17_cervieres_abandon_by_car() {
    let b = cervieres();
    let sc = scen(
        &b,
        "Cervières, abandon en voiture",
        vec![Step::Vehicle { at_s: 3000.0 }],
    );
    let run = play(&b, &sc, TrackerParams::default());
    started_on_time(&run);
    expect(&run, &[(M::Finished(End::Abandoned), 0.0, 1e6)]);
    let recap = run
        .outputs
        .iter()
        .find_map(|o| match o {
            Output::Recap { recap, .. } => Some(recap.clone()),
            _ => None,
        })
        .expect("recap");
    assert!(recap.done_share < 0.4, "{recap:?}");
    never(&run, &[M::NearFinish]);
}

#[test]
fn q9_18_chapelle_abandon_on_foot() {
    let b = chapelle();
    let sc = scen(
        &b,
        "Chapelle-des-Bois, demi-tour et abandon",
        vec![Step::TurnBack { at_s: 2500.0 }],
    );
    let run = play(&b, &sc, TrackerParams::default());
    no_manipulation(&run);
    expect(
        &run,
        &[
            (M::NearFinish, 0.0, 1e6),
            (M::Finished(End::Arrived), 0.0, 1e6),
        ],
    );
    let recap = run
        .outputs
        .iter()
        .find_map(|o| match o {
            Output::Recap { recap, .. } => Some(recap.clone()),
            _ => None,
        })
        .unwrap();
    assert!(recap.done_share < 0.4, "{recap:?}");
    assert!(
        recap.distance_km > 3.5 && recap.distance_km < 7.0,
        "{recap:?}"
    );
}

// ---------------------------------------------------------------------------------------
// GPS gap, rate, GPS modes.

#[test]
fn q9_19_chapelle_gps_gap() {
    let b = chapelle();
    let sc = scen(
        &b,
        "Chapelle-des-Bois, 4 min sans GPS",
        vec![Step::Gap {
            at_s: 4000.0,
            secs: 240.0,
        }],
    );
    let run = play(&b, &sc, TrackerParams::default());
    no_manipulation(&run);
    expect(&run, &[(M::Finished(End::Arrived), 0.0, 1e6)]);
    never(&run, &[M::Alert(Alert::OffRoute)]);
}

#[test]
fn q9_20_chapelle_hard_rate_cap_keeps_safety() {
    // 2 groups an hour at most, directions included: safety (zones without network) is
    // still said every time.
    let b = chapelle();
    let mut p = TrackerParams::default();
    p.speech.max_per_hour = 2;
    p.speech.direction_exempt = false;
    let sc = scen(&b, "Chapelle-des-Bois, débit plafonné à 2 / h", vec![]);
    let run = play(&b, &sc, p);
    let (per_hour, safety) = rate(&run);
    let zones = b
        .plan
        .announcements
        .iter()
        .filter(|a| a.priority == Priority::Safety)
        .count();
    assert!(
        safety >= zones,
        "{safety} safety groups for {zones} safety announcements"
    );
    let non_safety: Vec<f64> = run
        .outputs
        .iter()
        .filter_map(|o| match o {
            Output::Spoken { spoken, t } if spoken.counted => Some(scenario::rel(&run, *t)),
            _ => None,
        })
        .collect();
    for (i, t0) in non_safety.iter().enumerate() {
        assert!(
            non_safety[i..]
                .iter()
                .take_while(|t| **t < t0 + 3600.0)
                .count()
                <= 2
        );
    }
    println!("{per_hour:.1} groupes / h, dont {safety} avec une part de sécurité");
}

#[test]
fn q9_21_gps_modes_follow_the_decision_points() {
    let b = chapelle();
    let sc = scen(&b, "Chapelle-des-Bois, modes GPS", vec![]);
    let run = play(&b, &sc, TrackerParams::default());
    let modes: Vec<GpsMode> = run
        .outputs
        .iter()
        .filter_map(|o| match o {
            Output::Gps { request, .. } => Some(request.mode),
            _ => None,
        })
        .collect();
    assert!(
        modes.contains(&GpsMode::Pocket) && modes.contains(&GpsMode::Approach),
        "{modes:?}"
    );
    // Every turn said is said in approach mode (1 s fixes).
    let mut mode = GpsMode::Idle;
    for o in &run.outputs {
        match o {
            Output::Gps { request, .. } => mode = request.mode,
            Output::Spoken { spoken, .. } if spoken.items.iter().any(|c| c.kind == "turn") => {
                assert!(
                    matches!(mode, GpsMode::Approach | GpsMode::Deviation),
                    "turn said in {mode:?}"
                );
            }
            _ => {}
        }
    }
    // Fewer fixes than one a second.
    let secs = scenario::rel(&run, run.t1);
    println!(
        "{} fixes en {secs:.0} s ({:.2} / s), {} lots",
        run.fixes,
        run.fixes as f64 / secs,
        run.batches
    );
    assert!((run.fixes as f64) < 0.6 * secs);
}

// ---------------------------------------------------------------------------------------
// C11 (#130): the pace of the group, proposals never imposed.

fn group(b: &Built, name: &str, bh: Behaviour, steps: Vec<Step>) -> Run {
    let mut sc = scen(b, name, steps);
    sc.behaviour = Some(bh);
    // The phone counts the steps (X10): a child's way back and forth is moving.
    sc.step_counter = true;
    play(b, &sc, TrackerParams::default())
}

fn proposals(run: &Run) -> Vec<(f64, pace::Proposal)> {
    run.outputs
        .iter()
        .filter_map(|o| match o {
            Output::Proposal { t, proposal } => {
                Some((scenario::rel(run, *t), (**proposal).clone()))
            }
            _ => None,
        })
        .collect()
}

/// Rules of every proposal: at most 3, 30 min apart, not before 20 min of outing plus
/// 10 min of lasting delay, never « sans danger », said as schedule (not safety).
fn proposal_rules(run: &Run, t0: f64) -> Vec<(f64, pace::Proposal)> {
    proposal_rules_from(run, t0, 1200.0)
}

/// [`proposal_rules`], none before `min_elapsed` seconds of outing (15 min with children).
fn proposal_rules_from(run: &Run, t0: f64, min_elapsed: f64) -> Vec<(f64, pace::Proposal)> {
    let ps = proposals(run);
    assert!(ps.len() <= 3, "{}: {} proposals", run.scenario, ps.len());
    for w in ps.windows(2) {
        assert!(
            w[1].0 - w[0].0 >= 1800.0 - 1.0,
            "{}: {} then {}",
            run.scenario,
            w[0].0,
            w[1].0
        );
    }
    for (t, p) in &ps {
        assert!(*t - t0 >= min_elapsed, "{}: proposal at {t}", run.scenario);
        assert!(never_safe(&p.text) && never_safe(&p.announcement.text()));
        println!("{} : proposition à {:.0} s : {}", run.scenario, t, p.text);
    }
    for o in &run.outputs {
        if let Output::Spoken { spoken, .. } = o
            && spoken.items.iter().any(|c| c.kind == "proposal")
        {
            assert!(
                spoken
                    .items
                    .iter()
                    .filter(|c| c.kind == "proposal")
                    .all(|c| c.priority == Priority::Schedule)
            );
        }
    }
    ps
}

fn arrival_rel(run: &Run) -> f64 {
    let o = run
        .outputs
        .iter()
        .find(|o| is(o, &M::Finished(End::Arrived)))
        .unwrap_or_else(|| panic!("{}: not arrived", run.scenario));
    event_time(run, o)
}

#[test]
fn q9_24_cervieres_family_on_an_adult_plan() {
    // A family (children of 6 and 9) on the 3 h loop planned for an adult, from 10 h: about
    // half the C5 pace, 4 stops an hour, detours. Proposed, not imposed: nobody answers,
    // the loop goes on.
    let b = built(&CERVIERES, "3h-10h", |r| {
        r.start_time = NaiveTime::from_hms_opt(10, 0, 0)
    });
    let run = group(
        &b,
        "Cervières, famille sur un plan d'adulte, sans réponse",
        Behaviour::family(),
        vec![Step::WaitAtStart { secs: 60.0 }],
    );
    no_manipulation(&run);
    let t0 = group_started(&run);
    expect(
        &run,
        &[
            (M::Started, t0, t0),
            (M::Proposal(None), t0 + 1800.0, t0 + 3600.0),
            (M::Said("proposal"), t0 + 1800.0, t0 + 4500.0),
        ],
    );
    let ps = proposal_rules(&run, t0);
    let p = &ps[0].1;
    assert!(
        matches!(p.reason, PaceReason::Slower | PaceReason::SlowerAndPauses),
        "{:?}",
        p.reason
    );
    assert!(p.slower_pct >= 30.0, "{}", p.slower_pct);
    assert!(p.route.is_some(), "an exit of the morning: {p:?}");
    assert!(p.arrival < p.loop_arrival);
    // Not answered: no switch for the pace.
    never(&run, &[M::Switched("pace"), M::Answered(true)]);
}

#[test]
fn q9_25_chapelle_family_accepts_the_short_loop() {
    let b = chapelle();
    let run = group(
        &b,
        "Chapelle-des-Bois, famille, proposition acceptée",
        Behaviour::family(),
        vec![Step::Answer {
            accept: true,
            after_s: 20.0,
        }],
    );
    let t0 = group_started(&run);
    let ps = proposal_rules(&run, t0);
    let (tp, p) = ps.first().cloned().expect("a proposal");
    expect(
        &run,
        &[
            (M::Proposal(None), tp, tp),
            (M::Switched("pace"), tp + 20.0, tp + 81.0),
            (M::Answered(true), tp + 20.0, tp + 81.0),
            (M::Said("proposal_accepted"), tp + 20.0, tp + 100.0),
            (M::Finished(End::Arrived), tp, 1e6),
        ],
    );
    let to = run.outputs.iter().find_map(|o| match o {
        Output::Switched {
            to,
            cause: Cause::Pace,
            ..
        } => Some(*to),
        _ => None,
    });
    assert_eq!(to, p.route, "the exit proposed");
    // Back well before the loop would have been, about when proposed: the plan has no
    // group (a lone adult expected), the stops still to come are those of an adult (the
    // statistics over many days: example `pace_eval`).
    let back = run.t0 + chrono::Duration::seconds(arrival_rel(&run) as i64);
    let back = scenario::local(back);
    println!(
        "proposé {} ({}–{}), la boucle {}, arrivée {}",
        p.arrival.format("%H:%M"),
        p.arrival_lo.format("%H:%M"),
        p.arrival_hi.format("%H:%M"),
        p.loop_arrival.format("%H:%M"),
        back.format("%H:%M")
    );
    assert!(back < p.loop_arrival - chrono::Duration::minutes(60));
    assert!((back - p.arrival).num_minutes().abs() <= 30);
    // Accepted: nothing else proposed, no other switch.
    assert_eq!(ps.len(), 1);
    never(&run, &[M::Switched("late")]);
}

#[test]
fn q9_26_chapelle_family_refuses() {
    let b = chapelle();
    let run = group(
        &b,
        "Chapelle-des-Bois, famille, proposition refusée",
        Behaviour::family(),
        vec![Step::Answer {
            accept: false,
            after_s: 15.0,
        }],
    );
    let t0 = group_started(&run);
    let ps = proposal_rules(&run, t0);
    let tp = ps[0].0;
    expect(
        &run,
        &[
            (M::Proposal(None), tp, tp),
            (M::Answered(false), tp + 15.0, tp + 76.0),
            (M::Said("kept"), tp + 15.0, tp + 100.0),
            (M::Finished(End::Arrived), tp, 1e6),
        ],
    );
    never(&run, &[M::Switched("pace")]);
    // Proposed again only when the delay grew by 20 min.
    for w in ps.windows(2) {
        assert!(
            w[1].1.delay_min >= w[0].1.delay_min + 20.0,
            "{} then {}",
            w[0].1.delay_min,
            w[1].1.delay_min
        );
    }
}

#[test]
fn q9_27_cervieres_fast_adult() {
    let b = cervieres();
    let run = group(
        &b,
        "Cervières, adulte rapide",
        Behaviour::fast_adult(),
        vec![Step::WaitAtStart { secs: 60.0 }],
    );
    no_manipulation(&run);
    let t0 = group_started(&run);
    never(&run, &[M::Proposal(None), M::Switched("late")]);
    let back = arrival_rel(&run) - t0;
    println!(
        "arrivée après {:.0} min (modèle {:.0})",
        back / 60.0,
        model_s(&b) / 60.0
    );
    assert!(back < 0.9 * model_s(&b));
}

#[test]
fn q9_28_chapelle_normal_adult() {
    let b = chapelle();
    let run = group(
        &b,
        "Chapelle-des-Bois, adulte au rythme du modèle avec pauses",
        Behaviour::normal_adult(),
        vec![Step::WaitAtStart { secs: 60.0 }],
    );
    no_manipulation(&run);
    group_started(&run);
    never(&run, &[M::Proposal(None)]);
    expect(&run, &[(M::Finished(End::Arrived), 0.0, 1e6)]);
}

#[test]
fn q9_29_chapelle_family_with_its_learned_profile() {
    // The same family after four outings learned (other days, other seeds): the plan
    // expects it, nothing to propose.
    let b = chapelle();
    let view = TrackView::new(&b.track).unwrap();
    let quiet = TrackerParams {
        propose: pace::ProposeParams {
            enabled: false,
            ..Default::default()
        },
        ..TrackerParams::default()
    };
    let mut past = Vec::new();
    for k in 0..4 {
        let mut sc = scen(&b, "passé", vec![]);
        sc.behaviour = Some(Behaviour::family().day(k));
        sc.seed = 100 + k;
        let run = scenario::run(&sc, &b.plan, view, &b.graph, quiet.clone()).unwrap();
        past.push(pace::digest(&b.plan, view, &b.graph, &run.inputs).unwrap());
    }
    let profile = GroupProfile::family(2, &[6, 9], Style::Skating);
    let prior = calibration::ProfileModel::new(profile.clone());
    let model = calibration::fit(profile, &past);
    let mut plan_prior = b.plan.clone();
    pace::with_pace(&mut plan_prior, &b.graph, &prior).unwrap();
    let mut plan = b.plan.clone();
    pace::with_pace(&mut plan, &b.graph, &model).unwrap();
    let (pp, pl) = (plan_prior.pace.clone().unwrap(), plan.pace.clone().unwrap());
    println!("{}\n{}", pp.text, pl.text);
    assert!(pp.range, "a new profile shows a range");
    assert!(pl.prediction.spread() < pp.prediction.spread());
    assert!(plan.text_fr().contains(&pl.text));
    let fam = Built {
        plan,
        track: b.track.clone(),
        graph: b.graph.clone(),
        layers: b.layers.clone(),
    };
    let mut sc = scen(
        &fam,
        "Chapelle-des-Bois, famille avec son profil appris",
        vec![],
    );
    sc.behaviour = Some(Behaviour::family().day(4));
    sc.seed = 104;
    let run = play(&fam, &sc, TrackerParams::default());
    no_manipulation(&run);
    let t0 = group_started(&run);
    // The same day on the plan without the group: proposed earlier and more.
    let mut plain_sc = sc.clone();
    plain_sc.name = "Chapelle-des-Bois, la même famille sans son profil".into();
    let plain = play(&b, &plain_sc, TrackerParams::default());
    let (with, without) = (
        proposal_rules(&run, t0),
        proposal_rules(&plain, group_started(&plain)),
    );
    println!(
        "propositions : {} avec le profil appris, {} sans (première à {:?} s)",
        with.len(),
        without.len(),
        without.first().map(|p| p.0.round())
    );
    assert!(!without.is_empty(), "the plain plan expects an adult");
    // With its profile: no change of route proposed (at most a new return time, a bad
    // day), and fewer proposals, later.
    assert!(
        with.iter().all(|(_, p)| p.kind == ProposalKind::Keep),
        "{with:?}"
    );
    assert!(with.len() < without.len() || with.first().map(|p| p.0) > without.first().map(|p| p.0));
    // The departure summary says the group's time.
    let summary = run
        .outputs
        .iter()
        .find_map(|o| match o {
            Output::Spoken { spoken, .. }
                if spoken.items.iter().any(|c| c.kind == "loop_summary") =>
            {
                Some(spoken.announcement.text())
            }
            _ => None,
        })
        .unwrap();
    println!("{summary}");
    let back = arrival_rel(&run) - t0;
    println!(
        "arrivée après {:.0} min ; prévu {:.0} min ({:.0} à {:.0}), modèle C5 {:.0} min",
        back / 60.0,
        pl.prediction.median_s / 60.0,
        pl.prediction.lo_s / 60.0,
        pl.prediction.hi_s / 60.0,
        model_s(&fam) / 60.0
    );
    assert!((pl.prediction.lo_s - 900.0..=pl.prediction.hi_s + 900.0).contains(&back));
}

#[test]
fn q9_30_chapelle_more_pauses_than_planned() {
    // An adult at the C5 pace stopping 3 times an hour for 12 min: the stops, not the
    // speed.
    let b = chapelle();
    let bh = Behaviour {
        speed: [1.0; 3],
        stops_per_h: 3.0,
        stop_median_s: 720.0,
        stop_sigma: 0.3,
        detours_per_h: 0.0,
        detour_m: 0.0,
        seed: 9,
    };
    let run = group(&b, "Chapelle-des-Bois, pauses longues", bh, vec![]);
    let t0 = group_started(&run);
    let ps = proposal_rules(&run, t0);
    let p = &ps.first().expect("a proposal").1;
    assert_eq!(p.reason, PaceReason::Pauses, "{p:?}");
    assert!(p.text.starts_with("Vous faites plus de pauses que prévu"));
    assert!(p.stops_min.0 > p.stops_min.1 + 5.0, "{:?}", p.stops_min);
}

/// The start of a group: a family potters at the start (stops, back and forth), so within
/// 10 min of the first movement rather than 60 s.
fn group_started(run: &Run) -> f64 {
    let tm = t_move(run);
    let s = expect(run, &[(M::Started, tm, tm + 600.0)])[0];
    println!(
        "{} : premier mouvement {tm:.0} s, départ décidé daté de {s:.0} s",
        run.scenario
    );
    s
}

/// The plan of `b` for a group (C11, `pace::with_pace`).
fn for_group(b: &Built, model: &calibration::ProfileModel) -> Built {
    let mut plan = b.plan.clone();
    pace::with_pace(&mut plan, &b.graph, model).unwrap();
    Built {
        plan,
        track: b.track.clone(),
        graph: b.graph.clone(),
        layers: b.layers.clone(),
    }
}

/// The turnaround said (« Le demi-tour est avancé. Heure limite de demi-tour. … »): when
/// and the time said (`clock.HHMM`).
fn turnaround_said(run: &Run) -> Option<(f64, String)> {
    run.outputs.iter().find_map(|o| match o {
        Output::Spoken { spoken, .. }
            if spoken.items.iter().any(|c| c.kind == "turnaround_advanced") =>
        {
            let clock = spoken
                .announcement
                .clip_ids()
                .into_iter()
                .find(|id| id.starts_with("clock."))?
                .trim_start_matches("clock.")
                .to_owned();
            println!(
                "{} : {:.0} s « {} »",
                run.scenario,
                event_time(run, o),
                spoken.announcement.text()
            );
            Some((event_time(run, o), clock))
        }
        _ => None,
    })
}

#[test]
fn q9_31_cervieres_family_turnaround_at_its_pace() {
    // The app's default family (beginner children) on its short loop (75 min of model
    // time) from 13 h, tired: well slower than even the slow side of its range. Its plan
    // keeps an hour of light (16:43, not 17:13); the turnaround of A8 runs at the group's
    // planned time: moved earlier and said (the late shortening at the start refused),
    // then « turn around now » before its limit. The same family on the plan without a
    // group is judged at the model pace: a later turnaround, and no « now ».
    let b = built(&CERVIERES, "famille-13h", |r| {
        *r = presets::family_request(&CERVIERES, presets::past_day(&CERVIERES));
        r.start_time = NaiveTime::from_hms_opt(13, 0, 0)
    });
    let model = calibration::ProfileModel::new(GroupProfile::app_default());
    let fam = for_group(&b, &model);
    let (tf, tb) = (&fam.plan.turnaround, &b.plan.turnaround);
    let pc = fam.plan.pace.as_ref().unwrap();
    println!(
        "{}\nlimite {:?} (sans groupe {:?}), arrivée prévue {} (médiane {}), demi-tour au plus tard {:?} (sans groupe {:?})",
        pc.text,
        tf.limit,
        tb.limit,
        pc.planned,
        pc.arrival,
        tf.latest_turnaround,
        tb.latest_turnaround
    );
    assert_eq!(
        tf.limit.unwrap(),
        tb.limit.unwrap() - chrono::Duration::minutes(30),
        "an hour of light kept with children"
    );
    assert!(pc.planned > pc.arrival, "planned on the slow side");
    let tired = Behaviour {
        speed: [0.4, 0.45, 0.4],
        stops_per_h: 3.0,
        stop_median_s: 360.0,
        stop_sigma: 0.5,
        detours_per_h: 4.0,
        detour_m: 30.0,
        seed: 11,
    };
    let steps = vec![Step::RefuseNext { after_s: 15.0 }];
    let run = group(
        &fam,
        "Cervières, famille, demi-tour avancé à son rythme",
        tired,
        steps.clone(),
    );
    let plain = group(
        &b,
        "Cervières, la même famille sur le plan sans groupe",
        tired,
        steps,
    );
    let t0 = group_started(&run);
    expect(
        &run,
        &[
            (M::Switched("late"), t0, t0 + 600.0),
            (M::Reverted(false), t0, t0 + 700.0),
            (M::Alert(Alert::TurnaroundAdvanced), t0 + 1200.0, 1e6),
            (M::Said("turnaround_advanced"), t0 + 1200.0, 1e6),
            (M::Alert(Alert::TurnaroundNow), t0 + 1200.0, 1e6),
            (M::Switched("no_time"), t0 + 1200.0, 1e6),
            (M::Finished(End::Arrived), 0.0, 1e6),
        ],
    );
    let (_, at_group) = turnaround_said(&run).expect("said with the group");
    let (_, at_model) = turnaround_said(&plain).expect("said without");
    assert!(
        at_group < at_model,
        "the group's turnaround {at_group} earlier than the model's {at_model}"
    );
    never(&plain, &[M::Alert(Alert::TurnaroundNow)]);
    // « Now » before the group's limit, back home in the hour of light kept.
    let now = run
        .outputs
        .iter()
        .find(|o| is(o, &M::Alert(Alert::TurnaroundNow)))
        .unwrap();
    assert!(scenario::local(now.t()) < tf.limit.unwrap());
    let back = run.t0 + chrono::Duration::seconds(arrival_rel(&run) as i64);
    println!(
        "retour {} (limite du groupe {}, limite de lumière moins 30 min {})",
        scenario::local(back),
        tf.limit.unwrap(),
        tb.limit.unwrap()
    );
    assert!(scenario::local(back) <= tb.limit.unwrap());
}

#[test]
fn q9_32_chapelle_children_are_offered_a_shorter_loop_sooner() {
    // The same pace parameters and the same slow day (a family of 6 and 9 skiing): with
    // children on the profile, the short loop is offered sooner than to adults alone at
    // that pace (C11: thresholds of two thirds, 5 min, 15 min, more likely than not).
    let b = chapelle();
    let kids = calibration::ProfileModel::new(GroupProfile::app_default());
    let mut adults = kids.clone();
    adults.profile.children.clear();
    adults.profile.name = "adultes au même rythme".into();
    let mut first = Vec::new();
    for (model, name, min) in [
        (&kids, "avec enfants", 900.0),
        (&adults, "adultes seuls", 1200.0),
    ] {
        let bb = for_group(&b, model);
        let run = group(
            &bb,
            &format!("Chapelle-des-Bois, même rythme, {name}"),
            Behaviour::family(),
            vec![],
        );
        let t0 = group_started(&run);
        let ps = proposal_rules_from(&run, t0, min);
        println!(
            "{name} : propositions {:?}",
            ps.iter()
                .map(|p| ((p.0 - t0).round(), p.1.kind))
                .collect::<Vec<_>>()
        );
        never(&run, &[M::Switched("pace")]);
        first.push(
            ps.first()
                .map(|p| (p.0 - t0, p.1.kind))
                .expect("a proposal"),
        );
    }
    let (kids_first, adults_first) = (first[0], first[1]);
    assert!(
        kids_first.0 <= 1200.0 && 2.0 * kids_first.0 < adults_first.0,
        "{kids_first:?} vs {adults_first:?}"
    );
    assert_eq!(
        kids_first.1,
        ProposalKind::ShortLoop,
        "a shorter loop first"
    );
}
