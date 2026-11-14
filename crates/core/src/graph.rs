//! Typed trail graph shared by routing (C1), map matching (C2) and the time model (C5).
//!
//! The graph is built on the pipeline side from the `skimap-osm` outputs
//! (`data/out/<site>/pistes/graph_edges.geojson` and `graph_nodes.geojson`, P2 #15) by
//! [`PisteGraph::from_geojson`], then, on the pipeline side again, enriched with an
//! elevation profile per edge sampled from the terrain model (P5 #18, feature `pipeline`,
//! [`crate::dem`]). It is serialised as JSON ([`GraphData`]): the phone loads it with
//! [`PisteGraph::from_json`] and never reads the 5 m raster.
//!
//! Geometry: positions are WGS84; lengths are haversine sums (the same as `length_m` in
//! the OSM layer); projections onto segments use a local equirectangular plane centred on
//! the graph (error below 0.1 % over a 20 km site).

use std::path::Path;

use rstar::RTree;
use rstar::primitives::{GeomWithData, Line};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use skimap_common::geom::LonLat;

use crate::error::{Error, Result};

/// Metres per degree of latitude (spherical Earth, R = 6 371 km), as in `skimap-osm`.
pub const M_PER_DEG_LAT: f64 = 111_195.0;

/// Kind of edge.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EdgeKind {
    /// Part of an OSM way.
    Piste,
    /// Junction of at most 10 m added by `skimap-osm` between a dead end and a trail.
    Connector,
}

/// Trail difficulty from the colour of the OSM relations (`skimap-osm`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Difficulty {
    /// Green.
    Novice,
    /// Blue.
    Easy,
    /// Red.
    Intermediate,
    /// Black.
    Advanced,
}

impl Difficulty {
    fn parse(s: &str) -> Option<Self> {
        match s {
            "novice" => Some(Self::Novice),
            "easy" => Some(Self::Easy),
            "intermediate" => Some(Self::Intermediate),
            "advanced" | "expert" | "freeride" => Some(Self::Advanced),
            _ => None,
        }
    }
}

/// Attributes of an edge, from the OSM layer.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EdgeAttrs {
    /// Piste or connector.
    pub kind: EdgeKind,
    /// Trail name (way name, else relation names joined by « / »).
    pub name: Option<String>,
    /// OSM way identifier (none for connectors).
    pub osm_way_id: Option<i64>,
    /// Classic tracks (`piste:grooming` contains `classic`); `None` when unknown.
    pub classic: Option<bool>,
    /// Groomed for skating; `None` when unknown.
    pub skating: Option<bool>,
    /// Not groomed (`piste:grooming=backcountry`): tracks made by skiers.
    pub backcountry: bool,
    /// Used as a Nordic ski trail.
    pub nordic: bool,
    /// Used as a snowshoe or winter walking trail.
    pub hike: bool,
    /// Easiest difficulty of the relations containing the way.
    pub difficulty: Option<Difficulty>,
    /// Hardest difficulty of the same relations.
    pub difficulty_max: Option<Difficulty>,
}

impl EdgeAttrs {
    /// Snowshoe or walking trail without Nordic use.
    pub fn hike_only(&self) -> bool {
        self.hike && !self.nordic && self.kind == EdgeKind::Piste
    }
}

/// Elevation profile of an edge, from its start node to its end node.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Profile {
    /// Distance from the start of the edge, metres, strictly increasing from 0 to the
    /// edge length.
    pub dist_m: Vec<f32>,
    /// Terrain elevation, metres (IGN69).
    pub elev_m: Vec<f32>,
    /// Stations without terrain value (outside LiDAR HD coverage), filled by linear
    /// interpolation along the edge.
    pub filled: u32,
}

impl Profile {
    /// Cumulative climb and descent from start to end, metres.
    pub fn gain_loss(&self) -> (f64, f64) {
        let (mut up, mut down) = (0.0, 0.0);
        for w in self.elev_m.windows(2) {
            let dz = f64::from(w[1] - w[0]);
            if dz > 0.0 { up += dz } else { down -= dz }
        }
        (up, down)
    }

