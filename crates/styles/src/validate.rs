//! Static validation of a style against what the packs offer ([`Inventory`]).
//!
//! Checked: MapLibre structure; every source points to a file of every pack (or is a
//! declared runtime source); no source without layer; every `source-layer` exists in the
//! tiles at the zooms where the layer is visible; every property read exists, every value
//! compared exists; geometry types fit the layer types; fonts and sprite icons are in the
//! pack; only the expression subset of [`crate::expr::OPERATORS`]; rasters sharp and under
//! lines and labels; contrast of the marked layers and of every text with its halo.

use std::collections::{BTreeMap, BTreeSet};

use serde_json::Value;

use crate::declared::declared_values;
use crate::expr::{LITERAL_ARRAYS, OPERATORS, walk};
use crate::inventory::{Inventory, Key, Layer, number_text};

/// WCAG ratio for graphics (lines, symbols drawn on a shape).
pub const MIN_GRAPHIC_CONTRAST: f64 = 3.0;
/// WCAG ratio for text against its halo.
pub const MIN_TEXT_CONTRAST: f64 = 4.5;
/// Thinnest text halo (pixels): labels must stay readable on any ground.
pub const MIN_HALO: f64 = 1.5;

const LAYER_TYPES: &[&str] = &["background", "fill", "line", "symbol", "circle", "raster"];

/// Where a style source takes its data.
#[derive(Debug, Clone)]
pub enum Source {
    /// Vector tiles of the pack (path relative to the pack root).
    Vector(String),
    /// Raster tiles of the pack.
    Raster(String),
    /// GeoJSON layer of the pack.
    GeoJson(String),
    /// Filled by the app; schema from the style metadata.
    Runtime(Layer),
}

/// Validates a style against the inventories of the packs (all must offer every file;
/// properties and values are looked up in their union). Returns the problems found.
pub fn validate(style: &Value, packs: &[Inventory]) -> Vec<String> {
    let mut v = Validator {
        errors: Vec::new(),
        packs,
    };
    v.style(style);
    v.errors
}

struct Validator<'a> {
    errors: Vec<String>,
    packs: &'a [Inventory],
}

