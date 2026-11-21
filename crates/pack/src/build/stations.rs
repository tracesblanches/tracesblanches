//! Station sheets and trail-pass zones of the pack, from the France-wide outputs of
//! `skimap-stations` (`data/out/france/stations/`, crates/stations/README.md), cut to the
//! site:
//!
//! - `stations` (public, ODbL, SkiRadar attribution): the sheet of every domain whose
//!   representative point lies in the site box (rescue numbers with 112 first, rescue
//!   posts, sale points, contacts, parkings);
//! - `stations-pois` (public, ODbL): the OSM points (rescue, sale, tourist office, ski
//!   school, parking) attached to those domains;
//! - `stations-status` (public): when the sheets (and, with `personal_sources`, the
//!   passes) were last read, and the policy that marks a value stale, so that the app and
//!   the viewer show an outdated value as such;
//! - `fee-zones` (only with `personal_sources`: the passes, their prices and members come
//!   from pages without a reuse licence): one feature per pass valid on a domain of the
//!   site, the footprints of its domains that touch the site box, with the schema the app
//!   reads (`android/docs/telechargement.md`): `fee_id`, `name`, `issuer`, `kind`,
//!   `prices`, plus `scope`, `season`, `status`, `domains`, `ways` (OSM ways of the pass
//!   in the box, from `couverture.json`).
//!
//! The France-wide metadata lists every source of the stations crate; each layer keeps
//! only its own (public ones for the public layers, deduplicated by attribution).

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use serde_json::{Value, json};
use skimap_common::geom::{BBox, LonLat};
use skimap_common::layout::DataLayout;

use crate::build::layers::{EntryParts, Outcome, copy_meta, layer_entry, read_meta, unauthorised};
use crate::build::write_bytes;
use crate::error::{Error, Result};
use crate::manifest::{LayerKind, Missing, Source};

/// Directory of the France-wide outputs under `data/out/france/`.
pub const ORIGIN: &str = "stations";
/// Metadata file of the stations crate.
pub const META: &str = "stations.meta.json";

fn read_json(path: &Path) -> Result<Option<Value>> {
    if !path.is_file() {
        return Ok(None);
    }
    let text = std::fs::read_to_string(path).map_err(|e| Error::io(path, e))?;
    Ok(Some(
        serde_json::from_str(&text).map_err(|e| Error::json(path.display().to_string(), e))?,
    ))
}

fn features(v: &Value) -> impl Iterator<Item = &Value> {
    v["features"].as_array().into_iter().flatten()
}

fn point_of(f: &Value) -> Option<LonLat> {
    let c = f["geometry"]["coordinates"].as_array()?;
    Some(LonLat::new(c.first()?.as_f64()?, c.get(1)?.as_f64()?))
}

/// Box of a ring list (`[[lon, lat], ...]`).
fn ring_box(poly: &Value) -> Option<BBox> {
    let (mut w, mut s, mut e, mut n) = (f64::MAX, f64::MAX, f64::MIN, f64::MIN);
    for ring in poly.as_array()? {
        for p in ring.as_array()? {
            let (x, y) = (p.get(0)?.as_f64()?, p.get(1)?.as_f64()?);
            w = w.min(x);
            e = e.max(x);
            s = s.min(y);
            n = n.max(y);
        }
    }
    (w <= e).then(|| BBox::new(s, w, n, e))
}

fn overlaps(a: &BBox, b: &BBox) -> bool {
    a.west <= b.east && b.west <= a.east && a.south <= b.north && b.south <= a.north
}

/// Sources of a layer: those of the metadata that are (`personal`) or are not personal
/// use only, one per attribution and licence.
fn sources_of(all: &[Source], personal: bool) -> Vec<Source> {
    let mut seen = BTreeSet::new();
    all.iter()
        .filter(|s| unauthorised(s) == personal)
        .filter(|s| seen.insert((s.attribution.clone(), s.licence.clone())))
        .cloned()
        .collect()
}

fn missing(id: &str, reason: &str) -> Outcome {
    Outcome::Missing(Missing {
        id: id.to_owned(),
        reason: reason.to_owned(),
    })
}

