//! Starts of the SRN areas and traverses (X9b, #143): where a skier arriving by car
//! begins a ski de randonnée nordique outing outside the groomed domains.
//!
//! Same method as the domains ([`crate::departs`]), on other inputs, for every area alike:
//! - network: the seed routes of the area (`srn/srn_routes.geojson`, routes whose `areas`
//!   lists the area with the role `seed`), nodes shared by two parts of a route (same
//!   coordinates) joined, so that the dead ends are the ends of the routes;
//! - points: the OSM points of the local Geofabrik index (`skimap_osm::local`, the same as
//!   the station cards) in the footprint box plus [`NEAR_MARGIN_DEG`]: car parks not private
//!   and open in winter ([`winter_closed`]), nordic lodges, pass desks not of the alpine lifts ([`srn_kind`]), kept
//!   when within the distances of [`crate::departs::kind_rule`] of a route;
//! - every cluster is kept in `departs.json` (`srn`): a traverse of 100 km has starts all
//!   along; the registry gives each site or section the best ones inside its box
//!   ([`pick`]).
//!
//! Without any such point: the route point nearest to a car park, lodge or desk within
//! [`crate::departs::NEAR_MAX_M`] (`near`), else the end of a route nearest to the village
//! (tourist office or ski school within [`VILLAGE_MARGIN_DEG`]), else the beginning of the
//! longest route (`route_end`), else the representative point of the area (`centre`).

use std::collections::BTreeMap;

use serde_json::Value;
use skimap_common::geom::{BBox, LonLat};
use skimap_common::output::round7;
use skimap_common::site::Start;

use crate::departs::{
    Depart, MAX_STARTS, MIN_SHARE, Method, NEAR_MAX_M, Net, Place, PlaceKind, Point, Way, d2,
    kind_rule, number, private,
};
use crate::domains::Local;
use crate::error::{Result, StationsError};

/// France layer of the SRN areas (`skimap_srn::build::LAYER`).
pub const SRN_LAYER: &str = "srn";
/// SRN areas and traverses (`skimap_srn::build::AREAS_FILE`).
pub const AREAS_FILE: &str = "srn_areas.geojson";
/// SRN routes (`skimap_srn::build::ROUTES_FILE`).
pub const ROUTES_FILE: &str = "srn_routes.geojson";
/// Margin of the query of the near points (car parks, lodges, desks) around the footprint
/// box, degrees (about 1.3 km, beyond [`NEAR_MAX_M`]).
pub const NEAR_MARGIN_DEG: f64 = 0.012;
/// Margin of the query of the villages (tourist offices, ski schools), degrees (about 6 km).
pub const VILLAGE_MARGIN_DEG: f64 = 0.055;
/// Largest number of starts kept per area in `departs.json` (a traverse of 100 km).
pub const MAX_AREA_STARTS: usize = 40;
/// Bonus of a car park said to be cleared in winter (`winter_service=yes`,
/// `seasonal=winter`).
pub const WINTER_BONUS: f64 = 1.0;

/// An SRN area or traverse of `srn_areas.geojson`.
#[derive(Debug, Clone)]
pub struct Area {
    /// Id (`srn-domaine-…`, `srn-traversee-…`), the id of its site in the registry.
    pub id: String,
    /// Name and kind.
    pub place: Place,
    /// Bounding box of the footprint.
    pub bbox: BBox,
}

fn walk(v: &Value, f: &mut impl FnMut(LonLat)) {
    match v.as_array() {
        Some(a) if a.len() >= 2 && a[0].is_number() => {
            if let (Some(lon), Some(lat)) = (a[0].as_f64(), a[1].as_f64()) {
                f(LonLat::new(lon, lat));
            }
        }
        Some(a) => a.iter().for_each(|x| walk(x, f)),
        None => {}
    }
}

