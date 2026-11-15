//! Camptocamp routes (CC BY-SA 3.0, licence carried by each document).
//!
//! Camptocamp has no nordic activity (report 04): SRN is filed as `skitouring` or
//! `snowshoeing`. Two listings over the France area (`a=14274`):
//! - `act=skitouring&trat=1.1,1.3`: ski rating 1.1 to 1.3 (slopes under 30°);
//! - `q=nordique` and `q=ski de fond`: routes that say they are nordic.
//!
//! Only documents with a detailed geometry (`has_geom_detail`) give an itinerary; each is
//! fetched once (`/routes/<id>?cook=fr`, cached [`DOC_MAX_AGE`]).

use std::collections::BTreeMap;
use std::time::Duration;

use serde_json::Value;
use skimap_common::crs::from_web_mercator;
use skimap_common::download::{Downloader, Fetched, Request};
use skimap_common::geom::LonLat;

use crate::error::{Result, SrnError};
use crate::route::{Activity, Hint, Provider, Route, Shape};

/// Downloader source name.
pub const SOURCE: &str = "camptocamp";
/// API root.
pub const API: &str = "https://api.camptocamp.org";
/// Area id of France on Camptocamp.
pub const FRANCE_AREA: u64 = 14274;
/// Licence (each document carries its own; all seen are CC BY-SA 3.0).
pub const LICENCE: &str = "CC BY-SA 3.0";
/// Attribution.
pub const ATTRIBUTION: &str = "© les contributeurs de Camptocamp.org (CC BY-SA 3.0)";
/// Easy ski touring seeds an area up to this ascent, m.
pub const EASY_MAX_ASCENT_M: f64 = 500.0;
/// Highest ski rating of an easy route (`1.1` to `1.3`).
pub const EASY_MAX_RATING: &str = "1.3";
/// Highest ski rating of a route that says it is nordic.
pub const NORDIC_MAX_RATING: &str = "2.1";
/// Cache lifetime of listings and documents.
pub const DOC_MAX_AGE: Duration = Duration::from_secs(30 * 86_400);
const PAGE: usize = 100;

/// Listings queried (query string without paging).
pub fn listings() -> Vec<String> {
    vec![
        format!("act=skitouring&trat=1.1,{EASY_MAX_RATING}&a={FRANCE_AREA}"),
        format!("q=nordique&a={FRANCE_AREA}"),
        format!("q=ski%20de%20fond&a={FRANCE_AREA}"),
    ]
}

fn title(doc: &Value) -> String {
    let l = doc
        .pointer("/cooked")
        .or_else(|| doc.pointer("/locales/0"))
        .cloned()
        .unwrap_or(Value::Null);
    let prefix = l
        .get("title_prefix")
        .and_then(Value::as_str)
        .map(strip_html)
        .unwrap_or_default();
    let t = l.get("title").and_then(Value::as_str).unwrap_or_default();
    if prefix.is_empty() {
        t.to_owned()
    } else {
        format!("{prefix} : {t}")
    }
}

