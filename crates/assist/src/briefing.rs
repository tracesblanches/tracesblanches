//! Daily briefing, where to ski today (A1, #88): rank areas and routes by open trails,
//! estimated snow, weather, expected crowd and distance, each reason explained.
//!
//! # Candidates
//!
//! - the nordic **domains** touching the site (layer `domaines`, P3);
//! - the **itineraries** of the status sources of the site (ENJ, Nordic France, town hall
//!   page; layer `status`, P13), each assigned to the domain holding most of its
//!   points (Nordic France pistes take the geometry of the OSM ways of the same name).
//!
//! # Score
//!
//! A sum of points, each one a [`Reason`] with its factor, its points and a French
//! sentence; the ranking is the order of the scores. Weights are judgement, documented
//! here and in the README, to be tuned with the field tests (T5) and the users:
//!
//! | Factor | Points |
//! |---|---|
//! | status of the day | all itineraries closed −10; otherwise +3 × share of open nordic km; unknown 0 |
//! | estimated snow (median along the pistes, snow map plus snowfall since its date) | < 20 cm −3; 20 to 40 cm +0.5; ≥ 40 cm +2 (P6 thresholds for groomed pistes) |
//! | fresh snow (24 h before 09 h) | ≥ 5 cm +0.5 |
//! | snow surface (A3, flat, 10–15 h) | fresh or transformed +0.5; wind-blown, hard or wet −0.5; icy or slush −1 |
//! | gusts 09–16 h | ≥ 60 km/h −2; ≥ 40 km/h −1 |
//! | wind chill 09–16 h | ≤ −20 °C −1.5; ≤ −12 °C −0.5 |
//! | fog 09–16 h | ≥ 4 h −1; ≥ 2 h −0.5 |
//! | snowfall 09–16 h | ≥ 5 cm −0.5 |
//! | rain 09–16 h | ≥ 1 mm −1.5 |
//! | sun (cloud cover 09–16 h < 30 %) | +0.5 |
//! | crowd (A4, whole site) | calm 0, moderate −0.25, busy −0.5, very busy −1 |
//! | distance from home (straight line) | −0.02 per km |
//! | historic avalanche paths (CLPA) crossed | BERA ≥ 3 −2; BERA not available −0.5; BERA 1–2: noted, 0 |
//! | size of the domain | + min(km / 50, 1) |
//!
//! The weather of a candidate is the one of the nearest forecast point (P19), with its
//! distance in the reason when it is more than 5 km away.
//!
//! Status is « of the day » when polled on that local date; a later day uses the last
//! known state (said so); an earlier day has no status.

use chrono::{Datelike, Duration, NaiveDate, NaiveDateTime, Timelike};
use serde::Serialize;
use skimap_common::geom::LonLat;
use skimap_voice::Event;
use skimap_voice::bank::{Briefing as Line, Crowd as VoiceCrowd, Name};
use skimap_voice::names::PisteName;
use skimap_voice::quantity::Clock;

use crate::crowd::{self, CrowdLevel};
use crate::inputs::SiteData;
use crate::layers::{BeraStatus, DayWeather, Itinerary, OpenState, Practice, WeatherPoint};
use crate::snow_quality::{self, DayQuality, Quality};

/// Factor of a reason.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Factor {
    /// Open or closed.
    Status,
    /// Estimated snow depth.
    Snow,
    /// Fresh snow.
    FreshSnow,
    /// Snow surface (A3).
    SnowSurface,
    /// Wind and gusts.
    Wind,
    /// Wind chill.
    Cold,
    /// Fog.
    Fog,
    /// Snowfall during the outing.
    Snowfall,
    /// Rain.
    Rain,
    /// Sun.
    Sun,
    /// Crowd.
    Crowd,
    /// Distance from home.
    Distance,
    /// Historic avalanche paths.
    Avalanche,
    /// Size of the domain.
    Size,
    /// Missing or approximate data.
    Data,
}

/// One reason of a score.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Reason {
    /// Factor.
    pub factor: Factor,
    /// Points (positive helps).
    pub points: f64,
    /// French sentence.
    pub text: String,
}

/// Kind of candidate.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    /// A nordic domain.
    Domain,
    /// An itinerary of the status sources.
    Itinerary,
}

/// Estimated snow along a candidate.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct SnowStats {
    /// Points sampled.
    pub samples: usize,
    /// Median depth of the snow map, cm.
    pub median_cm: f64,
    /// 10th percentile, cm.
    pub p10_cm: f64,
    /// Share of points with at least 20 cm.
    pub share_20cm: f64,
    /// Snowfall added since the date of the map, cm.
    pub added_cm: f64,
    /// Date of the snow map.
    pub map_date: NaiveDate,
}

impl SnowStats {
    /// Median with the snowfall since the map.
    pub fn depth_cm(&self) -> f64 {
        self.median_cm + self.added_cm
    }
}

/// A ranked candidate.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Candidate {
    /// Kind.
    pub kind: Kind,
    /// Identifier.
    pub id: String,
    /// Name.
    pub name: String,
    /// Domain (itineraries).
    pub domain: Option<String>,
    /// Practice (itineraries).
    pub practice: Option<Practice>,
    /// State (itineraries) or aggregated state (domains: open if any nordic itinerary is).
    pub state: OpenState,
    /// Open and total nordic km, when known.
    pub open_km: Option<(f64, f64)>,
    /// Snow.
    pub snow: Option<SnowStats>,
    /// Forecast point used.
    pub weather_point: Option<String>,
    /// Weather of the skiing window.
    pub weather: Option<DayWeather>,
    /// Straight-line distance from home, km.
    pub distance_km: Option<f64>,
    /// Kilometres inside historic avalanche paths.
    pub clpa_km: f64,
    /// Score.
    pub score: f64,
    /// Reasons.
    pub reasons: Vec<Reason>,
}

/// Options.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Options {
    /// Home point, for the distance.
    pub home: Option<LonLat>,
}

/// Crowd forecast of the day (A4, `crowd::forecast_day`).
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct CrowdInfo {
    /// Score.
    pub score: f64,
    /// Level, French.
    pub level_fr: &'static str,
    /// Reasons, French.
    pub reasons: Vec<String>,
    /// Domain open according to its published dates.
    pub open: Option<bool>,
    /// Level.
    #[serde(skip)]
    pub level: CrowdLevel,
}

impl From<crowd::DayForecast> for CrowdInfo {
    fn from(f: crowd::DayForecast) -> Self {
        Self {
            score: f.score,
            level_fr: f.level.label(),
            reasons: f.reasons,
            open: f.open,
            level: f.level,
        }
    }
}

/// Briefing of one day.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct DayBriefing {
    /// Date.
    pub date: NaiveDate,
    /// Whether the weather covers the skiing window (09–16 h).
    pub weather_available: bool,
    /// Status of the pistes for this day: `day`, `last_known`, `none`.
    pub status: &'static str,
    /// Crowd (A4).
    pub crowd: Option<CrowdInfo>,
    /// BERA level, when a bulletin of that date was read.
    pub bera_level: Option<u8>,
    /// Domains, best first.
    pub domains: Vec<Candidate>,
    /// Nordic itineraries, best first.
    pub itineraries: Vec<Candidate>,
    /// Snow quality per forecast point (A3).
    pub snow_quality: Vec<DayQuality>,
    /// Sunset (local).
    pub sunset: Option<NaiveDateTime>,
    /// Notes on the data.
    pub notes: Vec<String>,
    /// Text, French.
    pub text: Vec<String>,
    /// Voice lines.
    pub voice: Vec<Event>,
}

