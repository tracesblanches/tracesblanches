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
