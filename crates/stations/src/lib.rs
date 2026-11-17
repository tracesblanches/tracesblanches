//! Nordic stations of France: practical cards (H1, #132), access titles and their
//! validity zones (H2, #133), refreshed from their sources by a scheduled task.
//!
//! Input: the national census of the domains (P3, `data/out/france/domaines/`).
//! Output, in `data/out/france/stations/` (WGS84):
//! - `stations.geojson`: one card per domain (rescue numbers, 112 first, and posts; pass
//!   sale points; station, tourist office and ski school contacts; car parks), from
//!   OpenStreetMap and OpenSkiMap (ODbL) and SkiRadar (attribution « skiradar.fr »);
//! - `pois.geojson`: the OSM points behind the cards, with their domains;
//! - `departs.json`, `departs.geojson`: the trail starts of every domain (X9, #143,
//!   [`departs`]), written by the CLI after the cards ran;
//! - with `personal_sources` only (the issuers display no reuse licence): `titres.json`
//!   (titles, validity, dated prices, members, source states), `titres.geojson`
//!   (footprint of each title), `couverture.json` (titles valid on each domain, ways of
//!   each title);
//! - `status.json` (state of every source and part), `journal.jsonl` (changes between
//!   runs), `refresh_state.json`, `stations.meta.json`.
//!
//! [`refresh()`] is what the server's CronJob calls; [`build`] forces every part.
//! Pipeline side: uses `skimap_common::download`.

pub mod catalogue;
pub mod contacts;
pub mod departs;
pub mod domains;
pub mod error;
pub mod fiche;
pub mod pois;
pub mod refresh;
pub mod sources;
pub mod titres;

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};
use serde_json::{Value, json};
use skimap_common::download::Downloader;
use skimap_common::layout::{DataLayout, FRANCE};
use skimap_common::meta::{FileMeta, LayerMeta, SourceRef};
use skimap_common::output::{feature, point, round7, write_geojson, write_json};

pub use error::{Result, StationsError};

/// Name of this crate, checked by the crate smoke test.
pub const CRATE: &str = env!("CARGO_PKG_NAME");
/// Layer name.
pub const LAYER: &str = "stations";
/// Census layer read.
pub const CENSUS_LAYER: &str = "domaines";
/// Files written only with `personal_sources`.
pub const PERSONAL_FILES: &[&str] = &["titres.json", "titres.geojson", "couverture.json"];

/// Result of [`build`].
#[derive(Debug, Clone)]
pub struct StationsOutput {
    /// Layer directory.
    pub dir: PathBuf,
    /// Key figures (also in the meta file).
    pub stats: Value,
}

/// Result of [`refresh()`].
#[derive(Debug, Clone)]
pub struct RefreshReport {
    /// Layer directory.
    pub dir: PathBuf,
    /// Parts run (`fiches`, `titres`).
    pub ran: Vec<String>,
    /// Parts not due.
    pub skipped: Vec<String>,
    /// Changes written to the journal.
    pub changes: usize,
    /// Sources that failed (id: status).
    pub broken_sources: Vec<String>,
    /// Titles kept from a previous run (`stale`) or withdrawn (`perime`).
    pub stale_titres: Vec<String>,
    /// Key figures.
    pub stats: Value,
}

fn count<'a>(it: impl Iterator<Item = &'a str>) -> BTreeMap<&'a str, usize> {
    let mut m = BTreeMap::new();
    for k in it {
        *m.entry(k).or_default() += 1;
    }
    m
}

fn multipolygon(ds: &[&domains::Domain]) -> Value {
    let polys: Vec<Value> = ds
        .iter()
        .flat_map(|d| d.polygons.iter())
        .map(|rings| {
            Value::Array(
                rings
                    .iter()
                    .map(|r| {
                        Value::Array(
                            r.iter()
                                .map(|p| json!([round7(p.lon), round7(p.lat)]))
                                .collect(),
                        )
                    })
                    .collect(),
            )
        })
        .collect();
    json!({"type": "MultiPolygon", "coordinates": polys})
}

fn read_json(path: &Path) -> Option<Value> {
    std::fs::read(path)
        .ok()
        .and_then(|b| serde_json::from_slice(&b).ok())
}

/// Records of a previous file keyed by id: GeoJSON features (`properties.id`) or an array
/// under `key`.
fn keyed(v: Option<&Value>, key: Option<&str>, id_field: &str) -> BTreeMap<String, Value> {
    let arr = match (v, key) {
        (Some(v), Some(k)) => v.get(k).and_then(Value::as_array).cloned(),
        (Some(v), None) => v.get("features").and_then(Value::as_array).cloned(),
        _ => None,
    };
    arr.unwrap_or_default()
        .into_iter()
        .filter_map(|x| {
            let rec = if key.is_none() {
                x.get("properties").cloned()?
            } else {
                x
            };
            let id = rec.get(id_field)?.as_str()?.to_owned();
            Some((id, rec))
        })
        .collect()
}

