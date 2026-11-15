//! Speech policy « silence si tout va bien » (J4): what is said, when, and what is not.
//!
//! The tracker offers *candidates* (an announcement, a priority, a key, an expiry); the
//! policy decides at each tick what is spoken:
//!
//! - **hierarchy**: safety > direction > schedule > comfort; inside one spoken group the
//!   candidates are ordered by priority, then by time;
//! - **grouping**: candidates offered less than [`SpeechParams::group_s`] apart are said
//!   together (one announcement, long pauses between the parts); a safety candidate is
//!   said at once and takes the pending ones with it;
//! - **rate**: at most [`SpeechParams::max_per_hour`] spoken groups with a non-safety part
//!   in any hour; comfort only within [`SpeechParams::comfort_share`] of it; directions
//!   are exempt by default ([`SpeechParams::direction_exempt`]: a missed turn costs more
//!   than one more sentence), set it to `false` for a hard cap;
//! - **supersession**: a candidate replaces a pending one with the same key; a candidate
//!   past its expiry (a turn after its junction) is dropped;
//! - **pause**: while the outing is paused, only safety is said, the rest waits (and
//!   expires as usual);
//! - **safety is never lost**: a safety candidate is never dropped, never superseded by a
//!   non-safety one, never held by the pause or the rate, and is said at the first tick
//!   at or after its offer (property tested).
//!
//! Every candidate ends spoken or dropped with its reason ([`Speech::dropped`]).

use std::collections::VecDeque;

use serde::{Deserialize, Serialize};
use skimap_voice::bank::Announcement;

/// Priority of a candidate, highest first.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Priority {
    /// Safety: turn back now, weather turning, no network, avalanche path.
    Safety,
    /// Direction: turns, off the route, plan B on the way.
    Direction,
    /// Schedule: late, shortened loop.
    Schedule,
    /// Comfort: loop summary, end of the outing.
    Comfort,
}

/// An announcement offered to the policy.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Candidate {
    /// Time of the offer, seconds.
    pub t: f64,
    /// Priority.
    pub priority: Priority,
    /// Key: a newer candidate with the same key replaces a pending one.
    pub key: String,
    /// What it is (for logs and tests: `turn`, `off_route`, `fog`...).
    pub kind: String,
    /// Clips to play.
    pub announcement: Announcement,
    /// Not said after this time, seconds.
    pub expires: Option<f64>,
}

/// Why a candidate was not said.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DropReason {
    /// The hourly budget was spent.
    Budget,
    /// Past its expiry.
    Stale,
    /// Replaced by a newer candidate with the same key.
    Superseded,
}

/// A spoken group.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Spoken {
    /// Time, seconds.
    pub t: f64,
    /// Candidates said, in the order said.
    pub items: Vec<Candidate>,
    /// The announcement played.
    pub announcement: Announcement,
    /// Whether it counted in the hourly budget (it has a non-safety part).
    pub counted: bool,
    /// Whether it went beyond the budget (exempt directions).
    pub over_budget: bool,
}

/// Settings (user settings).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct SpeechParams {
    /// Spoken groups with a non-safety part per sliding hour.
    pub max_per_hour: u32,
    /// Share of that budget that comfort may use.
    pub comfort_share: f64,
    /// Directions said even beyond the budget.
    pub direction_exempt: bool,
    /// Grouping window, seconds.
    pub group_s: f64,
    /// Non-safety candidates in one group, at most (the rest goes to the next group).
    pub max_items: usize,
}

impl Default for SpeechParams {
    /// 12 an hour (one every five minutes on average), comfort half of it, directions
    /// exempt, 8 s of grouping, 3 items: assumptions to tune on the field (T5).
    fn default() -> Self {
        Self {
            max_per_hour: 12,
            comfort_share: 0.5,
            direction_exempt: true,
            group_s: 8.0,
            max_items: 3,
        }
    }
}

/// The policy.
#[derive(Debug, Clone)]
pub struct Speech {
    p: SpeechParams,
    pending: Vec<Candidate>,
    counted: VecDeque<f64>,
    dropped: Vec<(Candidate, DropReason)>,
    paused: bool,
}

impl Speech {
    /// Policy with settings.
    pub fn new(p: SpeechParams) -> Self {
        Self {
            p,
            pending: Vec::new(),
            counted: VecDeque::new(),
            dropped: Vec::new(),
            paused: false,
        }
    }

