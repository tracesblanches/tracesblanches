//! ENJ (Espace Nordique Jurassien) Snowmap, a Geotrek-admin API: trail status of the
//! Jura domains whose site table gives an ENJ city (`status.enj_city`, P13, #26).
//!
//! `GET https://admin.snowmap.espacenordiquejurassien.com/api/v2/trek/?cities=<city>&language=fr`
//! answers without authentication: the cross-country, snowshoe and multi-activity
//! itineraries of the city. Fields used: `route` (1 = Fermé,
//! 2 = Ouvert, from `/api/v2/trek_route/`), `labels` (status notes such as « Marquée
//! partiellement ouverte le 13/01/2025 à 15h15 », names from `/api/v2/label/`),
//! `update_datetime`, `practice`, `difficulty`, `length_2d`, `networks` (1 = GTJ).
//!
//! The status poll asks only for these fields (`fields=`, about 9 kB instead of 800 kB);
//! the geometry is fetched at most once a day. The API sends neither `ETag` nor
//! `Last-Modified`, so every poll downloads the (small) status list.
//!
//! Licence: none displayed (ticket L1 open), hence personal use only for now.

use std::collections::BTreeMap;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use skimap_common::download::{CachePolicy, Downloader, Request};
use skimap_common::site::Site;

use crate::error::{Error, Result};

/// Base URL of the API.
pub const API: &str = "https://admin.snowmap.espacenordiquejurassien.com/api/v2";
/// Downloader source.
pub const SOURCE: &str = "enj";
/// Fields requested by the status poll.
pub const STATUS_FIELDS: &str =
    "id,name,route,labels,update_datetime,practice,difficulty,length_2d,networks,departure_city";

/// ENJ city identifier of a site (`cities=` filter), if the site is in the ENJ: the
/// `status.enj_city` of the site table (`skimap_common::site`).
pub fn city_id(site: &Site) -> Option<u32> {
    site.status.enj_city
}

/// URL of the status list of a city.
pub fn status_url(city: u32) -> String {
    format!("{API}/trek/?cities={city}&language=fr&fields={STATUS_FIELDS}")
}

/// URL of the geometries of a city.
pub fn geometry_url(city: u32) -> String {
    format!("{API}/trek/?cities={city}&language=fr&fields=id,geometry,gpx")
}

/// URL of the labels.
pub fn labels_url() -> String {
    format!("{API}/label/?language=fr")
}

/// Practice names (`/api/v2/trek_practice/`, 14/11/2026).
pub fn practice_name(id: i64) -> Option<&'static str> {
    Some(match id {
        1 => "Raquettes",
        2 => "Ski de fond",
        3 => "Pistes multi-activités",
        4 => "Piste chiens de traîneaux",
        _ => return None,
    })
}

/// Difficulty labels (`/api/v2/trek_difficulty/`, 14/11/2026).
pub fn difficulty_name(id: i64) -> Option<&'static str> {
    Some(match id {
        1 => "Piste facile",
        2 => "Piste moyenne",
        3 => "Piste difficile",
        4 => "Piste très difficile",
        _ => return None,
    })
}

/// State of a trail.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TrailState {
    /// Open (`route` 2, or « ouverte » in a note).
    Open,
    /// Partially open (status notes only).
    PartiallyOpen,
    /// Closed (`route` 1, or « fermée » in a note).
    Closed,
    /// Unknown value.
    Unknown,
}

/// State from the `route` field.
pub fn route_state(route: Option<i64>) -> TrailState {
    match route {
        Some(1) => TrailState::Closed,
        Some(2) => TrailState::Open,
        _ => TrailState::Unknown,
    }
}

/// A status note label (« Marquée fermée le 10/12/2021 à 10h09 »).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StatusNote {
    /// State announced.
    pub state: TrailState,
    /// Local date-time of the change, `YYYY-MM-DDTHH:MM` (Europe/Paris).
    pub at: String,
}

