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
//! - `osm.geojson`: `tourism=alpine_hut`, `tourism=wilderness_hut`, `amenity=shelter`
//!   (local Geofabrik extracts, `skimap_osm::local`);
//! - `camptocamp.geojson`: huts, shelters and gîtes with the unstaffed capacity;
//! - `links.json`, `refuges.meta.json` (sources, licences, attributions, dates, figures).
//!
//! Each point also says whether it lies in France (`in_france`, commune found by
//! geo.api.gouv.fr): both site boxes overlap Italy or Switzerland.
//!
//! Pipeline side: uses `skimap_common::download`.

// Unit tests write their inputs directly; the code itself goes through
// `skimap_common::output` (clippy.toml, #164).
#![cfg_attr(test, allow(clippy::disallowed_methods))]

pub mod camptocamp;
pub mod error;
pub mod fiche;
pub mod journal;
pub mod links;
pub mod osm;
pub mod privacy;
pub mod refresh;
pub mod refuges_info;

use std::collections::{BTreeMap, HashMap};
use std::path::PathBuf;

use serde_json::{Value, json};
use skimap_common::download::{Downloader, Fetched, Request};
use skimap_common::geom::LonLat;
use skimap_common::layout::DataLayout;
use skimap_common::meta::{FileMeta, LayerMeta, SourceRef};
use skimap_common::output::{feature, point, write_geojson, write_json};
use skimap_common::site::Site;

pub use error::{RefugesError, Result};
pub use refresh::refresh;

/// Name of this crate, checked by the crate smoke test.
pub const CRATE: &str = env!("CARGO_PKG_NAME");
/// Layer name.
pub const LAYER: &str = "refuges";
/// Practical cards file (H1, #132).
pub const FICHES_FILE: &str = "refuges.geojson";

/// Result of [`build`].
#[derive(Debug, Clone)]
pub struct RefugesOutput {
    /// Layer directory.
    pub dir: PathBuf,
    /// Key figures (also in the meta file).
    pub stats: Value,
}

