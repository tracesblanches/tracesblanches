//! Start, pause, refuge and end of the outing without any manipulation (J3, #112): the
//! decision on the core side, from the fixes and, when the phone gives them, the activity
//! recognised by Android (`ActivityRecognition`: still, on foot, in a vehicle), the
//! significant motion sensor (report 09 « batterie ») and the steps counted by the phone
//! ([`Session::steps`], X10 #144) with the terrain of the route ([`Session::set_terrain`]).
//!
//! Positions are smoothed over blocks of [`SessionParams::block_s`] (the mean of the fixes
//! of the block): a stationary skier's fixes wander by the GPS error (σ 15 m and forest
//! outliers of 30 m in the C2 traces), their mean does not. No allocation per fix: the
//! blocks live in a ring reserved at the start, events go to the caller's buffer.
//!
//! | Decision | Rule |
//! |---|---|
//! | start | not in a vehicle; within [`SessionParams::start_radius_m`] of the plan's start and [`SessionParams::start_route_m`] of the route; the smoothed position moves [`SessionParams::move_m`] over [`SessionParams::move_s`]: the start is the beginning of that movement |
//! | pause | over [`SessionParams::pause_s`] (90 s), the mean position of the second half is within [`SessionParams::pause_ms`] (0.3 m/s, after the phone said « still » for [`SessionParams::still_pause_s`], or when it counts the steps and none came uphill or on the flat) or [`SessionParams::pause_fix_ms`] (0.15 m/s, the fixes alone; uphill without the steps, [`SessionParams::climb_pause_fix_ms`] over [`SessionParams::climb_pause_s`], 0.08 m/s over 180 s: a slow climb reads like a stop; longer and slower still for a skier slower than that, from its expected speed [`Session::set_expected_speed`]: the time it takes to cover [`SessionParams::pause_span_m`], up to [`SessionParams::pause_max_s`], under [`SessionParams::pause_share`] of it, #144) × half the window of the first half's, the progress along the route (when on it) is below that too (hairpins), no smoothed position is farther than [`SessionParams::pause_m`] from the centre, and fewer than [`SessionParams::pause_steps`] steps over the window: the pause starts at the beginning of that window, or when the striding ended if the skier stayed there since |
//! | refuge | a pause within [`SessionParams::refuge_m`] (50 m) of a refuge of the plan lasting [`SessionParams::refuge_s`] (5 min): said at the first input after those 5 min (the GNSS is off in a pause, report 09), dated at their end |
//! | waking | significant motion, « on foot » or [`SessionParams::waking_steps`] steps during a pause: the GPS goes back to 1 s |
//! | resume | the smoothed position leaves the pause by [`SessionParams::resume_m`], dated when the striding came back (with the steps) or when the block that left started |
//! | near the finish | within [`SessionParams::finish_m`] of the finish after having been [`SessionParams::away_m`] away from it (the arrival is announced) |
//! | distance | the smoothed position every [`SessionParams::distance_s`] while moving (fewer, longer steps: the GPS error adds little) |
//! | end | a pause near the finish; or a vehicle (the phone says so, or the smoothed speed stays above [`SessionParams::vehicle_ms`] for [`SessionParams::vehicle_s`]): the outing is abandoned unless near the finish |

use std::collections::VecDeque;

use serde::{Deserialize, Serialize};
use skimap_common::geom::LonLat;
use skimap_core::calibration::Terrain;

/// Activity recognised by the phone.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Activity {
    /// Still.
    Still,
    /// On foot (walking, running, skiing as Android sees it).
    OnFoot,
    /// In a vehicle.
    InVehicle,
    /// Unknown, tilting, bicycle.
    Unknown,
    /// Significant motion sensor fired (report 09).
    SignificantMotion,
}

