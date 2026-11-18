//! Pace learned per group (C11 #130, base of the personal calibration C6 #38): the C5
//! model ([`crate::eta`]) gives the time *moving* on every piece of a route; a group (« seul
//! », « en famille », « avec X ») moves at its own pace relative to it, stops, plays and
//! wanders. This module holds the per-group parameters, their priors, their conjugate
//! Bayesian update on past outings (offline, morning preparation E2) and on the outing of
//! the day (online, no allocation), and the prediction of a duration with its range.
//!
//! # Model
//!
//! Time of a route = moving time + stopped time.
//!
//! - **Moving**: on terrain `c` (uphill, flat, downhill: the classes of
//!   [`crate::eta::Breakdown`], grade beyond ±2 %), the group covers the route at
//!   `exp(θ_c)` times the C5 model pace. `θ_c` is the *progress* pace: it includes the
//!   detours and explorations (distance skied that does not advance along the route), kept
//!   apart as `ρ = ln(skied / progress)` for display ([`PaceParams::detour`]); the moving
//!   speed itself is `θ_c + ρ`.
//! - **Stops** (at least 90 s, the pause rule of the tracker J3): a Poisson process in
//!   moving time of rate `λ` per hour ([`GammaRate`]), log-normal durations of log-mean
//!   `μ_D` ([`PaceParams::stop_len`]).
//!
//! # Learning
//!
//! - Across outings (offline): one observation per outing and terrain, `y = ln(model time
//!   progressed / real moving time)` when the terrain has 10 min of moving time; a
//!   normal–inverse-gamma prior per terrain ([`Nig`]) gives the mean pace of the group and
//!   its day-to-day spread, and the Student-t predictive for the next outing. Stops: gamma
//!   prior on `λ` with the counts and moving hours of each outing; normal–inverse-gamma on
//!   the log durations. Robust: every observation is weighted by Huber's weight
//!   (`min(1, 2 / |z|)`, `z` its standardised residual against the current predictive), a
//!   weighted (power-likelihood) conjugate update.
//! - During the outing (online, [`DayPace`]): a day effect `δ` shared by the terrains,
//!   prior `N(0, V₀)` with `V₀` the predictive variance of the group, observed through the
//!   ratio of the time predicted at the group's pace to the real moving time, both summed
//!   with a forgetting of [`DAY_TAU_S`] (the pace drifts: tired children); stops of the
//!   day update the group's gamma and duration priors, capped at [`DAY_PRIOR_H`] hours and
//!   [`DAY_PRIOR_STOPS`] stops of weight so that the day can move them. A group nobody
//!   described (X14) takes as stop priors the mixture of the profiles it may be, weighted
//!   by how well each explains the day so far ([`stop_mixture`],
//!   [`DayPace::set_stop_prior`]): a slow group with children is not predicted with the
//!   stops of a lone adult, nor a lone adult with those of a family.
//!
//! # Prediction and range
//!
//! Moving time log-normal (terrains fully correlated: a slow day is slow everywhere);
//! stopped time a compound Poisson sum with the uncertainty of `λ` (plus a day-to-day
//! spread of [`STOP_RATE_DAY_CV`]) and of `μ_D`; the total is moment-matched to a
//! log-normal and given with its 80 % interval ([`Prediction::lo_s`], [`Prediction::hi_s`]).
//!
//! # Priors
//!
//! See [`prior_row`]. Only the shape by terrain of the children's pace is from a
//! publication (Sollie et al. 2021, Scand J Med Sci Sports, PMID 33231327: competitive
//! skiers of 14 years 13 % slower than adults, 19 % uphill, 11 % on undulating terrain, 8 %
//! downhill). No publication gives the speed of young children skiing for leisure, their
//! stops or their detours. The rows of children of 8 and more are set on one testimony
//! ([`TESTIMONY`], n = 1, not a study); the others are assumptions. All are wide
//! (predictive standard deviation 0.30 to 0.45 in log) and marked as such, to be replaced
//! by the groups' own outings (and checked on real outings, T1).
//!
//! # Caution with children (choices, not measurements)
//!
//! A group with children is planned on the slow side of its range
//! ([`ProfileModel::plan_quantile`]: the 80 % quantile of the total time,
//! [`PLAN_Q_CHILDREN`]) rather than its median, keeps a larger light margin
//! ([`GroupProfile::turnaround_params`]: back [`CHILD_LIGHT_MARGIN_MIN`] minutes before the
//! light limit instead of 30) and is offered a shorter loop sooner during the outing
//! (`skimap_plan::pace`).

use serde::{Deserialize, Serialize};

use crate::eta::{EtaModel, FLAT_GRADE};
use crate::routing::Style;
use crate::turnaround::TurnaroundParams;

/// Terrain classes (index of the arrays of model times): uphill, flat, downhill.
pub const TERRAINS: usize = 3;

/// Terrain class of a piece of route.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Terrain {
    /// Grade above +2 %.
    Up = 0,
    /// Between −2 % and +2 %.
    Flat = 1,
    /// Grade below −2 %.
    Down = 2,
}

impl Terrain {
    /// The three classes, in index order.
    pub const ALL: [Terrain; TERRAINS] = [Terrain::Up, Terrain::Flat, Terrain::Down];

    /// Class of a grade (the bounds of [`crate::eta::Breakdown`]).
    pub fn of_grade(grade: f64) -> Self {
        if grade > FLAT_GRADE {
            Terrain::Up
        } else if grade < -FLAT_GRADE {
            Terrain::Down
        } else {
            Terrain::Flat
        }
    }

    /// Class of a piece from its model times out and back (a time profile without
    /// elevations): uphill when `out / back` exceeds `ratio` ([`terrain_ratio`], the ratio
    /// at +2 %), downhill below `1 / ratio`.
    pub fn of_times(out_s: f64, back_s: f64, ratio: f64) -> Self {
        if out_s <= 0.0 || back_s <= 0.0 {
            return Terrain::Flat;
        }
        let r = out_s / back_s;
        if r > ratio {
            Terrain::Up
        } else if r < 1.0 / ratio {
            Terrain::Down
        } else {
            Terrain::Flat
        }
    }

    /// French name.
    pub fn fr(self) -> &'static str {
        match self {
            Terrain::Up => "montée",
            Terrain::Flat => "plat",
            Terrain::Down => "descente",
        }
    }
}

/// Ratio of the model times out and back of a +2 % piece, `v(−2 %) / v(+2 %)`: the bound
/// of [`Terrain::of_times`] for a model (speeds are monotonic in the grade).
pub fn terrain_ratio(eta: &EtaModel) -> f64 {
    let up = eta.speed_ms(FLAT_GRADE, eta.default_elev_m, false);
    let down = eta.speed_ms(-FLAT_GRADE, eta.default_elev_m, false);
    if up > 0.0 { down / up } else { 1.0 }
}

/// Technical level of a group (its slowest member).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Level {
    /// A few outings a season.
    Beginner,
    /// Regular.
    Intermediate,
    /// Trained.
    Expert,
}

/// A group profile saved on the phone (« seul », « en famille », « avec X »).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GroupProfile {
    /// Name given by the user.
    pub name: String,
    /// Adults.
    pub adults: u8,
    /// Ages of the children, years.
    pub children: Vec<u8>,
    /// Technique.
    pub style: Style,
    /// Level of the adults.
    pub level: Level,
}

