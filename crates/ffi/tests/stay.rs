//! A stay of three days in airplane mode after one evening of preparation (E2 #126):
//! the evening before, on network, the preparation (`prep_run`) plans the next three days
//! the pack's forecast covers; then three days without any network, each morning ski mode
//! (`prep_read`) shows the prepared days and the date of every information, the skier
//! takes the day's plan (`prep_take`) or, on a day without forecast, plans on the phone
//! (`open_day_planner`, the pack only), skis it through the tracking session (a family
//! outing simulated by the core's bench), gets the recap, and the group learns (C11).
//!
//! Airplane mode is the process's: run it in a network namespace without any interface
//! (`unshare -rn`) with `SKIMAP_AIRPLANE=1`, and the test first checks that no address
//! can be reached nor any name resolved.
//!
//! ```sh
//! SKIMAP_DATA_DIR=<data directory with packs> SKIMAP_AIRPLANE=1 unshare -rn \
//!   cargo test --release --offline -p skimap-ffi --test stay -- --ignored --nocapture
//! ```
//!
//! On every pack of the data directory the phone can plan on and whose forecast has at
//! least two days (Cervières and Chapelle-des-Bois are test data like any other).

use std::path::{Path, PathBuf};
use std::time::Instant;

use chrono::NaiveDate;
use skimap_ffi::card::Lang;
use skimap_ffi::prep::{forecast_days, prep_read, prep_run, prep_take};
use skimap_ffi::tracking::{BatchOut, RecapView, load_graph, open_tracking, pack_fixes};
use skimap_ffi::tracking_day::{
    DayRequest, GroupSpec, day_pack_starts, group_learn, group_new, open_day_planner, pack_site,
    read_day_plan,
};
use skimap_pack::Pack;
use skimap_plan::plan::DayPlan;
use skimap_plan::scenario::{self, Behaviour, Scenario, Step};
use skimap_plan::track::TrackView;
use skimap_plan::tracker::{Input, TrackerParams};

fn data() -> PathBuf {
    let root = std::env::var("SKIMAP_DATA_DIR")
        .unwrap_or_else(|_| format!("{}/../../data", env!("CARGO_MANIFEST_DIR")));
    PathBuf::from(root)
}

/// Packs the morning can open, with at least two forecast days.
fn packs() -> Vec<PathBuf> {
    let mut v: Vec<PathBuf> = std::fs::read_dir(data().join("out"))
        .unwrap()
        .flatten()
        .map(|e| e.path().join("pack"))
        .filter(|p| {
            Pack::open(p.clone()).is_ok_and(|k| {
                k.manifest().layer("piste-graph").is_some()
                    && pack_site(&k).is_ok()
                    && forecast_days(&k).len() >= 2
            })
        })
        .collect();
    v.sort();
    v
}

/// Airplane mode: no address reachable, no name resolved.
fn assert_offline() {
    use std::net::{SocketAddr, TcpStream, ToSocketAddrs};
    for a in ["1.1.1.1:443", "9.9.9.9:53", "[2606:4700:4700::1111]:443"] {
        let addr: SocketAddr = a.parse().unwrap();
        let r = TcpStream::connect_timeout(&addr, std::time::Duration::from_secs(2));
        assert!(r.is_err(), "{a} reachable: not in airplane mode");
        println!("airplane mode: {a}: {}", r.unwrap_err());
    }
    let r = "skimap.fr:443".to_socket_addrs();
    assert!(r.is_err(), "names resolved: not in airplane mode");
    println!("airplane mode: skimap.fr: {}", r.unwrap_err());
}

fn family() -> String {
    group_new(
        GroupSpec {
            name: "famille".into(),
            adults: 2,
            children: vec![10, 14],
            skating: false,
            level: "beginner".into(),
        },
        Lang::Fr,
    )
    .unwrap()
    .model_json
}

