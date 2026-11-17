//! Plans B computed in the morning (J2, #111): at the useful junctions of the validated
//! loop, the way to the finish (*exit*: the shortcut when the conditions worsen, the
//! shortened loop when late), and for the trails of the loop, the way around them if they
//! close (*detour*).
//!
//! # Exits
//!
//! From the last node of each junction, the fastest route to the finish under the day's
//! time model (C5, the style preferences and the level of A5). Kept when it saves at least
//! [`MIN_EXIT_SAVING_S`] of model time, from [`EXIT_FROM_M`] after the start to
//! [`EXIT_BEFORE_END_M`] before the finish, and [`EXIT_SPACING_M`] after the previous one
//! kept (unless it saves 10 more minutes).
//!
//! # Detours
//!
//! For every OSM way of the loop: from the last junction before its first use, the
//! fastest route that does not use it to the first junction after its last use, then the
//! rest of the loop; if that adds more than [`DETOUR_MAX_SHARE`] of the loop's time (20
//! min at least) or no longer ends before the limit, the fastest route to the finish
//! without it; when the network has neither, the plan says so (the closure is then
//! announced without a detour). Ways with the same way around share one plan B. The way is
//! the unit of the closures: the status sources (P13) give them per way (Nordic France,
//! joined to the ways by the status layer) or per itinerary geometry (ENJ).

use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};
use skimap_assist::suggest::node_snap;
use skimap_core::eta::EtaModel;
use skimap_core::graph::{Difficulty, EdgeAttrs, PisteGraph, Snap};
use skimap_core::routing::{CostModel, Route, Router};

use crate::course::{Course, PlanLeg};
use crate::guidance::Junction;

/// Model time an exit must save over the rest of the loop, seconds.
pub const MIN_EXIT_SAVING_S: f64 = 600.0;
/// Factor on trails harder than the level asked (the same as A5, `skimap_assist::suggest`).
pub const LEVEL_FACTOR: f64 = 4.0;
/// An exit is useful from this far along the loop, metres.
pub const EXIT_FROM_M: f64 = 1000.0;
/// …until this far before the finish, metres.
pub const EXIT_BEFORE_END_M: f64 = 1000.0;
/// Two exits kept are this far apart along the loop at least, metres, unless the later
/// one saves 10 more minutes.
pub const EXIT_SPACING_M: f64 = 1000.0;
/// A detour that comes back to the loop is kept if it adds at most this share of the
/// loop's model time (20 min at least); else the way to the finish.
pub const DETOUR_MAX_SHARE: f64 = 0.25;

/// Cost of the plan's routes: C5 time with the style preferences, trails harder than the
/// level made [`LEVEL_FACTOR`] times more costly, closed ways forbidden.
#[derive(Debug, Clone)]
pub struct PlanCost {
    /// Time model of the day.
    pub eta: EtaModel,
    /// Hardest difficulty wanted.
    pub max_difficulty: Option<Difficulty>,
    /// Closed OSM ways.
    pub closed: BTreeSet<i64>,
}

impl CostModel for PlanCost {
    fn piece_cost(&self, attrs: &EdgeAttrs, dist_m: f64, dz_m: f64, elev_m: Option<f64>) -> f64 {
        self.eta.piece_cost(attrs, dist_m, dz_m, elev_m)
    }

    fn edge_factor(&self, attrs: &EdgeAttrs) -> f64 {
        if attrs.osm_way_id.is_some_and(|w| self.closed.contains(&w)) {
            return f64::INFINITY;
        }
        let mut f = self.eta.edge_factor(attrs);
        if let (Some(max), Some(d)) = (self.max_difficulty, attrs.difficulty)
            && d > max
        {
            f *= LEVEL_FACTOR;
        }
        f
    }
}

/// Kind of plan B.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FallbackKind {
    /// Fastest way to the finish from a junction.
    Exit,
    /// Way around closed trails.
    Detour {
        /// OSM ways avoided (the ways of the loop with the same way around).
        ways: Vec<i64>,
        /// Their names, if any.
        name: Option<String>,
        /// Whether the detour comes back to the loop (else it goes to the finish).
        rejoins: bool,
    },
}

