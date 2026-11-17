//! Texts in English.

#[allow(unused_imports)]
use chrono::{NaiveDate, NaiveDateTime};

use super::Texts;

/// English.
pub struct En;

impl Texts for En {
    fn assist(&self) -> &'static dyn skimap_assist::text::Texts {
        skimap_assist::text::of(skimap_assist::text::Lang::En)
    }

    fn never_safe(&self) -> &'static str {
        "This plan is never a guarantee: information is indicative, read the BERA and the official sources, and decide on the spot."
    }

    fn no_hourly_weather(&self) -> &'static str {
        "no hourly weather: no best time, no wax, no condition alerts"
    }

    fn no_best_time(&self, start: NaiveDateTime) -> String {
        format!(
            "no best time computed: start at {}",
            self.assist().clock(start)
        )
    }
}
