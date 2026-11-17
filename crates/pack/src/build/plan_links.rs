//! Link to the official piste plan of each domain (`plan_url`, G1 #117), in public packs
//! too: an address is not a redistribution, the plan itself never goes into a pack.
//!
//! Source: the catalogue of the plan collector (`data/out/france/plans/catalogue.json`,
//! `skimap-pisteplan` `collect`). For each census domain, the files that name it (a
//! duplicate names it for its original file) are kept when downloaded without error and
//! readable (PDF or picture); a plan of the cross-country pistes is preferred to one that
//! says « raquettes » or « piétons » only, then the most recent (upload date on the
//! publisher's site, else creation date of the PDF). The pistes get the link of the
//! domain of their OSM way (`data/out/france/domaines/domaines_ways.json`).
//!
//! The pack layers `domaines`, `stations` and `pistes` get `plan_url` (address of the
//! file), `plan_title` and `plan_publisher` on the features of a domain that has a plan.

use std::collections::{BTreeMap, HashMap};
use std::path::Path;

use serde_json::{Value, json};
use skimap_common::layout::DataLayout;

use crate::error::{Error, Result};
use crate::manifest::Layer;

/// Link to the official plan of a domain.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlanLink {
    /// Address of the file (PDF or picture) on the publisher's site.
    pub url: String,
    /// Title of the file.
    pub title: String,
    /// Publisher (station, massif, Nordic France).
    pub publisher: String,
    /// Date used to pick the most recent file (`YYYY-MM-DD`, empty if unknown).
    pub date: String,
}

/// Plan links by census domain and by OSM way.
#[derive(Debug, Clone, Default)]
pub struct PlanLinks {
    by_domain: BTreeMap<String, PlanLink>,
    domain_of_way: HashMap<i64, String>,
}

/// Date of a `pdfinfo` creation date (`Wed Nov 14 10:02:44 2018 CET`), `YYYY-MM-DD`.
fn pdfinfo_date(s: &str) -> Option<String> {
    const MONTHS: [&str; 12] = [
        "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
    ];
    let w: Vec<&str> = s.split_whitespace().collect();
    let m = w.iter().find_map(|t| MONTHS.iter().position(|m| m == t))?;
    let i = w.iter().position(|t| MONTHS.contains(t))?;
    let day: u32 = w.get(i + 1)?.parse().ok()?;
    let year: u32 = w
        .iter()
        .skip(i + 2)
        .find_map(|t| t.parse::<u32>().ok().filter(|y| (1990..=2100).contains(y)))?;
    Some(format!("{year:04}-{:02}-{day:02}", m + 1))
}

fn date_of(c: &Value) -> String {
    c["candidate"]["uploaded"]
        .as_str()
        .filter(|s| s.len() >= 10)
        .map(|s| s[..10].to_owned())
        .or_else(|| c["created"].as_str().and_then(pdfinfo_date))
        .unwrap_or_default()
}

fn readable(c: &Value) -> bool {
    c["error"].is_null()
        && c["duplicate_of"].is_null()
        && c["format"]
            .as_str()
            .is_some_and(|f| f.starts_with("pdf") || f == "image")
}

/// Whether a title says the file is a plan for snowshoes or walkers only.
fn not_ski(title: &str, url: &str) -> bool {
    let t = format!("{title} {url}").to_lowercase();
    (t.contains("raquette") || t.contains("pieton") || t.contains("piéton"))
        && !(t.contains("ski") || t.contains("fond"))
}

