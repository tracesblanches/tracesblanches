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

use std::collections::{BTreeMap, HashMap};

use rstar::{AABB, RTree, primitives::GeomWithData};
use skimap_common::geom::LonLat;

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

type Vertex = GeomWithData<[f64; 2], (usize, usize)>;

/// Degree box around `p` covering `radius_m` (with margin).
pub fn search_box(p: LonLat, radius_m: f64) -> AABB<[f64; 2]> {
    let dlat = radius_m / M_PER_DEG_LAT * 1.01;
    let dlon = dlat / p.lat.to_radians().cos().max(0.05);
    AABB::from_corners([p.lon - dlon, p.lat - dlat], [p.lon + dlon, p.lat + dlat])
}

impl Graph {
    /// Builds the graph of `set`, adding connectors for gaps up to `connector_max_m`
    /// (0 disables them).
    pub fn build(set: &PisteSet, connector_max_m: f64) -> Graph {
        // Occurrences of each node over all pistes.
        let mut uses: HashMap<i64, u32> = HashMap::new();
        for p in &set.pistes {
            for (id, _) in &p.points {
                *uses.entry(*id).or_default() += 1;
            }
        }
        let mut junction: HashMap<i64, LonLat> = HashMap::new();
        for p in &set.pistes {
            let n = p.points.len();
            for (i, (id, pos)) in p.points.iter().enumerate() {
                if i == 0 || i + 1 == n || uses[id] >= 2 {
                    junction.insert(*id, *pos);
                }
            }
        }

        // Connectors from dead ends.
        let mut connectors: Vec<(i64, i64)> = Vec::new();
        if connector_max_m > 0.0 {
            let tree: RTree<Vertex> = RTree::bulk_load(
                set.pistes
                    .iter()
                    .enumerate()
                    .flat_map(|(pi, p)| {
                        p.points
                            .iter()
                            .enumerate()
                            .map(move |(vi, (_, pos))| Vertex::new([pos.lon, pos.lat], (pi, vi)))
                    })
                    .collect(),
            );
            for (pi, p) in set.pistes.iter().enumerate() {
                let n = p.points.len();
                for &end in &[0, n - 1] {
                    let (id, pos) = p.points[end];
                    if uses[&id] != 1 {
                        continue;
                    }
                    let best = tree
                        .locate_in_envelope(&search_box(pos, connector_max_m))
                        .filter(|v| v.data.0 != pi)
                        .map(|v| {
                            let (qi, qv) = v.data;
                            let (qid, qpos) = set.pistes[qi].points[qv];
                            (pos.haversine_m(qpos), qid, qpos)
                        })
                        .filter(|(d, qid, _)| *d <= connector_max_m && *qid != id)
                        .min_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)));
                    if let Some((_, qid, qpos)) = best {
                        connectors.push((id, qid));
                        junction.insert(qid, qpos);
                    }
                }
            }
            // Two dead ends pointing at each other give the same connector twice.
            for c in &mut connectors {
                if c.0 > c.1 {
                    *c = (c.1, c.0);
                }
            }
            connectors.sort_unstable();
            connectors.dedup();
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
            let mut start = 0;
            for i in 1..p.points.len() {
                if !index.contains_key(&p.points[i].0) {
                    continue;
                }
                let part = &p.points[start..=i];
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
        // Way 2 ends at node 9, 5 m from node 3 of way 1; way 3 ends 500 m away.
        let d = data(vec![
            way(1, &[1, 2, 3, 4, 5], None),
            way(2, &[9, 8], None),
            way(3, &[10, 6], None),
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
