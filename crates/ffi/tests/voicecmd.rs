//! Voice commands (V3 #66, VO5 #87, A11 #98) on the real packs of both sites
//! (`--ignored`: needs `data/out/<site>/pack/`): a plan validated as the app does, an
//! outing simulated by the core's bench and fed to the tracking session, then every
//! command of the closed list asked by its question, classified and answered with the
//! pack's clips.
//!
//! ```sh
//! SKIMAP_DATA_DIR=~/projects/skimap/data \
//!   cargo test --release -p skimap-ffi --test voicecmd -- --ignored --nocapture
//! ```

use std::path::{Path, PathBuf};

use skimap_common::site::Site;
/// Evaluation site of `data/sites.json`, test data.
static CERVIERES: std::sync::LazyLock<skimap_common::site::Site> =
    std::sync::LazyLock::new(|| *skimap_common::site::site("cervieres").expect("evaluation site"));
/// Evaluation site of `data/sites.json`, test data.
static CHAPELLE_DES_BOIS: std::sync::LazyLock<skimap_common::site::Site> =
    std::sync::LazyLock::new(|| {
        *skimap_common::site::site("chapelle-des-bois").expect("evaluation site")
    });
use skimap_ffi::card::Lang;
use skimap_ffi::tracking::{PhaseView, SpokenView, load_graph, open_tracking, pack_fixes};
use skimap_ffi::tracking_day::{DayRequest, day_starts, open_day_planner};
use skimap_ffi::voicecmd::{
    ReportKind, ReportOutcome, VoiceAnswers, VoiceCommand, classify_command, command_questions,
};
use skimap_plan::plan::DayPlan;
use skimap_plan::scenario::{self, Scenario, Step};
use skimap_plan::track::TrackView;
use skimap_plan::tracker::{Input, TrackerParams};

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

fn show(what: &str, s: &SpokenView) {
    let clips = s.segments.iter().filter(|c| c.file.is_some()).count();
    println!(
        "  {what}: « {} » ({clips}/{} clips of the pack)",
        s.text,
        s.segments.len()
    );
    for c in &s.segments {
        if let Some(f) = &c.file {
            assert!(Path::new(f).is_file(), "{f}");
        }
    }
}

