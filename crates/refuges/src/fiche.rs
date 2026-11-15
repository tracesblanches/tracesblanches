//! Practical hut cards (H1, #132): one point per refuges.info hut, gîte or shelter, with
//! the Camptocamp details of the same place.
//!
//! Licences (L3, #8): refuges.info (CC BY-SA 2.0) gives every geometry; Camptocamp
//! (CC BY-SA 3.0, collaborative documents only) adds properties, matched by position and
//! name **without OSM**; the result is under CC BY-SA 4.0 (« later version » clause of
//! both). OpenStreetMap stays in `osm.geojson` and `links.json` (ODbL, never merged).
//!
//! Personal data: free texts go through [`crate::privacy::redact`]; phones are only those
//! of establishments ([`crate::privacy::establishment_phones`], Camptocamp `phone` of a
//! staffed hut or a gîte); the last comment is reduced to its date.

use std::collections::BTreeMap;

use serde::Serialize;
use skimap_common::geom::LonLat;

use crate::camptocamp::Waypoint;
use crate::links;
use crate::privacy::Phone;
use crate::refuges_info::Point;

/// Licence of the cards file.
pub const LICENCE: &str =
    "CC BY-SA 4.0 (refuges.info CC BY-SA 2.0, Camptocamp CC BY-SA 3.0, version ultérieure)";
/// refuges.info types kept as cards: cabane non gardée, gîte d'étape, refuge gardé,
/// bâtiment (« à investiguer »).
pub const TYPES: &[u64] = &[7, 9, 10, 28];

/// A link to a Camptocamp document.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct CamptocampRef {
    /// Document id.
    pub id: u64,
    /// Title.
    pub title: String,
    /// Page.
    pub url: String,
    /// Authors to credit (history page).
    pub attribution_url: Option<String>,
}

/// Equipment, each `None` when no source says.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct Equipment {
    /// Stove (refuges.info).
    pub stove: Option<bool>,
    /// Fireplace (refuges.info).
    pub fireplace: Option<bool>,
    /// Heating when unstaffed (Camptocamp).
    pub heating_winter: Option<bool>,
    /// Forest within 100 m (refuges.info « bois »: firewood can be found, not a wood store).
    pub wood_nearby: Option<bool>,
    /// Water within 100 m (refuges.info).
    pub water_nearby: Option<bool>,
    /// Blankets (refuges.info, else Camptocamp when unstaffed).
    pub blankets: Option<bool>,
    /// Mattresses (refuges.info places on mattresses > 0, else Camptocamp when unstaffed).
    pub mattresses: Option<bool>,
    /// Gas when unstaffed (Camptocamp).
    pub gas_winter: Option<bool>,
    /// Latrines (refuges.info).
    pub latrines: Option<bool>,
}

/// A hut card.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Fiche {
    /// `ri-<refuges.info id>`.
    pub id: String,
    /// refuges.info id.
    pub refuges_info_id: u64,
    /// Name.
    pub name: String,
    /// `garde` (refuge gardé), `gite` (gîte d'étape), `non_garde` (cabane non gardée),
    /// `abri` (shelter: a wall missing, no sleeping place, or « Abri » in the name),
    /// `batiment` (building of unknown use).
    pub kind: String,
    /// French label of the kind.
    pub kind_label: String,
    /// State from refuges.info (« Fermé au public », « Détruite », « Inutilisable »).
    pub state: Option<String>,
    /// No closing state.
    pub open: bool,
    /// Altitude, m.
    pub alt: Option<f64>,
    /// Sleeping places (refuges.info).
    pub places: Option<i64>,
    /// Places on mattresses (refuges.info).
    pub places_mattress: Option<i64>,
    /// Places when not staffed, winter room (Camptocamp `capacity`).
    pub places_winter: Option<i64>,
    /// Places when staffed (Camptocamp).
    pub places_staffed: Option<i64>,
    /// Staffed period as written by Camptocamp.
    pub wardened_period: Option<String>,
    /// Winter access: `ferme`, `local_hiver`, `cle`, `pendant_gardiennage`, `libre`,
    /// `inconnu`.
    pub winter_access: String,
    /// French sentence for `winter_access`.
    pub winter_access_text: String,
    /// Source of `winter_access` (`refuges.info`, `camptocamp`).
    pub winter_access_source: String,
    /// Equipment.
    pub equipment: Equipment,
    /// Establishment phones (staffed hut, gîte, town hall, tourist office, patrol post).
    pub phones: Vec<Phone>,
    /// Official site.
    pub website: Option<String>,
    /// Source of `website`.
    pub website_source: Option<String>,
    /// Access (filtered free text, refuges.info).
    pub access: Option<String>,
    /// Remarks (filtered free text, refuges.info).
    pub remark: Option<String>,
    /// Whom to ask (filtered free text, refuges.info).
    pub owner: Option<String>,
    /// refuges.info page.
    pub refuges_info_url: Option<String>,
    /// Camptocamp documents of the same place.
    pub camptocamp: Vec<CamptocampRef>,
    /// Date of the latest information (YYYY-MM-DD): last modification of the refuges.info
    /// record or last comment, whichever is later.
    pub info_date: Option<String>,
    /// Last modification of the refuges.info record (YYYY-MM-DD).
    pub refuges_info_modified: Option<String>,
    /// Date of the last refuges.info comment (YYYY-MM-DD); its text is not kept.
    pub last_comment_date: Option<String>,
    /// Commune française au point (geo.api.gouv.fr).
    pub in_france: bool,
    /// When the sources of the card were read (RFC 3339): refuges.info snapshot or update.
    pub checked_at: Option<String>,
    /// Last time the card changed (set by [`crate::refresh()`], from `journal.jsonl`).
    pub changed_at: Option<String>,
    /// Licence of the card.
    pub licence: String,
    /// Attribution to display with the card.
    pub attribution: String,
}