/// Parses a status note label; `None` for other labels (« Les bonnes pratiques… »).
pub fn parse_note(label: &str) -> Option<StatusNote> {
    let rest = label.trim().strip_prefix("Marquée ")?;
    let (state, rest) = rest.split_once(" le ")?;
    let state = match state.trim() {
        "ouverte" => TrailState::Open,
        "fermée" => TrailState::Closed,
        "partiellement ouverte" => TrailState::PartiallyOpen,
        _ => TrailState::Unknown,
    };
    let (date, time) = rest.split_once(" à ")?;
    let mut d = date.trim().split('/');
    let (dd, mm, yyyy) = (d.next()?, d.next()?, d.next()?);
    let (hh, mi) = time.trim().split_once('h')?;
    let n = |s: &str, len: usize| s.len() == len && s.bytes().all(|b| b.is_ascii_digit());
    if !(n(dd, 2) && n(mm, 2) && n(yyyy, 4) && n(hh, 2) && n(mi, 2)) {
        return None;
    }
    Some(StatusNote {
        state,
        at: format!("{yyyy}-{mm}-{dd}T{hh}:{mi}"),
    })
}

#[derive(Deserialize)]
struct Page<T> {
    next: Option<String>,
    results: Vec<T>,
}

#[derive(Deserialize)]
struct RawTrek {
    id: i64,
    name: String,
    #[serde(default)]
    practice: Option<i64>,
    #[serde(default)]
    difficulty: Option<i64>,
    #[serde(default)]
    route: Option<i64>,
    #[serde(default)]
    labels: Vec<i64>,
    #[serde(default)]
    update_datetime: Option<String>,
    #[serde(default)]
    length_2d: Option<f64>,
    #[serde(default)]
    networks: Vec<i64>,
    #[serde(default)]
    departure_city: Option<i64>,
}

#[derive(Deserialize)]
struct RawLabel {
    id: i64,
    name: String,
}

#[derive(Deserialize)]
struct RawGeom {
    id: i64,
    #[serde(default)]
    geometry: Option<Value>,
    #[serde(default)]
    gpx: Option<String>,
}

/// Status of one itinerary.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TrekStatus {
    /// Geotrek identifier.
    pub id: i64,
    /// Name.
    pub name: String,
    /// Practice name.
    pub practice: Option<String>,
    /// Difficulty label.
    pub difficulty: Option<String>,
    /// State from `route`.
    pub state: TrailState,
    /// Raw `route` value.
    pub route: Option<i64>,
    /// Label names.
    pub labels: Vec<String>,
    /// Status notes parsed from the labels.
    pub notes: Vec<StatusNote>,
    /// `update_datetime` as published (local time with offset).
    pub update_datetime: Option<String>,
    /// Length, metres (`length_2d`).
    pub length_m: Option<f64>,
    /// Part of the Grande Traversée du Jura (`networks` contains 1).
    pub gtj: bool,
    /// Departure city identifier.
    pub departure_city: Option<i64>,
}

/// Normalised content of one status poll (what a snapshot stores).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EnjSnapshot {
    /// ENJ city identifier.
    pub city: u32,
    /// Itineraries, sorted by identifier.
    pub treks: Vec<TrekStatus>,
    /// Status notes of the whole ENJ (`/label/`), sorted: when the managers' changes
    /// reached the API, all itineraries included.
    pub enj_notes: Vec<StatusNote>,
}

impl EnjSnapshot {
    /// Number of itineraries per state.
    pub fn counts(&self) -> BTreeMap<TrailState, usize> {
        let mut m = BTreeMap::new();
        for t in &self.treks {
            *m.entry(t.state).or_default() += 1;
        }
        m
    }
}

fn parse_pages<T: for<'de> Deserialize<'de>>(
    dl: &Downloader,
    first: &str,
    policy: CachePolicy,
    max_age: Option<Duration>,
) -> Result<(Vec<T>, skimap_common::download::Origin)> {
    let mut url = first.to_owned();
    let mut out = Vec::new();
    let mut origin = None;
    for _ in 0..50 {
        let mut req = Request::get(SOURCE, &url).cache(policy);
        if let Some(a) = max_age {
            req = req.max_age(a);
        }
        let f = dl.fetch(&req)?;
        origin.get_or_insert(f.origin);
        let page: Page<T> = serde_json::from_slice(&f.bytes()?)
            .map_err(|e| Error::format(SOURCE, format!("{url}: {e}")))?;
        out.extend(page.results);
        match page.next {
            Some(n) => url = n,
            None => {
                return Ok((
                    out,
                    origin.unwrap_or(skimap_common::download::Origin::Cache),
                ));
            }
        }
    }
    Err(Error::format(SOURCE, "more than 50 pages"))
}