/// Whether a point lies in France: inside the French part of the site box (union of the
/// commune outlines of geo.api.gouv.fr, the same requests as the zones layer, cached),
/// else a commune is looked for at the point (box edges, foreign side).
fn in_france(
    dl: &Downloader,
    cache: &mut HashMap<(i64, i64), bool>,
    french: Option<&geo::MultiPolygon<f64>>,
    p: LonLat,
) -> Result<bool> {
    if french.is_some_and(|f| geo::Contains::contains(f, &geo::Point::new(p.lon, p.lat))) {
        return Ok(true);
    }
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

fn source(name: &str, licence: &str, attribution: &str, f: &Fetched) -> SourceRef {
    SourceRef::from_fetched(name, licence, attribution, f)
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
    let osm_read = osm::read(layout, dl, &site.bbox)?;
    let huts = osm_read.huts;
    let c2c = camptocamp::fetch(dl, &site.bbox)?;
    let links = links::link(&ri.points, &huts, &c2c.waypoints);

    let mut fr_cache = HashMap::new();
    // One point query per hut otherwise (about 100 per site, 1 s each on geo.api.gouv.fr):
    // the bottleneck of building every domain of France (G2). A box without any French
    // commune keeps the point queries.
    let french = skimap_zones::sources::jurisdiction::france_part(dl, site.bbox).ok();
    let mut ri_fr = Vec::with_capacity(ri.points.len());
    for p in &ri.points {
        ri_fr.push(in_france(dl, &mut fr_cache, french.as_ref(), p.pos)?);
    }
    let mut osm_fr = Vec::with_capacity(huts.len());
    for h in &huts {
        osm_fr.push(in_france(dl, &mut fr_cache, french.as_ref(), h.pos)?);
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

    // Practical cards (H1): refuges.info geometry + Camptocamp properties, CC BY-SA 4.0.
    let mut cards = fiche::fiches(&ri.points, &ri_fr, &c2c.waypoints);
    let read_at = ri
        .incremental
        .as_ref()
        .unwrap_or(&ri.full)
        .meta
        .fetched_at_unix;
    let read_at = skimap_common::meta::unix_to_rfc3339(read_at as i64);
    for (_, c) in &mut cards {
        c.checked_at = read_at.clone();
    }
    let card_features: Vec<Value> = cards
        .iter()
        .map(|(pos, c)| feature(point(*pos), serde_json::to_value(c).unwrap_or(Value::Null)))
        .collect();
    write_geojson(&dir.join(FICHES_FILE), &card_features)?;

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
        "fiches": {
            "cards": cards.len(),
            "by_kind": count_by(cards.iter().map(|(_, c)| (c.kind.as_str(), c.in_france))),
            "winter_access": count_by(cards.iter().map(|(_, c)| (c.winter_access.as_str(), c.in_france))),
            "with_phone": cards.iter().filter(|(_, c)| !c.phones.is_empty()).count(),
            "phones_by_source": count_by(cards.iter().flat_map(|(_, c)| c.phones.iter().map(|p| (p.source.as_str(), true)))),
            "with_website": cards.iter().filter(|(_, c)| c.website.is_some()).count(),
            "with_camptocamp": cards.iter().filter(|(_, c)| !c.camptocamp.is_empty()).count(),
            "with_places_winter": cards.iter().filter(|(_, c)| c.places_winter.is_some()).count(),
            "with_info_date": cards.iter().filter(|(_, c)| c.info_date.is_some()).count(),
            "staffed_or_gite_without_phone": cards.iter().filter(|(_, c)| matches!(c.kind.as_str(), "garde" | "gite") && c.open && c.phones.is_empty()).map(|(_, c)| c.name.clone()).collect::<Vec<_>>(),
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
    sources.extend(osm_read.sources);
    for f in &c2c.listing {
        sources.push(source(
            "Camptocamp /waypoints",
            camptocamp::LICENCE,
            camptocamp::ATTRIBUTION,
            f,
        ));
    }
    sources.push(SourceRef::new(
        format!("Camptocamp /waypoints/<id> ({} documents)", c2c.documents),
        format!("{}/waypoints/<id>?cook=fr", camptocamp::API),
        camptocamp::LICENCE.to_owned(),
        camptocamp::ATTRIBUTION.to_owned(),
    ));
    sources.push(SourceRef::new(
        "geo.api.gouv.fr communes (en France ou non)",
        "https://geo.api.gouv.fr/communes",
        "Licence Ouverte 2.0",
        "geo.api.gouv.fr",
    ));
    let osm_at = osm_read
        .valid_at
        .as_deref()
        .and_then(|t| chrono::DateTime::parse_from_rfc3339(t).ok())
        .map_or(0, |t| t.timestamp().max(0) as u64);
    let valid_at = ri.full.meta.fetched_at_unix.max(osm_at);
    let meta = LayerMeta {
        tickets: vec!["P11 (#24)".to_owned(), "H1 (#132)".to_owned()],
        valid_from: skimap_common::meta::unix_to_rfc3339(valid_at as i64),
        crs: Some("EPSG:4326 (WGS84, longitude, latitude)".to_owned()),
        sources,
        files: vec![
            FileMeta::new(FICHES_FILE, "fiches pratiques des refuges, gîtes et abris : géométrie refuges.info, propriétés Camptocamp rattachées par position et nom (sans OSM), une source par type d'objet (L3)")
                .licence(fiche::LICENCE)
                .count(card_features.len())
                .properties_json(&json!({
                    "id": "ri-<identifiant refuges.info>", "refuges_info_id": "identifiant refuges.info", "name": "nom",
                    "kind": "garde, gite, non_garde, abri (mur manquant, aucune place pour dormir ou « Abri » dans le nom), batiment",
                    "kind_label": "libellé français du type",
                    "state": "état refuges.info (« Fermé au public », « Détruite », « Inutilisable ») ; null si ouvert ou inconnu",
                    "open": "aucun état de fermeture",
                    "alt": "altitude, m",
                    "places": "places pour dormir (refuges.info)", "places_mattress": "places sur matelas (refuges.info)",
                    "places_winter": "places hors gardiennage, local d'hiver (Camptocamp capacity)",
                    "places_staffed": "places en période gardée (Camptocamp)",
                    "wardened_period": "période de gardiennage, texte Camptocamp filtré",
                    "winter_access": "ferme, local_hiver, cle, pendant_gardiennage, libre, inconnu",
                    "winter_access_text": "phrase française", "winter_access_source": "refuges.info ou camptocamp",
                    "equipment": "stove, fireplace (refuges.info), heating_winter, gas_winter (Camptocamp hors gardiennage), wood_nearby (forêt à moins de 100 m, pas une réserve de bois), water_nearby, blankets, mattresses, latrines ; null = inconnu",
                    "phones": "téléphones d'établissement seulement : e164, display, label (etablissement, sms, mairie, office_tourisme, pc_securite, onf, club), source ; jamais le numéro d'un particulier ni d'un gardien nommé",
                    "website": "site officiel", "website_source": "refuges.info (« Site officiel ») ou camptocamp (url)",
                    "access": "accès (texte filtré : téléphones, e-mails et noms retirés)", "remark": "remarques (filtrées)", "owner": "auprès de qui se renseigner (filtré)",
                    "refuges_info_url": "fiche refuges.info", "camptocamp": "documents Camptocamp : id, title, url, attribution_url (auteurs à citer)",
                    "info_date": "date de la dernière information (AAAA-MM-JJ) : modification de la fiche ou dernier commentaire",
                    "refuges_info_modified": "dernière modification de la fiche refuges.info", "last_comment_date": "date du dernier commentaire (texte et auteur non repris)",
                    "in_france": "commune française au point", "licence": "CC BY-SA 4.0", "attribution": "mention à afficher avec la fiche",
                    "osm": "lien OSM : pas dans ce fichier (ODbL) ; osm.geojson et links.json, usage personnel (L3)",
                })),
            FileMeta::new("refuges_info.geojson", "points refuges.info de l'emprise (instantané hebdomadaire + mise à jour depuis=), dernier commentaire daté")
                .licence(refuges_info::LICENCE)
                .count(ri_features.len())
                .properties_json(&json!({
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
                    "access": "accès (filtré : téléphones, e-mails, noms retirés)", "remark": "remarques (filtrées)", "owner": "auprès de qui se renseigner (filtré)",
                    "phones": "téléphones d'établissement tirés du champ propriétaire (e164, display, label, source)", "website": "site officiel (« Site officiel: » du champ propriétaire)",
                    "last_comment": "dernier commentaire : id, point, date, photo (texte et auteur jamais repris, H1)",
                    "in_france": "commune française au point (geo.api.gouv.fr)",
                    "link": "index dans links.json",
                    "hiver": "aucun champ « ouvert en hiver » ni « local d'hiver » dans refuges.info",
                })),
            FileMeta::new("osm.geojson", "refuges et abris OSM (tourism=alpine_hut, tourism=wilderness_hut, amenity=shelter, ref:refuges.info) ; centre des ways et relations")
                .licence(osm::LICENCE)
                .count(osm_features.len())
                .properties_json(&json!({
                    "osm_type": "node, way ou relation", "osm_id": "identifiant OSM",
                    "kind": "alpine_hut, wilderness_hut, shelter, bus_shelter (shelter_type=public_transport) ou other",
                    "refuges_info_id": "identifiant refuges.info (ref:refuges.info)",
                    "tags": format!("étiquettes retenues : {} (description filtrée ; phone et contact:phone jamais en étiquette)", osm::KEPT_TAGS.join(", ")),
                    "phones": "téléphones d'un tourism=alpine_hut (phone, contact:phone) ; aucun pour les cabanes et abris", "url": "page openstreetmap.org",
                    "in_france": "commune française au point", "link": "index dans links.json",
                })),
            FileMeta::new("camptocamp.geojson", "cabanes (hut), abris (shelter) et gîtes Camptocamp de l'emprise, avec la capacité hors gardiennage")
                .licence(camptocamp::LICENCE)
                .count(c2c_features.len())
                .properties_json(&json!({
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
                    "phones": "téléphone d'un gîte ou d'un refuge gardé (phone) ; phone_custodian jamais repris", "website": "site officiel (url)",
                    "in_france": "pays = France", "link": "index dans links.json",
                })),
            FileMeta::new("links.json", "correspondances entre les trois sources (identifiants seulement) : ref:refuges.info d'OSM, sinon position (250 m) et nom, ou position seule (40 m)")
                .licence("identifiants ; couches séparées, licence de la combinaison à trancher (L3)")
                .count(links.len())
                .properties_json(&json!({
                    "refuges_info": "identifiant refuges.info", "osm": "éléments OSM",
                    "camptocamp": "documents Camptocamp", "methods": "méthode de chaque rattachement",
                })),
        ],
        parameters: json!({
            "bbox_south_west_north_east": [site.bbox.south, site.bbox.west, site.bbox.north, site.bbox.east],
            "refuges_info_refresh": "instantané complet gardé 7 jours, puis requête depuis=<date de l'instantané> gardée 1 h ; commentaires gardés 1 jour",
            "links_near_m": links::NEAR_M,
            "links_same_spot_m": links::SAME_SPOT_M,
            "links_min_name_similarity": links::MIN_NAME,
        }),
        stats: stats.clone(),
        ..LayerMeta::new(LAYER, site.id)
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
