//! Routing graph of the pistes: ways split at junctions, plus short connectors that close
//! mapping gaps.
//!
//! - Graph nodes are OSM nodes where pistes meet (shared by two ways, or used twice by one
//!   way), way ends, and the targets of connectors.
//! - An edge is the part of one way between two consecutive graph nodes; it keeps the index
//!   of its piste so that every attribute can be joined back.
//! - A **connector** joins a dead end to the nearest vertex of another piste when they are
//!   closer than a threshold but not joined in OSM (a frequent mapping slip). Connectors are
//!   flagged so that a router can trust or ignore them.

use std::collections::{BTreeMap, HashMap, HashSet};

use rstar::primitives::{GeomWithData, Rectangle};
use rstar::{AABB, RTree};
use skimap_common::geom::LonLat;
use skimap_common::output::round7;

use crate::pistes::{Oneway, PisteSet};

/// Metres per degree of latitude (mean).
pub const M_PER_DEG_LAT: f64 = 111_195.0;

/// Kind of a graph edge.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EdgeKind {
    /// Part of a piste way.
    Piste {
        /// Index of the piste in the [`PisteSet`].
        piste: usize,
    },
    /// Straight connector closing a gap between a dead end and another piste.
    Connector,
}

/// A graph node.
#[derive(Debug, Clone, PartialEq)]
pub struct Node {
    /// OSM node id.
    pub osm_id: i64,
    /// Position.
    pub pos: LonLat,
}

/// A graph edge.
#[derive(Debug, Clone, PartialEq)]
pub struct Edge {
    /// Kind (piste part or connector).
    pub kind: EdgeKind,
    /// Index of the start node.
    pub from: usize,
    /// Index of the end node.
    pub to: usize,
    /// Geometry from `from` to `to` (direction of travel for one-way edges).
    pub points: Vec<LonLat>,
    /// Length in metres.
    pub length_m: f64,
    /// Travel allowed only from `from` to `to`.
    pub oneway: bool,
}

/// Undirected multigraph of the pistes.
#[derive(Debug, Clone, Default)]
pub struct Graph {
    /// Nodes.
    pub nodes: Vec<Node>,
    /// Edges.
    pub edges: Vec<Edge>,
}

/// A path over pistes outside a site that leads from one node of the site network to
/// another one that the site network alone cannot reach from it (see
/// [`Graph::reentry_paths`]).
#[derive(Debug, Clone, PartialEq)]
pub struct Reentry {
    /// Graph node where the path leaves the site network.
    pub from: usize,
    /// Graph node where it comes back.
    pub to: usize,
    /// Length of the outside part, metres.
    pub length_m: f64,
    /// Pistes of the outside part, in path order (indices into the [`PisteSet`]).
    pub pistes: Vec<usize>,
}

/// One-way trap of a graph: a node joined to the network by one-way edges only, all
/// pointing in (nothing can leave it) or all pointing out (nothing can reach it).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OnewayTrap {
    /// Graph node.
    pub node: usize,
    /// `true`: every edge points in (sink); `false`: every edge points out (source).
    pub sink: bool,
}

/// Largest gap closed between two dead ends that are in different components, metres: the
/// width of a road with its verges and snow banks, where piste ways stop on both sides
/// and the road (not a piste) joins them (B166, #166). Only components are joined: a
/// network already connected never gets a shortcut.
pub const DEAD_END_PAIR_MAX_M: f64 = 25.0;

/// A projected point closer than this to a vertex is snapped to it, metres.
pub const VERTEX_SNAP_M: f64 = 1.0;

/// A node to insert into piste `.0`, inside segment `.1` (from vertex `.1` to `.1 + 1`), at
/// fraction `.2` of it: node id `.3`, position `.4`.
type Cut = (usize, usize, f64, i64, LonLat);

type Segment = GeomWithData<Rectangle<[f64; 2]>, (usize, usize)>;

/// Occurrences of each node over all pistes.
fn node_uses(points: &[Vec<(i64, LonLat)>]) -> HashMap<i64, u32> {
    let mut uses: HashMap<i64, u32> = HashMap::new();
    for pts in points {
        for (id, _) in pts {
            *uses.entry(*id).or_default() += 1;
        }
    }
    uses
}

/// Inserts the nodes `cuts` into the pistes.
fn insert_cuts(points: &mut [Vec<(i64, LonLat)>], mut cuts: Vec<Cut>) {
    // Last segment first, and inside a segment the farthest point first, so that every
    // insertion keeps the indices of the next ones.
    cuts.sort_by(|a, b| {
        (b.0, b.1)
            .cmp(&(a.0, a.1))
            .then(b.2.total_cmp(&a.2))
            .then(a.3.cmp(&b.3))
    });
    cuts.dedup_by(|a, b| a.0 == b.0 && a.3 == b.3);
    for (pi, si, _, id, pos) in cuts {
        // At the precision the graph is written with (OSM's, 1e-7°): a node added a few
        // millimetres from another one is that one once written, so it is merged with it
        // here ([`merge_coincident`]) rather than joined by an edge of length 0 (#168).
        let pos = LonLat::new(round7(pos.lon), round7(pos.lat));
        points[pi].insert(si + 1, (id, pos));
    }
}

/// Local plane in metres around `origin` (equirectangular).
fn plane(origin: LonLat) -> impl Fn(LonLat) -> [f64; 2] {
    let k = origin.lat.to_radians().cos();
    move |p: LonLat| {
        [
            (p.lon - origin.lon) * k * M_PER_DEG_LAT,
            (p.lat - origin.lat) * M_PER_DEG_LAT,
        ]
    }
}