/// Builds every part now (`refresh` forced).
pub fn build(
    layout: &DataLayout,
    dl: &Downloader,
    personal_sources: bool,
) -> Result<StationsOutput> {
    let r = refresh(layout, dl, personal_sources, Utc::now(), true)?;
    Ok(StationsOutput {
        dir: r.dir,
        stats: r.stats,
    })
}

/// Output of the station cards part.
struct Fiches {
    cards: Vec<(domains::Domain, Value)>,
    pois: Vec<Value>,
    sources: Vec<SourceRef>,
    clusters: usize,
}

fn fiches_part(
    layout: &DataLayout,
    dl: &Downloader,
    ds: &[domains::Domain],
    census_dir: &Path,
) -> Result<Fiches> {
    let pf = pois::fetch(layout, dl, ds)?;
    let cf = contacts::fetch(layout, dl, ds, census_dir)?;
    // Date of the OSM data read (replication timestamp of the extracts).
    let checked = pf.valid_at.clone();
    let cards = ds
        .iter()
        .map(|d| {
            let c = fiche::fiche(d, &pf.pois, cf.by_domain.get(&d.id));
            let mut v = serde_json::to_value(c).unwrap_or(Value::Null);
            v["checked_at"] = json!(checked);
            (d.clone(), v)
        })
        .collect();
    let pois = pf
        .pois
        .iter()
        .map(|p| {
            let mut v = serde_json::to_value(p).unwrap_or(Value::Null);
            v["domains"] = Value::Array(
                p.domains
                    .iter()
                    .map(|(id, m)| json!({"id": id, "distance_m": m}))
                    .collect(),
            );
            feature(point(p.pos), v)
        })
        .collect();
    let mut sources: Vec<SourceRef> = pf.sources.clone();
    for s in &cf.osm_sources {
        if !sources.iter().any(|x| x.url == s.url) {
            sources.push(s.clone());
        }
    }
    for (name, lic, attr, f) in &cf.sources {
        sources.push(SourceRef::from_fetched(
            name.clone(),
            lic.clone(),
            attr.clone(),
            f,
        ));
    }
    Ok(Fiches {
        cards,
        pois,
        sources,
        clusters: pf.clusters,
    })
}

