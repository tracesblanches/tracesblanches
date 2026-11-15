//! A route followed on the trail graph: its legs, geometry, model times and the mapping
//! from a matched position (C2) to the distance travelled along it.
//!
//! The same type serves the validated loop, its plans B and the routes computed during the
//! outing: everything is derived from the legs, the graph and the time model of the day.

use std::collections::{BTreeSet, HashMap};

use serde::{Deserialize, Serialize};
use skimap_assist::suggest::Profile;
use skimap_common::geom::LonLat;
use skimap_core::eta::EtaModel;
use skimap_core::graph::{PisteGraph, Snap};
use skimap_core::routing::{Leg, Piece};

use crate::error::{Error, Result};
use crate::line::RouteLine;

/// A leg of a route: an edge of the graph from `from_m` to `to_m` metres along it
/// (backward when `to_m < from_m`). Serialisable copy of `skimap_core::routing::Leg`.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct PlanLeg {
    /// Edge index in the trail graph.
    pub edge: usize,
    /// Start, metres from the edge start.
    pub from_m: f64,
    /// End, metres from the edge start.
    pub to_m: f64,
}

impl From<Leg> for PlanLeg {
    fn from(l: Leg) -> Self {
        Self {
            edge: l.edge,
            from_m: l.from_m,
            to_m: l.to_m,
        }
    }
}

impl From<PlanLeg> for Leg {
    fn from(l: PlanLeg) -> Self {
        Leg {
            edge: l.edge,
            from_m: l.from_m,
            to_m: l.to_m,
        }
    }
}

impl PlanLeg {
    /// Length, metres.
    pub fn length_m(&self) -> f64 {
        (self.to_m - self.from_m).abs()
    }
}

/// A node of the route between two legs.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RouteNode {
    /// Node index in the graph.
    pub node: usize,
    /// Distance along the route, metres.
    pub s: f64,
    /// Index of the leg that starts at this node.
    pub next_leg: usize,
}

/// A route followed.
#[derive(Debug, Clone)]
pub struct Course {
    legs: Vec<PlanLeg>,
    line: RouteLine,
    /// Distance along the line at the start and end of each leg.
    leg_s: Vec<(f64, f64)>,
    edge_legs: HashMap<usize, Vec<usize>>,
    nodes: Vec<RouteNode>,
    profile: Profile,
    /// Profile distance per line distance.
    scale: f64,
}

impl Course {
    /// Course of `legs` on `g` with the time model `eta`.
    pub fn new(g: &PisteGraph, legs: &[PlanLeg], eta: &EtaModel) -> Result<Self> {
        let legs: Vec<PlanLeg> = legs
            .iter()
            .copied()
            .filter(|l| l.length_m() > 1e-6)
            .collect();
        if legs.is_empty() {
            return Err(Error::Invalid("route without any leg".into()));
        }
        let mut pts: Vec<LonLat> = Vec::new();
        let mut bounds: Vec<usize> = Vec::with_capacity(legs.len() + 1);
        for l in &legs {
            if l.edge >= g.edges().len() {
                return Err(Error::Invalid(format!("edge {} not in the graph", l.edge)));
            }
            let len = g.length_m(l.edge);
            if l.from_m < -0.5 || l.to_m < -0.5 || l.from_m > len + 0.5 || l.to_m > len + 0.5 {
                return Err(Error::Invalid(format!("leg outside edge {}", l.edge)));
            }
            let sub = g.sub_polyline(l.edge, l.from_m, l.to_m);
            let skip = usize::from(pts.last().is_some_and(|p| *p == sub[0]));
            bounds.push(pts.len().saturating_sub(skip));
            pts.extend_from_slice(&sub[skip..]);
        }
        let line = RouteLine::new(pts.clone());
        // Distances at the leg boundaries, on the de-duplicated line: recompute by walking
        // the raw points (the line only drops consecutive duplicates).
        let mut raw_cum = Vec::with_capacity(pts.len());
        let mut acc = 0.0;
        for (i, p) in pts.iter().enumerate() {
            if i > 0 {
                acc += pts[i - 1].haversine_m(*p);
            }
            raw_cum.push(acc);
        }
        let mut leg_s = Vec::with_capacity(legs.len());
        for i in 0..legs.len() {
            let a = raw_cum[bounds[i]];
            let b = if i + 1 < legs.len() {
                raw_cum[bounds[i + 1]]
            } else {
                *raw_cum.last().expect("points")
            };
            leg_s.push((a, b));
        }
        let mut edge_legs: HashMap<usize, Vec<usize>> = HashMap::new();
        for (i, l) in legs.iter().enumerate() {
            edge_legs.entry(l.edge).or_default().push(i);
        }
        let mut nodes = Vec::new();
        for i in 1..legs.len() {
            let prev = legs[i - 1];
            let e = g.edge(prev.edge);
            let len = g.length_m(prev.edge);
            let node = if prev.to_m >= len - 0.5 {
                Some(e.to)
            } else if prev.to_m <= 0.5 {
                Some(e.from)
            } else {
                None
            };
            if let Some(node) = node {
                nodes.push(RouteNode {
                    node,
                    s: leg_s[i].0,
                    next_leg: i,
                });
            }
        }
        let pieces: Vec<Piece<'_>> = legs
            .iter()
            .flat_map(|l| skimap_core::routing::edge_pieces(g, l.edge, l.from_m, l.to_m))
            .collect();
        let profile = Profile::of(eta, &pieces);
        let scale = if line.length_m() > 0.0 {
            profile.length_m() / line.length_m()
        } else {
            1.0
        };
        Ok(Self {
            legs,
            line,
            leg_s,
            edge_legs,
            nodes,
            profile,
            scale,
        })
    }

