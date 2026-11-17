//! Estimated time learned per group (C11, #130), on the plan's routes: the model of
//! [`skimap_core::calibration`] (moving pace by terrain, stops, detours) applied to a
//! route ([`PaceRoute`]), the morning range of the plan ([`PlanPace`], [`with_pace`]), what
//! the tracker proposes when the group is durably slower than planned ([`Proposal`]), and
//! the digest of a past outing for the learning ([`digest`], morning preparation E2).
//!
//! # Proposals, never imposed
//!
//! Once a minute the tracker predicts the arrival at the day's pace ([`crate::tracker::Eta`])
//! and compares it with the arrival planned for the group (the median of [`PlanPace`], or
//! of the prior of a lone adult when the plan has no profile). A proposal is made when, for
//! [`ProposeParams::durable_s`] in a row, the predicted arrival is late by at least
//! [`ProposeParams::min_delay_s`] and [`ProposeParams::delay_frac`] of the planned time
//! *and* the 10 % quantile of the prediction is late too (nine chances in ten that the
//! group is late), not before [`ProposeParams::min_elapsed_s`] of outing. Then
//! [`ProposeParams::spacing_s`] at least before the next one, and only if the delay grew by
//! [`ProposeParams::worse_s`] since the last one (unless it was accepted: the plan changed);
//! at most [`ProposeParams::max`] per
//! outing; none on a plan B (the route was already changed), nor while a switch can still
//! be refused, nor during a stop, nor when the group progressed less than
//! [`ProposeParams::recent_progress_m`] over its last [`ProposeParams::recent_moving_s`] of
//! moving (turned back, lost: its arrival on the loop means nothing).
//!
//! What is proposed, among the exits of the morning still ahead (plans B of J2), each
//! predicted at the day's pace: the **short loop** (the longest exit back by the planned
//! arrival, [`ProposeParams::slack_s`] of slack), else the **nearest exit** (the earliest
//! arrival, if it saves [`ProposeParams::min_saving_s`]), else **keep the loop** with the
//! new return time; nothing with less than [`ProposeParams::min_rest_s`] left. Accepted
//! ([`crate::tracker::Input::Answer`]): the switch to the exit (refusable afterwards like
//! any switch) or the new return time; refused: « Itinéraire initial conservé. ». Turning
//! back before nightfall stays the safety alert of J2 / A8.

use chrono::{Duration, NaiveDateTime, Timelike};
use serde::{Deserialize, Serialize};
use skimap_assist::alerts;
use skimap_core::calibration::{
    DAY_SIGMA_W, GroupProfile, Level, OutingObs, PaceParams, Prediction, ProfileModel, TERRAINS,
    Terrain, terrain_ratio,
};
use skimap_core::graph::PisteGraph;
use skimap_core::routing::Style;
use skimap_core::turnaround;
use skimap_voice::bank::{Announcement, Pause, Segment};
use skimap_voice::clips::{self, Fixed};
use skimap_voice::quantity::Clock;

use crate::course::Course;
use crate::error::Result;
use crate::plan::DayPlan;
use crate::track::TrackView;
use crate::tracker::{Input, RouteRef, Tracker, TrackerParams};
use skimap_common::Lang;

/// Model time per terrain along a route (cumulative), from the C5 time profile.
#[derive(Debug, Clone, Default)]
pub struct PaceRoute {
    /// Profile distances, metres.
    dist: Vec<f64>,
    /// Model time from the start per terrain, seconds.
    cum: Vec<[f64; TERRAINS]>,
    /// Profile distance per route distance.
    scale: f64,
}

impl PaceRoute {
    /// The route of a course; `ratio`: [`terrain_ratio`] of the plan's model.
    pub fn of(c: &Course, ratio: f64) -> Self {
        let p = c.profile();
        let n = p.dist_m.len();
        let mut cum = Vec::with_capacity(n);
        let mut acc = [0.0; TERRAINS];
        cum.push(acc);
        for i in 1..n {
            let out = p.out_s[i] - p.out_s[i - 1];
            let back = p.back_s[i] - p.back_s[i - 1];
            acc[Terrain::of_times(out, back, ratio) as usize] += out.max(0.0);
            cum.push(acc);
        }
        Self {
            dist: p.dist_m.clone(),
            cum,
            scale: c.profile_at(1.0),
        }
    }

