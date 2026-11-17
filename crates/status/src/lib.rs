//! Trail status snapshots (P13, #26), polled once for all users.
//!
//! The sources of a site come from the site table (`status`):
//! - ENJ city or domain box crossed by ENJ itineraries: ENJ Snowmap Geotrek API ([`enj`]), field `route` (1 closed, 2 open),
//!   status notes in `labels`, `update_datetime`.
//! - Nordic France station: white-label widget ([`nordicfrance`]).
//! - Town hall page: « Info damage » page of the town hall ([`mairie`]).
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
//! - `status.geojson`: itineraries with geometry (WGS84) and state; ENJ: the geometry of
//!   the API; Nordic France: the OSM ways of the `pistes` layer joined by name to the widget
//!   pistes and the town hall rows ([`join`]), unmatched names in `status.meta.json`;
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
pub mod join;
pub mod mairie;
pub mod nordicfrance;
pub mod store;

use chrono::{DateTime, Utc};
use serde::Serialize;
use serde_json::{Value, json};
use skimap_common::download::{Downloader, Origin, Request};
use skimap_common::layout::DataLayout;
use skimap_common::meta::{FileMeta, LayerMeta, SourceRef};
use skimap_common::output::write_json;
use skimap_common::site::Site;

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
    SourceRef::new(name, url, licence, attribution).personal_use_only()
}

/// The pistes layer of a site, to which the ENJ itineraries are attached.
struct EnjPistes {
    graphs: join::ItineraryGraphs,
    ways: Vec<join::OsmWay>,
    bbox: skimap_common::geom::BBox,
}

