//! `<layer>.meta.json`: sources, licences, dates and file descriptions of a layer.
//!
//! The same structure is used by the `snow` crate. It belongs in `skimap-common` once the
//! orchestrator agrees on a shared schema (see the crate README).

use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};

/// A data source of a layer.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SourceMeta {
    /// Name of the dataset.
    pub name: String,
    /// Producer.
    pub producer: String,
    /// Access URL (service endpoint or file).
    pub url: String,
    /// Licence.
    pub licence: String,
    /// Attribution to display.
    pub attribution: String,
    /// Edition, version or time stamp of the data when known.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub edition: Option<String>,
}

/// A file of a layer.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FileMeta {
    /// File name inside the layer directory.
    pub name: String,
    /// What the file contains.
    pub description: String,
    /// Unit of the values.
    pub unit: String,
    /// Raster format (`GeoTIFF Float32, Deflate`, ...).
    pub format: String,
    /// CRS of the file (`EPSG:2154`).
    pub crs: String,
    /// Cell size in CRS units.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cell_size: Option<f64>,
    /// Width × height in cells.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub size_cells: Option<[usize; 2]>,
    /// No-data encoding.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub nodata: Option<String>,
    /// Size on disk, bytes.
    pub bytes: u64,
}

/// Metadata of one layer of one site.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LayerMeta {
    /// Layer name (directory name).
    pub layer: String,
    /// Site identifier.
    pub site: String,
    /// Generation time, RFC 3339 UTC.
    pub generated_at: String,
    /// Date or period the content is valid for.
    pub valid_for: String,
    /// Sources.
    pub sources: Vec<SourceMeta>,
    /// Files.
    pub files: Vec<FileMeta>,
    /// Method and known limitations, in French (displayed to developers, not users).
    pub notes: Vec<String>,
}

impl LayerMeta {
    /// Writes `<dir>/<layer>.meta.json`.
    pub fn write(&self, dir: &Path) -> Result<std::path::PathBuf> {
        let path = dir.join(format!("{}.meta.json", self.layer));
        let json = serde_json::to_vec_pretty(self)?;
        std::fs::write(&path, json).map_err(|e| Error::io(&path, e))?;
        Ok(path)
    }
}

/// Current time as RFC 3339 UTC (seconds).
pub fn now_rfc3339() -> String {
    chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true)
}
