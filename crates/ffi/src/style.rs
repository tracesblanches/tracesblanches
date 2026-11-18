//! Map styles of the app: the styles validated by the user (`styles/`, K15 #123), loaded
//! as they are and only *instantiated* on the packs shown.
//!
//! The styles use URLs relative to a pack root (`pmtiles://basemap/plan-ign.pmtiles`,
//! `layers/pistes.geojson`, `basemap/plan-ign/fonts/{fontstack}/{range}.pbf`). MapLibre
//! Native resolves nothing against the style location, so every URL is rewritten to an
//! absolute `file://` URL (`pmtiles://file://…` for archives) after checking that it stays
//! inside the pack. Nothing of the drawing is decided here: colours, widths, filters,
//! zooms and the order of the layers are those of the style file.
//!
//! Several packs (free mode, H2 #133) make one style: each source of the style is
//! repeated once per pack that has its file (ids suffixed `~p1`, `~p2`… from the second
//! pack), and each layer is repeated right after itself for every pack. The order of the
//! style is kept *across* packs (every forest under every piste, every piste under every
//! label), so adjacent areas join without a seam. Sources filled by the app at run time
//! (`metadata.skimap:runtime_sources`: position, route, decision) and the background are
//! kept once.
//!
//! The France basemap (X20, #154) is a shared source (`metadata.skimap:shared_sources`,
//! URL `pmtiles://france/france.pmtiles`): resolved once against the France basemap of the
//! phone cache ([`France`]), drawn under every pack, and left out with its layers when the
//! phone has none. With no pack at all (free mode before any download) the style shows it
//! alone, with its own glyphs.

use std::collections::{BTreeSet, HashMap};
use std::path::Path;

use serde_json::{Map, Value};
use skimap_pack::Pack;
use skimap_pack::reader::check_relative;

use crate::PackError;

/// Separator of the suffix that keeps the sources and layers of several packs apart
/// (`pistes-line~p1`); the first pack has none.
pub const PACK_SUFFIX: char = '~';

/// A MapLibre style ready to load.
#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct MapStyle {
    /// Style JSON (version 8), absolute URLs.
    pub json: String,
    /// `name` of the style: `skimap <site or libre> <style>-<theme>`.
    pub name: String,
    /// `metadata.skimap:style` (`fond`, `srn`, `guidage`).
    pub style_id: String,
    /// `metadata.skimap:theme` (`clair`, `sombre`).
    pub theme: String,
    /// Layer groups of the style (`metadata.skimap:groups`), in menu order, with the
    /// style layers of each.
    pub groups: Vec<LayerGroup>,
    /// Style layers drawing each pack layer (touch on a feature: its card).
    pub overlays: Vec<OverlayLayers>,
    /// Attributions of what this style shows, from the manifests of the packs.
    pub attributions: Vec<String>,
    /// Legend rows, read from the style itself (colours of the style file).
    pub legend: Vec<LegendEntry>,
    /// Sources of the style a pack does not have (`<site>: <source>`), left out.
    pub missing: Vec<String>,
}

/// A group of style layers (`metadata.skimap:group`), shown or hidden as a whole.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct LayerGroup {
    /// Identifier (`refuges`, `pentes`...), stored with the user's choice.
    pub id: String,
    /// Title, French (from the style; the app has its own translations).
    pub title: String,
    /// Whether the user may hide it (safety groups and the basemap may not).
    pub toggleable: bool,
    /// Visibility before the user chooses: hidden when every layer of the group is
    /// hidden in the style (`snow-*`: `metadata.skimap:optional_layers`).
    pub visible_by_default: bool,
    /// Ids of the style layers of the group, every pack included.
    pub style_layers: Vec<String>,
}

/// Style layers of one pack layer.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct OverlayLayers {
    /// Pack layer identifier (`refuges-info`).
    pub pack_layer: String,
    /// Ids of the style layers that draw it.
    pub style_layers: Vec<String>,
}

/// How a legend row is drawn.
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum LegendKind {
    /// Solid line.
    Line,
    /// Dashed line.
    Dashed,
    /// Dotted line.
    Dotted,
    /// Filled area.
    Area,
    /// Wide band of [`LegendEntry::color`] with a thin core of [`LegendEntry::core`]
    /// (a piste whose state is unknown).
    Band,
}