impl PlanLinks {
    /// Links from the collector's catalogue (`catalogue`, JSON array) and the ways of each
    /// domain (`ways`, `{domain: [way id…]}`).
    pub fn from_values(catalogue: &Value, ways: Option<&Value>) -> Self {
        let all: Vec<&Value> = catalogue.as_array().into_iter().flatten().collect();
        let by_id: HashMap<&str, &Value> = all
            .iter()
            .filter_map(|c| c["candidate"]["id"].as_str().map(|id| (id, *c)))
            .collect();
        // Domains named by each readable file, those of its duplicates included.
        let mut named: BTreeMap<String, Vec<&Value>> = BTreeMap::new();
        for c in &all {
            let file = match c["duplicate_of"].as_str() {
                Some(orig) => match by_id.get(orig) {
                    Some(o) => *o,
                    None => continue,
                },
                None => *c,
            };
            if !readable(file) {
                continue;
            }
            for d in c["candidate"]["domains"].as_array().into_iter().flatten() {
                if let Some(d) = d.as_str() {
                    let v = named.entry(d.to_owned()).or_default();
                    if !v.iter().any(|x| std::ptr::eq(*x, file)) {
                        v.push(file);
                    }
                }
            }
        }
        let mut by_domain = BTreeMap::new();
        for (d, files) in named {
            let best = files.into_iter().max_by(|a, b| {
                let key = |c: &Value| {
                    let title = c["candidate"]["title"].as_str().unwrap_or("");
                    let url = c["candidate"]["url"].as_str().unwrap_or("");
                    (!not_ski(title, url), date_of(c), url.to_owned())
                };
                key(a).cmp(&key(b))
            });
            if let Some(c) = best {
                let s = |k: &str| c["candidate"][k].as_str().unwrap_or("").to_owned();
                by_domain.insert(
                    d,
                    PlanLink {
                        url: s("url"),
                        title: s("title"),
                        publisher: s("publisher"),
                        date: date_of(c),
                    },
                );
            }
        }
        let mut domain_of_way = HashMap::new();
        for (d, ids) in ways.and_then(Value::as_object).into_iter().flatten() {
            if !by_domain.contains_key(d) {
                continue;
            }
            for id in ids
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(Value::as_i64)
            {
                // A way of two domains: the first in identifier order keeps it.
                domain_of_way.entry(id).or_insert_with(|| d.clone());
            }
        }
        Self {
            by_domain,
            domain_of_way,
        }
    }

    /// Links of the data directory (empty when the collector has not run).
    pub fn load(layout: &DataLayout) -> Result<Self> {
        let france = layout.france_dir();
        let read = |p: &Path| -> Result<Option<Value>> {
            if !p.is_file() {
                return Ok(None);
            }
            let text = std::fs::read_to_string(p).map_err(|e| Error::io(p, e))?;
            Ok(Some(
                serde_json::from_str(&text).map_err(|e| Error::json(p.display().to_string(), e))?,
            ))
        };
        let Some(cat) = read(&france.join("plans").join("catalogue.json"))? else {
            return Ok(Self::default());
        };
        let ways = read(&france.join("domaines").join("domaines_ways.json"))?;
        Ok(Self::from_values(&cat, ways.as_ref()))
    }

    /// Link of a census domain.
    pub fn for_domain(&self, id: &str) -> Option<&PlanLink> {
        self.by_domain.get(id)
    }

    /// Link of the domain of an OSM way.
    pub fn for_way(&self, id: i64) -> Option<&PlanLink> {
        self.domain_of_way
            .get(&id)
            .and_then(|d| self.by_domain.get(d))
    }

    /// Domains with a link.
    pub fn len(&self) -> usize {
        self.by_domain.len()
    }

    /// Whether no domain has a link.
    pub fn is_empty(&self) -> bool {
        self.by_domain.is_empty()
    }
}

fn set_link(props: &mut serde_json::Map<String, Value>, l: &PlanLink) {
    props.insert("plan_url".into(), json!(l.url));
    props.insert("plan_title".into(), json!(l.title));
    props.insert("plan_publisher".into(), json!(l.publisher));
}

/// Adds the plan links to a FeatureCollection: by `properties.id` (census domain) for
/// `domaines` and `stations`, by `properties.osm_way_id` for `pistes`. Returns the
/// number of features linked.
pub fn annotate(v: &mut Value, layer: &str, links: &PlanLinks) -> usize {
    let mut n = 0;
    for f in v["features"].as_array_mut().into_iter().flatten() {
        let Some(props) = f["properties"].as_object_mut() else {
            continue;
        };
        let link = match layer {
            "pistes" => props
                .get("osm_way_id")
                .and_then(Value::as_i64)
                .and_then(|id| links.for_way(id)),
            _ => props
                .get("id")
                .and_then(Value::as_str)
                .and_then(|id| links.for_domain(id)),
        };
        if let Some(l) = link {
            set_link(props, l);
            n += 1;
        }
    }
    n
}

/// Layers of the pack that get the plan links.
pub const LINKED_LAYERS: [&str; 3] = ["domaines", "stations", "pistes"];

