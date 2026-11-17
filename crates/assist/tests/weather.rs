//! The assistant on synthetic weather (X15, #149): briefing (A1), wax (A2), snow quality
//! (A3), crowd (A4), synthesis (A7), alerts (A8) and best time to leave (A9) on the
//! versioned weather sets of `tests/data/weather/` (see `support/weather_sets.rs`), with the
//! reference layers of `fixtures/` (graph, pistes, zones, coverage) and synthetic snow map,
//! domains, piste status and avalanche bulletins built here.
//!
//! Each test checks what the weather of its set must give (dry cold: old dry snow and a
//! hard wax; a thaw: wet snow, klister, rain; wind: gusts and the wind rising...); the
//! texts and figures of every output are pinned by golden fingerprints
//! (`fixtures/golden/assist/weather-<set>.json`, `SKIMAP_BLESS=1` after review).

#[path = "../../../fixtures/support/mod.rs"]
mod fixtures;
#[path = "support/weather_sets.rs"]
mod weather_sets;

use chrono::{Duration, NaiveDate, NaiveDateTime, NaiveTime};
use skimap_assist::inputs::{Paths, SiteData};
use skimap_assist::layers::{
    Area, Bera, BeraAspects, BeraBulletin, BeraRisk, BeraStatus, Domain, Itinerary, OpenState,
    Practice, SnowMap, Status, WeatherPoint, multipolygon,
};
use skimap_common::geom::LonLat;
use skimap_common::grid::{GeoTransform, Grid};
use skimap_common::layout::DataLayout;
use skimap_common::site::Site;
/// Evaluation site of `data/sites.json`, test data.
static CERVIERES: std::sync::LazyLock<skimap_common::site::Site> =
    std::sync::LazyLock::new(|| *skimap_common::site::site("cervieres").expect("evaluation site"));
/// Evaluation site of `data/sites.json`, test data.
static CHAPELLE_DES_BOIS: std::sync::LazyLock<skimap_common::site::Site> =
    std::sync::LazyLock::new(|| {
        *skimap_common::site::site("chapelle-des-bois").expect("evaluation site")
    });
use weather_sets::{SETS, set};

fn site_of(name: &str) -> Site {
    match set(name).site {
        "cervieres" => *CERVIERES,
        _ => *CHAPELLE_DES_BOIS,
    }
}

/// A uniform snow map of `depth_cm` over the site, valid at 06 h UTC of `date`.
fn snow_map(site: &Site, date: NaiveDate, depth_cm: f32) -> SnowMap {
    let b = site.bbox;
    let (x0, y0) =
        skimap_common::crs::to_lambert93(LonLat::new(b.west - 0.05, b.north + 0.05)).unwrap();
    let (x1, y1) =
        skimap_common::crs::to_lambert93(LonLat::new(b.east + 0.05, b.south - 0.05)).unwrap();
    let cell = 100.0;
    let w = ((x1 - x0) / cell).ceil() as usize;
    let h = ((y0 - y1) / cell).ceil() as usize;
    let grid = |v: f32| {
        Grid::filled(
            w,
            h,
            GeoTransform::north_up(x0, y0, cell),
            skimap_common::crs::Crs::Lambert93,
            v,
        )
    };
    SnowMap {
        valid_at: date.and_hms_opt(6, 0, 0).unwrap().and_utc(),
        depth: grid(depth_cm),
        low: Some(grid(depth_cm * 0.7)),
        high: Some(grid(depth_cm * 1.3)),
    }
}

fn rect(west: f64, south: f64, east: f64, north: f64) -> Area {
    let g = serde_json::json!({"type": "Polygon", "coordinates": [[
        [west, south], [east, south], [east, north], [west, north], [west, south]
    ]]});
    Area::new("domaine", multipolygon(&g))
}

/// The layers of the fixtures with the weather of a set and, for the day of the set,
/// a snow map of `snow_cm` (none: the snow map of the fixtures), two domains, the state of
/// three pistes and a bulletin.
fn data_with(name: &str, snow_cm: Option<f32>, bera: Option<Bera>) -> SiteData {
    let s = set(name);
    let site = site_of(name);
    let paths = Paths {
        meteo: vec![weather_sets::path(name)],
        ..Paths::default()
    };
    let mut d = SiteData::load(&DataLayout::new(fixtures::data()), &site, &paths).unwrap();
    assert!(d.weather.is_some(), "{:?}", d.missing);
    let date = s.date();
    if let Some(cm) = snow_cm {
        d.snows = vec![snow_map(&site, date, cm)];
    }
    // Two domains: the half of the site of each forecast point, with its pistes.
    let mid = (s.points[0].lon + s.points[1].lon) / 2.0;
    let b = site.bbox;
    let halves = if s.points[0].lon < s.points[1].lon {
        [(b.west, mid), (mid, b.east)]
    } else {
        [(mid, b.east), (b.west, mid)]
    };
    d.domains = s
        .points
        .iter()
        .zip(halves)
        .enumerate()
        .map(|(i, (p, (w, e)))| {
            let area = rect(w, b.south, e, b.north);
            let ways = d
                .pistes
                .iter()
                .filter(|pi| pi.line.iter().any(|q| area.contains(*q)))
                .map(|pi| pi.way_id)
                .collect();
            Domain {
                id: format!("domaine-{i}"),
                // The first one has the name of the site: its published season applies.
                name: if i == 0 {
                    site.name.to_owned()
                } else {
                    p.name.to_owned()
                },
                km_nordic: 30.0 + 10.0 * i as f64,
                point: LonLat::new(p.lon, p.lat),
                area,
                ways,
            }
        })
        .collect();
    // Three named pistes of the first domain: open, closed, open.
    let mut its = Vec::new();
    for pi in d.pistes.iter().filter(|pi| {
        pi.name.is_some() && d.domains[0].ways.contains(&pi.way_id) && pi.line.len() >= 2
    }) {
        let name = pi.name.clone().unwrap();
        if its.iter().any(|i: &Itinerary| i.name == name) {
            continue;
        }
        let state = if its.len() == 1 {
            OpenState::Closed
        } else {
            OpenState::Open
        };
        its.push(Itinerary {
            id: format!("nordicfrance:{}", its.len()),
            name,
            source: "nordicfrance".into(),
            sector: Some("test".into()),
            practice: Practice::Nordic,
            state,
            length_km: Some(5.0),
            difficulty: None,
            comment: None,
            line: pi.line.clone(),
            osm_way_ids: vec![pi.way_id],
        });
        if its.len() == 3 {
            break;
        }
    }
    assert_eq!(its.len(), 3);
    d.status = Some(Status {
        polled_at: skimap_assist::sun::local_to_utc(date.and_hms_opt(7, 30, 0).unwrap()),
        itineraries: its,
        messages: vec!["Nordic France : 2/3 ouverts".into()],
        conflicts: vec![],
    });
    d.beras = bera.into_iter().collect();
    // The layers the fixtures do not have, without their local paths.
    for m in &mut d.missing {
        *m = m.split(" : ").next().unwrap_or_default().to_owned();
    }
    d
}

fn bera(date: NaiveDate, level: u8, split: bool) -> Bera {
    Bera {
        site: "cervieres".into(),
        date: date.to_string(),
        massif: Some("Queyras".into()),
        status: BeraStatus::Bulletin,
        pdf_url: None,
        bulletin: Some(BeraBulletin {
            massif: "QUEYRAS".into(),
            date_validity: None,
            risk: BeraRisk {
                risk1: Some(if split { level - 1 } else { level }),
                loc1: split.then(|| "<2200".to_owned()),
                altitude: split.then_some(2200),
                risk2: split.then_some(level),
                loc2: split.then(|| ">2200".to_owned()),
                max: Some(level),
                comment: None,
            },
            aspects: BeraAspects {
                n: true,
                ne: true,
                nw: true,
                ..Default::default()
            },
            accidental: None,
            natural: None,
            snow_quality: None,
        }),
        notice: String::new(),
        links: vec![],
    }
}

/// A bulletin of the day without a level: not readable offline (Queyras) or no BERA in the
/// massif (Jura).
fn bera_status(date: NaiveDate, status: BeraStatus) -> Bera {
    Bera {
        status,
        bulletin: None,
        ..bera(date, 1, false)
    }
}

/// The data of a set: 60 cm of snow at Cervières (35 cm at Chapelle-des-Bois, none but the
/// snow map of the fixtures in November) and a bulletin per set (level 2, 3 or 4, with or
/// without an altitude split; not readable offline in the fog; no BERA in the Jura; none
/// loaded in November).
fn data(name: &str) -> SiteData {
    let date = set(name).date();
    match name {
        "froid-sec" => data_with(name, Some(60.0), Some(bera(date, 2, false))),
        "vent-fort" => data_with(name, Some(60.0), Some(bera(date, 3, true))),
        "brouillard" => data_with(
            name,
            Some(60.0),
            Some(bera_status(date, BeraStatus::Unavailable)),
        ),
        "neige-fraiche" => data_with(name, Some(60.0), Some(bera(date, 4, true))),
        "regel" => data_with(name, Some(60.0), Some(bera(date, 2, true))),
        "redoux" => data_with(
            name,
            Some(35.0),
            Some(bera_status(date, BeraStatus::NoBera)),
        ),
        "novembre" => data_with(name, None, None),
        _ => panic!("{name}"),
    }
}

/// The access town of the site (Briançon, Pontarlier), for the distances of the briefing.
fn home(name: &str) -> LonLat {
    match set(name).site {
        "cervieres" => LonLat::new(6.6426, 44.8986),
        _ => LonLat::new(6.3554, 46.9035),
    }
}

fn point<'a>(d: &'a SiteData, id: &str) -> &'a WeatherPoint {
    d.weather.as_ref().unwrap().point(id).unwrap()
}