    /// Elevation at a distance along the edge (linear between stations, clamped).
    pub fn elevation_at(&self, d: f64) -> f64 {
        let i = self.dist_m.partition_point(|&x| f64::from(x) <= d);
        if i == 0 {
            return f64::from(self.elev_m[0]);
        }
        if i >= self.dist_m.len() {
            return f64::from(*self.elev_m.last().expect("non-empty profile"));
        }
        let (d0, d1) = (f64::from(self.dist_m[i - 1]), f64::from(self.dist_m[i]));
        let (z0, z1) = (f64::from(self.elev_m[i - 1]), f64::from(self.elev_m[i]));
        z0 + (z1 - z0) * (d - d0) / (d1 - d0)
    }
}

/// A node of the graph.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Node {
    /// Position.
    pub pos: LonLat,
    /// OSM node identifier, when the node is an OSM node.
    pub osm_node_id: Option<i64>,
}

/// An edge of the graph.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Edge {
    /// Start node.
    pub from: usize,
    /// End node.
    pub to: usize,
    /// Travel only from `from` to `to`.
    pub oneway: bool,
    /// Geometry from `from` to `to`.
    pub points: Vec<LonLat>,
    /// Attributes.
    pub attrs: EdgeAttrs,
    /// Elevation profile, when the terrain model covers the edge.
    pub profile: Option<Profile>,
}

/// Serialisable content of a [`PisteGraph`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GraphData {
    /// Site identifier.
    pub site: String,
    /// Nodes.
    pub nodes: Vec<Node>,
    /// Edges.
    pub edges: Vec<Edge>,
    /// Terrain model used for the profiles, if any.
    pub elevation_source: Option<String>,
}

/// Directed half-edge: an edge travelled forward (`from` → `to`) or backward.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HalfEdge {
    /// Edge index.
    pub edge: usize,
    /// Forward direction.
    pub forward: bool,
}

/// Local equirectangular plane, metres, centred on the graph.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LocalPlane {
    origin: LonLat,
    kx: f64,
}

impl LocalPlane {
    /// Plane centred on `origin`.
    pub fn new(origin: LonLat) -> Self {
        Self {
            origin,
            kx: M_PER_DEG_LAT * origin.lat.to_radians().cos(),
        }
    }

    /// Plane coordinates of a position.
    pub fn xy(&self, p: LonLat) -> [f64; 2] {
        [
            (p.lon - self.origin.lon) * self.kx,
            (p.lat - self.origin.lat) * M_PER_DEG_LAT,
        ]
    }

    /// Position of plane coordinates.
    pub fn lonlat(&self, xy: [f64; 2]) -> LonLat {
        LonLat::new(
            self.origin.lon + xy[0] / self.kx,
            self.origin.lat + xy[1] / M_PER_DEG_LAT,
        )
    }
}

/// A point of the network: a position along an edge.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Snap {
    /// Edge index.
    pub edge: usize,
    /// Distance from the start of the edge, metres.
    pub along_m: f64,
    /// Position on the edge.
    pub pos: LonLat,
    /// Distance from the query point, metres (plane).
    pub dist_m: f64,
}

type SegIndex = RTree<GeomWithData<Line<[f64; 2]>, (u32, u32)>>;

/// The trail graph with its derived indexes.
#[derive(Debug, Clone)]
pub struct PisteGraph {
    data: GraphData,
    /// Cumulative haversine distance at each vertex of each edge.
    cum: Vec<Vec<f64>>,
    out: Vec<Vec<HalfEdge>>,
    incident: Vec<Vec<HalfEdge>>,
    plane: LocalPlane,
    index: SegIndex,
}