/// Briefing of a site from `date` over `days` days.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SiteBriefing {
    /// Site.
    pub site: String,
    /// Days.
    pub days: Vec<DayBriefing>,
}

const WEEKDAYS: [&str; 7] = [
    "lundi", "mardi", "mercredi", "jeudi", "vendredi", "samedi", "dimanche",
];
const MONTHS: [&str; 12] = [
    "janvier",
    "février",
    "mars",
    "avril",
    "mai",
    "juin",
    "juillet",
    "août",
    "septembre",
    "octobre",
    "novembre",
    "décembre",
];

/// « mardi 5 janvier 2026 ».
pub fn date_fr(d: NaiveDate) -> String {
    format!(
        "{} {} {} {}",
        WEEKDAYS[d.weekday().num_days_from_monday() as usize],
        d.day(),
        MONTHS[d.month0() as usize],
        d.year()
    )
}

/// A temperature in whole degrees: « 0 » and not « -0 » between −0.5 and 0.
pub(crate) fn degrees(x: f64) -> String {
    let s = format!("{x:.0}");
    if s == "-0" { "0".to_owned() } else { s }
}

pub(crate) fn fmt_num(x: f64, decimals: usize) -> String {
    format!("{x:.decimals$}").replace('.', ",")
}

pub(crate) fn reason(out: &mut Vec<Reason>, factor: Factor, points: f64, text: String) {
    out.push(Reason {
        factor,
        points,
        text,
    });
}

fn quantile(v: &mut [f64], q: f64) -> f64 {
    v.sort_by(f64::total_cmp);
    let i = ((v.len() - 1) as f64 * q).round() as usize;
    v[i]
}

/// Snow along points (vertices), with the snowfall since the map at `added_cm`.
pub fn snow_stats(
    map: &crate::layers::SnowMap,
    pts: &[LonLat],
    added_cm: f64,
) -> Option<SnowStats> {
    let mut v: Vec<f64> = pts
        .iter()
        .filter_map(|p| map.sample(*p))
        .map(|s| s.depth_cm)
        .collect();
    if v.is_empty() {
        return None;
    }
    let share = v.iter().filter(|d| **d + added_cm >= 20.0).count() as f64 / v.len() as f64;
    Some(SnowStats {
        samples: v.len(),
        p10_cm: quantile(&mut v, 0.1),
        median_cm: quantile(&mut v, 0.5),
        share_20cm: share,
        added_cm,
        map_date: map.date(),
    })
}

/// Domain of an itinerary: the one whose outline holds most of its points.
fn domain_of<'a>(data: &'a SiteData, it: &Itinerary) -> Option<&'a str> {
    data.domains
        .iter()
        .map(|d| (d, it.line.iter().filter(|p| d.area.contains(**p)).count()))
        .filter(|(_, n)| *n > 0)
        .max_by_key(|(_, n)| *n)
        .map(|(d, _)| d.id.as_str())
}

pub(crate) fn crowd_points(level: CrowdLevel) -> f64 {
    match level {
        CrowdLevel::Calm => 0.0,
        CrowdLevel::Moderate => -0.25,
        CrowdLevel::Busy => -0.5,
        CrowdLevel::VeryBusy => -1.0,
    }
}

/// Weather reasons of a candidate.
pub(crate) fn weather_reasons(out: &mut Vec<Reason>, w: &DayWeather) {
    if let Some(g) = w.gust_max_kmh {
        if g >= 60.0 {
            reason(
                out,
                Factor::Wind,
                -2.0,
                format!("rafales jusqu'à {g:.0} km/h"),
            );
        } else if g >= 40.0 {
            reason(
                out,
                Factor::Wind,
                -1.0,
                format!("rafales jusqu'à {g:.0} km/h"),
            );
        } else {
            let w = if g < 25.0 {
                "vent faible"
            } else {
                "vent modéré"
            };
            reason(out, Factor::Wind, 0.0, format!("{w} (rafales {g:.0} km/h)"));
        }
    }
    if let Some(c) = w.wind_chill_min_c {
        if c <= -20.0 {
            reason(
                out,
                Factor::Cold,
                -1.5,
                format!("très froid, ressenti {} °C", degrees(c)),
            );
        } else if c <= -12.0 {
            reason(
                out,
                Factor::Cold,
                -0.5,
                format!("froid, ressenti {} °C", degrees(c)),
            );
        } else if let Some(t) = w.temperature_mean_c {
            reason(
                out,
                Factor::Cold,
                0.0,
                format!("{} °C en moyenne, ressenti {} °C", degrees(t), degrees(c)),
            );
        }
    }
    if w.fog_hours >= 4 {
        reason(
            out,
            Factor::Fog,
            -1.0,
            format!("brouillard {} h sur 7", w.fog_hours),
        );
    } else if w.fog_hours >= 2 {
        reason(
            out,
            Factor::Fog,
            -0.5,
            format!("brouillard {} h sur 7", w.fog_hours),
        );
    }
    if w.snowfall_cm >= 5.0 {
        reason(
            out,
            Factor::Snowfall,
            -0.5,
            format!(
                "{} cm de neige pendant la journée",
                fmt_num(w.snowfall_cm, 0)
            ),
        );
    } else if w.snowfall_cm >= 1.0 {
        reason(
            out,
            Factor::Snowfall,
            0.0,
            format!(
                "{} cm de neige pendant la journée",
                fmt_num(w.snowfall_cm, 0)
            ),
        );
    }
    if w.rain_mm >= 1.0 {
        reason(
            out,
            Factor::Rain,
            -1.5,
            format!("pluie ({} mm)", fmt_num(w.rain_mm, 1)),
        );
    }
    if w.cloud_mean_pct.is_some_and(|c| c < 30.0) {
        reason(out, Factor::Sun, 0.5, "ensoleillé".into());
    }
}

pub(crate) fn snow_reasons(out: &mut Vec<Reason>, s: &Option<SnowStats>, fresh24: Option<f64>) {
    match s {
        Some(s) => {
            let d = s.depth_cm();
            let added = if s.added_cm >= 0.5 {
                format!(" dont {} cm tombés depuis", fmt_num(s.added_cm, 0))
            } else {
                String::new()
            };
            let base = format!(
                "{} cm de neige estimée (médiane, carte du {}{added}, {} % des points ≥ 20 cm)",
                fmt_num(d, 0),
                s.map_date.format("%d/%m"),
                (s.share_20cm * 100.0).round()
            );
            if d < 20.0 {
                reason(
                    out,
                    Factor::Snow,
                    -3.0,
                    format!("{base} : insuffisante pour damer"),
                );
            } else if d < 40.0 {
                reason(out, Factor::Snow, 0.5, format!("{base} : limite"));
            } else {
                reason(out, Factor::Snow, 2.0, format!("{base} : suffisante"));
            }
        }
        None => reason(
            out,
            Factor::Snow,
            -1.5,
            "neige estimée inconnue ici (hors de la carte du site ou sans tracé) : à vérifier"
                .into(),
        ),
    }
    if let Some(f) = fresh24
        && f >= 5.0
    {
        reason(
            out,
            Factor::FreshSnow,
            0.5,
            format!("{} cm de neige fraîche en 24 h", fmt_num(f, 0)),
        );
    }
}

