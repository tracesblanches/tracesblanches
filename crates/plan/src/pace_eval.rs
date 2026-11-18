//! Evaluation of the estimated time learned per group (C11, #130) on synthetic outings of
//! the scenario bench ([`crate::scenario`], [`Behaviour`]): a group skis `days` outings of
//! the same plan, one a day ([`Behaviour::day`]: its pace, stops and detours vary from day
//! to day). Each day is predicted
//!
//! - in the morning: C5 alone, C5 with the margin of C7 (× 1.2), the prior of the group,
//!   the group learned on the days before (leave-future-out cross-validation: learn on N
//!   outings, test on the next);
//! - during the outing, at 25, 50 and 75 % of the route: C5 for the rest, the observed pace
//!   extrapolated (elapsed / model time done, as A8 and J2), C11 online from the prior, C11
//!   online from the group learned on the days before, and C11 online on the plan without a
//!   group (X14: nobody described the group; the tracker compares the day with the
//!   profiles of [`pace::profile_presets`]).
//!
//! The day's outing is simulated once in closed loop (proposals off: the route stays the
//! plan's), its inputs replayed through the trackers of each variant. Synthetic traces
//! only: the real validation waits for real outings (T1).

use serde::Serialize;
use skimap_core::calibration::{self, GroupProfile, OutingObs, ProfileModel};
use skimap_core::graph::PisteGraph;

use crate::course::Course;
use crate::error::Result;
use crate::pace::{self, ProposeParams};
use crate::plan::DayPlan;
use crate::scenario::{self, Behaviour, Scenario, Step};
use crate::session::{End, SessionEvent};
use crate::track::TrackView;
use crate::tracker::{Input, Output, Tracker, TrackerParams};

/// A group on a plan.
#[derive(Debug, Clone)]
pub struct Case<'a> {
    /// Name.
    pub name: String,
    /// Plan (without a group).
    pub plan: &'a DayPlan,
    /// Its flat table.
    pub track: &'a [u8],
    /// Graph.
    pub graph: &'a PisteGraph,
    /// How the group skis.
    pub behaviour: Behaviour,
    /// The group as saved on the phone.
    pub profile: GroupProfile,
    /// Outings.
    pub days: u64,
    /// Seed of the GPS noise of day 0.
    pub seed: u64,
}

/// Predicted durations of an outing (from its start), seconds.
#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize)]
pub struct Sample {
    /// Share of the route done (0: the morning).
    pub frac: f64,
    /// Time from the start, seconds.
    pub t_s: f64,
    /// C5 alone: the elapsed time plus the model time of the rest.
    pub c5_s: f64,
    /// C5 with the margin of C7 (× 1.2) on the rest.
    pub c5_margin_s: f64,
    /// Observed pace extrapolated (elapsed / model time done × model time left), the morning
    /// C5.
    pub pace_s: f64,
    /// C11 from the prior of the group.
    pub prior_s: f64,
    /// Within its 80 % interval.
    pub prior_in: bool,
    /// C11 from the group learned on the days before.
    pub learned_s: f64,
    /// Within its 80 % interval.
    pub learned_in: bool,
    /// Its 80 % interval (from the start), seconds.
    pub learned_range: (f64, f64),
    /// C11 on the plan without a group (X14; the morning: C5).
    pub unknown_s: f64,
    /// Within its 80 % interval.
    pub unknown_in: bool,
}

/// One outing.
#[derive(Debug, Clone, Serialize)]
pub struct Day {
    /// Day.
    pub day: u64,
    /// Outings learned before it.
    pub learned_on: usize,
    /// Real duration, from the start to the arrival, seconds.
    pub actual_s: f64,
    /// Predictions in the morning and at 25, 50, 75 %.
    pub samples: Vec<Sample>,
    /// The route changed (not counted).
    pub switched: bool,
    /// What it teaches.
    pub obs: OutingObs,
    /// The morning text of the plan with the group learned on the days before.
    pub morning_text: String,
    /// Cost of its digest (replay of its inputs through the tracker), milliseconds.
    pub digest_ms: f64,
    /// Inputs replayed.
    pub inputs: usize,
}

fn started_finished(out: &[Output]) -> (Option<f64>, Option<f64>) {
    let mut s = None;
    let mut f = None;
    for o in out {
        if let Output::Session { event, .. } = o {
            match event {
                SessionEvent::Started { t, .. } => s = s.or(Some(*t)),
                SessionEvent::Finished {
                    t,
                    end: End::Arrived,
                } => f = f.or(Some(*t)),
                _ => {}
            }
        }
    }
    (s, f)
}