fn at(date: NaiveDate, h: u32, m: u32) -> NaiveDateTime {
    date.and_time(NaiveTime::from_hms_opt(h, m, 0).unwrap())
}

#[test]
fn the_sets_are_the_rules() {
    let bless = std::env::var_os("SKIMAP_BLESS").is_some();
    for s in &SETS {
        let now = weather_sets::text(s);
        let parsed: serde_json::Value = serde_json::from_str(&now).unwrap();
        assert_eq!(parsed, weather_sets::file(s));
        let p = weather_sets::path(s.name);
        if bless {
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(&p, &now).unwrap();
        }
        let before = std::fs::read_to_string(&p)
            .unwrap_or_else(|e| panic!("{}: {e} (SKIMAP_BLESS=1 to write it)", p.display()));
        assert!(before == now, "{} differs from its rules", p.display());
        // Read by the layer reader: 96 hours at each point, in order.
        let w = skimap_assist::layers::Weather::load(std::slice::from_ref(&p)).unwrap();
        assert_eq!(w.points.len(), 2);
        for wp in &w.points {
            assert_eq!(wp.hours.len(), 96);
            assert_eq!(wp.hours[0].time, s.start());
            assert!(wp.covers(s.date(), 0, 24));
        }
        assert_eq!(
            w.files[0].span,
            Some((s.start(), s.start() + Duration::hours(95)))
        );
    }
}

/// Every output of the assistant on a set, for the golden fingerprints: the briefing of
/// the day and of the next one (text, voice), per point the snow quality of the day, the
/// wax and the best time for 2 h, the weather replay of the alerts; the synthesis.
fn outputs(name: &str, d: &SiteData) -> Vec<(String, String)> {
    use skimap_assist::{alerts, best_time, briefing, synthesis, wax};
    let date = set(name).date();
    let mut out = Vec::new();
    let opts = briefing::Options {
        home: Some(home(name)),
    };
    let b = briefing::brief(d, date, 1, &opts);
    for (i, day) in b.days.iter().enumerate() {
        out.push((format!("briefing.{i}.text"), day.text.join("\n")));
        out.push((format!("briefing.{i}.voice"), briefing::spoken(day)));
    }
    let crowd = b.days[0].crowd.as_ref().map(|c| c.level);
    for q in &b.days[0].snow_quality {
        let p = point(d, &q.point);
        out.push((
            format!("quality.{}", q.point),
            skimap_assist::layer::quality_table(q),
        ));
        if let Some(w) = wax::day_wax(p, q) {
            out.push((
                format!("wax.{}", q.point),
                format!(
                    "{}\n{}\n{}\n{}",
                    w.text,
                    w.morning.reasons.join(" ; "),
                    w.afternoon.reasons.join(" ; "),
                    w.voice().announce().text()
                ),
            ));
        }
        let bt = best_time::best_time(
            p,
            Some(q),
            crowd,
            &best_time::Request {
                date,
                duration_min: 120.0,
                earliest: NaiveTime::from_hms_opt(8, 0, 0).unwrap(),
                classes: vec![],
            },
        );
        out.push((format!("best_time.{}", q.point), bt.text.clone()));
    }
    for p in &d.weather.as_ref().unwrap().points {
        let a = alerts::day_replay(p, date, alerts::Params::default());
        out.push((
            format!("alerts.{}", p.id),
            a.iter()
                .map(|x| {
                    format!(
                        "{} {:?} {} ({})",
                        x.at.format("%H:%M"),
                        x.kind,
                        x.text,
                        x.spoken
                    )
                })
                .collect::<Vec<_>>()
                .join("\n"),
        ));
    }
    let s = synthesis::synthesise(d, date);
    let voice: Vec<String> = s.voice.iter().map(|e| e.announce().text()).collect();
    out.push((
        "synthesis".into(),
        format!("{}\n{}", s.text(), voice.join(" ")),
    ));
    out
}

#[test]
#[ignore]
fn print_outputs() {
    for s in &SETS {
        let d = data(s.name);
        for (k, v) in outputs(s.name, &d) {
            println!("=== {} {k}\n{v}", s.name);
        }
    }
}

/// Golden fingerprints of every output of every set: texts exact (the rules of the sets
/// are fixed, the computation deterministic).
#[test]
fn golden_outputs_of_every_set() {
    for s in &SETS {
        let d = data(s.name);
        let mut k = fixtures::golden::Golden::new(&format!("assist/weather-{}", s.name));
        let all = outputs(s.name, &d);
        for (key, v) in &all {
            k.lines(key, v);
            // Never « sans danger », in any output.
            let low = v.to_lowercase();
            assert!(!low.contains("sans danger"), "{} {key}: {v}", s.name);
        }
        k.check();
    }
}

// ---------------------------------------------------------------------------------------
// A3: snow quality.

use skimap_assist::snow_quality::{self, DayQuality, Params, Quality};

fn quality(d: &SiteData, id: &str, date: NaiveDate) -> DayQuality {
    snow_quality::day_quality(point(d, id), date, &Params::default(), Some(60.0)).unwrap()
}

fn class<'a>(q: &'a DayQuality, code: &str) -> &'a snow_quality::ClassDay {
    q.classes.iter().find(|c| c.code == code).unwrap()
}

/// The surface letters of a class from 06 h to 19 h (as in the table of the layer).
fn letters(c: &snow_quality::ClassDay) -> String {
    c.hours
        .iter()
        .map(|h| match h.quality {
            Quality::Fresh => 'F',
            Quality::Windblown => 'V',
            Quality::OldDry => 'A',
            Quality::Transformed => 'T',
            Quality::Wet => 'H',
            Quality::Slush => 'S',
            Quality::Hard => 'D',
            Quality::Icy => 'G',
        })
        .collect()
}

#[test]
fn a3_dry_cold_stays_old_dry_snow_everywhere() {
    let d = data("froid-sec");
    let date = set("froid-sec").date();
    for id in ["le-laus", "les-fonts"] {
        let q = quality(&d, id, date);
        assert_eq!(q.flat, Quality::OldDry);
        assert_eq!(q.classes.len(), 17);
        for c in &q.classes {
            assert_eq!(letters(c), "AAAAAAAAAAAAAA", "{id} {}", c.code);
            assert!(c.softens_at.is_none() && c.wet_at.is_none());
            assert_eq!(c.morning, Quality::OldDry);
            // Clear and cold: the surface loses energy all day, no water.
            assert!(
                c.hours
                    .iter()
                    .all(|h| h.liquid_mm == 0.0 && h.fresh_cm == 0.0)
            );
        }
        // Every class the same all day: the best is the flat, over all the daylight.
        let (code, slot) = q.best.clone().unwrap();
        assert_eq!(code, "flat");
        assert_eq!(slot.quality, Quality::OldDry);
        assert_eq!(slot.score, 2.5);
        assert_eq!(slot.end.time(), NaiveTime::from_hms_opt(17, 0, 0).unwrap());
        assert!(slot.start + Duration::minutes(30) >= q.daylight.0);
        assert!(slot.start - Duration::minutes(30) < q.daylight.0);
        assert!(q.summary[0].contains("neige ancienne froide et sèche entre 10 h et 15 h"));
        assert!(!q.summary.iter().any(|s| s.contains("ramollissement")));
        assert!(q.enough_snow);
        assert_eq!(q.snow_cm, Some(60.0));
    }
}

#[test]
fn a3_fresh_snow_then_wind_blows_it() {
    let d = data("vent-fort");
    let date = set("vent-fort").date();
    let q = quality(&d, "le-laus", date);
    // 9.6 cm overnight, calm: fresh until 10 h; gusts of 75 km/h from 11 h: wind-blown.
    for c in &q.classes {
        assert_eq!(letters(c), "FFFFFVVVVVVVVV", "{}", c.code);
    }
    assert_eq!(q.flat, Quality::Windblown);
    assert_eq!(class(&q, "flat").morning, Quality::Fresh);
    let h7 = &class(&q, "flat").hours[1];
    assert!((8.0..9.6).contains(&h7.fresh_cm), "{h7:?}");
    // The best slot: the fresh snow of the morning.
    let (_, slot) = q.best.clone().unwrap();
    assert_eq!(slot.quality, Quality::Fresh);
    assert_eq!(slot.end.time(), NaiveTime::from_hms_opt(11, 0, 0).unwrap());
    // The snow starts at 20 h the day before: old snow until 19 h.
    let before = quality(&d, "le-laus", date - Duration::days(1));
    assert_eq!(before.flat, Quality::OldDry);
    assert_eq!(letters(&before.classes[0]), "AAAAAAAAAAAAAA");
}

#[test]
fn a3_fresh_snow_all_day() {
    let d = data("neige-fraiche");
    let date = set("neige-fraiche").date();
    let q = quality(&d, "le-laus", date);
    for c in &q.classes {
        assert_eq!(letters(c), "FFFFFFFFFFFFFF", "{}", c.code);
    }
    assert_eq!(q.flat, Quality::Fresh);
    // 19.2 cm from 16 h the day before, settled by 1.5 % an hour.
    let h7 = &class(&q, "flat").hours[1];
    assert!((16.0..19.2).contains(&h7.fresh_cm), "{h7:?}");
    assert_eq!(q.best.as_ref().unwrap().1.score, 3.0);
}

#[test]
fn a3_a_thaw_wets_everything_without_refreeze() {
    let d = data("redoux");
    let date = set("redoux").date();
    let q = quality(&d, "chapelle-des-bois", date);
    // +3 °C at night under clouds: no crust in the morning; rain from 10 h: heavy snow.
    assert!(
        class(&q, "flat").morning.is_wet(),
        "{}",
        letters(class(&q, "flat"))
    );
    assert!(matches!(q.flat, Quality::Wet | Quality::Slush));
    for c in q.classes.iter().filter(|c| c.class.slope_deg == 20.0) {
        assert_eq!(
            c.wet_at.map(|t| t.time()),
            NaiveTime::from_hms_opt(10, 0, 0)
        );
        assert!(!matches!(c.morning, Quality::Hard | Quality::Icy));
    }
    assert!(q.summary.iter().any(|s| s.starts_with("Neige lourde : ")));
}