    /// Model time from the start to a route distance per terrain, seconds (no allocation).
    #[inline]
    pub fn at(&self, s: f64) -> [f64; TERRAINS] {
        let n = self.dist.len();
        if n < 2 {
            return [0.0; TERRAINS];
        }
        let d = (s * self.scale).clamp(0.0, self.dist[n - 1]);
        let i = self.dist.partition_point(|x| *x <= d).clamp(1, n - 1);
        let (d0, d1) = (self.dist[i - 1], self.dist[i]);
        let f = if d1 > d0 { (d - d0) / (d1 - d0) } else { 0.0 };
        let (a, b) = (self.cum[i - 1], self.cum[i]);
        [
            a[0] + (b[0] - a[0]) * f,
            a[1] + (b[1] - a[1]) * f,
            a[2] + (b[2] - a[2]) * f,
        ]
    }

    /// Model time per terrain of the whole route, seconds.
    pub fn total(&self) -> [f64; TERRAINS] {
        self.cum.last().copied().unwrap_or([0.0; TERRAINS])
    }

    /// Model time per terrain between two route distances, seconds.
    #[inline]
    pub fn between(&self, a: f64, b: f64) -> [f64; TERRAINS] {
        let (x, y) = (self.at(a), self.at(b));
        [y[0] - x[0], y[1] - x[1], y[2] - x[2]]
    }

    /// Model time per terrain from a route distance to the end, seconds.
    pub fn rest(&self, s: f64) -> [f64; TERRAINS] {
        let (x, y) = (self.at(s), self.total());
        [
            (y[0] - x[0]).max(0.0),
            (y[1] - x[1]).max(0.0),
            (y[2] - x[2]).max(0.0),
        ]
    }
}

/// Outings learned below which the morning gives a range rather than a figure.
pub const FIGURE_AFTER_OUTINGS: u32 = 3;
/// Relative half-width of the 80 % interval below which a figure is given.
pub const FIGURE_SPREAD: f64 = 0.15;

/// The estimated time of the plan for a group (morning).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PlanPace {
    /// The group.
    pub profile: GroupProfile,
    /// Outings learned.
    pub outings: u32,
    /// Its parameters (prior or posterior): the tracker starts from them.
    pub params: PaceParams,
    /// Model time of the loop per terrain (C5), seconds.
    pub model_s: [f64; TERRAINS],
    /// Duration predicted for the group.
    pub prediction: Prediction,
    /// Arrival: median, 10 % and 90 % quantiles (local).
    pub arrival: NaiveDateTime,
    /// 10 % quantile.
    pub arrival_lo: NaiveDateTime,
    /// 90 % quantile.
    pub arrival_hi: NaiveDateTime,
    /// Quantile of the total time the group is planned on (`ProfileModel::plan_quantile`:
    /// the slow side of the range with children, else the median).
    pub plan_q: f64,
    /// Arrival planned on (the median, or the slow side with children): the return time
    /// said, the reference of the proposals.
    pub planned: NaiveDateTime,
    /// A range is shown (little history or a wide spread), else a figure.
    pub range: bool,
    /// Text, French.
    pub text: String,
}

fn secs(s: f64) -> Duration {
    Duration::milliseconds((s * 1000.0).round() as i64)
}

/// « 2 h 30 », « 45 min », rounded to 5 min.
pub fn duration_fr(s: f64) -> String {
    crate::text::of(Lang::Fr).duration5(s)
}

/// « 15 h 20 », rounded to 5 min.
pub fn clock_fr(t: NaiveDateTime) -> String {
    crate::text::of(Lang::Fr).clock5(t)
}

/// The estimated time of a plan for a group.
pub fn plan_pace(plan: &DayPlan, g: &PisteGraph, model: &ProfileModel) -> Result<PlanPace> {
    let c = Course::new(g, &plan.route.legs, &plan.eta)?;
    let route = PaceRoute::of(&c, terrain_ratio(&plan.eta));
    let model_s = route.total();
    let p = model.params.predict(model_s);
    let plan_q = model.plan_quantile();
    let planned_s = model.plan_s(&p);
    let dep = plan.departure;
    let range = model.outings < FIGURE_AFTER_OUTINGS || p.spread() > FIGURE_SPREAD;
    let c5 = c.model_s();
    let tx = crate::text::of(plan.lang);
    let learned = tx.pc_learned(model.outings);
    let duration = tx.pc_duration((!range).then_some(p.median_s), p.lo_s, p.hi_s);
    let back = if plan_q > 0.5 {
        // With children: planned on the slow side of the range, the median said too.
        tx.pc_slow_side(
            p.median_s,
            planned_s,
            (plan_q * 10.0).round(),
            dep + secs(planned_s),
        )
    } else if range {
        tx.pc_back_between(dep + secs(p.lo_s), dep + secs(p.hi_s))
    } else {
        tx.pc_back_around(dep + secs(p.median_s))
    };
    let text = tx.pc_text(
        &tx.assist().group_label(&model.profile),
        &learned,
        &duration,
        &back,
        p.moving_s,
        p.stop_s,
        c5,
    );
    Ok(PlanPace {
        profile: model.profile.clone(),
        outings: model.outings,
        params: model.params,
        model_s,
        prediction: p,
        arrival: dep + secs(p.median_s),
        arrival_lo: dep + secs(p.lo_s),
        arrival_hi: dep + secs(p.hi_s),
        plan_q,
        planned: dep + secs(planned_s),
        range,
        text,
    })
}

