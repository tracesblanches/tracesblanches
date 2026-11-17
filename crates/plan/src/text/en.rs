//! Texts in English.

#[allow(unused_imports)]
use chrono::{NaiveDate, NaiveDateTime, Timelike};
use skimap_core::routing::Style;

use super::Texts;
use crate::pace::{PaceReason, ProposalKind};
use crate::plan::KeyKind;
use crate::session::End;

/// English.
pub struct En;

fn hhmm(t: NaiveDateTime) -> String {
    format!("{}:{:02}", t.hour(), t.minute())
}

fn km(m: f64) -> String {
    format!("{:.1}", m / 1000.0)
}

/// Minutes of the plans B, absolute value: « 45 min », « 1 h 05 ».
fn minutes(s: f64) -> String {
    let m = (s / 60.0).round().abs() as i64;
    if m < 60 {
        format!("{m} min")
    } else {
        format!("{} h {:02} min", m / 60, m % 60)
    }
}

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

    fn outing_label(
        &self,
        budget_min: u32,
        style: Style,
        easy_only: bool,
        return_before: Option<chrono::NaiveTime>,
    ) -> String {
        let (h, m) = (budget_min / 60, budget_min % 60);
        let mut s = match (h, m) {
            (0, m) => format!("{m} min"),
            (h, 0) => format!("{h} h"),
            (h, m) => format!("{h} h {m:02} min"),
        };
        s.push_str(match style {
            Style::Skating => ", skating",
            Style::Classic => ", classic",
        });
        if easy_only {
            s.push_str(", green and blue");
        }
        if let Some(t) = return_before {
            s.push_str(&format!(", back before {}:{:02}", t.hour(), t.minute()));
        }
        s
    }

    fn start_phrase<'a>(&self, fr: &'a str, en: Option<&'a str>) -> &'a str {
        en.unwrap_or(fr)
    }

    fn p_title(&self, date: NaiveDate, label: &str, start: &str, at: NaiveDateTime) -> String {
        format!(
            "Plan for {} — {label} — start {start} at {}",
            self.assist().date(date),
            hhmm(at)
        )
    }

    fn p_loop(
        &self,
        rank: usize,
        label: &str,
        km: f64,
        gain_m: f64,
        minutes: f64,
        style: Style,
        score: f64,
    ) -> String {
        format!(
            "Loop no. {rank}: {label} — {km:.1} km, {gain_m} m climb, about {} min (C5 model, {}), score {score:.2}",
            minutes.round(),
            self.assist().style(style),
        )
    }

    fn p_trails(&self, names: &[String]) -> String {
        format!("Trails: {}", names.join(", "))
    }

    fn p_arrival(
        &self,
        arrival: NaiveDateTime,
        with_margin: NaiveDateTime,
        deadline: Option<NaiveDateTime>,
        return_before: Option<NaiveDateTime>,
    ) -> String {
        format!(
            "Planned arrival {} at the model's pace, {} with the 20 % margin{}{}",
            hhmm(arrival),
            hhmm(with_margin),
            deadline
                .map(|d| format!("; light limit minus 30 min (C7): {}", hhmm(d)))
                .unwrap_or_default(),
            return_before
                .map(|d| format!("; back asked before {}", hhmm(d)))
                .unwrap_or_default()
        )
    }

    fn p_turnaround(&self, at: Option<NaiveDateTime>, group: bool) -> String {
        match at {
            Some(x) if group => format!(
                "Turn back at {} at the latest, at the group's planned pace (A8, C11)",
                hhmm(x)
            ),
            Some(x) => format!(
                "Turn back at {} at the latest, at the planned pace (A8)",
                hhmm(x)
            ),
            None => "The loop ends before the light limit at the planned pace: no turnaround time"
                .into(),
        }
    }

    fn p_reasons(&self) -> &'static str {
        "Reasons:"
    }

    fn p_reason(&self, text: &str, points: f64) -> String {
        format!("  - {text} [{points:+.2}]")
    }

    fn p_key_points(&self) -> &'static str {
        "Key points (time at the model's pace):"
    }

    fn p_key_kind(&self, k: KeyKind) -> &'static str {
        match k {
            KeyKind::Start => "start",
            KeyKind::Turn => "junction, announced",
            KeyKind::Junction => "obvious junction (nothing said)",
            KeyKind::Refuge => "refuge",
            KeyKind::NoNetworkStart => "start of a zone without network",
            KeyKind::NoNetworkEnd => "end of the zone without network",
            KeyKind::AvalancheStart => "start of a known avalanche path (CLPA)",
            KeyKind::AvalancheEnd => "end of the known avalanche path",
            KeyKind::Finish => "finish",
        }
    }

    fn p_key_point(&self, s_m: f64, at: NaiveDateTime, what: &str, name: Option<&str>) -> String {
        format!(
            "  {} km  {}  {what}{}",
            km(s_m),
            hhmm(at),
            name.map(|n| format!(": {n}")).unwrap_or_default()
        )
    }

    fn p_announcements(&self) -> &'static str {
        "Planned announcements:"
    }

    fn p_when(&self, s_m: Option<f64>, arrival: bool) -> String {
        match (s_m, arrival) {
            (Some(s), _) => format!("at {} km", km(s)),
            (None, false) => "at the start".to_owned(),
            (None, true) => "at the finish".to_owned(),
        }
    }

    fn p_announcement(&self, when: &str, priority: &str, text: &str) -> String {
        format!("  {when} [{priority}] “{text}”")
    }

    fn p_plans_b(&self, n: usize) -> String {
        format!("Plans B ({n}):")
    }

    fn p_plan_b(&self, text: &str, switch: &str) -> String {
        format!("  - {text} — announcement “{switch}”")
    }

    fn p_latest_exit(&self, s_m: f64, at: NaiveDateTime) -> String {
        format!(
            "  exit at the junction at {} km: pass it before {} to finish before the limit",
            km(s_m),
            hhmm(at)
        )
    }

    fn p_morning(&self) -> &'static str {
        "Morning:"
    }

    fn p_morning_line(&self, part: &str, line: &str) -> String {
        format!("  [{part}] {line}")
    }

    fn p_morning_voice(&self, part: &str, spoken: &str) -> String {
        format!("  [{part}, voice] {spoken}")
    }

    fn p_equipment(&self) -> &'static str {
        "Equipment:"
    }

    fn p_item(&self, label: &str, reason: &str) -> String {
        format!("  - {label}: {reason}")
    }

    fn p_refuges(&self, within_m: f64, names: &[String]) -> String {
        format!(
            "Refuges within {within_m} m of the loop: {}",
            names.join(", ")
        )
    }

    fn p_unnamed_shelter(&self, kind: &str) -> String {
        format!("shelter without a name ({kind})")
    }

    fn p_clips(&self, required: usize, missing: &[String]) -> String {
        format!(
            "Clips: {required} needed, {} missing from the pack{}",
            missing.len(),
            if missing.is_empty() {
                String::new()
            } else {
                format!(" ({})", missing.join(", "))
            }
        )
    }

    fn p_limit(&self, note: &str) -> String {
        format!("Limit: {note}")
    }

    fn fb_unnamed_trail(&self) -> &'static str {
        "the trail without a name"
    }

    fn fb_closed(&self, name: &str, span: Option<(f64, f64)>) -> String {
        format!(
            "If {name} is closed{}",
            span.map(|(a, b)| format!(" (from {} to {} km of the loop)", km(a), km(b)))
                .unwrap_or_default()
        )
    }

    fn fb_at(&self, junction_s_m: Option<f64>) -> String {
        match junction_s_m {
            Some(s) => format!("at the junction at {} km", km(s)),
            None => "at the start".to_owned(),
        }
    }

    fn fb_exit(&self, at: &str, length_m: f64, loop_m: f64, saves_s: f64, names: &str) -> String {
        format!(
            "Exit {at}: {} km in all instead of {}, {} less (C5 model), by {names}",
            km(length_m),
            km(loop_m),
            minutes(saves_s),
        )
    }

    fn fb_no_detour(&self, closed: &str) -> String {
        format!("{closed}: no other way in the trail graph")
    }

    fn fb_detour(
        &self,
        closed: &str,
        at: &str,
        rejoin_m: Option<f64>,
        length_m: f64,
        saves_s: f64,
        names: &str,
    ) -> String {
        format!(
            "{closed}: detour {at}{}, {} km in all, {} {} (C5 model), by {names}",
            match rejoin_m {
                Some(s) => format!(", back on the loop at {} km", km(s)),
                None => ", then straight to the finish".into(),
            },
            km(length_m),
            minutes(saves_s),
            if saves_s >= 0.0 { "less" } else { "more" },
        )
    }

    fn duration5(&self, s: f64) -> String {
        let m = ((s / 60.0 / 5.0).round() * 5.0) as i64;
        if m < 60 {
            format!("{m} min")
        } else {
            format!("{} h {:02} min", m / 60, m % 60)
        }
    }

    fn clock5(&self, t: NaiveDateTime) -> String {
        let m = (t.hour() * 60 + t.minute()) as f64 + t.second() as f64 / 60.0;
        let m = ((m / 5.0).round() * 5.0) as u32 % (24 * 60);
        format!("{}:{:02}", m / 60, m % 60)
    }

    fn pc_learned(&self, outings: u32) -> String {
        match outings {
            0 => "prior, no outing learned".to_owned(),
            1 => "one outing learned".to_owned(),
            n => format!("{n} outings learned"),
        }
    }

    fn pc_duration(&self, median: Option<f64>, lo: f64, hi: f64) -> String {
        match median {
            None => format!("{} to {}", self.duration5(lo), self.duration5(hi)),
            Some(m) => format!(
                "about {} ({} to {})",
                self.duration5(m),
                self.duration5(lo),
                self.duration5(hi)
            ),
        }
    }

    fn pc_slow_side(&self, median: f64, planned: f64, chances: f64, back: NaiveDateTime) -> String {
        format!(
            "median {}, planned on {} (slow side: {chances} chances in 10 to have finished), back planned around {}",
            self.duration5(median),
            self.duration5(planned),
            self.clock5(back)
        )
    }

    fn pc_back_between(&self, lo: NaiveDateTime, hi: NaiveDateTime) -> String {
        format!("back between {} and {}", self.clock5(lo), self.clock5(hi))
    }

    fn pc_back_around(&self, t: NaiveDateTime) -> String {
        format!("back around {}", self.clock5(t))
    }

    fn pc_text(
        &self,
        group: &str,
        learned: &str,
        duration: &str,
        back: &str,
        moving_s: f64,
        stops_s: f64,
        c5_s: f64,
    ) -> String {
        format!(
            "Estimated time for {group} ({learned}): {duration}, {back}; moving {} at the group's pace, stops {}; C5 model alone: {}",
            self.duration5(moving_s),
            self.duration5(stops_s),
            self.duration5(c5_s)
        )
    }

    fn pr_why(&self, reason: PaceReason, slower_pct: f64) -> String {
        match reason {
            PaceReason::Slower => format!("You are {slower_pct:.0} % slower than planned."),
            PaceReason::SlowerAndPauses => {
                format!("You are {slower_pct:.0} % slower than planned, with more breaks.")
            }
            PaceReason::Pauses => {
                format!("You take more breaks than planned ({slower_pct:.0} % slower in all).")
            }
        }
    }

    fn pr_what(&self, kind: &ProposalKind) -> &'static str {
        match kind {
            ProposalKind::ShortLoop => "I suggest the short loop",
            ProposalKind::NearestExit => "I suggest the nearest exit",
            ProposalKind::Keep => "I suggest keeping the loop",
        }
    }

    fn pr_text(
        &self,
        why: &str,
        what: &str,
        back: (NaiveDateTime, NaiveDateTime, NaiveDateTime),
        saved_min: Option<f64>,
    ) -> String {
        let saved = saved_min
            .filter(|m| *m >= 5.0)
            .map(|m| format!(", {} less", self.duration5(m * 60.0)))
            .unwrap_or_default();
        format!(
            "{why} {what}, back {} ({} to {}){saved}. Accept or decline.",
            self.clock5(back.0),
            self.clock5(back.1),
            self.clock5(back.2)
        )
    }

    fn recap(
        &self,
        end: End,
        km: f64,
        moving_min: f64,
        paused_min: f64,
        done_pct: f64,
        route_km: f64,
        ratio: Option<f64>,
        switches: usize,
        spoken: usize,
        refuges: &[String],
    ) -> String {
        format!(
            "{}: {km:.1} km in {moving_min} min moving and {paused_min} min stopped; {done_pct} % of the route followed ({route_km:.1} km){}; {switches} route change(s), {spoken} announcement(s){}",
            match end {
                End::Arrived => "Outing ended at the finish",
                End::Abandoned => "Outing stopped far from the finish",
            },
            ratio
                .map(|p| format!(", real time / model time: {p:.2}"))
                .unwrap_or_default(),
            if refuges.is_empty() {
                String::new()
            } else {
                format!(", stops: {}", refuges.join(", "))
            }
        )
    }
}