/// A legend row. The app words it from [`LegendEntry::key`] (French and English
/// resources); the colour comes from the style.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct LegendEntry {
    /// Group of rows (`pistes`, `pentes`, `dangers`).
    pub group: String,
    /// What the row shows: a piste difficulty (`novice`, `easy`, `intermediate`,
    /// `advanced`, `unknown`), `closed`, `state-unknown` (no source publishes the state
    /// of the piste), `backcountry`, `hike`, a slope class
    /// (`slope-1` … `slope-4`) or an exposure class (`exposure-2`, `exposure-3`).
    pub key: String,
    /// `#rrggbb`.
    pub color: String,
    /// Line, dashes, dots, area or band.
    pub kind: LegendKind,
    /// `#rrggbb` of the thin core of a [`LegendKind::Band`]; `None` for other kinds.
    #[uniffi(default)]
    pub core: Option<String>,
}

/// The France basemap of the phone cache (`france.json` and its files).
#[derive(Debug, Clone)]
pub struct France {
    /// Its directory (absolute).
    pub dir: std::path::PathBuf,
    /// Its description.
    pub basemap: skimap_pack::france::FranceBasemap,
}

impl France {
    /// Reads the description of the France basemap in `dir` (made absolute).
    pub fn open(dir: &Path) -> Result<Self, PackError> {
        let dir = std::path::absolute(dir).map_err(|e| PackError::Invalid {
            detail: format!("{}: {e}", dir.display()),
        })?;
        let basemap = skimap_pack::france::FranceBasemap::read(&dir)?;
        if !dir.join(skimap_pack::france::TILES_FILE).is_file() {
            return Err(PackError::NotFound {
                path: dir
                    .join(skimap_pack::france::TILES_FILE)
                    .display()
                    .to_string(),
            });
        }
        Ok(Self { dir, basemap })
    }
}

/// Prefix of the URLs of the France basemap in the styles.
const FRANCE_PREFIX: &str = "france/";

/// One pack of the style and the suffix of its sources and layers.
struct Part<'a> {
    pack: &'a Pack,
    suffix: String,
}

fn invalid(message: &str) -> PackError {
    PackError::Invalid {
        detail: message.to_owned(),
    }
}

/// Instantiates `style_json` (a style of `styles/`, relative URLs) on `packs`, in order.
/// `name_id` names the style (`skimap <name_id> <style>-<theme>`).
pub fn instantiate(
    style_json: &str,
    packs: &[&Pack],
    name_id: &str,
) -> Result<MapStyle, PackError> {
    instantiate_over(style_json, packs, None, name_id)
}