#[test]
fn a3_refreeze_then_spring_snow() {
    let d = data("regel");
    let date = set("regel").date();
    let q = quality(&d, "le-laus", date);
    // Rained on the day before, clear night at −8 °C: a crust in the morning, icy where
    // the rain and the sun of the day before left the most water.
    assert_eq!(class(&q, "flat").morning, Quality::Hard);
    assert!(
        q.classes.iter().any(|c| c.morning == Quality::Icy),
        "{:?}",
        q.classes
            .iter()
            .map(|c| (c.code.clone(), letters(c)))
            .collect::<Vec<_>>()
    );
    // East softens first, west last; the flat transformed at 10 h, heavy from 12 h.
    let soft = |code: &str| class(&q, code).softens_at.unwrap();
    assert!(soft("E20") < soft("W20"), "{} {}", soft("E20"), soft("W20"));
    assert!(soft("SE20") <= soft("S20"));
    assert_eq!(
        soft("flat").time(),
        NaiveTime::from_hms_opt(10, 0, 0).unwrap()
    );
    assert_eq!(
        class(&q, "flat").wet_at.map(|t| t.time()),
        NaiveTime::from_hms_opt(12, 0, 0)
    );
    // North 40°: transformed the latest, never heavy.
    assert!(class(&q, "N40").wet_at.is_none());
    assert!(q.classes.iter().all(|c| soft(&c.code) <= soft("N40")));
    assert!(
        q.summary
            .iter()
            .any(|s| s.starts_with("Pentes de 20° : ramollissement vers "))
    );
    assert!(q.summary.iter().any(|s| s.starts_with("Neige lourde : ")));
}

#[test]
fn a3_fog_day_crusts_the_south_slopes_only() {
    let d = data("brouillard");
    let date = set("brouillard").date();
    let q = quality(&d, "le-laus", date);
    assert_eq!(q.flat, Quality::OldDry);
    assert_eq!(letters(class(&q, "flat")), "AAAAAAAAAAAAAA");
    // The sun of the day before softened the steep south-facing slopes, refrozen.
    for code in ["SE40", "S40", "SW40"] {
        assert_eq!(class(&q, code).morning, Quality::Hard, "{code}");
    }
    for code in ["N20", "N40", "NE40", "NW40"] {
        assert!(class(&q, code).softens_at.is_none(), "{code}");
    }
}

#[test]
fn a3_no_snow_no_quality() {
    let d = data("novembre");
    let date = set("novembre").date();
    let b = skimap_assist::briefing::day(&d, date, &skimap_assist::briefing::Options::default());
    assert_eq!(b.snow_quality.len(), 2);
    for q in &b.snow_quality {
        assert!(!q.enough_snow && q.best.is_none(), "{q:?}");
        assert!(q.snow_cm.unwrap() < snow_quality::MIN_SNOW_CM);
        assert!(q.summary[0].starts_with("Pas ou trop peu de neige estimée au point"));
        assert!(skimap_assist::wax::day_wax(point(&d, &q.point), q).is_none());
    }
}

// ---------------------------------------------------------------------------------------
// A2: wax.

use skimap_assist::wax::{self, DayWax, SnowType};

fn day_wax(name: &str, id: &str) -> DayWax {
    let d = data(name);
    let q = quality(&d, id, set(name).date());
    wax::day_wax(point(&d, id), &q).unwrap()
}

#[test]
fn a2_dry_cold_hard_wax_by_the_swix_columns() {
    let w = day_wax("froid-sec", "le-laus");
    let m = &w.morning;
    assert_eq!(m.conditions.snow_type, SnowType::Old);
    assert_eq!(m.conditions.hours, (9, 12));
    assert_eq!(m.conditions.surface, (Quality::OldDry, Quality::OldDry));
    // −11.1 °C, old snow: V30 (old −5 to −15, centre −10) before V20 (centre −14).
    assert_eq!(m.kick.product, "V30 Blue");
    assert_eq!(m.kick_alternative.unwrap().product, "Nordic GripWax blue");
    assert!(m.kick_reliable && m.skins.is_none());
    assert_eq!(m.glide.product, "PS6 Blue");
    // Clear sky: the snow 2 °C under the air.
    assert!((m.conditions.snow_c - (m.conditions.air_c - 2.0 * 0.95)).abs() < 0.11);
    assert_eq!(
        m.glide_alternative.unwrap().product,
        "Base Performance Hot Wax blue"
    );
    assert_eq!(m.conditions.humidity_pct, Some(55.0));
    assert!(w.text.ends_with("Même fartage l'après-midi."), "{}", w.text);
    assert_eq!(
        w.voice().announce().text(),
        "Fartage. Fart de retenue bleu. Fart de glisse pour neige froide."
    );
    // Higher and colder at Les Fonts: V20 and PS5 in the morning, the afternoon differs.
    let f = day_wax("froid-sec", "les-fonts");
    assert_eq!(f.morning.kick.product, "V20 Green");
    assert_eq!(f.morning.glide.product, "PS5 Turquoise");
    assert_eq!(f.afternoon.kick.product, "V30 Blue");
    assert!(f.text.contains(" Après-midi 12 h–16 h"), "{}", f.text);
    assert!(f.sources.iter().any(|s| s.contains("v20-green")));
    assert!(
        f.sources.windows(2).all(|p| p[0] < p[1]),
        "sorted, no duplicate"
    );
}

#[test]
fn a2_fresh_snow_new_snow_column() {
    let w = day_wax("neige-fraiche", "le-laus");
    assert_eq!(w.morning.conditions.snow_type, SnowType::New);
    // −5.3 °C: V30 (new −2 to −10, centre −6); −4.1 °C in the afternoon: V40 (centre −4).
    assert_eq!(w.morning.kick.product, "V30 Blue");
    assert_eq!(w.afternoon.kick.product, "V40 Blue Extra");
    // Snowfall 13–15 h in the afternoon window: new snow too.
    assert!(w.afternoon.conditions.snowfall_cm >= 1.5);
    assert_eq!(w.afternoon.conditions.snow_type, SnowType::New);
    assert!(w.morning.kick_reliable && w.afternoon.kick_reliable);
}

#[test]
fn a2_a_thaw_takes_klister_and_skins() {
    let w = day_wax("redoux", "chapelle-des-bois");
    for a in [&w.morning, &w.afternoon] {
        assert_eq!(a.conditions.snow_type, SnowType::Wet);
        assert_eq!(a.kick.product, "KX75 Red Extra Wet Klister");
        assert!(a.kick.klister);
        assert!(!a.kick_reliable);
        assert_eq!(
            a.skins.as_deref(),
            Some("skis à écailles ou peaux conseillés (pluie ou neige mouillée qui tombe)")
        );
        assert_eq!(a.glide.product, "PS10 Yellow");
        assert_eq!(a.conditions.snow_c, 0.0);
        assert!(a.conditions.rain_mm >= 3.0);
    }
    assert_eq!(
        w.voice().announce().text(),
        "Fartage. Prenez des skis à écailles ou des peaux. Fart de glisse pour neige chaude et humide."
    );
}

#[test]
fn a2_refrozen_then_soft_no_wax_holds() {
    let w = day_wax("regel", "le-laus");
    let m = &w.morning;
    assert_eq!(m.conditions.surface.0, Quality::Hard);
    assert!(m.conditions.surface.1.is_wet());
    assert!(!m.kick_reliable);
    assert!(
        m.skins.as_deref().unwrap().contains("au début du créneau"),
        "{:?}",
        m.skins
    );
    assert!(m.kick.klister);
    // Wet in the afternoon, above +1 °C: a red klister.
    assert_eq!(w.afternoon.conditions.snow_type, SnowType::Wet);
    assert!(
        w.afternoon.kick.product.starts_with("KX65")
            || w.afternoon.kick.product.starts_with("KX75")
    );
}

#[test]
fn a2_around_zero_in_the_fog() {
    let w = day_wax("brouillard", "le-laus");
    // Old snow at 0 °C: V60 (old +1 to −1); overcast: the snow at the air temperature.
    assert_eq!(w.morning.kick.product, "V60 Red/Silver");
    assert_eq!(w.morning.glide.product, "PS8 Red");
    assert_eq!(w.morning.conditions.snow_c, 0.0);
    assert_eq!(
        w.morning.voice_glide(),
        skimap_voice::bank::Glide::Warm,
        "PS8 at 0 °C or above is warm"
    );
}

// ---------------------------------------------------------------------------------------
// A1: briefing, and A4: crowd.

use skimap_assist::briefing::{self, Candidate, Factor};
use skimap_assist::crowd::CrowdLevel;

fn brief(name: &str, offset: i64) -> briefing::DayBriefing {
    let d = data(name);
    let opts = briefing::Options {
        home: Some(home(name)),
    };
    briefing::day(&d, set(name).date() + Duration::days(offset), &opts)
}

/// The reason of a factor of a candidate: (points, text).
fn why(c: &Candidate, f: Factor) -> Option<(f64, String)> {
    c.reasons
        .iter()
        .find(|r| r.factor == f)
        .map(|r| (r.points, r.text.clone()))
}

fn domain<'a>(b: &'a briefing::DayBriefing, name: &str) -> &'a Candidate {
    b.domains.iter().find(|c| c.name == name).unwrap()
}

