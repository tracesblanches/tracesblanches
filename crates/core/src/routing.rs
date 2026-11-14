//! Trail graph and on-trail routing (C1, #33): directed graph (one-way trails), cost from
//! slope and style, route between two points.
//!
//! - The graph is [`crate::graph::PisteGraph`]; one-way edges are only travelled from
//!   `from` to `to`.
//! - The cost is pluggable ([`CostModel`]): a cost per piece of edge (horizontal
//!   distance, signed elevation change, elevation) times a factor per edge (style,
//!   preferences, `f64::INFINITY` to forbid). [`Distance`] is the shortest distance;
//!   [`crate::eta::EtaModel`] is the travel time with slope, snow and technique (C5).
//!   Pieces come from the elevation profile of the edge (10 m stations sampled on the
//!   5 m terrain model); an edge without profile (outside LiDAR HD coverage, Switzerland,
//!   Italy) is flat.
//! - Endpoints are snapped to the nearest point of the network; the route may start and
//!   end in the middle of an edge.
//! - Search: Dijkstra on precomputed half-edge costs (forward and backward per edge).

use std::cmp::Ordering;
use std::collections::BinaryHeap;

use serde::Serialize;
use serde_json::{Map, Value, json};
use skimap_common::geom::LonLat;

use crate::error::{Error, Result};
use crate::graph::{EdgeAttrs, HalfEdge, PisteGraph, Snap};

/// Ski technique.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Style {
    /// Classic (diagonal stride, double poling) in tracks.
    Classic,
    /// Skating on a groomed lane.
    Skating,
}

/// Preferences turning edge attributes into a cost factor (≥ 1, `INFINITY` forbids).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, serde::Deserialize)]
pub struct StylePolicy {
    /// Classic skier on a trail groomed only for skating (no tracks: slower, possible).
    pub classic_on_skating_only: f64,
    /// Skater on a trail with classic tracks only (skating spoils the tracks; most sites
    /// forbid it): strongly avoided by default, `INFINITY` to forbid.
    pub skating_on_classic_only: f64,
    /// Snowshoe / winter walking trail without Nordic use.
    pub hike_only: f64,
}

impl Default for StylePolicy {
    fn default() -> Self {
        Self {
            classic_on_skating_only: 1.5,
            skating_on_classic_only: 3.0,
            hike_only: 3.0,
        }
    }
}

impl StylePolicy {
    /// Factor of an edge for a style. Unknown grooming (`None`) is neutral.
    pub fn factor(&self, attrs: &EdgeAttrs, style: Style) -> f64 {
        let mut f = 1.0;
        match style {
            Style::Classic if attrs.classic == Some(false) && attrs.skating == Some(true) => {
                f *= self.classic_on_skating_only;
            }
            Style::Skating if attrs.skating == Some(false) && attrs.classic == Some(true) => {
                f *= self.skating_on_classic_only;
            }
            _ => {}
        }
        if attrs.hike_only() {
            f *= self.hike_only;
        }
        f
    }
}

/// Additive cost along the network.
pub trait CostModel {
    /// Cost of a piece of edge: horizontal distance (m), signed elevation change in the
    /// direction of travel (m, 0 without profile), mean elevation (m) when known.
    fn piece_cost(&self, attrs: &EdgeAttrs, dist_m: f64, dz_m: f64, elev_m: Option<f64>) -> f64;

    /// Factor applied to the whole edge (style, preferences); `INFINITY` forbids it.
    fn edge_factor(&self, _attrs: &EdgeAttrs) -> f64 {
        1.0
    }
}

/// Shortest distance, with the style factors.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Distance {
    /// Style of the skier.
    pub style: Style,
    /// Style preferences.
    pub policy: StylePolicy,
}

impl CostModel for Distance {
    fn piece_cost(&self, _: &EdgeAttrs, dist_m: f64, _: f64, _: Option<f64>) -> f64 {
        dist_m
    }
    fn edge_factor(&self, attrs: &EdgeAttrs) -> f64 {
        self.policy.factor(attrs, self.style)
    }
}

/// A piece of route: one interval between two profile stations.
#[derive(Debug, Clone, Copy)]
pub struct Piece<'a> {
    /// Attributes of the edge.
    pub attrs: &'a EdgeAttrs,
    /// Horizontal distance, metres.
    pub dist_m: f64,
    /// Elevation change in the direction of travel, metres (0 without profile).
    pub dz_m: f64,
    /// Mean elevation, metres, when known.
    pub elev_m: Option<f64>,
}

