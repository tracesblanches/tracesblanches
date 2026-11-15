//! From itineraries to areas: groomed share, clustering, footprints, attachment.
//!
//! All geometry is done in Lambert-93 (metres):
//! 1. every itinerary is densified ([`DENSIFY_M`]); its share of points within
//!    [`ON_PISTE_M`] of a groomed nordic piste (OSM `nordic` without `backcountry`) is its
//!    groomed share; an itinerary that is not OSM
//!    `backcountry` and runs at least [`GROOMED_DROP_SHARE`] on groomed pistes is dropped:
//!    it is a station's business. Its share inside each domain footprint of the census
//!    (P3) is kept to list the stations it touches (the footprints hold the forests
//!    between the pistes, where SRN is skied: they cannot decide);
//! 2. a seeding traverse of at least [`TRAVERSE_AREA_MIN_KM`] is a linear area
//!    (corridor of [`CORRIDOR_M`]);
//! 3. the seeding itineraries shorter than [`DOMAIN_MAX_ROUTE_KM`] are grouped when they
//!    come within [`LINK_M`] of each other; a group is a play area (its itineraries
//!    closed by [`CLOSE_M`], then [`CORRIDOR_M`] around them, [`closing_footprint`])
//!    unless it is a single traverse already kept as linear, or it holds less than
//!    [`DOMAIN_MIN_KM`] of itineraries;
//! 4. an attached-only itinerary (snowshoe, ski touring) joins the area holding at least
//!    [`ATTACH_SHARE`] of its points.

use std::collections::{BTreeMap, BTreeSet};

use geo::{Area, BooleanOps, BoundingRect, Buffer, Contains, Intersects, Simplify};
use geo_types::{Coord, LineString, MultiLineString, MultiPolygon, Point, Polygon};
use rstar::{AABB, RTree, primitives::GeomWithData};
use serde::Serialize;
use skimap_common::crs::{from_lambert93, to_lambert93};
use skimap_common::geom::LonLat;
use skimap_common::output::round7;

use crate::error::Result;
use crate::route::{Activity, Provider, Route, Shape};

/// Densification step of the itineraries, m.
pub const DENSIFY_M: f64 = 100.0;
/// An itinerary at least this much on groomed pistes is dropped (measured on the real
/// sources: GTJ à ski de fond on Camptocamp 0.72, GTJ à ski de randonnée nordique 0.47).
pub const GROOMED_DROP_SHARE: f64 = 0.6;
/// A point closer than this to a groomed piste is on the piste, m (GPS and drawing
/// offsets of the sources).
pub const ON_PISTE_M: f64 = 30.0;
/// Smallest play area, km of itineraries.
pub const DOMAIN_MIN_KM: f64 = 2.0;
/// Shortest seeding traverse kept as a linear area, km.
pub const TRAVERSE_AREA_MIN_KM: f64 = 25.0;
/// Longest itinerary that takes part in the grouping into play areas, km (longer
/// traverses cross several areas and would glue them).
pub const DOMAIN_MAX_ROUTE_KM: f64 = 60.0;
/// Two itineraries closer than this belong to the same play area, m.
pub const LINK_M: f64 = 1_500.0;
/// Buffer of the footprints, m.
pub const CORRIDOR_M: f64 = 500.0;
/// Closing distance of the play areas: gaps up to twice this between itineraries are
/// filled ([`LINK_M`] / 2), m.
pub const CLOSE_M: f64 = 750.0;
/// Share of an attached-only itinerary that must lie in an area.
pub const ATTACH_SHARE: f64 = 0.5;
/// Simplification tolerance of the footprints, m.
pub const SIMPLIFY_M: f64 = 20.0;

/// A groomed domain of the census (P3), in Lambert-93.
#[derive(Debug, Clone)]
pub struct Station {
    /// Domain id (`domaines.geojson`).
    pub id: String,
    /// Name.
    pub name: String,
    /// Footprint, Lambert-93.
    pub shape: MultiPolygon<f64>,
}

/// Kind of an area.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    /// Play area: a group of itineraries.
    Domaine,
    /// Linear traverse: one long itinerary.
    Traversee,
}

impl Kind {
    /// Identifier used in the outputs.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Domaine => "domaine",
            Self::Traversee => "traversee",
        }
    }
}

