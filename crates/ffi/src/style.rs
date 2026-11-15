//! MapLibre style of a pack: basemap style with absolute URLs, pack layers on top.
//!
//! The styles of the pack use URLs relative to the pack root (`pmtiles://basemap/x.pmtiles`,
//! `basemap/x/fonts/{fontstack}/{range}.pbf`, crate `pack`, `docs/format.md`). MapLibre
//! Native resolves nothing against the style location: every URL is rewritten to an
//! absolute `file://` URL (`pmtiles://file://…` for archives), after checking that it stays
//! inside the pack.

use std::path::Path;

use geojson::{GeometryValue, Position};
use serde_json::{Map, Value, json};
use skimap_pack::Pack;
use skimap_pack::manifest::{Basemap, Layer, LayerKind};
use skimap_pack::reader::check_relative;

use crate::PackError;
use crate::symbols::{
    ARROW_SYMBOL, CASING_COLOR, CLOSED_COLOR, CLOSED_SYMBOL, DIFFICULTIES, NO_NETWORK_COLOR,
    OPEN_COLOR, OPEN_SYMBOL, REFUGE_COLOR, REFUGE_SYMBOL, UNKNOWN_COLOR, UNKNOWN_SYMBOL,
    WATER_SYMBOL, difficulty_symbol,
};

/// Pack layers drawn on the map, bottom to top.
pub const OVERLAY_LAYERS: [&str; 6] = [
    "no-network",
    "status",
    "pistes",
    "refuges-camptocamp",
    "refuges-osm",
    "refuges-info",
];

/// Background of a pack without basemap: snow white.
pub const BLANK_BACKGROUND: &str = "#f7f9fb";

/// Fonts used for the overlay labels, best first; the first one present in the glyphs of
/// the basemap is used (Plan IGN: Source Sans Pro, Protomaps: Noto Sans).
const FONT_CANDIDATES: [&str; 6] = [
    "Source Sans Pro Bold",
    "Noto Sans Medium",
    "Source Sans Pro Semibold",
    "Noto Sans Regular",
    "Source Sans Pro Regular",
    "Source Sans Pro",
];

/// Prefix of the ids of the style layers and sources added for the pack layers.
pub const OVERLAY_PREFIX: &str = "skimap-";

/// A MapLibre style ready to load.
#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct MapStyle {
    /// Style JSON (version 8).
    pub json: String,
    /// Basemap used, `None` for the plain background.
    pub basemap: Option<String>,
    /// Font of the overlay labels, `None` when the basemap has no glyphs (no labels).
    pub font: Option<String>,
    /// Style layers drawn for each pack layer.
    pub overlays: Vec<OverlayLayers>,
    /// Attributions of what this style shows (basemap and pack layers), deduplicated.
    pub attributions: Vec<String>,
    /// Short form of [`MapStyle::attributions`] for the map footer.
    pub attribution_short: String,
    /// Secondary elements the user may hide (panel "Couches"), switched by layer
    /// visibility without reloading the style. Pistes, their difficulty, direction and
    /// status are not listed: they are never hidden.
    pub toggles: Vec<LayerToggle>,
}

/// A category of secondary map elements, shown or hidden as a whole.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct LayerToggle {
    /// Stable identifier (`refuges`, `water`, `place-names`, `no-network`), stored with
    /// the user's choice.
    pub id: String,
    /// Label, French.
    pub label: String,
    /// Style layers switched together.
    pub style_layers: Vec<String>,
    /// Visibility when the user has not chosen yet.
    pub visible_by_default: bool,
}

/// Style layers of one pack layer.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct OverlayLayers {
    /// Pack layer identifier.
    pub pack_layer: String,
    /// Ids of the style layers that draw it.
    pub style_layers: Vec<String>,
}

/// Builds the style of `pack` with `basemap` (`None`: the first basemap of the pack).
/// `labels`: the piste name points ([`piste_labels`]), computed once per opened pack.
pub fn compose(
    pack: &Pack,
    basemap: Option<&str>,
    labels: Option<&Value>,
) -> Result<MapStyle, PackError> {
    compose_set(&[(pack, labels)], basemap, &pack.manifest().site.id)
}

