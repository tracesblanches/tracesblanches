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

    fn group_learned(&self, outings: u32) -> String {
        match outings {
            0 => "prior, no outing learned".to_owned(),
            1 => "one outing learned".to_owned(),
            n => format!("{n} outings learned"),
        }
    }

    fn group_who(&self, adults: u8, children: &[u8]) -> String {
        let adults = match adults {
            1 => "1 adult".to_owned(),
            n => format!("{n} adults"),
        };
        let ages: Vec<String> = children.iter().map(u8::to_string).collect();
        match ages.split_last() {
            None => adults,
            Some((last, [])) => format!("{adults}, children of {last}"),
            Some((last, rest)) => format!("{adults}, children of {} and {last}", rest.join(", ")),
        }
    }

    fn duration(&self, s: f64) -> String {
        skimap_plan::pace::duration_fr(s)
    }

    fn clock5(&self, t: NaiveDateTime) -> String {
        skimap_plan::pace::clock_fr(t).replace(" h ", ":")
    }

    fn clock_day(&self, t: NaiveDateTime, day: NaiveDate) -> String {
        if t.date() > day {
            format!("{} the next day", self.clock5(t))
        } else {
            self.clock5(t)
        }
    }

    fn duration_range(&self, lo: &str, hi: &str) -> String {
        format!("{lo} to {hi}")
    }

    fn duration_about(&self, median: &str, lo: &str, hi: &str) -> String {
        format!("about {median} ({lo} to {hi})")
    }

    fn back_slow_side(&self, median: &str, plan: &str, back: &str) -> String {
        format!("median {median}, planned on {plan} (slow side), back around {back}")
    }

    fn back_between(&self, lo: &str, hi: &str) -> String {
        format!("back between {lo} and {hi}")
    }

    fn back_around(&self, at: &str) -> String {
        format!("back around {at}")
    }

    fn model_time(&self, d: &str) -> String {
        format!("about {d} (model)")
    }

    fn back_with_margin(&self, at: &str) -> String {
        format!("back around {at} with the margin")
    }

    fn after_limit(&self, limit: NaiveDateTime, for_group: bool) -> String {
        format!(
            "Expected back after the limit of {}: a long loop for {}",
            self.assist().clock(limit),
            if for_group { "this group" } else { "this day" }
        )
    }

    fn key_kind(&self, k: skimap_plan::plan::KeyKind) -> &'static str {
        use skimap_plan::plan::KeyKind;
        match k {
            KeyKind::Start => "start",
            KeyKind::Turn => "junction",
            KeyKind::Junction => "obvious junction",
            KeyKind::Refuge => "hut",
            KeyKind::NoNetworkStart => "start of a zone without network",
            KeyKind::NoNetworkEnd => "end of a zone without network",
            KeyKind::AvalancheStart => "start of a known avalanche path",
            KeyKind::AvalancheEnd => "end of a known avalanche path",
            KeyKind::Finish => "finish",
        }
    }

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
    ) -> String {
        let a = self.assist();
        format!(
            "Cross-country ski outing {site} on {}: start {start} at {}, loop {label} ({} km, trails {}). Back around {}{}. No news from me at {}: call 112 and give them this plan.",
            date.format("%d/%m"),
            a.clock(departure),
            a.num(km, 1),
            trails.join(", "),
            a.clock(back),
            limit
                .map(|l| format!(", {} at the latest", a.clock(l)))
                .unwrap_or_default(),
            a.clock(alert)
        )
    }

    fn default_group_name(&self) -> &'static str {
        "Family with beginner children"
    }

    fn solo_group_name(&self) -> &'static str {
        "Solo"
    }
}