    /// Settings.
    pub fn params(&self) -> &SpeechParams {
        &self.p
    }

    /// Offers a candidate.
    pub fn offer(&mut self, c: Candidate) {
        // Same key: the newer one replaces the pending one, except a safety one, which is
        // never replaced (both stay).
        let mut i = 0;
        while i < self.pending.len() {
            let old = &self.pending[i];
            if old.key == c.key && old.priority != Priority::Safety {
                let old = self.pending.remove(i);
                self.dropped.push((old, DropReason::Superseded));
            } else {
                i += 1;
            }
        }
        self.pending.push(c);
    }

    /// Pause (only safety is said) or resume.
    pub fn set_paused(&mut self, paused: bool) {
        self.paused = paused;
    }

    /// Withdraws the pending candidates of a key (a turn that no longer applies).
    pub fn withdraw(&mut self, key: &str) {
        let mut i = 0;
        while i < self.pending.len() {
            if self.pending[i].key == key && self.pending[i].priority != Priority::Safety {
                let c = self.pending.remove(i);
                self.dropped.push((c, DropReason::Superseded));
            } else {
                i += 1;
            }
        }
    }

    /// Candidates not said, with the reason.
    pub fn dropped(&self) -> &[(Candidate, DropReason)] {
        &self.dropped
    }

    /// Candidates waiting.
    pub fn pending(&self) -> &[Candidate] {
        &self.pending
    }

    fn used(&mut self, now: f64) -> u32 {
        while self.counted.front().is_some_and(|t| *t <= now - 3600.0) {
            self.counted.pop_front();
        }
        self.counted.len() as u32
    }

