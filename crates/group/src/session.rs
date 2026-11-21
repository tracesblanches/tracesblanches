//! One phone's view of the group during an outing: its own position and flags, the last
//! known position of every other member (age, source, hops), the frames to send (its own,
//! and in turn the last positions of the others: the hop-by-hop relay), and the alerts.
//!
//! The clock is given by the caller (milliseconds): the scenarios run with a simulated one.
//!
//! Alerts (report 10, §1), each raised once and raised again only after its condition
//! cleared:
//! - **straggler**: not heard for more than [`Config::straggler_after_s`] (5 min) **and**
//!   last position more than [`Config::straggler_behind_m`] (500 m) behind along the
//!   common plan, the member having been behind this phone at the time of that position
//!   (else this phone is the one who dropped: the others are ahead); without the progress
//!   along the plan, last position more than that distance away (both sides alert);
//! - **long stop**: the member's phone says it is stopped for long away from a refuge
//!   (paused for more than [`Config::stop_after_s`], not at a refuge: flag `STOPPED`),
//!   unless this phone is stopped with it ([`TOGETHER_M`]: the whole group stopped);
//! - **off the route**: the member's phone says it is off the route (flag `OFF_ROUTE`).
//!
//! A loss of contact alone never raises anything: the body masks the link at 5 m.
//!
//! Off the trails (SRN, #77), [`Config::off_piste`]: the straggler told as soon after the
//! loss of contact at the pace of nordic touring, last positions kept all day, « regroup »
//! instead of « wait at the next junction ».

use crate::Error;
use crate::frame::{FRAME_LEN, Keys, MAX_HOPS, PREFIX_LEN, Position, epoch, flags};
use crate::group::Group;

/// Language of the alerts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lang {
    /// French.
    Fr,
    /// English.
    En,
}

/// Settings of the alerts.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Config {
    /// Not heard for more than this, seconds (straggler).
    pub straggler_after_s: i64,
    /// Behind by more than this, metres (straggler).
    pub straggler_behind_m: f64,
    /// Paused away from a refuge for more than this, seconds (own flag `STOPPED`).
    pub stop_after_s: i64,
    /// Positions older than this are neither kept nor relayed, seconds.
    pub max_age_s: i64,
    /// Fixes dated more than this after now are refused (clock skew), seconds.
    pub max_skew_s: i64,
    /// Offset of local time, seconds (clock times in the alerts).
    pub utc_offset_s: i64,
    /// Whether the plan this phone follows is the group's: only then is the progress along
    /// the plan sent and compared.
    pub plan_matches: bool,
    /// Language of the alerts.
    pub lang: Lang,
    /// Off-piste outing (SRN): no junction to wait at, the group regroups where it is.
    pub off_piste: bool,
}

impl Config {
    /// Settings of an off-piste outing (SRN, #77), chosen by the outing itself (a guided
    /// off-piste line or bearing), never by a setting:
    /// - straggler when the last position heard is [`SRN_STRAGGLER_BEHIND_M`] behind
    ///   instead of 500 m: at the pace of nordic touring off the trails (≈ 3 km/h) a
    ///   member who drops behind is told as soon as on trails at the family pace (5 km/h,
    ///   500 m): ≈ 8 min after starting to drop 0.2 m/s behind, instead of ≈ 12 min
    ///   (scenarios, `tests/scenarios.rs`);
    /// - last positions kept and relayed [`SRN_MAX_AGE_S`] (6 h) instead of 3 h: off the
    ///   trails the last known position of a member is all there is to go back to, over a
    ///   whole day out (the frame's time stays readable for 9 h);
    /// - alerts that say to regroup where one is (no junction off the trails).
    pub fn off_piste(self) -> Self {
        Self {
            straggler_behind_m: SRN_STRAGGLER_BEHIND_M,
            max_age_s: SRN_MAX_AGE_S,
            off_piste: true,
            ..self
        }
    }
}

/// Straggler distance off the trails (SRN), metres ([`Config::off_piste`]).
pub const SRN_STRAGGLER_BEHIND_M: f64 = 300.0;

/// Age of the positions kept and relayed off the trails (SRN), seconds.
pub const SRN_MAX_AGE_S: i64 = 6 * 3600;

/// Within this of this phone, by a position dated within this phone's stop (or the
/// [`TOGETHER_HEARD_S`] around it): together (a stop of the whole group is no alert).
pub const TOGETHER_M: f64 = 100.0;