pub(crate) fn surface_reason(out: &mut Vec<Reason>, q: Option<Quality>) {
    let Some(q) = q else { return };
    let p = match q {
        Quality::Fresh | Quality::Transformed => 0.5,
        Quality::OldDry => 0.0,
        Quality::Windblown | Quality::Hard | Quality::Wet => -0.5,
        Quality::Icy | Quality::Slush => -1.0,
    };
    reason(
        out,
        Factor::SnowSurface,
        p,
        format!("surface : {}", q.label_fr()),
    );
}

struct DayContext<'a> {
    date: NaiveDate,
    crowd: Option<CrowdInfo>,
    bera_level: Option<u8>,
    bera_status: Option<BeraStatus>,
    quality: Vec<DayQuality>,
    opts: &'a Options,
}

/// Reasons shared by domains and itineraries, and what they were computed from.
struct Common {
    reasons: Vec<Reason>,
    snow: Option<SnowStats>,
    weather_point: Option<String>,
    weather: Option<DayWeather>,
    distance: Option<f64>,
}

fn common_reasons(
    data: &SiteData,
    ctx: &DayContext,
    at: LonLat,
    pts: &[LonLat],
    clpa_km: f64,
) -> Common {
    let mut out = Vec::new();
    let nine = ctx.date.and_hms_opt(9, 0, 0).expect("09 h");
    let wp: Option<(&WeatherPoint, f64)> = data.weather.as_ref().and_then(|w| w.nearest(at));
    // Snow: map plus snowfall since the map (06 h UTC = 07 h local) until 09 h.
    let map = data.snow_for(ctx.date);
    let added = match (map, wp) {
        (Some(m), Some((p, _))) => {
            let from = m.valid_at.naive_utc() + Duration::hours(1);
            p.snowfall_between(from, nine)
        }
        _ => 0.0,
    };
    let snow = map.and_then(|m| snow_stats(m, pts, added));
    if map.is_none() && !data.snows.is_empty() {
        reason(
            &mut out,
            Factor::Data,
            0.0,
            "aucune carte d'enneigement de ce jour ou des 4 jours avant".into(),
        );
    }
    let fresh24 = wp.map(|(p, _)| p.snowfall_between(nine - Duration::hours(24), nine));
    snow_reasons(&mut out, &snow, fresh24);
    let mut weather = None;
    let mut wp_id = None;
    if let Some((p, dist)) = wp {
        wp_id = Some(p.id.clone());
        if p.covers(ctx.date, 9, 16) {
            let w = p.summary(ctx.date, 9, 16);
            weather_reasons(&mut out, &w);
            if dist > 5_000.0 {
                reason(
                    &mut out,
                    Factor::Data,
                    0.0,
                    format!(
                        "météo du point {} à {} km",
                        p.name,
                        fmt_num(dist / 1000.0, 0)
                    ),
                );
            }
            weather = Some(w);
            let q = ctx
                .quality
                .iter()
                .find(|q| q.point == p.id && q.enough_snow)
                .map(|q| q.flat);
            surface_reason(&mut out, q);
        } else {
            reason(
                &mut out,
                Factor::Data,
                0.0,
                "pas de prévision météo pour ce jour".into(),
            );
        }
    }
    if let Some(c) = &ctx.crowd {
        let why = if c.reasons.is_empty() {
            String::new()
        } else {
            format!(" ({})", c.reasons.join(", "))
        };
        reason(
            &mut out,
            Factor::Crowd,
            crowd_points(c.level),
            format!("niveau d'affluence {}{why}", c.level.label()),
        );
    }
    let distance = ctx.opts.home.map(|h| h.haversine_m(at) / 1000.0);
    if let Some(d) = distance {
        reason(
            &mut out,
            Factor::Distance,
            -0.02 * d,
            format!("à {} km à vol d'oiseau", fmt_num(d, 0)),
        );
    }
    if clpa_km >= 0.05 {
        let km = fmt_num(clpa_km, 1);
        match (ctx.bera_level, ctx.bera_status) {
            (Some(l), _) if l >= 3 => reason(
                &mut out,
                Factor::Avalanche,
                -2.0,
                format!("{km} km dans des zones d'avalanche connues, BERA {l} sur 5"),
            ),
            (Some(l), _) => reason(
                &mut out,
                Factor::Avalanche,
                0.0,
                format!("{km} km dans des zones d'avalanche connues (BERA {l} sur 5)"),
            ),
            _ => reason(
                &mut out,
                Factor::Avalanche,
                -0.5,
                format!("{km} km dans des zones d'avalanche connues, BERA non disponible"),
            ),
        }
    }
    Common {
        reasons: out,
        snow,
        weather_point: wp_id,
        weather,
        distance,
    }
}

pub(crate) fn status_for(data: &SiteData, date: NaiveDate) -> &'static str {
    match &data.status {
        Some(s) => {
            let polled = skimap_core::ephemeris::local_time(s.polled_at).date();
            if polled == date {
                "day"
            } else if polled < date && (date - polled).num_days() <= 3 {
                "last_known"
            } else {
                "none"
            }
        }
        None => "none",
    }
}

fn state_for(it: &Itinerary, status: &str) -> OpenState {
    if status == "none" {
        OpenState::Unknown
    } else {
        it.state
    }
}