fn day(s: &str) -> Option<String> {
    let d = s.get(..10)?;
    (d.len() == 10 && d.as_bytes()[4] == b'-').then(|| d.to_owned())
}

/// Camptocamp waypoints of each refuges.info point, by position and name only (no OSM).
/// Only CC BY-SA documents are used (L3 rule 2).
pub fn match_camptocamp(points: &[Point], c2c: &[Waypoint]) -> BTreeMap<u64, Vec<usize>> {
    let mut out: BTreeMap<u64, Vec<usize>> = BTreeMap::new();
    for (wi, w) in c2c.iter().enumerate() {
        if w.licence
            .as_deref()
            .is_some_and(|l| !l.starts_with("CC-BY-SA"))
        {
            continue;
        }
        let best = points
            .iter()
            .filter(|p| p.type_id.is_some_and(|t| TYPES.contains(&t)))
            .filter_map(|p| {
                links::same_place((&w.title, w.pos), (&p.name, p.pos)).map(|(_, k)| (p.id, k))
            })
            .min_by(|a, b| a.1.total_cmp(&b.1));
        if let Some((id, _)) = best {
            out.entry(id).or_default().push(wi);
        }
    }
    out
}

fn kind_of(p: &Point) -> (&'static str, &'static str) {
    match p.type_id {
        Some(10) => ("garde", "refuge gardé"),
        Some(9) => ("gite", "gîte d'étape"),
        Some(28) => ("batiment", "bâtiment"),
        _ => {
            let n = p.name.to_lowercase();
            let shelter =
                p.missing_wall == Some(true) || n.starts_with("abri") || p.places == Some(0);
            if shelter {
                ("abri", "abri")
            } else {
                ("non_garde", "cabane non gardée")
            }
        }
    }
}

fn winter_access(p: &Point, kind: &str, c2c: &[&Waypoint]) -> (String, String, String) {
    let t = |a: &str, b: &str, s: &str| (a.to_owned(), b.to_owned(), s.to_owned());
    if let Some(s) = &p.state {
        return t(
            "ferme",
            &format!("fermé ou inutilisable : {s}"),
            "refuges.info",
        );
    }
    if let Some(c) = c2c.iter().find_map(|w| w.custodianship.as_deref()) {
        return match c {
            "always_accessible" => t(
                "local_hiver",
                "ouvert toute l'année : local d'hiver hors gardiennage",
                "camptocamp",
            ),
            "key_needed" => t("cle", "clé à demander hors gardiennage", "camptocamp"),
            "accessible_when_wardened" => t(
                "pendant_gardiennage",
                "accessible seulement pendant le gardiennage",
                "camptocamp",
            ),
            _ => t("libre", "non gardé, ouvert", "camptocamp"),
        };
    }
    match kind {
        "non_garde" | "abri" => t(
            "libre",
            "cabane ou abri non gardé, ouvert (selon refuges.info)",
            "refuges.info",
        ),
        _ => t(
            "inconnu",
            "accès hors gardiennage inconnu : se renseigner auprès de l'établissement",
            "refuges.info",
        ),
    }
}

