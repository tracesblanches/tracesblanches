//! « Ramène-moi » (K4 #54, F21): where to go back to, the way there, and guidance along it.
//!
//! **Where.** [`choose_target`]: the destination of the day plan when it ends somewhere
//! else than where it started (a lodging, another car park), otherwise the car; without a
//! car known, the start of the trace. The car is recorded on its own ([`CarSpotter`]): the
//! first fix of the outing that the next one confirms (a cached position from elsewhere is
//! not), moved onto the nearest OSM car park when one is within [`PARKING_SNAP_M`]; a long
//! touch on the map (« La voiture est ici ») corrects it. A car park drawn as an area in OSM
//! is a single point of it, often tens of metres from where the car stands: the fix that
//! recorded the car is kept, and getting back there is arriving at the car as well.
//!
//! **The way.** [`way_back`], on the phone, offline: along the pistes of the pack
//! ([`Network`], the plan's time model with its level, closed trails avoided) from where the
//! skier is; off the pistes, back along the way skied to the last point of it on the
//! network, then along the pistes; without a network route, the way skied reversed.
//!
//! **Guidance.** The way back is the recorded trace (a point every 10 m) reversed from the
//! last fix to the first one, smoothed (GPS noise), with its loops cut: wherever the trace
//! comes back within [`SHORTCUT_M`] of an earlier point (a spur skied out and back, a loop
//! closed), the way back jumps there; a piste route is kept as it is. Turns are the bends
//! of that line of 50° or more over 50 m arms; each is said once, about [`TURN_LEAD_M`]
//! before it, with the phrase bank of the pack (`Event::Turn`); off the way for 15 s at
//! more than 40 m (C2) and back under 15 m are said as on the plan; the arrival as well,
//! within [`ARRIVED_M`] of the end along the way or as the crow flies (a skier who took
//! another way than the one guided is at the car all the same).
//! The GPS mode comes from the core's rule on the distance to the next turn
//! (`skimap_plan::gps::decide`). [`Backtrack::guide_point`] is the next useful point of the
//! way (the arrow of the screen points there, never across the terrain to the end).
//!
//! Pure and allocation-free per fix (the line is built once, when the skier asks).

use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};
use skimap_common::geom::LonLat;
use skimap_core::eta::EtaModel;
use skimap_core::graph::{Difficulty, LocalPlane, PisteGraph, Snap};
use skimap_core::routing::Router;
use skimap_plan::fallback::PlanCost;
use skimap_voice::Event;
use skimap_voice::bank::{Announcement, Side};
use skimap_voice::clips::Direction;

/// The trace coming back within this of an earlier point is cut there, metres.
pub const SHORTCUT_M: f64 = 15.0;
/// A turn is said this far before it, metres.
pub const TURN_LEAD_M: f64 = 150.0;
/// Bends below this are not said, degrees.
pub const TURN_MIN_DEG: f64 = 50.0;
/// Arms of a bend, metres.
pub const ARM_M: f64 = 50.0;
/// Off the way: above this…
pub const OFF_M: f64 = 40.0;
/// …for this long, seconds.
pub const OFF_S: f64 = 15.0;
/// Back on the way under this…
pub const ON_M: f64 = 15.0;
/// …for this long, seconds.
pub const ON_S: f64 = 5.0;
/// Arrived within this of the end, metres.
pub const ARRIVED_M: f64 = 40.0;
/// Speed assumed before one is measured, m/s (C5 on the flat, classic).
pub const DEFAULT_SPEED_MS: f64 = 2.5;
/// On the way, the arrow points this far ahead, metres.
pub const GUIDE_AHEAD_M: f64 = 60.0;
/// Off the way, the arrow points this far past the nearest point of the way, metres.
pub const REJOIN_AHEAD_M: f64 = 30.0;

/// What a fix changed.
#[derive(Debug, Default)]
pub struct BackStep {
    /// Off the way (`true`) or back on it (`false`).
    pub off: Option<bool>,
    /// To say, with its kind.
    pub say: Vec<(Announcement, &'static str)>,
    /// At the end of the way.
    pub arrived: bool,
}

/// The way back and where the skier is on it.
#[derive(Debug, Clone)]
pub struct Backtrack {
    plane: LocalPlane,
    pts: Vec<[f64; 2]>,
    s: Vec<f64>,
    turns: Vec<(f64, Direction)>,
    cursor: f64,
    d: f64,
    next_turn: usize,
    said_turn: usize,
    off: bool,
    off_since: Option<f64>,
    on_since: Option<f64>,
    last_t: f64,
    arrived: bool,
    /// Speed over the way, m/s (smoothed).
    speed: f64,
    mark: Option<(f64, f64)>,
    /// Last fix, in the plane.
    here: Option<[f64; 2]>,
    /// Another point where the skier has arrived (where the car was recorded before it
    /// was moved onto the car park), in the plane.
    also_end: Option<[f64; 2]>,
}

fn dist(a: [f64; 2], b: [f64; 2]) -> f64 {
    (a[0] - b[0]).hypot(a[1] - b[1])
}

fn heading(a: [f64; 2], b: [f64; 2]) -> f64 {
    // Clockwise from north, degrees.
    (b[0] - a[0]).atan2(b[1] - a[1]).to_degrees()
}

fn turn(from: f64, to: f64) -> f64 {
    let d = (to - from).rem_euclid(360.0);
    if d > 180.0 { d - 360.0 } else { d }
}

fn direction(angle: f64) -> Direction {
    let a = angle.abs();
    match (a, angle > 0.0) {
        (a, true) if a < 70.0 => Direction::SlightRight,
        (a, false) if a < 70.0 => Direction::SlightLeft,
        (a, true) if a <= 155.0 => Direction::Right,
        (a, false) if a <= 155.0 => Direction::Left,
        _ => Direction::UTurn,
    }
}

/// Projection of `q` on segment `a`–`b`: (fraction, distance).
fn project(q: [f64; 2], a: [f64; 2], b: [f64; 2]) -> (f64, f64) {
    let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
    let l2 = dx * dx + dy * dy;
    let f = if l2 > 0.0 {
        (((q[0] - a[0]) * dx + (q[1] - a[1]) * dy) / l2).clamp(0.0, 1.0)
    } else {
        0.0
    };
    let p = [a[0] + f * dx, a[1] + f * dy];
    (f, dist(q, p))
}

/// The way skied from `here` back to the first point of `trace` (oldest first), in the
/// plane: smoothed (moving average over 5 points, ≈ 40 m, ends kept), loops cut.
fn trace_back(plane: &LocalPlane, trace: &[LonLat], here: LonLat) -> Vec<[f64; 2]> {
    let mut raw: Vec<[f64; 2]> = Vec::with_capacity(trace.len() + 1);
    raw.push(plane.xy(here));
    raw.extend(trace.iter().rev().map(|p| plane.xy(*p)));
    let n = raw.len();
    let mut sm = raw.clone();
    for i in 2..n.saturating_sub(2) {
        let mut acc = [0.0, 0.0];
        for p in &raw[i - 2..=i + 2] {
            acc[0] += p[0];
            acc[1] += p[1];
        }
        sm[i] = [acc[0] / 5.0, acc[1] / 5.0];
    }
    // Loops cut: from each point, jump to the last later point within SHORTCUT_M.
    let mut pts = Vec::with_capacity(n);
    let mut i = 0;
    while i < n {
        pts.push(sm[i]);
        let mut j = n - 1;
        while j > i + 1 && dist(sm[i], sm[j]) > SHORTCUT_M {
            j -= 1;
        }
        i = if j > i + 1 { j } else { i + 1 };
    }
    if pts.len() < 2 && n >= 2 {
        pts.push(sm[n - 1]);
    }
    pts
}

/// Appends `p` unless it repeats the last point.
fn push_xy(pts: &mut Vec<[f64; 2]>, p: [f64; 2]) {
    if pts.last().is_none_or(|q| dist(*q, p) > 0.5) {
        pts.push(p);
    }
}

impl Backtrack {
    /// The way back from `here` along `trace` (oldest first); `None` when the trace is
    /// shorter than two points.
    pub fn new(trace: &[LonLat], here: LonLat) -> Option<Self> {
        Self::new_to(trace, here, None)
    }