impl GroupProfile {
    /// One adult.
    pub fn solo(style: Style, level: Level) -> Self {
        Self {
            name: "seul".into(),
            adults: 1,
            children: Vec::new(),
            style,
            level,
        }
    }

    /// Adults with children of these ages.
    pub fn family(adults: u8, children: &[u8], style: Style) -> Self {
        Self {
            name: "en famille".into(),
            adults,
            children: children.to_vec(),
            style,
            level: Level::Intermediate,
        }
    }

    /// The profile the app offers first (C11): « Famille avec enfants débutants »,
    /// a family with beginner children, classic (the group of
    /// [`TESTIMONY`]).
    pub fn app_default() -> Self {
        Self {
            name: "Famille avec enfants débutants".into(),
            adults: 2,
            children: vec![10, 14],
            style: Style::Classic,
            level: Level::Beginner,
        }
    }

    /// Age of the youngest child.
    pub fn youngest(&self) -> Option<u8> {
        self.children.iter().copied().min()
    }

    /// The group has children.
    pub fn has_children(&self) -> bool {
        !self.children.is_empty()
    }

    /// Light limit and margins of the turnaround (C7) for the group: with children, back
    /// [`CHILD_LIGHT_MARGIN_MIN`] minutes before the light limit instead of 30 (a choice of
    /// caution: see the module).
    pub fn turnaround_params(&self) -> TurnaroundParams {
        let mut t = TurnaroundParams::default();
        if self.has_children() {
            t.margin_min = CHILD_LIGHT_MARGIN_MIN;
        }
        t
    }

    /// Description, French.
    pub fn label_fr(&self) -> String {
        let level = match self.level {
            Level::Beginner => "débutant",
            Level::Intermediate => "intermédiaire",
            Level::Expert => "confirmé",
        };
        let style = match self.style {
            Style::Classic => "classique",
            Style::Skating => "skating",
        };
        let mut ages: Vec<u8> = self.children.clone();
        ages.sort_unstable();
        let words: Vec<String> = ages.iter().map(u8::to_string).collect();
        let kids = match words.as_slice() {
            [] => String::new(),
            [one] => format!(", un enfant de {one} ans"),
            [rest @ .., last] => format!(", enfants de {} et {last} ans", rest.join(", ")),
        };
        format!(
            "« {} » : {} adulte{}{kids}, {style}, niveau {level}",
            self.name,
            self.adults,
            if self.adults > 1 { "s" } else { "" }
        )
    }
}

/// Normal–inverse-gamma distribution of a log quantity: mean `m`, strength `kappa`
/// (observations' worth), shape `alpha`, scale `beta` of the variance between observations.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Nig {
    /// Mean.
    pub m: f64,
    /// Strength of the mean, observations.
    pub kappa: f64,
    /// Shape of the variance.
    pub alpha: f64,
    /// Scale of the variance.
    pub beta: f64,
}

impl Nig {
    /// A prior of mean `mean` whose predictive (one new observation) has standard deviation
    /// `pred_sd`, worth one observation (`kappa` 1, `alpha` 3).
    pub fn prior(mean: f64, pred_sd: f64) -> Self {
        let (kappa, alpha) = (1.0, 3.0);
        Self {
            m: mean,
            kappa,
            alpha,
            // pred_var = beta (kappa + 1) / (kappa (alpha − 1)).
            beta: pred_sd * pred_sd * kappa * (alpha - 1.0) / (kappa + 1.0),
        }
    }

    /// Expected variance between observations, `E[σ²] = beta / (alpha − 1)`.
    pub fn sigma2(&self) -> f64 {
        self.beta / (self.alpha - 1.0).max(0.5)
    }

    /// Variance of the mean.
    pub fn mean_var(&self) -> f64 {
        self.sigma2() / self.kappa
    }

    /// Variance of the predictive of one new observation (Student-t, `2 alpha` degrees of
    /// freedom).
    pub fn pred_var(&self) -> f64 {
        self.sigma2() * (self.kappa + 1.0) / self.kappa
    }

    /// Standardised residual of an observation against the predictive.
    pub fn z(&self, y: f64) -> f64 {
        (y - self.m) / self.pred_var().sqrt()
    }

    /// Conjugate update with an observation of weight `w` (1: a full observation).
    pub fn update(&mut self, y: f64, w: f64) {
        if !y.is_finite() || w <= 0.0 {
            return;
        }
        let k1 = self.kappa + w;
        self.beta += 0.5 * w * self.kappa * (y - self.m).powi(2) / k1;
        self.m = (self.kappa * self.m + w * y) / k1;
        self.kappa = k1;
        self.alpha += 0.5 * w;
    }

    /// Conjugate update with `n` observations of mean `mean` and sum of squared deviations
    /// `ss` (same result as `n` calls of [`Nig::update`] with weight 1).
    pub fn update_batch(&mut self, n: f64, mean: f64, ss: f64) {
        if n <= 0.0 {
            return;
        }
        let k1 = self.kappa + n;
        self.beta += 0.5 * ss + 0.5 * self.kappa * n * (mean - self.m).powi(2) / k1;
        self.m = (self.kappa * self.m + n * mean) / k1;
        self.kappa = k1;
        self.alpha += 0.5 * n;
    }

    /// The same mean and variance estimate, worth at most `kappa_max` observations for the
    /// mean (the day can move it).
    pub fn capped(&self, kappa_max: f64) -> Self {
        let mut c = *self;
        if c.kappa > kappa_max {
            c.kappa = kappa_max;
        }
        c
    }
}

/// Gamma distribution of a rate per hour: shape `a` (events' worth), rate `b` (hours).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct GammaRate {
    /// Shape: events.
    pub a: f64,
    /// Rate: hours of exposure.
    pub b: f64,
}

impl GammaRate {
    /// A prior of mean `per_hour` worth `a` events.
    pub fn prior(per_hour: f64, a: f64) -> Self {
        Self {
            a,
            b: a / per_hour.max(1e-6),
        }
    }

    /// Mean rate, per hour.
    pub fn mean(&self) -> f64 {
        self.a / self.b
    }

    /// Variance of the rate.
    pub fn var(&self) -> f64 {
        self.a / (self.b * self.b)
    }

    /// Conjugate update with `n` events in `hours`.
    pub fn update(&mut self, n: f64, hours: f64) {
        self.a += n.max(0.0);
        self.b += hours.max(0.0);
    }

    /// The same mean, worth at most `hours_max` hours of exposure.
    pub fn capped(&self, hours_max: f64) -> Self {
        if self.b <= hours_max {
            return *self;
        }
        let f = hours_max / self.b;
        Self {
            a: self.a * f,
            b: hours_max,
        }
    }
}