/// Sets the group of a plan: its estimated time (the tracker's starting point), its limit
/// to be back (C7 with the group's light margin: an hour with children,
/// `GroupProfile::turnaround_params`) and the latest turnaround of A8 at its pace
/// (`alerts::Monitor::for_model`: the loop's model profile at the time the group is planned
/// on, the slow side of its range with children; none without weather, as
/// [`crate::plan::build`]). The plans B stay those of the plan (computed by
/// [`crate::plan::build`] on the limit of the morning's group, if any).
pub fn with_pace(plan: &mut DayPlan, g: &PisteGraph, model: &ProfileModel) -> Result<()> {
    let pace = plan_pace(plan, g, model)?;
    let c = Course::new(g, &plan.route.legs, &plan.eta)?;
    let ta = &mut plan.turnaround;
    let tp = model.profile.turnaround_params();
    let dl = turnaround::deadline(plan.date, plan.request.start, &tp);
    let deadline = dl.map(|d| skimap_core::ephemeris::local_time(d.deadline));
    let limit = match (deadline, ta.return_before) {
        (Some(a), Some(b)) => Some(a.min(b)),
        (a, b) => a.or(b),
    };
    // The latest passages at the exits keep their margin to the limit.
    if let (Some(old), Some(new)) = (ta.limit, limit) {
        for (_, at) in &mut ta.latest_exit {
            *at += new - old;
        }
    }
    ta.deadline = deadline;
    ta.limit = limit;
    ta.margin_min =
        limit.map(|l| ((l - ta.arrival_with_margin).num_seconds() as f64 / 60.0).round());
    ta.latest_turnaround = plan.weather.as_ref().and_then(|w| {
        alerts::Monitor::for_model(
            plan.departure,
            c.profile(),
            std::borrow::Cow::Borrowed(w),
            alerts::Params::default(),
            model,
            terrain_ratio(&plan.eta),
        )
        .planned_turnaround()
    });
    plan.pace = Some(pace);
    Ok(())
}

/// The group of a plan as a model (what the tracker plans on).
pub fn plan_model(p: &PlanPace) -> ProfileModel {
    ProfileModel {
        profile: p.profile.clone(),
        params: p.params,
        outings: p.outings,
        plan_q: Some(p.plan_q),
    }
}

/// The parameters the tracker starts from: the plan's group, else a group nobody
/// described ([`PaceParams::unknown`]).
pub fn base_params(plan: &DayPlan) -> PaceParams {
    match &plan.pace {
        Some(p) => p.params,
        None => PaceParams::unknown(plan.request.style),
    }
}

/// Settings of the proposals (see the module).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct ProposeParams {
    /// Proposals made at all.
    pub enabled: bool,
    /// Not before this much outing, seconds.
    pub min_elapsed_s: f64,
    /// Not before this much progress, metres.
    pub min_progress_m: f64,
    /// Progress over the last [`ProposeParams::recent_moving_s`] of moving at least this,
    /// metres: a group that turned back or wanders off the plan gets no proposal (its
    /// arrival on the loop means nothing)…
    pub recent_progress_m: f64,
    /// …seconds of moving time.
    pub recent_moving_s: f64,
    /// Late by at least this, seconds…
    pub min_delay_s: f64,
    /// …and this share of the planned time…
    pub delay_frac: f64,
    /// …for this long, seconds.
    pub durable_s: f64,
    /// Back under this share of the threshold: the count restarts.
    pub clear_frac: f64,
    /// Between two proposals, seconds.
    pub spacing_s: f64,
    /// After a proposal (refused, or not answered), the delay must have grown by this for
    /// another one, seconds.
    pub worse_s: f64,
    /// Nothing is proposed with less than this left on the loop (almost there), seconds.
    pub min_rest_s: f64,
    /// Proposals per outing.
    pub max: u32,
    /// The short loop may end this late after the planned arrival, seconds.
    pub slack_s: f64,
    /// The nearest exit must save this, seconds.
    pub min_saving_s: f64,
    /// A proposal not answered lapses after this, seconds.
    pub ttl_s: f64,
}