    /// [`Backtrack::new`], then on to `end` when given (the car parked a few metres from
    /// where the trace starts).
    pub fn new_to(trace: &[LonLat], here: LonLat, end: Option<LonLat>) -> Option<Self> {
        let first = *trace.first()?;
        let plane = LocalPlane::new(first);
        if trace.len() + 1 < 2 {
            return None;
        }
        let mut pts = trace_back(&plane, trace, here);
        if let Some(e) = end {
            push_xy(&mut pts, plane.xy(e));
        }
        Self::from_points(plane, pts)
    }

    /// Guidance along `line`, in travel order, kept as it is (a route on the pistes);
    /// `None` when it has fewer than two distinct points.
    pub fn along(line: &[LonLat]) -> Option<Self> {
        let plane = LocalPlane::new(*line.first()?);
        let mut pts = Vec::with_capacity(line.len());
        for p in line {
            push_xy(&mut pts, plane.xy(*p));
        }
        Self::from_points(plane, pts)
    }

    /// Back along `trace` from `here` to `trace[from]` (smoothed, loops cut), then along
    /// `line` (a route on the pistes from there).
    pub fn trace_then(
        trace: &[LonLat],
        from: usize,
        here: LonLat,
        line: &[LonLat],
    ) -> Option<Self> {
        let first = *trace.get(from)?;
        let plane = LocalPlane::new(first);
        let mut pts = trace_back(&plane, &trace[from..], here);
        for p in line {
            push_xy(&mut pts, plane.xy(*p));
        }
        Self::from_points(plane, pts)
    }

    fn from_points(plane: LocalPlane, pts: Vec<[f64; 2]>) -> Option<Self> {
        if pts.len() < 2 {
            return None;
        }
        let mut s = Vec::with_capacity(pts.len());
        let mut acc = 0.0;
        s.push(0.0);
        for w in pts.windows(2) {
            acc += dist(w[0], w[1]);
            s.push(acc);
        }
        let mut b = Self {
            plane,
            pts,
            s,
            turns: Vec::new(),
            cursor: 0.0,
            d: 0.0,
            next_turn: 0,
            said_turn: 0,
            off: false,
            off_since: None,
            on_since: None,
            last_t: f64::NEG_INFINITY,
            arrived: false,
            speed: DEFAULT_SPEED_MS,
            mark: None,
            here: None,
            also_end: None,
        };
        b.turns = b.find_turns();
        Some(b)
    }

    /// Getting within [`ARRIVED_M`] of `p` is arriving as well (where the car was recorded
    /// before it was moved onto the car park, [`Target::recorded`]).
    #[must_use]
    pub fn arriving_at(mut self, p: Option<LonLat>) -> Self {
        self.also_end = p.map(|p| self.plane.xy(p));
        self
    }

    /// End of the way.
    pub fn end(&self) -> LonLat {
        self.plane
            .lonlat(*self.pts.last().expect("two points at least"))
    }

    /// The way, in travel order.
    pub fn line(&self) -> Vec<LonLat> {
        self.pts.iter().map(|p| self.plane.lonlat(*p)).collect()
    }

    /// Off the way (said, C2 hysteresis), or farther than [`OFF_M`] from it now.
    pub fn is_off(&self) -> bool {
        self.off || self.d > OFF_M
    }

    /// Whether the end was reached.
    pub fn arrived(&self) -> bool {
        self.arrived
    }

    /// The next useful point of the way, where the arrow of the screen points: on the way,
    /// [`GUIDE_AHEAD_M`] ahead of the skier, or the next turn when it comes first (no corner
    /// cut across the terrain); off it, the nearest point of the way still ahead, a little
    /// further along ([`REJOIN_AHEAD_M`]). Never the end straight across.
    pub fn guide_point(&self) -> LonLat {
        let (base, ahead) = match self.here {
            Some(q) if self.d > ON_M => (self.nearest_ahead(q), REJOIN_AHEAD_M),
            _ => (self.cursor, GUIDE_AHEAD_M),
        };
        let mut s = base + ahead;
        if let Some((ts, _)) = self.turns.iter().find(|(ts, _)| *ts > base + 15.0) {
            s = s.min(*ts);
        }
        self.plane.lonlat(self.point_at(s.min(self.length_m())))
    }

    /// Distance along the way of its point nearest to `q`, from the cursor on.
    fn nearest_ahead(&self, q: [f64; 2]) -> f64 {
        let first = self
            .s
            .partition_point(|x| *x <= self.cursor)
            .saturating_sub(1);
        let mut best = (self.cursor, f64::INFINITY);
        for i in first..self.pts.len() - 1 {
            let (f, d) = project(q, self.pts[i], self.pts[i + 1]);
            if d < best.1 {
                let sp = self.s[i] + f * (self.s[i + 1] - self.s[i]);
                best = (sp.max(self.cursor), d);
            }
        }
        best.0
    }

    fn point_at(&self, s: f64) -> [f64; 2] {
        let s = s.clamp(0.0, self.length_m());
        let i = match self.s.binary_search_by(|x| x.total_cmp(&s)) {
            Ok(i) => return self.pts[i],
            Err(i) => i.clamp(1, self.pts.len() - 1),
        };
        let (a, b) = (self.pts[i - 1], self.pts[i]);
        let l = self.s[i] - self.s[i - 1];
        let f = if l > 0.0 {
            (s - self.s[i - 1]) / l
        } else {
            0.0
        };
        [a[0] + f * (b[0] - a[0]), a[1] + f * (b[1] - a[1])]
    }