fn commands_on(site: &'static Site, budget_min: f64, return_before: Option<&str>) {
    let work = tempfile::tempdir().unwrap();
    let planner = open_day_planner(
        pack(site).display().to_string(),
        work.path().join("view").display().to_string(),
        skimap_ffi::card::Lang::Fr,
    )
    .unwrap();
    let req = DayRequest {
        label: "sortie".into(),
        start: day_starts(site.id.into(), Lang::Fr).remove(0),
        date: "2026-11-16".into(),
        start_time: Some("10:00".into()),
        earliest: "08:00".into(),
        budget_min,
        skating: false,
        return_before: return_before.map(str::to_owned),
        max_difficulty: None,
    };
    planner.morning(req, None).unwrap();
    let plan_dir = work.path().join("plan");
    let v = planner
        .validate(0, None, plan_dir.display().to_string())
        .unwrap();
    let plan =
        DayPlan::from_json(&std::fs::read_to_string(plan_dir.join("plan.json")).unwrap()).unwrap();
    let trk = std::fs::read(plan_dir.join("plan.trk")).unwrap();
    let g = load_graph(&pack(site)).unwrap();
    let sc = Scenario {
        name: "seul".into(),
        start: scenario::departure_utc(&plan),
        slowdown: 1.0,
        steps: vec![Step::WaitAtStart { secs: 60.0 }],
        sigma_m: 10.0,
        forest: true,
        seed: 5,
        tail_s: 300.0,
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
    println!(
        "{}: {} km, limit {:?}, latest turnaround {:?}",
        site.id, v.length_km, v.limit, plan.turnaround.latest_turnaround
    );

    // A third of the outing, fixes in batches of ten as the phone sends them.
    let s = open_tracking(v.plan_dir.clone(), None, "fr".into()).unwrap();
    let fixes: Vec<(i64, f64, f64)> = run
        .inputs
        .iter()
        .filter_map(|i| match i {
            Input::Fix { t, pos } => Some((t.as_millisecond(), pos.lon, pos.lat)),
            _ => None,
        })
        .collect();
    let third = fixes.len() / 3;
    for c in fixes[..third].chunks(10) {
        s.push_fixes(pack_fixes(c)).unwrap();
    }
    let (t_ms, lon, lat) = fixes[third - 1];
    let g1 = s.guidance().unwrap();
    assert_eq!(g1.phase, PhaseView::Moving, "{g1:?}");

    let answers = VoiceAnswers::new(Some(pack(site).join("voice").display().to_string()));
    assert!(answers.has_clips());
    for lang in [Lang::Fr, Lang::En] {
        for (q, want) in command_questions(lang).iter().zip([
            VoiceCommand::WhereAmI,
            VoiceCommand::TimeLeft,
            VoiceCommand::NextDirection,
            VoiceCommand::BringMeBack,
            VoiceCommand::TurnaroundTime,
            VoiceCommand::ReportTracksSet,
            VoiceCommand::ReportGroomed,
            VoiceCommand::Battery,
            VoiceCommand::Repeat,
        ]) {
            assert_eq!(classify_command(q.clone(), lang).command, Some(want), "{q}");
        }
    }

    // On the route: where, time left, next direction, turnaround, battery.
    let w = answers.where_am_i(g1.clone(), t_ms);
    show("Où suis-je ?", &w);
    assert!(
        w.text.starts_with("Toujours sur l'itinéraire."),
        "{}",
        w.text
    );
    assert!(w.all_clips, "{w:?}");
    let r = s.say_remaining(t_ms).unwrap().expect("time left");
    show("Temps restant ?", &r);
    // « Où suis-je ? » says the same distance and time left as « Temps restant ? ».
    assert!(w.text.ends_with(&r.text), "{} / {}", w.text, r.text);
    if let Some(n) = s.say_next(t_ms).unwrap() {
        show("Prochaine direction ?", &n);
    }
    let ta = answers
        .turnaround(v.plan_dir.clone(), t_ms, Lang::Fr)
        .unwrap();
    show("Heure de demi-tour ?", &ta);
    if plan.turnaround.latest_turnaround.is_some() {
        assert!(ta.all_clips, "{ta:?}");
    }
    // The countdown, the time and « now », on the plan with a latest turnaround set
    // (none of the loops of the day needs one).
    let now = skimap_ffi::tracking::epoch_ms;
    let at = |min: i64| {
        let mut p = plan.clone();
        let local = plan.departure + jiff::SignedDuration::from_mins(90);
        p.turnaround.latest_turnaround = Some(local);
        answers.turnaround_at(&p, now(local) - min * 60_000, skimap_intent::Lang::Fr)
    };
    let soon = at(20);
    show("Heure de demi-tour ? (dans 20 min)", &soon);
    assert!(
        soon.all_clips && soon.text.contains("vingt minutes"),
        "{}",
        soon.text
    );
    let late = at(200);
    show("Heure de demi-tour ? (dans 3 h 20)", &late);
    assert!(
        late.all_clips && late.text.starts_with("Heure limite de demi-tour."),
        "{}",
        late.text
    );
    let past = at(-3);
    show("Heure de demi-tour ? (passée)", &past);
    assert!(
        past.text.starts_with("Faites demi-tour maintenant"),
        "{}",
        past.text
    );
    let b = answers.battery(63.0, Some(4.0 * 3600.0), t_ms);
    show("Combien de batterie ?", &b);
    assert!(b.all_clips, "{b:?}");
    assert!(b.text.starts_with("Batterie à soixante pour cent."));
    show(
        "Signale : trace faite",
        &answers.report(
            ReportKind::TracksSet,
            ReportOutcome::Unavailable,
            Lang::Fr,
            t_ms,
        ),
    );
    show("(pas compris)", &answers.not_understood(Lang::Fr, t_ms));

    // 600 m off the route, for long enough: « Où suis-je ? » says so.
    let off: Vec<(i64, f64, f64)> = (1..=40)
        .map(|k| (t_ms + k * 5000, lon + 0.008, lat))
        .collect();
    s.push_fixes(pack_fixes(&off)).unwrap();
    let g2 = s.guidance().unwrap();
    let w2 = answers.where_am_i(g2.clone(), off.last().unwrap().0);
    show("Où suis-je ? (hors itinéraire)", &w2);
    assert!(
        g2.off_route_m > TrackerParams::default().enter_m,
        "{}",
        g2.off_route_m
    );
    assert!(
        w2.text.starts_with("Vous avez quitté l'itinéraire."),
        "{}",
        w2.text
    );
    assert!(w2.all_clips, "{w2:?}");

    // « Ramène-moi »: the way back starts.
    let back = s.back_start(off.last().unwrap().0).unwrap();
    assert_eq!(back.guidance.route, "back");
}

#[test]
#[ignore = "full-data: needs data/out/<site>/pack"]
fn full_data_voice_commands_cervieres() {
    commands_on(&CERVIERES, 180.0, None);
}

#[test]
#[ignore = "full-data: needs data/out/<site>/pack"]
fn full_data_voice_commands_chapelle() {
    commands_on(&CHAPELLE_DES_BOIS, 120.0, Some("16:00"));
}
