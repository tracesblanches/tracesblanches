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
}