impl Validator<'_> {
    fn err(&mut self, msg: String) {
        self.errors.push(msg);
    }

    fn style(&mut self, s: &Value) {
        if s["version"] != 8 {
            self.err("version must be 8".into());
        }
        if !s["name"].is_string() {
            self.err("missing name".into());
        }
        let meta = &s["metadata"];
        for k in ["skimap:style", "skimap:theme"] {
            if !meta[k].is_string() {
                self.err(format!("metadata {k} missing"));
            }
        }
        if meta["skimap:relative_urls"] != true {
            self.err("metadata skimap:relative_urls must be true".into());
        }
        let glyph_dir = self.glyphs(s);
        let sprite = self.sprite(s);
        let sources = self.sources(s);
        let Some(layers) = s["layers"].as_array() else {
            self.err("layers must be an array".into());
            return;
        };
        let mut ids = BTreeSet::new();
        let mut used = BTreeSet::new();
        for l in layers {
            let id = l["id"].as_str().unwrap_or("?").to_string();
            if !ids.insert(id.clone()) {
                self.err(format!("layer {id}: duplicate id"));
            }
            if let Some(src) = l["source"].as_str() {
                used.insert(src.to_string());
            }
            self.layer(&id, l, &sources, glyph_dir.as_deref(), sprite.as_deref());
        }
        for id in sources.keys() {
            if !used.contains(id) {
                self.err(format!("source {id}: no layer uses it (orphan source)"));
            }
        }
        self.order(layers);
        self.contrast(layers);
        if let Some(opt) = meta["skimap:optional_layers"].as_array() {
            for o in opt {
                let hidden = layers
                    .iter()
                    .any(|l| l["id"] == *o && l["layout"]["visibility"] == "none");
                if !hidden {
                    self.err(format!("optional layer {o}: absent or not hidden"));
                }
            }
        }
    }

    fn glyphs(&mut self, s: &Value) -> Option<String> {
        let Some(g) = s["glyphs"].as_str() else {
            self.err("glyphs missing: MapLibre would fetch fonts from the network".into());
            return None;
        };
        let Some(dir) = g.strip_suffix("/{fontstack}/{range}.pbf") else {
            self.err(format!(
                "glyphs {g}: expected <dir>/{{fontstack}}/{{range}}.pbf"
            ));
            return None;
        };
        for p in self.packs {
            if !p.fonts.contains_key(dir) {
                self.err(format!("glyphs {dir}: absent from the {} pack", p.site));
            }
        }
        Some(dir.to_string())
    }

    fn sprite(&mut self, s: &Value) -> Option<String> {
        let sprite = s["sprite"].as_str()?.to_string();
        for p in self.packs {
            if !p.sprites.contains_key(&sprite) {
                self.err(format!("sprite {sprite}: absent from the {} pack", p.site));
            }
        }
        Some(sprite)
    }

    fn sources(&mut self, s: &Value) -> BTreeMap<String, Source> {
        let mut out = BTreeMap::new();
        let Some(m) = s["sources"].as_object() else {
            self.err("sources must be an object".into());
            return out;
        };
        let runtime = &s["metadata"]["skimap:runtime_sources"];
        for (id, src) in m {
            let url = src["url"]
                .as_str()
                .and_then(|u| u.strip_prefix("pmtiles://"));
            let source = match (src["type"].as_str(), url, &src["data"]) {
                (Some("vector"), Some(path), _) => Source::Vector(path.to_string()),
                (Some("raster"), Some(path), _) => Source::Raster(path.to_string()),
                (Some("geojson"), _, Value::String(path)) => Source::GeoJson(path.clone()),
                (Some("geojson"), _, Value::Object(_)) => match runtime_layer(&runtime[id]) {
                    Some(l) => {
                        if src["data"]["features"]
                            .as_array()
                            .is_none_or(|f| !f.is_empty())
                        {
                            self.err(format!("source {id}: runtime source must start empty"));
                        }
                        Source::Runtime(l)
                    }
                    None => {
                        self.err(format!(
                            "source {id}: inline data without skimap:runtime_sources schema"
                        ));
                        continue;
                    }
                },
                _ => {
                    self.err(format!(
                        "source {id}: unsupported (pmtiles:// vector or raster, geojson only)"
                    ));
                    continue;
                }
            };
            for p in self.packs {
                let present = match &source {
                    Source::Vector(path) => p.vector.contains_key(path),
                    Source::Raster(path) => p.raster.contains_key(path),
                    Source::GeoJson(path) => p.geojson.contains_key(path),
                    Source::Runtime(_) => true,
                };
                if !present {
                    self.err(format!("source {id}: file absent from the {} pack", p.site));
                }
            }
            out.insert(id.clone(), source);
        }
        out
    }

    /// Layer schema of a source (union of the packs), and the tile zoom range.
    fn schema(
        &self,
        src: &Source,
        source_layer: Option<&str>,
    ) -> Option<(Layer, Option<(u8, u8)>)> {
        let mut found: Option<Layer> = None;
        let mut archive_zoom = None;
        for p in self.packs {
            let l = match src {
                Source::Vector(path) => {
                    let a = p.vector.get(path)?;
                    archive_zoom = Some((a.zooms.minzoom, a.zooms.maxzoom));
                    a.layers.get(source_layer?)
                }
                Source::GeoJson(path) => p.geojson.get(path),
                Source::Runtime(l) => return Some((l.clone(), None)),
                Source::Raster(_) => return None,
            };
            if let Some(l) = l {
                found = Some(match found {
                    None => l.clone(),
                    Some(acc) => union(acc, l),
                });
            }
        }
        found.map(|l| (l, archive_zoom))
    }

    fn layer(
        &mut self,
        id: &str,
        l: &Value,
        sources: &BTreeMap<String, Source>,
        glyph_dir: Option<&str>,
        sprite: Option<&str>,
    ) {
        let ty = l["type"].as_str().unwrap_or("");
        if !LAYER_TYPES.contains(&ty) {
            self.err(format!("layer {id}: type {ty:?} not allowed"));
            return;
        }
        let minzoom = l["minzoom"].as_f64().unwrap_or(0.0);
        let maxzoom = l["maxzoom"].as_f64().unwrap_or(24.0);
        if minzoom >= maxzoom {
            self.err(format!(
                "layer {id}: minzoom {minzoom} >= maxzoom {maxzoom}"
            ));
        }
        self.expressions(id, l);
        if ty == "background" {
            if l.get("source").is_some() {
                self.err(format!("layer {id}: background with a source"));
            }
            return;
        }
        let Some(src_id) = l["source"].as_str() else {
            self.err(format!("layer {id}: no source"));
            return;
        };
        let Some(src) = sources.get(src_id) else {
            self.err(format!("layer {id}: unknown source {src_id}"));
            return;
        };
        let source_layer = l["source-layer"].as_str();
        match (src, ty, source_layer) {
            (Source::Raster(_), "raster", None) => {
                if l["paint"]["raster-resampling"] != "nearest" {
                    self.err(format!(
                        "layer {id}: raster classes must use raster-resampling nearest (sharp)"
                    ));
                }
                return;
            }
            (Source::Raster(_), _, _) | (_, "raster", _) => {
                self.err(format!("layer {id}: raster layer and source must match"));
                return;
            }
            (Source::Vector(_), _, None) => {
                self.err(format!("layer {id}: vector source without source-layer"));
                return;
            }
            (Source::GeoJson(_) | Source::Runtime(_), _, Some(_)) => {
                self.err(format!("layer {id}: source-layer on a GeoJSON source"));
                return;
            }
            _ => {}
        }
        let Some((schema, archive_zoom)) = self.schema(src, source_layer) else {
            self.err(format!(
                "layer {id}: source-layer {} absent from the tiles of {src_id}",
                source_layer.unwrap_or("?")
            ));
            return;
        };
        if let Some((_, amax)) = archive_zoom {
            // Tiles shown between minzoom and maxzoom (overzoomed past the archive).
            let z0 = (minzoom.floor() as u8).min(amax);
            let z1 = ((maxzoom.ceil() as u8).saturating_sub(1)).min(amax);
            if schema.features > 0 && (schema.maxzoom < z0 || schema.minzoom > z1) {
                self.err(format!(
                    "layer {id}: visible z{minzoom}-{maxzoom} but {} has features only z{}-{}",
                    source_layer.unwrap_or("?"),
                    schema.minzoom,
                    schema.maxzoom
                ));
            }
        }
        if schema.features > 0 || matches!(src, Source::Runtime(_)) {
            let fits = match ty {
                "fill" => schema.geometry.contains("Polygon"),
                "line" => {
                    schema.geometry.contains("LineString") || schema.geometry.contains("Polygon")
                }
                _ => true,
            };
            if !fits {
                self.err(format!(
                    "layer {id}: {ty} on geometries {:?}",
                    schema.geometry
                ));
            }
        }
        let path = match src {
            Source::Vector(p) | Source::GeoJson(p) => Some(p.as_str()),
            _ => None,
        };
        self.properties(id, l, &schema, path);
        if ty == "symbol" {
            self.symbol(id, l, glyph_dir, sprite);
        }
    }

    /// Operator subset and legacy filters.
    fn expressions(&mut self, id: &str, l: &Value) {
        let mut bad = Vec::new();
        for e in expressions_of(l) {
            walk(e, &mut |a| {
                let op = a[0].as_str().unwrap_or("");
                if !OPERATORS.contains(&op) {
                    bad.push(format!("operator {op:?} outside the supported subset"));
                }
                if matches!(op, "==" | "!=" | "<" | "<=" | ">" | ">=")
                    && !a[1..].iter().any(Value::is_array)
                {
                    bad.push(format!(
                        "constant comparison (legacy filter syntax?) {}",
                        Value::Array(a.to_vec())
                    ));
                }
            });
        }
        if let Some(f) = l.get("filter")
            && !f.is_array()
        {
            bad.push("filter must be an expression".into());
        }
        for b in bad {
            self.err(format!("layer {id}: {b}"));
        }
    }

    /// Properties read exist; values compared exist.
    fn properties(&mut self, id: &str, l: &Value, schema: &Layer, path: Option<&str>) {
        let mut problems = Vec::new();
        for e in expressions_of(l) {
            walk(e, &mut |a| {
                let op = a[0].as_str().unwrap_or("");
                if matches!(op, "get" | "has")
                    && let Some(k) = a.get(1).and_then(Value::as_str)
                    && !schema.keys.contains_key(k)
                {
                    problems.push(format!("property {k} absent from the data"));
                }
                for (key, lit) in compared(a) {
                    let Some(Key {
                        values: Some(values),
                        ..
                    }) = schema.keys.get(&key)
                    else {
                        continue;
                    };
                    let ok = match &lit {
                        Compared::Exact(v) => {
                            let t = value_text(v);
                            values.contains_key(&t)
                                || path.is_some_and(|p| declared_values(p, &key).contains(&t))
                        }
                        Compared::Contains(needle) => {
                            let quoted = serde_json::Value::String(needle.clone()).to_string();
                            values.keys().any(|t| {
                                let decoded: Option<String> = serde_json::from_str(t).ok();
                                t.contains(&quoted)
                                    || decoded.is_some_and(|d| d.contains(needle.as_str()))
                            })
                        }
                    };
                    if !ok {
                        problems.push(format!("{key} never has the value {lit:?}"));
                    }
                }
            });
        }
        for p in problems {
            self.err(format!("layer {id}: {p}"));
        }
    }

    fn symbol(&mut self, id: &str, l: &Value, glyph_dir: Option<&str>, sprite: Option<&str>) {
        let layout = &l["layout"];
        if layout.get("text-field").is_some() {
            match layout["text-font"].as_array().map(Vec::as_slice) {
                Some([Value::String(font)]) => {
                    for p in self.packs {
                        let ok = glyph_dir
                            .and_then(|d| p.fonts.get(d))
                            .is_some_and(|f| f.contains(font));
                        if !ok {
                            self.err(format!(
                                "layer {id}: font {font} absent from the {} pack",
                                p.site
                            ));
                        }
                    }
                }
                _ => self.err(format!(
                    "layer {id}: text-font must be one constant font of the pack (the default \
                     font stack would be fetched from the network)"
                )),
            }
        }
        if let Some(icon) = layout.get("icon-image") {
            let mut names = Vec::new();
            string_outputs(icon, &mut names);
            for name in names {
                for p in self.packs {
                    let ok = sprite
                        .and_then(|s| p.sprites.get(s))
                        .is_some_and(|i| i.contains(&name));
                    if !ok {
                        self.err(format!(
                            "layer {id}: icon {name} absent from the {} sprite",
                            p.site
                        ));
                    }
                }
            }
        }
    }

    /// Rasters under every line, circle and label.
    fn order(&mut self, layers: &[Value]) {
        let last_raster = layers.iter().rposition(|l| l["type"] == "raster");
        let first_above = layers
            .iter()
            .position(|l| matches!(l["type"].as_str(), Some("line" | "symbol" | "circle")));
        if let (Some(r), Some(a)) = (last_raster, first_above)
            && r > a
        {
            self.err(format!(
                "raster layer {} drawn above {} (rasters go under lines and labels)",
                layers[r]["id"], layers[a]["id"]
            ));
        }
    }

    fn contrast(&mut self, layers: &[Value]) {
        let by_id: BTreeMap<&str, &Value> = layers
            .iter()
            .filter_map(|l| Some((l["id"].as_str()?, l)))
            .collect();
        for l in layers {
            let id = l["id"].as_str().unwrap_or("?");
            let against = l["metadata"]["skimap:contrast"].as_str();
            if let Some(against) = against {
                let Some(other) = by_id.get(against) else {
                    self.err(format!(
                        "layer {id}: contrast against unknown layer {against}"
                    ));
                    continue;
                };
                match (colour_cases(l), colour_cases(other)) {
                    (Some(fg), Some(bg)) => {
                        for (label, c) in &fg {
                            // The main colour of the other layer (its first colour).
                            let b = pick(&bg, label.as_ref())
                                .and_then(|v| v.first().copied())
                                .unwrap_or((0, 0, 0));
                            let ratio = c.iter().map(|c| contrast_ratio(*c, b)).fold(0.0, f64::max);
                            if ratio < MIN_GRAPHIC_CONTRAST {
                                self.err(format!(
                                    "layer {id}: contrast {ratio:.2} against {against} for {} (min {MIN_GRAPHIC_CONTRAST})",
                                    label.as_ref().map_or("default".into(), |v| v.to_string())
                                ));
                            }
                        }
                    }
                    _ => self.err(format!("layer {id}: colours not checkable (use #rrggbb)")),
                }
            } else if l["type"] == "symbol" && l["layout"].get("text-field").is_some() {
                let paint = &l["paint"];
                let halo_w = paint["text-halo-width"].as_f64().unwrap_or(0.0);
                if halo_w < MIN_HALO {
                    self.err(format!("layer {id}: text halo {halo_w} px < {MIN_HALO} px"));
                }
                let (Some(t), Some(h)) = (
                    colours(&paint["text-color"]),
                    colours(&paint["text-halo-color"]),
                ) else {
                    self.err(format!(
                        "layer {id}: text colours not checkable (use #rrggbb)"
                    ));
                    continue;
                };
                for tc in &t {
                    for hc in &h {
                        let r = contrast_ratio(*tc, *hc);
                        if r < MIN_TEXT_CONTRAST {
                            self.err(format!(
                                "layer {id}: text contrast {r:.2} < {MIN_TEXT_CONTRAST}"
                            ));
                        }
                    }
                }
            }
        }
    }
}

