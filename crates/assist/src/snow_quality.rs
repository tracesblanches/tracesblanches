//! Snow quality per hour and per aspect (A3, #90): overnight refreeze, softening from
//! temperature, sun and aspect; best slot and best aspect.
//!
//! **This is a heuristic**, not a snowpack model: it tells which slopes soften first and
//! when the snow turns heavy, from the hourly weather of a forecast point. The real
//! validation is the field observations of T5 (#47); until then it has only been checked
//! qualitatively on past days (README, issue #90).
//!
//! # Slope classes
//!
//! The 17 classes of the Météo-France snow simulation used by P6: flat, and the 8 aspects
//! (N, NE, E, SE, S, SW, W, NW) at 20° and at 40°. The weather is the one of the forecast
//! point (its model elevation); the classes are generic slopes around it.
//!
//! # Surface energy index, per hour
//!
//! ```text
//! E = (1 − α) · G  +  H · T  −  L          (W/m², positive: melt, negative: refreeze)
//! G = global irradiance on the slope with cloud cover (module `sun`)
//! α = albedo: 0.85 fresh snow, 0.75 old dry snow, 0.65 transformed, wet or crusted
//! H = 12 + 0.5 · wind (km/h), at most 35 W/m²/K   (turbulent exchange, linearised)
//! T = air temperature at 2 m, °C (the surface is taken at 0 °C)
//! L = 15 + 65 · (1 − N)  (net long-wave loss, N = cloud fraction: ≈ 80 clear, 15 overcast)
//! ```
//!
//! The constants are orders of magnitude of the snow surface energy balance (net
//! long-wave loss 60 to 100 W/m² under a clear sky, turbulent coefficient 10 to 30
//! W/m²/K), chosen so that the index changes sign where experience puts it: a clear
//! January day at −3 °C crusts a 40° south slope at noon but not a flat field; a clear
//! April day at +2 °C softens the east slopes in the morning and turns the south ones to
//! slush in the early afternoon while the 40° north slopes stay hard.
//!
//! # Surface state, per hour
//!
//! `E · 3600 / 334 000` is the water (mm) melted (E > 0) or refrozen (E < 0) in the hour:
//! melt first pays the cold content of the surface layer (at most 2 mm, about the top 20
//! cm cooled by 5 °C), then eats the crust and wets fresh snow; refreeze turns liquid
//! water into crust, then cools the layer.
//! Snowfall adds fresh snow (it settles by 1.5 % an hour), rain adds water, wind of 30
//! km/h or gusts of 55 km/h on dry fresh snow make it wind-blown. The surface holds at most
//! 8 mm of water and 40 % of the water above 0.3 mm (the film it retains) percolates down
//! each melting hour, so the wetness settles
//! where melt and percolation balance. The surface class is,
//! in this order: fresh (≥ 2 cm, dry), wind-blown fresh, slush (≥ 6 mm of water), wet
//! (≥ 3 mm), transformed (≥ 0.2 mm: the « décaille » of spring snow), icy crust (≥ 3 mm
//! refrozen after rain), hard refrozen crust (≥ 0.2 mm), otherwise old dry snow.
//!
//! The simulation starts at the first hour of the weather series with the old dry state,
//! and again after any gap of more than 6 hours: a day is only evaluated with at least 24
//! hours of history without gap before 06 h.
//!
//! # Best slot and best aspect
//!
//! Each class gives a score per hour (fresh 3, transformed 3, old dry 2.5, wind-blown 1.5,
//! hard 1, wet 1, icy 0, slush 0) for nordic skiing and ski touring. The best slot of a
//! class is the longest run of daylight hours (from 30 min after sunrise or 08 h, to 30
//! min before sunset or 17 h) with its best score; the best aspect is the class whose
//! slot has the best score, then is the longest, then has the best mean over the day.
//!
//! Not modelled: shade of the relief and forest, snow drift, grooming (the groomer breaks
//! the crust in the morning), the snowpack below the surface.

use chrono::{Duration, NaiveDate, NaiveDateTime, Timelike};
use serde::Serialize;
use skimap_common::geom::LonLat;

use crate::layers::{WeatherHour, WeatherPoint};
use crate::sun;