/// Rewrites the linked layers of the pack in `root` with `plan_url` (see the module
/// documentation) and updates their entries (size, note).
pub fn apply(root: &Path, layers: &mut [Layer], links: &PlanLinks) -> Result<()> {
    if links.is_empty() {
        return Ok(());
    }
    for l in layers.iter_mut() {
        if !LINKED_LAYERS.contains(&l.id.as_str()) || l.format != "geojson" {
            continue;
        }
        let path = root.join(&l.file);
        let text = std::fs::read_to_string(&path).map_err(|e| Error::io(&path, e))?;
        let mut v: Value =
            serde_json::from_str(&text).map_err(|e| Error::json(path.display().to_string(), e))?;
        let n = annotate(&mut v, &l.id, links);
        let bytes = serde_json::to_vec(&v).map_err(|e| Error::json(&l.file, e))?;
        l.bytes = crate::build::write_bytes(&path, &bytes)?;
        l.notes.push(format!(
            "Lien vers le plan officiel de la station (`plan_url`, `plan_title`, `plan_publisher`) sur {n} entités : adresse du fichier publié par la station ou son massif (catalogue du collecteur de plans), jamais le plan lui-même."
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cand(id: &str, title: &str, uploaded: Option<&str>, domains: &[&str]) -> Value {
        json!({"candidate": {"id": id, "url": format!("https://example.org/{id}.pdf"), "title": title,
                             "publisher": "Station", "uploaded": uploaded, "domains": domains},
               "format": "pdf_text", "duplicate_of": null, "error": null, "created": null})
    }

    #[test]
    fn picks_the_most_recent_ski_plan_of_each_domain() {
        let mut dup = cand("c", "copie", Some("2026-01-01T00:00:00"), &["d2"]);
        dup["duplicate_of"] = json!("a");
        let mut err = cand("e", "Plan ski 2027", Some("2027-01-01T00:00:00"), &["d1"]);
        err["error"] = json!("HTTP 404");
        let mut old = cand("o", "Plan des pistes", None, &["d1"]);
        old["created"] = json!("Wed Nov 14 10:02:44 2018 CET");
        let cat = json!([
            cand(
                "a",
                "Plan des pistes 2023",
                Some("2023-11-02T10:00:00"),
                &["d1"]
            ),
            cand("b", "Plan raquettes", Some("2025-11-02T10:00:00"), &["d1"]),
            dup,
            err,
            old,
        ]);
        let ways = json!({"d1": [10, 11], "d3": [12]});
        let l = PlanLinks::from_values(&cat, Some(&ways));
        let d1 = l.for_domain("d1").unwrap();
        assert_eq!(d1.url, "https://example.org/a.pdf");
        assert_eq!(d1.date, "2023-11-02");
        // A duplicate names its domain for the original file.
        assert_eq!(l.for_domain("d2").unwrap().url, "https://example.org/a.pdf");
        assert!(l.for_domain("d3").is_none());
        assert_eq!(l.for_way(11).unwrap().url, d1.url);
        assert!(l.for_way(12).is_none());
        assert_eq!(
            pdfinfo_date("Wed Nov  4 10:02:44 2018 CET").unwrap(),
            "2018-11-04"
        );
    }

    #[test]
    fn annotates_domains_and_pistes() {
        let cat = json!([cand("a", "Plan", Some("2023-11-02"), &["d1"])]);
        let l = PlanLinks::from_values(&cat, Some(&json!({"d1": [10]})));
        let mut doms =
            json!({"features": [{"properties": {"id": "d1"}}, {"properties": {"id": "d2"}}]});
        assert_eq!(annotate(&mut doms, "domaines", &l), 1);
        assert_eq!(
            doms["features"][0]["properties"]["plan_url"],
            "https://example.org/a.pdf"
        );
        assert!(doms["features"][1]["properties"].get("plan_url").is_none());
        let mut pistes = json!({"features": [{"properties": {"osm_way_id": 10}}, {"properties": {"osm_way_id": 9}}]});
        assert_eq!(annotate(&mut pistes, "pistes", &l), 1);
        assert_eq!(
            pistes["features"][0]["properties"]["plan_publisher"],
            "Station"
        );
    }
}