fn union(mut a: Layer, b: &Layer) -> Layer {
    a.minzoom = a.minzoom.min(b.minzoom);
    a.maxzoom = a.maxzoom.max(b.maxzoom);
    a.features += b.features;
    a.geometry.extend(b.geometry.iter().cloned());
    for (k, kb) in &b.keys {
        match a.keys.get_mut(k) {
            None => {
                a.keys.insert(k.clone(), kb.clone());
            }
            Some(ka) => {
                ka.values = match (ka.values.take(), &kb.values) {
                    (Some(mut x), Some(y)) => {
                        for (v, z) in y {
                            let e = x.entry(v.clone()).or_insert(*z);
                            *e = (*e).min(*z);
                        }
                        Some(x)
                    }
                    _ => None,
                };
            }
        }
    }
    a
}

/// Layer schema of a runtime source from the style metadata
/// (`{"geometry": "Point", "properties": {"label": "string", "part": ["a", "b"]}}`).
fn runtime_layer(v: &Value) -> Option<Layer> {
    let mut l = Layer {
        minzoom: 0,
        maxzoom: 24,
        ..Layer::default()
    };
    l.geometry.insert(v["geometry"].as_str()?.to_string());
    for (k, t) in v["properties"].as_object()? {
        let values = t.as_array().map(|vals| {
            vals.iter()
                .map(|x| (value_text(x), 0u8))
                .collect::<BTreeMap<_, _>>()
        });
        l.keys.insert(k.clone(), Key { values });
    }
    Some(l)
}