/// See [`TOGETHER_M`], seconds.
pub const TOGETHER_HEARD_S: i64 = 120;

impl Default for Config {
    fn default() -> Self {
        Self {
            straggler_after_s: 300,
            straggler_behind_m: 500.0,
            stop_after_s: 600,
            max_age_s: 3 * 3600,
            max_skew_s: 120,
            utc_offset_s: 3600,
            plan_matches: true,
            lang: Lang::Fr,
            off_piste: false,
        }
    }
}

/// Phase of the outing as the tracker sees it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Phase {
    /// Moving (or waiting for the start).
    Moving,
    /// Paused, not at a refuge.
    Paused,
    /// Paused at a refuge.
    Refuge,
}

/// This phone's own position, from the tracker.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct OwnFix {
    /// Time of the fix, Unix milliseconds.
    pub t_ms: i64,
    /// Degrees.
    pub lat: f64,
    /// Degrees.
    pub lon: f64,
    /// Phase of the outing.
    pub phase: Phase,
    /// Off the route followed (the tracker's rule).
    pub off_route: bool,
    /// Progress along the plan followed, metres.
    pub progress_m: Option<f64>,
    /// Battery low.
    pub battery_low: bool,
}

/// Kind of alert.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AlertKind {
    /// Not heard for long and far behind.
    Straggler,
    /// Stopped for long away from a refuge.
    LongStop,
    /// Off the common route.
    OffRoute,
}

const KINDS: [AlertKind; 3] = [
    AlertKind::Straggler,
    AlertKind::LongStop,
    AlertKind::OffRoute,
];

/// An alert about a member.
#[derive(Debug, Clone, PartialEq)]
pub struct Alert {
    /// Kind.
    pub kind: AlertKind,
    /// Member concerned.
    pub member: u8,
    /// What to say and show.
    pub text: String,
    /// Age of the member's last position, seconds.
    pub age_s: i64,
    /// Straight-line distance from this phone, metres.
    pub distance_m: f64,
    /// Behind along the plan, metres (positive: behind).
    pub behind_m: Option<f64>,
}

/// How a member's last position came.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Source {
    /// From the member's own phone.
    Direct,
    /// Relayed by other members, that many hops.
    Relayed(u8),
}

/// What the map and the list show of a member.
#[derive(Debug, Clone, PartialEq)]
pub struct MemberView {
    /// Number.
    pub member: u8,
    /// Name.
    pub name: String,
    /// This phone's member.
    pub me: bool,
    /// Last known position (`None`: never heard), with its time, Unix seconds.
    pub position: Option<(f64, f64, i64)>,
    /// Age of the last position, seconds.
    pub age_s: Option<i64>,
    /// Source of the last position.
    pub source: Option<Source>,
    /// Flags of the last position.
    pub flags: u8,
    /// Straight-line distance from this phone, metres.
    pub distance_m: Option<f64>,
    /// Behind along the plan, metres (positive: behind this phone).
    pub behind_m: Option<f64>,
}

/// What a received frame did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Received {
    /// A newer position of the member.
    New,
    /// The same position by fewer hops.
    Shorter,
    /// Not newer than the one known (a replay, or a relay of what is known).
    Stale,
    /// This phone's own position relayed back.
    Own,
}

/// Counters (logs and tests).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Stats {
    /// Frames received.
    pub received: u32,
    /// Frames refused (other group, old tag, MAC, unknown member, future).
    pub refused: u32,
    /// Frames that gave a newer position.
    pub new: u32,
    /// Frames not newer (replays, relays of what is known).
    pub stale: u32,
}

/// One phone's group session.
pub struct Session {
    group: Group,
    me: u8,
    keys: Keys,
    cfg: Config,
    own: Option<OwnFix>,
    paused_since_ms: Option<i64>,
    /// This phone's last stop (paused or at a refuge): its first and last fix, Unix
    /// milliseconds.
    stop_ms: Option<(i64, i64)>,
    /// This phone's progress along the plan over time (seconds, metres), for the order of
    /// the members at the time of their last position.
    history: std::collections::VecDeque<(i64, f64)>,
    table: Vec<Option<Position>>,
    relay_cursor: usize,
    raised: Vec<[bool; 3]>,
    stats: Stats,
}

