//! Camptocamp waypoints (CC BY-SA 3.0, licence per document): huts, shelters and gîtes of
//! a box, with the unstaffed capacity and equipment of each.
//!
//! The listing (`/waypoints?wtyp=hut,shelter,gite&bbox=` in EPSG:3857) gives the type,
//! the position and the country; the capacity comes from each document
//! (`/waypoints/<id>?cook=fr`).

use serde::Serialize;
use serde_json::Value;
use skimap_common::crs::{Crs, from_web_mercator, project_bbox};
use skimap_common::download::{Downloader, Fetched, Request};
use skimap_common::geom::{BBox, LonLat};

use crate::error::{RefugesError, Result};

/// Downloader source name.
pub const SOURCE: &str = "camptocamp";
/// API root.
pub const API: &str = "https://api.camptocamp.org";
/// Licence (each document carries its own; all seen are CC BY-SA 3.0).
pub const LICENCE: &str = "CC BY-SA 3.0";
/// Attribution.
pub const ATTRIBUTION: &str = "© les contributeurs de Camptocamp.org (CC BY-SA 3.0)";
/// Waypoint types kept.
pub const TYPES: &str = "hut,shelter,gite";
/// Page size of the listing.
const PAGE: usize = 100;

/// A Camptocamp waypoint.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Waypoint {
    /// Document id.
    pub id: u64,
    /// Title (French locale if any).
    pub title: String,
    /// `hut`, `shelter` or `gite`.
    pub waypoint_type: String,
    /// Position.
    #[serde(skip)]
    pub pos: LonLat,
    /// Elevation, m.
    pub elevation: Option<f64>,
    /// Country (area of type `country`).
    pub country: Option<String>,
    /// Places when not staffed (winter room).
    pub capacity_unstaffed: Option<i64>,
    /// Places when staffed.
    pub capacity_staffed: Option<i64>,
    /// `custodianship`: `accessible_when_wardened`, `always_accessible`,
    /// `key_needed`, `no_warden`.
    pub custodianship: Option<String>,
    /// Mattresses when unstaffed.
    pub mattress_unstaffed: Option<bool>,
    /// Blankets when unstaffed.
    pub blanket_unstaffed: Option<bool>,
    /// Gas when unstaffed.
    pub gas_unstaffed: Option<bool>,
    /// Heating when unstaffed.
    pub heating_unstaffed: Option<bool>,
    /// Staffed period (text).
    pub access_period: Option<String>,
    /// Quality of the document.
    pub quality: Option<String>,
    /// Page on camptocamp.org.
    pub url: String,
    /// Licence of the document.
    pub licence: Option<String>,
    /// History page (authors) required for attribution.
    pub attribution_url: Option<String>,
}

