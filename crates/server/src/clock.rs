//! Time source: the system clock in production, a hand-moved clock in tests.

use std::sync::Mutex;

use chrono::{DateTime, Duration, Utc};

/// Current time.
pub trait Clock: Send + Sync {
    /// Now, UTC.
    fn now(&self) -> DateTime<Utc>;
}

/// The system clock.
#[derive(Debug, Default, Clone, Copy)]
pub struct SystemClock;

impl Clock for SystemClock {
    fn now(&self) -> DateTime<Utc> {
        Utc::now()
    }
}

/// A clock that only moves when told (tests, simulations).
#[derive(Debug)]
pub struct ManualClock(Mutex<DateTime<Utc>>);

impl ManualClock {
    /// A clock showing `t`.
    pub fn new(t: DateTime<Utc>) -> Self {
        Self(Mutex::new(t))
    }

    /// Moves the clock forward.
    pub fn advance(&self, d: Duration) {
        let mut t = self.0.lock().unwrap_or_else(|p| p.into_inner());
        *t += d;
    }

    /// Sets the time.
    pub fn set(&self, t: DateTime<Utc>) {
        *self.0.lock().unwrap_or_else(|p| p.into_inner()) = t;
    }
}

impl Clock for ManualClock {
    fn now(&self) -> DateTime<Utc> {
        *self.0.lock().unwrap_or_else(|p| p.into_inner())
    }
}
