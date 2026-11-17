//! Texts of the assistant in the language of the app (VO6, #129): one module per language
//! ([`fr`], [`en`]), each implementing every text of [`Texts`]; [`of`] gives the texts of
//! a language. A text missing in one language does not compile.
//!
//! Short, concrete sentences; never « sans danger » / « safe ».

use chrono::{NaiveDate, NaiveDateTime};
pub use skimap_common::Lang;

use crate::crowd::{CrowdLevel, Population};
use crate::snow_quality::{Quality, SlopeClass, Slot};

pub mod en;
pub mod fr;

/// The texts of [`Lang::Fr`] or [`Lang::En`].
pub fn of(lang: Lang) -> &'static dyn Texts {
    match lang {
        Lang::Fr => &fr::Fr,
        Lang::En => &en::En,
    }
}

/// Every text of the assistant, in one language.
pub trait Texts: Sync {
    // ---- Numbers and dates.

    /// A number with `decimals` decimals: `1,5` in French, `1.5` in English.
    fn num(&self, x: f64, decimals: usize) -> String;
    /// « mardi 5 janvier 2026 », « Tuesday 5 January 2026 ».
    fn date(&self, d: NaiveDate) -> String;
    /// Name of a weekday, Monday = 0 (« dimanche », « Sunday »).
    fn weekday(&self, monday0: usize) -> &'static str;
    /// A clock time to the minute: « 9 h 05 », « 9:05 ».
    fn clock(&self, t: NaiveDateTime) -> String;
    /// A whole hour: « 13 h », « 13:00 ».
    fn hour(&self, t: NaiveDateTime) -> String;

    // ---- Briefing (A1): reasons of the scores.

    /// Gusts of 40 km/h or more.
    fn gusts_up_to(&self, kmh: f64) -> String;
    /// Light wind, gusts below 25 km/h.
    fn wind_light(&self, gust_kmh: f64) -> String;
    /// Moderate wind, gusts from 25 to 40 km/h.
    fn wind_moderate(&self, gust_kmh: f64) -> String;
    /// Wind chill at or below −20 °C.
    fn very_cold(&self, chill: &str) -> String;
    /// Wind chill at or below −12 °C.
    fn cold(&self, chill: &str) -> String;
    /// Mean temperature and wind chill.
    fn temperature(&self, mean: &str, chill: &str) -> String;
    /// Hours of fog between 09 h and 16 h.
    fn fog_hours(&self, hours: usize) -> String;
    /// Snowfall during the day.
    fn day_snowfall(&self, cm: f64) -> String;
    /// Rain during the day.
    fn rain(&self, mm: f64) -> String;
    /// Cloud cover below 30 %.
    fn sunny(&self) -> &'static str;
    /// Estimated snow along the trails: median, date of the map, snowfall since, share of
    /// the points with 20 cm or more.
    fn snow_estimate(&self, depth: f64, map: NaiveDate, added: f64, share_pct: f64) -> String;
    /// Verdict of the estimated snow: 0 not enough to groom, 1 borderline, 2 enough.
    fn snow_verdict(&self, base: &str, verdict: u8) -> String;
    /// No estimated snow here.
    fn snow_unknown(&self) -> &'static str;
    /// Fresh snow in 24 h.
    fn fresh_snow_24h(&self, cm: f64) -> String;
    /// The snow surface.
    fn surface(&self, q: Quality) -> String;
    /// No snow map of the day or of the 4 days before.
    fn no_snow_map(&self) -> &'static str;
    /// The weather is the one of a point further than 5 km.
    fn weather_from(&self, point: &str, km: f64) -> String;
    /// No weather forecast for the day.
    fn no_forecast(&self) -> &'static str;
    /// The crowd level with its reasons.
    fn crowd(&self, level: CrowdLevel, reasons: &[String]) -> String;
    /// Distance from home, straight line.
    fn as_the_crow_flies(&self, km: f64) -> String;
    /// Km in known avalanche paths: `level` the BERA (`None`: not available); `bad`: level
    /// 3 or more.
    fn avalanche_km(&self, km: f64, level: Option<u8>) -> String;
    /// Calendars known for 2026-27 only.
    fn calendars_2026_27(&self) -> &'static str;
    /// Status of the trails unknown for the day.
    fn status_unknown_day(&self) -> &'static str;
    /// Status of the trails: the last one known, at `when` (« 05/01 08:10 »).
    fn status_last_known(&self, when: &str) -> String;
    /// Sources disagree.
    fn sources_disagree(&self, what: &str) -> String;
    /// Open according to the source, but too little snow.
    fn open_without_snow(&self) -> &'static str;
    /// Open.
    fn open(&self) -> &'static str;
    /// Closed.
    fn closed(&self) -> &'static str;
    /// Unknown status.
    fn status_unknown(&self) -> &'static str;
    /// Suffix: the last known status.
    fn last_known_suffix(&self) -> &'static str;
    /// No status source for this area.
    fn no_status_source(&self) -> &'static str;
    /// The one known itinerary is closed.
    fn only_itinerary_closed(&self, suffix: &str) -> String;
    /// None of the `n` known itineraries is open.
    fn none_open(&self, n: usize, suffix: &str) -> String;
    /// `open` itineraries open out of `known` (`open_km` of `total_km`), with a caveat
    /// when the snow is too little.
    fn itineraries_open(
        &self,
        open: usize,
        known: usize,
        open_km: f64,
        total_km: f64,
        suffix: &str,
        no_snow: bool,
    ) -> String;
    /// The area is closed at this date (published opening dates).
    fn area_closed_dates(&self) -> &'static str;
    /// Km of nordic trails of an area.
    fn nordic_km(&self, km: f64) -> String;
    /// A ranked candidate: « 1. Name (score 2,5) : reasons. ».
    fn candidate(&self, rank: usize, name: &str, score: f64, reasons: &[String]) -> String;
    /// A reason with its points: « ouvert [+3,00] ».
    fn reason_points(&self, text: &str, points: f64) -> String;
    /// First line: site and date.
    fn briefing_title(&self, site: &str, date: NaiveDate) -> String;
    /// BERA level.
    fn briefing_bera(&self, level: u8) -> String;
    /// Header of the areas.
    fn briefing_domains(&self) -> &'static str;
    /// All `n` known itineraries are closed.
    fn briefing_all_closed(&self, n: usize) -> String;
    /// Header of the itineraries.
    fn briefing_itineraries(&self) -> &'static str;
    /// Snow quality at a forecast point.
    fn briefing_snow_quality(&self, point: &str, summary: &[String]) -> String;
    /// Sunset.
    fn briefing_sunset(&self, at: NaiveDateTime) -> String;
    /// Notes on the data.
    fn briefing_data(&self, notes: &[String]) -> String;

