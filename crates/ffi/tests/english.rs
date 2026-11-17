//! The texts of the app's core in English (VO6, #129): every text function of the ffi in
//! [`Lang::En`], on the synthetic test pack (`examples/test_pack.rs`) and on outings and
//! structures built by hand, without French accented letters nor common French function
//! words (proper names aside). The texts the data do not reach are called on every value
//! of their closed lists.

use std::path::Path;

use chrono::{NaiveDate, NaiveDateTime};
use skimap_common::geom::LonLat;
use skimap_core::zone_eval::{Inactive, Verdict};
use skimap_ffi::fog::{FogTarget, guided_bearing};
use skimap_ffi::prep::{battery_forecast, prep_read, updated_text};
use skimap_ffi::recap::{AltitudeSource, outing_history, outing_recap};
use skimap_ffi::tracking::INPUTS_FILE;
use skimap_ffi::tracking_back::{Target, TargetKind, proposal_text};
use skimap_ffi::{Lang, open_packs, text};
use skimap_plan::plan::KeyKind;
use skimap_plan::tracker::{Cause, Condition, Input};
use skimap_zones::rules::{SourceKind, Topic};

#[path = "../examples/test_pack.rs"]
#[allow(dead_code)]
mod test_pack;

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
    "inconnue",
    "carte",
    "domaine",
    "domaines",
    "itinéraire",
    "itinéraires",
    "météo",
    "sortie",
    "sorties",
    "prévu",
    "prévue",
    "temps",
    "vitesse",
    "dont",
    "fois",
    "bilan",
    "règle",
    "règles",
    "interdit",
    "batterie",
    "mis",
    "jusqu",
    "départ",
    "arrivée",
    "voiture",
    "refuges",
    "cap",
    "hors",
    "aucun",
    "aucune",
    "modèle",
    "groupe",
];

/// Letters of French words that English sentences do not have.
const FRENCH_LETTERS: &[char] = &[
    'é', 'è', 'ê', 'ë', 'à', 'â', 'î', 'ï', 'ô', 'ù', 'û', 'ç', 'œ', 'É', 'È', 'À', 'Ç', '«', '»',
];

/// The French left in an English text once the proper `names` are taken out.
fn french_in(text: &str, names: &[&str]) -> Vec<String> {
    let mut t = text.to_owned();
    let mut names: Vec<&str> = names.iter().copied().filter(|n| !n.is_empty()).collect();
    names.sort_by_key(|n| std::cmp::Reverse(n.len()));
    for n in names {
        t = t.replace(n, " ");
    }
    let mut found = Vec::new();
    for c in FRENCH_LETTERS {
        if t.contains(*c) {
            found.push(c.to_string());
        }
    }
    // Hyphenated English words (« off-piste ») are one word.
    for w in t.split(|c: char| !c.is_alphanumeric() && c != '\'' && c != '-') {
        let w = w.to_lowercase();
        let w = w.trim_start_matches("d'").trim_start_matches("l'");
        if FRENCH_WORDS.contains(&w) {
            found.push(w.to_owned());
        }
    }
    found
}

fn assert_english(texts: &[String], names: &[&str]) {
    assert!(!texts.is_empty());
    for s in texts {
        let left = french_in(s, names);
        assert!(left.is_empty(), "French {left:?} in « {s} »");
    }
}

/// 2026-02-15 09:00 UTC, 10:00 in France.
const T0: i64 = 1_771_146_000_000;

/// A bearing outing to a refuge in `root/name`: `n` fixes 10 m apart every 5 s.
fn outing(root: &Path, name: &str, start: i64, n: usize) -> String {
    let d = root.join(name);
    guided_bearing(
        d.display().to_string(),
        FogTarget {
            kind: "refuge".into(),
            name: "Refuge test".into(),
            lon: 6.0,
            lat: 45.05,
            distance_m: None,
        },
        None,
        0.0,
        Lang::En,
        None,
        None,
    )
    .unwrap();
    let mut s = String::new();
    for i in 0..n {
        let t = chrono::DateTime::from_timestamp_millis(start + i as i64 * 5000).unwrap();
        let pos = LonLat::new(6.0, 45.0 + i as f64 * 10.0 / 111_195.0);
        s.push_str(&serde_json::to_string(&Input::Fix { t, pos }).unwrap());
        s.push('\n');
    }
    std::fs::write(d.join(INPUTS_FILE), s).unwrap();
    d.display().to_string()
}

