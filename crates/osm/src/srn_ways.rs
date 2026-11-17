//! The SRN itineraries of an SRN site as ways of its routing graph (#168).
//!
//! An SRN play area or traverse is skied on its itineraries (`srn/srn_routes.geojson` of
//! the site, written by the `srn` step from the France layer, G5), mostly outside the OSM
//! pistes: without them, the routing graph of the site is only the few OSM nordic,
//! snowshoe or hiking ways of its box, often far from its itineraries and from the car
//! parks its starts come from (X9b snaps them to the itineraries). The itineraries become
//! ways of the graph, joined to the OSM ways and to each other like any piste (shared
//! coordinates, crossings, short gaps: [`crate::graph::Graph::build`]); they are not OSM
//! ways (`osm_way_id` null), their geometry is the itinerary's own, nothing is drawn:
//!
//! 1. **licence** ([`public_licence`]): an itinerary enters the graph of a published pack
//!    only when its licence lets it into an ODbL database (docs/licences/l3-combinaison.md):
//!    ODbL, Licence Ouverte, CC BY 4.0, CC0. Camptocamp (CC BY-SA 3.0: share-alike,
//!    incompatible with the ODbL of the OSM ways of the same graph) and the Geotrek
//!    instances without a licence shown come in with `personal_sources` only, their
//!    sources marked `personal_use_only` (the pack then leaves the graph out of a
//!    published pack);
//! 2. **box**: only the vertices inside the site box are kept (the box of the terrain
//!    model, the zones and the snow of the site), each run of them a part;
//! 3. **one trail, one way**: where an itinerary follows an OSM way travelled both ways
//!    (a one-way piste does not carry an itinerary skied both ways) or an itinerary
//!    already taken (every vertex within [`FOLLOW_M`] of it over at least
//!    [`FOLLOW_MIN_M`]), that stretch is left out and the trail already in the graph
//!    carries it; the parts on either side end within [`FOLLOW_M`] of it and are joined
//!    to it by the connectors of the graph;
//! 4. **turning points**: a part is cut into ways of at most [`SPLIT_M`] (at its
//!    vertices, a longer segment at points along it), so that the planner, which turns at the nodes of the graph, can turn along a long
//!    itinerary (a loop of 8 km drawn as one way has no node but its start).
//!
//! Only SRN sites take them: the graph of a nordic domain stays its OSM pistes.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::Path;

use rstar::RTree;
use rstar::primitives::Line;
use serde_json::Value;
use skimap_common::geom::{BBox, LonLat};
use skimap_common::layout::DataLayout;
use skimap_common::meta::{LayerMeta, SourceRef};
use skimap_common::output::round7;
use skimap_common::site::{Site, SiteKind};

use crate::error::Result;
use crate::graph::M_PER_DEG_LAT;
use crate::pistes::{Grooming, Oneway, Piste, PisteSet, Use, polyline_length_m};

/// Layer of the SRN areas and itineraries of a site (`skimap_srn::build::LAYER`).
pub const SRN_LAYER: &str = "srn";
/// Itineraries of the site (`skimap_srn::build::ROUTES_FILE`).
pub const ROUTES_FILE: &str = "srn_routes.geojson";
/// Node ids of the itinerary vertices start here, downwards: far below the ids of the
/// nodes [`crate::graph::Graph::build`] adds (-1, -2, …), never an OSM id (positive).
pub const FIRST_NODE_ID: i64 = -(1 << 50);
/// An itinerary vertex this close to a trail already in the graph is on it, metres: the
/// largest gap a connector of the graph closes ([`crate::extract::CONNECTOR_MAX_M`]), so
/// that the parts left on either side of a stretch on that trail are joined to it.
pub const FOLLOW_M: f64 = crate::extract::CONNECTOR_MAX_M;
/// Shortest stretch on a trail already in the graph left out, metres (shorter: a
/// crossing, kept).
pub const FOLLOW_MIN_M: f64 = 50.0;
/// Longest way made of an itinerary, metres: the planner turns at nodes.
pub const SPLIT_M: f64 = 500.0;