impl PisteGraph {
    /// Builds the derived indexes (cumulative lengths, adjacency, segment R-tree).
    pub fn new(data: GraphData) -> Result<Self> {
        let n = data.nodes.len();
        if n == 0 {
            return Err(Error::format("graph", "no node"));
        }
        let (slon, slat) = data
            .nodes
            .iter()
            .fold((0.0, 0.0), |(a, b), nd| (a + nd.pos.lon, b + nd.pos.lat));
        let plane = LocalPlane::new(LonLat::new(slon / n as f64, slat / n as f64));
        let mut cum = Vec::with_capacity(data.edges.len());
        let mut out = vec![Vec::new(); n];
        let mut incident = vec![Vec::new(); n];
        let mut segs = Vec::new();
        for (i, e) in data.edges.iter().enumerate() {
            if e.from >= n || e.to >= n || e.points.len() < 2 {
                return Err(Error::format(
                    "graph",
                    format!("edge {i}: bad nodes or fewer than 2 points"),
                ));
            }
            let mut c = Vec::with_capacity(e.points.len());
            let mut acc = 0.0;
            c.push(0.0);
            for w in e.points.windows(2) {
                acc += w[0].haversine_m(w[1]);
                c.push(acc);
            }
            cum.push(c);
            out[e.from].push(HalfEdge {
                edge: i,
                forward: true,
            });
            if !e.oneway {
                out[e.to].push(HalfEdge {
                    edge: i,
                    forward: false,
                });
            }
            incident[e.from].push(HalfEdge {
                edge: i,
                forward: true,
            });
            incident[e.to].push(HalfEdge {
                edge: i,
                forward: false,
            });
            for (k, w) in e.points.windows(2).enumerate() {
                let line = Line::new(plane.xy(w[0]), plane.xy(w[1]));
                segs.push(GeomWithData::new(line, (i as u32, k as u32)));
            }
        }
        Ok(Self {
            data,
            cum,
            out,
            incident,
            plane,
            index: RTree::bulk_load(segs),
        })
    }

    /// Serialisable content.
    pub fn data(&self) -> &GraphData {
        &self.data
    }

    /// Nodes.
    pub fn nodes(&self) -> &[Node] {
        &self.data.nodes
    }

    /// Edges.
    pub fn edges(&self) -> &[Edge] {
        &self.data.edges
    }

    /// Edge by index.
    pub fn edge(&self, i: usize) -> &Edge {
        &self.data.edges[i]
    }

    /// Length of an edge, metres (haversine).
    pub fn length_m(&self, edge: usize) -> f64 {
        *self.cum[edge].last().expect("at least two points")
    }

    /// Cumulative distance at each vertex of an edge.
    pub fn cumulative_m(&self, edge: usize) -> &[f64] {
        &self.cum[edge]
    }

    /// Total length of the network, metres.
    pub fn total_length_m(&self) -> f64 {
        (0..self.data.edges.len()).map(|e| self.length_m(e)).sum()
    }

    /// Half-edges that can be travelled from a node (one-way edges only forward).
    pub fn out_edges(&self, node: usize) -> &[HalfEdge] {
        &self.out[node]
    }

    /// All half-edges leaving a node, ignoring one-way restrictions.
    pub fn incident_edges(&self, node: usize) -> &[HalfEdge] {
        &self.incident[node]
    }

    /// Node at the end of a half-edge.
    pub fn head(&self, h: HalfEdge) -> usize {
        let e = &self.data.edges[h.edge];
        if h.forward { e.to } else { e.from }
    }

    /// Node at the start of a half-edge.
    pub fn tail(&self, h: HalfEdge) -> usize {
        let e = &self.data.edges[h.edge];
        if h.forward { e.from } else { e.to }
    }

    /// Local plane used for projections.
    pub fn plane(&self) -> &LocalPlane {
        &self.plane
    }

    /// Position at a distance along an edge (clamped to the edge).
    pub fn point_at(&self, edge: usize, along_m: f64) -> LonLat {
        let c = &self.cum[edge];
        let pts = &self.data.edges[edge].points;
        let d = along_m.clamp(0.0, *c.last().expect("two points"));
        let i = c.partition_point(|&x| x <= d).clamp(1, c.len() - 1);
        let span = c[i] - c[i - 1];
        let t = if span > 0.0 {
            (d - c[i - 1]) / span
        } else {
            0.0
        };
        let (a, b) = (pts[i - 1], pts[i]);
        LonLat::new(a.lon + (b.lon - a.lon) * t, a.lat + (b.lat - a.lat) * t)
    }

    /// Polyline of an edge between two distances (reversed when `b < a`).
    pub fn sub_polyline(&self, edge: usize, a: f64, b: f64) -> Vec<LonLat> {
        let c = &self.cum[edge];
        let pts = &self.data.edges[edge].points;
        let (lo, hi) = if a <= b { (a, b) } else { (b, a) };
        let mut out = vec![self.point_at(edge, lo)];
        for (k, &d) in c.iter().enumerate() {
            if d > lo && d < hi {
                out.push(pts[k]);
            }
        }
        out.push(self.point_at(edge, hi));
        if a > b {
            out.reverse();
        }
        out
    }