/// Builds the normalised snapshot from the raw API answers (testable without network).
pub fn snapshot_from_json(city: u32, treks_json: &[u8], labels_json: &[u8]) -> Result<EnjSnapshot> {
    let treks: Page<RawTrek> = serde_json::from_slice(treks_json)
        .map_err(|e| Error::format(SOURCE, format!("treks: {e}")))?;
    let labels: Page<RawLabel> = serde_json::from_slice(labels_json)
        .map_err(|e| Error::format(SOURCE, format!("labels: {e}")))?;
    Ok(normalise(city, treks.results, labels.results))
}

fn normalise(city: u32, treks: Vec<RawTrek>, labels: Vec<RawLabel>) -> EnjSnapshot {
    let mut enj_notes: Vec<StatusNote> =
        labels.iter().filter_map(|l| parse_note(&l.name)).collect();
    enj_notes.sort_by(|a, b| a.at.cmp(&b.at));
    let names: BTreeMap<i64, String> = labels.into_iter().map(|l| (l.id, l.name)).collect();
    let mut treks: Vec<TrekStatus> = treks
        .into_iter()
        .map(|t| {
            let labels: Vec<String> = t
                .labels
                .iter()
                .map(|id| {
                    names
                        .get(id)
                        .cloned()
                        .unwrap_or_else(|| format!("label {id}"))
                })
                .collect();
            TrekStatus {
                id: t.id,
                name: t.name,
                practice: t.practice.and_then(practice_name).map(str::to_owned),
                difficulty: t.difficulty.and_then(difficulty_name).map(str::to_owned),
                state: route_state(t.route),
                route: t.route,
                notes: labels.iter().filter_map(|l| parse_note(l)).collect(),
                labels,
                update_datetime: t.update_datetime,
                length_m: t.length_2d,
                gtj: t.networks.contains(&1),
                departure_city: t.departure_city,
            }
        })
        .collect();
    treks.sort_by_key(|t| t.id);
    EnjSnapshot {
        city,
        treks,
        enj_notes,
    }
}

/// Polls the status of a city: always asks the server (the cache only absorbs errors).
pub fn fetch_status(
    dl: &Downloader,
    city: u32,
) -> Result<(EnjSnapshot, skimap_common::download::Origin)> {
    let (treks, origin) =
        parse_pages::<RawTrek>(dl, &status_url(city), CachePolicy::Revalidate, None)?;
    let (labels, _) = parse_pages::<RawLabel>(dl, &labels_url(), CachePolicy::Revalidate, None)?;
    Ok((normalise(city, treks, labels), origin))
}

/// Geometry and GPX link per itinerary, cached one day.
pub fn fetch_geometry(
    dl: &Downloader,
    city: u32,
) -> Result<BTreeMap<i64, (Value, Option<String>)>> {
    let (geoms, _) = parse_pages::<RawGeom>(
        dl,
        &geometry_url(city),
        CachePolicy::Normal,
        Some(Duration::from_secs(86_400)),
    )?;
    Ok(geoms
        .into_iter()
        .filter_map(|g| Some((g.id, (g.geometry?, g.gpx))))
        .collect())
}

/// Published stamps of a snapshot: the distinct `update_datetime` values, truncated to the
/// minute (the API writes them in batches a few seconds apart).
pub fn stamps(v: &Value) -> Vec<String> {
    let mut s: Vec<String> = v["treks"]
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(|t| t["update_datetime"].as_str())
                .map(|u| u.chars().take(16).collect())
                .collect()
        })
        .unwrap_or_default();
    s.sort();
    s.dedup();
    s
}

/// A state change of one itinerary between two snapshots.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Transition {
    /// First poll that saw the new state.
    pub seen_at: String,
    /// Itinerary identifier.
    pub id: i64,
    /// Itinerary name.
    pub name: String,
    /// Previous state.
    pub from: TrailState,
    /// New state.
    pub to: TrailState,
}

/// State changes across snapshots `(poll time, content)`.
pub fn transitions(snaps: &[(String, Value)]) -> Vec<Transition> {
    let mut out = Vec::new();
    let mut prev: BTreeMap<i64, TrailState> = BTreeMap::new();
    for (k, (t, v)) in snaps.iter().enumerate() {
        let Ok(s) = serde_json::from_value::<EnjSnapshot>(v.clone()) else {
            continue;
        };
        for tr in &s.treks {
            if let Some(&p) = prev.get(&tr.id)
                && p != tr.state
                && k > 0
            {
                out.push(Transition {
                    seen_at: t.clone(),
                    id: tr.id,
                    name: tr.name.clone(),
                    from: p,
                    to: tr.state,
                });
            }
            prev.insert(tr.id, tr.state);
        }
    }
    out
}