/// Reads `srn_areas.geojson`.
pub fn read_areas(v: &Value) -> Result<Vec<Area>> {
    let feats = v
        .get("features")
        .and_then(Value::as_array)
        .ok_or_else(|| StationsError::format("srn_areas.geojson", "no features"))?;
    let mut out = Vec::new();
    for f in feats {
        let p = &f["properties"];
        let id = p["id"]
            .as_str()
            .ok_or_else(|| StationsError::format("srn_areas.geojson", "area without id"))?
            .to_owned();
        let kind = match p["kind"].as_str() {
            Some("domaine") => PlaceKind::SrnArea,
            Some("traversee") => PlaceKind::Traverse,
            other => {
                return Err(StationsError::format(
                    "srn_areas.geojson",
                    format!("{id}: kind {other:?}"),
                ));
            }
        };
        let (mut s, mut w, mut n, mut e) = (90.0f64, 180.0f64, -90.0f64, -180.0f64);
        walk(&f["geometry"]["coordinates"], &mut |q| {
            s = s.min(q.lat);
            n = n.max(q.lat);
            w = w.min(q.lon);
            e = e.max(q.lon);
        });
        if s > n {
            return Err(StationsError::format(
                "srn_areas.geojson",
                format!("{id}: empty footprint"),
            ));
        }
        let point = match (p["lon"].as_f64(), p["lat"].as_f64()) {
            (Some(lon), Some(lat)) => LonLat::new(lon, lat),
            _ => LonLat::new(f64::midpoint(w, e), f64::midpoint(s, n)),
        };
        out.push(Area {
            place: Place {
                kind,
                name: p["name"].as_str().unwrap_or(&id).to_owned(),
                point,
            },
            id,
            bbox: BBox::new(s, w, n, e),
        });
    }
    Ok(out)
}

/// Node id of a route point: its coordinates to 1e-7° (OSM precision), so that the parts
/// of a route sharing a node are joined.
fn node_id(p: LonLat) -> i64 {
    let lon = (p.lon * 1e7).round() as i64;
    let lat = (p.lat * 1e7).round() as i64;
    lon.wrapping_mul(2_000_000_007).wrapping_add(lat)
}

/// Seed routes of each area, from `srn_routes.geojson`: area id → ways (one per part of
/// each route, named after the route).
pub fn read_routes(v: &Value) -> BTreeMap<String, Vec<Way>> {
    let mut out: BTreeMap<String, Vec<Way>> = BTreeMap::new();
    for f in v["features"].as_array().into_iter().flatten() {
        let p = &f["properties"];
        let name = p["name"]
            .as_str()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_owned);
        let g = &f["geometry"];
        let parts: Vec<&Value> = match g["type"].as_str() {
            Some("LineString") => vec![&g["coordinates"]],
            Some("MultiLineString") => g["coordinates"].as_array().into_iter().flatten().collect(),
            _ => Vec::new(),
        };
        let ways: Vec<Way> = parts
            .into_iter()
            .map(|part| {
                let mut points = Vec::new();
                walk(part, &mut |q| points.push((node_id(q), q)));
                Way {
                    name: name.clone(),
                    points,
                }
            })
            .filter(|w| !w.points.is_empty())
            .collect();
        for a in p["areas"].as_array().into_iter().flatten() {
            if a["role"].as_str() == Some("seed")
                && let Some(id) = a["id"].as_str()
            {
                out.entry(id.to_owned())
                    .or_default()
                    .extend(ways.iter().cloned());
            }
        }
    }
    out
}

/// Whether OSM says a car park is not usable in winter: `seasonal=summer|yes`,
/// `winter_service=no`, or an `access:conditional` / `motor_vehicle:conditional` closing it
/// in winter (« no @ … » naming winter or a month from December to March).
pub fn winter_closed(get: impl Fn(&str) -> Option<String>) -> bool {
    if matches!(get("seasonal").as_deref(), Some("summer" | "yes"))
        || get("winter_service").as_deref() == Some("no")
    {
        return true;
    }
    ["access:conditional", "motor_vehicle:conditional"]
        .iter()
        .filter_map(|k| get(k))
        .any(|c| {
            let c = c.to_lowercase();
            c.contains("no @")
                && ["winter", "dec", "jan", "feb", "mar", "nov-", "oct-"]
                    .iter()
                    .any(|w| c.contains(w))
        })
}