/// R-tree of the segments of every piste, in degrees: data `(piste, segment)`.
fn segment_tree(points: &[Vec<(i64, LonLat)>]) -> RTree<Segment> {
    RTree::bulk_load(
        points
            .iter()
            .enumerate()
            .flat_map(|(pi, pts)| {
                pts.windows(2).enumerate().map(move |(si, w)| {
                    Segment::new(
                        Rectangle::from_corners([w[0].1.lon, w[0].1.lat], [w[1].1.lon, w[1].1.lat]),
                        (pi, si),
                    )
                })
            })
            .collect(),
    )
}

/// Nodes at the crossings of segments of two pistes that share no node there, unless one
/// of them is on another level ([`crate::pistes::Piste::layered`]).
fn crossings(set: &PisteSet, points: &[Vec<(i64, LonLat)>], next_id: &mut i64) -> Vec<Cut> {
    let tree = segment_tree(points);
    let mut cuts = Vec::new();
    for (pi, pts) in points.iter().enumerate() {
        if set.pistes[pi].layered {
            continue;
        }
        for (si, w) in pts.windows(2).enumerate() {
            let env = AABB::from_corners([w[0].1.lon, w[0].1.lat], [w[1].1.lon, w[1].1.lat]);
            let mut hits: Vec<(usize, usize)> = tree
                .locate_in_envelope_intersecting(&env)
                .map(|s| s.data)
                .filter(|&(qi, _)| qi > pi && !set.pistes[qi].layered)
                .collect();
            hits.sort_unstable();
            for (qi, qs) in hits {
                let v = &points[qi][qs..qs + 2];
                if [w[0].0, w[1].0]
                    .iter()
                    .any(|id| *id == v[0].0 || *id == v[1].0)
                {
                    continue;
                }
                let xy = plane(w[0].1);
                let (a, b, c, d) = (xy(w[0].1), xy(w[1].1), xy(v[0].1), xy(v[1].1));
                let Some((t, u)) = segment_intersection(a, b, c, d) else {
                    continue;
                };
                let pos = LonLat::new(
                    w[0].1.lon + t * (w[1].1.lon - w[0].1.lon),
                    w[0].1.lat + t * (w[1].1.lat - w[0].1.lat),
                );
                let id = *next_id;
                *next_id -= 1;
                cuts.push((pi, si, t, id, pos));
                cuts.push((qi, qs, u, id, pos));
            }
        }
    }
    cuts
}

/// Fractions `(t, u)` along `a b` and `c d` of their crossing, when they cross strictly
/// inside both.
fn segment_intersection(a: [f64; 2], b: [f64; 2], c: [f64; 2], d: [f64; 2]) -> Option<(f64, f64)> {
    let r = [b[0] - a[0], b[1] - a[1]];
    let s = [d[0] - c[0], d[1] - c[1]];
    let den = r[0] * s[1] - r[1] * s[0];
    if den.abs() < 1e-12 {
        return None;
    }
    let ac = [c[0] - a[0], c[1] - a[1]];
    let t = (ac[0] * s[1] - ac[1] * s[0]) / den;
    let u = (ac[0] * r[1] - ac[1] * r[0]) / den;
    (t > 0.0 && t < 1.0 && u > 0.0 && u < 1.0).then_some((t, u))
}

/// Distance from the origin to the segment `a b` (local plane), and the fraction of the
/// nearest point along it.
fn point_segment(a: [f64; 2], b: [f64; 2]) -> (f64, f64) {
    let d = [b[0] - a[0], b[1] - a[1]];
    let l = d[0] * d[0] + d[1] * d[1];
    let t = if l == 0.0 {
        0.0
    } else {
        (-(a[0] * d[0] + a[1] * d[1]) / l).clamp(0.0, 1.0)
    };
    let q = [a[0] + t * d[0], a[1] + t * d[1]];
    ((q[0] * q[0] + q[1] * q[1]).sqrt(), t)
}

/// Connectors from every dead end to the nearest point of another piste up to `max_m`,
/// and the nodes to insert where that point is inside a segment.
fn dead_end_connectors(
    points: &[Vec<(i64, LonLat)>],
    max_m: f64,
    next_id: &mut i64,
) -> (Vec<Cut>, Vec<(i64, i64)>) {
    let uses = node_uses(points);
    let tree = segment_tree(points);
    let mut cuts = Vec::new();
    let mut connectors = Vec::new();
    for (pi, pts) in points.iter().enumerate() {
        let n = pts.len();
        if n < 2 {
            continue;
        }
        for end in [0, n - 1] {
            let (id, pos) = pts[end];
            if uses[&id] != 1 {
                continue;
            }
            let xy = plane(pos);
            // (distance, piste, segment, fraction) of the nearest point; ties by piste and
            // segment.
            let best = tree
                .locate_in_envelope_intersecting(&search_box(pos, max_m))
                .filter(|s| s.data.0 != pi)
                .map(|s| {
                    let (qi, qs) = s.data;
                    let (a, b) = (xy(points[qi][qs].1), xy(points[qi][qs + 1].1));
                    let (d, t) = point_segment(a, b);
                    (d, qi, qs, t)
                })
                .filter(|(d, ..)| *d <= max_m)
                .min_by(|a, b| a.0.total_cmp(&b.0).then((a.1, a.2).cmp(&(b.1, b.2))));
            let Some((_, qi, qs, t)) = best else {
                continue;
            };
            let (a, b) = (points[qi][qs], points[qi][qs + 1]);
            let seg_m = a.1.haversine_m(b.1);
            let target = if t * seg_m <= VERTEX_SNAP_M {
                a.0
            } else if (1.0 - t) * seg_m <= VERTEX_SNAP_M {
                b.0
            } else {
                let at = LonLat::new(
                    a.1.lon + t * (b.1.lon - a.1.lon),
                    a.1.lat + t * (b.1.lat - a.1.lat),
                );
                let nid = *next_id;
                *next_id -= 1;
                cuts.push((qi, qs, t, nid, at));
                nid
            };
            if target != id {
                connectors.push((id, target));
            }
        }
    }
    (cuts, connectors)
}

