//! SRN itineraries from OpenStreetMap (OpenSnowMap `planet_pistes.osm.gz`, ODbL).
//!
//! Way classes ([`way_activity`]):
//! - `piste:type=nordic` with `piste:grooming=backcountry`: SRN (the wiki: "un-prepared
//!   itinerary, tracks are made manually by skiers");
//! - `piste:type=skitour`: ski touring (attached to an area, never seeds one);
//! - `piste:type=hike` or `snowshoe`: snowshoe itinerary (attached only).
//!
//! A route relation (`type=route`, `route=piste|ski`) is one itinerary when its
//! `piste:type` and grooming give a class, or when most of its member length is of one
//! class; the remaining classified ways are grouped into networks of connected ways.

use std::collections::{BTreeMap, HashMap, HashSet};

use skimap_common::geom::LonLat;
use skimap_osm::parse::{MemberKind, OsmData, Relation, Tags, Way};

use crate::route::{Activity, Provider, Route, Shape};

/// Licence of the OSM data.
pub const LICENCE: &str = "ODbL 1.0";
/// Attribution.
pub const ATTRIBUTION: &str = skimap_osm::extract::ATTRIBUTION;

/// Groomed nordic pistes of the extract (P2 classification: the way's or its relations'
/// `piste:type=nordic`, grooming not `backcountry`), as polylines.
pub fn groomed_lines(set: &skimap_osm::pistes::PisteSet) -> Vec<Vec<LonLat>> {
    set.pistes
        .iter()
        .filter(|p| p.is_nordic() && !p.grooming.backcountry)
        .map(|p| p.points.iter().map(|(_, q)| *q).collect::<Vec<_>>())
        .filter(|l| l.len() >= 2)
        .collect()
}

/// Class of a way from its own tags.
pub fn way_activity(t: &Tags) -> Option<Activity> {
    let backcountry = t.list_contains("piste:grooming", "backcountry");
    if t.list_contains("piste:type", "nordic") && backcountry {
        Some(Activity::Srn)
    } else if t.list_contains("piste:type", "skitour") {
        Some(Activity::SkiTouring)
    } else if t.list_contains("piste:type", "hike") || t.list_contains("piste:type", "snowshoe") {
        Some(Activity::Snowshoe)
    } else {
        None
    }
}

/// Class of a route relation from its own tags (`piste:type`, `piste:grooming`).
fn relation_activity(t: &Tags) -> Option<Activity> {
    if !t.is("type", "route") || !(t.is("route", "piste") || t.is("route", "ski")) {
        return None;
    }
    way_activity(t)
}

fn way_length_m(data: &OsmData, w: &Way) -> f64 {
    let pts = data.way_points(w);
    pts.windows(2).map(|p| p[0].1.haversine_m(p[1].1)).sum()
}

fn member_ways<'a>(data: &'a OsmData, r: &Relation, seen: &mut HashSet<i64>) -> Vec<&'a Way> {
    if !seen.insert(r.id) {
        return Vec::new();
    }
    let mut out = Vec::new();
    for m in &r.members {
        match m.kind {
            MemberKind::Way => out.extend(data.way(m.id)),
            MemberKind::Relation => {
                if let Some(sub) = data.relation(m.id) {
                    out.extend(member_ways(data, sub, seen));
                }
            }
            MemberKind::Node => {}
        }
    }
    out
}