/// Whether OSM says a car park is cleared in winter.
pub fn winter_open(get: impl Fn(&str) -> Option<String>) -> bool {
    get("winter_service").as_deref() == Some("yes") || get("seasonal").as_deref() == Some("winter")
}

/// Points of an answer of the local index (Overpass shape): the kinds of
/// [`crate::pois::classify`], car parks closed in winter left out (counted in the second
/// value), car parks open in winter marked (`winter_open`).
pub fn read_index_points(v: &Value) -> (Vec<(Point, bool)>, usize) {
    let mut out = Vec::new();
    let mut closed = 0;
    for e in v["elements"].as_array().into_iter().flatten() {
        let t: BTreeMap<String, String> = e["tags"]
            .as_object()
            .map(|m| {
                m.iter()
                    .filter_map(|(k, v)| v.as_str().map(|s| (k.clone(), s.trim().to_owned())))
                    .collect()
            })
            .unwrap_or_default();
        let Some((_, kind)) = crate::pois::classify(&t) else {
            continue;
        };
        let (Some(lat), Some(lon)) = (
            e["lat"].as_f64().or_else(|| e["center"]["lat"].as_f64()),
            e["lon"].as_f64().or_else(|| e["center"]["lon"].as_f64()),
        ) else {
            continue;
        };
        let get = |k: &str| t.get(k).filter(|s| !s.is_empty()).cloned();
        if kind == "parking" && winter_closed(get) {
            closed += 1;
            continue;
        }
        out.push((
            Point {
                kind: kind.to_owned(),
                name: get("name"),
                pos: LonLat::new(lon, lat),
                capacity: get("capacity").and_then(|c| c.parse().ok()),
                access: get("access"),
                url: format!(
                    "https://www.openstreetmap.org/{}/{}",
                    e["type"].as_str().unwrap_or("node"),
                    e["id"].as_i64().unwrap_or_default()
                ),
                domains: Vec::new(),
                bonus: 0.0,
            },
            kind == "parking" && winter_open(get),
        ));
    }
    (out, closed)
}

/// Starts of one area from its seed routes, its near points (with whether a car park is
/// cleared in winter) and its villages (tourist offices, ski schools).
pub fn area_departs(
    area: &Area,
    ways: &[Way],
    points: &[(Point, bool)],
    villages: &[&Point],
) -> Vec<Depart> {
    // A car park cleared in winter weighs more.
    let boosted: Vec<Point> = points
        .iter()
        .map(|(p, open)| {
            let mut p = p.clone();
            if *open {
                p.bonus += WINTER_BONUS;
            }
            p
        })
        .collect();
    let refs: Vec<&Point> = boosted.iter().collect();
    let mut out = crate::departs::clustered(&area.place, ways, &refs, MAX_AREA_STARTS, 0.0, None);
    if out.is_empty() {
        out.push(fallback(&area.place, ways, &refs, villages));
    }
    for d in &mut out {
        d.start.provenance = d.start.provenance.replacen("(X9)", "(X9b)", 1);
    }
    number(&mut out);
    out
}

