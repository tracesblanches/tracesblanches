//! Piste and itinerary lines used by the CLPA crossing computation.
//!
//! [`PisteLine`] is the only input of the crossing computation. The pipeline reads the
//! `pistes.geojson` layer of the OSM crate (P2, #15) with [`from_pistes_geojson`]; an
//! Overpass extract (`out body geom`) saved by the research (report 07) is read with
//! [`from_overpass_json`] to reproduce the figures of the report.

use serde::Deserialize;
use skimap_common::geom::{BBox, LonLat};

/// Which network a line belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Network {
    /// Groomed nordic ski piste: way with `piste:type` containing `nordic`, or member of a
    /// nordic route relation (same rule as report 07).
    Nordic,
    /// Snowshoe / winter walking itinerary (`piste:type=hike` or `snowshoe`) that is not
    /// part of the nordic network.
    Hike,
}

/// A line of the piste network.
#[derive(Debug, Clone, PartialEq)]
pub struct PisteLine {
    /// OSM way identifier, if the line comes from OSM.
    pub osm_way_id: Option<i64>,
    /// Display name: the way `name`, else the name of the first route relation containing
    /// it (rule of report 07).
    pub name: Option<String>,
    /// Names of all route relations containing the way.
    pub relations: Vec<String>,
    /// Network.
    pub network: Network,
    /// Raw `piste:type` of the way (empty if only the relation is tagged).
    pub piste_type: String,
    /// Vertices, WGS84.
    pub coords: Vec<LonLat>,
}

impl PisteLine {
    /// Mean of the vertices (the centroid used by report 07 to keep the Cervières side of
    /// the col d'Izoard).
    pub fn vertex_mean(&self) -> LonLat {
        let n = self.coords.len().max(1) as f64;
        let (sx, sy) = self
            .coords
            .iter()
            .fold((0.0, 0.0), |(x, y), p| (x + p.lon, y + p.lat));
        LonLat::new(sx / n, sy / n)
    }
}

/// Overpass QL query of the piste and itinerary lines of a box (ways and route relations,
/// with geometry), the query of report 02 extended to the whole site box.
pub fn overpass_query(bbox: &BBox) -> String {
    let b = bbox.overpass();
    format!(
        "[out:json][timeout:120];(way[\"piste:type\"~\"nordic|hike|snowshoe|skitour|sled\"]({b});\
         relation[\"piste:type\"~\"nordic|hike|snowshoe\"]({b}););out body geom;"
    )
}

#[derive(Deserialize)]
struct Doc {
    elements: Vec<Element>,
}

#[derive(Deserialize)]
struct Pt {
    lat: f64,
    lon: f64,
}

#[derive(Deserialize)]
struct Member {
    #[serde(rename = "type")]
    kind: String,
    #[serde(rename = "ref")]
    id: i64,
    #[serde(default)]
    geometry: Option<Vec<Option<Pt>>>,
}

#[derive(Deserialize)]
struct Element {
    #[serde(rename = "type")]
    kind: String,
    id: i64,
    #[serde(default)]
    tags: std::collections::BTreeMap<String, String>,
    #[serde(default)]
    geometry: Option<Vec<Option<Pt>>>,
    #[serde(default)]
    members: Vec<Member>,
}

fn coords(g: &[Option<Pt>]) -> Vec<LonLat> {
    g.iter()
        .flatten()
        .map(|p| LonLat::new(p.lon, p.lat))
        .collect()
}