#[test]
fn a1_dry_cold_sunny_day() {
    let b = brief("froid-sec", 0);
    assert!(b.weather_available);
    assert_eq!(b.status, "day");
    assert_eq!(b.bera_level, Some(2));
    let c = domain(&b, "Cervières");
    // Status: 2 of 3 itineraries open, 10 of 15 km: 3 × 2/3.
    assert_eq!(
        why(c, Factor::Status),
        Some((2.0, "2 itinéraires ouverts sur 3 (10 km sur 15)".into()))
    );
    let (p, t) = why(c, Factor::Snow).unwrap();
    assert_eq!(p, 2.0);
    assert!(t.starts_with("60 cm de neige estimée (médiane, carte du 12/01, 100 % des points ≥ 20 cm) : suffisante"), "{t}");
    assert_eq!(
        why(c, Factor::Wind),
        Some((0.0, "vent faible (rafales 15 km/h)".into()))
    );
    assert_eq!(
        why(c, Factor::Cold),
        Some((-0.5, "froid, ressenti -17 °C".into()))
    );
    assert_eq!(why(c, Factor::Sun), Some((0.5, "ensoleillé".into())));
    assert_eq!(
        why(c, Factor::SnowSurface),
        Some((0.0, "surface : neige ancienne froide et sèche".into()))
    );
    assert_eq!(
        why(c, Factor::Size),
        Some((0.6, "30 km de pistes nordiques (OSM)".into()))
    );
    let (p, t) = why(c, Factor::Distance).unwrap();
    assert!((p + 0.02 * c.distance_km.unwrap()).abs() < 1e-9 && t.starts_with("à 8 km"));
    // Les Fonts, higher: very cold.
    let f = domain(&b, "Les Fonts");
    assert_eq!(
        why(f, Factor::Cold),
        Some((-1.5, "très froid, ressenti -21 °C".into()))
    );
    assert_eq!(
        why(f, Factor::Status),
        Some((
            0.0,
            "pas de source d'état des pistes pour ce domaine".into()
        ))
    );
    assert!(why(f, Factor::Fog).is_none() && why(f, Factor::Rain).is_none());
    // The score is the sum of the points; the best first.
    for c in b.domains.iter().chain(&b.itineraries) {
        let s: f64 = c.reasons.iter().map(|r| r.points).sum();
        assert!((c.score - s).abs() < 1e-12);
    }
    assert!(b.domains.windows(2).all(|w| w[0].score >= w[1].score));
    assert_eq!(b.domains[0].name, "Cervières");
    // The closed itinerary is not among the « non fermés » and ranks last.
    assert_eq!(b.itineraries.last().unwrap().state, OpenState::Closed);
    assert_eq!(
        why(b.itineraries.last().unwrap(), Factor::Status),
        Some((-10.0, "fermé".into()))
    );
    // A4: a Tuesday without holidays, sunny: calm.
    let crowd = b.crowd.as_ref().unwrap();
    assert_eq!(crowd.level, CrowdLevel::Calm);
    assert_eq!(crowd.reasons, ["soleil"]);
    assert_eq!(crowd.score, 1.5);
    assert_eq!(crowd.open, Some(true));
    // Voice: the best open itinerary is recommended, the weather of the best domain.
    let voice = briefing::spoken(&b);
    assert!(
        voice.contains("Risque d'avalanche deux sur cinq"),
        "{voice}"
    );
    assert!(voice.contains("Conseillé aujourd'hui."), "{voice}");
    assert!(voice.contains("Neige froide et sèche."), "{voice}");
    assert!(voice.contains("Affluence faible."), "{voice}");
    // The day after: last known status, said.
    let next = brief("froid-sec", 1);
    assert_eq!(next.status, "last_known");
    assert!(
        next.notes
            .iter()
            .any(|n| n.starts_with("état des pistes : dernier connu (12/01 07:30)")),
        "{:?}",
        next.notes
    );
    assert!(
        why(domain(&next, "Cervières"), Factor::Status)
            .unwrap()
            .1
            .ends_with("(dernier état connu)")
    );
}

#[test]
fn a1_wind_and_fresh_snow() {
    let b = brief("vent-fort", 0);
    let c = domain(&b, "Cervières");
    assert_eq!(
        why(c, Factor::Wind),
        Some((-2.0, "rafales jusqu'à 75 km/h".into()))
    );
    assert_eq!(
        why(c, Factor::FreshSnow),
        Some((0.5, "10 cm de neige fraîche en 24 h".into()))
    );
    assert_eq!(
        why(c, Factor::Cold),
        Some((-1.5, "très froid, ressenti -24 °C".into()))
    );
    assert_eq!(
        why(c, Factor::SnowSurface),
        Some((-0.5, "surface : neige fraîche soufflée par le vent".into()))
    );
    // Snowfall since the map (07 h local) until 09 h: 0.8 cm, said with the depth.
    let s = c.snow.unwrap();
    assert!((s.added_cm - 0.8).abs() < 1e-9, "{s:?}");
    assert_eq!(s.median_cm, 60.0);
    assert!((s.depth_cm() - 60.8).abs() < 1e-9);
    assert!(
        why(c, Factor::Snow)
            .unwrap()
            .1
            .contains("dont 1 cm tombés depuis")
    );
    assert!(why(c, Factor::Sun).is_none(), "60 % of clouds");
    // A4: a Saturday.
    let crowd = b.crowd.as_ref().unwrap();
    assert_eq!(crowd.level, CrowdLevel::Moderate);
    assert_eq!(crowd.reasons, ["samedi"]);
    assert_eq!(why(c, Factor::Crowd).unwrap().0, -0.25);
    assert!(briefing::spoken(&b).contains("Neige soufflée par le vent."));
}

#[test]
fn a1_fog() {
    let b = brief("brouillard", 0);
    let c = domain(&b, "Cervières");
    assert_eq!(
        why(c, Factor::Fog),
        Some((-1.0, "brouillard 4 h sur 7".into()))
    );
    assert!(why(c, Factor::Sun).is_none());
    // The bulletin of the day could not be read: no level, nothing said of it.
    assert_eq!(b.bera_level, None);
    assert!(!b.text.iter().any(|t| t.starts_with("BERA")));
    let w = c.weather.as_ref().unwrap();
    assert_eq!(w.fog_hours, 4);
    assert_eq!(w.hours, 7);
    assert!(
        briefing::spoken(&b).contains("Brouillard"),
        "{}",
        briefing::spoken(&b)
    );
}

#[test]
fn a1_a4_fresh_snow_draws_the_crowd_the_next_day() {
    let b = brief("neige-fraiche", 0);
    let c = domain(&b, "Cervières");
    assert_eq!(
        why(c, Factor::FreshSnow),
        Some((0.5, "19 cm de neige fraîche en 24 h".into()))
    );
    assert_eq!(
        why(c, Factor::Snowfall),
        Some((0.0, "2 cm de neige pendant la journée".into()))
    );
    assert_eq!(
        why(c, Factor::SnowSurface),
        Some((0.5, "surface : neige fraîche".into()))
    );
    // The snow of the two days before the day is 9.6 cm: no bonus yet; mean cloud 36 %.
    let crowd = b.crowd.as_ref().unwrap();
    assert_eq!(crowd.level, CrowdLevel::Calm);
    assert!(crowd.reasons.is_empty(), "{:?}", crowd.reasons);
    // The next day: 20.8 cm in the two days before, sunny: +1, +0.5 and +0.5.
    let next = brief("neige-fraiche", 1);
    let crowd = next.crowd.as_ref().unwrap();
    assert_eq!(crowd.reasons, ["neige fraîche (21 cm)", "soleil"]);
    assert_eq!(crowd.score, 3.0);
    assert_eq!(crowd.level, CrowdLevel::Moderate);
    assert_eq!(
        why(domain(&next, "Cervières"), Factor::Crowd),
        Some((
            -0.25,
            "niveau d'affluence modéré (neige fraîche (21 cm), soleil)".into()
        ))
    );
}

#[test]
fn a1_a4_thaw_rain_and_holidays() {
    let b = brief("redoux", 0);
    let c = domain(&b, "Chapelle-des-Bois");
    assert_eq!(why(c, Factor::Rain), Some((-1.5, "pluie (9,0 mm)".into())));
    assert_eq!(
        why(c, Factor::Wind),
        Some((0.0, "vent modéré (rafales 35 km/h)".into()))
    );
    assert_eq!(
        why(c, Factor::Cold),
        Some((0.0, "7 °C en moyenne, ressenti 2 °C".into()))
    );
    // 35 cm: enough, but at the limit.
    assert_eq!(why(c, Factor::Snow).unwrap().0, 0.5);
    assert!(why(c, Factor::Snow).unwrap().1.ends_with(" : limite"));
    let crowd = b.crowd.as_ref().unwrap();
    assert_eq!(crowd.level, CrowdLevel::VeryBusy);
    assert_eq!(
        crowd.reasons,
        [
            "vacances d'hiver (zone A, zone B)",
            "vacances de détente (Belgique (FWB))"
        ]
    );
    assert_eq!(crowd.open, None, "no published season at Chapelle-des-Bois");
    // Risoux, 1 °C colder and 5 km/h more: gusts of 40 km/h.
    assert_eq!(
        why(domain(&b, "Risoux"), Factor::Wind),
        Some((-1.0, "rafales jusqu'à 40 km/h".into()))
    );
    // No BERA in the Jura, said by the voice.
    let voice = briefing::spoken(&b);
    assert!(
        voice.contains("Pas de bulletin d'avalanche pour ce massif."),
        "{voice}"
    );
    assert!(voice.contains("Affluence forte."), "{voice}");
}