fn fallback(place: &Place, ways: &[Way], points: &[&Point], villages: &[&Point]) -> Depart {
    let local = Local::new(place.point);
    let net = Net::new(ways, &local);
    let near = points
        .iter()
        .filter(|p| kind_rule(&p.kind).is_some() && !(p.kind == "parking" && private(p)))
        .filter_map(|p| net.nearest(local.xy(p.pos)).map(|s| (p, s)))
        .min_by(|a, b| a.1.dist.total_cmp(&b.1.dist))
        .filter(|(_, s)| s.dist <= NEAR_MAX_M);
    let way_name = |i: usize| {
        ways[i]
            .name
            .as_ref()
            .map(|n| format!(" « {n} »"))
            .unwrap_or_default()
    };
    // The walk from the car park, lodge or desk to the itinerary (#168).
    let access = near.map(|(_, s)| s.dist.round());
    let (pos, method, provenance, urls) = if let Some((p, s)) = near {
        (
            local.lonlat(s.xy),
            Method::Near,
            format!(
                "déduit (X9b) : point de l'itinéraire{} le plus proche de {} ({}, à {:.0} m de l'itinéraire)",
                way_name(s.way),
                p.kind,
                p.url,
                s.dist
            ),
            vec![p.url.clone()],
        )
    } else if !ways.is_empty() {
        // Ends of the routes (dead ends of the network), else the first point of each part.
        let mut ends: Vec<((f64, f64), usize)> = Vec::new();
        for (wi, w) in ways.iter().enumerate() {
            for (_, q) in [w.points.first(), w.points.last()].into_iter().flatten() {
                let xy = local.xy(*q);
                if net
                    .vertices
                    .iter()
                    .any(|(v, dead)| *dead && d2(*v, xy) < 0.01)
                {
                    ends.push((xy, wi));
                }
            }
        }
        let village = villages
            .iter()
            .flat_map(|v| {
                let vxy = local.xy(v.pos);
                ends.iter().map(move |(e, wi)| (v, *e, *wi, d2(*e, vxy)))
            })
            .min_by(|a, b| a.3.total_cmp(&b.3));
        match village {
            Some((v, e, wi, dist)) => (
                local.lonlat(e),
                Method::RouteEnd,
                format!(
                    "déduit (X9b) : extrémité de l'itinéraire{} la plus proche du village ({}, {}, à {:.1} km)",
                    way_name(wi),
                    v.kind,
                    v.url,
                    dist / 1000.0
                ),
                vec![v.url.clone()],
            ),
            None => {
                let len = |w: &Way| -> f64 {
                    w.points
                        .windows(2)
                        .map(|p| d2(local.xy(p[0].1), local.xy(p[1].1)))
                        .sum()
                };
                let (wi, w) = ways
                    .iter()
                    .enumerate()
                    .max_by(|a, b| len(a.1).total_cmp(&len(b.1)))
                    .expect("ways not empty");
                (
                    w.points[0].1,
                    Method::RouteEnd,
                    format!(
                        "déduit (X9b) : début de l'itinéraire le plus long{} ; aucun parking, foyer ni accueil OSM à moins de {NEAR_MAX_M:.0} m, aucun office de tourisme ni école de ski autour",
                        way_name(wi)
                    ),
                    Vec::new(),
                )
            }
        }
    } else {
        (
            place.point,
            Method::Centre,
            "point représentatif de la zone (G5 : point d'itinéraire le plus proche du barycentre) ; aucun itinéraire amorce lu".to_owned(),
            Vec::new(),
        )
    };
    Depart {
        start: Start {
            id: String::new(),
            name: format!("Départ des {} ({})", place.trails_fr(), place.name),
            phrase: format!("le départ des {} {}", place.trails_fr(), place.of_fr()),
            phrase_en: Some(format!("{} start", place.the_en())),
            lon: round7(pos.lon),
            lat: round7(pos.lat),
            provenance,
            access_m: access,
        },
        method,
        score: 0.0,
        points: urls,
        network: None,
    }
}

/// Starts of a site or section of an SRN area, from the starts of the area (best first):
/// those inside `within` (a section's box) or all (the area), each at least
/// [`MIN_SHARE`] of the best of them, at most [`MAX_STARTS`], numbered again. Empty when no
/// start of the area lies inside `within`.
pub fn pick(list: &[Depart], within: Option<&BBox>) -> Vec<Depart> {
    let mut out: Vec<Depart> = list
        .iter()
        .filter(|d| within.is_none_or(|b| b.contains(d.start.position())))
        .cloned()
        .collect();
    let best = out.first().map_or(0.0, |d| d.score);
    out.retain(|d| d.score >= MIN_SHARE * best);
    out.truncate(MAX_STARTS);
    number(&mut out);
    out
}

/// Whether a point may make an SRN start: a car park, a nordic lodge, a pass desk that is
/// not that of the alpine lifts ([`crate::departs::alpine`]); a ski school (in the
/// village) or an alpine desk is not where an SRN route begins.
pub fn srn_kind(p: &Point) -> bool {
    match p.kind.as_str() {
        "parking" | "foyer" => true,
        "ticket" => !p.name.as_deref().is_some_and(crate::departs::alpine),
        _ => false,
    }
}