/// Itineraries of the extract (France area of interest), every class.
pub fn routes(data: &OsmData) -> Vec<Route> {
    let mut out = Vec::new();
    let mut used: HashSet<i64> = HashSet::new();

    // Relations first, longest first so a way serves the most complete itinerary.
    let mut rels: Vec<(f64, &Relation, Activity, Vec<&Way>)> = Vec::new();
    for r in &data.relations {
        if !r.tags.is("type", "route")
            || !(r.tags.is("route", "piste") || r.tags.is("route", "ski"))
        {
            continue;
        }
        let ways = member_ways(data, r, &mut HashSet::new());
        if ways.is_empty() {
            continue;
        }
        let mut by_class: BTreeMap<Activity, f64> = BTreeMap::new();
        let mut total = 0.0;
        for w in &ways {
            let l = way_length_m(data, w);
            total += l;
            if let Some(a) = way_activity(&w.tags) {
                *by_class.entry(a).or_default() += l;
            }
        }
        let own = relation_activity(&r.tags);
        let majority = by_class
            .iter()
            .filter(|(_, l)| **l > total / 2.0)
            .map(|(a, _)| *a)
            .next();
        // A relation tagged nordic without grooming is a groomed piste even if some of
        // its ways are backcountry; its own tag decides when it has one.
        let class = match own {
            Some(a) => Some(a),
            None if r.tags.list_contains("piste:type", "nordic") => None,
            None => majority,
        };
        if let Some(a) = class
            && total > 0.0
        {
            rels.push((total, r, a, ways));
        }
    }
    rels.sort_by(|a, b| b.0.total_cmp(&a.0).then(a.1.id.cmp(&b.1.id)));
    for (_, r, activity, ways) in rels {
        let mut lines = Vec::new();
        let mut seen = HashSet::new();
        for w in ways {
            if !seen.insert(w.id) {
                continue;
            }
            used.insert(w.id);
            let pts: Vec<LonLat> = data.way_points(w).into_iter().map(|(_, p)| p).collect();
            if pts.len() >= 2 {
                lines.push(pts);
            }
        }
        if lines.is_empty() {
            continue;
        }
        let length_km = Route::lines_length_km(&lines);
        let shape = chain_shape(&lines);
        out.push(Route {
            id: format!("osm:relation/{}", r.id),
            name: r.tags.non_empty("name").map(str::to_owned),
            provider: Provider::Osm,
            publisher: "OpenStreetMap".into(),
            url: format!("https://www.openstreetmap.org/relation/{}", r.id),
            licence: LICENCE.into(),
            attribution: ATTRIBUTION.into(),
            activity,
            shape,
            lines,
            length_km,
            published_length_km: None,
            ascent_m: None,
            difficulty: r.tags.non_empty("piste:difficulty").map(str::to_owned),
        });
    }

    // Remaining classified ways: connected components per class.
    let mut free: Vec<(&Way, Activity)> = data
        .ways
        .iter()
        .filter(|w| !used.contains(&w.id))
        .filter_map(|w| way_activity(&w.tags).map(|a| (w, a)))
        .collect();
    free.sort_by_key(|(w, _)| w.id);
    let mut parent: Vec<usize> = (0..free.len()).collect();
    fn find(p: &mut [usize], mut i: usize) -> usize {
        while p[i] != i {
            p[i] = p[p[i]];
            i = p[i];
        }
        i
    }
    let mut by_node: HashMap<(i64, Activity), usize> = HashMap::new();
    for (i, (w, a)) in free.iter().enumerate() {
        for n in &w.nodes {
            match by_node.get(&(*n, *a)) {
                Some(&j) => {
                    let (ri, rj) = (find(&mut parent, i), find(&mut parent, j));
                    if ri != rj {
                        parent[ri.max(rj)] = ri.min(rj);
                    }
                }
                None => {
                    by_node.insert((*n, *a), i);
                }
            }
        }
    }
    let mut groups: BTreeMap<usize, Vec<usize>> = BTreeMap::new();
    for i in 0..free.len() {
        let r = find(&mut parent, i);
        groups.entry(r).or_default().push(i);
    }
    for members in groups.values() {
        let activity = free[members[0]].1;
        let mut lines = Vec::new();
        let mut names: BTreeMap<&str, f64> = BTreeMap::new();
        for &i in members {
            let w = free[i].0;
            let pts: Vec<LonLat> = data.way_points(w).into_iter().map(|(_, p)| p).collect();
            if pts.len() < 2 {
                continue;
            }
            if let Some(n) = w.tags.non_empty("piste:name").or(w.tags.non_empty("name")) {
                *names.entry(n).or_default() += Route::lines_length_km(std::slice::from_ref(&pts));
            }
            lines.push(pts);
        }
        if lines.is_empty() {
            continue;
        }
        let first = free[members[0]].0.id;
        let name = names
            .iter()
            .max_by(|a, b| a.1.total_cmp(b.1).then(b.0.cmp(a.0)))
            .map(|(n, _)| (*n).to_owned());
        let length_km = Route::lines_length_km(&lines);
        out.push(Route {
            id: format!("osm:way/{first}"),
            name,
            provider: Provider::Osm,
            publisher: "OpenStreetMap".into(),
            url: format!("https://www.openstreetmap.org/way/{first}"),
            licence: LICENCE.into(),
            attribution: ATTRIBUTION.into(),
            activity,
            shape: Shape::Network,
            lines,
            length_km,
            published_length_km: None,
            ascent_m: None,
            difficulty: None,
        });
    }
    out
}