/// Expressions of a layer: filter, layout and paint values (not literal arrays).
pub(crate) fn expressions_of(l: &Value) -> Vec<&Value> {
    let mut out = Vec::new();
    if let Some(f) = l.get("filter") {
        out.push(f);
    }
    for group in ["layout", "paint"] {
        if let Some(m) = l[group].as_object() {
            for (k, v) in m {
                if !LITERAL_ARRAYS.contains(&k.as_str()) {
                    out.push(v);
                }
            }
        }
    }
    out
}

/// A literal compared with a property.
#[derive(Debug)]
pub(crate) enum Compared {
    /// `==`, `!=`, `match` label, `in` with a literal list.
    Exact(Value),
    /// `["in", "x", ["get", key]]`: item of an array or substring of a string.
    Contains(String),
}

/// Property read by an expression: `["get", k]` or `["coalesce", ["get", k], ...]`.
fn read_key(e: &Value) -> Option<String> {
    let a = e.as_array()?;
    match a.first()?.as_str()? {
        "get" => a.get(1)?.as_str().map(str::to_string),
        "coalesce" => read_key(a.get(1)?),
        _ => None,
    }
}

pub(crate) fn compared(a: &[Value]) -> Vec<(String, Compared)> {
    let op = a[0].as_str().unwrap_or("");
    let mut out = Vec::new();
    match op {
        "==" | "!=" => {
            if let (Some(k), Some(lit)) = (a.get(1).and_then(read_key), a.get(2))
                && !lit.is_array()
            {
                out.push((k, Compared::Exact(lit.clone())));
            }
        }
        "match" => {
            if let Some(k) = a.get(1).and_then(read_key) {
                let rest = &a[2..];
                for (i, x) in rest.iter().enumerate() {
                    if i % 2 == 0 && i + 1 < rest.len() {
                        match x {
                            Value::Array(labels) => {
                                out.extend(
                                    labels
                                        .iter()
                                        .map(|l| (k.clone(), Compared::Exact(l.clone()))),
                                );
                            }
                            l => out.push((k.clone(), Compared::Exact(l.clone()))),
                        }
                    }
                }
            }
        }
        "in" => {
            if let (Some(Value::String(needle)), Some(k)) = (a.get(1), a.get(2).and_then(read_key))
            {
                out.push((k, Compared::Contains(needle.clone())));
            }
        }
        _ => {}
    }
    // An empty string is the coalesce default of an absent property, not a data value.
    out.retain(|(_, c)| !matches!(c, Compared::Exact(Value::String(s)) if s.is_empty()));
    out
}

