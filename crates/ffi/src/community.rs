//! « Mes sorties », challenges and OSM corrections on the phone (V12 #75, V13 #76, V15
//! #78), over `skimap-collab` (feature `phone`). Coarse calls: one per screen.
//!
//! Outings live in the app's history (`plans/`, [`crate::recap`]): directories holding an
//! `outing.json` (imported from Strava, written there by [`strava_import`]) or a
//! `trace.gpx` (traced by the app; its `outing.json`, the summary with the thinned track
//! the challenges need, is written beside it at the first reading). The list of the
//! outings is the app's history ([`crate::recap::outing_history`]); « Mes sorties » only
//! shows the challenges and the Strava import.

use std::path::Path;

use jiff::Timestamp;
use skimap_collab::challenges::{self, Progress};
use skimap_collab::osm_note::{self, Correction, Piste};
use skimap_collab::strava_export;
use skimap_collab::track::{self, Outing};
use skimap_core::routing::Style;

/// Errors returned to Kotlin (`CommunityException`).
#[derive(Debug, thiserror::Error, uniffi::Error)]
pub enum CommunityError {
    /// Not a Strava export, unreadable archive, bad input.
    #[error("{detail}")]
    Invalid {
        /// What is wrong.
        detail: String,
    },
}

fn invalid(e: impl std::fmt::Display) -> CommunityError {
    CommunityError::Invalid {
        detail: e.to_string(),
    }
}

/// What an import of the Strava archive did.
#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct StravaImport {
    /// Outings imported now.
    pub imported: u32,
    /// Already on the phone.
    pub already: u32,
    /// Ski activities without a track.
    pub without_track: u32,
    /// Activities of other sports, skipped.
    pub other_sports: u32,
    /// Outings the calibration can learn (`observation.json` written).
    pub observations: u32,
    /// Directories of the outings imported now (their `observation.json`, if any, for the
    /// calibration of the group).
    pub new_dirs: Vec<String>,
    /// Problems with single activities.
    pub errors: Vec<String>,
}

/// Imports the Strava export archive at `zip_path` into `out_root`, on the phone. Nothing
/// is sent; the Strava API is not called. `skating`: the group's technique (model times
/// of the calibration).
#[uniffi::export]
pub fn strava_import(
    zip_path: String,
    out_root: String,
    skating: bool,
) -> Result<StravaImport, CommunityError> {
    let style = if skating {
        Style::Skating
    } else {
        Style::Classic
    };
    let r = strava_export::import(Path::new(&zip_path), Path::new(&out_root), style)
        .map_err(invalid)?;
    Ok(StravaImport {
        imported: r.imported.len() as u32,
        already: r.already,
        without_track: r.without_track,
        other_sports: r.other_types.values().sum(),
        observations: r.observations,
        new_dirs: r
            .imported
            .iter()
            .map(|id| Path::new(&out_root).join(id).display().to_string())
            .collect(),
        errors: r.errors,
    })
}

fn outings(roots: &[String]) -> Vec<Outing> {
    let mut out = Vec::new();
    for root in roots {
        let Ok(rd) = std::fs::read_dir(root) else {
            continue;
        };
        for e in rd.flatten() {
            if e.path().is_dir()
                && let Some(o) = track::load_or_summarise(&e.path())
            {
                out.push(o);
            }
        }
    }
    out.sort_by(|a, b| b.start.cmp(&a.start).then_with(|| a.id.cmp(&b.id)));
    out
}

/// Kind of challenge.
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum ChallengeKind {
    /// A whole route (the GTJ).
    Route,
    /// Every piste of a domain.
    Domain,
    /// Kilometres this season.
    Distance,
}

/// A challenge and its progress.
#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct ChallengeView {
    /// Identifier (`gtj-srn`, `domain-<site>`, `km-500`).
    pub id: String,
    /// Kind.
    pub kind: ChallengeKind,
    /// Title (route or domain name; the target for the distance).
    pub title: String,
    /// Kilometres done.
    pub done_km: f64,
    /// Kilometres to do.
    pub total_km: f64,
    /// Share done, 0 to 1.
    pub share: f64,
    /// Outings that count.
    pub outings: u32,
    /// Day done (`AAAA-MM-JJ`), if done.
    pub done_on: Option<String>,
    /// Source and licence of the reference (route), else empty.
    pub attribution: String,
}