/// An itinerary with its derived figures.
#[derive(Debug, Clone)]
pub struct Placed {
    /// The itinerary.
    pub route: Route,
    /// Densified points, Lambert-93.
    pub pts: Vec<(f64, f64)>,
    /// Share of points within [`ON_PISTE_M`] of a groomed piste.
    pub groomed_share: f64,
    /// Station ids touched, with the share of points inside each footprint.
    pub stations: Vec<(String, f64)>,
    /// Why the itinerary was dropped.
    pub dropped: Option<String>,
}

/// An area.
#[derive(Debug, Clone)]
pub struct AreaGeom {
    /// Kind.
    pub kind: Kind,
    /// Seeding itineraries (indices into the placed list).
    pub seeds: Vec<usize>,
    /// Attached itineraries.
    pub attached: Vec<usize>,
    /// Footprint, Lambert-93.
    pub shape: MultiPolygon<f64>,
}

fn project_line(l: &[LonLat]) -> Result<Vec<(f64, f64)>> {
    let mut out: Vec<(f64, f64)> = Vec::with_capacity(l.len());
    for p in l {
        let q = to_lambert93(*p)?;
        if let Some(&last) = out.last() {
            let d = ((q.0 - last.0).powi(2) + (q.1 - last.1).powi(2)).sqrt();
            let n = (d / DENSIFY_M).ceil() as usize;
            for k in 1..n {
                let t = k as f64 / n as f64;
                out.push((last.0 + t * (q.0 - last.0), last.1 + t * (q.1 - last.1)));
            }
        }
        out.push(q);
    }
    Ok(out)
}

fn lines_l93(r: &Route) -> Result<MultiLineString<f64>> {
    let mut lines = Vec::new();
    for l in &r.lines {
        let mut c = Vec::with_capacity(l.len());
        for p in l {
            let (x, y) = to_lambert93(*p)?;
            c.push(Coord { x, y });
        }
        lines.push(LineString::new(c));
    }
    Ok(MultiLineString::new(lines))
}

type StationIndex = RTree<GeomWithData<rstar::primitives::Rectangle<[f64; 2]>, usize>>;

fn station_index(stations: &[Station]) -> StationIndex {
    RTree::bulk_load(
        stations
            .iter()
            .enumerate()
            .filter_map(|(i, s)| {
                let b = s.shape.bounding_rect()?;
                Some(GeomWithData::new(
                    rstar::primitives::Rectangle::from_corners(
                        [b.min().x, b.min().y],
                        [b.max().x, b.max().y],
                    ),
                    i,
                ))
            })
            .collect(),
    )
}

/// Projects, densifies and measures the groomed share of every itinerary; marks the
/// seeding itineraries to drop.
pub fn place(
    routes: Vec<Route>,
    stations: &[Station],
    groomed: &[Vec<LonLat>],
) -> Result<Vec<Placed>> {
    let index = station_index(stations);
    let mut segs = Vec::new();
    for l in groomed {
        let mut prev: Option<[f64; 2]> = None;
        for p in l {
            let (x, y) = to_lambert93(*p)?;
            if let Some(a) = prev {
                segs.push(rstar::primitives::Line::new(a, [x, y]));
            }
            prev = Some([x, y]);
        }
    }
    let pistes = RTree::bulk_load(segs);
    let mut out = Vec::with_capacity(routes.len());
    for r in routes {
        let mut pts = Vec::new();
        for l in &r.lines {
            pts.extend(project_line(l)?);
        }
        let mut on_piste = 0usize;
        let mut per: BTreeMap<usize, usize> = BTreeMap::new();
        for &(x, y) in &pts {
            let p = Point::new(x, y);
            for c in index.locate_all_at_point(&[x, y]) {
                if stations[c.data].shape.contains(&p) {
                    *per.entry(c.data).or_default() += 1;
                }
            }
            if pistes
                .locate_within_distance([x, y], ON_PISTE_M * ON_PISTE_M)
                .next()
                .is_some()
            {
                on_piste += 1;
            }
        }
        let n = pts.len().max(1) as f64;
        let groomed_share = on_piste as f64 / n;
        let mut st: Vec<(String, f64)> = per
            .into_iter()
            .map(|(i, k)| (stations[i].id.clone(), k as f64 / n))
            .collect();
        st.sort_by(|a, b| b.1.total_cmp(&a.1).then(a.0.cmp(&b.0)));
        let explicit_backcountry = r.provider == Provider::Osm && r.activity == Activity::Srn;
        let dropped = (!explicit_backcountry && groomed_share >= GROOMED_DROP_SHARE)
            .then(|| format!("{:.0} % sur des pistes damées", groomed_share * 100.0));
        out.push(Placed {
            route: r,
            pts,
            groomed_share,
            stations: st,
            dropped,
        });
    }
    Ok(out)
}

