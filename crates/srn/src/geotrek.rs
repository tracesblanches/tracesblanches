//! Geotrek-admin instances of parks and départements.
//!
//! The official list (`utilisateurs.geotrek.fr/clients_map.geojson`) gives each instance
//! and its public website; the API root is read from the website (`"apiUrl":"…"` of a
//! Geotrek-rando v3 page). The roots found on 15/11/2026, and why the other instances
//! have none, are kept in `data/geotrek_apis.json` ([`KnownApi`]): dead websites are not
//! retried at every build (the downloader would wait for each timeout); an instance
//! missing from that table is discovered on the fly. Each API lists its practices
//! (`trek_practice/`); a practice named nordic / SRN gives SRN itineraries
//! ([`practice_activity`]). Snowshoe practices are only counted: their instances publish
//! no licence (see `docs/srn.md`).

use std::collections::BTreeMap;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use skimap_common::download::{Downloader, Fetched, Request};
use skimap_common::geom::{BBox, LonLat};

use crate::error::{Result, SrnError};
use crate::route::{Activity, Provider, Route, Shape};

/// Downloader source name (every instance; 1 request/s, the default policy).
pub const SOURCE: &str = "geotrek";
/// Official list of the instances.
pub const CLIENTS_URL: &str = "https://utilisateurs.geotrek.fr/clients_map.geojson";
/// Cache lifetime of the list, the websites and the API answers.
pub const MAX_AGE: Duration = Duration::from_secs(30 * 86_400);
const PAGE: usize = 100;
/// API roots read from the instance websites on 15/11/2026.
pub const KNOWN_APIS_JSON: &str = include_str!("../data/geotrek_apis.json");

/// API root of an instance of the list, as found by reading its website.
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct KnownApi {
    /// Instance name in the official list.
    pub name: String,
    /// API root, `None` when the website gives none.
    pub api: Option<String>,
    /// Why there is no API root.
    #[serde(default)]
    pub note: Option<String>,
}

/// The table of known API roots.
pub fn known_apis() -> Result<Vec<KnownApi>> {
    serde_json::from_str(KNOWN_APIS_JSON)
        .map_err(|e| SrnError::format("geotrek_apis.json", e.to_string()))
}

/// Licence statement of an instance, as read by hand (`docs/srn.md`).
pub fn licence_of(api: &str) -> &'static str {
    if api.contains("pnr-vercors.fr") {
        "aucune licence affichée par l'instance (mentions légales lues le 15/11/2026) ; le PNR \
         du Vercors publie le réseau d'itinéraires de cette base sous Licence Ouverte 2.0 \
         (data.gouv.fr, « Itinéraires de randonnée sur le Parc du Vercors ») : à confirmer \
         pour les itinéraires nordiques"
    } else {
        "aucune licence affichée par l'instance (non vérifiée)"
    }
}

/// One instance of the official list.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Instance {
    /// Name in the list.
    pub name: String,
    /// Public websites (URLs only).
    pub websites: Vec<String>,
    /// API root read from a website, without trailing slash.
    pub api: Option<String>,
    /// Practices `(id, name)` of the API.
    pub practices: Vec<(u64, String)>,
    /// Why the API could not be read.
    pub error: Option<String>,
    /// Practices kept, with their class.
    pub kept: Vec<(u64, String, Activity)>,
    /// Treks per kept practice id (all classes, including counted-only snowshoe).
    pub treks: BTreeMap<u64, usize>,
    /// Box of the instance footprint in the list.
    #[serde(skip)]
    pub bbox: Option<BBox>,
    /// Whether the instance was read (its footprint comes near a groomed domain).
    pub snowy: bool,
}

/// An instance is read when its footprint comes within this margin of a groomed domain
/// of the census (a proxy for snow), degrees (≈ 20 km).
pub const SNOW_MARGIN_DEG: f64 = 0.2;

/// Class of a practice from its name: nordic / SRN gives [`Activity::Srn`], snowshoe
/// [`Activity::Snowshoe`]; groomed cross-country skiing, Nordic walking, snow trail and
/// alpine ski touring are not kept.
pub fn practice_activity(name: &str) -> Option<Activity> {
    let n = name.to_lowercase();
    if n.contains("marche") || n.contains("trail") || n.contains("fond") {
        None
    } else if n.contains("nordique") || n.contains("srn") {
        Some(Activity::Srn)
    } else if n.contains("raquette") {
        Some(Activity::Snowshoe)
    } else {
        None
    }
}

/// API root in a Geotrek-rando page.
pub fn api_url(html: &str) -> Option<String> {
    let key = "\"apiUrl\":\"";
    let start = html.find(key)? + key.len();
    let end = html[start..].find('"')? + start;
    let url = html[start..end].replace("\\/", "/");
    url.starts_with("http")
        .then(|| url.trim_end_matches('/').to_owned())
}