/// Strips HTML tags and decodes the common entities.
pub fn strip_html(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut in_tag = false;
    for c in s.chars() {
        match c {
            '<' => in_tag = true,
            '>' => in_tag = false,
            c if !in_tag => out.push(c),
            _ => {}
        }
    }
    out.replace("&nbsp;", " ")
        .replace("&amp;", "&")
        .replace("&#39;", "'")
        .replace("&quot;", "\"")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn title_of(doc: &Value) -> String {
    let locales = doc.get("locales").and_then(Value::as_array);
    let pick = |lang: Option<&str>| {
        locales.and_then(|ls| {
            ls.iter()
                .find(|l| lang.is_none_or(|x| l.get("lang").and_then(Value::as_str) == Some(x)))
                .and_then(|l| l.get("title").and_then(Value::as_str))
        })
    };
    doc.pointer("/cooked/title")
        .and_then(Value::as_str)
        .or_else(|| pick(Some("fr")))
        .or_else(|| pick(None))
        .unwrap_or_default()
        .to_owned()
}

fn position(doc: &Value) -> Result<LonLat> {
    let id = doc.get("document_id").and_then(Value::as_u64).unwrap_or(0);
    let geom = doc
        .pointer("/geometry/geom")
        .and_then(Value::as_str)
        .ok_or_else(|| RefugesError::format(SOURCE, format!("waypoint {id} without geometry")))?;
    let g: Value = serde_json::from_str(geom)
        .map_err(|e| RefugesError::format(SOURCE, format!("waypoint {id}: {e}")))?;
    let c = g
        .get("coordinates")
        .and_then(Value::as_array)
        .ok_or_else(|| RefugesError::format(SOURCE, format!("waypoint {id}: no coordinates")))?;
    match (
        c.first().and_then(Value::as_f64),
        c.get(1).and_then(Value::as_f64),
    ) {
        (Some(x), Some(y)) => Ok(from_web_mercator(x, y)?),
        _ => Err(RefugesError::format(
            SOURCE,
            format!("waypoint {id}: bad coordinates"),
        )),
    }
}

/// Parses a waypoint document (listing entry or full document).
pub fn parse_waypoint(doc: &Value) -> Result<Waypoint> {
    let id = doc
        .get("document_id")
        .and_then(Value::as_u64)
        .ok_or_else(|| RefugesError::format(SOURCE, "waypoint without document_id"))?;
    let country = doc
        .get("areas")
        .and_then(Value::as_array)
        .and_then(|a| {
            a.iter()
                .find(|x| x.get("area_type").and_then(Value::as_str) == Some("country"))
        })
        .map(title_of);
    let b = |k: &str| doc.get(k).and_then(Value::as_bool);
    let i = |k: &str| doc.get(k).and_then(Value::as_i64);
    Ok(Waypoint {
        id,
        title: title_of(doc),
        waypoint_type: doc
            .get("waypoint_type")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned(),
        pos: position(doc)?,
        elevation: doc.get("elevation").and_then(Value::as_f64),
        country,
        capacity_unstaffed: i("capacity"),
        capacity_staffed: i("capacity_staffed"),
        custodianship: doc
            .get("custodianship")
            .and_then(Value::as_str)
            .map(str::to_owned),
        mattress_unstaffed: b("matress_unstaffed"),
        blanket_unstaffed: b("blanket_unstaffed"),
        gas_unstaffed: b("gas_unstaffed"),
        heating_unstaffed: b("heating_unstaffed"),
        access_period: doc
            .pointer("/cooked/access_period")
            .and_then(Value::as_str)
            .map(strip_html)
            .filter(|s| !s.is_empty()),
        quality: doc
            .get("quality")
            .and_then(Value::as_str)
            .map(str::to_owned),
        url: format!("https://www.camptocamp.org/waypoints/{id}"),
        licence: doc
            .pointer("/license/id")
            .and_then(Value::as_str)
            .map(str::to_owned),
        attribution_url: doc
            .pointer("/license/attribution_url")
            .and_then(Value::as_str)
            .map(str::to_owned),
    })
}

/// What [`fetch`] downloaded.
#[derive(Debug, Clone)]
pub struct Fetch {
    /// Waypoints, with the details of each document.
    pub waypoints: Vec<Waypoint>,
    /// Listing pages.
    pub listing: Vec<Fetched>,
    /// Number of documents fetched.
    pub documents: usize,
}

/// Lists the huts, shelters and gîtes of `bbox` and fetches each document.
pub fn fetch(dl: &Downloader, bbox: &BBox) -> Result<Fetch> {
    let (x1, y1, x2, y2) = project_bbox(*bbox, Crs::WebMercator, 16)?;
    let mut listing = Vec::new();
    let mut ids = Vec::new();
    let mut offset = 0;
    loop {
        let url = format!(
            "{API}/waypoints?wtyp={TYPES}&bbox={x1:.0},{y1:.0},{x2:.0},{y2:.0}&limit={PAGE}&offset={offset}&pl=fr"
        );
        let f = dl.fetch(&Request::get(SOURCE, &url))?;
        let v: Value = f.json()?;
        let docs = v
            .get("documents")
            .and_then(Value::as_array)
            .ok_or_else(|| RefugesError::format(SOURCE, "listing without documents"))?;
        let total = v.get("total").and_then(Value::as_u64).unwrap_or(0) as usize;
        for d in docs {
            if let Some(id) = d.get("document_id").and_then(Value::as_u64) {
                ids.push(id);
            }
        }
        let n = docs.len();
        listing.push(f);
        offset += PAGE;
        if n < PAGE || offset >= total {
            break;
        }
    }
    let mut waypoints = Vec::with_capacity(ids.len());
    for id in &ids {
        let f = dl.fetch(&Request::get(
            SOURCE,
            &format!("{API}/waypoints/{id}?cook=fr"),
        ))?;
        let w = parse_waypoint(&f.json()?)?;
        // The listing box is projected; keep only what is in the WGS84 box.
        if bbox.contains(w.pos) {
            waypoints.push(w);
        }
    }
    waypoints.sort_by_key(|w| w.id);
    Ok(Fetch {
        documents: ids.len(),
        waypoints,
        listing,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn parse_document() {
        let doc = json!({
            "document_id": 162324,
            "waypoint_type": "hut",
            "elevation": 2040,
            "capacity": 9,
            "capacity_staffed": 62,
            "custodianship": "always_accessible",
            "matress_unstaffed": false,
            "heating_unstaffed": false,
            "quality": "medium",
            "geometry": {"geom": "{\"type\": \"Point\", \"coordinates\": [758734.83625301, 5597109.178331737]}"},
            "areas": [{"area_type": "admin_limits", "locales": [{"lang": "fr", "title": "Hautes-Alpes"}]},
                      {"area_type": "country", "locales": [{"lang": "fr", "title": "France"}]}],
            "locales": [{"lang": "fr", "title": "Les Fonts de Cervières"}],
            "cooked": {"title": "Les Fonts de Cervières", "access_period": "<p>01/03&nbsp;au 01/05&nbsp;et du 15/06&nbsp;au 15/09</p>"},
            "license": {"id": "CC-BY-SA-3.0", "attribution_url": "https://www.camptocamp.org/waypoints/history/162324/fr"}
        });
        let w = parse_waypoint(&doc).unwrap();
        assert_eq!(w.title, "Les Fonts de Cervières");
        assert_eq!(w.country.as_deref(), Some("France"));
        assert_eq!(w.capacity_unstaffed, Some(9));
        assert_eq!(w.capacity_staffed, Some(62));
        assert_eq!(w.custodianship.as_deref(), Some("always_accessible"));
        assert_eq!(w.mattress_unstaffed, Some(false));
        assert_eq!(w.blanket_unstaffed, None);
        assert_eq!(
            w.access_period.as_deref(),
            Some("01/03 au 01/05 et du 15/06 au 15/09")
        );
        assert_eq!(w.licence.as_deref(), Some("CC-BY-SA-3.0"));
        // EPSG:3857 → WGS84 (spherical Mercator, checked with the closed formula): 6.81583 E, 44.84472 N.
        assert!((w.pos.lon - 6.815831).abs() < 1e-6, "{:?}", w.pos);
        assert!((w.pos.lat - 44.844722).abs() < 1e-6, "{:?}", w.pos);
        assert!(parse_waypoint(&json!({"document_id": 1})).is_err());
    }

    #[test]
    fn html() {
        assert_eq!(strip_html("<p>a&nbsp;<b>b</b></p>\n<p>c</p>"), "a b c");
    }
}