/// Refreshes the parts that are due at `now` (all with `force`): station cards weekly,
/// titles daily in season ([`mod@refresh`] module). Idempotent: a second call with nothing due
/// rewrites the status but no data. A failing source keeps the previous values, flagged.
pub fn refresh(
    layout: &DataLayout,
    dl: &Downloader,
    personal_sources: bool,
    now: DateTime<Utc>,
    force: bool,
) -> Result<RefreshReport> {
    let census_dir = layout.france_layer_dir(CENSUS_LAYER)?;
    let (ds, ways) = domains::load(&census_dir)?;
    let dir = layout.france_layer_dir(LAYER)?;
    let at = now.to_rfc3339_opts(chrono::SecondsFormat::Secs, true);
    let today = now.format("%Y-%m-%d").to_string();
    let state_path = dir.join("refresh_state.json");
    let mut state = refresh::State::read(&state_path);
    let prev_meta = LayerMeta::read(&dir.join(format!("{LAYER}.meta.json"))).ok();
    let mut changes: Vec<refresh::Change> = Vec::new();
    let mut ran = Vec::new();
    let mut skipped = Vec::new();
    let mut fiche_sources: Option<Vec<SourceRef>> = None;
    let mut clusters = None;

    // Station cards.
    let cards_path = dir.join("stations.geojson");
    if force || state.due("fiches", now) || !cards_path.exists() {
        ran.push("fiches".to_owned());
        let part = state.parts.entry("fiches".into()).or_default();
        part.last_run = Some(at.clone());
        match fiches_part(layout, dl, &ds, &census_dir) {
            Ok(f) => {
                part.last_ok = Some(at.clone());
                part.error = None;
                let old = keyed(read_json(&cards_path).as_ref(), None, "id");
                let mut new: BTreeMap<String, Value> = f
                    .cards
                    .iter()
                    .map(|(d, v)| (d.id.clone(), v.clone()))
                    .collect();
                changes.extend(refresh::diff(&old, &new, "stations", &at));
                refresh::carry_changed_at(&old, &mut new, &at);
                let feats: Vec<Value> = f
                    .cards
                    .iter()
                    .map(|(d, _)| feature(point(d.point), new[&d.id].clone()))
                    .collect();
                write_geojson(&cards_path, &feats)?;
                write_geojson(&dir.join("pois.geojson"), &f.pois)?;
                fiche_sources = Some(f.sources);
                clusters = Some(f.clusters);
            }
            Err(e) => {
                part.error = Some(e.to_string());
                changes.push(refresh::Change {
                    at: at.clone(),
                    part: "sources".into(),
                    id: "osm-local".into(),
                    change: "unreachable".into(),
                    detail: vec![e.to_string()],
                });
                if !cards_path.exists() {
                    return Err(e);
                }
                tracing::warn!(error = %e, "station cards not refreshed, previous ones kept");
            }
        }
    } else {
        skipped.push("fiches".to_owned());
    }
    let fiche_sources = fiche_sources.unwrap_or_else(|| {
        prev_meta
            .as_ref()
            .map(|m| {
                m.sources
                    .iter()
                    .filter(|s| !s.personal_use_only)
                    .cloned()
                    .collect()
            })
            .unwrap_or_default()
    });

    // Titles (personal use only).
    let mut titre_sources: Vec<SourceRef> = Vec::new();
    let mut stale = Vec::new();
    let mut personal_stats = Value::Null;
    if personal_sources {
        let due = force
            || state.due("titres", now)
            || PERSONAL_FILES.iter().any(|f| !dir.join(f).exists());
        if due {
            ran.push("titres".to_owned());
            let read = titres::read_titles(dl, &today);
            titre_sources = read
                .fetched
                .iter()
                .map(|(id, f)| {
                    let publisher = catalogue::DOCS
                        .iter()
                        .find(|d| d.id == id)
                        .map_or("Nordic Alpes du Sud", |d| d.publisher);
                    SourceRef::from_fetched(
                        format!("{publisher} : {id}"),
                        "aucune licence de réutilisation affichée : usage personnel",
                        publisher,
                        f,
                    )
                    .personal_use_only()
                })
                .collect();
            let (c, s) = titres_files(&dir, &ds, &ways, &read, &mut state, &at, &today, now)?;
            changes.extend(c);
            stale = s;
            let part = state.parts.entry("titres".into()).or_default();
            part.last_run = Some(at.clone());
            let all_ok = read.docs.iter().all(|d| d.status == "ok");
            part.last_ok = Some(at.clone());
            part.error = (!all_ok).then(|| "sources en échec : voir status.json".to_owned());
        } else {
            skipped.push("titres".to_owned());
            titre_sources = prev_meta
                .as_ref()
                .map(|m| {
                    m.sources
                        .iter()
                        .filter(|s| s.personal_use_only)
                        .cloned()
                        .collect()
                })
                .unwrap_or_default();
        }
        personal_stats = personal_figures(&dir, &ds);
    } else {
        for f in PERSONAL_FILES {
            let p = dir.join(f);
            if p.exists() {
                std::fs::remove_file(&p).map_err(|e| skimap_common::Error::io(&p, e))?;
            }
        }
    }

    // Journal, state, status.
    refresh::append_journal(&dir.join("journal.jsonl"), &changes)
        .map_err(|e| skimap_common::Error::io(dir.join("journal.jsonl"), e))?;
    write_json(&state_path, &state)?;
    let broken: Vec<String> = state
        .sources
        .iter()
        .filter(|(_, s)| s.status != "ok")
        .map(|(id, s)| format!("{id}: {}", s.status))
        .collect();
    write_json(
        &dir.join("status.json"),
        &json!({
            "checked_at": at,
            "parts": state.parts,
            "sources": state.sources,
            "stale_titres": stale,
            "policy": {
                "fiches_every_days": refresh::FICHES_EVERY_DAYS,
                "titres_every_days_in_season": refresh::TITRES_EVERY_DAYS.0,
                "titres_every_days_off_season": refresh::TITRES_EVERY_DAYS.1,
                "stale_max_days": refresh::STALE_MAX_DAYS,
            },
        }),
    )?;

    let mut stats = card_figures(&cards_path);
    if let Some(c) = clusters {
        stats["osm_queries"] = json!(c);
    }
    if !personal_stats.is_null() {
        stats["personal"] = personal_stats;
    }
    stats["refresh"] = json!({"ran": ran, "skipped": skipped, "changes": changes.len(), "broken_sources": broken, "stale_titres": stale});
    write_meta(
        &dir,
        &census_dir,
        fiche_sources,
        titre_sources,
        personal_sources,
        &stats,
        &at,
    )?;
    Ok(RefreshReport {
        dir,
        ran,
        skipped,
        changes: changes.len(),
        broken_sources: broken,
        stale_titres: stale,
        stats,
    })
}