fn walk_coords(v: &Value, b: &mut Option<BBox>) {
    match v.as_array() {
        Some(a) if a.len() >= 2 && a[0].is_number() && a[1].is_number() => {
            let (lon, lat) = (a[0].as_f64().unwrap_or(0.0), a[1].as_f64().unwrap_or(0.0));
            *b = Some(match *b {
                None => BBox::new(lat, lon, lat, lon),
                Some(x) => BBox::new(
                    x.south.min(lat),
                    x.west.min(lon),
                    x.north.max(lat),
                    x.east.max(lon),
                ),
            });
        }
        Some(a) => a.iter().for_each(|x| walk_coords(x, b)),
        None => {
            if let Some(o) = v.as_object() {
                for k in ["coordinates", "geometries"] {
                    if let Some(x) = o.get(k) {
                        walk_coords(x, b);
                    }
                }
            }
        }
    }
}

/// Bounding box of a GeoJSON geometry (any type, collections included).
pub fn geometry_bbox(g: &Value) -> Option<BBox> {
    let mut b = None;
    walk_coords(g, &mut b);
    b
}

/// Whether two boxes come within `margin_deg` of each other.
pub fn boxes_near(a: &BBox, b: &BBox, margin_deg: f64) -> bool {
    a.south <= b.north + margin_deg
        && b.south <= a.north + margin_deg
        && a.west <= b.east + margin_deg
        && b.west <= a.east + margin_deg
}

/// Instances of the official list (name, websites, footprint box), in list order.
pub fn parse_clients(v: &Value) -> Vec<Instance> {
    v.get("features")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|f| {
            let p = f.get("properties")?;
            let name = p.get("name")?.as_str()?.trim().to_owned();
            let websites = p
                .get("website")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .split_whitespace()
                .filter(|s| s.starts_with("http"))
                .map(str::to_owned)
                .collect();
            Some(Instance {
                name,
                websites,
                api: None,
                practices: Vec::new(),
                error: None,
                kept: Vec::new(),
                treks: BTreeMap::new(),
                bbox: f.get("geometry").and_then(geometry_bbox),
                snowy: false,
            })
        })
        .collect()
}

fn get(dl: &Downloader, url: &str) -> Result<Fetched> {
    Ok(dl.fetch(&Request::get(SOURCE, url).max_age(MAX_AGE))?)
}

/// Itinerary of one trek of the API (`fields=…,geometry`).
pub fn parse_trek(api: &str, publisher: &str, activity: Activity, t: &Value) -> Option<Route> {
    let id = t.get("id")?.as_u64()?;
    let g = t.get("geometry")?;
    let line = |c: &Value| -> Vec<LonLat> {
        c.as_array()
            .into_iter()
            .flatten()
            .filter_map(|p| Some(LonLat::new(p.get(0)?.as_f64()?, p.get(1)?.as_f64()?)))
            .collect()
    };
    let coords = g.get("coordinates")?;
    let lines: Vec<Vec<LonLat>> = match g.get("type")?.as_str()? {
        "LineString" => vec![line(coords)],
        "MultiLineString" => coords.as_array()?.iter().map(line).collect(),
        _ => return None,
    };
    let lines: Vec<Vec<LonLat>> = lines.into_iter().filter(|l| l.len() >= 2).collect();
    if lines.is_empty() {
        return None;
    }
    let host = api
        .trim_start_matches("https://")
        .trim_start_matches("http://")
        .split('/')
        .next()
        .unwrap_or(api);
    // `route`: 1 Boucle, 2 Aller-retour, 3 Traversée on the instances read; the names are
    // per instance, so the shape comes from the geometry, out-and-back excepted.
    let shape = match Route::shape_from_ends(&lines) {
        Shape::Loop if t.get("route").and_then(Value::as_u64) == Some(2) => Shape::OutAndBack,
        s => s,
    };
    let length_km = Route::lines_length_km(&lines);
    Some(Route {
        id: format!("geotrek:{host}:{id}"),
        name: t
            .get("name")
            .and_then(Value::as_str)
            .map(|s| s.trim().to_owned()),
        provider: Provider::Geotrek,
        publisher: publisher.to_owned(),
        url: format!("{api}/trek/{id}/"),
        licence: licence_of(api).to_owned(),
        attribution: format!("© {publisher} (Geotrek)"),
        activity,
        shape,
        lines,
        length_km,
        published_length_km: t
            .get("length_2d")
            .and_then(Value::as_f64)
            .map(|m| m / 1000.0),
        ascent_m: t.get("ascent").and_then(Value::as_f64),
        difficulty: t
            .get("difficulty")
            .and_then(Value::as_u64)
            .map(|d| format!("niveau {d}")),
    })
}