impl Default for ProposeParams {
    fn default() -> Self {
        Self {
            enabled: true,
            min_elapsed_s: 1200.0,
            min_progress_m: 1000.0,
            recent_progress_m: 200.0,
            recent_moving_s: 900.0,
            min_delay_s: 900.0,
            delay_frac: 0.15,
            durable_s: 600.0,
            clear_frac: 0.5,
            spacing_s: 1800.0,
            worse_s: 1200.0,
            min_rest_s: 1800.0,
            max: 3,
            slack_s: 600.0,
            min_saving_s: 900.0,
            ttl_s: 900.0,
        }
    }
}

impl ProposeParams {
    /// With children: a shorter loop offered sooner than to adults (a choice of caution,
    /// not a measurement: children tire faster than their pace so far says, and an exit
    /// left behind is gone): late by two thirds of the thresholds (10 min and 10 % with the
    /// defaults), for half as long (5 min), from three quarters of the outing time (15
    /// min), and on the median (more likely late than not) rather than nine chances in ten.
    pub fn with_children(self) -> Self {
        Self {
            min_delay_s: self.min_delay_s * 2.0 / 3.0,
            delay_frac: self.delay_frac * 2.0 / 3.0,
            durable_s: self.durable_s * 0.5,
            min_elapsed_s: self.min_elapsed_s * 0.75,
            ..self
        }
    }
}

/// What is proposed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProposalKind {
    /// The longest exit back by the planned arrival.
    ShortLoop,
    /// The exit arriving first.
    NearestExit,
    /// The loop, with a new return time.
    Keep,
}

/// Why.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PaceReason {
    /// Slower moving.
    Slower,
    /// More stops.
    Pauses,
    /// Both.
    SlowerAndPauses,
}

/// A proposal of the tracker.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Proposal {
    /// Number in the outing, from 1.
    pub id: u32,
    /// What.
    pub kind: ProposalKind,
    /// Why.
    pub reason: PaceReason,
    /// The route proposed (`None`: keep the loop).
    pub route: Option<RouteRef>,
    /// Slower than planned to get here, percent (rounded to 5).
    pub slower_pct: f64,
    /// Stopped time so far, minutes, and planned for the moving time so far.
    pub stops_min: (f64, f64),
    /// Arrival planned (local).
    pub planned: NaiveDateTime,
    /// Arrival on the loop at the day's pace (median).
    pub loop_arrival: NaiveDateTime,
    /// Arrival with the proposal (median, 10 %, 90 %).
    pub arrival: NaiveDateTime,
    /// 10 % quantile.
    pub arrival_lo: NaiveDateTime,
    /// 90 % quantile.
    pub arrival_hi: NaiveDateTime,
    /// Delay of the loop on the planned arrival, minutes.
    pub delay_min: f64,
    /// Text, French (screen).
    pub text: String,
    /// What is said.
    pub announcement: Announcement,
}

/// Fixed phrases of the proposals in the phrase bank (`Fixed::PlanPace*`,
/// `Fixed::PlanPropose*`, `Fixed::PlanProposalAccepted`, ids `fix.plan-*`): the reason,
/// the first part of the proposal (completed by a `loop.back.*` clip), the acceptance.
pub const PROPOSAL_PHRASES: [Fixed; 7] = [
    Fixed::PlanPaceSlower,
    Fixed::PlanPaceSlowerPauses,
    Fixed::PlanPacePauses,
    Fixed::PlanProposeShort,
    Fixed::PlanProposeExit,
    Fixed::PlanProposeKeep,
    Fixed::PlanProposalAccepted,
];