/// A slope class: flat, or an aspect at a slope angle.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct SlopeClass {
    /// Slope angle, degrees.
    pub slope_deg: f64,
    /// Aspect (direction the slope faces), degrees from north; `None` for flat.
    pub aspect_deg: Option<f64>,
}

const ASPECTS: [(&str, &str); 8] = [
    ("N", "nord"),
    ("NE", "nord-est"),
    ("E", "est"),
    ("SE", "sud-est"),
    ("S", "sud"),
    ("SW", "sud-ouest"),
    ("W", "ouest"),
    ("NW", "nord-ouest"),
];

impl SlopeClass {
    /// Short code (`flat`, `S20`, `NE40`).
    pub fn code(&self) -> String {
        match self.aspect_deg {
            None => "flat".into(),
            Some(a) => format!("{}{}", ASPECTS[octant(a)].0, self.slope_deg.round()),
        }
    }

    /// French label (« plat », « sud, pente de 20° »).
    pub fn label_fr(&self) -> String {
        match self.aspect_deg {
            None => "plat".into(),
            Some(a) => format!(
                "{}, pente de {}°",
                ASPECTS[octant(a)].1,
                self.slope_deg.round()
            ),
        }
    }

    /// French aspect name, `None` for flat.
    pub fn aspect_fr(&self) -> Option<&'static str> {
        self.aspect_deg.map(|a| ASPECTS[octant(a)].1)
    }
}

fn octant(a: f64) -> usize {
    ((a.rem_euclid(360.0) + 22.5) / 45.0) as usize % 8
}

/// The 17 classes: flat, then the 8 aspects at 20°, then at 40°.
pub fn classes() -> Vec<SlopeClass> {
    let mut v = vec![SlopeClass {
        slope_deg: 0.0,
        aspect_deg: None,
    }];
    for slope in [20.0, 40.0] {
        for i in 0..8 {
            v.push(SlopeClass {
                slope_deg: slope,
                aspect_deg: Some(45.0 * i as f64),
            });
        }
    }
    v
}

/// Surface quality.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Quality {
    /// Fresh dry snow.
    Fresh,
    /// Fresh snow worked by the wind.
    Windblown,
    /// Old dry snow, never wetted since it fell.
    OldDry,
    /// Softening refrozen snow (spring snow, « ça décaille »).
    Transformed,
    /// Wet, heavy snow.
    Wet,
    /// Slush (« soupe »).
    Slush,
    /// Hard refrozen crust.
    Hard,
    /// Thick refrozen crust, icy.
    Icy,
}

impl Quality {
    /// French label.
    pub fn label_fr(self) -> &'static str {
        match self {
            Quality::Fresh => "neige fraîche",
            Quality::Windblown => "neige fraîche soufflée par le vent",
            Quality::OldDry => "neige ancienne froide et sèche",
            Quality::Transformed => "neige transformée qui ramollit (décaille)",
            Quality::Wet => "neige humide et lourde",
            Quality::Slush => "neige soupe",
            Quality::Hard => "neige dure, regelée",
            Quality::Icy => "croûte de regel épaisse, plaques de glace",
        }
    }

    /// Score for nordic skiing and touring (0 bad to 3 good).
    pub fn score(self) -> f64 {
        match self {
            Quality::Fresh | Quality::Transformed => 3.0,
            Quality::OldDry => 2.5,
            Quality::Windblown => 1.5,
            Quality::Hard | Quality::Wet => 1.0,
            Quality::Icy | Quality::Slush => 0.0,
        }
    }

    /// Phrase of the voice bank, when one exists (old dry snow has none yet).
    pub fn voice(self) -> Option<skimap_voice::bank::Snow> {
        use skimap_voice::bank::Snow;
        Some(match self {
            Quality::Fresh => Snow::Fresh,
            Quality::Windblown => Snow::Windblown,
            Quality::Transformed => Snow::Transformed,
            Quality::Wet | Quality::Slush => Snow::Wet,
            Quality::Hard => Snow::Hard,
            Quality::Icy => Snow::Icy,
            Quality::OldDry => return None,
        })
    }

    /// Whether the surface is wet or softening (for the wax model).
    pub fn is_wet(self) -> bool {
        matches!(self, Quality::Transformed | Quality::Wet | Quality::Slush)
    }
}

