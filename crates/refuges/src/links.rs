//! Cross-references between the three sources, as identifiers only.
//!
//! The sources stay in separate layers (refuges.info and Camptocamp under CC BY-SA, OSM
//! under ODbL; licence compatibility, ticket L3, is still open): this table only says which
//! records describe the same place, so that the app can show them together.
//!
//! - OSM → refuges.info: the `ref:refuges.info` tag (exact).
//! - Camptocamp → refuges.info, Camptocamp → OSM, and OSM without tag → refuges.info:
//!   nearest record within [`NEAR_M`] whose name shares words, or nearest within
//!   [`SAME_SPOT_M`] whatever the name.

use std::collections::{BTreeMap, BTreeSet};

use serde::Serialize;
use skimap_common::geom::LonLat;
use skimap_osm::census::{jaccard, name_tokens};

use crate::camptocamp::Waypoint;
use crate::osm::Hut;
use crate::refuges_info::Point;

/// Largest distance for a match by position and name, m.
pub const NEAR_M: f64 = 250.0;
/// Largest distance for a match by position alone, m.
pub const SAME_SPOT_M: f64 = 40.0;
/// Smallest name similarity (Jaccard of the words) for a match within [`NEAR_M`].
pub const MIN_NAME: f64 = 0.3;

/// One place seen by several sources.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct Link {
    /// refuges.info point id.
    pub refuges_info: Option<u64>,
    /// OSM elements (`node/123`).
    pub osm: Vec<String>,
    /// Camptocamp document ids.
    pub camptocamp: Vec<u64>,
    /// How each member was attached: `ref:refuges.info`, `position+name`, `position`.
    pub methods: Vec<String>,
}

/// Words of a hut name, without the generic words (refuge, cabane, abri...).
pub fn hut_tokens(name: &str) -> BTreeSet<String> {
    const GENERIC: &[&str] = &[
        "refuge", "refuges", "cabane", "abri", "abris", "chalet", "gite", "etape", "hutte", "hut",
        "refugio", "rifugio", "baita", "bivouac", "bivacco", "cabanon", "loge", "cab", "auberge",
        "buvette", "metairie", "capanna",
    ];
    name_tokens(name)
        .into_iter()
        .filter(|t| !GENERIC.contains(&t.as_str()))
        .collect()
}

/// Matching method between two records and a sort key (smaller is better), or `None`.
pub fn same_place(a: (&str, LonLat), b: (&str, LonLat)) -> Option<(&'static str, f64)> {
    let d = a.1.haversine_m(b.1);
    let sim = jaccard(&hut_tokens(a.0), &hut_tokens(b.0));
    if d <= NEAR_M && sim >= MIN_NAME {
        // Name similarity first, distance second.
        Some(("position+name", (1.0 - sim) * 1000.0 + d))
    } else if d <= SAME_SPOT_M {
        Some(("position", 2000.0 + d))
    } else {
        None
    }
}

fn best<'a, T>(
    target: (&str, LonLat),
    cands: impl Iterator<Item = (&'a T, &'a str, LonLat)>,
) -> Option<(&'a T, &'static str)>
where
    T: 'a,
{
    cands
        .filter_map(|(t, name, pos)| same_place(target, (name, pos)).map(|(m, k)| (t, m, k)))
        .min_by(|a, b| a.2.total_cmp(&b.2))
        .map(|(t, m, _)| (t, m))
}

/// refuges.info types that are not shelters (water points, difficult passages) are never
/// matched by position.
pub const NOT_SHELTER_TYPES: &[u64] = &[3, 23];

fn shelters(points: &[Point]) -> impl Iterator<Item = &Point> {
    points
        .iter()
        .filter(|p| !p.type_id.is_some_and(|t| NOT_SHELTER_TYPES.contains(&t)))
}

fn osm_key(h: &Hut) -> String {
    format!("{}/{}", h.osm_type, h.osm_id)
}

