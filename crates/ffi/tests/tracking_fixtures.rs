//! The tracking session on the reference data set (`fixtures/`, versioned: runs in every
//! `cargo test --workspace`): a Cervières plan built from the fixture layers, an outing
//! simulated by the core's bench, fed to the session by batches as the phone does; the
//! session says, switches and sets the GPS exactly as the core's tracker, survives a
//! restart, writes the GPX, guides « ramène-moi » and gives what the group learns.

#[path = "../../../fixtures/support/mod.rs"]
mod fixtures;

use chrono::NaiveDate;
use skimap_assist::inputs::{Paths, SiteData};
use skimap_common::geom::LonLat;
use skimap_common::layout::DataLayout;
/// Evaluation site of `data/sites.json`, test data.
static CERVIERES: std::sync::LazyLock<skimap_common::site::Site> =
    std::sync::LazyLock::new(|| *skimap_common::site::site("cervieres").expect("evaluation site"));
use skimap_ffi::tracking::{
    BackView, CAR_FILE, GpsModeView, PhaseView, TrackEvent, TrackingSession, pack_fixes,
};
use skimap_ffi::tracking_day::{GroupSpec, group_learn, group_new, sms_text};
use skimap_plan::plan::{self, Context, PlanRequest};
use skimap_plan::presets;
use skimap_plan::scenario::{self, Behaviour, Scenario, Step};
use skimap_plan::track::TrackView;
use skimap_plan::tracker::{Input, Output, Tracker, TrackerParams};