/// Union-find over node indices.
struct Components(Vec<usize>);

impl Components {
    fn find(&mut self, mut x: usize) -> usize {
        while self.0[x] != x {
            self.0[x] = self.0[self.0[x]];
            x = self.0[x];
        }
        x
    }

    /// Joins the components of `a` and `b`; `false` when they were one already.
    fn union(&mut self, a: usize, b: usize) -> bool {
        let (ra, rb) = (self.find(a), self.find(b));
        if ra == rb {
            return false;
        }
        self.0[ra.max(rb)] = ra.min(rb);
        true
    }
}

/// Connectors between dead ends of different components up to `max_m` apart, the
/// shortest first: each one joins two components, none is added inside one.
fn dead_end_pairs(
    points: &[Vec<(i64, LonLat)>],
    connectors: &[(i64, i64)],
    max_m: f64,
) -> Vec<(i64, i64)> {
    let mut id_index: HashMap<i64, usize> = HashMap::new();
    for (id, _) in points.iter().flatten() {
        let n = id_index.len();
        id_index.entry(*id).or_insert(n);
    }
    let mut comps = Components((0..id_index.len()).collect());
    for pts in points {
        for w in pts.windows(2) {
            comps.union(id_index[&w[0].0], id_index[&w[1].0]);
        }
    }
    let mut connected: HashMap<i64, u32> = HashMap::new();
    for (a, b) in connectors {
        comps.union(id_index[a], id_index[b]);
        *connected.entry(*a).or_default() += 1;
        *connected.entry(*b).or_default() += 1;
    }
    let uses = node_uses(points);
    let ends: Vec<(i64, LonLat)> = points
        .iter()
        .filter(|pts| pts.len() >= 2)
        .flat_map(|pts| [pts[0], pts[pts.len() - 1]])
        .filter(|(id, _)| uses[id] == 1 && !connected.contains_key(id))
        .collect();
    let tree: RTree<GeomWithData<[f64; 2], usize>> = RTree::bulk_load(
        ends.iter()
            .enumerate()
            .map(|(i, (_, p))| GeomWithData::new([p.lon, p.lat], i))
            .collect(),
    );
    let mut pairs: Vec<(f64, i64, i64)> = Vec::new();
    for (i, (id, pos)) in ends.iter().enumerate() {
        for v in tree.locate_in_envelope(&search_box(*pos, max_m)) {
            let (qid, qpos) = ends[v.data];
            if v.data <= i || qid == *id {
                continue;
            }
            let d = pos.haversine_m(qpos);
            if d <= max_m {
                pairs.push((d, (*id).min(qid), (*id).max(qid)));
            }
        }
    }
    pairs.sort_by(|a, b| a.0.total_cmp(&b.0).then((a.1, a.2).cmp(&(b.1, b.2))));
    pairs
        .into_iter()
        .filter(|&(_, a, b)| comps.union(id_index[&a], id_index[&b]))
        .map(|(_, a, b)| (a, b))
        .collect()
}

/// Makes one node of the nodes that are at the very same place (same coordinates), unless
/// one of them is on a piste on another level ([`crate::pistes::Piste::layered`]): two
/// OSM nodes duplicated by the mapper, or two crossing nodes put on one way where it
/// crosses two ways drawn on top of each other. Kept apart, they would give an edge of
/// zero length, which no elevation profile can describe (`skimap-core` rejects the
/// graph). The kept id is the smallest OSM id, or the added node made first. Returns the
/// replaced ids and their replacement.
fn merge_coincident(set: &PisteSet, points: &mut [Vec<(i64, LonLat)>]) -> HashMap<i64, i64> {
    let layered: HashSet<i64> = points
        .iter()
        .enumerate()
        .filter(|(pi, _)| set.pistes[*pi].layered)
        .flat_map(|(_, pts)| pts.iter().map(|(id, _)| *id))
        .collect();
    let mut keep: HashMap<(u64, u64), i64> = HashMap::new();
    for (id, pos) in points.iter().flatten() {
        if layered.contains(id) {
            continue;
        }
        let k = keep
            .entry((pos.lon.to_bits(), pos.lat.to_bits()))
            .or_insert(*id);
        if (*id < 0, id.unsigned_abs()) < (*k < 0, k.unsigned_abs()) {
            *k = *id;
        }
    }
    let mut remap = HashMap::new();
    for (id, pos) in points.iter().flatten() {
        if let Some(&k) = keep.get(&(pos.lon.to_bits(), pos.lat.to_bits()))
            && k != *id
            && !layered.contains(id)
        {
            remap.insert(*id, k);
        }
    }
    if remap.is_empty() {
        return remap;
    }
    for pts in points.iter_mut() {
        for p in pts.iter_mut() {
            if let Some(&k) = remap.get(&p.0) {
                p.0 = k;
            }
        }
        // The two nodes were consecutive on this way: one vertex is left.
        pts.dedup_by_key(|p| p.0);
        // A way that was all at one place is no way: no edge, no node, no dead end.
        if pts.len() < 2 {
            pts.clear();
        }
    }
    remap
}

/// Degree box around `p` covering `radius_m` (with margin).
pub fn search_box(p: LonLat, radius_m: f64) -> AABB<[f64; 2]> {
    let dlat = radius_m / M_PER_DEG_LAT * 1.01;
    let dlon = dlat / p.lat.to_radians().cos().max(0.05);
    AABB::from_corners([p.lon - dlon, p.lat - dlat], [p.lon + dlon, p.lat + dlat])
}

