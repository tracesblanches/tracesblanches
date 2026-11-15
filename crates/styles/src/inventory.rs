//! What a pack offers to a style: vector tile layers with their attributes and values,
//! GeoJSON layers with their properties, raster archives, glyph font stacks and sprite
//! icons. Built from a real pack ([`Inventory::from_pack`]) or read back from the snapshot
//! committed next to the styles (`styles/pack-schema.json`), so that the validator also
//! runs where the packs are absent (CI).

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use serde::{Deserialize, Serialize};
use skimap_common::geom::BBox;
use skimap_pack::manifest::LayerKind;
use skimap_pack::reader::{Pack, TileArchive};
use skimap_pack::tiles::tiles_covering;

use crate::mvt::{self, GeomType, Value};
use crate::{Error, Result};

/// Above this many distinct values, a key is recorded without its values (names, ids).
pub const MAX_VALUES: usize = 160;

/// Names in other languages and scripts of the OSM tiles (`name:de`, `pgf:name:hi`):
/// not recorded (`name` and `name:fr` are).
pub fn translated_name(key: &str) -> bool {
    (key.starts_with("name:") && key != "name:fr") || key.starts_with("pgf:")
}

/// Whether the values of a key are worth recording: names (`name`, `name:fr`, `texte`
/// is caught by [`MAX_VALUES`]), scripts and identifiers are never compared by a style.
pub fn records_values(key: &str) -> bool {
    !(key.starts_with("name")
        || key.starts_with("pgf:")
        || key.starts_with("script")
        || matches!(
            key,
            "wikidata" | "sort_key" | "population" | "addr_housenumber"
        ))
}

/// Glyph ranges every font stack must have (latin, latin extended A, punctuation), the
/// ranges copied into the packs by `skimap-pack`.
pub const GLYPH_RANGES: [&str; 3] = ["0-255", "256-511", "8192-8447"];

/// Inventory of one pack.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Inventory {
    /// Site identifier.
    pub site: String,
    /// Vector tile archives (MVT), by path relative to the pack root.
    pub vector: BTreeMap<String, Archive>,
    /// GeoJSON layers, by path relative to the pack root.
    pub geojson: BTreeMap<String, Layer>,
    /// Raster tile archives, by path relative to the pack root.
    pub raster: BTreeMap<String, ZoomRange>,
    /// Font stacks with every range of [`GLYPH_RANGES`], by glyph directory
    /// (`basemap/plan-ign/fonts`).
    pub fonts: BTreeMap<String, BTreeSet<String>>,
    /// Icon names present in both the 1x and the 2x sprite, by sprite path without
    /// extension (`basemap/plan-ign/sprite/PlanIgn`).
    pub sprites: BTreeMap<String, BTreeSet<String>>,
}

/// Zoom range of an archive (header).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ZoomRange {
    /// Lowest zoom.
    pub minzoom: u8,
    /// Highest zoom.
    pub maxzoom: u8,
}

/// A vector tile archive.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Archive {
    /// Zooms of the header.
    pub zooms: ZoomRange,
    /// Tiles read.
    pub tiles: u64,
    /// Layers by name.
    pub layers: BTreeMap<String, Layer>,
}

/// A layer: where it has features, geometry types and attributes.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Layer {
    /// Lowest zoom with a feature (GeoJSON: 0).
    pub minzoom: u8,
    /// Highest zoom with a feature (GeoJSON: 24).
    pub maxzoom: u8,
    /// Number of features (all zooms).
    pub features: u64,
    /// Geometry types (`Point`, `LineString`, `Polygon`).
    pub geometry: BTreeSet<String>,
    /// Attributes by key.
    pub keys: BTreeMap<String, Key>,
}

/// An attribute key (serialised as its values, or `null`).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Key {
    /// Value (JSON text: `"forêt"`, `3`, `true`) → lowest zoom where it appears; `None`
    /// when there are more than [`MAX_VALUES`] distinct values, or values not recorded.
    pub values: Option<BTreeMap<String, u8>>,
}

impl Key {
    fn add(&mut self, value: String, z: u8) {
        if let Some(v) = &mut self.values {
            let e = v.entry(value).or_insert(z);
            *e = (*e).min(z);
            if v.len() > MAX_VALUES {
                self.values = None;
            }
        }
    }
}

