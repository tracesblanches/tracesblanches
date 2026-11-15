//! Export of a pack for Locus Map and OsmAnd (P21, #100): one folder per site with
//! GPX and KML for the vector layers, MBTiles (Locus) and SQLiteDB (OsmAnd) for the raster
//! overlays, and a French how-to (`LISEZMOI.md`).
//!
//! Everything is read from the pack through [`crate::reader::Pack`]: the export shows
//! exactly what the pack holds.

pub mod db;
mod readme;
pub mod xml;

use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use geojson::{Feature, GeometryValue};
use serde::Serialize;
use serde_json::Value;

use crate::build::{create_dir, write_bytes};
use crate::error::{Error, Result};
use crate::manifest::{Layer, ValueEncoding};
use crate::png;
use crate::reader::Pack;
use crate::tiles::{TILE_SIZE, tiles_covering};
use xml::{coord, esc, html_lines};

/// Layer name of the export in `data/out/<site>/`.
pub const EXPORT_LAYER: &str = "export";

/// A ring or line, `(lon, lat)`.
pub type Line = Vec<(f64, f64)>;

/// Geometry of an exported item.
#[derive(Debug, Clone, PartialEq)]
pub enum Geom {
    /// Waypoint.
    Point(f64, f64),
    /// One or more lines.
    Lines(Vec<Line>),
    /// Polygons: rings, outer first.
    Polygons(Vec<Vec<Line>>),
}

/// Drawing style.
#[derive(Debug, Clone, PartialEq)]
pub struct Style {
    /// Identifier (KML style id).
    pub id: String,
    /// Line colour RGB.
    pub line: [u8; 3],
    /// Line opacity 0-255.
    pub line_alpha: u8,
    /// Line width in pixels.
    pub width: f32,
    /// Fill RGBA (polygons).
    pub fill: Option<[u8; 4]>,
}

/// One exported feature.
#[derive(Debug, Clone)]
pub struct Item {
    /// Name.
    pub name: String,
    /// Description lines (French).
    pub desc: Vec<String>,
    /// Geometry.
    pub geom: Geom,
    /// Style id.
    pub style: String,
    /// KML folder.
    pub folder: String,
}

/// A set of items written to one GPX and one KML file.
#[derive(Debug, Clone)]
pub struct Collection {
    /// File stem (`pistes-rouges`).
    pub stem: String,
    /// Title.
    pub title: String,
    /// Description: sources, licences, date.
    pub about: Vec<String>,
    /// Styles used.
    pub styles: Vec<Style>,
    /// Items.
    pub items: Vec<Item>,
}

fn hex_rgb(c: [u8; 3]) -> String {
    format!("{:02X}{:02X}{:02X}", c[0], c[1], c[2])
}

/// KML colour `aabbggrr`.
fn kml_color(c: [u8; 3], a: u8) -> String {
    format!("{a:02x}{:02x}{:02x}{:02x}", c[2], c[1], c[0])
}

fn bounds(items: &[Item]) -> Option<[f64; 4]> {
    let mut b = [f64::MAX, f64::MAX, f64::MIN, f64::MIN];
    let mut add = |(x, y): (f64, f64)| {
        b[0] = b[0].min(x);
        b[1] = b[1].min(y);
        b[2] = b[2].max(x);
        b[3] = b[3].max(y);
    };
    for it in items {
        match &it.geom {
            Geom::Point(x, y) => add((*x, *y)),
            Geom::Lines(ls) => ls.iter().flatten().copied().for_each(&mut add),
            Geom::Polygons(ps) => ps.iter().flatten().flatten().copied().for_each(&mut add),
        }
    }
    (b[0] <= b[2]).then_some(b)
}

/// GPX 1.1: waypoints, then one track per line item (one segment per line or ring).
/// Track colours: OsmAnd (`osmand:color`) and GPX style 0.2 (`gpx_style:line`, Locus).
pub fn gpx(c: &Collection, generator: &str, time: &str) -> String {
    let mut s = String::new();
    s.push_str("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n");
    let _ = writeln!(
        s,
        "<gpx version=\"1.1\" creator=\"{}\" xmlns=\"http://www.topografix.com/GPX/1/1\" \
         xmlns:xsi=\"http://www.w3.org/2001/XMLSchema-instance\" xmlns:osmand=\"https://osmand.net\" \
         xmlns:gpx_style=\"http://www.topografix.com/GPX/gpx_style/0/2\" \
         xsi:schemaLocation=\"http://www.topografix.com/GPX/1/1 http://www.topografix.com/GPX/1/1/gpx.xsd\">",
        esc(generator)
    );
    s.push_str("<metadata>");
    let _ = write!(
        s,
        "<name>{}</name><desc>{}</desc>",
        esc(&c.title),
        esc(&c.about.join("\n"))
    );
    let _ = write!(s, "<time>{}</time>", esc(time));
    if let Some(b) = bounds(&c.items) {
        let _ = write!(
            s,
            "<bounds minlat=\"{}\" minlon=\"{}\" maxlat=\"{}\" maxlon=\"{}\"/>",
            coord(b[1]),
            coord(b[0]),
            coord(b[3]),
            coord(b[2])
        );
    }
    s.push_str("</metadata>\n");
    let style = |id: &str| c.styles.iter().find(|st| st.id == id);
    for it in c.items.iter().filter(|i| matches!(i.geom, Geom::Point(..))) {
        let Geom::Point(x, y) = it.geom else { continue };
        let _ = write!(s, "<wpt lat=\"{}\" lon=\"{}\">", coord(y), coord(x));
        let _ = write!(
            s,
            "<name>{}</name><desc>{}</desc>",
            esc(&it.name),
            esc(&it.desc.join("\n"))
        );
        let _ = write!(s, "<type>{}</type>", esc(&it.folder));
        if let Some(st) = style(&it.style) {
            let _ = write!(
                s,
                "<extensions><osmand:color>#{}</osmand:color></extensions>",
                hex_rgb(st.line)
            );
        }
        s.push_str("</wpt>\n");
    }
    for it in &c.items {
        let segs: Vec<&Line> = match &it.geom {
            Geom::Point(..) => continue,
            Geom::Lines(ls) => ls.iter().collect(),
            Geom::Polygons(ps) => ps.iter().flatten().collect(),
        };
        s.push_str("<trk>");
        let _ = write!(
            s,
            "<name>{}</name><desc>{}</desc>",
            esc(&it.name),
            esc(&it.desc.join("\n"))
        );
        let _ = write!(s, "<type>{}</type>", esc(&it.folder));
        if let Some(st) = style(&it.style) {
            let opacity = f32::from(st.line_alpha) / 255.0;
            let _ = write!(
                s,
                "<extensions><osmand:color>#{a:02X}{rgb}</osmand:color><osmand:width>{w}</osmand:width>\
                 <gpx_style:line><gpx_style:color>{rgb}</gpx_style:color><gpx_style:opacity>{opacity:.2}</gpx_style:opacity>\
                 <gpx_style:width>{mm:.1}</gpx_style:width></gpx_style:line></extensions>",
                a = st.line_alpha,
                rgb = hex_rgb(st.line),
                w = if st.width >= 4.0 {
                    "bold"
                } else if st.width >= 2.5 {
                    "medium"
                } else {
                    "thin"
                },
                mm = st.width * 0.25,
            );
        }
        for seg in segs {
            s.push_str("<trkseg>");
            for (x, y) in seg {
                let _ = write!(s, "<trkpt lat=\"{}\" lon=\"{}\"/>", coord(*y), coord(*x));
            }
            s.push_str("</trkseg>");
        }
        s.push_str("</trk>\n");
    }
    s.push_str("</gpx>\n");
    s
}