/// Huber's threshold, standard deviations.
pub const HUBER_C: f64 = 2.0;
/// A terrain counts in an outing with this much moving time on it, seconds.
pub const MIN_TERRAIN_S: f64 = 600.0;
/// Detours count in an outing with this much progress, metres.
pub const MIN_PROGRESS_M: f64 = 1000.0;
/// Day-to-day spread of the stop rate not learned (coefficient of variation; assumption).
pub const STOP_RATE_DAY_CV: f64 = 0.3;
/// Online: forgetting of the pace observations, seconds of moving time.
pub const DAY_TAU_S: f64 = 3600.0;
/// Online: standard deviation of the log pace of 10 min of moving time around the day's
/// pace (assumption: terrain, snow, a child that tires).
pub const DAY_SIGMA_W: f64 = 0.2;
/// Online: the group's stop rate weighs at most this many hours against the day's.
pub const DAY_PRIOR_H: f64 = 1.0;
/// Online: the group's mean stop duration weighs at most this many stops.
pub const DAY_PRIOR_STOPS: f64 = 3.0;
/// Two-sided 80 % quantile of the standard normal.
pub const Z80: f64 = 1.281_551_565_545;
/// Quantile of the total time a group with children is planned on (loops, return time,
/// turnaround): the slow side of its range, 8 chances in 10 to be back by then. A choice of
/// caution, not a measurement.
pub const PLAN_Q_CHILDREN: f64 = 0.8;
/// With children, back this long before the light limit (C7: 30 min for adults), minutes:
/// tired children at the end of the day go slower than their morning pace, the cold comes
/// with the shade and the dusk, and a child cannot be hurried. A choice of caution, not a
/// measurement.
pub const CHILD_LIGHT_MARGIN_MIN: f64 = 60.0;

/// The one testimony the priors of children of 8 and more are set on (n = 1, not a study;
/// a user of the app, November 2026). The prior of that group on loops of about 20 km of a
/// Jura domain (C5: 3 h 15 to 3 h 40) gives 3 h 30 to 4 h 30 moving and 5 h 30 to
/// 6 h 30 in total (median); tested in `skimap_plan` (`tests/pace.rs`).
pub const TESTIMONY: &str = "famille avec enfants débutants, classique : « 20 bornes par jour et jusqu'à piste rouge. Ça nous a pris 4 h de ski et 6 h en tout peut-être, avec les pauses et les détours » (témoignage unique, n = 1)";

fn huber(z: f64) -> f64 {
    if z.abs() <= HUBER_C {
        1.0
    } else {
        HUBER_C / z.abs()
    }
}

/// Parameters of a group: prior or posterior.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct PaceParams {
    /// Log progress pace relative to C5, per terrain ([`Terrain`] order).
    pub pace: [Nig; TERRAINS],
    /// Log ratio of the distance skied to the progress along the route (detours).
    pub detour: Nig,
    /// Stops of 90 s or more per hour of moving time.
    pub stops: GammaRate,
    /// Log stop duration, seconds.
    pub stop_len: Nig,
    /// Calibration of the range on the group's own outings (X14).
    #[serde(default)]
    pub spread: Spread,
}

/// Calibration of the range of a group on its own outings (X14 #148): each outing learned
/// is first predicted with the posterior of the outings before it (leave-future-out), its
/// standardised error `z = ln(real / median) / σ` (σ the log standard deviation of the
/// range) kept from the [`CAL_FROM`]th outing on; the range is then the median times
/// `exp(± Z80 k σ)` with `k² = (CAL_PRIOR + Σ z²) / (CAL_PRIOR + n)`: wider when the group
/// is less regular than its model says, narrower when more. Without such outings, `k` = 1
/// (the prior's range).
#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
pub struct Spread {
    /// Outings learned.
    pub seen: u32,
    /// Outings with a standardised error kept.
    pub n: f64,
    /// Sum of their squares.
    pub z2: f64,
}

/// The range is calibrated on the outings from this one on (the first ones are predicted
/// from the prior: their errors say how wide the prior is, not the group).
pub const CAL_FROM: u32 = 4;
/// Weight of the model's own range in the calibration, outings.
pub const CAL_PRIOR: f64 = 2.0;
/// An outing counts in the calibration with this much moving time and elapsed time,
/// seconds.
pub const CAL_MIN_S: f64 = 1200.0;

impl Spread {
    /// Factor of the log standard deviation of the range.
    pub fn factor(&self) -> f64 {
        if self.n <= 0.0 {
            return 1.0;
        }
        ((CAL_PRIOR + self.z2) / (CAL_PRIOR + self.n)).sqrt()
    }

    /// The prediction with its range calibrated (the median kept).
    pub fn apply(&self, p: Prediction) -> Prediction {
        let k = self.factor();
        if k == 1.0 || p.median_s <= 0.0 || p.hi_s <= p.median_s {
            return p;
        }
        let st = (p.hi_s / p.median_s).ln() / Z80;
        Prediction {
            lo_s: p.median_s * (-Z80 * k * st).exp(),
            hi_s: p.median_s * (Z80 * k * st).exp(),
            ..p
        }
    }
}

/// A prior row: moving pace (C5 = 1) per terrain, predictive standard deviation of its
/// log, detours, stops per hour, median stop, minutes.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PriorRow {
    /// Moving speed relative to C5, per terrain.
    pub speed: [f64; TERRAINS],
    /// Predictive standard deviation of the log pace.
    pub sd: f64,
    /// Extra distance skied, share (0.1: 10 % more than the route).
    pub detour: f64,
    /// Stops of 90 s or more per hour of moving time.
    pub stops_per_h: f64,
    /// Median stop, minutes.
    pub stop_min: f64,
}

/// Terrain shape of the children's pace, relative to the flat: Sollie et al. 2021 (PMID
/// 33231327), skiers of 14 years against adults: −19 % uphill, −11 % undulating, −8 %
/// downhill, i.e. 0.81 / 0.89 = 0.91 uphill and 0.92 / 0.89 = 1.03 downhill.
pub const CHILD_SHAPE: [f64; TERRAINS] = [0.91, 1.0, 1.03];

/// The prior of a group ([`CHILD_SHAPE`] from a publication, the rows of children of 8
/// and more set on [`TESTIMONY`] (n = 1), the rest assumptions; see the module).
///
/// | Group (youngest child) | Moving speed (C5 = 1) | Predictive sd (log) | Detours | Stops / h | Median stop |
/// |---|---|---|---|---|---|
/// | adult, intermediate | 1.0 | 0.25 | 3 % | 0.5 | 4 min |
/// | adult, beginner / expert | 0.9 / 1.2 | 0.25 | 3 % | 0.5 | 4 min |
/// | children 14–17 | 1.0 × shape | 0.30 | 4 % | 1.5 | 6 min |
/// | children 11–13 | 1.0 × shape | 0.35 | 5 % | 2.5 | 7 min |
/// | children 8–10 | 1.0 × shape | 0.35 | 6 % | 3 | 8 min |
/// | children 5–7 | 0.55 × shape | 0.40 | 12 % | 3.5 | 8 min |
/// | children under 5 | 0.30 × shape | 0.45 | 15 % | 4 | 8.5 min |
///
/// The adults' level multiplies the children's rows too, at half strength. With children
/// of 8 and more, the testimony puts a beginner family at about the C5 pace moving (C5 is
/// a leisure adult: 5.3 to 6 km/h on the loops of the two sites) and a third of the day
/// stopped; under 8, nothing was observed: the rows stay slow (caution).
pub fn prior_row(p: &GroupProfile) -> PriorRow {
    let level = match p.level {
        Level::Beginner => 0.9,
        Level::Intermediate => 1.0,
        Level::Expert => 1.2,
    };
    let Some(age) = p.youngest() else {
        return PriorRow {
            speed: [level; TERRAINS],
            sd: 0.25,
            detour: 0.03,
            stops_per_h: 0.5,
            stop_min: 4.0,
        };
    };
    let (k, sd, detour, stops, stop_min) = match age {
        0..=4 => (0.30, 0.45, 0.15, 4.0, 8.5),
        5..=7 => (0.55, 0.40, 0.12, 3.5, 8.0),
        8..=10 => (1.0, 0.35, 0.06, 3.0, 8.0),
        11..=13 => (1.0, 0.35, 0.05, 2.5, 7.0),
        _ => (1.0, 0.30, 0.04, 1.5, 6.0),
    };
    let k = k * (1.0 + 0.5 * (level - 1.0));
    PriorRow {
        speed: CHILD_SHAPE.map(|s| k * s),
        sd,
        detour,
        stops_per_h: stops,
        stop_min,
    }
}

