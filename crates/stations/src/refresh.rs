//! Scheduled refresh (H1, H2): which part is due, change journal, source states.
//!
//! The server CronJob calls [`crate::refresh`] (daily is enough): each part only runs
//! when it is due, so a second call the same day changes nothing (idempotent).
//!
//! - Station cards (OSM, OpenSkiMap, SkiRadar): weekly ([`FICHES_EVERY_DAYS`]); points
//!   of interest change slowly and the Overpass instances are shared.
//! - Titles, prices, member lists, day prices: daily during the season (1 November to
//!   30 April), when the issuers publish and change them (pre-sales end mid-November),
//!   weekly otherwise ([`TITRES_EVERY_DAYS`]).
//!
//! Every run compares the new values with the previous files and appends the changes to
//! `journal.jsonl`; `status.json` gives the state of every source (last success, error).
//! A title whose source fails keeps its last values marked `stale`; after
//! [`STALE_MAX_DAYS`] its prices are withdrawn (`perime`) rather than shown wrong.

use std::collections::BTreeMap;
use std::path::Path;

use chrono::{DateTime, NaiveDate, Utc};
use serde::{Deserialize, Serialize};
pub use skimap_refuges::journal::{Change, VOLATILE, append_journal, carry_changed_at, diff};
pub use skimap_refuges::refresh::in_season;

/// Days between two refreshes of the station cards.
pub const FICHES_EVERY_DAYS: i64 = 7;
/// Days between two refreshes of the titles: in season, off season.
pub const TITRES_EVERY_DAYS: (i64, i64) = (1, 7);
/// Days a title read before may still be shown when its source fails.
pub const STALE_MAX_DAYS: i64 = 14;

/// Days between two refreshes of a part on `d`.
pub fn every_days(part: &str, d: NaiveDate) -> i64 {
    match part {
        "titres" => {
            if in_season(d) {
                TITRES_EVERY_DAYS.0
            } else {
                TITRES_EVERY_DAYS.1
            }
        }
        _ => FICHES_EVERY_DAYS,
    }
}

/// State of a part.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct PartState {
    /// Last run (RFC 3339).
    pub last_run: Option<String>,
    /// Last successful run.
    pub last_ok: Option<String>,
    /// Error of the last run.
    pub error: Option<String>,
}

/// State of a source document.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct SourceState {
    /// URL.
    pub url: String,
    /// `ok`, `unreachable`, `broken`, `changed`.
    pub status: String,
    /// Last time it was read correctly.
    pub last_ok: Option<String>,
    /// Error of the last read.
    pub error: Option<String>,
    /// SHA-256 of the last body read.
    pub sha256: Option<String>,
}

/// Persistent state of the refreshes (`refresh_state.json`).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct State {
    /// Parts (`fiches`, `titres`).
    pub parts: BTreeMap<String, PartState>,
    /// Sources by id.
    pub sources: BTreeMap<String, SourceState>,
}

impl State {
    /// Reads the state, or an empty one.
    pub fn read(path: &Path) -> Self {
        std::fs::read(path)
            .ok()
            .and_then(|b| serde_json::from_slice(&b).ok())
            .unwrap_or_default()
    }

    /// Whether `part` is due at `now` (never run, or older than its period).
    pub fn due(&self, part: &str, now: DateTime<Utc>) -> bool {
        let Some(last) = self
            .parts
            .get(part)
            .and_then(|p| p.last_ok.as_deref())
            .and_then(|s| DateTime::parse_from_rfc3339(s).ok())
        else {
            return true;
        };
        // A day is « due » from the same hour: a CronJob a few minutes early still runs.
        let age = now.signed_duration_since(last.with_timezone(&Utc));
        age.num_hours() + 1 >= every_days(part, now.date_naive()) * 24
    }
}

/// Age in days of an RFC 3339 date at `now`.
pub fn age_days(at: &str, now: DateTime<Utc>) -> Option<i64> {
    DateTime::parse_from_rfc3339(at)
        .ok()
        .map(|d| now.signed_duration_since(d.with_timezone(&Utc)).num_days())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(s: &str) -> DateTime<Utc> {
        DateTime::parse_from_rfc3339(s).unwrap().with_timezone(&Utc)
    }

    #[test]
    fn schedule() {
        let mut s = State::default();
        assert!(s.due("titres", at("2026-11-15T03:00:00Z")));
        s.parts.insert(
            "titres".into(),
            PartState {
                last_ok: Some("2026-11-15T03:00:00Z".into()),
                ..Default::default()
            },
        );
        s.parts.insert(
            "fiches".into(),
            PartState {
                last_ok: Some("2026-11-15T03:00:00Z".into()),
                ..Default::default()
            },
        );
        // Same day: nothing due (idempotent); next day: titles in season, not the cards.
        assert!(!s.due("titres", at("2026-11-15T09:00:00Z")));
        assert!(s.due("titres", at("2026-11-16T02:30:00Z")));
        assert!(!s.due("fiches", at("2026-11-16T03:00:00Z")));
        assert!(s.due("fiches", at("2026-11-22T03:00:00Z")));
        // Off season the titles are weekly.
        s.parts.get_mut("titres").unwrap().last_ok = Some("2026-06-01T03:00:00Z".into());
        assert!(!s.due("titres", at("2026-06-03T03:00:00Z")));
        assert!(s.due("titres", at("2026-06-08T03:00:00Z")));
        assert!(in_season(NaiveDate::from_ymd_opt(2027, 1, 10).unwrap()));
        assert!(!in_season(NaiveDate::from_ymd_opt(2027, 5, 1).unwrap()));
    }
}