/// Shape of a relation: chains its ways end to end (greedy) and compares the ends of the
/// longest chain.
fn chain_shape(lines: &[Vec<LonLat>]) -> Shape {
    if lines.len() == 1 {
        return Route::shape_from_ends(lines);
    }
    // Ends that appear once in the set of way ends are the ends of the itinerary.
    let key = |p: LonLat| ((p.lon * 1e6).round() as i64, (p.lat * 1e6).round() as i64);
    let mut count: HashMap<(i64, i64), (usize, LonLat)> = HashMap::new();
    for l in lines {
        for p in [l[0], l[l.len() - 1]] {
            count.entry(key(p)).or_insert((0, p)).0 += 1;
        }
    }
    let mut ends: Vec<LonLat> = count
        .values()
        .filter(|(n, _)| *n % 2 == 1)
        .map(|(_, p)| *p)
        .collect();
    ends.sort_by(|a, b| a.lon.total_cmp(&b.lon).then(a.lat.total_cmp(&b.lat)));
    if ends.len() < 2 {
        return Shape::Loop;
    }
    let mut far = 0.0f64;
    for (i, a) in ends.iter().enumerate() {
        for b in &ends[i + 1..] {
            far = far.max(a.haversine_m(*b));
        }
    }
    if far > crate::route::TRAVERSE_MIN_GAP_M {
        Shape::Traverse
    } else {
        Shape::Loop
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tags(kv: &[(&str, &str)]) -> Tags {
        Tags(
            kv.iter()
                .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
                .collect(),
        )
    }

    #[test]
    fn classes() {
        let t = tags(&[("piste:type", "nordic"), ("piste:grooming", "backcountry")]);
        assert_eq!(way_activity(&t), Some(Activity::Srn));
        let t = tags(&[
            ("piste:type", "nordic"),
            ("piste:grooming", "classic;skating"),
        ]);
        assert_eq!(way_activity(&t), None);
        assert_eq!(
            way_activity(&tags(&[("piste:type", "skitour")])),
            Some(Activity::SkiTouring)
        );
        assert_eq!(
            way_activity(&tags(&[
                ("piste:type", "hike"),
                ("piste:grooming", "backcountry")
            ])),
            Some(Activity::Snowshoe)
        );
        assert_eq!(way_activity(&tags(&[("piste:type", "downhill")])), None);
    }

    #[test]
    fn relation_and_network() {
        let xml = r#"<osm>
<node id="1" lat="45.0" lon="6.0"/><node id="2" lat="45.01" lon="6.0"/>
<node id="3" lat="45.02" lon="6.0"/><node id="4" lat="45.05" lon="6.0"/>
<node id="5" lat="45.3" lon="6.3"/><node id="6" lat="45.31" lon="6.3"/>
<node id="7" lat="45.31" lon="6.31"/>
<way id="10"><nd ref="1"/><nd ref="2"/><tag k="piste:type" v="nordic"/></way>
<way id="11"><nd ref="2"/><nd ref="3"/><nd ref="4"/><tag k="piste:type" v="nordic"/></way>
<way id="20"><nd ref="5"/><nd ref="6"/><tag k="piste:type" v="nordic"/><tag k="piste:grooming" v="backcountry"/><tag k="name" v="Combe"/></way>
<way id="21"><nd ref="6"/><nd ref="7"/><tag k="piste:type" v="nordic"/><tag k="piste:grooming" v="backcountry"/></way>
<relation id="100"><member type="way" ref="10" role=""/><member type="way" ref="11" role=""/>
<tag k="type" v="route"/><tag k="route" v="piste"/><tag k="piste:type" v="nordic"/><tag k="piste:grooming" v="backcountry"/><tag k="name" v="Traversée"/></relation>
</osm>"#;
        let aoi =
            skimap_osm::parse::Aoi(vec![skimap_common::geom::BBox::new(44.0, 5.0, 46.0, 7.0)]);
        let data = skimap_osm::parse::read(xml.as_bytes(), &aoi).unwrap();
        let rs = routes(&data);
        assert_eq!(rs.len(), 2, "{rs:?}");
        let r = rs.iter().find(|r| r.id == "osm:relation/100").unwrap();
        assert_eq!(r.activity, Activity::Srn);
        assert_eq!(r.shape, Shape::Traverse);
        assert!((r.length_km - 5.56).abs() < 0.05, "{}", r.length_km);
        let n = rs.iter().find(|r| r.id == "osm:way/20").unwrap();
        assert_eq!(n.shape, Shape::Network);
        assert_eq!(n.name.as_deref(), Some("Combe"));
        assert_eq!(n.lines.len(), 2);
    }
}
