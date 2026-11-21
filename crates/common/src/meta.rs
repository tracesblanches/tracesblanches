//! `<layer>.meta.json`: one metadata schema for every layer of the pipeline.
//!
//! Each layer directory (`data/out/<site>/<layer>/`, or `data/out/france/<layer>/`) holds
//! a `<layer>.meta.json` describing its sources (URL, licence, attribution, personal use),
//! its files (with documented properties, raster details), its validity and its key
//! figures. The fields that only some layers fill are optional and omitted when empty.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::error::{Error, Result};
use crate::output::write_json;
use jiff::Timestamp;

/// Provenance of one source used by a layer.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SourceRef {
    /// Short name of the source (dataset, API endpoint).
    pub name: String,
    /// Producer of the data, when different from the name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub producer: Option<String>,
    /// Public URL (never with a key).
    pub url: String,
    /// Licence of the data as published, or what is known of it.
    pub licence: String,
    /// Attribution to display.
    pub attribution: String,
    /// Whether the source may only be used for personal, non-commercial purposes (behind
    /// `Config::personal_sources`).
    #[serde(default)]
    pub personal_use_only: bool,
    /// Edition, version or date of the data when the source gives it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub edition: Option<String>,
    /// Remark (quirks, limits).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
    /// Download time of the body, RFC 3339 UTC.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fetched_at: Option<String>,
    /// `Last-Modified` of the response.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_modified: Option<String>,
    /// `ETag` of the response.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub etag: Option<String>,
    /// SHA-256 of the body.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sha256: Option<String>,
}

impl SourceRef {
    /// A source with its name, public URL, licence and attribution.
    pub fn new(
        name: impl Into<String>,
        url: impl Into<String>,
        licence: impl Into<String>,
        attribution: impl Into<String>,
    ) -> Self {
        Self {
            name: name.into(),
            producer: None,
            url: url.into(),
            licence: licence.into(),
            attribution: attribution.into(),
            personal_use_only: false,
            edition: None,
            note: None,
            fetched_at: None,
            last_modified: None,
            etag: None,
            sha256: None,
        }
    }

    /// Source description from a downloaded body (URL, date, `ETag`, `Last-Modified`,
    /// SHA-256 from the cache metadata).
    #[cfg(feature = "download")]
    pub fn from_fetched(
        name: impl Into<String>,
        licence: impl Into<String>,
        attribution: impl Into<String>,
        f: &crate::download::Fetched,
    ) -> Self {
        let mut s = Self::new(name, f.meta.url.clone(), licence, attribution);
        s.fetched_at = unix_to_rfc3339(f.meta.fetched_at_unix as i64);
        s.last_modified = f.meta.last_modified.clone();
        s.etag = f.meta.etag.clone();
        s.sha256 = Some(f.meta.sha256.clone());
        s
    }

    /// Marks the source as personal use only.
    pub fn personal_use_only(mut self) -> Self {
        self.personal_use_only = true;
        self
    }

    /// Sets the producer.
    pub fn producer(mut self, producer: impl Into<String>) -> Self {
        self.producer = Some(producer.into());
        self
    }

    /// Sets the edition, version or date of the data.
    pub fn edition(mut self, edition: impl Into<String>) -> Self {
        self.edition = Some(edition.into());
        self
    }

    /// Sets the remark.
    pub fn note(mut self, note: impl Into<String>) -> Self {
        self.note = Some(note.into());
        self
    }
}