#[test]
fn session_follows_the_core_on_the_fixtures() {
    let layout = DataLayout::new(fixtures::data());
    let mut data = SiteData::load(&layout, &CERVIERES, &Paths::default()).unwrap();
    let ctx = Context::load(&layout, &CERVIERES).unwrap();
    let date = NaiveDate::from_ymd_opt(2027, 1, 15).unwrap();
    let req: PlanRequest = PlanRequest {
        start_time: chrono::NaiveTime::from_hms_opt(10, 0, 0),
        ..presets::requests(&CERVIERES, date).remove(0)
    };
    let m = plan::morning(&data, &req);
    assert!(!m.suggestions.suggestions.is_empty(), "{:?}", m.notes);
    let mut p = plan::build(&mut data, &m, 0, &ctx).unwrap();
    let g = data.graph.take().unwrap();
    let model = group_new(GroupSpec {
        name: "famille".into(),
        adults: 2,
        children: vec![6, 9],
        skating: false,
        level: "intermediate".into(),
    })
    .unwrap();
    let pm: skimap_core::calibration::ProfileModel =
        serde_json::from_str(&model.model_json).unwrap();
    skimap_plan::pace::with_pace(&mut p, &g, &pm).unwrap();
    let trk = plan::track(&p, &g).unwrap().to_bytes();
    let sms = sms_text(&p, p.pace.as_ref().unwrap().arrival);
    assert!(sms.contains("112") && sms.contains("Le Laus"), "{sms}");

    let sc = Scenario {
        name: "famille".into(),
        start: scenario::departure_utc(&p),
        slowdown: 1.0,
        steps: vec![Step::WaitAtStart { secs: 60.0 }],
        sigma_m: 10.0,
        forest: true,
        seed: 3,
        tail_s: 300.0,
        behaviour: Some(Behaviour::family()),
        step_counter: false,
    };
    let run = scenario::run(
        &sc,
        &p,
        TrackView::new(&trk).unwrap(),
        &g,
        TrackerParams::default(),
    )
    .unwrap();
    let mut direct = Tracker::new(
        &p,
        TrackView::new(&trk).unwrap(),
        &g,
        None,
        TrackerParams::default(),
    )
    .unwrap();
    let dir = tempfile::tempdir().unwrap();
    let open = || {
        TrackingSession::start(
            p.clone(),
            trk.clone(),
            g.clone(),
            None,
            dir.path().to_path_buf(),
        )
        .unwrap()
    };
    let mut session = open();
    let mut spoken = (0, 0);
    let mut modes = std::collections::BTreeSet::new();
    let mut delay = 0i64;
    let mut i = 0;
    let mut calls = 0;
    let mut finished = false;
    let half = run.inputs.len() / 2;
    let mut restarted = false;
    while i < run.inputs.len() {
        if !restarted && i >= half {
            // The process dies: a new session replays what was recorded.
            let before = session.guidance().unwrap();
            drop(session);
            session = open();
            assert_eq!(session.replayed() as usize, i);
            assert_eq!(session.guidance().unwrap(), before);
            restarted = true;
        }
        let mut outs: Vec<Output> = Vec::new();
        let b = match &run.inputs[i] {
            Input::Fix { t, .. } => {
                let first = t.timestamp_millis();
                let mut fixes = Vec::new();
                while let Some(Input::Fix { t, pos }) = run.inputs.get(i) {
                    if t.timestamp_millis() - first > delay && !fixes.is_empty() {
                        break;
                    }
                    fixes.push((t.timestamp_millis(), pos.lon, pos.lat));
                    direct.push(&run.inputs[i], &mut outs);
                    i += 1;
                }
                session.push_fixes(pack_fixes(&fixes)).unwrap()
            }
            other => {
                direct.push(other, &mut outs);
                i += 1;
                let t_ms = other.t().timestamp_millis();
                let e = match other {
                    Input::Activity { activity, .. } => TrackEvent::Activity {
                        t_ms,
                        activity: match activity {
                            skimap_plan::session::Activity::Still => {
                                skimap_ffi::tracking::TrackActivity::Still
                            }
                            skimap_plan::session::Activity::OnFoot => {
                                skimap_ffi::tracking::TrackActivity::OnFoot
                            }
                            skimap_plan::session::Activity::InVehicle => {
                                skimap_ffi::tracking::TrackActivity::InVehicle
                            }
                            skimap_plan::session::Activity::Unknown => {
                                skimap_ffi::tracking::TrackActivity::Unknown
                            }
                            skimap_plan::session::Activity::SignificantMotion => {
                                skimap_ffi::tracking::TrackActivity::SignificantMotion
                            }
                        },
                    },
                    Input::Answer { accept, .. } => TrackEvent::Answer {
                        t_ms,
                        accept: *accept,
                    },
                    Input::Refuse { .. } => TrackEvent::Refuse { t_ms },
                    Input::ProfileAnswer { accept, .. } => TrackEvent::ProfileAnswer {
                        t_ms,
                        accept: *accept,
                    },
                    Input::Steps { steps, .. } => TrackEvent::Steps {
                        t_ms,
                        steps: *steps,
                    },
                    Input::Screen { on, .. } => TrackEvent::Screen { t_ms, on: *on },
                    Input::Closed { ways, .. } => TrackEvent::Closed {
                        t_ms,
                        ways: ways.clone(),
                    },
                    Input::Fix { .. } => unreachable!(),
                };
                session.push_event(e).unwrap()
            }
        };
        calls += 1;
        let want: Vec<String> = outs
            .iter()
            .filter_map(|o| match o {
                Output::Spoken { spoken, .. } => Some(spoken.announcement.text()),
                _ => None,
            })
            .collect();
        let got: Vec<String> = b.spoken.iter().map(|s| s.text.clone()).collect();
        assert_eq!(got, want, "call {calls}");
        let want_gps = outs.iter().rev().find_map(|o| match o {
            Output::Gps { request, .. } => Some((
                (request.interval_s * 1000.0).round() as i64,
                (request.max_delay_s * 1000.0).round() as i64,
            )),
            _ => None,
        });
        assert_eq!(
            b.gps.map(|x| (x.interval_ms, x.max_delay_ms)),
            want_gps,
            "call {calls}"
        );
        if let Some(gq) = b.gps {
            delay = gq.max_delay_ms;
            modes.insert(format!("{:?}", gq.mode));
        }
        spoken.0 += got.len();
        // Without the pack's clips, every announcement goes to the Android voice.
        spoken.1 += b.spoken.iter().filter(|s| s.all_clips).count();
        finished |= b.recap.is_some();
        // The banner always has its texts.
        assert!(!b.guidance.remaining_text.is_empty());
        assert!(b.guidance.arrival.contains(" h "));
    }
    assert!(restarted && finished, "{} inputs", run.inputs.len());
    assert!(spoken.0 > 3 && spoken.1 == 0, "{spoken:?}");
    for m in ["Pocket", "Approach", "Pause", "Waking"] {
        assert!(modes.contains(m), "{m} not in {modes:?}");
    }
    let g_end = session.guidance().unwrap();
    assert_eq!(g_end.phase, PhaseView::Finished);
    assert_eq!(g_end.gps_mode, GpsModeView::Idle);
    let gpx = std::fs::read_to_string(session.write_gpx().unwrap()).unwrap();
    assert_eq!(gpx.matches("<trkpt").count(), run.fixes);
    let learned = group_learn(model.model_json, session.observation().unwrap()).unwrap();
    assert_eq!(learned.outings, 1);
    assert_eq!(learned.learned, "une sortie apprise");
    println!(
        "{} inputs, {calls} calls, {} said, GPS modes {modes:?}",
        run.inputs.len(),
        spoken.0
    );
}