    // ---- Crowd (A4).

    /// Level of the crowd.
    fn crowd_level(&self, level: CrowdLevel) -> &'static str;
    /// Name of a school holiday or a public holiday (the French name given).
    fn holiday(&self, fr: &'static str) -> &'static str;
    /// Who: a population (« zone A ») or a country.
    fn population(&self, p: Population) -> &'static str;
    /// France, as a country of a public holiday.
    fn france(&self) -> &'static str;
    /// A holiday and who has it: « vacances d'hiver (zone A, zone B) ».
    fn holiday_for(&self, name: &str, who: &[&str]) -> String;
    /// Fresh snow of the two days before.
    fn crowd_fresh_snow(&self, cm: f64) -> String;
    /// Sun.
    fn crowd_sun(&self) -> &'static str;
    /// The area is closed (published opening dates).
    fn crowd_area_closed(&self) -> &'static str;

    // ---- Snow quality (A3).

    /// Label of a snow quality.
    fn quality(&self, q: Quality) -> &'static str;
    /// Name of an aspect, octant 0 = north.
    fn aspect(&self, octant: usize) -> &'static str;
    /// « plat », « sud, pente de 20° ».
    fn slope_class(&self, c: &SlopeClass) -> String;
    /// « de 10 h à 13 h ».
    fn slot(&self, s: &Slot) -> String;
    /// No or too little snow at the point.
    fn sq_no_snow(&self, cm: f64, elevation: f64) -> String;
    /// On the flat between 10 h and 15 h.
    fn sq_flat(&self, elevation: f64, q: Quality) -> String;
    /// Best slot.
    fn sq_best(&self, class: &str, slot: &str, q: Quality) -> String;
    /// « sud 13 h ».
    fn sq_aspect_at(&self, aspect: &str, at: NaiveDateTime) -> String;
    /// « sud 30° dès 14 h ».
    fn sq_aspect_slope_from(&self, aspect: &str, slope: f64, at: NaiveDateTime) -> String;
    /// 20° slopes soften around.
    fn sq_softens(&self, list: &[String]) -> String;
    /// Heavy snow.
    fn sq_heavy(&self, list: &[String]) -> String;
    /// Does not soften (20°).
    fn sq_never_softens(&self, list: &[&str]) -> String;
    /// The limits of the model.
    fn sq_model(&self) -> &'static str;
    // ---- Groups (C11).
    /// Label of a group: « « Famille » : 2 adultes, enfants de 10 et 14 ans, classique, niveau débutant ».
    fn group_label(&self, p: &skimap_core::calibration::GroupProfile) -> String;
    /// Technique: « classique », « skating ».
    fn style(&self, s: skimap_core::routing::Style) -> &'static str;
    /// Outings learned by a group: « a priori », « une sortie apprise ».
    fn learned(&self, outings: u32) -> String;
    /// A duration: « 45 min », « 1 h 50 ».
    fn hm(&self, seconds: f64) -> String;
    // ---- Loops (A5).
    /// No trail graph.
    fn s_no_graph(&self) -> &'static str;
    /// No trail within 300 m of the start.
    fn s_no_trail_near(&self, start: &str) -> String;
    /// A loop without a name.
    fn s_unnamed_loop(&self) -> &'static str;
    /// Time of a loop for a group planned on the slow side.
    #[allow(clippy::too_many_arguments)]
    fn s_time_group_slow(
        &self,
        median: &str,
        group: &str,
        lo: &str,
        hi: &str,
        learned: &str,
        plan: &str,
        style: &str,
        model: &str,
    ) -> String;
    /// Time of a loop for a group.
    #[allow(clippy::too_many_arguments)]
    fn s_time_group(
        &self,
        median: &str,
        group: &str,
        lo: &str,
        hi: &str,
        learned: &str,
        style: &str,
        model: &str,
    ) -> String;
    /// Model time of a loop.
    fn s_time_model(&self, model: &str, style: &str) -> String;
    /// Length, climb, time and return of a loop.
    fn s_loop(&self, km: f64, gain: f64, time: &str, back: NaiveDateTime) -> String;
    /// Share of the loop out and back on the same way.
    fn s_out_and_back(&self, pct: f64) -> String;
    /// A figure of eight.
    fn s_eight(&self, km: f64) -> String;
    /// Km on closed trails.
    fn s_closed_km(&self, km: f64) -> String;
    /// Km on open trails.
    fn s_open_km(&self, km: f64) -> String;
    /// Km without a known status.
    fn s_unknown_km(&self, km: f64) -> String;
    /// Trail status unknown.
    fn s_status_unknown(&self) -> &'static str;
    /// BERA level.
    fn s_bera(&self, level: u8) -> String;
    /// No BERA in this range.
    fn s_no_bera_range(&self) -> &'static str;
    /// BERA not available.
    fn s_bera_unavailable(&self) -> &'static str;
    /// Km in terrain exposed to avalanches.
    fn s_exposed(&self, km: f64, why: &str) -> String;
    /// Km on trails harder than asked.
    fn s_harder(&self, km: f64) -> String;
    /// The trails of a loop.
    fn s_trails(&self, names: &[String]) -> String;
    /// A loop that needs all to go well.
    fn s_if_all_goes_well(&self, median: &str, slow: &str) -> String;
    /// Turning points without a way back.
    fn s_no_way_back(&self, n: usize) -> String;
    /// Loops too short or too long.
    fn s_evaluated(
        &self,
        evaluated: usize,
        short: usize,
        long: usize,
        group: Option<&str>,
    ) -> String;
    /// Loops set aside: back too late.
    fn s_too_late(&self, n: usize, limit: Option<NaiveDateTime>) -> String;
    /// No loop fits.
    fn s_none_fits(&self) -> &'static str;
    /// No real loop: the loops proposed are partly out and back.
    fn s_no_real_loop(
        &self,
        lo: &str,
        hi: &str,
        start: &str,
        one: bool,
        longest: Option<(f64, &str)>,
    ) -> String;
    /// Too little snow on every loop.
    fn s_no_snow_all(&self) -> &'static str;
    /// Closed trails on most of each loop.
    fn s_closed_most(&self) -> &'static str;
    /// First line of the loops: request, start, date and time, limit, group.
    fn s_title(
        &self,
        label: &str,
        start: &str,
        date: NaiveDate,
        at: NaiveDateTime,
        deadline: Option<NaiveDateTime>,
        group: Option<&str>,
    ) -> String;
    /// A ranked loop with its reasons.
    fn s_line(&self, rank: usize, hopeful: bool, label: &str, score: f64, why: &[String])
    -> String;
    /// Limits of the loops.
    fn s_limits(&self, notes: &[String]) -> String;
    /// Nodes out of reach from the start.
    fn s_unreachable(&self, nodes: usize, of: usize, km: f64, start: &str) -> String;
}
