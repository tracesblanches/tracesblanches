//! Trail status snapshots (P13, #26), polled once for all users.
//!
//! - Chapelle-des-Bois: ENJ Snowmap Geotrek API ([`enj`]), field `route` (1 closed,
//!   2 open), status notes in `labels`, `update_datetime`.
//! - Cervières: Nordic France white-label widget, station 3167 ([`nordicfrance`]), and the
//!   « Info damage » page of the town hall ([`mairie`]).
//!
//! None of these sources displays a licence (ENJ: ticket L1 open; Nordic France and the town
//! hall: HTML only). They are therefore **personal use only** and [`poll`] refuses to run
//! unless `personal_sources` is set.
//!
//! Every poll is stored with its time (see [`store`]) so that a week of history in season
//! gives the real update frequency of each source.
//!
//! Output (`data/out/<site>/status/`):
//! - `status.json`: latest normalised content of every source of the site;
//! - `status.geojson` (ENJ): itineraries with geometry (WGS84) and state;
//! - `<source>/polls.jsonl`, `<source>/snapshots/*.json`: history;
//! - `<source>/history.json`: observed frequency ([`store::Frequency`]), and for ENJ the
//!   state transitions and the times of day of the status notes;
//! - `status.meta.json`.
//!
//! Scheduling: the pipeline (cron or systemd timer) calls [`poll`]; ENJ every 15 minutes is
//! the downloader cache lifetime of the source, Nordic France and the town hall are served
//! from the cache for 3 hours.

pub mod enj;
pub mod error;
pub mod html;
pub mod mairie;
pub mod nordicfrance;
pub mod store;

use chrono::{DateTime, Utc};
use serde::Serialize;
use serde_json::{Value, json};
use skimap_common::download::{Downloader, Origin, Request};
use skimap_common::layout::DataLayout;
use skimap_common::site::Site;
use skimap_meteo::meta::{LayerMeta, SourceRef, write_json};

pub use error::{Error, Result};
use store::Store;

/// Layer name.
pub const LAYER: &str = "status";

/// Name of this crate, checked by the crate smoke test.
pub const CRATE: &str = env!("CARGO_PKG_NAME");

/// Outcome of the poll of one source.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SourcePoll {
    /// Source name.
    pub source: String,
    /// Public URL.
    pub url: String,
    /// `downloaded`, `not_modified` or `cache` (cache: recorded only in an empty store).
    pub origin: String,
    /// Whether the content changed since the previous snapshot.
    pub changed: bool,
    /// Snapshot holding the content.
    pub snapshot: Option<String>,
    /// One-line summary.
    pub summary: String,
}

/// Outcome of a poll of a site.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct PollReport {
    /// Site identifier.
    pub site: String,
    /// Poll time, RFC 3339 UTC.
    pub polled_at: String,
    /// Sources.
    pub sources: Vec<SourcePoll>,
}

fn origin_name(o: Origin) -> &'static str {
    match o {
        Origin::Downloaded => "downloaded",
        Origin::NotModified => "not_modified",
        Origin::Cache => "cache",
    }
}

fn record(
    store: &Store,
    now: DateTime<Utc>,
    origin: Origin,
    content: &Value,
) -> Result<(bool, Option<String>)> {
    // A body served from the cache brings nothing new, unless the store is empty.
    if origin == Origin::Cache
        && let Some(last) = store.polls()?.pop()
    {
        return Ok((false, Some(last.snapshot)));
    }
    let r = store.record(now, origin_name(origin), content)?;
    Ok((r.changed, Some(r.snapshot)))
}

fn rfc3339(t: DateTime<Utc>) -> String {
    t.to_rfc3339_opts(chrono::SecondsFormat::Secs, true)
}

fn source_ref(name: &str, url: &str, licence: &str, attribution: &str) -> SourceRef {
    SourceRef {
        name: name.to_owned(),
        url: url.to_owned(),
        licence: licence.to_owned(),
        attribution: attribution.to_owned(),
        personal_use_only: true,
    }
}