/// Writes `titres.json`, `titres.geojson`, `couverture.json` from a read; keeps the
/// previous values of titles whose sources failed. Returns the changes and the stale or
/// withdrawn titles.
#[allow(clippy::too_many_arguments)]
fn titres_files(
    dir: &Path,
    ds: &[domains::Domain],
    ways: &domains::DomainWays,
    read: &titres::TitresRead,
    state: &mut refresh::State,
    at: &str,
    today: &str,
    now: DateTime<Utc>,
) -> Result<(Vec<refresh::Change>, Vec<String>)> {
    let mut changes = Vec::new();
    // Source states and their transitions.
    for d in &read.docs {
        let s = state.sources.entry(d.id.clone()).or_default();
        if s.status != d.status {
            changes.push(refresh::Change {
                at: at.to_owned(),
                part: "sources".into(),
                id: d.id.clone(),
                change: d.status.clone(),
                detail: d.error.iter().cloned().collect(),
            });
        }
        s.url = d.url.clone();
        s.status = d.status.clone();
        s.error = d.error.clone();
        if d.sha256.is_some() {
            s.sha256 = d.sha256.clone();
        }
        if d.status == "ok" {
            s.last_ok = Some(at.to_owned());
        }
    }
    let doc_ok = |id: &str| read.docs.iter().any(|d| d.id == id && d.status == "ok");
    let fetched_at = |id: &str| {
        read.docs
            .iter()
            .find(|d| d.id == id)
            .and_then(|d| d.fetched_at.clone())
    };
    let prev_path = dir.join("titres.json");
    let prev = read_json(&prev_path);
    let old_titres = keyed(prev.as_ref(), Some("titres"), "id");
    let old_pages = keyed(prev.as_ref(), Some("site_pages"), "url");

    let by_id: BTreeMap<&str, &domains::Domain> = ds.iter().map(|d| (d.id.as_str(), d)).collect();
    let mut new_titres: BTreeMap<String, Value> = BTreeMap::new();
    for t in &read.titres {
        let cov = titres::coverage(t, ds, today);
        let w: BTreeSet<i64> = cov
            .domains
            .iter()
            .flat_map(|d| ways.get(d).into_iter().flatten().copied())
            .collect();
        let prices: Vec<Value> = t
            .prices
            .iter()
            .map(|p| {
                let mut v = serde_json::to_value(p).unwrap_or(Value::Null);
                v["source"] = json!(
                    catalogue::DOCS
                        .iter()
                        .find(|d| d.id == p.doc)
                        .map(|d| d.url)
                );
                v["checked_at"] = json!(fetched_at(&p.doc));
                v
            })
            .collect();
        new_titres.insert(
            t.id.clone(),
            json!({
                "id": t.id, "name": t.name, "scope": t.scope, "issuer": t.issuer, "products": t.products,
                "validity": t.validity,
                "validity_source": catalogue::DOCS.iter().find(|d| d.id == t.validity_doc).map(|d| d.url),
                "notes": t.notes, "abroad": t.abroad, "prices": prices,
                "members": cov.members,
                "members_without_domain": cov.members.iter().filter(|m| m.domains.is_empty()).map(|m| m.label.clone()).collect::<Vec<_>>(),
                "domains": cov.domains, "ways": w.len(),
                "status": "ok", "checked_at": at, "personal_use_only": true,
            }),
        );
    }
    // Titles not read now: kept if their sources failed (stale, then withdrawn).
    let mut stale = Vec::new();
    let missing: BTreeSet<&str> = read.missing.iter().map(|(t, _)| t.as_str()).collect();
    for (id, old) in &old_titres {
        if new_titres.contains_key(id) {
            continue;
        }
        let src = old
            .get("validity_source")
            .and_then(Value::as_str)
            .and_then(|u| catalogue::DOCS.iter().find(|d| d.url == u))
            .map(|d| d.id);
        let failed = missing.contains(id.as_str()) || src.is_some_and(|s| !doc_ok(s));
        if !failed {
            continue; // removed by its source: dropped (journal: « removed »).
        }
        let mut v = old.clone();
        let checked = v
            .get("checked_at")
            .and_then(Value::as_str)
            .unwrap_or(at)
            .to_owned();
        let age = refresh::age_days(&checked, now).unwrap_or(0);
        if age > refresh::STALE_MAX_DAYS {
            v["status"] = json!("perime");
            v["prices"] = json!([]);
            v["withdrawn"] = json!(format!(
                "source en échec depuis le {checked} : prix retirés"
            ));
        } else {
            v["status"] = json!("stale");
        }
        v["age_days"] = json!(age);
        stale.push(format!(
            "{id}: {}",
            v["status"].as_str().unwrap_or_default()
        ));
        new_titres.insert(id.clone(), v);
    }
    changes.extend(refresh::diff(&old_titres, &new_titres, "titres", at));
    refresh::carry_changed_at(&old_titres, &mut new_titres, at);

    // Domain pages (day prices).
    let mut new_pages: BTreeMap<String, Value> = BTreeMap::new();
    for p in &read.site_pages {
        let m = titres::match_site_page(p, ds, today);
        let mut v = serde_json::to_value(p).unwrap_or(Value::Null);
        v["domains"] = json!(m.domains);
        v["match"] = json!(m.confidence);
        v["status"] = json!("ok");
        v["checked_at"] = json!(fetched_at(&p.url));
        new_pages.insert(p.url.clone(), v);
    }
    for (url, old) in &old_pages {
        if !new_pages.contains_key(url) && !doc_ok(url) && !doc_ok("nads-tarifs") {
            let mut v = old.clone();
            v["status"] = json!("stale");
            new_pages.insert(url.clone(), v);
        }
    }
    changes.extend(refresh::diff(&old_pages, &new_pages, "site_pages", at));
    refresh::carry_changed_at(&old_pages, &mut new_pages, at);

    // Coverage by domain.
    let mut domain_titres: BTreeMap<String, Vec<Value>> = BTreeMap::new();
    let mut feats = Vec::new();
    let mut ways_by_titre: BTreeMap<String, Vec<i64>> = BTreeMap::new();
    for (id, t) in &new_titres {
        let ids: Vec<String> = t["domains"]
            .as_array()
            .map(|a| {
                a.iter()
                    .filter_map(|x| x.as_str().map(str::to_owned))
                    .collect()
            })
            .unwrap_or_default();
        let dd: Vec<&domains::Domain> = ids
            .iter()
            .filter_map(|i| by_id.get(i.as_str()).copied())
            .collect();
        let mut w: BTreeSet<i64> = BTreeSet::new();
        for d in &dd {
            domain_titres
                .entry(d.id.clone())
                .or_default()
                .push(json!({"id": id, "status": t["status"]}));
            w.extend(ways.get(&d.id).into_iter().flatten().copied());
        }
        ways_by_titre.insert(id.clone(), w.iter().copied().collect());
        let full = t["prices"].as_array().and_then(|ps| {
            ps.iter()
                .find(|p| {
                    p["category"] == "adulte"
                        && (p["tier"] == "plein_tarif" || p["tier"] == "tarif_unique")
                })
                .and_then(|p| p["eur"].as_f64())
        });
        feats.push(feature(
            multipolygon(&dd),
            json!({"id": id, "name": t["name"], "scope": t["scope"], "issuer": t["issuer"],
                "domains": dd.len(), "ways": w.len(), "adult_full_price_eur": full,
                "season": t["prices"].get(0).and_then(|p| p.get("season")), "status": t["status"],
                "personal_use_only": true}),
        ));
    }
    let rules: Vec<Value> = read
        .site_rules
        .iter()
        .map(|r| json!({"departements": r.depts, "rule": r.rule, "source": catalogue::DOCS.iter().find(|d| d.id == r.doc).map(|d| d.url)}))
        .collect();
    let couverture: BTreeMap<&str, Value> = ds
        .iter()
        .map(|d| {
            let rule = read.site_rules.iter().find(|r| r.depts.contains(&d.departement));
            let pages: Vec<&Value> = new_pages
                .values()
                .filter(|p| p["domains"].as_array().is_some_and(|a| a.iter().any(|x| x == d.id.as_str())))
                .collect();
            (d.id.as_str(), json!({
                "name": d.name, "departement": d.departement,
                "titres": domain_titres.get(&d.id).cloned().unwrap_or_default(),
                "site_title": {
                    "rule": rule.map(|r| r.rule.clone()),
                    "rule_source": rule.and_then(|r| catalogue::DOCS.iter().find(|x| x.id == r.doc)).map(|x| x.url),
                    "pages": pages.iter().map(|p| json!({"url": p["url"], "season": p["season"], "prices": p["prices"], "phones": p["phones"], "status": p["status"]})).collect::<Vec<_>>(),
                },
            }))
        })
        .collect();
    write_json(
        &prev_path,
        &json!({
            "checked_at": at,
            "personal_use_only": true,
            "reuse": "aucun émetteur n'affiche de licence de réutilisation ; tarifs : faits, extraction limitée ; listes d'adhérents : base de données possiblement protégée (CPI L341-1, L342-1) ; usage personnel en attendant l'accord des émetteurs (courriers L1, L4)",
            "documents": read.docs,
            "titres": new_titres.values().collect::<Vec<_>>(),
            "site_rules": rules,
            "site_pages": new_pages.values().collect::<Vec<_>>(),
            "complements": {
                "pdf_prices": catalogue::PDF_PRICES.iter().map(|p| json!({"titre": p.titre, "doc": p.doc, "sha256": p.sha256, "checked_on": p.checked_on, "expires": p.expires})).collect::<Vec<_>>(),
                "member_overrides": catalogue::OVERRIDES.iter().map(|o| json!({"label_contains": o.label_contains, "domains": o.domains, "confidence": o.confidence, "reason": o.reason, "checked_on": o.checked_on, "expires": o.expires})).collect::<Vec<_>>(),
            },
        }),
    )?;
    write_geojson(&dir.join("titres.geojson"), &feats)?;
    write_json(
        &dir.join("couverture.json"),
        &json!({
            "checked_at": at,
            "personal_use_only": true,
            "domains": couverture,
            "ways": ways_by_titre,
        }),
    )?;
    Ok((changes, stale))
}