/// Writes one GeoJSON or JSON layer of this module and returns its entry.
#[allow(clippy::too_many_arguments)]
fn entry(
    root: &Path,
    dir: &Path,
    id: &str,
    title: &str,
    kind: LayerKind,
    origin: &str,
    value: &Value,
    sources: Vec<Source>,
    notes: &[&str],
    with_meta: bool,
) -> Result<Outcome> {
    let ext = if kind == LayerKind::Data {
        "json"
    } else {
        "geojson"
    };
    let file = format!("layers/{id}.{ext}");
    let bytes = serde_json::to_vec(value).map_err(|e| Error::json(&file, e))?;
    let size = write_bytes(&root.join(&file), &bytes)?;
    let meta = read_meta(&dir.join(META))?;
    // The France-wide metadata names the personal-use sources too: copied into a pack
    // built with them only (the entry lists the sources of the layer in any case).
    let meta_rel = if with_meta {
        copy_meta(dir, META, root)?
    } else {
        None
    };
    let mut l = layer_entry(
        EntryParts {
            id,
            title,
            kind,
            format: ext.to_owned(),
            file,
            origin: format!("france/{ORIGIN}/{origin}"),
            meta_rel,
            bytes: size,
            visible: false,
            notes,
        },
        meta.as_ref(),
    );
    l.personal_use_only = sources.iter().any(unauthorised);
    l.sources = sources;
    if kind == LayerKind::Vector {
        let n = value["features"].as_array().map_or(0, Vec::len) as u64;
        l.features = Some(n);
        let types: BTreeSet<String> = features(value)
            .filter_map(|f| f["geometry"]["type"].as_str().map(str::to_owned))
            .collect();
        l.geometry_types = types.into_iter().collect();
    }
    Ok(Outcome::Included(Box::new(l)))
}