fn value_text(v: &Value) -> String {
    match v {
        Value::Number(n) => number_text(n.as_f64().unwrap_or(f64::NAN)),
        other => other.to_string(),
    }
}

/// Every string output of an expression (icon names).
fn string_outputs(e: &Value, out: &mut Vec<String>) {
    match e {
        Value::String(s) => out.push(s.clone()),
        Value::Array(a) => match a.first().and_then(Value::as_str) {
            Some("match") => {
                let rest = &a[2..];
                for (i, x) in rest.iter().enumerate() {
                    if !(i % 2 == 0 && i + 1 < rest.len()) {
                        string_outputs(x, out);
                    }
                }
            }
            Some("case") => {
                for (i, x) in a[1..].iter().enumerate() {
                    if i % 2 == 1 || i == a.len() - 2 {
                        string_outputs(x, out);
                    }
                }
            }
            _ => {}
        },
        _ => {}
    }
}

/// An sRGB colour.
pub type Rgb = (u8, u8, u8);

/// Parses `#rgb` or `#rrggbb`.
pub fn parse_colour(s: &str) -> Option<Rgb> {
    let h = s.strip_prefix('#')?;
    let x = |i: usize, n: usize| u8::from_str_radix(h.get(i..i + n)?, 16).ok();
    match h.len() {
        6 => Some((x(0, 2)?, x(2, 2)?, x(4, 2)?)),
        3 => Some((x(0, 1)? * 17, x(1, 1)? * 17, x(2, 1)? * 17)),
        _ => None,
    }
}