/// Model constants (see the module documentation).
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct Params {
    /// Base turbulent exchange coefficient, W/m²/K.
    pub h_base: f64,
    /// Increase per km/h of wind, W/m²/K.
    pub h_per_kmh: f64,
    /// Maximum exchange coefficient, W/m²/K.
    pub h_max: f64,
    /// Net long-wave loss under overcast sky, W/m².
    pub lw_overcast: f64,
    /// Additional long-wave loss under clear sky, W/m².
    pub lw_clear_extra: f64,
    /// Hourly settlement of fresh snow (fraction kept).
    pub settle_per_hour: f64,
    /// Wind making fresh snow wind-blown, km/h.
    pub wind_drift_kmh: f64,
    /// Gusts making fresh snow wind-blown, km/h.
    pub gust_drift_kmh: f64,
}

impl Default for Params {
    fn default() -> Self {
        Self {
            h_base: 12.0,
            h_per_kmh: 0.5,
            h_max: 35.0,
            lw_overcast: 15.0,
            lw_clear_extra: 65.0,
            settle_per_hour: 0.985,
            wind_drift_kmh: 30.0,
            gust_drift_kmh: 55.0,
        }
    }
}

/// State of the surface at the end of an hour.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct HourState {
    /// Local time of the start of the hour.
    pub time: NaiveDateTime,
    /// Surface quality.
    pub quality: Quality,
    /// Score of the quality.
    pub score: f64,
    /// Energy index, W/m².
    pub energy_wm2: f64,
    /// Liquid water in the surface, mm.
    pub liquid_mm: f64,
    /// Refrozen crust, mm of water.
    pub crust_mm: f64,
    /// Fresh snow on top, cm.
    pub fresh_cm: f64,
    /// Sun elevation, degrees.
    pub sun_elevation_deg: f64,
}

#[derive(Debug, Clone, Copy, Default)]
struct Surface {
    liquid: f64,
    crust: f64,
    fresh: f64,
    windblown: bool,
    /// Rain has wetted the surface since the last fresh snow.
    rained: bool,
    /// Cold content of the surface layer, mm of melt it takes to bring it to 0 °C.
    cold: f64,
}

impl Surface {
    fn quality(&self) -> Quality {
        if self.fresh >= 2.0 && self.liquid < 0.5 {
            return if self.windblown {
                Quality::Windblown
            } else {
                Quality::Fresh
            };
        }
        if self.liquid >= 6.0 {
            Quality::Slush
        } else if self.liquid >= 3.0 {
            Quality::Wet
        } else if self.liquid >= 0.2 {
            Quality::Transformed
        } else if self.crust >= 3.0 && self.rained {
            Quality::Icy
        } else if self.crust >= 0.2 {
            Quality::Hard
        } else {
            Quality::OldDry
        }
    }

    fn albedo(&self) -> f64 {
        match self.quality() {
            Quality::Fresh | Quality::Windblown => 0.85,
            Quality::OldDry => 0.75,
            _ => 0.65,
        }
    }
}

/// Liquid water the surface layer holds, mm; the rest drains into the snowpack.
pub const MAX_SURFACE_WATER_MM: f64 = 8.0;

/// Share of the surface water that percolates down each melting hour: the wetness settles
/// where melt and percolation balance (1 mm/h of melt → 2.5 mm: transformed; 2 mm/h → 5
/// mm: wet; 3 mm/h → 7.5 mm: slush).
pub const DRAINAGE_PER_HOUR: f64 = 0.4;

/// Water the surface keeps against percolation (irreducible water content), mm.
pub const RETAINED_WATER_MM: f64 = 0.3;

/// Cold content the surface layer can store, mm of melt (about the top 20 cm of snow at
/// 300 kg/m³ cooled by 5 °C).
pub const MAX_COLD_CONTENT_MM: f64 = 2.0;

/// Water melted or refrozen by one hour at `e` W/m², mm.
fn water_mm(e: f64) -> f64 {
    e * 3600.0 / 334_000.0
}

/// Runs the model for one class over every hour of the point (hours without temperature
/// keep the previous state).
pub fn simulate(point: &WeatherPoint, class: SlopeClass, p: &Params) -> Vec<HourState> {
    simulate_hours(&point.hours, point.position, point.elevation_m, class, p)
}

/// Hours without data longer than this restart the model from the old dry state.
pub const MAX_GAP_HOURS: i64 = 6;