/// Briefing of one day.
pub fn day(data: &SiteData, date: NaiveDate, opts: &Options) -> DayBriefing {
    let status = status_for(data, date);
    let crowd_date = crowd::Date::new(date.year(), date.month() as u8, date.day() as u8);
    // Crowd conditions from the weather: fresh snow of the two previous days, sun.
    let main_point = data.weather.as_ref().and_then(|w| w.points.first());
    let conditions = main_point.and_then(|p| {
        let d0 = date.and_hms_opt(0, 0, 0)?;
        p.covers(date, 9, 16).then(|| crowd::Conditions {
            fresh_snow_cm: p.snowfall_between(d0 - Duration::days(2), d0),
            sunny: p
                .summary(date, 9, 16)
                .cloud_mean_pct
                .is_some_and(|c| c < 30.0),
        })
    });
    let mut crowd = Some(CrowdInfo::from(crowd::forecast_day(
        &data.site,
        crowd_date,
        conditions.as_ref(),
    )));
    // The calendars and the opening dates of A4 are those of 2026-27: outside, only the
    // weekends and the conditions count, and the opening dates say nothing.
    let calendar_known = (NaiveDate::from_ymd_opt(2026, 9, 1).expect("date")
        ..=NaiveDate::from_ymd_opt(2027, 8, 31).expect("date"))
        .contains(&date);
    if let Some(c) = crowd.as_mut() {
        c.reasons.retain(|r| !r.starts_with("domaine fermé"));
        if !calendar_known {
            c.open = None;
        }
    }
    let bera_for_day = data.bera_for(date);
    let bera_level = bera_for_day.and_then(|b| b.level());
    let quality: Vec<DayQuality> = data
        .weather
        .iter()
        .flat_map(|w| &w.points)
        .filter_map(|p| {
            let snow_cm = data.snow_for(date).and_then(|m| {
                let nine = date.and_hms_opt(9, 0, 0)?;
                let from = m.valid_at.naive_utc() + Duration::hours(1);
                Some(m.sample(p.position)?.depth_cm + p.snowfall_between(from, nine))
            });
            snow_quality::day_quality(p, date, &snow_quality::Params::default(), snow_cm)
        })
        .collect();
    let ctx = DayContext {
        date,
        crowd,
        bera_level,
        bera_status: bera_for_day.map(|b| b.status),
        quality,
        opts,
    };
    let mut notes = data.missing.clone();
    if !calendar_known {
        notes.push(
            "calendriers scolaires connus seulement pour 2026-27 : l'affluence de ce jour ne \
             compte que les week-ends"
                .into(),
        );
    }
    match status {
        "none" => notes.push("état des pistes inconnu pour ce jour".into()),
        "last_known" => notes.push(format!(
            "état des pistes : dernier connu ({})",
            data.status
                .as_ref()
                .map(|s| skimap_core::ephemeris::local_time(s.polled_at)
                    .format("%d/%m %H:%M")
                    .to_string())
                .unwrap_or_default()
        )),
        _ => {}
    }
    if let Some(st) = &data.status {
        notes.extend(
            st.conflicts
                .iter()
                .map(|c| format!("sources en désaccord : {c}")),
        );
        if status != "none" {
            notes.extend(st.messages.iter().cloned());
        }
    }

    // Itineraries (nordic and shared trails).
    let mut itineraries = Vec::new();
    let empty = Vec::new();
    let its: &Vec<Itinerary> = data.status.as_ref().map_or(&empty, |s| &s.itineraries);
    for it in its.iter().filter(|i| i.practice != Practice::Snowshoe) {
        let at = it
            .line
            .get(it.line.len() / 2)
            .copied()
            .or_else(|| data.domains.first().map(|d| d.point));
        let Some(at) = at else { continue };
        let clpa_km = data
            .clpa
            .as_ref()
            .map(|c| {
                c.piste_km
                    .iter()
                    .filter(|(n, _)| crate::layers::same_name(n, &it.name))
                    .map(|(_, k)| *k)
                    .fold(0.0, f64::max)
            })
            .unwrap_or(0.0);
        let Common {
            mut reasons,
            snow,
            weather_point: wp,
            weather,
            distance,
        } = common_reasons(data, &ctx, at, &it.line, clpa_km);
        let state = state_for(it, status);
        let mut st = Vec::new();
        let no_snow = snow.is_some_and(|s| s.depth_cm() < 20.0);
        match state {
            OpenState::Open if no_snow => reason(
                &mut st,
                Factor::Status,
                0.0,
                "ouvert selon la source, mais neige estimée insuffisante : état sans doute hors \
                 saison, à vérifier"
                    .into(),
            ),
            OpenState::Open => reason(&mut st, Factor::Status, 3.0, "ouvert".into()),
            OpenState::Closed => reason(&mut st, Factor::Status, -10.0, "fermé".into()),
            OpenState::Unknown => reason(&mut st, Factor::Status, 0.0, "état inconnu".into()),
        }
        if status == "last_known" && state != OpenState::Unknown {
            st[0].text.push_str(" (dernier état connu)");
        }
        st.append(&mut reasons);
        let score = st.iter().map(|r| r.points).sum();
        itineraries.push(Candidate {
            kind: Kind::Itinerary,
            id: it.id.clone(),
            name: it.name.clone(),
            domain: domain_of(data, it).map(str::to_owned),
            practice: Some(it.practice),
            state,
            open_km: None,
            snow,
            weather_point: wp,
            weather,
            distance_km: distance,
            clpa_km,
            score,
            reasons: st,
        });
    }

    // Domains.
    let mut domains = Vec::new();
    for d in &data.domains {
        let pts: Vec<LonLat> = data
            .pistes
            .iter()
            .filter(|p| d.ways.contains(&p.way_id))
            .flat_map(|p| p.line.iter().copied())
            .collect();
        let clpa_km: f64 = data
            .clpa
            .as_ref()
            .map(|c| {
                c.piste_km
                    .iter()
                    .filter(|(n, _)| {
                        data.pistes.iter().any(|p| {
                            d.ways.contains(&p.way_id) && p.relations.iter().any(|r| r == *n)
                        })
                    })
                    .map(|(_, k)| *k)
                    .sum()
            })
            .unwrap_or(0.0);
        let Common {
            mut reasons,
            snow,
            weather_point: wp,
            weather,
            distance,
        } = common_reasons(data, &ctx, d.point, &pts, clpa_km);
        let mine: Vec<&Candidate> = itineraries
            .iter()
            .filter(|c| c.domain.as_deref() == Some(d.id.as_str()))
            .collect();
        let mut st = Vec::new();
        let mut open_km = None;
        let known: Vec<&&Candidate> = mine
            .iter()
            .filter(|c| c.state != OpenState::Unknown)
            .collect();
        let mut state = if known.is_empty() {
            let why = if mine.is_empty() {
                "pas de source d'état des pistes pour ce domaine"
            } else {
                "état des pistes inconnu pour ce jour"
            };
            reason(&mut st, Factor::Status, 0.0, why.into());
            OpenState::Unknown
        } else {
            let km = |c: &&&Candidate| {
                its.iter()
                    .find(|i| i.id == c.id)
                    .and_then(|i| i.length_km)
                    .unwrap_or(0.0)
            };
            let total: f64 = known.iter().map(km).sum();
            let open: f64 = known
                .iter()
                .filter(|c| c.state == OpenState::Open)
                .map(km)
                .sum();
            let n_open = known.iter().filter(|c| c.state == OpenState::Open).count();
            open_km = Some((open, total));
            let suffix = if status == "last_known" {
                " (dernier état connu)"
            } else {
                ""
            };
            if n_open == 0 {
                reason(
                    &mut st,
                    Factor::Status,
                    -10.0,
                    if known.len() == 1 {
                        format!("le seul itinéraire connu est fermé{suffix}")
                    } else {
                        format!(
                            "aucun des {} itinéraires connus n'est ouvert{suffix}",
                            known.len()
                        )
                    },
                );
                OpenState::Closed
            } else {
                let share = if total > 0.0 { open / total } else { 0.0 };
                let no_snow = snow.is_some_and(|s| s.depth_cm() < 20.0);
                let (points, caveat) = if no_snow {
                    (0.0, " mais neige estimée insuffisante : à vérifier")
                } else {
                    (3.0 * share, "")
                };
                reason(
                    &mut st,
                    Factor::Status,
                    points,
                    format!(
                        "{n_open} {} sur {} ({} km sur {}){suffix}{caveat}",
                        if n_open == 1 {
                            "itinéraire ouvert"
                        } else {
                            "itinéraires ouverts"
                        },
                        known.len(),
                        fmt_num(open, 0),
                        fmt_num(total, 0)
                    ),
                );
                OpenState::Open
            }
        };
        if ctx.crowd.as_ref().and_then(|c| c.open) == Some(false)
            && crate::layers::same_name(&d.name, data.site.name)
        {
            reason(
                &mut st,
                Factor::Status,
                -10.0,
                "domaine fermé à cette date (dates d'ouverture publiées par la commune)".into(),
            );
            state = OpenState::Closed;
        }
        st.append(&mut reasons);
        reason(
            &mut st,
            Factor::Size,
            (d.km_nordic / 50.0).min(1.0),
            format!("{} km de pistes nordiques (OSM)", fmt_num(d.km_nordic, 0)),
        );
        let score = st.iter().map(|r| r.points).sum();
        domains.push(Candidate {
            kind: Kind::Domain,
            id: d.id.clone(),
            name: d.name.clone(),
            domain: None,
            practice: None,
            state,
            open_km,
            snow,
            weather_point: wp,
            weather,
            distance_km: distance,
            clpa_km,
            score,
            reasons: st,
        });
    }
    let by_score =
        |a: &Candidate, b: &Candidate| b.score.total_cmp(&a.score).then(a.name.cmp(&b.name));
    domains.sort_by(by_score);
    itineraries.sort_by(by_score);

    let sunset = skimap_core::ephemeris::sun_times(date, data.site.bbox.center())
        .map(|(_, s)| skimap_core::ephemeris::local_time(s));
    let weather_available = data
        .weather
        .as_ref()
        .and_then(|w| w.points.first())
        .is_some_and(|p| p.covers(date, 9, 16));
    let mut b = DayBriefing {
        date,
        weather_available,
        status,
        crowd: ctx.crowd.clone(),
        bera_level,
        domains,
        itineraries,
        snow_quality: ctx.quality,
        sunset,
        notes,
        text: Vec::new(),
        voice: Vec::new(),
    };
    b.text = text(data, &b);
    b.voice = voice(data, &b);
    b
}