/// Reads an Overpass JSON answer (`out body geom`) into piste lines, without duplicates.
///
/// Nordic lines follow the rule of report 07: ways whose `piste:type` contains `nordic`,
/// then members of nordic relations not already taken (named after the relation). Hike and
/// snowshoe ways and members of hike relations that are not nordic are returned as
/// [`Network::Hike`].
pub fn from_overpass_json(json: &[u8]) -> Result<Vec<PisteLine>, serde_json::Error> {
    let doc: Doc = serde_json::from_slice(json)?;
    let mut lines: Vec<PisteLine> = Vec::new();
    let mut index: std::collections::HashMap<i64, usize> = std::collections::HashMap::new();
    let piste_type = |t: &std::collections::BTreeMap<String, String>| {
        t.get("piste:type").cloned().unwrap_or_default()
    };
    let is_nordic = |t: &str| t.contains("nordic");
    let is_hike = |t: &str| t.contains("hike") || t.contains("snowshoe");

    // Relation names per member way (all relations, for the summary).
    let mut rel_names: std::collections::HashMap<i64, Vec<String>> =
        std::collections::HashMap::new();
    for r in doc.elements.iter().filter(|e| e.kind == "relation") {
        let Some(name) = r.tags.get("name").or_else(|| r.tags.get("piste:name")) else {
            continue;
        };
        for m in r.members.iter().filter(|m| m.kind == "way") {
            let v = rel_names.entry(m.id).or_default();
            if !v.contains(name) {
                v.push(name.clone());
            }
        }
    }

    // Pass 1: nordic ways; pass 2: members of nordic relations; pass 3: hike ways and
    // members of hike relations.
    for pass in 0..3 {
        for e in &doc.elements {
            match (pass, e.kind.as_str()) {
                (0, "way") | (2, "way") => {
                    let t = piste_type(&e.tags);
                    let wanted = if pass == 0 {
                        is_nordic(&t)
                    } else {
                        is_hike(&t)
                    };
                    if !wanted || index.contains_key(&e.id) {
                        continue;
                    }
                    let Some(g) = &e.geometry else { continue };
                    let c = coords(g);
                    if c.len() < 2 {
                        continue;
                    }
                    let relations = rel_names.get(&e.id).cloned().unwrap_or_default();
                    index.insert(e.id, lines.len());
                    lines.push(PisteLine {
                        osm_way_id: Some(e.id),
                        name: e.tags.get("name").cloned(),
                        relations,
                        network: if pass == 0 {
                            Network::Nordic
                        } else {
                            Network::Hike
                        },
                        piste_type: t,
                        coords: c,
                    });
                }
                (1, "relation") | (2, "relation") => {
                    let t = piste_type(&e.tags);
                    let wanted = if pass == 1 {
                        is_nordic(&t)
                    } else {
                        is_hike(&t)
                    };
                    if !wanted {
                        continue;
                    }
                    for m in e.members.iter().filter(|m| m.kind == "way") {
                        if index.contains_key(&m.id) {
                            continue;
                        }
                        let Some(g) = &m.geometry else { continue };
                        let c = coords(g);
                        if c.len() < 2 {
                            continue;
                        }
                        index.insert(m.id, lines.len());
                        lines.push(PisteLine {
                            osm_way_id: Some(m.id),
                            name: e.tags.get("name").cloned(),
                            relations: rel_names.get(&m.id).cloned().unwrap_or_default(),
                            network: if pass == 1 {
                                Network::Nordic
                            } else {
                                Network::Hike
                            },
                            piste_type: String::new(),
                            coords: c,
                        });
                    }
                }
                _ => {}
            }
        }
    }
    Ok(lines)
}

#[derive(Deserialize)]
struct LayerDoc {
    features: Vec<LayerFeature>,
}

#[derive(Deserialize)]
struct LayerFeature {
    geometry: LayerLine,
    properties: LayerProps,
}

#[derive(Deserialize)]
struct LayerLine {
    coordinates: Vec<[f64; 2]>,
}

#[derive(Deserialize)]
struct LayerRelation {
    name: Option<String>,
}

#[derive(Deserialize)]
struct LayerProps {
    osm_way_id: i64,
    way_name: Option<String>,
    uses: Vec<String>,
    tagged: bool,
    relations: Vec<LayerRelation>,
}