#[test]
fn the_recap_in_english() {
    let root = tempfile::tempdir().unwrap();
    let work = tempfile::tempdir().unwrap();
    let plans = root.path().display().to_string();
    outing(root.path(), "a", T0, 400);
    let second = outing(root.path(), "b", T0 + 86_400_000, 160);
    let h = outing_history(plans.clone(), Lang::En);
    assert_eq!(h.len(), 2);
    let r = outing_recap(second, plans, work.path().display().to_string(), Lang::En).unwrap();
    eprintln!("{}\n{}", r.text, r.spoken);
    let mut all = vec![r.text.clone(), r.spoken.clone()];
    all.extend(r.figures.iter().cloned());
    all.extend(r.comparisons.iter().cloned());
    all.extend(r.conditions.iter().cloned());
    all.extend(h.iter().map(|o| o.when.clone()));
    assert_english(&all, &["Refuge test"]);
    assert!(r.text.contains("Distance: 1.6 km"), "{}", r.text);
    assert!(
        r.comparisons[0].starts_with("2nd time on this route"),
        "{:?}",
        r.comparisons
    );
    assert!(r.spoken.starts_with("Recap: "), "{}", r.spoken);
    // The same outing in French is unchanged.
    let fr = outing_recap(
        h[0].dir.clone(),
        root.path().display().to_string(),
        work.path().display().to_string(),
        Lang::Fr,
    )
    .unwrap();
    assert!(fr.text.contains("Distance : 1,6 km"), "{}", fr.text);
}

#[test]
fn the_test_pack_in_english() {
    let tmp = tempfile::tempdir().unwrap();
    let a = tmp.path().join("packs/test");
    test_pack::write_test_pack(&a, 1).unwrap();
    let dir = a.display().to_string();
    let style = std::fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../styles/fond-clair.json"),
    )
    .unwrap();
    // Free mode: the packs merged.
    let view = open_packs(vec![dir.clone()], style, true, Lang::En).unwrap();
    // The name of the pack's site stays as published.
    let site = skimap_pack::Pack::open(a.clone())
        .unwrap()
        .manifest()
        .site
        .name
        .clone();
    let site = site.as_str();
    let mut all = vec![view.info.site_name.clone(), view.info.validity_note.clone()];
    // Ski mode: the age of every information of the pack.
    let out = tmp.path().join("prep");
    let st = prep_read(
        out.display().to_string(),
        vec![dir],
        "2026-02-15".into(),
        Lang::En,
    );
    assert!(st.infos.len() >= 3, "{:?}", st.infos);
    for i in &st.infos {
        all.push(i.label.clone());
        all.push(i.updated_text.clone());
    }
    all.push(updated_text(T0, Lang::En));
    // Battery: enough, until a time, not enough, in the cold.
    for (level, temp, end_h, measured) in [
        (80.0, 15.0, 5, (0.0, 0.0)),
        (40.0, 5.0, 2, (1.0, 6.0)),
        (18.0, -12.0, 6, (1.0, 9.0)),
    ] {
        let b = battery_forecast(
            T0,
            level,
            temp,
            0.0,
            T0 + end_h * 3_600_000,
            20.0,
            0.5,
            false,
            measured.0,
            measured.1,
            Lang::En,
        );
        all.push(b.text.clone());
        all.extend(b.warning.clone());
    }
    eprintln!("{all:#?}");
    assert_english(&all, &[site, "Test"]);
    assert!(all.iter().any(|s| s.starts_with("Battery too low")));
}

