//! Junctions of a route and the turn to announce at each, or nothing when the way is
//! obvious (J4: « rien à dire sur une piste évidente »).
//!
//! # Junctions
//!
//! The graph nodes between two legs of the route are grouped when they are less than
//! [`CLUSTER_M`] apart along it (the OSM layer adds connectors of at most 10 m between a
//! dead end and a trail, and two ways often meet a few metres apart). A cluster is a
//! junction when a trail leaves it that the route does not take (its *branches*), one-way
//! trails counted only where they can be entered.
//!
//! # Direction
//!
//! Headings are chords over [`HEADING_M`] before and after the junction, and along each
//! branch. The turn angle is the change of heading (positive to the right):
//!
//! | Case | Announced |
//! |---|---|
//! | no branch | nothing (not a junction) |
//! | every branch at least 45° away from the route, and the angle at most 35° (straight on), or at most 60° on the same trail (it bends) | nothing: the way is obvious |
//! | a branch within 30° of the route (a fork) | keep left / keep right |
//! | angle < 25° | straight on |
//! | 25 to 60° | bear left / right |
//! | 60 to 155° | turn left / right |
//! | more than 155° | turn back |
//!
//! The announcement says the trail taken after the junction when it has a name.

use serde::{Deserialize, Serialize};
use skimap_common::geom::LonLat;
use skimap_core::graph::{HalfEdge, PisteGraph};
use skimap_voice::clips::Direction;

use crate::course::Course;
use crate::line::{bearing_deg, turn_deg};

/// Nodes closer than this along the route form one junction, metres.
pub const CLUSTER_M: f64 = 25.0;
/// Length of the chords giving the headings, metres.
pub const HEADING_M: f64 = 40.0;

/// A junction of the route.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Junction {
    /// Distance along the route where the junction starts, metres.
    pub s: f64,
    /// Distance where it ends (after the connectors), metres.
    pub s_end: f64,
    /// Graph nodes of the junction.
    pub nodes: Vec<usize>,
    /// Position (first node).
    pub pos: LonLat,
    /// Index in [`Course::nodes`] of the last node of the junction (where a plan B leaves
    /// the route).
    pub route_node: usize,
    /// Trails leaving the junction that the route does not take.
    pub branches: usize,
    /// Turn angle, degrees, positive to the right.
    pub angle_deg: f64,
    /// Angles of the branches, degrees, relative to the incoming heading.
    pub branch_angles_deg: Vec<f64>,
    /// Direction to announce; `None`: the way is obvious, nothing is said.
    pub direction: Option<Direction>,
    /// Trail name before the junction.
    pub name_in: Option<String>,
    /// Trail name after the junction.
    pub name_out: Option<String>,
    /// Edge taken after the junction (for its difficulty colour).
    pub edge_out: usize,
}

impl Junction {
    /// Whether something is said at this junction.
    pub fn announced(&self) -> bool {
        self.direction.is_some()
    }
}

fn half_heading(g: &PisteGraph, h: HalfEdge) -> Option<f64> {
    let len = g.length_m(h.edge);
    let (a, b) = if h.forward {
        (0.0, HEADING_M.min(len))
    } else {
        (len, (len - HEADING_M).max(0.0))
    };
    let (pa, pb) = (g.point_at(h.edge, a), g.point_at(h.edge, b));
    (pa.haversine_m(pb) >= 1.0).then(|| bearing_deg(pa, pb))
}

fn first_name(name: &Option<String>) -> Option<String> {
    name.as_ref()
        .map(|n| n.split(" / ").next().unwrap_or(n).to_owned())
}