/// Pieces of an edge between two distances (travel order).
pub fn edge_pieces(g: &PisteGraph, edge: usize, a: f64, b: f64) -> Vec<Piece<'_>> {
    let attrs = &g.edge(edge).attrs;
    g.stations(edge, a, b)
        .windows(2)
        .map(|w| {
            let ((d0, z0), (d1, z1)) = (w[0], w[1]);
            let (dz, elev) = match (z0, z1) {
                (Some(z0), Some(z1)) => (z1 - z0, Some(0.5 * (z0 + z1))),
                _ => (0.0, None),
            };
            Piece {
                attrs,
                dist_m: (d1 - d0).abs(),
                dz_m: dz,
                elev_m: elev,
            }
        })
        .filter(|p| p.dist_m > 0.0)
        .collect()
}

/// Cost of an edge between two distances under a model (factor included).
pub fn partial_cost<M: CostModel + ?Sized>(
    g: &PisteGraph,
    model: &M,
    edge: usize,
    a: f64,
    b: f64,
) -> f64 {
    let e = g.edge(edge);
    if b < a && e.oneway {
        return f64::INFINITY;
    }
    let f = model.edge_factor(&e.attrs);
    if !f.is_finite() {
        return f64::INFINITY;
    }
    let s: f64 = edge_pieces(g, edge, a, b)
        .iter()
        .map(|p| model.piece_cost(p.attrs, p.dist_m, p.dz_m, p.elev_m))
        .sum();
    f * s
}

/// A route leg: an edge travelled from `from_m` to `to_m` metres along it (backward when
/// `to_m < from_m`).
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct Leg {
    /// Edge index.
    pub edge: usize,
    /// Start, metres from the edge start.
    pub from_m: f64,
    /// End, metres from the edge start.
    pub to_m: f64,
}

impl Leg {
    /// Length, metres.
    pub fn length_m(&self) -> f64 {
        (self.to_m - self.from_m).abs()
    }
}

/// A route on the network.
#[derive(Debug, Clone, PartialEq)]
pub struct Route {
    /// Snapped start.
    pub start: Snap,
    /// Snapped end.
    pub end: Snap,
    /// Legs in travel order.
    pub legs: Vec<Leg>,
    /// Cost under the model used.
    pub cost: f64,
}

impl Route {
    /// Length, metres.
    pub fn length_m(&self) -> f64 {
        self.legs.iter().map(Leg::length_m).sum()
    }

    /// Geometry in travel order.
    pub fn polyline(&self, g: &PisteGraph) -> Vec<LonLat> {
        let mut out: Vec<LonLat> = Vec::new();
        for l in &self.legs {
            let pts = g.sub_polyline(l.edge, l.from_m, l.to_m);
            let skip = usize::from(out.last().is_some_and(|p| *p == pts[0]));
            out.extend_from_slice(&pts[skip..]);
        }
        out
    }

    /// Pieces in travel order.
    pub fn pieces<'g>(&self, g: &'g PisteGraph) -> Vec<Piece<'g>> {
        self.legs
            .iter()
            .flat_map(|l| edge_pieces(g, l.edge, l.from_m, l.to_m))
            .collect()
    }

    /// Climb and descent, metres.
    pub fn gain_loss(&self, g: &PisteGraph) -> (f64, f64) {
        self.pieces(g).iter().fold((0.0, 0.0), |(u, d), p| {
            if p.dz_m > 0.0 {
                (u + p.dz_m, d)
            } else {
                (u, d - p.dz_m)
            }
        })
    }

    /// Share of the length with an elevation profile.
    pub fn profile_coverage(&self, g: &PisteGraph) -> f64 {
        let p = self.pieces(g);
        let all: f64 = p.iter().map(|p| p.dist_m).sum();
        let with: f64 = p
            .iter()
            .filter(|p| p.elev_m.is_some())
            .map(|p| p.dist_m)
            .sum();
        if all > 0.0 { with / all } else { 1.0 }
    }

    /// Distinct trail names in travel order.
    pub fn names(&self, g: &PisteGraph) -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        for l in &self.legs {
            if let Some(n) = &g.edge(l.edge).attrs.name
                && out.last() != Some(n)
            {
                out.push(n.clone());
            }
        }
        out
    }

    /// GeoJSON feature (LineString) with the given properties plus length, climb,
    /// descent and edge list.
    pub fn to_feature(&self, g: &PisteGraph, mut props: Map<String, Value>) -> Value {
        let (up, down) = self.gain_loss(g);
        props.insert("length_m".into(), json!(round1(self.length_m())));
        props.insert("gain_m".into(), json!(round1(up)));
        props.insert("loss_m".into(), json!(round1(down)));
        props.insert(
            "profile_coverage".into(),
            json!((self.profile_coverage(g) * 1000.0).round() / 1000.0),
        );
        props.insert("snap_start_m".into(), json!(round1(self.start.dist_m)));
        props.insert("snap_end_m".into(), json!(round1(self.end.dist_m)));
        props.insert(
            "edges".into(),
            json!(self.legs.iter().map(|l| l.edge).collect::<Vec<_>>()),
        );
        props.insert("names".into(), json!(self.names(g)));
        let coords: Vec<[f64; 2]> = self
            .polyline(g)
            .iter()
            .map(|p| [round7(p.lon), round7(p.lat)])
            .collect();
        json!({
            "type": "Feature",
            "geometry": {"type": "LineString", "coordinates": coords},
            "properties": props,
        })
    }
}