impl std::fmt::Debug for Session {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Session")
            .field("group", &self.group)
            .field("me", &self.me)
            .field("stats", &self.stats)
            .finish_non_exhaustive()
    }
}

/// Great-circle distance, metres.
pub fn distance_m(a: (f64, f64), b: (f64, f64)) -> f64 {
    let (la1, lo1, la2, lo2) = (
        a.0.to_radians(),
        a.1.to_radians(),
        b.0.to_radians(),
        b.1.to_radians(),
    );
    let h = ((la2 - la1) / 2.0).sin().powi(2)
        + la1.cos() * la2.cos() * ((lo2 - lo1) / 2.0).sin().powi(2);
    2.0 * 6_371_008.8 * h.sqrt().min(1.0).asin()
}

impl Session {
    /// The session of member `me` of `group`.
    pub fn new(group: Group, me: u8, cfg: Config) -> Result<Self, Error> {
        if usize::from(me) >= group.members.len() {
            return Err(Error::Member(me));
        }
        let n = group.members.len();
        Ok(Self {
            keys: Keys::derive(&group.key),
            group,
            me,
            cfg,
            own: None,
            paused_since_ms: None,
            stop_ms: None,
            history: std::collections::VecDeque::new(),
            table: vec![None; n],
            relay_cursor: 0,
            raised: vec![[false; 3]; n],
            stats: Stats::default(),
        })
    }

    /// The group.
    pub fn group(&self) -> &Group {
        &self.group
    }

    /// This phone's member.
    pub fn me(&self) -> u8 {
        self.me
    }

    /// Counters.
    pub fn stats(&self) -> Stats {
        self.stats
    }

    /// Settings.
    pub fn config(&self) -> &Config {
        &self.cfg
    }

    /// This phone's new position (each GPS batch).
    pub fn own(&mut self, fix: OwnFix) {
        if fix.phase != Phase::Moving {
            let moving_before = self.own.is_none_or(|o| o.phase == Phase::Moving);
            self.stop_ms = match self.stop_ms {
                Some((start, _)) if !moving_before => Some((start, fix.t_ms)),
                _ => Some((fix.t_ms, fix.t_ms)),
            };
        }
        self.paused_since_ms = match fix.phase {
            Phase::Paused => Some(self.paused_since_ms.unwrap_or(fix.t_ms)),
            Phase::Moving | Phase::Refuge => None,
        };
        self.own = Some(fix);
        if let Some(m) = self.own_progress() {
            let t = fix.t_ms.div_euclid(1000);
            if self.history.back().is_none_or(|&(u, _)| t > u) {
                self.history.push_back((t, m));
            }
            while self
                .history
                .front()
                .is_some_and(|&(u, _)| t - u > self.cfg.max_age_s)
            {
                self.history.pop_front();
            }
        }
    }

    /// This phone's progress at `t_s` (interpolated; `None` before the history).
    fn progress_at(&self, t_s: i64) -> Option<f64> {
        let i = self.history.partition_point(|&(u, _)| u <= t_s);
        let (t0, m0) = *self.history.get(i.checked_sub(1)?)?;
        Some(match self.history.get(i) {
            Some(&(t1, m1)) => m0 + (m1 - m0) * (t_s - t0) as f64 / (t1 - t0) as f64,
            None => m0,
        })
    }

    /// Flags of this phone's frame now.
    pub fn own_flags(&self, now_ms: i64) -> u8 {
        let Some(o) = self.own else { return 0 };
        let mut f = 0;
        if self
            .paused_since_ms
            .is_some_and(|t| now_ms - t > self.cfg.stop_after_s * 1000)
        {
            f |= flags::STOPPED;
        }
        if o.off_route {
            f |= flags::OFF_ROUTE;
        }
        if o.battery_low {
            f |= flags::BATTERY_LOW;
        }
        f
    }

    fn own_progress(&self) -> Option<f64> {
        if !(self.cfg.plan_matches && self.group.has_plan()) {
            return None;
        }
        self.own.and_then(|o| o.progress_m)
    }

    /// This phone's frame, sent at `now_ms` (`None` before the first position).
    pub fn own_frame(&self, now_ms: i64) -> Option<[u8; FRAME_LEN]> {
        let o = self.own?;
        let p = Position {
            member: self.me,
            hops: 0,
            t_s: o.t_ms.div_euclid(1000),
            lat: o.lat,
            lon: o.lon,
            flags: self.own_flags(now_ms),
            progress_m: self.own_progress(),
        };
        Some(self.keys.seal(&p, now_ms.div_euclid(1000)))
    }