/// A domain whose pistes make a challenge: the pack's `pistes` layer.
#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct DomainRef {
    /// Pack identifier.
    pub site_id: String,
    /// Domain name.
    pub name: String,
    /// Path of the pack's `layers/pistes.geojson`.
    pub pistes_path: String,
}

/// Targets of the distance challenge, km: the next one not reached is shown.
pub const DISTANCE_TARGETS_KM: [f64; 7] = [50.0, 100.0, 250.0, 500.0, 1000.0, 2000.0, 5000.0];

fn view(
    id: String,
    kind: ChallengeKind,
    title: String,
    p: &Progress,
    attribution: String,
) -> ChallengeView {
    ChallengeView {
        id,
        kind,
        title,
        done_km: p.done_m / 1000.0,
        total_km: p.total_m / 1000.0,
        share: p.share,
        outings: p.outings,
        done_on: p.done_on.map(|t| t.strftime("%Y-%m-%d").to_string()),
        attribution,
    }
}

/// Start of the season of `now` (1 October).
fn season_start(now: Timestamp) -> Timestamp {
    let d = skimap_common::dates::utc_date(now);
    let y = if d.month() >= 10 {
        d.year()
    } else {
        d.year() - 1
    };
    skimap_common::dates::ymd(i32::from(y), 10, 1)
        .and_then(|d| skimap_common::dates::utc(d.at(0, 0, 0, 0)))
        .unwrap_or(now)
}

/// The challenges: routes of `routes_json` (the app's `challenges.json`), every piste of
/// each domain of `domains`, kilometres this season (`now_ms`). All from the outings under
/// `roots`, on the phone.
#[uniffi::export]
pub fn challenges_compute(
    roots: Vec<String>,
    routes_json: String,
    domains: Vec<DomainRef>,
    now_ms: i64,
) -> Result<Vec<ChallengeView>, CommunityError> {
    let all = outings(&roots);
    let mut out = Vec::new();
    let routes: serde_json::Value = serde_json::from_str(&routes_json).map_err(invalid)?;
    for r in routes["routes"].as_array().into_iter().flatten() {
        let line: Vec<[f64; 2]> = r["line"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|p| Some([p.get(0)?.as_f64()?, p.get(1)?.as_f64()?]))
            .collect();
        let p = challenges::coverage(&[line], &all);
        out.push(view(
            r["id"].as_str().unwrap_or("route").to_owned(),
            ChallengeKind::Route,
            r["name"].as_str().unwrap_or_default().to_owned(),
            &p,
            format!(
                "{} ({})",
                r["publisher"].as_str().unwrap_or_default(),
                r["licence"].as_str().unwrap_or_default()
            ),
        ));
    }
    for d in domains {
        let Ok(json) = std::fs::read_to_string(&d.pistes_path) else {
            continue;
        };
        let lines = challenges::geojson_lines(&json, |_| true).map_err(invalid)?;
        let p = challenges::coverage(&lines, &all);
        out.push(view(
            format!("domain-{}", d.site_id),
            ChallengeKind::Domain,
            d.name,
            &p,
            "© OpenStreetMap (ODbL)".to_owned(),
        ));
    }
    let now = Timestamp::from_millisecond(now_ms).unwrap_or_else(|_| Timestamp::now());
    let since = Some(season_start(now));
    let done = challenges::distance(&all, since, 0.0).done_m / 1000.0;
    let target = DISTANCE_TARGETS_KM
        .into_iter()
        .find(|t| *t > done)
        .unwrap_or(DISTANCE_TARGETS_KM[DISTANCE_TARGETS_KM.len() - 1]);
    let p = challenges::distance(&all, since, target);
    out.push(view(
        format!("km-{target}"),
        ChallengeKind::Distance,
        format!("{target}"),
        &p,
        String::new(),
    ));
    Ok(out)
}

/// A note for OpenStreetMap, to review and send.
#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct OsmNote {
    /// Latitude.
    pub lat: f64,
    /// Longitude.
    pub lon: f64,
    /// Text as published.
    pub text: String,
    /// Form body of `POST /api/0.6/notes`.
    pub form_body: String,
}