fn kml_coords(line: &Line) -> String {
    line.iter()
        .map(|(x, y)| format!("{},{}", coord(*x), coord(*y)))
        .collect::<Vec<_>>()
        .join(" ")
}

fn kml_polygon(rings: &[Line]) -> String {
    let mut s = String::from("<Polygon><tessellate>1</tessellate>");
    for (i, r) in rings.iter().enumerate() {
        let tag = if i == 0 {
            "outerBoundaryIs"
        } else {
            "innerBoundaryIs"
        };
        let _ = write!(
            s,
            "<{tag}><LinearRing><coordinates>{}</coordinates></LinearRing></{tag}>",
            kml_coords(r)
        );
    }
    s.push_str("</Polygon>");
    s
}

/// KML 2.2: shared styles, one folder per [`Item::folder`], HTML descriptions.
pub fn kml(c: &Collection) -> String {
    let mut s = String::new();
    s.push_str("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<kml xmlns=\"http://www.opengis.net/kml/2.2\">\n<Document>");
    let _ = writeln!(
        s,
        "<name>{}</name><description>{}</description>",
        esc(&c.title),
        esc(&html_lines(&c.about))
    );
    for st in &c.styles {
        let _ = write!(s, "<Style id=\"{}\">", esc(&st.id));
        let _ = write!(
            s,
            "<IconStyle><color>{}</color></IconStyle>",
            kml_color(st.line, 255)
        );
        let _ = write!(
            s,
            "<LineStyle><color>{}</color><width>{}</width></LineStyle>",
            kml_color(st.line, st.line_alpha),
            st.width
        );
        if let Some(f) = st.fill {
            let _ = write!(
                s,
                "<PolyStyle><color>{}</color><fill>1</fill><outline>1</outline></PolyStyle>",
                kml_color([f[0], f[1], f[2]], f[3])
            );
        }
        s.push_str("</Style>\n");
    }
    let mut folders: Vec<&str> = Vec::new();
    for it in &c.items {
        if !folders.contains(&it.folder.as_str()) {
            folders.push(&it.folder);
        }
    }
    for f in folders {
        let _ = writeln!(s, "<Folder><name>{}</name>", esc(f));
        for it in c.items.iter().filter(|i| i.folder == f) {
            s.push_str("<Placemark>");
            let _ = write!(
                s,
                "<name>{}</name><description>{}</description><styleUrl>#{}</styleUrl>",
                esc(&it.name),
                esc(&html_lines(&it.desc)),
                esc(&it.style)
            );
            match &it.geom {
                Geom::Point(x, y) => {
                    let _ = write!(
                        s,
                        "<Point><coordinates>{},{}</coordinates></Point>",
                        coord(*x),
                        coord(*y)
                    );
                }
                Geom::Lines(ls) if ls.len() == 1 => {
                    let _ = write!(
                        s,
                        "<LineString><tessellate>1</tessellate><coordinates>{}</coordinates></LineString>",
                        kml_coords(&ls[0])
                    );
                }
                Geom::Lines(ls) => {
                    s.push_str("<MultiGeometry>");
                    for l in ls {
                        let _ = write!(
                            s,
                            "<LineString><tessellate>1</tessellate><coordinates>{}</coordinates></LineString>",
                            kml_coords(l)
                        );
                    }
                    s.push_str("</MultiGeometry>");
                }
                Geom::Polygons(ps) if ps.len() == 1 => s.push_str(&kml_polygon(&ps[0])),
                Geom::Polygons(ps) => {
                    s.push_str("<MultiGeometry>");
                    for p in ps {
                        s.push_str(&kml_polygon(p));
                    }
                    s.push_str("</MultiGeometry>");
                }
            }
            s.push_str("</Placemark>\n");
        }
        s.push_str("</Folder>\n");
    }
    s.push_str("</Document>\n</kml>\n");
    s
}

fn pos(p: &geojson::Position) -> (f64, f64) {
    (p[0], p[1])
}

fn line(l: &[geojson::Position]) -> Line {
    l.iter().map(pos).collect()
}

/// Geometry of a GeoJSON feature; `None` for null or empty geometries.
pub fn geom_of(f: &Feature) -> Option<Geom> {
    let g = f.geometry.as_ref()?;
    let geom = match &g.value {
        GeometryValue::Point { coordinates } => Geom::Point(coordinates[0], coordinates[1]),
        GeometryValue::MultiPoint { coordinates } => {
            let p = coordinates.first()?;
            Geom::Point(p[0], p[1])
        }
        GeometryValue::LineString { coordinates } => Geom::Lines(vec![line(coordinates)]),
        GeometryValue::MultiLineString { coordinates } => {
            Geom::Lines(coordinates.iter().map(|l| line(l)).collect())
        }
        GeometryValue::Polygon { coordinates } => {
            Geom::Polygons(vec![coordinates.iter().map(|r| line(r)).collect()])
        }
        GeometryValue::MultiPolygon { coordinates } => Geom::Polygons(
            coordinates
                .iter()
                .map(|p| p.iter().map(|r| line(r)).collect())
                .collect(),
        ),
        GeometryValue::GeometryCollection { .. } => return None,
    };
    let empty = match &geom {
        Geom::Point(..) => false,
        Geom::Lines(ls) => ls.iter().all(|l| l.len() < 2),
        Geom::Polygons(ps) => ps.iter().all(|p| p.first().is_none_or(|r| r.len() < 4)),
    };
    (!empty).then_some(geom)
}

fn prop<'a>(f: &'a Feature, k: &str) -> Option<&'a Value> {
    f.properties.as_ref()?.get(k).filter(|v| !v.is_null())
}

fn prop_str(f: &Feature, k: &str) -> Option<String> {
    prop(f, k).and_then(|v| match v {
        Value::String(s) if !s.trim().is_empty() => Some(s.trim().to_owned()),
        Value::Number(n) => Some(n.to_string()),
        _ => None,
    })
}

fn prop_f64(f: &Feature, k: &str) -> Option<f64> {
    prop(f, k).and_then(Value::as_f64)
}

fn about(layer: &Layer, generated: &str) -> Vec<String> {
    let mut v = vec![format!("{} — pack skimap du {generated}.", layer.title)];
    for s in &layer.sources {
        let mut line = format!("Source : {}", s.name);
        if !s.licence.is_empty() {
            let _ = write!(line, " ; licence : {}", s.licence);
        }
        if !s.attribution.is_empty() {
            let _ = write!(line, " ; {}", s.attribution);
        }
        v.push(line);
    }
    v
}