/// Distribution of the times of day (HH:MM) of the ENJ-wide status notes of a snapshot:
/// the moments at which the managers' changes reach the API.
pub fn note_times(s: &EnjSnapshot) -> BTreeMap<String, usize> {
    let mut m = BTreeMap::new();
    for n in &s.enj_notes {
        if let Some((_, hm)) = n.at.split_once('T') {
            *m.entry(hm.to_owned()).or_default() += 1;
        }
    }
    m
}

#[cfg(test)]
mod tests {
    use super::*;

    const TREKS: &str = r#"{"count":3,"next":null,"previous":null,"results":[
      {"id":245730,"departure_city":14,"difficulty":2,"labels":[1591],"length_2d":5172.3,"name":"Balcon du village","networks":[],"practice":1,"route":1,"update_datetime":"2026-11-14T15:15:10.033037+01:00"},
      {"id":244794,"departure_city":14,"difficulty":2,"labels":[1590],"length_2d":6242.8,"name":"Le Glacier","networks":[],"practice":2,"route":2,"update_datetime":"2026-11-14T15:15:26.414403+01:00"},
      {"id":244785,"departure_city":14,"difficulty":3,"labels":[1590,33],"length_2d":9346.8,"name":"La Chapellande - Hervé Balland","networks":[1],"practice":2,"route":1,"update_datetime":"2024-01-12T15:09:44.966360+01:00"}]}"#;
    const LABELS: &str = r#"{"count":3,"next":null,"previous":null,"results":[
      {"id":33,"advice":"","filter":false,"name":"Marquée fermée le 10/12/2021 à 10h09","pictogram":""},
      {"id":1590,"advice":"","filter":false,"name":"Les bonnes pratiques sur les pistes de ski","pictogram":""},
      {"id":1591,"advice":"","filter":false,"name":"Les bonnes pratiques sur les pistes raquettes","pictogram":""}]}"#;

    #[test]
    fn notes() {
        assert_eq!(
            parse_note("Marquée partiellement ouverte le 13/01/2025 à 15h15"),
            Some(StatusNote {
                state: TrailState::PartiallyOpen,
                at: "2025-01-13T15:15".into()
            })
        );
        assert_eq!(
            parse_note("Marquée fermée le 10/12/2021 à 10h09")
                .unwrap()
                .state,
            TrailState::Closed
        );
        assert_eq!(
            parse_note("Marquée ouverte le 11/01/2024 à 10h03")
                .unwrap()
                .at,
            "2024-01-11T10:03"
        );
        assert!(parse_note("Les bonnes pratiques sur les pistes de ski").is_none());
        assert!(parse_note("Marquée fermée le 1/1/2021 à 10h09").is_none());
    }

    #[test]
    fn snapshot_and_transitions() {
        let s = snapshot_from_json(14, TREKS.as_bytes(), LABELS.as_bytes()).unwrap();
        assert_eq!(s.treks.len(), 3);
        assert_eq!(s.treks[0].id, 244785, "sorted by id");
        assert!(s.treks[0].gtj);
        assert_eq!(s.treks[0].notes[0].at, "2021-12-10T10:09");
        assert_eq!(s.treks[1].state, TrailState::Open);
        assert_eq!(s.treks[2].practice.as_deref(), Some("Raquettes"));
        assert_eq!(s.counts()[&TrailState::Closed], 2);
        let v = serde_json::to_value(&s).unwrap();
        assert_eq!(stamps(&v), ["2024-01-12T15:09", "2026-11-14T15:15"]);
        let mut s2 = s.clone();
        s2.treks[1].state = TrailState::Closed;
        let tr = transitions(&[
            ("t0".into(), v),
            ("t1".into(), serde_json::to_value(&s2).unwrap()),
        ]);
        assert_eq!(tr.len(), 1);
        assert_eq!(
            (tr[0].from, tr[0].to),
            (TrailState::Open, TrailState::Closed)
        );
        assert_eq!(note_times(&s)["10:09"], 1);
        assert_eq!(s.enj_notes.len(), 1);
        assert_eq!(
            city_id(skimap_common::site::site("chapelle-des-bois").expect("evaluation site")),
            Some(14)
        );
        assert_eq!(
            city_id(skimap_common::site::site("cervieres").expect("evaluation site")),
            None
        );
        assert!(status_url(14).contains("cities=14") && status_url(14).contains("fields="));
    }
}
