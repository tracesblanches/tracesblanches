//! Scenario bench (Q9, #109): synthetic outings on the real trail graphs replayed through
//! the tracker with a simulated clock, in closed loop.
//!
//! - **The skier** moves along a course of the graph at the C5 model pace divided by a
//!   slowdown (one step per simulated second), with scripted events ([`Step`]): wait at
//!   the start, arrive by car, take a wrong branch and come back or ski on, stop (at a
//!   refuge or not), leave in a vehicle, turn back on foot, lose the GPS, answer a
//!   proposal. Obedient by default: when the tracker switches route, the skier takes the
//!   new one; a scripted refusal keeps the previous one.
//! - **A group** (C11, [`Behaviour`]): its own speed per terrain relative to C5, random
//!   stops (Poisson in moving time, log-normal durations) and detours (back along the
//!   route and forward again: distance skied without progress, the phone stays with the
//!   adult on the trail), drawn from its own seed; [`Behaviour::day`] varies them from
//!   day to day.
//! - **The GPS**: the true position plus the noise of C2 (`matching::synth`: Gauss–Markov
//!   error per axis and forest outliers), drawn once per second from a seed.
//! - **The client** applies the tracker's GPS requests ([`crate::gps`]): fix interval,
//!   batch delay (fixes delivered by batch with [`Tracker::push_batch`]), GNSS off during
//!   a pause, significant motion when the skier moves again.
//!
//! A [`Run`] keeps every input given to the tracker and every output, so that a run can be
//! replayed (the client's instrumented tests use the same inputs and expected outputs).

use jiff::{SignedDuration, Timestamp, civil::DateTime};
use serde::{Deserialize, Serialize};
use skimap_assist::inputs::SiteData;
use skimap_common::geom::LonLat;
use skimap_common::grid::Grid;
use skimap_core::graph::{HalfEdge, LocalPlane, PisteGraph};
use skimap_core::matching::synth::{Noise, Rng, Truth, add_noise};

use skimap_core::calibration::{TERRAINS, Terrain, terrain_ratio};

use crate::course::{Course, PlanLeg};
use crate::error::{Error, Result};
use crate::gps::GpsRequest;
use crate::line::{bearing_deg, turn_deg};
use crate::plan::DayPlan;
use crate::session::Activity;
use crate::track::TrackView;
use crate::tracker::{Input, Output, Tracker, TrackerParams};

/// A scripted event, at a distance along the skier's current course or a time.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Step {
    /// Stands at the start this long before skiing, seconds.
    WaitAtStart {
        /// Seconds.
        secs: f64,
    },
    /// Arrives at the start by car from this far (15 m/s), metres, then waits.
    ArriveByCar {
        /// Metres.
        from_m: f64,
    },
    /// Stops at a distance along the course, for a while.
    Stop {
        /// Distance, metres.
        at_s: f64,
        /// Seconds.
        secs: f64,
    },
    /// At junction `junction` of the plan, takes a branch for `length_m`, then comes back
    /// (or skis on until the tracker gives a new route).
    WrongTurn {
        /// Junction index in the plan.
        junction: usize,
        /// Metres on the branch.
        length_m: f64,
        /// Comes back to the junction.
        back: bool,
    },
    /// The ways are closed when the skier reaches this distance.
    Close {
        /// Distance, metres.
        at_s: f64,
        /// OSM ways.
        ways: Vec<i64>,
    },
    /// Refuses the next switch with a gesture after this delay, seconds.
    RefuseNext {
        /// Seconds.
        after_s: f64,
    },
    /// Ignores the next switch (skis on along the previous route).
    IgnoreNext,
    /// Gets in a vehicle at this distance.
    Vehicle {
        /// Distance, metres.
        at_s: f64,
    },
    /// Turns back to the start along the course at this distance.
    TurnBack {
        /// Distance, metres.
        at_s: f64,
    },
    /// Leaves the course at this distance, straight to a point and straight back at a
    /// walking speed, then skis on (off the trails: K6 #56).
    Excursion {
        /// Distance, metres.
        at_s: f64,
        /// Farthest point.
        to: LonLat,
        /// Speed, m/s.
        speed_mps: f64,
    },
    /// No fix for a while from this distance.
    Gap {
        /// Distance, metres.
        at_s: f64,
        /// Seconds.
        secs: f64,
    },
    /// Answers the next proposal of the tracker after a delay (in order of the steps).
    Answer {
        /// Accepts.
        accept: bool,
        /// Seconds after the proposal.
        after_s: f64,
    },
    /// Answers the proposal to choose another group profile after a delay (X14).
    AnswerProfile {
        /// Accepts.
        accept: bool,
        /// Seconds after the proposal.
        after_s: f64,
    },
}

/// How a group skis (C11): the truth of a simulated outing.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Behaviour {
    /// Moving speed relative to C5 per terrain (uphill, flat, downhill).
    pub speed: [f64; TERRAINS],
    /// Stops (of any length) per hour of moving.
    pub stops_per_h: f64,
    /// Median stop, seconds.
    pub stop_median_s: f64,
    /// Standard deviation of the log stop duration.
    pub stop_sigma: f64,
    /// Detours (back along the route and forward again) per hour of moving.
    pub detours_per_h: f64,
    /// Mean way back of a detour, metres (exponential).
    pub detour_m: f64,
    /// Seed of the stops and detours.
    pub seed: u64,
}

