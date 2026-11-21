//! Rules of the reports (V9 #72, V10 #73, V11 #74), pure functions of explicit times so
//! that they are tested with a simulated clock and read the same on the server and in the
//! documentation (`docs/regles.md`).
//!
//! # The rules
//!
//! 1. A report is one of six kinds ([`Kind`]), a position and the time it was seen. No
//!    free text: nobody reviews what the server would publish.
//! 2. It lives [`Kind::lifetime`] from the time it was seen: 24 h for the state of the
//!    snow (groomed, ice, wind-blown snow), 48 h for a track made off-piste, 3 days for a
//!    piste closed on the ground, 7 days for an obstacle.
//! 3. A confirmation (« toujours là ») by another device at time `t` moves the expiry to
//!    `t + lifetime` if that is later, never beyond `seen + 4 × lifetime`
//!    ([`MAX_LIFETIMES`]).
//! 4. A denial (« plus là ») weighs against it: the report is refuted, and hidden, once the
//!    weight against reaches the weight for plus [`REFUTE_MARGIN`]. The weight for is the
//!    author's plus each confirmer's; every vote weighs the voter's reputation, clamped to
//!    [[`MIN_WEIGHT`], [`MAX_WEIGHT`]]. A lone report therefore needs two people saying
//!    « plus là » (or one trusted).
//! 5. One vote per device and report (a new vote replaces the old one); nobody votes on
//!    their own report.
//! 6. Reputation of a device starts at 1. Each confirmation of one of its reports by
//!    someone else gives +0.1 (at most 2), each refutation of one of its reports −0.3 (at
//!    least 0). Below [`HOLD_BELOW`], its new reports are held: not shown until another
//!    device confirms them.
//! 7. The phone queues reports made without network and sends them with the time they were
//!    seen: accepted up to [`MAX_QUEUE_AGE_H`] hours old and [`MAX_CLOCK_SKEW_S`] seconds in
//!    the future; a report already expired when it arrives is dropped (410), not stored.
//! 8. Expired or refuted reports, their votes and photos are deleted by the hourly purge;
//!    a photo is shown only once another device has confirmed the report, and one flag
//!    deletes it.

use jiff::{SignedDuration, Timestamp};
use serde::{Deserialize, Serialize};

/// Kinds of report.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    /// « Trace faite » : a track opened off-piste.
    Track,
    /// « Piste damée » : groomed today.
    Groomed,
    /// « Glace » : icy.
    Ice,
    /// « Neige soufflée » : wind-blown snow, drifts.
    WindBlown,
    /// « Obstacle » : fallen tree, rocks, water.
    Obstacle,
    /// « Piste fermée sur le terrain » : closed on the ground (barrier, sign).
    Closed,
}

impl Kind {
    /// All kinds, in the order of the buttons of the app.
    pub const ALL: [Kind; 6] = [
        Kind::Track,
        Kind::Groomed,
        Kind::Ice,
        Kind::WindBlown,
        Kind::Obstacle,
        Kind::Closed,
    ];

    /// Identifier on the wire and in the database.
    pub fn id(self) -> &'static str {
        match self {
            Kind::Track => "track",
            Kind::Groomed => "groomed",
            Kind::Ice => "ice",
            Kind::WindBlown => "wind_blown",
            Kind::Obstacle => "obstacle",
            Kind::Closed => "closed",
        }
    }

    /// The kind of an identifier.
    pub fn parse(s: &str) -> Option<Kind> {
        Kind::ALL.into_iter().find(|k| k.id() == s)
    }

    /// How long a report lives without confirmation, from the time it was seen.
    pub fn lifetime(self) -> SignedDuration {
        match self {
            Kind::Groomed | Kind::Ice | Kind::WindBlown => SignedDuration::from_hours(24),
            Kind::Track => SignedDuration::from_hours(48),
            Kind::Closed => SignedDuration::from_hours(24 * 3),
            Kind::Obstacle => SignedDuration::from_hours(24 * 7),
        }
    }

    /// French label (documentation, logs).
    pub fn fr(self) -> &'static str {
        match self {
            Kind::Track => "trace faite",
            Kind::Groomed => "piste damée",
            Kind::Ice => "glace",
            Kind::WindBlown => "neige soufflée",
            Kind::Obstacle => "obstacle",
            Kind::Closed => "piste fermée sur le terrain",
        }
    }
}