/// A plan B: the loop up to the divergence (`prefix` legs), its own legs, then the loop
/// again from leg `rejoin` (detours that come back to it).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Fallback {
    /// Index in the plan's list.
    pub id: usize,
    /// Kind.
    pub kind: FallbackKind,
    /// Junction of the loop where it leaves the loop (index in the plan's junctions;
    /// `None`: at the start).
    pub junction: Option<usize>,
    /// Distance along the loop where it leaves it, metres.
    pub s_div: f64,
    /// Distance along the loop where it comes back to it, metres (detours that rejoin).
    pub s_rejoin: Option<f64>,
    /// Legs of the loop kept before the divergence.
    pub prefix: usize,
    /// Legs of the plan B after the divergence; empty for a detour that does not exist.
    pub legs: Vec<PlanLeg>,
    /// Leg of the loop where the plan B comes back to it.
    pub rejoin: Option<usize>,
    /// Stretch of the loop on the closed ways (detours), metres along it.
    pub closed_span: Option<(f64, f64)>,
    /// Length of the whole route, metres.
    pub length_m: f64,
    /// Model time of the whole route, seconds.
    pub model_s: f64,
    /// Model time saved over the loop, seconds (negative: longer).
    pub saves_s: f64,
    /// Trails after the divergence, in order.
    pub names: Vec<String>,
    /// French description.
    pub text: String,
}

impl Fallback {
    /// Whether the plan B exists (a detour may not).
    pub fn exists(&self) -> bool {
        !self.legs.is_empty()
    }

    /// The whole route from the start, given the legs of the loop.
    pub fn full_legs(&self, loop_legs: &[PlanLeg]) -> Vec<PlanLeg> {
        let mut v = loop_legs[..self.prefix.min(loop_legs.len())].to_vec();
        v.extend_from_slice(&self.legs);
        if let Some(r) = self.rejoin {
            v.extend_from_slice(&loop_legs[r.min(loop_legs.len())..]);
        }
        v
    }
}

/// Snap at a node on an incident edge that is not closed (a closed edge cannot be left).
pub fn node_snap_open(g: &PisteGraph, node: usize, closed: &BTreeSet<i64>) -> Option<Snap> {
    let h = g.incident_edges(node).iter().find(|h| {
        !g.edge(h.edge)
            .attrs
            .osm_way_id
            .is_some_and(|w| closed.contains(&w))
    })?;
    let e = g.edge(h.edge);
    Some(Snap {
        edge: h.edge,
        along_m: if e.from == node {
            0.0
        } else {
            g.length_m(h.edge)
        },
        pos: g.nodes()[node].pos,
        dist_m: 0.0,
    })
}

/// A snap moved to an open edge when it is at a node (end of its edge).
pub fn open_snap(g: &PisteGraph, s: Snap, closed: &BTreeSet<i64>) -> Snap {
    let e = g.edge(s.edge);
    let node = if s.along_m <= 0.5 {
        Some(e.from)
    } else if s.along_m >= g.length_m(s.edge) - 0.5 {
        Some(e.to)
    } else {
        None
    };
    node.and_then(|n| node_snap_open(g, n, closed)).unwrap_or(s)
}

fn snap_route<M: CostModel>(r: &Router<'_, M>, from: Snap, to: Snap) -> Option<Route> {
    r.route_snaps(from, to).ok().filter(|r| r.cost.is_finite())
}

fn names_text(names: &[String]) -> String {
    let mut v: Vec<String> = names.iter().take(4).cloned().collect();
    if names.len() > 4 {
        v.push("…".into());
    }
    v.join(", ")
}

/// Everything the plans B need from the loop.
pub struct Loop<'a> {
    /// Graph.
    pub graph: &'a PisteGraph,
    /// The validated loop.
    pub course: &'a Course,
    /// Its junctions.
    pub junctions: &'a [Junction],
    /// Time model.
    pub eta: EtaModel,
    /// Level asked.
    pub max_difficulty: Option<Difficulty>,
    /// Model time an exit must save, seconds ([`MIN_EXIT_SAVING_S`] in the plans).
    pub min_exit_saving_s: f64,
    /// Largest model time of a whole route that still ends before the limit (with the C7
    /// factor), seconds, if there is a limit.
    pub budget_s: Option<f64>,
    /// Language of the texts (VO6).
    pub lang: skimap_common::Lang,
}

struct Built {
    prefix: usize,
    legs: Vec<PlanLeg>,
    rejoin: Option<usize>,
    s_rejoin: Option<f64>,
}

