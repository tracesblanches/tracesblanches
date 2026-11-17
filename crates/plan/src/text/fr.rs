//! Texts in French.

#[allow(unused_imports)]
use chrono::{NaiveDate, NaiveDateTime};

use super::Texts;

/// French.
pub struct Fr;

impl Texts for Fr {
    fn assist(&self) -> &'static dyn skimap_assist::text::Texts {
        skimap_assist::text::of(skimap_assist::text::Lang::Fr)
    }

    fn never_safe(&self) -> &'static str {
        crate::plan::NEVER_SAFE
    }

    fn no_hourly_weather(&self) -> &'static str {
        "pas de météo horaire : ni meilleur moment, ni fartage, ni alertes de conditions"
    }

    fn no_best_time(&self, start: NaiveDateTime) -> String {
        format!(
            "pas de meilleur moment calculé : départ à {}",
            self.assist().clock(start)
        )
    }
}