/// Separator of the suffix that keeps the sources and layers of several packs apart
/// (`skimap-pistes-line~p1`); the first pack has none.
const PACK_SUFFIX: char = '~';

fn suffix(k: usize) -> String {
    if k == 0 {
        String::new()
    } else {
        format!("{PACK_SUFFIX}p{k}")
    }
}

/// Layer or source id without the pack suffix.
fn base_id(id: &str) -> &str {
    id.split(PACK_SUFFIX).next().unwrap_or(id)
}

/// Style of several packs at once (free mode: everything installed in the frame), or of
/// one. `basemap`: the basemap of each pack that has it (`None`: the first basemap of the
/// first pack); `name_id` names the style (`skimap <name_id> <basemap>`).
pub fn compose_set(
    parts: &[(&Pack, Option<&Value>)],
    basemap: Option<&str>,
    name_id: &str,
) -> Result<MapStyle, PackError> {
    let (first, _) = parts.first().ok_or_else(|| invalid("no pack to show"))?;
    let base_id_wanted: Option<String> = match basemap {
        Some(id) => {
            if !parts
                .iter()
                .any(|(p, _)| p.manifest().basemap(id).is_some())
            {
                return Err(PackError::UnknownBasemap { id: id.to_owned() });
            }
            Some(id.to_owned())
        }
        None => first.manifest().basemaps.first().map(|b| b.id.clone()),
    };
    let mut attributions: Vec<String> = Vec::new();
    let mut add_attr = |a: &str| {
        if !a.is_empty() && !attributions.iter().any(|x| x == a) {
            attributions.push(a.to_owned());
        }
    };

    // Basemap: the style of the first pack that has it, then the sources and layers of the
    // others (suffixed; their background is left out, it would hide the first one).
    let mut style: Option<Value> = None;
    let mut font: Option<String> = None;
    for (k, (pack, _)) in parts.iter().enumerate() {
        let Some(b) = base_id_wanted
            .as_deref()
            .and_then(|id| pack.manifest().basemap(id))
        else {
            continue;
        };
        b.sources.iter().for_each(|s| add_attr(&s.attribution));
        let (s, glyphs_rel) = basemap_style(pack, b)?;
        match style.as_mut() {
            None => {
                font = glyphs_rel
                    .as_deref()
                    .and_then(|g| pick_font(pack.root(), g));
                style = Some(s);
            }
            Some(st) => merge_basemap(st, s, &suffix(k))?,
        }
    }
    let based = style.is_some();
    let mut style = style.unwrap_or_else(blank_style);
    // Where no pack has tiles (between the areas of the free mode, or beyond a pack at low
    // zoom) the map shows snow white, not the black of an empty frame.
    if let Some(layers) = style.get_mut("layers").and_then(Value::as_array_mut)
        && !layers.iter().any(|l| l["type"] == "background")
    {
        layers.insert(
            0,
            json!({"id": "skimap-background", "type": "background",
                   "paint": {"background-color": BLANK_BACKGROUND}}),
        );
    }

    // Basemap symbol layers (place names, peaks, basemap icons): one switch.
    let basemap_symbols: Vec<String> = style["layers"]
        .as_array()
        .map(|ls| {
            ls.iter()
                .filter(|l| l["type"] == "symbol")
                .filter_map(|l| l["id"].as_str().map(str::to_owned))
                .collect()
        })
        .unwrap_or_default();
    let mut overlays: Vec<OverlayLayers> = Vec::new();
    let mut new_layers: Vec<Value> = Vec::new();
    let sources = style
        .get_mut("sources")
        .and_then(Value::as_object_mut)
        .ok_or_else(|| invalid("style without sources object"))?;
    for (k, (pack, labels)) in parts.iter().enumerate() {
        let m = pack.manifest();
        let sfx = suffix(k);
        for id in OVERLAY_LAYERS {
            let Some(l) = m
                .layer(id)
                .filter(|l| l.kind == LayerKind::Vector && l.format == "geojson")
            else {
                continue;
            };
            l.sources.iter().for_each(|s| add_attr(&s.attribution));
            let source_id = format!("{OVERLAY_PREFIX}{id}{sfx}");
            sources.insert(
                source_id.clone(),
                json!({"type": "geojson", "data": file_url(&pack.path(&l.file)?)}),
            );
            // Piste names: one point per named piste (computed once per pack: OSM splits
            // pistes into ways too short to carry a label along the line).
            let label_source = match labels {
                Some(data) if id == "pistes" && font.is_some() => {
                    let label_id = format!("{OVERLAY_PREFIX}{id}-labels{sfx}");
                    sources.insert(label_id.clone(), json!({"type": "geojson", "data": data}));
                    Some(label_id)
                }
                _ => None,
            };
            let mut layers =
                overlay_layers(l, &source_id, label_source.as_deref(), font.as_deref());
            for x in &mut layers {
                if let Some(Value::String(lid)) = x.get_mut("id") {
                    lid.push_str(&sfx);
                }
            }
            let ids = layers
                .iter()
                .filter_map(|x| x["id"].as_str().map(str::to_owned));
            match overlays.iter_mut().find(|o| o.pack_layer == id) {
                Some(o) => o.style_layers.extend(ids),
                None => overlays.push(OverlayLayers {
                    pack_layer: id.to_owned(),
                    style_layers: ids.collect(),
                }),
            }
            new_layers.extend(layers);
        }
    }
    // Lines first (status halo under the pistes), then the symbols of status and pistes,
    // then the refuges, so that piste labels and refuges stay readable above the lines.
    let rank = |x: &Value| {
        if is_refuge_layer(x) {
            2
        } else if x["type"] == "symbol" {
            1
        } else {
            0
        }
    };
    new_layers.sort_by_key(rank);
    let layers = style
        .get_mut("layers")
        .and_then(Value::as_array_mut)
        .ok_or_else(|| invalid("style without layers array"))?;
    layers.extend(new_layers);

    let basemap_used = base_id_wanted.filter(|_| based);
    if let Some(obj) = style.as_object_mut() {
        obj.insert(
            "name".to_owned(),
            Value::String(format!(
                "skimap {name_id} {}",
                basemap_used.as_deref().unwrap_or("blank")
            )),
        );
    }
    let attribution_short = short_attribution(&attributions);
    let toggles = toggles(&overlays, basemap_symbols);
    Ok(MapStyle {
        json: serde_json::to_string(&style).map_err(|e| invalid(&e.to_string()))?,
        basemap: basemap_used,
        font,
        overlays,
        attributions,
        attribution_short,
        toggles,
    })
}

