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
    /// Never « sans danger »: the text of every plan.
    fn never_safe(&self) -> &'static str;
    /// No hourly weather.
    fn no_hourly_weather(&self) -> &'static str;
    /// No best time computed: the start used.
    fn no_best_time(&self, start: NaiveDateTime) -> String;
}
