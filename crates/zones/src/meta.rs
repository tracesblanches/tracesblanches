//! `<layer>.meta.json`: provenance of a layer (sources, licences, dates, properties).
//!
//! Also used by `skimap-coverage` (P12).

use std::path::Path;

use serde::{Deserialize, Serialize};
use skimap_common::Error as CommonError;

use crate::error::Result;

/// One source of a layer.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SourceMeta {
    /// Short name.
    pub name: String,
    /// Public URL (service endpoint or file).
    pub url: String,
    /// Licence as published, or "non vérifiée" when unknown.
    pub license: String,
    /// Attribution to display.
    pub attribution: String,
    /// Date of the data when the source gives it (ISO).
    #[serde(default)]
    pub data_date: Option<String>,
    /// Remark (quirks, limits).
    #[serde(default)]
    pub note: Option<String>,
}

/// One output file of the layer.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FileMeta {
    /// File name, relative to the layer directory.
    pub name: String,
    /// What it holds.
    pub description: String,
}

/// Metadata of a layer.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LayerMeta {
    /// Layer name (directory name).
    pub layer: String,
    /// Site identifier.
    pub site: String,
    /// Generation time, RFC 3339 UTC.
    pub generated_at: String,
    /// Date after which the layer should be regenerated (ISO date), when known.
    #[serde(default)]
    pub valid_until: Option<String>,
    /// How long the layer stays valid, in words.
    pub validity: String,
    /// Coordinate reference system of the files.
    pub crs: String,
    /// Output files.
    pub files: Vec<FileMeta>,
    /// Feature properties: name → meaning.
    pub properties: Vec<(String, String)>,
    /// Sources.
    pub sources: Vec<SourceMeta>,
    /// Limits and caveats.
    pub notes: Vec<String>,
    /// Layer-specific figures.
    pub stats: serde_json::Value,
}

impl LayerMeta {
    /// Writes the metadata as pretty JSON.
    pub fn write(&self, path: &Path) -> Result<()> {
        write_json_pretty(path, self)
    }
}

/// Writes a value as pretty JSON, creating the parent directory.
pub fn write_json_pretty<T: Serialize>(path: &Path, value: &T) -> Result<()> {
    let s = serde_json::to_string_pretty(value)
        .map_err(|e| CommonError::json(path.display().to_string(), e))?;
    write_text(path, &s)
}

/// Writes text, creating the parent directory; the file is replaced atomically.
pub fn write_text(path: &Path, text: &str) -> Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| CommonError::io(dir, e))?;
    }
    let tmp = path.with_extension("tmp");
    std::fs::write(&tmp, text).map_err(|e| CommonError::io(&tmp, e))?;
    std::fs::rename(&tmp, path).map_err(|e| CommonError::io(path, e))?;
    Ok(())
}

/// Current UTC time, RFC 3339, second precision.
pub fn now_rfc3339() -> String {
    chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true)
}