/// A prediction of a duration.
#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
pub struct Prediction {
    /// Expected moving time, seconds.
    pub moving_s: f64,
    /// Expected stopped time, seconds.
    pub stop_s: f64,
    /// Expected total, seconds.
    pub mean_s: f64,
    /// Median total, seconds.
    pub median_s: f64,
    /// 10 % quantile, seconds.
    pub lo_s: f64,
    /// 90 % quantile, seconds.
    pub hi_s: f64,
}

impl Prediction {
    /// Quantile `q` of the total (log-normal through the median and the 90 % quantile),
    /// seconds; `q` 0.5 is [`Prediction::median_s`] exactly.
    pub fn quantile_s(&self, q: f64) -> f64 {
        if q == 0.5 || self.median_s <= 0.0 || self.hi_s <= self.median_s {
            return self.median_s;
        }
        let st = (self.hi_s / self.median_s).ln() / Z80;
        self.median_s * (probit(q) * st).exp()
    }

    /// Relative half-width of the 80 % interval.
    pub fn spread(&self) -> f64 {
        if self.median_s > 0.0 {
            0.5 * (self.hi_s - self.lo_s) / self.median_s
        } else {
            0.0
        }
    }
}

/// Standard normal cumulative distribution (Abramowitz–Stegun 7.1.26, error < 1.5e-7).
pub fn phi(x: f64) -> f64 {
    let z = x.abs() / std::f64::consts::SQRT_2;
    let t = 1.0 / (1.0 + 0.327_591_1 * z);
    let poly = t
        * (0.254_829_592
            + t * (-0.284_496_736
                + t * (1.421_413_741 + t * (-1.453_152_027 + t * 1.061_405_429))));
    let erfc = poly * (-z * z).exp();
    if x >= 0.0 {
        1.0 - 0.5 * erfc
    } else {
        0.5 * erfc
    }
}

/// Inverse of [`phi`] (bisection to 1e-10), for `q` in (0, 1).
pub fn probit(q: f64) -> f64 {
    let q = q.clamp(1e-9, 1.0 - 1e-9);
    let (mut lo, mut hi) = (-8.0_f64, 8.0_f64);
    for _ in 0..60 {
        let mid = 0.5 * (lo + hi);
        if phi(mid) < q {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    0.5 * (lo + hi)
}

/// Moving time (median `moving_median_s`, variance `var_log` of its log) plus stops, as a
/// log-normal total; `paused_for`: the group is stopped since that many seconds (its
/// expected remaining time is added).
fn combine(
    moving_median_s: f64,
    var_log: f64,
    stops: GammaRate,
    stop_len: Nig,
    paused_for: Option<f64>,
) -> Prediction {
    let mov_mean = moving_median_s * (0.5 * var_log).exp();
    let mov_var = mov_mean * mov_mean * (var_log.exp() - 1.0);
    let h = mov_mean / 3600.0;
    let lam = stops.mean();
    let var_lam = stops.var() + (STOP_RATE_DAY_CV * lam).powi(2);
    let (mu, s2) = (stop_len.m, stop_len.sigma2());
    let var_mu = stop_len.mean_var();
    let ed = (mu + 0.5 * s2 + 0.5 * var_mu).exp();
    let ed2 = (2.0 * mu + 2.0 * s2 + 2.0 * var_mu).exp();
    let var_ed = ed * ed * (var_mu.exp() - 1.0);
    let stop_mean = lam * h * ed;
    let stop_var = lam * h * ed2 + var_lam * h * h * ed * ed + (lam * h).powi(2) * var_ed;
    let k = 1.0 + lam * ed / 3600.0;
    let mut mean = mov_mean + stop_mean;
    let mut var = mov_var * k * k + stop_var;
    let mut stop_total = stop_mean;
    if let Some(p) = paused_for {
        // Mean residual life of the log-normal duration: E[D − p | D > p].
        let s = (s2 + var_mu).sqrt().max(1e-3);
        let lp = p.max(1.0).ln();
        let num = phi((mu + s * s - lp) / s);
        let den = phi((mu - lp) / s);
        let ed_all = (mu + 0.5 * s * s).exp();
        let rest = if den > 1e-9 {
            (ed_all * num / den - p).max(0.0)
        } else {
            p * s * s / (lp - mu).max(0.1)
        }
        .clamp(30.0, 3.0 * 3600.0);
        mean += rest;
        var += (0.5 * rest).powi(2);
        stop_total += rest;
    }
    if mean <= 0.0 {
        return Prediction::default();
    }
    let s2t = (1.0 + var / (mean * mean)).ln();
    let st = s2t.sqrt();
    let mu_t = mean.ln() - 0.5 * s2t;
    Prediction {
        moving_s: mov_mean,
        stop_s: stop_total,
        mean_s: mean,
        median_s: mu_t.exp(),
        lo_s: (mu_t - Z80 * st).exp(),
        hi_s: (mu_t + Z80 * st).exp(),
    }
}

impl PaceParams {
    /// The prior of a group ([`prior_row`]).
    pub fn prior(p: &GroupProfile) -> Self {
        Self::of_row(&prior_row(p))
    }

    /// A group nobody described (a plan without a profile): the means of a lone adult,
    /// with twice its spread on the pace (predictive standard deviation 0.5: a family or a
    /// racer just as well) and a stop rate worth one stop.
    pub fn unknown(style: Style) -> Self {
        let mut p = Self::prior(&GroupProfile::solo(style, Level::Intermediate));
        for n in &mut p.pace {
            *n = Nig::prior(n.m, 0.5);
        }
        p.stops = GammaRate::prior(p.stops.mean(), 1.0);
        p
    }

    /// Parameters of a prior row.
    pub fn of_row(r: &PriorRow) -> Self {
        let rho = (1.0 + r.detour).ln();
        Self {
            pace: r.speed.map(|k| Nig::prior(k.ln() - rho, r.sd)),
            detour: Nig::prior(rho, 0.08),
            stops: GammaRate::prior(r.stops_per_h, 2.0),
            stop_len: Nig::prior((r.stop_min * 60.0).ln(), 0.6),
            spread: Spread::default(),
        }
    }

    /// Progress pace relative to C5 on a terrain (median).
    pub fn progress_pace(&self, t: Terrain) -> f64 {
        self.pace[t as usize].m.exp()
    }

    /// Moving speed relative to C5 on a terrain (progress pace plus detours).
    pub fn moving_speed(&self, t: Terrain) -> f64 {
        (self.pace[t as usize].m + self.detour.m).exp()
    }

    /// Extra distance skied, share.
    pub fn detour_share(&self) -> f64 {
        self.detour.m.exp() - 1.0
    }

    /// Expected stop duration, seconds.
    pub fn stop_mean_s(&self) -> f64 {
        (self.stop_len.m + 0.5 * self.stop_len.sigma2()).exp()
    }

    /// Duration of a route of model time `model_s` per terrain (C5, seconds), with the
    /// range calibrated on the group's outings ([`Spread`]).
    pub fn predict(&self, model_s: [f64; TERRAINS]) -> Prediction {
        self.spread.apply(self.predict_model(model_s))
    }

    /// Duration of a route as the model says it, before the calibration of its range.
    pub fn predict_model(&self, model_s: [f64; TERRAINS]) -> Prediction {
        let (mut med, mut sd_w) = (0.0, 0.0);
        for (m, n) in model_s.iter().zip(&self.pace) {
            let m = m.max(0.0);
            if m <= 0.0 {
                continue;
            }
            let t = m * (-n.m).exp();
            med += t;
            sd_w += t * n.pred_var().sqrt();
        }
        let s = if med > 0.0 { sd_w / med } else { 0.0 };
        combine(med, s * s, self.stops, self.stop_len, None)
    }

    /// Updates with an outing (robust conjugate update); returns the Huber weights of the
    /// pace observations per terrain (`None`: not enough moving time on it).
    pub fn learn(&mut self, o: &OutingObs) -> [Option<f64>; TERRAINS] {
        // The outing as it was predicted before learning it: calibration of the range.
        if self.spread.seen >= CAL_FROM
            && o.elapsed_s >= CAL_MIN_S
            && o.moving_total_s() >= CAL_MIN_S
        {
            let p = self.predict_model(o.model_s);
            if p.median_s > 0.0 && p.hi_s > p.median_s {
                let st = (p.hi_s / p.median_s).ln() / Z80;
                let z = (o.elapsed_s / p.median_s).ln() / st;
                self.spread.n += 1.0;
                self.spread.z2 += z * z;
            }
        }
        self.spread.seen += 1;
        let mut w = [None; TERRAINS];
        for (c, wc) in w.iter_mut().enumerate() {
            if o.moving_s[c] >= MIN_TERRAIN_S && o.model_s[c] > 0.0 {
                let y = (o.model_s[c] / o.moving_s[c]).ln();
                let h = huber(self.pace[c].z(y));
                self.pace[c].update(y, h);
                *wc = Some(h);
            }
        }
        if o.progress_m >= MIN_PROGRESS_M && o.travelled_m > 0.0 {
            let y = (o.travelled_m / o.progress_m).ln();
            let wd = huber(self.detour.z(y));
            self.detour.update(y, wd);
        }
        let hours = o.moving_s.iter().sum::<f64>() / 3600.0;
        if hours > 0.0 {
            self.stops.update(o.stops_s.len() as f64, hours);
        }
        for &d in &o.stops_s {
            if d > 0.0 {
                let y = d.ln();
                let wd = huber(self.stop_len.z(y));
                self.stop_len.update(y, wd);
            }
        }
        w
    }
}

/// What an outing tells about its group (from the tracker's segmentation: moving, and
/// stops of 90 s or more).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct OutingObs {
    /// Model time (C5) progressed while moving, per terrain, seconds.
    pub model_s: [f64; TERRAINS],
    /// Real moving time, per terrain, seconds.
    pub moving_s: [f64; TERRAINS],
    /// Progress along the route, metres.
    pub progress_m: f64,
    /// Distance skied (smoothed track), metres.
    pub travelled_m: f64,
    /// Durations of the stops, seconds.
    pub stops_s: Vec<f64>,
    /// From the start to the end, seconds.
    pub elapsed_s: f64,
}

impl OutingObs {
    /// Total moving time, seconds.
    pub fn moving_total_s(&self) -> f64 {
        self.moving_s.iter().sum()
    }

    /// Total model time progressed, seconds.
    pub fn model_total_s(&self) -> f64 {
        self.model_s.iter().sum()
    }
}

/// A group with what it learned.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProfileModel {
    /// The group.
    pub profile: GroupProfile,
    /// Posterior (the prior with no outing).
    pub params: PaceParams,
    /// Outings learned.
    pub outings: u32,
    /// Quantile of the total time the group is planned on, if set (`None`: from the
    /// group, [`ProfileModel::plan_quantile`]).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub plan_q: Option<f64>,
}

