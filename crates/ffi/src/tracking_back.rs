//! « Ramène-moi » (K4 #54, F21): guided return along the way skied.
//!
//! The way back is the recorded trace (a point every 10 m) reversed from the last fix to
//! the first one, smoothed (GPS noise), with its loops cut: wherever the trace comes back
//! within [`SHORTCUT_M`] of an earlier point (a spur skied out and back, a loop closed),
//! the way back jumps there. Turns are the bends of that line of 50° or more over 50 m
//! arms; each is said once, about [`TURN_LEAD_M`] before it, with the phrase bank of the
//! pack (`Event::Turn`); off the way for 15 s at more than 40 m (C2) and back under 15 m
//! are said as on the plan; the arrival as well. The GPS mode comes from the core's rule
//! on the distance to the next turn (`skimap_plan::gps::decide`).
//!
//! Pure and allocation-free per fix (the line is built once, when the skier asks).

use skimap_common::geom::LonLat;
use skimap_core::graph::LocalPlane;
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

impl Backtrack {
    /// The way back from `here` along `trace` (oldest first); `None` when the trace is
    /// shorter than two points.
    pub fn new(trace: &[LonLat], here: LonLat) -> Option<Self> {
        let first = *trace.first()?;
        let plane = LocalPlane::new(first);
        let mut raw: Vec<[f64; 2]> = Vec::with_capacity(trace.len() + 1);
        raw.push(plane.xy(here));
        raw.extend(trace.iter().rev().map(|p| plane.xy(*p)));
        if raw.len() < 2 {
            return None;
        }
        // Smoothing: moving average over 5 points (≈ 40 m), ends kept.
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
        if pts.len() < 2 {
            pts.push(sm[n - 1]);
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
        };
        b.turns = b.find_turns();
        Some(b)
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
    /// off-the-way hysteresis, the arrival.
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
        if self.remaining_m() <= ARRIVED_M && d <= OFF_M {
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
}