/// Adds the sources and layers of another pack's basemap style to `into`, suffixed.
fn merge_basemap(into: &mut Value, other: Value, sfx: &str) -> Result<(), PackError> {
    let Value::Object(mut other) = other else {
        return Err(invalid("basemap style is not an object"));
    };
    if let (Some(Value::Object(dst)), Some(Value::Object(src))) =
        (into.get_mut("sources"), other.remove("sources"))
    {
        for (id, s) in src {
            dst.insert(format!("{id}{sfx}"), s);
        }
    }
    if let (Some(Value::Array(dst)), Some(Value::Array(src))) =
        (into.get_mut("layers"), other.remove("layers"))
    {
        for mut l in src {
            if l["type"] == "background" {
                continue;
            }
            if let Some(Value::String(id)) = l.get_mut("id") {
                id.push_str(sfx);
            }
            if let Some(Value::String(s)) = l.get_mut("source") {
                s.push_str(sfx);
            }
            dst.push(l);
        }
    }
    Ok(())
}

/// The switches of the panel "Couches", for the layers this style has.
fn toggles(overlays: &[OverlayLayers], basemap_symbols: Vec<String>) -> Vec<LayerToggle> {
    let ids = |pred: &dyn Fn(&str) -> bool| -> Vec<String> {
        overlays
            .iter()
            .flat_map(|o| o.style_layers.iter())
            .filter(|l| pred(base_id(l)))
            .cloned()
            .collect()
    };
    let mut v = Vec::new();
    let mut push = |id: &str, label: &str, style_layers: Vec<String>, visible: bool| {
        if !style_layers.is_empty() {
            v.push(LayerToggle {
                id: id.to_owned(),
                label: label.to_owned(),
                style_layers,
                visible_by_default: visible,
            });
        }
    };
    push(
        "refuges",
        "Refuges, cabanes et abris",
        ids(&|l| l.starts_with("skimap-refuges-") && l.ends_with("-icon")),
        true,
    );
    push(
        "water",
        "Points d'eau",
        ids(&|l| l == "skimap-refuges-info-water"),
        true,
    );
    push(
        "no-network",
        "Zones sans réseau mobile (112 compris)",
        ids(&|l| l.starts_with("skimap-no-network-")),
        false,
    );
    push(
        "place-names",
        "Noms de lieux et symboles du fond",
        basemap_symbols,
        true,
    );
    v
}

