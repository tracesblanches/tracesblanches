//! Scheduled refresh of the hut layer of a site (H1): the server CronJob calls
//! [`refresh`] every day; the layer is rebuilt when due, daily in the nordic season
//! (1 November to 30 April: wardening periods, closures and winter rooms change), weekly
//! otherwise. Changes go to `journal.jsonl`, the state of the run to `status.json`; when
//! a source fails, the previous files are kept and the failure is reported.

use std::collections::BTreeMap;
use std::path::Path;

use chrono::{DateTime, Datelike, NaiveDate, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use skimap_common::download::Downloader;
use skimap_common::layout::DataLayout;
use skimap_common::output::write_json;
use skimap_common::site::Site;

use crate::journal;
use crate::{FICHES_FILE, LAYER, Result};

/// Days between two refreshes: in season, off season.
pub const EVERY_DAYS: (i64, i64) = (1, 7);

/// Whether `d` is in the nordic season (1 November to 30 April).
pub fn in_season(d: NaiveDate) -> bool {
    d.month() >= 11 || d.month() <= 4
}

/// State of the refreshes of a site (`refresh_state.json`).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct State {
    /// Last run (RFC 3339).
    pub last_run: Option<String>,
    /// Last successful run.
    pub last_ok: Option<String>,
    /// Error of the last run.
    pub error: Option<String>,
}

impl State {
    /// Whether a refresh is due at `now`.
    pub fn due(&self, now: DateTime<Utc>) -> bool {
        let Some(last) = self
            .last_ok
            .as_deref()
            .and_then(|s| DateTime::parse_from_rfc3339(s).ok())
        else {
            return true;
        };
        let every = if in_season(now.date_naive()) {
            EVERY_DAYS.0
        } else {
            EVERY_DAYS.1
        };
        now.signed_duration_since(last.with_timezone(&Utc))
            .num_hours()
            + 1
            >= every * 24
    }
}

/// Result of [`refresh`].
#[derive(Debug, Clone)]
pub struct RefreshReport {
    /// Whether the layer was rebuilt.
    pub ran: bool,
    /// Changes written to the journal.
    pub changes: usize,
    /// Error of the sources, if the rebuild failed (previous files kept).
    pub error: Option<String>,
    /// Figures of the build, if it ran.
    pub stats: Value,
}

fn cards(path: &Path) -> BTreeMap<String, Value> {
    std::fs::read(path)
        .ok()
        .and_then(|b| serde_json::from_slice::<Value>(&b).ok())
        .and_then(|v| v["features"].as_array().cloned())
        .unwrap_or_default()
        .into_iter()
        .filter_map(|f| {
            let p = f.get("properties")?.clone();
            Some((p.get("id")?.as_str()?.to_owned(), p))
        })
        .collect()
}

/// Rebuilds the hut layer of `site` when due (always with `force`), journals the changes
/// of the cards and writes the state. Idempotent: a second call the same day does nothing
/// but rewrite `status.json`.
pub fn refresh(
    site: &Site,
    layout: &DataLayout,
    dl: &Downloader,
    now: DateTime<Utc>,
    force: bool,
) -> Result<RefreshReport> {
    let dir = layout.layer_dir(site, LAYER)?;
    let at = now.to_rfc3339_opts(chrono::SecondsFormat::Secs, true);
    let state_path = dir.join("refresh_state.json");
    let mut state: State = std::fs::read(&state_path)
        .ok()
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or_default();
    let cards_path = dir.join(FICHES_FILE);
    let mut report = RefreshReport {
        ran: false,
        changes: 0,
        error: None,
        stats: Value::Null,
    };
    if force || state.due(now) || !cards_path.exists() {
        report.ran = true;
        state.last_run = Some(at.clone());
        let old = cards(&cards_path);
        match crate::build(site, layout, dl) {
            Ok(out) => {
                let mut new = cards(&cards_path);
                let changes = journal::diff(&old, &new, "refuges", &at);
                journal::carry_changed_at(&old, &mut new, &at);
                // Write the cards again with their change dates.
                let mut v: Value = serde_json::from_slice(
                    &std::fs::read(&cards_path)
                        .map_err(|e| skimap_common::Error::io(&cards_path, e))?,
                )
                .map_err(|e| crate::RefugesError::format(LAYER, e.to_string()))?;
                if let Some(fs) = v["features"].as_array_mut() {
                    for f in fs {
                        let id = f["properties"]["id"]
                            .as_str()
                            .unwrap_or_default()
                            .to_owned();
                        if let Some(c) = new.get(&id) {
                            f["properties"]["changed_at"] = c["changed_at"].clone();
                        }
                    }
                }
                let feats = v["features"].as_array().cloned().unwrap_or_default();
                skimap_common::output::write_geojson(&cards_path, &feats)?;
                journal::append_journal(&dir.join("journal.jsonl"), &changes)
                    .map_err(|e| skimap_common::Error::io(dir.join("journal.jsonl"), e))?;
                report.changes = changes.len();
                report.stats = out.stats;
                state.last_ok = Some(at.clone());
                state.error = None;
            }
            Err(e) => {
                if !cards_path.exists() {
                    return Err(e);
                }
                tracing::warn!(site = site.id, error = %e, "refuges not refreshed, previous files kept");
                let c = journal::Change {
                    at: at.clone(),
                    part: "sources".into(),
                    id: LAYER.into(),
                    change: "unreachable".into(),
                    detail: vec![e.to_string()],
                };
                journal::append_journal(&dir.join("journal.jsonl"), &[c])
                    .map_err(|e| skimap_common::Error::io(dir.join("journal.jsonl"), e))?;
                state.error = Some(e.to_string());
                report.error = Some(e.to_string());
            }
        }
    }
    write_json(&state_path, &state)?;
    let stale_days = state
        .last_ok
        .as_deref()
        .and_then(|s| DateTime::parse_from_rfc3339(s).ok())
        .map(|d| now.signed_duration_since(d.with_timezone(&Utc)).num_days());
    write_json(
        &dir.join("status.json"),
        &json!({
            "checked_at": at,
            "last_run": state.last_run,
            "last_ok": state.last_ok,
            "error": state.error,
            "age_days": stale_days,
            "policy": {"every_days_in_season": EVERY_DAYS.0, "every_days_off_season": EVERY_DAYS.1},
        }),
    )?;
    Ok(report)
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
        assert!(s.due(at("2026-12-01T03:00:00Z")));
        s.last_ok = Some("2026-12-01T03:00:00Z".into());
        assert!(!s.due(at("2026-12-01T20:00:00Z")));
        assert!(s.due(at("2026-12-02T02:30:00Z")), "daily in season");
        s.last_ok = Some("2026-07-01T03:00:00Z".into());
        assert!(!s.due(at("2026-07-03T03:00:00Z")));
        assert!(s.due(at("2026-07-08T03:00:00Z")), "weekly off season");
    }
}
