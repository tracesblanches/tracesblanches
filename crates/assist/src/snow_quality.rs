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

use jiff::{
    SignedDuration,
    civil::{Date, DateTime},
};
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
        crate::text::of(crate::text::Lang::Fr).slope_class(self)
    }

    /// Octant of the aspect (0 north, 1 north-east…), `None` for flat.
    pub fn octant(&self) -> Option<usize> {
        self.aspect_deg.map(octant)
    }

    /// French aspect name, `None` for flat.
    pub fn aspect_fr(&self) -> Option<&'static str> {
        self.aspect_in(crate::text::Lang::Fr)
    }

    /// Aspect name in `lang`, `None` for flat.
    pub fn aspect_in(&self, lang: crate::text::Lang) -> Option<&'static str> {
        self.octant().map(|o| crate::text::of(lang).aspect(o))
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
        self.label(crate::text::Lang::Fr)
    }

    /// Label in `lang`.
    pub fn label(self, lang: crate::text::Lang) -> &'static str {
        crate::text::of(lang).quality(self)
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

    /// Phrase of the voice bank (old dry snow: « Neige froide et sèche. »).
    pub fn voice(self) -> Option<skimap_voice::bank::Snow> {
        use skimap_voice::bank::Snow;
        Some(match self {
            Quality::Fresh => Snow::Fresh,
            Quality::Windblown => Snow::Windblown,
            Quality::Transformed => Snow::Transformed,
            Quality::Wet | Quality::Slush => Snow::Wet,
            Quality::Hard => Snow::Hard,
            Quality::Icy => Snow::Icy,
            Quality::OldDry => Snow::ColdDry,
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
    pub time: DateTime,
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
    let mut prev: Option<DateTime> = None;
    for h in hours {
        if prev.is_some_and(|t| h.time.duration_since(t).as_hours() > MAX_GAP_HOURS) {
            s = Surface::default();
        }
        prev = Some(h.time);
        // Sun at the middle of the hour.
        let mid = sun::local_to_utc(h.time + SignedDuration::from_mins(30));
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
    pub start: DateTime,
    /// End (local, excluded).
    pub end: DateTime,
    /// Quality at the start of the slot.
    pub quality: Quality,
    /// Score.
    pub score: f64,
}

impl Slot {
    /// Number of hours.
    pub fn hours(&self) -> i64 {
        self.end.duration_since(self.start).as_hours()
    }

    /// « de 10 h à 13 h ».
    pub fn label_fr(&self) -> String {
        crate::text::of(crate::text::Lang::Fr).slot(self)
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
    pub softens_at: Option<DateTime>,
    /// First daylight hour where it is wet or slush.
    pub wet_at: Option<DateTime>,
    /// Quality at 07 h (end of the night).
    pub morning: Quality,
}

/// Snow quality of a day at a forecast point.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct DayQuality {
    /// Date.
    pub date: Date,
    /// Forecast point.
    pub point: String,
    /// Name of the forecast point, as shown to the skier (empty when the point has none).
    pub point_name: String,
    /// Elevation of the weather of the point, m.
    pub elevation_m: f64,
    /// Daylight window used (local).
    pub daylight: (DateTime, DateTime),
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
    date: Date,
    p: &Params,
    snow_cm: Option<f64>,
) -> Option<DayQuality> {
    let six = date.at(6, 0, 0, 0);
    let end = date.at(20, 0, 0, 0);
    if !point.covers(date, 6, 20) {
        return None;
    }
    // Start of the series without gap that contains the day.
    let i6 = point.hours.partition_point(|h| h.time < six);
    let mut i0 = i6;
    while i0 > 0
        && point.hours[i0]
            .time
            .duration_since(point.hours[i0 - 1].time)
            .as_hours()
            <= MAX_GAP_HOURS
    {
        i0 -= 1;
    }
    if six.duration_since(point.hours[i0].time).as_hours() < SPIN_UP_HOURS {
        return None;
    }
    let i1 = point.hours.partition_point(|h| h.time < end);
    let series = &point.hours[i0..i1];
    let (rise, set) = skimap_core::ephemeris::sun_times(date, point.position)?;
    let local = skimap_core::ephemeris::local_time;
    let day_start = (local(rise) + SignedDuration::from_mins(30)).max(date.at(8, 0, 0, 0));
    let day_end = (local(set) - SignedDuration::from_mins(30)).min(date.at(17, 0, 0, 0));
    let mut days = Vec::new();
    for class in classes() {
        let all = simulate_hours(series, point.position, point.elevation_m, class, p);
        let hours: Vec<HourState> = all.into_iter().filter(|h| h.time >= six).collect();
        let daylight: Vec<&HourState> = hours
            .iter()
            .filter(|h| h.time + SignedDuration::from_mins(30) >= day_start && h.time < day_end)
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
        point_name: point.name.clone(),
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
        end: hours[b].time + SignedDuration::from_hours(1),
        quality: hours[a].quality,
        score: top,
    })
}

fn summarise(q: &DayQuality) -> Vec<String> {
    summary(q, crate::text::Lang::Fr)
}

/// The summary of a day at a point (sentences), in `lang`.
pub fn summary(q: &DayQuality, lang: crate::text::Lang) -> Vec<String> {
    let t = crate::text::of(lang);
    if !q.enough_snow {
        return vec![t.sq_no_snow(q.snow_cm.unwrap_or(0.0).round(), q.elevation_m.round())];
    }
    let mut out = vec![t.sq_flat(q.elevation_m.round(), q.flat)];
    if let Some((code, slot)) = &q.best
        && let Some(c) = q.classes.iter().find(|c| &c.code == code)
    {
        out.push(t.sq_best(&t.slope_class(&c.class), &t.slot(slot), slot.quality));
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
        .filter_map(|c| Some(t.sq_aspect_at(c.class.aspect_in(lang)?, c.softens_at?)))
        .collect();
    let never: Vec<&str> = at20
        .iter()
        .filter(|c| c.softens_at.is_none())
        .filter_map(|c| c.class.aspect_in(lang))
        .collect();
    if !soft.is_empty() {
        out.push(t.sq_softens(&soft));
    }
    let wet: Vec<String> = q
        .classes
        .iter()
        .filter(|c| c.class.slope_deg > 0.0)
        .filter_map(|c| {
            Some(t.sq_aspect_slope_from(c.class.aspect_in(lang)?, c.class.slope_deg, c.wet_at?))
        })
        .collect();
    if !wet.is_empty() {
        out.push(t.sq_heavy(&wet));
    }
    if !never.is_empty() && never.len() < 8 && !soft.is_empty() {
        out.push(t.sq_never_softens(&never));
    }
    out.push(t.sq_model().into());
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use jiff::civil::date;

    /// A weather point with `days` days of hours from `start`, filled by `f(hour index,
    /// local hour)`.
    fn point(
        lat: f64,
        start: &str,
        days: usize,
        f: impl Fn(usize, u32) -> (f64, f64, f64, f64),
    ) -> WeatherPoint {
        let t0 = DateTime::strptime("%Y-%m-%dT%H:%M", start).unwrap();
        let hours = (0..days * 24)
            .map(|i| {
                let time = t0 + SignedDuration::from_hours(i as i64);
                let (t, cloud, snow, wind) = f(i, time.hour() as u32);
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
        let d = date(2026, 4, 14);
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
        assert!(s40.softens_at.unwrap() + SignedDuration::from_hours(2) <= n40.softens_at.unwrap());
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
        let d = date(2026, 1, 5);
        let q = day_quality(&p, d, &Params::default(), None).unwrap();
        assert_eq!(q.flat, Quality::OldDry);
        assert!(class(&q, "flat").softens_at.is_none());
        assert!(class(&q, "N40").softens_at.is_none());
    }

    #[test]
    fn overcast_mild_night_does_not_refreeze() {
        // Overcast and +2 °C day and night: the surface stays wet in the morning.
        let p = point(44.85, "2026-03-01T00:00", 3, |_, _| (2.0, 100.0, 0.0, 5.0));
        let d = date(2026, 3, 2);
        let q = day_quality(&p, d, &Params::default(), None).unwrap();
        assert!(class(&q, "flat").morning.is_wet());
        // Wet since the morning everywhere: nothing « softens » during the day.
        for c in q.classes.iter().filter(|c| c.class.slope_deg == 20.0) {
            assert!(c.morning.is_wet(), "{}", c.code);
        }
        assert!(
            !q.summary.iter().any(|s| s.contains("ramollissement")),
            "{:?}",
            q.summary
        );
    }

    #[test]
    fn snowfall_gives_fresh_then_wind_blows_it() {
        // 10 cm overnight on day 2 (calm), then a windy day 3.
        let p = point(46.6, "2026-02-20T00:00", 4, |i, h| {
            let snow = if (30..40).contains(&i) { 1.0 } else { 0.0 };
            let wind = if i >= 54 { 40.0 } else { 5.0 };
            (cycle(h, -8.0, -4.0), 100.0, snow, wind)
        });
        let d2 = date(2026, 2, 21);
        let q2 = day_quality(&p, d2, &Params::default(), None).unwrap();
        assert_eq!(q2.flat, Quality::Fresh);
        let d3 = date(2026, 2, 22);
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
        let d = date(2026, 4, 14);
        let q = day_quality(&p, d, &Params::default(), Some(2.0)).unwrap();
        assert!(!q.enough_snow && q.best.is_none());
        assert!(q.summary[0].starts_with("Pas ou trop peu de neige"));
        // A gap of a year before the day: the model restarts, so the day needs 24 h after it.
        let mut p2 = p.clone();
        for h in p2.hours.iter_mut().skip(24) {
            h.time += SignedDuration::from_hours(24 * 365);
        }
        let d2 = date(2027, 4, 14);
        assert!(day_quality(&p2, d2, &Params::default(), None).is_some());
        let d3 = date(2027, 4, 13);
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
        let d = date(2026, 4, 14);
        assert!(day_quality(&p, d, &Params::default(), None).is_none());
        assert_eq!(classes().len(), 17);
        assert_eq!(classes()[5].code(), "S20");
        assert_eq!(classes()[9].code(), "N40");
    }

    fn surface(fresh: f64, liquid: f64, crust: f64, rained: bool) -> Surface {
        Surface {
            liquid,
            crust,
            fresh,
            windblown: false,
            rained,
            cold: 0.0,
        }
    }

    #[test]
    fn surface_classes_at_their_bounds() {
        assert_eq!(surface(2.0, 0.49, 0.0, false).quality(), Quality::Fresh);
        assert_eq!(
            surface(2.0, 0.5, 0.0, false).quality(),
            Quality::Transformed
        );
        assert_eq!(surface(1.99, 0.0, 0.0, false).quality(), Quality::OldDry);
        let mut w = surface(2.0, 0.0, 0.0, false);
        w.windblown = true;
        assert_eq!(w.quality(), Quality::Windblown);
        assert_eq!(surface(0.0, 6.0, 0.0, false).quality(), Quality::Slush);
        assert_eq!(surface(0.0, 5.99, 0.0, false).quality(), Quality::Wet);
        assert_eq!(surface(0.0, 3.0, 0.0, false).quality(), Quality::Wet);
        assert_eq!(
            surface(0.0, 2.99, 0.0, false).quality(),
            Quality::Transformed
        );
        assert_eq!(
            surface(0.0, 0.2, 0.0, false).quality(),
            Quality::Transformed
        );
        assert_eq!(surface(0.0, 0.19, 3.0, true).quality(), Quality::Icy);
        assert_eq!(surface(0.0, 0.0, 2.99, true).quality(), Quality::Hard);
        assert_eq!(surface(0.0, 0.0, 3.0, false).quality(), Quality::Hard);
        assert_eq!(surface(0.0, 0.0, 0.2, false).quality(), Quality::Hard);
        assert_eq!(surface(0.0, 0.0, 0.19, false).quality(), Quality::OldDry);
        assert_eq!(surface(5.0, 0.0, 0.0, false).albedo(), 0.85);
        assert_eq!(surface(0.0, 0.0, 0.0, false).albedo(), 0.75);
        assert_eq!(surface(0.0, 0.0, 1.0, false).albedo(), 0.65);
        assert!((water_mm(334_000.0 / 3600.0) - 1.0).abs() < 1e-12);
    }

    /// One hour of `step` at night (no sun) on the flat.
    fn night(s: &mut Surface, t: f64, wind: f64, snow: f64, precip: f64) -> f64 {
        let h = WeatherHour {
            time: date(2027, 1, 12).at(2, 0, 0, 0),
            temperature_c: Some(t),
            apparent_temperature_c: None,
            wind_chill_c: None,
            wind_kmh: Some(wind),
            gust_kmh: Some(wind),
            visibility_m: None,
            fog: None,
            freezing_level_m: None,
            snowfall_cm: Some(snow),
            precipitation_mm: Some(precip),
            cloud_cover_pct: Some(0.0),
            relative_humidity_pct: None,
            weather_code: None,
        };
        let sp = sun::SunPosition {
            elevation_deg: -30.0,
            azimuth_deg: 0.0,
        };
        let mut e = 0.0;
        step(
            s,
            &h,
            t,
            sp,
            classes()[0],
            1750.0,
            &Params::default(),
            &mut e,
        );
        e
    }

    #[test]
    fn one_hour_of_the_model() {
        // Clear night at −10 °C, 5 km/h: E = (12 + 2.5) × −10 − (15 + 65) = −225 W/m², all
        // of it cooling the dry surface (2 mm of cold content at most).
        let mut s = surface(0.0, 0.0, 0.0, false);
        let e = night(&mut s, -10.0, 5.0, 0.0, 0.0);
        assert!((e + 225.0).abs() < 1e-9, "{e}");
        assert!((s.cold - MAX_COLD_CONTENT_MM).abs() < 1e-12);
        // Wet snow refreezes first: 225 W/m² for an hour is 2.43 mm.
        let mut s = surface(0.0, 3.0, 0.0, true);
        night(&mut s, -10.0, 5.0, 0.0, 0.0);
        assert!(
            (s.crust - water_mm(225.0)).abs() < 1e-9
                && (s.liquid - (3.0 - water_mm(225.0))).abs() < 1e-9
        );
        // Snow of 2 cm or more in a calm hour: no longer wind-blown; rained no more.
        let mut s = surface(3.0, 0.0, 0.0, true);
        s.windblown = true;
        night(&mut s, -10.0, 19.0, 2.0, 2.0);
        assert!(!s.windblown && !s.rained);
        assert!((s.fresh - 5.0 * Params::default().settle_per_hour).abs() < 1e-9);
        // In wind of 20 km/h, or less than 2 cm: still wind-blown.
        let mut s = surface(3.0, 0.0, 0.0, false);
        s.windblown = true;
        night(&mut s, -10.0, 20.0, 2.0, 2.0);
        assert!(s.windblown);
        let mut s = surface(3.0, 0.0, 0.0, false);
        s.windblown = true;
        night(&mut s, -10.0, 10.0, 1.9, 1.9);
        assert!(s.windblown);
        // A little snow on a rained crust under 2 cm in all: still « rained ».
        let mut s = surface(0.0, 0.0, 3.0, true);
        night(&mut s, -10.0, 5.0, 1.9, 1.9);
        assert!(s.rained);
        // Rain above +0.5 °C: water in, fresh snow out, « rained » from 0.5 mm.
        let mut s = surface(3.0, 0.0, 0.0, false);
        night(&mut s, 0.6, 5.0, 0.0, 0.5);
        assert!(s.rained);
        assert!(
            (s.fresh - 2.5 * Params::default().settle_per_hour).abs() < 1e-9,
            "{}",
            s.fresh
        );
        let mut s = surface(0.0, 0.0, 0.0, false);
        night(&mut s, 0.6, 5.0, 0.0, 0.4);
        assert!(!s.rained);
        let mut s = surface(0.0, 0.0, 0.0, false);
        night(&mut s, 0.5, 5.0, 0.0, 2.0);
        assert!(!s.rained && s.liquid == 0.0, "no rain at +0.5 °C");
        // A melting hour: the cold content first, then the fresh snow, then water.
        let mut s = surface(3.0, 0.0, 0.0, false);
        s.cold = 0.5;
        let e = night(&mut s, 20.0, 5.0, 0.0, 0.0);
        let w = water_mm(e);
        assert!(w > 0.5);
        assert_eq!(s.cold, 0.0);
        let fresh = (3.0 - (w - 0.5)).max(0.0) * Params::default().settle_per_hour;
        assert!((s.fresh - fresh).abs() < 1e-9, "{} {fresh}", s.fresh);
    }

    #[test]
    fn hours_reported_and_gaps() {
        let p = point(44.85, "2027-01-10T00:00", 2, |_, h| {
            (cycle(h, -10.0, -2.0), 0.0, 0.0, 5.0)
        });
        let all = simulate(&p, classes()[0], &Params::default());
        assert_eq!(all.len(), 48);
        for (h, w) in all.iter().zip(&p.hours) {
            assert_eq!(h.time, w.time);
            let mid = sun::local_to_utc(w.time + SignedDuration::from_mins(30));
            let sp = skimap_core::ephemeris::sun_position(mid, p.position);
            assert_eq!(
                h.sun_elevation_deg,
                (sp.elevation_deg * 10.0).round() / 10.0
            );
            assert!(h.liquid_mm <= MAX_SURFACE_WATER_MM && h.crust_mm <= 20.0);
            assert_eq!(h.liquid_mm, (h.liquid_mm * 100.0).round() / 100.0);
            assert_eq!(h.energy_wm2, (h.energy_wm2 * 10.0).round() / 10.0);
        }
        // At 02 h (no sun), −9.x °C, 5 km/h, clear.
        let t = p.hours[2].temperature_c.unwrap();
        let e = 14.5 * t - 80.0;
        assert_eq!(all[2].energy_wm2, (e * 10.0).round() / 10.0);
        // A warm afternoon then a frozen night leaves a crust, reported to 0.01 mm.
        let p = point(44.85, "2027-03-18T00:00", 2, |_, h| {
            (cycle(h, -8.0, 9.0), 0.0, 0.0, 5.0)
        });
        let all = simulate(&p, classes()[5], &Params::default());
        let h = all.iter().find(|h| h.crust_mm > 0.0).expect("a crust");
        assert_eq!(h.crust_mm, (h.crust_mm * 100.0).round() / 100.0);
        assert!(all.iter().any(|h| h.liquid_mm > 0.0 && h.liquid_mm < 1.0));
        // A gap of 6 hours keeps the state; of 7 hours, starts again from old dry snow.
        let mut gap = p.clone();
        let cut = gap.hours.iter().position(|h| h.time.hour() == 20).unwrap();
        let after: Vec<WeatherHour> = gap.hours[cut + 6..].to_vec();
        gap.hours.truncate(cut + 1);
        gap.hours.extend(after.iter().cloned());
        let s20 = classes()[5];
        let kept = simulate(&gap, s20, &Params::default());
        assert_eq!(kept[cut + 1].crust_mm > 0.0, all[cut + 6].crust_mm > 0.0);
        assert!(kept[cut + 1].crust_mm > 0.0, "{:?}", kept[cut + 1]);
        let mut gap7 = p.clone();
        gap7.hours.truncate(cut + 1);
        gap7.hours.extend(after.iter().skip(1).cloned());
        let reset = simulate(&gap7, s20, &Params::default());
        // Restarted: refreezes from nothing, no crust in the first hour after the gap.
        assert_eq!(reset[cut + 1].crust_mm, 0.0, "{:?}", reset[cut + 1]);
    }

    #[test]
    fn refreeze_cold_content_fresh_snow_and_drift() {
        // 0.5 mm of water refreezes, the rest of the 225 W/m² hour cools the layer.
        let mut s = surface(0.0, 0.5, 0.0, false);
        night(&mut s, -10.0, 5.0, 0.0, 0.0);
        assert!((s.crust - 0.5).abs() < 1e-12);
        assert!(
            (s.cold - (water_mm(225.0) - 0.5)).abs() < 1e-12,
            "{}",
            s.cold
        );
        // A trace of fresh snow under 0.05 cm after settling disappears, with its drift.
        let mut s = surface(0.05, 0.0, 0.0, false);
        s.windblown = true;
        night(&mut s, -10.0, 5.0, 0.0, 0.0);
        assert_eq!(s.fresh, 0.0);
        assert!(!s.windblown);
        let mut s = surface(0.1, 0.0, 0.0, false);
        night(&mut s, -10.0, 5.0, 0.0, 0.0);
        assert!((s.fresh - 0.1 * Params::default().settle_per_hour).abs() < 1e-12);
        // Wind of 30 km/h (gusts 30) on 2 cm of dry fresh snow: wind-blown; gusts of 55
        // alone too; not with 0.5 mm of water.
        let drift = |fresh: f64, liquid: f64, wind: f64, gust: f64| {
            let mut s = surface(fresh, liquid, 0.0, false);
            let h = WeatherHour {
                time: date(2027, 1, 12).at(2, 0, 0, 0),
                temperature_c: Some(-10.0),
                apparent_temperature_c: None,
                wind_chill_c: None,
                wind_kmh: Some(wind),
                gust_kmh: Some(gust),
                visibility_m: None,
                fog: None,
                freezing_level_m: None,
                snowfall_cm: Some(0.0),
                precipitation_mm: Some(0.0),
                cloud_cover_pct: Some(100.0),
                relative_humidity_pct: None,
                weather_code: None,
            };
            let sp = sun::SunPosition {
                elevation_deg: -30.0,
                azimuth_deg: 0.0,
            };
            let mut e = 0.0;
            step(
                &mut s,
                &h,
                -10.0,
                sp,
                classes()[0],
                1750.0,
                &Params::default(),
                &mut e,
            );
            s.windblown
        };
        assert!(drift(3.0, 0.0, 30.0, 30.0));
        assert!(!drift(3.0, 0.0, 29.0, 54.0));
        assert!(drift(3.0, 0.0, 10.0, 55.0));
        assert!(!drift(1.9, 0.0, 40.0, 60.0), "under 2 cm after settling");
    }

    #[test]
    fn wet_fresh_snow_does_not_drift() {
        // At 0 °C under clouds the hour refreezes 0.16 mm: 0.7 mm leaves 0.54 mm of water,
        // too wet to drift; 0.3 mm leaves none.
        let drift = |liquid: f64| {
            let mut s = surface(3.0, liquid, 0.0, false);
            let h = WeatherHour {
                time: date(2027, 1, 12).at(2, 0, 0, 0),
                temperature_c: Some(0.0),
                apparent_temperature_c: None,
                wind_chill_c: None,
                wind_kmh: Some(30.0),
                gust_kmh: Some(30.0),
                visibility_m: None,
                fog: None,
                freezing_level_m: None,
                snowfall_cm: Some(0.0),
                precipitation_mm: Some(0.0),
                cloud_cover_pct: Some(100.0),
                relative_humidity_pct: None,
                weather_code: None,
            };
            let sp = sun::SunPosition {
                elevation_deg: -30.0,
                azimuth_deg: 0.0,
            };
            let mut e = 0.0;
            step(
                &mut s,
                &h,
                0.0,
                sp,
                classes()[0],
                1750.0,
                &Params::default(),
                &mut e,
            );
            assert!((e + 15.0).abs() < 1e-9, "{e}");
            (s.windblown, s.liquid)
        };
        let (w, l) = drift(0.7);
        assert!(!w && (l - (0.7 - water_mm(15.0))).abs() < 1e-12, "{l}");
        assert!(drift(0.3).0);
    }

    #[test]
    fn longest_run_of_the_best_score() {
        let t0 = date(2027, 1, 12).at(8, 0, 0, 0);
        let hs: Vec<HourState> = [3.0, 3.0, 1.0, 3.0, 3.0, 3.0, 2.5]
            .iter()
            .enumerate()
            .map(|(i, sc)| HourState {
                time: t0 + SignedDuration::from_hours(i as i64),
                quality: if *sc == 3.0 {
                    Quality::Fresh
                } else {
                    Quality::OldDry
                },
                score: *sc,
                energy_wm2: 0.0,
                liquid_mm: 0.0,
                crust_mm: 0.0,
                fresh_cm: 0.0,
                sun_elevation_deg: 0.0,
            })
            .collect();
        let refs: Vec<&HourState> = hs.iter().collect();
        let s = best_slot(&refs).unwrap();
        // The second run (11 h to 14 h) is longer than the first (8 h to 10 h).
        assert_eq!((s.start.hour(), s.end.hour(), s.hours()), (11, 14, 3));
        assert_eq!(s.label_fr(), "de 11 h à 14 h");
        // Two runs of the same length: the first.
        let s = best_slot(&refs[..5]).unwrap();
        assert_eq!((s.start.hour(), s.hours()), (8, 2));
        assert_eq!(best_slot(&[]), None);
    }

    #[test]
    fn the_morning_is_seven_and_the_history_24_hours() {
        // 3 cm fall in the hour of 07 h only: old snow at 06 h, fresh at 07 h.
        let p = point(44.85, "2027-01-10T00:00", 3, |i, h| {
            let snow = if i >= 48 && h == 7 && i < 72 {
                3.0
            } else {
                0.0
            };
            (cycle(h, -10.0, -4.0), 100.0, snow, 5.0)
        });
        let d = date(2027, 1, 12);
        let q = day_quality(&p, d, &Params::default(), None).unwrap();
        let flat = class(&q, "flat");
        assert_eq!(flat.hours[0].quality, Quality::OldDry);
        assert_eq!(flat.morning, Quality::Fresh);
        // The daylight ends 30 min before the sunset (17 h at the latest).
        let (_, set) = skimap_core::ephemeris::sun_times(d, p.position).unwrap();
        let end = (skimap_core::ephemeris::local_time(set) - SignedDuration::from_mins(30))
            .min(d.at(17, 0, 0, 0));
        assert_eq!(q.daylight.1, end);
        // The mean score over the daylight hours.
        let n = flat
            .hours
            .iter()
            .filter(|h| {
                h.time + SignedDuration::from_mins(30) >= q.daylight.0 && h.time < q.daylight.1
            })
            .count();
        let sum: f64 = flat
            .hours
            .iter()
            .filter(|h| {
                h.time + SignedDuration::from_mins(30) >= q.daylight.0 && h.time < q.daylight.1
            })
            .map(|h| h.score)
            .sum();
        assert!((flat.mean_score - sum / n as f64).abs() < 1e-12);
        // Exactly 24 h of history before 06 h is enough, 23 h is not.
        let p = point(44.85, "2027-01-11T06:00", 2, |_, h| {
            (cycle(h, -10.0, -4.0), 100.0, 0.0, 5.0)
        });
        assert!(day_quality(&p, d, &Params::default(), None).is_some());
        let p = point(44.85, "2027-01-11T07:00", 2, |_, h| {
            (cycle(h, -10.0, -4.0), 100.0, 0.0, 5.0)
        });
        assert!(day_quality(&p, d, &Params::default(), None).is_none());
    }
}