/// The layers of this module for a site box (see the module documentation).
pub fn include(
    layout: &DataLayout,
    bbox: BBox,
    root: &Path,
    personal_sources: bool,
) -> Result<Vec<Outcome>> {
    let dir = layout.france_dir().join(ORIGIN);
    let Some(stations) = read_json(&dir.join("stations.geojson"))? else {
        return Ok(["stations", "stations-pois", "stations-status", "fee-zones"]
            .iter()
            .map(|id| {
                missing(
                    id,
                    "france/stations/stations.geojson absent des sorties du pipeline (skimap-stations)",
                )
            })
            .collect());
    };
    let all_sources = read_meta(&dir.join(META))?
        .map(|m| m.meta.sources)
        .unwrap_or_default();
    let public = sources_of(&all_sources, false);
    let mut out = Vec::new();

    // Sheets of the domains of the box.
    let kept: Vec<Value> = features(&stations)
        .filter(|f| point_of(f).is_some_and(|p| bbox.contains(p)))
        .cloned()
        .collect();
    let ids: BTreeSet<String> = kept
        .iter()
        .filter_map(|f| f["properties"]["id"].as_str().map(str::to_owned))
        .collect();
    out.push(entry(
        root,
        &dir,
        "stations",
        "Fiches des stations (secours, vente, contacts)",
        LayerKind::Vector,
        "stations.geojson",
        &json!({"type": "FeatureCollection", "features": kept}),
        public.clone(),
        &[
            "Fiche de chaque domaine dont le point représentatif est dans l'emprise : `secours.numbers` (112 toujours en premier), `secours.posts`, `vente`, `links`, `phones`, `parkings`, `checked_at`, `changed_at`. Information seulement, pas d'achat.",
        ],
        personal_sources,
    )?);

    // Points of those domains.
    match read_json(&dir.join("pois.geojson"))? {
        Some(pois) => {
            let kept: Vec<Value> = features(&pois)
                .filter(|f| {
                    f["properties"]["domains"]
                        .as_array()
                        .into_iter()
                        .flatten()
                        // `{id, distance_m}` (or a bare identifier).
                        .any(|d| {
                            d["id"]
                                .as_str()
                                .or_else(|| d.as_str())
                                .is_some_and(|d| ids.contains(d))
                        })
                })
                .cloned()
                .collect();
            out.push(entry(
                root,
                &dir,
                "stations-pois",
                "Points des stations (secours, vente, offices, écoles, parkings)",
                LayerKind::Vector,
                "pois.geojson",
                &json!({"type": "FeatureCollection", "features": kept}),
                public.clone(),
                &[
                    "Points OSM rattachés aux domaines de l'emprise : `category` (secours, vente, office_tourisme, ecole_ski, parking), `kind`, `name`, `phones`, `opening_hours`, `domains`.",
                ],
                personal_sources,
            )?);
        }
        None => out.push(missing(
            "stations-pois",
            "france/stations/pois.geojson absent des sorties du pipeline",
        )),
    }

    // Freshness of the sheets (and of the passes with the personal sources).
    let status = read_json(&dir.join("status.json"))?.unwrap_or(Value::Null);
    let mut s = json!({
        "checked_at": status["checked_at"],
        "policy": status["policy"],
        "fiches": status["parts"]["fiches"],
    });
    if personal_sources {
        s["titres"] = status["parts"]["titres"].clone();
        s["stale_titres"] = status["stale_titres"].clone();
    }
    out.push(entry(
        root,
        &dir,
        "stations-status",
        "Fraîcheur des fiches des stations",
        LayerKind::Data,
        "status.json",
        &s,
        public,
        &[
            "Dernière lecture réussie des fiches (`fiches.last_ok`) et, avec les sources personnelles, des titres (`titres`, `stale_titres`) ; `policy.stale_max_days` : au-delà, une valeur est périmée et dite telle.",
        ],
        personal_sources,
    )?);

    // Trail-pass zones: personal use only.
    if !personal_sources {
        out.push(missing(
            "fee-zones",
            "titres de redevance et leurs zones : sources à usage personnel seulement (option personal_sources désactivée)",
        ));
        return Ok(out);
    }
    let (Some(titres), Some(couverture)) = (
        read_json(&dir.join("titres.geojson"))?,
        read_json(&dir.join("couverture.json"))?,
    ) else {
        out.push(missing(
            "fee-zones",
            "france/stations/titres.geojson ou couverture.json absent (skimap-stations sans personal_sources)",
        ));
        return Ok(out);
    };
    out.push(entry(
        root,
        &dir,
        "fee-zones",
        "Zones de validité des redevances",
        LayerKind::Vector,
        "titres.geojson + couverture.json",
        &fee_zones(&titres, &couverture, &ids, bbox),
        sources_of(&all_sources, true),
        &[
            "Un titre par entité, valable sur un domaine de l'emprise (`couverture.json`), géométrie : emprises de ses domaines qui touchent l'emprise du site. `kind` : site, groupement, departement, region (massif) ou national, comme dans android/docs/telechargement.md ; `scope` brut ; `status` (ok, stale, perime) ; `prices` indicatifs et datés ; `ways` : voies OSM du titre dans l'emprise. Usage personnel.",
        ],
        personal_sources,
    )?);
    Ok(out)
}

/// `kind` of the app schema from the `scope` of the stations crate.
pub fn kind_of(scope: &str) -> &'static str {
    match scope {
        "site" => "site",
        "regroupement" => "groupement",
        "departemental" => "departement",
        "massif" | "regional" => "region",
        _ => "national",
    }
}

