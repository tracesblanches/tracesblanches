//! refuges.info (CC BY-SA 2.0): points of a box, incremental refresh with `depuis=`, last
//! dated comment of each point.
//!
//! API: <https://www.refuges.info/api/doc/>, no key. `detail=complet` gives every field;
//! comments come from `/api/commentaires?id_point=a,b,c` (several points per request,
//! each comment dated, newest first).
//!
//! Refresh: the full box is cached for a week (downloader policy of `refuges-info`);
//! every build also asks for the points modified since that snapshot (`depuis=<unix>`,
//! cached one hour) and lays them over it. A new full snapshot each week catches deleted
//! points.

use std::collections::BTreeMap;
use std::time::Duration;

use serde::Serialize;
use serde_json::Value;
use skimap_common::download::{Downloader, Fetched, Request};
use skimap_common::geom::{BBox, LonLat};

use crate::error::{RefugesError, Result};
use crate::privacy::{self, Phone};

/// Downloader source name.
pub const SOURCE: &str = "refuges-info";
/// API root.
pub const API: &str = "https://www.refuges.info/api";
/// Licence of the data.
pub const LICENCE: &str = "CC BY-SA 2.0";
/// Attribution.
pub const ATTRIBUTION: &str = "© les contributeurs de refuges.info (CC BY-SA 2.0)";
/// Points per comment request.
const COMMENT_CHUNK: usize = 40;

/// Dated comment of a point: only its date and id. The text and the author are never
/// kept (H1, #132: no signed comment in the layers; reproducing the text would also
/// require naming its author, CC BY-SA).
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Comment {
    /// Comment id.
    pub id: u64,
    /// Point id.
    pub point: u64,
    /// Date as given by the API (`YYYY-MM-DD hh:mm:ss.ffffff+zz`).
    pub date: String,
    /// Whether a photo is attached.
    pub photo: bool,
}

/// A refuges.info point.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Point {
    /// Point id.
    pub id: u64,
    /// Name.
    pub name: String,
    /// Type id (7 cabane non gardée, 9 gîte d'étape, 10 refuge gardé, ...).
    pub type_id: Option<u64>,
    /// Type label.
    pub type_label: Option<String>,
    /// Position.
    #[serde(skip)]
    pub pos: LonLat,
    /// Altitude, m.
    pub alt: Option<f64>,
    /// State (« Fermé au public », « Inutilisable », « Détruite », or empty).
    pub state: Option<String>,
    /// Sleeping places.
    pub places: Option<i64>,
    /// Mattress places.
    pub places_mattress: Option<i64>,
    /// Stove.
    pub stove: Option<bool>,
    /// Fireplace.
    pub fireplace: Option<bool>,
    /// Forest within 100 m (`bois`: not a wood store).
    pub forest_100m: Option<bool>,
    /// Water within 100 m.
    pub water_100m: Option<bool>,
    /// Blankets.
    pub blankets: Option<bool>,
    /// Latrines.
    pub latrines: Option<bool>,
    /// One wall missing.
    pub missing_wall: Option<bool>,
    /// Page on refuges.info.
    pub url: Option<String>,
    /// Last modification of the record.
    pub modified: Option<String>,
    /// Access text (phones, e-mails and person names removed, [`privacy::redact`]).
    pub access: Option<String>,
    /// Remarks (same filter).
    pub remark: Option<String>,
    /// Owner / whom to ask (same filter).
    pub owner: Option<String>,
    /// Phones of the establishment found in the owner text
    /// ([`privacy::establishment_phones`]).
    pub phones: Vec<Phone>,
    /// Official site given in the owner text (« Site officiel: »).
    pub website: Option<String>,
    /// Last dated comment.
    pub last_comment: Option<Comment>,
}

/// Whether a point is an open establishment (staffed hut or gîte, not closed).
pub fn is_establishment(type_id: Option<u64>, state: Option<&str>) -> bool {
    matches!(type_id, Some(9 | 10)) && state.is_none()
}

fn str_at<'a>(v: &'a Value, path: &str) -> Option<&'a str> {
    v.pointer(path)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
}

fn yes_no(v: &Value, key: &str) -> Option<bool> {
    match str_at(v, &format!("/info_comp/{key}/valeur")) {
        Some("Oui") => Some(true),
        Some("Non") => Some(false),
        _ => None,
    }
}