/// One file of a layer.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FileMeta {
    /// File name, relative to the layer directory.
    pub name: String,
    /// What the file contains.
    pub description: String,
    /// Licence of the file content, when it differs between files of the layer.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub licence: Option<String>,
    /// Number of features (GeoJSON) or entries.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub count: Option<usize>,
    /// Documented feature properties: name → meaning.
    #[serde(default, skip_serializing_if = "std::collections::BTreeMap::is_empty")]
    pub properties: std::collections::BTreeMap<String, String>,
    /// Unit of the raster values.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unit: Option<String>,
    /// File format (`GeoTIFF Float32, Deflate`, ...).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub format: Option<String>,
    /// CRS of the file (`EPSG:2154`), when it differs from the layer CRS.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub crs: Option<String>,
    /// Raster cell size in CRS units.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cell_size: Option<f64>,
    /// Raster width × height in cells.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub size_cells: Option<[usize; 2]>,
    /// No-data encoding of the raster.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub nodata: Option<String>,
    /// Size on disk, bytes.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bytes: Option<u64>,
}

impl FileMeta {
    /// A file with its name and description.
    pub fn new(name: impl Into<String>, description: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            description: description.into(),
            licence: None,
            count: None,
            properties: Default::default(),
            unit: None,
            format: None,
            crs: None,
            cell_size: None,
            size_cells: None,
            nodata: None,
            bytes: None,
        }
    }

    /// A Float32 GeoTIFF written by `crate::geotiff::write_f32` (Deflate, `NaN` no-data):
    /// unit, CRS, cell size, size in cells and on disk.
    pub fn geotiff_f32(
        name: impl Into<String>,
        description: impl Into<String>,
        unit: impl Into<String>,
        grid: &crate::grid::Grid<f32>,
        bytes: u64,
    ) -> Self {
        Self {
            unit: Some(unit.into()),
            format: Some("GeoTIFF Float32, Deflate".into()),
            crs: Some(format!("EPSG:{}", grid.crs().epsg())),
            cell_size: Some(grid.transform().pixel_width),
            size_cells: Some([grid.width(), grid.height()]),
            nodata: Some("NaN".into()),
            bytes: Some(bytes),
            ..Self::new(name, description)
        }
    }

    /// Sets the licence of the content.
    pub fn licence(mut self, licence: impl Into<String>) -> Self {
        self.licence = Some(licence.into());
        self
    }

    /// Sets the number of features or entries.
    pub fn count(mut self, count: usize) -> Self {
        self.count = Some(count);
        self
    }

    /// Adds documented properties.
    pub fn properties<K: Into<String>, V: Into<String>>(
        mut self,
        props: impl IntoIterator<Item = (K, V)>,
    ) -> Self {
        self.properties
            .extend(props.into_iter().map(|(k, v)| (k.into(), v.into())));
        self
    }

    /// Adds documented properties from a JSON object of strings (other values are
    /// written as JSON text).
    pub fn properties_json(self, props: &Value) -> Self {
        let pairs: Vec<(String, String)> = props
            .as_object()
            .into_iter()
            .flatten()
            .map(|(k, v)| {
                let text = v.as_str().map_or_else(|| v.to_string(), str::to_owned);
                (k.clone(), text)
            })
            .collect();
        self.properties(pairs)
    }
}

/// Metadata of a layer (`<layer>.meta.json`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LayerMeta {
    /// Layer name (directory name).
    pub layer: String,
    /// Site identifier, or [`crate::layout::FRANCE`] for the national layers.
    pub site: String,
    /// Ticket(s) producing the layer.
    #[serde(default)]
    pub tickets: Vec<String>,
    /// Generation time, RFC 3339 UTC.
    pub generated_at: String,
    /// Start of validity of the content, or the date of the state it describes (RFC 3339
    /// or ISO local date-time), if known.
    #[serde(default)]
    pub valid_from: Option<String>,
    /// End of validity of the content, if known.
    #[serde(default)]
    pub valid_until: Option<String>,
    /// How long the content stays valid, in words.
    #[serde(default)]
    pub validity: Option<String>,
    /// Coordinate reference system of the files.
    #[serde(default)]
    pub crs: Option<String>,
    /// Sources.
    pub sources: Vec<SourceRef>,
    /// Files of the layer.
    pub files: Vec<FileMeta>,
    /// Method, limits and caveats (French).
    #[serde(default)]
    pub notes: Vec<String>,
    /// Parameters of the computation.
    #[serde(default, skip_serializing_if = "Value::is_null")]
    pub parameters: Value,
    /// Layer-specific key figures.
    #[serde(default, skip_serializing_if = "Value::is_null")]
    pub stats: Value,
}