fn style(id: &str, line: [u8; 3], line_alpha: u8, width: f32, fill: Option<[u8; 4]>) -> Style {
    Style {
        id: id.to_owned(),
        line,
        line_alpha,
        width,
        fill,
    }
}

/// Piste colours by difficulty (OSM `piste:difficulty` after the `osm` crate's choice).
pub fn difficulty_style(d: Option<&str>) -> (&'static str, &'static str, [u8; 3]) {
    match d {
        Some("novice") => ("vertes", "Pistes vertes", [0x1a, 0x98, 0x50]),
        Some("easy") => ("bleues", "Pistes bleues", [0x21, 0x66, 0xac]),
        Some("intermediate") => ("rouges", "Pistes rouges", [0xd7, 0x30, 0x27]),
        Some("advanced" | "expert" | "freeride") => ("noires", "Pistes noires", [0x10, 0x10, 0x10]),
        _ => (
            "autres",
            "Pistes sans difficulté connue",
            [0x80, 0x80, 0x80],
        ),
    }
}

fn difficulty_fr(d: Option<&str>) -> &'static str {
    match d {
        Some("novice") => "verte",
        Some("easy") => "bleue",
        Some("intermediate") => "rouge",
        Some("advanced") => "noire",
        Some("expert") => "experte",
        Some(_) => "autre",
        None => "inconnue",
    }
}

fn piste_items(fc: &geojson::FeatureCollection) -> Vec<Item> {
    let mut items = Vec::new();
    for f in &fc.features {
        let Some(geom) = geom_of(f) else { continue };
        let d = prop_str(f, "difficulty");
        let (key, folder, _) = difficulty_style(d.as_deref());
        let name = prop_str(f, "name")
            .or_else(|| prop_str(f, "way_name"))
            .unwrap_or_else(|| "Piste sans nom".to_owned());
        let mut desc = vec![format!("Difficulté : {}", difficulty_fr(d.as_deref()))];
        if let Some(dm) = prop_str(f, "difficulty_max")
            && Some(&dm) != d.as_ref()
        {
            desc.push(format!(
                "Tronçon partagé, la plus difficile : {}",
                difficulty_fr(Some(&dm))
            ));
        }
        if let Some(g) = prop_str(f, "grooming") {
            desc.push(format!(
                "Damage : {}",
                g.replace(';', " + ").replace("classic", "classique")
            ));
        }
        if let Some(Value::Array(uses)) = prop(f, "uses") {
            let u: Vec<&str> = uses.iter().filter_map(Value::as_str).collect();
            desc.push(format!(
                "Usage : {}",
                u.join(", ")
                    .replace("nordic", "ski nordique")
                    .replace("hike", "raquettes / piétons")
            ));
        }
        if prop(f, "oneway").and_then(Value::as_bool) == Some(true) {
            desc.push("Sens unique (sens du tracé)".to_owned());
        }
        if let Some(Value::Array(rels)) = prop(f, "relations") {
            let names: Vec<&str> = rels
                .iter()
                .filter_map(|r| r.get("name")?.as_str())
                .collect();
            if !names.is_empty() {
                desc.push(format!("Itinéraires : {}", names.join(", ")));
            }
        }
        if let Some(l) = prop_f64(f, "length_m") {
            desc.push(format!("Longueur : {l:.0} m"));
        }
        if let Some(id) = prop_str(f, "osm_way_id") {
            desc.push(format!("OSM way {id}"));
        }
        items.push(Item {
            name,
            desc,
            geom,
            style: format!("piste-{key}"),
            folder: folder.to_owned(),
        });
    }
    items
}

fn piste_styles() -> Vec<Style> {
    ["novice", "easy", "intermediate", "advanced", "other"]
        .iter()
        .map(|d| {
            let (key, _, rgb) = difficulty_style(Some(d));
            style(&format!("piste-{key}"), rgb, 255, 4.0, None)
        })
        .collect()
}

fn yes_no(f: &Feature, k: &str, label: &str, out: &mut Vec<String>) {
    match prop(f, k).and_then(Value::as_bool) {
        Some(true) => out.push(format!("{label} : oui")),
        Some(false) => out.push(format!("{label} : non")),
        None => {}
    }
}

fn refuge_items(layer: &str, fc: &geojson::FeatureCollection) -> Vec<Item> {
    let mut items = Vec::new();
    for f in &fc.features {
        let Some(Geom::Point(x, y)) = geom_of(f) else {
            continue;
        };
        let mut desc = Vec::new();
        let (name, kind) = match layer {
            "refuges-info" => {
                let kind = prop_str(f, "type_label").unwrap_or_else(|| "point".to_owned());
                if let Some(s) = prop_str(f, "state") {
                    desc.push(format!("État : {s}"));
                }
                if let Some(p) = prop_str(f, "places") {
                    desc.push(format!("Places : {p}"));
                }
                if let Some(a) = prop_f64(f, "alt") {
                    desc.push(format!("Altitude : {a:.0} m"));
                }
                yes_no(f, "stove", "Poêle", &mut desc);
                yes_no(f, "fireplace", "Cheminée", &mut desc);
                yes_no(f, "blankets", "Couvertures", &mut desc);
                yes_no(f, "water_100m", "Eau à moins de 100 m", &mut desc);
                if let Some(c) = prop(f, "last_comment") {
                    let date = c
                        .get("date")
                        .and_then(Value::as_str)
                        .unwrap_or("")
                        .get(..10)
                        .unwrap_or("");
                    let text = c.get("text").and_then(Value::as_str).unwrap_or("");
                    desc.push(format!("Dernier commentaire ({date}) : {text}"));
                }
                if let Some(u) = prop_str(f, "url") {
                    desc.push(u);
                }
                (prop_str(f, "name").unwrap_or_else(|| kind.clone()), kind)
            }
            "refuges-osm" => {
                let kind = prop_str(f, "kind").unwrap_or_else(|| "abri".to_owned());
                let tags = prop(f, "tags").and_then(Value::as_object);
                let tag = |k: &str| {
                    tags.and_then(|t| t.get(k))
                        .and_then(Value::as_str)
                        .map(str::to_owned)
                };
                if let Some(c) = tag("capacity") {
                    desc.push(format!("Capacité : {c}"));
                }
                if let Some(w) = tag("winter_room") {
                    desc.push(format!("Local d'hiver : {w}"));
                }
                if let Some(o) = tag("opening_hours") {
                    desc.push(format!("Horaires : {o}"));
                }
                if let (Some(t), Some(id)) = (prop_str(f, "osm_type"), prop_str(f, "osm_id")) {
                    desc.push(format!("https://www.openstreetmap.org/{t}/{id}"));
                }
                (tag("name").unwrap_or_else(|| kind.clone()), kind)
            }
            _ => {
                let kind = prop_str(f, "waypoint_type").unwrap_or_else(|| "refuge".to_owned());
                if let Some(c) = prop_str(f, "capacity_unstaffed") {
                    desc.push(format!("Places hors gardiennage : {c}"));
                }
                if let Some(c) = prop_str(f, "custodianship") {
                    desc.push(format!("Gardiennage : {c}"));
                }
                if let Some(p) = prop_str(f, "access_period") {
                    desc.push(format!("Période : {p}"));
                }
                if let Some(e) = prop_f64(f, "elevation") {
                    desc.push(format!("Altitude : {e:.0} m"));
                }
                if let Some(u) = prop_str(f, "url") {
                    desc.push(u);
                }
                (prop_str(f, "title").unwrap_or_else(|| kind.clone()), kind)
            }
        };
        // Bus shelters (OSM) are not mountain shelters.
        if kind == "bus_shelter" {
            continue;
        }
        items.push(Item {
            name,
            desc,
            geom: Geom::Point(x, y),
            style: "refuge".to_owned(),
            folder: kind,
        });
    }
    items
}