fn int_at(v: &Value, path: &str) -> Option<i64> {
    let x = v.pointer(path)?;
    x.as_i64()
        .or_else(|| x.as_f64().map(|f| f as i64))
        .or_else(|| x.as_str().and_then(|s| s.trim().parse().ok()))
}

/// Parses a GeoJSON FeatureCollection from `/api/bbox` (`detail=complet`).
pub fn parse_points(v: &Value) -> Result<Vec<Point>> {
    let features = v
        .get("features")
        .and_then(Value::as_array)
        .ok_or_else(|| RefugesError::format(SOURCE, "no features"))?;
    let mut out = Vec::with_capacity(features.len());
    for f in features {
        let p = f
            .get("properties")
            .ok_or_else(|| RefugesError::format(SOURCE, "feature without properties"))?;
        let id = f
            .get("id")
            .and_then(Value::as_u64)
            .or_else(|| p.get("id").and_then(Value::as_u64))
            .ok_or_else(|| RefugesError::format(SOURCE, "feature without id"))?;
        let c = f
            .pointer("/geometry/coordinates")
            .and_then(Value::as_array)
            .ok_or_else(|| RefugesError::format(SOURCE, format!("point {id} without geometry")))?;
        let (lon, lat) = match (
            c.first().and_then(Value::as_f64),
            c.get(1).and_then(Value::as_f64),
        ) {
            (Some(a), Some(b)) => (a, b),
            _ => {
                return Err(RefugesError::format(
                    SOURCE,
                    format!("point {id}: bad coordinates"),
                ));
            }
        };
        let type_id = p.pointer("/type/id").and_then(Value::as_u64);
        let state = str_at(p, "/etat/valeur").map(str::to_owned);
        let owner_raw = str_at(p, "/proprio/valeur");
        let establishment = is_establishment(type_id, state.as_deref());
        out.push(Point {
            id,
            name: str_at(p, "/nom").unwrap_or_default().to_owned(),
            type_id,
            type_label: str_at(p, "/type/valeur").map(str::to_owned),
            pos: LonLat::new(lon, lat),
            alt: p.pointer("/coord/alt").and_then(Value::as_f64),
            state,
            places: int_at(p, "/places/valeur"),
            places_mattress: int_at(p, "/info_comp/places_matelas/valeur"),
            stove: yes_no(p, "poele"),
            fireplace: yes_no(p, "cheminee"),
            forest_100m: yes_no(p, "bois"),
            water_100m: yes_no(p, "eau"),
            blankets: yes_no(p, "couvertures"),
            latrines: yes_no(p, "latrines"),
            missing_wall: yes_no(p, "manque_un_mur"),
            url: str_at(p, "/lien").map(str::to_owned),
            modified: str_at(p, "/date/derniere_modif").map(str::to_owned),
            access: str_at(p, "/acces/valeur").and_then(privacy::redact),
            remark: str_at(p, "/remarque/valeur").and_then(privacy::redact),
            owner: owner_raw.and_then(privacy::redact),
            phones: owner_raw
                .map(|o| privacy::establishment_phones(o, establishment, "refuges.info"))
                .unwrap_or_default(),
            website: owner_raw.and_then(privacy::official_site),
            last_comment: None,
        });
    }
    Ok(out)
}

/// Lays the points of an incremental answer over a full snapshot (same id: replaced; new
/// id: added). Returns the merged list ordered by id and the number of points updated or
/// added.
pub fn merge_incremental(full: Vec<Point>, inc: Vec<Point>) -> (Vec<Point>, usize) {
    let mut by_id: BTreeMap<u64, Point> = full.into_iter().map(|p| (p.id, p)).collect();
    let n = inc.len();
    for p in inc {
        by_id.insert(p.id, p);
    }
    (by_id.into_values().collect(), n)
}