#[test]
fn a1_a4_november_closed_and_no_snow() {
    let b = brief("novembre", 0);
    // The commune opens the domain on 19 December: closed, after Les Fonts.
    let c = domain(&b, "Cervières");
    assert_eq!(c.state, OpenState::Closed);
    assert!(c.reasons.iter().any(|r| r.points == -10.0
        && r.text == "domaine fermé à cette date (dates d'ouverture publiées par la commune)"));
    assert_eq!(b.domains[0].name, "Les Fonts");
    // Open according to the source, but no snow: no points, said.
    assert_eq!(
        why(c, Factor::Status),
        Some((
            0.0,
            "2 itinéraires ouverts sur 3 (10 km sur 15) mais neige estimée insuffisante : à vérifier"
                .into()
        ))
    );
    let it = &b.itineraries[0];
    assert_eq!(it.state, OpenState::Open);
    assert_eq!(why(it, Factor::Status).unwrap().0, 0.0);
    assert!(
        why(it, Factor::Status)
            .unwrap()
            .1
            .starts_with("ouvert selon la source, mais neige estimée insuffisante")
    );
    let (p, t) = why(c, Factor::Snow).unwrap();
    assert_eq!(p, -3.0);
    assert!(t.ends_with(" : insuffisante pour damer"), "{t}");
    // Nothing is recommended without snow; a Sunday of November: moderate.
    assert!(!briefing::spoken(&b).contains("Conseillé"));
    assert_eq!(b.crowd.as_ref().unwrap().reasons, ["dimanche"]);
    // No bulletin loaded for Cervières: nothing said of the level.
    assert_eq!(b.bera_level, None);
}

#[test]
fn a1_spring_sun_after_the_refreeze() {
    let b = brief("regel", 0);
    let c = domain(&b, "Cervières");
    assert_eq!(why(c, Factor::Sun), Some((0.5, "ensoleillé".into())));
    // From 10 to 15 h the flat is wet: −0.5.
    assert_eq!(
        why(c, Factor::SnowSurface),
        Some((-0.5, "surface : neige humide et lourde".into()))
    );
    // A Saturday in March without holidays, sunny.
    let crowd = b.crowd.as_ref().unwrap();
    assert_eq!(crowd.reasons, ["samedi", "soleil"]);
    assert_eq!(crowd.level, CrowdLevel::Moderate);
}

// ---------------------------------------------------------------------------------------
// A7: synthesis, never « sans danger ».

use skimap_assist::synthesis;

#[test]
fn a7_weather_sentence_of_each_set() {
    let s = |name: &str| synthesis::synthesise(&data(name), set(name).date());
    let w = |name: &str| s(name).sentences[1].clone();
    assert_eq!(
        w("froid-sec"),
        "Météo de 9 h à 16 h (Le Laus, 1750 m) : de -12 à -9 °C, ressenti -17 °C, vent faible, \
         isotherme 0 °C vers 600 m, ciel dégagé."
    );
    assert_eq!(
        w("vent-fort"),
        "Météo de 9 h à 16 h (Le Laus, 1750 m) : de -12 à -5 °C, ressenti -24 °C, rafales \
         jusqu'à 75 km/h, isotherme 0 °C vers 800 m."
    );
    // Between −0.5 and 0 °C: « 0 », not « -0 ».
    assert_eq!(
        w("brouillard"),
        "Météo de 9 h à 16 h (Le Laus, 1750 m) : de 0 à 1 °C, vent faible, brouillard possible \
         (4 h), isotherme 0 °C vers 1700 m, ciel couvert."
    );
    assert_eq!(
        w("neige-fraiche"),
        "Météo de 9 h à 16 h (Le Laus, 1750 m) : de -6 à -4 °C, vent faible, 2 cm de neige, \
         isotherme 0 °C vers 900 m."
    );
    assert_eq!(
        w("redoux"),
        "Météo de 9 h à 16 h (Chapelle-des-Bois, 1090 m) : de 6 à 8 °C, vent modéré (rafales \
         35 km/h), pluie (9,0 mm), ciel couvert."
    );
    assert_eq!(
        w("regel"),
        "Météo de 9 h à 16 h (Le Laus, 1750 m) : de 0 à 5 °C, vent faible, ciel dégagé."
    );
    // Every set: three sentences, never « sans danger », never « sûr ».
    for x in &SETS {
        let t = s(x.name);
        assert_eq!(t.sentences.len(), 3);
        let low = t.text().to_lowercase();
        assert!(
            !low.contains("sans danger") && !low.contains(" sûr"),
            "{low}"
        );
    }
    // The day after the sets: no weather for that day at the point.
    let d = data("froid-sec");
    let late = synthesis::synthesise(&d, set("froid-sec").date() + Duration::days(5));
    assert_eq!(
        late.sentences[1],
        "Pas de prévision météo hors ligne pour ce jour au point Le Laus."
    );
}

#[test]
fn a7_avalanche_sentence_and_voice() {
    use skimap_voice::Event;
    use skimap_voice::bank::Briefing as Line;
    // Level 2 without split; level 3 above 2200 m; level 4.
    let s = synthesis::synthesise(&data("froid-sec"), set("froid-sec").date());
    assert!(
        s.sentences[0].starts_with("Risque d'avalanche deux sur cinq, limité, pentes nord, nord-est, nord-ouest les plus exposées : lisez le bulletin complet avant de partir."),
        "{}",
        s.sentences[0]
    );
    assert_eq!(s.facts.bera_level, Some(2));
    assert_eq!(s.facts.bera_status, Some(BeraStatus::Bulletin));
    assert!(matches!(
        s.voice[0],
        Event::Briefing(Line::Avalanche { level: Some(2) })
    ));
    let s = synthesis::synthesise(&data("vent-fort"), set("vent-fort").date());
    assert!(
        s.sentences[0].starts_with(
            "Risque d'avalanche trois sur cinq, marqué au-dessus de 2200 m (2 sur 5 plus bas), "
        ),
        "{}",
        s.sentences[0]
    );
    let s = synthesis::synthesise(&data("neige-fraiche"), set("neige-fraiche").date());
    assert!(s.sentences[0].starts_with("Risque d'avalanche quatre sur cinq"));
    // Not readable offline: said, never nil.
    let s = synthesis::synthesise(&data("brouillard"), set("brouillard").date());
    assert_eq!(
        s.sentences[0],
        "Le BERA Queyras de ce jour n'est pas disponible hors ligne : consultez-le avant de \
         partir, le risque d'avalanche n'est jamais nul en montagne."
    );
    assert_eq!(s.voice[0], Event::Briefing(Line::AvalancheOffline));
    assert!(
        s.voice
            .iter()
            .any(|e| matches!(e, Event::Briefing(Line::Weather { fog: true, .. })))
    );
    // Not loaded: said too.
    let s = synthesis::synthesise(&data("novembre"), set("novembre").date());
    assert!(s.sentences[0].starts_with("Le BERA Queyras de ce jour n'est pas chargé"));
    assert_eq!(s.facts.bera_status, None);
    // The Jura: no bulletin, avalanches still possible.
    let s = synthesis::synthesise(&data("redoux"), set("redoux").date());
    assert!(s.sentences[0].starts_with(
        "Pas de bulletin d'avalanche pour ce massif (Jura) : des avalanches restent possibles"
    ));
    assert_eq!(s.facts.bera_status, Some(BeraStatus::NoBera));
    assert_eq!(s.voice[0], Event::Briefing(Line::Avalanche { level: None }));
    assert_eq!(s.voice[1], Event::Briefing(Line::AvalanchesPossible));
}

#[test]
fn a7_route_through_avalanche_paths_and_without_network() {
    use skimap_assist::layers::{Areas, Clpa, Route};
    use skimap_voice::Event;
    use skimap_voice::bank::Briefing as Line;
    let mut d = data("vent-fort");
    let date = set("vent-fort").date();
    // A route along the longest piste of the first domain, a historic path over its
    // middle third.
    let line = d
        .pistes
        .iter()
        .filter(|p| d.domains[0].ways.contains(&p.way_id))
        .max_by(|a, b| {
            skimap_assist::layers::line_length_m(&a.line)
                .total_cmp(&skimap_assist::layers::line_length_m(&b.line))
        })
        .unwrap()
        .line
        .clone();
    let n = line.len();
    let (lo, hi) = (line[n / 3], line[2 * n / 3]);
    let (w, e) = (lo.lon.min(hi.lon) - 1e-4, lo.lon.max(hi.lon) + 1e-4);
    let (s, nn) = (lo.lat.min(hi.lat) - 1e-4, lo.lat.max(hi.lat) + 1e-4);
    d.clpa = Some(Clpa {
        covered: true,
        areas: Areas {
            areas: vec![Area::new("CLPA 7", rect(w, s, e, nn).shape)],
        },
        piste_km: Default::default(),
    });
    d.route = Some(Route {
        name: "test".into(),
        line: line.clone(),
    });
    let t = synthesis::synthesise(&d, date);
    let km = t.facts.clpa_km.unwrap();
    assert!(km > 0.05, "{:?}", t.facts);
    assert_eq!(t.facts.clpa_paths, 1);
    assert!(
        t.sentences[0].contains("; votre itinéraire traverse ")
            && t.sentences[0].contains(" km de 1 zones d'avalanche connues (CLPA)"),
        "{}",
        t.sentences[0]
    );
    assert!(
        t.voice
            .contains(&Event::Briefing(Line::RouteAvalancheAreas))
    );
    assert_eq!(t.facts.route.as_deref(), Some("test"));
    let len = skimap_assist::layers::line_length_m(&line) / 1000.0;
    assert!((t.facts.route_km.unwrap() - len).abs() < 0.006);
    assert!(t.sentences[2].starts_with("Sur votre itinéraire : "));
    // Outside the paths: said, with what it does not exclude.
    d.clpa.as_mut().unwrap().areas.areas.clear();
    let t = synthesis::synthesise(&d, date);
    assert_eq!(t.facts.clpa_km, Some(0.0));
    assert!(t.sentences[0].contains(
        "votre itinéraire ne traverse aucune zone d'avalanche connue (CLPA), ce qui n'exclut pas une avalanche ailleurs"
    ));
    assert!(
        !t.voice
            .contains(&Event::Briefing(Line::RouteAvalancheAreas))
    );
    // A massif without the map.
    d.clpa.as_mut().unwrap().covered = false;
    let t = synthesis::synthesise(&d, date);
    assert!(t.sentences[0].contains("pas de carte des avalanches connues (CLPA) dans ce massif"));
    assert_eq!(t.facts.clpa_km, None);
}

