//! Property tests of the trail graph (Q3, #103): on-trail routing against a reference
//! shortest-path computation, monotonicity of the time model, map matching of a trace
//! lying exactly on the network.
//!
//! Graphs are random: nodes in a 1 km box around Cervières, edges between random node
//! pairs (parallel edges, self-loops and zero-length edges included), random one-way
//! flags, grooming attributes and elevation profiles; for the exact matching property,
//! lattice networks without ambiguous geometry.

use proptest::prelude::*;
use skimap_common::geom::LonLat;
use skimap_core::eta::{Conditions, EtaModel, SkierParams, SnowClass};
use skimap_core::graph::{
    Edge, EdgeAttrs, EdgeKind, GraphData, HalfEdge, Node, PisteGraph, Profile, Snap,
};
use skimap_core::matching::{CorridorMonitor, Fix, MatchParams, MatchState, Matcher};
use skimap_core::routing::{
    self, CostModel, Distance, Leg, Route, Router, Style, StylePolicy, edge_pieces,
};

const LON0: f64 = 6.75;
const LAT0: f64 = 44.85;

// ---------------------------------------------------------------------------------------
// Random graphs
// ---------------------------------------------------------------------------------------

#[derive(Debug, Clone)]
struct EdgeSpec {
    from: usize,
    to: usize,
    oneway: bool,
    /// Intermediate points, offsets in degrees.
    mid: Vec<(f64, f64)>,
    classic: Option<bool>,
    skating: Option<bool>,
    hike: bool,
    nordic: bool,
    backcountry: bool,
    /// Station fractions in (0, 1) and elevations (first and last added).
    profile: Option<(Vec<f64>, Vec<f32>)>,
}

fn attrs(e: &EdgeSpec) -> EdgeAttrs {
    EdgeAttrs {
        kind: EdgeKind::Piste,
        name: None,
        osm_way_id: None,
        classic: e.classic,
        skating: e.skating,
        backcountry: e.backcountry,
        nordic: e.nordic,
        hike: e.hike,
        difficulty: None,
        difficulty_max: None,
    }
}

fn build(nodes: &[(f64, f64)], edges: &[EdgeSpec]) -> PisteGraph {
    let nodes: Vec<Node> = nodes
        .iter()
        .map(|&(dx, dy)| Node {
            pos: LonLat::new(LON0 + dx, LAT0 + dy),
            osm_node_id: None,
        })
        .collect();
    let edges = edges
        .iter()
        .map(|e| {
            let mut points = vec![nodes[e.from].pos];
            points.extend(
                e.mid
                    .iter()
                    .map(|&(dx, dy)| LonLat::new(LON0 + dx, LAT0 + dy)),
            );
            points.push(nodes[e.to].pos);
            let len: f64 = points.windows(2).map(|w| w[0].haversine_m(w[1])).sum();
            let profile = e.profile.as_ref().and_then(|(fr, z)| {
                // Strictly increasing f32 distances from 0 to the length.
                let mut d = vec![0.0f32];
                let mut fr = fr.clone();
                fr.sort_by(f64::total_cmp);
                for f in fr {
                    let v = (f * len) as f32;
                    if v > *d.last().expect("seeded") && v < len as f32 {
                        d.push(v);
                    }
                }
                if (len as f32) <= *d.last().expect("seeded") {
                    return None;
                }
                d.push(len as f32);
                let elev = (0..d.len()).map(|k| z[k % z.len()]).collect();
                Some(Profile {
                    dist_m: d,
                    elev_m: elev,
                    filled: 0,
                })
            });
            Edge {
                from: e.from,
                to: e.to,
                oneway: e.oneway,
                points,
                attrs: attrs(e),
                profile,
            }
        })
        .collect();
    PisteGraph::new(GraphData {
        site: "prop".into(),
        nodes,
        edges,
        elevation_source: None,
    })
    .expect("valid random graph")
}

fn offset() -> impl Strategy<Value = (f64, f64)> {
    (-0.006f64..0.006, -0.004f64..0.004)
}