impl ProfileModel {
    /// A new group: its prior.
    pub fn new(profile: GroupProfile) -> Self {
        let params = PaceParams::prior(&profile);
        Self {
            profile,
            params,
            outings: 0,
            plan_q: None,
        }
    }

    /// Quantile of the total time the group is planned on (loops, return time,
    /// turnaround): [`PLAN_Q_CHILDREN`] with children (the slow side of the range), else
    /// the median; [`ProfileModel::plan_q`] when set.
    pub fn plan_quantile(&self) -> f64 {
        self.plan_q.unwrap_or(if self.profile.has_children() {
            PLAN_Q_CHILDREN
        } else {
            0.5
        })
    }

    /// The time the group is planned on for a prediction, seconds.
    pub fn plan_s(&self, p: &Prediction) -> f64 {
        p.quantile_s(self.plan_quantile())
    }

    /// Learns one more outing.
    pub fn learn(&mut self, o: &OutingObs) {
        self.params.learn(o);
        self.outings += 1;
    }
}

/// The posterior of a group after its outings, in order.
pub fn fit(profile: GroupProfile, outings: &[OutingObs]) -> ProfileModel {
    let mut m = ProfileModel::new(profile);
    for o in outings {
        m.learn(o);
    }
    m
}

/// Stop priors of a mixture of groups (`(weight, parameters)`, weights normalised here):
/// the rate and the log duration of a stop with the mixture's mean and variance (moments
/// matched). `None` without a positive weight.
pub fn stop_mixture<'a>(
    parts: impl IntoIterator<Item = (f64, &'a PaceParams)>,
) -> Option<(GammaRate, Nig)> {
    let (mut w, mut r1, mut r2, mut l1, mut l2) = (0.0, 0.0, 0.0, 0.0, 0.0);
    for (wi, p) in parts {
        if !(wi.is_finite() && wi > 0.0) {
            continue;
        }
        let (m, l) = (p.stops.mean(), p.stop_len.m);
        w += wi;
        r1 += wi * m;
        r2 += wi * (p.stops.var() + m * m);
        l1 += wi * l;
        l2 += wi * (p.stop_len.pred_var() + l * l);
    }
    if w <= 0.0 {
        return None;
    }
    let (rm, lm) = (r1 / w, l1 / w);
    let rv = (r2 / w - rm * rm).max(1e-9);
    let lv = (l2 / w - lm * lm).max(1e-9);
    Some((
        GammaRate {
            a: rm * rm / rv,
            b: rm / rv,
        },
        Nig::prior(lm, lv.sqrt()),
    ))
}

/// The pace of the day, updated during the outing: fixed size, no allocation.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DayPace {
    base: PaceParams,
    /// Prior variance of the day effect.
    v0: f64,
    /// Forgotten sums: real moving time and time predicted at the group's pace, seconds.
    a_r: f64,
    a_t: f64,
    /// Totals of the day.
    model_s: [f64; TERRAINS],
    moving_s: [f64; TERRAINS],
    last: Terrain,
    stops: u32,
    stop_total_s: f64,
    /// Sum and sum of squares of the log durations of the day's stops.
    stop_log: (f64, f64),
}

