//! Texts of the assistant in the language of the app (VO6, #129): one module per language
//! ([`fr`], [`en`]), each implementing every text of [`Texts`]; [`of`] gives the texts of
//! a language. A text missing in one language does not compile.
//!
//! Short, concrete sentences; never « sans danger » / « safe ».

use chrono::{NaiveDate, NaiveDateTime};
pub use skimap_common::Lang;

use crate::crowd::{CrowdLevel, Population};
use crate::equipment::Kind as EquipmentKind;
use crate::inputs::LayerName;
use crate::snow_quality::{Quality, SlopeClass, Slot};
use crate::wax::SnowType;

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

    // ---- Best time to leave (A9).

    /// Snow surface of a slope class over the outing: « plat : neige dure puis neige humide ».
    fn bt_surface(&self, class: &str, qualities: &[&str]) -> String;
    /// Separator of the slope classes.
    fn bt_sep(&self) -> &'static str;
    /// Gusts of 40 km/h or more during the outing.
    fn bt_gusts(&self, kmh: f64) -> String;
    /// Hours of fog during the outing.
    fn bt_fog(&self, hours: f64) -> String;
    /// Snowfall during the outing.
    fn bt_snowfall(&self) -> &'static str;
    /// Rain during the outing.
    fn bt_rain(&self) -> &'static str;
    /// Wind chill down to.
    fn bt_cold(&self, chill: &str) -> String;
    /// Sun.
    fn bt_sun(&self) -> &'static str;
    /// Busy day: assumed peak from 10 h 30 to 14 h.
    fn bt_crowd(&self, level: CrowdLevel) -> String;
    /// Return less than 30 min before the deadline.
    fn bt_near_deadline(&self, deadline: NaiveDateTime) -> String;
    /// What a start two hours or more away would lose.
    fn bt_other_start(&self, start: NaiveDateTime, lost: &[String]) -> String;
    /// The best start, its return, the outing and why; `others` the other starts.
    fn bt_best(
        &self,
        start: NaiveDateTime,
        back: NaiveDateTime,
        minutes: f64,
        why: &[String],
        others: &[String],
    ) -> String;
    /// No start ends before the deadline.
    fn bt_none(&self, minutes: f64, deadline: Option<NaiveDateTime>) -> String;
    /// No best time: the starts that end before the deadline have no hourly forecast.
    fn bt_no_forecast(&self) -> &'static str;

    // ---- Wax (A2).

    /// Snow type of the grip wax.
    fn wax_snow_type(&self, t: SnowType) -> &'static str;
    /// A window: hours, air and snow temperatures (« +2 »), snow type, grip and glide.
    #[allow(clippy::too_many_arguments)]
    fn wax_window(
        &self,
        hours: (u32, u32),
        air: &str,
        snow: &str,
        snow_type: SnowType,
        kick: &str,
        glide: (&str, &str),
        glide_alt: Option<(&str, &str)>,
    ) -> String;
    /// Grip wax (product, brand) with its alternative.
    fn wax_kick(&self, kick: (&str, &str), alt: Option<(&str, &str)>) -> String;
    /// Grip wax unreliable: the skins advice, otherwise the wax.
    fn wax_kick_uncertain(
        &self,
        skins: Option<&str>,
        kick: (&str, &str),
        alt: Option<(&str, &str)>,
    ) -> String;
    /// First reason: snow type and air temperatures.
    fn wax_model(&self, snow_type: SnowType, mean: f64, min: f64, max: f64) -> String;
    /// Colder than every range.
    fn wax_colder_than_all(&self) -> &'static str;
    /// Warmer than the hard waxes for this snow.
    fn wax_warmer_than_hard(&self) -> &'static str;
    /// Coarse refrozen snow: klister.
    fn wax_coarse_frozen(&self) -> &'static str;
    /// Fresh snow around 0 °C.
    fn wax_fresh_near_zero(&self) -> &'static str;
    /// Refrozen at the start of the window, wet after.
    fn wax_refrozen_then_wet(&self, first: Quality, last: Quality) -> String;
    /// The temperature crosses 0 °C in the window.
    fn wax_crosses_zero(&self, min: f64, max: f64) -> String;
    /// Rain or wet snow falling.
    fn wax_wet_falling(&self) -> &'static str;
    /// Waxless or skin skis advised, and why.
    fn wax_skins(&self, why: &str) -> String;
    /// Glide: air and snow temperatures of the tables.
    fn wax_glide(&self, air: f64, snow: f64) -> String;
    /// Mean humidity.
    fn wax_humidity(&self, pct: f64) -> String;
    /// Humidity unknown.
    fn wax_humidity_unknown(&self) -> &'static str;
    /// The day: morning, and the afternoon when it differs.
    fn wax_day(&self, morning: &str, afternoon: Option<&str>) -> String;

    // ---- Equipment (A6).

    /// Label of an equipment item.
    fn eq_label(&self, k: EquipmentKind) -> &'static str;
    /// Km in terrain exposed to avalanches.
    fn eq_exposed(&self, km: f64) -> String;
    /// Km in known avalanche paths.
    fn eq_clpa(&self, km: f64) -> String;
    /// Off-piste with a BERA of 3 or more.
    fn eq_offpiste_bera(&self, level: u8) -> String;
    /// Back after sunset (or less than an hour before).
    fn eq_back_after_sunset(&self, back: NaiveDateTime, sunset: NaiveDateTime) -> String;
    /// Start before sunrise.
    fn eq_start_before_sunrise(&self, start: NaiveDateTime, sunrise: NaiveDateTime) -> String;
    /// Wind chill down to.
    fn eq_chill(&self, chill: &str) -> String;
    /// Km without any mobile network.
    fn eq_no_network(&self, km: f64, pct: f64) -> String;
    /// Off-piste outing.
    fn eq_offpiste(&self) -> &'static str;
    /// Outing of more than 3 h.
    fn eq_long(&self, minutes: f64) -> String;
    /// Fog forecast during the outing.
    fn eq_fog(&self, hours: usize) -> String;
    /// Clear sky.
    fn eq_clear_sky(&self, cloud_pct: f64) -> String;
    /// Grip wax unreliable.
    fn eq_kick_unreliable(&self) -> &'static str;
    /// Nothing to add to the usual equipment.
    fn eq_nothing(&self, outing: &str) -> String;
    /// What to take, item and reason.
    fn eq_take(&self, outing: &str, items: &[(&str, &str)]) -> String;

    // ---- Synthesis of the day (A7).

    /// An altitude limit of the BERA (« >2400 »): « au-dessus de 2400 m »; anything else
    /// as is.
    fn sy_altitude(&self, loc: &str) -> String;
    /// The other level, below or above the limit.
    fn sy_risk_other(&self, loc: &str, level: u8) -> String;
    /// The most exposed aspects.
    fn sy_aspects(&self, aspects: &[&str]) -> String;
    /// Read the whole bulletin.
    fn sy_read_bulletin(&self) -> &'static str;
    /// No BERA (Jura): the voice clip « no bulletin », then the SLF.
    fn sy_no_bera_jura(&self, no_bulletin: &str) -> String;
    /// No BERA for this massif.
    fn sy_no_bera(&self, no_bulletin: &str) -> String;
    /// BERA not available offline (`loaded`: read but not available; otherwise not loaded).
    fn sy_bera_missing(&self, massif: Option<&str>, loaded: bool) -> String;
    /// The route crosses km of known avalanche paths.
    fn sy_clpa(&self, km: f64, paths: usize) -> String;
    /// The route crosses no known avalanche path.
    fn sy_clpa_none(&self) -> &'static str;
    /// No map of the known avalanche paths in this massif.
    fn sy_clpa_no_map(&self) -> &'static str;
    /// No offline weather for the day.
    fn sy_no_weather(&self) -> &'static str;
    /// No offline forecast for the day at a point.
    fn sy_no_forecast(&self, point: &str) -> String;
    /// One temperature, or a range.
    fn sy_temperature(&self, min: &str, max: Option<&str>) -> String;
    /// Wind chill.
    fn sy_chill(&self, chill: &str) -> String;
    /// Light wind.
    fn sy_wind_light(&self) -> &'static str;
    /// Fog possible.
    fn sy_fog(&self, hours: usize) -> String;
    /// Snowfall.
    fn sy_snow(&self, cm: f64) -> String;
    /// Freezing level.
    fn sy_freezing(&self, m: f64) -> String;
    /// Clear sky.
    fn sy_clear(&self) -> &'static str;
    /// Overcast.
    fn sy_overcast(&self) -> &'static str;
    /// The weather sentence: point, elevation, parts.
    fn sy_weather(&self, point: &str, elevation: f64, parts: &[String]) -> String;
    /// Words of a rule (topic, verdict, stoves included); `None`: not said.
    fn sy_verdict(
        &self,
        topic: skimap_zones::rules::Topic,
        verdict: skimap_core::zone_eval::Verdict,
        stoves: bool,
    ) -> Option<&'static str>;
    /// A rule at night.
    fn sy_night(&self, words: &str) -> String;
    /// Three zones or more: the first two and how many others.
    fn sy_zones_more(&self, first: &str, second: &str, others: usize) -> String;
    /// Until a date.
    fn sy_until(&self, day: u8, month: u8) -> String;
    /// Rules of the protected zones not loaded.
    fn sy_rules_not_loaded(&self) -> &'static str;
    /// No access rule known active this day.
    fn sy_no_rule(&self) -> &'static str;
    /// Points outside France and Switzerland.
    fn sy_not_covered(&self) -> &'static str;
    /// Km without any mobile network on the route.
    fn sy_no_network_km(&self, km: f64) -> String;
    /// Mobile network on the whole route.
    fn sy_network_everywhere(&self) -> &'static str;
    /// Share of the trails without any mobile network.
    fn sy_no_network_share(&self, pct: f64) -> String;
    /// The rules sentence: on the route or on the trails of the site.
    fn sy_rules(&self, route: bool, rules: &str, not_covered: &str, network: &str) -> String;

    // ---- Alerts during the outing (A8).

    /// The wind rises.
    fn al_wind_rising(&self, gust_kmh: f64, before_kmh: f64) -> String;
    /// Fog within the hour.
    fn al_fog(&self) -> &'static str;
    /// Snowfall within the hour.
    fn al_snow(&self, cm_h: f64) -> String;
    /// The temperature drops.
    fn al_cold(&self, temperature: &str, chill: &str) -> String;
    /// Late: turn back now to be back before the deadline.
    fn al_turn_now(&self, late_min: f64, pace: f64, deadline: Option<NaiveDateTime>) -> String;
    /// Late: the loop no longer ends before the deadline; latest turnaround.
    fn al_turn_advanced(
        &self,
        late_min: f64,
        pace: f64,
        deadline: Option<NaiveDateTime>,
        turn: NaiveDateTime,
    ) -> String;

    // ---- Missing layers.

    /// Name of a layer of the assistant.
    fn layer_name(&self, l: LayerName) -> &'static str;
    /// A layer that could not be read, and why (reader error, local path).
    fn layer_missing(&self, l: LayerName, why: &str) -> String;
    /// Why a layer is left out of a public build: one of its sources may not go into a
    /// published pack (`skimap_common::licence::publishable`, #151).
    fn layer_personal(&self) -> &'static str;
}
