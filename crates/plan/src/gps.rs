//! GPS modes decided by the core (E1 #125, report 09 « batterie », section 5): the client
//! only applies the request. Pure function of the state of the outing, tested here and in
//! the scenarios (Q9).
//!
//! | Mode | Entry | Request |
//! |---|---|---|
//! | idle | before the start, after the end | 1 s (the start is dated by the first movement) |
//! | pocket | default: D > [`GpsParams::approach_m`] | fix every [`GpsParams::pocket_interval_s`], batch delay min(60 s, (D − 300 m) / v_max − 10 s), ≥ 0 |
//! | approach | D ≤ 300 m, or D / speed ≤ [`GpsParams::approach_s`] | 1 s, no batching |
//! | deviation | more than [`GpsParams::deviation_m`] from the route (distance smoothed over 10 s by the tracker, against single noisy fixes) | 1 s, no batching |
//! | interaction | screen on and interactive | 1 s, no batching |
//! | pause | stopped [`crate::session::SessionParams::pause_s`] (90 s) | GNSS off (passive), significant motion armed |
//! | refuge | paused within 50 m of a refuge for 5 min | GNSS off, significant motion armed |
//!
//! D is the distance along the route to the next decision point (junction said, zone
//! entry, avalanche path). The approach ends when the point is passed by
//! [`GpsParams::passed_m`] and the skier has stayed within the deviation threshold for
//! [`GpsParams::good_s`]. Anti-yoyo: at least [`GpsParams::min_mode_s`] in a mode, except
//! to enter deviation or interaction, or to leave for a pause decided by the session.
//! After a significant motion in a pause, 1 s fixes until the session decides.

use serde::{Deserialize, Serialize};

/// A mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GpsMode {
    /// Before the start or after the end: the client's own settings.
    Idle,
    /// In the pocket on the route.
    Pocket,
    /// Approaching a decision point.
    Approach,
    /// Off the route.
    Deviation,
    /// Screen on.
    Interaction,
    /// Stopped.
    Pause,
    /// Stopped at a refuge.
    Refuge,
    /// Moving again after a pause, until the session says so.
    Waking,
}

/// Settings (report 09, section 5).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct GpsParams {
    /// Approach: distance to the next decision point, metres.
    pub approach_m: f64,
    /// Approach: time to the next decision point, seconds.
    pub approach_s: f64,
    /// Deviation: distance to the route, metres.
    pub deviation_m: f64,
    /// End of the approach: point passed by, metres…
    pub passed_m: f64,
    /// …and on the route for, seconds.
    pub good_s: f64,
    /// Minimum time in a mode, seconds.
    pub min_mode_s: f64,
    /// Pocket fix interval, seconds.
    pub pocket_interval_s: f64,
    /// Largest batch delay, seconds.
    pub max_batch_s: f64,
    /// Highest speed (C5 `max_speed_ms`), m/s.
    pub v_max_ms: f64,
}

impl Default for GpsParams {
    fn default() -> Self {
        Self {
            approach_m: 300.0,
            approach_s: 45.0,
            deviation_m: 25.0,
            passed_m: 50.0,
            good_s: 10.0,
            min_mode_s: 30.0,
            pocket_interval_s: 5.0,
            max_batch_s: 60.0,
            v_max_ms: 7.0,
        }
    }
}

/// What the core knows at a fix.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GpsInput {
    /// Time, seconds.
    pub t: f64,
    /// Outing started and not finished.
    pub running: bool,
    /// Distance along the route to the next decision point, metres (`None`: none left).
    pub d_next: Option<f64>,
    /// Distance along the route since the last decision point passed, metres.
    pub since_point: Option<f64>,
    /// Speed, m/s.
    pub speed_ms: f64,
    /// Distance to the route, metres.
    pub d_route: f64,
    /// Paused (session).
    pub paused: bool,
    /// Paused at a refuge (session).
    pub refuge: bool,
    /// Significant motion during the pause.
    pub waking: bool,
    /// Screen on and interactive.
    pub screen_on: bool,
}