impl Loop<'_> {
    fn cost(&self, closed: BTreeSet<i64>) -> PlanCost {
        PlanCost {
            eta: self.eta,
            max_difficulty: self.max_difficulty,
            closed,
        }
    }

    fn closed_text(&self, name: &Option<String>, span: Option<(f64, f64)>) -> String {
        let tx = crate::text::of(self.lang);
        let name = match name {
            Some(n) => {
                let parts: Vec<&str> = n.split(" / ").collect();
                if parts.len() > 3 {
                    format!("{} / …", parts[..3].join(" / "))
                } else {
                    n.clone()
                }
            }
            None => tx.fb_unnamed_trail().into(),
        };
        tx.fb_closed(&name, span)
    }

    fn fallback(
        &self,
        b: Built,
        kind: FallbackKind,
        j: Option<usize>,
        s_div: f64,
    ) -> Option<Fallback> {
        let g = self.graph;
        let mut f = Fallback {
            id: 0,
            kind,
            junction: j,
            s_div,
            s_rejoin: b.s_rejoin,
            prefix: b.prefix,
            legs: b.legs,
            rejoin: b.rejoin,
            closed_span: None,
            length_m: 0.0,
            model_s: 0.0,
            saves_s: 0.0,
            names: Vec::new(),
            text: String::new(),
        };
        let c = Course::new(g, &f.full_legs(self.course.legs()), &self.eta).ok()?;
        f.names = c.names_from(g, s_div + 1.0);
        f.length_m = c.length_m();
        f.model_s = c.model_s();
        f.saves_s = self.course.model_s() - c.model_s();
        self.describe(&mut f);
        Some(f)
    }

    fn describe(&self, f: &mut Fallback) {
        let tx = crate::text::of(self.lang);
        let at = tx.fb_at(f.junction.map(|j| self.junctions[j].s));
        f.text = match &f.kind {
            FallbackKind::Exit => tx.fb_exit(
                &at,
                f.length_m,
                self.course.length_m(),
                f.saves_s,
                &names_text(&f.names),
            ),
            FallbackKind::Detour { name, .. } if !f.exists() => {
                tx.fb_no_detour(&self.closed_text(name, f.closed_span))
            }
            FallbackKind::Detour { name, rejoins, .. } => tx.fb_detour(
                &self.closed_text(name, f.closed_span),
                &at,
                rejoins.then(|| f.s_rejoin.unwrap_or_default()),
                f.length_m,
                f.saves_s,
                &names_text(&f.names),
            ),
        };
    }

    /// Exits at the useful junctions.
    pub fn exits(&self) -> Vec<Fallback> {
        let g = self.graph;
        let c = self.course;
        let router = Router::new(g, self.cost(BTreeSet::new()));
        let end = c.end_snap(g);
        let mut out: Vec<Fallback> = Vec::new();
        for (ji, j) in self.junctions.iter().enumerate() {
            let node = c.nodes()[j.route_node];
            if node.s < EXIT_FROM_M || c.length_m() - node.s < EXIT_BEFORE_END_M {
                continue;
            }
            let Some(from) = node_snap(g, node.node) else {
                continue;
            };
            let Some(r) = snap_route(&router, from, end) else {
                continue;
            };
            let legs: Vec<PlanLeg> = r.legs.iter().map(|l| PlanLeg::from(*l)).collect();
            if legs[..] == c.legs()[node.next_leg..] {
                continue;
            }
            let b = Built {
                prefix: node.next_leg,
                legs,
                rejoin: None,
                s_rejoin: None,
            };
            let Some(f) = self.fallback(b, FallbackKind::Exit, Some(ji), node.s) else {
                continue;
            };
            if f.saves_s < self.min_exit_saving_s {
                continue;
            }
            if let Some(last) = out.last()
                && f.s_div - last.s_div < EXIT_SPACING_M
                && f.saves_s - last.saves_s < 600.0
            {
                continue;
            }
            out.push(f);
        }
        out
    }

    /// Detours around the ways of the loop; ways with the same way around share one.
    pub fn detours(&self) -> Vec<Fallback> {
        let g = self.graph;
        let c = self.course;
        // First and last use of each way.
        let mut ways: Vec<(i64, f64, f64)> = Vec::new();
        for (i, l) in c.legs().iter().enumerate() {
            let Some(w) = g.edge(l.edge).attrs.osm_way_id else {
                continue;
            };
            let (a, b) = c.leg_bounds()[i];
            match ways.iter_mut().find(|x| x.0 == w) {
                Some(x) => x.2 = b,
                None => ways.push((w, a, b)),
            }
        }
        let end = c.end_snap(g);
        let total = c.model_s();
        let mut out: Vec<Fallback> = Vec::new();
        for (w, first, last) in ways {
            let name = c
                .legs()
                .iter()
                .find(|l| g.edge(l.edge).attrs.osm_way_id == Some(w))
                .and_then(|l| g.edge(l.edge).attrs.name.clone());
            let closed = BTreeSet::from([w]);
            let router = Router::new(g, self.cost(closed.clone()));
            // Divergence: the last junction that ends before the first use; else the start.
            let div = self.junctions.iter().rposition(|j| j.s_end <= first + 0.5);
            let (from, prefix, s_div) = match div {
                Some(ji) => {
                    let n = c.nodes()[self.junctions[ji].route_node];
                    match node_snap_open(g, n.node, &closed) {
                        Some(s) => (s, n.next_leg, n.s),
                        None => continue,
                    }
                }
                None => (open_snap(g, c.start_snap(g), &closed), 0, 0.0),
            };
            // Rejoin: the first junction that starts after the last use.
            let rejoin = self
                .junctions
                .iter()
                .find(|j| j.s >= last - 0.5)
                .and_then(|j| c.nodes().iter().find(|n| (n.s - j.s).abs() < 1e-6).copied());
            let kind = |rejoins| FallbackKind::Detour {
                ways: vec![w],
                name: name.clone(),
                rejoins,
            };
            let back = rejoin.and_then(|n| {
                let to = node_snap_open(g, n.node, &closed)?;
                let r = snap_route(&router, from, to)?;
                let b = Built {
                    prefix,
                    legs: r.legs.iter().map(|l| PlanLeg::from(*l)).collect(),
                    rejoin: Some(n.next_leg),
                    s_rejoin: Some(n.s),
                };
                self.fallback(b, kind(true), div, s_div)
            });
            let home = snap_route(&router, from, open_snap(g, end, &closed)).and_then(|r| {
                let b = Built {
                    prefix,
                    legs: r.legs.iter().map(|l| PlanLeg::from(*l)).collect(),
                    rejoin: None,
                    s_rejoin: None,
                };
                self.fallback(b, kind(false), div, s_div)
            });
            let reasonable = |f: &Fallback| {
                f.model_s <= total + (DETOUR_MAX_SHARE * total).max(1200.0)
                    && self.budget_s.is_none_or(|b| f.model_s <= b.max(total))
            };
            let f = match (back, home) {
                (Some(b), Some(h)) => {
                    if reasonable(&b) {
                        b
                    } else {
                        h
                    }
                }
                (Some(b), None) => b,
                (None, Some(h)) => h,
                (None, None) => Fallback {
                    id: 0,
                    kind: kind(false),
                    junction: div,
                    s_div,
                    s_rejoin: None,
                    prefix,
                    legs: Vec::new(),
                    rejoin: None,
                    closed_span: None,
                    length_m: 0.0,
                    model_s: 0.0,
                    saves_s: 0.0,
                    names: Vec::new(),
                    text: String::new(),
                },
            };
            let mut f = f;
            f.closed_span = Some((first, last));
            self.describe(&mut f);
            debug_assert!(
                f.legs
                    .iter()
                    .all(|l| g.edge(l.edge).attrs.osm_way_id != Some(w)),
                "detour uses the closed way {w}"
            );
            // Same way around as another way, or no way around a trail of the same name:
            // one plan B for both.
            let same_name = |o: &Fallback| match (&o.kind, &f.kind) {
                (FallbackKind::Detour { name: a, .. }, FallbackKind::Detour { name: b, .. }) => {
                    a == b
                }
                _ => false,
            };
            if let Some(o) = out.iter_mut().find(|o| {
                (o.prefix == f.prefix && o.legs == f.legs && o.rejoin == f.rejoin)
                    || (!o.exists() && !f.exists() && same_name(o))
            }) {
                o.closed_span = o.closed_span.map(|(a, b)| (a.min(first), b.max(last)));
                if let FallbackKind::Detour { ways, name: on, .. } = &mut o.kind {
                    ways.push(w);
                    if let Some(n) = &name {
                        let known = on.as_ref().is_some_and(|x| {
                            n.split(" / ").all(|p| x.split(" / ").any(|q| q == p))
                        });
                        if !known {
                            *on = Some(match on.take() {
                                Some(x) => {
                                    let mut parts: Vec<&str> = x.split(" / ").collect();
                                    for p in n.split(" / ") {
                                        if !parts.contains(&p) {
                                            parts.push(p);
                                        }
                                    }
                                    parts.join(" / ")
                                }
                                None => n.clone(),
                            });
                        }
                    }
                }
                self.describe(o);
                continue;
            }
            out.push(f);
        }
        out
    }

    /// Exits then detours, numbered.
    pub fn all(&self) -> Vec<Fallback> {
        let mut v = self.exits();
        v.extend(self.detours());
        for (i, f) in v.iter_mut().enumerate() {
            f.id = i;
        }
        v
    }
}