fn zone_items(fc: &geojson::FeatureCollection) -> Vec<Item> {
    let mut items = Vec::new();
    for f in &fc.features {
        let Some(geom) = geom_of(f) else { continue };
        let kind = prop_str(f, "kind").unwrap_or_else(|| "zone".to_owned());
        // Whole-country / département / commune outlines carry the general rules; drawn
        // as zones they would cover the whole map.
        let admin = matches!(
            kind.as_str(),
            "country" | "department" | "commune" | "canton"
        );
        let mut desc = Vec::new();
        if let Some(m) = prop(f, "months").and_then(Value::as_array) {
            let ms: Vec<String> = m
                .iter()
                .filter_map(Value::as_u64)
                .map(|x| x.to_string())
                .collect();
            if !ms.is_empty() {
                desc.push(format!("Mois sensibles : {}", ms.join(", ")));
            }
        }
        match prop_str(f, "rules_text") {
            Some(r) => desc.push(format!("Règles :\n{r}")),
            None => desc.push("Aucune règle saisie pour cette zone.".to_owned()),
        }
        if let Some(u) = prop_str(f, "url") {
            desc.push(u);
        }
        let style = if admin {
            "zone-admin"
        } else if kind == "biodiv_sports" {
            "zone-biodiv"
        } else {
            "zone"
        };
        items.push(Item {
            name: prop_str(f, "name").unwrap_or_else(|| kind.clone()),
            desc,
            geom,
            style: style.to_owned(),
            folder: kind,
        });
    }
    items
}

/// Readable texts of a JSON document of the assistant: the `text` field (string or list
/// of strings) of the outermost objects that have one; the details below such an object
/// (reasons, per-hour values) are not repeated.
fn collect_texts(v: &Value, out: &mut Vec<String>) {
    match v {
        Value::Object(m) => match m.get("text") {
            Some(Value::String(t)) => out.push(t.clone()),
            Some(Value::Array(a)) if a.iter().all(Value::is_string) => {
                out.extend(a.iter().filter_map(Value::as_str).map(str::to_owned));
            }
            _ => m.values().for_each(|x| collect_texts(x, out)),
        },
        Value::Array(a) => a.iter().for_each(|x| collect_texts(x, out)),
        _ => {}
    }
}

/// « 13 km, ~2 h, retour 12 h 15 »: length to the kilometre, time to the quarter hour,
/// return time as given (`HH:MM`).
pub fn loop_name(length_km: f64, eta_min: f64, arrival: Option<&str>) -> String {
    let q = ((eta_min / 15.0).round() as i64 * 15).max(15);
    let dur = match (q / 60, q % 60) {
        (0, m) => format!("~{m} min"),
        (h, 0) => format!("~{h} h"),
        (h, m) => format!("~{h} h {m:02}"),
    };
    let mut s = format!("{} km, {dur}", length_km.round() as i64);
    if let Some((h, m)) = arrival.and_then(|a| a.split_once(':')) {
        let h = h.trim_start_matches('0');
        let h = if h.is_empty() { "0" } else { h };
        if m == "00" {
            let _ = write!(s, ", retour {h} h");
        } else {
            let _ = write!(s, ", retour {h} h {m}");
        }
    }
    s
}

/// Items of the suggestions GeoJSON of the assistant.
fn loop_items(fc: &geojson::FeatureCollection) -> Vec<Item> {
    fc.features
        .iter()
        .filter_map(|f| {
            let name = loop_name(
                prop_f64(f, "length_km").unwrap_or(0.0),
                prop_f64(f, "eta_min").unwrap_or(0.0),
                prop_str(f, "arrival").as_deref(),
            );
            let rank = prop_f64(f, "rank").unwrap_or(1.0) as u32;
            let mut desc = Vec::new();
            if let Some(r) = prop_str(f, "request") {
                desc.push(format!("Demande : {r} (choix n° {rank})"));
            }
            if let Some(l) = prop_str(f, "label") {
                desc.push(format!("Boucle : {l}"));
            }
            if let Some(Value::Array(r)) = prop(f, "reasons") {
                desc.extend(r.iter().filter_map(Value::as_str).map(str::to_owned));
            }
            desc.push("Suggestion de l'assistant, pas une garantie : vérifier neige, BERA et état des pistes.".to_owned());
            Some(Item {
                name,
                desc,
                geom: geom_of(f)?,
                style: if rank == 1 { "loop-best" } else { "loop" }.to_owned(),
                folder: prop_str(f, "request").unwrap_or_else(|| "Boucles".to_owned()),
            })
        })
        .collect()
}

/// Label of an exposure class of the `avalanche-exposure` layer (never « sûr »).
pub fn exposure_label(class: u64) -> &'static str {
    match class {
        1 => "terrain peu exposé (modèle)",
        2 => "exposé",
        3 => "très exposé",
        _ => "exposition inconnue",
    }
}

fn exposure_styles() -> Vec<Style> {
    vec![
        style("exposure-1", [0x4d, 0xaf, 0x4a], 200, 4.0, None),
        style("exposure-2", [0xff, 0x7f, 0x00], 230, 5.0, None),
        style("exposure-3", [0xe4, 0x1a, 0x1c], 255, 6.0, None),
    ]
}

/// Items of `segments.geojson`: name of the piste and class, class in the description.
fn exposure_items(fc: &geojson::FeatureCollection) -> Vec<Item> {
    fc.features
        .iter()
        .filter_map(|f| {
            let class = prop(f, "exposure").and_then(Value::as_u64).unwrap_or(0);
            let label = exposure_label(class);
            let piste = prop_str(f, "name")
                .or_else(|| match prop(f, "relations") {
                    Some(Value::Array(r)) if !r.is_empty() => Some(
                        r.iter()
                            .filter_map(Value::as_str)
                            .collect::<Vec<_>>()
                            .join(" / "),
                    ),
                    _ => None,
                })
                .unwrap_or_else(|| "Piste sans nom".to_owned());
            let mut desc = vec![format!("Exposition aux avalanches : {label}")];
            if let Some(len) = prop_f64(f, "length_m") {
                desc.push(format!("Longueur du tronçon : {len:.0} m"));
            }
            if let Some(id) = prop(f, "osm_way_id").and_then(Value::as_i64) {
                desc.push(format!("Way OSM {id}"));
            }
            desc.push("Classe d'un modèle de terrain (type ATES) et des avalanches historiques CLPA : jamais une garantie. Consultez le BERA.".to_owned());
            Some(Item {
                name: format!("{piste} — {label}"),
                desc,
                geom: geom_of(f)?,
                style: format!("exposure-{}", class.clamp(1, 3)),
                folder: label.to_owned(),
            })
        })
        .collect()
}

