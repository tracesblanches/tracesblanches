//! Station-level contacts: web sites and phones of the station itself.
//!
//! - OSM `site=piste` relation of the domain (ODbL): `website`, `contact:website`, `phone`,
//!   `contact:phone`, `operator` (one Overpass query for all relations);
//! - OpenSkiMap `ski_areas.csv` (ODbL): `websites` of the areas overlapping the domain;
//! - SkiRadar geo-feed (« Données factuelles réutilisables avec attribution « skiradar.fr »
//!   et lien vers la fiche »): official site and ski school (name, site) of the station
//!   matched to the domain by the census (`rapprochements.json`, within 3 km).

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use serde::Serialize;
use serde_json::Value;
use skimap_common::download::{Downloader, Fetched, Request};
use skimap_osm::census::{OPENSKIMAP_AREAS_URL, SKIRADAR_URL};
use skimap_refuges::privacy::{self, Phone};

use crate::domains::Domain;
use crate::error::{Result, StationsError};

/// SkiRadar attribution required by its licence line.
pub const SKIRADAR_ATTRIBUTION: &str = "skiradar.fr (lien vers la fiche de la station)";
/// SkiRadar licence line (read in the feed, 14/11/2026).
pub const SKIRADAR_LICENCE: &str =
    "Données factuelles réutilisables avec attribution « skiradar.fr » et lien vers la fiche";

/// A web link with its source.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize)]
pub struct Link {
    /// URL.
    pub url: String,
    /// What it is: `station`, `ecole_ski`, `skiradar` (the SkiRadar page, required by its
    /// licence).
    pub role: String,
    /// Label (school name...).
    pub label: Option<String>,
    /// Source: `openstreetmap`, `openskimap`, `skiradar`.
    pub source: String,
}

/// Contacts of a domain.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct StationContacts {
    /// Web links, without duplicates.
    pub links: Vec<Link>,
    /// Phones of the station (OSM site relation).
    pub phones: Vec<Phone>,
    /// Operator (OSM site relation).
    pub operator: Option<String>,
}

fn split_urls(s: &str) -> impl Iterator<Item = &str> {
    s.split([';', ' ', ',', '\n'])
        .map(str::trim)
        .filter(|u| u.starts_with("http"))
}

/// `websites` of OpenSkiMap areas, by area id.
pub fn parse_openskimap_websites(csv_text: &[u8]) -> Result<BTreeMap<String, Vec<String>>> {
    let mut r = csv::Reader::from_reader(csv_text);
    let h = r
        .headers()
        .map_err(|e| StationsError::format("openskimap", e.to_string()))?
        .clone();
    let col = |n: &str| {
        h.iter()
            .position(|x| x == n)
            .ok_or_else(|| StationsError::format("openskimap", format!("no column {n}")))
    };
    let (ci, cw) = (col("id")?, col("websites")?);
    let mut out = BTreeMap::new();
    for rec in r.records() {
        let rec = rec.map_err(|e| StationsError::format("openskimap", e.to_string()))?;
        let urls: Vec<String> = split_urls(rec.get(cw).unwrap_or_default())
            .map(str::to_owned)
            .collect();
        if !urls.is_empty() {
            out.insert(rec.get(ci).unwrap_or_default().to_owned(), urls);
        }
    }
    Ok(out)
}

/// A SkiRadar station: page, official site, ski school.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct SkiRadar {
    /// SkiRadar page.
    pub url: Option<String>,
    /// Official site of the station.
    pub official: Option<String>,
    /// Ski school name and site.
    pub school: Option<(String, Option<String>)>,
}

/// Stations of one SkiRadar page, by id; and the number of pages.
pub fn parse_skiradar(v: &Value) -> (BTreeMap<String, SkiRadar>, u64) {
    let mut out = BTreeMap::new();
    for s in v["stations"].as_array().into_iter().flatten() {
        let Some(id) = s["id"].as_str() else { continue };
        let st = |p: &str| s.pointer(p).and_then(Value::as_str).map(str::to_owned);
        out.insert(
            id.to_owned(),
            SkiRadar {
                url: st("/url"),
                official: st("/siteOfficiel").filter(|u| u.starts_with("http")),
                school: st("/ecoleDeSki/nom").map(|n| (n, st("/ecoleDeSki/site"))),
            },
        );
    }
    (out, v["totalPages"].as_u64().unwrap_or(1))
}

/// Tags of OSM relations from an Overpass answer, by relation id.
pub fn parse_relations(v: &Value) -> BTreeMap<i64, BTreeMap<String, String>> {
    v["elements"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|e| {
            let id = e["id"].as_i64()?;
            let tags = e["tags"]
                .as_object()?
                .iter()
                .filter_map(|(k, v)| v.as_str().map(|s| (k.clone(), s.to_owned())))
                .collect();
            Some((id, tags))
        })
        .collect()
}

/// Domain → SkiRadar station ids, from the census `rapprochements.json`.
pub fn skiradar_matches(census_dir: &Path) -> Result<BTreeMap<String, Vec<String>>> {
    let path = census_dir.join("rapprochements.json");
    let b = std::fs::read(&path)
        .map_err(|e| StationsError::Input(format!("{} ({e})", path.display())))?;
    let v: Value = serde_json::from_slice(&b)
        .map_err(|e| StationsError::format("rapprochements.json", e.to_string()))?;
    let mut out: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for m in v
        .pointer("/skiradar/matches")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        if let (Some(d), Some(id)) = (m["domain"].as_str(), m["skiradar_id"].as_str()) {
            out.entry(d.to_owned()).or_default().push(id.to_owned());
        }
    }
    Ok(out)
}

