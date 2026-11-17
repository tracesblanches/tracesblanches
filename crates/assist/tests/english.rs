//! The assistant in English (VO6, #129): goldens of the English briefing and loops on the
//! reference data set, and the guard that every text of the assistant has its English
//! version: the same calls in both languages, the English output without French accented
//! letters nor common French function words (proper names of the data aside).
//!
//! References: `fixtures/golden/assist/<site>.en.json`; update with
//! `SKIMAP_BLESS=1 cargo test -p skimap-assist --test english` and review the diff. The
//! French goldens (`golden.rs`, `weather.rs`) are unchanged byte for byte.

#[path = "../../../fixtures/support/mod.rs"]
mod fixtures;

use chrono::NaiveDate;
use fixtures::golden::Golden;
use skimap_assist::briefing;
use skimap_assist::crowd::{self, Date};
use skimap_assist::inputs::{Paths, SiteData};
use skimap_assist::snow_quality::Quality;
use skimap_assist::suggest::{self, Request};
use skimap_assist::text::{self, Lang};
use skimap_common::geom::LonLat;
use skimap_common::layout::DataLayout;
use skimap_common::site::Site;
use skimap_core::routing::Style;

/// Evaluation sites of `data/sites.json`, test data.
fn site(id: &str) -> &'static Site {
    skimap_common::site::site(id).expect("evaluation site")
}

fn data(site: &Site) -> SiteData {
    SiteData::load(&DataLayout::new(fixtures::data()), site, &Paths::default()).unwrap()
}

/// French words that never appear in an English sentence of the app.
const FRENCH_WORDS: &[&str] = &[
    "le",
    "la",
    "les",
    "des",
    "du",
    "de",
    "et",
    "sur",
    "pour",
    "avec",
    "sans",
    "dans",
    "au",
    "aux",
    "une",
    "un",
    "est",
    "pas",
    "ou",
    "à",
    "vers",
    "entre",
    "piste",
    "pistes",
    "neige",
    "retour",
    "boucle",
    "boucles",
    "heure",
    "jour",
    "environ",
    "fermé",
    "ouvert",
    "ouverte",
    "inconnu",
    "carte",
    "domaine",
    "itinéraire",
    "itinéraires",
    "affluence",
    "météo",
];

/// Letters of French words that English sentences do not have.
const FRENCH_LETTERS: &[char] = &[
    'é', 'è', 'ê', 'ë', 'à', 'â', 'î', 'ï', 'ô', 'ù', 'û', 'ç', 'œ', 'É', 'È', 'À', 'Ç', '«', '»',
];

/// The French left in an English text once the proper `names` are taken out: accented
/// letters, French function words.
pub fn french_in(text: &str, names: &[String]) -> Vec<String> {
    let mut t = text.to_owned();
    let mut names: Vec<&String> = names.iter().filter(|n| !n.is_empty()).collect();
    names.sort_by_key(|n| std::cmp::Reverse(n.len()));
    for n in names {
        t = t.replace(n.as_str(), " ");
    }
    let mut found = Vec::new();
    for c in FRENCH_LETTERS {
        if t.contains(*c) {
            found.push(c.to_string());
        }
    }
    for w in t.split(|c: char| !c.is_alphanumeric() && c != '\'') {
        let w = w.to_lowercase();
        let w = w.trim_start_matches("d'").trim_start_matches("l'");
        if FRENCH_WORDS.contains(&w) {
            found.push(w.to_owned());
        }
    }
    found
}

/// Proper names of a site's data: they stay as published.
fn names(d: &SiteData) -> Vec<String> {
    let mut v = vec![d.site.name.to_owned()];
    v.extend(d.domains.iter().map(|x| x.name.clone()));
    if let Some(s) = &d.status {
        v.extend(s.itineraries.iter().map(|i| i.name.clone()));
        v.extend(s.messages.iter().cloned());
        v.extend(s.conflicts.iter().cloned());
    }
    for p in &d.pistes {
        v.extend(p.name.iter().cloned());
        v.extend(p.relations.iter().cloned());
    }
    if let Some(w) = &d.weather {
        v.extend(w.points.iter().map(|p| p.name.clone()));
    }
    // Missing layers name local paths and reader errors (not app texts).
    v.extend(d.missing.iter().cloned());
    v
}

fn request(start: LonLat, name: &str, at: &str, min: f64, style: Style, lang: Lang) -> Request {
    Request {
        label: format!("{min} min"),
        start,
        start_name: name.into(),
        start_time: chrono::NaiveDateTime::parse_from_str(at, "%Y-%m-%dT%H:%M").unwrap(),
        budget_min: min,
        style,
        return_before: Some(chrono::NaiveTime::from_hms_opt(16, 0, 0).unwrap()),
        max_difficulty: Some(skimap_core::graph::Difficulty::Novice),
        max_results: 3,
        lang,
    }
}