fn simple_items(
    fc: &geojson::FeatureCollection,
    name: &dyn Fn(&Feature) -> String,
    desc: &dyn Fn(&Feature) -> Vec<String>,
    style: &str,
    folder: &str,
) -> Vec<Item> {
    fc.features
        .iter()
        .filter_map(|f| {
            Some(Item {
                name: name(f),
                desc: desc(f),
                geom: geom_of(f)?,
                style: style.to_owned(),
                folder: folder.to_owned(),
            })
        })
        .collect()
}

/// One written file.
#[derive(Debug, Clone, Serialize)]
pub struct ExportedFile {
    /// Path relative to the export folder.
    pub path: String,
    /// Size.
    pub bytes: u64,
    /// Items (GPX/KML) or tiles (MBTiles/SQLiteDB).
    pub count: u64,
}

/// Result of an export.
#[derive(Debug, Clone, Serialize)]
pub struct ExportReport {
    /// Export folder.
    pub dir: PathBuf,
    /// Files written.
    pub files: Vec<ExportedFile>,
    /// Layers skipped, with the reason.
    pub skipped: Vec<(String, String)>,
    /// Total size.
    pub total_bytes: u64,
}

/// Highest zoom of the exported raster overlays: OsmAnd draws an overlay up to two zooms
/// above its highest zoom (`MapTileLayer.OVERZOOM_IN`), so tiles are upsampled
/// (nearest) from the pack's highest zoom to this one.
pub fn export_maxzoom(layer_maxzoom: u8) -> u8 {
    layer_maxzoom.max(15).max(layer_maxzoom + 1).min(16)
}

/// Child tile `(z, x, y)` cut from a palette tile at zoom `pz` (nearest upsampling).
fn upsample(parent: &png::Indexed, pz: u8, z: u8, x: u32, y: u32) -> Result<Vec<u8>> {
    let d = z - pz;
    let k = 1u32 << d;
    let ts = TILE_SIZE;
    let (ox, oy) = ((x % k) * ts / k, (y % k) * ts / k);
    let mut idx = Vec::with_capacity((ts * ts) as usize);
    for py in 0..ts {
        for px in 0..ts {
            idx.push(parent.index_at(ox + px / k, oy + py / k).unwrap_or(0));
        }
    }
    png::encode_indexed(ts, ts, &parent.palette, &idx)
}

/// Tiles of a palette raster layer, zoom `minzoom..=export_maxzoom`.
pub fn raster_tiles(pack: &Pack, layer: &Layer) -> Result<(u8, u8, Vec<db::DbTile>)> {
    let r = layer
        .raster
        .as_ref()
        .ok_or_else(|| Error::Pack(format!("{} is not a raster", layer.id)))?;
    let archive = pack.tiles(&layer.id)?;
    let b = archive.bounds();
    let bbox = skimap_common::geom::BBox::new(b[1], b[0], b[3], b[2]);
    let top = export_maxzoom(r.maxzoom);
    let mut out = Vec::new();
    for (z, x, y) in tiles_covering(bbox, r.minzoom, r.maxzoom) {
        if let Some(t) = archive.tile(z, x, y)? {
            out.push((z, x, y, t));
        }
    }
    let base: Vec<(u32, u32, png::Indexed)> = out
        .iter()
        .filter(|t| t.0 == r.maxzoom)
        .map(|(_, x, y, t)| Ok((*x, *y, png::decode_indexed(t)?)))
        .collect::<Result<_>>()?;
    for z in r.maxzoom + 1..=top {
        let d = z - r.maxzoom;
        for (px, py, img) in &base {
            for cx in px << d..(px + 1) << d {
                for cy in py << d..(py + 1) << d {
                    out.push((z, cx, cy, upsample(img, r.maxzoom, z, cx, cy)?));
                }
            }
        }
    }
    Ok((r.minzoom, top, out))
}

fn write_collection(
    dir: &Path,
    c: &Collection,
    generator: &str,
    time: &str,
    files: &mut Vec<ExportedFile>,
) -> Result<()> {
    let n = c.items.len() as u64;
    let g = gpx(c, generator, time);
    let p = format!("gpx/{}.gpx", c.stem);
    files.push(ExportedFile {
        bytes: write_bytes(&dir.join(&p), g.as_bytes())?,
        path: p,
        count: n,
    });
    let k = kml(c);
    let p = format!("kml/{}.kml", c.stem);
    files.push(ExportedFile {
        bytes: write_bytes(&dir.join(&p), k.as_bytes())?,
        path: p,
        count: n,
    });
    Ok(())
}

