//! No orphan layer: every layer of a style draws at least one feature of the real packs
//! (one of them suffices: avalanche areas exist at Cervières only). Filters are evaluated
//! on every feature of every tile at the zooms where the layer is visible. A pack without
//! the file of a source (optional sources, `metadata.skimap:optional_sources`) counts no
//! feature for its layers.

use std::collections::BTreeMap;
use std::path::Path;

use serde_json::{Map, Value};
use skimap_pack::reader::{Pack, TileArchive};

use crate::expr::{self, Feature, truthy};
use crate::inventory::{for_each_tile, geojson_geometry_type};
use crate::mvt::{self, Value as TileValue};
use crate::{Error, Result};

/// Features drawn by each layer of `style` in the packs `dirs` (layer id → count).
/// Background and runtime layers are not counted.
pub fn matches(style: &Value, dirs: &[&Path]) -> Result<BTreeMap<String, u64>> {
    let layers = style["layers"].as_array().cloned().unwrap_or_default();
    let mut counts: BTreeMap<String, u64> = BTreeMap::new();
    for dir in dirs {
        let pack = Pack::open(*dir).map_err(Error::pack)?;
        // Vector tiles: one pass per archive for all its layers.
        let mut by_archive: BTreeMap<String, Vec<&Value>> = BTreeMap::new();
        for l in &layers {
            let Some(src) = l["source"].as_str() else {
                continue;
            };
            let def = &style["sources"][src];
            let file = def["data"].as_str().or_else(|| {
                def["url"]
                    .as_str()
                    .map(|u| u.trim_start_matches("pmtiles://"))
            });
            if file.is_some_and(|f| !dir.join(f).is_file()) {
                continue;
            }
            match def["type"].as_str() {
                Some("vector") => {
                    let path = def["url"]
                        .as_str()
                        .unwrap_or("")
                        .trim_start_matches("pmtiles://");
                    by_archive.entry(path.to_string()).or_default().push(l);
                }
                Some("geojson") => {
                    let Some(path) = def["data"].as_str() else {
                        continue;
                    };
                    let n = geojson_matches(&pack, path, l)?;
                    *counts.entry(id(l)).or_default() += n;
                }
                Some("raster") => {
                    let path = def["url"]
                        .as_str()
                        .unwrap_or("")
                        .trim_start_matches("pmtiles://");
                    let a = TileArchive::open(pack.path(path).map_err(Error::pack)?)
                        .map_err(Error::pack)?;
                    let n = a.addressed_tiles().unwrap_or(0);
                    *counts.entry(id(l)).or_default() += n;
                }
                _ => {}
            }
        }
        for (path, ls) in by_archive {
            let archive =
                TileArchive::open(pack.path(&path).map_err(Error::pack)?).map_err(Error::pack)?;
            let (_, amax) = archive.zoom_range();
            let mut err = None;
            for_each_tile(&archive, |z, tile| {
                for l in &ls {
                    let (z0, z1) = tile_zooms(l, amax);
                    if z < z0 || z > z1 {
                        continue;
                    }
                    let sl = l["source-layer"].as_str().unwrap_or("");
                    let mut n = 0;
                    for tl in tile.iter().filter(|t| t.name == sl) {
                        for f in &tl.features {
                            let feat = TileFeature { layer: tl, f };
                            match passes(l, &feat, f64::from(z)) {
                                Ok(true) => n += 1,
                                Ok(false) => {}
                                Err(e) => err = Some(Error::Json(id(l), e.to_string())),
                            }
                        }
                    }
                    *counts.entry(id(l)).or_default() += n;
                }
            })?;
            if let Some(e) = err {
                return Err(e);
            }
        }
    }
    Ok(counts)
}

/// Layers of `style` that draw nothing in the packs `dirs`.
pub fn orphans(style: &Value, dirs: &[&Path]) -> Result<Vec<String>> {
    let counts = matches(style, dirs)?;
    let runtime = &style["metadata"]["skimap:runtime_sources"];
    Ok(style["layers"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|l| l["type"] != "background")
        .filter(|l| runtime.get(l["source"].as_str().unwrap_or("")).is_none())
        .map(id)
        .filter(|i| counts.get(i).copied().unwrap_or(0) == 0)
        .collect())
}

fn id(l: &Value) -> String {
    l["id"].as_str().unwrap_or("?").to_string()
}

/// Tile zooms shown while the layer is visible (overzoom past the archive maximum).
fn tile_zooms(l: &Value, amax: u8) -> (u8, u8) {
    let minzoom = l["minzoom"].as_f64().unwrap_or(0.0);
    let maxzoom = l["maxzoom"].as_f64().unwrap_or(24.0);
    let z0 = (minzoom.floor() as u8).min(amax);
    let z1 = (maxzoom.ceil() as u8).saturating_sub(1).min(amax);
    (z0, z1)
}

fn passes(l: &Value, f: &dyn Feature, zoom: f64) -> std::result::Result<bool, expr::EvalError> {
    match l.get("filter") {
        None => Ok(true),
        Some(e) => Ok(truthy(&expr::eval(e, f, zoom)?)),
    }
}

struct TileFeature<'a> {
    layer: &'a mvt::Layer,
    f: &'a mvt::Feature,
}

impl Feature for TileFeature<'_> {
    fn get(&self, key: &str) -> Option<Value> {
        self.layer
            .attributes(self.f)
            .find(|(k, _)| *k == key)
            .map(|(_, v)| match v {
                TileValue::Str(s) => Value::String(s.clone()),
                TileValue::Num(n) => serde_json::json!(n),
                TileValue::Bool(b) => Value::Bool(*b),
            })
    }
    fn geometry_type(&self) -> &str {
        self.f.geom.name()
    }
}

struct JsonFeature<'a> {
    props: Option<&'a Map<String, Value>>,
    geom: &'static str,
}

impl Feature for JsonFeature<'_> {
    fn get(&self, key: &str) -> Option<Value> {
        self.props?.get(key).filter(|v| !v.is_null()).cloned()
    }
    fn geometry_type(&self) -> &str {
        self.geom
    }
}

fn geojson_matches(pack: &Pack, path: &str, l: &Value) -> Result<u64> {
    let fc: Value = serde_json::from_slice(&pack.read_file(path).map_err(Error::pack)?)
        .map_err(|e| Error::Json(path.into(), e.to_string()))?;
    let minzoom = l["minzoom"].as_f64().unwrap_or(0.0);
    let mut n = 0;
    for f in fc["features"].as_array().into_iter().flatten() {
        let feat = JsonFeature {
            props: f["properties"].as_object(),
            geom: f
                .get("geometry")
                .and_then(geojson_geometry_type)
                .unwrap_or("Unknown"),
        };
        if passes(l, &feat, minzoom).map_err(|e| Error::Json(id(l), e.to_string()))? {
            n += 1;
        }
    }
    Ok(n)
}