/// Settings (report 09, section 5; to check on the field, T5).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct SessionParams {
    /// Smoothing block, seconds.
    pub block_s: f64,
    /// Start: distance to the plan's start, metres.
    pub start_radius_m: f64,
    /// Start: distance to the route, metres.
    pub start_route_m: f64,
    /// Start: movement, metres…
    pub move_m: f64,
    /// …over this time, seconds.
    pub move_s: f64,
    /// Pause after the phone said « still »: smoothed speed below this, m/s (report 09:
    /// 0.3 m/s)…
    pub pause_ms: f64,
    /// Pause from the fixes alone: speed below this, m/s (the GPS error alone reads 0.2 to
    /// 0.3 m/s on a skier standing still, σ 10 m and forest outliers; a slow climb on
    /// hairpins reads little more).
    pub pause_fix_ms: f64,
    /// …and no smoothed position farther than this from the centre, metres…
    pub pause_m: f64,
    /// …for this long, seconds.
    pub pause_s: f64,
    /// Same after the phone said « still », seconds.
    pub still_pause_s: f64,
    /// Resume: distance from the pause, metres.
    pub resume_m: f64,
    /// Refuge: distance of a pause to a refuge, metres…
    pub refuge_m: f64,
    /// …for this long, seconds.
    pub refuge_s: f64,
    /// Near the finish, metres.
    pub finish_m: f64,
    /// Away from the finish before coming back to it, metres.
    pub away_m: f64,
    /// Distance: step of the coarse track, seconds.
    pub distance_s: f64,
    /// Vehicle: smoothed speed, m/s…
    pub vehicle_ms: f64,
    /// …held this long, seconds.
    pub vehicle_s: f64,
    /// Pause: fewer steps than this over its window, when the phone counts them (X10: a
    /// skier climbing slowly strides, a stopped one does not).
    pub pause_steps: u32,
    /// Waking: this many steps during a pause (as the significant motion sensor).
    pub waking_steps: u32,
    /// On a climb, the fixes alone: speed below this, m/s (a slow climb in hairpins reads
    /// little more than the GPS error over the window)…
    pub climb_pause_fix_ms: f64,
    /// …over this window, seconds.
    pub climb_pause_s: f64,
    /// The fixes alone, with the skier's expected speed here ([`Session::set_expected_speed`],
    /// #144): the window lasts the time the skier takes to cover this at that speed (0:
    /// the fixed windows above), metres…
    pub pause_span_m: f64,
    /// …between [`SessionParams::pause_s`] and this, seconds…
    pub pause_max_s: f64,
    /// …and the speed is below this share of the expected speed…
    pub pause_share: f64,
    /// …but not below this, m/s (nor above [`SessionParams::pause_fix_ms`]).
    pub pause_min_ms: f64,
}

impl Default for SessionParams {
    fn default() -> Self {
        Self {
            block_s: 10.0,
            start_radius_m: 300.0,
            start_route_m: 40.0,
            move_m: 40.0,
            move_s: 40.0,
            pause_ms: 0.3,
            pause_fix_ms: 0.15,
            pause_m: 40.0,
            pause_s: 90.0,
            still_pause_s: 30.0,
            resume_m: 40.0,
            refuge_m: 50.0,
            refuge_s: 300.0,
            finish_m: 120.0,
            away_m: 500.0,
            distance_s: 30.0,
            vehicle_ms: 10.0,
            vehicle_s: 60.0,
            pause_steps: 10,
            waking_steps: 5,
            climb_pause_fix_ms: 0.08,
            climb_pause_s: 180.0,
            pause_span_m: 150.0,
            pause_max_s: 360.0,
            pause_share: 0.2,
            pause_min_ms: 0.04,
        }
    }
}

/// Phase of the outing.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Phase {
    /// Before the start.
    Waiting,
    /// Skiing.
    Moving,
    /// Stopped.
    Paused {
        /// Since, seconds.
        since: f64,
        /// Refuge, once recognised (`""`: an unnamed shelter).
        refuge: Option<String>,
    },
    /// Over.
    Finished,
}

/// How the outing ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum End {
    /// Back at the finish.
    Arrived,
    /// Left in a vehicle away from the finish.
    Abandoned,
}

/// A decision.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SessionEvent {
    /// The outing starts (at the beginning of the movement).
    Started {
        /// Time, seconds.
        t: f64,
        /// Where.
        at: LonLat,
    },
    /// A pause starts.
    Paused {
        /// Start of the pause, seconds.
        t: f64,
    },
    /// The pause is at a refuge.
    Refuge {
        /// Time of the recognition, seconds.
        t: f64,
        /// Refuge (`""`: an unnamed shelter).
        name: String,
    },
    /// The skier moves again.
    Resumed {
        /// Time, seconds.
        t: f64,
        /// Length of the pause, seconds.
        paused_s: f64,
    },
    /// Near the finish.
    NearFinish {
        /// Time, seconds.
        t: f64,
    },
    /// The outing is over.
    Finished {
        /// Time, seconds.
        t: f64,
        /// How.
        end: End,
    },
}

/// Totals of the outing so far.
#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
pub struct Totals {
    /// Start, seconds.
    pub started: Option<f64>,
    /// End, seconds.
    pub finished: Option<f64>,
    /// Time stopped, seconds.
    pub paused_s: f64,
    /// Distance of the smoothed track while moving, metres.
    pub distance_m: f64,
    /// Pauses.
    pub pauses: usize,
}

#[derive(Debug, Clone, Copy)]
struct Block {
    t0: f64,
    t1: f64,
    pos: LonLat,
    /// Distance along the route at the end of the block, when known.
    s: Option<f64>,
    /// Steps counted during the block.
    steps: u32,
}