/// What the client applies.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct GpsRequest {
    /// Mode.
    pub mode: GpsMode,
    /// GNSS on (high accuracy); off: passive.
    pub gnss: bool,
    /// Fix interval, seconds.
    pub interval_s: f64,
    /// Batch delay (`setMaxUpdateDelayMillis`), seconds.
    pub max_delay_s: f64,
    /// Arm the significant motion sensor.
    pub significant_motion: bool,
}

/// State carried from one fix to the next.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GpsState {
    /// Current mode.
    pub mode: GpsMode,
    /// Since, seconds.
    pub since: f64,
    /// Within the deviation threshold since, seconds.
    pub good_since: Option<f64>,
}

impl Default for GpsState {
    fn default() -> Self {
        Self {
            mode: GpsMode::Idle,
            since: f64::NEG_INFINITY,
            good_since: None,
        }
    }
}

fn request(mode: GpsMode, d_next: Option<f64>, p: &GpsParams) -> GpsRequest {
    match mode {
        GpsMode::Pocket => {
            let delay = match d_next {
                Some(d) => ((d - p.approach_m) / p.v_max_ms - 10.0).clamp(0.0, p.max_batch_s),
                None => p.max_batch_s,
            };
            GpsRequest {
                mode,
                gnss: true,
                interval_s: p.pocket_interval_s,
                max_delay_s: delay,
                significant_motion: false,
            }
        }
        GpsMode::Pause | GpsMode::Refuge => GpsRequest {
            mode,
            gnss: false,
            interval_s: 0.0,
            max_delay_s: 0.0,
            significant_motion: true,
        },
        // Before the start (the outing starts with the first movement on the route: 1 s
        // fixes to date it) and after the end until the client stops.
        GpsMode::Idle => GpsRequest {
            mode,
            gnss: true,
            interval_s: 1.0,
            max_delay_s: 0.0,
            significant_motion: false,
        },
        _ => GpsRequest {
            mode,
            gnss: true,
            interval_s: 1.0,
            max_delay_s: 0.0,
            significant_motion: false,
        },
    }
}

