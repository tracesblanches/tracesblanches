//! The site a pack carries (P163, #163): the registry record of the site, so that the
//! phone plans on it offline, with starts on the trail network when the pipeline found
//! none (X9 covers the census domains: SRN areas and traverses have none).
//!
//! Such a site gets its starts from its own trail graph (`pistes/graph_nodes.geojson`), on
//! its main connected part (the most nodes): the start of each of its SRN itineraries
//! (`srn/srn_routes.geojson`, longest first) brought to the nearest node, else the node
//! nearest to the site centre. A point farther than [`MAX_SNAP_M`] from the network is not
//! a start (the morning looks for a trail within that distance). A published pack takes
//! only the itineraries that enter its graph (ODbL or attribution-only licence,
//! [`skimap_common::licence`], as the graph, #169): the start and the name of an
//! itinerary under CC BY-SA or without a reuse licence go into personal packs only.

use std::path::Path;

use serde_json::Value;
use skimap_common::geom::LonLat;
use skimap_common::layout::DataLayout;
use skimap_common::site::{Site, SiteRecord, Start};

/// Farthest a start is brought to the network, metres (the morning's snapping radius).
pub const MAX_SNAP_M: f64 = 300.0;
/// Most starts made for one site.
pub const MAX_STARTS: usize = 3;

/// The record a pack carries for `site` ([`SiteRecord::of_site`]), with network starts
/// when the pipeline found none and the site table has none; `personal_sources`: the
/// pack is personal (every itinerary may give a start).
pub fn record(site: &Site, layout: &DataLayout, personal_sources: bool) -> SiteRecord {
    let mut r = SiteRecord::of_site(site);
    let local = skimap_common::site::local(site.id).is_some_and(|l| !l.starts.is_empty());
    if r.starts.is_empty() && !local {
        r.starts = network_starts(site, layout, personal_sources);
    }
    r
}

fn read(path: &Path) -> Option<Value> {
    serde_json::from_slice(&std::fs::read(path).ok()?).ok()
}

fn point(c: &Value) -> Option<LonLat> {
    Some(LonLat::new(c.get(0)?.as_f64()?, c.get(1)?.as_f64()?))
}

/// Nodes of the main connected part of the trail graph of `site`.
fn main_nodes(site: &Site, layout: &DataLayout) -> Vec<LonLat> {
    let Some(v) = layout
        .layer_dir(site, "pistes")
        .ok()
        .and_then(|d| read(&d.join("graph_nodes.geojson")))
    else {
        return Vec::new();
    };
    let nodes: Vec<(i64, LonLat)> = v["features"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|f| {
            Some((
                f["properties"]["component"].as_i64().unwrap_or(0),
                point(&f["geometry"]["coordinates"])?,
            ))
        })
        .collect();
    let mut count = std::collections::BTreeMap::<i64, usize>::new();
    for (c, _) in &nodes {
        *count.entry(*c).or_default() += 1;
    }
    let Some(main) = count
        .iter()
        .max_by_key(|(c, n)| (**n, std::cmp::Reverse(**c)))
        .map(|(c, _)| *c)
    else {
        return Vec::new();
    };
    nodes
        .into_iter()
        .filter(|(c, _)| *c == main)
        .map(|(_, p)| p)
        .collect()
}

fn nearest(nodes: &[LonLat], p: LonLat) -> Option<(LonLat, f64)> {
    nodes
        .iter()
        .map(|n| (*n, n.haversine_m(p)))
        .min_by(|a, b| a.1.total_cmp(&b.1))
}

/// Starts of the SRN itineraries of `site` (first point of each line, longest first);
/// without `personal_sources`, of those whose licence lets them into a published pack.
fn route_starts(site: &Site, layout: &DataLayout, personal_sources: bool) -> Vec<(String, LonLat)> {
    let Some(v) = layout
        .layer_dir(site, "srn")
        .ok()
        .and_then(|d| read(&d.join("srn_routes.geojson")))
    else {
        return Vec::new();
    };
    let mut out: Vec<(f64, String, LonLat)> = v["features"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|f| {
            personal_sources
                || skimap_common::licence::classify(
                    f["properties"]["licence"].as_str().unwrap_or(""),
                )
                .enters_odbl()
        })
        .filter_map(|f| {
            let g = &f["geometry"];
            let first = match g["type"].as_str()? {
                "LineString" => &g["coordinates"][0],
                "MultiLineString" => &g["coordinates"][0][0],
                _ => return None,
            };
            let name = f["properties"]["name"].as_str()?.trim();
            (!name.is_empty()).then_some(())?;
            Some((
                f["properties"]["length_km"].as_f64().unwrap_or(0.0),
                name.to_owned(),
                point(first)?,
            ))
        })
        .collect();
    out.sort_by(|a, b| b.0.total_cmp(&a.0).then(a.1.cmp(&b.1)));
    out.into_iter().map(|(_, n, p)| (n, p)).collect()
}