fn invalid(message: &str) -> PackError {
    PackError::Invalid {
        detail: message.to_owned(),
    }
}

fn is_refuge_layer(x: &Value) -> bool {
    x["source"]
        .as_str()
        .is_some_and(|s| s.starts_with("skimap-refuges-"))
}

fn blank_style() -> Value {
    json!({
        "version": 8,
        "sources": {},
        "layers": [{"id": "skimap-background", "type": "background",
                    "paint": {"background-color": BLANK_BACKGROUND}}]
    })
}

/// Basemap style with absolute URLs, and its relative glyphs template if any.
fn basemap_style(pack: &Pack, b: &Basemap) -> Result<(Value, Option<String>), PackError> {
    let bytes = pack.read_file(&b.style)?;
    let mut style: Value =
        serde_json::from_slice(&bytes).map_err(|e| invalid(&format!("style {}: {e}", b.style)))?;
    let obj = style
        .as_object_mut()
        .ok_or_else(|| invalid(&format!("style {} is not an object", b.style)))?;
    let root = pack.root();
    let mut glyphs_rel = None;
    if let Some(Value::String(g)) = obj.get_mut("glyphs") {
        if !has_scheme(g) {
            glyphs_rel = Some(g.clone());
        }
        absolutize(root, g)?;
    }
    match obj.get_mut("sprite") {
        Some(Value::String(s)) => absolutize(root, s)?,
        Some(Value::Array(list)) => {
            for e in list {
                if let Some(Value::String(u)) = e.get_mut("url") {
                    absolutize(root, u)?;
                }
            }
        }
        _ => {}
    }
    if let Some(Value::Object(sources)) = obj.get_mut("sources") {
        for src in sources.values_mut() {
            let Some(src) = src.as_object_mut() else {
                continue;
            };
            rewrite_source(root, src)?;
        }
    }
    Ok((style, glyphs_rel))
}

fn rewrite_source(root: &Path, src: &mut Map<String, Value>) -> Result<(), PackError> {
    if let Some(Value::String(u)) = src.get_mut("url") {
        if let Some(rest) = u.strip_prefix("pmtiles://") {
            let mut rest = rest.to_owned();
            absolutize(root, &mut rest)?;
            *u = format!("pmtiles://{rest}");
        } else {
            absolutize(root, u)?;
        }
    }
    if let Some(Value::Array(tiles)) = src.get_mut("tiles") {
        for t in tiles {
            if let Value::String(t) = t {
                absolutize(root, t)?;
            }
        }
    }
    if let Some(Value::String(d)) = src.get_mut("data") {
        absolutize(root, d)?;
    }
    Ok(())
}

/// Whether `s` starts with a URL scheme (`file://`, `https://`, `asset://`...).
fn has_scheme(s: &str) -> bool {
    let Some((scheme, _)) = s.split_once("://") else {
        return false;
    };
    let mut c = scheme.chars();
    c.next().is_some_and(|f| f.is_ascii_alphabetic())
        && c.all(|x| x.is_ascii_alphanumeric() || matches!(x, '+' | '-' | '.'))
}

/// Rewrites a pack-relative URL in place to an absolute `file://` URL; URLs with a scheme
/// are left as they are. Fails on a path that leaves the pack.
fn absolutize(root: &Path, url: &mut String) -> Result<(), PackError> {
    if has_scheme(url) {
        return Ok(());
    }
    check_relative(url)?;
    *url = file_url(&root.join(url.as_str()));
    Ok(())
}

