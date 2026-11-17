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
use skimap_assist::equipment::Kind as EqKind;
use skimap_assist::inputs::{Paths, SiteData};
use skimap_assist::snow_quality::Quality;
use skimap_assist::suggest::{self, Request};
use skimap_assist::text::{self, Lang};
use skimap_assist::wax::SnowType;
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
    // The reasons of the missing layers (local paths, reader errors) are not app texts;
    // their layer names are.
    v.extend(
        d.missing
            .iter()
            .filter_map(|m| m.split_once(" : ").map(|(_, why)| why.to_owned())),
    );
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
                let left = fixtures::french::french_in(e, &names);
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
    let flat = skimap_assist::snow_quality::SlopeClass {
        slope_deg: 0.0,
        aspect_deg: None,
    };
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
        // Best time (A9).
        t.bt_surface(
            &t.slope_class(&flat),
            &[t.quality(Quality::Hard), t.quality(Quality::Wet)],
        ),
        t.bt_gusts(55.0),
        t.bt_fog(2.0),
        t.bt_snowfall().into(),
        t.bt_rain().into(),
        t.bt_cold("-16"),
        t.bt_sun().into(),
        t.bt_crowd(crowd::CrowdLevel::VeryBusy),
        t.bt_near_deadline(at),
        t.bt_other_start(at, &[t.bt_rain().into()]),
        t.bt_best(
            at,
            at,
            120.0,
            &[],
            &[t.bt_other_start(at, &[t.bt_sun().into()])],
        ),
        t.bt_best(at, at, 120.0, &[t.bt_sun().into()], &[]),
        t.bt_none(120.0, Some(at)),
        t.bt_none(120.0, None),
        // Wax (A2).
        t.wax_kick(("V30 Blue", "Swix"), Some(("Nordic GripWax blue", "Toko"))),
        t.wax_kick_uncertain(None, ("V30 Blue", "Swix"), None),
        t.wax_kick_uncertain(
            Some(&t.wax_skins(t.wax_wet_falling())),
            ("K22", "Swix"),
            None,
        ),
        t.wax_window(
            (9, 12),
            "-3",
            "-5",
            SnowType::Old,
            &t.wax_kick(("V30 Blue", "Swix"), None),
            ("PS6 Blue", "Swix"),
            Some(("Base Performance Hot Wax blue", "Toko")),
        ),
        t.wax_colder_than_all().into(),
        t.wax_warmer_than_hard().into(),
        t.wax_coarse_frozen().into(),
        t.wax_skins(t.wax_fresh_near_zero()),
        t.wax_skins(&t.wax_refrozen_then_wet(Quality::Hard, Quality::Wet)),
        t.wax_skins(&t.wax_crosses_zero(-3.0, 2.0)),
        t.wax_glide(-3.0, -5.0),
        t.wax_humidity(80.0),
        t.wax_humidity_unknown().into(),
        t.wax_day("x.", Some("y.")),
        t.wax_day("x.", None),
        // Equipment (A6).
        t.eq_exposed(1.2),
        t.eq_clpa(0.4),
        t.eq_offpiste_bera(3),
        t.eq_back_after_sunset(at, at),
        t.eq_start_before_sunrise(at, at),
        t.eq_chill("-22"),
        t.eq_no_network(2.0, 30.0),
        t.eq_offpiste().into(),
        t.eq_long(200.0),
        t.eq_fog(2),
        t.eq_clear_sky(10.0),
        t.eq_kick_unreliable().into(),
        t.eq_nothing("X"),
        t.eq_take("X", &[(t.eq_label(EqKind::Headlamp), "y")]),
        // Synthesis (A7).
        t.sy_risk_other(">2400", 2),
        t.sy_risk_other("<2400", 2),
        t.sy_aspects(&[t.aspect(0), t.aspect(1)]),
        t.sy_read_bulletin().into(),
        t.sy_no_bera_jura("No avalanche bulletin for this massif"),
        t.sy_no_bera("No avalanche bulletin for this massif"),
        t.sy_bera_missing(Some("X"), true),
        t.sy_bera_missing(None, false),
        t.sy_clpa(1.0, 2),
        t.sy_clpa_none().into(),
        t.sy_clpa_no_map().into(),
        t.sy_no_weather().into(),
        t.sy_no_forecast("P1"),
        t.sy_weather(
            "P1",
            1800.0,
            &[
                t.sy_temperature("-5", Some("2")),
                t.sy_temperature("-5", None),
                t.sy_chill("-12"),
                t.sy_wind_light().into(),
                t.sy_fog(2),
                t.sy_snow(5.0),
                t.sy_freezing(1500.0),
                t.sy_clear().into(),
                t.sy_overcast().into(),
            ],
        ),
        t.sy_night("x"),
        t.sy_zones_more("X", "X", 3),
        t.sy_until(1, 3),
        t.sy_until(15, 6),
        t.sy_rules(
            true,
            t.sy_rules_not_loaded(),
            t.sy_not_covered(),
            &t.sy_no_network_km(2.0),
        ),
        t.sy_rules(false, t.sy_no_rule(), "", t.sy_network_everywhere()),
        t.sy_rules(false, t.sy_no_rule(), "", &t.sy_no_network_share(20.0)),
        // Alerts (A8).
        t.al_wind_rising(60.0, 20.0),
        t.al_fog().into(),
        t.al_snow(1.5),
        t.al_cold("-12", "-20"),
        t.al_turn_now(25.0, 1.3, Some(at)),
        t.al_turn_advanced(25.0, 1.3, Some(at), at),
    ]);
    for k in [
        EqKind::AvalancheKit,
        EqKind::Headlamp,
        EqKind::WarmLayer,
        EqKind::FaceProtection,
        EqKind::PowerBank,
        EqKind::TellSomeone,
        EqKind::Compass,
        EqKind::SunProtection,
        EqKind::WaxlessSkis,
    ] {
        all.push(t.eq_label(k).into());
    }
    for l in skimap_assist::inputs::LayerName::ALL {
        all.push(t.layer_name(l).into());
        all.push(t.layer_missing(l, "X"));
    }
    for st in [
        SnowType::New,
        SnowType::Old,
        SnowType::CoarseFrozen,
        SnowType::TransformedMoist,
        SnowType::Wet,
    ] {
        all.push(t.wax_model(st, -3.0, -6.0, 1.0));
    }
    {
        use skimap_core::zone_eval::Verdict;
        use skimap_zones::rules::Topic;
        for topic in Topic::ALL {
            for v in [
                Verdict::Forbidden,
                Verdict::ForbiddenInPlaces,
                Verdict::AvoidRecommended,
            ] {
                for stoves in [false, true] {
                    all.extend(t.sy_verdict(topic, v, stoves).map(str::to_owned));
                }
            }
        }
    }
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
        let left = fixtures::french::french_in(s, &["G".into(), "X".into(), "P1".into()]);
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
                        let left = fixtures::french::french_in(r, &[]);
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