/// The state machine.
#[derive(Debug, Clone)]
pub struct Session {
    p: SessionParams,
    start: LonLat,
    finish: LonLat,
    refuges: Vec<(Option<String>, LonLat)>,
    phase: Phase,
    /// Current block: first time, sums, count.
    acc: Option<(f64, f64, f64, usize)>,
    blocks: VecDeque<Block>,
    keep_s: f64,
    /// Blocks the start looks back on (the rest at the start).
    start_keep_s: f64,
    pause_at: Option<LonLat>,
    still_since: Option<f64>,
    waking: bool,
    vehicle: bool,
    fast_since: Option<f64>,
    away: bool,
    near: bool,
    /// Refuge found at the pause, said after `refuge_s`.
    refuge_due: Option<String>,
    /// Last distance along the route, and the lowest one while waiting (the rest).
    route_s: Option<f64>,
    rest_s: Option<f64>,
    /// Sum and count of the distances along the route in the current block.
    s_acc: (f64, usize),
    /// Coarse track: current sums (first time, lon, lat, count) and last position.
    coarse: Option<(f64, f64, f64, usize)>,
    coarse_last: Option<LonLat>,
    totals: Totals,
    /// Steps counted since the last block, and during the pause.
    acc_steps: u32,
    pause_steps: u32,
    /// Terrain of the route here (the tracker says so from the plan's profile).
    terrain: Terrain,
    /// Expected moving speed of the skier here, m/s (#144).
    expected_ms: Option<f64>,
    /// The phone counts steps (a report came).
    counts_steps: bool,
    /// Last report of the steps, and the start of the striding in progress (reports of
    /// [`STRIDE_STEPS`] steps or more in a row).
    steps_t: Option<f64>,
    stride_from: Option<f64>,
    /// Last report of striding.
    stride_last: Option<f64>,
}

/// A report of the steps with this many is striding (a few steps are fidgeting).
const STRIDE_STEPS: u32 = 3;

impl Session {
    /// Session of a plan from `start` to `finish` with its refuges.
    pub fn new(
        p: SessionParams,
        start: LonLat,
        finish: LonLat,
        refuges: Vec<(Option<String>, LonLat)>,
    ) -> Self {
        let start_keep_s = p.pause_s.max(p.move_s).max(p.vehicle_s) + 3.0 * p.block_s;
        let keep_s = start_keep_s.max(p.climb_pause_s.max(p.pause_max_s) + 3.0 * p.block_s);
        // Blocks of at least a second each: the ring never grows past this.
        let cap = (keep_s / p.block_s.max(1.0)).ceil() as usize * 2 + 8;
        Self {
            p,
            start,
            finish,
            refuges,
            phase: Phase::Waiting,
            acc: None,
            blocks: VecDeque::with_capacity(cap),
            keep_s,
            start_keep_s,
            pause_at: None,
            still_since: None,
            waking: false,
            vehicle: false,
            fast_since: None,
            away: false,
            near: false,
            refuge_due: None,
            route_s: None,
            rest_s: None,
            s_acc: (0.0, 0),
            coarse: None,
            coarse_last: None,
            totals: Totals::default(),
            acc_steps: 0,
            pause_steps: 0,
            terrain: Terrain::Flat,
            expected_ms: None,
            counts_steps: false,
            steps_t: None,
            stride_from: None,
            stride_last: None,
        }
    }

    /// Terrain of the route where the skier is (X10): on a climb, the fixes alone need a
    /// slower speed over a longer window to decide a pause; with the steps, no stride
    /// uphill or on the flat says the skier stopped (downhill, one glides without steps).
    pub fn set_terrain(&mut self, terrain: Terrain) {
        self.terrain = terrain;
    }

    /// Expected moving speed of the skier where it is, m/s: its pace of the day on the
    /// terrain of the route (#144). With the fixes alone, the pause window lasts the time
    /// it takes at that speed to cover [`SessionParams::pause_span_m`], and the speed
    /// threshold is [`SessionParams::pause_share`] of it: a slow climber at 1 km/h moves
    /// as far over its window as a skier at 10 km/h over 90 s, beyond the GPS error.
    pub fn set_expected_speed(&mut self, v_ms: Option<f64>) {
        self.expected_ms = v_ms.filter(|v| v.is_finite() && *v > 0.0);
    }

    /// Steps counted by the phone since its previous report.
    pub fn steps(&mut self, t: f64, n: u32, out: &mut Vec<SessionEvent>) {
        self.refuge_now(t, out);
        self.counts_steps = true;
        if n >= STRIDE_STEPS {
            // The steps of the report since the previous one (15 s at most).
            let from = self.steps_t.map_or(t, |p| p.max(t - 15.0));
            self.stride_from.get_or_insert(from);
            self.stride_last = Some(t);
        } else {
            self.stride_from = None;
        }
        self.steps_t = Some(t);
        self.acc_steps = self.acc_steps.saturating_add(n);
        if self.paused() {
            self.pause_steps = self.pause_steps.saturating_add(n);
            if self.pause_steps >= self.p.waking_steps {
                self.waking = true;
            }
        }
    }

    /// Phase.
    pub fn phase(&self) -> &Phase {
        &self.phase
    }

    /// Totals.
    pub fn totals(&self) -> Totals {
        self.totals
    }

    /// Whether the outing is under way (started, not finished).
    pub fn running(&self) -> bool {
        matches!(self.phase, Phase::Moving | Phase::Paused { .. })
    }

    /// Whether paused.
    pub fn paused(&self) -> bool {
        matches!(self.phase, Phase::Paused { .. })
    }

    /// Whether paused at a recognised refuge.
    pub fn at_refuge(&self) -> bool {
        matches!(
            self.phase,
            Phase::Paused {
                refuge: Some(_),
                ..
            }
        )
    }

