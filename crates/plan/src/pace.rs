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
use skimap_core::calibration::{
    GroupProfile, OutingObs, PaceParams, Prediction, ProfileModel, TERRAINS, Terrain, terrain_ratio,
};
use skimap_core::graph::PisteGraph;
use skimap_voice::bank::{Announcement, Pause, Segment};
use skimap_voice::clips::{self, Fixed};
use skimap_voice::quantity::Clock;

use crate::course::Course;
use crate::error::Result;
use crate::plan::DayPlan;
use crate::track::TrackView;
use crate::tracker::{Input, RouteRef, Tracker, TrackerParams};

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
    let m = ((s / 60.0 / 5.0).round() * 5.0) as i64;
    if m < 60 {
        format!("{m} min")
    } else {
        format!("{} h {:02}", m / 60, m % 60)
    }
}

/// « 15 h 20 », rounded to 5 min.
pub fn clock_fr(t: NaiveDateTime) -> String {
    let m = (t.hour() * 60 + t.minute()) as f64 + t.second() as f64 / 60.0;
    let m = ((m / 5.0).round() * 5.0) as u32 % (24 * 60);
    format!("{} h {:02}", m / 60, m % 60)
}

/// The estimated time of a plan for a group.
pub fn plan_pace(plan: &DayPlan, g: &PisteGraph, model: &ProfileModel) -> Result<PlanPace> {
    let c = Course::new(g, &plan.route.legs, &plan.eta)?;
    let route = PaceRoute::of(&c, terrain_ratio(&plan.eta));
    let model_s = route.total();
    let p = model.params.predict(model_s);
    let dep = plan.departure;
    let range = model.outings < FIGURE_AFTER_OUTINGS || p.spread() > FIGURE_SPREAD;
    let c5 = c.model_s();
    let learned = match model.outings {
        0 => "a priori, aucune sortie apprise".to_owned(),
        1 => "une sortie apprise".to_owned(),
        n => format!("{n} sorties apprises"),
    };
    let duration = if range {
        format!("{} à {}", duration_fr(p.lo_s), duration_fr(p.hi_s))
    } else {
        format!(
            "environ {} ({} à {})",
            duration_fr(p.median_s),
            duration_fr(p.lo_s),
            duration_fr(p.hi_s)
        )
    };
    let back = if range {
        format!(
            "retour entre {} et {}",
            clock_fr(dep + secs(p.lo_s)),
            clock_fr(dep + secs(p.hi_s))
        )
    } else {
        format!("retour vers {}", clock_fr(dep + secs(p.median_s)))
    };
    let text = format!(
        "Temps estimé pour {} ({learned}) : {duration}, {back} ; en mouvement {} au rythme du groupe, arrêts {} ; modèle C5 seul : {}",
        model.profile.label_fr(),
        duration_fr(p.moving_s),
        duration_fr(p.stop_s),
        duration_fr(c5)
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
        range,
        text,
    })
}

/// Sets the group of a plan (its estimated time, the tracker's starting point).
pub fn with_pace(plan: &mut DayPlan, g: &PisteGraph, model: &ProfileModel) -> Result<()> {
    plan.pace = Some(plan_pace(plan, g, model)?);
    Ok(())
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

/// « Vous allez moins vite que prévu, avec plus de pauses. Je propose la boucle courte,
/// retour vers quinze heures et quart. »: the reason, the proposal, the return time (the
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
/// pauses. Je propose la boucle courte, retour 15 h 20 (15 h 05 à 15 h 40). »
pub fn proposal_text(
    reason: PaceReason,
    slower_pct: f64,
    kind: ProposalKind,
    p: (NaiveDateTime, NaiveDateTime, NaiveDateTime),
    saved_min: Option<f64>,
) -> String {
    let why = match reason {
        PaceReason::Slower => format!("Vous allez {slower_pct:.0} % moins vite que prévu."),
        PaceReason::SlowerAndPauses => {
            format!("Vous allez {slower_pct:.0} % moins vite que prévu, avec plus de pauses.")
        }
        PaceReason::Pauses => {
            format!("Vous faites plus de pauses que prévu ({slower_pct:.0} % moins vite au total).")
        }
    };
    let what = match kind {
        ProposalKind::ShortLoop => "Je propose la boucle courte",
        ProposalKind::NearestExit => "Je propose la sortie la plus proche",
        ProposalKind::Keep => "Je propose de garder la boucle",
    };
    let saved = saved_min
        .filter(|m| *m >= 5.0)
        .map(|m| format!(", {} de moins", duration_fr(m * 60.0)))
        .unwrap_or_default();
    format!(
        "{why} {what}, retour {} ({} à {}){saved}. Accepter ou refuser.",
        clock_fr(p.0),
        clock_fr(p.1),
        clock_fr(p.2)
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
            "Vous allez moins vite que prévu, avec plus de pauses. Je propose la boucle courte, retour vers quinze heures et quart."
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
            "Vous allez 40 % moins vite que prévu, avec plus de pauses. Je propose la boucle courte, retour 15 h 20 (15 h 05 à 15 h 40), 45 min de moins. Accepter ou refuser."
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
