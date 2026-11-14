//! Huts and shelters: refuges.info, OSM, Camptocamp (P11, #24).
//!
//! Three sources, three layers, never merged into one database: refuges.info (CC BY-SA
//! 2.0) and Camptocamp (CC BY-SA 3.0) on one side, OSM (ODbL) on the other, and the
//! compatibility of these licences (ticket L3) is still open. A table of identifiers
//! (`links.json`) says which records describe the same place: OSM `ref:refuges.info`, or
//! position and name.
//!
//! Output, in `data/out/<site>/refuges/` (WGS84):
//! - `refuges_info.geojson`: points with places, stove, state and last dated comment;
//! - `osm.geojson`: `tourism=alpine_hut`, `tourism=wilderness_hut`, `amenity=shelter`;
//! - `camptocamp.geojson`: huts, shelters and gîtes with the unstaffed capacity;
//! - `links.json`, `refuges.meta.json` (sources, licences, attributions, dates, figures).
//!
//! Each point also says whether it lies in France (`in_france`, commune found by
//! geo.api.gouv.fr): both site boxes overlap Italy or Switzerland.
//!
//! Pipeline side: uses `skimap_common::download`.

pub mod camptocamp;
pub mod error;
pub mod links;
pub mod osm;
pub mod refuges_info;

use std::collections::{BTreeMap, HashMap};
use std::path::PathBuf;

use serde_json::{Value, json};
use skimap_common::download::{Downloader, Fetched, Request};
use skimap_common::geom::LonLat;
use skimap_common::layout::DataLayout;
use skimap_common::site::Site;
use skimap_osm::output::{
    FileMeta, LayerMeta, SourceMeta, feature, now_rfc3339, point, write_geojson, write_json,
};

pub use error::{RefugesError, Result};

/// Name of this crate, checked by the crate smoke test.
pub const CRATE: &str = env!("CARGO_PKG_NAME");
/// Layer name.
pub const LAYER: &str = "refuges";

/// Result of [`build`].
#[derive(Debug, Clone)]
pub struct RefugesOutput {
    /// Layer directory.
    pub dir: PathBuf,
    /// Key figures (also in the meta file).
    pub stats: Value,
}

/// Whether a point lies in France (a commune is found there).
fn in_france(dl: &Downloader, cache: &mut HashMap<(i64, i64), bool>, p: LonLat) -> Result<bool> {
    let key = ((p.lon * 1e5).round() as i64, (p.lat * 1e5).round() as i64);
    if let Some(v) = cache.get(&key) {
        return Ok(*v);
    }
    let url = format!(
        "https://geo.api.gouv.fr/communes?lat={:.5}&lon={:.5}&fields=code&format=json",
        p.lat, p.lon
    );
    let f = dl.fetch(&Request::get("geo-api", &url))?;
    let v: Value = f.json()?;
    let fr = v.as_array().is_some_and(|a| !a.is_empty());
    cache.insert(key, fr);
    Ok(fr)
}

fn source(name: &str, licence: &str, attribution: &str, f: &Fetched) -> SourceMeta {
    SourceMeta::from_fetched(name, licence, attribution, f)
}

fn count_by<'a>(items: impl Iterator<Item = (&'a str, bool)>) -> Value {
    let mut m: BTreeMap<&str, (usize, usize)> = BTreeMap::new();
    for (k, fr) in items {
        let e = m.entry(k).or_default();
        if fr {
            e.0 += 1;
        } else {
            e.1 += 1;
        }
    }
    Value::Object(
        m.into_iter()
            .map(|(k, (fr, other))| (k.to_owned(), json!({"france": fr, "hors_france": other})))
            .collect(),
    )
}