    /// Whether a significant motion came during the pause.
    pub fn waking(&self) -> bool {
        self.waking
    }

    /// Last smoothed position.
    pub fn smoothed(&self) -> Option<LonLat> {
        self.blocks.back().map(|b| b.pos)
    }

    fn window(&self, span: f64) -> Option<(&Block, &Block)> {
        let last = self.blocks.back()?;
        let first = self.blocks.iter().rev().find(|b| last.t1 - b.t0 >= span)?;
        Some((first, last))
    }

    /// Smoothed speed over the last `span_s` seconds, m/s.
    pub fn speed(&self, span_s: f64) -> Option<f64> {
        let (a, b) = self
            .window(span_s)
            .or_else(|| Some((self.blocks.front()?, self.blocks.back()?)))?;
        let dt = b.t1 - a.t0;
        (dt > 0.0).then(|| a.pos.haversine_m(b.pos) / dt)
    }

    /// Smoothed heading over the last `span_s` seconds, degrees, when it moved 10 m.
    pub fn heading(&self, span_s: f64) -> Option<f64> {
        let last = self.blocks.back()?;
        let first = self
            .blocks
            .iter()
            .rev()
            .find(|b| last.t1 - b.t0 >= span_s)
            .or_else(|| self.blocks.front())?;
        (first.pos.haversine_m(last.pos) >= 10.0)
            .then(|| crate::line::bearing_deg(first.pos, last.pos))
    }

    /// Activity from the phone.
    pub fn activity(&mut self, t: f64, a: Activity, out: &mut Vec<SessionEvent>) {
        self.refuge_now(t, out);
        match a {
            Activity::Still => {
                self.still_since.get_or_insert(t);
            }
            Activity::InVehicle => {
                self.still_since = None;
                self.vehicle = true;
            }
            Activity::OnFoot | Activity::SignificantMotion => {
                self.still_since = None;
                self.vehicle = false;
                if self.paused() {
                    self.waking = true;
                }
            }
            Activity::Unknown => {
                self.still_since = None;
                self.vehicle = false;
            }
        }
        if a == Activity::InVehicle && self.running() {
            self.finish(t, out);
        }
    }

    /// The refuge of a pause, once it has lasted `refuge_s`.
    fn refuge_now(&mut self, t: f64, out: &mut Vec<SessionEvent>) {
        if let Phase::Paused {
            since,
            refuge: None,
        } = self.phase
            && self.refuge_due.is_some()
            && t - since >= self.p.refuge_s
        {
            let name = self.refuge_due.take().expect("due");
            self.phase = Phase::Paused {
                since,
                refuge: Some(name.clone()),
            };
            out.push(SessionEvent::Refuge {
                t: since + self.p.refuge_s,
                name,
            });
        }
    }

    fn finish(&mut self, t: f64, out: &mut Vec<SessionEvent>) {
        if let Phase::Paused { since, .. } = self.phase {
            self.totals.paused_s += (t - since).max(0.0);
        }
        let at = self.smoothed().unwrap_or(self.finish);
        let end = if at.haversine_m(self.finish) <= self.p.finish_m && self.away {
            End::Arrived
        } else {
            End::Abandoned
        };
        self.phase = Phase::Finished;
        self.totals.finished = Some(t);
        out.push(SessionEvent::Finished { t, end });
    }

    /// A fix, with its distance to the route (`None`: not projected).
    /// A fix, with its distance to the route and its own projection along it (`None`: not
    /// projected).
    pub fn fix_on(
        &mut self,
        t: f64,
        pos: LonLat,
        route_m: Option<f64>,
        route_s: Option<f64>,
        out: &mut Vec<SessionEvent>,
    ) {
        if let Some(s) = route_s {
            self.route_s = Some(s);
            self.s_acc.0 += s;
            self.s_acc.1 += 1;
            if self.phase == Phase::Waiting && self.rest_s.is_none_or(|r| s < r) {
                self.rest_s = Some(s);
            }
        }
        self.fix(t, pos, route_m, out);
    }

    /// A fix, with its distance to the route (`None`: not projected).
    pub fn fix(&mut self, t: f64, pos: LonLat, route_m: Option<f64>, out: &mut Vec<SessionEvent>) {
        if self.phase == Phase::Finished {
            return;
        }
        self.refuge_now(t, out);
        if self.phase == Phase::Moving {
            let (c0, clon, clat, cn) = self.coarse.get_or_insert((t, 0.0, 0.0, 0));
            *clon += pos.lon;
            *clat += pos.lat;
            *cn += 1;
            if t - *c0 >= self.p.distance_s {
                let (_, clon, clat, cn) = self.coarse.take().expect("coarse");
                let m = LonLat::new(clon / cn as f64, clat / cn as f64);
                if let Some(l) = self.coarse_last {
                    self.totals.distance_m += l.haversine_m(m);
                }
                self.coarse_last = Some(m);
            }
        }
        let (t0, slon, slat, n) = self.acc.get_or_insert((t, 0.0, 0.0, 0));
        *slon += pos.lon;
        *slat += pos.lat;
        *n += 1;
        if t - *t0 < self.p.block_s {
            return;
        }
        let (t0, slon, slat, n) = self.acc.take().expect("block");
        let b = Block {
            t0,
            t1: t,
            pos: LonLat::new(slon / n as f64, slat / n as f64),
            s: (self.s_acc.1 > 0).then(|| self.s_acc.0 / self.s_acc.1 as f64),
            steps: std::mem::take(&mut self.acc_steps),
        };
        self.s_acc = (0.0, 0);
        while self.blocks.front().is_some_and(|f| t - f.t0 > self.keep_s)
            || self.blocks.len() == self.blocks.capacity()
        {
            self.blocks.pop_front();
        }
        self.blocks.push_back(b);
        self.decide(t, route_m, out);
    }

