//! Checks on the real layers of both sites and on the archived past days
//! (`data/out/<site>/assist/history/`, written by the `history` example). Reads only;
//! writes nothing.
//!
//! ```sh
//! cargo test -p skimap-assist --test real_sites -- --ignored
//! ```

use std::path::{Path, PathBuf};

use chrono::NaiveDate;
use skimap_assist::briefing::{self, Factor};
use skimap_assist::inputs::{Paths, SiteData};
use skimap_assist::layers::OpenState;
use skimap_assist::snow_quality::Quality;
use skimap_assist::{synthesis, wax};
use skimap_common::layout::DataLayout;
use skimap_common::site::{CERVIERES, CHAPELLE_DES_BOIS, Site};

fn layout() -> DataLayout {
    DataLayout::new(
        std::env::var("SKIMAP_DATA_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|_| Path::new(env!("CARGO_MANIFEST_DIR")).join("../../data")),
    )
}

fn history(site: &Site, forecast: bool, route: &str) -> SiteData {
    let l = layout();
    let h = l.layer_dir(site, "assist").unwrap().join("history");
    let mut paths = Paths::default();
    let mut entries: Vec<PathBuf> = std::fs::read_dir(&h)
        .unwrap()
        .map(|e| e.unwrap().path())
        .collect();
    entries.sort();
    for e in entries {
        let n = e.file_name().unwrap().to_string_lossy().into_owned();
        if n.starts_with("meteo_") && n.ends_with(".json") {
            paths.meteo.push(e);
        } else if n.starts_with("snow_") {
            paths.snow_dirs.push(e);
        } else if n.starts_with("bera_") {
            paths.beras.push(e);
        }
    }
    if forecast {
        paths
            .meteo
            .push(l.layer_dir(site, "meteo-hourly").unwrap().join("hourly.json"));
        paths.snow_dirs.push(l.layer_dir(site, "snow").unwrap());
    }
    paths.route = Some(l.layer_dir(site, "routes").unwrap().join(route));
    SiteData::load(&l, site, &paths).unwrap()
}

fn date(s: &str) -> NaiveDate {
    NaiveDate::parse_from_str(s, "%Y-%m-%d").unwrap()
}

#[test]
#[ignore]
fn cervieres_january_2026_snowfall_and_cold() {
    let d = history(&CERVIERES, false, "routes.geojson");
    assert!(d.missing.is_empty(), "{:?}", d.missing);
    let opts = briefing::Options::default();
    // 5 January: cold, dry, the snow map gives 16 cm on the Cervières pistes (the station
    // at 1614 m measured 19 cm at 06 h UTC): insufficient for grooming.
    let b = briefing::day(&d, date("2026-01-05"), &opts);
    let cerv = b.domains.iter().find(|c| c.name == "Cervières").unwrap();
    let snow = cerv.snow.unwrap();
    assert!((10.0..20.0).contains(&snow.depth_cm()), "{snow:?}");
    assert!(cerv.reasons.iter().any(|r| r.factor == Factor::Snow && r.points == -3.0));
    let q = &b.snow_quality[0];
    assert_eq!(q.flat, Quality::OldDry);
    // 9 January: snowfall (the station went from 19 to 40 cm): fresh snow, limit depth.
    let b = briefing::day(&d, date("2026-01-09"), &opts);
    assert_eq!(b.snow_quality[0].flat, Quality::Fresh);
    let cerv = b.domains.iter().find(|c| c.name == "Cervières").unwrap();
    assert!(cerv.snow.unwrap().depth_cm() >= 20.0);
    assert_eq!(b.domains[0].name, "Cervières", "{:?}", b.domains[0]);
    // Wax that day: fresh snow at −3 °C: a blue or violet hard wax, by the Swix columns.
    let w = wax::day_wax(
        d.weather.as_ref().unwrap().point("le-laus").unwrap(),
        &b.snow_quality[0],
    )
    .unwrap();
    assert!(!w.morning.kick.klister, "{:?}", w.morning.kick);
    // Synthesis: the BERA was not readable offline (PDF only), the route Le Laus → Les
    // Fonts crosses historic avalanche paths (report 07: 1.38 km on the liaison alone).
    let s = synthesis::synthesise(&d, date("2026-01-05"));
    assert!(s.sentences[0].contains("pas disponible hors ligne"));
    let km = s.facts.clpa_km.unwrap();
    assert!(km >= 1.38 && km < 4.0, "{km}");
    assert_eq!(s.sentences.len(), 3);
    assert!(!s.text().contains("sans danger"));
}

#[test]
#[ignore]
fn cervieres_spring_2023_bera_and_softening() {
    let d = history(&CERVIERES, false, "offpiste_laus_izoard.geojson");
    let b = briefing::day(&d, date("2023-04-15"), &briefing::Options::default());
    assert_eq!(b.bera_level, Some(2));
    // The BERA says « fond dur ou crouté »: refrozen in the morning, softening during the
    // day, east before south before west at 20°.
    let q = &b.snow_quality[0];
    let c = |code: &str| q.classes.iter().find(|c| c.code == code).unwrap();
    assert_eq!(c("S20").morning, Quality::Hard);
    let (e, s, w) = (c("E20").softens_at, c("S20").softens_at, c("W20").softens_at);
    assert!(e.unwrap() <= s.unwrap() && s.unwrap() <= w.unwrap());
    let syn = synthesis::synthesise(&d, date("2023-04-15"));
    assert!(syn.sentences[0].starts_with("Risque d'avalanche deux sur cinq, limité"));
}

#[test]
#[ignore]
fn chapelle_february_2026_thaw() {
    let d = history(&CHAPELLE_DES_BOIS, false, "offpiste_risoux_fevrier.geojson");
    let day = date("2026-02-24");
    let b = briefing::day(&d, day, &briefing::Options::default());
    // Thaw week (La Boissaude 1222 m: 33 cm on the 22nd, 9 cm on the 28th): wet snow,
    // klister, no hard wax.
    assert!(b.snow_quality[0].flat.is_wet(), "{:?}", b.snow_quality[0].flat);
    let w = wax::day_wax(&d.weather.as_ref().unwrap().points[0], &b.snow_quality[0]).unwrap();
    assert!(w.afternoon.kick.klister);
    // No BERA in the Jura, never « safe »; the APPB rule on the route is said.
    let s = synthesis::synthesise(&d, day);
    assert!(s.sentences[0].starts_with("Pas de bulletin d'avalanche pour ce massif"));
    assert!(s.sentences[2].contains("sortie des itinéraires balisés interdite"), "{}", s.sentences[2]);
    assert!(!s.text().contains("sans danger"));
}

#[test]
#[ignore]
fn today_both_sites() {
    let t = date("2026-11-15");
    let d = history(&CERVIERES, true, "routes.geojson");
    let b = briefing::day(&d, t, &briefing::Options::default());
    assert_eq!(b.status, "day");
    // Nordic France says closed; the commune opens on 19 December.
    let cerv = b.domains.iter().find(|c| c.name == "Cervières").unwrap();
    assert_eq!(cerv.state, OpenState::Closed);
    assert!(b.snow_quality.iter().all(|q| !q.enough_snow));
    let d = history(&CHAPELLE_DES_BOIS, true, "offpiste_risoux_fevrier.geojson");
    let b = briefing::day(&d, t, &briefing::Options::default());
    // The ENJ lists Le Glacier as open with no snow: not recommended.
    assert!(!briefing::spoken(&b).contains("Conseillé"));
}