/// Figures of the cards file.
fn card_figures(cards_path: &Path) -> Value {
    let v = read_json(cards_path).unwrap_or(Value::Null);
    let cards: Vec<&Value> = v["features"]
        .as_array()
        .map(|a| a.iter().map(|f| &f["properties"]).collect())
        .unwrap_or_default();
    let n = cards.len();
    let has = |c: &Value, k: &str| {
        c["has"]
            .as_array()
            .is_some_and(|a| a.iter().any(|x| x == k))
    };
    let with = |k: &str| cards.iter().filter(|c| has(c, k)).count();
    let pct = |x: usize| (1000.0 * x as f64 / n.max(1) as f64).round() / 10.0;
    let kinds = |field: &str| {
        count(
            cards
                .iter()
                .flat_map(|c| c[field].as_array().into_iter().flatten())
                .filter_map(|p| p["kind"].as_str()),
        )
    };
    json!({
        "domains": n,
        "with_secours_post": with("secours"), "with_secours_post_pct": pct(with("secours")),
        "with_vente": with("vente"), "with_vente_pct": pct(with("vente")),
        "with_phone": with("telephone"), "with_phone_pct": pct(with("telephone")),
        "with_site": with("site"), "with_site_pct": pct(with("site")),
        "with_parking": with("parking"), "with_parking_pct": pct(with("parking")),
        "with_secours_and_vente": cards.iter().filter(|c| has(c, "secours") && has(c, "vente")).count(),
        "secours_by_kind": count(cards.iter().flat_map(|c| c["secours"]["posts"].as_array().into_iter().flatten()).filter_map(|p| p["kind"].as_str())),
        "vente_by_kind": kinds("vente"),
        "links_by_source": count(cards.iter().flat_map(|c| c["links"].as_array().into_iter().flatten()).filter_map(|l| l["source"].as_str())),
    })
}