    fn snap_on(&self, edge: usize, seg: usize, q: [f64; 2]) -> Snap {
        let pts = &self.data.edges[edge].points;
        let (a, b) = (self.plane.xy(pts[seg]), self.plane.xy(pts[seg + 1]));
        let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
        let l2 = dx * dx + dy * dy;
        let t = if l2 > 0.0 {
            (((q[0] - a[0]) * dx + (q[1] - a[1]) * dy) / l2).clamp(0.0, 1.0)
        } else {
            0.0
        };
        let p = [a[0] + t * dx, a[1] + t * dy];
        let c = &self.cum[edge];
        Snap {
            edge,
            along_m: c[seg] + t * (c[seg + 1] - c[seg]),
            pos: self.plane.lonlat(p),
            dist_m: ((q[0] - p[0]).powi(2) + (q[1] - p[1]).powi(2)).sqrt(),
        }
    }

    /// Nearest point of the network within `max_m` metres.
    pub fn snap(&self, p: LonLat, max_m: f64) -> Option<Snap> {
        let q = self.plane.xy(p);
        let nn = self.index.nearest_neighbor(&q)?;
        let (e, s) = nn.data;
        let snap = self.snap_on(e as usize, s as usize, q);
        (snap.dist_m <= max_m).then_some(snap)
    }

    /// Nearest point of each edge within `radius_m`, closest first, at most `max_k`.
    pub fn candidates(&self, p: LonLat, radius_m: f64, max_k: usize) -> Vec<Snap> {
        let q = self.plane.xy(p);
        let mut best: Vec<Snap> = Vec::new();
        for g in self.index.locate_within_distance(q, radius_m * radius_m) {
            let (e, s) = g.data;
            let snap = self.snap_on(e as usize, s as usize, q);
            match best.iter_mut().find(|b| b.edge == snap.edge) {
                Some(b) if b.dist_m > snap.dist_m => *b = snap,
                Some(_) => {}
                None => best.push(snap),
            }
        }
        best.sort_by(|a, b| a.dist_m.total_cmp(&b.dist_m));
        best.truncate(max_k);
        best
    }

    /// Connected components ignoring one-way restrictions: component of each node and
    /// number of components.
    pub fn components(&self) -> (Vec<usize>, usize) {
        let n = self.data.nodes.len();
        let mut comp = vec![usize::MAX; n];
        let mut k = 0;
        for s in 0..n {
            if comp[s] != usize::MAX {
                continue;
            }
            let mut stack = vec![s];
            comp[s] = k;
            while let Some(u) = stack.pop() {
                for h in &self.incident[u] {
                    let v = self.head(*h);
                    if comp[v] == usize::MAX {
                        comp[v] = k;
                        stack.push(v);
                    }
                }
            }
            k += 1;
        }
        (comp, k)
    }

    /// Climb and descent of an edge travelled in a direction, metres (0 without profile).
    pub fn gain_loss(&self, h: HalfEdge) -> (f64, f64) {
        match &self.data.edges[h.edge].profile {
            Some(p) => {
                let (up, down) = p.gain_loss();
                if h.forward { (up, down) } else { (down, up) }
            }
            None => (0.0, 0.0),
        }
    }

    /// Stations (distance along the edge, elevation) between `a` and `b` metres along an
    /// edge, in travel order; elevations are `None` without profile. The first and last
    /// stations are at `a` and `b`.
    pub fn stations(&self, edge: usize, a: f64, b: f64) -> Vec<(f64, Option<f64>)> {
        let (lo, hi) = if a <= b { (a, b) } else { (b, a) };
        let mut out = Vec::new();
        match &self.data.edges[edge].profile {
            Some(p) => {
                out.push((lo, Some(p.elevation_at(lo))));
                for (d, z) in p.dist_m.iter().zip(&p.elev_m) {
                    let d = f64::from(*d);
                    if d > lo && d < hi {
                        out.push((d, Some(f64::from(*z))));
                    }
                }
                out.push((hi, Some(p.elevation_at(hi))));
            }
            None => {
                out.push((lo, None));
                for &d in &self.cum[edge] {
                    if d > lo && d < hi {
                        out.push((d, None));
                    }
                }
                out.push((hi, None));
            }
        }
        if a > b {
            out.reverse();
        }
        out
    }

    /// Reads the JSON written by [`PisteGraph::to_json`].
    pub fn from_json(s: &str) -> Result<Self> {
        let data: GraphData = serde_json::from_str(s).map_err(|e| Error::json("graph", e))?;
        Self::new(data)
    }