impl Behaviour {
    /// A family with children of 6 and 9: 0.55 to 0.65 of the C5 speed (slower uphill and
    /// downhill: snowplough), 3 stops an hour of 4 min (median), 4 detours an hour of 30 m
    /// back.
    pub fn family() -> Self {
        Self {
            speed: [0.55, 0.65, 0.55],
            stops_per_h: 3.0,
            stop_median_s: 240.0,
            stop_sigma: 0.7,
            detours_per_h: 4.0,
            detour_m: 30.0,
            seed: 1,
        }
    }

    /// A trained adult: 25 % faster than C5 (10 % downhill), one short stop in three hours.
    pub fn fast_adult() -> Self {
        Self {
            speed: [1.3, 1.25, 1.1],
            stops_per_h: 0.3,
            stop_median_s: 150.0,
            stop_sigma: 0.5,
            detours_per_h: 0.0,
            detour_m: 0.0,
            seed: 2,
        }
    }

    /// An adult at the C5 pace, a stop of 4 min every hour and a half.
    pub fn normal_adult() -> Self {
        Self {
            speed: [1.0, 1.0, 1.0],
            stops_per_h: 0.6,
            stop_median_s: 240.0,
            stop_sigma: 0.5,
            detours_per_h: 0.5,
            detour_m: 20.0,
            seed: 3,
        }
    }

    /// The same group on day `k`: speed × e^(0.1 N), stops × e^(0.3 N), median stop ×
    /// e^(0.2 N), detours × e^(0.3 N), a seed of its own.
    pub fn day(&self, k: u64) -> Self {
        let mut r = Rng(self.seed.wrapping_mul(0x9E37_79B9_7F4A_7C15)
            ^ (k + 1).wrapping_mul(0xBF58_476D_1CE4_E5B9));
        let f = (0.1 * r.normal()).exp();
        Self {
            speed: self.speed.map(|v| v * f),
            stops_per_h: self.stops_per_h * (0.3 * r.normal()).exp(),
            stop_median_s: self.stop_median_s * (0.2 * r.normal()).exp(),
            stop_sigma: self.stop_sigma,
            detours_per_h: self.detours_per_h * (0.3 * r.normal()).exp(),
            detour_m: self.detour_m,
            seed: r.next_u64(),
        }
    }
}

/// A scenario.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Scenario {
    /// Name.
    pub name: String,
    /// Start of the skier (UTC); the plan's departure by default.
    pub start: Timestamp,
    /// Model pace divided by this (1: the model pace).
    pub slowdown: f64,
    /// Script.
    pub steps: Vec<Step>,
    /// GPS noise, metres (standard deviation per axis).
    pub sigma_m: f64,
    /// Forest outliers (30 m episodes of 2 to 10 s, one every 50 s on average).
    pub forest: bool,
    /// Seed of the noise.
    pub seed: u64,
    /// Simulated time after the end of the course, seconds.
    pub tail_s: f64,
    /// The group's behaviour (C11); `None`: the C5 pace divided by `slowdown`, no stop or
    /// detour but those scripted.
    #[serde(default)]
    pub behaviour: Option<Behaviour>,
    /// The phone counts steps (X10, [`Input::Steps`] every 10 s with the fixes): striding on
    /// skis, one a second uphill, one every two seconds on the flat (diagonal stride mixed
    /// with double poling), none downhill (gliding), standing or in a vehicle.
    #[serde(default)]
    pub step_counter: bool,
}

/// A run: everything given to the tracker and everything it said.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Run {
    /// Scenario.
    pub scenario: String,
    /// Inputs in order.
    pub inputs: Vec<Input>,
    /// Outputs in order.
    pub outputs: Vec<Output>,
    /// Start of the skier.
    pub t0: Timestamp,
    /// End of the simulation.
    pub t1: Timestamp,
    /// Fixes given.
    pub fixes: usize,
    /// Batches given.
    pub batches: usize,
    /// Every 10 s: seconds from the start, the skier's distance along its course and
    /// position, the tracker's cursor and distance to the route.
    pub truth: Vec<(f64, f64, LonLat, f64, f64)>,
    /// The skier's stops on the way, scripted or drawn (not the wait at the start nor at
    /// the finish): seconds from the start, length.
    #[serde(default)]
    pub stops: Vec<(f64, f64)>,
}

/// Legs from a half-edge on, `length` metres, going straight on at each node.
pub fn walk(g: &PisteGraph, mut h: HalfEdge, mut length: f64) -> Vec<PlanLeg> {
    let mut out = Vec::new();
    while length > 0.0 {
        let len = g.length_m(h.edge);
        let take = length.min(len);
        let (a, b) = if h.forward {
            (0.0, take)
        } else {
            (len, len - take)
        };
        out.push(PlanLeg {
            edge: h.edge,
            from_m: a,
            to_m: b,
        });
        length -= take;
        if length <= 0.0 {
            break;
        }
        let node = g.head(h);
        let pts = &g.edge(h.edge).points;
        let (p0, p1) = if h.forward {
            (pts[pts.len() - 2], pts[pts.len() - 1])
        } else {
            (pts[1], pts[0])
        };
        let hin = bearing_deg(p0, p1);
        let next = g
            .out_edges(node)
            .iter()
            .filter(|x| x.edge != h.edge)
            .min_by(|x, y| {
                let b = |k: &HalfEdge| {
                    let q = &g.edge(k.edge).points;
                    let (a, b) = if k.forward {
                        (q[0], q[1])
                    } else {
                        (q[q.len() - 1], q[q.len() - 2])
                    };
                    turn_deg(hin, bearing_deg(a, b)).abs()
                };
                b(x).total_cmp(&b(y))
            })
            .copied();
        match next {
            Some(n) => h = n,
            None => break,
        }
    }
    out
}