    /// Legs.
    pub fn legs(&self) -> &[PlanLeg] {
        &self.legs
    }

    /// Geometry.
    pub fn line(&self) -> &RouteLine {
        &self.line
    }

    /// Length, metres.
    pub fn length_m(&self) -> f64 {
        self.line.length_m()
    }

    /// Distance along the line at the start and end of each leg.
    pub fn leg_bounds(&self) -> &[(f64, f64)] {
        &self.leg_s
    }

    /// Graph nodes between legs, in travel order.
    pub fn nodes(&self) -> &[RouteNode] {
        &self.nodes
    }

    /// Time profile of the C5 model (distances of the profile pieces).
    pub fn profile(&self) -> &Profile {
        &self.profile
    }

    /// Profile distance of a line distance (the A8 monitor works on the profile).
    pub fn profile_at(&self, s: f64) -> f64 {
        s * self.scale
    }

    /// Model time from the start to a distance, seconds (linear inside a piece).
    pub fn model_s_at(&self, s: f64) -> f64 {
        let p = &self.profile;
        let d = self.profile_at(s).clamp(0.0, p.length_m());
        let i = p
            .dist_m
            .partition_point(|&x| x <= d)
            .clamp(1, p.dist_m.len().max(2) - 1);
        if p.dist_m.len() < 2 {
            return 0.0;
        }
        let (d0, d1) = (p.dist_m[i - 1], p.dist_m[i]);
        let f = if d1 > d0 { (d - d0) / (d1 - d0) } else { 0.0 };
        p.out_s[i - 1] + (p.out_s[i] - p.out_s[i - 1]) * f
    }

    /// Total model time, seconds.
    pub fn model_s(&self) -> f64 {
        self.profile.total_s()
    }

    /// Climb and descent, metres.
    pub fn gain_loss(&self, g: &PisteGraph) -> (f64, f64) {
        let r = skimap_core::routing::Route {
            start: self.start_snap(g),
            end: self.end_snap(g),
            legs: self.legs.iter().map(|l| Leg::from(*l)).collect(),
            cost: 0.0,
        };
        r.gain_loss(g)
    }

    /// Snap at the start of the route.
    pub fn start_snap(&self, g: &PisteGraph) -> Snap {
        let l = self.legs[0];
        Snap {
            edge: l.edge,
            along_m: l.from_m,
            pos: g.point_at(l.edge, l.from_m),
            dist_m: 0.0,
        }
    }

    /// Snap at the end of the route.
    pub fn end_snap(&self, g: &PisteGraph) -> Snap {
        let l = *self.legs.last().expect("legs");
        Snap {
            edge: l.edge,
            along_m: l.to_m,
            pos: g.point_at(l.edge, l.to_m),
            dist_m: 0.0,
        }
    }

    /// Index of the leg at a distance.
    pub fn leg_at(&self, s: f64) -> usize {
        self.leg_s
            .partition_point(|(_, b)| *b < s)
            .min(self.legs.len() - 1)
    }