/// Builds the huts and shelters layers of `site`.
pub fn build(site: &Site, layout: &DataLayout, dl: &Downloader) -> Result<RefugesOutput> {
    let dir = layout.layer_dir(site, LAYER)?;
    let ri = refuges_info::fetch(dl, &site.bbox)?;
    let (huts, osm_f) = osm::fetch(dl, &site.bbox)?;
    let c2c = camptocamp::fetch(dl, &site.bbox)?;
    let links = links::link(&ri.points, &huts, &c2c.waypoints);

    let mut fr_cache = HashMap::new();
    let mut ri_fr = Vec::with_capacity(ri.points.len());
    for p in &ri.points {
        ri_fr.push(in_france(dl, &mut fr_cache, p.pos)?);
    }
    let mut osm_fr = Vec::with_capacity(huts.len());
    for h in &huts {
        osm_fr.push(in_france(dl, &mut fr_cache, h.pos)?);
    }

    let mut link_of_ri: HashMap<u64, usize> = HashMap::new();
    let mut link_of_osm: HashMap<String, usize> = HashMap::new();
    let mut link_of_c2c: HashMap<u64, usize> = HashMap::new();
    for (i, l) in links.iter().enumerate() {
        if let Some(r) = l.refuges_info {
            link_of_ri.insert(r, i);
        }
        for o in &l.osm {
            link_of_osm.insert(o.clone(), i);
        }
        for c in &l.camptocamp {
            link_of_c2c.insert(*c, i);
        }
    }

    let ri_features: Vec<Value> = ri
        .points
        .iter()
        .zip(&ri_fr)
        .map(|(p, fr)| {
            let mut v = serde_json::to_value(p).unwrap_or(Value::Null);
            if let Value::Object(m) = &mut v {
                m.insert("in_france".into(), json!(fr));
                m.insert("link".into(), json!(link_of_ri.get(&p.id)));
            }
            feature(point(p.pos), v)
        })
        .collect();
    write_geojson(&dir.join("refuges_info.geojson"), &ri_features)?;

    let osm_features: Vec<Value> = huts
        .iter()
        .zip(&osm_fr)
        .map(|(h, fr)| {
            let mut v = serde_json::to_value(h).unwrap_or(Value::Null);
            if let Value::Object(m) = &mut v {
                m.insert("in_france".into(), json!(fr));
                m.insert(
                    "link".into(),
                    json!(link_of_osm.get(&format!("{}/{}", h.osm_type, h.osm_id))),
                );
            }
            feature(point(h.pos), v)
        })
        .collect();
    write_geojson(&dir.join("osm.geojson"), &osm_features)?;

    let c2c_features: Vec<Value> = c2c
        .waypoints
        .iter()
        .map(|w| {
            let mut v = serde_json::to_value(w).unwrap_or(Value::Null);
            if let Value::Object(m) = &mut v {
                m.insert(
                    "in_france".into(),
                    json!(w.country.as_deref().map(|c| c == "France")),
                );
                m.insert("link".into(), json!(link_of_c2c.get(&w.id)));
            }
            feature(point(w.pos), v)
        })
        .collect();
    write_geojson(&dir.join("camptocamp.geojson"), &c2c_features)?;

    write_json(
        &dir.join("links.json"),
        &json!({
            "note": "identifiants seulement ; les sources restent dans des couches séparées (licences CC BY-SA et ODbL, ticket L3)",
            "links": links,
        }),
    )?;

    // Figures compared with report 06.
    let mut by_method: BTreeMap<String, usize> = BTreeMap::new();
    for l in &links {
        for m in &l.methods {
            let kind = if m.contains("ref:refuges.info") {
                "osm_ref_refuges_info"
            } else if m.starts_with("osm") {
                "osm_position"
            } else if m.contains("via osm") {
                "camptocamp_via_osm"
            } else if m.starts_with("camptocamp") && l.refuges_info.is_some() {
                "camptocamp_refuges_info_position"
            } else {
                "camptocamp_osm_position"
            };
            *by_method.entry(kind.to_owned()).or_default() += 1;
        }
    }
    let tag_count = |k: &str| huts.iter().filter(|h| h.tags.contains_key(k)).count();
    let stats = json!({
        "refuges_info": {
            "points": ri.points.len(),
            "by_type": count_by(ri.points.iter().zip(&ri_fr).map(|(p, fr)| (p.type_label.as_deref().unwrap_or("?"), *fr))),
            "states": count_by(ri.points.iter().zip(&ri_fr).filter_map(|(p, fr)| p.state.as_deref().map(|s| (s, *fr)))),
            "with_stove": ri.points.iter().filter(|p| p.stove == Some(true)).count(),
            "with_last_comment": ri.points.iter().filter(|p| p.last_comment.is_some()).count(),
            "incremental_points": ri.incremental_points,
        },
        "osm": {
            "elements": huts.len(),
            "by_kind": count_by(huts.iter().zip(&osm_fr).map(|(h, fr)| (h.kind.as_str(), *fr))),
            "with_ref_refuges_info": huts.iter().filter(|h| h.refuges_info_id.is_some()).count(),
            "with_winter_room": tag_count("winter_room"),
            "with_fireplace": tag_count("fireplace"),
            "with_opening_hours": tag_count("opening_hours"),
        },
        "camptocamp": {
            "documents": c2c.documents,
            "in_box": c2c.waypoints.len(),
            "by_type": count_by(c2c.waypoints.iter().map(|w| (w.waypoint_type.as_str(), w.country.as_deref() == Some("France")))),
            "with_unstaffed_capacity": c2c.waypoints.iter().filter(|w| w.capacity_unstaffed.is_some()).count(),
        },
        "links": {
            "places": links.len(),
            "by_method": by_method,
            "refuges_info_with_osm": links.iter().filter(|l| l.refuges_info.is_some() && !l.osm.is_empty()).count(),
            "refuges_info_with_camptocamp": links.iter().filter(|l| l.refuges_info.is_some() && !l.camptocamp.is_empty()).count(),
        },
    });

    let mut sources = vec![source(
        "refuges.info /api/bbox (instantané complet)",
        refuges_info::LICENCE,
        refuges_info::ATTRIBUTION,
        &ri.full,
    )];
    if let Some(f) = &ri.incremental {
        sources.push(source(
            "refuges.info /api/bbox depuis= (mise à jour incrémentale)",
            refuges_info::LICENCE,
            refuges_info::ATTRIBUTION,
            f,
        ));
    }
    for f in &ri.comments {
        sources.push(source(
            "refuges.info /api/commentaires",
            refuges_info::LICENCE,
            refuges_info::ATTRIBUTION,
            f,
        ));
    }
    sources.push(source(
        "OpenStreetMap (Overpass)",
        osm::LICENCE,
        osm::ATTRIBUTION,
        &osm_f,
    ));
    for f in &c2c.listing {
        sources.push(source(
            "Camptocamp /waypoints",
            camptocamp::LICENCE,
            camptocamp::ATTRIBUTION,
            f,
        ));
    }
    sources.push(SourceMeta {
        name: format!("Camptocamp /waypoints/<id> ({} documents)", c2c.documents),
        url: format!("{}/waypoints/<id>?cook=fr", camptocamp::API),
        licence: camptocamp::LICENCE.to_owned(),
        attribution: camptocamp::ATTRIBUTION.to_owned(),
        fetched_at: None,
        last_modified: None,
        etag: None,
        sha256: None,
    });
    sources.push(SourceMeta {
        name: "geo.api.gouv.fr communes (en France ou non)".to_owned(),
        url: "https://geo.api.gouv.fr/communes".to_owned(),
        licence: "Licence Ouverte 2.0".to_owned(),
        attribution: "geo.api.gouv.fr".to_owned(),
        fetched_at: None,
        last_modified: None,
        etag: None,
        sha256: None,
    });
    let valid_at = ri.full.meta.fetched_at_unix.max(osm_f.meta.fetched_at_unix);
    let meta = LayerMeta {
        layer: LAYER.to_owned(),
        site: site.id.to_owned(),
        tickets: vec!["P11 (#24)".to_owned()],
        generated_at: now_rfc3339(),
        valid_at: skimap_osm::output::unix_to_rfc3339(valid_at as i64),
        crs: "EPSG:4326 (WGS84, longitude, latitude)".to_owned(),
        sources,
        files: vec![
            FileMeta {
                path: "refuges_info.geojson".to_owned(),
                description: "points refuges.info de l'emprise (instantané hebdomadaire + mise à jour depuis=), dernier commentaire daté".to_owned(),
                licence: refuges_info::LICENCE.to_owned(),
                count: ri_features.len(),
                properties: json!({
                    "id": "identifiant refuges.info",
                    "name": "nom",
                    "type_id": "type (7 cabane non gardée, 9 gîte d'étape, 10 refuge gardé, 23 point d'eau...)",
                    "type_label": "libellé du type",
                    "alt": "altitude, m",
                    "state": "état (« Fermé au public », « Inutilisable », « Détruite »), null si ouvert ou inconnu",
                    "places": "places prévues pour dormir",
                    "places_mattress": "places sur matelas",
                    "stove": "poêle", "fireplace": "cheminée",
                    "forest_100m": "forêt à moins de 100 m (« bois » : pas une réserve de bois)",
                    "water_100m": "eau à moins de 100 m", "blankets": "couvertures",
                    "latrines": "latrines", "missing_wall": "manque un mur",
                    "url": "fiche refuges.info", "modified": "dernière modification de la fiche",
                    "access": "accès", "remark": "remarques", "owner": "auprès de qui se renseigner",
                    "last_comment": "dernier commentaire : id, point, date, text, author, photo",
                    "in_france": "commune française au point (geo.api.gouv.fr)",
                    "link": "index dans links.json",
                    "hiver": "aucun champ « ouvert en hiver » ni « local d'hiver » dans refuges.info",
                }),
            },
            FileMeta {
                path: "osm.geojson".to_owned(),
                description: "refuges et abris OSM (tourism=alpine_hut, tourism=wilderness_hut, amenity=shelter, ref:refuges.info) ; centre des ways et relations".to_owned(),
                licence: osm::LICENCE.to_owned(),
                count: osm_features.len(),
                properties: json!({
                    "osm_type": "node, way ou relation", "osm_id": "identifiant OSM",
                    "kind": "alpine_hut, wilderness_hut, shelter, bus_shelter (shelter_type=public_transport) ou other",
                    "refuges_info_id": "identifiant refuges.info (ref:refuges.info)",
                    "tags": format!("étiquettes retenues : {}", osm::KEPT_TAGS.join(", ")),
                    "in_france": "commune française au point", "link": "index dans links.json",
                }),
            },
            FileMeta {
                path: "camptocamp.geojson".to_owned(),
                description: "cabanes (hut), abris (shelter) et gîtes Camptocamp de l'emprise, avec la capacité hors gardiennage".to_owned(),
                licence: camptocamp::LICENCE.to_owned(),
                count: c2c_features.len(),
                properties: json!({
                    "id": "document Camptocamp", "title": "nom", "waypoint_type": "hut, shelter, gite",
                    "elevation": "altitude, m", "country": "pays (zone Camptocamp)",
                    "capacity_unstaffed": "places hors gardiennage (local d'hiver)",
                    "capacity_staffed": "places gardé",
                    "custodianship": "accessible_when_wardened, always_accessible, key_needed, no_warden",
                    "mattress_unstaffed": "matelas hors gardiennage", "blanket_unstaffed": "couvertures",
                    "gas_unstaffed": "gaz", "heating_unstaffed": "chauffage",
                    "access_period": "période de gardiennage (texte)", "quality": "qualité de la fiche",
                    "url": "page Camptocamp", "licence": "licence du document",
                    "attribution_url": "historique (auteurs) à citer",
                    "in_france": "pays = France", "link": "index dans links.json",
                }),
            },
            FileMeta {
                path: "links.json".to_owned(),
                description: "correspondances entre les trois sources (identifiants seulement) : ref:refuges.info d'OSM, sinon position (250 m) et nom, ou position seule (40 m)".to_owned(),
                licence: "identifiants ; couches séparées, licence de la combinaison à trancher (L3)".to_owned(),
                count: links.len(),
                properties: json!({
                    "refuges_info": "identifiant refuges.info", "osm": "éléments OSM",
                    "camptocamp": "documents Camptocamp", "methods": "méthode de chaque rattachement",
                }),
            },
        ],
        parameters: json!({
            "bbox_south_west_north_east": [site.bbox.south, site.bbox.west, site.bbox.north, site.bbox.east],
            "refuges_info_refresh": "instantané complet gardé 7 jours, puis requête depuis=<date de l'instantané> gardée 1 h ; commentaires gardés 1 jour",
            "links_near_m": links::NEAR_M,
            "links_same_spot_m": links::SAME_SPOT_M,
            "links_min_name_similarity": links::MIN_NAME,
        }),
        stats: stats.clone(),
    };
    write_json(&dir.join(format!("{LAYER}.meta.json")), &meta)?;
    Ok(RefugesOutput { dir, stats })
}

#[cfg(test)]
mod tests {
    #[test]
    fn crate_name() {
        assert!(super::CRATE.starts_with("skimap-"));
    }
}