    /// The next of the others' last positions to relay (in turn, one per call), one hop
    /// more; `None` when there is nothing to relay.
    pub fn relay_frame(&mut self, now_ms: i64) -> Option<[u8; FRAME_LEN]> {
        let now_s = now_ms.div_euclid(1000);
        let n = self.table.len();
        for k in 0..n {
            let i = (self.relay_cursor + k) % n;
            let Some(p) = self.table[i] else { continue };
            if i == usize::from(self.me) || p.hops >= MAX_HOPS || now_s - p.t_s > self.cfg.max_age_s
            {
                continue;
            }
            self.relay_cursor = (i + 1) % n;
            let relayed = Position {
                hops: p.hops + 1,
                ..p
            };
            return Some(self.keys.seal(&relayed, now_s));
        }
        None
    }

    /// Prefixes (version and tag) of the frames that may be received at `now_ms`: the
    /// epochs before, of and after now. The scan filters on them (the controller wakes the
    /// processor only for this group).
    pub fn scan_prefixes(&self, now_ms: i64) -> Vec<[u8; PREFIX_LEN]> {
        let e = epoch(now_ms.div_euclid(1000));
        vec![
            self.keys.prefix(e),
            self.keys.prefix(e.wrapping_add(1)),
            self.keys.prefix(e.wrapping_sub(1)),
        ]
    }

    /// A frame received at `now_ms`. Anti-replay: only a newer position of a member is
    /// kept (or the same by fewer hops); frames of old epochs are refused by their tag.
    pub fn receive(&mut self, now_ms: i64, frame: &[u8]) -> Result<Received, Error> {
        self.stats.received += 1;
        let now_s = now_ms.div_euclid(1000);
        let r = self.accept(now_s, frame);
        match r {
            Ok(Received::New | Received::Shorter) => self.stats.new += 1,
            Ok(Received::Stale | Received::Own) => self.stats.stale += 1,
            Err(_) => self.stats.refused += 1,
        }
        r
    }

    fn accept(&mut self, now_s: i64, frame: &[u8]) -> Result<Received, Error> {
        let p = self.keys.open(frame, now_s)?;
        let i = usize::from(p.member);
        if i >= self.table.len() {
            return Err(Error::Member(p.member));
        }
        if p.t_s > now_s + self.cfg.max_skew_s {
            return Err(Error::Frame("future"));
        }
        if p.member == self.me {
            return Ok(Received::Own);
        }
        if now_s - p.t_s > self.cfg.max_age_s {
            return Ok(Received::Stale);
        }
        match self.table[i] {
            Some(q) if p.t_s < q.t_s || (p.t_s == q.t_s && p.hops >= q.hops) => Ok(Received::Stale),
            Some(q) if p.t_s == q.t_s => {
                self.table[i] = Some(Position { hops: p.hops, ..q });
                Ok(Received::Shorter)
            }
            _ => {
                self.table[i] = Some(p);
                Ok(Received::New)
            }
        }
    }

    fn behind(&self, p: &Position) -> Option<f64> {
        let mine = self.own_progress()?;
        p.progress_m.map(|theirs| mine - theirs)
    }

    /// Every member, this phone's first: last position, age, source, distance, behind.
    pub fn members(&self, now_ms: i64) -> Vec<MemberView> {
        let now_s = now_ms.div_euclid(1000);
        let here = self.own.map(|o| (o.lat, o.lon));
        let mut out = Vec::with_capacity(self.table.len());
        for (i, name) in self.group.members.iter().enumerate() {
            let me = i == usize::from(self.me);
            let v = if me {
                MemberView {
                    member: i as u8,
                    name: name.clone(),
                    me,
                    position: self.own.map(|o| (o.lat, o.lon, o.t_ms.div_euclid(1000))),
                    age_s: self.own.map(|o| now_s - o.t_ms.div_euclid(1000)),
                    source: self.own.map(|_| Source::Direct),
                    flags: self.own_flags(now_ms),
                    distance_m: here.map(|_| 0.0),
                    behind_m: self.own_progress().map(|_| 0.0),
                }
            } else {
                let p = self.table[i];
                MemberView {
                    member: i as u8,
                    name: name.clone(),
                    me,
                    position: p.map(|p| (p.lat, p.lon, p.t_s)),
                    age_s: p.map(|p| now_s - p.t_s),
                    source: p.map(|p| {
                        if p.hops == 0 {
                            Source::Direct
                        } else {
                            Source::Relayed(p.hops)
                        }
                    }),
                    flags: p.map_or(0, |p| p.flags),
                    distance_m: p.and_then(|p| here.map(|h| distance_m(h, (p.lat, p.lon)))),
                    behind_m: p.and_then(|p| self.behind(&p)),
                }
            };
            out.push(v);
        }
        out.sort_by_key(|v| !v.me);
        out
    }