/// WCAG 2 contrast ratio of two colours.
pub fn contrast_ratio(a: Rgb, b: Rgb) -> f64 {
    let lum = |(r, g, b): Rgb| {
        let c = |v: u8| {
            let s = f64::from(v) / 255.0;
            if s <= 0.039_28 {
                s / 12.92
            } else {
                ((s + 0.055) / 1.055).powf(2.4)
            }
        };
        0.2126 * c(r) + 0.7152 * c(g) + 0.0722 * c(b)
    };
    let (x, y) = (lum(a), lum(b));
    (x.max(y) + 0.05) / (x.min(y) + 0.05)
}

/// Colours a property can take (constant or `match` outputs).
fn colours(v: &Value) -> Option<Vec<Rgb>> {
    Some(match_cases(v)?.into_iter().map(|(_, c)| c).collect())
}

/// `(match label, colour)` of a colour property; `None` label for the default.
fn match_cases(v: &Value) -> Option<Vec<(Option<Value>, Rgb)>> {
    match v {
        Value::String(s) => Some(vec![(None, parse_colour(s)?)]),
        Value::Array(a) if a.first() == Some(&Value::from("match")) => {
            let rest = &a[2..];
            let mut out = Vec::new();
            for (i, x) in rest.iter().enumerate() {
                if i % 2 == 1 {
                    let labels = match &rest[i - 1] {
                        Value::Array(ls) => ls.clone(),
                        l => vec![l.clone()],
                    };
                    let c = parse_colour(x.as_str()?)?;
                    out.extend(labels.into_iter().map(|l| (Some(l), c)));
                } else if i == rest.len() - 1 {
                    out.push((None, parse_colour(x.as_str()?)?));
                }
            }
            Some(out)
        }
        _ => None,
    }
}