fn round1(x: f64) -> f64 {
    (x * 10.0).round() / 10.0
}

fn round7(x: f64) -> f64 {
    (x * 1e7).round() / 1e7
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct Item(f64, usize);

impl Eq for Item {}

impl Ord for Item {
    fn cmp(&self, other: &Self) -> Ordering {
        other.0.total_cmp(&self.0).then(other.1.cmp(&self.1))
    }
}

impl PartialOrd for Item {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

/// Cumulative costs of an edge at its profile stations.
#[derive(Debug, Clone)]
struct EdgeCosts {
    /// Station distances from the edge start.
    st: Vec<f64>,
    /// Cost from the start to each station, travelling forward (factor included).
    fwd: Vec<f64>,
    /// Cost from each station back to the start, travelling backward (factor included;
    /// `INFINITY` on one-way edges).
    bwd: Vec<f64>,
}

impl EdgeCosts {
    /// Cumulative value at a distance, linear inside a piece.
    fn at(&self, cum: &[f64], d: f64) -> f64 {
        let i = self
            .st
            .partition_point(|&x| x <= d)
            .clamp(1, self.st.len() - 1);
        let (d0, d1) = (self.st[i - 1], self.st[i]);
        let t = if d1 > d0 {
            ((d - d0) / (d1 - d0)).clamp(0.0, 1.0)
        } else {
            0.0
        };
        cum[i - 1] + (cum[i] - cum[i - 1]) * t
    }
}

/// Router with precomputed costs: cumulative cost at every profile station of every
/// edge in both directions, so that a full edge costs one lookup and a partial edge two
/// interpolations.
#[derive(Debug, Clone)]
pub struct Router<'g, M> {
    graph: &'g PisteGraph,
    model: M,
    costs: Vec<EdgeCosts>,
}

impl<'g, M: CostModel> Router<'g, M> {
    /// Precomputes the cumulative costs of every edge in both directions.
    pub fn new(graph: &'g PisteGraph, model: M) -> Self {
        let mut costs = Vec::with_capacity(graph.edges().len());
        for e in 0..graph.edges().len() {
            let edge = graph.edge(e);
            let f = model.edge_factor(&edge.attrs);
            let stations = graph.stations(e, 0.0, graph.length_m(e));
            let mut st = Vec::with_capacity(stations.len());
            let (mut fwd, mut bwd) = (vec![0.0], vec![0.0]);
            st.push(stations[0].0);
            for w in stations.windows(2) {
                let ((d0, z0), (d1, z1)) = (w[0], w[1]);
                let (dz, elev) = match (z0, z1) {
                    (Some(z0), Some(z1)) => (z1 - z0, Some(0.5 * (z0 + z1))),
                    _ => (0.0, None),
                };
                let dist = d1 - d0;
                let (cf, cb) = if dist > 0.0 {
                    (
                        model.piece_cost(&edge.attrs, dist, dz, elev),
                        model.piece_cost(&edge.attrs, dist, -dz, elev),
                    )
                } else {
                    (0.0, 0.0)
                };
                st.push(d1);
                fwd.push(fwd.last().expect("seeded") + f * cf);
                bwd.push(if edge.oneway {
                    f64::INFINITY
                } else {
                    bwd.last().expect("seeded") + f * cb
                });
            }
            costs.push(EdgeCosts { st, fwd, bwd });
        }
        Self {
            graph,
            model,
            costs,
        }
    }

    /// The graph.
    pub fn graph(&self) -> &'g PisteGraph {
        self.graph
    }

    /// The cost model.
    pub fn model(&self) -> &M {
        &self.model
    }

    /// Cost of a full half-edge.
    pub fn half_cost(&self, h: HalfEdge) -> f64 {
        let c = &self.costs[h.edge];
        let last = c.st.len() - 1;
        if h.forward { c.fwd[last] } else { c.bwd[last] }
    }

    /// Cost of an edge between two distances (backward when `b < a`), from the
    /// precomputed cumulative costs.
    pub fn partial_cost(&self, edge: usize, a: f64, b: f64) -> f64 {
        let c = &self.costs[edge];
        if b >= a {
            c.at(&c.fwd, b) - c.at(&c.fwd, a)
        } else if self.graph.edge(edge).oneway {
            f64::INFINITY
        } else {
            c.at(&c.bwd, a) - c.at(&c.bwd, b)
        }
    }

    /// Route between two positions, each snapped to the network within `max_snap_m`.
    pub fn route(&self, from: LonLat, to: LonLat, max_snap_m: f64) -> Result<Route> {
        let snap = |p: LonLat| {
            self.graph.snap(p, max_snap_m).ok_or(Error::NoTrailNearby {
                lon: p.lon,
                lat: p.lat,
                max_m: max_snap_m,
            })
        };
        self.route_snaps(snap(from)?, snap(to)?)
    }

    /// Route between two points of the network.
    pub fn route_snaps(&self, s: Snap, t: Snap) -> Result<Route> {
        let g = self.graph;
        let n = g.nodes().len();
        let mut dist = vec![f64::INFINITY; n];
        let mut prev: Vec<Option<HalfEdge>> = vec![None; n];
        // Leg leaving the start edge towards a seed node.
        let mut seed_leg: Vec<Option<Leg>> = vec![None; n];
        let mut heap = BinaryHeap::new();
        let se = g.edge(s.edge);
        let sl = g.length_m(s.edge);
        for (node, to_m) in [(se.to, sl), (se.from, 0.0)] {
            let c = self.partial_cost(s.edge, s.along_m, to_m);
            if c < dist[node] {
                dist[node] = c;
                seed_leg[node] = Some(Leg {
                    edge: s.edge,
                    from_m: s.along_m,
                    to_m,
                });
                heap.push(Item(c, node));
            }
        }
        // Best arrival: cost, last node reached (None: direct along a shared edge) and
        // the final partial leg.
        let mut best = f64::INFINITY;
        let mut best_end: Option<(Option<usize>, Leg)> = None;
        if s.edge == t.edge {
            let c = self.partial_cost(s.edge, s.along_m, t.along_m);
            if c < best {
                best = c;
                best_end = Some((
                    None,
                    Leg {
                        edge: s.edge,
                        from_m: s.along_m,
                        to_m: t.along_m,
                    },
                ));
            }
        }
        let te = g.edge(t.edge);
        let tl = g.length_m(t.edge);
        let entries = [
            (te.from, 0.0, self.partial_cost(t.edge, 0.0, t.along_m)),
            (te.to, tl, self.partial_cost(t.edge, tl, t.along_m)),
        ];
        while let Some(Item(c, u)) = heap.pop() {
            if c > dist[u] {
                continue;
            }
            if c >= best {
                break;
            }
            for &(node, from_m, enter) in &entries {
                if u == node && c + enter < best {
                    best = c + enter;
                    best_end = Some((
                        Some(u),
                        Leg {
                            edge: t.edge,
                            from_m,
                            to_m: t.along_m,
                        },
                    ));
                }
            }
            for &h in g.out_edges(u) {
                let w = self.half_cost(h);
                if !w.is_finite() {
                    continue;
                }
                let v = g.head(h);
                let nc = c + w;
                if nc < dist[v] {
                    dist[v] = nc;
                    prev[v] = Some(h);
                    seed_leg[v] = None;
                    heap.push(Item(nc, v));
                }
            }
        }
        let Some((last, tail)) = best_end else {
            return Err(Error::NoRoute);
        };
        let mut legs = Vec::new();
        if let Some(last) = last {
            let mut mid = Vec::new();
            let mut u = last;
            while let Some(h) = prev[u] {
                let l = g.length_m(h.edge);
                let (from_m, to_m) = if h.forward { (0.0, l) } else { (l, 0.0) };
                mid.push(Leg {
                    edge: h.edge,
                    from_m,
                    to_m,
                });
                u = g.tail(h);
            }
            legs.push(seed_leg[u].expect("the chain ends at a seed node"));
            mid.reverse();
            legs.extend(mid);
        }
        legs.push(tail);
        legs.retain(|l| l.length_m() > 1e-6);
        Ok(Route {
            start: s,
            end: t,
            legs,
            cost: best,
        })
    }
}