/// Every value of the closed lists the data do not all reach.
#[test]
fn every_text_of_the_ffi_has_its_english() {
    let t = text::of(Lang::En);
    let day = NaiveDate::from_ymd_opt(2027, 2, 20).unwrap();
    let at: NaiveDateTime = day.and_hms_opt(14, 5, 0).unwrap();
    let mut all: Vec<String> = vec![
        t.group_learned(0),
        t.group_learned(1),
        t.group_learned(4),
        t.group_who(1, &[]),
        t.group_who(2, &[10, 14]),
        t.duration(5400.0),
        t.clock5(at),
        t.clock_day(at + chrono::Duration::days(1), day),
        t.duration_range("1 h", "2 h"),
        t.duration_about("1 h", "50 min", "1 h 20"),
        t.back_slow_side("1 h", "1 h 20", "15:00"),
        t.back_between("15:00", "15:30"),
        t.back_around("15:00"),
        t.model_time("1 h"),
        t.back_with_margin("15:00"),
        t.after_limit(at, true),
        t.after_limit(at, false),
        t.sms(
            "X",
            day,
            "S",
            at,
            "L",
            12.5,
            &["P".into()],
            at,
            Some(at),
            at,
        ),
        t.default_group_name().into(),
        t.solo_group_name().into(),
        t.when(at),
        t.delta(3600.0, 3600.0),
        t.delta(4800.0, 3600.0),
        t.delta(2400.0, 3600.0),
        t.recap_distance(12.4),
        t.recap_time(9000.0, 7500.0),
        t.recap_speed(6.1),
        t.recap_estimate(true, 7200.0, 9000.0),
        t.recap_estimate(false, 7200.0, 7300.0),
        t.recap_gain_imported(310.0),
        t.recap_same_route(2, "15/02 at 10:00", 9000.0, 0.0),
        t.recap_same_route(3, "15/02 at 10:00", 9000.0, -900.0),
        t.recap_same_route(11, "15/02 at 10:00", 9000.0, 900.0),
        t.recap_speed_vs(6.0, 5.5, 1),
        t.recap_speed_vs(6.0, 5.5, 3),
        t.recap_first().into(),
        t.recap_spoken("12 kilometres", 9000.0, Some("as planned"), Some("C.")),
        t.recap_spoken("12 kilometres", 9000.0, None, None),
        t.cond_temperature(-8.0, -2.0),
        t.cond_gusts(45.0),
        t.cond_fog(3),
        t.cond_snow(2.5),
        t.cond_source("P1"),
        t.cond_altitude(1500.0, 1800.0),
        t.cond_none().into(),
        t.not_an_outing("X"),
        t.stoves_included().into(),
        t.zone_off_season("X"),
        t.rules_at(at),
        t.rules_note().into(),
        t.day(day),
        t.updated(at),
        t.date_unknown().into(),
        t.skipped_no_forecast("Saturday 20/02"),
        t.skipped_no_start("Saturday 20/02"),
        t.time_group(3600.0, 3000.0, 4200.0),
        t.time_model(3600.0),
        t.battery_day(10.0, None),
        t.battery_day(10.0, Some(-12.0)),
        t.battery_until("15:00", 10.0, Some(-3.0)),
        t.battery_short("16:00", "14:30"),
        t.never_guarantee().into(),
        t.no_offpiste_relief().into(),
        t.outside_relief().into(),
        t.no_passable(200.0),
        t.no_offpiste_route().into(),
        t.arrival_range(at, at + chrono::Duration::minutes(20)),
        t.all_domains().into(),
        t.packs_note(&["X", "Y"]),
    ];
    for id in [
        "bera",
        "meteo-hourly",
        "status",
        "snow-depth",
        "offpiste-snow",
        "avalanche-exposure",
        "zones",
        "refuges",
        "pistes",
        "pack",
    ] {
        all.push(t.info_label(id).expect(id).into());
    }
    for id in ["snow", "exposure", "zones", "network"] {
        all.push(t.missing_layer(id).into());
    }
    for s in [
        AltitudeSource::BarometerAndRelief,
        AltitudeSource::Barometer,
        AltitudeSource::Relief,
    ] {
        all.push(t.recap_elevation(320.0, 310.0, s));
    }
    for k in [
        KeyKind::Start,
        KeyKind::Turn,
        KeyKind::Junction,
        KeyKind::Refuge,
        KeyKind::NoNetworkStart,
        KeyKind::NoNetworkEnd,
        KeyKind::AvalancheStart,
        KeyKind::AvalancheEnd,
        KeyKind::Finish,
    ] {
        all.push(t.key_kind(k).into());
    }
    for topic in [
        Topic::Bivouac,
        Topic::Access,
        Topic::LeaveRoutes,
        Topic::Fire,
    ] {
        all.push(t.rule_topic(topic).into());
    }
    for v in [
        Verdict::Forbidden,
        Verdict::ForbiddenInPlaces,
        Verdict::Tolerated,
        Verdict::ConsentRequired,
        Verdict::AvoidRecommended,
        Verdict::Allowed,
        Verdict::NoRule,
    ] {
        all.push(t.rule_verdict(v).into());
    }
    for i in [
        Inactive::NotInForce,
        Inactive::OutOfPeriod,
        Inactive::Daytime,
        Inactive::ZoneNotSensitive,
        Inactive::NoSunEvent,
    ] {
        all.push(t.rule_inactive(i).into());
    }
    for k in [
        SourceKind::Law,
        SourceKind::SignedAct,
        SourceKind::DraftAct,
        SourceKind::DecisionSummary,
        SourceKind::Leaflet,
        SourceKind::OfficialPage,
        SourceKind::ThirdPartyPage,
        SourceKind::DataService,
    ] {
        all.push(t.source_kind(k).into());
    }
    for c in [
        Cause::Closure { ways: vec![1] },
        Cause::Late { limit: at },
        Cause::Conditions(Condition::Fog),
        Cause::Conditions(Condition::WindRising),
        Cause::Conditions(Condition::Snow),
        Cause::Conditions(Condition::TemperatureDrop),
        Cause::Deviation,
        Cause::NoTime,
        Cause::Pace,
    ] {
        all.push(t.cause(&c));
    }
    let target = |kind, name: Option<&str>| Target {
        kind,
        pos: LonLat::new(6.0, 45.0),
        name: name.map(str::to_owned),
    };
    for tg in [
        Some(target(TargetKind::Destination, Some("G"))),
        Some(target(TargetKind::Destination, None)),
        Some(target(TargetKind::Car, Some("G"))),
        Some(target(TargetKind::Start, None)),
        None,
    ] {
        all.push(proposal_text(tg.as_ref(), Lang::En));
    }
    assert_english(&all, &["X", "Y", "S", "L", "P", "P1", "G", "C."]);
    // Every text changes with the language, the proper names aside.
    let f = text::of(Lang::Fr);
    assert_ne!(f.recap_first(), t.recap_first());
    assert_ne!(f.rules_note(), t.rules_note());
    assert_ne!(f.never_guarantee(), t.never_guarantee());
}
