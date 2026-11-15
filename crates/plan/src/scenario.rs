//! Scenario bench (Q9, #109): synthetic outings on the real trail graphs replayed through
//! the tracker with a simulated clock, in closed loop.
//!
//! - **The skier** moves along a course of the graph at the C5 model pace divided by a
//!   slowdown (one step per simulated second), with scripted events ([`Step`]): wait at
//!   the start, arrive by car, take a wrong branch and come back or ski on, stop (at a
//!   refuge or not), leave in a vehicle, turn back on foot, lose the GPS. Obedient by
//!   default: when the tracker switches route, the skier takes the new one; a scripted
//!   refusal keeps the previous one.
//! - **The GPS**: the true position plus the noise of C2 (`matching::synth`: Gauss–Markov
//!   error per axis and forest outliers), drawn once per second from a seed.
//! - **The client** applies the tracker's GPS requests ([`crate::gps`]): fix interval,
//!   batch delay (fixes delivered by batch with [`Tracker::push_batch`]), GNSS off during
//!   a pause, significant motion when the skier moves again.
//!
//! A [`Run`] keeps every input given to the tracker and every output, so that a run can be
//! replayed (the client's instrumented tests use the same inputs and expected outputs).

use chrono::{DateTime, Duration, NaiveDateTime, Utc};
use serde::{Deserialize, Serialize};
use skimap_common::geom::LonLat;
use skimap_core::graph::{HalfEdge, LocalPlane, PisteGraph};
use skimap_core::matching::synth::{Noise, Rng, Truth, add_noise};

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
    /// No fix for a while from this distance.
    Gap {
        /// Distance, metres.
        at_s: f64,
        /// Seconds.
        secs: f64,
    },
}

/// A scenario.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Scenario {
    /// Name.
    pub name: String,
    /// Start of the skier (UTC); the plan's departure by default.
    pub start: DateTime<Utc>,
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
    pub t0: DateTime<Utc>,
    /// End of the simulation.
    pub t1: DateTime<Utc>,
    /// Fixes given.
    pub fixes: usize,
    /// Batches given.
    pub batches: usize,
    /// Every 10 s: seconds from the start, the skier's distance along its course and
    /// position, the tracker's cursor and distance to the route.
    pub truth: Vec<(f64, f64, LonLat, f64, f64)>,
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
}

impl Skier {
    /// Moves `dt` seconds at the model pace divided by `slow`.
    fn step(&mut self, dt: f64, slow: f64) {
        let p = self.course.profile();
        let scale = if self.course.length_m() > 0.0 {
            p.length_m() / self.course.length_m()
        } else {
            1.0
        };
        let tau = self.course.model_s_at(self.s) + dt / slow;
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
        self.course.line().point_at(self.s)
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
    let mut tr = Tracker::new(plan, track, g, None, params)?;
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
    let mut sk = Skier {
        course: Course::new(g, &legs, &plan.eta)?,
        s: 0.0,
    };
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
    let mut gap_until: Option<f64> = None;
    let mut vehicle = false;
    let mut fired = false;
    let (mut still_for, mut moving_for, mut said_still) = (0.0, 0.0, false);
    let mut refuse_at: Option<f64> = None;
    let mut ignore_next = false;
    let mut finished_at: Option<f64> = None;
    let t_start = sc.start.timestamp() as f64;
    let at = |k: f64| {
        DateTime::<Utc>::from_timestamp_millis(((t_start + k) * 1000.0) as i64).expect("time")
    };
    for (k, o) in offsets.iter().enumerate() {
        let t = k as f64;
        // Script.
        let mut events: Vec<Input> = Vec::new();
        let mut keep = Vec::new();
        for st in steps.drain(..) {
            let fire = match &st {
                Step::Stop { at_s, secs } if sk.s >= *at_s && stop_until.is_none() => {
                    stop_until = Some(t + secs);
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
                    sk.course = Course::new(g, &l, &plan.eta)?;
                    sk.s = s;
                    true
                }
                Step::Gap { at_s, secs } if sk.s >= *at_s => {
                    gap_until = Some(t + secs);
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
                Step::WaitAtStart { .. } | Step::ArriveByCar { .. } | Step::WrongTurn { .. } => {
                    true
                }
                _ => false,
            };
            if !fire {
                keep.push(st);
            }
        }
        steps = keep;
        // Motion.
        let mut pos = sk.pos();
        let mut moving = false;
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
        } else if !sk.done() {
            sk.step(1.0, sc.slowdown);
            pos = sk.pos();
            moving = true;
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
        let delay = req.map_or(0.0, |r| r.max_delay_s);
        let oldest = batch.first().map(|i| i.t());
        let due =
            oldest.is_some_and(|o| (at(t) - o).num_milliseconds() as f64 / 1000.0 >= delay - 1e-9);
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
                        sk.course = new;
                        sk.s = s;
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
    let mut tr = Tracker::new(plan, track, g, None, params)?;
    let mut out = Vec::new();
    for i in inputs {
        tr.push(i, &mut out);
    }
    Ok(out)
}

/// Seconds from the start of a run.
pub fn rel(run: &Run, t: DateTime<Utc>) -> f64 {
    (t - run.t0).num_milliseconds() as f64 / 1000.0
}

/// Local time of a run instant.
pub fn local(t: DateTime<Utc>) -> NaiveDateTime {
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
            };
            format!("{:>6.0} s  {}  {what}", s, local(o.t()).format("%H:%M:%S"))
        })
        .collect()
}

/// The default start of a plan's scenario: its departure (local) in UTC.
pub fn departure_utc(plan: &DayPlan) -> DateTime<Utc> {
    skimap_assist::sun::local_to_utc(plan.departure)
}

/// A start at another local time of the plan's day.
pub fn start_at(plan: &DayPlan, hour: u32, minute: u32) -> DateTime<Utc> {
    let t = plan.date.and_hms_opt(hour, minute, 0).expect("time");
    skimap_assist::sun::local_to_utc(t) + Duration::zero()
}
