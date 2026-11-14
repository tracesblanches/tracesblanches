//! Writing layers: GeoJSON (WGS84) and `<layer>.meta.json`.

use std::io::{BufWriter, Write};
use std::path::Path;

use serde::Serialize;
use serde_json::{Value, json};
use skimap_common::download::Fetched;
use skimap_common::geom::LonLat;

use crate::error::{OsmError, Result};

/// Rounds a coordinate to 7 decimals (≈ 1 cm, the precision of OSM).
pub fn round7(v: f64) -> f64 {
    (v * 1e7).round() / 1e7
}

/// GeoJSON position.
pub fn position(p: LonLat) -> Value {
    json!([round7(p.lon), round7(p.lat)])
}

/// GeoJSON LineString geometry.
pub fn line_string(points: impl IntoIterator<Item = LonLat>) -> Value {
    json!({
        "type": "LineString",
        "coordinates": points.into_iter().map(position).collect::<Vec<_>>(),
    })
}

/// GeoJSON Point geometry.
pub fn point(p: LonLat) -> Value {
    json!({ "type": "Point", "coordinates": position(p) })
}

/// GeoJSON Feature.
pub fn feature(geometry: Value, properties: Value) -> Value {
    json!({ "type": "Feature", "geometry": geometry, "properties": properties })
}

/// Writes a FeatureCollection, one feature per line.
pub fn write_geojson(path: &Path, features: &[Value]) -> Result<()> {
    let io = |e| OsmError::io(path, e);
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(io)?;
    }
    let tmp = path.with_extension("geojson.tmp");
    let f = std::fs::File::create(&tmp).map_err(io)?;
    let mut w = BufWriter::new(f);
    w.write_all(b"{\"type\":\"FeatureCollection\",\"features\":[\n")
        .map_err(io)?;
    for (i, feat) in features.iter().enumerate() {
        if i > 0 {
            w.write_all(b",\n").map_err(io)?;
        }
        serde_json::to_writer(&mut w, feat).map_err(|e| {
            OsmError::Common(skimap_common::Error::json(path.display().to_string(), e))
        })?;
    }
    w.write_all(b"\n]}\n").map_err(io)?;
    w.flush().map_err(io)?;
    drop(w);
    std::fs::rename(&tmp, path).map_err(io)
}

/// Writes any serialisable value as pretty JSON.
pub fn write_json<T: Serialize>(path: &Path, value: &T) -> Result<()> {
    let io = |e| OsmError::io(path, e);
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(io)?;
    }
    let s = serde_json::to_string_pretty(value)
        .map_err(|e| OsmError::Common(skimap_common::Error::json(path.display().to_string(), e)))?;
    std::fs::write(path, s + "\n").map_err(io)
}

/// One source of a layer, as recorded in `<layer>.meta.json`.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct SourceMeta {
    /// Short name.
    pub name: String,
    /// Public URL.
    pub url: String,
    /// Licence.
    pub licence: String,
    /// Attribution text to display.
    pub attribution: String,
    /// Download date (RFC 3339, UTC).
    pub fetched_at: Option<String>,
    /// `Last-Modified` of the response.
    pub last_modified: Option<String>,
    /// `ETag` of the response.
    pub etag: Option<String>,
    /// SHA-256 of the body.
    pub sha256: Option<String>,
}

impl SourceMeta {
    /// Source description from a downloaded body.
    pub fn from_fetched(name: &str, licence: &str, attribution: &str, f: &Fetched) -> Self {
        Self {
            name: name.to_owned(),
            url: f.meta.url.clone(),
            licence: licence.to_owned(),
            attribution: attribution.to_owned(),
            fetched_at: unix_to_rfc3339(f.meta.fetched_at_unix as i64),
            last_modified: f.meta.last_modified.clone(),
            etag: f.meta.etag.clone(),
            sha256: Some(f.meta.sha256.clone()),
        }
    }
}

/// Seconds since the epoch as RFC 3339 UTC.
pub fn unix_to_rfc3339(secs: i64) -> Option<String> {
    chrono::DateTime::from_timestamp(secs, 0)
        .map(|d| d.to_rfc3339_opts(chrono::SecondsFormat::Secs, true))
}

/// Current time as RFC 3339 UTC.
pub fn now_rfc3339() -> String {
    chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true)
}

/// Content of `<layer>.meta.json`.
#[derive(Debug, Clone, Serialize)]
pub struct LayerMeta {
    /// Layer name (directory name).
    pub layer: String,
    /// Site identifier, or `france`.
    pub site: String,
    /// Ticket(s) producing the layer.
    pub tickets: Vec<String>,
    /// Generation date (RFC 3339, UTC).
    pub generated_at: String,
    /// Date the data describes (timestamp of the source extract).
    pub valid_at: Option<String>,
    /// Coordinate reference system of the files.
    pub crs: String,
    /// Sources.
    pub sources: Vec<SourceMeta>,
    /// Files of the layer, with their description.
    pub files: Vec<FileMeta>,
    /// Parameters of the computation.
    pub parameters: Value,
    /// Key figures.
    pub stats: Value,
}

/// One file of a layer.
#[derive(Debug, Clone, Serialize)]
pub struct FileMeta {
    /// File name, relative to the layer directory.
    pub path: String,
    /// What the file contains.
    pub description: String,
    /// Licence of the file content.
    pub licence: String,
    /// Number of features (GeoJSON) or entries.
    pub count: usize,
    /// Documented properties: name → meaning.
    pub properties: Value,
}