fn reversed(legs: &[PlanLeg]) -> Vec<PlanLeg> {
    legs.iter()
        .rev()
        .map(|l| PlanLeg {
            edge: l.edge,
            from_m: l.to_m,
            to_m: l.from_m,
        })
        .collect()
}

/// The legs of a wrong turn at a junction of the plan: the loop up to it, a branch, then
/// (if `back`) the way back and the rest of the loop.
pub fn wrong_turn(
    plan: &DayPlan,
    g: &PisteGraph,
    junction: usize,
    length_m: f64,
    back: bool,
) -> Result<Vec<PlanLeg>> {
    let c = Course::new(g, &plan.route.legs, &plan.eta)?;
    let j = plan
        .junctions
        .get(junction)
        .ok_or_else(|| Error::Invalid(format!("no junction {junction}")))?;
    let rn = c.nodes()[j.route_node];
    let used: Vec<usize> = c.legs().iter().map(|l| l.edge).collect();
    let h = g
        .out_edges(rn.node)
        .iter()
        .find(|h| !used.contains(&h.edge))
        .copied()
        .ok_or_else(|| Error::Invalid("no branch".into()))?;
    let branch = walk(g, h, length_m);
    let mut legs = c.prefix_to_node(j.route_node);
    legs.extend_from_slice(&branch);
    if back {
        legs.extend(reversed(&branch));
        legs.extend(c.suffix_from_node(j.route_node));
    }
    Ok(legs)
}

/// The skier on a course.
struct Skier {
    course: Course,
    /// Distance along it.
    s: f64,
    /// Terrain of each piece of the course's profile.
    terrain: Vec<Terrain>,
    /// Out of a detour: metres still to go back, metres behind the progress.
    back_left: f64,
    back: f64,
}

impl Skier {
    fn new(course: Course, ratio: f64) -> Self {
        let mut sk = Self {
            course,
            s: 0.0,
            terrain: Vec::new(),
            back_left: 0.0,
            back: 0.0,
        };
        sk.set_course(sk.course.clone(), ratio);
        sk
    }

    fn set_course(&mut self, c: Course, ratio: f64) {
        let p = c.profile();
        self.terrain = (1..p.dist_m.len())
            .map(|i| {
                Terrain::of_times(
                    p.out_s[i] - p.out_s[i - 1],
                    p.back_s[i] - p.back_s[i - 1],
                    ratio,
                )
            })
            .collect();
        self.course = c;
    }

    /// Terrain and model speed (m/s along the route) here.
    fn here(&self) -> (Terrain, f64) {
        let p = self.course.profile();
        let d = self.course.profile_at(self.s);
        let i = p.index_at(d).min(self.terrain.len().saturating_sub(1));
        let t = self.terrain.get(i).copied().unwrap_or(Terrain::Flat);
        let dt = p.out_s.get(i + 1).map_or(0.0, |x| x - p.out_s[i]);
        let dd = p.dist_m.get(i + 1).map_or(0.0, |x| x - p.dist_m[i]);
        let scale = self.course.profile_at(1.0).max(1e-9);
        (t, if dt > 0.0 { dd / dt / scale } else { 1.0 })
    }

    /// Moves `dt` seconds at the model pace times `speed` on the terrain here, divided by
    /// `slow`.
    fn step(&mut self, dt: f64, slow: f64, speed: [f64; TERRAINS]) {
        let p = self.course.profile();
        let scale = if self.course.length_m() > 0.0 {
            p.length_m() / self.course.length_m()
        } else {
            1.0
        };
        let k = speed[self.here().0 as usize];
        let tau = self.course.model_s_at(self.s) + dt * k / slow;
        // Inverse of the profile: distance at model time tau.
        let i = p
            .out_s
            .partition_point(|x| *x <= tau)
            .clamp(1, p.out_s.len().max(2) - 1);
        let d = if p.out_s.len() < 2 {
            0.0
        } else {
            let (t0, t1) = (p.out_s[i - 1], p.out_s[i]);
            let (d0, d1) = (p.dist_m[i - 1], p.dist_m[i]);
            if t1 > t0 {
                d0 + (d1 - d0) * ((tau - t0) / (t1 - t0)).clamp(0.0, 1.0)
            } else {
                d1
            }
        };
        self.s = (d / scale).clamp(self.s, self.course.length_m());
    }

    fn pos(&self) -> LonLat {
        self.course.line().point_at((self.s - self.back).max(0.0))
    }

    fn done(&self) -> bool {
        self.s >= self.course.length_m() - 0.5
    }
}

/// Runs a scenario on a plan.
pub fn run(
    sc: &Scenario,
    plan: &DayPlan,
    track: TrackView<'_>,
    g: &PisteGraph,
    params: TrackerParams,
) -> Result<Run> {
    run_with_layers(sc, plan, track, g, params, None)
}