/// Output layer of the routing pipeline: `data/out/<site>/routes/`.
pub const LAYER: &str = "routes";
/// Graph file in [`LAYER`].
pub const GRAPH_FILE: &str = "piste_graph.json";

/// Summary of [`build`].
#[cfg(feature = "pipeline")]
#[derive(Debug, Clone, Serialize)]
pub struct GraphBuild {
    /// Site identifier.
    pub site: String,
    /// Nodes.
    pub nodes: usize,
    /// Edges.
    pub edges: usize,
    /// Network length, km.
    pub length_km: f64,
    /// One-way length, km.
    pub oneway_km: f64,
    /// Connected components (ignoring one-way restrictions).
    pub components: usize,
    /// Profile coverage.
    pub profiles: crate::dem::ProfileStats,
    /// Total climb of all edges travelled forward, m.
    pub gain_forward_m: f64,
    /// Graph file written.
    pub graph_path: std::path::PathBuf,
    /// Bytes of the graph file.
    pub graph_bytes: u64,
    /// Seconds: reading OSM and terrain, sampling, writing.
    pub seconds: [f64; 3],
}

/// Pipeline (feature `pipeline`): reads `pistes/graph_*.geojson` (P2) and `terrain/dem_5m.tif`
/// (P5), samples a profile on every edge every [`crate::dem::PROFILE_STEP_M`] m, writes
/// `routes/piste_graph.json` and `routes/routes.meta.json`. No download.
#[cfg(feature = "pipeline")]
pub fn build(
    site: &skimap_common::site::Site,
    layout: &skimap_common::layout::DataLayout,
) -> Result<(PisteGraph, GraphBuild)> {
    use std::time::Instant;
    let t0 = Instant::now();
    let pistes = layout.layer_dir(site, "pistes")?;
    let mut g = PisteGraph::load_osm_layer(site.id, &pistes)?;
    let dem_path = layout.layer_dir(site, "terrain")?.join("dem_5m.tif");
    let dem = crate::dem::read_dem(&dem_path)?;
    let t_read = t0.elapsed().as_secs_f64();
    let t1 = Instant::now();
    let profiles = crate::dem::attach_profiles(
        &mut g,
        crate::dem::PROFILE_STEP_M,
        "IGN MNT LiDAR HD 5 m (data/out/<site>/terrain/dem_5m.tif, skimap-terrain P5), bilinéaire",
        |p| crate::dem::elevation(&dem, p),
    );
    let t_sample = t1.elapsed().as_secs_f64();
    let t2 = Instant::now();
    let dir = layout.layer_dir(site, LAYER)?;
    skimap_common::layout::DataLayout::ensure(&dir)?;
    let graph_path = dir.join(GRAPH_FILE);
    let json = g.to_json()?;
    skimap_common::output::write_text(&graph_path, &json)?;
    let t_write = t2.elapsed().as_secs_f64();
    let oneway_km = (0..g.edges().len())
        .filter(|&e| g.edge(e).oneway)
        .map(|e| g.length_m(e))
        .fold(0.0, |a, b| a + b)
        / 1000.0;
    let gain_forward_m = (0..g.edges().len())
        .map(|e| {
            g.gain_loss(HalfEdge {
                edge: e,
                forward: true,
            })
            .0
        })
        .sum();
    let summary = GraphBuild {
        site: site.id.into(),
        nodes: g.nodes().len(),
        edges: g.edges().len(),
        length_km: g.total_length_m() / 1000.0,
        oneway_km,
        components: g.components().1,
        profiles,
        gain_forward_m,
        graph_bytes: json.len() as u64,
        graph_path,
        seconds: [t_read, t_sample, t_write],
    };
    write_meta(site, layout, &summary)?;
    Ok((g, summary))
}