/// An itinerary piece made a way of the graph.
#[derive(Debug, Clone, PartialEq)]
pub struct SrnWay {
    /// The way (no OSM id, nordic, not groomed, two-way).
    pub piste: Piste,
    /// Identifier of the itinerary (`id` of `srn_routes.geojson`).
    pub route_id: String,
    /// Provider of the itinerary (`osm`, `geotrek`, `camptocamp`, `datatourisme`).
    pub provider: String,
    /// Licence of the itinerary as its source publishes it.
    pub licence: String,
}

/// What became of the itineraries of a site.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct SrnStats {
    /// Itineraries read.
    pub routes: usize,
    /// Itineraries left out for their licence (published pack): provider → count.
    pub left_out: BTreeMap<String, usize>,
    /// Km of itinerary inside the site box.
    pub km_in_box: f64,
    /// Km of it on a trail already in the graph (left out, carried by that trail).
    pub km_on_trails: f64,
    /// Km made ways of the graph.
    pub km_ways: f64,
}

/// The SRN itineraries of an SRN site, with the sources of those taken.
#[derive(Debug, Clone, Default)]
pub struct SiteSrn {
    /// Ways of the graph, at most [`SPLIT_M`] each.
    pub ways: Vec<SrnWay>,
    /// Sources of the itineraries taken, one per provider other than OSM (whose source
    /// is the extract of the layer), `personal_use_only` when their licence keeps them
    /// out of a published pack.
    pub sources: Vec<SourceRef>,
    /// Figures.
    pub stats: SrnStats,
}

/// Whether the routing graph of `site` takes its SRN itineraries.
pub fn takes_itineraries(site: &Site) -> bool {
    matches!(site.kind, SiteKind::SrnArea | SiteKind::Traverse)
}

/// Whether an itinerary published under `licence` may enter the routing graph of a
/// published pack, an ODbL database (docs/licences/l3-combinaison.md): ODbL, or a licence
/// asking for attribution only (Licence Ouverte, CC BY 4.0, CC0). Share-alike licences
/// other than the ODbL (CC BY-SA), no licence shown, or anything else: no.
pub fn public_licence(licence: &str) -> bool {
    let l = licence.trim().to_lowercase();
    ["odbl", "licence ouverte", "cc by 4.0", "cc-by-4.0", "cc0"]
        .iter()
        .any(|p| l.starts_with(p))
}

fn walk(v: &Value, out: &mut Vec<LonLat>) {
    if let Some(a) = v.as_array() {
        for p in a {
            if let (Some(lon), Some(lat)) = (
                p.get(0).and_then(Value::as_f64),
                p.get(1).and_then(Value::as_f64),
            ) {
                // To OSM precision, as the layers are written: a vertex of an itinerary on
                // an OSM node is that node, never a second node a few millimetres away
                // that the written graph would put at the same place.
                out.push(LonLat::new(round7(lon), round7(lat)));
            }
        }
    }
}

/// Local plane in metres (equirectangular around `origin`).
struct Plane {
    origin: LonLat,
    kx: f64,
}

impl Plane {
    fn new(origin: LonLat) -> Self {
        Self {
            origin,
            kx: M_PER_DEG_LAT * origin.lat.to_radians().cos(),
        }
    }

    fn xy(&self, p: LonLat) -> [f64; 2] {
        [
            (p.lon - self.origin.lon) * self.kx,
            (p.lat - self.origin.lat) * M_PER_DEG_LAT,
        ]
    }
}

/// Trails already in the graph, as segments in the plane.
struct Trails {
    plane: Plane,
    tree: RTree<Line<[f64; 2]>>,
}

impl Trails {
    fn add(&mut self, pts: &[LonLat]) {
        for w in pts.windows(2) {
            let (a, b) = (self.plane.xy(w[0]), self.plane.xy(w[1]));
            self.tree.insert(Line::new(a, b));
        }
    }

    fn near(&self, p: LonLat) -> bool {
        let xy = self.plane.xy(p);
        self.tree
            .locate_within_distance(xy, FOLLOW_M * FOLLOW_M)
            .next()
            .is_some()
    }
}

