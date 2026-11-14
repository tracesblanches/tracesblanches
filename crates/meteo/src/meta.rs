//! `<layer>.meta.json` written next to every product of the crate.

use std::path::Path;

use serde::{Deserialize, Serialize};
use skimap_common::Error as CommonError;

use crate::error::Result;

/// Provenance of one source used by a layer.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SourceRef {
    /// Short name of the source.
    pub name: String,
    /// Public URL (never with a key).
    pub url: String,
    /// Licence of the data.
    pub licence: String,
    /// Attribution to display.
    pub attribution: String,
    /// Whether the source may only be used for personal, non-commercial purposes.
    pub personal_use_only: bool,
}

/// Metadata of a layer (`<layer>.meta.json`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LayerMeta {
    /// Layer name (directory name under `data/out/<site>/`).
    pub layer: String,
    /// Site identifier.
    pub site: String,
    /// Generation time, RFC 3339 UTC.
    pub generated_at: String,
    /// Start of validity of the content (RFC 3339 or local ISO date-time), if known.
    pub valid_from: Option<String>,
    /// End of validity of the content, if known.
    pub valid_until: Option<String>,
    /// Sources used.
    pub sources: Vec<SourceRef>,
    /// Files of the layer and what they contain.
    pub files: Vec<String>,
    /// Remarks (limits, missing key, ...).
    pub notes: Vec<String>,
}

impl LayerMeta {
    /// Writes `<dir>/<layer>.meta.json`.
    pub fn write(&self, dir: &Path) -> Result<()> {
        write_json(&dir.join(format!("{}.meta.json", self.layer)), self)
    }
}

/// Current time, RFC 3339 UTC, second precision.
pub fn now_rfc3339() -> String {
    chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true)
}

/// Writes pretty JSON atomically (temporary file then rename).
pub fn write_json<T: Serialize>(path: &Path, value: &T) -> Result<()> {
    let body = serde_json::to_vec_pretty(value)
        .map_err(|e| CommonError::json(path.display().to_string(), e))?;
    write_bytes(path, &body)
}

/// Writes bytes atomically (temporary file then rename).
pub fn write_bytes(path: &Path, body: &[u8]) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| CommonError::io(parent, e))?;
    }
    let tmp = path.with_extension("tmp");
    std::fs::write(&tmp, body).map_err(|e| CommonError::io(&tmp, e))?;
    std::fs::rename(&tmp, path).map_err(|e| CommonError::io(path, e))?;
    Ok(())
}
