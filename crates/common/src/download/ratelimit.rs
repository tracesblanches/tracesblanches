//! Per-source sliding-window rate limiter, persisted in the cache directory so that a
//! daily quota (Skitour 1000/day) survives across runs of the pipeline.
//!
//! Every request attempt (including retries) is recorded. The state file holds the
//! timestamps of the requests still inside the longest window.

use std::collections::VecDeque;
use std::path::PathBuf;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use crate::config::RateLimit;
use crate::error::{Error, Result};

/// Time source, injectable for tests.
pub trait Clock: Send + Sync {
    /// Current wall-clock time.
    fn now(&self) -> SystemTime;
    /// Blocks the current thread for `d`.
    fn sleep(&self, d: Duration);
}

/// The real clock.
#[derive(Debug, Default, Clone, Copy)]
pub struct SystemClock;

impl Clock for SystemClock {
    fn now(&self) -> SystemTime {
        SystemTime::now()
    }
    fn sleep(&self, d: Duration) {
        std::thread::sleep(d);
    }
}

/// Milliseconds since the Unix epoch.
pub(crate) fn unix_ms(t: SystemTime) -> u64 {
    t.duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_millis() as u64)
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct State {
    /// Unix milliseconds of the recorded requests, oldest first.
    requests_ms: Vec<u64>,
}

/// Sliding-window limiter of one source.
#[derive(Debug)]
pub(crate) struct Limiter {
    limits: Vec<RateLimit>,
    history: VecDeque<u64>,
    state_path: Option<PathBuf>,
}

impl Limiter {
    /// Limiter persisted in `state_path` (loaded if it exists), or in memory if `None`.
    pub(crate) fn load(limits: Vec<RateLimit>, state_path: Option<PathBuf>) -> Result<Self> {
        let mut history = VecDeque::new();
        if let Some(p) = &state_path {
            match std::fs::read(p) {
                Ok(bytes) => {
                    let s: State = serde_json::from_slice(&bytes)
                        .map_err(|e| Error::json(p.display().to_string(), e))?;
                    let mut v = s.requests_ms;
                    v.sort_unstable();
                    history.extend(v);
                }
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                Err(e) => return Err(Error::io(p, e)),
            }
        }
        Ok(Self {
            limits,
            history,
            state_path,
        })
    }

    fn longest_window_ms(&self) -> u64 {
        self.limits
            .iter()
            .map(|l| l.per.as_millis() as u64)
            .max()
            .unwrap_or(0)
    }

    /// Time to wait at `now_ms` before one more request is allowed by every limit.
    pub(crate) fn wait_needed(&self, now_ms: u64) -> Duration {
        let mut wait_ms = 0u64;
        for l in &self.limits {
            let per = l.per.as_millis() as u64;
            let max = l.max_requests as usize;
            let in_window = self
                .history
                .iter()
                .rev()
                .take_while(|&&t| t + per > now_ms)
                .count();
            if in_window >= max {
                // The request that must leave the window before the next one is allowed.
                let t = self.history[self.history.len() - max];
                wait_ms = wait_ms.max((t + per).saturating_sub(now_ms));
            }
        }
        Duration::from_millis(wait_ms)
    }

    /// Records a request at `now_ms`, prunes old entries and persists the state.
    pub(crate) fn record(&mut self, now_ms: u64) -> Result<()> {
        self.history.push_back(now_ms);
        // An entry with t + longest <= now is outside every window.
        let longest = self.longest_window_ms();
        while self.history.front().is_some_and(|&t| t + longest <= now_ms) {
            self.history.pop_front();
        }
        if let Some(p) = &self.state_path {
            let state = State {
                requests_ms: self.history.iter().copied().collect(),
            };
            let bytes =
                serde_json::to_vec(&state).map_err(|e| Error::json("rate-limit state", e))?;
            super::cache::write_atomic(p, &bytes)?;
        }
        Ok(())
    }