/// Figures of the personal files: dated prices and their share.
fn personal_figures(dir: &Path, ds: &[domains::Domain]) -> Value {
    let t = read_json(&dir.join("titres.json")).unwrap_or(Value::Null);
    let cards = read_json(&dir.join("stations.geojson")).unwrap_or(Value::Null);
    let mut priced: BTreeSet<String> = BTreeSet::new();
    let mut by_titre = BTreeMap::new();
    for x in t["titres"].as_array().into_iter().flatten() {
        let doms: Vec<String> = x["domains"]
            .as_array()
            .map(|a| {
                a.iter()
                    .filter_map(|d| d.as_str().map(str::to_owned))
                    .collect()
            })
            .unwrap_or_default();
        by_titre.insert(x["id"].as_str().unwrap_or_default().to_owned(), json!({"domains": doms.len(), "prices": x["prices"].as_array().map_or(0, Vec::len), "status": x["status"]}));
        if x["prices"].as_array().is_some_and(|p| !p.is_empty()) {
            priced.extend(doms);
        }
    }
    let mut day: BTreeSet<String> = BTreeSet::new();
    for p in t["site_pages"].as_array().into_iter().flatten() {
        if p["prices"].as_array().is_some_and(|x| !x.is_empty()) {
            day.extend(
                p["domains"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(|d| d.as_str().map(str::to_owned)),
            );
        }
    }
    priced.extend(day.iter().cloned());
    let has = |c: &Value, k: &str| {
        c["has"]
            .as_array()
            .is_some_and(|a| a.iter().any(|x| x == k))
    };
    let all3 = cards["features"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|f| &f["properties"])
        .filter(|c| {
            has(c, "secours")
                && has(c, "vente")
                && c["id"].as_str().is_some_and(|i| priced.contains(i))
        })
        .count();
    let n = ds.len().max(1) as f64;
    json!({
        "with_dated_price": priced.len(), "with_dated_price_pct": (1000.0 * priced.len() as f64 / n).round() / 10.0,
        "with_day_price": day.len(),
        "with_secours_vente_and_price": all3, "with_secours_vente_and_price_pct": (1000.0 * all3 as f64 / n).round() / 10.0,
        "titres": by_titre,
        "site_pages": t["site_pages"].as_array().map_or(0, Vec::len),
    })
}

#[allow(clippy::too_many_arguments)]
fn write_meta(
    dir: &Path,
    census_dir: &Path,
    fiche_sources: Vec<SourceRef>,
    titre_sources: Vec<SourceRef>,
    personal_sources: bool,
    stats: &Value,
    at: &str,
) -> Result<()> {
    let mut sources = fiche_sources;
    sources.push(SourceRef::new(
        "Recensement des domaines (P3)",
        census_dir.join("domaines.geojson").display().to_string(),
        "ODbL 1.0 (OSM, OpenSkiMap) ; communes : Licence Ouverte 2.0",
        "© les contributeurs d'OpenStreetMap ; OpenSkiMap",
    ));
    sources.extend(titre_sources);
    let mut files = vec![
        FileMeta::new("stations.geojson", "fiche pratique de chaque domaine du recensement (point représentatif) : secours (112 en premier), points de vente de la redevance, contacts, parkings ; information seulement, pas d'achat")
            .licence("ODbL 1.0 (OSM, OpenSkiMap) ; SkiRadar : attribution « skiradar.fr » et lien vers la fiche")
            .properties_json(&json!({
                "id": "identifiant du domaine (recensement P3)", "name": "nom", "departement": "département", "commune": "commune du point représentatif", "km_nordic": "km nordiques",
                "secours": "numbers : numéros à composer, 112 toujours en premier, puis 15 et 114 ; posts : postes et équipements OSM (patrol_post, first_aid, mountain_rescue, ambulance_station, fire_station, defibrillator, emergency_phone, access_point), postes tenus d'abord puis par distance",
                "vente": "points de vente de la redevance OSM (ticket : shop=ticket, foyer : foyer, caisse ou billetterie nordique, vending : distributeur) à moins de 1 km",
                "links": "liens : url, role (station, ecole_ski, skiradar), label, source (openstreetmap, openskimap, skiradar)",
                "phones": "téléphones de la station (relation OSM site=piste) : e164, display, label, source",
                "operator": "exploitant (relation OSM)",
                "offices_tourisme": "offices de tourisme OSM, les 2 plus proches à moins de 6 km", "ecoles_ski": "écoles de ski OSM, les 2 plus proches à moins de 6 km",
                "parkings": "parkings OSM à moins de 300 m (sauf privés), du plus proche au plus lointain",
                "has": "ce que la fiche contient : secours, vente, telephone, site, parking",
                "place": "kind, name, phones, website, opening_hours, distance_m (à l'emprise, 0 dedans), lon_lat, url (OSM)",
                "checked_at": "date de lecture des données OSM de la fiche", "changed_at": "dernière fois que la fiche a changé (journal.jsonl)",
            })),
        FileMeta::new("pois.geojson", "points OSM des fiches (secours, vente, office de tourisme, école de ski, parking) et domaines auxquels ils sont rattachés")
            .licence(pois::LICENCE)
            .properties_json(&json!({
                "category": "secours, vente, office_tourisme, ecole_ski, parking", "kind": "sous-type (voir stations.geojson)",
                "name": "nom", "phones": "téléphones (phone, contact:phone, emergency:phone)", "website": "site", "opening_hours": "horaires",
                "operator": "exploitant", "capacity": "parking : places", "fee": "parking : payant", "access": "parking : accès",
                "url": "page OSM", "osm_type": "node, way, relation", "osm_id": "identifiant OSM",
                "domains": "domaines : id, distance_m à l'emprise",
            })),
        FileMeta::new(departs::FILE, "départs des pistes de chaque domaine (X9), du principal au moins fréquenté : foyers, accueils, écoles de ski et parkings OSM près du réseau ramenés sur le réseau ; repli sur le point du réseau le plus proche d'un parking, du village ou du recensement")
            .licence(pois::LICENCE)
            .properties_json(&json!({
                "domains": "identifiant du domaine (recensement P3) → départs : id, name, phrase, phrase_en, lon, lat, provenance, method (points, near, village, centre), score, points (pages OSM)",
                "osm_valid_at": "date des données OSM des pistes", "notes": "méthode",
            })),
        FileMeta::new(departs::GEOJSON, "un point par départ (contrôle sur carte) : domain, rank, name, method, score, points")
            .licence(pois::LICENCE),
        FileMeta::new("status.json", "état de chaque partie et de chaque source : dernière lecture réussie, erreur, titres conservés (stale) ou retirés (perime), politique de rafraîchissement"),
        FileMeta::new("journal.jsonl", "journal des changements entre deux rafraîchissements : at, part (stations, titres, site_pages, sources), id, change (added, removed, changed, état d'une source), detail (champs modifiés ou erreur)"),
    ];
    if personal_sources {
        files.push(
            FileMeta::new("titres.json", "titres d'accès (redevances, Nordic Pass) lus sur les pages des émetteurs à chaque rafraîchissement : validité citée, prix datés, membres et domaines, pages de domaine (prix journée, contacts), état des sources, compléments manuels datés")
                .licence("usage personnel (aucune licence de réutilisation affichée par les émetteurs)")
                .properties_json(&json!({
                    "documents": "sources : id, url, status (ok, unreachable, broken, changed), error, sha256, fetched_at",
                    "titres": "id, name, scope (national, massif, departemental, regroupement, site), issuer, products, validity (citation), validity_source, notes, abroad, prices, members (label, confidence auto / auto_national / verifie / partiel / probable / aucun, domains, complement), members_without_domain, domains, ways, status (ok, stale, perime), checked_at, changed_at, personal_use_only",
                    "prices": "product, category (adulte, jeune, senior), ages, eur, tier (prevente, promo_flash, promo, plein_tarif, tarif_unique), from, until (jours inclus), season, doc, source, quote (ligne de la source), checked_at",
                    "site_pages": "pages de domaine Nordic Alpes du Sud : name, url, season, prices (product « journee : … » ou « saison_site : … »), phones (gestionnaire, office de tourisme), domains, match, status, checked_at, changed_at",
                    "site_rules": "règles des titres de site (séance, journée) citées, par département",
                    "complements": "compléments manuels (prix d'une grille PDF épinglée par son SHA-256, rattachements vérifiés à la main) avec date de vérification et d'expiration",
                })),
        );
        files.push(
            FileMeta::new("titres.geojson", "emprise de chaque titre : réunion des emprises des domaines couverts (MultiPolygon)")
                .licence("usage personnel ; géométrie : ODbL 1.0 (recensement)")
                .properties_json(&json!({"id": "titre", "name": "nom", "scope": "portée", "issuer": "émetteur", "domains": "domaines couverts", "ways": "ways OSM couverts", "adult_full_price_eur": "plein tarif adulte", "season": "saison", "status": "ok, stale, perime", "personal_use_only": "true"})),
        );
        files.push(
            FileMeta::new("couverture.json", "titres valables sur chaque domaine et ways OSM de chaque titre, pour colorer « valable avec ton titre » / « autre redevance »")
                .licence("usage personnel")
                .properties_json(&json!({
                    "domains": "par domaine : name, departement, titres (id, status), site_title (rule citée, rule_source, pages : prix journée et contacts des pages de domaine)",
                    "ways": "par titre : identifiants des ways OSM couverts (domaines_ways.json du recensement)",
                })),
        );
    }
    let meta = LayerMeta {
        tickets: vec!["H1 (#132)".to_owned(), "H2 (#133)".to_owned()],
        valid_from: Some(at.to_owned()),
        validity: Some(format!(
            "fiches relues tous les {} jours ; titres tous les jours en saison (1er novembre - 30 avril), tous les {} jours sinon ; un titre dont la source échoue reste {} jours (stale), puis ses prix sont retirés",
            refresh::FICHES_EVERY_DAYS,
            refresh::TITRES_EVERY_DAYS.1,
            refresh::STALE_MAX_DAYS
        )),
        crs: Some("EPSG:4326 (WGS84, longitude, latitude)".to_owned()),
        sources,
        files,
        notes: vec![
            "Le 112 est en tête de toute fiche de secours (puis 15 et 114).".to_owned(),
            "Information seulement : l'app n'achète rien ; un prix n'est publié qu'avec sa saison, sa source et sa date de lecture.".to_owned(),
            "Points de vente : seuls ceux que OSM décrit (shop=ticket, foyer, caisse ou billetterie nordique) ; les offices de tourisme et commerces qui vendent les pass saison ne sont pas tous connus.".to_owned(),
            "Titres, tarifs et zones de validité : lus sur les pages des émetteurs à chaque rafraîchissement, seulement avec personal_sources (sources sans licence de réutilisation).".to_owned(),
        ],
        parameters: json!({
            "reach_m": {"secours (poste, premiers secours)": 1000, "secours de montagne, ambulance, centre de secours": 3000, "défibrillateur": 500, "téléphone d'urgence": 200, "point de rencontre des secours": 100, "vente": 1000, "office de tourisme, école de ski (2 plus proches)": 6000, "parking": 300},
            "osm_source": "extraits Geofabrik locaux (skimap_osm::local)",
            "personal_sources": personal_sources,
        }),
        stats: stats.clone(),
        ..LayerMeta::new(LAYER, FRANCE)
    };
    write_json(&dir.join(format!("{LAYER}.meta.json")), &meta)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    #[test]
    fn crate_name() {
        assert!(super::CRATE.starts_with("skimap-"));
    }
}