/// [`instantiate`] over the France basemap `france`, when the phone has it: its shared
/// sources are drawn under the packs. `packs` may then be empty.
pub fn instantiate_over(
    style_json: &str,
    packs: &[&Pack],
    france: Option<&France>,
    name_id: &str,
) -> Result<MapStyle, PackError> {
    if packs.is_empty() && france.is_none() {
        return Err(invalid("no pack to show"));
    }
    let mut style: Value =
        serde_json::from_str(style_json).map_err(|e| invalid(&format!("style JSON: {e}")))?;
    let obj = style
        .as_object_mut()
        .ok_or_else(|| invalid("style is not an object"))?;
    let meta = obj.get("metadata").cloned().unwrap_or(Value::Null);
    let runtime: BTreeSet<String> = meta
        .get("skimap:runtime_sources")
        .and_then(Value::as_object)
        .map(|m| m.keys().cloned().collect())
        .unwrap_or_default();
    let shared: BTreeSet<String> = meta
        .get("skimap:shared_sources")
        .and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .filter_map(Value::as_str)
                .map(str::to_owned)
                .collect()
        })
        .unwrap_or_default();
    let style_id = meta["skimap:style"].as_str().unwrap_or("").to_owned();
    let theme = meta["skimap:theme"].as_str().unwrap_or("").to_owned();
    let parts: Vec<Part> = packs
        .iter()
        .enumerate()
        .map(|(k, pack)| Part {
            pack,
            suffix: if k == 0 {
                String::new()
            } else {
                format!("{PACK_SUFFIX}p{k}")
            },
        })
        .collect();

    // Sources: runtime ones once, the others once per pack that has their file.
    let sources = match obj.remove("sources") {
        Some(Value::Object(s)) => s,
        _ => return Err(invalid("style without sources object")),
    };
    let mut out_sources = Map::new();
    // Source id -> suffixes of the packs that have it.
    let mut present: HashMap<String, Vec<String>> = HashMap::new();
    // Source id (suffixed) -> pack layer id, for the cards.
    let mut pack_layer_of: HashMap<String, String> = HashMap::new();
    let mut attributions: Vec<String> = Vec::new();
    let mut missing = Vec::new();
    for (id, src) in &sources {
        // Shared sources (France basemap): once, from the phone cache, or left out.
        if shared.contains(id) {
            match france.map(|f| resolve_france(f, src)).transpose()? {
                Some(Some(s)) => {
                    for a in france.map(|f| f.basemap.attributions()).unwrap_or_default() {
                        if !attributions.contains(&a) {
                            attributions.push(a);
                        }
                    }
                    out_sources.insert(id.clone(), s);
                    present.insert(id.clone(), vec![String::new()]);
                }
                _ => missing.push(format!("{FRANCE_PREFIX}: {id}")),
            }
            continue;
        }
        // Runtime sources and inline data: once, as they are.
        let file_backed = src.get("url").is_some_and(Value::is_string)
            || src.get("data").is_some_and(Value::is_string)
            || src.get("tiles").is_some_and(Value::is_array);
        if runtime.contains(id) || !file_backed {
            out_sources.insert(id.clone(), src.clone());
            present.insert(id.clone(), vec![String::new()]);
            continue;
        }
        for part in &parts {
            let mut s = src
                .as_object()
                .cloned()
                .ok_or_else(|| invalid(&format!("source {id} is not an object")))?;
            match resolve_source(part.pack, &mut s)? {
                Some(rel) => {
                    let m = part.pack.manifest();
                    let item_sources = m
                        .basemaps
                        .iter()
                        .find(|b| b.tiles == rel)
                        .map(|b| &b.sources)
                        .or_else(|| {
                            m.layers.iter().find(|l| l.file == rel).map(|l| {
                                pack_layer_of.insert(format!("{id}{}", part.suffix), l.id.clone());
                                &l.sources
                            })
                        });
                    for a in item_sources.into_iter().flatten() {
                        let a = a.attribution.trim();
                        if !a.is_empty() && !attributions.iter().any(|x| x == a) {
                            attributions.push(a.to_owned());
                        }
                    }
                    out_sources.insert(format!("{id}{}", part.suffix), Value::Object(s));
                    present
                        .entry(id.clone())
                        .or_default()
                        .push(part.suffix.clone());
                }
                None => missing.push(format!("{}: {id}", part.pack.manifest().site.id)),
            }
        }
    }
    obj.insert("sources".to_owned(), Value::Object(out_sources));

    // Layers: each one repeated right after itself for every pack that has its source.
    let layers = match obj.remove("layers") {
        Some(Value::Array(l)) => l,
        _ => return Err(invalid("style without layers array")),
    };
    let mut out_layers = Vec::with_capacity(layers.len() * parts.len());
    for l in layers {
        let Some(source) = l.get("source").and_then(Value::as_str).map(str::to_owned) else {
            out_layers.push(l);
            continue;
        };
        for sfx in present.get(&source).into_iter().flatten() {
            let mut c = l.clone();
            if let Some(Value::String(id)) = c.get_mut("id") {
                id.push_str(sfx);
            }
            c["source"] = Value::String(format!("{source}{sfx}"));
            out_layers.push(c);
        }
    }

    // Glyphs and sprite: those of the first pack that has them (the same in every pack).
    if let Some(Value::String(g)) = obj.get("glyphs").cloned() {
        match parts
            .iter()
            .find_map(|p| glyphs_url(p.pack, &g).transpose())
        {
            Some(url) => {
                obj.insert("glyphs".to_owned(), Value::String(url?));
            }
            // No pack: the glyphs of the France basemap (same font stacks).
            None => match france.and_then(|f| france_glyphs(f, &g)) {
                Some(url) => {
                    obj.insert("glyphs".to_owned(), Value::String(url));
                }
                None => {
                    obj.remove("glyphs");
                    missing.push(format!("glyphs: {g}"));
                }
            },
        }
    }
    if let Some(Value::String(s)) = obj.get("sprite").cloned() {
        match parts
            .iter()
            .find_map(|p| sprite_url(p.pack, &s).transpose())
        {
            Some(url) => {
                obj.insert("sprite".to_owned(), Value::String(url?));
            }
            None => {
                obj.remove("sprite");
                missing.push(format!("sprite: {s}"));
            }
        }
    }

    let groups = groups(&meta, &out_layers);
    let overlays = overlays(&out_layers, &pack_layer_of);
    let legend = legend(&out_layers);
    let name = format!("skimap {name_id} {style_id}-{theme}");
    obj.insert("name".to_owned(), Value::String(name.clone()));
    obj.insert("layers".to_owned(), Value::Array(out_layers));
    Ok(MapStyle {
        json: serde_json::to_string(&style).map_err(|e| invalid(&e.to_string()))?,
        name,
        style_id,
        theme,
        groups,
        overlays,
        attributions,
        legend,
        missing,
    })
}