/// What [`fetch`] read.
#[derive(Debug, Clone)]
pub struct Fetch {
    /// Contacts by domain id.
    pub by_domain: BTreeMap<String, StationContacts>,
    /// Downloads, for the metadata: (source name, licence, attribution, answer).
    pub sources: Vec<(String, String, String, Fetched)>,
}

/// Reads the station contacts of every domain.
pub fn fetch(dl: &Downloader, domains: &[Domain], census_dir: &Path) -> Result<Fetch> {
    let mut sources = Vec::new();
    // OSM site relations.
    let rels: Vec<i64> = domains.iter().filter_map(|d| d.site_relation).collect();
    let ids = rels
        .iter()
        .map(i64::to_string)
        .collect::<Vec<_>>()
        .join(",");
    let q = format!("[out:json][timeout:120];rel(id:{ids});out tags;");
    let (f, v) = crate::pois::overpass(dl, &q, &std::cell::Cell::new(0))?;
    let rel_tags = parse_relations(&v);
    sources.push((
        "OpenStreetMap (Overpass), relations site=piste des domaines".to_owned(),
        crate::pois::LICENCE.to_owned(),
        crate::pois::ATTRIBUTION.to_owned(),
        f,
    ));
    // OpenSkiMap.
    let f = dl.fetch(&Request::get("openskimap", OPENSKIMAP_AREAS_URL))?;
    let osk = parse_openskimap_websites(&f.bytes()?)?;
    sources.push((
        "OpenSkiMap ski_areas.csv (websites)".to_owned(),
        "ODbL 1.0".to_owned(),
        "OpenSkiData / OpenSkiMap.org, © les contributeurs d'OpenStreetMap (ODbL)".to_owned(),
        f,
    ));
    // SkiRadar (same requests as the census: served from the cache when fresh).
    let mut sr: BTreeMap<String, SkiRadar> = BTreeMap::new();
    let mut page = 1;
    loop {
        let f = dl.fetch(&Request::get(
            "skiradar",
            &format!("{SKIRADAR_URL}?page={page}"),
        ))?;
        let (st, pages) = parse_skiradar(&f.json()?);
        sr.extend(st);
        sources.push((
            format!("SkiRadar geo-feed, page {page}"),
            SKIRADAR_LICENCE.to_owned(),
            SKIRADAR_ATTRIBUTION.to_owned(),
            f,
        ));
        page += 1;
        if page > pages {
            break;
        }
    }
    let matches = skiradar_matches(census_dir)?;

    let mut by_domain = BTreeMap::new();
    for d in domains {
        let mut c = StationContacts::default();
        let mut seen: BTreeSet<(String, String)> = BTreeSet::new();
        let mut push = |c: &mut StationContacts,
                        url: &str,
                        role: &str,
                        label: Option<String>,
                        source: &str| {
            let key = (url.trim_end_matches('/').to_lowercase(), role.to_owned());
            if seen.insert(key) {
                c.links.push(Link {
                    url: url.to_owned(),
                    role: role.to_owned(),
                    label,
                    source: source.to_owned(),
                });
            }
        };
        if let Some(t) = d.site_relation.and_then(|r| rel_tags.get(&r)) {
            for k in ["website", "contact:website", "url"] {
                for u in t
                    .get(k)
                    .map(|v| split_urls(v).collect::<Vec<_>>())
                    .unwrap_or_default()
                {
                    push(&mut c, u, "station", None, "openstreetmap");
                }
            }
            for k in ["phone", "contact:phone"] {
                if let Some(v) = t.get(k) {
                    for p in privacy::structured_phone(v, "station", "openstreetmap") {
                        if !c.phones.iter().any(|x| x.e164 == p.e164) {
                            c.phones.push(p);
                        }
                    }
                }
            }
            c.operator = t.get("operator").cloned();
        }
        for a in &d.openskimap {
            for u in osk.get(a).into_iter().flatten() {
                push(&mut c, u, "station", None, "openskimap");
            }
        }
        for id in matches.get(&d.id).into_iter().flatten() {
            if let Some(s) = sr.get(id) {
                if let Some(u) = &s.official {
                    push(&mut c, u, "station", None, "skiradar");
                }
                if let Some((n, Some(u))) = &s.school {
                    push(&mut c, u, "ecole_ski", Some(n.clone()), "skiradar");
                }
                if let Some(u) = &s.url {
                    push(&mut c, u, "skiradar", None, "skiradar");
                }
            }
        }
        by_domain.insert(d.id.clone(), c);
    }
    Ok(Fetch { by_domain, sources })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn openskimap_websites() {
        let csv =
            b"name,id,websites,has_nordic\nA,x1,https://a.fr/ ; https://b.fr,yes\nB,x2,,yes\n";
        let w = parse_openskimap_websites(csv).unwrap();
        assert_eq!(w["x1"], vec!["https://a.fr/", "https://b.fr"]);
        assert!(!w.contains_key("x2"));
    }

    #[test]
    fn skiradar() {
        let v = json!({"totalPages": 2, "stations": [{"id": "abondance", "url": "https://skiradar.fr/stations/abondance",
            "siteOfficiel": "https://www.station.fr", "ecoleDeSki": {"nom": "ESF Abondance", "site": "https://esf.fr/"}}]});
        let (s, pages) = parse_skiradar(&v);
        assert_eq!(pages, 2);
        assert_eq!(
            s["abondance"].official.as_deref(),
            Some("https://www.station.fr")
        );
        assert_eq!(s["abondance"].school.as_ref().unwrap().0, "ESF Abondance");
    }
}