/// Every line of the briefing and the loops of a day in `lang`.
fn texts(d: &SiteData, date: NaiveDate, start: (LonLat, &str), lang: Lang) -> Vec<String> {
    let b = briefing::day(
        d,
        date,
        &briefing::Options {
            home: Some(start.0),
            lang,
        },
    );
    let mut out = b.text.clone();
    out.extend(b.notes.iter().cloned());
    for q in &b.snow_quality {
        out.extend(q.summary.iter().cloned());
    }
    for style in [Style::Classic, Style::Skating] {
        for min in [60.0, 120.0, 240.0] {
            let at = format!("{date}T10:00");
            let s = suggest::suggest(d, &request(start.0, start.1, &at, min, style, lang));
            out.extend(s.text.iter().cloned());
            out.extend(s.notes.iter().cloned());
            for x in &s.suggestions {
                out.push(x.label.clone());
                out.extend(x.reasons.iter().map(|r| r.text.clone()));
            }
        }
    }
    out
}

const STARTS: &[(&str, f64, f64, &str)] = &[
    ("chapelle-des-bois", 6.1125, 46.6032, "village"),
    ("cervieres", 6.8161, 44.8448, "Les Fonts"),
];

const DAYS: &[&str] = &["2026-01-09", "2026-02-22", "2027-01-15", "2027-02-20"];