fn simulate_hours(
    hours: &[WeatherHour],
    place: LonLat,
    elevation_m: f64,
    class: SlopeClass,
    p: &Params,
) -> Vec<HourState> {
    let mut s = Surface::default();
    let mut out = Vec::with_capacity(hours.len());
    let mut prev: Option<NaiveDateTime> = None;
    for h in hours {
        if prev.is_some_and(|t| (h.time - t).num_hours() > MAX_GAP_HOURS) {
            s = Surface::default();
        }
        prev = Some(h.time);
        // Sun at the middle of the hour.
        let mid = sun::local_to_utc(h.time + Duration::minutes(30));
        let sp = skimap_core::ephemeris::sun_position(mid, place);
        let mut energy = 0.0;
        if let Some(t) = h.temperature_c {
            step(&mut s, h, t, sp, class, elevation_m, p, &mut energy);
        }
        let q = s.quality();
        out.push(HourState {
            time: h.time,
            quality: q,
            score: q.score(),
            energy_wm2: (energy * 10.0).round() / 10.0,
            liquid_mm: (s.liquid * 100.0).round() / 100.0,
            crust_mm: (s.crust * 100.0).round() / 100.0,
            fresh_cm: (s.fresh * 10.0).round() / 10.0,
            sun_elevation_deg: (sp.elevation_deg * 10.0).round() / 10.0,
        });
    }
    out
}

#[allow(clippy::too_many_arguments)]
fn step(
    s: &mut Surface,
    h: &WeatherHour,
    t: f64,
    sp: sun::SunPosition,
    class: SlopeClass,
    elevation_m: f64,
    p: &Params,
    energy: &mut f64,
) {
    let n = h.cloud_cover_pct.unwrap_or(50.0) / 100.0;
    let wind = h.wind_kmh.unwrap_or(0.0);
    // Precipitation first: snow on top, rain into the surface.
    let snow = h.snowfall_cm.unwrap_or(0.0);
    if snow > 0.0 {
        if snow >= 2.0 && wind < 20.0 {
            s.windblown = false;
        }
        s.fresh += snow;
        if s.fresh >= 2.0 {
            s.rained = false;
        }
    } else if t > 0.5 {
        let rain = h.precipitation_mm.unwrap_or(0.0);
        if rain >= 0.5 {
            s.rained = true;
        }
        s.liquid = (s.liquid + rain).min(MAX_SURFACE_WATER_MM);
        s.fresh = (s.fresh - rain).max(0.0);
    }
    let g = sun::global_on_slope(
        sp,
        class.slope_deg,
        class.aspect_deg.unwrap_or(0.0),
        elevation_m,
        n,
    );
    let hcoef = (p.h_base + p.h_per_kmh * wind).min(p.h_max);
    let e = (1.0 - s.albedo()) * g + hcoef * t - (p.lw_overcast + p.lw_clear_extra * (1.0 - n));
    *energy = e;
    let mut w = water_mm(e.abs());
    if e > 0.0 {
        // The surface first warms to 0 °C (cold content), then melts: the melt wets the
        // fresh snow first (1 mm of water ≈ 1 cm of fresh snow), eats the crust, adds
        // liquid water. Water beyond what the surface layer holds drains away.
        let warm = s.cold.min(w);
        s.cold -= warm;
        w -= warm;
        s.fresh = (s.fresh - w).max(0.0);
        s.crust = (s.crust - w).max(0.0);
        s.liquid = (s.liquid + w).min(MAX_SURFACE_WATER_MM);
        // Percolation: part of the water above the retained film leaves every hour.
        let free = (s.liquid - RETAINED_WATER_MM).max(0.0);
        s.liquid -= free * DRAINAGE_PER_HOUR;
    } else {
        // Refreeze the liquid water, then cool the surface layer.
        let frozen = s.liquid.min(w);
        s.liquid -= frozen;
        s.crust = (s.crust + frozen).min(20.0);
        s.cold = (s.cold + w - frozen).min(MAX_COLD_CONTENT_MM);
    }
    s.fresh *= p.settle_per_hour;
    if s.fresh < 0.05 {
        s.fresh = 0.0;
        s.windblown = false;
    }
    let gust = h.gust_kmh.unwrap_or(0.0);
    if s.fresh >= 2.0 && s.liquid < 0.5 && (wind >= p.wind_drift_kmh || gust >= p.gust_drift_kmh) {
        s.windblown = true;
    }
}

