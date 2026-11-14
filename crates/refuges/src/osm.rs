//! OpenStreetMap huts and shelters (ODbL) through the public Overpass API (one query per
//! site, rate-limited by the shared downloader, cached a day).
//!
//! Kept: `tourism=alpine_hut`, `tourism=wilderness_hut`, `amenity=shelter` (every
//! `shelter_type`, bus shelters included and flagged) and anything carrying
//! `ref:refuges.info`. Ways and relations are reduced to their centre.

use std::collections::BTreeMap;

use serde::Serialize;
use serde_json::Value;
use skimap_common::download::{Downloader, Fetched, Request};
use skimap_common::geom::{BBox, LonLat};

use crate::error::{RefugesError, Result};

/// Downloader source name.
pub const SOURCE: &str = "overpass";
/// Public Overpass endpoint.
pub const ENDPOINT: &str = "https://overpass-api.de/api/interpreter";
/// Licence.
pub const LICENCE: &str = "ODbL 1.0";
/// Attribution.
pub const ATTRIBUTION: &str = "© les contributeurs d'OpenStreetMap (ODbL)";

/// Tags copied as properties (when present).
pub const KEPT_TAGS: &[&str] = &[
    "name",
    "tourism",
    "amenity",
    "shelter_type",
    "building",
    "capacity",
    "beds",
    "winter_room",
    "fireplace",
    "stove",
    "drinking_water",
    "toilets",
    "opening_hours",
    "access",
    "fee",
    "reservation",
    "operator",
    "phone",
    "website",
    "ele",
    "ref:refuges.info",
    "description",
    "disused",
    "abandoned",
];

/// A hut or shelter from OSM.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Hut {
    /// `node`, `way` or `relation`.
    pub osm_type: String,
    /// OSM id.
    pub osm_id: i64,
    /// Position (node, or centre of a way / relation).
    #[serde(skip)]
    pub pos: LonLat,
    /// Kind: `alpine_hut`, `wilderness_hut`, `shelter`, `bus_shelter` (shelter with
    /// `shelter_type=public_transport`) or `other` (only `ref:refuges.info`).
    pub kind: String,
    /// refuges.info id from `ref:refuges.info`.
    pub refuges_info_id: Option<u64>,
    /// Selected tags.
    pub tags: BTreeMap<String, String>,
}

/// Overpass QL query for the huts and shelters of a box.
pub fn query(b: &BBox) -> String {
    let bb = b.overpass();
    format!(
        "[out:json][timeout:120];(nwr[\"tourism\"~\"^(alpine_hut|wilderness_hut)$\"]({bb});\
         nwr[\"amenity\"=\"shelter\"]({bb});nwr[\"ref:refuges.info\"]({bb}););out center tags;"
    )
}

fn kind_of(tags: &BTreeMap<String, String>) -> String {
    match (
        tags.get("tourism").map(String::as_str),
        tags.get("amenity").map(String::as_str),
    ) {
        (Some("alpine_hut"), _) => "alpine_hut".to_owned(),
        (Some("wilderness_hut"), _) => "wilderness_hut".to_owned(),
        (_, Some("shelter")) => {
            if tags.get("shelter_type").map(String::as_str) == Some("public_transport") {
                "bus_shelter".to_owned()
            } else {
                "shelter".to_owned()
            }
        }
        _ => "other".to_owned(),
    }
}

/// Parses the Overpass JSON answer.
pub fn parse(v: &Value) -> Result<Vec<Hut>> {
    let elements = v
        .get("elements")
        .and_then(Value::as_array)
        .ok_or_else(|| RefugesError::format(SOURCE, "no elements"))?;
    let mut out = Vec::new();
    for e in elements {
        let osm_type = e.get("type").and_then(Value::as_str).unwrap_or_default();
        let Some(id) = e.get("id").and_then(Value::as_i64) else {
            continue;
        };
        let (lat, lon) = match (
            e.get("lat").and_then(Value::as_f64),
            e.get("lon").and_then(Value::as_f64),
        ) {
            (Some(a), Some(b)) => (a, b),
            _ => match (
                e.pointer("/center/lat").and_then(Value::as_f64),
                e.pointer("/center/lon").and_then(Value::as_f64),
            ) {
                (Some(a), Some(b)) => (a, b),
                _ => continue,
            },
        };
        let all: BTreeMap<String, String> = e
            .get("tags")
            .and_then(Value::as_object)
            .map(|m| {
                m.iter()
                    .filter_map(|(k, v)| v.as_str().map(|s| (k.clone(), s.to_owned())))
                    .collect()
            })
            .unwrap_or_default();
        let refuges_info_id = all
            .get("ref:refuges.info")
            .and_then(|r| r.split(';').next())
            .and_then(|r| r.trim().parse().ok());
        let kind = kind_of(&all);
        let tags = all
            .into_iter()
            .filter(|(k, _)| KEPT_TAGS.contains(&k.as_str()))
            .collect();
        out.push(Hut {
            osm_type: osm_type.to_owned(),
            osm_id: id,
            pos: LonLat::new(lon, lat),
            kind,
            refuges_info_id,
            tags,
        });
    }
    out.sort_by(|a, b| (&a.osm_type, a.osm_id).cmp(&(&b.osm_type, b.osm_id)));
    out.dedup_by(|a, b| a.osm_type == b.osm_type && a.osm_id == b.osm_id);
    Ok(out)
}

/// Downloads the huts and shelters of `bbox`.
pub fn fetch(dl: &Downloader, bbox: &BBox) -> Result<(Vec<Hut>, Fetched)> {
    let q = query(bbox);
    let f = dl.fetch(&Request::post_form(SOURCE, ENDPOINT, &[("data", &q)]))?;
    let huts = parse(&f.json()?)?;
    Ok((huts, f))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn query_uses_south_west_north_east() {
        let q = query(&BBox::new(44.77, 6.66, 44.92, 6.86));
        assert!(q.contains("(44.77,6.66,44.92,6.86)"));
        assert!(q.contains("out center tags"));
    }

    #[test]
    fn parse_elements() {
        let v = json!({"elements": [
            {"type": "node", "id": 1, "lat": 44.85, "lon": 6.81,
             "tags": {"tourism": "alpine_hut", "name": "Refuge des Fonts", "ref:refuges.info": "1428", "wikidata": "Q1"}},
            {"type": "way", "id": 2, "center": {"lat": 46.6, "lon": 6.1},
             "tags": {"amenity": "shelter", "shelter_type": "public_transport"}},
            {"type": "node", "id": 3, "lat": 46.6, "lon": 6.1, "tags": {"amenity": "shelter", "fireplace": "yes"}},
            {"type": "node", "id": 3, "lat": 46.6, "lon": 6.1, "tags": {"amenity": "shelter"}},
            {"type": "relation", "id": 4, "tags": {"tourism": "wilderness_hut"}}
        ]});
        let h = parse(&v).unwrap();
        assert_eq!(
            h.len(),
            3,
            "duplicate removed, element without position skipped"
        );
        assert_eq!(h[0].kind, "alpine_hut");
        assert_eq!(h[0].refuges_info_id, Some(1428));
        assert!(!h[0].tags.contains_key("wikidata"));
        assert_eq!(h[1].osm_id, 3);
        assert_eq!(h[1].kind, "shelter");
        assert_eq!(h[2].kind, "bus_shelter");
        assert!((h[2].pos.lat - 46.6).abs() < 1e-12);
    }
}