/// [`run`] with the layers of the day given to the tracker
/// ([`Tracker::with_layers`]: the routes computed during the outing announce them, X11).
pub fn run_with_layers(
    sc: &Scenario,
    plan: &DayPlan,
    track: TrackView<'_>,
    g: &PisteGraph,
    params: TrackerParams,
    layers: Option<&SiteData>,
) -> Result<Run> {
    run_with(sc, plan, track, g, params, layers, None)
}

/// [`run_with_layers`] with the terrain slope given to the tracker as well
/// ([`Tracker::with_slope`]: steep terrain off the trails, K6 #56).
pub fn run_with(
    sc: &Scenario,
    plan: &DayPlan,
    track: TrackView<'_>,
    g: &PisteGraph,
    params: TrackerParams,
    layers: Option<&SiteData>,
    slope: Option<&Grid<u8>>,
) -> Result<Run> {
    let mut tr = Tracker::new(plan, track, g, params)?;
    if let Some(d) = layers {
        tr = tr.with_layers(d);
    }
    if let Some(s) = slope {
        tr = tr.with_slope(s);
    }
    let mut legs = plan.route.legs.clone();
    for st in &sc.steps {
        if let Step::WrongTurn {
            junction,
            length_m,
            back,
        } = st
        {
            legs = wrong_turn(plan, g, *junction, *length_m, *back)?;
        }
    }
    let ratio = terrain_ratio(&plan.eta);
    let mut sk = Skier::new(Course::new(g, &legs, &plan.eta)?, ratio);
    let bh = sc.behaviour;
    let speed = bh.map_or([1.0; TERRAINS], |b| b.speed);
    let mut grng = Rng(bh.map_or(0, |b| b.seed));
    let mut answers: Vec<(bool, f64)> = sc
        .steps
        .iter()
        .filter_map(|s| match s {
            Step::Answer { accept, after_s } => Some((*accept, *after_s)),
            _ => None,
        })
        .collect();
    answers.reverse();
    let mut answer_at: Option<(f64, bool)> = None;
    let profile_answer = sc.steps.iter().find_map(|s| match s {
        Step::AnswerProfile { accept, after_s } => Some((*accept, *after_s)),
        _ => None,
    });
    let mut profile_at: Option<(f64, bool)> = None;
    let start_pos = sk.pos();
    let plane = LocalPlane::new(start_pos);
    // Noise of C2, one draw per second for the whole outing.
    let horizon =
        (plan.route.model_min * 60.0 * sc.slowdown.max(1.0) * 2.5 + sc.tail_s + 7200.0) as usize;
    let noise = Noise {
        sigma_m: sc.sigma_m,
        tau_s: 30.0,
        outlier_rate_hz: if sc.forest { 1.0 / 50.0 } else { 0.0 },
        outlier_m: 30.0,
        outlier_len_s: (2.0, 10.0),
    };
    let still: Vec<Truth> = (0..horizon)
        .map(|i| Truth {
            t_s: i as f64,
            pos: start_pos,
            along_m: 0.0,
        })
        .collect();
    let offsets: Vec<[f64; 2]> = add_noise(&still, &noise, &mut Rng(sc.seed))
        .iter()
        .map(|f| plane.xy(f.pos))
        .collect();
    let mut run = Run {
        scenario: sc.name.clone(),
        inputs: Vec::new(),
        outputs: Vec::new(),
        t0: sc.start,
        t1: sc.start,
        fixes: 0,
        batches: 0,
        truth: Vec::new(),
        stops: Vec::new(),
    };
    let mut out: Vec<Output> = Vec::new();
    let mut batch: Vec<Input> = Vec::new();
    let mut req: Option<GpsRequest> = None;
    let mut last_fix = f64::NEG_INFINITY;
    let mut steps = sc.steps.clone();
    let mut waiting = 0.0;
    let mut car: Option<(LonLat, f64)> = None;
    for st in &steps {
        match st {
            Step::WaitAtStart { secs } => waiting += secs,
            Step::ArriveByCar { from_m } => {
                car = Some((plane.lonlat([-from_m, 0.0]), *from_m));
                waiting += 120.0;
            }
            _ => {}
        }
    }
    let mut stop_until: Option<f64> = None;
    // An excursion off the course: from, to, speed, metres done.
    let mut excursion: Option<([f64; 2], [f64; 2], f64, f64)> = None;
    let mut gap_until: Option<f64> = None;
    let mut vehicle = false;
    let mut fired = false;
    let (mut still_for, mut moving_for, mut said_still) = (0.0, 0.0, false);
    let mut refuse_at: Option<f64> = None;
    let mut steps_acc = 0.0f64;
    let mut ignore_next = false;
    let mut finished_at: Option<f64> = None;
    let t_start = sc.start.as_second() as f64;
    let at = |k: f64| Timestamp::from_millisecond(((t_start + k) * 1000.0) as i64).expect("time");
    for (k, o) in offsets.iter().enumerate() {
        let t = k as f64;
        // Script.
        let mut events: Vec<Input> = Vec::new();
        let mut keep = Vec::new();
        for st in steps.drain(..) {
            let fire = match &st {
                Step::Stop { at_s, secs } if sk.s >= *at_s && stop_until.is_none() => {
                    stop_until = Some(t + secs);
                    run.stops.push((t, *secs));
                    true
                }
                Step::Close { at_s, ways } if sk.s >= *at_s => {
                    events.push(Input::Closed {
                        t: at(t),
                        ways: ways.clone(),
                    });
                    true
                }
                Step::Vehicle { at_s } if sk.s >= *at_s => {
                    vehicle = true;
                    events.push(Input::Activity {
                        t: at(t),
                        activity: Activity::InVehicle,
                    });
                    true
                }
                Step::TurnBack { at_s } if sk.s >= *at_s => {
                    let mut l = sk.course.prefix(sk.s);
                    let back = reversed(&l);
                    l.extend(back);
                    let s = sk.s;
                    sk.set_course(Course::new(g, &l, &plan.eta)?, ratio);
                    sk.s = s;
                    sk.back = 0.0;
                    sk.back_left = 0.0;
                    true
                }
                Step::Gap { at_s, secs } if sk.s >= *at_s => {
                    gap_until = Some(t + secs);
                    true
                }
                Step::Excursion {
                    at_s,
                    to,
                    speed_mps,
                } if sk.s >= *at_s && excursion.is_none() => {
                    excursion = Some((plane.xy(sk.pos()), plane.xy(*to), *speed_mps, 0.0));
                    true
                }
                Step::RefuseNext { after_s } => {
                    if let Some(r) = refuse_at
                        && t >= r
                    {
                        events.push(Input::Refuse { t: at(t) });
                        refuse_at = None;
                        true
                    } else {
                        let _ = after_s;
                        false
                    }
                }
                Step::IgnoreNext => {
                    ignore_next = true;
                    true
                }
                Step::WaitAtStart { .. }
                | Step::ArriveByCar { .. }
                | Step::WrongTurn { .. }
                | Step::Answer { .. }
                | Step::AnswerProfile { .. } => true,
                _ => false,
            };
            if !fire {
                keep.push(st);
            }
        }
        steps = keep;
        if let Some((at_t, accept)) = answer_at
            && t >= at_t
        {
            events.push(Input::Answer { t: at(t), accept });
            answer_at = None;
        }
        if let Some((at_t, accept)) = profile_at
            && t >= at_t
        {
            events.push(Input::ProfileAnswer { t: at(t), accept });
            profile_at = None;
        }
        // Motion.
        let mut pos = sk.pos();
        let mut moving = false;
        let mut striding = 0.0;
        if let Some((from, dist)) = car {
            // 15 m/s towards the start, then out of the car.
            let left = dist - 15.0 * t;
            if left > 0.0 {
                let p0 = plane.xy(from);
                let p1 = plane.xy(start_pos);
                let f = 1.0 - left / dist;
                pos = plane.lonlat([p0[0] + (p1[0] - p0[0]) * f, p0[1] + (p1[1] - p0[1]) * f]);
                moving = true;
            } else {
                car = None;
                waiting = (t + waiting).max(t);
            }
        } else if vehicle {
            // Leaves at 15 m/s eastwards.
            let p = plane.xy(sk.pos());
            pos = plane.lonlat([p[0] + 15.0 * (t - finished_at.unwrap_or(t)), p[1]]);
            moving = true;
            if finished_at.is_none() {
                finished_at = Some(t);
            }
        } else if t < waiting {
        } else if let Some(u) = stop_until {
            if t >= u {
                stop_until = None;
            }
        } else if let Some((a, b, v, done)) = &mut excursion {
            // Straight out and straight back.
            let len = (b[0] - a[0]).hypot(b[1] - a[1]).max(1e-9);
            *done += *v;
            let f = if *done <= len {
                *done / len
            } else {
                (2.0 - *done / len).max(0.0)
            };
            pos = plane.lonlat([a[0] + (b[0] - a[0]) * f, a[1] + (b[1] - a[1]) * f]);
            moving = true;
            striding = 0.5;
            if *done >= 2.0 * len {
                excursion = None;
            }
        } else if !sk.done() {
            let (_, v) = sk.here();
            let v = v * speed[sk.here().0 as usize] / sc.slowdown;
            if sk.back_left > 0.0 {
                // A detour: back along the route…
                sk.back_left -= v;
                sk.back += v;
            } else if sk.back > 0.0 {
                // …and forward again to where it left.
                sk.back = (sk.back - v).max(0.0);
            } else {
                sk.step(1.0, sc.slowdown, speed);
                if let Some(b) = bh {
                    if grng.unit() < b.stops_per_h / 3600.0 {
                        let d = (b.stop_median_s.ln() + b.stop_sigma * grng.normal()).exp();
                        stop_until = Some(t + d);
                        run.stops.push((t, d));
                    } else if grng.unit() < b.detours_per_h / 3600.0 {
                        sk.back_left = -b.detour_m * (1.0 - grng.unit()).max(1e-9).ln();
                    }
                }
            }
            pos = sk.pos();
            moving = true;
            striding = match sk.here().0 {
                Terrain::Up => 1.0,
                Terrain::Flat => 0.5,
                Terrain::Down => 0.0,
            };
        }
        // Client: Android's activity recognition, with its latency: « still » after 30 s
        // without moving, « on foot » 10 s after moving again.
        if moving {
            still_for = 0.0;
            moving_for += 1.0;
            if said_still && moving_for >= 10.0 {
                said_still = false;
                events.push(Input::Activity {
                    t: at(t),
                    activity: Activity::OnFoot,
                });
            }
        } else {
            moving_for = 0.0;
            still_for += 1.0;
            if !said_still && still_for >= 30.0 && !vehicle {
                said_still = true;
                events.push(Input::Activity {
                    t: at(t),
                    activity: Activity::Still,
                });
            }
        }
        // Client: the significant motion sensor, armed by the request, fires once on
        // motion (one-shot sensor, re-armed by the next request asking for it).
        if req.is_some_and(|r| r.significant_motion) && moving && !fired {
            fired = true;
            events.push(Input::Activity {
                t: at(t),
                activity: Activity::SignificantMotion,
            });
        }
        if k % 10 == 0 {
            run.truth
                .push((t, sk.s, pos, tr.progress(), tr.distance_to_route()));
        }
        // GPS.
        let gnss = req.is_none_or(|r| r.gnss);
        let interval = req.map_or(1.0, |r| r.interval_s.max(1.0));
        if gnss && gap_until.is_none_or(|g| t >= g) && t - last_fix >= interval - 1e-9 {
            last_fix = t;
            let xy = plane.xy(pos);
            let q = plane.xy(start_pos);
            let noisy = plane.lonlat([xy[0] + o[0] - q[0], xy[1] + o[1] - q[1]]);
            batch.push(Input::Fix {
                t: at(t),
                pos: noisy,
            });
            run.fixes += 1;
        }
        // Client: the step counter, read every 10 s and delivered with the fixes.
        if sc.step_counter {
            steps_acc += striding;
            if k % 10 == 9 {
                batch.push(Input::Steps {
                    t: at(t),
                    steps: steps_acc as u32,
                });
                steps_acc -= f64::from(steps_acc as u32);
            }
        }
        let delay = req.map_or(0.0, |r| r.max_delay_s);
        let oldest = batch.first().map(|i| i.t());
        let due =
            oldest.is_some_and(|o| (at(t) - o).as_millis() as i64 as f64 / 1000.0 >= delay - 1e-9);
        let before = out.len();
        for e in &events {
            if let Some(o) = batch.first().map(Input::t)
                && o <= e.t()
            {
                // Pending fixes first, in time order.
                tr.push_batch(&batch, &mut out);
                run.inputs.append(&mut batch);
                run.batches += 1;
            }
            tr.push(e, &mut out);
            run.inputs.push(e.clone());
        }
        if due && !batch.is_empty() {
            tr.push_batch(&batch, &mut out);
            run.inputs.append(&mut batch);
            run.batches += 1;
        }
        for o in &out[before..] {
            match o {
                Output::Gps { request, .. } => {
                    req = Some(*request);
                    if request.significant_motion {
                        // Armed again.
                        fired = false;
                    }
                }
                Output::Proposal { .. } => {
                    if let Some((accept, after)) = answers.pop() {
                        answer_at = Some((t + after, accept));
                    }
                }
                Output::ProfileProposal { .. } => {
                    if let Some((accept, after)) = profile_answer {
                        profile_at = Some((t + after, accept));
                    }
                }
                Output::Switched { .. } => {
                    if steps.iter().any(|s| matches!(s, Step::RefuseNext { .. }))
                        && refuse_at.is_none()
                    {
                        let after = steps
                            .iter()
                            .find_map(|s| match s {
                                Step::RefuseNext { after_s } => Some(*after_s),
                                _ => None,
                            })
                            .unwrap_or(10.0);
                        refuse_at = Some(t + after);
                    } else if ignore_next {
                        ignore_next = false;
                    } else {
                        // Obedient: takes the new route from where it is.
                        let new = Course::new(g, tr.legs(), &plan.eta)?;
                        let here = sk.pos();
                        let cur = tr.progress();
                        let s = new
                            .line()
                            .project(here, cur - 300.0, cur + 3000.0)
                            .filter(|(_, d)| *d < 60.0)
                            .or_else(|| new.line().project(here, 0.0, new.length_m()))
                            .map_or(cur, |(s, _)| s);
                        sk.set_course(new, ratio);
                        sk.s = s;
                        sk.back = 0.0;
                        sk.back_left = 0.0;
                    }
                }
                Output::Session {
                    event: crate::session::SessionEvent::Finished { .. },
                    ..
                } => {
                    finished_at.get_or_insert(t);
                }
                _ => {}
            }
        }
        if finished_at.is_some_and(|f| t - f >= sc.tail_s.min(120.0)) && !vehicle {
            break;
        }
        if vehicle && finished_at.is_some_and(|f| t - f >= 120.0) {
            break;
        }
        if sk.done() && stop_until.is_none() && finished_at.is_none() {
            // Stands at the finish.
            stop_until = Some(t + sc.tail_s);
        }
        run.t1 = at(t);
    }
    if !batch.is_empty() {
        tr.push_batch(&batch, &mut out);
        run.inputs.append(&mut batch);
        run.batches += 1;
    }
    run.outputs = out;
    Ok(run)
}