/// A slot of consecutive hours.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Slot {
    /// First hour (local).
    pub start: NaiveDateTime,
    /// End (local, excluded).
    pub end: NaiveDateTime,
    /// Quality at the start of the slot.
    pub quality: Quality,
    /// Score.
    pub score: f64,
}

impl Slot {
    /// Number of hours.
    pub fn hours(&self) -> i64 {
        (self.end - self.start).num_hours()
    }

    /// « de 10 h à 13 h ».
    pub fn label_fr(&self) -> String {
        format!("de {} h à {} h", self.start.hour(), self.end.hour())
    }
}

/// One class over one day.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ClassDay {
    /// Class code.
    pub code: String,
    /// Class.
    pub class: SlopeClass,
    /// Hours from 06 h to 20 h.
    pub hours: Vec<HourState>,
    /// Best slot in daylight.
    pub best: Option<Slot>,
    /// Mean score in daylight.
    pub mean_score: f64,
    /// First daylight hour where the surface is transformed or wetter.
    pub softens_at: Option<NaiveDateTime>,
    /// First daylight hour where it is wet or slush.
    pub wet_at: Option<NaiveDateTime>,
    /// Quality at 07 h (end of the night).
    pub morning: Quality,
}

/// Snow quality of a day at a forecast point.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct DayQuality {
    /// Date.
    pub date: NaiveDate,
    /// Forecast point.
    pub point: String,
    /// Elevation of the weather of the point, m.
    pub elevation_m: f64,
    /// Daylight window used (local).
    pub daylight: (NaiveDateTime, NaiveDateTime),
    /// Every class.
    pub classes: Vec<ClassDay>,
    /// Best class and its slot.
    pub best: Option<(String, Slot)>,
    /// Dominant quality on flat ground from 10 h to 15 h.
    pub flat: Quality,
    /// Estimated snow depth at the point (snow map plus snowfall since), cm.
    pub snow_cm: Option<f64>,
    /// Whether there is enough snow for the quality to mean anything (unknown depth: yes).
    pub enough_snow: bool,
    /// Summary, French.
    pub summary: Vec<String>,
}

/// Below this estimated depth at the point the surface is not evaluated, cm.
pub const MIN_SNOW_CM: f64 = 5.0;

/// Minimum history before 06 h of the day, hours.
pub const SPIN_UP_HOURS: i64 = 24;

