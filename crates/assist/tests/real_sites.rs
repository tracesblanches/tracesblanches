//! Checks on the real layers of both sites and on the archived past days. Reads only;
//! writes nothing.
//!
//! Two categories:
//!
//! - fixtures (default): the redistributable layers of `fixtures/` (graph, pistes, zones,
//!   coverage, huts, snow, exposure of Chapelle-des-Bois), without any weather: the
//!   suggestions (A5) and the equipment list (A6) do not need it;
//! - full data (`full_data_`, weekly job): the archived past days of
//!   `data/out/<site>/assist/history/` (Open-Meteo historical forecasts, personal use
//!   only, never versioned), the BERA archive and the status sources.
//!
//! ```sh
//! cargo test -p skimap-assist --test real_sites
//! cargo test -p skimap-assist --test real_sites -- --ignored full_data_
//! ```

#[path = "../../../fixtures/support/mod.rs"]
mod fixtures;

use std::path::PathBuf;

use chrono::NaiveDate;
use skimap_assist::briefing::{self, Factor};
use skimap_assist::inputs::{Paths, SiteData};
use skimap_assist::layers::OpenState;
use skimap_assist::snow_quality::Quality;
use skimap_assist::suggest::{self, Request};
use skimap_assist::{alerts, best_time, equipment, synthesis, wax};
use skimap_common::geom::LonLat;
use skimap_common::layout::DataLayout;
use skimap_common::site::{CERVIERES, CHAPELLE_DES_BOIS, Site};
use skimap_core::routing::{Router, Style};

fn layout() -> DataLayout {
    DataLayout::new(fixtures::full_data())
}

/// Layers of the fixtures, no weather, no status, no BERA.
fn fixture_data(site: &Site) -> SiteData {
    let l = DataLayout::new(fixtures::data());
    let d = SiteData::load(&l, site, &Paths::default()).unwrap();
    assert!(d.weather.is_none() && d.status.is_none() && d.beras.is_empty());
    assert!(d.graph.is_some() && !d.pistes.is_empty() && d.zones.is_some());
    assert!(d.no_network.is_some(), "{:?}", d.missing);
    d
}

fn history(site: &Site, forecast: bool, route: &str) -> SiteData {
    let l = layout();
    let h = l.layer_dir(site, "assist").unwrap().join("history");
    let mut paths = Paths::default();
    let mut entries: Vec<PathBuf> = std::fs::read_dir(&h)
        .unwrap_or_else(|e| panic!("{}: {e}", h.display()))
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
        paths.meteo.push(
            l.layer_dir(site, "meteo-hourly")
                .unwrap()
                .join("hourly.json"),
        );
        paths.snow_dirs.push(l.layer_dir(site, "snow").unwrap());
    }
    paths.route = Some(l.layer_dir(site, "routes").unwrap().join(route));
    SiteData::load(&l, site, &paths).unwrap()
}

fn date(s: &str) -> NaiveDate {
    NaiveDate::parse_from_str(s, "%Y-%m-%d").unwrap()
}

#[test]
#[ignore = "full-data: data/out/<site>/assist/history (Open-Meteo, personal use)"]
fn full_data_cervieres_january_2026_snowfall_and_cold() {
    let d = history(&CERVIERES, false, "routes.geojson");
    assert!(d.missing.is_empty(), "{:?}", d.missing);
    let opts = briefing::Options::default();
    // 5 January: cold, dry, the snow map gives 16 cm on the Cervières pistes (the station
    // at 1614 m measured 19 cm at 06 h UTC): insufficient for grooming.
    let b = briefing::day(&d, date("2026-01-05"), &opts);
    let cerv = b.domains.iter().find(|c| c.name == "Cervières").unwrap();
    let snow = cerv.snow.unwrap();
    assert!((10.0..20.0).contains(&snow.depth_cm()), "{snow:?}");
    assert!(
        cerv.reasons
            .iter()
            .any(|r| r.factor == Factor::Snow && r.points == -3.0)
    );
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
    assert!((1.38..4.0).contains(&km), "{km}");
    assert_eq!(s.sentences.len(), 3);
    assert!(!s.text().contains("sans danger"));
}