fn poll_enj(
    dir: &std::path::Path,
    dl: &Downloader,
    city: u32,
    now: DateTime<Utc>,
    latest: &mut serde_json::Map<String, Value>,
) -> Result<SourcePoll> {
    let (snap, origin) = enj::fetch_status(dl, city)?;
    let content =
        serde_json::to_value(&snap).map_err(|e| skimap_common::Error::json("ENJ snapshot", e))?;
    let store = Store::new(dir.join(enj::SOURCE));
    let (changed, snapshot) = record(&store, now, origin, &content)?;

    let geoms = enj::fetch_geometry(dl, city)?;
    let features: Vec<Value> = snap
        .treks
        .iter()
        .map(|t| {
            let (geometry, gpx) = geoms.get(&t.id).cloned().unwrap_or((Value::Null, None));
            json!({
                "type": "Feature",
                "geometry": strip_z(geometry),
                "properties": {
                    "id": t.id,
                    "name": t.name,
                    "practice": t.practice,
                    "difficulty": t.difficulty,
                    "state": t.state,
                    "route": t.route,
                    "notes": t.notes,
                    "update_datetime": t.update_datetime,
                    "length_m": t.length_m,
                    "gtj": t.gtj,
                    "gpx": gpx,
                    "polled_at": rfc3339(now),
                }
            })
        })
        .collect();
    write_json(
        &dir.join("status.geojson"),
        &json!({ "type": "FeatureCollection", "features": features }),
    )?;

    let freq = store::frequency(&store, enj::stamps)?;
    let snaps: Vec<(String, Value)> = store
        .snapshots()?
        .into_iter()
        .map(|(t, _, v)| (t, v))
        .collect();
    write_json(
        &store.dir().join("history.json"),
        &json!({
            "frequency": freq,
            "transitions": enj::transitions(&snaps),
            "note_times_of_day": enj::note_times(&snap),
        }),
    )?;
    let counts = snap.counts();
    let summary = format!(
        "{} itinéraires : {} ouverts, {} fermés",
        snap.treks.len(),
        counts.get(&enj::TrailState::Open).copied().unwrap_or(0),
        counts.get(&enj::TrailState::Closed).copied().unwrap_or(0)
    );
    latest.insert(enj::SOURCE.to_owned(), content);
    Ok(SourcePoll {
        source: enj::SOURCE.to_owned(),
        url: enj::status_url(city),
        origin: origin_name(origin).to_owned(),
        changed,
        snapshot,
        summary,
    })
}

/// Drops the third coordinate (altitude) of a GeoJSON LineString / MultiLineString.
fn strip_z(g: Value) -> Value {
    fn pt(p: &Value) -> Value {
        match p.as_array() {
            Some(a) if a.len() > 2 => Value::Array(a[..2].to_vec()),
            _ => p.clone(),
        }
    }
    let mut g = g;
    let kind = g["type"].as_str().unwrap_or("").to_owned();
    if let Some(c) = g.get_mut("coordinates") {
        *c = match kind.as_str() {
            "LineString" => Value::Array(c.as_array().into_iter().flatten().map(pt).collect()),
            "MultiLineString" => Value::Array(
                c.as_array()
                    .into_iter()
                    .flatten()
                    .map(|l| Value::Array(l.as_array().into_iter().flatten().map(pt).collect()))
                    .collect(),
            ),
            _ => c.clone(),
        };
    }
    g
}

/// An HTML page source polled at low frequency.
struct PageSource<'a> {
    source: &'a str,
    url: &'a str,
    min_interval: std::time::Duration,
    stamps: fn(&Value) -> Vec<String>,
}

fn poll_page<T: Serialize>(
    dir: &std::path::Path,
    dl: &Downloader,
    page: &PageSource<'_>,
    now: DateTime<Utc>,
    parse: impl Fn(&str) -> T,
    latest: &mut serde_json::Map<String, Value>,
) -> Result<(SourcePoll, Value)> {
    let PageSource {
        source,
        url,
        min_interval,
        stamps,
    } = *page;
    let f = dl.fetch(&Request::get(source, url).max_age(min_interval))?;
    let content = serde_json::to_value(parse(&f.text()?))
        .map_err(|e| skimap_common::Error::json(source.to_owned(), e))?;
    let store = Store::new(dir.join(source));
    let (changed, snapshot) = record(&store, now, f.origin, &content)?;
    let freq = store::frequency(&store, stamps)?;
    write_json(
        &store.dir().join("history.json"),
        &json!({ "frequency": freq }),
    )?;
    latest.insert(source.to_owned(), content.clone());
    Ok((
        SourcePoll {
            source: source.to_owned(),
            url: url.to_owned(),
            origin: origin_name(f.origin).to_owned(),
            changed,
            snapshot,
            summary: String::new(),
        },
        content,
    ))
}