    /// Serialises the graph (nodes, edges, profiles).
    pub fn to_json(&self) -> Result<String> {
        serde_json::to_string(&self.data).map_err(|e| Error::json("graph", e))
    }

    /// Mutable access to the edges, for the pipeline (profiles); indexes stay valid as
    /// long as geometry and topology are not changed.
    #[cfg(feature = "pipeline")]
    pub(crate) fn set_profiles(&mut self, profiles: Vec<Option<Profile>>, source: String) {
        for (e, p) in self.data.edges.iter_mut().zip(profiles) {
            e.profile = p;
        }
        self.data.elevation_source = Some(source);
    }

    /// Builds the graph from the `skimap-osm` GeoJSON (`graph_edges.geojson`,
    /// `graph_nodes.geojson`).
    pub fn from_geojson(site: &str, edges: &str, nodes: &str) -> Result<Self> {
        let nodes_v: Value = serde_json::from_str(nodes).map_err(|e| Error::json("nodes", e))?;
        let edges_v: Value = serde_json::from_str(edges).map_err(|e| Error::json("edges", e))?;
        let feats = |v: &Value, ctx: &str| -> Result<Vec<Value>> {
            v.get("features")
                .and_then(Value::as_array)
                .cloned()
                .ok_or_else(|| Error::format(ctx, "no features array"))
        };
        let coords = |g: &Value| -> Option<Vec<LonLat>> {
            let c = g.get("coordinates")?.as_array()?;
            c.iter()
                .map(|p| {
                    let a = p.as_array()?;
                    Some(LonLat::new(a.first()?.as_f64()?, a.get(1)?.as_f64()?))
                })
                .collect()
        };
        let nf = feats(&nodes_v, "nodes")?;
        let mut node_slots: Vec<Option<Node>> = vec![None; nf.len()];
        for f in &nf {
            let p = &f["properties"];
            let i = p["node"]
                .as_u64()
                .ok_or_else(|| Error::format("nodes", "missing node index"))?
                as usize;
            let c = f["geometry"]["coordinates"]
                .as_array()
                .ok_or_else(|| Error::format("nodes", format!("node {i}: no point")))?;
            let pos = LonLat::new(
                c.first().and_then(Value::as_f64).unwrap_or(f64::NAN),
                c.get(1).and_then(Value::as_f64).unwrap_or(f64::NAN),
            );
            if i >= node_slots.len() {
                node_slots.resize(i + 1, None);
            }
            node_slots[i] = Some(Node {
                pos,
                osm_node_id: p["osm_node_id"].as_i64(),
            });
        }
        let nodes: Vec<Node> = node_slots
            .into_iter()
            .enumerate()
            .map(|(i, n)| n.ok_or_else(|| Error::format("nodes", format!("node {i} missing"))))
            .collect::<Result<_>>()?;
        let ef = feats(&edges_v, "edges")?;
        let mut edges: Vec<Option<Edge>> = vec![None; ef.len()];
        for f in &ef {
            let p = &f["properties"];
            let idx = p["edge"]
                .as_u64()
                .ok_or_else(|| Error::format("edges", "missing edge index"))?
                as usize;
            let ctx = format!("edge {idx}");
            let from = p["from"]
                .as_u64()
                .ok_or_else(|| Error::format(&ctx, "missing from"))?
                as usize;
            let to = p["to"]
                .as_u64()
                .ok_or_else(|| Error::format(&ctx, "missing to"))? as usize;
            let points = coords(&f["geometry"]).ok_or_else(|| Error::format(&ctx, "geometry"))?;
            let kind = match p["kind"].as_str() {
                Some("connector") => EdgeKind::Connector,
                Some("piste") => EdgeKind::Piste,
                other => return Err(Error::format(&ctx, format!("unknown kind {other:?}"))),
            };
            let uses: Vec<&str> = p["uses"]
                .as_array()
                .map(|a| a.iter().filter_map(Value::as_str).collect())
                .unwrap_or_default();
            let attrs = EdgeAttrs {
                kind,
                name: p["name"].as_str().map(str::to_owned),
                osm_way_id: p["osm_way_id"].as_i64(),
                classic: p["classic"].as_bool(),
                skating: p["skating"].as_bool(),
                backcountry: p["backcountry"].as_bool().unwrap_or(false),
                nordic: uses.contains(&"nordic") || kind == EdgeKind::Connector,
                hike: uses.iter().any(|u| *u == "hike" || *u == "snowshoe"),
                difficulty: p["difficulty"].as_str().and_then(Difficulty::parse),
                difficulty_max: p["difficulty_max"].as_str().and_then(Difficulty::parse),
            };
            if idx >= edges.len() {
                edges.resize(idx + 1, None);
            }
            edges[idx] = Some(Edge {
                from,
                to,
                oneway: p["oneway"].as_bool().unwrap_or(false),
                points,
                attrs,
                profile: None,
            });
        }
        let edges = edges
            .into_iter()
            .enumerate()
            .map(|(i, e)| e.ok_or_else(|| Error::format("edges", format!("edge {i} missing"))))
            .collect::<Result<Vec<_>>>()?;
        Self::new(GraphData {
            site: site.to_owned(),
            nodes,
            edges,
            elevation_source: None,
        })
    }

