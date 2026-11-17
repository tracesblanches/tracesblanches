//! Texts of this crate in the language of the app (VO6, #129): one module per language
//! ([`fr`], [`en`]), each implementing every text of [`Texts`]; [`of`] gives the texts of
//! a language. A text missing in one language does not compile. Numbers, dates, clock
//! times and durations come from `skimap_assist::text`.
//!
//! Short, concrete sentences; never « sans danger » / « safe ».

#[allow(unused_imports)]
use chrono::{NaiveDate, NaiveDateTime};
pub use skimap_common::Lang;

pub mod en;
pub mod fr;

/// The texts of [`Lang::Fr`] or [`Lang::En`].
pub fn of(lang: Lang) -> &'static dyn Texts {
    match lang {
        Lang::Fr => &fr::Fr,
        Lang::En => &en::En,
    }
}

/// Every text of this crate, in one language.
pub trait Texts: Sync {
    /// The shared texts (numbers, dates, clock times, durations) of the same language.
    fn assist(&self) -> &'static dyn skimap_assist::text::Texts;
    // ---- Groups (C11).
    /// Outings learned: « a priori, aucune sortie apprise ».
    fn group_learned(&self, outings: u32) -> String;
    /// Who is in the group: « 2 adultes, enfants de 10 et 14 ans ».
    fn group_who(&self, adults: u8, children: &[u8]) -> String;
    // ---- The morning (J5).
    /// A duration rounded to 5 min: « 2 h 30 », « 45 min ».
    fn duration(&self, s: f64) -> String;
    /// A clock time rounded to 5 min: « 15 h 20 », « 15:20 ».
    fn clock5(&self, t: NaiveDateTime) -> String;
    /// A clock time, « le lendemain » when after the day.
    fn clock_day(&self, t: NaiveDateTime, day: NaiveDate) -> String;
    /// A range of durations: « 1 h 40 à 2 h 50 ».
    fn duration_range(&self, lo: &str, hi: &str) -> String;
    /// A median duration with its range.
    fn duration_about(&self, median: &str, lo: &str, hi: &str) -> String;
    /// Planned on the slow side of the group.
    fn back_slow_side(&self, median: &str, plan: &str, back: &str) -> String;
    /// Back between two times.
    fn back_between(&self, lo: &str, hi: &str) -> String;
    /// Back around a time.
    fn back_around(&self, at: &str) -> String;
    /// Model time of a loop.
    fn model_time(&self, d: &str) -> String;
    /// Back around a time with the margin.
    fn back_with_margin(&self, at: &str) -> String;
    /// Expected return after the limit of the day.
    fn after_limit(&self, limit: NaiveDateTime, for_group: bool) -> String;
    /// Kind of a key point of the plan.
    fn key_kind(&self, k: skimap_plan::plan::KeyKind) -> &'static str;
    /// The SMS to the contact (K10): route, return, what to do without news.
    #[allow(clippy::too_many_arguments)]
    fn sms(
        &self,
        site: &str,
        date: NaiveDate,
        start: &str,
        departure: NaiveDateTime,
        label: &str,
        km: f64,
        trails: &[String],
        back: NaiveDateTime,
        limit: Option<NaiveDateTime>,
        alert: NaiveDateTime,
    ) -> String;
    /// Name of the group the app offers first.
    fn default_group_name(&self) -> &'static str;
    /// Name of the group of one adult the app offers.
    fn solo_group_name(&self) -> &'static str;

    // ---- The recap of an outing (S4).
    /// When an outing started: « 15/02 à 10 h 05 ».
    fn when(&self, t: NaiveDateTime) -> String;
    /// Actual against estimated time: « comme prévu », « 10 min de plus que prévu ».
    fn delta(&self, actual_s: f64, estimated_s: f64) -> String;
    /// « Distance : 12,4 km ».
    fn recap_distance(&self, km: f64) -> String;
    /// « Temps : 2 h 30 dont 2 h 05 en mouvement ».
    fn recap_time(&self, elapsed_s: f64, moving_s: f64) -> String;
    /// « Vitesse en mouvement : 6,1 km/h ».
    fn recap_speed(&self, kmh: f64) -> String;
    /// Estimated against actual time, `group`: estimated at the pace of the group.
    fn recap_estimate(&self, group: bool, estimated_s: f64, actual_s: f64) -> String;
    /// Ascent from a Strava export.
    fn recap_gain_imported(&self, gain_m: f64) -> String;
    /// Ascent and descent with the source of the altitude.
    fn recap_elevation(
        &self,
        ascent_m: f64,
        descent_m: f64,
        source: crate::recap::AltitudeSource,
    ) -> String;
    /// The `n`th time on the same route: last time (`when`, `last_s`), today `diff_s` more.
    fn recap_same_route(&self, n: usize, when: &str, last_s: f64, diff_s: f64) -> String;
    /// Moving speed against the mean of `n` previous outings here.
    fn recap_speed_vs(&self, kmh: f64, mean_kmh: f64, n: usize) -> String;
    /// Nothing to compare with yet.
    fn recap_first(&self) -> &'static str;
    /// What the voice says: distance, time, delta, comparison.
    fn recap_spoken(
        &self,
        distance: &str,
        elapsed_s: f64,
        delta: Option<&str>,
        comparison: Option<&str>,
    ) -> String;
    /// Forecast temperature range.
    fn cond_temperature(&self, lo: f64, hi: f64) -> String;
    /// Forecast gusts.
    fn cond_gusts(&self, kmh: f64) -> String;
    /// Hours of forecast fog.
    fn cond_fog(&self, hours: usize) -> String;
    /// Forecast snow.
    fn cond_snow(&self, cm: f64) -> String;
    /// Where the forecast comes from.
    fn cond_source(&self, point: &str) -> String;
    /// Altitude range of the outing.
    fn cond_altitude(&self, lo: f64, hi: f64) -> String;
    /// No forecast in the outing.
    fn cond_none(&self) -> &'static str;
    /// Not an outing (error).
    fn not_an_outing(&self, dir: &str) -> String;

