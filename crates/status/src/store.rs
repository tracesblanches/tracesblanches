//! Timestamped snapshots of a status source, to keep a history and measure how often the
//! source really changes.
//!
//! Layout of one source (`data/out/<site>/status/<source>/`):
//! - `polls.jsonl`: one line per poll ([`PollRecord`]), whether or not the content changed;
//! - `snapshots/<YYYYMMDDTHHMMSSZ>.json`: the normalised content, written only when it
//!   differs from the previous snapshot.

use std::io::Write;
use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use skimap_common::Error as CommonError;

use crate::error::Result;

/// One poll of a source.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PollRecord {
    /// Poll time, RFC 3339 UTC.
    pub polled_at: String,
    /// Where the body came from: `downloaded`, `not_modified` (304) or `cache` (younger than
    /// the source cache lifetime, no request sent).
    pub origin: String,
    /// Whether the content differs from the previous snapshot.
    pub changed: bool,
    /// Snapshot file holding the content seen by this poll (relative to the source
    /// directory).
    pub snapshot: String,
}

/// Snapshot store of one source.
#[derive(Debug, Clone)]
pub struct Store {
    dir: PathBuf,
}

fn io(path: &Path, e: std::io::Error) -> CommonError {
    CommonError::io(path, e)
}

/// File name of a snapshot taken at `t`.
pub fn snapshot_name(t: DateTime<Utc>) -> String {
    format!("snapshots/{}.json", t.format("%Y%m%dT%H%M%SZ"))
}

impl Store {
    /// Store rooted at `dir` (created on first write).
    pub fn new(dir: impl Into<PathBuf>) -> Self {
        Self { dir: dir.into() }
    }

    /// Directory of the store.
    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// All polls, oldest first.
    pub fn polls(&self) -> Result<Vec<PollRecord>> {
        let path = self.dir.join("polls.jsonl");
        if !path.exists() {
            return Ok(Vec::new());
        }
        let body = std::fs::read_to_string(&path).map_err(|e| io(&path, e))?;
        body.lines()
            .filter(|l| !l.trim().is_empty())
            .map(|l| {
                serde_json::from_str(l)
                    .map_err(|e| CommonError::json(path.display().to_string(), e).into())
            })
            .collect()
    }

    /// Reads a snapshot by its relative name.
    pub fn read_snapshot(&self, name: &str) -> Result<Value> {
        let path = self.dir.join(name);
        let body = std::fs::read(&path).map_err(|e| io(&path, e))?;
        Ok(serde_json::from_slice(&body)
            .map_err(|e| CommonError::json(path.display().to_string(), e))?)
    }

    /// Distinct snapshots in poll order: (first poll time, name, content).
    pub fn snapshots(&self) -> Result<Vec<(String, String, Value)>> {
        let mut out: Vec<(String, String, Value)> = Vec::new();
        for p in self.polls()? {
            if out.last().is_some_and(|(_, n, _)| *n == p.snapshot) {
                continue;
            }
            let v = self.read_snapshot(&p.snapshot)?;
            out.push((p.polled_at, p.snapshot, v));
        }
        Ok(out)
    }

    /// Records a poll of `content` at `polled_at`; writes a snapshot if it changed.
    pub fn record(
        &self,
        polled_at: DateTime<Utc>,
        origin: &str,
        content: &Value,
    ) -> Result<PollRecord> {
        let last = self.polls()?.pop();
        let unchanged = match &last {
            Some(p) => self.read_snapshot(&p.snapshot).ok().as_ref() == Some(content),
            None => false,
        };
        let snapshot = match (&last, unchanged) {
            (Some(p), true) => p.snapshot.clone(),
            _ => {
                let name = snapshot_name(polled_at);
                let path = self.dir.join(&name);
                if let Some(parent) = path.parent() {
                    std::fs::create_dir_all(parent).map_err(|e| io(parent, e))?;
                }
                let body = serde_json::to_vec_pretty(content)
                    .map_err(|e| CommonError::json(path.display().to_string(), e))?;
                std::fs::write(&path, body).map_err(|e| io(&path, e))?;
                name
            }
        };
        let rec = PollRecord {
            polled_at: polled_at.to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
            origin: origin.to_owned(),
            changed: !unchanged,
            snapshot,
        };
        std::fs::create_dir_all(&self.dir).map_err(|e| io(&self.dir, e))?;
        let path = self.dir.join("polls.jsonl");
        let mut f = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
            .map_err(|e| io(&path, e))?;
        let line = serde_json::to_string(&rec)
            .map_err(|e| CommonError::json(path.display().to_string(), e))?;
        writeln!(f, "{line}").map_err(|e| io(&path, e))?;
        Ok(rec)
    }
}