fn candidate_line(i: usize, c: &Candidate) -> String {
    let why: Vec<String> = c
        .reasons
        .iter()
        .map(|r| {
            if r.points == 0.0 {
                r.text.clone()
            } else {
                format!("{} [{:+.2}]", r.text, r.points).replace('.', ",")
            }
        })
        .collect();
    format!(
        "{}. {} (score {}) : {}.",
        i + 1,
        c.name,
        fmt_num(c.score, 2),
        why.join(" ; ")
    )
}

fn text(data: &SiteData, b: &DayBriefing) -> Vec<String> {
    let mut out = vec![format!("{} — {}", data.site.name, date_fr(b.date))];
    if let Some(l) = b.bera_level {
        out.push(format!(
            "BERA : risque {l} sur 5 (lisez le bulletin complet)."
        ));
    }
    out.push("Domaines :".into());
    out.extend(
        b.domains
            .iter()
            .enumerate()
            .map(|(i, c)| candidate_line(i, c)),
    );
    let open: Vec<&Candidate> = b
        .itineraries
        .iter()
        .filter(|c| c.state != OpenState::Closed)
        .take(5)
        .collect();
    if !b.itineraries.is_empty() {
        if open.is_empty() {
            out.push(format!(
                "Itinéraires : les {} itinéraires connus sont fermés.",
                b.itineraries.len()
            ));
        } else {
            out.push("Itinéraires (les 5 premiers non fermés) :".into());
            out.extend(open.iter().enumerate().map(|(i, c)| candidate_line(i, c)));
        }
    }
    for q in &b.snow_quality {
        out.push(format!(
            "Qualité de neige ({}) : {}",
            q.point,
            q.summary.join(" ")
        ));
    }
    if let Some(s) = b.sunset {
        out.push(format!(
            "Coucher du soleil {} h {:02}.",
            s.hour(),
            s.minute()
        ));
    }
    if !b.notes.is_empty() {
        out.push(format!("Données : {}.", b.notes.join(" ; ")));
    }
    out
}

fn voice(data: &SiteData, b: &DayBriefing) -> Vec<Event> {
    let mut v = vec![Event::Briefing(Line::Intro {
        site: data.site.name.to_owned(),
    })];
    if data.no_bera() {
        v.push(Event::Briefing(Line::Avalanche { level: None }));
    } else if let Some(l) = b.bera_level {
        v.push(Event::Briefing(Line::Avalanche { level: Some(l) }));
    }
    // Open pistes of the best domain with a known status.
    if let Some((open, total)) = b.domains.iter().find(|c| c.open_km.is_some()).map(|c| {
        let mine: Vec<&Candidate> = b
            .itineraries
            .iter()
            .filter(|i| i.domain.as_deref() == Some(c.id.as_str()) && i.state != OpenState::Unknown)
            .collect();
        (
            mine.iter().filter(|i| i.state == OpenState::Open).count() as u32,
            mine.len() as u32,
        )
    }) {
        v.push(Event::Briefing(Line::PistesOpen { open, total }));
    }
    // Recommendation: the best open itinerary, else the best domain not closed.
    let snowy = |c: &Candidate| c.snow.is_none_or(|s| s.depth_cm() >= 20.0);
    if let Some(it) = b
        .itineraries
        .iter()
        .find(|c| c.state == OpenState::Open && snowy(c))
    {
        v.push(Event::Briefing(Line::Recommended {
            name: Name::Piste(PisteName::new(it.name.clone(), None)),
        }));
    } else if let Some(d) = b
        .domains
        .iter()
        .find(|c| c.state != OpenState::Closed && c.snow.is_some_and(|s| s.depth_cm() >= 20.0))
    {
        v.push(Event::Briefing(Line::Recommended {
            name: Name::Place(d.name.clone()),
        }));
    }
    if let Some(w) = b.domains.first().and_then(|d| d.weather.as_ref())
        && let Some(t) = w.temperature_mean_c
    {
        v.push(Event::Briefing(Line::Weather {
            temperature_c: t,
            wind_kmh: w.wind_max_kmh.unwrap_or(0.0),
            fog: w.fog_hours >= 2,
            snowfall: w.snowfall_cm >= 1.0,
        }));
    }
    if let Some(s) = b
        .snow_quality
        .first()
        .filter(|q| q.enough_snow)
        .and_then(|q| q.flat.voice())
    {
        v.push(Event::Briefing(Line::Snow(s)));
    }
    if let Some(c) = &b.crowd {
        v.push(Event::Briefing(Line::Crowd(match c.level {
            CrowdLevel::Calm => VoiceCrowd::Low,
            CrowdLevel::Moderate => VoiceCrowd::Medium,
            CrowdLevel::Busy | CrowdLevel::VeryBusy => VoiceCrowd::High,
        })));
    }
    if let Some(s) = b.sunset {
        v.push(Event::Briefing(Line::Sunset {
            at: Clock::nearest_quarter(s.hour() as u8, s.minute() as u8),
        }));
    }
    v
}

/// Briefing of `date` and the `extra_days` following days.
pub fn brief(data: &SiteData, date: NaiveDate, extra_days: u32, opts: &Options) -> SiteBriefing {
    SiteBriefing {
        site: data.site.id.to_owned(),
        days: (0..=i64::from(extra_days))
            .map(|i| day(data, date + Duration::days(i), opts))
            .collect(),
    }
}