    // ---- The rules of a place (F11).
    /// Title of a topic.
    fn rule_topic(&self, t: skimap_zones::rules::Topic) -> &'static str;
    /// A verdict: « interdit », « toléré sous conditions ».
    fn rule_verdict(&self, v: skimap_core::zone_eval::Verdict) -> &'static str;
    /// Suffix of a fire ban that includes stoves.
    fn stoves_included(&self) -> &'static str;
    /// A zone out of its sensitive months.
    fn zone_off_season(&self, name: &str) -> String;
    /// Why a rule is not in force.
    fn rule_inactive(&self, i: skimap_core::zone_eval::Inactive) -> &'static str;
    /// Kind of the source of a rule.
    fn source_kind(&self, k: skimap_zones::rules::SourceKind) -> &'static str;
    /// The instant of the rules: « le 15/02 à 9 h 00 ».
    fn rules_at(&self, t: NaiveDateTime) -> String;
    /// What the rules are not.
    fn rules_note(&self) -> &'static str;
    /// The act governing a zone besides the national rules (G4).
    fn local_act(&self, a: skimap_zones::local::OwnAct) -> &'static str;
    /// A zone whose own act was not read: « APPB X (arrêté de protection de biotope) :
    /// texte non lu ».
    fn local_unverified(&self, name: &str, act: &str) -> String;
    /// A territory some of whose acts were read.
    fn local_partial(&self, name: &str, act: &str) -> String;
    /// French municipal and prefectoral decrees, never listed.
    fn local_municipal_fr(&self) -> &'static str;
    /// Swiss communal regulations, never listed.
    fn local_municipal_ch(&self) -> &'static str;
    /// The line of the local rules not verified, from its items; empty without item.
    fn local_rules(&self, items: &[String]) -> String;

    // ---- Preparing the next days and ski mode (S3).
    /// A day: « lundi 16/02 ».
    fn day(&self, d: NaiveDate) -> String;
    /// « mis à jour le 15/11 à 10 h 05 ».
    fn updated(&self, t: NaiveDateTime) -> String;
    /// No date known.
    fn date_unknown(&self) -> &'static str;
    /// A day skipped: no forecast in the pack.
    fn skipped_no_forecast(&self, day: &str) -> String;
    /// A day skipped: no known start.
    fn skipped_no_start(&self, day: &str) -> String;
    /// Time of a route at the pace of the group: median and range.
    fn time_group(&self, median_s: f64, lo_s: f64, hi_s: f64) -> String;
    /// Time of a route by the model.
    fn time_model(&self, s: f64) -> String;
    /// Label of an information of the pack by layer id (`bera`, `status`…), `pack` for
    /// the pack itself.
    fn info_label(&self, id: &str) -> Option<&'static str>;
    /// Battery enough for more than 24 h.
    fn battery_day(&self, reserve_pct: f64, cold: Option<f64>) -> String;
    /// Battery enough until a time.
    fn battery_until(&self, until: &str, reserve_pct: f64, cold: Option<f64>) -> String;
    /// Battery not enough for the planned outing.
    fn battery_short(&self, end: &str, reserve: &str) -> String;

    // ---- Off-piste (SR2).
    /// « Jamais une garantie », said with every off-piste route and low-exposure zone.
    fn never_guarantee(&self) -> &'static str;
    /// A layer the pack lacks: `snow`, `exposure`, `zones`, `network`.
    fn missing_layer(&self, id: &str) -> &'static str;
    /// No off-piste relief in the pack.
    fn no_offpiste_relief(&self) -> &'static str;
    /// A point outside the relief.
    fn outside_relief(&self) -> &'static str;
    /// No passable cell near an end.
    fn no_passable(&self, radius_m: f64) -> String;
    /// No route under 30°.
    fn no_offpiste_route(&self) -> &'static str;

    // ---- Guidance (J7, #54).
    /// A range of arrival times: « 15 h 20 à 15 h 40 ».
    fn arrival_range(&self, lo: NaiveDateTime, hi: NaiveDateTime) -> String;
    /// Why the route changed.
    fn cause(&self, c: &skimap_plan::tracker::Cause) -> String;
    /// The written proposal of « ramène-moi ».
    fn back_proposal(&self, target: Option<&crate::tracking_back::Target>) -> String;

    // ---- Packs shown together (free mode).
    /// Name of the merged packs.
    fn all_domains(&self) -> &'static str;
    /// « 3 packs : A, B, C. »
    fn packs_note(&self, names: &[&str]) -> String;
    /// No pack installed: name of the map (the France basemap alone), never « all the
    /// installed areas » (#162).
    fn france_only_name(&self) -> &'static str;
    /// No pack installed: the France basemap alone, with its date.
    fn france_only_note(&self, date: &str) -> String;
}