/// What [`fetch`] found.
#[derive(Debug, Clone, Default)]
pub struct Fetch {
    /// Instances of the list with what was read from each.
    pub instances: Vec<Instance>,
    /// SRN itineraries (deduplicated: an aggregator republishing a trek keeps the copy
    /// of the strongest class).
    pub routes: Vec<Route>,
    /// The official list as fetched.
    pub list: Option<Fetched>,
}

/// Reads the official list, then for each instance whose footprint comes within
/// [`SNOW_MARGIN_DEG`] of one of `snow_boxes` (the groomed domains): its API root (known
/// table, else its website), then once per API its practices and its SRN treks; the
/// publisher of a trek is every instance of the list served by that API. Instances that
/// cannot be read are reported, not fatal.
pub fn fetch(dl: &Downloader, snow_boxes: &[BBox]) -> Result<Fetch> {
    let list = get(dl, CLIENTS_URL)?;
    let mut instances = parse_clients(&list.json::<Value>()?);
    let known: BTreeMap<String, KnownApi> = known_apis()?
        .into_iter()
        .map(|k| (k.name.clone(), k))
        .collect();
    // Pass 1: API roots.
    for inst in &mut instances {
        inst.snowy = inst.bbox.is_some_and(|b| {
            snow_boxes
                .iter()
                .any(|s| boxes_near(&b, s, SNOW_MARGIN_DEG))
        });
        if !inst.snowy {
            continue;
        }
        if let Some(k) = known.get(&inst.name) {
            inst.api = k.api.clone();
            inst.error = k.note.clone();
            continue;
        }
        for w in inst.websites.clone() {
            match get(dl, &w).and_then(|f| f.text().map_err(SrnError::from)) {
                Ok(html) => {
                    if let Some(api) = api_url(&html) {
                        inst.api = Some(api);
                        inst.error = None;
                        break;
                    }
                    inst.error = Some(format!("{w} : pas d'apiUrl dans la page"));
                }
                Err(e) => inst.error = Some(e.to_string()),
            }
        }
        if inst.api.is_none() && inst.websites.is_empty() {
            inst.error = Some("aucun site dans la liste".into());
        }
    }
    // Pass 2: once per API.
    let mut by_api: BTreeMap<String, Vec<usize>> = BTreeMap::new();
    for (i, inst) in instances.iter().enumerate() {
        if let Some(api) = &inst.api {
            by_api.entry(api.clone()).or_default().push(i);
        }
    }
    let mut routes: Vec<Route> = Vec::new();
    for (api, members) in &by_api {
        let mut names: Vec<&str> = members
            .iter()
            .map(|&i| instances[i].name.as_str())
            .collect();
        names.sort_unstable();
        let publisher = names.join(" ; ");
        let mut error = None;
        let url = format!("{api}/trek_practice/?language=fr");
        let practices: Vec<(u64, String)> = match get(dl, &url)
            .and_then(|f| f.json::<Value>().map_err(SrnError::from))
        {
            Ok(v) => v
                .get("results")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .filter_map(|p| Some((p.get("id")?.as_u64()?, p.get("name")?.as_str()?.to_owned())))
                .collect(),
            Err(e) => {
                error = Some(e.to_string());
                Vec::new()
            }
        };
        let kept: Vec<(u64, String, Activity)> = practices
            .iter()
            .filter_map(|(id, n)| practice_activity(n).map(|a| (*id, n.clone(), a)))
            .collect();
        let mut treks: BTreeMap<u64, usize> = BTreeMap::new();
        for (pid, _, activity) in &kept {
            let mut url = Some(if *activity == Activity::Srn {
                format!(
                    "{api}/trek/?language=fr&practices={pid}&fields=id,name,length_2d,ascent,route,difficulty,geometry&page_size={PAGE}"
                )
            } else {
                // Counted only.
                format!("{api}/trek/?language=fr&practices={pid}&fields=id&page_size=1")
            });
            while let Some(u) = url.take() {
                let v: Value = match get(dl, &u).and_then(|f| f.json().map_err(SrnError::from)) {
                    Ok(v) => v,
                    Err(e) => {
                        error = Some(format!("{u} : {e}"));
                        break;
                    }
                };
                if *activity != Activity::Srn {
                    let n = v.get("count").and_then(Value::as_u64).unwrap_or(0) as usize;
                    treks.insert(*pid, n);
                    break;
                }
                for t in v
                    .get("results")
                    .and_then(Value::as_array)
                    .into_iter()
                    .flatten()
                {
                    *treks.entry(*pid).or_default() += 1;
                    if let Some(r) = parse_trek(api, &publisher, *activity, t) {
                        routes.push(r);
                    }
                }
                url = v.get("next").and_then(Value::as_str).map(str::to_owned);
            }
        }
        for &i in members {
            let inst = &mut instances[i];
            inst.practices = practices.clone();
            inst.kept = kept.clone();
            inst.treks = treks.clone();
            if error.is_some() {
                inst.error = error.clone();
            }
        }
    }
    Ok(Fetch {
        instances,
        routes: dedup(routes),
        list: Some(list),
    })
}

