//! The SRN itineraries of an SRN site as ways of its routing graph (#168).
//!
//! An SRN play area or traverse is skied on its itineraries (`srn/srn_routes.geojson` of
//! the site, written by the `srn` step from the France layer, G5), mostly outside the OSM
//! pistes: without them, the routing graph of the site is only the few OSM nordic,
//! snowshoe or hiking ways of its box, often far from its itineraries and from the car
//! parks its starts come from (X9b snaps them to the itineraries). Every part of every
//! itinerary of the layer becomes a way of the graph, joined to the OSM ways and to the
//! other itineraries like any piste (shared coordinates, crossings, short gaps:
//! [`crate::graph::Graph::build`]); it is not an OSM way (`osm_way_id` null), its
//! geometry is the itinerary's own, nothing is drawn.
//!
//! Only SRN sites take them: the graph of a nordic domain stays its OSM pistes.

use std::collections::{BTreeSet, HashMap};
use std::path::Path;

use serde_json::Value;
use skimap_common::geom::LonLat;
use skimap_common::layout::DataLayout;
use skimap_common::meta::{LayerMeta, SourceRef};
use skimap_common::output::round7;
use skimap_common::site::{Site, SiteKind};

use crate::error::Result;
use crate::pistes::{Grooming, Oneway, Piste, Use, polyline_length_m};

/// Layer of the SRN areas and itineraries of a site (`skimap_srn::build::LAYER`).
pub const SRN_LAYER: &str = "srn";
/// Itineraries of the site (`skimap_srn::build::ROUTES_FILE`).
pub const ROUTES_FILE: &str = "srn_routes.geojson";
/// Node ids of the itinerary vertices start here, downwards: far below the ids of the
/// nodes [`crate::graph::Graph::build`] adds (-1, -2, …), never an OSM id (positive).
pub const FIRST_NODE_ID: i64 = -(1 << 50);

/// An itinerary part made a way of the graph.
#[derive(Debug, Clone, PartialEq)]
pub struct SrnWay {
    /// The way (no OSM id, nordic, not groomed, two-way).
    pub piste: Piste,
    /// Identifier of the itinerary (`id` of `srn_routes.geojson`).
    pub route_id: String,
}

/// The SRN itineraries of an SRN site, and the sources of their layer.
#[derive(Debug, Clone, Default)]
pub struct SiteSrn {
    /// One way per part of each itinerary.
    pub ways: Vec<SrnWay>,
    /// Sources of the `srn` layer of the site.
    pub sources: Vec<SourceRef>,
}

/// Whether the routing graph of `site` takes its SRN itineraries.
pub fn takes_itineraries(site: &Site) -> bool {
    matches!(site.kind, SiteKind::SrnArea | SiteKind::Traverse)
}

fn walk(v: &Value, out: &mut Vec<LonLat>) {
    if let Some(a) = v.as_array() {
        for p in a {
            if let (Some(lon), Some(lat)) = (
                p.get(0).and_then(Value::as_f64),
                p.get(1).and_then(Value::as_f64),
            ) {
                // To OSM precision, as the layers are written: a vertex of an itinerary on
                // an OSM node is that node, never a second node a few millimetres away
                // that the written graph would put at the same place.
                out.push(LonLat::new(round7(lon), round7(lat)));
            }
        }
    }
}