    /// Candidate distances along the route of a matched point (every leg on that edge
    /// that contains it).
    pub fn distances_of(&self, snap: &Snap) -> Vec<f64> {
        let mut out = Vec::new();
        if let Some(ls) = self.edge_legs.get(&snap.edge) {
            for &i in ls {
                let l = self.legs[i];
                let (lo, hi) = if l.from_m <= l.to_m {
                    (l.from_m, l.to_m)
                } else {
                    (l.to_m, l.from_m)
                };
                if snap.along_m >= lo - 1.0 && snap.along_m <= hi + 1.0 {
                    let (a, b) = self.leg_s[i];
                    let f = if l.length_m() > 0.0 {
                        ((snap.along_m - l.from_m).abs() / l.length_m()).clamp(0.0, 1.0)
                    } else {
                        0.0
                    };
                    out.push(a + (b - a) * f);
                }
            }
        }
        out
    }

    /// Edges travelled from distance `s` on (the leg containing `s` included).
    pub fn edges_from(&self, s: f64) -> BTreeSet<usize> {
        let i = self.leg_at(s);
        self.legs[i..].iter().map(|l| l.edge).collect()
    }

    /// OSM ways travelled from distance `s` on.
    pub fn ways_from(&self, g: &PisteGraph, s: f64) -> BTreeSet<i64> {
        self.edges_from(s)
            .into_iter()
            .filter_map(|e| g.edge(e).attrs.osm_way_id)
            .collect()
    }

    /// Legs of the route from the start up to distance `s` (the last one cut at `s`).
    pub fn prefix(&self, s: f64) -> Vec<PlanLeg> {
        let mut out = Vec::new();
        for (i, l) in self.legs.iter().enumerate() {
            let (a, b) = self.leg_s[i];
            if b <= s + 1e-6 {
                out.push(*l);
                continue;
            }
            if a < s {
                let f = ((s - a) / (b - a)).clamp(0.0, 1.0);
                let cut = l.from_m + (l.to_m - l.from_m) * f;
                out.push(PlanLeg {
                    edge: l.edge,
                    from_m: l.from_m,
                    to_m: cut,
                });
            }
            break;
        }
        out
    }

    /// Legs of the route from the route node `k` (index in [`Course::nodes`]) to the end.
    pub fn suffix_from_node(&self, k: usize) -> Vec<PlanLeg> {
        self.legs[self.nodes[k].next_leg..].to_vec()
    }

    /// Legs of the route from the start to the route node `k`.
    pub fn prefix_to_node(&self, k: usize) -> Vec<PlanLeg> {
        self.legs[..self.nodes[k].next_leg].to_vec()
    }

    /// Trail names from distance `s` on, in order, each once.
    pub fn names_from(&self, g: &PisteGraph, s: f64) -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        for l in &self.legs[self.leg_at(s)..] {
            if let Some(n) = &g.edge(l.edge).attrs.name {
                for part in n.split(" / ") {
                    if !out.iter().any(|x| x == part) {
                        out.push(part.to_owned());
                    }
                }
            }
        }
        out
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use skimap_core::graph::{Difficulty, Edge, EdgeAttrs, EdgeKind, GraphData, LocalPlane, Node};
    use skimap_core::routing::Style;

    pub(crate) fn plane() -> LocalPlane {
        LocalPlane::new(LonLat::new(6.1, 46.6))
    }

    fn attrs(name: &str, way: i64) -> EdgeAttrs {
        EdgeAttrs {
            kind: EdgeKind::Piste,
            name: Some(name.into()),
            osm_way_id: Some(way),
            classic: Some(true),
            skating: Some(true),
            backcountry: false,
            nordic: true,
            hike: false,
            difficulty: Some(Difficulty::Easy),
            difficulty_max: Some(Difficulty::Easy),
        }
    }