fn strip_html(s: &str) -> String {
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
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Whether a title says the route is nordic (SRN).
/// Place names that hold the word without saying the route is SRN.
const NORDIC_PLACES: [&str; 5] = [
    "site nordique",
    "domaine nordique",
    "centre nordique",
    "foyer nordique",
    "espace nordique",
];

/// Whether a title says the route is nordic (SRN), not only that it starts at a nordic
/// site.
pub fn says_nordic(title: &str) -> bool {
    let mut t = title.to_lowercase();
    for p in NORDIC_PLACES {
        t = t.replace(p, "");
    }
    t.contains("nordique") || t.contains("ski de fond")
}

/// Activity of a route document, `None` when it is not kept.
pub fn activity(doc: &Value) -> Option<Activity> {
    let acts: Vec<&str> = doc
        .get("activities")
        .and_then(Value::as_array)
        .map(|a| a.iter().filter_map(Value::as_str).collect())
        .unwrap_or_default();
    let ski = acts.contains(&"skitouring");
    let snowshoe = acts.contains(&"snowshoeing");
    if !ski && !snowshoe {
        return None;
    }
    let rating = doc.get("ski_rating").and_then(Value::as_str);
    let ascent = doc.get("height_diff_up").and_then(Value::as_f64);
    if ski && says_nordic(&title(doc)) {
        // A steep descent is not SRN, whatever the title says.
        return match rating {
            Some(r) if r > NORDIC_MAX_RATING => None,
            _ => Some(Activity::Srn),
        };
    }
    match rating {
        Some(r) if ski && r <= EASY_MAX_RATING => {
            if ascent.is_some_and(|a| a <= EASY_MAX_ASCENT_M) {
                Some(Activity::EasySkiTouring)
            } else {
                Some(Activity::SkiTouring)
            }
        }
        _ => None,
    }
}

/// Polylines of a `geom_detail` (EPSG:3857 LineString or MultiLineString).
pub fn detail_lines(doc: &Value) -> Result<Vec<Vec<LonLat>>> {
    let Some(g) = doc.pointer("/geometry/geom_detail").and_then(Value::as_str) else {
        return Ok(Vec::new());
    };
    let g: Value = serde_json::from_str(g).map_err(|e| SrnError::format(SOURCE, e.to_string()))?;
    let line = |v: &Value| -> Result<Vec<LonLat>> {
        let mut out = Vec::new();
        for c in v.as_array().into_iter().flatten() {
            if let (Some(x), Some(y)) = (
                c.get(0).and_then(Value::as_f64),
                c.get(1).and_then(Value::as_f64),
            ) {
                out.push(from_web_mercator(x, y)?);
            }
        }
        Ok(out)
    };
    let coords = g.get("coordinates").cloned().unwrap_or(Value::Null);
    let lines = match g.get("type").and_then(Value::as_str) {
        Some("LineString") => vec![line(&coords)?],
        Some("MultiLineString") => coords
            .as_array()
            .into_iter()
            .flatten()
            .map(line)
            .collect::<Result<Vec<_>>>()?,
        _ => Vec::new(),
    };
    Ok(lines.into_iter().filter(|l| l.len() >= 2).collect())
}

/// Itinerary of a full route document, `None` when not kept or without detail.
pub fn parse_route(doc: &Value) -> Result<Option<Route>> {
    let Some(activity) = activity(doc) else {
        return Ok(None);
    };
    let id = doc
        .get("document_id")
        .and_then(Value::as_u64)
        .ok_or_else(|| SrnError::format(SOURCE, "route without document_id"))?;
    let lines = detail_lines(doc)?;
    if lines.is_empty() {
        return Ok(None);
    }
    let types: Vec<&str> = doc
        .get("route_types")
        .and_then(Value::as_array)
        .map(|a| a.iter().filter_map(Value::as_str).collect())
        .unwrap_or_default();
    let shape = if types.contains(&"traverse") {
        Shape::Traverse
    } else if types.contains(&"loop") || types.contains(&"loop_hut") {
        Shape::Loop
    } else if types.contains(&"return_same_way") {
        Shape::OutAndBack
    } else {
        Route::shape_from_ends(&lines)
    };
    let licence = doc
        .pointer("/license/id")
        .and_then(Value::as_str)
        .map(|l| l.replace('-', " ").replace("CC BY SA", "CC BY-SA"))
        .unwrap_or_else(|| LICENCE.to_owned());
    let length_km = Route::lines_length_km(&lines);
    Ok(Some(Route {
        id: format!("camptocamp:{id}"),
        name: Some(title(doc)).filter(|t| !t.is_empty()),
        provider: Provider::Camptocamp,
        publisher: "Camptocamp".into(),
        url: format!("https://www.camptocamp.org/routes/{id}"),
        licence,
        attribution: format!(
            "{ATTRIBUTION}, auteurs : https://www.camptocamp.org/routes/history/{id}/fr"
        ),
        activity,
        shape,
        lines,
        length_km,
        published_length_km: doc
            .get("route_length")
            .and_then(Value::as_f64)
            .map(|m| m / 1000.0),
        ascent_m: doc.get("height_diff_up").and_then(Value::as_f64),
        difficulty: doc
            .get("ski_rating")
            .and_then(Value::as_str)
            .map(|r| format!("ski {r}")),
    }))
}

/// What [`fetch`] found.
#[derive(Debug, Clone, Default)]
pub struct Fetch {
    /// Kept itineraries (with a detailed geometry).
    pub routes: Vec<Route>,
    /// Listing pages (for the metadata).
    pub listing: Vec<Fetched>,
    /// Documents listed, by activity class (kept or not).
    pub listed: BTreeMap<String, usize>,
    /// Kept documents without a detailed geometry (no itinerary), as points.
    pub without_detail: Vec<Hint>,
}

/// Point of a listing entry (`geometry.geom`, EPSG:3857).
fn listing_point(doc: &Value) -> Option<LonLat> {
    let g: Value = serde_json::from_str(doc.pointer("/geometry/geom")?.as_str()?).ok()?;
    let c = g.get("coordinates")?;
    from_web_mercator(c.get(0)?.as_f64()?, c.get(1)?.as_f64()?).ok()
}

/// Lists the routes of [`listings`] and fetches each kept document with a detail.
pub fn fetch(dl: &Downloader) -> Result<Fetch> {
    let mut out = Fetch::default();
    let mut docs: BTreeMap<u64, Value> = BTreeMap::new();
    for q in listings() {
        let mut offset = 0;
        loop {
            let url = format!("{API}/routes?{q}&limit={PAGE}&offset={offset}&pl=fr");
            let f = dl.fetch(&Request::get(SOURCE, &url).max_age(DOC_MAX_AGE))?;
            let v: Value = f.json()?;
            let page = v
                .get("documents")
                .and_then(Value::as_array)
                .ok_or_else(|| SrnError::format(SOURCE, "listing without documents"))?;
            let total = v.get("total").and_then(Value::as_u64).unwrap_or(0) as usize;
            for d in page {
                if let Some(id) = d.get("document_id").and_then(Value::as_u64) {
                    docs.entry(id).or_insert_with(|| d.clone());
                }
            }
            let n = page.len();
            out.listing.push(f);
            offset += PAGE;
            if n < PAGE || offset >= total {
                break;
            }
        }
    }
    for (id, d) in &docs {
        let class = activity(d);
        let key = class.map_or("not_kept", Activity::as_str);
        *out.listed.entry(key.to_owned()).or_default() += 1;
        let Some(class) = class else {
            continue;
        };
        if d.pointer("/geometry/has_geom_detail")
            .and_then(Value::as_bool)
            != Some(true)
        {
            if let Some(pos) = listing_point(d) {
                out.without_detail.push(Hint {
                    id: format!("camptocamp:{id}"),
                    name: title(d),
                    provider: Provider::Camptocamp,
                    url: format!("https://www.camptocamp.org/routes/{id}"),
                    activity: class,
                    pos,
                    reason: "pas de tracé détaillé (geom_detail)".into(),
                });
            }
            continue;
        }
        let f = dl.fetch(
            &Request::get(SOURCE, &format!("{API}/routes/{id}?cook=fr")).max_age(DOC_MAX_AGE),
        )?;
        if let Some(r) = parse_route(&f.json()?)? {
            out.routes.push(r);
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn doc(title: &str, rating: Option<&str>, up: Option<f64>, acts: &[&str]) -> Value {
        json!({
            "document_id": 7,
            "locales": [{"lang": "fr", "title": title, "title_prefix": "Lieu"}],
            "activities": acts,
            "ski_rating": rating,
            "height_diff_up": up,
            "route_types": ["traverse"],
            "route_length": 12_500,
            "license": {"id": "CC-BY-SA-3.0"},
            "geometry": {
                "has_geom_detail": true,
                "geom_detail": "{\"type\": \"LineString\", \"coordinates\": [[667916.9, 5621521.5], [667916.9, 5635000.0]]}"
            }
        })
    }

    #[test]
    fn classes() {
        let a = |d: &Value| activity(d);
        assert_eq!(
            a(&doc("Traversée nordique", None, None, &["skitouring"])),
            Some(Activity::Srn)
        );
        assert_eq!(
            a(&doc("Centre nordique", Some("4.2"), None, &["skitouring"])),
            None
        );
        // A snowshoe loop of a nordic site.
        assert_eq!(
            a(&doc(
                "Site Nordique d'Agy : Petite boucle",
                None,
                None,
                &["hiking", "snowshoeing"]
            )),
            None
        );
        assert!(!says_nordic(
            "Site Nordique d'Agy : Petite boucle du Plateau d'Agy"
        ));
        assert!(says_nordic(
            "Sommet de Finiels : Parcours randonnée nordique"
        ));
        assert_eq!(
            a(&doc("Col", Some("1.2"), Some(300.0), &["skitouring"])),
            Some(Activity::EasySkiTouring)
        );
        assert_eq!(
            a(&doc("Col", Some("1.3"), Some(900.0), &["skitouring"])),
            Some(Activity::SkiTouring)
        );
        assert_eq!(
            a(&doc("Col", Some("2.1"), Some(300.0), &["skitouring"])),
            None
        );
        assert_eq!(a(&doc("Col", Some("1.1"), Some(300.0), &["hiking"])), None);
    }

    #[test]
    fn route_from_document() {
        let r = parse_route(&doc(
            "Traversée nordique",
            Some("1.1"),
            Some(100.0),
            &["skitouring"],
        ))
        .unwrap()
        .unwrap();
        assert_eq!(r.id, "camptocamp:7");
        assert_eq!(r.name.as_deref(), Some("Lieu : Traversée nordique"));
        assert_eq!(r.shape, Shape::Traverse);
        assert_eq!(r.licence, "CC BY-SA 3.0");
        assert_eq!(r.published_length_km, Some(12.5));
        assert!((r.lines[0][0].lon - 6.0).abs() < 1e-6 && (r.lines[0][0].lat - 45.0).abs() < 1e-3);
        // 13.5 km of Web Mercator northing at 45° N is about 9.5 km on the ground.
        assert!((r.length_km - 9.5).abs() < 0.2, "{}", r.length_km);
    }
}