    /// Bends of 50° or more, the sharpest of each 100 m stretch, none in the last 140 m.
    fn find_turns(&self) -> Vec<(f64, Direction)> {
        let len = self.length_m();
        let mut cands: Vec<(f64, f64)> = Vec::new();
        let mut s = ARM_M;
        while s + ARM_M <= len {
            let (a, p, b) = (
                self.point_at(s - ARM_M),
                self.point_at(s),
                self.point_at(s + ARM_M),
            );
            let t = turn(heading(a, p), heading(p, b));
            if t.abs() >= TURN_MIN_DEG {
                cands.push((s, t));
            }
            s += 5.0;
        }
        let mut out: Vec<(f64, f64)> = Vec::new();
        // Around the end, the trace is the skier waiting at the start (GPS noise): no turn.
        let last = len - ARRIVED_M - 2.0 * ARM_M;
        for (s, t) in cands.into_iter().filter(|c| c.0 < last) {
            match out.last_mut() {
                Some(last) if s - last.0 < 2.0 * ARM_M => {
                    if t.abs() > last.1.abs() {
                        *last = (s, t);
                    }
                }
                _ => out.push((s, t)),
            }
        }
        out.into_iter().map(|(s, t)| (s, direction(t))).collect()
    }

    /// Length of the way back, metres.
    pub fn length_m(&self) -> f64 {
        self.s.last().copied().unwrap_or(0.0)
    }

    /// Left, metres.
    pub fn remaining_m(&self) -> f64 {
        (self.length_m() - self.cursor).max(0.0)
    }

    /// Left at the speed of the way back so far, seconds.
    pub fn remaining_s(&self) -> f64 {
        self.remaining_m() / self.speed.max(0.5)
    }

    /// Distance from the last fix to the way, metres.
    pub fn distance_m(&self) -> f64 {
        self.d
    }

    /// Speed along the way, m/s.
    pub fn speed_ms(&self) -> f64 {
        self.speed
    }

    /// The next turn and its distance, metres.
    pub fn next_turn(&self) -> Option<(Direction, f64)> {
        self.turns
            .get(self.next_turn)
            .map(|(s, d)| (*d, (s - self.cursor).max(0.0)))
    }

    /// Distance since the last turn passed, metres.
    pub fn since_turn_m(&self) -> Option<f64> {
        self.next_turn
            .checked_sub(1)
            .and_then(|i| self.turns.get(i))
            .map(|(s, _)| self.cursor - s)
    }

    /// What is said when the skier asks: « Il reste deux kilomètres. Environ trente
    /// minutes. » then the first turn when it is near.
    pub fn start_announcement(&mut self) -> Announcement {
        let mut a = Event::Remaining {
            metres: self.remaining_m(),
            seconds: self.remaining_s(),
        }
        .announce();
        if let Some((d, m)) = self.next_turn()
            && m <= TURN_LEAD_M
        {
            a = a.then(
                Event::Turn {
                    metres: m,
                    direction: d,
                    piste: None,
                }
                .announce(),
            );
            self.said_turn = self.next_turn + 1;
        }
        a
    }

    /// A fix: moves the cursor (forward only, within what a skier can do), the turns, the
    /// off-the-way hysteresis, the arrival (within [`ARRIVED_M`] of the end along the way,
    /// or of the end or of [`Backtrack::arriving_at`]'s point as the crow flies, whichever
    /// way the skier took).
    pub fn fix(&mut self, t: f64, pos: LonLat) -> BackStep {
        let mut step = BackStep::default();
        if t <= self.last_t || self.arrived {
            return step;
        }
        let dt = if self.last_t.is_finite() {
            t - self.last_t
        } else {
            0.0
        };
        self.last_t = t;
        let q = self.plane.xy(pos);
        self.here = Some(q);
        // Window ahead of the cursor: at most 12 m/s since the last fix, 30 m behind.
        let lo = (self.cursor - 30.0).max(0.0);
        let hi = self.cursor + 50.0 + 12.0 * dt;
        let mut best: Option<(f64, f64)> = None;
        let mut first = self.s.partition_point(|x| *x < lo).saturating_sub(1);
        while first + 1 < self.pts.len() && self.s[first] <= hi {
            let (f, d) = project(q, self.pts[first], self.pts[first + 1]);
            let sp = self.s[first] + f * (self.s[first + 1] - self.s[first]);
            if best.is_none_or(|b| d < b.1 - 1e-9) {
                best = Some((sp, d));
            }
            first += 1;
        }
        let (sp, d) = best.unwrap_or((self.cursor, dist(q, self.point_at(self.cursor))));
        self.d = d;
        if d <= OFF_M && sp > self.cursor {
            self.cursor = sp;
        }
        // Speed over at least 30 s of progress.
        match self.mark {
            None => self.mark = Some((t, self.cursor)),
            Some((t0, c0)) if t - t0 >= 30.0 => {
                let v = (self.cursor - c0) / (t - t0);
                if v > 0.3 {
                    self.speed = 0.7 * self.speed + 0.3 * v;
                }
                self.mark = Some((t, self.cursor));
            }
            _ => {}
        }
        // Off the way and back (C2 hysteresis).
        if !self.off {
            if d > OFF_M {
                let since = *self.off_since.get_or_insert(t);
                if t - since >= OFF_S {
                    self.off = true;
                    self.on_since = None;
                    step.off = Some(true);
                    step.say.push((
                        Event::OffRoute {
                            side: self.side(q),
                            metres: d,
                        }
                        .announce(),
                        "off_route",
                    ));
                }
            } else {
                self.off_since = None;
            }
        } else if d < ON_M {
            let since = *self.on_since.get_or_insert(t);
            if t - since >= ON_S {
                self.off = false;
                self.off_since = None;
                step.off = Some(false);
                step.say.push((
                    Event::BackOnRoute { piste: None }.announce(),
                    "back_on_route",
                ));
            }
        } else {
            self.on_since = None;
        }
        // Turns passed and the next one said once, about TURN_LEAD_M before.
        while self
            .turns
            .get(self.next_turn)
            .is_some_and(|(s, _)| *s < self.cursor - 10.0)
        {
            self.next_turn += 1;
        }
        if let Some((dir, m)) = self.next_turn()
            && self.said_turn <= self.next_turn
            && m <= TURN_LEAD_M
            && !self.off
        {
            self.said_turn = self.next_turn + 1;
            step.say.push((
                Event::Turn {
                    metres: m,
                    direction: dir,
                    piste: None,
                }
                .announce(),
                "turn",
            ));
        }
        let near = |p: [f64; 2]| dist(q, p) <= ARRIVED_M;
        let along = self.remaining_m() <= ARRIVED_M && d <= OFF_M;
        let end = self.pts[self.pts.len() - 1];
        if along || near(end) || self.also_end.is_some_and(near) {
            self.arrived = true;
            step.arrived = true;
            step.say
                .push((Event::Arrive { metres: None }.announce(), "arrive"));
        }
        step
    }