fn poll_enj(
    dir: &std::path::Path,
    dl: &Downloader,
    area: enj::EnjArea,
    now: DateTime<Utc>,
    latest: &mut serde_json::Map<String, Value>,
    pistes: Option<&EnjPistes>,
    stats: &mut serde_json::Map<String, Value>,
) -> Result<SourcePoll> {
    let (mut snap, origin) = enj::fetch_status(dl, area)?;
    let geoms = enj::fetch_geometry(dl, area)?;
    // `in_bbox=` selects the itineraries whose own box overlaps the area: keep those whose
    // line crosses it (an itinerary 500 m away is not one of the domain).
    if let enj::EnjArea::BBox(b) = area {
        snap.treks
            .retain(|t| geoms.get(&t.id).is_some_and(|(g, _)| enj::crosses(g, &b)));
    }
    let content =
        serde_json::to_value(&snap).map_err(|e| skimap_common::Error::json("ENJ snapshot", e))?;
    let store = Store::new(dir.join(enj::SOURCE));
    let (changed, snapshot) = record(&store, now, origin, &content)?;

    let mut along = Vec::new();
    let (mut points, mut attached, mut reachable) = (0_usize, 0_usize, 0_usize);
    let features: Vec<Value> = snap
        .treks
        .iter()
        .map(|t| {
            let (geometry, gpx) = geoms.get(&t.id).cloned().unwrap_or((Value::Null, None));
            let geometry = strip_z(geometry);
            let practice = join::practice_of(t.practice.as_deref().unwrap_or(""));
            let matched =
                pistes.map(|p| p.graphs.match_itinerary(&geometry, practice, Some(p.bbox)));
            let ids: Option<Vec<i64>> = matched.as_ref().map(|m| m.ways.clone());
            let osm_len = pistes.zip(ids.as_ref()).map(|(p, ids)| {
                let len: f64 = p
                    .ways
                    .iter()
                    .filter(|w| ids.binary_search(&w.osm_way_id).is_ok())
                    .map(|w| w.length_m)
                    .sum();
                (len * 10.0).round() / 10.0 + 0.0
            });
            if let Some(m) = &matched {
                points += m.points;
                attached += m.attached;
                reachable += m.reachable;
            }
            along.push(json!({
                "itinerary": format!("{}:{}", enj::SOURCE, t.id),
                "name": t.name,
                "practice": practice,
                "ways": ids.as_ref().map(Vec::len),
                "osm_length_m": osm_len,
                "length_m": t.length_m,
                "points": matched.as_ref().map(|m| m.points),
                "attached_rate": matched.as_ref().map(|m| (m.rate() * 1000.0).round() / 1000.0),
                "reachable": matched.as_ref().map(|m| m.reachable),
                "gaps": matched.as_ref().map(|m| &m.gaps),
            }));
            json!({
                "type": "Feature",
                "geometry": geometry,
                "properties": {
                        "id": t.id,
                    "itinerary": format!("{}:{}", enj::SOURCE, t.id),
                    "source": enj::SOURCE,
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
                    "state_at": t.update_datetime,
                    "osm_practice": practice,
                    "osm_way_ids": ids,
                    "osm_length_m": osm_len,
                }
            })
        })
        .collect();
    stats.insert(
        "enj_join".to_owned(),
        json!({
            "rule": format!(
                "itinéraire ENJ échantillonné tous les 10 m et apparié au graphe des pistes de sa pratique par HMM (comme le rattachement C2) ; voies OSM du chemin de Viterbi appariées sur au moins {} m (ou la moitié de la voie) à moins de {} m",
                join::MIN_MATCHED_M,
                join::MATCH_MAX_M
            ),
            "pistes_layer": pistes.is_some(),
            "points_in_box": points,
            "attached_rate": (points > 0).then(|| (attached as f64 / points as f64 * 1000.0).round() / 1000.0),
            "points_reachable": reachable,
            "attached_rate_reachable": (reachable > 0).then(|| (attached as f64 / reachable as f64 * 1000.0).round() / 1000.0),
            "itineraries": along,
        }),
    );
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
    let pc = |n: usize, d: usize| 100.0 * n as f64 / d.max(1) as f64;
    let summary = format!(
        "{} itinéraires : {} ouverts, {} fermés ; {}",
        snap.treks.len(),
        counts.get(&enj::TrailState::Open).copied().unwrap_or(0),
        counts.get(&enj::TrailState::Closed).copied().unwrap_or(0),
        if pistes.is_some() {
            format!(
                "rattachement aux voies OSM : {:.1} % des {points} points dans l'emprise, {:.1} % des {reachable} points atteignables",
                pc(attached, points),
                pc(attached, reachable)
            )
        } else {
            "couche pistes absente, pas de rattachement aux voies OSM".to_owned()
        }
    );
    latest.insert(enj::SOURCE.to_owned(), content);
    Ok(SourcePoll {
        source: enj::SOURCE.to_owned(),
        url: enj::status_url(area),
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
    let mut stats = serde_json::Map::new();
    let mut files = vec![
        FileMeta::new("status.json", "dernier contenu normalisé de chaque source"),
        FileMeta::new(
            "<source>/polls.jsonl, <source>/snapshots/",
            "historique horodaté",
        ),
        FileMeta::new("<source>/history.json", "fréquence de mise à jour observée"),
    ];

    if let Some(area) = enj::area(site) {
        // OSM ways of the itineraries, when the pistes layer was built.
        let pistes_path = layout.layer_dir(site, "pistes")?.join("pistes.geojson");
        let pistes = if pistes_path.is_file() {
            let dir = layout.layer_dir(site, "pistes")?;
            Some(EnjPistes {
                graphs: join::ItineraryGraphs::load(site.id, &dir)?,
                ways: join::read_pistes(&pistes_path)?,
                bbox: site.bbox,
            })
        } else {
            None
        };
        sources.push(poll_enj(
            &dir,
            dl,
            area,
            now,
            &mut latest,
            pistes.as_ref(),
            &mut stats,
        )?);
        refs.push(source_ref(
            "ENJ Snowmap (Geotrek), état des itinéraires",
            &enj::status_url(area),
            "aucune licence affichée (ticket L1) : usage personnel",
            "Espace Nordique Jurassien",
        ));
        files.push(FileMeta::new(
            "status.geojson",
            "itinéraires ENJ (WGS84) et leur état",
        ));
    }
    if let Some(station) = nordicfrance::station_id(site) {
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
        // Town hall page of the site table (`status.town_hall_page`), when the site has one.
        let mut mairie_value = None;
        if let Some(town_hall) = site.status.town_hall_page {
            let page = PageSource {
                source: mairie::SOURCE,
                url: town_hall,
                min_interval: mairie::MIN_INTERVAL,
                stamps: mairie::stamps,
            };
            let (mut p, mv) = poll_page(&dir, dl, &page, now, mairie::parse, &mut latest)?;
            p.summary = format!(
                "{} secteurs, page modifiée {}, « {} »",
                mv["sectors"].as_array().map_or(0, Vec::len),
                mv["modified_time"].as_str().unwrap_or("?"),
                mv["message"].as_str().unwrap_or("")
            );
            sources.push(p);
            refs.push(source_ref(
                &format!("Mairie de {}, page Info damage", site.name),
                town_hall,
                "aucune licence affichée : usage personnel, faible fréquence",
                &format!("Commune de {}", site.name),
            ));
            mairie_value = Some(mv);
        }

        // Geometry: the OSM pistes of the site joined by name.
        let pistes_dir = layout.layer_dir(site, "pistes")?;
        let pistes_path = pistes_dir.join("pistes.geojson");
        if !pistes_path.is_file() {
            return Err(Error::MissingPistes(pistes_path));
        }
        let ways = join::read_pistes(&pistes_path)?;
        let station: nordicfrance::NfStation = serde_json::from_value(v)
            .map_err(|e| skimap_common::Error::json(nordicfrance::SOURCE, e))?;
        let page: Option<mairie::MairiePage> = mairie_value
            .map(|mv| {
                serde_json::from_value(mv)
                    .map_err(|e| skimap_common::Error::json(mairie::SOURCE, e))
            })
            .transpose()?;
        let (features, report) = join::features(&station, page.as_ref(), &ways, &rfc3339(now));
        write_json(
            &dir.join("status.geojson"),
            &json!({ "type": "FeatureCollection", "features": features }),
        )?;
        if let Some(nf) = sources
            .iter_mut()
            .find(|s| s.source == nordicfrance::SOURCE)
        {
            nf.summary.push_str(&format!(
                ", {}/{} pistes sur OSM (sans : {})",
                report.nordicfrance_matched,
                report.nordicfrance_pistes,
                if report.nordicfrance_unmatched.is_empty() {
                    "aucune".to_owned()
                } else {
                    report.nordicfrance_unmatched.join(", ")
                }
            ));
        }
        if let Some(m) = sources.iter_mut().find(|s| s.source == mairie::SOURCE) {
            m.summary.push_str(&format!(
                ", {} lignes dont {} liées à Nordic France, {} à OSM seul, sans lien : {}",
                report.mairie_rows,
                report.mairie_with_nordicfrance,
                report.mairie_with_osm_only,
                if report.mairie_unmatched.is_empty() {
                    "aucune".to_owned()
                } else {
                    report.mairie_unmatched.join(", ")
                }
            ));
        }
        let pistes_meta = LayerMeta::read(&pistes_dir.join(LayerMeta::file_name("pistes")))?;
        refs.extend(pistes_meta.sources);
        files.push(FileMeta {
            count: Some(features.len()),
            ..FileMeta::new(
                "status.geojson",
                "pistes du widget Nordic France et lignes de la page de la mairie avec leur état, géométrie des voies OSM de la couche pistes rattachées par le nom (MultiLineString, WGS84)",
            )
        });
        stats.insert(
            "join".to_owned(),
            serde_json::to_value(&report)
                .map_err(|e| skimap_common::Error::json("join report", e))?,
        );
    }
    if sources.is_empty() {
        return Err(Error::NoSource(site.id.to_owned()));
    }
    write_json(
        &dir.join("status.json"),
        &json!({ "site": site.id, "polled_at": rfc3339(now), "sources": latest }),
    )?;
    LayerMeta {
        tickets: vec!["P13 (#26)".to_owned()],
        generated_at: rfc3339(now),
        valid_from: Some(rfc3339(now)),
        crs: Some("EPSG:4326 (WGS84, longitude, latitude)".to_owned()),
        sources: refs,
        files,
        stats: Value::Object(stats),
        notes: vec![
            "Sources sans licence affichée : activées seulement avec personal_sources.".into(),
            "L'état publié n'est pas un état en temps réel ; afficher l'heure de la source.".into(),
            "Les pistes du widget Nordic France et de la page de la mairie n'ont pas de géométrie ; status.geojson prend les voies OSM de la couche pistes dont la relation (sinon le nom de la voie) désigne la piste, selon la pratique ; noms sans voie dans stats.join, jamais devinés.".into(),
        ],
        ..LayerMeta::new(LAYER, site.id)
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
        let e = poll(
            skimap_common::site::site("chapelle-des-bois").expect("evaluation site"),
            &layout,
            &dl,
            false,
        )
        .unwrap_err();
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