/// `file://` URL of an absolute path. Bytes outside the unreserved set are
/// percent-encoded, except `/` and the `{` `}` of MapLibre URL templates.
pub fn file_url(path: &Path) -> String {
    let s = path.to_string_lossy();
    let mut out = String::with_capacity(s.len() + 8);
    out.push_str("file://");
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'/' | b'-' | b'.' | b'_' | b'~' | b'{' | b'}')
        {
            out.push(char::from(b));
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

/// First font of [`FONT_CANDIDATES`] whose first glyph range is in the pack.
fn pick_font(root: &Path, glyphs_rel: &str) -> Option<String> {
    let (prefix, _) = glyphs_rel.split_once("{fontstack}")?;
    FONT_CANDIDATES
        .iter()
        .find(|f| root.join(format!("{prefix}{f}/0-255.pbf")).is_file())
        .map(|f| (*f).to_owned())
}

/// `["interpolate", ["linear"], ["zoom"], 10, a, 16, b]`.
fn by_zoom(a: f64, b: f64) -> Value {
    json!(["interpolate", ["linear"], ["zoom"], 10, a, 16, b])
}

fn difficulty_color() -> Value {
    let mut e = vec![json!("match"), json!(["get", "difficulty"])];
    for d in &DIFFICULTIES {
        e.push(json!(d.value));
        e.push(json!(d.color));
    }
    e.push(json!(UNKNOWN_COLOR));
    Value::Array(e)
}

fn difficulty_icon() -> Value {
    let mut e = vec![json!("match"), json!(["get", "difficulty"])];
    for d in &DIFFICULTIES {
        e.push(json!(d.value));
        e.push(json!(difficulty_symbol(d.value)));
    }
    e.push(json!(UNKNOWN_SYMBOL));
    Value::Array(e)
}

fn label(font: &str, field: Value, size: Value) -> Map<String, Value> {
    let mut layout = Map::new();
    layout.insert("text-field".into(), field);
    layout.insert("text-font".into(), json!([font]));
    layout.insert("text-size".into(), size);
    layout
}

/// Style layers drawing one pack layer (`labels`: source of the piste name points).
fn overlay_layers(l: &Layer, source: &str, labels: Option<&str>, font: Option<&str>) -> Vec<Value> {
    let id = l.id.as_str();
    let lid = |s: &str| format!("{OVERLAY_PREFIX}{id}-{s}");
    let mut v = Vec::new();
    match id {
        "no-network" => {
            v.push(json!({"id": lid("fill"), "type": "fill", "source": source,
                "paint": {"fill-color": NO_NETWORK_COLOR, "fill-opacity": 0.16}}));
            v.push(json!({"id": lid("line"), "type": "line", "source": source,
                "paint": {"line-color": NO_NETWORK_COLOR, "line-width": 1.5, "line-dasharray": [3, 2]}}));
        }
        "status" => {
            let state = json!(["get", "state"]);
            v.push(json!({"id": lid("halo"), "type": "line", "source": source,
                "filter": ["in", state.clone(), ["literal", ["open", "closed"]]],
                "layout": {"line-cap": "round", "line-join": "round"},
                "paint": {"line-color": ["match", state.clone(), "open", OPEN_COLOR, CLOSED_COLOR],
                          "line-width": by_zoom(7.0, 18.0), "line-opacity": 0.3}}));
            v.push(
                json!({"id": lid("icons"), "type": "symbol", "source": source, "minzoom": 11,
                "filter": ["in", state.clone(), ["literal", ["open", "closed"]]],
                "layout": {"symbol-placement": "line", "symbol-spacing": 320,
                           "icon-image": ["match", state, "open", OPEN_SYMBOL, CLOSED_SYMBOL],
                           "icon-rotation-alignment": "viewport", "icon-padding": 4}}),
            );
        }
        "pistes" => {
            let nordic = json!(["in", "nordic", ["get", "uses"]]);
            let walk = json!(["!", nordic.clone()]);
            let round = json!({"line-cap": "round", "line-join": "round"});
            v.push(json!({"id": lid("casing"), "type": "line", "source": source, "filter": nordic,
                "layout": round, "paint": {"line-color": CASING_COLOR, "line-width": by_zoom(4.5, 12.0)}}));
            v.push(json!({"id": lid("line"), "type": "line", "source": source, "filter": nordic,
                "layout": round, "paint": {"line-color": difficulty_color(), "line-width": by_zoom(2.5, 6.5)}}));
            v.push(json!({"id": lid("walk-casing"), "type": "line", "source": source, "filter": walk,
                "layout": round, "paint": {"line-color": CASING_COLOR, "line-width": by_zoom(3.0, 8.0)}}));
            v.push(
                json!({"id": lid("walk"), "type": "line", "source": source, "filter": walk,
                "paint": {"line-color": difficulty_color(), "line-width": by_zoom(1.5, 4.0),
                          "line-dasharray": [2, 1.5]}}),
            );
            v.push(json!({"id": lid("oneway"), "type": "symbol", "source": source, "minzoom": 12,
                "filter": ["==", ["get", "oneway"], true],
                "layout": {"symbol-placement": "line", "symbol-spacing": 90, "icon-image": ARROW_SYMBOL,
                           "icon-rotation-alignment": "map", "icon-keep-upright": false,
                           "icon-allow-overlap": true, "icon-ignore-placement": true}}));
            v.push(json!({"id": lid("difficulty"), "type": "symbol", "source": source, "minzoom": 11.5,
                "layout": {"symbol-placement": "line", "symbol-spacing": 260, "icon-image": difficulty_icon(),
                           "icon-rotation-alignment": "viewport", "icon-padding": 2}}));
            if let (Some(font), Some(labels)) = (font, labels) {
                // Name next to the shape of the difficulty: never colour alone.
                let mut layout = label(font, json!(["get", "name"]), by_zoom(14.0, 19.0));
                layout.insert("icon-image".into(), difficulty_icon());
                layout.insert("text-anchor".into(), json!("left"));
                layout.insert("text-offset".into(), json!([0.9, 0]));
                layout.insert("text-optional".into(), json!(true));
                layout.insert("text-padding".into(), json!(4));
                layout.insert("text-max-width".into(), json!(12));
                v.push(
                    json!({"id": lid("label"), "type": "symbol", "source": labels, "minzoom": 12,
                    "layout": layout,
                    "paint": {"text-color": difficulty_color(), "text-halo-color": CASING_COLOR,
                              "text-halo-width": 2.5}}),
                );
            }
        }
        "refuges-info" | "refuges-osm" | "refuges-camptocamp" => {
            let (filter, name) = match id {
                "refuges-info" => (
                    json!([
                        "!",
                        [
                            "in",
                            ["coalesce", ["get", "type_label"], ""],
                            ["literal", ["point d'eau", "passage délicat"]]
                        ]
                    ]),
                    json!(["coalesce", ["get", "name"], ""]),
                ),
                "refuges-osm" => (
                    json!([
                        "in",
                        ["coalesce", ["get", "kind"], ""],
                        ["literal", ["alpine_hut", "wilderness_hut", "shelter"]]
                    ]),
                    json!(["coalesce", ["get", "name", ["get", "tags"]], ""]),
                ),
                _ => (
                    json!([
                        "in",
                        ["coalesce", ["get", "waypoint_type"], ""],
                        ["literal", ["hut", "gite", "shelter", "bivouac"]]
                    ]),
                    json!(["coalesce", ["get", "title"], ""]),
                ),
            };
            let mut layout = Map::new();
            layout.insert("icon-image".into(), json!(REFUGE_SYMBOL));
            layout.insert("icon-padding".into(), json!(2));
            if let Some(font) = font {
                // Names from z12.5 only: an empty text below, so no halo is drawn either.
                let name = json!(["step", ["zoom"], "", 12.5, name]);
                layout.extend(label(font, name, by_zoom(13.0, 17.0)));
                layout.insert("text-anchor".into(), json!("top"));
                layout.insert("text-offset".into(), json!([0, 1.2]));
                layout.insert("text-optional".into(), json!(true));
                layout.insert("text-max-width".into(), json!(9));
            }
            let mut paint = Map::new();
            if font.is_some() {
                paint.insert("text-color".into(), json!(REFUGE_COLOR));
                paint.insert("text-halo-color".into(), json!(CASING_COLOR));
                paint.insert("text-halo-width".into(), json!(2.5));
            }
            v.push(
                json!({"id": lid("icon"), "type": "symbol", "source": source, "minzoom": 10,
                "filter": filter, "layout": layout, "paint": paint}),
            );
            if id == "refuges-info" {
                v.push(
                    json!({"id": lid("water"), "type": "symbol", "source": source, "minzoom": 12,
                    "filter": ["==", ["get", "type_label"], "point d'eau"],
                    "layout": {"icon-image": WATER_SYMBOL, "icon-padding": 2}}),
                );
            }
        }
        _ => {}
    }
    v
}

/// Distance between two `[lon, lat]` points in metres (haversine).
fn distance_m(a: [f64; 2], b: [f64; 2]) -> f64 {
    const R: f64 = 6_371_008.8;
    let (la1, la2) = (a[1].to_radians(), b[1].to_radians());
    let dla = la2 - la1;
    let dlo = (b[0] - a[0]).to_radians();
    let h = (dla / 2.0).sin().powi(2) + la1.cos() * la2.cos() * (dlo / 2.0).sin().powi(2);
    2.0 * R * h.sqrt().asin()
}

/// Length of a line and its point halfway along it.
fn length_and_middle(line: &[[f64; 2]]) -> Option<(f64, [f64; 2])> {
    let total: f64 = line.windows(2).map(|w| distance_m(w[0], w[1])).sum();
    let mut left = total / 2.0;
    for w in line.windows(2) {
        let d = distance_m(w[0], w[1]);
        if d >= left && d > 0.0 {
            let t = left / d;
            let p = [
                w[0][0] + t * (w[1][0] - w[0][0]),
                w[0][1] + t * (w[1][1] - w[0][1]),
            ];
            return Some((total, p));
        }
        left -= d;
    }
    line.first().map(|p| (total, *p))
}

/// Name and difficulty of a piste label.
type LabelKey = (String, String);
/// Length of a way and its middle point.
type WayMiddle = (f64, [f64; 2]);

/// Two labels of the same piste are at least this far apart.
const LABEL_SPACING_M: f64 = 1_500.0;

/// Label points of the pistes: for each name and difficulty, the middle of its longest
/// way, then of every other way farther than 1.5 km from the labels kept.
pub fn piste_labels(pack: &Pack) -> Result<Value, PackError> {
    let fc = pack.read_geojson("pistes")?;
    // (name, difficulty) -> [(length, middle)]
    let mut groups: Vec<(LabelKey, Vec<WayMiddle>)> = Vec::new();
    for f in &fc.features {
        let Some(props) = f.properties.as_ref() else {
            continue;
        };
        let name = ["name", "way_name"]
            .iter()
            .find_map(|k| props.get(*k).and_then(Value::as_str))
            .map(str::trim)
            .filter(|n| !n.is_empty());
        let Some(name) = name else { continue };
        let difficulty = props
            .get("difficulty")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_owned();
        let lines: Vec<Vec<[f64; 2]>> = match f.geometry.as_ref().map(|g| &g.value) {
            Some(GeometryValue::LineString { coordinates }) => vec![to_points(coordinates)],
            Some(GeometryValue::MultiLineString { coordinates }) => {
                coordinates.iter().map(|l| to_points(l)).collect()
            }
            _ => continue,
        };
        let key = (name.to_owned(), difficulty);
        let pos = match groups.iter().position(|(k, _)| *k == key) {
            Some(i) => i,
            None => {
                groups.push((key, Vec::new()));
                groups.len() - 1
            }
        };
        groups[pos]
            .1
            .extend(lines.iter().filter_map(|l| length_and_middle(l)));
    }
    let mut features = Vec::new();
    for ((name, difficulty), mut ways) in groups {
        ways.sort_by(|a, b| b.0.total_cmp(&a.0));
        let mut kept: Vec<[f64; 2]> = Vec::new();
        for (_, p) in ways {
            if kept.iter().all(|k| distance_m(*k, p) > LABEL_SPACING_M) {
                kept.push(p);
                features.push(json!({"type": "Feature",
                    "properties": {"name": name, "difficulty": difficulty},
                    "geometry": {"type": "Point", "coordinates": [p[0], p[1]]}}));
            }
        }
    }
    Ok(json!({"type": "FeatureCollection", "features": features}))
}

fn to_points(l: &[Position]) -> Vec<[f64; 2]> {
    l.iter()
        .map(Position::as_slice)
        .filter(|p| p.len() >= 2)
        .map(|p| [p[0], p[1]])
        .collect()
}

/// `"© IGN – Plan IGN v2, mise à jour…"` → `"© IGN – Plan IGN v2"`,
/// `"© OSM (ODbL), extrait…"` → `"© OSM"`, deduplicated. The full text (licences) is in
/// [`MapStyle::attributions`].
fn short_attribution(full: &[String]) -> String {
    let mut parts: Vec<&str> = Vec::new();
    for a in full {
        let s = a.split(',').next().unwrap_or(a);
        let s = s.split(" (").next().unwrap_or(s).trim();
        if !s.is_empty() && !parts.contains(&s) {
            parts.push(s);
        }
    }
    parts.join(" · ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn schemes() {
        assert!(has_scheme("file:///a"));
        assert!(has_scheme("https://x"));
        assert!(has_scheme("asset://x"));
        assert!(!has_scheme("basemap/x.pmtiles"));
        assert!(!has_scheme("://x"));
        assert!(!has_scheme("1a://x"));
    }

    #[test]
    fn file_urls_keep_templates_and_encode_the_rest() {
        assert_eq!(
            file_url(Path::new("/data/p/basemap/f/{fontstack}/{range}.pbf")),
            "file:///data/p/basemap/f/{fontstack}/{range}.pbf"
        );
        assert_eq!(
            file_url(Path::new("/a b/Cervières")),
            "file:///a%20b/Cervi%C3%A8res"
        );
    }

    #[test]
    fn relative_urls_become_absolute_and_stay_inside() {
        let root = Path::new("/r");
        let mut u = "basemap/osm.pmtiles".to_owned();
        absolutize(root, &mut u).unwrap();
        assert_eq!(u, "file:///r/basemap/osm.pmtiles");
        let mut keep = "https://example.org/x".to_owned();
        absolutize(root, &mut keep).unwrap();
        assert_eq!(keep, "https://example.org/x");
        for bad in ["../x", "/etc/passwd", "a/../../b"] {
            let mut b = bad.to_owned();
            assert!(absolutize(root, &mut b).is_err(), "{bad}");
        }
        let mut src = Map::new();
        src.insert("url".into(), json!("pmtiles://basemap/a.pmtiles"));
        src.insert("tiles".into(), json!(["t/{z}/{x}/{y}.png"]));
        rewrite_source(root, &mut src).unwrap();
        assert_eq!(src["url"], "pmtiles://file:///r/basemap/a.pmtiles");
        assert_eq!(src["tiles"][0], "file:///r/t/{z}/{x}/{y}.png");
        let mut evil = Map::new();
        evil.insert("url".into(), json!("pmtiles://../../a.pmtiles"));
        assert!(rewrite_source(root, &mut evil).is_err());
    }

    #[test]
    fn middles_and_lengths() {
        // 0.01° of latitude is 1111.95 m.
        let (len, mid) = length_and_middle(&[[6.0, 45.0], [6.0, 45.01], [6.0, 45.03]]).unwrap();
        assert!((len - 3335.85).abs() < 0.1, "{len}");
        assert!((mid[1] - 45.015).abs() < 1e-9 && mid[0] == 6.0, "{mid:?}");
        assert!((distance_m([6.0, 45.0], [6.0, 45.01]) - 1111.95).abs() < 0.01);
        assert_eq!(length_and_middle(&[]), None);
    }

    #[test]
    fn short_attributions() {
        let full = vec![
            "© IGN – Plan IGN v2, mise à jour du 2026-10-08".to_owned(),
            "© OSM (ODbL), extrait".to_owned(),
            "© OSM".to_owned(),
            "Nordic France".to_owned(),
        ];
        assert_eq!(
            short_attribution(&full),
            "© IGN – Plan IGN v2 · © OSM · Nordic France"
        );
    }
}