    /// Which side of the way the skier is, seen along it.
    fn side(&self, q: [f64; 2]) -> Side {
        let a = self.point_at(self.cursor);
        let b = self.point_at(self.cursor + 20.0);
        let cross = (b[0] - a[0]) * (q[1] - a[1]) - (b[1] - a[1]) * (q[0] - a[0]);
        // The way is to the right of a skier standing on its left.
        if cross > 0.0 { Side::Right } else { Side::Left }
    }
}

// ---------------------------------------------------------------------------------------
// The car
// ---------------------------------------------------------------------------------------

/// The first fix is confirmed by a later one within this, metres…
pub const CAR_CONFIRM_M: f64 = 50.0;
/// …that comes within this, seconds.
pub const CAR_CONFIRM_S: f64 = 120.0;
/// The car is moved onto the nearest OSM car park within this, metres.
pub const PARKING_SNAP_M: f64 = 150.0;

/// Where the car is.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Car {
    /// Longitude.
    pub lon: f64,
    /// Latitude.
    pub lat: f64,
    /// The car park it was moved onto (its name, else « Parking »).
    pub parking: Option<String>,
    /// Set by the skier (a long touch on the map), not recorded on its own.
    pub manual: bool,
    /// The fix that recorded it, when it was moved onto a car park.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub recorded: Option<LonLat>,
}

impl Car {
    /// Position.
    pub fn pos(&self) -> LonLat {
        LonLat::new(self.lon, self.lat)
    }
}

/// A car park of the pack: name, position.
pub type Parking = (Option<String>, LonLat);

/// `pos` moved onto the nearest car park within [`PARKING_SNAP_M`], with its name (« Parking »
/// when it has none); `pos` itself and `None` without a car park that near.
pub fn snap_to_parking(pos: LonLat, parkings: &[Parking]) -> (LonLat, Option<String>) {
    parkings
        .iter()
        .map(|(n, p)| (pos.haversine_m(*p), n, *p))
        .filter(|(d, _, _)| *d <= PARKING_SNAP_M)
        .min_by(|a, b| a.0.total_cmp(&b.0))
        .map_or((pos, None), |(_, n, p)| {
            (p, Some(n.clone().unwrap_or_else(|| "Parking".into())))
        })
}

/// Records the car on its own: the first fix of the outing that a later one confirms
/// (within [`CAR_CONFIRM_M`] and [`CAR_CONFIRM_S`]; a cached position from elsewhere is
/// followed by a jump and is not), moved onto the nearest car park. A car set by the skier
/// is kept.
#[derive(Debug, Clone, Default)]
pub struct CarSpotter {
    candidate: Option<(f64, LonLat)>,
    car: Option<Car>,
}

impl CarSpotter {
    /// Starts from a car already known (set by the skier before a restart).
    pub fn with(car: Option<Car>) -> Self {
        Self {
            candidate: None,
            car,
        }
    }

    /// A fix at `t` (seconds); true when the car was recorded by this fix.
    pub fn fix(&mut self, t: f64, pos: LonLat, parkings: &[Parking]) -> bool {
        if self.car.is_some() {
            return false;
        }
        match self.candidate {
            Some((t0, p0)) if t - t0 <= CAR_CONFIRM_S && p0.haversine_m(pos) <= CAR_CONFIRM_M => {
                let (p, parking) = snap_to_parking(p0, parkings);
                self.car = Some(Car {
                    lon: p.lon,
                    lat: p.lat,
                    recorded: parking.is_some().then_some(p0),
                    parking,
                    manual: false,
                });
                true
            }
            _ => {
                self.candidate = Some((t, pos));
                false
            }
        }
    }

    /// « La voiture est ici »: the skier's position of the car, kept as given.
    pub fn set(&mut self, pos: LonLat) -> &Car {
        self.car.insert(Car {
            lon: pos.lon,
            lat: pos.lat,
            parking: None,
            manual: true,
            recorded: None,
        })
    }

    /// The car, once known.
    pub fn car(&self) -> Option<&Car> {
        self.car.as_ref()
    }
}

// ---------------------------------------------------------------------------------------
// Where to go back to
// ---------------------------------------------------------------------------------------

/// The end of the day plan farther than this from where it starts (and from the car) is a
/// destination of its own (a lodging, another car park), metres.
pub const DESTINATION_M: f64 = 300.0;

/// What « ramène-moi » goes back to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TargetKind {
    /// The destination of the day plan (it ends elsewhere than where it starts).
    Destination,
    /// The car.
    Car,
    /// Where the outing started (no car known).
    Start,
}

impl TargetKind {
    /// `destination`, `car`, `start`.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Destination => "destination",
            Self::Car => "car",
            Self::Start => "start",
        }
    }
}

/// Where « ramène-moi » goes.
#[derive(Debug, Clone, PartialEq)]
pub struct Target {
    /// What it is.
    pub kind: TargetKind,
    /// Position.
    pub pos: LonLat,
    /// Name (the destination, the car park), when known.
    pub name: Option<String>,
    /// The car: where it was recorded, when it was moved onto a car park (arriving there
    /// is arriving at the car).
    pub recorded: Option<LonLat>,
}

/// Where to go back to: the end of the day plan when it is a destination of its own
/// (farther than [`DESTINATION_M`] from the start of the plan and from the car), else the
/// car, else where the trace starts, else the start of the plan.
pub fn choose_target(
    plan_end: Option<(LonLat, Option<String>)>,
    plan_start: Option<LonLat>,
    car: Option<&Car>,
    trace_start: Option<LonLat>,
) -> Option<Target> {
    if let Some((end, name)) = plan_end {
        let far = |p: Option<LonLat>| p.is_none_or(|p| p.haversine_m(end) > DESTINATION_M);
        if plan_start.is_some() && far(plan_start) && far(car.map(Car::pos)) {
            return Some(Target {
                kind: TargetKind::Destination,
                pos: end,
                name,
                recorded: None,
            });
        }
    }
    if let Some(c) = car {
        return Some(Target {
            kind: TargetKind::Car,
            pos: c.pos(),
            name: c.parking.clone(),
            recorded: c.recorded,
        });
    }
    trace_start.or(plan_start).map(|p| Target {
        kind: TargetKind::Start,
        pos: p,
        name: None,
        recorded: None,
    })
}

// ---------------------------------------------------------------------------------------
// The way back
// ---------------------------------------------------------------------------------------

/// The skier is on the pistes within this of the network, metres.
pub const ON_PISTE_M: f64 = 50.0;
/// The target is reached from the network within this, metres (a car park beside the
/// trails, a lodging up a lane).
pub const TARGET_SNAP_M: f64 = 400.0;

/// How the way back goes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WayKind {
    /// Along the pistes of the pack from where the skier is.
    Pistes,
    /// Back along the way skied to the pistes, then along them.
    TracePistes,
    /// The way skied, reversed.
    Trace,
}

impl WayKind {
    /// `pistes`, `trace_pistes`, `trace`.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Pistes => "pistes",
            Self::TracePistes => "trace_pistes",
            Self::Trace => "trace",
        }
    }
}

/// The trails of the pack for the way back: the plan's time model with its level, closed
/// trails avoided (`skimap_plan::fallback::PlanCost`, the plans B's own cost).
#[derive(Debug)]
pub struct Network<'g> {
    graph: &'g PisteGraph,
    router: Router<'g, PlanCost>,
}

impl<'g> Network<'g> {
    /// The network of `graph` under the day's model (costs computed once, when the skier
    /// asks).
    pub fn new(
        graph: &'g PisteGraph,
        eta: EtaModel,
        max_difficulty: Option<Difficulty>,
        closed: &BTreeSet<i64>,
    ) -> Self {
        let cost = PlanCost {
            eta,
            max_difficulty,
            closed: closed.clone(),
        };
        Self {
            graph,
            router: Router::new(graph, cost),
        }
    }