    /// Waits (with `clock`) until a request is allowed, then records it.
    ///
    /// Fails with [`Error::RateLimited`] without waiting when the required wait exceeds
    /// `max_wait`.
    pub(crate) fn acquire(
        &mut self,
        source: &str,
        clock: &dyn Clock,
        max_wait: Duration,
    ) -> Result<()> {
        loop {
            let now = unix_ms(clock.now());
            let wait = self.wait_needed(now);
            if wait.is_zero() {
                return self.record(now);
            }
            if wait > max_wait {
                return Err(Error::RateLimited {
                    source_name: source.to_owned(),
                    wait,
                });
            }
            tracing::debug!(
                source,
                wait_ms = wait.as_millis() as u64,
                "rate limit: waiting"
            );
            clock.sleep(wait);
        }
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use std::sync::Mutex;

    /// Deterministic clock: `sleep` advances time instantly.
    #[derive(Debug)]
    pub(crate) struct MockClock(pub Mutex<SystemTime>);

    impl MockClock {
        pub(crate) fn at_ms(ms: u64) -> Self {
            Self(Mutex::new(UNIX_EPOCH + Duration::from_millis(ms)))
        }
    }

    impl Clock for MockClock {
        fn now(&self) -> SystemTime {
            *self.0.lock().unwrap()
        }
        fn sleep(&self, d: Duration) {
            *self.0.lock().unwrap() += d;
        }
    }

    fn ms(c: &MockClock) -> u64 {
        unix_ms(c.now())
    }

    #[test]
    fn sliding_window() {
        let clock = MockClock::at_ms(1_000_000);
        let mut l = Limiter::load(vec![RateLimit::new(3, Duration::from_secs(10))], None).unwrap();
        let mut times = Vec::new();
        for _ in 0..7 {
            l.acquire("t", &clock, Duration::from_secs(60)).unwrap();
            times.push(ms(&clock) - 1_000_000);
        }
        // 3 at t=0, then each later one waits for the oldest to leave the 10 s window.
        assert_eq!(times, [0, 0, 0, 10_000, 10_000, 10_000, 20_000]);
    }

    #[test]
    fn several_limits_and_max_wait() {
        let clock = MockClock::at_ms(0);
        let limits = vec![
            RateLimit::new(1, Duration::from_secs(1)),
            RateLimit::per_hour(3),
        ];
        let mut l = Limiter::load(limits, None).unwrap();
        for _ in 0..3 {
            l.acquire("t", &clock, Duration::from_secs(2)).unwrap();
        }
        assert_eq!(ms(&clock), 2_000);
        // The hourly quota is used up: the wait (~1 h) exceeds max_wait, no sleeping.
        let err = l.acquire("t", &clock, Duration::from_secs(2)).unwrap_err();
        match err {
            Error::RateLimited { source_name, wait } => {
                assert_eq!(source_name, "t");
                assert_eq!(wait, Duration::from_millis(3_600_000 - 2_000));
            }
            e => panic!("unexpected {e}"),
        }
        assert_eq!(ms(&clock), 2_000);
    }

    #[test]
    fn persisted_across_instances() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("ratelimit.json");
        let limits = vec![RateLimit::per_day(2)];
        let clock = MockClock::at_ms(5_000);
        {
            let mut l = Limiter::load(limits.clone(), Some(path.clone())).unwrap();
            l.acquire("skitour", &clock, Duration::ZERO).unwrap();
            l.acquire("skitour", &clock, Duration::ZERO).unwrap();
        }
        let mut l = Limiter::load(limits, Some(path)).unwrap();
        assert_eq!(l.wait_needed(ms(&clock)), Duration::from_millis(86_400_000));
        assert!(matches!(
            l.acquire("skitour", &clock, Duration::from_secs(1)),
            Err(Error::RateLimited { .. })
        ));
        // Old entries are pruned once outside the window.
        *clock.0.lock().unwrap() += Duration::from_secs(86_400);
        l.acquire("skitour", &clock, Duration::ZERO).unwrap();
        assert_eq!(l.history.len(), 1);
    }
}