impl LayerMeta {
    /// Empty metadata of `layer` for `site` (a site identifier or
    /// [`crate::layout::FRANCE`]), generated now.
    pub fn new(layer: impl Into<String>, site: impl Into<String>) -> Self {
        Self {
            layer: layer.into(),
            site: site.into(),
            tickets: Vec::new(),
            generated_at: now_rfc3339(),
            valid_from: None,
            valid_until: None,
            validity: None,
            crs: None,
            sources: Vec::new(),
            files: Vec::new(),
            notes: Vec::new(),
            parameters: Value::Null,
            stats: Value::Null,
        }
    }

    /// File name of the metadata of `layer`.
    pub fn file_name(layer: &str) -> String {
        format!("{layer}.meta.json")
    }

    /// Writes `<dir>/<layer>.meta.json` and returns its path.
    pub fn write(&self, dir: &Path) -> Result<PathBuf> {
        let path = dir.join(Self::file_name(&self.layer));
        write_json(&path, self)?;
        Ok(path)
    }

    /// Reads a `<layer>.meta.json`.
    pub fn read(path: &Path) -> Result<Self> {
        let body = std::fs::read(path).map_err(|e| Error::io(path, e))?;
        serde_json::from_slice(&body).map_err(|e| Error::json(path.display().to_string(), e))
    }
}

/// Current time, RFC 3339 UTC, second precision.
pub fn now_rfc3339() -> String {
    jiff::Timestamp::now()
        .strftime("%Y-%m-%dT%H:%M:%SZ")
        .to_string()
}

/// Seconds since the epoch as RFC 3339 UTC, second precision.
pub fn unix_to_rfc3339(secs: i64) -> Option<String> {
    Timestamp::new(secs, 0)
        .ok()
        .map(|d| d.strftime("%Y-%m-%dT%H:%M:%SZ").to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_and_optional_fields() {
        let dir = tempfile::tempdir().unwrap();
        let mut m = LayerMeta::new("pistes", "cervieres");
        m.tickets = vec!["P2 (#15)".into()];
        m.crs = Some("EPSG:4326".into());
        m.sources = vec![
            SourceRef::new("OSM", "https://example.org/x", "ODbL 1.0", "© OSM")
                .edition("2026-11-13"),
            SourceRef::new("Widget", "https://example.org/w", "aucune", "W").personal_use_only(),
        ];
        m.files = vec![
            FileMeta::new("pistes.geojson", "pistes")
                .count(3)
                .properties([("name", "nom")]),
            FileMeta::new("dem.tif", "relief").properties_json(&serde_json::json!({"a": "b"})),
        ];
        let path = m.write(dir.path()).unwrap();
        assert_eq!(path, dir.path().join("pistes.meta.json"));
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(text.ends_with("}\n"), "trailing newline");
        assert!(
            !text.contains("\"etag\""),
            "empty optional source fields omitted"
        );
        assert!(!text.contains("\"parameters\""), "null parameters omitted");
        assert!(
            text.contains("\"valid_until\": null"),
            "top-level fields kept"
        );
        assert!(text.contains("\"personal_use_only\": false"));
        let back = LayerMeta::read(&path).unwrap();
        assert_eq!(back, m);
        assert!(back.sources[1].personal_use_only);
        assert_eq!(back.files[1].properties["a"], "b");
    }

    #[test]
    fn unix_time() {
        assert_eq!(
            unix_to_rfc3339(1_794_693_847).as_deref(),
            Some("2026-11-14T22:04:07Z")
        );
    }
}