/// Snow quality of `date` at a point, or `None` when the weather does not cover the day
/// (06 h to 20 h) or lacks the 24 hours of history.
pub fn day_quality(
    point: &WeatherPoint,
    date: NaiveDate,
    p: &Params,
    snow_cm: Option<f64>,
) -> Option<DayQuality> {
    let six = date.and_hms_opt(6, 0, 0)?;
    let end = date.and_hms_opt(20, 0, 0)?;
    if !point.covers(date, 6, 20) {
        return None;
    }
    // Start of the series without gap that contains the day.
    let i6 = point.hours.partition_point(|h| h.time < six);
    let mut i0 = i6;
    while i0 > 0 && (point.hours[i0].time - point.hours[i0 - 1].time).num_hours() <= MAX_GAP_HOURS {
        i0 -= 1;
    }
    if (six - point.hours[i0].time).num_hours() < SPIN_UP_HOURS {
        return None;
    }
    let i1 = point.hours.partition_point(|h| h.time < end);
    let series = &point.hours[i0..i1];
    let (rise, set) = skimap_core::ephemeris::sun_times(date, point.position)?;
    let local = skimap_core::ephemeris::local_time;
    let day_start = (local(rise) + Duration::minutes(30)).max(date.and_hms_opt(8, 0, 0)?);
    let day_end = (local(set) - Duration::minutes(30)).min(date.and_hms_opt(17, 0, 0)?);
    let mut days = Vec::new();
    for class in classes() {
        let all = simulate_hours(series, point.position, point.elevation_m, class, p);
        let hours: Vec<HourState> = all.into_iter().filter(|h| h.time >= six).collect();
        let daylight: Vec<&HourState> = hours
            .iter()
            .filter(|h| h.time + Duration::minutes(30) >= day_start && h.time < day_end)
            .collect();
        let best = best_slot(&daylight);
        let mean_score = if daylight.is_empty() {
            0.0
        } else {
            daylight.iter().map(|h| h.score).sum::<f64>() / daylight.len() as f64
        };
        let morning = hours
            .iter()
            .find(|h| h.time.hour() == 7)
            .map_or(Quality::OldDry, |h| h.quality);
        days.push(ClassDay {
            code: class.code(),
            class,
            softens_at: daylight.iter().find(|h| h.quality.is_wet()).map(|h| h.time),
            wet_at: daylight
                .iter()
                .find(|h| matches!(h.quality, Quality::Wet | Quality::Slush))
                .map(|h| h.time),
            hours,
            best,
            mean_score,
            morning,
        });
    }
    let best = days
        .iter()
        .filter_map(|d| d.best.as_ref().map(|s| (d, s)))
        .max_by(|(da, a), (db, b)| {
            a.score
                .total_cmp(&b.score)
                .then(a.hours().cmp(&b.hours()))
                .then(da.mean_score.total_cmp(&db.mean_score))
                // Prefer the gentler slope on a tie (listed first).
                .then(db.class.slope_deg.total_cmp(&da.class.slope_deg))
        })
        .map(|(d, s)| (d.code.clone(), s.clone()));
    let flat = dominant(
        days[0]
            .hours
            .iter()
            .filter(|h| (10..15).contains(&h.time.hour())),
    );
    let mut q = DayQuality {
        date,
        point: point.id.clone(),
        elevation_m: point.elevation_m,
        daylight: (day_start, day_end),
        classes: days,
        best,
        flat,
        snow_cm,
        enough_snow: snow_cm.is_none_or(|c| c >= MIN_SNOW_CM),
        summary: Vec::new(),
    };
    if !q.enough_snow {
        q.best = None;
    }
    q.summary = summarise(&q);
    Some(q)
}

fn dominant<'a>(hours: impl Iterator<Item = &'a HourState>) -> Quality {
    let mut counts: Vec<(Quality, usize)> = Vec::new();
    for h in hours {
        match counts.iter_mut().find(|(q, _)| *q == h.quality) {
            Some((_, n)) => *n += 1,
            None => counts.push((h.quality, 1)),
        }
    }
    counts
        .into_iter()
        .max_by_key(|(_, n)| *n)
        .map_or(Quality::OldDry, |(q, _)| q)
}

fn best_slot(hours: &[&HourState]) -> Option<Slot> {
    let top = hours.iter().map(|h| h.score).reduce(f64::max)?;
    let mut best: Option<(usize, usize)> = None;
    let mut i = 0;
    while i < hours.len() {
        if hours[i].score == top {
            let mut j = i;
            while j + 1 < hours.len() && hours[j + 1].score == top {
                j += 1;
            }
            if best.is_none_or(|(a, b)| j - i > b - a) {
                best = Some((i, j));
            }
            i = j + 1;
        } else {
            i += 1;
        }
    }
    let (a, b) = best?;
    Some(Slot {
        start: hours[a].time,
        end: hours[b].time + Duration::hours(1),
        quality: hours[a].quality,
        score: top,
    })
}

fn hour_fr(t: NaiveDateTime) -> String {
    format!("{} h", t.hour())
}