/// A family outing along the plan in `plan_dir` (core's bench), through the session in
/// batches of the GPS delay asked; the recap and the number of positions.
fn ski(pack: &Path, plan_dir: &Path, seed: u64) -> (RecapView, usize, String) {
    let plan =
        DayPlan::from_json(&std::fs::read_to_string(plan_dir.join("plan.json")).unwrap()).unwrap();
    let trk = std::fs::read(plan_dir.join("plan.trk")).unwrap();
    let g = load_graph(pack).unwrap();
    let sc = Scenario {
        name: "famille".into(),
        start: scenario::departure_utc(&plan),
        slowdown: 1.0,
        steps: vec![Step::WaitAtStart { secs: 60.0 }],
        sigma_m: 10.0,
        forest: true,
        seed,
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
    let s = open_tracking(plan_dir.display().to_string(), None, "fr".into()).unwrap();
    let mut delay_ms: i64 = 0;
    let mut recap = None;
    let mut fixes = Vec::new();
    let mut n = 0;
    let flush = |fixes: &mut Vec<(i64, f64, f64)>, delay_ms: &mut i64| -> BatchOut {
        let b = s.push_fixes(pack_fixes(fixes)).unwrap();
        fixes.clear();
        if let Some(g) = &b.gps {
            *delay_ms = g.max_delay_ms;
        }
        b
    };
    for i in &run.inputs {
        let Input::Fix { t, pos } = i else { continue };
        let t = t.timestamp_millis();
        if fixes.first().is_some_and(|&(f, _, _)| t - f > delay_ms) {
            recap = recap.or(flush(&mut fixes, &mut delay_ms).recap);
        }
        fixes.push((t, pos.lon, pos.lat));
        n += 1;
    }
    if !fixes.is_empty() {
        recap = recap.or(flush(&mut fixes, &mut delay_ms).recap);
    }
    let obs = s.observation().unwrap();
    (recap.expect("recap"), n, obs)
}

#[test]
#[ignore = "full-data: needs packs with a forecast; airplane mode with SKIMAP_AIRPLANE=1 under unshare -rn"]
fn three_days_in_airplane_mode_after_one_preparation() {
    let packs = packs();
    assert!(!packs.is_empty(), "no pack under {}", data().display());
    for pack in &packs {
        stay(pack);
    }
    println!("{} packs: three days each", packs.len());
}

fn stay(pack: &Path) {
    let p = Pack::open(pack.to_path_buf()).unwrap();
    let site = pack_site(&p).unwrap();
    let covered = forecast_days(&p);
    let eve: NaiveDate = covered[0];
    let pack_s = pack.display().to_string();
    let phone = tempfile::tempdir().unwrap();
    let prep_out = phone.path().join("prep");
    let mut group = family();

    // The evening, on charge and on Wi-Fi (the pack is the one downloaded that day).
    let t0 = Instant::now();
    let r = prep_run(
        pack_s.clone(),
        phone.path().join("prep-view").display().to_string(),
        prep_out.display().to_string(),
        Some(group.clone()),
        None,
        eve.to_string(),
        3,
        Lang::Fr,
    )
    .unwrap();
    println!(
        "{}: evening of {eve}: {} days prepared, {:?} skipped, {} ms (wall {} ms)",
        site.id,
        r.days.len(),
        r.skipped,
        r.compute_ms,
        t0.elapsed().as_millis()
    );
    assert_eq!(r.days.len() + r.skipped.len(), 3);

    // Three days without network from here on.
    if std::env::var_os("SKIMAP_AIRPLANE").is_some() {
        assert_offline();
    }
    let ages_eve = prep_read(
        prep_out.display().to_string(),
        vec![pack_s.clone()],
        eve.to_string(),
        Lang::Fr,
    )
    .infos;
    for k in 1..=3 {
        let date = eve + chrono::Duration::days(k);
        let t = Instant::now();
        let st = prep_read(
            prep_out.display().to_string(),
            vec![pack_s.clone()],
            date.to_string(),
            Lang::Fr,
        );
        let read_ms = t.elapsed().as_secs_f64() * 1e3;
        // Nothing refreshed: every information keeps its date, never « périmé ».
        assert_eq!(st.infos, ages_eve);
        assert!(st.infos.iter().all(|i| !i.updated_text.contains("périmé")));
        assert!(
            st.infos
                .iter()
                .any(|i| i.updated_text.starts_with("mis à jour le"))
        );
        let outing = phone.path().join("plans").join(format!("{date}-1"));
        let today = st.days.iter().find(|d| d.date == date.to_string());
        let how = if let Some(d) = today {
            // The prepared day, taken as it is: no heavy computation in the morning.
            assert!(st.days.iter().all(|d| d.date >= date.to_string()));
            let t = Instant::now();
            prep_take(
                d.plan_dir.clone().expect("prepared plan"),
                outing.display().to_string(),
            )
            .unwrap();
            format!(
                "prepared on {} (« {} »), taken in {:.1} ms, back planned {:?}",
                eve,
                d.prepared_text,
                t.elapsed().as_secs_f64() * 1e3,
                d.expected_back
            )
        } else {
            // Not in the forecast: never planned on another day's weather; planned on the
            // phone from the pack alone, the morning saying the weather is missing.
            assert!(!covered.contains(&date));
            let t = Instant::now();
            let planner = open_day_planner(
                pack_s.clone(),
                phone.path().join("view").display().to_string(),
                Lang::Fr,
            )
            .unwrap();
            let start = day_pack_starts(pack_s.clone(), Lang::Fr).remove(0);
            let m = planner
                .morning(
                    DayRequest {
                        label: "sortie en famille".into(),
                        start,
                        date: date.to_string(),
                        start_time: Some("10:00".into()),
                        earliest: "08:00".into(),
                        budget_min: 120.0,
                        skating: false,
                        return_before: None,
                        max_difficulty: None,
                    },
                    Some(group.clone()),
                )
                .unwrap();
            assert!(!m.loops.is_empty(), "{}: no loop on {date}", site.id);
            // Never another day's weather: no wax, no best time, the missing forecast said.
            let best = m.best_time.clone().unwrap_or_default();
            assert!(best.contains("aucune prévision météo"), "{best}");
            assert!(m.wax.is_none(), "{:?}", m.wax);
            assert!(
                m.briefing
                    .iter()
                    .any(|l| l.contains("pas de prévision météo pour ce jour")),
                "{:?}",
                m.briefing
            );
            let v = planner
                .validate(0, Some(group.clone()), outing.display().to_string())
                .unwrap();
            format!(
                "not in the forecast: planned on the phone in {} ms (« {} »), back planned {}",
                t.elapsed().as_millis(),
                best,
                v.expected_back
            )
        };
        read_day_plan(outing.display().to_string(), Lang::Fr).unwrap();
        let t = Instant::now();
        let (recap, fixes, obs) = ski(pack, &outing, 10 + k as u64);
        let ski_ms = t.elapsed().as_secs_f64() * 1e3;
        assert!(recap.arrived, "{}: {date}: {}", site.id, recap.text);
        let learned = group_learn(group, obs, Lang::Fr).unwrap();
        assert_eq!(learned.outings, k as u32);
        group = learned.model_json;
        println!(
            "  day {k} {date}: ski mode read in {read_ms:.1} ms ({} days, {} informations dated); {how}; outing {fixes} positions in {ski_ms:.0} ms: {}; group: {}",
            st.days.len(),
            st.infos.len(),
            recap.text,
            learned.learned
        );
    }
}