/// The OSM note of a correction (`missing_piste`, `piste_gone`, `wrong_name`,
/// `wrong_difficulty`, `wrong_grooming`, `wrong_oneway`, `other`) at a position, written
/// in `lang`.
#[uniffi::export]
pub fn osm_note_draft(
    correction: String,
    piste_name: Option<String>,
    osm_way_id: Option<i64>,
    comment: String,
    lon: f64,
    lat: f64,
    lang: crate::card::Lang,
) -> Result<OsmNote, CommunityError> {
    let c = Correction::parse(&correction)
        .ok_or_else(|| invalid(format!("correction {correction:?}")))?;
    let d = osm_note::draft(
        c,
        &Piste {
            name: piste_name,
            osm_way_id,
        },
        &comment,
        lon,
        lat,
        lang == crate::card::Lang::Fr,
    )
    .map_err(invalid)?;
    Ok(OsmNote {
        form_body: osm_note::form_body(&d),
        lat: d.lat,
        lon: d.lon,
        text: d.text,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn season_starts_in_october() {
        let t = |s: &str| s.parse::<Timestamp>().unwrap();
        assert_eq!(
            season_start(t("2026-01-10T00:00:00Z")),
            t("2025-10-01T00:00:00Z")
        );
        assert_eq!(
            season_start(t("2026-11-15T00:00:00Z")),
            t("2026-10-01T00:00:00Z")
        );
    }

    #[test]
    fn challenges_from_a_traced_outing() {
        let dir = tempfile::tempdir().unwrap();
        let plan = dir.path().join("plans/2026-01-10-1");
        std::fs::create_dir_all(&plan).unwrap();
        // 2 km north from (6.77, 44.85), one point every 5 m, 1 m/s... 4 m/s.
        let mut gpx = String::from("<gpx><trk><trkseg>");
        for i in 0..400 {
            gpx.push_str(&format!(
                "<trkpt lat=\"{}\" lon=\"6.77\"><time>2026-01-10T09:{:02}:{:02}Z</time></trkpt>",
                44.85 + f64::from(i) * 5.0 / 111_195.0,
                i * 2 / 60,
                (i * 2) % 60
            ));
        }
        gpx.push_str("</trkseg></trk></gpx>");
        std::fs::write(plan.join("trace.gpx"), gpx).unwrap();
        let routes = r#"{"routes":[{"id":"r","name":"Route","publisher":"P","licence":"L",
            "line":[[6.77,44.85],[6.77,44.8679865]]}]}"#;
        let pistes = dir.path().join("pistes.geojson");
        std::fs::write(
            &pistes,
            r#"{"type":"FeatureCollection","features":[{"type":"Feature","properties":{},
            "geometry":{"type":"LineString","coordinates":[[6.77,44.85],[6.77,44.859]]}},
            {"type":"Feature","properties":{},
            "geometry":{"type":"LineString","coordinates":[[6.80,44.85],[6.80,44.859]]}}]}"#,
        )
        .unwrap();
        let roots = vec![dir.path().join("plans").display().to_string()];
        let list = outings(&roots);
        assert_eq!(list.len(), 1);
        assert!(
            (list[0].distance_m - 1995.0).abs() < 10.0,
            "{}",
            list[0].distance_m
        );
        let now = "2026-01-20T00:00:00Z"
            .parse::<Timestamp>()
            .unwrap()
            .as_millisecond();
        let c = challenges_compute(
            roots,
            routes.into(),
            vec![DomainRef {
                site_id: "cervieres".into(),
                name: "Cervières".into(),
                pistes_path: pistes.display().to_string(),
            }],
            now,
        )
        .unwrap();
        assert_eq!(c.len(), 3);
        assert_eq!(c[0].kind, ChallengeKind::Route);
        assert!(c[0].share > 0.99 && c[0].done_on.as_deref() == Some("2026-01-10"));
        assert_eq!(c[1].kind, ChallengeKind::Domain);
        assert!((c[1].share - 0.5).abs() < 0.02, "{}", c[1].share);
        assert_eq!((c[2].kind, c[2].total_km), (ChallengeKind::Distance, 50.0));
        assert!((c[2].done_km - 1.995).abs() < 0.01);
    }

    #[test]
    fn note_and_errors() {
        let n = osm_note_draft(
            "piste_gone".into(),
            Some("Les Fonts".into()),
            Some(1),
            String::new(),
            6.77,
            44.85,
            crate::card::Lang::Fr,
        )
        .unwrap();
        assert!(
            n.text
                .starts_with("Cette piste de ski de fond n'existe plus")
        );
        assert!(n.form_body.starts_with("lat=44.85&lon=6.77&text="));
        assert!(
            osm_note_draft(
                "nope".into(),
                None,
                None,
                String::new(),
                6.77,
                44.85,
                crate::card::Lang::Fr
            )
            .is_err()
        );
        assert!(strava_import("/nonexistent.zip".into(), "/tmp".into(), false).is_err());
    }
}