fn to_wgs84(shape: &MultiPolygon<f64>) -> Result<MultiPolygon<f64>> {
    let ring = |ls: &LineString<f64>| -> Result<LineString<f64>> {
        ls.0.iter()
            .map(|c| {
                from_lambert93(c.x, c.y).map(|p| Coord {
                    x: round7(p.lon),
                    y: round7(p.lat),
                })
            })
            .collect::<std::result::Result<Vec<_>, _>>()
            .map(LineString::new)
            .map_err(crate::error::SrnError::from)
    };
    let mut out = Vec::new();
    for poly in &shape.0 {
        let ext = ring(poly.exterior())?;
        let ints = poly
            .interiors()
            .iter()
            .map(ring)
            .collect::<Result<Vec<_>>>()?;
        out.push(Polygon::new(ext, ints));
    }
    Ok(MultiPolygon::new(out))
}

/// Footprint of a play area: the itineraries buffered by [`CLOSE_M`], then shrunk back
/// to [`CORRIDOR_M`] (a morphological closing): the gaps between itineraries narrower
/// than twice [`CLOSE_M`] are filled, the wide empty slopes between two branches are
/// not (a concave hull filled them).
/// The corridor is united back: the inward buffer of i_overlay leaves gaps on some
/// itineraries (up to 15 % of the points of a route measured outside on the real data).
pub fn closing_footprint(lines: &MultiLineString<f64>) -> MultiPolygon<f64> {
    let closed = lines.buffer(CLOSE_M).buffer(CORRIDOR_M - CLOSE_M);
    clean(closed.union(&lines.buffer(CORRIDOR_M)).simplify(SIMPLIFY_M))
}

/// Footprint of a linear area: corridor around the polylines.
pub fn corridor_footprint(lines: &MultiLineString<f64>) -> MultiPolygon<f64> {
    clean(lines.buffer(CORRIDOR_M).simplify(SIMPLIFY_M))
}

/// Smallest part kept in a footprint, m² (the buffers leave slivers).
pub const MIN_PART_M2: f64 = 10_000.0;
/// Smallest hole kept in a footprint, m² (an itinerary that doubles back leaves small
/// holes in its corridor).
pub const MIN_HOLE_M2: f64 = 1_000_000.0;

/// Drops the slivers and the small holes of a footprint.
pub fn clean(mp: MultiPolygon<f64>) -> MultiPolygon<f64> {
    MultiPolygon::new(
        mp.0.into_iter()
            .filter(|p| Polygon::new(p.exterior().clone(), vec![]).unsigned_area() >= MIN_PART_M2)
            .map(|p| {
                let (ext, holes) = p.into_inner();
                let holes = holes
                    .into_iter()
                    .filter(|h| Polygon::new(h.clone(), vec![]).unsigned_area() >= MIN_HOLE_M2)
                    .collect();
                Polygon::new(ext, holes)
            })
            .collect(),
    )
}

fn find(p: &mut [usize], mut i: usize) -> usize {
    while p[i] != i {
        p[i] = p[p[i]];
        i = p[i];
    }
    i
}

