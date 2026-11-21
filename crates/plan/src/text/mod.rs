//! Texts of this crate in the language of the app (VO6, #129): one module per language
//! ([`fr`], [`en`]), each implementing every text of [`Texts`]; [`of`] gives the texts of
//! a language. A text missing in one language does not compile. Numbers, dates, clock
//! times and durations come from `skimap_assist::text`.
//!
//! Short, concrete sentences; never « sans danger » / « safe ».

#[allow(unused_imports)]
use jiff::civil::{Date, DateTime};
pub use skimap_common::Lang;
use skimap_core::routing::Style;

use crate::pace::{PaceReason, ProposalKind};
use crate::plan::KeyKind;
use crate::session::End;

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
    fn no_best_time(&self, start: DateTime) -> String;

    // ---- Start names.

    /// A named start as said in a sentence: the French phrase of the site table, its
    /// English counterpart when the site table gives one.
    fn start_phrase<'a>(&self, fr: &'a str, en: Option<&'a str>) -> &'a str;

    // ---- The outing asked (`crate::outing`).

    /// Label of an outing: « 2 h, classique », « 1 h 30, skating, vertes et bleues,
    /// retour avant 16 h 00 ».
    fn outing_label(
        &self,
        budget_min: u32,
        style: Style,
        easy_only: bool,
        return_before: Option<jiff::civil::Time>,
    ) -> String;

    // ---- The plan as text (`DayPlan::text`).

    /// First line: date, label, start, departure.
    fn p_title(&self, date: Date, label: &str, start: &str, at: DateTime) -> String;
    /// The loop: rank, label, km, climb, model minutes, technique, score.
    #[allow(clippy::too_many_arguments)]
    fn p_loop(
        &self,
        rank: usize,
        label: &str,
        km: f64,
        gain_m: f64,
        minutes: f64,
        style: Style,
        score: f64,
    ) -> String;
    /// The trails of the loop.
    fn p_trails(&self, names: &[String]) -> String;
    /// Planned arrival, with the margin, the light limit and the return asked.
    fn p_arrival(
        &self,
        arrival: DateTime,
        with_margin: DateTime,
        deadline: Option<DateTime>,
        return_before: Option<DateTime>,
    ) -> String;
    /// Latest turnaround (`group`: at the group's planned pace); `None`: none needed.
    fn p_turnaround(&self, at: Option<DateTime>, group: bool) -> String;
    /// Header of the reasons.
    fn p_reasons(&self) -> &'static str;
    /// A reason and its points.
    fn p_reason(&self, text: &str, points: f64) -> String;
    /// Header of the key points.
    fn p_key_points(&self) -> &'static str;
    /// What a key point is.
    fn p_key_kind(&self, k: KeyKind) -> &'static str;
    /// A key point: distance (m), time, what, name.
    fn p_key_point(&self, s_m: f64, at: DateTime, what: &str, name: Option<&str>) -> String;
    /// Header of the announcements.
    fn p_announcements(&self) -> &'static str;
    /// When an announcement is said: at the departure (`None` and `false`), at a distance
    /// (m), at the arrival (`None` and `true`).
    fn p_when(&self, s_m: Option<f64>, arrival: bool) -> String;
    /// An announcement: when, priority, text.
    fn p_announcement(&self, when: &str, priority: &str, text: &str) -> String;
    /// Header of the plans B.
    fn p_plans_b(&self, n: usize) -> String;
    /// A plan B and its switch announcement.
    fn p_plan_b(&self, text: &str, switch: &str) -> String;
    /// The latest passage at a junction exit (m) to finish before the limit.
    fn p_latest_exit(&self, s_m: f64, at: DateTime) -> String;
    /// Header of the morning texts.
    fn p_morning(&self) -> &'static str;
    /// A morning text.
    fn p_morning_line(&self, part: &str, line: &str) -> String;
    /// The spoken morning texts.
    fn p_morning_voice(&self, part: &str, spoken: &str) -> String;
    /// Header of the equipment.
    fn p_equipment(&self) -> &'static str;
    /// An equipment item.
    fn p_item(&self, label: &str, reason: &str) -> String;
    /// Refuges near the loop.
    fn p_refuges(&self, within_m: f64, names: &[String]) -> String;
    /// A shelter without a name, and its kind.
    fn p_unnamed_shelter(&self, kind: &str) -> String;
    /// Clips needed and missing from the pack.
    fn p_clips(&self, required: usize, missing: &[String]) -> String;
    /// A limit of the plan.
    fn p_limit(&self, note: &str) -> String;

    // ---- Plans B.

    /// A trail without a name.
    fn fb_unnamed_trail(&self) -> &'static str;
    /// If a trail is closed, with its span on the loop (m).
    fn fb_closed(&self, name: &str, span: Option<(f64, f64)>) -> String;
    /// Where a plan B leaves the loop: at a junction (m), or at the start.
    fn fb_at(&self, junction_s_m: Option<f64>) -> String;
    /// An exit: where, length (m), loop length (m), model time saved (s), trails.
    fn fb_exit(&self, at: &str, length_m: f64, loop_m: f64, saves_s: f64, names: &str) -> String;
    /// No other way around a closed trail.
    fn fb_no_detour(&self, closed: &str) -> String;
    /// A detour: where, back on the loop (m) or straight to the finish, length (m), model
    /// time saved (s, negative: more), trails.
    #[allow(clippy::too_many_arguments)]
    fn fb_detour(
        &self,
        closed: &str,
        at: &str,
        rejoin_m: Option<f64>,
        length_m: f64,
        saves_s: f64,
        names: &str,
    ) -> String;

    // ---- Time of a group (C11) and pace proposals.

    /// A duration rounded to 5 min: « 2 h 30 », « 45 min ».
    fn duration5(&self, seconds: f64) -> String;
    /// A clock time rounded to 5 min: « 15 h 20 », « 15:20 ».
    fn clock5(&self, t: DateTime) -> String;
    /// Outings learned by the group.
    fn pc_learned(&self, outings: u32) -> String;
    /// The range of the time (`about`: with its median).
    fn pc_duration(&self, median: Option<f64>, lo: f64, hi: f64) -> String;
    /// Planned on the slow side: median, planned, chances in 10 to have finished, back.
    fn pc_slow_side(&self, median: f64, planned: f64, chances: f64, back: DateTime) -> String;
    /// Back between.
    fn pc_back_between(&self, lo: DateTime, hi: DateTime) -> String;
    /// Back around.
    fn pc_back_around(&self, t: DateTime) -> String;
    /// The time of the plan for a group.
    #[allow(clippy::too_many_arguments)]
    fn pc_text(
        &self,
        group: &str,
        learned: &str,
        duration: &str,
        back: &str,
        moving_s: f64,
        stops_s: f64,
        c5_s: f64,
    ) -> String;
    /// Why a proposal.
    fn pr_why(&self, reason: PaceReason, slower_pct: f64) -> String;
    /// What a proposal proposes.
    fn pr_what(&self, kind: &ProposalKind) -> &'static str;
    /// The proposal: why, what, return (planned, lo, hi), time saved (min).
    fn pr_text(
        &self,
        why: &str,
        what: &str,
        back: (DateTime, DateTime, DateTime),
        saved_min: Option<f64>,
    ) -> String;

    // ---- End of the outing.

    /// Recap of the outing: how it ended, km, moving and stopped minutes, share of the
    /// route followed, route km, real / model time, route changes, announcements, stops.
    #[allow(clippy::too_many_arguments)]
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
    ) -> String;
}