/// Replays recorded inputs (determinism, client tests).
pub fn replay(
    inputs: &[Input],
    plan: &DayPlan,
    track: TrackView<'_>,
    g: &PisteGraph,
    params: TrackerParams,
) -> Result<Vec<Output>> {
    replay_with_layers(inputs, plan, track, g, params, None)
}

/// [`replay`] with the layers of the day ([`run_with_layers`]).
pub fn replay_with_layers(
    inputs: &[Input],
    plan: &DayPlan,
    track: TrackView<'_>,
    g: &PisteGraph,
    params: TrackerParams,
    layers: Option<&SiteData>,
) -> Result<Vec<Output>> {
    replay_with(inputs, plan, track, g, params, layers, None)
}

/// [`replay_with_layers`] with the terrain slope as well ([`run_with`]).
pub fn replay_with(
    inputs: &[Input],
    plan: &DayPlan,
    track: TrackView<'_>,
    g: &PisteGraph,
    params: TrackerParams,
    layers: Option<&SiteData>,
    slope: Option<&Grid<u8>>,
) -> Result<Vec<Output>> {
    let mut tr = Tracker::new(plan, track, g, params)?;
    if let Some(d) = layers {
        tr = tr.with_layers(d);
    }
    if let Some(s) = slope {
        tr = tr.with_slope(s);
    }
    let mut out = Vec::new();
    for i in inputs {
        tr.push(i, &mut out);
    }
    Ok(out)
}