/// Groups the kept seeding itineraries into areas and attaches the others.
pub fn build_areas(placed: &[Placed]) -> Result<Vec<AreaGeom>> {
    let seeds: Vec<usize> = (0..placed.len())
        .filter(|&i| placed[i].route.activity.seeds() && placed[i].dropped.is_none())
        .collect();
    let mut areas: Vec<AreaGeom> = Vec::new();
    let mut linear: BTreeSet<usize> = BTreeSet::new();
    for &i in &seeds {
        let r = &placed[i].route;
        if r.shape == Shape::Traverse && r.length_km >= TRAVERSE_AREA_MIN_KM {
            linear.insert(i);
            areas.push(AreaGeom {
                kind: Kind::Traversee,
                seeds: vec![i],
                attached: Vec::new(),
                shape: corridor_footprint(&lines_l93(r)?),
            });
        }
    }
    // Grouping.
    let members: Vec<usize> = seeds
        .iter()
        .copied()
        .filter(|&i| placed[i].route.length_km < DOMAIN_MAX_ROUTE_KM)
        .collect();
    let tree: RTree<GeomWithData<[f64; 2], usize>> = RTree::bulk_load(
        members
            .iter()
            .enumerate()
            .flat_map(|(k, &i)| {
                placed[i]
                    .pts
                    .iter()
                    .map(move |&(x, y)| GeomWithData::new([x, y], k))
            })
            .collect(),
    );
    let mut parent: Vec<usize> = (0..members.len()).collect();
    for (k, &i) in members.iter().enumerate() {
        for &(x, y) in &placed[i].pts {
            let env = AABB::from_corners([x - LINK_M, y - LINK_M], [x + LINK_M, y + LINK_M]);
            for q in tree.locate_in_envelope(&env) {
                let j = q.data;
                if j == k {
                    continue;
                }
                let [qx, qy] = *q.geom();
                if (qx - x).powi(2) + (qy - y).powi(2) <= LINK_M * LINK_M {
                    let (a, b) = (find(&mut parent, k), find(&mut parent, j));
                    if a != b {
                        parent[a.max(b)] = a.min(b);
                    }
                }
            }
        }
    }
    let mut groups: BTreeMap<usize, Vec<usize>> = BTreeMap::new();
    for (k, &m) in members.iter().enumerate() {
        let r = find(&mut parent, k);
        groups.entry(r).or_default().push(m);
    }
    for g in groups.into_values() {
        let km: f64 = g.iter().map(|&i| placed[i].route.length_km).sum();
        if (g.len() == 1 && linear.contains(&g[0])) || km < DOMAIN_MIN_KM {
            continue;
        }
        let mut lines = Vec::new();
        for &i in &g {
            lines.extend(lines_l93(&placed[i].route)?.0);
        }
        let shape = closing_footprint(&MultiLineString::new(lines));
        areas.push(AreaGeom {
            kind: Kind::Domaine,
            seeds: g,
            attached: Vec::new(),
            shape,
        });
    }
    // Attachment.
    for (i, p) in placed.iter().enumerate() {
        if p.route.activity.seeds() || p.pts.is_empty() || p.dropped.is_some() {
            continue;
        }
        let mut best: Option<(f64, Kind, usize)> = None;
        for (a, area) in areas.iter().enumerate() {
            let Some(b) = area.shape.bounding_rect() else {
                continue;
            };
            let inside = p
                .pts
                .iter()
                .filter(|&&(x, y)| {
                    x >= b.min().x && x <= b.max().x && y >= b.min().y && y <= b.max().y
                })
                .filter(|&&(x, y)| area.shape.contains(&Point::new(x, y)))
                .count();
            let share = inside as f64 / p.pts.len() as f64;
            if share >= ATTACH_SHARE {
                let better = match best {
                    None => true,
                    // Play areas first, then the larger share.
                    Some((s, k, _)) => (area.kind, -share) < (k, -s),
                };
                if better {
                    best = Some((share, area.kind, a));
                }
            }
        }
        if let Some((_, _, a)) = best {
            areas[a].attached.push(i);
        }
    }
    Ok(areas)
}

/// Overlap of an area with a groomed domain.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct StationOverlap {
    /// Domain id.
    pub id: String,
    /// Domain name.
    pub name: String,
    /// Share of the area footprint inside the domain footprint.
    pub area_share: f64,
    /// Seeding itinerary length inside the domain, km.
    pub km_inside: f64,
}

/// Groomed domains overlapping an area footprint.
pub fn station_overlaps(
    area: &AreaGeom,
    placed: &[Placed],
    stations: &[Station],
) -> Vec<StationOverlap> {
    let total = area.shape.unsigned_area().max(1.0);
    let mut out = Vec::new();
    for s in stations {
        if !s.shape.intersects(&area.shape) {
            continue;
        }
        let inter = s.shape.intersection(&area.shape).unsigned_area();
        let km_inside: f64 = area
            .seeds
            .iter()
            .map(|&i| {
                let share = placed[i]
                    .stations
                    .iter()
                    .find(|(id, _)| *id == s.id)
                    .map_or(0.0, |(_, f)| *f);
                share * placed[i].route.length_km
            })
            .sum();
        out.push(StationOverlap {
            id: s.id.clone(),
            name: s.name.clone(),
            area_share: inter / total,
            km_inside,
        });
    }
    out.sort_by(|a, b| b.area_share.total_cmp(&a.area_share).then(a.id.cmp(&b.id)));
    out
}

/// Footprint in WGS84 (rounded to 1e-7°).
pub fn footprint_wgs84(area: &AreaGeom) -> Result<MultiPolygon<f64>> {
    to_wgs84(&area.shape)
}