/// Reads the `pistes.geojson` layer of the OSM crate (`skimap_osm::extract`, one
/// LineString per OSM way) into piste lines.
///
/// Same rule as [`from_overpass_json`] (report 07): a way is [`Network::Nordic`] when it
/// or one of its piste relations is nordic (`uses` contains `nordic`), otherwise
/// [`Network::Hike`] (hike or snowshoe only). A way tagged itself keeps its own name; a
/// way that is a piste only through its relations takes the name of its first relation. `piste_type` holds the uses of a way tagged itself, joined by
/// `;`, and is empty for a way that is a piste only through its relations.
pub fn from_pistes_geojson(json: &[u8]) -> Result<Vec<PisteLine>, serde_json::Error> {
    let doc: LayerDoc = serde_json::from_slice(json)?;
    Ok(doc
        .features
        .into_iter()
        .filter(|f| f.geometry.coordinates.len() >= 2)
        .map(|f| {
            let p = f.properties;
            let relations: Vec<String> = p.relations.into_iter().filter_map(|r| r.name).collect();
            let nordic = p.uses.iter().any(|u| u == "nordic");
            PisteLine {
                osm_way_id: Some(p.osm_way_id),
                // Report 07: a way tagged itself keeps its own name; a way that is a piste
                // only through its relations (a road section) takes the relation name.
                name: if p.tagged {
                    p.way_name
                } else {
                    relations.first().cloned()
                },
                relations,
                network: if nordic {
                    Network::Nordic
                } else {
                    Network::Hike
                },
                piste_type: if p.tagged {
                    p.uses.join(";")
                } else {
                    String::new()
                },
                coords: f
                    .geometry
                    .coordinates
                    .into_iter()
                    .map(|[lon, lat]| LonLat::new(lon, lat))
                    .collect(),
            }
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn osm_layer_rules() {
        let j = br#"{"type":"FeatureCollection","features":[
{"geometry":{"type":"LineString","coordinates":[[6.7,44.85],[6.7,44.86]]},"properties":{"osm_way_id":1,"name":"A","way_name":"A","uses":["nordic","hike"],"tagged":true,"relations":[{"id":10,"name":"R"}],"length_m":1.0}},
{"geometry":{"type":"LineString","coordinates":[[6.7,44.86],[6.7,44.87]]},"properties":{"osm_way_id":3,"name":"Route","way_name":"Route","uses":["nordic","hike"],"tagged":false,"relations":[{"id":10,"name":"R"},{"id":11,"name":"H"}]}},
{"geometry":{"type":"LineString","coordinates":[[6.7,44.8],[6.7,44.81]]},"properties":{"osm_way_id":5,"name":null,"way_name":null,"uses":["snowshoe"],"tagged":true,"relations":[]}},
{"geometry":{"type":"LineString","coordinates":[[6.7,44.8]]},"properties":{"osm_way_id":6,"way_name":null,"uses":["hike"],"tagged":true,"relations":[]}}
]}"#;
        let l = from_pistes_geojson(j).unwrap();
        let ids: Vec<_> = l
            .iter()
            .map(|p| (p.osm_way_id.unwrap(), p.network))
            .collect();
        assert_eq!(
            ids,
            [
                (1, Network::Nordic),
                (3, Network::Nordic),
                (5, Network::Hike)
            ],
            "single-vertex line dropped"
        );
        assert_eq!(l[0].name.as_deref(), Some("A"));
        assert_eq!(l[0].piste_type, "nordic;hike");
        assert_eq!(
            l[1].name.as_deref(),
            Some("R"),
            "named after the first relation"
        );
        assert_eq!(l[1].relations, ["R", "H"]);
        assert_eq!(l[1].piste_type, "");
        assert_eq!(l[2].name, None);
        assert!((l[0].coords[1].lat - 44.86).abs() < 1e-12);
    }

    #[test]
    fn overpass_rules() {
        let j = br#"{"elements":[
          {"type":"way","id":1,"tags":{"piste:type":"nordic","name":"A"},
           "geometry":[{"lat":44.85,"lon":6.7},{"lat":44.86,"lon":6.7}]},
          {"type":"way","id":2,"tags":{"piste:type":"hike"},
           "geometry":[{"lat":44.85,"lon":6.7},{"lat":44.85,"lon":6.71}]},
          {"type":"relation","id":10,"tags":{"piste:type":"nordic","name":"R"},
           "members":[{"type":"way","ref":1,"role":"","geometry":[{"lat":44.85,"lon":6.7},{"lat":44.86,"lon":6.7}]},
                      {"type":"way","ref":3,"role":"","geometry":[{"lat":44.86,"lon":6.7},null,{"lat":44.87,"lon":6.7}]},
                      {"type":"node","ref":4,"role":""}]},
          {"type":"relation","id":11,"tags":{"piste:type":"hike","name":"H"},
           "members":[{"type":"way","ref":3,"role":""},{"type":"way","ref":5,"role":"","geometry":[{"lat":44.8,"lon":6.7},{"lat":44.81,"lon":6.7}]}]}
        ]}"#;
        let l = from_overpass_json(j).unwrap();
        let ids: Vec<_> = l
            .iter()
            .map(|p| (p.osm_way_id.unwrap(), p.network))
            .collect();
        assert_eq!(
            ids,
            [
                (1, Network::Nordic),
                (3, Network::Nordic),
                (2, Network::Hike),
                (5, Network::Hike)
            ]
        );
        assert_eq!(l[0].name.as_deref(), Some("A"));
        assert_eq!(l[0].relations, ["R"]);
        assert_eq!(l[1].name.as_deref(), Some("R"), "named after the relation");
        assert_eq!(l[1].relations, ["R", "H"]);
        assert_eq!(l[1].coords.len(), 2, "null vertices skipped");
        let c = l[0].vertex_mean();
        assert!((c.lat - 44.855).abs() < 1e-12);
        assert!(
            overpass_query(&BBox::new(44.77, 6.66, 44.92, 6.86))
                .contains("(44.77,6.66,44.92,6.86)")
        );
    }
}