#[test]
fn bring_me_back_on_the_fixtures() {
    let layout = DataLayout::new(fixtures::data());
    let mut data = SiteData::load(&layout, &CERVIERES, &Paths::default()).unwrap();
    let ctx = Context::load(&layout, &CERVIERES).unwrap();
    let date = NaiveDate::from_ymd_opt(2027, 1, 15).unwrap();
    let req = presets::requests(&CERVIERES, date).remove(0);
    let m = plan::morning(&data, &req);
    let p = plan::build(&mut data, &m, 0, &ctx).unwrap();
    let g = data.graph.take().unwrap();
    let trk = plan::track(&p, &g).unwrap().to_bytes();
    let sc = Scenario {
        name: "aller".into(),
        start: scenario::departure_utc(&p),
        slowdown: 1.0,
        steps: Vec::new(),
        sigma_m: 5.0,
        forest: false,
        seed: 1,
        tail_s: 0.0,
        behaviour: None,
        step_counter: false,
    };
    let run = scenario::run(
        &sc,
        &p,
        TrackView::new(&trk).unwrap(),
        &g,
        TrackerParams::default(),
    )
    .unwrap();
    let dir = tempfile::tempdir().unwrap();
    let s = TrackingSession::start(p, trk, g, None, dir.path().to_path_buf()).unwrap();
    let t0 = run.inputs[0].t();
    let out: Vec<(i64, f64, f64)> = run
        .inputs
        .iter()
        .filter_map(|i| match i {
            Input::Fix { t, pos } if (*t - t0).num_seconds() < 1800 => {
                Some((t.timestamp_millis(), pos.lon, pos.lat))
            }
            _ => None,
        })
        .collect();
    // The phone's first fix is a cached position from elsewhere (seen on the emulator:
    // California): the way back must not go there.
    s.push_fixes(pack_fixes(&[(out[0].0 - 5000, -123.150_031_7, 39.237_255)]))
        .unwrap();
    for c in out.chunks(12) {
        s.push_fixes(pack_fixes(c)).unwrap();
    }
    let gone = s.guidance().unwrap().progress_m;
    let mut t = out.last().unwrap().0 + 1000;
    let b = s.back_start(t).unwrap();
    assert_eq!(b.guidance.route, "back");
    assert!(b.spoken[0].text.starts_with("Il reste"));
    // On the pistes: home to the car (the first fix confirmed, not the cached one) along
    // them, not longer than the way skied.
    let bv = b.guidance.back.clone().expect("way back");
    assert_eq!((bv.target.as_str(), bv.way.as_str()), ("car", "pistes"));
    let car = LonLat::new(bv.target_lon, bv.target_lat);
    let first = LonLat::new(out[0].1, out[0].2);
    assert!(car.haversine_m(first) < 30.0, "{bv:?}");
    assert!(
        b.guidance.remaining_m > 0.3 * gone && b.guidance.remaining_m < 1.3 * gone,
        "{} {gone}",
        b.guidance.remaining_m
    );
    // Skiing where the arrow points (the next useful point of the way), 4 m/s, a fix every
    // 2 s, leads to the car.
    let mut kinds = Vec::new();
    let mut arrived = false;
    let mut here = LonLat::new(bv.here_lon, bv.here_lat);
    let mut guide = (bv.guide_lon, bv.guide_lat);
    let mut offs = 0;
    'ski: for _ in 0..400 {
        let mut fixes = Vec::new();
        for _ in 0..5 {
            let g = LonLat::new(guide.0, guide.1);
            let d = here.haversine_m(g);
            let f = if d > 8.0 { 8.0 / d } else { 1.0 };
            here = LonLat::new(
                here.lon + f * (g.lon - here.lon),
                here.lat + f * (g.lat - here.lat),
            );
            t += 2000;
            fixes.push((t, here.lon, here.lat));
        }
        let r = s.push_fixes(pack_fixes(&fixes)).unwrap();
        kinds.extend(r.spoken.iter().flat_map(|x| x.kinds.clone()));
        match r.guidance.back {
            Some(v) => {
                offs += usize::from(v.off_way);
                guide = (v.guide_lon, v.guide_lat);
            }
            None => {
                arrived = true;
                break 'ski;
            }
        }
    }
    assert!(arrived, "{kinds:?}");
    assert!(here.haversine_m(car) < 60.0, "{}", here.haversine_m(car));
    assert_eq!(offs, 0, "{kinds:?}");
    assert_eq!(kinds.iter().filter(|k| *k == "arrive").count(), 1);
    assert!(kinds.iter().all(|k| k != "off_route"), "{kinds:?}");
    // Back on the tracker's route and GPS mode.
    let after = s.guidance().unwrap();
    assert_ne!(after.route, "back");
    let stop = s.back_stop().unwrap();
    assert_ne!(stop.guidance.route, "back");
}