/// Parts of `pts` left once the stretches on `trails` are taken out (module, 3), and the
/// length of those stretches, metres. The ends of a stretch stay, each the end of the part
/// beside it (within [`FOLLOW_M`] of the trail).
fn off_trails(pts: &[LonLat], trails: &Trails) -> (Vec<Vec<LonLat>>, f64) {
    let near: Vec<bool> = pts.iter().map(|p| trails.near(*p)).collect();
    // Segment k joins vertex k to vertex k + 1.
    let mut on = vec![false; pts.len().saturating_sub(1)];
    let mut on_m = 0.0;
    let mut i = 0;
    while i < pts.len() {
        if !near[i] {
            i += 1;
            continue;
        }
        let mut j = i;
        while j + 1 < pts.len() && near[j + 1] {
            j += 1;
        }
        let len: f64 = pts[i..=j].windows(2).map(|w| w[0].haversine_m(w[1])).sum();
        if len >= FOLLOW_MIN_M {
            on[i..j].iter_mut().for_each(|x| *x = true);
            on_m += len;
        }
        i = j + 1;
    }
    let mut parts = Vec::new();
    let mut cur: Vec<LonLat> = vec![pts[0]];
    for (k, on_trail) in on.iter().enumerate() {
        if *on_trail {
            if cur.len() >= 2 {
                parts.push(std::mem::take(&mut cur));
            }
            cur = vec![pts[k + 1]];
        } else {
            cur.push(pts[k + 1]);
        }
    }
    if cur.len() >= 2 {
        parts.push(cur);
    }
    (parts, on_m)
}

/// Runs of the vertices of `pts` inside `bbox`.
fn in_box(pts: &[LonLat], bbox: &BBox) -> Vec<Vec<LonLat>> {
    let mut out = Vec::new();
    let mut cur = Vec::new();
    for p in pts {
        if bbox.contains(*p) {
            cur.push(*p);
        } else if !cur.is_empty() {
            out.push(std::mem::take(&mut cur));
        }
    }
    out.push(cur);
    out.retain(|r| r.len() >= 2);
    out
}

/// Pieces of `pts` of at most [`SPLIT_M`], consecutive pieces sharing their end vertex:
/// cut at vertices, a segment longer than that at evenly spaced points of the segment.
fn split(pts: &[LonLat]) -> Vec<Vec<LonLat>> {
    let mut dense = vec![pts[0]];
    for w in pts.windows(2) {
        let n = (w[0].haversine_m(w[1]) / SPLIT_M).ceil().max(1.0) as usize;
        for k in 1..n {
            let t = k as f64 / n as f64;
            dense.push(LonLat::new(
                round7(w[0].lon + (w[1].lon - w[0].lon) * t),
                round7(w[0].lat + (w[1].lat - w[0].lat) * t),
            ));
        }
        dense.push(w[1]);
    }
    let mut out = Vec::new();
    let mut cur = vec![dense[0]];
    let mut len = 0.0;
    for w in dense.windows(2) {
        let d = w[0].haversine_m(w[1]);
        if len + d > SPLIT_M && cur.len() >= 2 {
            out.push(std::mem::replace(&mut cur, vec![w[0]]));
            len = 0.0;
        }
        cur.push(w[1]);
        len += d;
    }
    out.push(cur);
    out
}

/// One itinerary of `srn_routes.geojson`.
struct Route {
    id: String,
    name: Option<String>,
    provider: String,
    licence: String,
    attribution: String,
    url: String,
    parts: Vec<Vec<LonLat>>,
}

fn routes_of(v: &Value) -> Vec<Route> {
    let mut out = Vec::new();
    for f in v["features"].as_array().into_iter().flatten() {
        let p = &f["properties"];
        let s = |k: &str| p[k].as_str().unwrap_or_default().trim().to_owned();
        let g = &f["geometry"];
        let parts: Vec<&Value> = match g["type"].as_str() {
            Some("LineString") => vec![&g["coordinates"]],
            Some("MultiLineString") => g["coordinates"].as_array().into_iter().flatten().collect(),
            _ => Vec::new(),
        };
        let parts = parts
            .into_iter()
            .map(|part| {
                let mut pts = Vec::new();
                walk(part, &mut pts);
                pts.dedup();
                pts
            })
            .filter(|pts| pts.len() >= 2)
            .collect();
        out.push(Route {
            id: s("id"),
            name: Some(s("name")).filter(|n| !n.is_empty()),
            provider: s("provider"),
            licence: s("licence"),
            attribution: s("attribution"),
            url: s("url"),
            parts,
        });
    }
    out
}