/// Terrain slope at a position (`slope_5m.skr`: half degrees, Lambert-93), degrees; 0
/// where unknown.
pub fn slope_deg(g: &Grid<u8>, p: LonLat) -> f64 {
    skimap_common::crs::to_lambert93(p)
        .ok()
        .and_then(|(x, y)| g.value_at(x, y).copied())
        .filter(|v| *v != skimap_core::offpiste::SLOPE_NODATA)
        .map_or(0.0, |v| f64::from(v) / 2.0)
}

/// Where a skier can leave the loop of a plan for steep terrain away from every trail (K6
/// #56, [`Step::Excursion`]): the distance along the loop (between 20 and 80 % of it), the
/// farthest point and its distance from the loop. Straight from the loop, square to it:
/// under 27° up to a stretch of at least 60 m at 31° or more, every point beyond the first
/// 40 m at 25 m or more from any trail; the farthest point 60 m into that stretch (one
/// passage out and back). `None`: no such terrain near the loop.
pub fn steep_excursion(
    plan: &DayPlan,
    g: &PisteGraph,
    slope: &Grid<u8>,
) -> Result<Option<(f64, LonLat, f64)>> {
    let c = Course::new(g, &plan.route.legs, &plan.eta)?;
    let len = c.length_m();
    let mut s = 0.2 * len;
    while s < 0.8 * len {
        let plane = LocalPlane::new(c.line().point_at(s));
        let a = plane.xy(c.line().point_at(s - 10.0));
        let z = plane.xy(c.line().point_at(s + 10.0));
        let (dx, dy) = (z[0] - a[0], z[1] - a[1]);
        let n = dx.hypot(dy).max(1e-9);
        for side in [1.0, -1.0] {
            let (ux, uy) = (-dy / n * side, dx / n * side);
            let at = |d: f64| plane.lonlat([ux * d, uy * d]);
            let mut run = 0.0;
            let mut d = 0.0;
            while d <= 1000.0 {
                let q = at(d);
                if d >= 40.0 && g.snap(q, 25.0).is_some() {
                    break;
                }
                let v = slope_deg(slope, q);
                if v >= 31.0 {
                    run += 5.0;
                    if run >= 60.0 {
                        return Ok(Some((s, at(d), d)));
                    }
                } else if run > 0.0 || v >= 27.0 {
                    break;
                }
                d += 5.0;
            }
        }
        s += 25.0;
    }
    Ok(None)
}