    /// The alerts raised at `now_ms` (each once, again only after its condition cleared).
    pub fn alerts(&mut self, now_ms: i64) -> Vec<Alert> {
        let now_s = now_ms.div_euclid(1000);
        let Some(here) = self.own.map(|o| (o.lat, o.lon)) else {
            return Vec::new();
        };
        let mut out = Vec::new();
        for i in 0..self.table.len() {
            if i == usize::from(self.me) {
                continue;
            }
            let Some(p) = self.table[i] else { continue };
            let age_s = now_s - p.t_s;
            let distance = distance_m(here, (p.lat, p.lon));
            let behind = self.behind(&p);
            // Stopped together: this phone stopped too (or just now), the member's position
            // nearby during that stop (a picnic, a change of skins, a regrouping; its last
            // frame of the stop still known when the group starts again).
            let together = self.stop_ms.is_some_and(|(start, end)| {
                now_s - end.div_euclid(1000) <= TOGETHER_HEARD_S
                    && p.t_s >= start.div_euclid(1000) - TOGETHER_HEARD_S
            }) && distance <= TOGETHER_M;
            // Along the plan: far behind now, and not ahead of this phone when last heard
            // (else it is this phone that dropped behind). Without the plan: far away.
            let far = match behind {
                Some(b) => {
                    b > self.cfg.straggler_behind_m
                        && self
                            .progress_at(p.t_s)
                            .zip(p.progress_m)
                            .is_none_or(|(mine, theirs)| theirs <= mine)
                }
                None => distance > self.cfg.straggler_behind_m,
            };
            for (k, kind) in KINDS.iter().enumerate() {
                let on = match kind {
                    AlertKind::Straggler => age_s > self.cfg.straggler_after_s && far,
                    AlertKind::LongStop => p.flags & flags::STOPPED != 0 && !together,
                    AlertKind::OffRoute => p.flags & flags::OFF_ROUTE != 0,
                };
                if on && !self.raised[i][k] {
                    out.push(Alert {
                        kind: *kind,
                        member: i as u8,
                        text: self.text(
                            *kind,
                            &self.group.members[i],
                            age_s,
                            distance,
                            behind,
                            p.t_s,
                        ),
                        age_s,
                        distance_m: distance,
                        behind_m: behind,
                    });
                }
                self.raised[i][k] = on;
            }
        }
        out
    }

    fn text(
        &self,
        kind: AlertKind,
        name: &str,
        age_s: i64,
        distance: f64,
        behind: Option<f64>,
        t_s: i64,
    ) -> String {
        let fr = self.cfg.lang == Lang::Fr;
        let min = age_s.max(0) / 60;
        let clock = clock(t_s + self.cfg.utc_offset_s, fr);
        let where_ = match behind {
            Some(b) if fr => format!("{} derrière", metres(b.abs(), fr)),
            Some(b) => format!("{} behind", metres(b.abs(), fr)),
            None if fr => format!("à {}", metres(distance, fr)),
            None => format!("{} away", metres(distance, fr)),
        };
        match (kind, fr) {
            (AlertKind::Straggler, true) if self.cfg.off_piste => format!(
                "{name} : pas de nouvelles depuis {min} min, dernière position {where_} à {clock}. S'arrêter et regrouper ?"
            ),
            (AlertKind::Straggler, false) if self.cfg.off_piste => format!(
                "{name}: not heard for {min} min, last seen {where_} at {clock}. Stop and regroup?"
            ),
            (AlertKind::Straggler, true) => format!(
                "{name} : pas de nouvelles depuis {min} min, dernière position {where_} à {clock}. Attendre à la prochaine intersection ?"
            ),
            (AlertKind::Straggler, false) => format!(
                "{name}: not heard for {min} min, last seen {where_} at {clock}. Wait at the next junction?"
            ),
            (AlertKind::LongStop, true) => {
                format!("{name} : arrêt prolongé hors d'un refuge, {where_}, position de {clock}.")
            }
            (AlertKind::LongStop, false) => format!(
                "{name}: stopped for long away from a refuge, {where_}, position of {clock}."
            ),
            (AlertKind::OffRoute, true) => {
                format!("{name} : hors de l'itinéraire commun, {where_}, position de {clock}.")
            }
            (AlertKind::OffRoute, false) => {
                format!("{name}: off the common route, {where_}, position of {clock}.")
            }
        }
    }
}