/// A shared source resolved in the France basemap: `pmtiles://france/<file>` becomes the
/// absolute URL of that file of the France basemap; `None` when it lacks it.
fn resolve_france(france: &France, src: &Value) -> Result<Option<Value>, PackError> {
    let mut s = src
        .as_object()
        .cloned()
        .ok_or_else(|| invalid("shared source is not an object"))?;
    let Some(Value::String(u)) = s.get_mut("url") else {
        return Ok(None);
    };
    let Some(rel) = u
        .strip_prefix("pmtiles://")
        .and_then(|r| r.strip_prefix(FRANCE_PREFIX))
        .map(str::to_owned)
    else {
        return Ok(None);
    };
    check_relative(&rel)?;
    let path = france.dir.join(&rel);
    if !path.is_file() {
        return Ok(None);
    }
    *u = format!("pmtiles://{}", file_url(&path));
    Ok(Some(Value::Object(s)))
}

/// The glyphs of the France basemap for a glyphs template of the packs (the font stacks
/// are the same): `<france>/fonts/{fontstack}/{range}.pbf`.
fn france_glyphs(france: &France, template: &str) -> Option<String> {
    let fonts = france.dir.join(skimap_pack::france::FONTS_DIR);
    (template.contains("{fontstack}") && fonts.is_dir())
        .then(|| file_url(&fonts.join("{fontstack}/{range}.pbf")))
}

/// Rewrites the URLs of a source to absolute ones in `pack`; returns the pack-relative
/// file path, or `None` when the pack does not have the file.
fn resolve_source(pack: &Pack, src: &mut Map<String, Value>) -> Result<Option<String>, PackError> {
    let root = pack.root();
    if let Some(Value::String(u)) = src.get_mut("url") {
        let (scheme, rel) = match u.strip_prefix("pmtiles://") {
            Some(rest) => ("pmtiles://", rest.to_owned()),
            None => ("", u.clone()),
        };
        if has_scheme(&rel) {
            return Ok(Some(rel));
        }
        check_relative(&rel)?;
        let path = root.join(&rel);
        if !path.is_file() {
            return Ok(None);
        }
        *u = format!("{scheme}{}", file_url(&path));
        return Ok(Some(rel));
    }
    if let Some(Value::String(d)) = src.get_mut("data") {
        if has_scheme(d) {
            return Ok(Some(d.clone()));
        }
        let rel = d.clone();
        check_relative(&rel)?;
        let path = root.join(&rel);
        if !path.is_file() {
            return Ok(None);
        }
        *d = file_url(&path);
        return Ok(Some(rel));
    }
    if let Some(Value::Array(tiles)) = src.get_mut("tiles") {
        let mut first = None;
        for t in tiles {
            if let Value::String(t) = t
                && !has_scheme(t)
            {
                let dir = t.split('{').next().unwrap_or("");
                let dir = dir.trim_end_matches('/');
                check_relative(dir)?;
                if !root.join(dir).is_dir() {
                    return Ok(None);
                }
                first.get_or_insert_with(|| dir.to_owned());
                *t = file_url(&root.join(t.as_str()));
            }
        }
        return Ok(first);
    }
    Ok(None)
}

