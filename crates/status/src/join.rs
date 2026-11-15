//! Geometry of the pistes of the sources without geometry (Nordic France widget, town hall
//! page): their names joined to the OSM pistes of the `pistes` layer (P2).
//!
//! - A widget piste takes the OSM ways of the site box whose **relation** name designates
//!   it ([`skimap_common::names::same_name`]); only when no relation matches, the ways
//!   whose own name does (a way name is often the road, « Route du Col d'Izoard », which
//!   goes on beyond the piste). The ways must suit the practice of the widget activity
//!   line: Nordic ski ways (`uses` contains `nordic`) for « Ski de fond », walking ways
//!   (`hike`) for « Raquettes », « Marche/trail », « Fatbike », any way for both.
//! - A row of the town hall page goes to the widget piste whose name is the closest
//!   ([`skimap_common::names::name_score`]); a row that designates no widget piste is
//!   joined to OSM itself, any practice.
//! - Names joined to no way are reported ([`JoinReport`]), never guessed.

use std::path::Path;

use serde::Serialize;
use serde_json::{Value, json};
use skimap_common::names::{name_key, name_score, same_name};

use crate::error::{Error, Result};
use crate::mairie::{self, MairiePage, MairieRow};
use crate::nordicfrance::{self, NfStation};

/// One OSM way of the `pistes` layer.
#[derive(Debug, Clone, PartialEq)]
pub struct OsmWay {
    /// OSM way identifier.
    pub osm_way_id: i64,
    /// Names of the piste relations holding the way.
    pub relations: Vec<String>,
    /// Name of the way itself.
    pub way_name: Option<String>,
    /// Nordic ski use.
    pub nordic: bool,
    /// Walking / snowshoe use.
    pub hike: bool,
    /// Length, m.
    pub length_m: f64,
    /// Coordinates, WGS84 `[lon, lat]`.
    pub coords: Value,
}

/// Reads the ways of the site box from `pistes/pistes.geojson`.
pub fn read_pistes(path: &Path) -> Result<Vec<OsmWay>> {
    let bytes = std::fs::read(path).map_err(|e| skimap_common::Error::io(path, e))?;
    let v: Value = serde_json::from_slice(&bytes)
        .map_err(|e| skimap_common::Error::json(path.display().to_string(), e))?;
    let mut out = Vec::new();
    for f in v["features"].as_array().into_iter().flatten() {
        let p = &f["properties"];
        if p["in_box"] == Value::Bool(false) || f["geometry"]["type"] != "LineString" {
            continue;
        }
        let Some(id) = p["osm_way_id"].as_i64() else {
            return Err(Error::format("pistes", "a feature without osm_way_id"));
        };
        let uses: Vec<&str> = p["uses"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
            .collect();
        out.push(OsmWay {
            osm_way_id: id,
            relations: p["relations"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|r| r["name"].as_str().map(str::to_owned))
                .collect(),
            way_name: p["way_name"].as_str().map(str::to_owned),
            nordic: uses.contains(&"nordic"),
            hike: uses.contains(&"hike"),
            length_m: p["length_m"].as_f64().unwrap_or(0.0),
            coords: f["geometry"]["coordinates"].clone(),
        });
    }
    Ok(out)
}

/// Practice of a widget piste, from its activity line.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Practice {
    /// « Ski de fond ».
    Nordic,
    /// « Raquettes », « Marche/trail », « Fatbike », « Piéton ».
    Foot,
    /// Both, or not stated.
    Any,
}

/// Practice of an activity line (« 3.5km - Marche/trail , Ski de fond »).
pub fn practice_of(activity: &str) -> Practice {
    let t = activity.to_lowercase();
    let ski = t.contains("ski de fond") || t.contains("skating") || t.contains("classique");
    let foot = ["raquette", "marche", "fatbike", "piéton"]
        .iter()
        .any(|w| t.contains(w));
    match (ski, foot) {
        (true, false) => Practice::Nordic,
        (false, true) => Practice::Foot,
        _ => Practice::Any,
    }
}

fn suits(w: &OsmWay, p: Practice) -> bool {
    match p {
        Practice::Nordic => w.nordic,
        Practice::Foot => w.hike,
        Practice::Any => true,
    }
}