fn osm_name(h: &Hut) -> &str {
    h.tags.get("name").map(String::as_str).unwrap_or_default()
}

/// Builds the cross-reference table.
pub fn link(points: &[Point], huts: &[Hut], waypoints: &[Waypoint]) -> Vec<Link> {
    let mut by_ri: BTreeMap<u64, Link> = BTreeMap::new();
    let mut osm_linked: BTreeMap<String, u64> = BTreeMap::new();

    // OSM tag, exact.
    for h in huts {
        if let Some(ri) = h.refuges_info_id {
            let l = by_ri.entry(ri).or_default();
            l.refuges_info = Some(ri);
            l.osm.push(osm_key(h));
            l.methods
                .push(format!("osm {} ref:refuges.info", osm_key(h)));
            osm_linked.insert(osm_key(h), ri);
        }
    }
    // OSM huts without tag, by position (bus shelters excluded).
    for h in huts {
        if h.refuges_info_id.is_some() || h.kind == "bus_shelter" {
            continue;
        }
        let cands = shelters(points).map(|p| (p, p.name.as_str(), p.pos));
        if let Some((p, m)) = best((osm_name(h), h.pos), cands) {
            let l = by_ri.entry(p.id).or_default();
            l.refuges_info = Some(p.id);
            l.osm.push(osm_key(h));
            l.methods.push(format!("osm {} {m}", osm_key(h)));
            osm_linked.insert(osm_key(h), p.id);
        }
    }
    // Camptocamp: refuges.info first, else OSM.
    let mut others: Vec<Link> = Vec::new();
    for w in waypoints {
        let target = (w.title.as_str(), w.pos);
        if let Some((p, m)) = best(
            target,
            shelters(points).map(|p| (p, p.name.as_str(), p.pos)),
        ) {
            let l = by_ri.entry(p.id).or_default();
            l.refuges_info = Some(p.id);
            l.camptocamp.push(w.id);
            l.methods.push(format!("camptocamp {} {m}", w.id));
            continue;
        }
        let cands = huts
            .iter()
            .filter(|h| h.kind != "bus_shelter")
            .map(|h| (h, osm_name(h), h.pos));
        if let Some((h, m)) = best(target, cands) {
            let key = osm_key(h);
            if let Some(ri) = osm_linked.get(&key) {
                let l = by_ri.entry(*ri).or_default();
                l.camptocamp.push(w.id);
                l.methods
                    .push(format!("camptocamp {} {m} via osm {key}", w.id));
            } else if let Some(l) = others.iter_mut().find(|l| l.osm.contains(&key)) {
                l.camptocamp.push(w.id);
                l.methods.push(format!("camptocamp {} {m}", w.id));
            } else {
                others.push(Link {
                    refuges_info: None,
                    osm: vec![key],
                    camptocamp: vec![w.id],
                    methods: vec![format!("camptocamp {} {m}", w.id)],
                });
            }
        }
    }
    let mut out: Vec<Link> = by_ri.into_values().collect();
    out.extend(others);
    for l in &mut out {
        l.osm.sort();
        l.osm.dedup();
        l.camptocamp.sort_unstable();
        l.camptocamp.dedup();
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap as Map;

    fn point(id: u64, name: &str, lon: f64, lat: f64) -> Point {
        Point {
            id,
            name: name.to_owned(),
            type_id: Some(7),
            type_label: None,
            pos: LonLat::new(lon, lat),
            alt: None,
            state: None,
            places: None,
            places_mattress: None,
            stove: None,
            fireplace: None,
            forest_100m: None,
            water_100m: None,
            blankets: None,
            latrines: None,
            missing_wall: None,
            url: None,
            modified: None,
            access: None,
            remark: None,
            owner: None,
            last_comment: None,
        }
    }

    fn hut(id: i64, name: &str, kind: &str, ri: Option<u64>, lon: f64, lat: f64) -> Hut {
        let mut tags = Map::new();
        tags.insert("name".to_owned(), name.to_owned());
        Hut {
            osm_type: "node".to_owned(),
            osm_id: id,
            pos: LonLat::new(lon, lat),
            kind: kind.to_owned(),
            refuges_info_id: ri,
            tags,
        }
    }

    fn wp(id: u64, title: &str, lon: f64, lat: f64) -> Waypoint {
        Waypoint {
            id,
            title: title.to_owned(),
            waypoint_type: "hut".to_owned(),
            pos: LonLat::new(lon, lat),
            elevation: None,
            country: None,
            capacity_unstaffed: None,
            capacity_staffed: None,
            custodianship: None,
            mattress_unstaffed: None,
            blanket_unstaffed: None,
            gas_unstaffed: None,
            heating_unstaffed: None,
            access_period: None,
            quality: None,
            url: String::new(),
            licence: None,
            attribution_url: None,
        }
    }

    #[test]
    fn links_by_tag_position_and_name() {
        // 0.001° of latitude ≈ 111 m.
        let mut fountain = point(7845, "Fontaine des Fonts de Cervières", 6.8160, 44.8515);
        fountain.type_id = Some(23);
        let points = vec![
            fountain,
            point(1428, "Refuge des Fonts de Cervières", 6.8158, 44.8513),
            point(4215, "Cabane de la Gardère", 6.7147, 44.7730),
        ];
        let huts = vec![
            hut(
                1,
                "Refuge des Fonts",
                "alpine_hut",
                Some(1428),
                6.8159,
                44.8514,
            ),
            hut(
                2,
                "Cabane de la Gardère",
                "wilderness_hut",
                None,
                6.7147,
                44.7735,
            ),
            hut(3, "Abri bus", "bus_shelter", None, 6.7147, 44.7730),
            hut(4, "Chalet isolé", "shelter", None, 6.9, 44.9),
        ];
        let wps = vec![
            wp(162324, "Les Fonts de Cervières", 6.8160, 44.8515),
            wp(9, "Gîte du Plan", 6.7147, 44.7731),
            wp(7, "Chalet isolé", 6.9001, 44.9001),
            wp(8, "Ailleurs", 7.5, 45.5),
        ];
        let l = link(&points, &huts, &wps);
        assert_eq!(l.len(), 3);
        let fonts = l.iter().find(|x| x.refuges_info == Some(1428)).unwrap();
        assert_eq!(fonts.osm, vec!["node/1".to_owned()]);
        assert_eq!(fonts.camptocamp, vec![162324]);
        assert!(
            fonts
                .methods
                .iter()
                .any(|m| m.ends_with("ref:refuges.info"))
        );
        assert!(
            l.iter().all(|x| x.refuges_info != Some(7845)),
            "water point never linked"
        );
        let gardere = l.iter().find(|x| x.refuges_info == Some(4215)).unwrap();
        assert_eq!(
            gardere.osm,
            vec!["node/2".to_owned()],
            "bus shelter not linked"
        );
        // Different name, 11 m away: same spot.
        assert_eq!(gardere.camptocamp, vec![9]);
        let chalet = l.iter().find(|x| x.refuges_info.is_none()).unwrap();
        assert_eq!(chalet.osm, vec!["node/4".to_owned()]);
        assert_eq!(chalet.camptocamp, vec![7]);
    }

    #[test]
    fn same_place_rules() {
        let a = LonLat::new(6.0, 45.0);
        let near = LonLat::new(6.0, 45.0 + 200.0 / 111_195.0);
        let spot = LonLat::new(6.0, 45.0 + 20.0 / 111_195.0);
        assert!(same_place(("Refuge de la Blanche", a), ("Blanche refuge", near)).is_some());
        assert!(same_place(("Refuge de la Blanche", a), ("Chalet", near)).is_none());
        assert_eq!(
            same_place(("Refuge de la Blanche", a), ("Chalet", spot))
                .unwrap()
                .0,
            "position"
        );
    }
}