/// « Vous allez moins vite que prévu, avec plus de pauses. Je vous propose la boucle
/// courte, retour vers quinze heures quinze. »: the reason, the proposal, the return time (the
/// bank's `loop.back.*` clips, to the nearest quarter hour).
pub fn proposal_announcement(
    reason: PaceReason,
    kind: ProposalKind,
    arrival: NaiveDateTime,
) -> Announcement {
    let r = match reason {
        PaceReason::Slower => Fixed::PlanPaceSlower,
        PaceReason::Pauses => Fixed::PlanPacePauses,
        PaceReason::SlowerAndPauses => Fixed::PlanPaceSlowerPauses,
    };
    let k = match kind {
        ProposalKind::ShortLoop => Fixed::PlanProposeShort,
        ProposalKind::NearestExit => Fixed::PlanProposeExit,
        ProposalKind::Keep => Fixed::PlanProposeKeep,
    };
    let c = Clock::nearest_quarter(arrival.hour() as u8, arrival.minute() as u8);
    Announcement {
        segments: vec![
            Segment {
                clip: r.clip(),
                pause: Pause::Long,
            },
            Segment {
                clip: k.clip(),
                pause: Pause::Short,
            },
            Segment {
                clip: clips::loop_back(c),
                pause: Pause::Long,
            },
        ],
    }
}

/// The screen text of a proposal: « Vous allez 40 % moins vite que prévu, avec plus de
/// pauses. Je vous propose la boucle courte, retour 15 h 20 (15 h 05 à 15 h 40). »
pub fn proposal_text(
    reason: PaceReason,
    slower_pct: f64,
    kind: ProposalKind,
    p: (NaiveDateTime, NaiveDateTime, NaiveDateTime),
    saved_min: Option<f64>,
) -> String {
    proposal_text_in(reason, slower_pct, kind, p, saved_min, Lang::Fr)
}

/// [`proposal_text`] in `lang` (the plan's language, VO6).
pub fn proposal_text_in(
    reason: PaceReason,
    slower_pct: f64,
    kind: ProposalKind,
    p: (NaiveDateTime, NaiveDateTime, NaiveDateTime),
    saved_min: Option<f64>,
    lang: Lang,
) -> String {
    let tx = crate::text::of(lang);
    tx.pr_text(
        &tx.pr_why(reason, slower_pct),
        tx.pr_what(&kind),
        p,
        saved_min,
    )
}

/// The digest of a past outing for the learning: its recorded inputs replayed through the
/// tracker (same segmentation as during the outing: moving, stops of 90 s or more).
pub fn digest(
    plan: &DayPlan,
    track: TrackView<'_>,
    g: &PisteGraph,
    inputs: &[Input],
) -> Result<OutingObs> {
    let params = TrackerParams {
        propose: ProposeParams {
            enabled: false,
            ..ProposeParams::default()
        },
        ..TrackerParams::default()
    };
    let mut tr = Tracker::new(plan, track, g, None, params)?;
    let mut out = Vec::new();
    for i in inputs {
        tr.push(i, &mut out);
        out.clear();
    }
    Ok(tr.pace_observation())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::phrases::never_safe;

    #[test]
    fn proposal_phrases_are_in_the_bank_and_never_safe() {
        for f in PROPOSAL_PHRASES {
            let c = f.clip();
            assert!(c.id.starts_with("fix.plan-"), "{}", c.id);
            let end = if f.continued() { ',' } else { '.' };
            assert!(c.text.ends_with(end), "{}", c.text);
            assert!(never_safe(&c.text), "{}", c.text);
        }
        let t = NaiveDateTime::parse_from_str("2026-01-09 15:20:00", "%Y-%m-%d %H:%M:%S").unwrap();
        let a = proposal_announcement(PaceReason::SlowerAndPauses, ProposalKind::ShortLoop, t);
        assert_eq!(
            a.text(),
            "Vous allez moins vite que prévu, avec plus de pauses. Je vous propose la boucle courte, retour vers quinze heures quinze."
        );
        let s = proposal_text(
            PaceReason::SlowerAndPauses,
            40.0,
            ProposalKind::ShortLoop,
            (t, t - Duration::minutes(15), t + Duration::minutes(20)),
            Some(45.0),
        );
        assert_eq!(
            s,
            "Vous allez 40 % moins vite que prévu, avec plus de pauses. Je vous propose la boucle courte, retour 15 h 20 (15 h 05 à 15 h 40), 45 min de moins. Accepter ou refuser."
        );
        assert!(never_safe(&s));
    }

    #[test]
    fn durations_and_clocks_in_french() {
        assert_eq!(duration_fr(2.5 * 3600.0), "2 h 30");
        assert_eq!(duration_fr(3.0 * 3600.0 + 13.0 * 60.0), "3 h 15");
        assert_eq!(duration_fr(42.0 * 60.0), "40 min");
        let t = NaiveDateTime::parse_from_str("2026-01-09 15:18:40", "%Y-%m-%d %H:%M:%S").unwrap();
        assert_eq!(clock_fr(t), "15 h 20");
    }
}