    fn snap(&self, p: LonLat, max_m: f64) -> Option<Snap> {
        self.graph.snap(p, max_m)
    }

    /// The route between two points of the network, as a line.
    fn route(&self, a: Snap, b: Snap) -> Option<Vec<LonLat>> {
        let r = self
            .router
            .route_snaps(a, b)
            .ok()
            .filter(|r| r.cost.is_finite())?;
        Some(r.polyline(self.graph))
    }
}

/// The way back and what it goes to.
#[derive(Debug, Clone)]
pub struct Way {
    /// Guidance along it.
    pub back: Backtrack,
    /// How it goes.
    pub kind: WayKind,
    /// Where it goes.
    pub target: Target,
}

/// The way back from `here` to `target`, computed on the phone: along the pistes when the
/// skier is on them; off them, back along `trace` (the way skied, oldest first) to its
/// last point on the network, then along the pistes; without a route on the network, the
/// way skied reversed — to `target` when the trace leads there, else to `home` (a
/// destination of the plan reached only by the pistes: the car or the start instead).
pub fn way_back(
    net: Option<&Network<'_>>,
    trace: &[LonLat],
    here: LonLat,
    target: Target,
    home: Option<Target>,
) -> Option<Way> {
    if let Some(n) = net
        && let Some(to) = n.snap(target.pos, TARGET_SNAP_M)
    {
        let mut line: Vec<LonLat> = Vec::new();
        let mut kind = None;
        let mut from_trace = None;
        if let Some(from) = n.snap(here, ON_PISTE_M) {
            if let Some(r) = n.route(from, to) {
                line.push(here);
                line.extend(r);
                kind = Some(WayKind::Pistes);
            }
        } else if let Some((i, from)) = trace
            .iter()
            .enumerate()
            .rev()
            .find_map(|(i, p)| n.snap(*p, ON_PISTE_M).map(|s| (i, s)))
            && let Some(r) = n.route(from, to)
        {
            line.extend(r);
            kind = Some(WayKind::TracePistes);
            from_trace = Some(i);
        }
        if let Some(k) = kind {
            line.push(target.pos);
            let back = match from_trace {
                Some(i) => Backtrack::trace_then(trace, i, here, &line),
                None => Backtrack::along(&line),
            };
            if let Some(back) = back {
                return Some(Way {
                    back: back.arriving_at(target.recorded),
                    kind: k,
                    target,
                });
            }
        }
    }
    // The way skied: it leads to where the outing started (the car, a few metres off).
    let to = match target.kind {
        TargetKind::Destination => home?,
        _ => target,
    };
    let back = Backtrack::new_to(trace, here, Some(to.pos))?.arriving_at(to.recorded);
    Some(Way {
        back,
        kind: WayKind::Trace,
        target: to,
    })
}

/// Whether to propose « ramène-moi »: the return time of the plan calls for going home
/// (A8's « turn around now », `home_time`) and the tracker found no way home on the pistes
/// (`home_route`: it then guides home itself), the skier is not going back already and no
/// other proposal waits for an answer.
pub fn should_propose(home_time: bool, home_route: bool, backing: bool, pending: bool) -> bool {
    home_time && !home_route && !backing && !pending
}

/// The written proposal of « ramène-moi » in `lang` (one gesture to accept).
pub fn proposal_text(target: Option<&Target>, lang: skimap_common::Lang) -> String {
    crate::text::of(lang).back_proposal(target)
}

