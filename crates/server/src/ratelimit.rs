//! In-memory sliding-window limits per key (client address, or a global key).

use std::collections::{HashMap, VecDeque};
use std::sync::Mutex;

use jiff::{SignedDuration, Timestamp};

/// Counts events per key over a window.
#[derive(Debug, Default)]
pub struct Limiter {
    hits: Mutex<HashMap<String, VecDeque<Timestamp>>>,
}

impl Limiter {
    /// Records an event for `key` at `now` if fewer than `max` happened in the last
    /// `window`; returns whether it was allowed.
    pub fn allow(&self, key: &str, max: usize, window: SignedDuration, now: Timestamp) -> bool {
        let mut hits = self.hits.lock().unwrap_or_else(|p| p.into_inner());
        if hits.len() > 10_000 {
            hits.retain(|_, q| q.back().is_some_and(|t| now - *t < window));
        }
        let q = hits.entry(key.to_owned()).or_default();
        while q.front().is_some_and(|t| now - *t >= window) {
            q.pop_front();
        }
        if q.len() >= max {
            return false;
        }
        q.push_back(now);
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn window_slides() {
        let l = Limiter::default();
        let t0 = Timestamp::now();
        let h = SignedDuration::from_hours(1);
        assert!(l.allow("a", 2, h, t0));
        assert!(l.allow("a", 2, h, t0));
        assert!(!l.allow("a", 2, h, t0 + SignedDuration::from_mins(59)));
        assert!(l.allow("b", 2, h, t0));
        assert!(l.allow("a", 2, h, t0 + h));
    }
}