/// Fastest route between two points of the network with closed ways, under the plan's
/// cost (used during the outing when no plan B fits).
pub fn route(
    g: &PisteGraph,
    eta: EtaModel,
    max_difficulty: Option<Difficulty>,
    closed: &BTreeSet<i64>,
    from: Snap,
    to: Snap,
) -> Option<Route> {
    let router = Router::new(
        g,
        PlanCost {
            eta,
            max_difficulty,
            closed: closed.clone(),
        },
    );
    snap_route(&router, from, to)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::course::tests::{square, square_loop};
    use crate::guidance::junctions;
    use skimap_core::routing::Style;

    #[test]
    fn square_has_exits_and_detours() {
        let g = square();
        let eta = EtaModel::new(Style::Classic);
        let c = Course::new(&g, &square_loop(&g), &eta).unwrap();
        let js = junctions(&g, &c);
        let l = Loop {
            graph: &g,
            course: &c,
            junctions: &js,
            eta,
            max_difficulty: None,
            min_exit_saving_s: 60.0,
            budget_s: None,
            lang: skimap_common::Lang::Fr,
        };
        let all = l.all();
        // Exits from 1 km to 1 km before the end: node 1 (1.0 km, back along « Sud », 2 km
        // instead of 4) and node 2 (2.0 km, by the diagonal, 3.41 km), 1 km apart.
        let exits: Vec<&Fallback> = all
            .iter()
            .filter(|f| f.kind == FallbackKind::Exit)
            .collect();
        assert_eq!(exits.len(), 2, "{all:#?}");
        assert!((exits[0].length_m - 2000.0).abs() < 5.0, "{:?}", exits[0]);
        assert_eq!(exits[0].names, vec!["Sud"]);
        assert!((exits[1].length_m - 3414.0).abs() < 10.0, "{:?}", exits[1]);
        assert_eq!(exits[1].names, vec!["Diagonale"]);
        // 586 m less at about 10.6 km/h.
        assert!(
            (exits[1].saves_s - 199.0).abs() < 10.0,
            "{}",
            exits[1].saves_s
        );
        // Detours: « Nord » and « Ouest » have the same one (the diagonal from node 2): one
        // plan B for both; « Sud » and « Est » each have their own.
        let det: Vec<&Fallback> = all
            .iter()
            .filter(|f| matches!(f.kind, FallbackKind::Detour { .. }))
            .collect();
        assert_eq!(det.len(), 3, "{det:#?}");
        let shared = det
            .iter()
            .find(|d| matches!(&d.kind, FallbackKind::Detour { ways, .. } if ways.len() == 2))
            .expect("one detour for two ways");
        let FallbackKind::Detour {
            ways,
            name,
            rejoins,
        } = &shared.kind
        else {
            unreachable!()
        };
        assert_eq!(ways, &vec![3, 4]);
        assert_eq!(name.as_deref(), Some("Nord / Ouest"));
        assert!(!rejoins);
        assert!(shared.text.contains("Nord / Ouest"), "{}", shared.text);
        for d in &det {
            let FallbackKind::Detour { ways, .. } = &d.kind else {
                unreachable!()
            };
            assert!(d.exists(), "{d:?}");
            assert!(
                d.legs
                    .iter()
                    .all(|l| !ways.contains(&g.edge(l.edge).attrs.osm_way_id.unwrap_or(0))),
                "{d:?}"
            );
            let full = d.full_legs(c.legs());
            let c2 = Course::new(&g, &full, &eta).unwrap();
            // Starts and ends where the loop does.
            assert!(c2.line().points()[0].haversine_m(c.line().points()[0]) < 1.0);
            let (a, b) = (
                c2.line().points().last().unwrap(),
                c.line().points().last().unwrap(),
            );
            assert!(a.haversine_m(*b) < 1.0);
            assert!((c2.length_m() - d.length_m).abs() < 1e-6);
        }
        // Ids numbered in order.
        assert!(all.iter().enumerate().all(|(i, f)| f.id == i));
    }
}