/// A way off the loop into a protected zone ([`zone_excursion`]).
#[derive(Debug, Clone, PartialEq)]
pub struct ZoneExcursion {
    /// Where it leaves the loop, metres along it.
    pub at_s: f64,
    /// Farthest point.
    pub to: LonLat,
    /// Its distance from the loop, metres.
    pub far_m: f64,
    /// The zone's identifier.
    pub zone: String,
    /// The loop is already inside the zone where the skier leaves it.
    pub from_inside: bool,
}

/// Where a skier can leave the loop of a plan into a protected zone away from every trail
/// (K6 #56, [`Step::Excursion`]): a protected zone of the layers
/// ([`crate::plan::is_protected`]) with rules for a day outing at the departure
/// ([`crate::plan::zone_rules`]). Straight from the loop (between 20 and 80 % of it),
/// square to it, up to 1 km: every point beyond the first 40 m at `clear_m` or more from
/// any trail, 100 to 300 m inside the zone (one entry out and back), the deepest such way
/// in. `None`: no such zone near the loop.
pub fn zone_excursion(
    plan: &DayPlan,
    g: &PisteGraph,
    data: &SiteData,
    clear_m: f64,
) -> Result<Option<ZoneExcursion>> {
    let Some((layer, rules)) = &data.zones else {
        return Ok(None);
    };
    let at_t = departure_utc(plan);
    let c = Course::new(g, &plan.route.legs, &plan.eta)?;
    // Depth inside the zone: at least, at most, metres.
    const MIN_IN_M: f64 = 100.0;
    const MAX_IN_M: f64 = 300.0;
    let len = c.length_m();
    let mut best: Option<(f64, ZoneExcursion)> = None;
    let mut s = 0.2 * len;
    while s < 0.8 * len {
        let plane = LocalPlane::new(c.line().point_at(s));
        let a = plane.xy(c.line().point_at(s - 10.0));
        let z = plane.xy(c.line().point_at(s + 10.0));
        let (dx, dy) = (z[0] - a[0], z[1] - a[1]);
        let n = dx.hypot(dy).max(1e-9);
        for side in [1.0, -1.0] {
            let (ux, uy) = (-dy / n * side, dx / n * side);
            let at = |d: f64| plane.lonlat([ux * d, uy * d]);
            let mut zone: Option<(usize, f64)> = None;
            let mut far: Option<f64> = None;
            let mut d = 0.0;
            while d <= 1000.0 {
                let q = at(d);
                if d >= 40.0 && g.snap(q, clear_m).is_some() {
                    break;
                }
                match zone {
                    Some((k, from)) => {
                        if !layer.zones[k].contains(q) || d - from > MAX_IN_M {
                            break;
                        }
                        if d - from >= MIN_IN_M {
                            far = Some(d);
                        }
                    }
                    None if d >= 40.0 => {
                        let ev = skimap_core::zone_eval::evaluate(layer, rules, q, at_t);
                        zone = layer
                            .zones
                            .iter()
                            .enumerate()
                            .filter(|(_, z)| crate::plan::is_protected(z) && z.contains(q))
                            .find(|(_, z)| {
                                !crate::plan::zone_rules(&ev, rules, z.id()).0.is_empty()
                            })
                            .map(|(k, _)| (k, d));
                    }
                    None => {}
                }
                d += 10.0;
            }
            if let (Some((k, from)), Some(d)) = (zone, far)
                && best.as_ref().is_none_or(|b| d - from > b.0)
            {
                let start = layer.zones[k].contains(at(0.0));
                best = Some((
                    d - from,
                    ZoneExcursion {
                        at_s: s,
                        to: at(d),
                        far_m: d,
                        zone: layer.zones[k].id().to_owned(),
                        from_inside: start,
                    },
                ));
            }
        }
        s += 50.0;
    }
    Ok(best.map(|b| b.1))
}