fn quiet() -> TrackerParams {
    TrackerParams {
        propose: ProposeParams {
            enabled: false,
            ..ProposeParams::default()
        },
        ..TrackerParams::default()
    }
}

/// Evaluates a case, day after day.
pub fn evaluate(c: &Case<'_>) -> Result<Vec<Day>> {
    let view = TrackView::new(c.track)?;
    let course = Course::new(c.graph, &c.plan.route.legs, &c.plan.eta)?;
    let model_total = course.model_s();
    let len = course.length_m();
    let margin = skimap_core::turnaround::TurnaroundParams::default().eta_factor;
    let mut days = Vec::new();
    let mut learned: Vec<OutingObs> = Vec::new();
    for k in 0..c.days {
        let sc = Scenario {
            name: format!("{} jour {k}", c.name),
            start: scenario::departure_utc(c.plan),
            slowdown: 1.0,
            steps: vec![Step::WaitAtStart { secs: 60.0 }],
            sigma_m: 10.0,
            forest: true,
            seed: c.seed + k,
            tail_s: 300.0,
            behaviour: Some(c.behaviour.day(k)),
            step_counter: true,
        };
        let run = scenario::run(&sc, c.plan, view, c.graph, quiet())?;
        let (s0, s1) = started_finished(&run.outputs);
        let switched = run
            .outputs
            .iter()
            .any(|o| matches!(o, Output::Switched { .. }));
        let clock = std::time::Instant::now();
        let obs = pace::digest(c.plan, view, c.graph, &run.inputs)?;
        let digest_ms = clock.elapsed().as_secs_f64() * 1e3;
        let (Some(t0), Some(t1)) = (s0, s1) else {
            learned.push(obs);
            continue;
        };
        let actual = t1 - t0;
        let prior = ProfileModel::new(c.profile.clone());
        let model = calibration::fit(c.profile.clone(), &learned);
        let mut plan_prior = c.plan.clone();
        pace::with_pace(&mut plan_prior, c.graph, &prior)?;
        let mut plan_learned = c.plan.clone();
        pace::with_pace(&mut plan_learned, c.graph, &model)?;
        let (pp, pl) = (
            plan_prior.pace.as_ref().expect("set"),
            plan_learned.pace.as_ref().expect("set"),
        );
        let inside = |x: f64, p: &calibration::Prediction| (p.lo_s..=p.hi_s).contains(&x);
        let mut samples = vec![Sample {
            frac: 0.0,
            t_s: 0.0,
            c5_s: model_total,
            c5_margin_s: model_total * margin,
            pace_s: model_total,
            prior_s: pp.prediction.median_s,
            prior_in: inside(actual, &pp.prediction),
            learned_s: pl.prediction.median_s,
            learned_in: inside(actual, &pl.prediction),
            learned_range: (pl.prediction.lo_s, pl.prediction.hi_s),
            unknown_s: model_total,
            unknown_in: false,
        }];
        // Online, both variants on the same inputs.
        let mut a = Tracker::new(&plan_prior, view, c.graph, None, quiet())?;
        let mut b = Tracker::new(&plan_learned, view, c.graph, None, quiet())?;
        let mut u = Tracker::new(c.plan, view, c.graph, None, quiet())?;
        let mut out = Vec::new();
        let mut next = 0;
        let fracs = [0.25, 0.5, 0.75];
        let mut replay_switched = false;
        for i in &run.inputs {
            a.push(i, &mut out);
            b.push(i, &mut out);
            u.push(i, &mut out);
            replay_switched |= out.iter().any(|o| matches!(o, Output::Switched { .. }));
            out.clear();
            let Input::Fix { t, .. } = i else { continue };
            if next >= fracs.len() || b.progress() < fracs[next] * len {
                continue;
            }
            let t = t.timestamp_millis() as f64 / 1000.0;
            let (Some(ea), Some(eb), Some(eu)) = (a.eta(), b.eta(), u.eta()) else {
                continue;
            };
            let el = t - t0;
            let done = course.model_s_at(b.progress());
            let rest = (model_total - done).max(0.0);
            let total = |p: &calibration::Prediction| el + p.median_s;
            let within =
                |p: &calibration::Prediction| (el + p.lo_s..=el + p.hi_s).contains(&actual);
            samples.push(Sample {
                frac: fracs[next],
                t_s: el,
                c5_s: el + rest,
                c5_margin_s: el + margin * rest,
                pace_s: if done > 60.0 {
                    el + el / done * rest
                } else {
                    el + rest
                },
                prior_s: total(&ea.remaining),
                prior_in: within(&ea.remaining),
                learned_s: total(&eb.remaining),
                learned_in: within(&eb.remaining),
                learned_range: (el + eb.remaining.lo_s, el + eb.remaining.hi_s),
                unknown_s: total(&eu.remaining),
                unknown_in: within(&eu.remaining),
            });
            next += 1;
        }
        days.push(Day {
            day: k,
            learned_on: learned.len(),
            actual_s: actual,
            samples,
            switched: switched || replay_switched,
            obs: obs.clone(),
            morning_text: pl.text.clone(),
            digest_ms,
            inputs: run.inputs.len(),
        });
        learned.push(obs);
    }
    Ok(days)
}