#[test]
fn the_car_set_on_the_map_is_where_bring_me_back_goes() {
    let layout = DataLayout::new(fixtures::data());
    let mut data = SiteData::load(&layout, &CERVIERES, &Paths::default()).unwrap();
    let ctx = Context::load(&layout, &CERVIERES).unwrap();
    let date = NaiveDate::from_ymd_opt(2027, 1, 15).unwrap();
    let req = presets::requests(&CERVIERES, date).remove(0);
    let m = plan::morning(&data, &req);
    let p = plan::build(&mut data, &m, 0, &ctx).unwrap();
    let g = data.graph.take().unwrap();
    let trk = plan::track(&p, &g).unwrap().to_bytes();
    let sc = Scenario {
        name: "aller".into(),
        start: scenario::departure_utc(&p),
        slowdown: 1.0,
        steps: Vec::new(),
        sigma_m: 5.0,
        forest: false,
        seed: 1,
        tail_s: 0.0,
        behaviour: None,
        step_counter: false,
    };
    let run = scenario::run(
        &sc,
        &p,
        TrackView::new(&trk).unwrap(),
        &g,
        TrackerParams::default(),
    )
    .unwrap();
    let dir = tempfile::tempdir().unwrap();
    let open = || {
        TrackingSession::start(
            p.clone(),
            trk.clone(),
            g.clone(),
            None,
            dir.path().to_path_buf(),
        )
        .unwrap()
    };
    let s = open();
    let t0 = run.inputs[0].t();
    let out: Vec<(i64, f64, f64)> = run
        .inputs
        .iter()
        .filter_map(|i| match i {
            Input::Fix { t, pos } if (*t - t0).num_seconds() < 900 => {
                Some((t.timestamp_millis(), pos.lon, pos.lat))
            }
            _ => None,
        })
        .collect();
    for c in out.chunks(12) {
        s.push_fixes(pack_fixes(c)).unwrap();
    }
    let t = out.last().unwrap().0 + 1000;
    // Recorded on its own: the first fix.
    let auto = s.back_start(t).unwrap().guidance.back.unwrap();
    assert_eq!(auto.target, "car");
    let first = LonLat::new(out[0].1, out[0].2);
    let at = |v: &BackView| LonLat::new(v.target_lon, v.target_lat);
    assert!(at(&auto).haversine_m(first) < 30.0, "{auto:?}");
    // « La voiture est ici », 120 m east of the start, while going back: the way back goes
    // there now.
    let (lon, lat) = (out[0].1 + 120.0 / 78_700.0, out[0].2);
    let r = s.set_car(lon, lat).unwrap();
    let v = r.guidance.back.unwrap();
    assert_eq!(v.target, "car");
    let car = LonLat::new(lon, lat);
    assert!(at(&v).haversine_m(car) < 1.0, "{v:?}");
    assert!(dir.path().join(CAR_FILE).is_file());
    assert!(s.set_car(f64::NAN, 0.0).is_err());
    // Kept after a restart.
    drop(s);
    let s = open();
    let v = s.back_start(t + 1000).unwrap().guidance.back.unwrap();
    assert!(at(&v).haversine_m(car) < 1.0, "{v:?}");
    let stop = s.back_stop().unwrap();
    assert!(stop.guidance.back.is_none());
}