/// Point of the area nearest to the centroid of its seeding points (WGS84).
pub fn representative(area: &AreaGeom, placed: &[Placed]) -> Result<LonLat> {
    let pts: Vec<(f64, f64)> = area
        .seeds
        .iter()
        .flat_map(|&i| placed[i].pts.iter().copied())
        .collect();
    let n = pts.len().max(1) as f64;
    let (cx, cy) = pts
        .iter()
        .fold((0.0, 0.0), |(a, b), &(x, y)| (a + x / n, b + y / n));
    let best = pts
        .iter()
        .min_by(|a, b| {
            let da = (a.0 - cx).powi(2) + (a.1 - cy).powi(2);
            let db = (b.0 - cx).powi(2) + (b.1 - cy).powi(2);
            da.total_cmp(&db)
        })
        .copied()
        .unwrap_or((cx, cy));
    Ok(from_lambert93(best.0, best.1)?)
}

/// Points to locate an area (WGS84): the representative point, then points at 0, 25, 50,
/// 75 and 100 % of the longest seeding itinerary.
pub fn sample_points(area: &AreaGeom, placed: &[Placed]) -> Result<Vec<LonLat>> {
    let mut out = vec![representative(area, placed)?];
    if let Some(&i) = area.seeds.iter().max_by(|&&a, &&b| {
        placed[a]
            .route
            .length_km
            .total_cmp(&placed[b].route.length_km)
    }) {
        let pts = &placed[i].pts;
        for k in 0..=4 {
            if let Some(&(x, y)) = pts.get(k * pts.len().saturating_sub(1) / 4) {
                out.push(from_lambert93(x, y)?);
            }
        }
    }
    Ok(out)
}