impl Graph {
    /// Builds the graph of `set`, repairing the mapping gaps when `connector_max_m` is
    /// positive (0 disables every repair):
    ///
    /// 1. two pistes whose segments cross without a shared node (neither on a bridge, in a
    ///    tunnel or on a layer) get a node at the crossing;
    /// 2. a dead end gets a connector to the nearest point of another piste up to
    ///    `connector_max_m`, on a vertex or inside a segment (a node is added there);
    /// 3. two dead ends still in different components get a connector when at most
    ///    [`DEAD_END_PAIR_MAX_M`] apart, the shortest first, never inside one component.
    ///
    /// Added nodes have negative ids (OSM ids are positive).
    pub fn build(set: &PisteSet, connector_max_m: f64) -> Graph {
        let repair = connector_max_m > 0.0;
        let mut points: Vec<Vec<(i64, LonLat)>> =
            set.pistes.iter().map(|p| p.points.clone()).collect();
        let mut next_id = -1i64;
        let mut connectors: Vec<(i64, i64)> = Vec::new();
        if repair {
            let cuts = crossings(set, &points, &mut next_id);
            insert_cuts(&mut points, cuts);
            // Duplicated nodes are one before looking for dead ends: a dead end on the
            // very node of another way is a junction, not a connector of zero length.
            merge_coincident(set, &mut points);
            let (cuts, found) = dead_end_connectors(&points, connector_max_m, &mut next_id);
            insert_cuts(&mut points, cuts);
            // Two dead ends facing one point of a segment put two nodes there.
            let remap = merge_coincident(set, &mut points);
            connectors = found
                .into_iter()
                .map(|(a, b)| {
                    let m = |x: i64| remap.get(&x).copied().unwrap_or(x);
                    (m(a), m(b))
                })
                .filter(|(a, b)| a != b)
                .collect();
            let pairs = dead_end_pairs(&points, &connectors, DEAD_END_PAIR_MAX_M);
            connectors.extend(pairs);
            // Two dead ends pointing at each other give the same connector twice.
            for c in &mut connectors {
                if c.0 > c.1 {
                    *c = (c.1, c.0);
                }
            }
            connectors.sort_unstable();
            connectors.dedup();
        }

        let uses = node_uses(&points);
        let mut junction: HashMap<i64, LonLat> = HashMap::new();
        for pts in &points {
            let n = pts.len();
            for (i, (id, pos)) in pts.iter().enumerate() {
                if i == 0 || i + 1 == n || uses[id] >= 2 {
                    junction.insert(*id, *pos);
                }
            }
        }
        let position: HashMap<i64, LonLat> = points.iter().flatten().copied().collect();
        for (a, b) in &connectors {
            junction.insert(*a, position[a]);
            junction.insert(*b, position[b]);
        }

        let ordered: BTreeMap<i64, LonLat> = junction.into_iter().collect();
        let index: HashMap<i64, usize> =
            ordered.keys().enumerate().map(|(i, id)| (*id, i)).collect();
        let nodes: Vec<Node> = ordered
            .iter()
            .map(|(id, pos)| Node {
                osm_id: *id,
                pos: *pos,
            })
            .collect();

        let mut edges = Vec::new();
        for (pi, p) in set.pistes.iter().enumerate() {
            let pp = &points[pi];
            let mut start = 0;
            for i in 1..pp.len() {
                if !index.contains_key(&pp[i].0) {
                    continue;
                }
                let part = &pp[start..=i];
                let mut pts: Vec<LonLat> = part.iter().map(|(_, q)| *q).collect();
                let (mut a, mut b) = (index[&part[0].0], index[&part[part.len() - 1].0]);
                let oneway = matches!(p.oneway, Oneway::Forward | Oneway::Backward);
                if p.oneway == Oneway::Backward {
                    pts.reverse();
                    std::mem::swap(&mut a, &mut b);
                }
                let length_m = pts.windows(2).map(|w| w[0].haversine_m(w[1])).sum();
                edges.push(Edge {
                    kind: EdgeKind::Piste { piste: pi },
                    from: a,
                    to: b,
                    points: pts,
                    length_m,
                    oneway,
                });
                start = i;
            }
        }
        for (a, b) in connectors {
            let (ia, ib) = (index[&a], index[&b]);
            let (pa, pb) = (nodes[ia].pos, nodes[ib].pos);
            edges.push(Edge {
                kind: EdgeKind::Connector,
                from: ia,
                to: ib,
                points: vec![pa, pb],
                length_m: pa.haversine_m(pb),
                oneway: false,
            });
        }
        Graph { nodes, edges }
    }

    /// Adjacency lists: for each node, `(edge index, other node)`.
    pub fn adjacency(&self) -> Vec<Vec<(usize, usize)>> {
        let mut adj = vec![Vec::new(); self.nodes.len()];
        for (e, edge) in self.edges.iter().enumerate() {
            adj[edge.from].push((e, edge.to));
            if edge.to != edge.from {
                adj[edge.to].push((e, edge.from));
            }
        }
        adj
    }

    /// Directed adjacency lists: for each node, `(edge index, node reached)` for every
    /// edge that may be travelled from it.
    pub fn out_adjacency(&self) -> Vec<Vec<(usize, usize)>> {
        let mut out = vec![Vec::new(); self.nodes.len()];
        for (e, edge) in self.edges.iter().enumerate() {
            out[edge.from].push((e, edge.to));
            if !edge.oneway && edge.to != edge.from {
                out[edge.to].push((e, edge.from));
            }
        }
        out
    }