fn clock(local_s: i64, fr: bool) -> String {
    let m = local_s.rem_euclid(86_400) / 60;
    if fr {
        format!("{} h {:02}", m / 60, m % 60)
    } else {
        format!("{}:{:02}", m / 60, m % 60)
    }
}

fn metres(m: f64, fr: bool) -> String {
    if m >= 1000.0 {
        let km = format!("{:.1}", m / 1000.0);
        if fr {
            format!("{} km", km.replace('.', ","))
        } else {
            format!("{km} km")
        }
    } else {
        format!("{} m", ((m / 10.0).round() * 10.0) as i64)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    const T0: i64 = 1_768_000_000_000;

    fn group() -> Group {
        Group::create(
            [3u8; 32],
            vec!["Pierre".into(), "Léa".into(), "Jo".into()],
            [1u8; 8],
            0,
        )
        .unwrap()
    }

    fn fix(t_ms: i64, lat: f64, progress: f64) -> OwnFix {
        OwnFix {
            t_ms,
            lat,
            lon: 6.73,
            phase: Phase::Moving,
            off_route: false,
            progress_m: Some(progress),
            battery_low: false,
        }
    }

    #[test]
    fn texts() {
        assert_eq!(clock(10 * 3600 + 42 * 60 + 5, true), "10 h 42");
        assert_eq!(clock(9 * 3600 + 5 * 60, false), "9:05");
        assert_eq!(metres(447.0, true), "450 m");
        assert_eq!(metres(1_234.0, true), "1,2 km");
        assert_eq!(metres(1_234.0, false), "1.2 km");
        assert!((distance_m((44.0, 6.0), (44.009, 6.0)) - 1000.8).abs() < 1.0);
    }

    #[test]
    fn unknown_member_is_refused() {
        assert!(matches!(
            Session::new(group(), 3, Config::default()),
            Err(Error::Member(3))
        ));
    }

    #[test]
    fn replay_and_relays_keep_the_newest() {
        let mut a = Session::new(group(), 0, Config::default()).unwrap();
        let mut b = Session::new(group(), 1, Config::default()).unwrap();
        b.own(fix(T0, 44.87, 100.0));
        let old = b.own_frame(T0).unwrap();
        b.own(fix(T0 + 30_000, 44.871, 200.0));
        let new = b.own_frame(T0 + 30_000).unwrap();
        assert_eq!(a.receive(T0 + 30_000, &new).unwrap(), Received::New);
        // The older frame replayed afterwards changes nothing.
        assert_eq!(a.receive(T0 + 31_000, &old).unwrap(), Received::Stale);
        assert_eq!(a.receive(T0 + 31_000, &new).unwrap(), Received::Stale);
        let v = &a.members(T0 + 31_000)[1];
        assert_eq!(v.position.unwrap().2, (T0 + 30_000) / 1000);
        assert_eq!(v.source, Some(Source::Direct));
        // C gets B's position from A (one hop), then from B itself (shorter).
        let mut c = Session::new(group(), 2, Config::default()).unwrap();
        let relayed = a.relay_frame(T0 + 32_000).unwrap();
        assert_eq!(c.receive(T0 + 32_000, &relayed).unwrap(), Received::New);
        assert_eq!(c.members(T0 + 32_000)[2].source, Some(Source::Relayed(1)));
        assert_eq!(c.receive(T0 + 33_000, &new).unwrap(), Received::Shorter);
        assert_eq!(c.members(T0 + 33_000)[2].source, Some(Source::Direct));
        // B hears its own position relayed: ignored.
        assert_eq!(b.receive(T0 + 33_000, &relayed).unwrap(), Received::Own);
        // A frame 30 min old (two epochs) is refused by its tag.
        assert!(a.receive(T0 + 30_000 + 1_800_000, &new).is_err());
        assert_eq!(a.stats().refused, 1);
    }

    #[test]
    fn future_fixes_are_refused() {
        let mut a = Session::new(group(), 0, Config::default()).unwrap();
        let mut b = Session::new(group(), 1, Config::default()).unwrap();
        b.own(fix(T0 + 600_000, 44.87, 0.0));
        let f = b.own_frame(T0).unwrap();
        assert!(matches!(a.receive(T0, &f), Err(Error::Frame("future"))));
    }

    #[test]
    fn relay_turns_over_the_members() {
        let mut a = Session::new(group(), 0, Config::default()).unwrap();
        assert!(a.relay_frame(T0).is_none());
        for m in [1u8, 2] {
            let mut s = Session::new(group(), m, Config::default()).unwrap();
            s.own(fix(T0, 44.87 + f64::from(m) * 0.001, 0.0));
            a.receive(T0, &s.own_frame(T0).unwrap()).unwrap();
        }
        let k = Keys::derive(&group().key);
        let got: Vec<u8> = (0..4)
            .map(|_| {
                k.open(&a.relay_frame(T0).unwrap(), T0 / 1000)
                    .unwrap()
                    .member
            })
            .collect();
        assert_eq!(got, vec![1, 2, 1, 2]);
        // Too old: not relayed any more.
        assert!(a.relay_frame(T0 + 4 * 3_600_000).is_none());
    }

    #[test]
    fn own_stop_flag_only_away_from_a_refuge() {
        let mut a = Session::new(group(), 0, Config::default()).unwrap();
        let mut f = fix(T0, 44.87, 0.0);
        f.phase = Phase::Paused;
        a.own(f);
        a.own(OwnFix {
            t_ms: T0 + 300_000,
            ..f
        });
        assert_eq!(a.own_flags(T0 + 300_000) & flags::STOPPED, 0);
        a.own(OwnFix {
            t_ms: T0 + 601_000,
            ..f
        });
        assert_ne!(a.own_flags(T0 + 601_000) & flags::STOPPED, 0);
        a.own(OwnFix {
            t_ms: T0 + 900_000,
            phase: Phase::Refuge,
            ..f
        });
        assert_eq!(a.own_flags(T0 + 900_000 + 3_600_000) & flags::STOPPED, 0);
    }

    proptest! {
        /// Never an alert while the member stays within the distance, whatever the silence.
        #[test]
        fn no_alert_on_loss_of_contact_alone(
            behind in 0.0f64..495.0, silence_s in 0i64..10_800, no_plan in any::<bool>(),
        ) {
            let cfg = Config { plan_matches: !no_plan, ..Config::default() };
            let mut a = Session::new(group(), 0, cfg).unwrap();
            let mut b = Session::new(group(), 1, cfg).unwrap();
            // B is `behind` metres behind along the plan and along the meridian.
            let dlat = behind / 111_195.0;
            b.own(fix(T0, 44.87, 5_000.0 - behind));
            a.receive(T0, &b.own_frame(T0).unwrap()).unwrap();
            a.own(fix(T0, 44.87 + dlat, 5_000.0));
            let now = T0 + silence_s * 1000;
            a.own(fix(now, 44.87 + dlat, 5_000.0));
            prop_assert!(a.alerts(now).is_empty());
        }

        /// Far behind and silent for more than T: exactly one straggler alert.
        #[test]
        fn straggler_once(behind in 520.0f64..5_000.0, silence_s in 301i64..7_200) {
            let mut a = Session::new(group(), 0, Config::default()).unwrap();
            let mut b = Session::new(group(), 1, Config::default()).unwrap();
            b.own(fix(T0, 44.87, 6_000.0 - behind));
            a.receive(T0, &b.own_frame(T0).unwrap()).unwrap();
            let now = T0 + silence_s * 1000;
            a.own(fix(now, 44.87 + behind / 111_195.0, 6_000.0));
            let al = a.alerts(now);
            prop_assert_eq!(al.len(), 1);
            prop_assert_eq!(al[0].kind, AlertKind::Straggler);
            prop_assert!(a.alerts(now + 60_000).is_empty());
        }
    }
}