impl Layer {
    /// A layer without feature (lowest zoom at its maximum until a feature comes).
    fn empty() -> Self {
        Self {
            minzoom: u8::MAX,
            ..Self::default()
        }
    }
    fn add_feature<'a>(
        &mut self,
        z: u8,
        geom: &str,
        attrs: impl Iterator<Item = (&'a str, String)>,
    ) {
        self.minzoom = self.minzoom.min(z);
        self.maxzoom = self.maxzoom.max(z);
        self.features += 1;
        self.geometry.insert(geom.to_string());
        for (k, v) in attrs.filter(|(k, _)| !translated_name(k)) {
            self.keys
                .entry(k.to_string())
                .or_insert_with(|| Key {
                    values: records_values(k).then(BTreeMap::new),
                })
                .add(v, z);
        }
    }
}

/// JSON text of a tile value, the key of [`Key::values`].
pub fn tile_value_text(v: &Value) -> String {
    match v {
        Value::Str(s) => serde_json::Value::String(s.clone()).to_string(),
        Value::Num(n) => number_text(*n),
        Value::Bool(b) => b.to_string(),
    }
}

/// JSON text of a number, integers without a fraction (`3`, not `3.0`).
pub fn number_text(n: f64) -> String {
    if n.fract() == 0.0 && n.abs() < 1e15 {
        format!("{}", n as i64)
    } else {
        serde_json::Number::from_f64(n).map_or_else(|| "null".into(), |x| x.to_string())
    }
}

/// Reads every tile of a vector archive inside its header bounds and calls `f` for each
/// decoded tile with its zoom.
pub fn for_each_tile(archive: &TileArchive, mut f: impl FnMut(u8, &[mvt::Layer])) -> Result<u64> {
    let [w, s, e, n] = archive.bounds();
    let (z0, z1) = archive.zoom_range();
    let mut count = 0;
    for (z, x, y) in tiles_covering(BBox::new(s, w, n, e), z0, z1) {
        let Some(bytes) = archive.tile(z, x, y).map_err(Error::pack)? else {
            continue;
        };
        let layers = mvt::decode(&bytes).map_err(|e| Error::Tile {
            path: archive.path().display().to_string(),
            tile: format!("{z}/{x}/{y}"),
            msg: e.to_string(),
        })?;
        count += 1;
        f(z, &layers);
    }
    Ok(count)
}

impl Archive {
    /// Inventory of a vector tile archive.
    pub fn read(path: &Path) -> Result<Self> {
        let archive = TileArchive::open(path).map_err(Error::pack)?;
        let (minzoom, maxzoom) = archive.zoom_range();
        let mut layers: BTreeMap<String, Layer> = BTreeMap::new();
        let tiles = for_each_tile(&archive, |z, tile| {
            for l in tile {
                let inv = layers.entry(l.name.clone()).or_insert_with(Layer::empty);
                for f in &l.features {
                    let attrs = l.attributes(f).map(|(k, v)| (k, tile_value_text(v)));
                    inv.add_feature(z, geom_name(f.geom), attrs);
                }
            }
        })?;
        Ok(Self {
            zooms: ZoomRange { minzoom, maxzoom },
            tiles,
            layers,
        })
    }
}

fn geom_name(g: GeomType) -> &'static str {
    g.name()
}

/// MapLibre geometry type of a GeoJSON geometry (`MultiPolygon` → `Polygon`).
pub fn geojson_geometry_type(g: &serde_json::Value) -> Option<&'static str> {
    match g.get("type")?.as_str()? {
        "Point" | "MultiPoint" => Some("Point"),
        "LineString" | "MultiLineString" => Some("LineString"),
        "Polygon" | "MultiPolygon" => Some("Polygon"),
        _ => None,
    }
}

/// Properties of a GeoJSON feature as a style sees them through MapLibre: scalars as
/// they are, arrays and objects as JSON values (`in` on an array works, `==` does not).
pub fn geojson_layer(fc: &serde_json::Value) -> Layer {
    let mut layer = Layer::empty();
    let features = fc
        .get("features")
        .and_then(|f| f.as_array())
        .map_or(&[][..], |v| v.as_slice());
    for f in features {
        let geom = f
            .get("geometry")
            .and_then(geojson_geometry_type)
            .unwrap_or("Unknown");
        let props = f.get("properties").and_then(|p| p.as_object());
        let attrs = props
            .into_iter()
            .flatten()
            .filter(|(_, v)| !v.is_null())
            .map(|(k, v)| (k.as_str(), v.to_string()));
        layer.add_feature(0, geom, attrs);
    }
    if layer.features == 0 {
        layer.minzoom = 0;
    }
    layer.maxzoom = 24;
    layer
}