/// Spoken text of a day (voice bank, Android fallback voice).
pub fn spoken(b: &DayBriefing) -> String {
    b.voice
        .iter()
        .map(|e| e.announce().text())
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::layers::{Area, Domain, Status, Weather, WeatherHour};
    use crate::snow_quality::Quality;
    /// Evaluation site of `data/sites.json`, test data.
    static CHAPELLE_DES_BOIS: std::sync::LazyLock<skimap_common::site::Site> =
        std::sync::LazyLock::new(|| {
            *skimap_common::site::site("chapelle-des-bois").expect("evaluation site")
        });

    fn hours(date: NaiveDate, f: impl Fn(u32) -> (f64, f64, f64, f64)) -> Vec<WeatherHour> {
        (0..24 * 3)
            .map(|i| {
                let time =
                    date.and_hms_opt(0, 0, 0).unwrap() - Duration::days(1) + Duration::hours(i);
                let (t, gust, cloud, snow) = f(time.hour());
                WeatherHour {
                    time,
                    temperature_c: Some(t),
                    apparent_temperature_c: None,
                    wind_chill_c: Some(t - 2.0),
                    wind_kmh: Some(gust / 2.0),
                    gust_kmh: Some(gust),
                    visibility_m: None,
                    fog: Some(false),
                    freezing_level_m: None,
                    snowfall_cm: Some(snow),
                    precipitation_mm: Some(snow),
                    cloud_cover_pct: Some(cloud),
                    relative_humidity_pct: None,
                    weather_code: None,
                }
            })
            .collect()
    }

    fn square(lon: f64, lat: f64) -> Area {
        let g = serde_json::json!({"type":"Polygon","coordinates":[[[lon-0.01,lat-0.01],[lon+0.01,lat-0.01],[lon+0.01,lat+0.01],[lon-0.01,lat+0.01],[lon-0.01,lat-0.01]]]});
        Area::new("d", crate::layers::multipolygon(&g))
    }

    fn data(date: NaiveDate, gust_b: f64) -> SiteData {
        let a = LonLat::new(6.10, 46.60);
        let b = LonLat::new(6.20, 46.60);
        let point = |id: &str, p: LonLat, gust: f64| WeatherPoint {
            id: id.into(),
            name: id.into(),
            position: p,
            elevation_m: 1100.0,
            hours: hours(date, |_| (-5.0, gust, 80.0, 0.0)),
        };
        let it = |id: &str, name: &str, p: LonLat, state| Itinerary {
            id: id.into(),
            name: name.into(),
            source: "enj".into(),
            sector: None,
            practice: Practice::Nordic,
            state,
            length_km: Some(10.0),
            difficulty: None,
            comment: None,
            line: vec![p, LonLat::new(p.lon + 0.001, p.lat)],
            osm_way_ids: Vec::new(),
        };
        SiteData {
            site: *CHAPELLE_DES_BOIS,
            weather: Some(Weather {
                points: vec![point("a", a, 20.0), point("b", b, gust_b)],
                files: vec![],
            }),
            snows: vec![],
            beras: vec![],
            status: Some(Status {
                polled_at: crate::sun::local_to_utc(date.and_hms_opt(8, 0, 0).unwrap()),
                itineraries: vec![
                    it("1", "Alpha", a, OpenState::Open),
                    it("2", "Beta", b, OpenState::Open),
                    it("3", "Gamma", a, OpenState::Closed),
                ],
                messages: vec![],
                conflicts: vec![],
            }),
            domains: vec![
                Domain {
                    id: "da".into(),
                    name: "Domaine A".into(),
                    km_nordic: 50.0,
                    point: a,
                    area: square(a.lon, a.lat),
                    ways: vec![],
                },
                Domain {
                    id: "db".into(),
                    name: "Domaine B".into(),
                    km_nordic: 50.0,
                    point: b,
                    area: square(b.lon, b.lat),
                    ways: vec![],
                },
            ],
            pistes: vec![],
            clpa: None,
            no_network: None,
            zones: None,
            route: None,
            graph: None,
            exposure: None,
            missing: vec![],
            sources: vec![],
            files: vec![],
        }
    }

    #[test]
    fn wind_and_closures_rank_with_reasons() {
        let date = NaiveDate::from_ymd_opt(2027, 1, 12).unwrap(); // a Tuesday, no holidays
        let d = data(date, 70.0);
        let b = day(&d, date, &Options::default());
        assert_eq!(b.status, "day");
        // Domain B has 70 km/h gusts: A first.
        assert_eq!(b.domains[0].id, "da");
        let wind_b = b.domains[1]
            .reasons
            .iter()
            .find(|r| r.factor == Factor::Wind)
            .unwrap();
        assert_eq!(wind_b.points, -2.0);
        assert!(wind_b.text.contains("70 km/h"));
        // Domain A: 1 of 2 open (10 of 20 km): +1.5.
        let st = &b.domains[0].reasons[0];
        assert_eq!(st.factor, Factor::Status);
        assert!((st.points - 1.5).abs() < 1e-9, "{st:?}");
        // The closed itinerary ranks last; the voice recommends the best open one.
        assert_eq!(b.itineraries.last().unwrap().name, "Gamma");
        assert!(
            spoken(&b).contains("Conseillé aujourd'hui. Alpha."),
            "{}",
            spoken(&b)
        );
        assert!(
            b.voice
                .iter()
                .any(|e| matches!(e, Event::Briefing(Line::PistesOpen { open: 1, total: 2 })))
        );
        // Home near B: distance reason appears and reduces A.
        let opts = Options {
            home: Some(LonLat::new(6.25, 46.60)),
        };
        let b2 = day(&d, date, &opts);
        let dist = b2.domains.iter().find(|c| c.id == "da").unwrap();
        assert!(
            dist.reasons
                .iter()
                .any(|r| r.factor == Factor::Distance && r.points < -0.2)
        );
        // Day after: last known status; two days before: no status.
        let later = day(&d, date + Duration::days(1), &Options::default());
        assert_eq!(later.status, "last_known");
        let before = day(&d, date - Duration::days(2), &Options::default());
        assert_eq!(before.status, "none");
        assert!(
            before
                .itineraries
                .iter()
                .all(|c| c.state == OpenState::Unknown)
        );
    }

    #[test]
    fn french_dates() {
        assert_eq!(
            date_fr(NaiveDate::from_ymd_opt(2026, 1, 5).unwrap()),
            "lundi 5 janvier 2026"
        );
    }

    /// A uniform snow map of `cm` over the test area, valid at 06 h UTC of `date`.
    fn snow(date: NaiveDate, cm: f32) -> crate::layers::SnowMap {
        use skimap_common::grid::{GeoTransform, Grid};
        let (x0, y0) = skimap_common::crs::to_lambert93(LonLat::new(6.0, 46.7)).unwrap();
        let (x1, y1) = skimap_common::crs::to_lambert93(LonLat::new(6.3, 46.5)).unwrap();
        let w = ((x1 - x0) / 100.0).ceil() as usize;
        let h = ((y0 - y1) / 100.0).ceil() as usize;
        crate::layers::SnowMap {
            valid_at: date.and_hms_opt(6, 0, 0).unwrap().and_utc(),
            depth: Grid::filled(
                w,
                h,
                GeoTransform::north_up(x0, y0, 100.0),
                skimap_common::crs::Crs::Lambert93,
                cm,
            ),
            low: None,
            high: None,
        }
    }

    fn w(f: impl FnOnce(&mut DayWeather)) -> DayWeather {
        let mut d = DayWeather {
            hours: 7,
            ..Default::default()
        };
        f(&mut d);
        d
    }

    fn reasons_of(d: &DayWeather) -> Vec<(Factor, f64, String)> {
        let mut out = Vec::new();
        weather_reasons(&mut out, d);
        out.into_iter()
            .map(|r| (r.factor, r.points, r.text))
            .collect()
    }

    #[test]
    fn weather_reason_thresholds() {
        let one = |d: DayWeather| reasons_of(&d).remove(0);
        assert_eq!(
            one(w(|d| d.gust_max_kmh = Some(24.9))),
            (Factor::Wind, 0.0, "vent faible (rafales 25 km/h)".into())
        );
        assert_eq!(
            one(w(|d| d.gust_max_kmh = Some(25.0))),
            (Factor::Wind, 0.0, "vent modéré (rafales 25 km/h)".into())
        );
        assert_eq!(one(w(|d| d.gust_max_kmh = Some(40.0))).1, -1.0);
        assert_eq!(one(w(|d| d.gust_max_kmh = Some(39.9))).1, 0.0);
        assert_eq!(one(w(|d| d.gust_max_kmh = Some(60.0))).1, -2.0);
        assert_eq!(one(w(|d| d.gust_max_kmh = Some(59.9))).1, -1.0);
        assert_eq!(
            one(w(|d| d.wind_chill_min_c = Some(-20.0))),
            (Factor::Cold, -1.5, "très froid, ressenti -20 °C".into())
        );
        assert_eq!(one(w(|d| d.wind_chill_min_c = Some(-19.9))).1, -0.5);
        assert_eq!(
            one(w(|d| d.wind_chill_min_c = Some(-12.0))),
            (Factor::Cold, -0.5, "froid, ressenti -12 °C".into())
        );
        assert!(reasons_of(&w(|d| d.wind_chill_min_c = Some(-11.9))).is_empty());
        assert_eq!(
            one(w(|d| {
                d.wind_chill_min_c = Some(-0.4);
                d.temperature_mean_c = Some(-0.3);
            })),
            (Factor::Cold, 0.0, "0 °C en moyenne, ressenti 0 °C".into())
        );
        assert_eq!(
            one(w(|d| d.fog_hours = 4)),
            (Factor::Fog, -1.0, "brouillard 4 h sur 7".into())
        );
        assert_eq!(
            one(w(|d| d.fog_hours = 3)),
            (Factor::Fog, -0.5, "brouillard 3 h sur 7".into())
        );
        assert_eq!(one(w(|d| d.fog_hours = 2)).1, -0.5);
        assert!(reasons_of(&w(|d| d.fog_hours = 1)).is_empty());
        assert_eq!(
            one(w(|d| d.snowfall_cm = 5.0)),
            (
                Factor::Snowfall,
                -0.5,
                "5 cm de neige pendant la journée".into()
            )
        );
        assert_eq!(
            one(w(|d| d.snowfall_cm = 4.9)),
            (
                Factor::Snowfall,
                0.0,
                "5 cm de neige pendant la journée".into()
            )
        );
        assert_eq!(one(w(|d| d.snowfall_cm = 1.0)).1, 0.0);
        assert!(reasons_of(&w(|d| d.snowfall_cm = 0.9)).is_empty());
        assert_eq!(
            one(w(|d| d.rain_mm = 1.0)),
            (Factor::Rain, -1.5, "pluie (1,0 mm)".into())
        );
        assert!(reasons_of(&w(|d| d.rain_mm = 0.9)).is_empty());
        assert_eq!(
            one(w(|d| d.cloud_mean_pct = Some(29.9))),
            (Factor::Sun, 0.5, "ensoleillé".into())
        );
        assert!(reasons_of(&w(|d| d.cloud_mean_pct = Some(30.0))).is_empty());
    }

    #[test]
    fn snow_and_surface_reasons() {
        let stats = |median: f64, added: f64| SnowStats {
            samples: 3,
            median_cm: median,
            p10_cm: median,
            share_20cm: 1.0,
            added_cm: added,
            map_date: NaiveDate::from_ymd_opt(2027, 1, 12).unwrap(),
        };
        let first = |s: Option<SnowStats>, fresh: Option<f64>| {
            let mut out = Vec::new();
            snow_reasons(&mut out, &s, fresh);
            out
        };
        assert_eq!(first(Some(stats(19.9, 0.0)), None)[0].points, -3.0);
        let r = &first(Some(stats(19.0, 1.0)), None)[0];
        assert_eq!(r.points, 0.5, "19 cm and 1 cm since: 20 cm");
        assert_eq!(
            r.text,
            "20 cm de neige estimée (médiane, carte du 12/01 dont 1 cm tombés depuis, 100 % des points ≥ 20 cm) : limite"
        );
        assert_eq!(first(Some(stats(39.9, 0.0)), None)[0].points, 0.5);
        let r = &first(Some(stats(40.0, 0.4)), None)[0];
        assert_eq!(r.points, 2.0);
        assert!(
            r.text.contains("carte du 12/01, 100 %"),
            "no « dont » under 0.5 cm"
        );
        let r = first(None, Some(5.0));
        assert_eq!(r[0].points, -1.5);
        assert_eq!(
            (r[1].factor, r[1].points, r[1].text.as_str()),
            (Factor::FreshSnow, 0.5, "5 cm de neige fraîche en 24 h")
        );
        assert_eq!(first(None, Some(4.9)).len(), 1);
        let s = |q: Quality| {
            let mut out = Vec::new();
            surface_reason(&mut out, Some(q));
            out[0].points
        };
        assert_eq!(s(Quality::Fresh), 0.5);
        assert_eq!(s(Quality::Transformed), 0.5);
        assert_eq!(s(Quality::OldDry), 0.0);
        assert_eq!(s(Quality::Windblown), -0.5);
        assert_eq!(s(Quality::Hard), -0.5);
        assert_eq!(s(Quality::Wet), -0.5);
        assert_eq!(s(Quality::Icy), -1.0);
        assert_eq!(s(Quality::Slush), -1.0);
        let mut out = Vec::new();
        surface_reason(&mut out, None);
        assert!(out.is_empty());
        assert_eq!(crowd_points(CrowdLevel::Calm), 0.0);
        assert_eq!(crowd_points(CrowdLevel::Moderate), -0.25);
        assert_eq!(crowd_points(CrowdLevel::Busy), -0.5);
        assert_eq!(crowd_points(CrowdLevel::VeryBusy), -1.0);
        let mut v = vec![5.0, 1.0, 4.0, 2.0, 3.0];
        assert_eq!(quantile(&mut v, 0.5), 3.0);
        assert_eq!(quantile(&mut v, 0.9), 5.0);
        assert_eq!(quantile(&mut v, 0.1), 1.0);
    }

    #[test]
    fn snow_maps_domains_and_avalanche_paths() {
        let date = NaiveDate::from_ymd_opt(2027, 1, 12).unwrap();
        let mut d = data(date, 20.0);
        // 15 cm on the map, 5 cm since: 20 cm, all points at 20 cm or more.
        d.snows = vec![snow(date, 15.0)];
        for p in d.weather.as_mut().unwrap().points.iter_mut() {
            for h in &mut p.hours {
                h.snowfall_cm = Some(
                    if h.time.date() == date && (7..12).contains(&h.time.hour()) {
                        1.0
                    } else {
                        0.0
                    },
                );
            }
        }
        let s = snow_stats(&d.snows[0], &[LonLat::new(6.1, 46.6)], 5.0).unwrap();
        assert_eq!((s.median_cm, s.added_cm, s.share_20cm), (15.0, 5.0, 1.0));
        assert_eq!(
            snow_stats(&d.snows[0], &[LonLat::new(7.5, 46.6)], 5.0),
            None
        );
        // A3 at the points: the map plus the snowfall from 07 h to 09 h (2 cm).
        let b = day(&d, date, &Options::default());
        assert!(
            b.snow_quality.iter().all(|q| q.snow_cm == Some(17.0)),
            "{:?}",
            b.snow_quality.iter().map(|q| q.snow_cm).collect::<Vec<_>>()
        );
        // The candidates: snowfall from 07 h until 09 h added.
        let c = b.itineraries.iter().find(|c| c.name == "Alpha").unwrap();
        assert_eq!(c.snow.unwrap().added_cm, 2.0);
        assert!(b.notes.iter().all(|n| !n.starts_with("aucune carte")));
        // Exactly 20 cm: enough (not « hors saison »).
        d.snows = vec![snow(date, 20.0)];
        for p in d.weather.as_mut().unwrap().points.iter_mut() {
            for h in &mut p.hours {
                h.snowfall_cm = Some(0.0);
            }
        }
        let b = day(&d, date, &Options::default());
        let alpha = b.itineraries.iter().find(|c| c.name == "Alpha").unwrap();
        assert_eq!(alpha.reasons[0].text, "ouvert");
        assert!(!b.domains[0].reasons[0].text.contains("insuffisante"));
        // A map too old for the day: said.
        let b = day(&d, date + Duration::days(5), &Options::default());
        assert!(
            b.domains[0]
                .reasons
                .iter()
                .any(|r| r.text == "aucune carte d'enneigement de ce jour ou des 4 jours avant")
        );
        // An itinerary outside every domain belongs to none.
        let it = Itinerary {
            line: vec![LonLat::new(6.5, 46.9)],
            ..d.status.as_ref().unwrap().itineraries[0].clone()
        };
        assert_eq!(domain_of(&d, &it), None);
        assert_eq!(
            domain_of(&d, &d.status.as_ref().unwrap().itineraries[0]),
            Some("da")
        );
        // Historic avalanche paths: with the BERA of the day.
        d.clpa = Some(crate::layers::Clpa {
            covered: true,
            areas: Default::default(),
            piste_km: [("Alpha".to_owned(), 1.2), ("Ailleurs".to_owned(), 3.0)]
                .into_iter()
                .collect(),
        });
        d.pistes = vec![crate::layers::Piste {
            way_id: 7,
            name: Some("Alpha".into()),
            relations: vec!["Alpha".into()],
            uses: vec!["nordic".into()],
            line: vec![LonLat::new(6.10, 46.60), LonLat::new(6.101, 46.60)],
        }];
        d.domains[0].ways = vec![7];
        let clpa = |b: &DayBriefing, name: &str| {
            b.domains
                .iter()
                .chain(&b.itineraries)
                .find(|c| c.name == name)
                .unwrap()
                .reasons
                .iter()
                .find(|r| r.factor == Factor::Avalanche)
                .map(|r| (r.points, r.text.clone()))
        };
        let b = day(&d, date, &Options::default());
        assert_eq!(
            clpa(&b, "Alpha"),
            Some((
                -0.5,
                "1,2 km dans des zones d'avalanche connues, BERA non disponible".into()
            ))
        );
        assert_eq!(
            b.domains.iter().find(|c| c.id == "da").unwrap().clpa_km,
            1.2
        );
        assert_eq!(
            b.domains.iter().find(|c| c.id == "db").unwrap().clpa_km,
            0.0
        );
        let bera = |level: u8| crate::layers::Bera {
            site: "test".into(),
            date: date.to_string(),
            massif: None,
            status: BeraStatus::Bulletin,
            pdf_url: None,
            bulletin: Some(crate::layers::BeraBulletin {
                massif: "X".into(),
                date_validity: None,
                risk: crate::layers::BeraRisk {
                    max: Some(level),
                    ..Default::default()
                },
                aspects: Default::default(),
                accidental: None,
                natural: None,
                snow_quality: None,
            }),
            notice: String::new(),
            links: vec![],
        };
        d.beras = vec![bera(3)];
        let b = day(&d, date, &Options::default());
        assert_eq!(
            clpa(&b, "Alpha"),
            Some((
                -2.0,
                "1,2 km dans des zones d'avalanche connues, BERA 3 sur 5".into()
            ))
        );
        d.beras = vec![bera(2)];
        let b = day(&d, date, &Options::default());
        assert_eq!(
            clpa(&b, "Alpha"),
            Some((
                0.0,
                "1,2 km dans des zones d'avalanche connues (BERA 2 sur 5)".into()
            ))
        );
    }

    #[test]
    fn domain_status_from_its_itineraries() {
        let date = NaiveDate::from_ymd_opt(2027, 1, 12).unwrap();
        let mut d = data(date, 20.0);
        let its = &mut d.status.as_mut().unwrap().itineraries;
        // Alpha open (10 km), Gamma closed (4 km) in domain A.
        its[2].length_km = Some(4.0);
        let b = day(&d, date, &Options::default());
        let a = b.domains.iter().find(|c| c.id == "da").unwrap();
        assert_eq!(a.open_km, Some((10.0, 14.0)));
        assert!((a.reasons[0].points - 3.0 * 10.0 / 14.0).abs() < 1e-12);
        assert_eq!(
            a.reasons[0].text,
            "1 itinéraire ouvert sur 2 (10 km sur 14)"
        );
        // Every known itinerary closed: −10, said with their number.
        let its = &mut d.status.as_mut().unwrap().itineraries;
        its[0].state = OpenState::Closed;
        let b = day(&d, date, &Options::default());
        let a = b.domains.iter().find(|c| c.id == "da").unwrap();
        assert_eq!(a.state, OpenState::Closed);
        assert_eq!(
            (a.reasons[0].points, a.reasons[0].text.as_str()),
            (-10.0, "aucun des 2 itinéraires connus n'est ouvert")
        );
        // Domain B has one itinerary, closed.
        d.status.as_mut().unwrap().itineraries[1].state = OpenState::Closed;
        let b = day(&d, date, &Options::default());
        let bb = b.domains.iter().find(|c| c.id == "db").unwrap();
        assert_eq!(bb.reasons[0].text, "le seul itinéraire connu est fermé");
        // Itineraries without length: no share, no points (not NaN).
        let its = &mut d.status.as_mut().unwrap().itineraries;
        for i in its.iter_mut() {
            i.length_km = None;
            i.state = OpenState::Open;
        }
        let b = day(&d, date, &Options::default());
        let a = b.domains.iter().find(|c| c.id == "da").unwrap();
        assert_eq!(a.open_km, Some((0.0, 0.0)));
        assert_eq!(a.reasons[0].points, 0.0);
        // Two days before the poll: no status, said, and the best domain not closed with
        // snow is recommended.
        d.snows = vec![snow(date - Duration::days(2), 30.0)];
        d.pistes = vec![crate::layers::Piste {
            way_id: 7,
            name: Some("Alpha".into()),
            relations: vec![],
            uses: vec!["nordic".into()],
            line: vec![LonLat::new(6.10, 46.60), LonLat::new(6.101, 46.60)],
        }];
        d.domains[0].ways = vec![7];
        let b = day(&d, date - Duration::days(2), &Options::default());
        assert_eq!(b.status, "none");
        assert!(
            b.notes
                .iter()
                .any(|n| n == "état des pistes inconnu pour ce jour")
        );
        assert!(
            spoken(&b).contains("Conseillé aujourd'hui. Domaine "),
            "{}",
            spoken(&b)
        );
        // Mean cloud cover of 30 %: not sunny for the crowd.
        for p in d.weather.as_mut().unwrap().points.iter_mut() {
            for h in &mut p.hours {
                h.cloud_cover_pct = Some(30.0);
            }
        }
        let b = day(&d, date, &Options::default());
        assert!(b.crowd.unwrap().reasons.is_empty());
        for p in d.weather.as_mut().unwrap().points.iter_mut() {
            for h in &mut p.hours {
                h.cloud_cover_pct = Some(29.0);
            }
        }
        let b = day(&d, date, &Options::default());
        assert_eq!(b.crowd.unwrap().reasons, ["soleil"]);
    }
}