/// Seconds from the start of a run.
pub fn rel(run: &Run, t: Timestamp) -> f64 {
    (t - run.t0).as_millis() as i64 as f64 / 1000.0
}

/// Local time of a run instant.
pub fn local(t: Timestamp) -> DateTime {
    skimap_core::ephemeris::local_time(t)
}

/// One line per output, seconds from the start (for reading a run).
pub fn timeline(run: &Run) -> Vec<String> {
    run.outputs
        .iter()
        .map(|o| {
            let s = rel(run, o.t());
            let what = match o {
                Output::Spoken { spoken, .. } => format!(
                    "dit [{}] « {} »",
                    spoken
                        .items
                        .iter()
                        .map(|c| c.kind.as_str())
                        .collect::<Vec<_>>()
                        .join(", "),
                    spoken.announcement.text()
                ),
                Output::Session { event, .. } => format!("sortie : {event:?}"),
                Output::Alert { alert, .. } => format!("alerte : {alert:?}"),
                Output::Switched {
                    to, cause, s_div, ..
                } => {
                    format!("bascule vers {to:?} ({cause:?}) à {:.0} m", s_div)
                }
                Output::Reverted { to, implicit, .. } => {
                    format!(
                        "refus {} : retour à {to:?}",
                        if *implicit { "implicite" } else { "par geste" }
                    )
                }
                Output::Gps { request, .. } => {
                    format!(
                        "GPS {:?} {} s, lot {:.0} s",
                        request.mode, request.interval_s, request.max_delay_s
                    )
                }
                Output::Recap { recap, .. } => format!("bilan : {}", recap.text),
                Output::Proposal { proposal, .. } => format!(
                    "proposition {} {:?} ({:?}, {:?}) : {}",
                    proposal.id, proposal.kind, proposal.route, proposal.reason, proposal.text
                ),
                Output::Answered {
                    kind,
                    accept,
                    applied,
                    ..
                } => format!(
                    "réponse {:?} : {}{}",
                    kind,
                    if *accept { "acceptée" } else { "refusée" },
                    if *accept && !*applied {
                        " (sans effet)"
                    } else {
                        ""
                    }
                ),
                Output::ProfileProposal { proposal, .. } => format!(
                    "profil proposé {} ({:.1}) : {}",
                    proposal.profile.name, proposal.log_ratio, proposal.text
                ),
                Output::ProfileAnswered {
                    accept, profile, ..
                } => format!(
                    "profil {} {}",
                    if *accept { "choisi" } else { "refusé" },
                    profile.as_ref().map_or("", |p| p.name.as_str())
                ),
            };
            format!(
                "{:>6.0} s  {}  {what}",
                s,
                local(o.t()).strftime("%H:%M:%S")
            )
        })
        .collect()
}

/// The default start of a plan's scenario: its departure (local) in UTC.
pub fn departure_utc(plan: &DayPlan) -> Timestamp {
    skimap_assist::sun::local_to_utc(plan.departure)
}

/// A start at another local time of the plan's day.
pub fn start_at(plan: &DayPlan, hour: u32, minute: u32) -> Timestamp {
    let t = plan
        .date
        .to_datetime(skimap_common::dates::hms(hour, minute, 0).unwrap());
    skimap_assist::sun::local_to_utc(t) + SignedDuration::ZERO
}