impl Inventory {
    /// Inventory of a pack directory (`data/out/<site>/pack`).
    pub fn from_pack(dir: &Path) -> Result<Self> {
        let pack = Pack::open(dir).map_err(Error::pack)?;
        let m = pack.manifest();
        let mut inv = Inventory {
            site: m.site.id.clone(),
            ..Self::default()
        };
        for b in &m.basemaps {
            inv.vector.insert(
                b.tiles.clone(),
                Archive::read(&pack.path(&b.tiles).map_err(Error::pack)?)?,
            );
            let style: serde_json::Value =
                serde_json::from_slice(&pack.read_file(&b.style).map_err(Error::pack)?)
                    .map_err(|e| Error::Json(b.style.clone(), e.to_string()))?;
            if let Some(glyphs) = style.get("glyphs").and_then(|g| g.as_str()) {
                let fonts_dir = glyphs
                    .strip_suffix("/{fontstack}/{range}.pbf")
                    .ok_or_else(|| Error::Json(b.style.clone(), format!("glyphs {glyphs}")))?;
                inv.fonts.insert(
                    fonts_dir.to_string(),
                    font_stacks(&pack.path(fonts_dir).map_err(Error::pack)?)?,
                );
            }
            for sprite in sprite_paths(style.get("sprite")) {
                inv.sprites
                    .insert(sprite.clone(), sprite_icons(&pack, &sprite)?);
            }
        }
        for l in &m.layers {
            match (l.kind, l.format.as_str()) {
                (LayerKind::Vector, "geojson") => {
                    let fc: serde_json::Value =
                        serde_json::from_slice(&pack.read_file(&l.file).map_err(Error::pack)?)
                            .map_err(|e| Error::Json(l.file.clone(), e.to_string()))?;
                    inv.geojson.insert(l.file.clone(), geojson_layer(&fc));
                }
                (LayerKind::Vector, "pmtiles") => {
                    inv.vector.insert(
                        l.file.clone(),
                        Archive::read(&pack.path(&l.file).map_err(Error::pack)?)?,
                    );
                }
                (LayerKind::Raster, "pmtiles") => {
                    let a = TileArchive::open(pack.path(&l.file).map_err(Error::pack)?)
                        .map_err(Error::pack)?;
                    let (minzoom, maxzoom) = a.zoom_range();
                    inv.raster
                        .insert(l.file.clone(), ZoomRange { minzoom, maxzoom });
                }
                _ => {}
            }
        }
        Ok(inv)
    }
}

/// Sprite paths of a style `sprite` (string or array of `{id, url}`).
pub fn sprite_paths(sprite: Option<&serde_json::Value>) -> Vec<String> {
    match sprite {
        Some(serde_json::Value::String(s)) => vec![s.clone()],
        Some(serde_json::Value::Array(a)) => a
            .iter()
            .filter_map(|s| s.get("url")?.as_str().map(str::to_string))
            .collect(),
        _ => Vec::new(),
    }
}

fn font_stacks(dir: &Path) -> Result<BTreeSet<String>> {
    let mut out = BTreeSet::new();
    let rd = std::fs::read_dir(dir).map_err(|e| Error::Io(dir.display().to_string(), e))?;
    for e in rd {
        let e = e.map_err(|e| Error::Io(dir.display().to_string(), e))?;
        let name = e.file_name().to_string_lossy().into_owned();
        if GLYPH_RANGES
            .iter()
            .all(|r| e.path().join(format!("{r}.pbf")).is_file())
        {
            out.insert(name);
        }
    }
    Ok(out)
}

fn sprite_icons(pack: &Pack, sprite: &str) -> Result<BTreeSet<String>> {
    let mut sets = Vec::new();
    for suffix in ["", "@2x"] {
        let json = format!("{sprite}{suffix}.json");
        let png = format!("{sprite}{suffix}.png");
        pack.path(&png).map_err(Error::pack).and_then(|p| {
            p.is_file()
                .then_some(())
                .ok_or_else(|| Error::Json(png.clone(), "missing sprite image".into()))
        })?;
        let v: serde_json::Value =
            serde_json::from_slice(&pack.read_file(&json).map_err(Error::pack)?)
                .map_err(|e| Error::Json(json.clone(), e.to_string()))?;
        let names: BTreeSet<String> = v
            .as_object()
            .map(|o| o.keys().cloned().collect())
            .unwrap_or_default();
        sets.push(names);
    }
    Ok(sets[0].intersection(&sets[1]).cloned().collect())
}