fn edge_spec(n: usize) -> impl Strategy<Value = EdgeSpec> {
    (
        (0..n, 0..n, prop::bool::weighted(0.25)),
        prop::collection::vec(offset(), 0..4),
        (
            prop::option::of(any::<bool>()),
            prop::option::of(any::<bool>()),
            any::<bool>(),
            prop::bool::weighted(0.8),
            prop::bool::weighted(0.2),
        ),
        prop::option::weighted(
            0.7,
            (
                prop::collection::vec(0.0f64..1.0, 0..12),
                prop::collection::vec(1200.0f32..1500.0, 1..14),
            ),
        ),
    )
        .prop_map(
            |((from, to, oneway), mid, (classic, skating, hike, nordic, backcountry), profile)| {
                EdgeSpec {
                    from,
                    to,
                    oneway,
                    mid,
                    classic,
                    skating,
                    hike,
                    nordic,
                    backcountry,
                    profile,
                }
            },
        )
}

fn graph() -> impl Strategy<Value = (Vec<(f64, f64)>, Vec<EdgeSpec>)> {
    (2usize..10).prop_flat_map(|n| {
        (
            prop::collection::vec(offset(), n),
            prop::collection::vec(edge_spec(n), 1..18),
        )
    })
}

/// Trail networks without ambiguous geometry: nodes on a jittered 160 m lattice, edges
/// between lattice neighbours (orthogonal or diagonal, at most one per pair), each bent by
/// up to 15 m at its middle. Two edges are then never parallel within the GPS noise except
/// at a shared node, so every fix has a unique closest trail.
fn lattice_graph() -> impl Strategy<Value = (Vec<(f64, f64)>, Vec<EdgeSpec>)> {
    const COLS: usize = 4;
    const ROWS: usize = 3;
    const DX: f64 = 160.0 / (111_195.0 * 0.708_7);
    const DY: f64 = 160.0 / 111_195.0;
    let pairs: Vec<(usize, usize)> = (0..COLS * ROWS)
        .flat_map(|i| {
            let (c, r) = (i % COLS, i / COLS);
            let mut v = Vec::new();
            if c + 1 < COLS {
                v.push((i, i + 1));
            }
            if r + 1 < ROWS {
                v.push((i, i + COLS));
                if c + 1 < COLS {
                    v.push((i, i + COLS + 1));
                }
            }
            v
        })
        .collect();
    let np = pairs.len();
    (
        prop::collection::vec((-0.1f64..0.1, -0.1f64..0.1), COLS * ROWS),
        prop::collection::vec(
            (
                any::<bool>(),
                any::<bool>(),
                prop::bool::weighted(0.2),
                -1.0f64..1.0,
            ),
            np,
        ),
    )
        .prop_map(move |(jitter, picks)| {
            let nodes: Vec<(f64, f64)> = jitter
                .iter()
                .enumerate()
                .map(|(i, (jx, jy))| (((i % COLS) as f64 + jx) * DX, ((i / COLS) as f64 + jy) * DY))
                .collect();
            let mut edges = Vec::new();
            for (&(u, v), &(keep, flip, oneway, bend)) in pairs.iter().zip(&picks) {
                if !keep {
                    continue;
                }
                let (from, to) = if flip { (v, u) } else { (u, v) };
                let (a, b) = (nodes[from], nodes[to]);
                // Middle point moved perpendicularly by up to 15 m.
                let (ex, ey) = ((b.0 - a.0) / DX * 160.0, (b.1 - a.1) / DY * 160.0);
                let l = ex.hypot(ey);
                let k = 15.0 * bend / l;
                let mid = (
                    0.5 * (a.0 + b.0) - k * ey * DX / 160.0,
                    0.5 * (a.1 + b.1) + k * ex * DY / 160.0,
                );
                edges.push(EdgeSpec {
                    from,
                    to,
                    oneway,
                    mid: vec![mid],
                    ..plain_spec()
                });
            }
            if edges.is_empty() {
                edges.push(EdgeSpec {
                    from: 0,
                    to: 1,
                    mid: vec![(
                        0.5 * (nodes[0].0 + nodes[1].0),
                        0.5 * (nodes[0].1 + nodes[1].1),
                    )],
                    ..plain_spec()
                });
            }
            (nodes, edges)
        })
}