#[test]
#[ignore = "full-data: data/out/<site>/assist/history (Open-Meteo, personal use)"]
fn full_data_cervieres_spring_2023_bera_and_softening() {
    let d = history(&CERVIERES, false, "offpiste_laus_izoard.geojson");
    let b = briefing::day(&d, date("2023-04-15"), &briefing::Options::default());
    assert_eq!(b.bera_level, Some(2));
    // The BERA says « fond dur ou crouté »: refrozen in the morning, softening during the
    // day, east before south before west at 20°.
    let q = &b.snow_quality[0];
    let c = |code: &str| q.classes.iter().find(|c| c.code == code).unwrap();
    assert_eq!(c("S20").morning, Quality::Hard);
    let (e, s, w) = (
        c("E20").softens_at,
        c("S20").softens_at,
        c("W20").softens_at,
    );
    assert!(e.unwrap() <= s.unwrap() && s.unwrap() <= w.unwrap());
    let syn = synthesis::synthesise(&d, date("2023-04-15"));
    assert!(syn.sentences[0].starts_with("Risque d'avalanche deux sur cinq, limité"));
}

#[test]
#[ignore = "full-data: data/out/<site>/assist/history (Open-Meteo, personal use)"]
fn full_data_chapelle_february_2026_thaw() {
    let d = history(&CHAPELLE_DES_BOIS, false, "offpiste_risoux_fevrier.geojson");
    let day = date("2026-02-24");
    let b = briefing::day(&d, day, &briefing::Options::default());
    // Thaw week (La Boissaude 1222 m: 33 cm on the 22nd, 9 cm on the 28th): wet snow,
    // klister, no hard wax.
    assert!(
        b.snow_quality[0].flat.is_wet(),
        "{:?}",
        b.snow_quality[0].flat
    );
    let w = wax::day_wax(&d.weather.as_ref().unwrap().points[0], &b.snow_quality[0]).unwrap();
    assert!(w.afternoon.kick.klister);
    // No BERA in the Jura, never « safe »; the APPB rule on the route is said.
    let s = synthesis::synthesise(&d, day);
    assert!(s.sentences[0].starts_with("Pas de bulletin d'avalanche pour ce massif"));
    assert!(
        s.sentences[2].contains("sortie des itinéraires balisés interdite"),
        "{}",
        s.sentences[2]
    );
    assert!(!s.text().contains("sans danger"));
}

