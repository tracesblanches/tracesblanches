//! The assistant in the published packs (#151): every public pack of the data directory
//! (`personal_sources` off) that carries its site holds the assistant layers of the
//! pipeline (`assist-briefing`, `assist-wax`, `assist-snow-quality`,
//! `assist-suggestions`), and the app, opening it offline as the phone does
//! (`open_day_planner`), gives its morning: briefing with the AROME weather, wax or the
//! reason there is none, best time, loops.
//!
//! `--ignored`: needs public packs rebuilt in a working copy of the data (`skimap
//! build-all` without `SKIMAP_PERSONAL_SOURCES`).
//!
//! ```sh
//! SKIMAP_DATA_DIR=<working copy with public packs> \
//!   cargo test --release -p skimap-ffi --test public_packs -- --ignored --nocapture
//! ```

use std::path::PathBuf;

use skimap_ffi::card::Lang;
use skimap_ffi::tracking_day::{DayRequest, day_pack_starts, open_day_planner, pack_site};
use skimap_pack::Pack;

const ASSIST: [&str; 4] = [
    "assist-briefing",
    "assist-wax",
    "assist-snow-quality",
    "assist-suggestions",
];

fn data() -> PathBuf {
    let root = std::env::var("SKIMAP_DATA_DIR")
        .unwrap_or_else(|_| format!("{}/../../data", env!("CARGO_MANIFEST_DIR")));
    PathBuf::from(root)
}

#[test]
#[ignore = "full-data: needs public packs rebuilt in a working copy"]
fn public_packs_carry_the_assistant_and_the_app_shows_it() {
    let mut checked = 0;
    let mut with_wax = 0;
    let mut dirs: Vec<PathBuf> = std::fs::read_dir(data().join("out"))
        .unwrap()
        .flatten()
        .map(|e| e.path().join("pack"))
        .collect();
    dirs.sort();
    for dir in dirs {
        let Ok(pack) = Pack::open(&dir) else { continue };
        let m = pack.manifest();
        if m.personal_sources || m.site.record.is_none() {
            continue;
        }
        let site = pack_site(&pack).unwrap();
        assert!(pack.verify().unwrap().is_empty(), "{}: verify", site.id);
        for id in ASSIST {
            let l = m
                .layers
                .iter()
                .find(|l| l.id == id)
                .unwrap_or_else(|| panic!("{}: {id} absent of the public pack", site.id));
            assert!(
                !l.personal_use_only && l.sources.iter().all(skimap_common::licence::publishable),
                "{}: {id} with a source that may not be published",
                site.id
            );
        }
        let dir_s = dir.display().to_string();
        let work = tempfile::tempdir().unwrap();
        let planner = open_day_planner(
            dir_s.clone(),
            work.path().join("view").display().to_string(),
            Lang::Fr,
        )
        .unwrap_or_else(|e| panic!("{}: {e}", site.id));
        let starts = day_pack_starts(dir_s, Lang::Fr);
        assert!(!starts.is_empty(), "{}: no start", site.id);
        // The first forecast day from the day the pipeline ran the assistant (the app
        // asks for today or a later day).
        let wax_layer = m.layers.iter().find(|l| l.id == "assist-wax").unwrap();
        let wax: serde_json::Value =
            serde_json::from_slice(&std::fs::read(pack.path(&wax_layer.file).unwrap()).unwrap())
                .unwrap();
        let first: chrono::NaiveDate = wax["date"].as_str().unwrap().parse().unwrap();
        let days = skimap_ffi::prep::forecast_days(&pack);
        let date = days
            .iter()
            .find(|d| **d >= first)
            .expect("AROME forecast days")
            .to_string();
        let mo = planner
            .morning(
                DayRequest {
                    label: "sortie".into(),
                    start: starts[0].clone(),
                    date: date.clone(),
                    start_time: Some("10:00".into()),
                    earliest: "08:00".into(),
                    budget_min: 120.0,
                    skating: false,
                    return_before: None,
                    max_difficulty: None,
                },
                None,
            )
            .unwrap_or_else(|e| panic!("{}: {e}", site.id));
        assert!(!mo.briefing.is_empty(), "{}: no briefing", site.id);
        let weather = mo.briefing.iter().any(|l| l.contains("°C"));
        assert!(
            weather,
            "{}: briefing without the weather: {:?}",
            site.id, mo.briefing
        );
        with_wax += usize::from(mo.wax.is_some());
        println!(
            "{} {date}: {} briefing lines, wax {:?}, best time {:?}, {} loops",
            site.id,
            mo.briefing.len(),
            mo.wax
                .as_deref()
                .map(|w| w.chars().take(90).collect::<String>()),
            mo.best_time
                .as_deref()
                .map(|w| w.chars().take(90).collect::<String>()),
            mo.loops.len()
        );
        println!(
            "  {}",
            mo.briefing
                .iter()
                .find(|l| l.contains("°C"))
                .map(|l| l.chars().take(160).collect::<String>())
                .unwrap_or_default()
        );
        checked += 1;
    }
    println!("{checked} public packs, {with_wax} with a wax advice");
    assert!(checked >= 6, "public packs checked: {checked}");
    assert!(with_wax >= 1, "no pack with a wax advice");
}