/// When a published stamp (the source's own "updated" date) was first seen.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StampSeen {
    /// Stamp as published.
    pub stamp: String,
    /// First poll that saw it, RFC 3339 UTC.
    pub first_seen: String,
}

/// Observed update frequency of a source, from its polls.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Frequency {
    /// Number of polls.
    pub polls: usize,
    /// First poll.
    pub first_poll: Option<String>,
    /// Last poll.
    pub last_poll: Option<String>,
    /// Span between the first and last poll, hours.
    pub span_hours: f64,
    /// Number of distinct contents.
    pub distinct_contents: usize,
    /// Poll times at which the content changed (after the first).
    pub changes: Vec<String>,
    /// Mean interval between content changes, hours (None with fewer than two changes).
    pub mean_hours_between_changes: Option<f64>,
    /// Published stamps of the source in order of first sight.
    pub stamps: Vec<StampSeen>,
}

fn parse_t(s: &str) -> Option<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(s)
        .ok()
        .map(|d| d.with_timezone(&Utc))
}

/// Computes the observed frequency; `stamps_of` extracts the published stamps of one
/// snapshot content.
pub fn frequency(store: &Store, stamps_of: impl Fn(&Value) -> Vec<String>) -> Result<Frequency> {
    let polls = store.polls()?;
    let snaps = store.snapshots()?;
    let first = polls.first().map(|p| p.polled_at.clone());
    let last = polls.last().map(|p| p.polled_at.clone());
    let span_hours = match (
        first.as_deref().and_then(parse_t),
        last.as_deref().and_then(parse_t),
    ) {
        (Some(a), Some(b)) => (b - a).num_seconds() as f64 / 3600.0,
        _ => 0.0,
    };
    let changes: Vec<String> = snaps.iter().skip(1).map(|(t, _, _)| t.clone()).collect();
    let times: Vec<DateTime<Utc>> = changes.iter().filter_map(|t| parse_t(t)).collect();
    let mean = (times.len() >= 2).then(|| {
        (times[times.len() - 1] - times[0]).num_seconds() as f64 / 3600.0 / (times.len() - 1) as f64
    });
    let mut stamps: Vec<StampSeen> = Vec::new();
    for (t, _, v) in &snaps {
        for s in stamps_of(v) {
            if !stamps.iter().any(|x| x.stamp == s) {
                stamps.push(StampSeen {
                    stamp: s,
                    first_seen: t.clone(),
                });
            }
        }
    }
    Ok(Frequency {
        polls: polls.len(),
        first_poll: first,
        last_poll: last,
        span_hours,
        distinct_contents: snaps.len(),
        changes,
        mean_hours_between_changes: mean,
        stamps,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn snapshots_only_on_change() {
        let dir = tempfile::tempdir().unwrap();
        let s = Store::new(dir.path().join("enj"));
        let t0 = DateTime::parse_from_rfc3339("2027-01-10T08:00:00Z")
            .unwrap()
            .to_utc();
        let a = json!({"updated": "A", "open": 1});
        let b = json!({"updated": "B", "open": 2});
        assert!(s.record(t0, "downloaded", &a).unwrap().changed);
        let r = s
            .record(t0 + chrono::Duration::minutes(15), "not_modified", &a)
            .unwrap();
        assert!(!r.changed);
        assert_eq!(r.snapshot, "snapshots/20270110T080000Z.json");
        assert!(
            s.record(t0 + chrono::Duration::hours(2), "downloaded", &b)
                .unwrap()
                .changed
        );
        assert!(
            s.record(t0 + chrono::Duration::hours(6), "downloaded", &a)
                .unwrap()
                .changed
        );
        assert_eq!(s.polls().unwrap().len(), 4);
        let snaps = s.snapshots().unwrap();
        assert_eq!(snaps.len(), 3);
        let f = frequency(&s, |v| vec![v["updated"].as_str().unwrap().to_owned()]).unwrap();
        assert_eq!(f.polls, 4);
        assert_eq!(f.distinct_contents, 3);
        assert_eq!(f.changes.len(), 2);
        assert_eq!(f.span_hours, 6.0);
        assert_eq!(f.mean_hours_between_changes, Some(4.0));
        assert_eq!(f.stamps.len(), 2, "A, B");
        assert_eq!(f.stamps[1].first_seen, "2027-01-10T10:00:00Z");
    }
}