/// Builds the card of a refuges.info point.
pub fn fiche(p: &Point, c2c: &[&Waypoint], in_france: bool) -> Fiche {
    let (kind, kind_label) = kind_of(p);
    let (winter_access, winter_access_text, winter_access_source) = winter_access(p, kind, c2c);
    let first = |f: &dyn Fn(&Waypoint) -> Option<i64>| c2c.iter().find_map(|w| f(w));
    let firstb = |f: &dyn Fn(&Waypoint) -> Option<bool>| c2c.iter().find_map(|w| f(w));
    // A closed place has no phone to call (« Fermé définitivement »...).
    let mut phones = if p.state.is_none() {
        p.phones.clone()
    } else {
        Vec::new()
    };
    for w in c2c.iter().filter(|_| p.state.is_none()) {
        for ph in &w.phones {
            if !phones.iter().any(|x| x.e164 == ph.e164) {
                phones.push(ph.clone());
            }
        }
    }
    let (website, website_source) = match (&p.website, c2c.iter().find_map(|w| w.website.clone())) {
        (Some(u), _) => (Some(u.clone()), Some("refuges.info".to_owned())),
        (None, Some(u)) => (Some(u), Some("camptocamp".to_owned())),
        _ => (None, None),
    };
    let modified = p.modified.as_deref().and_then(day);
    let comment = p.last_comment.as_ref().and_then(|c| day(&c.date));
    let info_date = modified.clone().max(comment.clone());
    let mut attribution = "© les contributeurs de refuges.info (CC BY-SA 2.0)".to_owned();
    for w in c2c {
        attribution.push_str(&format!(
            " ; Camptocamp, « {} », auteurs : {} (CC BY-SA 3.0)",
            w.title,
            w.attribution_url.as_deref().unwrap_or(&w.url)
        ));
    }
    Fiche {
        id: format!("ri-{}", p.id),
        refuges_info_id: p.id,
        name: p.name.clone(),
        kind: kind.to_owned(),
        kind_label: kind_label.to_owned(),
        state: p.state.clone(),
        open: p.state.is_none(),
        alt: p.alt,
        places: p.places,
        places_mattress: p.places_mattress,
        places_winter: first(&|w| w.capacity_unstaffed),
        places_staffed: first(&|w| w.capacity_staffed),
        wardened_period: c2c.iter().find_map(|w| w.access_period.clone()),
        winter_access,
        winter_access_text,
        winter_access_source,
        equipment: Equipment {
            stove: p.stove,
            fireplace: p.fireplace,
            heating_winter: firstb(&|w| w.heating_unstaffed),
            wood_nearby: p.forest_100m,
            water_nearby: p.water_100m,
            blankets: p.blankets.or_else(|| firstb(&|w| w.blanket_unstaffed)),
            mattresses: p
                .places_mattress
                .map(|n| n > 0)
                .or_else(|| firstb(&|w| w.mattress_unstaffed)),
            gas_winter: firstb(&|w| w.gas_unstaffed),
            latrines: p.latrines,
        },
        phones,
        website,
        website_source,
        access: p.access.clone(),
        remark: p.remark.clone(),
        owner: p.owner.clone(),
        refuges_info_url: p.url.clone(),
        camptocamp: c2c
            .iter()
            .map(|w| CamptocampRef {
                id: w.id,
                title: w.title.clone(),
                url: w.url.clone(),
                attribution_url: w.attribution_url.clone(),
            })
            .collect(),
        info_date,
        refuges_info_modified: modified,
        last_comment_date: comment,
        in_france,
        checked_at: None,
        changed_at: None,
        licence: "CC BY-SA 4.0".to_owned(),
        attribution,
    }
}