/// Fixes every `step` metres along the legs of a route, 1 s apart.
fn fixes_on(g: &PisteGraph, route: &Route, step: f64) -> Vec<Fix> {
    let mut fixes = Vec::new();
    let mut t_s = 0.0;
    for l in &route.legs {
        let len = (l.to_m - l.from_m).abs();
        let n = (len / step).ceil().max(1.0) as usize;
        for k in 0..=n {
            let along = l.from_m + (l.to_m - l.from_m) * k as f64 / n as f64;
            fixes.push(Fix {
                t_s,
                pos: g.point_at(l.edge, along),
            });
            t_s += 1.0;
        }
    }
    fixes
}

fn style() -> impl Strategy<Value = Style> {
    prop_oneof![Just(Style::Classic), Just(Style::Skating)]
}

fn policy() -> impl Strategy<Value = StylePolicy> {
    (
        prop_oneof![Just(1.0), Just(1.5), Just(f64::INFINITY)],
        prop_oneof![Just(1.0), Just(3.0), Just(f64::INFINITY)],
        prop_oneof![Just(1.0), Just(3.0), Just(f64::INFINITY)],
    )
        .prop_map(|(a, b, c)| StylePolicy {
            classic_on_skating_only: a,
            skating_on_classic_only: b,
            hike_only: c,
        })
}

fn conditions() -> impl Strategy<Value = Conditions> {
    (
        prop_oneof![
            Just(SnowClass::Groomed),
            Just(SnowClass::NewSnow),
            Just(SnowClass::Wet),
            Just(SnowClass::Icy)
        ],
        -30.0f64..8.0,
    )
        .prop_map(|(snow, temperature_c)| Conditions {
            snow,
            temperature_c,
        })
}

fn skier() -> impl Strategy<Value = SkierParams> {
    (
        40.0f64..120.0,
        20.0f64..300.0,
        0.2f64..1.0,
        3.0f64..15.0,
        0.5f64..2.0,
    )
        .prop_map(
            |(mass_kg, power_w, cda_m2, max_speed_ms, friction_factor)| SkierParams {
                mass_kg,
                power_w,
                cda_m2,
                max_speed_ms,
                friction_factor,
            },
        )
}

fn eta_model() -> impl Strategy<Value = EtaModel> {
    (skier(), conditions(), style(), policy()).prop_map(|(skier, conditions, style, policy)| {
        EtaModel {
            skier,
            conditions,
            style,
            policy,
            default_elev_m: 1000.0,
        }
    })
}

// ---------------------------------------------------------------------------------------
// Reference shortest paths
// ---------------------------------------------------------------------------------------

/// Cost of a full half-edge by direct integration of its pieces (no cached sums).
fn reference_half_cost<M: CostModel>(g: &PisteGraph, m: &M, h: HalfEdge) -> f64 {
    let l = g.length_m(h.edge);
    if h.forward {
        routing::partial_cost(g, m, h.edge, 0.0, l)
    } else {
        routing::partial_cost(g, m, h.edge, l, 0.0)
    }
}

