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
use skimap_common::layout::DataLayout;
use skimap_common::site::{CERVIERES, CHAPELLE_DES_BOIS, Site};
use skimap_core::graph::PisteGraph;
use skimap_plan::gps::GpsMode;
use skimap_plan::phrases::never_safe;
use skimap_plan::plan::{self, DayPlan, PlanRequest};
use skimap_plan::presets;
use skimap_plan::scenario::{self, Run, Scenario, Step};
use skimap_plan::session::{End, SessionEvent};
use skimap_plan::speech::Priority;
use skimap_plan::track::TrackView;
use skimap_plan::tracker::{Alert, Cause, Condition, Input, Output, RouteRef, TrackerParams};

struct Built {
    plan: DayPlan,
    track: Vec<u8>,
    graph: PisteGraph,
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
            Input::Fix { .. } | Input::Activity { .. } | Input::Closed { .. }
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