/// Writes `routes/routes.meta.json` (`skimap_common::meta::LayerMeta`): sources (from the
/// `pistes` and `terrain` metadata), the files of the layer that exist (graph, proof
/// routes, matching example, off-piste proof routes of C3), parameters and the build
/// summary.
#[cfg(feature = "pipeline")]
pub fn write_meta(
    site: &skimap_common::site::Site,
    layout: &skimap_common::layout::DataLayout,
    summary: &GraphBuild,
) -> Result<std::path::PathBuf> {
    use skimap_common::meta::{FileMeta, LayerMeta};
    let read_meta = |layer: &str| -> Result<LayerMeta> {
        let p = layout
            .layer_dir(site, layer)?
            .join(LayerMeta::file_name(layer));
        Ok(LayerMeta::read(&p)?)
    };
    let pistes = read_meta("pistes")?;
    let terrain = read_meta("terrain")?;
    let mut meta = LayerMeta::new(LAYER, site.id);
    meta.tickets = ["C1 (#33)", "C2 (#34)", "C3 (#35)", "C5 (#37)", "C7 (#39)"]
        .map(String::from)
        .to_vec();
    meta.valid_from = pistes.valid_from.clone();
    meta.validity = Some(
        "réseau OSM à la date de l'extrait (valid_from) ; relief permanent (levés LiDAR HD)".into(),
    );
    meta.crs = Some("EPSG:4326 (WGS84, longitude, latitude) ; altitudes IGN69".into());
    meta.sources = pistes.sources.clone();
    meta.sources.extend(terrain.sources.first().cloned());
    meta.files.push(
        FileMeta::new(
            GRAPH_FILE,
            "graphe de pistes typé (skimap_core::graph::GraphData) : nœuds, arêtes (sens unique, damage classique / skating, hors damage, usages, difficulté), profil d'altitude par arête",
        )
        .licence("ODbL 1.0 (géométrie et attributs OSM) ; altitudes : IGN Licence Ouverte 2.0")
        .count(summary.edges)
        .properties([
            ("nodes[].pos", "position WGS84 {lon, lat}"),
            ("edges[].from / to", "index des nœuds ; oneway : circulation seulement de from vers to"),
            ("edges[].points", "géométrie WGS84 de from vers to"),
            ("edges[].attrs", "kind (piste, connector), name, osm_way_id, classic, skating (null si inconnu), backcountry, nordic, hike, difficulty, difficulty_max"),
            ("edges[].profile", "dist_m (distance depuis from, m) et elev_m (altitude IGN69, m) tous les 10 m et aux deux extrémités ; filled = stations sans altitude interpolées ; null si aucune altitude (hors couverture LiDAR HD : pente nulle)"),
        ]),
    );
    meta.files.last_mut().expect("pushed").format = Some("JSON (serde)".into());
    meta.files.last_mut().expect("pushed").bytes = Some(summary.graph_bytes);
    let dir = layout.layer_dir(site, LAYER)?;
    if dir.join("routes.geojson").exists() {
        meta.files.push(
            FileMeta::new(
                "routes.geojson",
                "itinéraires de preuve (C1, C5, C7) entre lieux réels, dans les deux sens, trois modèles de coût, et leurs extrémités (exemple `routes`)",
            )
            .licence("ODbL 1.0")
            .properties([
                ("kind", "route ou endpoint"),
                ("from / to / name", "lieux de départ et d'arrivée"),
                ("model", "distance (plus court), temps_classique, temps_skating (coût = temps estimé C5 × facteurs de style)"),
                ("length_m", "longueur de l'itinéraire (haversine)"),
                ("osm_length_sum_m", "somme des length_m OSM des arêtes parcourues (au prorata pour les arêtes partielles)"),
                ("gain_m / loss_m", "dénivelé positif / négatif sur le profil 10 m"),
                ("profile_coverage", "part de la longueur avec profil d'altitude"),
                ("eta_classic_s / eta_skating_s", "temps du modèle physique C5 (paramètres par défaut, neige damée à −5 °C)"),
                ("eta_classic_flat_s", "même modèle sans relief (pente nulle)"),
                ("munter_s / tobler_s", "formules de Munter (4 / 10) et de Tobler sur les mêmes morceaux"),
                ("classic_split_s", "temps classique en montée (> 2 %), plat, descente (< −2 %)"),
                ("turnaround_2027_01_15", "C7 : aller-retour le 15/01/2027, départ 13:00 locale : limite de retour, aller-retour complet possible, point (m) et heure de demi-tour, heure de retour (heure légale)"),
                ("snap_start_m / snap_end_m / snap_m", "distance de rattachement au réseau"),
                ("edges / names", "arêtes parcourues, noms des pistes"),
            ]),
        );
    }
    if dir.join("matching_example.geojson").exists() {
        meta.files.push(
            FileMeta::new(
                "matching_example.geojson",
                "trace GPS synthétique bruitée (C2) sur un itinéraire de preuve : positions vraies, positions GPS, points rattachés en ligne et par Viterbi, alertes (exemple `matching_bench`)",
            )
            .licence("ODbL 1.0 (réseau) ; trace synthétique")
            .properties([
                ("kind", "plan (itinéraire prévu), truth (vraie trace), gps (fixes bruités), online (rattachement en ligne), viterbi (rattachement lissé), alert (début d'alerte)"),
                ("noise", "scénario de bruit"),
                ("t_s", "temps depuis le départ, s"),
            ]),
        );
    }
    let mut offpiste: Vec<String> = std::fs::read_dir(&dir)
        .map_err(|e| Error::io(&dir, e))?
        .filter_map(|e| e.ok()?.file_name().into_string().ok())
        .filter(|n| n.starts_with("offpiste_") && n.ends_with(".geojson"))
        .collect();
    offpiste.sort();
    for name in offpiste {
        meta.files.push(
            FileMeta::new(
                name,
                "itinéraire hors piste de preuve (C3, exemple `offpiste_bench proofs`) : chemin optimal sur la grille de 5 m, tronçons piste / hors piste, points demandés ; grille et sources dans offpiste/offpiste.meta.json",
            )
            .licence("ODbL 1.0 (pistes) ; IGN et Météo-France Licence Ouverte 2.0")
            .properties([
                ("kind", "route (résumé dans summary), stretch (tronçon ; network : groomed, marked ou offpiste), requested_from / requested_to (points demandés)"),
            ]),
        );
    }
    meta.parameters = json!({
        "profile_step_m": crate::dem::PROFILE_STEP_M,
        "elevation_sampling": "bilinéaire sur le MNT 5 m Lambert-93 (projection WGS84 → Lambert-93 de skimap-common)",
        "style_policy_default": StylePolicy::default(),
    });
    meta.stats = serde_json::to_value(summary).map_err(|e| Error::json("stats", e))?;
    Ok(meta.write(&dir)?)
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::graph::tests::{toy, toy_plane};

    fn dist_router(g: &PisteGraph, style: Style) -> Router<'_, Distance> {
        Router::new(
            g,
            Distance {
                style,
                policy: StylePolicy::default(),
            },
        )
    }

    #[test]
    fn oneway_forces_the_detour() {
        let g = toy();
        let r = dist_router(&g, Style::Classic);
        let p = |x, y| toy_plane().lonlat([x, y]);
        // 0 → 2 along e0, e1 (one-way allowed): 2000 m.
        let a = r.route(p(10.0, 5.0), p(1990.0, 5.0), 50.0).unwrap();
        assert!((a.length_m() - 1980.0).abs() < 2.0, "{}", a.length_m());
        assert_eq!(
            a.legs.iter().map(|l| l.edge).collect::<Vec<_>>(),
            vec![0, 1]
        );
        // 2 → 0: e1 is one-way, detour e3, e4, e2, e0 = 500 + 1000 + 500 + 990.
        let b = r.route(p(1990.0, 5.0), p(10.0, 5.0), 50.0).unwrap();
        assert_eq!(
            b.legs.iter().map(|l| l.edge).collect::<Vec<_>>(),
            vec![1, 3, 4, 2, 0]
        );
        assert!((b.length_m() - 3000.0).abs() < 2.0, "{}", b.length_m());
        assert!((b.cost - b.length_m()).abs() < 1e-6);
        // The polyline starts and ends at the snapped points.
        let pl = b.polyline(&g);
        assert!(pl[0].haversine_m(b.start.pos) < 0.01);
        assert!(pl.last().unwrap().haversine_m(b.end.pos) < 0.01);
    }

    #[test]
    fn same_edge_both_directions() {
        let g = toy();
        let r = dist_router(&g, Style::Classic);
        let p = |x, y| toy_plane().lonlat([x, y]);
        let a = r.route(p(200.0, 0.0), p(700.0, 0.0), 10.0).unwrap();
        assert_eq!(a.legs.len(), 1);
        assert!((a.length_m() - 500.0).abs() < 1.0);
        let b = r.route(p(700.0, 0.0), p(200.0, 0.0), 10.0).unwrap();
        assert!(b.legs[0].to_m < b.legs[0].from_m);
        // Backward on the one-way e1: around the loop.
        let c = r.route(p(1800.0, 0.0), p(1200.0, 0.0), 10.0).unwrap();
        assert!((c.length_m() - (200.0 + 500.0 + 1000.0 + 500.0 + 200.0)).abs() < 2.0);
    }

    #[test]
    fn style_policy_changes_the_route() {
        let mut data = toy().data().clone();
        // e0 + e1 classic only; the southern loop is skating.
        for e in [0, 1] {
            data.edges[e].attrs.skating = Some(false);
        }
        let g = PisteGraph::new(data).unwrap();
        let p = |x, y| toy_plane().lonlat([x, y]);
        let classic = dist_router(&g, Style::Classic)
            .route(p(1000.0, -5.0), p(2000.0, -10.0), 20.0)
            .unwrap();
        assert!((classic.length_m() - 1015.0).abs() < 2.0);
        // Skater: e1 costs 3 × 1000 m > 1985 m around the south.
        let skating = dist_router(&g, Style::Skating)
            .route(p(1000.0, -5.0), p(2000.0, -10.0), 20.0)
            .unwrap();
        assert!((skating.length_m() - 1985.0).abs() < 2.0);
        assert!(skating.legs.iter().all(|l| l.edge != 1), "{skating:?}");
        // Forbidding skating on classic-only trails.
        let forbid = Router::new(
            &g,
            Distance {
                style: Style::Skating,
                policy: StylePolicy {
                    skating_on_classic_only: f64::INFINITY,
                    ..StylePolicy::default()
                },
            },
        );
        assert!(matches!(
            forbid.route(p(10.0, 0.0), p(1990.0, 0.0), 20.0),
            Err(Error::NoRoute)
        ));
    }

    /// Test model: distance plus 10 m per metre of climb, 2 m per metre of descent.
    struct Climb;

    impl CostModel for Climb {
        fn piece_cost(&self, _: &EdgeAttrs, dist_m: f64, dz_m: f64, _: Option<f64>) -> f64 {
            dist_m + 10.0 * dz_m.max(0.0) + 2.0 * (-dz_m).max(0.0)
        }
    }

    /// Toy graph with a bumpy profile on e0 (ramp of 4 % plus a sinusoid).
    pub(crate) fn bumpy() -> PisteGraph {
        use crate::graph::Profile;
        let mut data = toy().data().clone();
        let n = 100;
        data.edges[0].profile = Some(Profile {
            dist_m: (0..=n).map(|k| k as f32 * 10.0).collect(),
            elev_m: (0..=n)
                .map(|k| 1000.0 + 0.04 * k as f32 * 10.0 + 8.0 * (k as f32 * 0.3).sin())
                .collect(),
            filled: 0,
        });
        PisteGraph::new(data).unwrap()
    }

    #[test]
    fn cached_partial_costs_equal_direct_integration() {
        let g = bumpy();
        let r = Router::new(&g, Climb);
        for (a, b) in [
            (0.0, g.length_m(0)),
            (g.length_m(0), 0.0),
            (123.4, 678.9),
            (678.9, 123.4),
            (15.0, 17.0),
        ] {
            let direct = partial_cost(&g, &Climb, 0, a, b);
            let cached = r.partial_cost(0, a, b);
            assert!(
                (direct - cached).abs() < 1e-7 * direct.max(1.0),
                "{a}->{b}: {direct} vs {cached}"
            );
        }
        // Climb and descent from the profile: forward = len + 10 × gain + 2 × loss.
        let (up, down) = g.edge(0).profile.as_ref().unwrap().gain_loss();
        let fwd = r.half_cost(HalfEdge {
            edge: 0,
            forward: true,
        });
        let bwd = r.half_cost(HalfEdge {
            edge: 0,
            forward: false,
        });
        assert!((fwd - (g.length_m(0) + 10.0 * up + 2.0 * down)).abs() < 1e-6);
        assert!((bwd - (g.length_m(0) + 10.0 * down + 2.0 * up)).abs() < 1e-6);
        assert!(fwd > bwd);
        // An edge without profile is flat: cost = length.
        let h = HalfEdge {
            edge: 4,
            forward: true,
        };
        assert!((r.half_cost(h) - g.length_m(4)).abs() < 1e-9);
    }

    #[test]
    fn snapping_too_far_is_an_error() {
        let g = toy();
        let r = dist_router(&g, Style::Classic);
        let p = |x, y| toy_plane().lonlat([x, y]);
        assert!(matches!(
            r.route(p(500.0, 300.0), p(10.0, 0.0), 100.0),
            Err(Error::NoTrailNearby { .. })
        ));
    }

    #[test]
    #[allow(clippy::needless_range_loop)]
    fn dijkstra_matches_brute_force_on_all_node_pairs() {
        // Floyd–Warshall on half-edge lengths versus the router on node positions.
        let g = toy();
        let r = dist_router(&g, Style::Classic);
        let n = g.nodes().len();
        let mut d = vec![vec![f64::INFINITY; n]; n];
        for (i, row) in d.iter_mut().enumerate() {
            row[i] = 0.0;
        }
        for u in 0..n {
            for &h in g.out_edges(u) {
                let v = g.head(h);
                d[u][v] = d[u][v].min(g.length_m(h.edge));
            }
        }
        for k in 0..n {
            for i in 0..n {
                for j in 0..n {
                    if d[i][k] + d[k][j] < d[i][j] {
                        d[i][j] = d[i][k] + d[k][j];
                    }
                }
            }
        }
        for i in 0..n {
            for j in 0..n {
                if i == j {
                    continue;
                }
                let route = r.route(g.nodes()[i].pos, g.nodes()[j].pos, 1.0).unwrap();
                assert!(
                    (route.cost - d[i][j]).abs() < 1e-6,
                    "{i}->{j}: {} vs {}",
                    route.cost,
                    d[i][j]
                );
                assert!((route.length_m() - d[i][j]).abs() < 1e-6);
            }
        }
    }
}