/// The mode at a fix and the request to apply.
pub fn decide(prev: &GpsState, i: &GpsInput, p: &GpsParams) -> (GpsState, GpsRequest) {
    let good_since = if i.d_route <= p.deviation_m {
        prev.good_since.or(Some(i.t))
    } else {
        None
    };
    let near = i
        .d_next
        .is_some_and(|d| d <= p.approach_m || (i.speed_ms > 0.1 && d / i.speed_ms <= p.approach_s));
    let target = if !i.running {
        GpsMode::Idle
    } else if i.paused {
        if i.waking {
            GpsMode::Waking
        } else if i.refuge {
            GpsMode::Refuge
        } else {
            GpsMode::Pause
        }
    } else if i.d_route > p.deviation_m {
        GpsMode::Deviation
    } else if i.screen_on {
        GpsMode::Interaction
    } else if near {
        GpsMode::Approach
    } else if prev.mode == GpsMode::Approach
        && !(i.since_point.is_some_and(|s| s >= p.passed_m)
            && good_since.is_some_and(|g| i.t - g >= p.good_s))
    {
        // Still leaving the point.
        GpsMode::Approach
    } else {
        GpsMode::Pocket
    };
    let urgent = matches!(
        target,
        GpsMode::Deviation
            | GpsMode::Interaction
            | GpsMode::Idle
            | GpsMode::Pause
            | GpsMode::Refuge
            | GpsMode::Waking
    ) || matches!(
        prev.mode,
        GpsMode::Idle | GpsMode::Pause | GpsMode::Refuge | GpsMode::Waking
    );
    let mode = if target == prev.mode || urgent || i.t - prev.since >= p.min_mode_s {
        target
    } else {
        prev.mode
    };
    let state = GpsState {
        mode,
        since: if mode == prev.mode { prev.since } else { i.t },
        good_since,
    };
    (state, request(mode, i.d_next, p))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn input(t: f64, d_next: Option<f64>, d_route: f64) -> GpsInput {
        GpsInput {
            t,
            running: true,
            d_next,
            since_point: None,
            speed_ms: 3.0,
            d_route,
            paused: false,
            refuge: false,
            waking: false,
            screen_on: false,
        }
    }

    #[test]
    fn pocket_approach_deviation_and_anti_yoyo() {
        let p = GpsParams::default();
        let s0 = GpsState::default();
        // Far from the next point: pocket, batch delay (2000 − 300) / 7 − 10 = 232 → 60 s.
        let (s, r) = decide(&s0, &input(0.0, Some(2000.0), 5.0), &p);
        assert_eq!(
            (s.mode, r.interval_s, r.max_delay_s),
            (GpsMode::Pocket, 5.0, 60.0)
        );
        // 600 m: delay (600 − 300) / 7 − 10 = 32.9 s.
        let (s, r) = decide(&s, &input(40.0, Some(600.0), 5.0), &p);
        assert!((r.max_delay_s - 32.857).abs() < 0.01, "{r:?}");
        // 120 s away at 3 m/s… 360 m: still pocket; 130 m: approach, 1 s, no batch.
        let (s, r) = decide(&s, &input(60.0, Some(130.0), 5.0), &p);
        assert_eq!(
            (s.mode, r.interval_s, r.max_delay_s),
            (GpsMode::Approach, 1.0, 0.0)
        );
        // Point passed by 20 m only: stays in approach.
        let mut i = input(100.0, Some(900.0), 5.0);
        i.since_point = Some(20.0);
        let (s, _) = decide(&s, &i, &p);
        assert_eq!(s.mode, GpsMode::Approach);
        // Passed by 60 m, on the route for 10 s: pocket.
        i.t = 111.0;
        i.since_point = Some(60.0);
        let (s, _) = decide(&s, &i, &p);
        assert_eq!(s.mode, GpsMode::Pocket);
        // Deviation at once (no anti-yoyo), back on the route: approach? no, pocket only
        // after 30 s in deviation.
        let (s, r) = decide(&s, &input(112.0, Some(900.0), 30.0), &p);
        assert_eq!((s.mode, r.interval_s), (GpsMode::Deviation, 1.0));
        let (s2, _) = decide(&s, &input(120.0, Some(900.0), 5.0), &p);
        assert_eq!(s2.mode, GpsMode::Deviation, "anti-yoyo");
        let (s3, _) = decide(&s2, &input(143.0, Some(900.0), 5.0), &p);
        assert_eq!(s3.mode, GpsMode::Pocket);
    }

    #[test]
    fn pause_refuge_waking_and_idle() {
        let p = GpsParams::default();
        let mut i = input(0.0, Some(2000.0), 5.0);
        let (s, _) = decide(&GpsState::default(), &i, &p);
        i.paused = true;
        i.t = 5.0;
        let (s, r) = decide(&s, &i, &p);
        assert_eq!(
            (s.mode, r.gnss, r.significant_motion),
            (GpsMode::Pause, false, true)
        );
        i.refuge = true;
        let (s, _) = decide(&s, &i, &p);
        assert_eq!(s.mode, GpsMode::Refuge);
        i.waking = true;
        let (s, r) = decide(&s, &i, &p);
        assert_eq!((s.mode, r.interval_s, r.gnss), (GpsMode::Waking, 1.0, true));
        i.paused = false;
        i.waking = false;
        let (s, _) = decide(&s, &i, &p);
        assert_eq!(s.mode, GpsMode::Pocket, "after a pause, no anti-yoyo");
        i.running = false;
        assert_eq!(decide(&s, &i, &p).0.mode, GpsMode::Idle);
    }
}