/// Parses `/api/commentaires` (an object keyed "0", "1", ... or an array).
pub fn parse_comments(v: &Value) -> Result<Vec<Comment>> {
    let items: Vec<&Value> = match v {
        Value::Object(m) => m.values().collect(),
        Value::Array(a) => a.iter().collect(),
        _ => return Err(RefugesError::format(SOURCE, "comments: not an object")),
    };
    let mut out = Vec::new();
    for c in items {
        let (Some(id), Some(point)) = (
            c.get("id_commentaire").and_then(Value::as_u64),
            c.get("id_point").and_then(Value::as_u64),
        ) else {
            continue;
        };
        out.push(Comment {
            id,
            point,
            date: str_at(c, "/date_commentaire")
                .unwrap_or_default()
                .to_owned(),
            photo: c.get("photo-originale").is_some(),
        });
    }
    Ok(out)
}

/// Latest comment of each point (by date, then id).
pub fn latest_comments(comments: Vec<Comment>) -> BTreeMap<u64, Comment> {
    let mut out: BTreeMap<u64, Comment> = BTreeMap::new();
    for c in comments {
        let newer = out
            .get(&c.point)
            .is_none_or(|old| (c.date.as_str(), c.id) > (old.date.as_str(), old.id));
        if newer {
            out.insert(c.point, c);
        }
    }
    out
}

fn bbox_url(b: &BBox) -> String {
    format!(
        "{API}/bbox?bbox={},{},{},{}&format=geojson&detail=complet&nb_points=all&format_texte=texte",
        b.west, b.south, b.east, b.north
    )
}

/// What [`fetch`] downloaded.
#[derive(Debug, Clone)]
pub struct Fetch {
    /// Points with their last comment.
    pub points: Vec<Point>,
    /// Full snapshot.
    pub full: Fetched,
    /// Incremental answer (`depuis=`), if it succeeded.
    pub incremental: Option<Fetched>,
    /// Points updated or added by the incremental answer.
    pub incremental_points: usize,
    /// Comment requests.
    pub comments: Vec<Fetched>,
}