/// Settings of the proposal to choose another group profile (X14 #148).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct ProfileFitParams {
    /// Proposed at all.
    pub enabled: bool,
    /// Moving time of the day before the pace says anything, seconds.
    pub min_moving_s: f64,
    /// Progress along the route before, metres.
    pub min_progress_m: f64,
    /// The other profile explains the day this much better (log likelihood ratio: 3 is
    /// twenty times more likely).
    pub min_log_ratio: f64,
    /// The same when the group is durably late and a shorter way would be proposed (C11):
    /// the profile is asked first.
    pub min_log_ratio_late: f64,
    /// Not answered after this long, the proposal lapses, seconds.
    pub ttl_s: f64,
}

impl Default for ProfileFitParams {
    fn default() -> Self {
        Self {
            enabled: true,
            min_moving_s: 900.0,
            min_progress_m: 1000.0,
            min_log_ratio: 3.0,
            min_log_ratio_late: 0.5,
            ttl_s: 180.0,
        }
    }
}

/// The profiles the tracker compares the day with when the app gives none
/// ([`crate::tracker::Tracker::with_profiles`]): a lone adult of each level, the family the
/// app offers first ([`GroupProfile::app_default`]) and a family with a child of 6, in the
/// technique of the plan.
pub fn profile_presets(style: Style) -> Vec<ProfileModel> {
    let solo = |level, name: &str| GroupProfile {
        name: name.into(),
        ..GroupProfile::solo(style, level)
    };
    let mut family = GroupProfile::app_default();
    family.style = style;
    let young = GroupProfile {
        name: "Famille, enfant de 6 ans, débutants".into(),
        adults: 2,
        children: vec![6],
        style,
        level: Level::Beginner,
    };
    [
        solo(Level::Beginner, "Adulte débutant"),
        solo(Level::Intermediate, "Adulte"),
        solo(Level::Expert, "Adulte confirmé"),
        family,
        young,
    ]
    .into_iter()
    .map(ProfileModel::new)
    .collect()
}

/// How well a group's parameters explain the day so far (log likelihood, up to a constant
/// common to every profile): the moving time against the time the group would take for the
/// model time progressed per terrain (log-normal: the group's predictive spread and the
/// day's own), and the stops of 90 s or more (Poisson at the group's rate).
pub fn day_fit(p: &PaceParams, model_done: [f64; TERRAINS], moving_s: f64, stops: u32) -> f64 {
    let (mut pred, mut var_w) = (0.0, 0.0);
    for (m, n) in model_done.iter().zip(&p.pace) {
        let t = m.max(0.0) * (-n.m).exp();
        pred += t;
        var_w += t * n.pred_var();
    }
    if pred <= 0.0 || moving_s <= 0.0 {
        return 0.0;
    }
    let var = var_w / pred + DAY_SIGMA_W * DAY_SIGMA_W * 600.0 / moving_s.max(600.0);
    let z = (moving_s / pred).ln();
    let pace = -0.5 * z * z / var - 0.5 * var.ln();
    let mu = p.stops.mean().max(1e-3) * moving_s / 3600.0;
    let stops = f64::from(stops) * mu.ln() - mu;
    pace + stops
}

/// A proposal to choose another group profile (X14): the pace of the day fits it better
/// than the one planned on. Never imposed: answered by
/// [`crate::tracker::Input::ProfileAnswer`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProfileProposal {
    /// Index in the profiles compared ([`crate::tracker::Tracker::profiles`]).
    pub index: usize,
    /// The profile proposed.
    pub profile: GroupProfile,
    /// How much better it explains the day (log likelihood ratio).
    pub log_ratio: f64,
    /// Text, French (screen).
    pub text: String,
    /// What is said (`Fixed::PlanProfileMismatch`).
    pub announcement: Announcement,
}

/// The phrase of the profile proposal in the bank (`fix.plan-profile-mismatch`, X14).
pub const PROFILE_PHRASES: [Fixed; 1] = [Fixed::PlanProfileMismatch];

/// « Votre rythme ressemble à celui du profil « Famille, enfant de 6 ans ». Voulez-vous le
/// choisir ? »
pub fn profile_text(p: &GroupProfile) -> String {
    format!(
        "Votre rythme ressemble à celui du profil « {} ». Voulez-vous le choisir ? Accepter ou refuser.",
        p.name
    )
}