impl DayPace {
    /// The day of a group on a route of model time `route_model_s` per terrain (it
    /// weighs the prior variance of the day effect).
    pub fn new(base: &PaceParams, route_model_s: [f64; TERRAINS]) -> Self {
        let tot: f64 = route_model_s.iter().sum();
        let v0 = if tot > 0.0 {
            (0..TERRAINS)
                .map(|c| route_model_s[c] / tot * base.pace[c].pred_var())
                .sum()
        } else {
            base.pace[Terrain::Flat as usize].pred_var()
        };
        Self {
            base: *base,
            v0,
            a_r: 0.0,
            a_t: 0.0,
            model_s: [0.0; TERRAINS],
            moving_s: [0.0; TERRAINS],
            last: Terrain::Flat,
            stops: 0,
            stop_total_s: 0.0,
            stop_log: (0.0, 0.0),
        }
    }

    /// The group's parameters.
    pub fn base(&self) -> &PaceParams {
        &self.base
    }

    /// Replaces the group's stop priors (a group nobody described: [`stop_mixture`] of the
    /// profiles it may be, X14); the day's stops still update them.
    pub fn set_stop_prior(&mut self, rate: GammaRate, len: Nig) {
        self.base.stops = rate;
        self.base.stop_len = len;
    }

    /// `dt` seconds moving, progressing `d_model` model seconds per terrain.
    #[inline]
    pub fn moving(&mut self, dt: f64, d_model: [f64; TERRAINS]) {
        if dt <= 0.0 {
            return;
        }
        let f = (-dt / DAY_TAU_S).exp();
        let mut pred = 0.0;
        let mut best = (0.0, self.last);
        for (c, t) in Terrain::ALL.into_iter().enumerate() {
            let d = d_model[c].max(0.0);
            let p = d * (-self.base.pace[c].m).exp();
            pred += p;
            self.model_s[c] += d;
            if p > best.0 {
                best = (p, t);
            }
        }
        self.a_r = self.a_r * f + dt;
        self.a_t = self.a_t * f + pred;
        // Real time split by the predicted time of each terrain (all to the last terrain
        // when not progressing: a detour, a slow stretch).
        if pred > 0.0 {
            for ((m, d), n) in self.moving_s.iter_mut().zip(d_model).zip(&self.base.pace) {
                *m += dt * d.max(0.0) * (-n.m).exp() / pred;
            }
            self.last = best.1;
        } else {
            self.moving_s[self.last as usize] += dt;
        }
    }

    /// A stop decided late: its first `dt` seconds were counted as moving.
    pub fn unmove(&mut self, dt: f64) {
        let dt = dt.max(0.0);
        self.a_r = (self.a_r - dt).max(0.0);
        let m = &mut self.moving_s[self.last as usize];
        *m = (*m - dt).max(0.0);
    }

    /// A stop of the day ended, seconds.
    pub fn stop(&mut self, dur_s: f64) {
        if dur_s <= 0.0 {
            return;
        }
        self.stops += 1;
        self.stop_total_s += dur_s;
        let y = dur_s.ln();
        self.stop_log.0 += y;
        self.stop_log.1 += y * y;
    }

    /// Day effect on the log pace: posterior mean and variance.
    pub fn day_effect(&self) -> (f64, f64) {
        if self.a_r <= 0.0 || self.a_t <= 0.0 {
            return (0.0, self.v0);
        }
        let y = (self.a_t / self.a_r).ln();
        let info = self.a_r / (600.0 * DAY_SIGMA_W * DAY_SIGMA_W);
        let p = 1.0 / self.v0 + info;
        (info * y / p, 1.0 / p)
    }

    /// Moving time so far, seconds.
    pub fn moving_total_s(&self) -> f64 {
        self.moving_s.iter().sum()
    }

    /// Model time progressed so far, seconds.
    pub fn model_total_s(&self) -> f64 {
        self.model_s.iter().sum()
    }

    /// Stops so far and their total, seconds.
    pub fn stops(&self) -> (u32, f64) {
        (self.stops, self.stop_total_s)
    }

    /// Stop rate and stop duration of the day (the group's, updated with the day's).
    pub fn stop_params(&self) -> (GammaRate, Nig) {
        let mut rate = self.base.stops.capped(DAY_PRIOR_H);
        rate.update(f64::from(self.stops), self.moving_total_s() / 3600.0);
        let mut len = self.base.stop_len.capped(DAY_PRIOR_STOPS);
        if self.stops > 0 {
            let n = f64::from(self.stops);
            let mean = self.stop_log.0 / n;
            let ss = (self.stop_log.1 - n * mean * mean).max(0.0);
            len.update_batch(n, mean, ss);
        }
        (rate, len)
    }

    /// Progress pace of the day on a terrain, relative to C5 (median).
    pub fn pace(&self, t: Terrain) -> f64 {
        (self.base.pace[t as usize].m + self.day_effect().0).exp()
    }

    /// Remaining duration for `rem_model_s` model seconds per terrain; `paused_for`: the
    /// group is stopped since that many seconds.
    pub fn predict(&self, rem_model_s: [f64; TERRAINS], paused_for: Option<f64>) -> Prediction {
        self.predict_with(self.day_effect(), rem_model_s, paused_for)
    }

    /// As [`DayPace::predict`] at the group's own pace (the day effect at its prior: what
    /// the first minutes say of the day is not used), the day's stops still counted.
    pub fn predict_prior(
        &self,
        rem_model_s: [f64; TERRAINS],
        paused_for: Option<f64>,
    ) -> Prediction {
        self.predict_with((0.0, self.v0), rem_model_s, paused_for)
    }

    fn predict_with(
        &self,
        (d, vd): (f64, f64),
        rem_model_s: [f64; TERRAINS],
        paused_for: Option<f64>,
    ) -> Prediction {
        let mut med = 0.0;
        for (c, m) in rem_model_s.iter().enumerate() {
            med += m.max(0.0) * (-(self.base.pace[c].m + d)).exp();
        }
        // The pace of the rest scatters around the day's (averaged over its length).
        let within = DAY_SIGMA_W * DAY_SIGMA_W * 600.0 / med.max(600.0);
        let (rate, len) = self.stop_params();
        combine(med, vd + within, rate, len, paused_for)
    }