/// Polls the trail status sources of a site and stores a timestamped snapshot.
///
/// Refuses to run unless `personal_sources` is set (no source displays a licence).
pub fn poll(
    site: &Site,
    layout: &DataLayout,
    dl: &Downloader,
    personal_sources: bool,
) -> Result<PollReport> {
    if !personal_sources {
        return Err(Error::PersonalSourcesDisabled(site.id.to_owned()));
    }
    let dir = layout.layer_dir(site, LAYER)?;
    DataLayout::ensure(&dir)?;
    let now = Utc::now();
    let mut latest = serde_json::Map::new();
    let mut sources = Vec::new();
    let mut refs = Vec::new();
    let mut files = vec![
        "status.json: dernier contenu normalisé de chaque source".to_owned(),
        "<source>/polls.jsonl, <source>/snapshots/: historique horodaté".to_owned(),
        "<source>/history.json: fréquence de mise à jour observée".to_owned(),
    ];

    if let Some(city) = enj::city_id(site.id) {
        sources.push(poll_enj(&dir, dl, city, now, &mut latest)?);
        refs.push(source_ref(
            "ENJ Snowmap (Geotrek), état des itinéraires",
            &enj::status_url(city),
            "aucune licence affichée (ticket L1) : usage personnel",
            "Espace Nordique Jurassien",
        ));
        files.push("status.geojson: itinéraires ENJ (WGS84) et leur état".to_owned());
    }
    if let Some(station) = nordicfrance::station_id(site.id) {
        let url = nordicfrance::station_url(station);
        let page = PageSource {
            source: nordicfrance::SOURCE,
            url: &url,
            min_interval: nordicfrance::MIN_INTERVAL,
            stamps: nordicfrance::stamps,
        };
        let (mut p, v) = poll_page(&dir, dl, &page, now, nordicfrance::parse, &mut latest)?;
        let pistes = v["sectors"]
            .as_array()
            .map(|s| {
                s.iter()
                    .map(|x| x["pistes"].as_array().map_or(0, Vec::len))
                    .sum::<usize>()
            })
            .unwrap_or(0);
        p.summary = format!(
            "{} secteurs, {} pistes, ouvert {}, mise à jour {}",
            v["sectors"].as_array().map_or(0, Vec::len),
            pistes,
            v["open_summary"].as_str().unwrap_or("?"),
            v["updated"].as_str().unwrap_or("?")
        );
        sources.push(p);
        refs.push(source_ref(
            "Nordic France, bulletin en marque blanche",
            &url,
            "aucune licence, HTML seulement : usage personnel, faible fréquence",
            "Nordic France",
        ));
        let page = PageSource {
            source: mairie::SOURCE,
            url: mairie::URL,
            min_interval: mairie::MIN_INTERVAL,
            stamps: mairie::stamps,
        };
        let (mut p, v) = poll_page(&dir, dl, &page, now, mairie::parse, &mut latest)?;
        p.summary = format!(
            "{} secteurs, page modifiée {}, « {} »",
            v["sectors"].as_array().map_or(0, Vec::len),
            v["modified_time"].as_str().unwrap_or("?"),
            v["message"].as_str().unwrap_or("")
        );
        sources.push(p);
        refs.push(source_ref(
            "Mairie de Cervières, page Info damage",
            mairie::URL,
            "aucune licence affichée : usage personnel, faible fréquence",
            "Commune de Cervières",
        ));
    }
    if sources.is_empty() {
        return Err(Error::NoSource(site.id.to_owned()));
    }
    write_json(
        &dir.join("status.json"),
        &json!({ "site": site.id, "polled_at": rfc3339(now), "sources": latest }),
    )?;
    LayerMeta {
        layer: LAYER.to_owned(),
        site: site.id.to_owned(),
        generated_at: rfc3339(now),
        valid_from: Some(rfc3339(now)),
        valid_until: None,
        sources: refs,
        files,
        notes: vec![
            "Sources sans licence affichée : activées seulement avec personal_sources.".into(),
            "L'état publié n'est pas un état en temps réel ; afficher l'heure de la source.".into(),
        ],
    }
    .write(&dir)?;
    Ok(PollReport {
        site: site.id.to_owned(),
        polled_at: rfc3339(now),
        sources,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crate_name() {
        assert!(super::CRATE.starts_with("skimap-"));
    }

    #[test]
    fn personal_sources_required() {
        let dir = tempfile::tempdir().unwrap();
        let layout = DataLayout::new(dir.path());
        let dl = Downloader::builder(layout.cache_root())
            .offline(true)
            .build()
            .unwrap();
        let e = poll(&skimap_common::site::CHAPELLE_DES_BOIS, &layout, &dl, false).unwrap_err();
        assert!(matches!(e, Error::PersonalSourcesDisabled(_)));
        assert!(!layout.out_root().exists(), "nothing written");
    }

    #[test]
    fn z_is_dropped() {
        let g = json!({"type":"LineString","coordinates":[[6.1,46.6,1085.0],[6.2,46.7,1090.0]]});
        assert_eq!(strip_z(g)["coordinates"], json!([[6.1, 46.6], [6.2, 46.7]]));
        let g = json!({"type":"MultiLineString","coordinates":[[[6.1,46.6,1.0]]]});
        assert_eq!(strip_z(g)["coordinates"], json!([[[6.1, 46.6]]]));
    }
}