/// The car parks of the pack in `pack_dir` (`stations-pois`, kind `parking`).
pub fn pack_parkings(pack_dir: &std::path::Path) -> Vec<Parking> {
    let Ok(pack) = skimap_pack::Pack::open(pack_dir.to_path_buf()) else {
        return Vec::new();
    };
    if pack.layer("stations-pois").is_err() {
        return Vec::new();
    }
    let Ok(fc) = pack.read_geojson("stations-pois") else {
        return Vec::new();
    };
    fc.features
        .iter()
        .filter(|f| crate::fog::prop(f, "kind") == Some("parking"))
        .filter_map(|f| {
            crate::fog::point_of(f).map(|p| (crate::fog::prop(f, "name").map(str::to_owned), p))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A trace in a local plane around Cervières: points every 10 m along `xy`.
    fn trace(xy: &[[f64; 2]]) -> (LocalPlane, Vec<LonLat>) {
        let plane = LocalPlane::new(LonLat::new(6.7269, 44.8552));
        let mut out = Vec::new();
        for w in xy.windows(2) {
            let n = (dist(w[0], w[1]) / 10.0).ceil() as usize;
            for k in 0..n {
                let f = k as f64 / n as f64;
                out.push(plane.lonlat([
                    w[0][0] + f * (w[1][0] - w[0][0]),
                    w[0][1] + f * (w[1][1] - w[0][1]),
                ]));
            }
        }
        out.push(plane.lonlat(*xy.last().unwrap()));
        (plane, out)
    }

    #[test]
    fn an_l_shaped_way_back_says_one_turn_and_arrives() {
        // Out: 600 m north, then 400 m east. Back: 400 m west, then a left turn, 600 m
        // south.
        let (plane, tr) = trace(&[[0.0, 0.0], [0.0, 600.0], [400.0, 600.0]]);
        let here = *tr.last().unwrap();
        let mut b = Backtrack::new(&tr, here).unwrap();
        assert!((b.length_m() - 1000.0).abs() < 20.0, "{}", b.length_m());
        assert_eq!(b.turns.len(), 1, "{:?}", b.turns);
        assert_eq!(b.turns[0].1, Direction::Left);
        assert!((b.turns[0].0 - 400.0).abs() < 25.0, "{:?}", b.turns);
        let a = b.start_announcement();
        assert!(
            a.text().starts_with("Il reste un kilomètre."),
            "{}",
            a.text()
        );
        // Ski back at 3 m/s, one fix a second.
        let mut said = Vec::new();
        let mut arrived = false;
        for k in 0..400 {
            let s = (3.0 * f64::from(k)).min(1000.0);
            let p = if s <= 400.0 {
                [400.0 - s, 600.0]
            } else {
                [0.0, 600.0 - (s - 400.0)]
            };
            let st = b.fix(f64::from(k), plane.lonlat(p));
            for (a, kind) in st.say {
                said.push((k, kind, a.text()));
            }
            if st.arrived {
                arrived = true;
                break;
            }
        }
        assert!(arrived);
        let turns: Vec<_> = said.iter().filter(|x| x.1 == "turn").collect();
        assert_eq!(turns.len(), 1, "{said:?}");
        assert!(turns[0].2.contains("tournez à gauche"), "{said:?}");
        // Said about 150 m before the corner (at 3 m/s: around 85 s).
        assert!((80..=90).contains(&turns[0].0), "{said:?}");
        assert!(said.iter().all(|x| x.1 != "off_route"), "{said:?}");
        assert!((b.speed_ms() - 3.0).abs() < 0.3, "{}", b.speed_ms());
    }

    #[test]
    fn a_spur_skied_out_and_back_is_cut_and_off_the_way_is_said() {
        // 500 m east, a 200 m spur north and back, 300 m more east.
        let (plane, tr) = trace(&[
            [0.0, 0.0],
            [500.0, 0.0],
            [500.0, 200.0],
            [500.0, 0.0],
            [800.0, 0.0],
        ]);
        let b0 = Backtrack::new(&tr, *tr.last().unwrap()).unwrap();
        assert!((b0.length_m() - 800.0).abs() < 30.0, "{}", b0.length_m());
        let mut b = b0.clone();
        // 60 m off the way for 20 s.
        let mut off = None;
        for k in 0..20 {
            let st = b.fix(f64::from(k), plane.lonlat([780.0, 60.0]));
            if st.off.is_some() {
                off = Some((k, st.say[0].0.text()));
            }
        }
        let (k, text) = off.expect("off the way");
        assert_eq!(k, 15);
        assert!(text.contains("mètres"), "{text}");
        let mut back = None;
        for k in 20..30 {
            let st = b.fix(f64::from(k), plane.lonlat([780.0, 2.0]));
            if st.off == Some(false) {
                back = Some(k);
            }
        }
        assert_eq!(back, Some(25));
    }

    // -----------------------------------------------------------------------------------
    // Where to go back to, the car, the way back
    // -----------------------------------------------------------------------------------

    use skimap_core::graph::{Edge, EdgeAttrs, EdgeKind, GraphData, Node};
    use skimap_core::routing::Style;

    fn plane() -> LocalPlane {
        LocalPlane::new(LonLat::new(6.7269, 44.8552))
    }

    fn at(x: f64, y: f64) -> LonLat {
        plane().lonlat([x, y])
    }

    /// Bearing from `a` to `b`, degrees clockwise from north (plane).
    fn bearing(a: LonLat, b: LonLat) -> f64 {
        let (p, q) = (plane().xy(a), plane().xy(b));
        heading(p, q).rem_euclid(360.0)
    }

    /// A small network: A (0, 0) — B (1000, 0) — C (2000, 0), and B — D (1000, 1000).
    fn network() -> PisteGraph {
        let node = |x, y| Node {
            pos: at(x, y),
            osm_node_id: None,
        };
        let edge = |from: usize, to: usize, a: [f64; 2], b: [f64; 2], id: i64| {
            let n = 10;
            let points = (0..=n)
                .map(|k| {
                    let f = f64::from(k) / f64::from(n);
                    at(a[0] + f * (b[0] - a[0]), a[1] + f * (b[1] - a[1]))
                })
                .collect();
            Edge {
                from,
                to,
                oneway: false,
                points,
                attrs: EdgeAttrs {
                    kind: EdgeKind::Piste,
                    name: Some(format!("piste {id}")),
                    osm_way_id: Some(id),
                    classic: Some(true),
                    skating: Some(true),
                    backcountry: false,
                    nordic: true,
                    hike: false,
                    difficulty: None,
                    difficulty_max: None,
                },
                profile: None,
            }
        };
        PisteGraph::new(GraphData {
            site: "test".into(),
            nodes: vec![
                node(0.0, 0.0),
                node(1000.0, 0.0),
                node(2000.0, 0.0),
                node(1000.0, 1000.0),
            ],
            edges: vec![
                edge(0, 1, [0.0, 0.0], [1000.0, 0.0], 1),
                edge(1, 2, [1000.0, 0.0], [2000.0, 0.0], 2),
                edge(1, 3, [1000.0, 0.0], [1000.0, 1000.0], 3),
            ],
            elevation_source: None,
        })
        .unwrap()
    }

    fn net(g: &PisteGraph) -> Network<'_> {
        Network::new(g, EtaModel::new(Style::Classic), None, &BTreeSet::new())
    }

    /// The way skied, a point every 10 m, along `xy`.
    fn skied(xy: &[[f64; 2]]) -> Vec<LonLat> {
        let (_, mut v) = trace(xy);
        v.dedup();
        v
    }

    fn car_at(x: f64, y: f64) -> Car {
        let p = at(x, y);
        Car {
            lon: p.lon,
            lat: p.lat,
            parking: Some("Parking du Bourget".into()),
            manual: false,
            recorded: None,
        }
    }

    #[test]
    fn the_car_is_the_first_confirmed_fix_moved_onto_the_car_park() {
        let parkings = vec![(Some("Parking du Bourget".into()), at(-60.0, -80.0))];
        let mut c = CarSpotter::default();
        // A cached position from elsewhere, then the real ones.
        assert!(!c.fix(0.0, LonLat::new(-123.15, 39.24), &parkings));
        assert!(!c.fix(5.0, at(0.0, 0.0), &parkings));
        assert!(c.fix(6.0, at(3.0, 2.0), &parkings));
        let car = c.car().unwrap().clone();
        assert_eq!(car.parking.as_deref(), Some("Parking du Bourget"));
        assert!(car.pos().haversine_m(at(-60.0, -80.0)) < 0.5);
        assert!(!car.manual);
        // Recorded once.
        assert!(!c.fix(7.0, at(500.0, 0.0), &parkings));
        // No car park within 150 m: the fix itself.
        let mut far = CarSpotter::default();
        far.fix(0.0, at(0.0, 0.0), &[(None, at(200.0, 0.0))]);
        far.fix(1.0, at(1.0, 0.0), &[(None, at(200.0, 0.0))]);
        assert!(far.car().unwrap().pos().haversine_m(at(0.0, 0.0)) < 0.5);
        assert_eq!(far.car().unwrap().parking, None);
        // A car park without a name is said « Parking ».
        let (_, name) = snap_to_parking(at(0.0, 0.0), &[(None, at(50.0, 0.0))]);
        assert_eq!(name.as_deref(), Some("Parking"));
        // « La voiture est ici » is kept as given and not replaced by a fix.
        let mut m = CarSpotter::default();
        m.set(at(10.0, 10.0));
        assert!(!m.fix(0.0, at(0.0, 0.0), &parkings));
        assert!(!m.fix(1.0, at(0.0, 0.0), &parkings));
        assert!(m.car().unwrap().manual);
        assert!(m.car().unwrap().pos().haversine_m(at(10.0, 10.0)) < 0.5);
    }

    #[test]
    fn the_target_is_the_destination_of_the_plan_else_the_car() {
        let car = car_at(-50.0, -50.0);
        // A plan that ends elsewhere (a lodging): its end.
        let t = choose_target(
            Some((at(2050.0, 100.0), Some("Gîte".into()))),
            Some(at(0.0, 0.0)),
            Some(&car),
            Some(at(0.0, 0.0)),
        )
        .unwrap();
        assert_eq!(t.kind, TargetKind::Destination);
        assert_eq!(t.name.as_deref(), Some("Gîte"));
        // A loop (ends where it starts): the car.
        let t = choose_target(
            Some((at(20.0, 0.0), None)),
            Some(at(0.0, 0.0)),
            Some(&car),
            Some(at(0.0, 0.0)),
        )
        .unwrap();
        assert_eq!(t.kind, TargetKind::Car);
        assert_eq!(t.name.as_deref(), Some("Parking du Bourget"));
        // A plan ending at the car parked away from its start: the car.
        let t = choose_target(
            Some((at(2000.0, 0.0), None)),
            Some(at(0.0, 0.0)),
            Some(&car_at(2000.0, 50.0)),
            None,
        )
        .unwrap();
        assert_eq!(t.kind, TargetKind::Car);
        // No plan: the car.
        let t = choose_target(None, None, Some(&car), Some(at(5.0, 5.0))).unwrap();
        assert_eq!(t.kind, TargetKind::Car);
        // No plan, no car: where the trace starts.
        let t = choose_target(None, None, None, Some(at(5.0, 5.0))).unwrap();
        assert_eq!(t.kind, TargetKind::Start);
        assert!(t.pos.haversine_m(at(5.0, 5.0)) < 0.5);
        assert!(choose_target(None, None, None, None).is_none());
    }

    #[test]
    fn without_a_plan_the_way_back_goes_to_the_car_by_the_pistes() {
        let g = network();
        let n = net(&g);
        let car = car_at(-100.0, -50.0);
        // Out: A → B → D on the pistes; back from D.
        let tr = skied(&[[0.0, 0.0], [1000.0, 0.0], [1000.0, 1000.0]]);
        let here = at(1000.0, 1000.0);
        let target = choose_target(None, None, Some(&car), tr.first().copied()).unwrap();
        let w = way_back(Some(&n), &tr, here, target, None).unwrap();
        assert_eq!(w.kind, WayKind::Pistes);
        assert_eq!(w.target.kind, TargetKind::Car);
        assert!(w.back.end().haversine_m(car.pos()) < 1.0);
        // D → B → A, then the 112 m to the car park.
        assert!(
            (w.back.length_m() - 2112.0).abs() < 15.0,
            "{}",
            w.back.length_m()
        );
        assert_eq!(w.back.turns.len(), 1, "{:?}", w.back.turns);
        assert_eq!(w.back.turns[0].1, Direction::Right);
        // The arrow: along the way, south, not straight at the car (south-west).
        let b = bearing(here, w.back.guide_point());
        assert!((b - 180.0).abs() < 2.0, "{b}");
    }

    #[test]
    fn a_plan_with_a_destination_goes_there_by_the_pistes() {
        let g = network();
        let n = net(&g);
        let car = car_at(-100.0, -50.0);
        let tr = skied(&[[0.0, 0.0], [1000.0, 0.0], [1000.0, 800.0]]);
        let here = at(1000.0, 800.0);
        let target = choose_target(
            Some((at(2050.0, 100.0), Some("Gîte".into()))),
            Some(at(0.0, 0.0)),
            Some(&car),
            tr.first().copied(),
        )
        .unwrap();
        let home = choose_target(None, Some(at(0.0, 0.0)), Some(&car), None);
        let w = way_back(Some(&n), &tr, here, target, home.clone()).unwrap();
        assert_eq!(w.kind, WayKind::Pistes);
        assert_eq!(w.target.kind, TargetKind::Destination);
        assert!(w.back.end().haversine_m(at(2050.0, 100.0)) < 1.0);
        // D' → B → C, then 112 m to the lodging.
        assert!(
            (w.back.length_m() - 1912.0).abs() < 15.0,
            "{}",
            w.back.length_m()
        );
        assert_eq!(w.back.turns[0].1, Direction::Left);
        // Without the pistes, the way skied goes back to the car instead.
        let target = choose_target(
            Some((at(2050.0, 100.0), Some("Gîte".into()))),
            Some(at(0.0, 0.0)),
            Some(&car),
            tr.first().copied(),
        )
        .unwrap();
        let w = way_back(None, &tr, here, target, home).unwrap();
        assert_eq!(w.kind, WayKind::Trace);
        assert_eq!(w.target.kind, TargetKind::Car);
        assert!(w.back.end().haversine_m(car.pos()) < 1.0);
    }

    #[test]
    fn off_the_pistes_the_way_back_follows_the_way_skied_to_them() {
        let g = network();
        let n = net(&g);
        let car = car_at(-100.0, -50.0);
        // Out: A → B → up D to 600 m, then off the pistes to (1300, 700).
        let tr = skied(&[[0.0, 0.0], [1000.0, 0.0], [1000.0, 600.0], [1300.0, 700.0]]);
        let here = at(1300.0, 700.0);
        let target = choose_target(None, None, Some(&car), tr.first().copied()).unwrap();
        let w = way_back(Some(&n), &tr, here, target, None).unwrap();
        assert_eq!(w.kind, WayKind::TracePistes);
        assert!(w.back.end().haversine_m(car.pos()) < 1.0);
        // ≈ 270 m back along the way skied to the piste, then ≈ 615 + 1000 m and the car park.
        let len = w.back.length_m();
        assert!((len - 2000.0).abs() < 60.0, "{len}");
        // The arrow: back along the way skied (west-south-west), never across to the car.
        let b = bearing(here, w.back.guide_point());
        assert!((b - 251.6).abs() < 8.0, "{b}");
        // The line passes by the piste where the way skied left it.
        let line = w.back.line();
        assert!(
            line.iter()
                .any(|p| plane().xy(*p)[0].abs() < 1.0 && plane().xy(*p)[1].abs() < 1.0),
            "A is on the way"
        );
    }

    #[test]
    fn without_the_pistes_the_way_skied_is_followed_back() {
        let car = car_at(-30.0, -20.0);
        let tr = skied(&[[0.0, 0.0], [0.0, 600.0], [400.0, 600.0]]);
        let here = *tr.last().unwrap();
        let target = choose_target(None, None, Some(&car), tr.first().copied()).unwrap();
        let w = way_back(None, &tr, here, target.clone(), None).unwrap();
        assert_eq!(w.kind, WayKind::Trace);
        assert!(w.back.end().haversine_m(car.pos()) < 1.0);
        assert!(
            (w.back.length_m() - 1036.0).abs() < 25.0,
            "{}",
            w.back.length_m()
        );
        // A network too far from the skier and from the car: the way skied as well.
        let g = network();
        let n = net(&g);
        let far: Vec<LonLat> = tr
            .iter()
            .map(|p| {
                let q = plane().xy(*p);
                at(q[0] + 5000.0, q[1] + 5000.0)
            })
            .collect();
        let car_far = car_at(4970.0, 4980.0);
        let target = choose_target(None, None, Some(&car_far), far.first().copied()).unwrap();
        let w = way_back(Some(&n), &far, *far.last().unwrap(), target, None).unwrap();
        assert_eq!(w.kind, WayKind::Trace);
        // Nothing skied: no way.
        assert!(way_back(None, &[], here, target_of(&car), None).is_none());
    }

    /// Skis from `from` to `to` (plane, metres) at 3 m/s, one fix a second from `t0`;
    /// the second at which `b` says the arrival, if it does.
    fn ski(b: &mut Backtrack, t0: f64, from: [f64; 2], to: [f64; 2]) -> (f64, Option<f64>) {
        let n = (dist(from, to) / 3.0).ceil().max(1.0) as u32;
        let mut t = t0;
        for k in 1..=n {
            let f = f64::from(k) / f64::from(n);
            t += 1.0;
            let p = [
                from[0] + f * (to[0] - from[0]),
                from[1] + f * (to[1] - from[1]),
            ];
            if b.fix(t, at(p[0], p[1])).arrived {
                return (t, Some(t));
            }
        }
        (t, None)
    }

    #[test]
    fn back_where_the_car_was_recorded_is_arrived_though_it_was_moved_onto_the_car_park() {
        // The car recorded at the first fix (0, 0), moved onto the car park 70 m south
        // (the representative point of a car park drawn as an area in OSM).
        let parkings = vec![(None, at(0.0, -70.0))];
        let mut spot = CarSpotter::default();
        spot.fix(0.0, at(0.0, 0.0), &parkings);
        spot.fix(5.0, at(2.0, 0.0), &parkings);
        let car = spot.car().unwrap().clone();
        assert!(car.pos().haversine_m(at(0.0, -70.0)) < 0.5);
        // Out along the pistes 2 km east, then « ramène-moi » back to where it started.
        let tr = skied(&[[0.0, 0.0], [2000.0, 0.0]]);
        let here = *tr.last().unwrap();
        let g = network();
        let n = net(&g);
        for net in [Some(&n), None] {
            let target = choose_target(None, None, Some(&car), tr.first().copied()).unwrap();
            assert_eq!(target.kind, TargetKind::Car);
            let mut w = way_back(net, &tr, here, target, None).unwrap();
            let (_, arrived) = ski(&mut w.back, 0.0, [2000.0, 0.0], [0.0, 0.0]);
            assert!(
                arrived.is_some(),
                "{:?}: {:.0} m left back where the car was recorded",
                w.kind,
                w.back.remaining_m()
            );
            // 60 m short of it: not yet; straight on to the car park (never within 40 m
            // of where the car was recorded): arrived there.
            let mut w = way_back(net, &tr, here, w.target, None).unwrap();
            let (t, arrived) = ski(&mut w.back, 0.0, [2000.0, 0.0], [60.0, 0.0]);
            assert!(arrived.is_none(), "{:?}: arrived 60 m short", w.kind);
            let (_, arrived) = ski(&mut w.back, t, [60.0, 0.0], [0.0, -70.0]);
            assert!(
                arrived.is_some(),
                "{:?}: not arrived at the car park",
                w.kind
            );
        }
    }

    #[test]
    fn back_at_the_car_by_another_way_than_the_one_guided_is_arrived() {
        // Out from the car at A (0, 0) along the pistes to B, then up to D (1000, 1000).
        // The way back goes down to B and west to A; the skier cuts straight across
        // instead, off the way, and gets to the car all the same.
        let car = car_at(0.0, 0.0);
        let tr = skied(&[[0.0, 0.0], [1000.0, 0.0], [1000.0, 1000.0]]);
        let here = *tr.last().unwrap();
        let g = network();
        let n = net(&g);
        for net in [Some(&n), None] {
            let target = choose_target(None, None, Some(&car), tr.first().copied()).unwrap();
            let mut w = way_back(net, &tr, here, target, None).unwrap();
            let (t, arrived) = ski(&mut w.back, 0.0, [1000.0, 1000.0], [60.0, 60.0]);
            assert!(arrived.is_none(), "{:?}: arrived 85 m short", w.kind);
            assert!(w.back.is_off(), "{:?}", w.kind);
            let (_, arrived) = ski(&mut w.back, t, [60.0, 60.0], [0.0, 0.0]);
            assert!(
                arrived.is_some(),
                "{:?}: at the car, {:.0} m left along the way",
                w.kind,
                w.back.remaining_m()
            );
        }
    }

    fn target_of(car: &Car) -> Target {
        Target {
            kind: TargetKind::Car,
            pos: car.pos(),
            name: None,
            recorded: None,
        }
    }

    #[test]
    fn the_arrow_points_to_the_next_point_of_the_way_not_across_a_ravine() {
        // A U: north 500 m, east 300 m, south 500 m; the end is 300 m east across the
        // ravine between the two arms.
        let line = [
            at(0.0, 0.0),
            at(0.0, 500.0),
            at(300.0, 500.0),
            at(300.0, 0.0),
        ];
        let mut b = Backtrack::along(&line).unwrap();
        let end = b.end();
        assert!((bearing(line[0], end) - 90.0).abs() < 1.0);
        // On the way at the start: 60 m ahead, north.
        b.fix(0.0, line[0]);
        let g = b.guide_point();
        assert!(bearing(line[0], g).abs() < 1.0);
        assert!((plane().xy(g)[1] - GUIDE_AHEAD_M).abs() < 1.0);
        // Near the first corner: the corner itself, not a cut across.
        b.fix(20.0, at(0.0, 460.0));
        let g = plane().xy(b.guide_point());
        assert!(g[0].abs() < 1.0 && (g[1] - 500.0).abs() < 1.0, "{g:?}");
        // Off the way, 100 m east of the first arm at 200 m (in the ravine side): back to
        // the way just ahead (west-north-west), never east to the end.
        let mut b = Backtrack::along(&line).unwrap();
        b.fix(0.0, line[0]);
        b.fix(60.0, at(100.0, 200.0));
        assert!(b.is_off());
        let here = at(100.0, 200.0);
        let g = plane().xy(b.guide_point());
        assert!(g[0].abs() < 1.0 && (g[1] - 230.0).abs() < 2.0, "{g:?}");
        let br = bearing(here, b.guide_point());
        assert!((270.0..=300.0).contains(&br), "{br}");
    }

    #[test]
    fn going_home_is_proposed_only_when_the_tracker_has_no_way_home() {
        assert!(should_propose(true, false, false, false));
        // The tracker guides home on the pistes itself.
        assert!(!should_propose(true, true, false, false));
        // Already going back, another proposal waiting, or no return time due.
        assert!(!should_propose(true, false, true, false));
        assert!(!should_propose(true, false, false, true));
        assert!(!should_propose(false, false, false, false));
        let car = target_of(&car_at(0.0, 0.0));
        assert_eq!(
            proposal_text(Some(&car), skimap_common::Lang::Fr),
            "Il est temps de rentrer. Être guidé jusqu'à la voiture ?"
        );
        let gite = Target {
            kind: TargetKind::Destination,
            pos: at(0.0, 0.0),
            name: Some("Gîte du Laus".into()),
            recorded: None,
        };
        assert_eq!(
            proposal_text(Some(&gite), skimap_common::Lang::Fr),
            "Il est temps de rentrer. Être guidé jusqu'à Gîte du Laus ?"
        );
        assert!(proposal_text(None, skimap_common::Lang::Fr).ends_with("jusqu'au départ ?"));
        for t in [
            proposal_text(Some(&car), skimap_common::Lang::Fr),
            proposal_text(Some(&gite), skimap_common::Lang::Fr),
        ] {
            assert!(!t.contains("sûr") && !t.contains("sans danger"), "{t}");
        }
    }
}