/// Exports the pack to `dir` (removed and recreated).
pub fn export(pack: &Pack, dir: &Path) -> Result<ExportReport> {
    if dir.exists() {
        std::fs::remove_dir_all(dir).map_err(|e| Error::io(dir, e))?;
    }
    create_dir(dir)?;
    let m = pack.manifest();
    let generated = m
        .generated_at
        .get(..10)
        .unwrap_or(&m.generated_at)
        .to_owned();
    let generator = format!("{} (pack {})", m.generator, m.pack_version);
    let time = m.generated_at.clone();
    let mut files = Vec::new();
    let mut skipped = Vec::new();
    let mut collections: Vec<(Collection, bool)> = Vec::new();

    let has = |id: &str| m.layer(id).is_some();
    if has("pistes") {
        let l = pack.layer("pistes")?;
        let items = piste_items(&pack.read_geojson("pistes")?);
        // KML: one file, a folder per colour. GPX: one file per colour, OsmAnd
        // colours a whole file more reliably than single tracks.
        collections.push((
            Collection {
                stem: "pistes".to_owned(),
                title: format!("Pistes nordiques — {}", m.site.name),
                about: about(l, &generated),
                styles: piste_styles(),
                items: items.clone(),
            },
            false,
        ));
        for d in ["novice", "easy", "intermediate", "advanced", "other"] {
            let (key, folder, _) = difficulty_style(Some(d));
            let sel: Vec<Item> = items
                .iter()
                .filter(|i| i.style == format!("piste-{key}"))
                .cloned()
                .collect();
            if sel.is_empty() {
                continue;
            }
            collections.push((
                Collection {
                    stem: format!("pistes-{key}"),
                    title: format!("{folder} — {}", m.site.name),
                    about: about(l, &generated),
                    styles: piste_styles(),
                    items: sel,
                },
                true,
            ));
        }
    }
    let refuge_layers = [
        ("refuges-info", "refuges-refuges-info"),
        ("refuges-osm", "refuges-osm"),
        ("refuges-camptocamp", "refuges-camptocamp"),
    ];
    for (id, stem) in refuge_layers {
        if !has(id) {
            continue;
        }
        let l = pack.layer(id)?;
        collections.push((
            Collection {
                stem: stem.to_owned(),
                title: format!("{} — {}", l.title, m.site.name),
                about: about(l, &generated),
                styles: vec![style("refuge", [0x8c, 0x2d, 0x04], 255, 2.0, None)],
                items: refuge_items(id, &pack.read_geojson(id)?),
            },
            false,
        ));
    }
    if has("zones") {
        let l = pack.layer("zones")?;
        collections.push((
            Collection {
                stem: "zones-protegees".to_owned(),
                title: format!("Zones protégées et règles — {}", m.site.name),
                about: about(l, &generated),
                styles: vec![
                    style(
                        "zone",
                        [0x1b, 0x7a, 0x3a],
                        230,
                        2.5,
                        Some([0x1b, 0x7a, 0x3a, 50]),
                    ),
                    style(
                        "zone-biodiv",
                        [0xe0, 0x6c, 0x00],
                        230,
                        2.5,
                        Some([0xe0, 0x6c, 0x00, 60]),
                    ),
                    style(
                        "zone-admin",
                        [0x60, 0x60, 0x60],
                        160,
                        1.5,
                        Some([0x60, 0x60, 0x60, 0]),
                    ),
                ],
                items: zone_items(&pack.read_geojson("zones")?),
            },
            false,
        ));
    }
    if has("no-network") {
        let l = pack.layer("no-network")?;
        let fc = pack.read_geojson("no-network")?;
        let items = simple_items(
            &fc,
            &|_| "Aucun réseau mobile (112 compris)".to_owned(),
            &|f| {
                let mut d = vec!["Aucun des quatre opérateurs ne couvre cette zone (2G/3G voix, cartes théoriques ARCEP) : le 112 ne passe pas.".to_owned()];
                if let Some(a) = prop_f64(f, "area_ha") {
                    d.push(format!("Surface : {a:.1} ha"));
                }
                if let Some(r) = prop_str(f, "release") {
                    d.push(format!("Cartes ARCEP : {r}"));
                }
                d
            },
            "no-network",
            "Sans réseau",
        );
        collections.push((
            Collection {
                stem: "sans-reseau".to_owned(),
                title: format!("Zones sans aucun réseau — {}", m.site.name),
                about: about(l, &generated),
                styles: vec![style(
                    "no-network",
                    [0x4a, 0x14, 0x86],
                    200,
                    1.0,
                    Some([0x4a, 0x14, 0x86, 70]),
                )],
                items,
            },
            false,
        ));
    }
    for (id, stem, title, st) in [
        (
            "avalanche-crossings",
            "avalanches-troncons",
            "Tronçons de pistes en zone d'avalanche connue",
            style("crossing", [0xe4, 0x1a, 0x1c], 255, 6.0, None),
        ),
        (
            "avalanche-areas",
            "avalanches-emprises",
            "Avalanches historiques (emprises CLPA)",
            style(
                "clpa",
                [0xe4, 0x1a, 0x1c],
                200,
                1.5,
                Some([0xe4, 0x1a, 0x1c, 60]),
            ),
        ),
    ] {
        if !has(id) {
            continue;
        }
        let l = pack.layer(id)?;
        let fc = pack.read_geojson(id)?;
        if fc.features.is_empty() {
            skipped.push((
                id.to_owned(),
                "couche vide pour ce site (pas de CLPA dans le Jura)".to_owned(),
            ));
            continue;
        }
        let crossing = id == "avalanche-crossings";
        let items = simple_items(
            &fc,
            &|f| {
                if crossing {
                    format!(
                        "{} — {:.0} m en zone d'avalanche",
                        prop_str(f, "name").unwrap_or_else(|| "Piste sans nom".to_owned()),
                        prop_f64(f, "length_m").unwrap_or(0.0)
                    )
                } else {
                    format!(
                        "Avalanche historique CLPA n° {}",
                        prop_str(f, "ddt05_index").unwrap_or_default()
                    )
                }
            },
            &|f| {
                let mut d = Vec::new();
                if crossing {
                    if let Some(a) = prop_str(f, "alert") {
                        d.push(a);
                    }
                    if let Some(l) = prop_f64(f, "length_m") {
                        d.push(format!("Longueur dans l'emprise : {l:.0} m"));
                    }
                    if let Some(Value::Array(r)) = prop(f, "risque") {
                        let r: Vec<&str> = r.iter().filter_map(Value::as_str).collect();
                        d.push(format!("Aléa DDT 05 : {}", r.join(", ")));
                    }
                } else {
                    for (k, label) in [
                        ("information", "Information"),
                        ("risque", "Aléa"),
                        ("source", "Source"),
                        ("commune", "Commune"),
                    ] {
                        if let Some(v) = prop_str(f, k) {
                            d.push(format!("{label} : {v}"));
                        }
                    }
                }
                d.push("Zone historique connue seulement : une avalanche peut se produire ailleurs. Consultez le BERA.".to_owned());
                d
            },
            &st.id.clone(),
            title,
        );
        collections.push((
            Collection {
                stem: stem.to_owned(),
                title: format!("{title} — {}", m.site.name),
                about: about(l, &generated),
                styles: vec![st],
                items,
            },
            false,
        ));
    }

    // Pistes by avalanche exposure class (P17, P18): one item per run of a class.
    if has("avalanche-segments") {
        let l = pack.layer("avalanche-segments")?;
        let fc = pack.read_geojson("avalanche-segments")?;
        let items = exposure_items(&fc);
        collections.push((
            Collection {
                stem: "avalanches-exposition".to_owned(),
                title: format!("Pistes par exposition aux avalanches — {}", m.site.name),
                about: about(l, &generated),
                styles: exposure_styles(),
                items,
            },
            false,
        ));
    }

    // Suggested loops of the assistant (A5): « 13 km, ~2 h, retour 12 h 15 ».
    if has("assist-suggestions") {
        let l = pack.layer("assist-suggestions")?;
        let fc = pack.read_geojson("assist-suggestions")?;
        collections.push((
            Collection {
                stem: "boucles-suggerees".to_owned(),
                title: format!("{} — {}", l.title, m.site.name),
                about: about(l, &generated),
                styles: vec![
                    style("loop-best", [0x00, 0x6d, 0xa8], 230, 5.0, None),
                    style("loop", [0x6a, 0x9f, 0xc8], 200, 4.0, None),
                ],
                items: loop_items(&fc),
            },
            false,
        ));
    }
    // Texts of the assistant (A1 briefing, A2 wax, A3 snow quality) for reading on the
    // phone next to the map.
    let mut texts = String::new();
    for id in ["assist-briefing", "assist-wax", "assist-snow-quality"] {
        if !has(id) {
            continue;
        }
        let l = pack.layer(id)?;
        let v = pack.read_json(id)?;
        let mut lines = Vec::new();
        collect_texts(&v, &mut lines);
        if lines.is_empty() {
            continue;
        }
        let _ = writeln!(texts, "== {}\n", l.title);
        for t in lines {
            let _ = writeln!(texts, "{t}");
        }
        texts.push('\n');
    }
    if !texts.is_empty() {
        texts.push_str("Sorties de l'assistant (skimap assist) : jamais une garantie de sécurité ; vérifier le BERA, la neige et les règles avant de partir.\n");
        let p = "assistant/assistant.txt".to_owned();
        files.push(ExportedFile {
            bytes: write_bytes(&dir.join(&p), texts.as_bytes())?,
            path: p,
            count: 0,
        });
    }

    // Itineraries: proof routes on trails (C1, C5) and off-piste (C3).
    let route_layers: Vec<&Layer> = m
        .layers
        .iter()
        .filter(|l| l.id == "routes" || l.id.starts_with("route-offpiste-"))
        .collect();
    if !route_layers.is_empty() {
        let mut items = Vec::new();
        let mut about_lines = Vec::new();
        for l in &route_layers {
            about_lines.extend(about(l, &generated));
            let fc = pack.read_geojson(&l.id)?;
            for f in &fc.features {
                let kind = prop_str(f, "kind").unwrap_or_default();
                let Some(geom) = geom_of(f) else { continue };
                match (kind.as_str(), &geom) {
                    ("route", Geom::Lines(_)) => {
                        let offpiste = l.id != "routes";
                        let sum = prop(f, "summary");
                        let num = |k: &str| {
                            prop_f64(f, k)
                                .or_else(|| sum.and_then(|s| s.get(k)).and_then(Value::as_f64))
                        };
                        let name = if offpiste {
                            format!(
                                "Hors piste : {}",
                                l.title
                                    .trim_start_matches("Itinéraire hors piste de preuve : ")
                            )
                        } else {
                            format!(
                                "{} → {} ({})",
                                prop_str(f, "from").unwrap_or_default(),
                                prop_str(f, "to").unwrap_or_default(),
                                prop_str(f, "model").unwrap_or_default()
                            )
                        };
                        let mut desc = Vec::new();
                        if let Some(v) = num("length_m") {
                            desc.push(format!("Longueur : {:.1} km", v / 1000.0));
                        }
                        if let Some(v) = num("gain_m").or_else(|| num("ascent_m")) {
                            desc.push(format!("Montée : {v:.0} m"));
                        }
                        if let Some(v) = num("eta_classic_s") {
                            desc.push(format!(
                                "Temps estimé classique : {} h {:02}",
                                (v / 3600.0) as u32,
                                ((v % 3600.0) / 60.0) as u32
                            ));
                        }
                        if let Some(c) = sum.and_then(|s| s.get("cost_hm")).and_then(Value::as_str)
                        {
                            desc.push(format!("Coût du routage : {c}"));
                        }
                        if let Some(Value::Array(n)) = prop(f, "names") {
                            let n: Vec<&str> = n.iter().filter_map(Value::as_str).collect();
                            desc.push(format!("Pistes : {}", n.join(", ")));
                        }
                        desc.push("Itinéraire de preuve du routage, pas une recommandation : vérifier neige, BERA et zones.".to_owned());
                        items.push(Item {
                            name,
                            desc,
                            geom,
                            style: if offpiste { "route-offpiste" } else { "route" }.to_owned(),
                            folder: if offpiste { "Hors piste" } else { "Sur pistes" }.to_owned(),
                        });
                    }
                    ("endpoint", Geom::Point(..)) => items.push(Item {
                        name: prop_str(f, "name").unwrap_or_else(|| "Extrémité".to_owned()),
                        desc: vec!["Extrémité d'itinéraire de preuve".to_owned()],
                        geom,
                        style: "route".to_owned(),
                        folder: "Extrémités".to_owned(),
                    }),
                    _ => {}
                }
            }
        }
        about_lines.dedup();
        collections.push((
            Collection {
                stem: "itineraires".to_owned(),
                title: format!("Itinéraires de preuve — {}", m.site.name),
                about: about_lines,
                styles: vec![
                    style("route", [0x00, 0x6d, 0xa8], 230, 5.0, None),
                    style("route-offpiste", [0xff, 0x7f, 0x00], 230, 5.0, None),
                ],
                items,
            },
            false,
        ));
    }

    for (c, gpx_only) in &collections {
        if *gpx_only {
            let g = gpx(c, &generator, &time);
            let p = format!("gpx/{}.gpx", c.stem);
            files.push(ExportedFile {
                bytes: write_bytes(&dir.join(&p), g.as_bytes())?,
                path: p,
                count: c.items.len() as u64,
            });
        } else if c.stem == "pistes" {
            // Pistes: KML only, the GPX are split by colour.
            let k = kml(c);
            let p = "kml/pistes.kml".to_owned();
            files.push(ExportedFile {
                bytes: write_bytes(&dir.join(&p), k.as_bytes())?,
                path: p,
                count: c.items.len() as u64,
            });
        } else {
            write_collection(dir, c, &generator, &time, &mut files)?;
        }
    }

    // Raster overlays.
    let mut overlays = Vec::new();
    for l in m.layers.iter().filter(|l| l.raster.is_some()) {
        let r = l
            .raster
            .as_ref()
            .ok_or_else(|| Error::Pack("raster".into()))?;
        if r.encoding != ValueEncoding::PaletteIndex {
            skipped.push((
                l.id.clone(),
                "tuiles IGN de comparaison : non exportées (usage hors ligne de la carte des pentes IGN à confirmer)".to_owned(),
            ));
            continue;
        }
        let (minz, maxz, tiles) = raster_tiles(pack, l)?;
        let stem = match l.id.as_str() {
            "slope" => "pentes",
            "snow-depth" => "neige-hauteur",
            "snow-groomed" => "neige-piste-damee",
            "snow-srn" => "neige-hors-piste",
            "avalanche-exposure" => "exposition-avalanches",
            other => other,
        };
        let attribution: Vec<String> = l
            .sources
            .iter()
            .map(|s| s.attribution.clone())
            .filter(|a| !a.is_empty())
            .collect();
        let desc = format!(
            "{} ; légende : {} ; pack {}",
            l.title,
            r.legend
                .iter()
                .filter(|e| !e.color.ends_with("00"))
                .map(|e| format!("{} {}", e.label, e.color))
                .collect::<Vec<_>>()
                .join(", "),
            m.pack_version
        );
        let archive = pack.tiles(&l.id)?;
        let o = db::Overlay {
            name: &format!("skimap {} — {}", l.title, m.site.name),
            description: &desc,
            attribution: &attribution.join(" ; "),
            bounds: archive.bounds(),
            minzoom: minz,
            maxzoom: maxz,
        };
        let p = format!("locus/{stem}.mbtiles");
        files.push(ExportedFile {
            bytes: db::write_mbtiles(&dir.join(&p), &o, &tiles)?,
            path: p,
            count: tiles.len() as u64,
        });
        let p = format!("osmand/{stem}.sqlitedb");
        files.push(ExportedFile {
            bytes: db::write_sqlitedb(&dir.join(&p), &o, &tiles)?,
            path: p,
            count: tiles.len() as u64,
        });
        overlays.push((l.clone(), stem.to_owned(), minz, maxz));
    }

    let readme = readme::lisezmoi(m, &files, &skipped, &overlays);
    for (p, text) in readme::split_parts(&readme) {
        files.push(ExportedFile {
            bytes: write_bytes(&dir.join(&p), text.as_bytes())?,
            path: p,
            count: 0,
        });
    }
    let total_bytes = files.iter().map(|f| f.bytes).sum();
    Ok(ExportReport {
        dir: dir.to_path_buf(),
        files,
        skipped,
        total_bytes,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Collection {
        Collection {
            stem: "t".into(),
            title: "Titre & <test>".into(),
            about: vec!["Source : OSM".into()],
            styles: vec![style("s", [255, 0, 0], 255, 4.0, Some([0, 255, 0, 80]))],
            items: vec![
                Item {
                    name: "Refuge \"A\"".into(),
                    desc: vec!["Places : 9".into(), "Commentaire : <ok>".into()],
                    geom: Geom::Point(6.75, 44.85),
                    style: "s".into(),
                    folder: "hut".into(),
                },
                Item {
                    name: "Piste".into(),
                    desc: vec!["rouge".into()],
                    geom: Geom::Lines(vec![vec![(6.7, 44.8), (6.71, 44.81)]]),
                    style: "s".into(),
                    folder: "rouges".into(),
                },
                Item {
                    name: "Zone".into(),
                    desc: vec![],
                    geom: Geom::Polygons(vec![vec![
                        vec![(6.0, 44.0), (6.1, 44.0), (6.1, 44.1), (6.0, 44.0)],
                        vec![(6.02, 44.01), (6.05, 44.01), (6.05, 44.03), (6.02, 44.01)],
                    ]]),
                    style: "s".into(),
                    folder: "zones".into(),
                },
            ],
        }
    }

    #[test]
    fn gpx_is_well_formed_and_ordered() {
        let g = gpx(&sample(), "skimap", "2026-11-15T00:00:00Z");
        assert!(g.contains("<wpt lat=\"44.85\" lon=\"6.75\"><name>Refuge &quot;A&quot;</name>"));
        assert!(g.contains("<osmand:color>#FFFF0000</osmand:color>"));
        assert!(g.contains("<gpx_style:color>FF0000</gpx_style:color>"));
        // waypoints before tracks (GPX 1.1 sequence), two segments for the polygon with a hole
        assert!(g.find("<wpt").unwrap() < g.find("<trk>").unwrap());
        assert_eq!(g.matches("<trkseg>").count(), 3);
        assert!(g.contains("Titre &amp; &lt;test&gt;"));
    }

    #[test]
    fn kml_styles_and_geometries() {
        let k = kml(&sample());
        assert!(k.contains("<LineStyle><color>ff0000ff</color><width>4</width></LineStyle>"));
        assert!(k.contains("<PolyStyle><color>5000ff00</color>"));
        assert!(k.contains("<innerBoundaryIs>"));
        assert!(k.contains("Places : 9&lt;br/&gt;Commentaire : &amp;lt;ok&amp;gt;"));
        assert_eq!(k.matches("<Folder>").count(), 3);
    }

    #[test]
    fn exposure_segments_are_named_by_class() {
        let fc: geojson::FeatureCollection = serde_json::from_value(serde_json::json!({
            "type": "FeatureCollection",
            "features": [
                {"type": "Feature", "geometry": {"type": "LineString", "coordinates": [[6.7, 44.8], [6.71, 44.81]]},
                 "properties": {"exposure": 3, "length_m": 120.4, "name": null, "relations": ["Izoard"], "osm_way_id": 7}},
                {"type": "Feature", "geometry": {"type": "LineString", "coordinates": [[6.7, 44.8], [6.72, 44.8]]},
                 "properties": {"exposure": 1, "length_m": 80.0, "name": "Le Cancel"}}
            ]
        }))
        .unwrap();
        let items = exposure_items(&fc);
        assert_eq!(items[0].name, "Izoard — très exposé");
        assert_eq!(items[0].style, "exposure-3");
        assert!(items[0].desc[0].ends_with("très exposé"));
        assert_eq!(items[1].name, "Le Cancel — terrain peu exposé (modèle)");
        for i in &items {
            assert!(!i.name.contains("sûr"));
        }
        let c = Collection {
            stem: "avalanches-exposition".into(),
            title: "t".into(),
            about: vec![],
            styles: exposure_styles(),
            items,
        };
        let k = kml(&c);
        assert!(k.contains("<styleUrl>#exposure-3</styleUrl>"), "{k}");
        let g = gpx(&c, "skimap", "2026-11-15T00:00:00Z");
        assert!(g.contains("Exposition aux avalanches : très exposé"));
    }

    #[test]
    fn loop_names_read_aloud() {
        assert_eq!(
            loop_name(12.8, 118.0, Some("12:15")),
            "13 km, ~2 h, retour 12 h 15"
        );
        assert_eq!(
            loop_name(10.03, 75.0, Some("16:00")),
            "10 km, ~1 h 15, retour 16 h"
        );
        assert_eq!(
            loop_name(3.2, 20.0, Some("09:45")),
            "3 km, ~15 min, retour 9 h 45"
        );
        assert_eq!(loop_name(15.59, 181.0, None), "16 km, ~3 h");
        let mut t = Vec::new();
        collect_texts(
            &serde_json::json!({"days": [
                {"text": ["a", "b"], "reasons": [{"text": "detail"}]},
                {"points": [{"text": "c"}]}
            ]}),
            &mut t,
        );
        assert_eq!(t, ["a", "b", "c"]);
    }

    #[test]
    fn upsampling_keeps_values() {
        let mut idx = vec![0u8; 256 * 256];
        for y in 0..256usize {
            for x in 0..256usize {
                idx[y * 256 + x] = ((x / 64 + y / 64) % 3) as u8;
            }
        }
        let pal = vec![[0, 0, 0, 0], [1, 1, 1, 255], [2, 2, 2, 255]];
        let parent = png::Indexed {
            width: 256,
            height: 256,
            palette: pal.clone(),
            indices: idx.clone(),
        };
        // Child (z+1) at (2x+1, 2y): east half, north half of the parent.
        let child =
            png::decode_indexed(&upsample(&parent, 10, 11, 2 * 5 + 1, 2 * 7).unwrap()).unwrap();
        for py in (0..256).step_by(7) {
            for px in (0..256).step_by(5) {
                assert_eq!(
                    child.indices[py * 256 + px],
                    idx[(py / 2) * 256 + 128 + px / 2]
                );
            }
        }
        assert_eq!(export_maxzoom(15), 16);
        assert_eq!(export_maxzoom(13), 15);
        assert_eq!(export_maxzoom(16), 16);
    }
}