/// Absolute glyphs template in `pack`, if the font directory exists there.
fn glyphs_url(pack: &Pack, template: &str) -> Result<Option<String>, PackError> {
    if has_scheme(template) {
        return Ok(Some(template.to_owned()));
    }
    let dir = template
        .split("{fontstack}")
        .next()
        .unwrap_or("")
        .trim_end_matches('/');
    check_relative(dir)?;
    let root = pack.root();
    Ok(root
        .join(dir)
        .is_dir()
        .then(|| file_url(&root.join(template))))
}

/// Absolute sprite URL in `pack`, if `<sprite>.json` exists there.
fn sprite_url(pack: &Pack, rel: &str) -> Result<Option<String>, PackError> {
    if has_scheme(rel) {
        return Ok(Some(rel.to_owned()));
    }
    check_relative(rel)?;
    let root = pack.root();
    Ok(root
        .join(format!("{rel}.json"))
        .is_file()
        .then(|| file_url(&root.join(rel))))
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

fn group_of(layer: &Value) -> Option<&str> {
    layer
        .get("metadata")
        .and_then(|m| m.get("skimap:group"))
        .and_then(Value::as_str)
}

fn hidden_in_style(layer: &Value) -> bool {
    layer
        .get("layout")
        .and_then(|l| l.get("visibility"))
        .and_then(Value::as_str)
        == Some("none")
}

/// Groups of `metadata.skimap:groups` that have at least one layer, in menu order.
fn groups(meta: &Value, layers: &[Value]) -> Vec<LayerGroup> {
    let declared = meta["skimap:groups"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    declared
        .iter()
        .filter_map(|g| {
            let id = g["id"].as_str()?;
            let members: Vec<&Value> = layers.iter().filter(|l| group_of(l) == Some(id)).collect();
            if members.is_empty() {
                return None;
            }
            Some(LayerGroup {
                id: id.to_owned(),
                title: g["title"].as_str().unwrap_or(id).to_owned(),
                toggleable: g["toggleable"].as_bool().unwrap_or(false),
                visible_by_default: !members.iter().all(|l| hidden_in_style(l)),
                style_layers: members
                    .iter()
                    .filter_map(|l| l["id"].as_str().map(str::to_owned))
                    .collect(),
            })
        })
        .collect()
}

/// Style layers of each pack layer drawn (GeoJSON sources of the pack).
fn overlays(layers: &[Value], pack_layer_of: &HashMap<String, String>) -> Vec<OverlayLayers> {
    let mut out: Vec<OverlayLayers> = Vec::new();
    for l in layers {
        let (Some(src), Some(id)) = (l["source"].as_str(), l["id"].as_str()) else {
            continue;
        };
        let Some(pack_layer) = pack_layer_of.get(src) else {
            continue;
        };
        match out.iter_mut().find(|o| &o.pack_layer == pack_layer) {
            Some(o) => o.style_layers.push(id.to_owned()),
            None => out.push(OverlayLayers {
                pack_layer: pack_layer.clone(),
                style_layers: vec![id.to_owned()],
            }),
        }
    }
    out
}

/// Pairs `(value, colour)` and the default colour of a `["match", input, v1, c1, …, d]`.
fn match_colours(e: &Value) -> Option<(Vec<(String, String)>, String)> {
    let a = e.as_array()?;
    if a.first()?.as_str()? != "match" || a.len() < 4 {
        return None;
    }
    let default = a.last()?.as_str()?.to_owned();
    let mut pairs = Vec::new();
    for pair in a[2..a.len() - 1].chunks(2) {
        let [v, c] = pair else { return None };
        let c = c.as_str()?.to_owned();
        match v {
            Value::String(s) => pairs.push((s.clone(), c)),
            Value::Array(vs) => {
                pairs.extend(
                    vs.iter()
                        .filter_map(|s| Some((s.as_str()?.to_owned(), c.clone()))),
                );
            }
            _ => {}
        }
    }
    Some((pairs, default))
}

/// Legend of the layers present, from their paint in the style: piste difficulties
/// (`pistes-line`), closed pistes, marked off-piste and pedestrian trails, slope classes
/// and avalanche exposure. Only the first pack's copy of each layer is read (same paint).
fn legend(layers: &[Value]) -> Vec<LegendEntry> {
    let find = |id: &str| layers.iter().find(|l| l["id"].as_str() == Some(id));
    let colour = |l: &Value, prop: &str| l["paint"][prop].as_str().map(str::to_owned);
    let mut out = Vec::new();
    let mut push = |group: &str, key: &str, color: String, kind: LegendKind, core| {
        out.push(LegendEntry {
            group: group.to_owned(),
            key: key.to_owned(),
            color,
            kind,
            core,
        });
    };
    if let Some((pairs, default)) =
        find("pistes-line").and_then(|l| match_colours(&l["paint"]["line-color"]))
    {
        for d in ["novice", "easy", "intermediate", "advanced"] {
            if let Some((_, c)) = pairs.iter().find(|(v, _)| v == d) {
                push("pistes", d, c.clone(), LegendKind::Line, None);
            }
        }
        push("pistes", "unknown", default, LegendKind::Line, None);
        // State unknown (#141): the neutral band of `pistes-unknown-casing` around a
        // thin core of the difficulty colour (blue as the sample), never the solid line
        // of an open piste.
        if let Some((_, band)) =
            find("pistes-unknown-casing").and_then(|l| match_colours(&l["paint"]["line-color"]))
            && let Some((_, c)) = pairs.iter().find(|(v, _)| v == "easy")
        {
            push(
                "pistes",
                "state-unknown",
                band,
                LegendKind::Band,
                Some(c.clone()),
            );
        }
        if find("pistes-closed-dash").is_some()
            && let Some((_, c)) = pairs.iter().find(|(v, _)| v == "easy")
        {
            push("pistes", "closed", c.clone(), LegendKind::Dashed, None);
        }
    }
    if let Some((pairs, _)) =
        find("pistes-backcountry").and_then(|l| match_colours(&l["paint"]["line-color"]))
        && let Some((_, c)) = pairs.iter().find(|(v, _)| v == "easy")
    {
        push("pistes", "backcountry", c.clone(), LegendKind::Dashed, None);
    }
    if let Some(c) = find("pistes-hike").and_then(|l| colour(l, "line-color")) {
        push("pistes", "hike", c, LegendKind::Dotted, None);
    }
    for k in 1..=4 {
        let id = format!("slope-{k}");
        if let Some(c) = find(&id).and_then(|l| colour(l, "fill-color")) {
            push("pentes", &id, c, LegendKind::Area, None);
        }
    }
    for k in 2..=3 {
        let id = format!("exposure-{k}");
        if let Some(c) = find(&id).and_then(|l| colour(l, "fill-color")) {
            push("dangers", &id, c, LegendKind::Area, None);
        }
    }
    out
}

/// Layer or source id without the pack suffix.
pub fn base_id(id: &str) -> &str {
    id.split(PACK_SUFFIX).next().unwrap_or(id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn match_expression_colours() {
        let e = json!(["match", ["get", "d"], "a", "#1", ["b", "c"], "#2", "#0"]);
        let (pairs, d) = match_colours(&e).unwrap();
        assert_eq!(d, "#0");
        assert_eq!(
            pairs,
            [
                ("a".to_owned(), "#1".to_owned()),
                ("b".to_owned(), "#2".to_owned()),
                ("c".to_owned(), "#2".to_owned())
            ]
        );
        assert!(match_colours(&json!("#fff")).is_none());
    }

    #[test]
    fn urls() {
        assert!(has_scheme("file:///a"));
        assert!(has_scheme("pmtiles://file:///a"));
        assert!(!has_scheme("layers/pistes.geojson"));
        assert_eq!(
            file_url(Path::new("/a b/{fontstack}/x")),
            "file:///a%20b/{fontstack}/x"
        );
        assert_eq!(base_id("pistes-line~p2"), "pistes-line");
        assert_eq!(base_id("pistes-line"), "pistes-line");
    }
}