/// Drops copies of the same trek (same name, lengths within 1 %), keeping the strongest
/// class, then the first seen.
pub fn dedup(routes: Vec<Route>) -> Vec<Route> {
    let mut out: Vec<Route> = Vec::new();
    for r in routes {
        let key = r.name.as_deref().map(str::to_lowercase);
        let same = out.iter().position(|o| {
            o.name.as_deref().map(str::to_lowercase) == key
                && (o.length_km - r.length_km).abs() <= 0.01 * o.length_km.max(r.length_km)
        });
        match same {
            Some(i) if r.activity < out[i].activity => out[i] = r,
            Some(_) => {}
            None => out.push(r),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn practices() {
        assert_eq!(practice_activity("Nordique"), Some(Activity::Srn));
        assert_eq!(practice_activity("Raquettes"), Some(Activity::Snowshoe));
        assert_eq!(
            practice_activity("Randonnée raquettes"),
            Some(Activity::Snowshoe)
        );
        assert_eq!(practice_activity("Marche nordique"), None);
        assert_eq!(practice_activity("Marche Nordique"), None);
        assert_eq!(practice_activity("Ski de fond"), None);
        assert_eq!(practice_activity("Snow trail"), None);
        assert_eq!(practice_activity("Pédestre"), None);
    }

    #[test]
    fn known_api_table() {
        let k = known_apis().unwrap();
        assert_eq!(k.len(), 81);
        assert!(
            k.iter()
                .any(|x| x.name == "Parc Naturel Régional du Vercors"
                    && x.api.as_deref() == Some("https://geo.pnr-vercors.fr/api/v2"))
        );
        assert!(k.iter().all(|x| x.api.is_some() || x.note.is_some()));
    }

    #[test]
    fn footprint_boxes() {
        let g = json!({"type": "GeometryCollection", "geometries": [
            {"type": "Point", "coordinates": [5.0, 45.0]},
            {"type": "MultiPolygon", "coordinates": [[[[5.5, 44.5], [6.0, 44.5], [6.0, 45.5], [5.5, 44.5]]]]}
        ]});
        let b = geometry_bbox(&g).unwrap();
        assert_eq!(b, BBox::new(44.5, 5.0, 45.5, 6.0));
        assert!(boxes_near(&b, &BBox::new(45.6, 6.1, 46.0, 6.5), 0.2));
        assert!(!boxes_near(&b, &BBox::new(46.0, 6.1, 46.5, 6.5), 0.2));
    }

    #[test]
    fn api_in_page() {
        let html = r#"<script>{"props":{"apiUrl":"https://geo.pnr-vercors.fr/api/v2","x":1}}"#;
        assert_eq!(
            api_url(html).as_deref(),
            Some("https://geo.pnr-vercors.fr/api/v2")
        );
        let html = r#"{"apiUrl":"https:\/\/adminrando.vanoise.com\/api\/v2\/"}"#;
        assert_eq!(
            api_url(html).as_deref(),
            Some("https://adminrando.vanoise.com/api/v2")
        );
        assert_eq!(api_url("<html></html>"), None);
    }

    #[test]
    fn clients_and_trek() {
        let v = json!({"features": [
            {"properties": {"name": "Parc", "website": "https://a.fr https://b.fr (site)"}},
            {"properties": {"name": "Sans site"}}
        ]});
        let c = parse_clients(&v);
        assert_eq!(c.len(), 2);
        assert_eq!(c[0].websites, vec!["https://a.fr", "https://b.fr"]);
        assert!(c[1].websites.is_empty());
        let t = json!({"id": 29411, "name": "Traversée", "length_2d": 36406.1, "route": 3,
            "geometry": {"type": "LineString", "coordinates": [[5.5, 45.0, 1200.0], [5.5, 44.7, 1300.0]]}});
        let r = parse_trek(
            "https://geo.pnr-vercors.fr/api/v2",
            "PNR",
            Activity::Srn,
            &t,
        )
        .unwrap();
        assert_eq!(r.id, "geotrek:geo.pnr-vercors.fr:29411");
        assert_eq!(r.shape, Shape::Traverse);
        assert!((r.published_length_km.unwrap() - 36.4061).abs() < 1e-9);
        assert!(r.licence.contains("Licence Ouverte"));
        let mut copy = r.clone();
        copy.id = "geotrek:aura:1".into();
        copy.activity = Activity::Snowshoe;
        let d = dedup(vec![copy, r]);
        assert_eq!(d.len(), 1);
        assert_eq!(d[0].activity, Activity::Srn);
    }
}