    /// What is said at `now`, if anything.
    pub fn tick(&mut self, now: f64) -> Option<Spoken> {
        if self.pending.is_empty() {
            return None;
        }
        // Expired non-safety candidates are dropped (safety has no expiry that drops it).
        let mut i = 0;
        while i < self.pending.len() {
            let c = &self.pending[i];
            if c.priority != Priority::Safety && c.expires.is_some_and(|e| e < now) {
                let c = self.pending.remove(i);
                self.dropped.push((c, DropReason::Stale));
            } else {
                i += 1;
            }
        }
        // Whether anything is due, without allocating (the common case: nothing).
        let eligible =
            |c: &Candidate| c.t <= now && (!self.paused || c.priority == Priority::Safety);
        let safety = self
            .pending
            .iter()
            .any(|c| c.t <= now && c.priority == Priority::Safety);
        let oldest = self
            .pending
            .iter()
            .filter(|c| eligible(c))
            .map(|c| c.t)
            .fold(f64::INFINITY, f64::min);
        // A candidate with less than two grouping windows left (a turn close ahead at
        // speed: its expiry is the time to pass the point) does not wait for the group.
        let urgent = self.pending.iter().any(|c| {
            eligible(c)
                && c.priority != Priority::Safety
                && c.expires.is_some_and(|e| e < oldest + 2.0 * self.p.group_s)
        });
        if !oldest.is_finite() || !(safety || urgent || oldest + self.p.group_s <= now) {
            return None;
        }
        let mut order: Vec<usize> = (0..self.pending.len())
            .filter(|&i| eligible(&self.pending[i]))
            .collect();
        order.sort_by(|&a, &b| {
            let (x, y) = (&self.pending[a], &self.pending[b]);
            x.priority.cmp(&y.priority).then(x.t.total_cmp(&y.t))
        });
        let used = self.used(now);
        let budget = self.p.max_per_hour;
        let comfort_budget = (f64::from(budget) * self.p.comfort_share).floor() as u32;
        let mut take: Vec<usize> = Vec::new();
        let mut drop: Vec<usize> = Vec::new();
        let mut non_safety = 0usize;
        let mut over = false;
        for i in order {
            let c = &self.pending[i];
            match c.priority {
                Priority::Safety => take.push(i),
                _ if non_safety >= self.p.max_items => {}
                Priority::Direction if used < budget => {
                    take.push(i);
                    non_safety += 1;
                }
                Priority::Direction if self.p.direction_exempt => {
                    take.push(i);
                    non_safety += 1;
                    over = true;
                }
                Priority::Schedule if used < budget => {
                    take.push(i);
                    non_safety += 1;
                }
                Priority::Comfort if used < comfort_budget.min(budget) => {
                    take.push(i);
                    non_safety += 1;
                }
                _ => drop.push(i),
            }
        }
        let mut gone: Vec<usize> = take.iter().chain(&drop).copied().collect();
        gone.sort_unstable();
        let mut items = Vec::new();
        let mut dropped = Vec::new();
        for &i in gone.iter().rev() {
            let c = self.pending.remove(i);
            if take.contains(&i) {
                items.push((take.iter().position(|x| *x == i).expect("taken"), c));
            } else {
                dropped.push(c);
            }
        }
        for c in dropped.into_iter().rev() {
            self.dropped.push((c, DropReason::Budget));
        }
        if items.is_empty() {
            return None;
        }
        items.sort_by_key(|(k, _)| *k);
        let items: Vec<Candidate> = items.into_iter().map(|(_, c)| c).collect();
        let counted = items.iter().any(|c| c.priority != Priority::Safety);
        if counted {
            self.counted.push_back(now);
        }
        let announcement = items
            .iter()
            .map(|c| c.announcement.clone())
            .reduce(Announcement::then)
            .unwrap_or_default();
        Some(Spoken {
            t: now,
            items,
            announcement,
            counted,
            over_budget: counted && over,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;
    use skimap_voice::Event;

    fn cand(t: f64, p: Priority, key: &str) -> Candidate {
        Candidate {
            t,
            priority: p,
            key: key.into(),
            kind: key.into(),
            announcement: Event::NoNetworkExit.announce(),
            expires: None,
        }
    }

    #[test]
    fn groups_orders_and_limits() {
        let mut s = Speech::new(SpeechParams::default());
        s.offer(cand(0.0, Priority::Comfort, "summary"));
        s.offer(cand(2.0, Priority::Direction, "turn"));
        assert!(s.tick(2.0).is_none(), "grouping window");
        let g = s.tick(8.0).expect("said after the window");
        let kinds: Vec<&str> = g.items.iter().map(|c| c.kind.as_str()).collect();
        assert_eq!(kinds, vec!["turn", "summary"], "direction before comfort");
        assert!(g.counted);
        // Safety is said at once and takes the pending candidates with it.
        s.offer(cand(100.0, Priority::Schedule, "late"));
        s.offer(cand(101.0, Priority::Safety, "fog"));
        let g = s.tick(101.0).unwrap();
        let kinds: Vec<&str> = g.items.iter().map(|c| c.kind.as_str()).collect();
        assert_eq!(kinds, vec!["fog", "late"]);
        // A pause holds everything but safety.
        s.set_paused(true);
        s.offer(cand(200.0, Priority::Direction, "x"));
        assert!(s.tick(300.0).is_none());
        s.offer(cand(301.0, Priority::Safety, "night"));
        let g = s.tick(301.0).unwrap();
        assert_eq!(g.items.len(), 1);
        assert!(!g.counted);
        s.set_paused(false);
        assert_eq!(s.tick(302.0).unwrap().items[0].kind, "x");
    }

    #[test]
    fn budget_supersession_and_expiry() {
        let p = SpeechParams {
            max_per_hour: 2,
            ..SpeechParams::default()
        };
        let mut s = Speech::new(p);
        for k in 0..5 {
            s.offer(cand(
                f64::from(k) * 60.0,
                Priority::Schedule,
                &format!("s{k}"),
            ));
            s.tick(f64::from(k) * 60.0 + 10.0);
        }
        let budget: usize = s
            .dropped()
            .iter()
            .filter(|(_, r)| *r == DropReason::Budget)
            .count();
        assert_eq!(budget, 3);
        // Comfort gets half the budget: 1 of 2, already spent.
        s.offer(cand(4000.0, Priority::Comfort, "c1"));
        assert!(s.tick(4010.0).is_some());
        s.offer(cand(4100.0, Priority::Comfort, "c2"));
        assert!(s.tick(4110.0).is_none());
        // Same key replaces; expiry drops.
        s.offer(cand(9000.0, Priority::Direction, "turn"));
        let mut c = cand(9001.0, Priority::Direction, "turn");
        c.expires = Some(9005.0);
        s.offer(c);
        assert!(s.tick(9010.0).is_none());
        let last: Vec<DropReason> = s.dropped().iter().rev().take(2).map(|(_, r)| *r).collect();
        assert_eq!(last, vec![DropReason::Stale, DropReason::Superseded]);
    }

    fn priority() -> impl Strategy<Value = Priority> {
        prop_oneof![
            Just(Priority::Safety),
            Just(Priority::Direction),
            Just(Priority::Schedule),
            Just(Priority::Comfort),
        ]
    }

    #[derive(Debug, Clone)]
    struct Offer {
        dt: f64,
        priority: Priority,
        key: u8,
        ttl: Option<f64>,
        pause: Option<bool>,
    }

    fn offers() -> impl Strategy<Value = Vec<Offer>> {
        prop::collection::vec(
            (
                0.0f64..400.0,
                priority(),
                0u8..6,
                prop::option::of(0.0f64..60.0),
                prop::option::of(any::<bool>()),
            )
                .prop_map(|(dt, priority, key, ttl, pause)| Offer {
                    dt,
                    priority,
                    key,
                    ttl,
                    pause,
                }),
            0..120,
        )
    }

    fn params() -> impl Strategy<Value = SpeechParams> {
        (
            0u32..20,
            0.0f64..1.0,
            any::<bool>(),
            0.0f64..30.0,
            1usize..5,
        )
            .prop_map(
                |(max_per_hour, comfort_share, direction_exempt, group_s, max_items)| {
                    SpeechParams {
                        max_per_hour,
                        comfort_share,
                        direction_exempt,
                        group_s,
                        max_items,
                    }
                },
            )
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(512))]

        /// No safety candidate is ever lost: each one is said exactly once, at the first
        /// tick at or after its offer, in the order offered; every other candidate is said
        /// or dropped with a reason; nothing is said after its expiry; the hourly budget
        /// holds for everything but exempt directions.
        #[test]
        fn safety_is_never_lost(p in params(), offers in offers()) {
            let mut s = Speech::new(p);
            let mut t = 0.0;
            let mut said: Vec<Spoken> = Vec::new();
            let mut safety_offered: Vec<(String, f64)> = Vec::new();
            let mut n = 0usize;
            let tick = 1.0;
            for o in &offers {
                // Ticks every second up to the offer time.
                let until = t + o.dt;
                while t + tick <= until {
                    t += tick;
                    said.extend(s.tick(t));
                }
                if let Some(pause) = o.pause {
                    s.set_paused(pause);
                }
                let id = format!("{n}");
                n += 1;
                if o.priority == Priority::Safety {
                    safety_offered.push((id.clone(), t));
                }
                s.offer(Candidate {
                    t,
                    priority: o.priority,
                    key: format!("k{}", o.key),
                    kind: id,
                    announcement: Event::NoNetworkExit.announce(),
                    expires: o.ttl.map(|x| t + x),
                });
                said.extend(s.tick(t));
            }
            s.set_paused(false);
            for _ in 0..5000 {
                t += tick;
                said.extend(s.tick(t));
            }
            prop_assert!(s.pending().is_empty());
            // Safety: exactly once, at its offer tick, in order.
            let spoken_safety: Vec<(String, f64)> = said
                .iter()
                .flat_map(|g| g.items.iter().filter(|c| c.priority == Priority::Safety).map(move |c| (c.kind.clone(), g.t)))
                .collect();
            prop_assert_eq!(spoken_safety.len(), safety_offered.len());
            for ((a, ta), (b, tb)) in spoken_safety.iter().zip(&safety_offered) {
                prop_assert_eq!(a, b);
                prop_assert!((ta - tb).abs() < 1e-9, "safety {} offered at {} said at {}", a, tb, ta);
            }
            prop_assert!(s.dropped().iter().all(|(c, _)| c.priority != Priority::Safety));
            // Conservation: every candidate said once or dropped once.
            let total = said.iter().map(|g| g.items.len()).sum::<usize>() + s.dropped().len();
            prop_assert_eq!(total, n);
            // Expiry.
            for g in &said {
                for c in &g.items {
                    if c.priority != Priority::Safety
                        && let Some(e) = c.expires
                    {
                        prop_assert!(g.t <= e + 1e-9);
                    }
                }
            }
            // Budget in every sliding hour (groups that are not over budget).
            let counted: Vec<f64> = said.iter().filter(|g| g.counted && !g.over_budget).map(|g| g.t).collect();
            for (i, t0) in counted.iter().enumerate() {
                let in_hour = counted[i..].iter().take_while(|t| **t < t0 + 3600.0).count();
                prop_assert!(in_hour as u32 <= p.max_per_hour, "{} in an hour, max {}", in_hour, p.max_per_hour);
            }
            if !p.direction_exempt {
                prop_assert!(said.iter().all(|g| !g.over_budget));
            }
        }
    }
}