/// Colour cases of a layer: its main colour, and for circles the stroke too (either may
/// carry the contrast: a blue dot ringed with white still stands out on snow).
fn colour_cases(l: &Value) -> Option<Vec<(Option<Value>, Vec<Rgb>)>> {
    let paint = &l["paint"];
    let main = match l["type"].as_str()? {
        "line" => "line-color",
        "fill" => "fill-color",
        "circle" => "circle-color",
        "background" => "background-color",
        "symbol" => "text-color",
        _ => return None,
    };
    let mut cases: Vec<(Option<Value>, Vec<Rgb>)> = match_cases(&paint[main])?
        .into_iter()
        .map(|(l, c)| (l, vec![c]))
        .collect();
    if l["type"] == "circle" && paint["circle-stroke-width"].as_f64().unwrap_or(0.0) >= MIN_HALO {
        let stroke = match_cases(&paint["circle-stroke-color"])?;
        for (label, cs) in &mut cases {
            cs.extend(pick(&stroke, label.as_ref()));
        }
    }
    Some(cases)
}

/// The case of `label` (else the default, else the first case).
fn pick<T: Clone>(cases: &[(Option<Value>, T)], label: Option<&Value>) -> Option<T> {
    let find = |want: Option<&Value>| cases.iter().find(|(l, _)| l.as_ref() == want);
    let hit = label
        .and_then(|l| find(Some(l)))
        .or_else(|| find(None))
        .or(cases.first());
    hit.map(|(_, c)| c.clone())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wcag_ratios() {
        let r = contrast_ratio((0, 0, 0), (255, 255, 255));
        assert!((r - 21.0).abs() < 1e-9);
        assert!((contrast_ratio((0x77, 0x77, 0x77), (255, 255, 255)) - 4.48).abs() < 0.01);
        assert_eq!(parse_colour("#0a6cff"), Some((10, 108, 255)));
        assert_eq!(parse_colour("#fff"), Some((255, 255, 255)));
        assert_eq!(parse_colour("rgb(1,2,3)"), None);
    }
}