/// A confirmation never pushes the expiry beyond `seen + MAX_LIFETIMES × lifetime`.
pub const MAX_LIFETIMES: i32 = 4;
/// Refuted when the weight against reaches the weight for plus this margin.
pub const REFUTE_MARGIN: f64 = 0.5;
/// Smallest weight of a vote (a device with a poor reputation still counts a little).
pub const MIN_WEIGHT: f64 = 0.5;
/// Largest weight of a vote.
pub const MAX_WEIGHT: f64 = 2.0;
/// Reputation of a new device.
pub const START_REPUTATION: f64 = 1.0;
/// Gain of the author per confirmation by someone else.
pub const CONFIRM_GAIN: f64 = 0.1;
/// Loss of the author per refuted report.
pub const REFUTE_LOSS: f64 = 0.3;
/// Below this reputation, new reports are held until confirmed.
pub const HOLD_BELOW: f64 = 0.5;
/// Oldest observation accepted (offline queue), hours.
pub const MAX_QUEUE_AGE_H: i64 = 72;
/// Clock skew tolerated for an observation in the future, seconds.
pub const MAX_CLOCK_SKEW_S: i64 = 300;

/// A vote on a report.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Vote {
    /// « Toujours là ».
    Confirm,
    /// « Plus là ».
    Deny,
}

/// A vote with its time and the voter's reputation when it was cast.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Cast {
    /// Confirm or deny.
    pub vote: Vote,
    /// When.
    pub at: Timestamp,
    /// Reputation of the voter.
    pub reputation: f64,
}

/// What the rules say of a report now.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum State {
    /// Shown.
    Active,
    /// Not shown until confirmed (author below [`HOLD_BELOW`]).
    Held,
    /// Past its expiry.
    Expired,
    /// Refuted by the denials.
    Refuted,
}

/// The evaluation of a report.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Evaluation {
    /// State now.
    pub state: State,
    /// Expiry after the confirmations.
    pub expires_at: Timestamp,
    /// Confirmations by others.
    pub confirmations: u32,
    /// Denials.
    pub denials: u32,
    /// Weight for (author and confirmers).
    pub support: f64,
    /// Weight against.
    pub against: f64,
}

/// Weight of a vote of a device with this reputation.
pub fn weight(reputation: f64) -> f64 {
    reputation.clamp(MIN_WEIGHT, MAX_WEIGHT)
}

/// Evaluates a report of `kind` seen at `seen`, by an author of reputation
/// `author_reputation` when it was made, with these votes, at `now`.
pub fn evaluate(
    kind: Kind,
    seen: Timestamp,
    author_reputation: f64,
    votes: &[Cast],
    now: Timestamp,
) -> Evaluation {
    let life = kind.lifetime();
    let cap = seen + life * MAX_LIFETIMES;
    let mut expires_at = seen + life;
    let mut support = weight(author_reputation);
    let mut against = 0.0;
    let (mut confirmations, mut denials) = (0u32, 0u32);
    for v in votes.iter().filter(|v| v.at <= now) {
        match v.vote {
            Vote::Confirm => {
                confirmations += 1;
                support += weight(v.reputation);
                expires_at = expires_at.max((v.at + life).min(cap));
            }
            Vote::Deny => {
                denials += 1;
                against += weight(v.reputation);
            }
        }
    }
    let state = if now >= expires_at {
        State::Expired
    } else if against >= support + REFUTE_MARGIN {
        State::Refuted
    } else if author_reputation < HOLD_BELOW && confirmations == 0 {
        State::Held
    } else {
        State::Active
    };
    Evaluation {
        state,
        expires_at,
        confirmations,
        denials,
        support,
        against,
    }
}

/// Why an observation time is refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TimeRefusal {
    /// More than [`MAX_CLOCK_SKEW_S`] in the future.
    Future,
    /// Older than [`MAX_QUEUE_AGE_H`].
    TooOld,
    /// Already past its lifetime: nothing to show.
    Expired,
}