/// Starts of every area: areas, seed routes, the local index.
pub fn all_departs(
    areas: &[Area],
    routes: &BTreeMap<String, Vec<Way>>,
    idx: &skimap_osm::local::Index,
) -> (BTreeMap<String, Vec<Depart>>, usize) {
    use skimap_osm::local;
    let mut closed = 0;
    let out = areas
        .iter()
        .map(|a| {
            let grow = |m: f64| {
                BBox::new(
                    a.bbox.south - m,
                    a.bbox.west - m * 1.4,
                    a.bbox.north + m,
                    a.bbox.east + m * 1.4,
                )
            };
            let near = idx.query(&[grow(NEAR_MARGIN_DEG)], |e| {
                local::is_station_near(|k| e.get(k))
            });
            let (mut points, c) = read_index_points(&near);
            points.retain(|(p, _)| srn_kind(p));
            closed += c;
            let far = idx.query(&[grow(VILLAGE_MARGIN_DEG)], |e| {
                local::is_station_far(|k| e.get(k))
            });
            let (vs, _) = read_index_points(&far);
            let villages: Vec<&Point> = vs
                .iter()
                .map(|(p, _)| p)
                .filter(|p| matches!(p.kind.as_str(), "tourist_office" | "ski_school"))
                .collect();
            let ways = routes.get(&a.id).map_or(&[][..], Vec::as_slice);
            (a.id.clone(), area_departs(a, ways, &points, &villages))
        })
        .collect();
    (out, closed)
}

/// Starts of every area of `srn/srn_areas.geojson`; none when the SRN layer was not built.
pub fn build(
    layout: &skimap_common::layout::DataLayout,
    dl: &skimap_common::download::Downloader,
) -> Result<crate::departs::SrnStarts> {
    let dir = layout.france_layer_dir(SRN_LAYER)?;
    let areas_path = dir.join(AREAS_FILE);
    if !areas_path.is_file() {
        tracing::warn!(path = %areas_path.display(), "SRN layer missing, no SRN starts");
        return Ok(crate::departs::SrnStarts::default());
    }
    let read = |name: &str| -> Result<Value> {
        let path = dir.join(name);
        let b = std::fs::read(&path).map_err(|e| skimap_common::Error::io(&path, e))?;
        serde_json::from_slice(&b).map_err(|e| StationsError::format(name, e.to_string()))
    };
    let areas = read_areas(&read(AREAS_FILE)?)?;
    let routes = read_routes(&read(ROUTES_FILE)?);
    let idx = skimap_osm::local::load(layout, dl)?;
    let (areas, winter_closed) = all_departs(&areas, &routes, &idx);
    Ok(crate::departs::SrnStarts {
        areas,
        winter_closed,
    })
}