    /// A square loop 0-1-2-3 of 1 km sides (east, north, west, south), a diagonal 0-2
    /// (« Diagonale ») and a spur 1-4 east; flat. Node positions in metres:
    ///
    /// ```text
    /// 3 ──────── 2
    /// │        ╱ │
    /// │     ╱    │
    /// │  ╱       │
    /// 0 ──────── 1 ─── 4
    /// ```
    pub(crate) fn square() -> PisteGraph {
        let p = plane();
        let xy = [
            [0.0, 0.0],
            [1000.0, 0.0],
            [1000.0, 1000.0],
            [0.0, 1000.0],
            [1600.0, 0.0],
        ];
        let nodes: Vec<Node> = xy
            .iter()
            .map(|q| Node {
                pos: p.lonlat(*q),
                osm_node_id: None,
            })
            .collect();
        let edge = |a: usize, b: usize, name: &str, way: i64| {
            let n = 20;
            Edge {
                from: a,
                to: b,
                oneway: false,
                points: (0..=n)
                    .map(|k| {
                        let t = k as f64 / n as f64;
                        p.lonlat([
                            xy[a][0] + (xy[b][0] - xy[a][0]) * t,
                            xy[a][1] + (xy[b][1] - xy[a][1]) * t,
                        ])
                    })
                    .collect(),
                attrs: attrs(name, way),
                profile: None,
            }
        };
        PisteGraph::new(GraphData {
            site: "square".into(),
            nodes,
            edges: vec![
                edge(0, 1, "Sud", 1),
                edge(1, 2, "Est", 2),
                edge(2, 3, "Nord", 3),
                edge(3, 0, "Ouest", 4),
                edge(0, 2, "Diagonale", 5),
                edge(1, 4, "Antenne", 6),
            ],
            elevation_source: None,
        })
        .unwrap()
    }

    /// The square loop counter-clockwise from node 0: Sud, Est, Nord, Ouest.
    pub(crate) fn square_loop(g: &PisteGraph) -> Vec<PlanLeg> {
        let full = |e: usize, fwd: bool| PlanLeg {
            edge: e,
            from_m: if fwd { 0.0 } else { g.length_m(e) },
            to_m: if fwd { g.length_m(e) } else { 0.0 },
        };
        vec![full(0, true), full(1, true), full(2, true), full(3, true)]
    }

    #[test]
    fn geometry_nodes_times_and_matching() {
        let g = square();
        let eta = EtaModel::new(Style::Classic);
        let c = Course::new(&g, &square_loop(&g), &eta).unwrap();
        assert!((c.length_m() - 4000.0).abs() < 5.0, "{}", c.length_m());
        let ns: Vec<usize> = c.nodes().iter().map(|n| n.node).collect();
        assert_eq!(ns, vec![1, 2, 3]);
        assert!((c.nodes()[1].s - 2000.0).abs() < 3.0);
        // Flat, groomed, −5 °C: about 10.6 km/h.
        let kmh = 4.0 / (c.model_s() / 3600.0);
        assert!((kmh - 10.6).abs() < 0.3, "{kmh}");
        assert!((c.model_s_at(2000.0) - c.model_s() / 2.0).abs() < 2.0);
        // A point 300 m along « Est ».
        let snap = Snap {
            edge: 1,
            along_m: 300.0,
            pos: g.point_at(1, 300.0),
            dist_m: 0.0,
        };
        let d = c.distances_of(&snap);
        assert_eq!(d.len(), 1);
        assert!((d[0] - 1300.0).abs() < 3.0, "{d:?}");
        // Prefix and suffix.
        let pre = c.prefix(1300.0);
        assert_eq!(pre.len(), 2);
        assert!((pre[1].to_m - 300.0).abs() < 3.0);
        assert_eq!(c.suffix_from_node(1)[0].edge, 2);
        assert_eq!(c.names_from(&g, 2500.0), vec!["Nord", "Ouest"]);
        assert_eq!(
            c.ways_from(&g, 2500.0).into_iter().collect::<Vec<_>>(),
            vec![3, 4]
        );
    }

    #[test]
    fn out_and_back_on_the_same_edge_has_two_distances() {
        let g = square();
        let eta = EtaModel::new(Style::Classic);
        let l = g.length_m(5);
        let legs = vec![
            PlanLeg {
                edge: 5,
                from_m: 0.0,
                to_m: l,
            },
            PlanLeg {
                edge: 5,
                from_m: l,
                to_m: 0.0,
            },
        ];
        let c = Course::new(&g, &legs, &eta).unwrap();
        let snap = Snap {
            edge: 5,
            along_m: 100.0,
            pos: g.point_at(5, 100.0),
            dist_m: 0.0,
        };
        let mut d = c.distances_of(&snap);
        d.sort_by(f64::total_cmp);
        assert_eq!(d.len(), 2);
        assert!(
            (d[0] - 100.0).abs() < 2.0 && (d[1] - (2.0 * l - 100.0)).abs() < 2.0,
            "{d:?}"
        );
        assert!(Course::new(&g, &[], &eta).is_err());
        assert!(
            Course::new(
                &g,
                &[PlanLeg {
                    edge: 99,
                    from_m: 0.0,
                    to_m: 1.0
                }],
                &eta
            )
            .is_err()
        );
    }
}