fn add(out: &mut Vec<Start>, s: Start) {
    if out.len() < MAX_STARTS && !out.iter().any(|o| o.lon == s.lon && o.lat == s.lat) {
        out.push(s);
    }
}

/// Starts on the main part of the trail network of `site` (module documentation).
pub fn network_starts(site: &Site, layout: &DataLayout, personal_sources: bool) -> Vec<Start> {
    let nodes = main_nodes(site, layout);
    let mut out: Vec<Start> = Vec::new();
    for (name, p) in route_starts(site, layout, personal_sources) {
        let id = format!("itineraire-{}", out.len() + 1);
        let Some((n, d)) = nearest(&nodes, p) else {
            break;
        };
        if d > MAX_SNAP_M {
            continue;
        }
        add(
            &mut out,
            Start {
                id,
                name: format!("Départ de l'itinéraire {name}"),
                phrase: format!("le départ de l'itinéraire {name}"),
                phrase_en: Some(format!("the start of the {name} route")),
                lon: n.lon,
                lat: n.lat,
                provenance: format!(
                    "début de l'itinéraire SRN ramené au nœud le plus proche de la partie principale du réseau ({d:.0} m)"
                ),
                access_m: None,
            },
        );
    }
    if out.is_empty()
        && let Some((n, d)) = nearest(&nodes, site.commune_center)
    {
        add(
            &mut out,
            Start {
                id: "reseau".into(),
                name: format!("Centre du domaine {}", site.name),
                phrase: format!("le centre du domaine {}", site.name),
                phrase_en: Some(format!("the centre of the {} area", site.name)),
                lon: n.lon,
                lat: n.lat,
                provenance: format!(
                    "nœud de la partie principale du réseau le plus proche du point représentatif du site ({d:.0} m)"
                ),
                access_m: None,
            },
        );
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write(dir: &Path, rel: &str, v: &Value) {
        let p = dir.join(rel);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, serde_json::to_vec(v).unwrap()).unwrap();
    }

    fn node(c: i64, lon: f64, lat: f64) -> Value {
        serde_json::json!({"type": "Feature", "properties": {"component": c},
            "geometry": {"type": "Point", "coordinates": [lon, lat]}})
    }

    #[test]
    fn starts_on_the_main_network_from_the_itineraries_then_the_centre() {
        let tmp = tempfile::tempdir().unwrap();
        let layout = DataLayout::new(tmp.path());
        let mut site = skimap_common::site::SITES[0];
        site.starts = &[];
        site.commune_center = LonLat::new(6.0, 45.0);
        let out = layout.out_dir(&site);
        // A small part next to the itinerary start, the main part 100 m east of it.
        write(
            &out,
            "pistes/graph_nodes.geojson",
            &serde_json::json!({"features": [
                node(1, 6.1, 45.1),
                node(0, 6.1013, 45.1),
                node(0, 6.11, 45.1),
                node(0, 6.12, 45.1),
                node(0, 6.0005, 45.0),
            ]}),
        );
        let none = network_starts(&site, &layout, true);
        assert_eq!(none.len(), 1);
        assert_eq!((none[0].lon, none[0].lat), (6.0005, 45.0));
        assert_eq!(none[0].id, "reseau");
        write(
            &out,
            "srn/srn_routes.geojson",
            &serde_json::json!({"features": [
                {"properties": {"name": "Court", "length_km": 3.0, "licence": "ODbL 1.0"},
                 "geometry": {"type": "LineString", "coordinates": [[6.12, 45.1], [6.13, 45.1]]}},
                {"properties": {"name": "Loin", "length_km": 30.0, "licence": "ODbL 1.0"},
                 "geometry": {"type": "LineString", "coordinates": [[7.0, 46.0], [7.1, 46.0]]}},
                {"properties": {"name": "Long", "length_km": 9.0, "licence": "aucune licence affichée"},
                 "geometry": {"type": "LineString", "coordinates": [[6.1, 45.1], [6.2, 45.1]]}}
            ]}),
        );
        // A published pack: the itinerary without a reuse licence gives no start.
        let public = network_starts(&site, &layout, false);
        let names: Vec<&str> = public.iter().map(|s| s.name.as_str()).collect();
        assert_eq!(names, ["Départ de l'itinéraire Court"]);
        let s = network_starts(&site, &layout, true);
        let names: Vec<&str> = s.iter().map(|s| s.name.as_str()).collect();
        assert_eq!(
            names,
            [
                "Départ de l'itinéraire Long",
                "Départ de l'itinéraire Court"
            ]
        );
        // On the main part (component 0), not on the closer small one.
        assert_eq!((s[0].lon, s[0].lat), (6.1013, 45.1));
        // A site whose table has starts keeps them: no network start is added.
        let eval = &skimap_common::site::SITES[0];
        assert!(skimap_common::site::local(eval.id).is_some_and(|l| !l.starts.is_empty()));
        assert_eq!(record(eval, &layout, false).starts, eval.starts.to_vec());
    }
}