    /// What the day tells about the group (for [`PaceParams::learn`] after the outing).
    pub fn observation(
        &self,
        progress_m: f64,
        travelled_m: f64,
        stops_s: &[f64],
        elapsed_s: f64,
    ) -> OutingObs {
        OutingObs {
            model_s: self.model_s,
            moving_s: self.moving_s,
            progress_m,
            travelled_m,
            stops_s: stops_s.to_vec(),
            elapsed_s,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::matching::synth::Rng;

    fn lognormal(rng: &mut Rng, mu: f64, s: f64) -> f64 {
        (mu + s * rng.normal()).exp()
    }

    #[test]
    fn nig_batch_equals_sequential() {
        let ys = [0.1, -0.3, 0.25, 0.05, -0.1];
        let mut a = Nig::prior(0.0, 0.3);
        for y in ys {
            a.update(y, 1.0);
        }
        let n = ys.len() as f64;
        let mean = ys.iter().sum::<f64>() / n;
        let ss = ys.iter().map(|y| (y - mean).powi(2)).sum::<f64>();
        let mut b = Nig::prior(0.0, 0.3);
        b.update_batch(n, mean, ss);
        for (x, y) in [
            (a.m, b.m),
            (a.kappa, b.kappa),
            (a.alpha, b.alpha),
            (a.beta, b.beta),
        ] {
            assert!((x - y).abs() < 1e-12, "{x} {y}");
        }
        // The prior's predictive has the asked spread.
        assert!((Nig::prior(0.0, 0.3).pred_var().sqrt() - 0.3).abs() < 1e-12);
    }

    #[test]
    fn stop_mixture_matches_the_moments() {
        let adult = PaceParams::prior(&GroupProfile::solo(Style::Classic, Level::Intermediate));
        let fam = PaceParams::prior(&GroupProfile::app_default());
        // One component: itself (mean and spread).
        let (r, l) = stop_mixture([(2.0, &fam)]).expect("weight");
        assert!((r.mean() - fam.stops.mean()).abs() < 1e-9);
        assert!((r.var() - fam.stops.var()).abs() < 1e-9);
        assert!((l.m - fam.stop_len.m).abs() < 1e-9);
        assert!((l.pred_var() - fam.stop_len.pred_var()).abs() < 1e-9);
        // Half and half: the mean rate between, the variance of the mixture (within plus
        // between).
        let (r, l) = stop_mixture([(1.0, &adult), (1.0, &fam)]).expect("weight");
        let (ma, mf) = (adult.stops.mean(), fam.stops.mean());
        let mid = 0.5 * (ma + mf);
        assert!((r.mean() - mid).abs() < 1e-9, "{}", r.mean());
        let var = 0.5 * (adult.stops.var() + fam.stops.var()) + 0.25 * (mf - ma).powi(2);
        assert!((r.var() - var).abs() < 1e-9, "{} {var}", r.var());
        assert!(l.m > adult.stop_len.m && l.m < fam.stop_len.m);
        // No weight: none.
        assert!(stop_mixture([(0.0, &adult), (f64::NAN, &fam)]).is_none());
        // Set on the day: the family's stops expected for the rest, not an adult's.
        let route = [600.0, 3600.0, 600.0];
        let mut day = DayPace::new(&PaceParams::unknown(Style::Classic), route);
        let before = day.predict(route, None).stop_s;
        let (r, l) = stop_mixture([(1.0, &fam)]).expect("weight");
        day.set_stop_prior(r, l);
        let after = day.predict(route, None).stop_s;
        assert!(after > 3.0 * before, "{before} → {after}");
    }

    #[test]
    fn priors_slow_down_with_younger_children() {
        let adult = PaceParams::prior(&GroupProfile::solo(Style::Classic, Level::Intermediate));
        let mut last = adult.progress_pace(Terrain::Flat);
        assert!((adult.moving_speed(Terrain::Flat) - 1.0).abs() < 1e-12);
        let mut stopped = adult.stops.mean() * adult.stop_mean_s();
        for age in [15u8, 12, 9, 6, 3] {
            let p = PaceParams::prior(&GroupProfile::family(2, &[age, 16], Style::Classic));
            let v = p.progress_pace(Terrain::Flat);
            assert!(v < last, "{age}: {v} ≥ {last}");
            assert!(p.progress_pace(Terrain::Up) < v);
            // More stopped time per moving hour with younger children.
            let s = p.stops.mean() * p.stop_mean_s();
            assert!(s > stopped, "{age}: {s} ≤ {stopped}");
            last = v;
            stopped = s;
        }
        // Under 8 (nothing observed): far slower than from 8 (the testimony).
        let p8 = PaceParams::prior(&GroupProfile::family(2, &[8], Style::Classic));
        let p7 = PaceParams::prior(&GroupProfile::family(2, &[7], Style::Classic));
        assert!(p7.moving_speed(Terrain::Flat) < 0.6 * p8.moving_speed(Terrain::Flat));
        // Planned on the slow side with children, on the median without.
        let fam = ProfileModel::new(GroupProfile::app_default());
        let solo = ProfileModel::new(GroupProfile::solo(Style::Classic, Level::Beginner));
        assert_eq!(fam.plan_quantile(), PLAN_Q_CHILDREN);
        assert_eq!(solo.plan_quantile(), 0.5);
        let pr = fam.params.predict([3600.0, 3600.0, 1800.0]);
        assert!(fam.plan_s(&pr) > pr.median_s && fam.plan_s(&pr) < pr.hi_s);
        assert_eq!(solo.plan_s(&pr), pr.median_s);
        assert_eq!(
            GroupProfile::app_default().turnaround_params().margin_min,
            CHILD_LIGHT_MARGIN_MIN
        );
        assert_eq!(
            GroupProfile::solo(Style::Classic, Level::Beginner)
                .turnaround_params()
                .margin_min,
            TurnaroundParams::default().margin_min
        );
        let fam = GroupProfile::family(2, &[9, 6], Style::Classic);
        assert_eq!(fam.youngest(), Some(6));
        assert!(
            fam.label_fr().contains("enfants de 6 et 9 ans"),
            "{}",
            fam.label_fr()
        );
    }

    #[test]
    fn terrain_classes_from_model_times() {
        let m = EtaModel::new(Style::Classic);
        let r = terrain_ratio(&m);
        assert!(r > 1.2, "{r}");
        let t = |g: f64| {
            let out = 100.0 / m.speed_ms(g, m.default_elev_m, false);
            let back = 100.0 / m.speed_ms(-g, m.default_elev_m, false);
            Terrain::of_times(out, back, r)
        };
        assert_eq!(t(0.05), Terrain::Up);
        assert_eq!(t(-0.05), Terrain::Down);
        assert_eq!(t(0.01), Terrain::Flat);
        assert_eq!(t(-0.015), Terrain::Flat);
        assert_eq!(Terrain::of_grade(0.03), Terrain::Up);
    }

    #[test]
    fn quantiles_of_a_prediction() {
        assert!(probit(0.5).abs() < 1e-9);
        assert!((probit(0.9) - Z80).abs() < 1e-6);
        assert!((probit(0.8) - 0.841_621_233_572_914_3).abs() < 1e-6);
        let p = PaceParams::prior(&GroupProfile::family(2, &[9], Style::Classic))
            .predict([3600.0, 1800.0, 900.0]);
        assert_eq!(p.quantile_s(0.5), p.median_s);
        assert!((p.quantile_s(0.9) - p.hi_s).abs() < 1e-3 * p.hi_s);
        assert!((p.quantile_s(0.1) - p.lo_s).abs() < 1e-3 * p.lo_s);
    }

    #[test]
    fn phi_values() {
        assert!((phi(0.0) - 0.5).abs() < 1e-7);
        assert!((phi(Z80) - 0.9).abs() < 1e-6);
        assert!((phi(-1.96) - 0.025).abs() < 1e-4);
    }

    /// Simulated outings of a group: day pace `exp(theta + day)`, Poisson stops, log-normal
    /// durations.
    fn simulate(
        rng: &mut Rng,
        theta: f64,
        day_sd: f64,
        per_h: f64,
        stop_med_s: f64,
        model: [f64; 3],
    ) -> (OutingObs, f64) {
        let day = day_sd * rng.normal();
        let moving: Vec<f64> = model.iter().map(|m| m / (theta + day).exp()).collect();
        let h: f64 = moving.iter().sum::<f64>() / 3600.0;
        // Poisson by inversion.
        let lam = per_h * h;
        let (mut n, mut p, u) = (0usize, (-lam).exp(), rng.unit());
        let mut cum = p;
        while u > cum && n < 200 {
            n += 1;
            p *= lam / n as f64;
            cum += p;
        }
        let stops: Vec<f64> = (0..n)
            .map(|_| lognormal(rng, stop_med_s.ln(), 0.5))
            .collect();
        let total = moving.iter().sum::<f64>() + stops.iter().sum::<f64>();
        (
            OutingObs {
                model_s: model,
                moving_s: [moving[0], moving[1], moving[2]],
                progress_m: 8000.0,
                travelled_m: 8000.0 * 1.12,
                stops_s: stops,
                elapsed_s: total,
            },
            total,
        )
    }

    #[test]
    fn learning_converges_and_ranges_cover() {
        // A family slower than its prior (0.30 instead of 0.45 × shape) and stopping more.
        let profile = GroupProfile::family(2, &[6], Style::Classic);
        let model = [3000.0, 2500.0, 1500.0];
        let theta = (0.30f64).ln();
        let mut rng = Rng(42);
        let mut m = ProfileModel::new(profile.clone());
        let mut errs = Vec::new();
        let (mut inside, mut tests) = (0, 0);
        for k in 0..40 {
            let (o, total) = simulate(&mut rng, theta, 0.1, 4.0, 360.0, model);
            let p = m.params.predict(model);
            if k >= 10 {
                tests += 1;
                if (p.lo_s..=p.hi_s).contains(&total) {
                    inside += 1;
                }
            }
            errs.push(((p.median_s - total) / total).abs());
            m.learn(&o);
        }
        let first: f64 = errs[..3].iter().sum::<f64>() / 3.0;
        let late: f64 = errs[30..].iter().sum::<f64>() / 10.0;
        println!(
            "mean error of the first 3 {:.0} %, of the last 10 {:.0} %, 80 % interval covers {inside} of {tests}",
            first * 100.0,
            late * 100.0
        );
        assert!(late < first, "{late} vs {first}");
        assert!(late < 0.15, "{late}");
        assert!(
            (0.6..=0.97).contains(&(inside as f64 / tests as f64)),
            "{inside}/{tests}"
        );
        assert!((m.params.progress_pace(Terrain::Flat) - 0.30).abs() < 0.03);
        assert!(
            (m.params.stops.mean() - 4.0).abs() < 1.0,
            "{}",
            m.params.stops.mean()
        );
        assert!((m.params.detour_share() - 0.12).abs() < 0.02);
    }

    #[test]
    fn an_outlier_outing_barely_moves_a_learned_group() {
        let profile = GroupProfile::solo(Style::Skating, Level::Intermediate);
        let model = [2000.0, 2000.0, 1000.0];
        let mut rng = Rng(7);
        let outings: Vec<OutingObs> = (0..8)
            .map(|_| simulate(&mut rng, 0.0, 0.05, 0.5, 240.0, model).0)
            .collect();
        let m = fit(profile.clone(), &outings);
        // An outing at a third of the pace (an injured skier): Huber weight.
        let mut bad = outings[0].clone();
        for x in &mut bad.moving_s {
            *x *= 3.0;
        }
        let mut robust = m.clone();
        let w = robust.params.learn(&bad);
        assert!(w.iter().flatten().all(|w| *w < 0.3), "{w:?}");
        let mut plain = m.params;
        let y = (bad.model_s[1] / bad.moving_s[1]).ln();
        plain.pace[1].update(y, 1.0);
        let shift_robust = (robust.params.pace[1].m - m.params.pace[1].m).abs();
        let shift_plain = (plain.pace[1].m - m.params.pace[1].m).abs();
        assert!(
            shift_robust < 0.3 * shift_plain,
            "{shift_robust} vs {shift_plain}"
        );
    }

    #[test]
    fn day_pace_follows_the_day() {
        // Prior: an adult (pace 1); the day: 0.6 of the model pace, stops 3 / h of 5 min.
        let base = PaceParams::prior(&GroupProfile::solo(Style::Classic, Level::Intermediate));
        let route = [3600.0, 3600.0, 1800.0];
        let mut d = DayPace::new(&base, route);
        let mut done = [0.0; 3];
        for i in 0..3600 {
            // One second moving, a third of each terrain, at the day's pace.
            let c = i % 3;
            let mut dm = [0.0; 3];
            dm[c] = 0.6;
            done[c] += dm[c];
            d.moving(1.0, dm);
            if i % 1200 == 1199 {
                d.stop(300.0);
            }
        }
        let (e, v) = d.day_effect();
        let pace = d.pace(Terrain::Flat);
        println!(
            "day effect {e:.3} ± {:.3}, pace {pace:.2} (truth 0.60)",
            v.sqrt()
        );
        assert!((pace - 0.6).abs() < 0.05, "{pace}");
        let (rate, _) = d.stop_params();
        assert!(rate.mean() > 1.2, "{}", rate.mean());
        let rem = [route[0] - done[0], route[1] - done[1], route[2] - done[2]];
        let p = d.predict(rem, None);
        let moving_truth: f64 = rem.iter().sum::<f64>() / 0.6;
        assert!((p.moving_s / moving_truth - 1.0).abs() < 0.1, "{p:?}");
        assert!(p.lo_s < p.median_s && p.median_s < p.hi_s);
        // Stopped for 10 min: more time left than moving.
        let q = d.predict(rem, Some(600.0));
        assert!(q.mean_s > p.mean_s);
        // At the group's own pace: the day ignored, its stops kept.
        let fresh = DayPace::new(&base, route);
        let r = d.predict_prior(rem, None);
        let f = fresh.predict(rem, None);
        assert!((r.moving_s / f.moving_s - 1.0).abs() < 1e-9, "{r:?} {f:?}");
        assert!(r.stop_s > f.stop_s, "{r:?} {f:?}");
    }

    #[test]
    fn a_stop_not_decided_at_the_start_is_not_the_day() {
        // A family two minutes after the start, stopped but not yet decided (it counts as
        // moving without progress): its day pace collapses, its own pace does not.
        let base = PaceParams::prior(&GroupProfile::family(2, &[6, 9], Style::Classic));
        let route = [4000.0, 4000.0, 2000.0];
        let mut d = DayPace::new(&base, route);
        for _ in 0..30 {
            d.moving(1.0, [0.0, 0.6, 0.0]);
        }
        for _ in 0..90 {
            d.moving(1.0, [0.0, 0.0, 0.0]);
        }
        let fresh = DayPace::new(&base, route);
        let day = d.predict(route, None);
        let own = d.predict_prior(route, None);
        let prior = fresh.predict(route, None);
        assert!(day.median_s > 1.5 * prior.median_s, "{day:?} {prior:?}");
        // (Two minutes without a stop barely move the stop rate.)
        assert!(
            (own.median_s / prior.median_s - 1.0).abs() < 0.02,
            "{own:?} {prior:?}"
        );
        assert!(
            (own.moving_s / prior.moving_s - 1.0).abs() < 1e-9,
            "{own:?} {prior:?}"
        );
    }
}