fn piste(points: Vec<(i64, LonLat)>, name: Option<String>) -> Piste {
    let length_m = polyline_length_m(&points);
    Piste {
        way_id: 0,
        uses: BTreeSet::from([Use::Nordic]),
        tagged: false,
        way_name: name,
        relations: Vec::new(),
        difficulty: None,
        difficulty_max: None,
        difficulty_source: None,
        way_difficulty: None,
        grooming: Grooming {
            raw: None,
            classic: false,
            skating: false,
            backcountry: true,
            scooter: false,
        },
        oneway: Oneway::No,
        lit: None,
        points,
        length_m,
        layered: false,
    }
}

/// The ways of the itineraries of a `srn_routes.geojson` value for a graph of the OSM
/// ways `osm` in `bbox` (module documentation); `layer_sources`: the sources of the `srn`
/// layer, from which the source of each provider is taken.
pub fn ways_of(
    v: &Value,
    bbox: &BBox,
    osm: &PisteSet,
    personal_sources: bool,
    layer_sources: &[SourceRef],
) -> SiteSrn {
    let mut trails = Trails {
        plane: Plane::new(LonLat::new(
            f64::midpoint(bbox.west, bbox.east),
            f64::midpoint(bbox.south, bbox.north),
        )),
        tree: RTree::new(),
    };
    // A one-way piste does not carry an itinerary skied both ways: beside it, the
    // itinerary keeps its own way.
    for p in osm
        .pistes
        .iter()
        .filter(|p| !matches!(p.oneway, Oneway::Forward | Oneway::Backward))
    {
        let pts: Vec<LonLat> = p.points.iter().map(|(_, q)| *q).collect();
        trails.add(&pts);
    }
    let mut ids: HashMap<(u64, u64), i64> = HashMap::new();
    let mut next = FIRST_NODE_ID;
    let mut id_of = |q: LonLat| -> i64 {
        *ids.entry((q.lon.to_bits(), q.lat.to_bits()))
            .or_insert_with(|| {
                next -= 1;
                next
            })
    };
    let mut out = SiteSrn::default();
    let mut taken: BTreeMap<String, (bool, String, BTreeSet<String>, String)> = BTreeMap::new();
    for r in routes_of(v) {
        out.stats.routes += 1;
        let public = public_licence(&r.licence);
        if !public && !personal_sources {
            *out.stats.left_out.entry(r.provider.clone()).or_default() += 1;
            continue;
        }
        let mut kept_any = false;
        for part in &r.parts {
            for run in in_box(part, bbox) {
                let run_m: f64 = run.windows(2).map(|w| w[0].haversine_m(w[1])).sum();
                out.stats.km_in_box += run_m / 1000.0;
                let (pieces, on_m) = off_trails(&run, &trails);
                out.stats.km_on_trails += on_m / 1000.0;
                for piece in pieces {
                    trails.add(&piece);
                    for way in split(&piece) {
                        let pts: Vec<(i64, LonLat)> = way.iter().map(|q| (id_of(*q), *q)).collect();
                        let p = piste(pts, r.name.clone());
                        out.stats.km_ways += p.length_m / 1000.0;
                        out.ways.push(SrnWay {
                            piste: p,
                            route_id: r.id.clone(),
                            provider: r.provider.clone(),
                            licence: r.licence.clone(),
                        });
                        kept_any = true;
                    }
                }
            }
        }
        if kept_any && r.provider != "osm" {
            let e = taken
                .entry(r.provider.clone())
                .or_insert_with(|| (public, r.licence.clone(), BTreeSet::new(), r.url.clone()));
            e.0 &= public;
            if !r.attribution.is_empty() {
                e.2.insert(r.attribution.clone());
            }
        }
    }
    for (provider, (public, licence, attributions, url)) in taken {
        let attribution = attributions.into_iter().collect::<Vec<_>>().join(" ; ");
        let mut s = layer_sources
            .iter()
            .find(|s| s.name.to_lowercase().starts_with(&provider))
            .cloned()
            .unwrap_or_else(|| {
                SourceRef::new(
                    format!("Itinéraires SRN {provider}"),
                    url,
                    licence.clone(),
                    attribution.clone(),
                )
            });
        s.licence = licence;
        s.attribution = attribution;
        s.personal_use_only = !public;
        s.note = Some(if public {
            "itinéraires SRN du graphe de routage (#168) : licence admise dans une base ODbL (L3)"
                .to_owned()
        } else {
            "itinéraires SRN du graphe de routage (#168), usage personnel seulement : licence à partage à l'identique autre que l'ODbL, ou aucune licence affichée (L3)".to_owned()
        });
        out.sources.push(s);
    }
    out
}