/// OSM ways of a published piste name (see the module documentation).
pub fn ways_for<'a>(name: &str, practice: Practice, ways: &'a [OsmWay]) -> Vec<&'a OsmWay> {
    let by_relation: Vec<&OsmWay> = ways
        .iter()
        .filter(|w| suits(w, practice) && w.relations.iter().any(|r| same_name(r, name)))
        .collect();
    if !by_relation.is_empty() {
        return by_relation;
    }
    ways.iter()
        .filter(|w| suits(w, practice) && w.way_name.as_deref().is_some_and(|n| same_name(n, name)))
        .collect()
}

/// Open / closed / unknown, from a widget modifier (`closed`, `open`, `partially_open`) or
/// a town hall cell (« Fermé », « OUVERT »).
pub fn state_of(s: &str) -> &'static str {
    let s = s.trim().to_lowercase();
    if s == "open" || s == "partially_open" || s.starts_with("ouvert") {
        "open"
    } else if s == "closed" || s.starts_with("ferm") {
        "closed"
    } else {
        "unknown"
    }
}

/// Outcome of the join, written in `status.meta.json` (`stats.join`).
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct JoinReport {
    /// Widget pistes.
    pub nordicfrance_pistes: usize,
    /// Widget pistes joined to at least one OSM way.
    pub nordicfrance_matched: usize,
    /// Widget pistes joined to no way.
    pub nordicfrance_unmatched: Vec<String>,
    /// Town hall rows.
    pub mairie_rows: usize,
    /// Rows that designate a widget piste.
    pub mairie_with_nordicfrance: usize,
    /// Rows that designate no widget piste but OSM ways.
    pub mairie_with_osm_only: usize,
    /// Rows joined to nothing.
    pub mairie_unmatched: Vec<String>,
    /// Widget pistes whose state differs from the town hall row.
    pub conflicts: Vec<String>,
}

fn geometry(ways: &[&OsmWay]) -> Value {
    json!({
        "type": "MultiLineString",
        "coordinates": ways.iter().map(|w| w.coords.clone()).collect::<Vec<_>>(),
    })
}

fn osm_props(ways: &[&OsmWay]) -> (Vec<i64>, f64) {
    let ids = ways.iter().map(|w| w.osm_way_id).collect();
    let len: f64 = ways.iter().map(|w| w.length_m).sum();
    (ids, (len * 10.0).round() / 10.0)
}

fn mairie_json(r: &MairieRow) -> Value {
    json!({
        "piste": r.piste, "state": state_of(&r.state), "state_label": r.state,
        "last_grooming": r.last_grooming, "classic": r.classic, "skating": r.skating,
        "comment": r.comment,
    })
}

/// Itinerary identifier of a published name (`nordicfrance:le-gibera` style, words of
/// [`name_key`]), as read by the assistant.
pub fn itinerary_id(source: &str, name: &str) -> String {
    format!("{source}:{}", name_key(name).join("-"))
}

