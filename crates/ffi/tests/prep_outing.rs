//! The evening preparation prepares the outing the morning shows (#170): on every pack of
//! the data directory the phone can plan on, the preparation of the first forecast day
//! (`prep_run`, no group) proposes the loops the morning of the app proposes on the same
//! day from its first start, the best time of the day from 8 h: for the app's default
//! outing (2 h, classic) when the skier has chosen nothing, and for the skier's last
//! choice otherwise (1 h 30 skating, back before 16 h).
//!
//! `--ignored`: needs packs (`<SKIMAP_DATA_DIR>/out/<site>/pack`) of sites known offline
//! (packs carrying their site record, or the evaluation sites) with an hourly forecast.
//!
//! ```sh
//! SKIMAP_DATA_DIR=<data directory with packs> \
//!   cargo test --release -p skimap-ffi --test prep_outing -- --ignored --nocapture
//! ```

use std::path::{Path, PathBuf};

use skimap_ffi::card::Lang;
use skimap_ffi::tracking_day::{
    DayOuting, DayRequest, day_default_outing, day_outing_label, day_pack_starts, open_day_planner,
    pack_site,
};
use skimap_pack::Pack;

fn data() -> PathBuf {
    let root = std::env::var("SKIMAP_DATA_DIR")
        .unwrap_or_else(|_| format!("{}/../../data", env!("CARGO_MANIFEST_DIR")));
    PathBuf::from(root)
}

/// Packs of the data directory the morning can open, by directory name.
fn packs() -> Vec<PathBuf> {
    let mut v: Vec<PathBuf> = std::fs::read_dir(data().join("out"))
        .unwrap()
        .flatten()
        .map(|e| e.path().join("pack"))
        .filter(|p| {
            Pack::open(p.clone())
                .is_ok_and(|k| k.manifest().layer("piste-graph").is_some() && pack_site(&k).is_ok())
        })
        .collect();
    v.sort();
    v
}

/// (label, km) of loops, km to 10 m.
fn key(label: &str, km: f64) -> (String, i64) {
    (label.to_owned(), (km * 100.0).round() as i64)
}

/// Whether the evening before `first` prepares, for `chosen` (`None`: nothing chosen
/// yet), what the morning of `first` shows for `asked`; prints both.
fn same(
    dir: &Path,
    first: chrono::NaiveDate,
    asked: &DayOuting,
    chosen: Option<DayOuting>,
) -> bool {
    let pack = Pack::open(dir.to_path_buf()).unwrap();
    let site = pack_site(&pack).unwrap();
    let dir_s = dir.display().to_string();
    let work = tempfile::tempdir().unwrap();
    // The morning of the app on that day: `asked` from its first start.
    let starts = day_pack_starts(dir_s.clone(), Lang::Fr);
    let planner = open_day_planner(
        dir_s.clone(),
        work.path().join("view").display().to_string(),
        Lang::Fr,
    )
    .unwrap();
    let req = DayRequest {
        label: day_outing_label(asked.clone(), Lang::Fr).unwrap(),
        start: starts[0].clone(),
        date: first.to_string(),
        start_time: None,
        earliest: "08:00".into(),
        budget_min: asked.budget_min,
        skating: asked.skating,
        return_before: asked.return_before.clone(),
        max_difficulty: asked.easy_only.then(|| "easy".to_owned()),
    };
    let m = planner.morning(req.clone(), None).unwrap();
    let morning: Vec<_> = m.loops.iter().map(|l| key(&l.label, l.length_km)).collect();

    // The evening before.
    let out = tempfile::tempdir().unwrap();
    let today = (first - chrono::Duration::days(1)).to_string();
    let r = skimap_ffi::prep::prep_run(
        dir_s,
        Vec::new(),
        work.path().join("prep-view").display().to_string(),
        out.path().display().to_string(),
        None,
        chosen,
        today,
        1,
        Lang::Fr,
    )
    .unwrap();
    assert_eq!(r.days.len(), 1, "{}: {:?}", site.id, r.skipped);
    let d = &r.days[0];
    let evening: Vec<_> = d.loops.iter().map(|l| key(&l.label, l.length_km)).collect();
    let fmt = |v: &[(String, i64)]| {
        v.iter()
            .map(|(l, k)| format!("{l} {:.2} km", *k as f64 / 100.0))
            .collect::<Vec<_>>()
    };
    println!(
        "{} on {first}, « {} »:\n  morning from « {} » at {:?}: {} loops {:?}\n  evening from « {} » at {:?}: {} loops {:?}",
        site.id,
        req.label,
        starts[0].name,
        m.best_time,
        morning.len(),
        fmt(&morning),
        d.start_name,
        d.best_time,
        evening.len(),
        fmt(&evening),
    );
    morning == evening && d.start_name == starts[0].name && d.best_time == m.best_time
}

#[test]
#[ignore = "full-data: needs packs of sites known offline, with a forecast"]
fn the_evening_prepares_the_mornings_outing() {
    // The app's default outing is the one the morning asks with no choice made.
    let default = day_default_outing();
    assert_eq!(
        default,
        DayOuting {
            budget_min: 120.0,
            skating: false,
            return_before: None,
            easy_only: false,
        }
    );
    let chosen = DayOuting {
        budget_min: 90.0,
        skating: true,
        return_before: Some("16:00".into()),
        easy_only: false,
    };
    let packs = packs();
    assert!(!packs.is_empty(), "no pack under {}", data().display());
    let mut differ = Vec::new();
    let mut compared = 0;
    for dir in &packs {
        let pack = Pack::open(dir.clone()).unwrap();
        let site = pack_site(&pack).unwrap();
        let Some(first) = skimap_ffi::prep::forecast_days(&pack).first().copied() else {
            println!("{}: no forecast day in the pack, skipped", site.id);
            continue;
        };
        compared += 1;
        if !same(dir, first, &default, None) {
            differ.push(format!("{} (default)", site.id));
        }
        if !same(dir, first, &chosen, Some(chosen.clone())) {
            differ.push(format!("{} (chosen)", site.id));
        }
    }
    println!("{compared} packs compared; evening ≠ morning on {differ:?}");
    assert!(compared > 0);
    assert!(differ.is_empty(), "evening ≠ morning on {differ:?}");
}