/// The SRN itineraries of `site`, for its graph of the OSM ways `osm`, when it is an SRN
/// site whose `srn` layer exists (else none); those whose licence keeps them out of a
/// published pack only with `personal_sources`.
pub fn site_srn(
    site: &Site,
    layout: &DataLayout,
    osm: &PisteSet,
    personal_sources: bool,
) -> Result<SiteSrn> {
    if !takes_itineraries(site) {
        return Ok(SiteSrn::default());
    }
    let dir = layout.layer_dir(site, SRN_LAYER)?;
    let path = dir.join(ROUTES_FILE);
    if !path.is_file() {
        return Ok(SiteSrn::default());
    }
    let v = read(&path)?;
    let sources = LayerMeta::read(&dir.join(LayerMeta::file_name(SRN_LAYER)))
        .map(|m| m.sources)
        .unwrap_or_default();
    Ok(ways_of(&v, &site.bbox, osm, personal_sources, &sources))
}

fn read(path: &Path) -> Result<Value> {
    let body = std::fs::read(path).map_err(|e| crate::error::OsmError::io(path, e))?;
    serde_json::from_slice(&body)
        .map_err(|e| crate::error::OsmError::format(ROUTES_FILE, e.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graph::Graph;

    fn route(id: &str, provider: &str, licence: &str, coords: Value) -> Value {
        serde_json::json!({"properties": {"id": id, "name": id, "provider": provider,
            "licence": licence, "attribution": format!("© {provider}"), "url": "https://example.org"},
            "geometry": {"type": "LineString", "coordinates": coords}})
    }

    fn bbox() -> BBox {
        BBox::new(44.9, 5.9, 45.1, 6.2)
    }

    #[test]
    fn licences_of_a_published_graph() {
        assert!(public_licence("ODbL 1.0"));
        assert!(public_licence(
            "Licence Ouverte 2.0 (Etalab), via DATAtourisme"
        ));
        assert!(!public_licence("CC BY-SA 3.0"));
        assert!(!public_licence(
            "aucune licence affichée par l'instance ; publie sous Licence Ouverte 2.0 : à confirmer"
        ));
        assert!(!public_licence(""));
    }

    /// A published pack's graph has no edge of a personal-use source: the itineraries of
    /// Camptocamp (CC BY-SA) and of a Geotrek without licence are left out, their
    /// sources are not listed; with `personal_sources` they come in, marked personal.
    #[test]
    fn a_published_graph_has_no_itinerary_of_a_personal_source() {
        let v = serde_json::json!({"features": [
            route("c2c", "camptocamp", "CC BY-SA 3.0", serde_json::json!([[6.0, 45.0], [6.01, 45.0]])),
            route("gt", "geotrek", "aucune licence affichée", serde_json::json!([[6.0, 45.01], [6.01, 45.01]])),
            route("dt", "datatourisme", "Licence Ouverte 2.0 (Etalab), via DATAtourisme",
                  serde_json::json!([[6.0, 45.02], [6.01, 45.02]])),
        ]});
        let public = ways_of(&v, &bbox(), &PisteSet::default(), false, &[]);
        assert!(public.ways.iter().all(|w| w.route_id == "dt"));
        assert!(!public.ways.is_empty());
        assert!(public.sources.iter().all(|s| !s.personal_use_only));
        assert_eq!(public.sources.len(), 1);
        assert_eq!(public.stats.left_out.values().sum::<usize>(), 2);
        let personal = ways_of(&v, &bbox(), &PisteSet::default(), true, &[]);
        let ids: BTreeSet<&str> = personal.ways.iter().map(|w| w.route_id.as_str()).collect();
        assert_eq!(ids, BTreeSet::from(["c2c", "dt", "gt"]));
        let restricted: BTreeSet<&str> = personal
            .sources
            .iter()
            .filter(|s| s.personal_use_only)
            .map(|s| s.name.as_str())
            .collect();
        assert_eq!(
            restricted,
            BTreeSet::from(["Itinéraires SRN camptocamp", "Itinéraires SRN geotrek"])
        );
    }

    /// Kept inside the box, cut every 500 m at most, a stretch along an OSM way left to
    /// that way, joined to it on both sides; two itineraries sharing a vertex share a node.
    #[test]
    fn itineraries_inside_the_box_cut_and_not_doubling_a_trail() {
        // An OSM way along 45.0 from 6.02 to 6.03 (about 790 m).
        let osm_pts = [(1, LonLat::new(6.02, 45.0)), (2, LonLat::new(6.03, 45.0))];
        let osm = PisteSet {
            pistes: vec![piste(osm_pts.to_vec(), Some("OSM".into()))],
            ..Default::default()
        };
        let along: Vec<Value> = (0..=60)
            .map(|k| serde_json::json!([6.0 + 0.0005 * f64::from(k), 45.000_03]))
            .chain([serde_json::json!([6.5, 45.0])])
            .collect();
        let v = serde_json::json!({"features": [
            route("a", "datatourisme", "Licence Ouverte 2.0", Value::Array(along)),
            route("b", "datatourisme", "Licence Ouverte 2.0",
                  serde_json::json!([[6.0, 44.99], [6.0, 45.00003]])),
        ]});
        let s = ways_of(&v, &bbox(), &osm, false, &[]);
        // 6.5 E is outside the box; 6.02 to 6.03 (3 m from the OSM way) left out.
        assert!(
            s.ways
                .iter()
                .all(|w| w.piste.points.iter().all(|(_, p)| p.lon < 6.2))
        );
        assert!(s.ways.iter().all(|w| w.piste.length_m <= SPLIT_M + 1.0));
        assert!((s.stats.km_on_trails - 0.79).abs() < 0.05, "{:?}", s.stats);
        assert!(!s.ways.iter().any(|w| {
            w.piste
                .points
                .iter()
                .any(|(_, p)| p.lon > 6.021 && p.lon < 6.029)
        }));
        // « b » ends on the first vertex of « a »: one node.
        let first_a = s.ways[0].piste.points[0].0;
        assert!(
            s.ways.iter().filter(|w| w.route_id == "b").any(|w| w
                .piste
                .points
                .iter()
                .any(|(id, _)| *id == first_a))
        );
        let mut set = osm.clone();
        set.pistes.extend(s.ways.iter().map(|w| w.piste.clone()));
        let g = Graph::build(&set, crate::extract::CONNECTOR_MAX_M);
        assert_eq!(g.components().1, 1);
    }

    /// An itinerary beside a one-way piste keeps its own way (skied both ways).
    #[test]
    fn a_one_way_piste_does_not_carry_an_itinerary() {
        let mut p = piste(
            vec![(1, LonLat::new(6.02, 45.0)), (2, LonLat::new(6.03, 45.0))],
            None,
        );
        p.oneway = Oneway::Forward;
        let osm = PisteSet {
            pistes: vec![p],
            ..Default::default()
        };
        let v = serde_json::json!({"features": [route("a", "osm", "ODbL 1.0",
            serde_json::json!([[6.02, 45.00003], [6.025, 45.00003], [6.03, 45.00003]]))]});
        let s = ways_of(&v, &bbox(), &osm, false, &[]);
        assert_eq!(s.stats.km_on_trails, 0.0);
        assert!((s.stats.km_ways - 0.79).abs() < 0.05, "{:?}", s.stats);
    }
}