// ---------------------------------------------------------------------------------------
// A8: alerts.

use skimap_assist::alerts::{self, Kind};

fn replay(name: &str, id: &str) -> Vec<alerts::Alert> {
    let d = data(name);
    alerts::day_replay(point(&d, id), set(name).date(), alerts::Params::default())
}

#[test]
fn a8_wind_rising_and_temperature_drop() {
    let a = replay("vent-fort", "le-laus");
    let date = set("vent-fort").date();
    let kinds: Vec<(Kind, NaiveDateTime)> = a.iter().map(|x| (x.kind, x.at)).collect();
    // Gusts of 75 km/h from 11 h, seen one hour ahead from 10 h 10 (weather alerts start 10
    // minutes after the departure); the front of 14 h, three hours ahead from 11 h 05.
    assert_eq!(
        kinds,
        [
            (Kind::WindRising, at(date, 10, 10)),
            (Kind::TemperatureDrop, at(date, 11, 5))
        ]
    );
    assert_eq!(
        a[0].text,
        "le vent se lève : rafales à 75 km/h dans l'heure (avant : 20)"
    );
    assert_eq!(a[0].spoken, "Le vent se lève.");
    assert_eq!(
        a[1].text,
        "la température baisse : -12 °C, ressenti -24 °C d'ici 3 h"
    );
    // Les Fonts: 5 km/h more.
    let f = replay("vent-fort", "les-fonts");
    assert_eq!(
        f[0].text,
        "le vent se lève : rafales à 80 km/h dans l'heure (avant : 25)"
    );
    // Leaving at 12 h, in the wind already: the wind is not « rising ».
    let d = data("vent-fort");
    let p = point(&d, "le-laus");
    let profile = flat_profile(240);
    let mut m = alerts::Monitor::new(at(date, 12, 0), &profile, p, alerts::Params::default());
    let mut said = Vec::new();
    for k in 0..48 {
        let t = at(date, 12, 0) + Duration::minutes(5 * k);
        said.extend(m.update(t, 50.0 * (5 * k) as f64));
    }
    assert!(
        !said.iter().any(|x| x.kind == Kind::WindRising),
        "{said:#?}"
    );
    // The drop of 14 h is still ahead at 12 h 10: said once.
    assert_eq!(
        said.iter()
            .filter(|x| x.kind == Kind::TemperatureDrop)
            .count(),
        1
    );
}

/// A flat route of `minutes` model minutes, 50 m a minute.
fn flat_profile(minutes: u32) -> skimap_assist::suggest::Profile {
    skimap_assist::suggest::Profile {
        dist_m: (0..=minutes).map(|i| f64::from(i) * 50.0).collect(),
        out_s: (0..=minutes).map(|i| f64::from(i) * 60.0).collect(),
        back_s: (0..=minutes).map(|i| f64::from(i) * 60.0).collect(),
    }
}

#[test]
fn a8_fog_and_snow_ahead() {
    let date = set("brouillard").date();
    let a = replay("brouillard", "le-laus");
    assert_eq!(a.len(), 1, "{a:#?}");
    assert_eq!((a[0].kind, a[0].at), (Kind::Fog, at(date, 11, 5)));
    assert_eq!(a[0].text, "brouillard prévu dans l'heure");
    assert_eq!(a[0].spoken, "Le brouillard arrive.");
    let date = set("neige-fraiche").date();
    let a = replay("neige-fraiche", "le-laus");
    assert_eq!(a.len(), 1, "{a:#?}");
    assert_eq!((a[0].kind, a[0].at), (Kind::Snow, at(date, 12, 5)));
    assert_eq!(a[0].text, "chute de neige dans l'heure (0,8 cm/h)");
    // Calm days: nothing.
    for name in ["froid-sec", "regel", "redoux", "novembre"] {
        for p in set(name).points {
            assert!(replay(name, p.id).is_empty(), "{name} {}", p.id);
        }
    }
}

// ---------------------------------------------------------------------------------------
// A9: best time to leave.

use skimap_assist::best_time::{self, BestTime};

fn best(name: &str, id: &str, crowd: Option<CrowdLevel>) -> BestTime {
    let d = data(name);
    let date = set(name).date();
    let q = quality(&d, id, date);
    best_time::best_time(
        point(&d, id),
        Some(&q),
        crowd,
        &best_time::Request {
            date,
            duration_min: 120.0,
            earliest: NaiveTime::from_hms_opt(8, 0, 0).unwrap(),
            classes: vec![],
        },
    )
}

fn start_at(b: &BestTime, h: u32, m: u32) -> &best_time::Start {
    b.starts
        .iter()
        .find(|s| s.start == at(b.request.date, h, m))
        .unwrap()
}

#[test]
fn a9_leave_before_the_wind() {
    let b = best("vent-fort", "le-laus", None);
    let s = b.best.clone().unwrap();
    assert_eq!(s.start, at(b.request.date, 9, 0));
    assert_eq!(s.back, at(b.request.date, 11, 24));
    // From 11 h every hour of the outing has gusts of 75 km/h: −1.
    let late = start_at(&b, 11, 0);
    assert!(late.reasons.iter().any(|r| r.factor == Factor::Wind
        && r.points == -1.0
        && r.text == "rafales jusqu'à 75 km/h"));
    assert!(
        b.text
            .starts_with("Meilleur départ vers 9 h 00 (retour vers 11 h 24, sortie de 120 min)"),
        "{}",
        b.text
    );
    // Every start ends before the deadline, the last ones say they are close to it.
    let d = b.deadline.unwrap();
    assert!(b.starts.iter().all(|s| s.back <= d));
    let last = b.starts.last().unwrap();
    assert!(last.reasons.iter().any(|r| {
        r.points == -0.5
            && r.text
                .starts_with("retour à moins de 30 min de l'heure limite")
    }));
    assert!(matches!(
        b.voice(),
        Some(skimap_voice::Event::Briefing(
            skimap_voice::bank::Briefing::BestTime { .. }
        ))
    ));
}

#[test]
fn a9_dry_cold_leave_late_morning() {
    let b = best("froid-sec", "le-laus", None);
    let s = b.best.clone().unwrap();
    // Wind chill of −17 °C until 10 h: leaving at 11 h avoids it.
    assert_eq!(s.start, at(b.request.date, 11, 0), "{}", b.text);
    let early = start_at(&b, 9, 0);
    assert!(
        early
            .reasons
            .iter()
            .any(|r| r.factor == Factor::Cold && r.points < 0.0)
    );
    assert!(!s.reasons.iter().any(|r| r.factor == Factor::Cold));
    assert!(
        s.reasons
            .iter()
            .any(|r| r.factor == Factor::Sun && (r.points - 0.2).abs() < 1e-9)
    );
    assert!(
        b.text.contains("départ à 9 h 00 : ressenti jusqu'à -17 °C"),
        "{}",
        b.text
    );
}

#[test]
fn a9_fog_and_rain_in_the_afternoon() {
    let b = best("brouillard", "le-laus", None);
    assert_eq!(b.best.as_ref().unwrap().start, at(b.request.date, 9, 0));
    let foggy = start_at(&b, 13, 0);
    let f = foggy
        .reasons
        .iter()
        .find(|r| r.factor == Factor::Fog)
        .unwrap();
    assert_eq!(f.text, "brouillard 3 h");
    let b = best("redoux", "chapelle-des-bois", Some(CrowdLevel::VeryBusy));
    let s = b.best.clone().unwrap();
    assert_eq!(s.start, at(b.request.date, 8, 0), "{}", b.text);
    // A very busy day: the outing over the assumed peak (10 h 30 – 14 h) pays.
    let noon = start_at(&b, 11, 0);
    let c = noon
        .reasons
        .iter()
        .find(|r| r.factor == Factor::Crowd)
        .unwrap();
    assert!((c.points + 1.0).abs() < 1e-9, "{c:?}");
    assert!(
        noon.reasons
            .iter()
            .any(|r| r.factor == Factor::Rain && r.points == -1.0)
    );
    // Busy only: half the points.
    let b = best("redoux", "chapelle-des-bois", Some(CrowdLevel::Busy));
    let c = start_at(&b, 11, 0)
        .reasons
        .iter()
        .find(|r| r.factor == Factor::Crowd)
        .unwrap()
        .clone();
    assert!((c.points + 0.5).abs() < 1e-9, "{c:?}");
}

#[test]
fn a9_spring_snow_waits_for_the_softening() {
    let b = best("regel", "les-fonts", None);
    let s = b.best.clone().unwrap();
    // Hard at 8 h at Les Fonts, transformed from 10 h.
    assert_eq!(s.start, at(b.request.date, 10, 0), "{}", b.text);
    let snow = s
        .reasons
        .iter()
        .find(|r| r.factor == Factor::SnowSurface)
        .unwrap();
    assert_eq!(snow.points, 1.0, "transformed all along: 3 − 2");
    assert!(
        b.text
            .contains("départ à 8 h 00 : plat : neige dure, regelée puis"),
        "{}",
        b.text
    );
}