    /// Reads `graph_edges.geojson` and `graph_nodes.geojson` from a `pistes` layer
    /// directory.
    pub fn load_osm_layer(site: &str, dir: &Path) -> Result<Self> {
        let read = |name: &str| {
            let p = dir.join(name);
            std::fs::read_to_string(&p).map_err(|e| Error::io(p, e))
        };
        Self::from_geojson(
            site,
            &read("graph_edges.geojson")?,
            &read("graph_nodes.geojson")?,
        )
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// A small synthetic graph, around 46.6 N 6.1 E, in metres from the origin:
    ///
    /// ```text
    /// 0 ──(e0, 1000 m)── 1 ──(e1, 1000 m, one-way →)── 2
    ///                    │                              │
    ///                (e2, 500 m)                    (e3, 500 m)
    ///                    │                              │
    ///                    3 ─────────(e4, 1000 m)─────── 4
    /// ```
    pub(crate) fn toy_plane() -> LocalPlane {
        LocalPlane::new(LonLat::new(6.1, 46.6))
    }

    pub(crate) fn toy() -> PisteGraph {
        let plane = toy_plane();
        let at = |x: f64, y: f64| plane.lonlat([x, y]);
        let node = |x, y| Node {
            pos: at(x, y),
            osm_node_id: None,
        };
        let nodes = vec![
            node(0.0, 0.0),
            node(1000.0, 0.0),
            node(2000.0, 0.0),
            node(1000.0, -500.0),
            node(2000.0, -500.0),
        ];
        let attrs = |name: &str| EdgeAttrs {
            kind: EdgeKind::Piste,
            name: Some(name.into()),
            osm_way_id: None,
            classic: Some(true),
            skating: Some(true),
            backcountry: false,
            nordic: true,
            hike: false,
            difficulty: Some(Difficulty::Easy),
            difficulty_max: Some(Difficulty::Easy),
        };
        let edge = |from: usize, to: usize, oneway, name: &str| Edge {
            from,
            to,
            oneway,
            points: {
                // Intermediate vertex every 100 m.
                let (a, b) = (plane.xy(nodes[from].pos), plane.xy(nodes[to].pos));
                let n = ((b[0] - a[0]).hypot(b[1] - a[1]) / 100.0).round() as usize;
                (0..=n)
                    .map(|k| {
                        let t = k as f64 / n as f64;
                        plane.lonlat([a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t])
                    })
                    .collect()
            },
            attrs: attrs(name),
            profile: None,
        };
        let edges = vec![
            edge(0, 1, false, "e0"),
            edge(1, 2, true, "e1"),
            edge(1, 3, false, "e2"),
            edge(2, 4, false, "e3"),
            edge(3, 4, false, "e4"),
        ];
        PisteGraph::new(GraphData {
            site: "toy".into(),
            nodes,
            edges,
            elevation_source: None,
        })
        .unwrap()
    }

    #[test]
    fn lengths_snap_and_candidates() {
        let g = toy();
        assert_eq!(g.edges().len(), 5);
        for (e, want) in [(0, 1000.0), (1, 1000.0), (2, 500.0), (4, 1000.0)] {
            let l = g.length_m(e);
            assert!((l - want).abs() < 1.0, "edge {e}: {l}");
        }
        let p = toy_plane().lonlat([400.0, 30.0]);
        let s = g.snap(p, 50.0).unwrap();
        assert_eq!(s.edge, 0);
        assert!((s.along_m - 400.0).abs() < 1.0, "{s:?}");
        assert!((s.dist_m - 30.0).abs() < 0.01);
        assert!(g.snap(p, 20.0).is_none());
        // Near node 1: three edges within 60 m.
        let c = g.candidates(toy_plane().lonlat([1010.0, -20.0]), 60.0, 8);
        let mut es: Vec<usize> = c.iter().map(|s| s.edge).collect();
        es.sort();
        assert_eq!(es, vec![0, 1, 2]);
        assert!(c[0].dist_m <= c[1].dist_m);
    }

    #[test]
    fn oneway_adjacency_and_components() {
        let g = toy();
        assert!(g.out_edges(2).iter().all(|h| h.edge != 1));
        assert!(g.out_edges(1).iter().any(|h| h.edge == 1 && h.forward));
        assert_eq!(g.incident_edges(2).len(), 2);
        assert_eq!(g.components().1, 1);
    }

    #[test]
    fn sub_polyline_and_stations_follow_direction() {
        let g = toy();
        let fw = g.sub_polyline(0, 150.0, 450.0);
        let bw = g.sub_polyline(0, 450.0, 150.0);
        assert_eq!(fw.len(), 5);
        assert_eq!(fw.first(), bw.last());
        let st = g.stations(0, 450.0, 150.0);
        assert_eq!(st.first().unwrap().0, 450.0);
        assert_eq!(st.last().unwrap().0, 150.0);
    }

    #[test]
    fn profile_gain_loss_and_interpolation() {
        let p = Profile {
            dist_m: vec![0.0, 10.0, 20.0, 30.0],
            elev_m: vec![100.0, 102.0, 101.0, 105.0],
            filled: 0,
        };
        let (up, down) = p.gain_loss();
        assert!((up - 6.0).abs() < 1e-9 && (down - 1.0).abs() < 1e-9);
        assert!((p.elevation_at(5.0) - 101.0).abs() < 1e-6);
        assert!((p.elevation_at(-1.0) - 100.0).abs() < 1e-6);
        assert!((p.elevation_at(99.0) - 105.0).abs() < 1e-6);
    }

    #[test]
    fn json_round_trip() {
        let g = toy();
        let back = PisteGraph::from_json(&g.to_json().unwrap()).unwrap();
        assert_eq!(back.data(), g.data());
    }

    #[test]
    fn geojson_schema_of_osm_layer() {
        let edges = r#"{"type":"FeatureCollection","features":[
          {"type":"Feature","geometry":{"type":"LineString","coordinates":[[6.0,46.0],[6.001,46.0]]},
           "properties":{"edge":0,"from":0,"to":1,"kind":"piste","oneway":true,"name":"A","osm_way_id":5,
             "classic":true,"skating":false,"backcountry":false,"uses":["nordic"],"difficulty":"easy","difficulty_max":"advanced"}},
          {"type":"Feature","geometry":{"type":"LineString","coordinates":[[6.001,46.0],[6.001,46.00005]]},
           "properties":{"edge":1,"from":1,"to":2,"kind":"connector","length_m":5.6}}]}"#;
        let nodes = r#"{"type":"FeatureCollection","features":[
          {"type":"Feature","geometry":{"type":"Point","coordinates":[6.0,46.0]},"properties":{"node":0,"osm_node_id":1}},
          {"type":"Feature","geometry":{"type":"Point","coordinates":[6.001,46.0]},"properties":{"node":1,"osm_node_id":2}},
          {"type":"Feature","geometry":{"type":"Point","coordinates":[6.001,46.00005]},"properties":{"node":2,"osm_node_id":3}}]}"#;
        let g = PisteGraph::from_geojson("t", edges, nodes).unwrap();
        let e0 = g.edge(0);
        assert!(e0.oneway);
        assert_eq!(e0.attrs.classic, Some(true));
        assert_eq!(e0.attrs.skating, Some(false));
        assert_eq!(e0.attrs.difficulty, Some(Difficulty::Easy));
        assert_eq!(e0.attrs.difficulty_max, Some(Difficulty::Advanced));
        let e1 = g.edge(1);
        assert_eq!(e1.attrs.kind, EdgeKind::Connector);
        assert_eq!(e1.attrs.classic, None);
        assert!(e1.attrs.nordic);
        assert!((g.length_m(0) - 77.2).abs() < 0.5, "{}", g.length_m(0));
    }
}