/// Ways of the itineraries of a `srn_routes.geojson` value: one per part of each
/// LineString or MultiLineString; vertices at the same coordinates share one node id, so
/// that two itineraries meeting at a vertex are joined.
pub fn ways_of(v: &Value) -> Vec<SrnWay> {
    let mut ids: HashMap<(u64, u64), i64> = HashMap::new();
    let mut next = FIRST_NODE_ID;
    let mut out = Vec::new();
    for f in v["features"].as_array().into_iter().flatten() {
        let p = &f["properties"];
        let name = p["name"]
            .as_str()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_owned);
        let route_id = p["id"].as_str().unwrap_or_default().to_owned();
        let g = &f["geometry"];
        let parts: Vec<&Value> = match g["type"].as_str() {
            Some("LineString") => vec![&g["coordinates"]],
            Some("MultiLineString") => g["coordinates"].as_array().into_iter().flatten().collect(),
            _ => Vec::new(),
        };
        for part in parts {
            let mut pts = Vec::new();
            walk(part, &mut pts);
            let mut points: Vec<(i64, LonLat)> = pts
                .into_iter()
                .map(|q| {
                    let id = *ids
                        .entry((q.lon.to_bits(), q.lat.to_bits()))
                        .or_insert_with(|| {
                            next -= 1;
                            next
                        });
                    (id, q)
                })
                .collect();
            points.dedup_by_key(|x| x.0);
            if points.len() < 2 {
                continue;
            }
            let length_m = polyline_length_m(&points);
            out.push(SrnWay {
                piste: Piste {
                    way_id: 0,
                    uses: BTreeSet::from([Use::Nordic]),
                    tagged: false,
                    way_name: name.clone(),
                    relations: Vec::new(),
                    difficulty: None,
                    difficulty_max: None,
                    difficulty_source: None,
                    way_difficulty: None,
                    grooming: Grooming {
                        raw: None,
                        classic: false,
                        skating: false,
                        backcountry: true,
                        scooter: false,
                    },
                    oneway: Oneway::No,
                    lit: None,
                    points,
                    length_m,
                    layered: false,
                },
                route_id: route_id.clone(),
            });
        }
    }
    out
}

/// The SRN itineraries of `site` when it is an SRN site whose `srn` layer exists (else
/// none).
pub fn site_srn(site: &Site, layout: &DataLayout) -> Result<SiteSrn> {
    if !takes_itineraries(site) {
        return Ok(SiteSrn::default());
    }
    let dir = layout.layer_dir(site, SRN_LAYER)?;
    let path = dir.join(ROUTES_FILE);
    if !path.is_file() {
        return Ok(SiteSrn::default());
    }
    let v = read(&path)?;
    let sources = LayerMeta::read(&dir.join(LayerMeta::file_name(SRN_LAYER)))
        .map(|m| m.sources)
        .unwrap_or_default();
    Ok(SiteSrn {
        ways: ways_of(&v),
        sources,
    })
}

fn read(path: &Path) -> Result<Value> {
    let body = std::fs::read(path).map_err(|e| crate::error::OsmError::io(path, e))?;
    serde_json::from_slice(&body)
        .map_err(|e| crate::error::OsmError::format(ROUTES_FILE, e.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graph::Graph;
    use crate::pistes::PisteSet;

    #[test]
    fn itineraries_are_ways_joined_at_shared_vertices_and_crossings() {
        let v = serde_json::json!({"features": [
            {"properties": {"id": "a", "name": "Tour du plateau"},
             "geometry": {"type": "LineString", "coordinates": [[6.0, 45.0], [6.01, 45.0], [6.020_000_001, 45.0]]}},
            {"properties": {"id": "b", "name": " "},
             "geometry": {"type": "MultiLineString", "coordinates": [
                 [[6.02, 45.0], [6.02, 45.01]],
                 [[6.005, 44.995], [6.005, 45.005]],
                 [[6.1, 45.1]]
             ]}}
        ]});
        let ways = ways_of(&v);
        // The one-point part is no way.
        assert_eq!(ways.len(), 3);
        assert_eq!(ways[0].piste.way_name.as_deref(), Some("Tour du plateau"));
        assert_eq!(ways[1].piste.way_name, None);
        assert_eq!(ways[2].route_id, "b");
        assert!(ways.iter().all(|w| w.piste.is_nordic()
            && w.piste.grooming.backcountry
            && w.piste.points.iter().all(|(id, _)| *id < FIRST_NODE_ID)));
        // [6.02, 45.0] is one node of « a » and « b » (to OSM precision).
        assert_eq!(ways[0].piste.points[2].0, ways[1].piste.points[0].0);
        let set = PisteSet {
            pistes: ways.into_iter().map(|w| w.piste).collect(),
            ..Default::default()
        };
        // The third part crosses « a » without a shared vertex: joined at the crossing.
        let g = Graph::build(&set, crate::extract::CONNECTOR_MAX_M);
        assert_eq!(g.components().1, 1);
    }
}