/// Junctions of a route, in travel order.
pub fn junctions(g: &PisteGraph, c: &Course) -> Vec<Junction> {
    let nodes = c.nodes();
    let legs = c.legs();
    let mut out = Vec::new();
    let mut k = 0;
    while k < nodes.len() {
        // Cluster of route nodes within CLUSTER_M.
        let mut j = k;
        while j + 1 < nodes.len() && nodes[j + 1].s - nodes[k].s < CLUSTER_M {
            j += 1;
        }
        let cluster: Vec<usize> = nodes[k..=j].iter().map(|n| n.node).collect();
        // Edges used by the route inside the cluster: the incoming leg, the legs between
        // the cluster nodes and the outgoing leg.
        let first_leg = nodes[k].next_leg - 1;
        let last_leg = nodes[j].next_leg;
        let used: Vec<usize> = legs[first_leg..=last_leg].iter().map(|l| l.edge).collect();
        let mut branches: Vec<f64> = Vec::new();
        let s_in = nodes[k].s;
        let s_out = nodes[j].s;
        let line = c.line();
        let h_in = line.heading((s_in - HEADING_M).max(0.0), s_in);
        let h_out = line.heading(s_out, (s_out + HEADING_M).min(line.length_m()));
        for &u in &cluster {
            for &h in g.out_edges(u) {
                if used.contains(&h.edge) {
                    continue;
                }
                // A connector between two nodes of the cluster is not a branch.
                if cluster.contains(&g.head(h)) && g.length_m(h.edge) < CLUSTER_M {
                    continue;
                }
                if let (Some(hi), Some(hb)) = (h_in, half_heading(g, h)) {
                    branches.push(turn_deg(hi, hb));
                } else {
                    branches.push(f64::NAN);
                }
            }
        }
        if branches.is_empty() {
            k = j + 1;
            continue;
        }
        let angle = match (h_in, h_out) {
            (Some(a), Some(b)) => turn_deg(a, b),
            _ => 0.0,
        };
        let name_in = first_name(&g.edge(legs[first_leg].edge).attrs.name);
        let edge_out = legs[last_leg].edge;
        let name_out = first_name(&g.edge(edge_out).attrs.name);
        let near: Vec<f64> = branches
            .iter()
            .copied()
            .filter(|b| b.is_finite() && (b - angle).abs() < 30.0)
            .collect();
        let apart = branches
            .iter()
            .all(|b| b.is_finite() && (b - angle).abs() >= 45.0);
        let same_trail = name_in.is_some() && name_in == name_out;
        let obvious = apart && (angle.abs() <= 35.0 || (same_trail && angle.abs() <= 60.0));
        let direction = if obvious {
            None
        } else if let Some(b) = near.first() {
            Some(if angle < *b {
                Direction::KeepLeft
            } else {
                Direction::KeepRight
            })
        } else {
            let a = angle.abs();
            Some(match (a, angle > 0.0) {
                (a, _) if a < 25.0 => Direction::Straight,
                (a, true) if a < 60.0 => Direction::SlightRight,
                (a, false) if a < 60.0 => Direction::SlightLeft,
                (a, true) if a <= 155.0 => Direction::Right,
                (a, false) if a <= 155.0 => Direction::Left,
                _ => Direction::UTurn,
            })
        };
        out.push(Junction {
            s: s_in,
            s_end: s_out,
            nodes: cluster,
            pos: g.nodes()[nodes[k].node].pos,
            route_node: j,
            branches: branches.len(),
            angle_deg: angle,
            branch_angles_deg: branches,
            direction,
            name_in,
            name_out,
            edge_out,
        });
        k = j + 1;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::course::tests::{square, square_loop};
    use crate::course::{Course, PlanLeg};
    use skimap_core::eta::EtaModel;
    use skimap_core::routing::Style;

    #[test]
    fn square_loop_turns_left_at_every_corner_with_a_branch() {
        let g = square();
        let c = Course::new(&g, &square_loop(&g), &EtaModel::new(Style::Classic)).unwrap();
        let js = junctions(&g, &c);
        // Node 1 (spur east, Est north): left turn; node 2 (diagonal): left turn; node 3
        // has no branch.
        assert_eq!(js.len(), 2, "{js:#?}");
        assert_eq!(js[0].nodes, vec![1]);
        assert_eq!(js[0].direction, Some(Direction::Left));
        assert!((js[0].angle_deg + 90.0).abs() < 2.0, "{}", js[0].angle_deg);
        assert_eq!(js[0].name_out.as_deref(), Some("Est"));
        assert_eq!(js[1].nodes, vec![2]);
        assert_eq!(js[1].direction, Some(Direction::Left));
        assert_eq!(js[1].branches, 1);
        // The diagonal at node 2 comes from the south-west: 135° to the left of north.
        assert!(
            (js[1].branch_angles_deg[0] + 135.0).abs() < 2.0,
            "{:?}",
            js[1]
        );
    }

    #[test]
    fn straight_on_with_branches_far_off_is_obvious() {
        // From the spur west-bound at node 1: straight on along « Sud » (the branch « Est »
        // is 90° off): obvious, nothing said; turning right into « Est »: announced.
        let g = square();
        let l5 = g.length_m(5);
        let spur = PlanLeg {
            edge: 5,
            from_m: l5,
            to_m: 0.0,
        };
        let eta = EtaModel::new(Style::Classic);
        let on = vec![
            spur,
            PlanLeg {
                edge: 0,
                from_m: g.length_m(0),
                to_m: 0.0,
            },
        ];
        let js = junctions(&g, &Course::new(&g, &on, &eta).unwrap());
        assert_eq!(js.len(), 1);
        assert_eq!(js[0].direction, None, "{:?}", js[0]);
        assert!(!js[0].announced());
        assert!(js[0].angle_deg.abs() < 2.0);
        let right = vec![
            spur,
            PlanLeg {
                edge: 1,
                from_m: 0.0,
                to_m: g.length_m(1),
            },
        ];
        let js = junctions(&g, &Course::new(&g, &right, &eta).unwrap());
        assert_eq!(js[0].direction, Some(Direction::Right), "{:?}", js[0]);
    }
}