/// Notes of `departs.json` for the SRN areas.
pub fn notes() -> Vec<String> {
    vec![
        format!(
            "Zones et traversées SRN (X9b) : mêmes règles que les domaines, sur les itinéraires amorces de la zone (srn_routes.geojson) et les points OSM de l'index Geofabrik local à moins de {NEAR_MARGIN_DEG}° de l'emprise : parkings non privés, foyers nordiques, accueils (sauf caisses des remontées mécaniques ; écoles de ski écartées, elles sont au village) ; parkings fermés l'hiver écartés (seasonal=summer|yes, winter_service=no, access:conditional « no @ » hiver) ; parking déneigé (winter_service=yes, seasonal=winter) : +{WINTER_BONUS} ; tous les départs gardés (au plus {MAX_AREA_STARTS}), chaque site ou tronçon du registre prend les meilleurs dans son emprise (au plus {MAX_STARTS}, chacun au moins {:.0} % du premier).",
            MIN_SHARE * 100.0
        ),
        format!(
            "Sans tel point : point de l'itinéraire le plus proche d'un parking, foyer ou accueil à moins de {NEAR_MAX_M:.0} m (method = near), sinon extrémité d'itinéraire la plus proche du village (office de tourisme ou école de ski à moins de {VILLAGE_MARGIN_DEG}°), sinon début de l'itinéraire le plus long (method = route_end), sinon point représentatif de la zone (method = centre)."
        ),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn area(kind: PlaceKind) -> Area {
        Area {
            id: "srn-domaine-test".into(),
            place: Place {
                kind,
                name: "Plateau".into(),
                point: LonLat::new(6.01, 46.01),
            },
            bbox: BBox::new(46.0, 6.0, 46.02, 6.03),
        }
    }

    fn routes() -> BTreeMap<String, Vec<Way>> {
        let v = json!({"features": [
            {"geometry": {"type": "MultiLineString", "coordinates": [
                [[6.001, 46.01], [6.005, 46.01], [6.01, 46.01]],
                [[6.01, 46.01], [6.02, 46.01], [6.028, 46.01]]]},
             "properties": {"name": "Tour du plateau", "areas": [{"id": "srn-domaine-test", "role": "seed"}]}},
            {"geometry": {"type": "LineString", "coordinates": [[6.0, 46.0], [6.0, 46.005]]},
             "properties": {"name": "Raquettes", "areas": [{"id": "srn-domaine-test", "role": "attached"}]}}
        ]});
        read_routes(&v)
    }

    fn park(lon: f64, lat: f64, tags: Value) -> Value {
        let mut t = json!({"amenity": "parking"});
        for (k, v) in tags.as_object().unwrap() {
            t[k] = v.clone();
        }
        json!({"type": "node", "id": (lon * 1e4) as i64, "lat": lat, "lon": lon, "tags": t})
    }

    #[test]
    fn routes_join_their_parts_and_only_seeds_count() {
        let r = routes();
        let w = &r["srn-domaine-test"];
        assert_eq!(
            w.len(),
            2,
            "the attached snowshoe route is not a start network"
        );
        // The shared node of the two parts has one id: the network has two dead ends.
        assert_eq!(w[0].points[2].0, w[1].points[0].0);
        let local = Local::new(LonLat::new(6.01, 46.01));
        let net = Net::new(w, &local);
        assert_eq!(net.vertices.iter().filter(|v| v.1).count(), 2);
    }

    #[test]
    fn car_parks_open_in_winter_on_the_routes_make_the_starts() {
        let v = json!({"elements": [
            park(6.0012, 46.0102, json!({"name": "Le Plan"})),
            park(6.0281, 46.0101, json!({"seasonal": "summer"})),
            park(6.0279, 46.0099, json!({"access": "private"})),
            park(6.02, 46.0103, json!({"winter_service": "yes"})),
            json!({"type": "node", "id": 9, "lat": 46.0, "lon": 6.0, "tags": {"amenity": "bench"}}),
        ]});
        let (pts, closed) = read_index_points(&v);
        assert_eq!(closed, 1);
        // classify already drops private car parks.
        assert_eq!(pts.len(), 2);
        assert!(pts[1].1, "winter_service=yes");
        let s = area_departs(
            &area(PlaceKind::SrnArea),
            &routes()["srn-domaine-test"],
            &pts,
            &[],
        );
        assert_eq!(s.len(), 2);
        assert!(s.iter().all(|d| d.method == Method::Points));
        assert_eq!(s[0].start.name, "Parking Le Plan (Plateau)");
        assert_eq!(s[1].start.phrase, "le parking de la zone SRN Plateau");
        assert_eq!(
            s[1].start.phrase_en.as_deref(),
            Some("the Plateau SRN area car park")
        );
        assert!(s[0].start.provenance.starts_with("déduit (X9b)"));
        assert_eq!(s[1].start.id, "depart-2");
    }

    #[test]
    fn fallbacks_near_then_route_end_then_centre() {
        let a = area(PlaceKind::Traverse);
        let ways = routes()["srn-domaine-test"].clone();
        // A car park 670 m from the route.
        let v = json!({"elements": [park(6.01, 46.016, json!({}))]});
        let (pts, _) = read_index_points(&v);
        let s = area_departs(&a, &ways, &pts, &[]);
        assert_eq!(s.len(), 1);
        assert_eq!(s[0].method, Method::Near);
        assert_eq!((s[0].start.lon, s[0].start.lat), (6.01, 46.01));
        assert_eq!(s[0].start.name, "Départ des itinéraires (Plateau)");
        assert_eq!(
            s[0].start.phrase,
            "le départ des itinéraires de la traversée Plateau"
        );
        // Nothing near: the end nearest to the village (east).
        let office = Point {
            kind: "tourist_office".into(),
            name: None,
            pos: LonLat::new(6.04, 46.012),
            capacity: None,
            access: None,
            url: "u".into(),
            domains: Vec::new(),
            bonus: 0.0,
        };
        let s = area_departs(&a, &ways, &[], &[&office]);
        assert_eq!(s[0].method, Method::RouteEnd);
        assert_eq!((s[0].start.lon, s[0].start.lat), (6.028, 46.01));
        // No village: the beginning of the longest part.
        let s = area_departs(&a, &ways, &[], &[]);
        assert_eq!(s[0].method, Method::RouteEnd);
        assert_eq!((s[0].start.lon, s[0].start.lat), (6.01, 46.01));
        // No route: the representative point.
        let s = area_departs(&a, &[], &[], &[]);
        assert_eq!(s[0].method, Method::Centre);
        assert_eq!((s[0].start.lon, s[0].start.lat), (6.01, 46.01));
    }

    #[test]
    fn winter_tags() {
        let t = |pairs: &[(&str, &str)]| {
            let m: BTreeMap<String, String> = pairs
                .iter()
                .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
                .collect();
            winter_closed(|k| m.get(k).cloned())
        };
        assert!(t(&[("seasonal", "summer")]));
        assert!(t(&[("winter_service", "no")]));
        assert!(t(&[("access:conditional", "no @ (Dec-Apr)")]));
        assert!(t(&[("motor_vehicle:conditional", "no @ winter")]));
        assert!(!t(&[("access:conditional", "no @ (Jul-Aug 10:00-18:00)")]));
        assert!(!t(&[("seasonal", "winter")]));
        assert!(!t(&[]));
        let p = |kind: &str, name: Option<&str>| Point {
            kind: kind.into(),
            name: name.map(str::to_owned),
            pos: LonLat::new(6.0, 46.0),
            capacity: None,
            access: None,
            url: String::new(),
            domains: Vec::new(),
            bonus: 0.0,
        };
        assert!(srn_kind(&p("parking", None)));
        assert!(srn_kind(&p("foyer", Some("Foyer nordique"))));
        assert!(srn_kind(&p("ticket", Some("Caisse forfaits ski nordique"))));
        assert!(!srn_kind(&p(
            "ticket",
            Some("Caisse des Remontés Mecaniques")
        )));
        assert!(!srn_kind(&p("ski_school", Some("ESF"))));
    }

    #[test]
    fn sections_pick_their_starts() {
        let mk = |lon: f64, score: f64| Depart {
            start: Start {
                id: String::new(),
                name: format!("{lon}"),
                phrase: String::new(),
                phrase_en: None,
                lon,
                lat: 46.0,
                provenance: String::new(),
                access_m: None,
            },
            method: Method::Points,
            score,
            points: Vec::new(),
            network: None,
        };
        let list = vec![mk(6.0, 10.0), mk(6.5, 4.0), mk(6.6, 2.0), mk(6.7, 1.0)];
        let all = pick(&list, None);
        assert_eq!(all.len(), 2, "2 and 1 are under 25 % of 10");
        let east = BBox::new(45.9, 6.4, 46.1, 6.8);
        let s = pick(&list, Some(&east));
        assert_eq!(
            s.iter().map(|d| d.start.lon).collect::<Vec<_>>(),
            vec![6.5, 6.6, 6.7]
        );
        assert_eq!(s[0].start.id, "depart-1");
        assert!(pick(&list, Some(&BBox::new(0.0, 0.0, 1.0, 1.0))).is_empty());
    }
}