/// Cards of all huts, gîtes and shelters of the box, with their position.
pub fn fiches(points: &[Point], in_france: &[bool], c2c: &[Waypoint]) -> Vec<(LonLat, Fiche)> {
    let matched = match_camptocamp(points, c2c);
    points
        .iter()
        .zip(in_france)
        .filter(|(p, _)| p.type_id.is_some_and(|t| TYPES.contains(&t)))
        .map(|(p, fr)| {
            let ws: Vec<&Waypoint> = matched
                .get(&p.id)
                .map(|v| v.iter().map(|&i| &c2c[i]).collect())
                .unwrap_or_default();
            (p.pos, fiche(p, &ws, *fr))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::privacy;
    use crate::refuges_info::{Comment, parse_points};
    use serde_json::json;

    // Invented records (no real person, no real number).
    fn ri() -> Vec<Point> {
        let mut p = parse_points(&json!({"features": [
            {"id": 1, "geometry": {"coordinates": [6.8158, 44.8447]}, "properties": {
                "nom": "Refuge des Fonts", "type": {"id": 10, "valeur": "refuge gardé"},
                "places": {"valeur": 40},
                "date": {"derniere_modif": "2025-11-13 16:21:31+01"},
                "proprio": {"valeur": "Gardien : Jean Martin\r\n0600000009"},
                "info_comp": {"eau": {"valeur": "Oui"}}}},
            {"id": 2, "geometry": {"coordinates": [6.10, 46.60]}, "properties": {
                "nom": "Abri des Sapins", "type": {"id": 7, "valeur": "cabane non gardée"},
                "proprio": {"valeur": "Mairie - 25240 Village - 03 00 00 00 04"},
                "remarque": {"valeur": "Clé chez M. Dupont, jean.dupont@free.fr"},
                "info_comp": {"poele": {"valeur": "Oui"}, "bois": {"valeur": "Oui"}, "places_matelas": {"valeur": "0"}}}},
            {"id": 3, "geometry": {"coordinates": [6.2, 46.7]}, "properties": {
                "nom": "Point d'eau", "type": {"id": 23, "valeur": "point d'eau"}}}
        ]}))
        .unwrap();
        p[0].last_comment = Some(Comment {
            id: 9,
            point: 1,
            date: "2026-08-11 13:40:14+02".to_owned(),
            photo: false,
        });
        p
    }

    fn c2c() -> Vec<Waypoint> {
        let doc = json!({
            "document_id": 162324, "waypoint_type": "hut", "capacity": 9, "capacity_staffed": 62,
            "custodianship": "always_accessible", "heating_unstaffed": false,
            "phone": "04 00 00 00 01", "phone_custodian": "06 00 00 00 02",
            "geometry": {"geom": "{\"type\": \"Point\", \"coordinates\": [758734.8, 5597109.2]}"},
            "locales": [{"lang": "fr", "title": "Les Fonts de Cervières"}],
            "cooked": {"title": "Les Fonts de Cervières", "access_period": "<p>du 01/03 au 01/05</p>"},
            "license": {"id": "CC-BY-SA-3.0", "attribution_url": "https://www.camptocamp.org/waypoints/history/162324/fr"}
        });
        vec![crate::camptocamp::parse_waypoint(&doc).unwrap()]
    }

    #[test]
    fn cards() {
        let f = fiches(&ri(), &[true, true, true], &c2c());
        assert_eq!(f.len(), 2, "water point is not a card");
        let fonts = &f[0].1;
        assert_eq!(fonts.kind, "garde");
        assert_eq!(fonts.places_winter, Some(9));
        assert_eq!(fonts.winter_access, "local_hiver");
        assert_eq!(fonts.camptocamp.len(), 1);
        // The custodian's mobile from refuges.info is dropped; Camptocamp's hut phone stays.
        assert_eq!(fonts.phones.len(), 1);
        assert_eq!(fonts.phones[0].display, "04 00 00 00 01");
        assert_eq!(fonts.info_date.as_deref(), Some("2026-08-11"));
        assert_eq!(fonts.wardened_period.as_deref(), Some("du 01/03 au 01/05"));
        let abri = &f[1].1;
        assert_eq!(abri.kind, "abri");
        assert_eq!(abri.winter_access, "libre");
        assert_eq!(abri.equipment.stove, Some(true));
        assert_eq!(abri.equipment.mattresses, Some(false));
        assert_eq!(abri.phones[0].label, "mairie");
    }

    #[test]
    fn no_personal_data_in_cards() {
        let f = fiches(&ri(), &[true, true, true], &c2c());
        for (_, c) in &f {
            let found = privacy::findings(&serde_json::to_value(c).unwrap());
            assert!(found.is_empty(), "{}: {found:?}", c.id);
        }
        let all = serde_json::to_string(&f.iter().map(|x| &x.1).collect::<Vec<_>>()).unwrap();
        for leaked in [
            "0600000009",
            "06 00 00 00 02",
            "Jean",
            "Martin",
            "Dupont",
            "free.fr",
        ] {
            assert!(!all.contains(leaked), "{leaked} leaked");
        }
    }
}
