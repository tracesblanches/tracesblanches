//! Time source: the system clock in production, a hand-moved clock in tests.

use std::sync::Mutex;

use jiff::{SignedDuration, Timestamp};

/// Current time.
pub trait Clock: Send + Sync {
    /// Now, UTC.
    fn now(&self) -> Timestamp;
}

/// The system clock.
#[derive(Debug, Default, Clone, Copy)]
pub struct SystemClock;

impl Clock for SystemClock {
    fn now(&self) -> Timestamp {
        Timestamp::now()
    }
}

/// A clock that only moves when told (tests, simulations).
#[derive(Debug)]
pub struct ManualClock(Mutex<Timestamp>);

impl ManualClock {
    /// A clock showing `t`.
    pub fn new(t: Timestamp) -> Self {
        Self(Mutex::new(t))
    }

    /// Moves the clock forward.
    pub fn advance(&self, d: SignedDuration) {
        let mut t = self.0.lock().unwrap_or_else(|p| p.into_inner());
        *t += d;
    }

    /// Sets the time.
    pub fn set(&self, t: Timestamp) {
        *self.0.lock().unwrap_or_else(|p| p.into_inner()) = t;
    }
}

impl Clock for ManualClock {
    fn now(&self) -> Timestamp {
        *self.0.lock().unwrap_or_else(|p| p.into_inner())
    }
}