#[test]
#[ignore = "full-data: data/out/<site>/assist/history (Open-Meteo, personal use)"]
fn full_data_today_both_sites() {
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

fn request(
    start: LonLat,
    name: &str,
    at: &str,
    min: f64,
    style: Style,
    back: Option<u32>,
) -> Request {
    Request {
        label: format!("{min} min"),
        start,
        start_name: name.into(),
        start_time: chrono::NaiveDateTime::parse_from_str(at, "%Y-%m-%dT%H:%M").unwrap(),
        budget_min: min,
        style,
        return_before: back.map(|h| chrono::NaiveTime::from_hms_opt(h, 0, 0).unwrap()),
        max_difficulty: None,
        max_results: 3,
    }
}

#[test]
#[ignore = "full-data: data/out/<site>/assist/history (Open-Meteo, personal use)"]
fn full_data_suggestions_respect_time_and_report_graph_limits() {
    let d = history(&CHAPELLE_DES_BOIS, false, "offpiste_risoux_fevrier.geojson");
    let village = LonLat::new(6.1125, 46.6032);
    let r = request(
        village,
        "village",
        "2026-02-22T10:00",
        120.0,
        Style::Skating,
        Some(16),
    );
    let s = suggest::suggest(&d, &r);
    assert_eq!(s.suggestions.len(), 3, "{:?}", s.text);
    for x in &s.suggestions {
        assert!((90.0..=132.0).contains(&x.eta_min), "{}", x.eta_min);
        assert!(x.arrival.time() <= chrono::NaiveTime::from_hms_opt(16, 0, 0).unwrap());
        assert!(x.length_km > 5.0);
    }
    // One-way trails of the graph (C1): part of the network is out of reach, said.
    assert!(
        s.notes
            .iter()
            .any(|n| n.contains("ne sont pas atteignables")),
        "{:?}",
        s.notes
    );
    // Cervières: from Les Fonts, the Le Laus network is disconnected (18 m gap in P2).
    let c = history(&CERVIERES, false, "routes.geojson");
    let fonts = LonLat::new(6.8161, 44.8448);
    let s = suggest::suggest(
        &c,
        &request(
            fonts,
            "Les Fonts",
            "2026-01-09T14:30",
            90.0,
            Style::Classic,
            None,
        ),
    );
    assert!(
        s.notes.iter().any(|n| n.starts_with("138 nœuds sur 189")),
        "{:?}",
        s.notes
    );
    // A8 on the best loop, started late and 1.5 times slower: the turnaround is advanced.
    let best = &s.suggestions[0];
    let late = chrono::NaiveDateTime::parse_from_str("2026-01-09T15:00", "%Y-%m-%dT%H:%M").unwrap();
    let p = c.weather.as_ref().unwrap().point("les-fonts").unwrap();
    let trace = alerts::synthetic_trace(&best.profile, 1.5, 300.0);
    let a = alerts::replay(late, &best.profile, p, &trace, alerts::Params::default());
    assert!(
        a.iter().any(|x| matches!(
            x.kind,
            alerts::Kind::TurnaroundAdvanced | alerts::Kind::TurnaroundNow
        )),
        "{a:#?}"
    );
}

/// Without a group, the loops (A5) and the turnaround (A8) of the presets on both sites:
/// suggestions (JSON) and, for each loop, the planned turnaround and the alerts of a trace
/// 1.6 times slower than the model from 13:00. `SKIMAP_GOLDEN=<file>`: written on the
/// first run (before a change), compared on the next ones (non-regression).
#[test]
#[ignore]
fn suggestions_and_turnaround_without_group_golden() {
    let Some(golden) = std::env::var_os("SKIMAP_GOLDEN").map(PathBuf::from) else {
        eprintln!("SKIMAP_GOLDEN not set: nothing to compare");
        return;
    };
    let mut all = Vec::new();
    for (site, day, route) in [
        (&CERVIERES, "2026-01-09", "routes.geojson"),
        (
            &CHAPELLE_DES_BOIS,
            "2026-02-22",
            "offpiste_risoux_fevrier.geojson",
        ),
    ] {
        let d = history(site, false, route);
        for r in skimap_assist::presets::suggestion_requests(site, date(day)) {
            let s = suggest::suggest(&d, &r);
            let mut turns = Vec::new();
            if let Some((p, _)) = d.weather.as_ref().and_then(|w| w.nearest(r.start)) {
                for x in &s.suggestions {
                    let late = r.start_time.date().and_hms_opt(13, 0, 0).unwrap();
                    let m = alerts::Monitor::new(late, &x.profile, p, alerts::Params::default());
                    let trace = alerts::synthetic_trace(&x.profile, 1.6, 300.0);
                    let a = alerts::replay(late, &x.profile, p, &trace, alerts::Params::default());
                    turns.push(serde_json::json!({
                        "planned_turnaround": m.planned_turnaround(),
                        "alerts": a,
                    }));
                }
            }
            all.push(serde_json::json!({"site": site.id, "suggestions": s, "a8": turns}));
        }
    }
    let now = serde_json::to_string_pretty(&all).unwrap();
    if !golden.is_file() {
        std::fs::write(&golden, &now).unwrap();
        eprintln!("written {} ({} bytes)", golden.display(), now.len());
        return;
    }
    let before = std::fs::read_to_string(&golden).unwrap();
    assert!(before == now, "outputs differ from {}", golden.display());
    eprintln!("identical to {} ({} bytes)", golden.display(), now.len());
}

/// The presets for a family (children of 6 and 9, prior) on both sites: loops on its time
/// (C11), shorter than the adult's for the same time; A8 turnaround at its pace.
#[test]
#[ignore]
fn suggestions_and_turnaround_for_a_family() {
    use skimap_core::calibration::{GroupProfile, ProfileModel, terrain_ratio};
    let family = ProfileModel::new(GroupProfile::family(2, &[6, 9], Style::Classic));
    for (site, day, route) in [
        (&CERVIERES, "2026-01-09", "routes.geojson"),
        (
            &CHAPELLE_DES_BOIS,
            "2026-02-22",
            "offpiste_risoux_fevrier.geojson",
        ),
    ] {
        let d = history(site, false, route);
        for r in skimap_assist::presets::suggestion_requests(site, date(day)) {
            let adult = suggest::suggest(&d, &r);
            let fam = suggest::suggest_for(&d, &r, Some(&family));
            let km = |s: &suggest::Suggestions| {
                s.suggestions
                    .iter()
                    .map(|x| x.length_km)
                    .fold(0.0, f64::max)
            };
            println!(
                "{} « {} » : adulte {} boucles (≤ {:.1} km, modèle {:?} min) ; famille {} boucles (≤ {:.1} km, modèle {:?} min, groupe {:?} min) {:?}",
                site.id,
                r.label,
                adult.suggestions.len(),
                km(&adult),
                adult
                    .suggestions
                    .iter()
                    .map(|x| x.eta_min)
                    .collect::<Vec<_>>(),
                fam.suggestions.len(),
                km(&fam),
                fam.suggestions
                    .iter()
                    .map(|x| x.eta_min)
                    .collect::<Vec<_>>(),
                fam.suggestions
                    .iter()
                    .map(|x| x.group.as_ref().map(|g| (g.lo_min, g.median_min, g.hi_min)))
                    .collect::<Vec<_>>(),
                fam.notes
            );
            for x in &fam.suggestions {
                let g = x.group.as_ref().unwrap();
                // Planned on the slow side of its range (children); the option « si tout
                // va bien » on its median.
                let t = if x.if_all_goes_well {
                    g.median_min
                } else {
                    g.plan_min
                };
                assert!(
                    (0.75 * r.budget_min..=1.1 * r.budget_min).contains(&t),
                    "{g:?}"
                );
                assert!(g.plan_min > g.median_min);
                assert!(x.eta_min < g.median_min);
                if let Some(l) = r.return_before {
                    assert!(x.arrival.time() <= l);
                }
            }
            if !fam.suggestions.is_empty() && !adult.suggestions.is_empty() {
                assert!(km(&fam) < km(&adult), "{} {}", km(&fam), km(&adult));
            }
            // A8 on the adult's best loop: the family's turnaround comes before the
            // model's, or exists where the model has none.
            if let (Some(best), Some((p, _))) = (
                adult.suggestions.first(),
                d.weather.as_ref().and_then(|w| w.nearest(r.start)),
            ) {
                let ratio = terrain_ratio(&best.eta);
                let model =
                    alerts::Monitor::new(r.start_time, &best.profile, p, alerts::Params::default());
                let group = alerts::Monitor::for_group(
                    r.start_time,
                    &best.profile,
                    p,
                    alerts::Params::default(),
                    &family.params,
                    ratio,
                );
                println!(
                    "   A8 sur la boucle de l'adulte ({} km) : demi-tour au plus tard modèle {:?}, famille {:?} (temps famille {:.0} min, modèle {:.0} min)",
                    best.length_km,
                    model.planned_turnaround().map(|t| t.time()),
                    group.planned_turnaround().map(|t| t.time()),
                    group.profile().total_s() / 60.0,
                    best.profile.total_s() / 60.0
                );
                match (model.planned_turnaround(), group.planned_turnaround()) {
                    (Some(a), Some(b)) => assert!(b <= a),
                    (None, _) => {}
                    (Some(_), None) => panic!("a turnaround for the model, none for the family"),
                }
            }
        }
    }
}

#[test]
#[ignore = "full-data: data/out/<site>/assist/history (Open-Meteo, personal use)"]
fn full_data_best_time_equipment_and_weather_replay() {
    let d = history(&CERVIERES, false, "routes.geojson");
    let p = d.weather.as_ref().unwrap().point("le-laus").unwrap();
    // 8 January 2026: fog from late morning at Le Laus.
    let day = date("2026-01-08");
    let a = alerts::day_replay(p, day, alerts::Params::default());
    assert!(a.iter().any(|x| x.kind == alerts::Kind::Fog), "{a:#?}");
    let q = skimap_assist::snow_quality::day_quality(p, day, &Default::default(), None).unwrap();
    let req = best_time::Request {
        date: day,
        duration_min: 120.0,
        earliest: chrono::NaiveTime::from_hms_opt(8, 0, 0).unwrap(),
        classes: vec![],
    };
    let b = best_time::best_time(p, Some(&q), None, &req);
    let best = b.best.clone().unwrap();
    assert!(
        best.start.time() < chrono::NaiveTime::from_hms_opt(11, 0, 0).unwrap(),
        "{}",
        b.text
    );
    assert!(b.starts.iter().all(|s| s.back <= b.deadline.unwrap()));
    // Le Laus → Les Fonts on the pistes: avalanche terrain (P16–P18, CLPA) and 5 km
    // without network.
    let route = d.route.as_ref().unwrap().line.clone();
    let o = equipment::Outing {
        label: "Le Laus → Les Fonts".into(),
        start: date("2026-01-05").and_hms_opt(10, 0, 0).unwrap(),
        duration_min: 135.0,
        route,
        offpiste: false,
        kick_unreliable: false,
    };
    let e = equipment::equipment(&d, &o);
    let kinds: Vec<equipment::Kind> = e.items.iter().map(|i| i.kind).collect();
    assert!(kinds.contains(&equipment::Kind::AvalancheKit), "{}", e.text);
    assert!(kinds.contains(&equipment::Kind::PowerBank), "{}", e.text);
    assert!(
        e.facts.no_network_km > 4.0 && e.facts.clpa_km > 1.38,
        "{:?}",
        e.facts
    );
}

#[test]
fn suggestions_on_the_fixtures_without_weather() {
    let d = fixture_data(&CHAPELLE_DES_BOIS);
    let village = LonLat::new(6.1125, 46.6032);
    let r = request(
        village,
        "village",
        "2026-02-22T10:00",
        120.0,
        Style::Skating,
        Some(16),
    );
    let s = suggest::suggest(&d, &r);
    println!("{}", s.text.join("\n"));
    assert_eq!(s.suggestions.len(), 3, "{:?}", s.text);
    for x in &s.suggestions {
        assert!((90.0..=132.0).contains(&x.eta_min), "{}", x.eta_min);
        assert!(x.arrival.time() <= chrono::NaiveTime::from_hms_opt(16, 0, 0).unwrap());
        assert!(x.length_km > 5.0);
    }
    // One-way trails of the graph (C1): part of the network is out of reach, said.
    assert!(
        s.notes
            .iter()
            .any(|n| n.contains("ne sont pas atteignables")),
        "{:?}",
        s.notes
    );
    // Cervières: from Les Fonts, the Le Laus network is disconnected (18 m gap in P2).
    let c = fixture_data(&CERVIERES);
    let s = suggest::suggest(
        &c,
        &request(
            LonLat::new(6.8161, 44.8448),
            "Les Fonts",
            "2026-01-09T14:30",
            90.0,
            Style::Classic,
            None,
        ),
    );
    println!("{}", s.text.join("\n"));
    assert!(
        s.notes.iter().any(|n| n.starts_with("138 nœuds sur 189")),
        "{:?}",
        s.notes
    );
    assert!(!s.suggestions.is_empty());
}

#[test]
fn equipment_on_the_fixtures_le_laus_les_fonts() {
    let d = fixture_data(&CERVIERES);
    let g = d.graph.as_ref().unwrap();
    // Le Laus → Les Fonts on the pistes (C1), 11.36 km: about 5 km without any network
    // (report 06, ARCEP T2 2026).
    let route = Router::new(g, skimap_core::eta::EtaModel::new(Style::Classic))
        .route(
            LonLat::new(6.725_006, 44.858_940),
            LonLat::new(6.8157, 44.84465),
            500.0,
        )
        .unwrap()
        .polyline(g);
    let o = equipment::Outing {
        label: "Le Laus → Les Fonts".into(),
        start: date("2026-01-05").and_hms_opt(10, 0, 0).unwrap(),
        duration_min: 135.0,
        route,
        offpiste: false,
        kick_unreliable: false,
    };
    let e = equipment::equipment(&d, &o);
    println!("{}", e.text);
    let kinds: Vec<equipment::Kind> = e.items.iter().map(|i| i.kind).collect();
    assert!(kinds.contains(&equipment::Kind::PowerBank), "{}", e.text);
    assert!(e.facts.no_network_km > 4.0, "{:?}", e.facts);
    // Neither the CLPA (not redistributable) nor the exposure of Cervières is in the
    // fixtures: nothing about avalanches may be claimed from missing data.
    assert_eq!(e.facts.clpa_km, 0.0, "{:?}", e.facts);
}

/// X16 (#150) on the fixtures (no weather): from Le Laus, a real loop for 2 h of skating
/// (the 11 km half out-and-back before), and for 3 h, where the network has no real loop
/// that long, the out-and-backs said as the only option with the longest real loop; at
/// Chapelle-des-Bois only real loops. The share is measured on the legs.
#[test]
fn real_loops_on_the_fixtures() {
    let share = |x: &suggest::Suggestion| {
        let (t, l) = travelled_twice_m(&x.legs);
        t / l
    };
    let c = fixture_data(&CERVIERES);
    let reqs = skimap_assist::presets::suggestion_requests(&CERVIERES, date("2026-01-09"));
    let s = suggest::suggest(&c, &reqs[0]);
    println!("{}", s.text.join("\n"));
    let x = s.suggestions.first().expect("a loop for 2 h");
    assert!(x.real_loop && share(x) < 0.25, "{}", share(x));
    assert!(!s.notes.iter().any(|n| n.contains("vraie boucle")));
    let s = suggest::suggest(&c, &reqs[1]);
    println!("{}", s.text.join("\n"));
    assert!(!s.suggestions.is_empty());
    assert!(s.suggestions.iter().all(|x| !x.real_loop));
    assert!(
        share(&s.suggestions[0]) < 0.35,
        "{}",
        share(&s.suggestions[0])
    );
    let note = s
        .notes
        .iter()
        .find(|n| n.starts_with("aucune vraie boucle trouvée de 2 h 15 à 3 h 18 depuis Le Laus"))
        .expect("said");
    assert!(
        note.contains("la plus longue vraie boucle trouvée fait"),
        "{note}"
    );
    let d = fixture_data(&CHAPELLE_DES_BOIS);
    for r in skimap_assist::presets::suggestion_requests(&CHAPELLE_DES_BOIS, date("2026-02-22")) {
        let s = suggest::suggest(&d, &r);
        assert_eq!(s.suggestions.len(), 3, "{:?}", s.text);
        for x in &s.suggestions {
            assert!(x.real_loop && share(x) < 0.05, "{}: {}", x.label, share(x));
            assert!((x.out_and_back_share - share(x)).abs() < 0.006);
        }
    }
}

/// Length of a route on stretches of the graph already travelled, and its length, m:
/// interval overlap on each edge (an OSM way split into several edges travelled once is
/// not travelled twice).
fn travelled_twice_m(legs: &[skimap_core::routing::Leg]) -> (f64, f64) {
    let mut seen: std::collections::HashMap<usize, Vec<(f64, f64)>> = Default::default();
    let (mut twice, mut total) = (0.0, 0.0);
    for l in legs {
        let (a, b) = (l.from_m.min(l.to_m), l.from_m.max(l.to_m));
        total += b - a;
        let prev = seen.entry(l.edge).or_default();
        // `prev` holds disjoint intervals: the overlap is the sum of the intersections.
        twice += prev
            .iter()
            .map(|(x, y)| (b.min(*y) - a.max(*x)).max(0.0))
            .sum::<f64>();
        let (mut lo, mut hi) = (a, b);
        prev.retain(|(x, y)| {
            let touch = *x <= hi && *y >= lo;
            if touch {
                lo = lo.min(*x);
                hi = hi.max(*y);
            }
            !touch
        });
        prev.push((lo, hi));
    }
    (twice, total)
}

/// X16 (#150): share of the proposed loops travelled twice (out-and-back), on both sites
/// with the weather of an archived day: the three presets and a grid of times (60 to 240
/// min, classic and skating) from each start. Prints one line per request and the totals;
/// the share is measured here on the legs (interval overlap per edge), independently of
/// the one the assistant gives (`out_and_back_share`, printed second).
#[test]
#[ignore = "full-data: data/out/<site>/assist/history (Open-Meteo, personal use)"]
fn full_data_out_and_back_share_of_the_loops() {
    let grid = [60.0, 90.0, 120.0, 180.0, 240.0];
    for (site, day, route, starts) in [
        (
            &CERVIERES,
            "2026-01-09",
            "routes.geojson",
            vec![
                (LonLat::new(6.7269, 44.8552), "Le Laus"),
                (LonLat::new(6.8161, 44.8448), "Les Fonts"),
            ],
        ),
        (
            &CHAPELLE_DES_BOIS,
            "2026-02-22",
            "offpiste_risoux_fevrier.geojson",
            vec![(LonLat::new(6.1125, 46.6032), "village")],
        ),
    ] {
        let d = history(site, false, route);
        let mut reqs = skimap_assist::presets::suggestion_requests(site, date(day));
        for (p, name) in &starts {
            for style in [Style::Classic, Style::Skating] {
                for min in grid {
                    let mut r = request(*p, name, &format!("{day}T10:00"), min, style, None);
                    r.label = format!("{name} {min} min {style:?}");
                    reqs.push(r);
                }
            }
        }
        let (mut first_n, mut first_sum, mut km, mut km_twice, mut loops, mut oab) =
            (0, 0.0, 0.0, 0.0, 0, 0);
        let mut none = 0;
        let mut n_said = 0;
        let mut secs = 0.0;
        for r in &reqs {
            let clock = std::time::Instant::now();
            let s = suggest::suggest(&d, r);
            secs += clock.elapsed().as_secs_f64();
            let parts: Vec<String> = s
                .suggestions
                .iter()
                .map(|x| {
                    let (t, l) = travelled_twice_m(&x.legs);
                    format!(
                        "{:.1} km {:.0} % ({:.0} %)",
                        x.length_km,
                        100.0 * t / l,
                        x.out_and_back_share * 100.0
                    )
                })
                .collect();
            let said: Vec<&String> = s
                .notes
                .iter()
                .filter(|n| n.contains("vraie boucle"))
                .collect();
            println!(
                "{} « {} » : {}{}",
                site.id,
                r.label,
                parts.join(" ; "),
                if said.is_empty() {
                    String::new()
                } else {
                    format!(" — {}", said[0])
                }
            );
            if let Some(f) = s.suggestions.first() {
                let (t, l) = travelled_twice_m(&f.legs);
                first_n += 1;
                first_sum += t / l;
            } else {
                none += 1;
            }
            if !said.is_empty() {
                n_said += 1;
            }
            for x in &s.suggestions {
                let (t, l) = travelled_twice_m(&x.legs);
                loops += 1;
                km += l;
                km_twice += t;
                if t / l >= 0.25 {
                    oab += 1;
                }
            }
        }
        println!(
            "{} : {} demandes ({} sans boucle, {} « aucune vraie boucle »), {} boucles ; part \
             d'aller-retour moyenne de la première boucle {:.1} %, de toutes (pondérée par la \
             longueur) {:.1} % ; {} boucles à 25 % ou plus ; {:.2} s par demande",
            site.id,
            reqs.len(),
            none,
            n_said,
            loops,
            100.0 * first_sum / f64::from(first_n.max(1)),
            100.0 * km_twice / km.max(1e-9),
            oab,
            secs / reqs.len() as f64
        );
    }
}