#[test]
fn a9_the_snow_of_the_hours_of_the_outing_only() {
    // 2 h 30 (3 h planned) from 9 h: back at 12 h; the flat at Le Laus is hard at 9 h,
    // transformed at 10 and 11 h, heavy from 12 h: the hour of the return is not counted.
    let d = data("regel");
    let date = set("regel").date();
    let q = quality(&d, "le-laus", date);
    assert_eq!(&letters(class(&q, "flat"))[3..7], "DTTH");
    let b = best_time::best_time(
        point(&d, "le-laus"),
        Some(&q),
        None,
        &best_time::Request {
            date,
            duration_min: 150.0,
            earliest: NaiveTime::from_hms_opt(8, 0, 0).unwrap(),
            classes: vec![],
        },
    );
    let s = start_at(&b, 9, 0);
    assert_eq!(s.back, at(date, 12, 0));
    let snow = s
        .reasons
        .iter()
        .find(|r| r.factor == Factor::SnowSurface)
        .unwrap();
    assert_eq!(
        snow.text,
        "plat : neige dure, regelée puis neige transformée qui ramollit (décaille)"
    );
    assert!((snow.points - ((1.0 + 3.0 + 3.0) / 3.0 - 2.0)).abs() < 1e-12);
    // The slopes asked instead of the flat: their own hours.
    let b = best_time::best_time(
        point(&d, "le-laus"),
        Some(&q),
        None,
        &best_time::Request {
            date,
            duration_min: 150.0,
            earliest: NaiveTime::from_hms_opt(8, 0, 0).unwrap(),
            classes: vec!["N40".into(), "S20".into()],
        },
    );
    let snow = start_at(&b, 9, 0)
        .reasons
        .iter()
        .find(|r| r.factor == Factor::SnowSurface)
        .unwrap()
        .text
        .clone();
    assert!(
        snow.starts_with("sud, pente de 20° : ") && snow.contains(" ; nord, pente de 40° : "),
        "{snow}"
    );
}

// ---------------------------------------------------------------------------------------
// The `assist` layer on a windy day.

#[test]
fn layer_build_writes_every_output_of_a_windy_day() {
    let tmp = tempfile::tempdir().unwrap();
    fixtures::copy_tree(
        &fixtures::data().join("out/cervieres"),
        &tmp.path().join("out/cervieres"),
    );
    let layout = DataLayout::new(tmp.path());
    let date = set("vent-fort").date();
    let opts = skimap_assist::Options {
        date: Some(date),
        extra_days: 1,
        home: Some(home("vent-fort")),
        paths: Paths {
            meteo: vec![weather_sets::path("vent-fort")],
            ..Paths::default()
        },
        requests: skimap_assist::presets::suggestion_requests(&CERVIERES, date),
        outings: vec![],
    };
    let out = skimap_assist::build(&CERVIERES, &layout, &opts).unwrap();
    let dir = layout.layer_dir(&CERVIERES, "assist").unwrap();
    let names: Vec<String> = out
        .files
        .iter()
        .map(|f| f.file_name().unwrap().to_string_lossy().into_owned())
        .collect();
    for kind in [
        "briefing",
        "snow_quality",
        "wax",
        "synthesis",
        "best_time",
        "equipment",
        "alerts",
        "suggestions",
    ] {
        for ext in ["json", "txt"] {
            let n = format!("{kind}_{date}.{ext}");
            assert!(names.contains(&n), "{n} in {names:?}");
            assert!(dir.join(&n).is_file());
        }
    }
    assert!(names.contains(&format!("suggestions_{date}.geojson")));
    // Two days, two points: wax and best times for each.
    assert_eq!(out.briefing.days.len(), 2);
    assert_eq!(out.wax.len(), 4);
    assert_eq!(out.best_times.len(), 4);
    assert_eq!(out.synthesis.len(), 2);
    // The best time of the first point is in the briefing, said before the sunset.
    let day = &out.briefing.days[0];
    assert!(
        day.text.iter().any(|t| t
            .starts_with("Meilleur moment pour partir (le-laus) : Meilleur départ vers 9 h 00")),
        "{:?}",
        day.text
    );
    let at_best = day
        .voice
        .iter()
        .position(|e| {
            matches!(
                e,
                skimap_voice::Event::Briefing(skimap_voice::bank::Briefing::BestTime { .. })
            )
        })
        .unwrap();
    assert!(matches!(
        day.voice[at_best + 1],
        skimap_voice::Event::Briefing(skimap_voice::bank::Briefing::Sunset { .. })
    ));
    // Every JSON starts with the inputs and says it is never « safe ».
    let j: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(dir.join(format!("briefing_{date}.json"))).unwrap(),
    )
    .unwrap();
    assert_eq!(j["site"], "cervieres");
    assert_eq!(j["date"], date.to_string());
    assert!(j["never_safe"].as_str().unwrap().contains("sans danger"));
    assert!(
        j["inputs"]
            .as_array()
            .unwrap()
            .iter()
            .any(|i| i.as_str().unwrap().ends_with("vent-fort.json"))
    );
    assert_eq!(j["briefing"]["days"].as_array().unwrap().len(), 2);
    // The alerts: the weather replay of each day and point, the loops at four paces.
    let alerts = std::fs::read_to_string(dir.join(format!("alerts_{date}.txt"))).unwrap();
    assert!(
        alerts.contains(&format!(
            "Rejeu météo {date} le-laus (sortie de 10 h, retour prévu à 16 h) : 10:10 le vent se lève"
        )),
        "{alerts}"
    );
    let loops = out
        .suggestions
        .iter()
        .filter(|s| !s.suggestions.is_empty())
        .count();
    assert!(
        loops >= 2,
        "{:?}",
        out.suggestions.iter().map(|s| &s.text).collect::<Vec<_>>()
    );
    assert!(out.replays.len() >= 3 * loops, "{}", out.replays.len());
    for r in &out.replays {
        let d = r.deadline.unwrap();
        assert!(r.fixes > 0);
        // Every alert of a replay between its start and the end of its trace.
        for a in &r.alerts {
            assert!(a.at >= r.start, "{} before {}", a.at, r.start);
        }
        // The C7 deadline of the day at the forecast point nearest to the start.
        let s = out
            .suggestions
            .iter()
            .find(|s| s.request.label == r.request)
            .unwrap();
        let p = point(
            &data_with("vent-fort", None, None),
            if s.request.start.lon > 6.78 {
                "les-fonts"
            } else {
                "le-laus"
            },
        )
        .position;
        let tp = skimap_core::turnaround::TurnaroundParams::default();
        let dl = skimap_core::ephemeris::local_time(
            skimap_core::turnaround::deadline(date, p, &tp)
                .unwrap()
                .deadline,
        );
        assert_eq!(d, dl);
    }
    // At the request's start at three paces, then a late start: its planned return (model
    // time × 1.2) 30 minutes before the deadline, at 1.5 times the model time.
    for s in out.suggestions.iter().filter(|s| !s.suggestions.is_empty()) {
        let best = &s.suggestions[0];
        let rs: Vec<&skimap_assist::layer::Replay> = out
            .replays
            .iter()
            .filter(|r| r.request == s.request.label)
            .collect();
        assert_eq!(
            rs.iter().map(|r| r.slowdown).collect::<Vec<_>>()[..3],
            [1.0, 1.5, 2.0]
        );
        assert!(rs[..3].iter().all(|r| r.start == s.request.start_time));
        assert_eq!(rs[0].route, best.label);
        let d = rs[0].deadline.unwrap();
        let late =
            d - Duration::seconds((best.profile.total_s() * 1.2) as i64) - Duration::minutes(30);
        if late > s.request.start_time {
            assert_eq!(rs.len(), 4);
            assert_eq!((rs[3].start, rs[3].slowdown), (late, 1.5));
        } else {
            assert_eq!(rs.len(), 3);
        }
    }
    // A6 on the best loop of each request, with the weather of the day.
    assert_eq!(out.equipment.len(), loops);
    // The snow quality table and the wax text are written.
    let sq = std::fs::read_to_string(dir.join(format!("snow_quality_{date}.txt"))).unwrap();
    assert!(
        sq.contains("neige fraîche soufflée par le vent entre 10 h et 15 h"),
        "{sq}"
    );
    let wx = std::fs::read_to_string(dir.join(format!("wax_{date}.txt"))).unwrap();
    assert!(wx.contains("  Voix : Fartage."), "{wx}");
    // The meta file keeps one run per first day.
    let opts2 = skimap_assist::Options {
        date: Some(date + Duration::days(1)),
        extra_days: 0,
        requests: vec![],
        ..opts
    };
    skimap_assist::build(&CERVIERES, &layout, &opts2).unwrap();
    let meta = skimap_common::meta::LayerMeta::read(&dir.join("assist.meta.json")).unwrap();
    let runs = meta.parameters["runs"].as_object().unwrap();
    assert!(
        runs.contains_key(&date.to_string())
            && runs.contains_key(&(date + Duration::days(1)).to_string())
    );
    assert!(meta.sources.iter().any(|s| s.name.starts_with("Swix")));
    assert!(
        meta.files
            .iter()
            .any(|f| f.name == format!("wax_{date}.json"))
    );
    // Sources once each, after two runs; the data files only, not the meta file itself.
    for (i, s) in meta.sources.iter().enumerate() {
        assert!(
            !meta.sources[..i]
                .iter()
                .any(|x| x.url == s.url && x.name == s.name),
            "{s:?} twice"
        );
    }
    assert!(
        meta.sources.len() > 2,
        "the layers' sources and the wax tables"
    );
    assert!(meta.files.iter().all(|f| f.name != "assist.meta.json"));
    assert!(
        meta.files
            .iter()
            .any(|f| f.name == format!("suggestions_{date}.geojson"))
    );
    assert!(meta.tickets.iter().any(|t| t == "A1 (#88)"));
    assert_eq!(
        meta.valid_from.as_deref(),
        Some((date + Duration::days(1)).to_string().as_str())
    );
    assert!(meta.validity.is_some() && !meta.notes.is_empty());
}