    /// Paths over the pistes not in the site (`inside[piste] == false`) that are needed to
    /// travel between nodes of the site network.
    ///
    /// The site network is cut where its pistes leave the site box. A one-way trail that
    /// leaves the box and comes back outside it then ends in a node that nothing leaves
    /// (or that nothing reaches): every route through it fails, although the trail is
    /// skiable. For every node of the site network with an edge to the outside, a
    /// Dijkstra over outside edges only (one-way respected, at most `max_m` metres) looks
    /// for the site nodes it reaches; each one that the site network (plus the paths kept
    /// so far) cannot reach from that node gives a path to keep. Paths never run through
    /// the site network: each is an outside piece between two site nodes.
    ///
    /// Connectors count as inside when both ends are site nodes.
    pub fn reentry_paths(&self, inside: &[bool], max_m: f64) -> Vec<Reentry> {
        use std::cmp::Reverse;
        use std::collections::BinaryHeap;

        let n = self.nodes.len();
        let mut inside_node = vec![false; n];
        let mut kept = vec![false; self.edges.len()];
        for edge in &self.edges {
            if let EdgeKind::Piste { piste } = edge.kind
                && inside[piste]
            {
                inside_node[edge.from] = true;
                inside_node[edge.to] = true;
            }
        }
        let is_inside_edge = |e: &Edge| match e.kind {
            EdgeKind::Piste { piste } => inside[piste],
            EdgeKind::Connector => inside_node[e.from] && inside_node[e.to],
        };
        for (e, edge) in self.edges.iter().enumerate() {
            kept[e] = is_inside_edge(edge);
        }
        let inside_edge = kept.clone();
        let out = self.out_adjacency();
        let adj = self.adjacency();

        // Marks in `reach` every node reachable from `s` over kept edges.
        let spread = |s: usize, reach: &mut Vec<bool>, kept: &[bool]| {
            let mut stack = vec![s];
            reach[s] = true;
            while let Some(u) = stack.pop() {
                for &(e, v) in &out[u] {
                    if kept[e] && !reach[v] {
                        reach[v] = true;
                        stack.push(v);
                    }
                }
            }
        };

        let mut found = Vec::new();
        for x in 0..n {
            if !inside_node[x] || adj[x].iter().all(|&(e, _)| inside_edge[e]) {
                continue;
            }
            let mut reach = vec![false; n];
            spread(x, &mut reach, &kept);
            let mut dist = vec![f64::INFINITY; n];
            let mut pred = vec![usize::MAX; n];
            let mut heap = BinaryHeap::new();
            dist[x] = 0.0;
            // Distances are non-negative: their bit patterns sort like the values.
            heap.push(Reverse((0f64.to_bits(), x)));
            while let Some(Reverse((bits, u))) = heap.pop() {
                let d = f64::from_bits(bits);
                if d > dist[u] {
                    continue;
                }
                if u != x && inside_node[u] {
                    if !reach[u] {
                        let mut edges = Vec::new();
                        let mut v = u;
                        while v != x {
                            let e = pred[v];
                            edges.push(e);
                            let edge = &self.edges[e];
                            v = if edge.to == v { edge.from } else { edge.to };
                        }
                        edges.reverse();
                        let mut pistes: Vec<usize> = Vec::new();
                        for &e in &edges {
                            kept[e] = true;
                            if let EdgeKind::Piste { piste } = self.edges[e].kind
                                && pistes.last() != Some(&piste)
                            {
                                pistes.push(piste);
                            }
                        }
                        // Everything reachable from `u` is now reachable from `x`.
                        spread(u, &mut reach, &kept);
                        found.push(Reentry {
                            from: x,
                            to: u,
                            length_m: d,
                            pistes,
                        });
                    }
                    continue;
                }
                for &(e, v) in &out[u] {
                    if inside_edge[e] {
                        continue;
                    }
                    let nd = d + self.edges[e].length_m;
                    if nd <= max_m && nd < dist[v] {
                        dist[v] = nd;
                        pred[v] = e;
                        heap.push(Reverse((nd.to_bits(), v)));
                    }
                }
            }
        }
        found
    }

    /// Nodes of degree two or more whose edges are all one-way and all point in, or all
    /// point out. On a trail network this is a tagging contradiction (one `piste:oneway`
    /// against the others), never a real place: it is reported, not repaired.
    pub fn oneway_traps(&self) -> Vec<OnewayTrap> {
        let adj = self.adjacency();
        let mut traps = Vec::new();
        for (u, inc) in adj.iter().enumerate() {
            if inc.len() < 2 {
                continue;
            }
            let edges: Vec<&Edge> = inc.iter().map(|&(e, _)| &self.edges[e]).collect();
            if edges.iter().any(|e| !e.oneway || e.from == e.to) {
                continue;
            }
            if edges.iter().all(|e| e.to == u) {
                traps.push(OnewayTrap {
                    node: u,
                    sink: true,
                });
            } else if edges.iter().all(|e| e.from == u) {
                traps.push(OnewayTrap {
                    node: u,
                    sink: false,
                });
            }
        }
        traps
    }

    /// Connected component of every node (undirected), and the number of components.
    pub fn components(&self) -> (Vec<usize>, usize) {
        let adj = self.adjacency();
        let mut comp = vec![usize::MAX; self.nodes.len()];
        let mut n = 0;
        for s in 0..self.nodes.len() {
            if comp[s] != usize::MAX {
                continue;
            }
            let mut stack = vec![s];
            comp[s] = n;
            while let Some(u) = stack.pop() {
                for &(_, v) in &adj[u] {
                    if comp[v] == usize::MAX {
                        comp[v] = n;
                        stack.push(v);
                    }
                }
            }
            n += 1;
        }
        (comp, n)
    }