/// GeoJSON features (one per widget piste or town hall row joined to OSM ways) and the
/// report of the join.
pub fn features(
    nf: &NfStation,
    page: Option<&MairiePage>,
    ways: &[OsmWay],
    polled_at: &str,
) -> (Vec<Value>, JoinReport) {
    let mut report = JoinReport::default();
    let rows: Vec<&MairieRow> = page
        .map(|p| p.sectors.iter().flat_map(|s| &s.rows).collect())
        .unwrap_or_default();
    report.mairie_rows = rows.len();
    let nf_pistes: Vec<(&str, &nordicfrance::NfPiste)> = nf
        .sectors
        .iter()
        .flat_map(|s| s.pistes.iter().map(move |p| (s.name.as_str(), p)))
        .collect();
    report.nordicfrance_pistes = nf_pistes.len();
    // Each row to its closest widget piste (ties: the first).
    let mut row_of: Vec<Option<&MairieRow>> = vec![None; nf_pistes.len()];
    let mut row_used = vec![false; rows.len()];
    for (ri, r) in rows.iter().enumerate() {
        let best = nf_pistes
            .iter()
            .enumerate()
            .filter_map(|(i, (_, p))| name_score(&r.piste, &p.name).map(|s| (s, i)))
            .max_by_key(|(s, i)| (*s, std::cmp::Reverse(*i)));
        if let Some((_, i)) = best
            && row_of[i].is_none()
        {
            row_of[i] = Some(r);
            row_used[ri] = true;
            report.mairie_with_nordicfrance += 1;
        }
    }
    let mut out = Vec::new();
    for ((sector, p), row) in nf_pistes.iter().zip(&row_of) {
        let practice = practice_of(&p.activity);
        let matched = ways_for(&p.name, practice, ways);
        let state = state_of(p.state.as_deref().unwrap_or(""));
        let conflict = row.and_then(|r| {
            let m = state_of(&r.state);
            (m != "unknown" && state != "unknown" && m != state)
                .then(|| format!("{} : Nordic France {state}, mairie {m}", p.name))
        });
        if let Some(c) = &conflict {
            report.conflicts.push(c.clone());
        }
        if matched.is_empty() {
            report.nordicfrance_unmatched.push(p.name.clone());
            continue;
        }
        report.nordicfrance_matched += 1;
        let (ids, len) = osm_props(&matched);
        out.push(json!({
            "type": "Feature",
            "geometry": geometry(&matched),
            "properties": {
                "itinerary": itinerary_id(nordicfrance::SOURCE, &p.name),
                "source": nordicfrance::SOURCE,
                "name": p.name,
                "sector": sector,
                "activity": p.activity,
                "practice": practice,
                "state": state,
                "state_label": p.state_label,
                "length_km": p.length_km,
                "flag": p.flag,
                "comment": p.comment,
                "mairie": row.map(mairie_json),
                "conflict": conflict,
                "osm_way_ids": ids,
                "osm_length_m": len,
                "polled_at": polled_at,
            }
        }));
    }
    for (r, _) in rows.iter().zip(&row_used).filter(|(_, used)| !**used) {
        let matched = ways_for(&r.piste, Practice::Any, ways);
        if matched.is_empty() {
            report.mairie_unmatched.push(r.piste.clone());
            continue;
        }
        report.mairie_with_osm_only += 1;
        let (ids, len) = osm_props(&matched);
        out.push(json!({
            "type": "Feature",
            "geometry": geometry(&matched),
            "properties": {
                "itinerary": itinerary_id(mairie::SOURCE, &r.piste),
                "source": mairie::SOURCE,
                "name": r.piste,
                "practice": Practice::Any,
                "state": state_of(&r.state),
                "state_label": r.state,
                "mairie": mairie_json(r),
                "osm_way_ids": ids,
                "osm_length_m": len,
                "polled_at": polled_at,
            }
        }));
    }
    (out, report)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mairie::MairieSector;
    use crate::nordicfrance::{NfPiste, NfSector};

    fn way(
        id: i64,
        relations: &[&str],
        way_name: Option<&str>,
        nordic: bool,
        hike: bool,
    ) -> OsmWay {
        OsmWay {
            osm_way_id: id,
            relations: relations.iter().map(|s| (*s).to_owned()).collect(),
            way_name: way_name.map(str::to_owned),
            nordic,
            hike,
            length_m: 100.0,
            coords: json!([[6.7, 44.8], [6.71, 44.81]]),
        }
    }

    fn piste(name: &str, activity: &str, state: &str) -> NfPiste {
        NfPiste {
            name: name.into(),
            activity: activity.into(),
            length_km: None,
            flag: None,
            state: Some(state.into()),
            state_label: None,
            comment: None,
        }
    }

    fn row(name: &str, state: &str) -> MairieRow {
        MairieRow {
            piste: name.into(),
            state: state.into(),
            last_grooming: String::new(),
            classic: String::new(),
            skating: String::new(),
            comment: String::new(),
        }
    }

    /// Ways shaped like Cervières: the road, the Izoard relation on part of it, a hike
    /// relation, the Sioux, an unnamed way.
    fn ways() -> Vec<OsmWay> {
        vec![
            way(
                1,
                &["Izoard", "Route du Col d'Izoard"],
                Some("Route du Col d'Izoard"),
                true,
                true,
            ),
            way(
                2,
                &["Le Cancel"],
                Some("Route du Col d'Izoard"),
                true,
                false,
            ),
            way(3, &["Route du Col d'Izoard"], None, false, true),
            way(4, &["L'Aigalan", "Le Chemin du Sioux"], None, true, false),
            way(5, &[], Some("Le Gibera"), true, false),
            way(6, &[], None, true, true),
        ]
    }

    #[test]
    fn practices() {
        assert_eq!(practice_of("16km - Ski de fond"), Practice::Nordic);
        assert_eq!(
            practice_of("16km - Raquettes , Marche/trail , Fatbike"),
            Practice::Foot
        );
        assert_eq!(
            practice_of("3.5km - Marche/trail , Ski de fond"),
            Practice::Any
        );
        assert_eq!(state_of("Fermé"), "closed");
        assert_eq!(state_of("OUVERT"), "open");
        assert_eq!(state_of("partially_open"), "open");
        assert_eq!(state_of(""), "unknown");
    }

    #[test]
    fn relations_first_then_way_names_with_the_practice() {
        let w = ways();
        let ids = |v: Vec<&OsmWay>| v.iter().map(|w| w.osm_way_id).collect::<Vec<_>>();
        // The ski piste: Nordic ways of the Izoard relations, not way 2 (only its way
        // name is the road) nor the walking way 3.
        assert_eq!(ids(ways_for("L'Izoard", Practice::Nordic, &w)), vec![1]);
        // The walking itinerary along the road: walking ways of the same relations.
        assert_eq!(
            ids(ways_for("Piéton multiactivités Izoard", Practice::Foot, &w)),
            vec![1, 3]
        );
        assert_eq!(ids(ways_for("Le Sioux", Practice::Nordic, &w)), vec![4]);
        // No relation: the way name.
        assert_eq!(ids(ways_for("Le Gibera", Practice::Nordic, &w)), vec![5]);
        assert!(ways_for("Piéton les Fonts", Practice::Foot, &w).is_empty());
        assert!(ways_for("Le Sioux", Practice::Foot, &w).is_empty());
    }

    #[test]
    fn widget_and_town_hall_joined_unmatched_reported() {
        let nf = NfStation {
            name: None,
            snow_depth: None,
            last_grooming: None,
            snow_quality: None,
            updated: None,
            open_summary: None,
            sectors: vec![NfSector {
                name: "IZOARD".into(),
                state: None,
                grooming: None,
                comment: None,
                pistes: vec![
                    piste("L'Izoard", "16km - Ski de fond", "open"),
                    piste("Piéton multiactivités Izoard", "16km - Raquettes", "closed"),
                    piste("Piéton les Fonts", "22km - Raquettes", "closed"),
                ],
            }],
        };
        let page = MairiePage {
            modified_time: None,
            message: String::new(),
            sectors: vec![MairieSector {
                header: String::new(),
                rows: vec![
                    row("Izoard 16km", "Fermé"),
                    row("Le Gibera 1.5km", "Ouvert"),
                    row("Liaison Cervières-le Laus 1.5m", "Fermé"),
                ],
            }],
        };
        let (f, r) = features(&nf, Some(&page), &ways(), "2026-11-15T00:00:00Z");
        assert_eq!(f.len(), 3);
        let p = &f[0]["properties"];
        assert_eq!(p["itinerary"], "nordicfrance:izoard");
        assert_eq!(p["state"], "open");
        assert_eq!(p["mairie"]["piste"], "Izoard 16km");
        assert_eq!(p["osm_way_ids"], json!([1]));
        assert_eq!(f[0]["geometry"]["type"], "MultiLineString");
        assert_eq!(f[1]["properties"]["osm_way_ids"], json!([1, 3]));
        assert_eq!(f[1]["properties"]["mairie"], Value::Null);
        // Town hall row without widget piste, joined to OSM by itself.
        assert_eq!(f[2]["properties"]["itinerary"], "cervieres-mairie:gibera");
        assert_eq!(f[2]["properties"]["state"], "open");
        assert_eq!(r.nordicfrance_pistes, 3);
        assert_eq!(r.nordicfrance_matched, 2);
        assert_eq!(r.nordicfrance_unmatched, vec!["Piéton les Fonts"]);
        assert_eq!(r.mairie_rows, 3);
        assert_eq!(r.mairie_with_nordicfrance, 1);
        assert_eq!(r.mairie_with_osm_only, 1);
        assert_eq!(r.mairie_unmatched, vec!["Liaison Cervières-le Laus 1.5m"]);
        assert_eq!(
            r.conflicts,
            vec!["L'Izoard : Nordic France open, mairie closed"]
        );
    }
}