fn summarise(q: &DayQuality) -> Vec<String> {
    if !q.enough_snow {
        return vec![format!(
            "Pas ou trop peu de neige estimée au point ({} cm, {} m) : qualité de neige non \
             évaluée.",
            q.snow_cm.unwrap_or(0.0).round(),
            q.elevation_m.round()
        )];
    }
    let mut out = vec![format!(
        "Au plat ({} m) : {} entre 10 h et 15 h.",
        q.elevation_m.round(),
        q.flat.label_fr()
    )];
    if let Some((code, slot)) = &q.best
        && let Some(c) = q.classes.iter().find(|c| &c.code == code)
    {
        out.push(format!(
            "Meilleur créneau : {} {} ({}).",
            c.class.label_fr(),
            slot.label_fr(),
            slot.quality.label_fr()
        ));
    }
    // 20° slopes: when each aspect softens and turns wet.
    let at20: Vec<&ClassDay> = q
        .classes
        .iter()
        .filter(|c| c.class.slope_deg == 20.0)
        .collect();
    let soft: Vec<String> = at20
        .iter()
        .filter(|c| c.morning != Quality::Transformed && !c.morning.is_wet())
        .filter_map(|c| {
            Some(format!(
                "{} {}",
                c.class.aspect_fr()?,
                hour_fr(c.softens_at?)
            ))
        })
        .collect();
    let never: Vec<&str> = at20
        .iter()
        .filter(|c| c.softens_at.is_none())
        .filter_map(|c| c.class.aspect_fr())
        .collect();
    if !soft.is_empty() {
        out.push(format!(
            "Pentes de 20° : ramollissement vers {}.",
            soft.join(", ")
        ));
    }
    let wet: Vec<String> = q
        .classes
        .iter()
        .filter(|c| c.class.slope_deg > 0.0)
        .filter_map(|c| {
            Some(format!(
                "{} {}° dès {}",
                c.class.aspect_fr()?,
                c.class.slope_deg,
                hour_fr(c.wet_at?)
            ))
        })
        .collect();
    if !wet.is_empty() {
        out.push(format!("Neige lourde : {}.", wet.join(", ")));
    }
    if !never.is_empty() && never.len() < 8 && !soft.is_empty() {
        out.push(format!("Ne ramollit pas (20°) : {}.", never.join(", ")));
    }
    out.push(
        "Modèle simple (température, soleil, nuages, vent) : ni ombre du relief ni damage ; \
         à confirmer sur le terrain."
            .into(),
    );
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A weather point with `days` days of hours from `start`, filled by `f(hour index,
    /// local hour)`.
    fn point(
        lat: f64,
        start: &str,
        days: usize,
        f: impl Fn(usize, u32) -> (f64, f64, f64, f64),
    ) -> WeatherPoint {
        let t0 = NaiveDateTime::parse_from_str(start, "%Y-%m-%dT%H:%M").unwrap();
        let hours = (0..days * 24)
            .map(|i| {
                let time = t0 + Duration::hours(i as i64);
                let (t, cloud, snow, wind) = f(i, time.hour());
                WeatherHour {
                    time,
                    temperature_c: Some(t),
                    apparent_temperature_c: None,
                    wind_chill_c: None,
                    wind_kmh: Some(wind),
                    gust_kmh: Some(wind * 1.5),
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
            .collect();
        WeatherPoint {
            id: "p".into(),
            name: "P".into(),
            position: LonLat::new(6.7269, lat),
            elevation_m: 1750.0,
            hours,
        }
    }

    /// Daily cycle: `tmin` at 05 h, `tmax` at 14 h.
    fn cycle(hour: u32, tmin: f64, tmax: f64) -> f64 {
        let x = ((hour as f64 - 14.0) / 24.0 * std::f64::consts::TAU).cos();
        tmin + (tmax - tmin) * (x + 1.0) / 2.0
    }

    fn class<'a>(q: &'a DayQuality, code: &str) -> &'a ClassDay {
        q.classes.iter().find(|c| c.code == code).unwrap()
    }

    #[test]
    fn spring_clear_day_orders_aspects() {
        // Clear April days, −4 °C at night, +2 °C in the afternoon, at 1750 m.
        let p = point(44.85, "2026-04-12T00:00", 4, |_, h| {
            (cycle(h, -4.0, 2.0), 0.0, 0.0, 5.0)
        });
        let d = NaiveDate::from_ymd_opt(2026, 4, 14).unwrap();
        let q = day_quality(&p, d, &Params::default(), None).unwrap();
        let (e, s, n40) = (class(&q, "E20"), class(&q, "S20"), class(&q, "N40"));
        // Refrozen in the morning, everywhere.
        assert_eq!(s.morning, Quality::Hard, "{:?}", s.hours[2]);
        // East softens before south, south before west.
        let w = class(&q, "W20");
        assert!(e.softens_at.unwrap() <= s.softens_at.unwrap());
        assert!(s.softens_at.unwrap() < w.softens_at.unwrap());
        // South 40° softens two hours before north 40° and turns heavy; north 40° does not.
        let s40 = class(&q, "S40");
        assert!(s40.softens_at.unwrap() + Duration::hours(2) <= n40.softens_at.unwrap());
        assert!(s40.wet_at.is_some());
        assert!(n40.wet_at.is_none(), "{:?}", n40.hours);
        // On flat ground: a transformed day, softening around 10 h.
        assert_eq!(q.flat, Quality::Transformed);
        // A best slot exists with transformed snow.
        let (_, slot) = q.best.clone().unwrap();
        assert_eq!(slot.quality, Quality::Transformed);
        assert!(q.summary.iter().any(|s| s.contains("ramollissement")));
    }

    #[test]
    fn cold_january_stays_dry_on_flat() {
        let p = point(44.85, "2026-01-03T00:00", 4, |_, h| {
            (cycle(h, -12.0, -5.0), 0.0, 0.0, 5.0)
        });
        let d = NaiveDate::from_ymd_opt(2026, 1, 5).unwrap();
        let q = day_quality(&p, d, &Params::default(), None).unwrap();
        assert_eq!(q.flat, Quality::OldDry);
        assert!(class(&q, "flat").softens_at.is_none());
        assert!(class(&q, "N40").softens_at.is_none());
    }

    #[test]
    fn overcast_mild_night_does_not_refreeze() {
        // Overcast and +2 °C day and night: the surface stays wet in the morning.
        let p = point(44.85, "2026-03-01T00:00", 3, |_, _| (2.0, 100.0, 0.0, 5.0));
        let d = NaiveDate::from_ymd_opt(2026, 3, 2).unwrap();
        let q = day_quality(&p, d, &Params::default(), None).unwrap();
        assert!(class(&q, "flat").morning.is_wet());
    }

    #[test]
    fn snowfall_gives_fresh_then_wind_blows_it() {
        // 10 cm overnight on day 2 (calm), then a windy day 3.
        let p = point(46.6, "2026-02-20T00:00", 4, |i, h| {
            let snow = if (30..40).contains(&i) { 1.0 } else { 0.0 };
            let wind = if i >= 54 { 40.0 } else { 5.0 };
            (cycle(h, -8.0, -4.0), 100.0, snow, wind)
        });
        let d2 = NaiveDate::from_ymd_opt(2026, 2, 21).unwrap();
        let q2 = day_quality(&p, d2, &Params::default(), None).unwrap();
        assert_eq!(q2.flat, Quality::Fresh);
        let d3 = NaiveDate::from_ymd_opt(2026, 2, 22).unwrap();
        let q3 = day_quality(&p, d3, &Params::default(), None).unwrap();
        assert_eq!(q3.flat, Quality::Windblown);
        assert_eq!(
            Quality::Windblown.voice(),
            Some(skimap_voice::bank::Snow::Windblown)
        );
    }

    #[test]
    fn no_snow_no_quality_and_gaps_restart() {
        let p = point(44.85, "2026-04-12T00:00", 4, |_, h| {
            (cycle(h, -4.0, 2.0), 0.0, 0.0, 5.0)
        });
        let d = NaiveDate::from_ymd_opt(2026, 4, 14).unwrap();
        let q = day_quality(&p, d, &Params::default(), Some(2.0)).unwrap();
        assert!(!q.enough_snow && q.best.is_none());
        assert!(q.summary[0].starts_with("Pas ou trop peu de neige"));
        // A gap of a year before the day: the model restarts, so the day needs 24 h after it.
        let mut p2 = p.clone();
        for h in p2.hours.iter_mut().skip(24) {
            h.time += Duration::days(365);
        }
        let d2 = NaiveDate::from_ymd_opt(2027, 4, 14).unwrap();
        assert!(day_quality(&p2, d2, &Params::default(), None).is_some());
        let d3 = NaiveDate::from_ymd_opt(2027, 4, 13).unwrap();
        assert!(
            day_quality(&p2, d3, &Params::default(), None).is_none(),
            "only 0 h of history"
        );
    }

    #[test]
    fn needs_history() {
        let p = point(44.85, "2026-04-14T00:00", 2, |_, h| {
            (cycle(h, -3.0, 5.0), 0.0, 0.0, 5.0)
        });
        let d = NaiveDate::from_ymd_opt(2026, 4, 14).unwrap();
        assert!(day_quality(&p, d, &Params::default(), None).is_none());
        assert_eq!(classes().len(), 17);
        assert_eq!(classes()[5].code(), "S20");
        assert_eq!(classes()[9].code(), "N40");
    }
}