/// All-pairs node distances (Floyd–Warshall) on the directed half-edges.
#[allow(clippy::needless_range_loop)]
fn floyd<M: CostModel>(g: &PisteGraph, m: &M) -> Vec<Vec<f64>> {
    let n = g.nodes().len();
    let mut d = vec![vec![f64::INFINITY; n]; n];
    for (i, row) in d.iter_mut().enumerate() {
        row[i] = 0.0;
    }
    for e in 0..g.edges().len() {
        for forward in [true, false] {
            let h = HalfEdge { edge: e, forward };
            if !forward && g.edge(e).oneway {
                continue;
            }
            let (u, v) = (g.tail(h), g.head(h));
            let c = reference_half_cost(g, m, h);
            if c < d[u][v] {
                d[u][v] = c;
            }
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
    d
}

/// Reference cost between two points of the network: direct along a shared edge, or exit
/// the start edge at one of its nodes, shortest node path, enter the end edge. `part` is
/// the cost of a partial edge.
fn reference_cost(
    g: &PisteGraph,
    part: impl Fn(usize, f64, f64) -> f64,
    d: &[Vec<f64>],
    s: Snap,
    t: Snap,
) -> f64 {
    let mut best = f64::INFINITY;
    if s.edge == t.edge {
        best = best.min(part(s.edge, s.along_m, t.along_m));
    }
    let (se, te) = (g.edge(s.edge), g.edge(t.edge));
    let (sl, tl) = (g.length_m(s.edge), g.length_m(t.edge));
    let exits = [
        (se.to, part(s.edge, s.along_m, sl)),
        (se.from, part(s.edge, s.along_m, 0.0)),
    ];
    let entries = [
        (te.from, part(t.edge, 0.0, t.along_m)),
        (te.to, part(t.edge, tl, t.along_m)),
    ];
    for (u, cu) in exits {
        for (v, cv) in entries {
            best = best.min(cu + d[u][v] + cv);
        }
    }
    best
}

fn snap_at(g: &PisteGraph, edge: usize, frac: f64) -> Snap {
    let along = frac * g.length_m(edge);
    Snap {
        edge,
        along_m: along,
        pos: g.point_at(edge, along),
        dist_m: 0.0,
    }
}

fn close(a: f64, b: f64, rel: f64) -> bool {
    (a.is_infinite() && b.is_infinite()) || (a - b).abs() <= rel * a.abs().max(b.abs()).max(1.0)
}

/// Checks that a route is a valid walk on the network and that its cost is the sum of its
/// legs.
fn check_route<M: CostModel>(r: &Router<'_, M>, route: &Route) {
    let g = r.graph();
    let legs: &[Leg] = &route.legs;
    let mut sum = 0.0;
    for l in legs {
        if l.to_m < l.from_m {
            assert!(
                !g.edge(l.edge).oneway,
                "one-way edge {} travelled backward",
                l.edge
            );
        }
        sum += r.partial_cost(l.edge, l.from_m, l.to_m);
    }
    assert!(
        close(sum, route.cost, 1e-9),
        "legs sum {sum}, route cost {}",
        route.cost
    );
    // Consecutive legs meet at a node of both edges (at the same position: zero-length
    // legs, e.g. a zero-length edge between two nodes at the same place, are dropped).
    let at_node = |e: usize, d: f64| d.abs() < 1e-6 || (d - g.length_m(e)).abs() < 1e-6;
    for w in legs.windows(2) {
        let (a, b) = (&w[0], &w[1]);
        assert!(at_node(a.edge, a.to_m), "leg {a:?} ends inside its edge");
        assert!(
            at_node(b.edge, b.from_m),
            "leg {b:?} starts inside its edge"
        );
        let pa = g.point_at(a.edge, a.to_m);
        let pb = g.point_at(b.edge, b.from_m);
        assert!(
            pa.haversine_m(pb) < 1e-3,
            "legs {a:?} and {b:?} do not meet"
        );
    }
    if let (Some(first), Some(last)) = (legs.first(), legs.last()) {
        let p0 = g.point_at(first.edge, first.from_m);
        let p1 = g.point_at(last.edge, last.to_m);
        assert!(
            p0.haversine_m(route.start.pos) < 1e-3,
            "route does not start at its start"
        );
        assert!(
            p1.haversine_m(route.end.pos) < 1e-3,
            "route does not end at its end"
        );
    }
}

// ---------------------------------------------------------------------------------------
// Properties
// ---------------------------------------------------------------------------------------

proptest! {

    /// Dijkstra of the router is optimal: same cost as the reference on every node pair
    /// and between random mid-edge points, under the distance model with style factors.
    #[test]
    fn routing_distance_is_optimal(
        (nodes, edges) in graph(),
        style in style(),
        policy in policy(),
        pairs in prop::collection::vec((any::<prop::sample::Index>(), 0.0f64..=1.0, any::<prop::sample::Index>(), 0.0f64..=1.0), 1..6),
    ) {
        let g = build(&nodes, &edges);
        let model = Distance { style, policy };
        let r = Router::new(&g, model);
        let d = floyd(&g, &model);
        let m = g.edges().len();
        for (ia, fa, ib, fb) in pairs {
            let s = snap_at(&g, ia.index(m), fa);
            let t = snap_at(&g, ib.index(m), fb);
            let want = reference_cost(&g, |e, a, b| routing::partial_cost(&g, &model, e, a, b), &d, s, t);
            match r.route_snaps(s, t) {
                Ok(route) => {
                    prop_assert!(close(route.cost, want, 1e-9), "router {} vs reference {want}", route.cost);
                    check_route(&r, &route);
                }
                Err(_) => prop_assert!(want.is_infinite(), "no route but reference {want}"),
            }
        }
    }

    /// Same with the time model (C5): the fastest route has the reference time.
    #[test]
    fn routing_time_is_optimal(
        (nodes, edges) in graph(),
        model in eta_model(),
        pairs in prop::collection::vec((any::<prop::sample::Index>(), 0.0f64..=1.0, any::<prop::sample::Index>(), 0.0f64..=1.0), 1..6),
    ) {
        let g = build(&nodes, &edges);
        let r = Router::new(&g, model);
        let d = floyd(&g, &model);
        let m = g.edges().len();
        for (ia, fa, ib, fb) in pairs {
            let s = snap_at(&g, ia.index(m), fa);
            let t = snap_at(&g, ib.index(m), fb);
            // Partial end pieces as the router defines them (linear inside a piece of
            // profile, see `partial_cost_interpolation_is_accurate`): the search itself is
            // compared exactly.
            let want = reference_cost(&g, |e, a, b| r.partial_cost(e, a, b), &d, s, t);
            match r.route_snaps(s, t) {
                Ok(route) => {
                    prop_assert!(close(route.cost, want, 1e-9), "router {} vs reference {want}", route.cost);
                    check_route(&r, &route);
                }
                Err(_) => prop_assert!(want.is_infinite(), "no route but reference {want}"),
            }
        }
    }

    /// Node to node, the router and the reference agree to rounding (no partial piece).
    #[test]
    fn routing_node_pairs_match_floyd((nodes, edges) in graph(), model in eta_model()) {
        let g = build(&nodes, &edges);
        let r = Router::new(&g, model);
        let d = floyd(&g, &model);
        for e in 0..g.edges().len() {
            for f in 0..g.edges().len() {
                let s = snap_at(&g, e, 0.0);
                let t = snap_at(&g, f, 1.0);
                let want = reference_cost(&g, |e, a, b| routing::partial_cost(&g, &model, e, a, b), &d, s, t);
                match r.route_snaps(s, t) {
                    Ok(route) => prop_assert!(close(route.cost, want, 1e-9), "{e}->{f}: {} vs {want}", route.cost),
                    Err(_) => prop_assert!(want.is_infinite()),
                }
            }
        }
    }

    /// The router's partial edge cost (cumulative cost interpolated inside a 10 m piece of
    /// profile) matches the direct integration of the partial pieces, on profiles like
    /// the real ones (10 m stations, grades within ±40 %): relative error below 1e-4.
    #[test]
    fn partial_cost_interpolation_is_accurate(
        model in eta_model(),
        grades in prop::collection::vec(-0.4f32..0.4, 2..80),
        cuts in prop::collection::vec((0.0f64..=1.0, 0.0f64..=1.0), 1..8),
    ) {
        let n = grades.len();
        let len = 10.0 * n as f64;
        // Straight east-west edge of `len` metres.
        let dlon = len / (111_195.0 * LAT0.to_radians().cos());
        let mut z = vec![1400.0f32];
        for gr in &grades {
            z.push(z.last().expect("seeded") + 10.0 * gr);
        }
        let mut data = build(&[(0.0, 0.0), (dlon, 0.0)], &[EdgeSpec { from: 0, to: 1, ..plain_spec() }]).data().clone();
        let l = data.edges[0].points[0].haversine_m(data.edges[0].points[1]);
        data.edges[0].profile = Some(Profile {
            dist_m: (0..=n).map(|k| (l * k as f64 / n as f64) as f32).collect(),
            elev_m: z,
            filled: 0,
        });
        let g = PisteGraph::new(data).expect("valid graph");
        let r = Router::new(&g, model);
        for (a, b) in cuts {
            let (a, b) = (a * l, b * l);
            let direct = routing::partial_cost(&g, &model, 0, a, b);
            let cached = r.partial_cost(0, a, b);
            prop_assert!(close(cached, direct, 1e-4), "{a}->{b}: cached {cached}, direct {direct}");
        }
    }

    /// ETA monotone with distance: at a fixed grade the time of a piece is proportional
    /// to its length; a route's time is the sum of its pieces, each non-negative.
    #[test]
    fn eta_is_proportional_to_distance_at_a_fixed_grade(
        model in eta_model(),
        grade in -0.6f64..0.6,
        dist in 0.1f64..500.0,
        k in 1.0f64..20.0,
        elev in prop::option::of(0.0f64..3500.0),
        ungroomed in any::<bool>(),
    ) {
        let a = EdgeAttrs { backcountry: ungroomed, ..attrs(&plain_spec()) };
        let t1 = model.piece_time_s(&a, dist, grade * dist, elev);
        let tk = model.piece_time_s(&a, k * dist, k * grade * dist, elev);
        prop_assert!(t1 > 0.0 && t1.is_finite());
        prop_assert!(close(tk, k * t1, 1e-9), "{tk} vs {k} × {t1}");
        prop_assert!(tk >= t1);
    }

    /// ETA monotone with climb: at a fixed horizontal distance, more climb never takes
    /// less time; the speed never increases with the grade (classic and skating).
    #[test]
    fn eta_grows_with_climb_and_speed_falls_with_grade(
        model in eta_model(),
        dist in 1.0f64..500.0,
        dz in prop::collection::vec(0.0f64..300.0, 2),
        grades in prop::collection::vec(-1.0f64..1.0, 2),
        elev in 0.0f64..3500.0,
        ungroomed in any::<bool>(),
    ) {
        let a = EdgeAttrs { backcountry: ungroomed, ..attrs(&plain_spec()) };
        let (lo, hi) = if dz[0] <= dz[1] { (dz[0], dz[1]) } else { (dz[1], dz[0]) };
        let tl = model.piece_time_s(&a, dist, lo, Some(elev));
        let th = model.piece_time_s(&a, dist, hi, Some(elev));
        prop_assert!(th >= tl * (1.0 - 1e-12), "climb {lo} m: {tl} s, climb {hi} m: {th} s");
        let (g0, g1) = if grades[0] <= grades[1] { (grades[0], grades[1]) } else { (grades[1], grades[0]) };
        let v0 = model.speed_ms(g0, elev, ungroomed);
        let v1 = model.speed_ms(g1, elev, ungroomed);
        prop_assert!(v0 > 0.0 && v1 > 0.0);
        prop_assert!(v1 <= v0 * (1.0 + 1e-9), "grade {g0}: {v0} m/s, grade {g1}: {v1} m/s");
    }

    /// A route's estimated time grows with every leg: the time of a prefix of the route
    /// is at most the time of the route.
    #[test]
    fn eta_of_a_route_grows_along_it(
        (nodes, edges) in graph(),
        model in eta_model(),
        ia in any::<prop::sample::Index>(),
        ib in any::<prop::sample::Index>(),
    ) {
        let g = build(&nodes, &edges);
        let r = Router::new(&g, model);
        let m = g.edges().len();
        let (s, t) = (snap_at(&g, ia.index(m), 0.3), snap_at(&g, ib.index(m), 0.7));
        if let Ok(route) = r.route_snaps(s, t) {
            let mut acc = 0.0;
            let mut last = 0.0;
            for l in &route.legs {
                let pieces = edge_pieces(&g, l.edge, l.from_m, l.to_m);
                acc += model.time_s(&pieces).total_s;
                prop_assert!(acc >= last);
                last = acc;
            }
            let total = model.time_s(&route.pieces(&g)).total_s;
            prop_assert!(close(acc, total, 1e-9), "{acc} vs {total}");
        }
    }

    /// A trace lying exactly on the network is attached to the network at every fix
    /// (100 %), online (filtered, what the phone shows) and after Viterbi decoding
    /// (smoothed, the recorded track), on any graph: parallel edges, self-loops,
    /// zero-length edges, edges drawn over themselves.
    #[test]
    fn trace_on_a_piste_is_never_off_the_network(
        (nodes, edges) in graph(),
        ia in any::<prop::sample::Index>(),
        ib in any::<prop::sample::Index>(),
        step in 3.0f64..25.0,
    ) {
        let g = build(&nodes, &edges);
        let r = Router::new(&g, Distance { style: Style::Classic, policy: StylePolicy::default() });
        let m = g.edges().len();
        let (s, t) = (snap_at(&g, ia.index(m), 0.1), snap_at(&g, ib.index(m), 0.9));
        let Ok(route) = r.route_snaps(s, t) else { return Ok(()); };
        let fixes = fixes_on(&g, &route, step);
        let mut matcher = Matcher::new(&g, MatchParams::default());
        for f in &fixes {
            prop_assert!(
                matches!(matcher.push(*f), MatchState::OnTrail(_)),
                "fix {f:?} matched off the network"
            );
        }
        let decoded = matcher.decode();
        prop_assert_eq!(decoded.len(), fixes.len());
        for (st, f) in decoded.iter().zip(&fixes) {
            prop_assert!(matches!(st, MatchState::OnTrail(_)), "decoded off the network at {f:?}");
        }
    }

    /// On a network without ambiguous geometry ([`lattice_graph`]), a trace lying exactly
    /// on a route is matched exactly: every fix, online and decoded, on the network at the
    /// fix position; a corridor monitor on the route polyline never alerts.
    ///
    /// Found by the general version of this test: the matcher keeps one candidate per edge
    /// (its nearest point), so on an edge drawn over itself (out and back) the candidate
    /// may be the other branch; and at a hairpin next to a trail a few metres away, the
    /// straight-line distance between two fixes is much shorter than the distance along
    /// the trail, so the transition model may prefer the other trail for a fix (online:
    /// 9.3 and 10.2 m away; decoded: on a branch 7 m away). Neither shape is in the graphs
    /// of the two sites (no edge drawn over itself among the 997 edges); both stay on the
    /// network (previous test).
    #[test]
    fn trace_on_a_piste_is_matched_exactly(
        (nodes, edges) in lattice_graph(),
        ia in any::<prop::sample::Index>(),
        ib in any::<prop::sample::Index>(),
        fa in 0.0f64..=1.0,
        fb in 0.0f64..=1.0,
        step in 3.0f64..25.0,
    ) {
        let g = build(&nodes, &edges);
        let r = Router::new(&g, Distance { style: Style::Classic, policy: StylePolicy::default() });
        let m = g.edges().len();
        let (s, t) = (snap_at(&g, ia.index(m), fa), snap_at(&g, ib.index(m), fb));
        let Ok(route) = r.route_snaps(s, t) else { return Ok(()); };
        let fixes = fixes_on(&g, &route, step);
        let mut matcher = Matcher::new(&g, MatchParams::default());
        for f in &fixes {
            match matcher.push(*f) {
                MatchState::OnTrail(snap) => {
                    prop_assert!(snap.dist_m < 0.5, "online match {} m away", snap.dist_m);
                }
                MatchState::OffTrail => prop_assert!(false, "fix {f:?} matched off the network"),
            }
        }
        for (st, f) in matcher.decode().iter().zip(&fixes) {
            match st {
                MatchState::OnTrail(snap) => {
                    prop_assert!(snap.dist_m < 0.5, "decoded {} m away", snap.dist_m);
                    prop_assert!(snap.pos.haversine_m(f.pos) < 0.5);
                }
                MatchState::OffTrail => prop_assert!(false, "decoded off the network at {f:?}"),
            }
        }
        let plan = route.polyline(&g);
        if plan.len() >= 2 {
            let mut corridor = CorridorMonitor::new(&plan, skimap_core::matching::CORRIDOR_HYSTERESIS);
            for f in &fixes {
                let out = corridor.push(*f);
                prop_assert!(out.dist_m < 0.5, "fix {} m from the plan", out.dist_m);
                prop_assert!(out.event.is_none(), "corridor alert on the plan at {f:?}");
            }
            prop_assert!(!corridor.alerting());
        }
    }
}

fn plain_spec() -> EdgeSpec {
    EdgeSpec {
        from: 0,
        to: 0,
        oneway: false,
        mid: vec![],
        classic: Some(true),
        skating: Some(true),
        hike: false,
        nordic: true,
        backcountry: false,
        profile: None,
    }
}