/// The fee zones of the domains `ids` (see [`include()`]).
pub fn fee_zones(titres: &Value, couverture: &Value, ids: &BTreeSet<String>, bbox: BBox) -> Value {
    // Passes valid on a domain of the site, and on which of its domains.
    let mut domains_of: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for id in ids {
        for t in couverture["domains"][id]["titres"]
            .as_array()
            .into_iter()
            .flatten()
        {
            if let Some(t) = t["id"].as_str() {
                domains_of.entry(t.to_owned()).or_default().push(id.clone());
            }
        }
    }
    let mut out = Vec::new();
    for f in features(titres) {
        let p = &f["properties"];
        let Some(id) = p["id"].as_str() else { continue };
        let Some(domains) = domains_of.get(id) else {
            continue;
        };
        let polys: Vec<Value> = match f["geometry"]["type"].as_str() {
            Some("Polygon") => vec![f["geometry"]["coordinates"].clone()],
            Some("MultiPolygon") => f["geometry"]["coordinates"]
                .as_array()
                .cloned()
                .unwrap_or_default(),
            _ => Vec::new(),
        };
        let near: Vec<Value> = polys
            .into_iter()
            .filter(|poly| ring_box(poly).is_some_and(|b| overlaps(&b, &bbox)))
            .collect();
        if near.is_empty() {
            continue;
        }
        let ways: Vec<Value> = couverture["ways"][id]
            .as_array()
            .cloned()
            .unwrap_or_default();
        let scope = p["scope"].as_str().unwrap_or("");
        let prices: Vec<Value> = p["adult_full_price_eur"]
            .as_f64()
            .map(|eur| {
                json!({"label": "Adulte, plein tarif", "price_eur": eur,
                       "date": p["season"].clone()})
            })
            .into_iter()
            .collect();
        out.push(json!({"type": "Feature",
        "geometry": {"type": "MultiPolygon", "coordinates": near},
        "properties": {
            "fee_id": id,
            "name": p["name"],
            "issuer": p["issuer"],
            "kind": kind_of(scope),
            "scope": scope,
            "season": p["season"],
            "status": p["status"],
            "prices": prices,
            "domains": domains,
            "ways": ways.len(),
            "personal_use_only": true,
        }}));
    }
    json!({"type": "FeatureCollection", "features": out})
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fee_zones_keep_the_passes_of_the_site_and_the_parts_near_it() {
        let square = |x: f64| {
            json!([[
                [x, 45.0],
                [x + 0.1, 45.0],
                [x + 0.1, 45.1],
                [x, 45.1],
                [x, 45.0]
            ]])
        };
        let titres = json!({"type": "FeatureCollection", "features": [
            {"type": "Feature", "geometry": {"type": "MultiPolygon", "coordinates": [square(6.0), square(8.0)]},
             "properties": {"id": "nat", "name": "National", "issuer": "NF", "scope": "national",
                            "adult_full_price_eur": 255.0, "season": "2026-2027", "status": "ok"}},
            {"type": "Feature", "geometry": {"type": "MultiPolygon", "coordinates": [square(6.0)]},
             "properties": {"id": "site", "name": "Site", "issuer": "Foyer", "scope": "site",
                            "adult_full_price_eur": null, "status": "stale"}},
            {"type": "Feature", "geometry": {"type": "MultiPolygon", "coordinates": [square(9.0)]},
             "properties": {"id": "other", "name": "Ailleurs", "scope": "massif"}}
        ]});
        let couverture = json!({"domains": {"d1": {"titres": [{"id": "nat"}, {"id": "site"}]},
                                            "d2": {"titres": [{"id": "other"}]}},
                                "ways": {"nat": [1, 2, 3], "site": [1]}});
        let ids: BTreeSet<String> = ["d1".to_owned()].into();
        let fc = fee_zones(
            &titres,
            &couverture,
            &ids,
            BBox::new(44.95, 5.95, 45.2, 6.2),
        );
        let fs = fc["features"].as_array().unwrap();
        assert_eq!(fs.len(), 2);
        let nat = &fs[0];
        assert_eq!(nat["properties"]["fee_id"], "nat");
        assert_eq!(nat["properties"]["kind"], "national");
        // The part far from the site is left out.
        assert_eq!(nat["geometry"]["coordinates"].as_array().unwrap().len(), 1);
        assert_eq!(nat["properties"]["prices"][0]["price_eur"], 255.0);
        assert_eq!(nat["properties"]["ways"], 3);
        let site = &fs[1];
        assert_eq!(site["properties"]["kind"], "site");
        assert_eq!(site["properties"]["status"], "stale");
        assert_eq!(site["properties"]["prices"], json!([]));
        assert_eq!(kind_of("massif"), "region");
        assert_eq!(kind_of("regroupement"), "groupement");
        assert_eq!(kind_of("departemental"), "departement");
    }
}
