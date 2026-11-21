//! The morning on any installed pack of France (P163, #163), not only on the evaluation
//! sites: every pack of the data directory that carries its site (`manifest.json`,
//! `site.record`) is installed as the phone installs it, listed by `day_packs`, opened
//! offline by `open_day_planner`, and asked for the app's default outing (2 h) from its
//! first start (`day_pack_starts`: the starts found by the pipeline, X9, or on the
//! network for an SRN area, in the order the planner gave them at the pack build, #166:
//! the first is the one with the most real loops, #150); the first loop is validated into a
//! plan, the evening preparation (`prep_run`) prepares the first forecast day and the
//! tracker loads its layers. Every pack must open; loops must come on four massifs at least, one SRN area
//! at least; a pack on which the planner finds no loop is listed with its reason.
//!
//! `--ignored`: needs packs rebuilt with the site record, of several massifs, one an SRN
//! area at least (`skimap departs`, then `skimap build-pack <site>`).
//!
//! ```sh
//! SKIMAP_DATA_DIR=<data directory with rebuilt packs> \
//!   cargo test --release -p skimap-ffi --test any_pack -- --ignored --nocapture
//! ```

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::time::Instant;

use skimap_common::site::SiteKind;
use skimap_ffi::card::Lang;
use skimap_ffi::tracking_day::{
    DayRequest, day_pack_starts, day_packs, day_starts, open_day_planner, pack_site,
};
use skimap_pack::Pack;

fn data() -> PathBuf {
    let root = std::env::var("SKIMAP_DATA_DIR")
        .unwrap_or_else(|_| format!("{}/../../data", env!("CARGO_MANIFEST_DIR")));
    PathBuf::from(root)
}

/// The packs of the data directory that carry their site, linked under `root` as the app
/// installs them (`<root>/<site>/manifest.json`).
fn install(root: &Path) -> usize {
    let mut n = 0;
    for e in std::fs::read_dir(data().join("out")).unwrap().flatten() {
        let pack = e.path().join("pack");
        let Ok(p) = Pack::open(&pack) else { continue };
        if p.manifest().site.record.is_none() {
            continue;
        }
        std::os::unix::fs::symlink(&pack, root.join(e.file_name())).unwrap();
        n += 1;
    }
    n
}

#[test]
#[ignore = "full-data: needs packs rebuilt with their site record"]
fn the_morning_opens_on_packs_of_several_massifs() {
    let installed = tempfile::tempdir().unwrap();
    let n = install(installed.path());
    let packs = day_packs(installed.path().display().to_string());
    println!(
        "{n} packs carrying their site, {} the morning can open",
        packs.len()
    );
    assert_eq!(packs.len(), n, "every pack carrying its site and graph");

    let mut massifs = BTreeSet::new();
    let mut srn = 0;
    let mut without = Vec::new();
    for e in &packs {
        let pack = Pack::open(PathBuf::from(&e.dir)).unwrap();
        let site = pack_site(&pack).unwrap();
        assert!(
            skimap_common::site::site(site.id).is_err(),
            "{}: test packs outside the evaluation sites",
            site.id
        );
        let work = tempfile::tempdir().unwrap();
        let t0 = Instant::now();
        let planner = open_day_planner(
            e.dir.clone(),
            work.path().join("view").display().to_string(),
            Lang::Fr,
        )
        .unwrap_or_else(|err| panic!("{}: {err}", site.id));
        let open_ms = t0.elapsed().as_millis();
        let starts = day_pack_starts(e.dir.clone(), Lang::Fr);
        assert!(!starts.is_empty(), "{}: no start", site.id);
        // The same starts by identifier once the pack is open (the app asks so).
        assert_eq!(
            day_starts(site.id.to_owned(), Lang::Fr)
                .iter()
                .map(|s| &s.name)
                .collect::<Vec<_>>(),
            starts.iter().map(|s| &s.name).collect::<Vec<_>>()
        );
        let days = skimap_ffi::prep::forecast_days(&pack);
        let date = days
            .first()
            .map_or_else(|| "2026-11-18".to_owned(), ToString::to_string);
        let req = DayRequest {
            label: "sortie".into(),
            start: starts[0].clone(),
            date: date.clone(),
            start_time: Some("10:00".into()),
            earliest: "08:00".into(),
            budget_min: 120.0,
            skating: false,
            return_before: None,
            max_difficulty: None,
        };
        let m = planner
            .morning(req, None)
            .unwrap_or_else(|err| panic!("{}: {err}", site.id));
        println!(
            "{} ({:?}, {}): open {open_ms} ms, morning {} ms, {} starts, from « {} » on {date}: {} loops {:?}",
            site.id,
            site.kind,
            site.main_massif().map_or("-", |m| m.name),
            m.compute_ms,
            starts.len(),
            starts[0].name,
            m.loops.len(),
            m.loops
                .iter()
                .map(|l| format!("{} {:.1} km", l.label, l.length_km))
                .collect::<Vec<_>>()
        );
        assert!(!m.briefing.is_empty(), "{}: no briefing", site.id);
        if m.loops.is_empty() {
            // Said by the planner (a network too small from the starts, …): listed.
            without.push((site.id, m.notes.last().cloned().unwrap_or_default()));
            continue;
        }
        let plan_dir = work.path().join("plan");
        let v = planner
            .validate(0, None, plan_dir.display().to_string())
            .unwrap_or_else(|err| panic!("{}: {err}", site.id));
        assert!(plan_dir.join("plan.trk").is_file());
        println!("  plan: {:.1} km, {:?}", v.length_km, v.pace_text);

        // The evening preparation on the same pack: the first forecast day.
        if let Some(first) = days.first() {
            let today = (*first - jiff::SignedDuration::from_hours(24)).to_string();
            let out = tempfile::tempdir().unwrap();
            let r = skimap_ffi::prep::prep_run(
                e.dir.clone(),
                work.path().join("prep-view").display().to_string(),
                out.path().display().to_string(),
                None,
                None,
                today,
                1,
                Lang::Fr,
            )
            .unwrap_or_else(|err| panic!("{}: prep {err}", site.id));
            assert_eq!(r.days.len(), 1, "{}: prep {:?}", site.id, r.skipped);
            assert!(
                !r.days[0].loops.is_empty(),
                "{}: prep without loop",
                site.id
            );
            assert!(
                r.days[0].plan_dir.is_some(),
                "{}: prep without plan",
                site.id
            );
            println!(
                "  prep {}: {} loops, plan back {:?}",
                r.days[0].date,
                r.days[0].loops.len(),
                r.days[0].expected_back
            );
        }
        // The tracker's layers on the same pack.
        let layers =
            skimap_ffi::tracking::load_layers(Path::new(&e.dir), &work.path().join("layers-view"));
        assert!(layers.is_ok(), "{}: {:?}", site.id, layers.err());
        if let Some(m) = site.main_massif() {
            massifs.insert(m.name);
        }
        if site.kind == SiteKind::SrnArea {
            srn += 1;
        }
    }
    println!("massifs with loops: {massifs:?}; SRN areas with loops: {srn}");
    for (id, why) in &without {
        println!("no loop on {id}: {why}");
    }
    assert!(massifs.len() >= 4, "massifs: {massifs:?}");
    assert!(srn >= 1, "no SRN area");
}