    /// Bridges of the undirected multigraph (edges whose removal disconnects their
    /// component). Iterative Tarjan: parallel edges are told apart by edge index, so a
    /// doubled edge is never a bridge.
    pub fn bridges(&self) -> Vec<bool> {
        let adj = self.adjacency();
        let n = self.nodes.len();
        let mut disc = vec![u32::MAX; n];
        let mut low = vec![0u32; n];
        let mut is_bridge = vec![false; self.edges.len()];
        let mut time = 0u32;
        // Stack frames: (node, edge used to enter it, next adjacency position).
        let mut stack: Vec<(usize, usize, usize)> = Vec::new();
        for s in 0..n {
            if disc[s] != u32::MAX {
                continue;
            }
            disc[s] = time;
            low[s] = time;
            time += 1;
            stack.push((s, usize::MAX, 0));
            while let Some(&mut (u, in_edge, ref mut pos)) = stack.last_mut() {
                if *pos < adj[u].len() {
                    let (e, v) = adj[u][*pos];
                    *pos += 1;
                    if e == in_edge {
                        continue;
                    }
                    if disc[v] == u32::MAX {
                        disc[v] = time;
                        low[v] = time;
                        time += 1;
                        stack.push((v, e, 0));
                    } else {
                        low[u] = low[u].min(disc[v]);
                    }
                } else {
                    stack.pop();
                    if let Some(&(p, _, _)) = stack.last() {
                        low[p] = low[p].min(low[u]);
                        if low[u] > disc[p] {
                            is_bridge[in_edge] = true;
                        }
                    }
                }
            }
        }
        is_bridge
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parse::{OsmData, Tags, Way};
    use crate::pistes::classify;

    fn way(id: i64, nodes: &[i64], oneway: Option<&str>) -> Way {
        let mut t = vec![("piste:type".to_owned(), "nordic".to_owned())];
        if let Some(o) = oneway {
            t.push(("piste:oneway".to_owned(), o.to_owned()));
        }
        Way {
            id,
            nodes: nodes.to_vec(),
            tags: Tags(t),
        }
    }

    /// Nodes on a line of latitude 46.6°, 0.001° (≈ 76 m) apart; node 9 is 5 m north of
    /// node 3 (a gap).
    fn data(ways: Vec<Way>) -> OsmData {
        let mut nodes: Vec<(i64, LonLat)> = (1..=8)
            .map(|i| (i, LonLat::new(6.0 + i as f64 * 0.001, 46.6)))
            .collect();
        nodes.push((9, LonLat::new(6.003, 46.6 + 5.0 / M_PER_DEG_LAT)));
        nodes.push((10, LonLat::new(6.003, 46.6 + 500.0 / M_PER_DEG_LAT)));
        OsmData::from_elements(nodes, ways, vec![])
    }

    #[test]
    fn splits_at_junctions_and_reverses_backward_ways() {
        // Way 1: 1-2-3-4-5 ; way 2: 3-6 ; way 3 (oneway -1): 5-7.
        let d = data(vec![
            way(1, &[1, 2, 3, 4, 5], None),
            way(2, &[3, 6], None),
            way(3, &[5, 7], Some("-1")),
        ]);
        let set = classify(&d);
        let g = Graph::build(&set, 0.0);
        let ids: Vec<i64> = g.nodes.iter().map(|n| n.osm_id).collect();
        assert_eq!(ids, vec![1, 3, 5, 6, 7]);
        assert_eq!(g.edges.len(), 4);
        let e = &g.edges[0];
        assert_eq!((g.nodes[e.from].osm_id, g.nodes[e.to].osm_id), (1, 3));
        assert_eq!(e.points.len(), 3);
        assert!((e.length_m - 2.0 * 76.5).abs() < 1.0, "{}", e.length_m);
        let back = &g.edges[3];
        assert!(back.oneway);
        assert_eq!((g.nodes[back.from].osm_id, g.nodes[back.to].osm_id), (7, 5));
        assert_eq!(g.components().1, 1);
    }

    #[test]
    fn connectors_close_small_gaps_only() {
        // Way 2 ends at node 9, 5 m from node 3 of way 1 and goes north; way 3 starts
        // 150 m beyond the end of way 1.
        let d = data(vec![
            way(1, &[1, 2, 3, 4, 5], None),
            way(2, &[9, 10], None),
            way(3, &[7, 8], None),
        ]);
        let set = classify(&d);
        let g = Graph::build(&set, 10.0);
        let conn: Vec<&Edge> = g
            .edges
            .iter()
            .filter(|e| e.kind == EdgeKind::Connector)
            .collect();
        assert_eq!(conn.len(), 1);
        let c = conn[0];
        let mut ends = [g.nodes[c.from].osm_id, g.nodes[c.to].osm_id];
        ends.sort_unstable();
        assert_eq!(ends, [3, 9]);
        assert!((c.length_m - 5.0).abs() < 0.1);
        // Node 3 became a junction: way 1 is split there.
        assert_eq!(
            g.edges
                .iter()
                .filter(|e| e.kind == EdgeKind::Piste { piste: 0 })
                .count(),
            2
        );
        assert_eq!(g.components().1, 2);
        assert_eq!(Graph::build(&set, 0.0).components().1, 3);
    }

    /// Data of nodes given as `(id, metres east, metres north)` of 6.0° E 46.6° N.
    fn data_m(nodes: &[(i64, f64, f64)], ways: Vec<Way>) -> OsmData {
        let k = 46.6f64.to_radians().cos();
        OsmData::from_elements(
            nodes.iter().map(|&(id, x, y)| {
                (
                    id,
                    LonLat::new(6.0 + x / (M_PER_DEG_LAT * k), 46.6 + y / M_PER_DEG_LAT),
                )
            }),
            ways,
            vec![],
        )
    }

    fn connectors(g: &Graph) -> Vec<(i64, i64, f64)> {
        g.edges
            .iter()
            .filter(|e| e.kind == EdgeKind::Connector)
            .map(|e| {
                let (a, b) = (g.nodes[e.from].osm_id, g.nodes[e.to].osm_id);
                (a.min(b), a.max(b), e.length_m)
            })
            .collect()
    }

    #[test]
    fn dead_ends_facing_across_a_road_join_two_networks() {
        // B166: two loops whose tails stop 17 m apart, on both sides of a road that is no
        // piste; nothing else joins them.
        let nodes = [
            (1, 0.0, 0.0),
            (2, 200.0, 0.0),
            (3, 200.0, 200.0),
            (4, 0.0, 200.0),
            (5, 300.0, 0.0),
            (6, 317.0, 0.0),
            (7, 500.0, 0.0),
            (8, 700.0, 0.0),
            (9, 700.0, 200.0),
            (10, 500.0, 200.0),
        ];
        let d = data_m(
            &nodes,
            vec![
                way(1, &[1, 2, 3, 4, 1], None),
                way(2, &[2, 5], None),
                way(3, &[6, 7], None),
                way(4, &[7, 8, 9, 10, 7], None),
            ],
        );
        let g = Graph::build(&classify(&d), 10.0);
        assert_eq!(g.components().1, 1);
        let c = connectors(&g);
        assert_eq!(c.len(), 1, "{c:?}");
        assert_eq!((c[0].0, c[0].1), (5, 6));
        assert!((c[0].2 - 17.0).abs() < 0.2, "{c:?}");
        // Repairs off: the two loops stay apart.
        assert_eq!(Graph::build(&classify(&d), 0.0).components().1, 2);
    }

    #[test]
    fn dead_ends_get_no_shortcut_inside_a_network_nor_beyond_the_gap() {
        // Way 1 is a U whose ends are 17 m apart: one network, no shortcut. Way 2 stops
        // 30 m from end 1: farther than a road, left apart.
        let nodes = [
            (1, 0.0, 0.0),
            (2, 300.0, 0.0),
            (3, 300.0, 17.0),
            (4, 0.0, 17.0),
            (5, 0.0, -30.0),
            (6, 0.0, -400.0),
        ];
        let d = data_m(
            &nodes,
            vec![way(1, &[1, 2, 3, 4], None), way(2, &[5, 6], None)],
        );
        let g = Graph::build(&classify(&d), 10.0);
        assert_eq!(connectors(&g), vec![]);
        assert_eq!(g.components().1, 2);
    }

    #[test]
    fn dead_end_joins_the_segment_it_stops_against() {
        // Way 2 stops 5 m from the middle of a 400 m segment of way 1, 200 m from its
        // vertices.
        let nodes = [
            (1, 0.0, 0.0),
            (2, 400.0, 0.0),
            (3, 200.0, 5.0),
            (4, 200.0, 300.0),
        ];
        let d = data_m(&nodes, vec![way(1, &[1, 2], None), way(2, &[3, 4], None)]);
        let g = Graph::build(&classify(&d), 10.0);
        assert_eq!(g.components().1, 1);
        let c = connectors(&g);
        assert_eq!(c.len(), 1, "{c:?}");
        assert!((c[0].2 - 5.0).abs() < 0.1, "{c:?}");
        // Way 1 is split at the foot of the connector.
        let parts: Vec<f64> = g
            .edges
            .iter()
            .filter(|e| e.kind == EdgeKind::Piste { piste: 0 })
            .map(|e| e.length_m)
            .collect();
        assert_eq!(parts.len(), 2, "{parts:?}");
        assert!(parts.iter().all(|l| (l - 200.0).abs() < 0.5), "{parts:?}");
    }

    #[test]
    fn crossing_ways_share_a_node_unless_on_another_level() {
        let nodes = [
            (1, 0.0, 0.0),
            (2, 400.0, 0.0),
            (3, 200.0, -200.0),
            (4, 200.0, 200.0),
        ];
        let d = data_m(&nodes, vec![way(1, &[1, 2], None), way(2, &[3, 4], None)]);
        let g = Graph::build(&classify(&d), 10.0);
        assert_eq!(g.components().1, 1);
        assert_eq!(g.edges.len(), 4);
        let adj = g.adjacency();
        let cross: Vec<usize> = (0..g.nodes.len()).filter(|&i| adj[i].len() == 4).collect();
        assert_eq!(cross.len(), 1);
        let p = g.nodes[cross[0]].pos;
        let k = 46.6f64.to_radians().cos();
        assert!(
            ((p.lon - 6.0) * M_PER_DEG_LAT * k - 200.0).abs() < 0.1,
            "{p:?}"
        );
        assert!(((p.lat - 46.6) * M_PER_DEG_LAT).abs() < 0.1, "{p:?}");
        // On a bridge, way 2 passes over way 1.
        let mut bridge = way(2, &[3, 4], None);
        bridge.tags.0.push(("bridge".to_owned(), "yes".to_owned()));
        let d = data_m(&nodes, vec![way(1, &[1, 2], None), bridge]);
        assert_eq!(Graph::build(&classify(&d), 10.0).components().1, 2);
    }

    #[test]
    fn a_way_crossing_two_ways_drawn_on_top_of_each_other_crosses_them_at_one_node() {
        // B166 (Autrans): ways 1 and 2 are the same trail drawn twice, with nodes of their
        // own at the same places; way 3 crosses both. One crossing node, not two nodes at
        // one place joined by an edge of zero length.
        let nodes = [
            (1, 0.0, 0.0),
            (2, 100.0, 0.0),
            (3, 300.0, 0.0),
            (12, 100.0, 0.0),
            (5, 200.0, -100.0),
            (6, 200.0, 100.0),
        ];
        let d = data_m(
            &nodes,
            vec![
                way(1, &[1, 2, 3], None),
                way(2, &[1, 12, 3], None),
                way(3, &[5, 6], None),
            ],
        );
        let g = Graph::build(&classify(&d), 10.0);
        assert!(g.edges.iter().all(|e| e.length_m > 0.0), "{:?}", g.edges);
        let adj = g.adjacency();
        let cross: Vec<usize> = (0..g.nodes.len()).filter(|&i| adj[i].len() == 6).collect();
        assert_eq!(cross.len(), 1, "{:?}", g.nodes);
        assert!(g.nodes[cross[0]].osm_id < 0);
        // The duplicated node 12 is node 2.
        assert!(g.nodes.iter().all(|n| n.osm_id != 12), "{:?}", g.nodes);
        assert_eq!(g.components().1, 1);
    }

    #[test]
    fn a_dead_end_on_a_duplicated_node_is_a_junction_not_a_connector() {
        // Way 2 starts on a node of its own at the very place of node 2 of way 1.
        let nodes = [
            (1, 0.0, 0.0),
            (2, 100.0, 0.0),
            (3, 200.0, 0.0),
            (4, 100.0, 0.0),
            (5, 100.0, 300.0),
        ];
        let d = data_m(
            &nodes,
            vec![way(1, &[1, 2, 3], None), way(2, &[4, 5], None)],
        );
        let g = Graph::build(&classify(&d), 10.0);
        assert_eq!(connectors(&g), vec![]);
        assert!(g.edges.iter().all(|e| e.length_m > 0.0), "{:?}", g.edges);
        assert_eq!(g.edges.len(), 3);
        assert_eq!(g.components().1, 1);
        // Repairs off: the raw OSM topology, two networks.
        assert_eq!(Graph::build(&classify(&d), 0.0).components().1, 2);
    }

    fn way_ids(set: &PisteSet, ids: &[usize]) -> Vec<i64> {
        ids.iter().map(|&i| set.pistes[i].way_id).collect()
    }

    #[test]
    fn reentry_closes_a_one_way_loop_cut_by_the_box() {
        // One-way loop 1 -> 3 -> 5 -> 1: ways 1 (1-2-3) and 3 (5-1) are in the site, way 2
        // (3-4-5) is outside. Way 4 (3-6) is an outside two-way spur, way 5 (7-8) an
        // unrelated outside piste.
        let d = data(vec![
            way(1, &[1, 2, 3], Some("yes")),
            way(2, &[3, 4, 5], Some("yes")),
            way(3, &[5, 1], Some("yes")),
            way(4, &[3, 6], None),
            way(5, &[7, 8], None),
        ]);
        let set = classify(&d);
        let g = Graph::build(&set, 0.0);
        let inside = [true, false, true, false, false];
        let r = g.reentry_paths(&inside, 10_000.0);
        assert_eq!(r.len(), 1, "{r:?}");
        assert_eq!((g.nodes[r[0].from].osm_id, g.nodes[r[0].to].osm_id), (3, 5));
        assert_eq!(way_ids(&set, &r[0].pistes), vec![2]);
        assert!(
            (r[0].length_m - 2.0 * 76.5).abs() < 1.0,
            "{}",
            r[0].length_m
        );
        // Too far: nothing kept.
        assert!(g.reentry_paths(&inside, 100.0).is_empty());
        // Cut at the box, the loop ends at 3 (nothing leaves) and starts at 5 (nothing
        // reaches it); these are dead ends, not tag contradictions.
        let cut = Graph::build(
            &set.subset(vec![set.pistes[0].clone(), set.pistes[2].clone()]),
            0.0,
        );
        let traps: Vec<(i64, bool)> = cut
            .oneway_traps()
            .iter()
            .map(|t| (cut.nodes[t.node].osm_id, t.sink))
            .collect();
        assert!(traps.is_empty(), "degree-1 ends are not traps: {traps:?}");
    }

    #[test]
    fn reentry_ignores_paths_the_site_already_has() {
        // Inside two-way way 1-2-3 and an outside two-way way 1-9-3 beside it: the site
        // already links 1 and 3 both ways.
        let d = data(vec![way(1, &[1, 2, 3], None), way(2, &[1, 9, 3], None)]);
        let set = classify(&d);
        let g = Graph::build(&set, 0.0);
        assert!(g.reentry_paths(&[true, false], 10_000.0).is_empty());
        // The same outside way joining two separate site pieces is kept.
        let d = data(vec![
            way(1, &[1, 2], None),
            way(2, &[2, 9, 3], None),
            way(3, &[3, 4], None),
        ]);
        let set = classify(&d);
        let g = Graph::build(&set, 0.0);
        let r = g.reentry_paths(&[true, false, true], 10_000.0);
        // One path: once kept, it is travelled both ways.
        assert_eq!(r.len(), 1, "{r:?}");
        assert!(r.iter().all(|x| way_ids(&set, &x.pistes) == vec![2]));
    }

    #[test]
    fn oneway_traps_find_contradicting_tags() {
        // 1 -> 2 <- 3: node 2 is a sink; 5 <- 4 -> 6: node 4 is a source; 7 -> 8 -> ...
        // through two-way way 6 is fine.
        let d = data(vec![
            way(1, &[1, 2], Some("yes")),
            way(2, &[2, 3], Some("-1")),
            way(3, &[4, 5], Some("yes")),
            way(4, &[4, 6], Some("yes")),
            way(5, &[7, 8], Some("yes")),
            way(6, &[8, 1], None),
        ]);
        let set = classify(&d);
        let g = Graph::build(&set, 0.0);
        let traps: Vec<(i64, bool)> = g
            .oneway_traps()
            .iter()
            .map(|t| (g.nodes[t.node].osm_id, t.sink))
            .collect();
        assert_eq!(traps, vec![(2, true), (4, false)]);
    }

    #[test]
    fn bridges_of_loop_with_tail() {
        // Loop 1-2-3-1 (ways 1, 2) with a tail 3-4-5 (way 3) and a doubled edge 5-6.
        let d = data(vec![
            way(1, &[1, 2, 3], None),
            way(2, &[3, 1], None),
            way(3, &[3, 4, 5], None),
            way(4, &[5, 6], None),
            way(5, &[5, 6], None),
        ]);
        let set = classify(&d);
        let g = Graph::build(&set, 0.0);
        let b = g.bridges();
        let bridged: Vec<usize> = g
            .edges
            .iter()
            .zip(&b)
            .filter(|(_, b)| **b)
            .map(|(e, _)| match e.kind {
                EdgeKind::Piste { piste } => piste,
                EdgeKind::Connector => usize::MAX,
            })
            .collect();
        assert_eq!(bridged, vec![2]);
    }
}