/// Station footprints from `domaines.geojson` features (WGS84 MultiPolygon).
pub fn stations_from_geojson(v: &serde_json::Value) -> Result<Vec<Station>> {
    let mut out = Vec::new();
    for f in v
        .get("features")
        .and_then(|f| f.as_array())
        .into_iter()
        .flatten()
    {
        let p = &f["properties"];
        let id = p["id"].as_str().unwrap_or_default().to_owned();
        let name = p["name"].as_str().unwrap_or_default().to_owned();
        let g = &f["geometry"];
        let polys: Vec<&serde_json::Value> = match g["type"].as_str() {
            Some("Polygon") => vec![&g["coordinates"]],
            Some("MultiPolygon") => g["coordinates"].as_array().into_iter().flatten().collect(),
            _ => continue,
        };
        let mut mp = Vec::new();
        for poly in polys {
            let mut rings = Vec::new();
            for ring in poly.as_array().into_iter().flatten() {
                let mut c = Vec::new();
                for q in ring.as_array().into_iter().flatten() {
                    if let (Some(x), Some(y)) = (q[0].as_f64(), q[1].as_f64()) {
                        let (x, y) = to_lambert93(LonLat::new(x, y))?;
                        c.push(Coord { x, y });
                    }
                }
                rings.push(LineString::new(c));
            }
            if rings.is_empty() {
                continue;
            }
            let ext = rings.remove(0);
            mp.push(Polygon::new(ext, rings));
        }
        out.push(Station {
            id,
            name,
            shape: MultiPolygon::new(mp),
        });
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn route(id: &str, activity: Activity, line: Vec<LonLat>) -> Route {
        let length_km = Route::lines_length_km(std::slice::from_ref(&line));
        Route {
            id: id.into(),
            name: Some(id.into()),
            provider: Provider::Camptocamp,
            publisher: "test".into(),
            url: String::new(),
            licence: String::new(),
            attribution: String::new(),
            activity,
            shape: Route::shape_from_ends(std::slice::from_ref(&line)),
            lines: vec![line],
            length_km,
            published_length_km: None,
            ascent_m: None,
            difficulty: None,
        }
    }

    fn seg(lon: f64, lat: f64, dlat: f64) -> Vec<LonLat> {
        vec![LonLat::new(lon, lat), LonLat::new(lon, lat + dlat)]
    }

    #[test]
    fn grouping_traverse_and_attachment() {
        let routes = vec![
            // Two loops-ish 1 km apart: one play area.
            route("a", Activity::Srn, seg(6.0, 45.0, 0.02)),
            route("b", Activity::EasySkiTouring, seg(6.012, 45.0, 0.02)),
            // Far away: its own area.
            route("c", Activity::Srn, seg(6.5, 45.0, 0.02)),
            // A 33 km traverse: a linear area, and not a play area on its own.
            route("t", Activity::Srn, seg(7.0, 45.0, 0.3)),
            // Snowshoe inside the first area: attached; far one: not.
            route("s", Activity::Snowshoe, seg(6.006, 45.005, 0.01)),
            route("x", Activity::Snowshoe, seg(5.0, 45.0, 0.01)),
            // 0.6 km alone: too small for a play area.
            route("z", Activity::Srn, seg(8.0, 45.0, 0.005)),
        ];
        let placed = place(routes, &[], &[]).unwrap();
        let areas = build_areas(&placed).unwrap();
        let kinds: Vec<(Kind, Vec<&str>)> = areas
            .iter()
            .map(|a| {
                (
                    a.kind,
                    a.seeds
                        .iter()
                        .map(|&i| placed[i].route.id.as_str())
                        .collect(),
                )
            })
            .collect();
        assert_eq!(kinds.len(), 3, "{kinds:?}");
        assert!(kinds.contains(&(Kind::Traversee, vec!["t"])));
        assert!(kinds.contains(&(Kind::Domaine, vec!["a", "b"])));
        assert!(kinds.contains(&(Kind::Domaine, vec!["c"])));
        let first = areas.iter().find(|a| a.seeds.len() == 2).unwrap();
        assert_eq!(first.attached.len(), 1);
        assert_eq!(placed[first.attached[0]].route.id, "s");
        let rep = representative(first, &placed).unwrap();
        assert!((rep.lat - 45.01).abs() < 0.002, "{rep:?}");
        let w = footprint_wgs84(first).unwrap();
        assert!(w.contains(&Point::new(6.006, 45.01)));
    }

    fn square(x: f64, y: f64, side: f64) -> LineString<f64> {
        LineString::new(vec![
            Coord { x, y },
            Coord { x: x + side, y },
            Coord {
                x: x + side,
                y: y + side,
            },
            Coord { x, y: y + side },
            Coord { x, y },
        ])
    }

    #[test]
    fn clean_drops_slivers_and_small_holes() {
        let big = Polygon::new(
            square(0.0, 0.0, 5_000.0),
            vec![
                square(100.0, 100.0, 50.0),
                square(1_000.0, 1_000.0, 2_000.0),
            ],
        );
        let sliver = Polygon::new(square(10_000.0, 0.0, 50.0), vec![]);
        let c = clean(MultiPolygon::new(vec![big, sliver]));
        assert_eq!(c.0.len(), 1);
        assert_eq!(c.0[0].interiors().len(), 1, "only the 4 km² hole is kept");
    }

    #[test]
    fn groomed_share_drops_seeds() {
        let (x0, y0) = to_lambert93(LonLat::new(5.99, 44.99)).unwrap();
        let (x1, y1) = to_lambert93(LonLat::new(6.03, 45.05)).unwrap();
        let st = Station {
            id: "d1".into(),
            name: "Station".into(),
            shape: MultiPolygon::new(vec![Polygon::new(
                LineString::new(vec![
                    Coord { x: x0, y: y0 },
                    Coord { x: x1, y: y0 },
                    Coord { x: x1, y: y1 },
                    Coord { x: x0, y: y1 },
                    Coord { x: x0, y: y0 },
                ]),
                vec![],
            )]),
        };
        let mut osm = route("o", Activity::Srn, seg(6.0, 45.0, 0.02));
        osm.provider = Provider::Osm;
        let routes = vec![
            route("a", Activity::Srn, seg(6.0, 45.0, 0.02)),
            osm,
            // Inside the station footprint but 400 m off the piste: kept.
            route("f", Activity::Srn, seg(6.005, 45.0, 0.02)),
        ];
        // The groomed piste, drawn 10 m east of the first itinerary.
        let piste = seg(6.000127, 45.0, 0.02);
        let placed = place(routes, std::slice::from_ref(&st), &[piste]).unwrap();
        assert!(placed[0].groomed_share > 0.99);
        assert!(placed[0].dropped.is_some());
        assert!(placed[1].dropped.is_none(), "OSM backcountry is kept");
        assert_eq!(placed[2].groomed_share, 0.0);
        assert!(
            placed[2].dropped.is_none(),
            "off-piste in a station is kept"
        );
        assert!(placed[2].stations[0].1 > 0.99);
        let areas = build_areas(&placed).unwrap();
        assert_eq!(areas.len(), 1);
        let ov = station_overlaps(&areas[0], &placed, std::slice::from_ref(&st));
        assert_eq!(ov.len(), 1);
        assert!(ov[0].km_inside > 2.0 && ov[0].area_share > 0.5, "{ov:?}");
    }
}