#[test]
fn every_text_of_the_assistant_has_its_english() {
    for (id, lon, lat, start) in STARTS {
        let d = data(site(id));
        let mut names = names(&d);
        names.push((*start).to_owned());
        for day in DAYS {
            let date = NaiveDate::parse_from_str(day, "%Y-%m-%d").unwrap();
            let fr = texts(&d, date, (LonLat::new(*lon, *lat), start), Lang::Fr);
            let en = texts(&d, date, (LonLat::new(*lon, *lat), start), Lang::En);
            assert_eq!(
                fr.len(),
                en.len(),
                "{id} {day}: the same texts in both languages"
            );
            let mut differ = 0;
            for (f, e) in fr.iter().zip(&en) {
                let left = french_in(e, &names);
                assert!(
                    left.is_empty(),
                    "{id} {day}: French {left:?} in « {e} » (French « {f} »)"
                );
                differ += usize::from(f != e);
            }
            assert!(
                differ * 2 > fr.len(),
                "{id} {day}: most texts change with the language"
            );
        }
    }
    // Texts the data set does not reach: every value of the closed lists.
    let t = text::of(Lang::En);
    let mut all: Vec<String> = Vec::new();
    for q in [
        Quality::Fresh,
        Quality::Windblown,
        Quality::OldDry,
        Quality::Transformed,
        Quality::Wet,
        Quality::Slush,
        Quality::Hard,
        Quality::Icy,
    ] {
        all.push(t.surface(q));
        all.push(t.sq_flat(1800.0, q));
    }
    for level in [
        crowd::CrowdLevel::Calm,
        crowd::CrowdLevel::Moderate,
        crowd::CrowdLevel::Busy,
        crowd::CrowdLevel::VeryBusy,
    ] {
        all.push(t.crowd(level, &["Saturday".into()]));
    }
    for o in 0..8 {
        all.push(t.aspect(o).into());
    }
    for d in 0..7 {
        all.push(t.weekday(d).into());
    }
    let day = NaiveDate::from_ymd_opt(2027, 2, 20).unwrap();
    let at = day.and_hms_opt(14, 5, 0).unwrap();
    all.extend([
        t.gusts_up_to(65.0),
        t.wind_light(10.0),
        t.wind_moderate(30.0),
        t.very_cold("-22"),
        t.cold("-14"),
        t.temperature("-3", "-8"),
        t.fog_hours(3),
        t.day_snowfall(6.0),
        t.rain(2.5),
        t.sunny().into(),
        t.snow_verdict(&t.snow_estimate(35.0, day, 4.0, 80.0), 0),
        t.snow_verdict(&t.snow_estimate(35.0, day, 0.0, 80.0), 1),
        t.snow_verdict(&t.snow_estimate(35.0, day, 0.0, 80.0), 2),
        t.snow_unknown().into(),
        t.fresh_snow_24h(12.0),
        t.no_snow_map().into(),
        t.weather_from("P1", 7.0),
        t.no_forecast().into(),
        t.as_the_crow_flies(40.0),
        t.avalanche_km(1.2, Some(3)),
        t.avalanche_km(1.2, Some(2)),
        t.avalanche_km(1.2, None),
        t.calendars_2026_27().into(),
        t.status_unknown_day().into(),
        t.status_last_known("05/01 08:10"),
        t.sources_disagree("X"),
        t.open_without_snow().into(),
        t.open().into(),
        t.closed().into(),
        t.status_unknown().into(),
        t.last_known_suffix().into(),
        t.no_status_source().into(),
        t.only_itinerary_closed(t.last_known_suffix()),
        t.none_open(3, ""),
        t.itineraries_open(1, 3, 5.0, 20.0, "", true),
        t.itineraries_open(2, 3, 15.0, 20.0, "", false),
        t.area_closed_dates().into(),
        t.nordic_km(40.0),
        t.briefing_bera(3),
        t.briefing_all_closed(4),
        t.briefing_sunset(at),
        t.crowd_fresh_snow(15.0),
        t.crowd_sun().into(),
        t.crowd_area_closed().into(),
        t.sq_no_snow(5.0, 1500.0),
        t.sq_softens(&[t.sq_aspect_at(t.aspect(4), at)]),
        t.sq_heavy(&[t.sq_aspect_slope_from(t.aspect(4), 30.0, at)]),
        t.sq_never_softens(&[t.aspect(0)]),
        t.sq_model().into(),
        t.learned(0),
        t.learned(3),
        t.s_no_graph().into(),
        t.s_no_trail_near("X"),
        t.s_unnamed_loop().into(),
        t.s_time_group_slow(
            "2 h",
            "G",
            "1 h",
            "3 h",
            &t.learned(1),
            "2 h 30",
            "classic",
            "2 h",
        ),
        t.s_out_and_back(30.0),
        t.s_eight(4.0),
        t.s_bera(2),
        t.s_no_bera_range().into(),
        t.s_bera_unavailable().into(),
        t.s_exposed(1.0, &t.s_bera(3)),
        t.s_harder(1.0),
        t.s_if_all_goes_well("2 h", "2 h 40"),
        t.s_no_way_back(3),
        t.s_evaluated(10, 2, 3, Some("G")),
        t.s_too_late(2, Some(at)),
        t.s_none_fits().into(),
        t.s_no_real_loop("1 h", "2 h", "X", true, Some((3.0, "1 h"))),
        t.s_no_real_loop("1 h", "2 h", "X", false, None),
        t.s_no_snow_all().into(),
        t.s_closed_most().into(),
        t.s_limits(&["x".into()]),
    ]);
    for p in [
        skimap_core::calibration::GroupProfile::app_default(),
        skimap_core::calibration::GroupProfile {
            name: "G".into(),
            adults: 1,
            children: vec![9],
            style: Style::Skating,
            level: skimap_core::calibration::Level::Expert,
        },
    ] {
        let mut label = t.group_label(&p);
        label = label.replace(&p.name, "G");
        all.push(label);
    }
    for s in &all {
        let left = french_in(s, &["G".into(), "X".into(), "P1".into()]);
        assert!(left.is_empty(), "French {left:?} in « {s} »");
    }
    // The holidays of the crowd calendars all have their English name.
    for year in [2026, 2027] {
        for month in 1..=12u8 {
            for day in 1..=31u8 {
                let date = Date::new(year, month, day);
                if Date::parse(&date.to_string()).is_none() {
                    continue;
                }
                for id in ["chapelle-des-bois", "cervieres"] {
                    let f = crowd::forecast_day_in(site(id), date, None, Lang::En);
                    for r in &f.reasons {
                        let left = french_in(r, &[]);
                        assert!(
                            left.is_empty(),
                            "French {left:?} in the crowd reason « {r} »"
                        );
                    }
                }
            }
        }
    }
}

#[test]
fn english_goldens() {
    for (id, lon, lat, start) in STARTS {
        let d = data(site(id));
        let mut k = Golden::new(&format!("assist/{id}.en"));
        let date = NaiveDate::from_ymd_opt(2027, 2, 20).unwrap();
        let b = briefing::day(
            &d,
            date,
            &briefing::Options {
                home: None,
                lang: Lang::En,
            },
        );
        // Without the local paths of the missing layers.
        let root = fixtures::data().display().to_string();
        k.lines("briefing.text", &b.text.join("\n").replace(&root, "<data>"));
        let s = suggest::suggest(
            &d,
            &request(
                LonLat::new(*lon, *lat),
                start,
                "2027-02-20T10:00",
                120.0,
                Style::Classic,
                Lang::En,
            ),
        );
        k.lines("suggest.text", &s.text.join("\n"))
            .count("suggest.suggestions", s.suggestions.len());
        k.check();
    }
}