    fn decide(&mut self, t: f64, route_m: Option<f64>, out: &mut Vec<SessionEvent>) {
        let last = *self.blocks.back().expect("a block");
        // Vehicle by speed.
        let fast = self
            .window(self.p.block_s * 3.0)
            .map(|(a, b)| a.pos.haversine_m(b.pos) / (b.t1 - a.t0).max(1.0))
            .is_some_and(|v| v > self.p.vehicle_ms);
        if fast {
            self.fast_since.get_or_insert(last.t0);
        } else {
            self.fast_since = None;
        }
        let vehicle = self.vehicle || self.fast_since.is_some_and(|f| t - f >= self.p.vehicle_s);
        match self.phase {
            Phase::Waiting => {
                if vehicle {
                    return;
                }
                // Not just out of a car: no step of the window faster than a skier.
                if let Some((a, _)) = self.window(self.p.move_s) {
                    let t_a = a.t0;
                    let mut prev: Option<&Block> = None;
                    for b in self.blocks.iter().filter(|b| b.t0 >= t_a) {
                        if let Some(p) = prev
                            && p.pos.haversine_m(b.pos) / (b.t1 - p.t1).max(1.0)
                                > 0.7 * self.p.vehicle_ms
                        {
                            self.rest_s = None;
                            return;
                        }
                        prev = Some(b);
                    }
                }
                let near = last.pos.haversine_m(self.start) <= self.p.start_radius_m;
                let on_route = route_m.is_some_and(|d| d <= self.p.start_route_m);
                let moved = self
                    .window(self.p.move_s)
                    .map(|(a, b)| (a.t0, a.pos, a.pos.haversine_m(b.pos)));
                // Along the route too, when the caller gives the distance along it: the
                // GPS error across the route does not start the outing.
                let along = match (self.route_s, self.rest_s) {
                    (Some(s), Some(r)) => s - r >= 0.75 * self.p.move_m,
                    _ => true,
                };
                if near
                    && on_route
                    && along
                    && let Some((t0, at, d)) = moved
                    && d >= self.p.move_m
                {
                    // Back to the end of the rest: the last block still within half of
                    // `move_m` of the first block of the start window (the skier standing
                    // at the start).
                    let rest = self.blocks.iter().find(|b| t - b.t0 <= self.start_keep_s);
                    let (t0, at) = match rest {
                        Some(rest) => self
                            .blocks
                            .iter()
                            .rev()
                            .find(|b| b.pos.haversine_m(rest.pos) <= 0.5 * self.p.move_m)
                            .filter(|b| b.t1 > t0 - self.start_keep_s)
                            .map_or((t0, at), |b| (b.t1.min(t), b.pos)),
                        None => (t0, at),
                    };
                    self.phase = Phase::Moving;
                    self.totals.started = Some(t0);
                    self.coarse_last = Some(at);
                    self.coarse = None;
                    out.push(SessionEvent::Started { t: t0, at });
                }
            }
            Phase::Moving => {
                if vehicle {
                    return self.finish(t, out);
                }
                let df = last.pos.haversine_m(self.finish);
                if df > self.p.away_m {
                    self.away = true;
                }
                if self.away && !self.near && df <= self.p.finish_m {
                    self.near = true;
                    out.push(SessionEvent::NearFinish { t });
                }
                // « Still » from the phone for `still_pause_s`: the threshold of report 09;
                // the fixes alone: a stricter one.
                let still_said = self
                    .still_since
                    .is_some_and(|s| s <= t - self.p.still_pause_s);
                let (span, limit) = if still_said {
                    (
                        self.p.pause_s.min(t - self.still_since.unwrap_or(t)),
                        self.p.pause_ms,
                    )
                } else if self.counts_steps && self.terrain != Terrain::Down {
                    // No stride (fewer than `pause_steps` over the window, below) where a
                    // skier strides: the threshold of « still ».
                    (self.p.pause_s, self.p.pause_ms)
                } else {
                    // The fixes alone: on a climb, a longer window and a lower speed (a slow
                    // climb reads like a stop)…
                    let (span, limit) = if self.terrain == Terrain::Up {
                        (self.p.climb_pause_s, self.p.climb_pause_fix_ms)
                    } else {
                        (self.p.pause_s, self.p.pause_fix_ms)
                    };
                    // …and slower than that for a skier slower than that (#144): as long as
                    // it takes at its own pace to move `pause_span_m`, under a share of it.
                    match self.expected_ms {
                        Some(v) if self.p.pause_span_m > 0.0 => (
                            span.max((self.p.pause_span_m / v).min(self.p.pause_max_s)),
                            limit.min((self.p.pause_share * v).max(self.p.pause_min_ms)),
                        ),
                        _ => (span, limit),
                    }
                };
                let Some((a, _)) = self.window(span) else {
                    return;
                };
                let since = a.t0;
                let mid = 0.5 * (since + last.t1);
                // Means of the two halves of the window, and of the whole.
                let (mut h, mut all) = ([(0.0, 0.0, 0usize); 2], (0.0, 0.0, 0usize));
                for b in self.blocks.iter().filter(|b| b.t0 >= since) {
                    let k = usize::from(b.t0 >= mid);
                    h[k].0 += b.pos.lon;
                    h[k].1 += b.pos.lat;
                    h[k].2 += 1;
                    all.0 += b.pos.lon;
                    all.1 += b.pos.lat;
                    all.2 += 1;
                }
                if h[0].2 == 0 || h[1].2 == 0 {
                    return;
                }
                let m = |x: (f64, f64, usize)| LonLat::new(x.0 / x.2 as f64, x.1 / x.2 as f64);
                let center = m(all);
                let speed = m(h[0]).haversine_m(m(h[1])) / (0.5 * (last.t1 - since)).max(1.0);
                // On the route, the progress along it decides (a hairpin brings the
                // skier back near where they were).
                // Means of the distances along the route of the two halves (each fix's own
                // projection: the cursor only goes forward and keeps the GPS error ahead).
                let (mut sa, mut sb) = ((0.0, 0usize), (0.0, 0usize));
                for b in self.blocks.iter().filter(|b| b.t0 >= since) {
                    if let Some(x) = b.s {
                        let h = if b.t0 >= mid { &mut sb } else { &mut sa };
                        h.0 += x;
                        h.1 += 1;
                    }
                }
                let along = if sa.1 > 0 && sb.1 > 0 {
                    (sb.0 / sb.1 as f64 - sa.0 / sa.1 as f64) / (0.5 * (last.t1 - since)).max(1.0)
                } else {
                    0.0
                };
                // Steps counted over the window: striding, not stopped.
                let steps: u32 = self
                    .blocks
                    .iter()
                    .filter(|b| b.t0 >= since)
                    .map(|b| b.steps)
                    .sum();
                let still = steps < self.p.pause_steps
                    && speed < limit
                    && along < limit
                    && self
                        .blocks
                        .iter()
                        .filter(|b| b.t0 >= since)
                        .all(|b| b.pos.haversine_m(center) <= self.p.pause_m);
                if !still {
                    return;
                }
                // With the steps: the stop began when the striding ended, if the skier
                // stayed in the pause since then (the GPS error may have delayed the
                // decision past the window).
                let since = match self.stride_last {
                    Some(e) if e < since && self.terrain != Terrain::Down => self
                        .blocks
                        .iter()
                        .find(|b| b.t0 >= e)
                        .filter(|b0| {
                            self.blocks
                                .iter()
                                .filter(|b| b.t0 >= b0.t0)
                                .all(|b| b.pos.haversine_m(center) <= self.p.pause_m)
                        })
                        .map_or(since, |b0| b0.t0),
                    _ => since,
                };
                // The coarse step in progress was GPS wander.
                self.coarse = None;
                self.coarse_last = Some(center);
                self.pause_at = Some(center);
                self.totals.pauses += 1;
                self.waking = false;
                self.pause_steps = 0;
                self.refuge_due = self
                    .refuges
                    .iter()
                    .filter(|(_, p)| p.haversine_m(center) <= self.p.refuge_m)
                    .min_by(|a, b| a.1.haversine_m(center).total_cmp(&b.1.haversine_m(center)))
                    .map(|(n, _)| n.clone().unwrap_or_default());
                self.phase = Phase::Paused {
                    since,
                    refuge: None,
                };
                out.push(SessionEvent::Paused { t: since });
                if self.near && center.haversine_m(self.finish) <= self.p.finish_m {
                    self.finish(since, out);
                }
            }
            Phase::Paused { since, .. } => {
                if vehicle {
                    return self.finish(t, out);
                }
                let center = self.pause_at.unwrap_or(last.pos);
                if last.pos.haversine_m(center) > self.p.resume_m {
                    // Resumed when the block that left the pause started, or when the
                    // steps came back if the phone counts them (a slow climber takes two
                    // minutes to leave the pause by `resume_m`).
                    let t_res = match self.stride_from {
                        Some(f) if f > since && f < last.t0 => f,
                        _ => last.t0,
                    };
                    self.totals.paused_s += t_res - since;
                    self.phase = Phase::Moving;
                    self.still_since = None;
                    self.pause_at = None;
                    self.waking = false;
                    self.refuge_due = None;
                    self.coarse_last = Some(center);
                    self.coarse = None;
                    out.push(SessionEvent::Resumed {
                        t: t_res,
                        paused_s: t_res - since,
                    });
                }
            }
            Phase::Finished => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use skimap_core::graph::LocalPlane;

    fn plane() -> LocalPlane {
        LocalPlane::new(LonLat::new(6.1, 46.6))
    }

    /// Positions every second: (t, x, y) along a scripted path.
    fn run(s: &mut Session, script: &[(f64, f64, f64)]) -> Vec<SessionEvent> {
        let p = plane();
        let mut out = Vec::new();
        for w in script.windows(2) {
            let ((t0, x0, y0), (t1, x1, y1)) = (w[0], w[1]);
            let mut t = t0;
            while t < t1 {
                let f = (t - t0) / (t1 - t0);
                let pos = p.lonlat([x0 + (x1 - x0) * f, y0 + (y1 - y0) * f]);
                s.fix(t, pos, Some(5.0), &mut out);
                t += 1.0;
            }
        }
        out
    }

    fn session() -> Session {
        let p = plane();
        Session::new(
            SessionParams::default(),
            p.lonlat([0.0, 0.0]),
            p.lonlat([0.0, 0.0]),
            vec![(Some("Refuge du test".into()), p.lonlat([2000.0, 10.0]))],
        )
    }

    fn kinds(ev: &[SessionEvent]) -> Vec<String> {
        ev.iter()
            .map(|e| {
                format!("{e:?}")
                    .split([' ', '{'])
                    .next()
                    .unwrap()
                    .to_owned()
            })
            .collect()
    }

    #[test]
    fn start_pause_refuge_resume_finish() {
        let mut s = session();
        // Waits 2 min at the start, skis east 2 km at 3 m/s, stops 10 min at the refuge,
        // skis back, stops at the finish.
        let ev = run(
            &mut s,
            &[
                (0.0, 0.0, 0.0),
                (120.0, 0.0, 0.0),
                (787.0, 2000.0, 0.0),
                (1387.0, 2000.0, 0.0),
                (2054.0, 0.0, 0.0),
                (2300.0, 0.0, 0.0),
            ],
        );
        assert_eq!(
            kinds(&ev),
            vec![
                "Started",
                "Paused",
                "Refuge",
                "Resumed",
                "NearFinish",
                "Paused",
                "Finished"
            ],
            "{ev:#?}"
        );
        let SessionEvent::Started { t, .. } = ev[0] else {
            panic!()
        };
        assert!((110.0..=140.0).contains(&t), "start at the movement: {t}");
        let SessionEvent::Paused { t } = ev[1] else {
            panic!()
        };
        assert!((765.0..=810.0).contains(&t), "{t}");
        let SessionEvent::Refuge { t: tr, ref name } = ev[2] else {
            panic!()
        };
        assert_eq!(name, "Refuge du test");
        assert!(
            (tr - t - 300.0).abs() <= 11.0,
            "5 min after the start of the pause: {tr}"
        );
        let SessionEvent::Resumed { paused_s, .. } = ev[3] else {
            panic!()
        };
        assert!((paused_s - 600.0).abs() < 30.0, "{paused_s}");
        let SessionEvent::Finished { end, .. } = ev[6] else {
            panic!()
        };
        assert_eq!(end, End::Arrived);
        let tot = s.totals();
        assert!((tot.distance_m - 4000.0).abs() < 120.0, "{tot:?}");
        assert_eq!(tot.pauses, 2);
    }

    #[test]
    fn a_short_stop_is_not_a_pause_and_still_shortens_it() {
        let mut s = session();
        // 60 s stop: under the 90 s.
        let ev = run(
            &mut s,
            &[
                (0.0, 0.0, 0.0),
                (60.0, 0.0, 0.0),
                (393.0, 1000.0, 0.0),
                (453.0, 1000.0, 0.0),
                (786.0, 2000.0, 0.0),
            ],
        );
        assert_eq!(kinds(&ev), vec!["Started"], "{ev:?}");
        // « Still » from the phone: 30 s are enough.
        let mut out = Vec::new();
        s.activity(786.0, Activity::Still, &mut out);
        let ev = run(&mut s, &[(786.0, 2000.0, 0.0), (850.0, 2000.0, 0.0)]);
        assert_eq!(kinds(&ev), vec!["Paused"], "{ev:?}");
        // Significant motion: waking until it moves.
        s.activity(850.0, Activity::SignificantMotion, &mut out);
        assert!(s.waking());
        let ev = run(&mut s, &[(850.0, 2000.0, 0.0), (900.0, 2150.0, 0.0)]);
        assert_eq!(kinds(&ev), vec!["Resumed"], "{ev:?}");
        assert!(!s.waking());
    }

    #[test]
    fn a_car_never_starts_and_a_car_ends_the_outing() {
        let mut s = session();
        // Drives through the start at 15 m/s.
        let ev = run(&mut s, &[(0.0, -3000.0, 0.0), (400.0, 3000.0, 0.0)]);
        assert!(ev.is_empty(), "{ev:?}");
        let mut s = session();
        let ev = run(
            &mut s,
            &[(0.0, 0.0, 0.0), (60.0, 0.0, 0.0), (727.0, 2000.0, 0.0)],
        );
        assert!(matches!(ev[0], SessionEvent::Started { .. }));
        let mut out = Vec::new();
        s.activity(800.0, Activity::InVehicle, &mut out);
        assert_eq!(
            out,
            vec![SessionEvent::Finished {
                t: 800.0,
                end: End::Abandoned
            }]
        );
    }

    #[test]
    fn far_from_the_start_never_starts() {
        let mut s = session();
        let ev = run(&mut s, &[(0.0, 1000.0, 1000.0), (600.0, 2800.0, 1000.0)]);
        assert!(ev.is_empty(), "{ev:?}");
    }

    /// Positions every second along a path with a GPS error of ±12 m (a slow wobble), and
    /// the steps counted every 10 s at `rate` per second while moving.
    fn run_steps(
        s: &mut Session,
        script: &[(f64, f64, f64)],
        rate: Option<f64>,
    ) -> Vec<SessionEvent> {
        let p = plane();
        let mut out = Vec::new();
        let mut acc = 0.0;
        let mut report = script[0].0 + 10.0;
        for w in script.windows(2) {
            let ((t0, x0, y0), (t1, x1, y1)) = (w[0], w[1]);
            let moving = (x1 - x0).hypot(y1 - y0) > 0.0;
            let mut t = t0;
            while t < t1 {
                let f = (t - t0) / (t1 - t0);
                let e = 12.0 * (t / 37.0).sin();
                let pos = p.lonlat([x0 + (x1 - x0) * f + e, y0 + (y1 - y0) * f - e]);
                s.fix(t, pos, Some(5.0), &mut out);
                if let Some(r) = rate {
                    if moving {
                        acc += r;
                    }
                    if t >= report {
                        s.steps(t, acc as u32, &mut out);
                        acc -= f64::from(acc as u32);
                        report += 10.0;
                    }
                }
                t += 1.0;
            }
        }
        out
    }

    #[test]
    fn steps_tell_a_slow_climb_from_a_stop() {
        // X10: a start at 3 m/s, then hairpins of 30 m climbed at 0.35 m/s for 10 min, a
        // stop of 3 min, the climb again.
        let mut script = vec![(0.0, 0.0, 0.0), (60.0, 0.0, 0.0), (127.0, 200.0, 0.0)];
        let (mut t, mut y) = (127.0, 0.0);
        for k in 0..7 {
            let x = if k % 2 == 0 { 230.0 } else { 200.0 };
            t += (30.0f64).hypot(10.0) / 0.35;
            y += 10.0;
            script.push((t, x, y));
        }
        let climb = t;
        script.push((t + 180.0, script.last().unwrap().1, y));
        let stop_end = t + 180.0;
        for k in 0..6 {
            let x = if k % 2 == 0 { 200.0 } else { 230.0 };
            let last = *script.last().unwrap();
            y += 10.0;
            script.push((last.0 + (30.0f64).hypot(10.0) / 0.35, x, y));
        }
        let kinds_of = |ev: &[SessionEvent]| kinds(ev);
        // The fixes alone on the flat thresholds take the climb for a stop.
        let mut s = session();
        let ev = run_steps(&mut s, &script, None);
        let gps = kinds_of(&ev);
        assert!(gps.iter().filter(|k| *k == "Paused").count() >= 2, "{ev:?}");
        // With the steps (one a second climbing): the stop and nothing else, dated when
        // the steps stopped and came back.
        let mut s = session();
        s.set_terrain(Terrain::Up);
        let ev = run_steps(&mut s, &script, Some(1.0));
        assert_eq!(
            kinds_of(&ev),
            vec!["Started", "Paused", "Resumed"],
            "{ev:?}"
        );
        let SessionEvent::Paused { t: tp } = ev[1] else {
            panic!()
        };
        let SessionEvent::Resumed { t: tr, .. } = ev[2] else {
            panic!()
        };
        assert!((tp - climb).abs() <= 15.0, "pause at {tp}, stop at {climb}");
        assert!(
            (tr - stop_end).abs() <= 15.0,
            "resumed at {tr}, stop end {stop_end}"
        );
        assert!(!s.waking());
    }

    #[test]
    fn steps_in_a_pause_wake_the_gps() {
        let mut s = session();
        let ev = run(
            &mut s,
            &[
                (0.0, 0.0, 0.0),
                (60.0, 0.0, 0.0),
                (393.0, 1000.0, 0.0),
                (600.0, 1000.0, 0.0),
            ],
        );
        assert_eq!(kinds(&ev), vec!["Started", "Paused"], "{ev:?}");
        let mut out = Vec::new();
        s.steps(600.0, 2, &mut out);
        assert!(!s.waking(), "fidgeting");
        s.steps(610.0, 8, &mut out);
        assert!(s.waking());
    }
}