/// Downloads the points of `bbox`, refreshes them incrementally and attaches the last
/// dated comment of each.
pub fn fetch(dl: &Downloader, bbox: &BBox) -> Result<Fetch> {
    let full_url = bbox_url(bbox);
    let full = dl.fetch(&Request::get(SOURCE, &full_url))?;
    let points = parse_points(&full.json()?)?;
    let since = full.meta.fetched_at_unix;
    let inc_url = format!("{full_url}&depuis={since}");
    let (points, incremental, incremental_points) = match dl
        .fetch(&Request::get(SOURCE, &inc_url).max_age(Duration::from_secs(3600)))
    {
        Ok(f) => {
            let inc = parse_points(&f.json()?)?;
            let (merged, n) = merge_incremental(points, inc);
            (merged, Some(f), n)
        }
        Err(e) => {
            tracing::warn!(error = %e, "refuges.info incremental refresh failed, full snapshot only");
            (points, None, 0)
        }
    };
    let mut points = points;
    let ids: Vec<u64> = points.iter().map(|p| p.id).collect();
    let mut all = Vec::new();
    let mut comments = Vec::new();
    for chunk in ids.chunks(COMMENT_CHUNK) {
        let list = chunk
            .iter()
            .map(u64::to_string)
            .collect::<Vec<_>>()
            .join(",");
        let url = format!("{API}/commentaires?id_point={list}&format_texte=texte");
        let f = dl.fetch(&Request::get(SOURCE, &url).max_age(Duration::from_secs(86_400)))?;
        all.extend(parse_comments(&f.json()?)?);
        comments.push(f);
    }
    let latest = latest_comments(all);
    for p in &mut points {
        p.last_comment = latest.get(&p.id).cloned();
    }
    Ok(Fetch {
        points,
        full,
        incremental,
        incremental_points,
        comments,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn sample() -> Value {
        json!({
            "type": "FeatureCollection",
            "features": [{
                "type": "Feature", "id": 648,
                "geometry": {"type": "Point", "coordinates": [6.7, 44.8]},
                "properties": {
                    "nom": "Gîte de Vers le Col",
                    "type": {"id": 9, "valeur": "gîte d'étape"},
                    "coord": {"alt": 2140},
                    "etat": {"valeur": ""},
                    "places": {"valeur": 16},
                    "lien": "https://www.refuges.info/point/648/",
                    "date": {"derniere_modif": "2025-11-13 16:21:31+01"},
                    "info_comp": {
                        "poele": {"valeur": "Oui"}, "cheminee": {"valeur": "Non"},
                        "bois": {"valeur": "Oui"}, "eau": {"valeur": ""},
                        "places_matelas": {"valeur": "4"}
                    }
                }
            }, {
                "type": "Feature", "id": 1428,
                "geometry": {"type": "Point", "coordinates": [6.81, 44.85]},
                "properties": {"nom": "Refuge des Fonts", "etat": {"valeur": "Fermé au public"}}
            }, {
                "type": "Feature", "id": 7,
                "geometry": {"type": "Point", "coordinates": [6.1, 46.6]},
                "properties": {
                    "nom": "Gîte fictif", "type": {"id": 9, "valeur": "gîte d'étape"},
                    "proprio": {"valeur": "Anne et Luc PERRIN\r\nTél : 03 00 00 00 07\r\nMail : anne.perrin@free.fr\r\nSite officiel: https://www.gite-fictif.fr/"},
                    "acces": {"valeur": "Clé chez M. Dupont, 06 00 00 00 01"}
                }
            }]
        })
    }

    #[test]
    fn points() {
        let p = parse_points(&sample()).unwrap();
        assert_eq!(p.len(), 3);
        // Personal data filtered, establishment phone and site kept.
        let g = &p[2];
        assert_eq!(g.phones.len(), 1);
        assert_eq!(g.phones[0].display, "03 00 00 00 07");
        assert_eq!(g.website.as_deref(), Some("https://www.gite-fictif.fr/"));
        let owner = g.owner.as_deref().unwrap();
        assert!(
            !owner.contains("PERRIN") && !owner.contains("free.fr") && !owner.contains("00 07"),
            "{owner}"
        );
        let access = g.access.as_deref().unwrap();
        assert!(
            !access.contains("Dupont") && !access.contains("06 00"),
            "{access}"
        );
        assert_eq!(p[0].id, 648);
        assert_eq!(p[0].type_id, Some(9));
        assert_eq!(p[0].alt, Some(2140.0));
        assert_eq!(p[0].places, Some(16));
        assert_eq!(p[0].places_mattress, Some(4));
        assert_eq!(p[0].stove, Some(true));
        assert_eq!(p[0].fireplace, Some(false));
        assert_eq!(p[0].water_100m, None);
        assert_eq!(p[0].state, None);
        assert_eq!(p[1].state.as_deref(), Some("Fermé au public"));
        assert!(parse_points(&json!({"x": 1})).is_err());
    }

    #[test]
    fn incremental_merge() {
        let mut full = parse_points(&sample()).unwrap();
        full.truncate(2);
        let mut changed = full[1].clone();
        changed.places = Some(9);
        let mut new = full[0].clone();
        new.id = 5000;
        let (m, n) = merge_incremental(full, vec![changed, new]);
        assert_eq!(n, 2);
        let ids: Vec<u64> = m.iter().map(|p| p.id).collect();
        assert_eq!(ids, vec![648, 1428, 5000]);
        assert_eq!(m[1].places, Some(9));
    }

    #[test]
    fn comments() {
        let v = json!({
            "0": {"id_point": 1428, "id_commentaire": 55664, "date_commentaire": "2026-08-11 13:40:14+02", "texte_commentaire": " Contact ", "auteur_commentaire": "x"},
            "1": {"id_point": 648, "id_commentaire": 54639, "date_commentaire": "2026-06-07 18:44:00+02", "texte_commentaire": "Passé le 5 juin", "auteur_commentaire": "", "photo-originale": "/p.jpeg"},
            "2": {"id_point": 648, "id_commentaire": 52321, "date_commentaire": "2025-11-13 16:21:31+01", "texte_commentaire": "Actualisation", "auteur_commentaire": "T"}
        });
        let c = parse_comments(&v).unwrap();
        assert_eq!(c.len(), 3);
        let l = latest_comments(c);
        assert_eq!(l[&648].id, 54639);
        assert!(l[&648].photo);
        assert_eq!(l[&1428].date, "2026-08-11 13:40:14+02");
        // Neither the text nor the author is kept.
        let v = serde_json::to_value(&l[&1428]).unwrap();
        assert!(v.get("text").is_none() && v.get("author").is_none(), "{v}");
    }
}