/// Mean absolute error in minutes and coverage of a method at a share of the route, over
/// days that learned on at least `min_learned` outings (and kept their route).
#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize)]
pub struct Score {
    /// Share of the route.
    pub frac: f64,
    /// Days counted.
    pub n: usize,
    /// Mean absolute errors, minutes: C5 alone, C5 × 1.2, pace extrapolated, C11 prior,
    /// C11 learned.
    pub mae_min: [f64; 5],
    /// Mean absolute relative errors.
    pub mare: [f64; 5],
    /// Coverage of the 80 % intervals: prior, learned.
    pub cover: [f64; 2],
    /// C11 on the plan without a group (X14): mean absolute error and mean error (predicted
    /// − real), minutes, and coverage of its 80 % interval.
    pub unknown: (f64, f64, f64),
}

/// Scores at each share of the route.
pub fn score(days: &[Day], min_learned: usize) -> Vec<Score> {
    let mut out = Vec::new();
    for frac in [0.0, 0.25, 0.5, 0.75] {
        let mut s = Score {
            frac,
            ..Score::default()
        };
        for d in days
            .iter()
            .filter(|d| !d.switched && d.learned_on >= min_learned)
        {
            let Some(x) = d.samples.iter().find(|x| (x.frac - frac).abs() < 1e-9) else {
                continue;
            };
            s.n += 1;
            let v = [x.c5_s, x.c5_margin_s, x.pace_s, x.prior_s, x.learned_s];
            for (k, p) in v.iter().enumerate() {
                s.mae_min[k] += (p - d.actual_s).abs() / 60.0;
                s.mare[k] += (p - d.actual_s).abs() / d.actual_s;
            }
            s.unknown.0 += (x.unknown_s - d.actual_s).abs() / 60.0;
            s.unknown.1 += (x.unknown_s - d.actual_s) / 60.0;
            s.unknown.2 += f64::from(u8::from(x.unknown_in));
            s.cover[0] += f64::from(u8::from(x.prior_in));
            s.cover[1] += f64::from(u8::from(x.learned_in));
        }
        if s.n > 0 {
            let n = s.n as f64;
            for k in 0..5 {
                s.mae_min[k] /= n;
                s.mare[k] /= n;
            }
            s.cover[0] /= n;
            s.cover[1] /= n;
            s.unknown = (s.unknown.0 / n, s.unknown.1 / n, s.unknown.2 / n);
        }
        out.push(s);
    }
    out
}

/// Lines of a table of scores (French).
pub fn table(name: &str, days: &[Day], min_learned: usize) -> Vec<String> {
    let mut l = vec![format!(
        "{name} : {} sorties (durée réelle {} à {} min), apprises sur au moins {min_learned} sorties précédentes",
        days.len(),
        days.iter()
            .map(|d| (d.actual_s / 60.0).round() as i64)
            .min()
            .unwrap_or(0),
        days.iter()
            .map(|d| (d.actual_s / 60.0).round() as i64)
            .max()
            .unwrap_or(0),
    )];
    l.push(
        "  part faite | n | erreur moyenne (min) C5 seul / C5 × 1,2 / rythme extrapolé / C11 a priori / C11 appris | couverture 80 % a priori / appris | sans profil : erreur moyenne absolue / signée (min), couverture"
            .into(),
    );
    for s in score(days, min_learned) {
        l.push(format!(
            "  {:>3.0} % | {} | {:.0} / {:.0} / {:.0} / {:.0} / {:.0} | {:.0} % / {:.0} % | {:.1} / {:+.1} / {:.0} %",
            s.frac * 100.0,
            s.n,
            s.mae_min[0],
            s.mae_min[1],
            s.mae_min[2],
            s.mae_min[3],
            s.mae_min[4],
            s.cover[0] * 100.0,
            s.cover[1] * 100.0,
            s.unknown.0,
            s.unknown.1,
            s.unknown.2 * 100.0
        ));
    }
    l
}