/// Whether a report of `kind` seen at `seen` and received at `now` is accepted.
pub fn accept_observed(kind: Kind, seen: Timestamp, now: Timestamp) -> Result<(), TimeRefusal> {
    if seen > now + SignedDuration::from_secs(MAX_CLOCK_SKEW_S) {
        Err(TimeRefusal::Future)
    } else if seen < now - SignedDuration::from_hours(MAX_QUEUE_AGE_H) {
        Err(TimeRefusal::TooOld)
    } else if seen + kind.lifetime() <= now {
        Err(TimeRefusal::Expired)
    } else {
        Ok(())
    }
}

/// Reputation of the author after one more confirmation of one of its reports.
pub fn after_confirmation(reputation: f64) -> f64 {
    (reputation + CONFIRM_GAIN).min(MAX_WEIGHT)
}

/// Reputation of the author after one of its reports was refuted.
pub fn after_refutation(reputation: f64) -> f64 {
    (reputation - REFUTE_LOSS).max(0.0)
}

/// Whether a position is in the area served (metropolitan France and its borders, Corsica).
pub fn in_area(lon: f64, lat: f64) -> bool {
    lon.is_finite()
        && lat.is_finite()
        && (-5.8..=10.0).contains(&lon)
        && (41.0..=51.5).contains(&lat)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn t0() -> Timestamp {
        "2026-01-10T09:00:00Z".parse().unwrap()
    }

    fn cast(vote: Vote, h: i64) -> Cast {
        Cast {
            vote,
            at: t0() + SignedDuration::from_hours(h),
            reputation: 1.0,
        }
    }

    #[test]
    fn snow_report_expires_after_its_lifetime_unless_confirmed() {
        // Ticket V10: a snow report expires after its lifetime unless confirmed.
        let e = evaluate(
            Kind::Ice,
            t0(),
            1.0,
            &[],
            t0() + SignedDuration::from_hours(23),
        );
        assert_eq!(e.state, State::Active);
        let e = evaluate(
            Kind::Ice,
            t0(),
            1.0,
            &[],
            t0() + SignedDuration::from_hours(24),
        );
        assert_eq!(e.state, State::Expired);
        // Confirmed at 20 h: lives until 44 h.
        let votes = [cast(Vote::Confirm, 20)];
        let e = evaluate(
            Kind::Ice,
            t0(),
            1.0,
            &votes,
            t0() + SignedDuration::from_hours(30),
        );
        assert_eq!((e.state, e.confirmations), (State::Active, 1));
        assert_eq!(e.expires_at, t0() + SignedDuration::from_hours(44));
        let e = evaluate(
            Kind::Ice,
            t0(),
            1.0,
            &votes,
            t0() + SignedDuration::from_hours(44),
        );
        assert_eq!(e.state, State::Expired);
    }

    #[test]
    fn confirmations_never_beyond_four_lifetimes() {
        let votes: Vec<Cast> = (0..10).map(|i| cast(Vote::Confirm, 20 * i)).collect();
        let e = evaluate(
            Kind::Groomed,
            t0(),
            1.0,
            &votes,
            t0() + SignedDuration::from_hours(95),
        );
        assert_eq!(e.expires_at, t0() + SignedDuration::from_hours(96));
        assert_eq!(e.state, State::Active);
        let e = evaluate(
            Kind::Groomed,
            t0(),
            1.0,
            &votes,
            t0() + SignedDuration::from_hours(96),
        );
        assert_eq!(e.state, State::Expired);
    }

    #[test]
    fn lifetimes_by_kind() {
        let h = |k: Kind| k.lifetime().as_hours();
        assert_eq!(
            Kind::ALL.map(h),
            [48, 24, 24, 24, 24 * 7, 24 * 3],
            "track, groomed, ice, wind, obstacle, closed"
        );
        for k in Kind::ALL {
            assert_eq!(Kind::parse(k.id()), Some(k));
        }
        assert_eq!(Kind::parse("free text"), None);
    }

    #[test]
    fn two_denials_refute_a_lone_report_one_does_not() {
        let one = [cast(Vote::Deny, 1)];
        let e = evaluate(
            Kind::Obstacle,
            t0(),
            1.0,
            &one,
            t0() + SignedDuration::from_hours(2),
        );
        assert_eq!(e.state, State::Active);
        let two = [cast(Vote::Deny, 1), cast(Vote::Deny, 2)];
        let e = evaluate(
            Kind::Obstacle,
            t0(),
            1.0,
            &two,
            t0() + SignedDuration::from_hours(3),
        );
        assert_eq!(e.state, State::Refuted);
        // Confirmed once: three denials needed.
        let mixed = [
            cast(Vote::Confirm, 0),
            cast(Vote::Deny, 1),
            cast(Vote::Deny, 2),
        ];
        let e = evaluate(
            Kind::Obstacle,
            t0(),
            1.0,
            &mixed,
            t0() + SignedDuration::from_hours(3),
        );
        assert_eq!((e.state, e.confirmations, e.denials), (State::Active, 1, 2));
        // A trusted denier (reputation 2) refutes a lone report alone... but a lone
        // report of a trusted author resists.
        let trusted = [Cast {
            reputation: 2.0,
            ..cast(Vote::Deny, 1)
        }];
        let e = evaluate(
            Kind::Ice,
            t0(),
            1.0,
            &trusted,
            t0() + SignedDuration::from_hours(2),
        );
        assert_eq!(e.state, State::Refuted);
        let e = evaluate(
            Kind::Ice,
            t0(),
            2.0,
            &trusted,
            t0() + SignedDuration::from_hours(2),
        );
        assert_eq!(e.state, State::Active);
    }

    #[test]
    fn poor_reputation_is_held_until_confirmed() {
        let e = evaluate(
            Kind::Ice,
            t0(),
            0.4,
            &[],
            t0() + SignedDuration::from_hours(1),
        );
        assert_eq!(e.state, State::Held);
        let e = evaluate(
            Kind::Ice,
            t0(),
            0.4,
            &[cast(Vote::Confirm, 1)],
            t0() + SignedDuration::from_hours(2),
        );
        assert_eq!(e.state, State::Active);
        // Reputation: 1 → two refutations → 0.4 (held), confirmations bring it back.
        let r = after_refutation(after_refutation(START_REPUTATION));
        assert!((r - 0.4).abs() < 1e-9 && r < HOLD_BELOW);
        let r = (0..2).fold(r, |r, _| after_confirmation(r));
        assert!(r >= HOLD_BELOW);
        assert_eq!((0..30).fold(1.0, |r, _| after_confirmation(r)), MAX_WEIGHT);
        assert_eq!((0..10).fold(1.0, |r, _| after_refutation(r)), 0.0);
    }

    #[test]
    fn votes_in_the_future_do_not_count() {
        let e = evaluate(
            Kind::Ice,
            t0(),
            1.0,
            &[cast(Vote::Confirm, 30)],
            t0() + SignedDuration::from_hours(25),
        );
        assert_eq!(e.state, State::Expired);
    }

    #[test]
    fn observation_times_of_the_offline_queue() {
        let now = t0();
        assert_eq!(
            accept_observed(Kind::Track, now - SignedDuration::from_hours(20), now),
            Ok(())
        );
        assert_eq!(
            accept_observed(Kind::Ice, now - SignedDuration::from_hours(25), now),
            Err(TimeRefusal::Expired)
        );
        assert_eq!(
            accept_observed(Kind::Obstacle, now - SignedDuration::from_hours(73), now),
            Err(TimeRefusal::TooOld)
        );
        assert_eq!(
            accept_observed(Kind::Obstacle, now + SignedDuration::from_mins(4), now),
            Ok(())
        );
        assert_eq!(
            accept_observed(Kind::Obstacle, now + SignedDuration::from_mins(6), now),
            Err(TimeRefusal::Future)
        );
    }

    #[test]
    fn area() {
        assert!(in_area(6.77, 44.85));
        assert!(in_area(9.2, 42.1));
        assert!(!in_area(-73.9, 40.7));
        assert!(!in_area(f64::NAN, 45.0));
    }
}
