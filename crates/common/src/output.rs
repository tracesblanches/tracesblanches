//! Writing layer files: JSON, raw bytes and GeoJSON (WGS84), always atomically.
//!
//! Every writer creates the parent directory, writes `<file>.tmp` next to the target and
//! renames it (after `fsync`), so that a reader never sees a half-written file, even after
//! a crash or a power cut. JSON is pretty-printed and ends with a newline.

use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};

use serde::Serialize;
use serde_json::{Value, json};

use crate::error::{Error, Result};
use crate::geom::LonLat;

/// Temporary file next to `path`: `<file name>.tmp`.
fn tmp_path(path: &Path) -> PathBuf {
    let mut name = path.file_name().unwrap_or_default().to_os_string();
    name.push(".tmp");
    path.with_file_name(name)
}

fn ensure_parent(path: &Path) -> Result<()> {
    if let Some(dir) = path.parent()
        && !dir.as_os_str().is_empty()
    {
        std::fs::create_dir_all(dir).map_err(|e| Error::io(dir, e))?;
    }
    Ok(())
}

/// Writes bytes atomically (temporary file then rename).
pub fn write_bytes(path: &Path, body: &[u8]) -> Result<()> {
    ensure_parent(path)?;
    let tmp = tmp_path(path);
    let io = |e| Error::io(&tmp, e);
    let mut f = std::fs::File::create(&tmp).map_err(io)?;
    f.write_all(body).map_err(io)?;
    // On disk before the rename: a crash never leaves the new name on empty content.
    f.sync_all().map_err(io)?;
    drop(f);
    std::fs::rename(&tmp, path).map_err(|e| Error::io(path, e))
}

/// Writes text atomically.
pub fn write_text(path: &Path, text: &str) -> Result<()> {
    write_bytes(path, text.as_bytes())
}

/// Writes a value as pretty JSON followed by a newline, atomically.
pub fn write_json<T: Serialize + ?Sized>(path: &Path, value: &T) -> Result<()> {
    let mut body =
        serde_json::to_vec_pretty(value).map_err(|e| Error::json(path.display().to_string(), e))?;
    body.push(b'\n');
    write_bytes(path, &body)
}

/// Rounds a coordinate to 7 decimals (≈ 1 cm, the precision of OSM).
pub fn round7(v: f64) -> f64 {
    (v * 1e7).round() / 1e7
}

/// GeoJSON position `[lon, lat]`, rounded to 7 decimals.
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

/// Writes a FeatureCollection with one feature per line (compact, diff-friendly),
/// atomically.
pub fn write_geojson(path: &Path, features: &[Value]) -> Result<()> {
    ensure_parent(path)?;
    let tmp = tmp_path(path);
    let io = |e| Error::io(&tmp, e);
    let f = std::fs::File::create(&tmp).map_err(io)?;
    let mut w = BufWriter::new(f);
    w.write_all(b"{\"type\":\"FeatureCollection\",\"features\":[\n")
        .map_err(io)?;
    for (i, feat) in features.iter().enumerate() {
        if i > 0 {
            w.write_all(b",\n").map_err(io)?;
        }
        serde_json::to_writer(&mut w, feat)
            .map_err(|e| Error::json(path.display().to_string(), e))?;
    }
    w.write_all(b"\n]}\n").map_err(io)?;
    let f = w.into_inner().map_err(|e| io(e.into_error()))?;
    f.sync_all().map_err(io)?;
    drop(f);
    std::fs::rename(&tmp, path).map_err(|e| Error::io(path, e))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn writers() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("a/b/x.json");
        write_json(&p, &json!({"k": 1})).unwrap();
        assert_eq!(std::fs::read_to_string(&p).unwrap(), "{\n  \"k\": 1\n}\n");
        // `x.geojson` and `x.json` in the same directory do not share a temporary file.
        let g = dir.path().join("a/b/x.geojson");
        let f = feature(point(LonLat::new(6.123456789, 44.9)), json!({"n": "a"}));
        write_geojson(&g, &[f.clone(), f]).unwrap();
        let text = std::fs::read_to_string(&g).unwrap();
        assert!(text.starts_with("{\"type\":\"FeatureCollection\",\"features\":[\n"));
        assert!(text.contains("[6.1234568,44.9]"));
        let v: Value = serde_json::from_str(&text).unwrap();
        assert_eq!(v["features"].as_array().unwrap().len(), 2);
        let names: Vec<_> = std::fs::read_dir(dir.path().join("a/b"))
            .unwrap()
            .map(|e| e.unwrap().file_name().into_string().unwrap())
            .collect();
        assert!(names.iter().all(|n| !n.ends_with(".tmp")), "{names:?}");
        write_bytes(&dir.path().join("raw.xml"), b"<a/>").unwrap();
        assert_eq!(
            line_string([LonLat::new(1.0, 2.0)])["coordinates"],
            json!([[1.0, 2.0]])
        );
    }
}