#[test]
fn layer_build_without_snow_or_weather() {
    let tmp = tempfile::tempdir().unwrap();
    fixtures::copy_tree(
        &fixtures::data().join("out/cervieres"),
        &tmp.path().join("out/cervieres"),
    );
    let layout = DataLayout::new(tmp.path());
    let dir = layout.layer_dir(&CERVIERES, "assist").unwrap();
    // November: the snow map of the fixtures has no snow; an outing given by the caller,
    // the morning's grip unreliable that day? No wax at all without snow.
    let date = set("novembre").date();
    let route = data_with("novembre", None, None).pistes[0].line.clone();
    let opts = skimap_assist::Options {
        date: Some(date),
        extra_days: 0,
        home: None,
        paths: Paths {
            meteo: vec![weather_sets::path("novembre")],
            ..Paths::default()
        },
        requests: vec![],
        outings: vec![skimap_assist::equipment::Outing {
            label: "test".into(),
            start: at(date, 10, 0),
            duration_min: 60.0,
            route: route.clone(),
            offpiste: false,
            kick_unreliable: false,
        }],
    };
    let out = skimap_assist::build(&CERVIERES, &layout, &opts).unwrap();
    assert!(out.wax.is_empty() && out.best_times.is_empty());
    let wx = std::fs::read_to_string(dir.join(format!("wax_{date}.txt"))).unwrap();
    assert!(
        wx.contains(&format!(
            "{} — le-laus : pas ou trop peu de neige estimée (0 cm), pas de fartage.",
            briefing::date_fr(date)
        )),
        "{wx}"
    );
    let bt = std::fs::read_to_string(dir.join(format!("best_time_{date}.txt"))).unwrap();
    assert_eq!(
        bt,
        "Pas de créneau : aucun jour avec de la neige et la météo de la journée.\n"
    );
    assert_eq!(out.equipment.len(), 1);
    assert!(
        !out.equipment[0]
            .items
            .iter()
            .any(|i| i.kind == skimap_assist::equipment::Kind::WaxlessSkis)
    );
    // The alerts file: the weather replay of the day, no loop.
    assert!(dir.join(format!("alerts_{date}.txt")).is_file());
    // The spring day of the refreeze: grip unreliable, the outing given gets waxless skis.
    let date = set("regel").date();
    let opts = skimap_assist::Options {
        date: Some(date),
        paths: Paths {
            meteo: vec![weather_sets::path("regel")],
            ..Paths::default()
        },
        outings: vec![skimap_assist::equipment::Outing {
            start: at(date, 10, 0),
            ..opts.outings[0].clone()
        }],
        ..opts
    };
    let out = skimap_assist::build(&CERVIERES, &layout, &opts).unwrap();
    assert!(out.wax.iter().any(|w| !w.morning.kick_reliable));
    assert!(
        out.equipment[0]
            .items
            .iter()
            .any(|i| i.kind == skimap_assist::equipment::Kind::WaxlessSkis)
    );
    // No weather at all (the file of another site's day is not there): no alerts file.
    let empty = tempfile::tempdir().unwrap();
    fixtures::copy_tree(
        &fixtures::data().join("out/cervieres"),
        &empty.path().join("out/cervieres"),
    );
    let layout = DataLayout::new(empty.path());
    let opts = skimap_assist::Options {
        date: Some(date),
        ..skimap_assist::Options::default()
    };
    let out = skimap_assist::build(&CERVIERES, &layout, &opts).unwrap();
    let dir = layout.layer_dir(&CERVIERES, "assist").unwrap();
    assert!(out.replays.is_empty());
    assert!(!dir.join(format!("alerts_{date}.txt")).exists());
    assert!(!dir.join(format!("equipment_{date}.txt")).exists());
    assert!(!dir.join(format!("suggestions_{date}.txt")).exists());
    let sq = std::fs::read_to_string(dir.join(format!("snow_quality_{date}.txt"))).unwrap();
    assert!(sq.starts_with("Pas de qualité de neige"), "{sq}");
    let wx = std::fs::read_to_string(dir.join(format!("wax_{date}.txt"))).unwrap();
    assert!(
        wx.contains("pas de fartage, la météo ne couvre pas la journée"),
        "{wx}"
    );
}

// ---------------------------------------------------------------------------------------
// A6 with the weather, and the layers of a site.

#[test]
fn a6_equipment_with_the_weather() {
    use skimap_assist::equipment::{self, Kind as Item, Outing};
    let mut d = data("froid-sec");
    let date = set("froid-sec").date();
    // A route on a piste of Les Fonts, where the wind chill is −21 °C at 9 h.
    let route = d
        .pistes
        .iter()
        .find(|p| d.domains[1].ways.contains(&p.way_id))
        .unwrap()
        .line
        .clone();
    let o = |h: u32, m: u32, minutes: f64| Outing {
        label: "test".into(),
        start: at(date, h, m),
        duration_min: minutes,
        route: route.clone(),
        offpiste: false,
        kick_unreliable: false,
    };
    let kinds = |e: &equipment::Equipment| e.items.iter().map(|i| i.kind).collect::<Vec<_>>();
    // From 9 h 30 for 20 minutes: the hour of 9 h counts (−21 °C at Les Fonts).
    let e = equipment::equipment(&d, &o(9, 30, 20.0));
    assert!(kinds(&e).contains(&Item::FaceProtection), "{}", e.text);
    assert!(kinds(&e).contains(&Item::WarmLayer));
    assert!(kinds(&e).contains(&Item::SunProtection), "{}", e.text);
    assert_eq!(e.facts.wind_chill_min_c, Some(-21.0));
    let face = e
        .items
        .iter()
        .find(|i| i.kind == Item::FaceProtection)
        .unwrap();
    assert_eq!(face.reason, "ressenti jusqu'à -21 °C");
    // In the afternoon: still cold, not −20.
    let e = equipment::equipment(&d, &o(13, 0, 60.0));
    assert!(!kinds(&e).contains(&Item::FaceProtection), "{}", e.text);
    assert!(kinds(&e).contains(&Item::WarmLayer));
    // A clouded day (fog set): no sun cream, but the compass in the fog.
    let f = data("brouillard");
    let fdate = set("brouillard").date();
    let e = equipment::equipment(
        &f,
        &Outing {
            start: at(fdate, 13, 0),
            ..o(13, 0, 60.0)
        },
    );
    assert!(!kinds(&e).contains(&Item::SunProtection));
    assert!(!kinds(&e).contains(&Item::WarmLayer), "chill around 0 °C");
    let c = e.items.iter().find(|i| i.kind == Item::Compass).unwrap();
    assert_eq!(c.reason, "brouillard prévu (2 h pendant la sortie)");
    // Leaving late: back after the sunset minus an hour, the headlamp, said with the times.
    let e = equipment::equipment(&d, &o(15, 30, 60.0));
    let h = e.items.iter().find(|i| i.kind == Item::Headlamp).unwrap();
    let (_, set_t) = skimap_core::ephemeris::sun_times(date, route[0]).unwrap();
    let set_t = skimap_core::ephemeris::local_time(set_t);
    let back = at(date, 15, 30) + Duration::seconds((60.0 * 60.0 * 1.2) as i64);
    assert_eq!(
        h.reason,
        format!(
            "retour prévu vers {} h {:02}, coucher du soleil à {} h {:02}",
            chrono::Timelike::hour(&back),
            chrono::Timelike::minute(&back),
            chrono::Timelike::hour(&set_t),
            chrono::Timelike::minute(&set_t)
        )
    );
    // Off the pistes with a BERA of 3: the avalanche kit, said; with 2: not for that.
    d.beras = vec![bera(date, 3, true)];
    let off = Outing {
        offpiste: true,
        ..o(10, 0, 60.0)
    };
    let e = equipment::equipment(&d, &off);
    let kit = e
        .items
        .iter()
        .find(|i| i.kind == Item::AvalancheKit)
        .unwrap();
    assert!(
        kit.reason.contains("hors piste avec un BERA à 3 sur 5"),
        "{}",
        kit.reason
    );
    d.beras = vec![bera(date, 2, false)];
    let e = equipment::equipment(&d, &off);
    assert!(
        e.items
            .iter()
            .filter(|i| i.kind == Item::AvalancheKit)
            .all(|i| !i.reason.contains("hors piste avec un BERA"))
    );
    // Historic avalanche paths over the route: counted along it, said.
    let n = route.len();
    let (lo, hi) = (route[0], route[n - 1]);
    let r = rect(
        lo.lon.min(hi.lon) - 1.0,
        lo.lat.min(hi.lat) - 1.0,
        lo.lon.max(hi.lon) + 1.0,
        lo.lat.max(hi.lat) + 1.0,
    );
    d.clpa = Some(skimap_assist::layers::Clpa {
        covered: true,
        areas: skimap_assist::layers::Areas { areas: vec![r] },
        piste_km: Default::default(),
    });
    let e = equipment::equipment(&d, &o(10, 0, 60.0));
    let len = skimap_assist::layers::line_length_m(&route) / 1000.0;
    assert!(
        (e.facts.clpa_km - len).abs() < 0.006,
        "{} {len}",
        e.facts.clpa_km
    );
    let kit = e
        .items
        .iter()
        .find(|i| i.kind == Item::AvalancheKit)
        .unwrap();
    assert!(
        kit.reason
            .contains("km dans des zones d'avalanche connues (CLPA)"),
        "{}",
        kit.reason
    );
}

#[test]
fn inputs_bera_of_the_massif_and_sources() {
    let date = set("froid-sec").date();
    let mut d = data("froid-sec");
    assert!(!d.no_bera(), "a bulletin of the Queyras");
    d.beras = vec![bera_status(date, BeraStatus::NoBera)];
    assert!(d.no_bera());
    d.beras = vec![bera_status(date, BeraStatus::Unavailable)];
    assert!(!d.no_bera());
    assert!(data("redoux").no_bera(), "the Jura");
    // The sources of the layers read, from their meta files.
    assert!(
        d.sources
            .iter()
            .any(|s| s.url.starts_with("https://data.arcep.fr/")),
        "{:?}",
        d.sources
    );
    assert!(d.files.iter().any(|f| f.ends_with("froid-sec.json")));
}
