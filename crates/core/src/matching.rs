//! Map matching on the trail network and deviation detection (C2, #34; validation C10,
//! #42): HMM matching, corridor around a planned off-piste track, threshold and delay
//! against false alarms.
//!
//! # Matching ([`Matcher`])
//!
//! Hidden Markov model after Newson & Krumm (2009), solved online by Viterbi:
//!
//! - states at each fix: the nearest point of every pass of an edge within
//!   [`MatchParams::radius_m`] ([`PisteGraph::candidates`]: a hairpin or a way drawn out
//!   and back gives both passes; at most [`MatchParams::max_candidates`]), plus one
//!   « off the network » state;
//! - emission: GPS distance `d` to the candidate under a mixture of two centred normals,
//!   `(1 − ε) N(d; σ) + ε N(d; σ_out)` (the wide one absorbs forest multipath outliers);
//!   the off state emits as a candidate at [`MatchParams::off_distance_m`];
//! - transition between two on-network states: `exp(−|d_route − d_gc| / β) / β`, with
//!   `d_route` the shortest distance along the network (one-way ignored: a skier can go
//!   the wrong way, the matcher must follow) and `d_gc` the distance between the fixes;
//!   switching on ↔ off costs [`MatchParams::switch_log_p`];
//! - network distances come from one bounded Dijkstra per previous candidate (bound
//!   `d_gc + 2 r`), with reused buffers.
//!
//! [`Matcher::push`] returns the filtered state of the newest fix (what the phone shows
//! and what the alerts use); [`Matcher::decode`] backtracks the Viterbi path over the
//! retained history (smoothed, for the recorded track).
//!
//! # Deviation detection
//!
//! [`Hysteresis`]: an alert starts when the distance stays above `enter_m` for
//! `enter_s` seconds and ends when it stays below `exit_m` for `exit_s` seconds.
//! [`RouteMonitor`] measures the distance from the matched point (on the network, or
//! the raw fix when off the network) to the planned route; [`CorridorMonitor`] measures
//! the raw fix against a planned off-piste polyline (corridor mode).

use std::cmp::Ordering;
use std::collections::{BinaryHeap, VecDeque};

use rstar::primitives::{GeomWithData, Line};
use rstar::{PointDistance, RTree};
use skimap_common::geom::LonLat;

use crate::graph::{LocalPlane, PisteGraph, Snap};

/// A GPS fix.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Fix {
    /// Time, seconds (any origin, increasing).
    pub t_s: f64,
    /// Position.
    pub pos: LonLat,
}

/// Matched state of a fix.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum MatchState {
    /// On the network.
    OnTrail(Snap),
    /// Off the network (or the GPS is too far from every trail).
    OffTrail,
}

/// Parameters of the HMM.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MatchParams {
    /// GPS noise, metres (standard deviation of the main emission component).
    pub sigma_m: f64,
    /// Weight of the outlier component.
    pub outlier_weight: f64,
    /// Standard deviation of the outlier component, metres.
    pub outlier_sigma_m: f64,
    /// Candidate search radius, metres.
    pub radius_m: f64,
    /// Maximum candidates per fix.
    pub max_candidates: usize,
    /// Transition scale β, metres.
    pub beta_m: f64,
    /// The off-network state emits like a candidate at this distance, metres.
    pub off_distance_m: f64,
    /// Log-probability of switching between on and off the network.
    pub switch_log_p: f64,
    /// Fixes kept for [`Matcher::decode`].
    pub history: usize,
}

impl Default for MatchParams {
    fn default() -> Self {
        Self {
            sigma_m: 10.0,
            outlier_weight: 0.1,
            outlier_sigma_m: 40.0,
            radius_m: 100.0,
            max_candidates: 8,
            beta_m: 10.0,
            off_distance_m: 60.0,
            switch_log_p: -8.0,
            history: 3600,
        }
    }
}

impl MatchParams {
    fn log_emission(&self, d: f64) -> f64 {
        let n = |s: f64| (-0.5 * (d / s).powi(2)).exp() / s;
        ((1.0 - self.outlier_weight) * n(self.sigma_m)
            + self.outlier_weight * n(self.outlier_sigma_m))
        .ln()
    }
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

/// Reusable buffers of the bounded Dijkstra.
#[derive(Debug, Clone)]
struct Workspace {
    dist: Vec<f64>,
    touched: Vec<usize>,
    heap: BinaryHeap<Item>,
}

impl Workspace {
    fn new(n: usize) -> Self {
        Self {
            dist: vec![f64::INFINITY; n],
            touched: Vec::new(),
            heap: BinaryHeap::new(),
        }
    }

    /// Network distances (undirected) from a point to the nodes within `limit` metres.
    fn run(&mut self, g: &PisteGraph, s: &Snap, limit: f64) {
        for &u in &self.touched {
            self.dist[u] = f64::INFINITY;
        }
        self.touched.clear();
        self.heap.clear();
        let e = g.edge(s.edge);
        for (node, d) in [(e.from, s.along_m), (e.to, g.length_m(s.edge) - s.along_m)] {
            if d <= limit && d < self.dist[node] {
                if self.dist[node].is_infinite() {
                    self.touched.push(node);
                }
                self.dist[node] = d;
                self.heap.push(Item(d, node));
            }
        }
        while let Some(Item(d, u)) = self.heap.pop() {
            if d > self.dist[u] {
                continue;
            }
            for h in g.incident_edges(u) {
                let v = g.head(*h);
                let nd = d + g.length_m(h.edge);
                if nd <= limit && nd < self.dist[v] {
                    if self.dist[v].is_infinite() {
                        self.touched.push(v);
                    }
                    self.dist[v] = nd;
                    self.heap.push(Item(nd, v));
                }
            }
        }
    }

    /// Distance from the source of the last [`Workspace::run`] to a point.
    fn to(&self, g: &PisteGraph, s: &Snap, t: &Snap) -> f64 {
        let e = g.edge(t.edge);
        let mut d =
            (self.dist[e.from] + t.along_m).min(self.dist[e.to] + g.length_m(t.edge) - t.along_m);
        if s.edge == t.edge {
            d = d.min((s.along_m - t.along_m).abs());
        }
        d
    }
}

#[derive(Debug, Clone)]
struct Step {
    fix: Fix,
    states: Vec<MatchState>,
    score: Vec<f64>,
    /// Best previous state; `u32::MAX` at a break (no feasible transition).
    back: Vec<u32>,
}

/// Online HMM map matcher.
#[derive(Debug, Clone)]
pub struct Matcher<'g> {
    graph: &'g PisteGraph,
    params: MatchParams,
    steps: VecDeque<Step>,
    ws: Workspace,
    breaks: usize,
}

impl<'g> Matcher<'g> {
    /// Matcher on a graph.
    pub fn new(graph: &'g PisteGraph, params: MatchParams) -> Self {
        Self {
            graph,
            params,
            steps: VecDeque::new(),
            ws: Workspace::new(graph.nodes().len()),
            breaks: 0,
        }
    }

    /// Parameters.
    pub fn params(&self) -> &MatchParams {
        &self.params
    }

    /// Number of HMM breaks so far (no feasible transition: restart from the emissions).
    pub fn breaks(&self) -> usize {
        self.breaks
    }

    /// Adds a fix; returns the most likely state of this fix given the fixes so far.
    pub fn push(&mut self, fix: Fix) -> MatchState {
        let p = self.params;
        let g = self.graph;
        let mut states: Vec<MatchState> = g
            .candidates(fix.pos, p.radius_m, p.max_candidates)
            .into_iter()
            .map(MatchState::OnTrail)
            .collect();
        states.push(MatchState::OffTrail);
        let emission: Vec<f64> = states
            .iter()
            .map(|s| match s {
                MatchState::OnTrail(c) => p.log_emission(c.dist_m),
                MatchState::OffTrail => p.log_emission(p.off_distance_m),
            })
            .collect();
        let mut score = vec![f64::NEG_INFINITY; states.len()];
        let mut back = vec![u32::MAX; states.len()];
        if let Some(prev) = self.steps.back() {
            let gc = prev.fix.pos.haversine_m(fix.pos);
            let limit = gc + 2.0 * p.radius_m;
            let ln_beta = p.beta_m.ln();
            for (i, ps) in prev.states.iter().enumerate() {
                let base = prev.score[i];
                if !base.is_finite() {
                    continue;
                }
                if let MatchState::OnTrail(src) = ps {
                    self.ws.run(g, src, limit);
                }
                for (j, cs) in states.iter().enumerate() {
                    let t = match (ps, cs) {
                        (MatchState::OnTrail(a), MatchState::OnTrail(b)) => {
                            let dr = self.ws.to(g, a, b);
                            if dr.is_finite() {
                                -(dr - gc).abs() / p.beta_m - ln_beta
                            } else {
                                f64::NEG_INFINITY
                            }
                        }
                        (MatchState::OffTrail, MatchState::OffTrail) => -ln_beta,
                        _ => p.switch_log_p - ln_beta,
                    };
                    let s = base + t;
                    if s > score[j] {
                        score[j] = s;
                        back[j] = i as u32;
                    }
                }
            }
        }
        if score.iter().all(|s| !s.is_finite()) {
            if !self.steps.is_empty() {
                self.breaks += 1;
            }
            score.fill(0.0);
            back.fill(u32::MAX);
        }
        for (s, e) in score.iter_mut().zip(&emission) {
            *s += e;
        }
        let best = score.iter().copied().fold(f64::NEG_INFINITY, f64::max);
        for s in &mut score {
            *s -= best;
        }
        let j = argmax(&score);
        let out = states[j];
        self.steps.push_back(Step {
            fix,
            states,
            score,
            back,
        });
        if self.steps.len() > p.history {
            self.steps.pop_front();
            if let Some(first) = self.steps.front_mut() {
                first.back.fill(u32::MAX);
            }
        }
        out
    }

    /// Viterbi path over the retained fixes (oldest first).
    pub fn decode(&self) -> Vec<MatchState> {
        let n = self.steps.len();
        let mut out = vec![MatchState::OffTrail; n];
        if n == 0 {
            return out;
        }
        let mut j = argmax(&self.steps[n - 1].score);
        for k in (0..n).rev() {
            let st = &self.steps[k];
            out[k] = st.states[j];
            if k > 0 {
                j = match st.back[j] {
                    u32::MAX => argmax(&self.steps[k - 1].score),
                    b => b as usize,
                };
            }
        }
        out
    }
}

fn argmax(v: &[f64]) -> usize {
    let mut best = 0;
    for (i, x) in v.iter().enumerate() {
        if *x > v[best] {
            best = i;
        }
    }
    best
}

/// Distance from points to a polyline, with a segment index.
#[derive(Debug, Clone)]
pub struct PolylineIndex {
    plane: LocalPlane,
    tree: RTree<GeomWithData<Line<[f64; 2]>, usize>>,
}

impl PolylineIndex {
    /// Index of a polyline (at least two points).
    pub fn new(points: &[LonLat]) -> Self {
        let plane = LocalPlane::new(points[points.len() / 2]);
        let segs = points
            .windows(2)
            .enumerate()
            .map(|(i, w)| GeomWithData::new(Line::new(plane.xy(w[0]), plane.xy(w[1])), i))
            .collect();
        Self {
            plane,
            tree: RTree::bulk_load(segs),
        }
    }

    /// Distance from a position to the polyline, metres.
    pub fn distance_m(&self, p: LonLat) -> f64 {
        let q = self.plane.xy(p);
        self.tree
            .nearest_neighbor(&q)
            .map_or(f64::INFINITY, |s| s.geom().distance_2(&q).sqrt())
    }
}

/// Threshold with time hysteresis.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Hysteresis {
    /// Alert when the distance stays above this, metres…
    pub enter_m: f64,
    /// …for this long, seconds.
    pub enter_s: f64,
    /// End the alert when the distance stays below this, metres…
    pub exit_m: f64,
    /// …for this long, seconds.
    pub exit_s: f64,
}

/// Alert transitions.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum AlertEvent {
    /// Deviation confirmed at this time.
    Raised(f64),
    /// Back on the plan at this time.
    Cleared(f64),
}

/// State machine of [`Hysteresis`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DeviationDetector {
    params: Hysteresis,
    alert: bool,
    /// Start of the current run beyond the threshold that would toggle the state.
    since: Option<f64>,
}

impl DeviationDetector {
    /// Detector in the « on plan » state.
    pub fn new(params: Hysteresis) -> Self {
        Self {
            params,
            alert: false,
            since: None,
        }
    }

    /// Whether an alert is active.
    pub fn alerting(&self) -> bool {
        self.alert
    }

    /// Feeds a distance at a time.
    pub fn push(&mut self, t_s: f64, dist_m: f64) -> Option<AlertEvent> {
        let p = self.params;
        let (beyond, hold) = if self.alert {
            (dist_m < p.exit_m, p.exit_s)
        } else {
            (dist_m > p.enter_m, p.enter_s)
        };
        if !beyond {
            self.since = None;
            return None;
        }
        let since = *self.since.get_or_insert(t_s);
        if t_s - since >= hold {
            self.alert = !self.alert;
            self.since = None;
            return Some(if self.alert {
                AlertEvent::Raised(t_s)
            } else {
                AlertEvent::Cleared(t_s)
            });
        }
        None
    }
}

/// Default hysteresis on trails: 40 m for 15 s to alert, 15 m for 5 s to clear (chosen
/// on synthetic traces of both sites, σ 15 m with forest outliers: 0.04 to 0.12 false
/// alert per hour, median delay 30 to 55 s after a wrong turn; see the C2 issue).
pub const ROUTE_HYSTERESIS: Hysteresis = Hysteresis {
    enter_m: 40.0,
    enter_s: 15.0,
    exit_m: 15.0,
    exit_s: 5.0,
};

/// Output of a monitor for one fix.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MonitorOutput {
    /// Matched state (route mode) or `OffTrail` (corridor mode).
    pub state: MatchState,
    /// Distance used by the detector, metres.
    pub dist_m: f64,
    /// Alert transition, if any.
    pub event: Option<AlertEvent>,
}

/// Deviation from a planned route on the network.
#[derive(Debug, Clone)]
pub struct RouteMonitor<'g> {
    matcher: Matcher<'g>,
    route: PolylineIndex,
    detector: DeviationDetector,
}

impl<'g> RouteMonitor<'g> {
    /// Monitor of a planned route (its polyline).
    pub fn new(
        graph: &'g PisteGraph,
        route: &[LonLat],
        params: MatchParams,
        hysteresis: Hysteresis,
    ) -> Self {
        Self {
            matcher: Matcher::new(graph, params),
            route: PolylineIndex::new(route),
            detector: DeviationDetector::new(hysteresis),
        }
    }

    /// The matcher.
    pub fn matcher(&self) -> &Matcher<'g> {
        &self.matcher
    }

    /// Whether an alert is active.
    pub fn alerting(&self) -> bool {
        self.detector.alerting()
    }

    /// Feeds a fix.
    pub fn push(&mut self, fix: Fix) -> MonitorOutput {
        let state = self.matcher.push(fix);
        let at = match state {
            MatchState::OnTrail(s) => s.pos,
            MatchState::OffTrail => fix.pos,
        };
        let dist_m = self.route.distance_m(at);
        MonitorOutput {
            state,
            dist_m,
            event: self.detector.push(fix.t_s, dist_m),
        }
    }
}

/// Default corridor: 50 m half-width for 20 s to alert, 35 m for 5 s to clear.
pub const CORRIDOR_HYSTERESIS: Hysteresis = Hysteresis {
    enter_m: 50.0,
    enter_s: 20.0,
    exit_m: 35.0,
    exit_s: 5.0,
};

/// Deviation from a planned off-piste polyline (corridor mode).
#[derive(Debug, Clone)]
pub struct CorridorMonitor {
    plan: PolylineIndex,
    detector: DeviationDetector,
}

impl CorridorMonitor {
    /// Monitor of a planned polyline.
    pub fn new(plan: &[LonLat], hysteresis: Hysteresis) -> Self {
        Self {
            plan: PolylineIndex::new(plan),
            detector: DeviationDetector::new(hysteresis),
        }
    }

    /// Whether an alert is active.
    pub fn alerting(&self) -> bool {
        self.detector.alerting()
    }

    /// Feeds a fix.
    pub fn push(&mut self, fix: Fix) -> MonitorOutput {
        let dist_m = self.plan.distance_m(fix.pos);
        MonitorOutput {
            state: MatchState::OffTrail,
            dist_m,
            event: self.detector.push(fix.t_s, dist_m),
        }
    }
}

/// Synthetic GPS traces for tests and benchmarks.
pub mod synth {
    use skimap_common::geom::LonLat;

    use super::Fix;
    use crate::graph::LocalPlane;

    /// SplitMix64 generator (deterministic, seeded).
    #[derive(Debug, Clone)]
    pub struct Rng(pub u64);

    impl Rng {
        /// Next 64-bit value.
        pub fn next_u64(&mut self) -> u64 {
            self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
            let mut z = self.0;
            z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
            z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
            z ^ (z >> 31)
        }

        /// Uniform in [0, 1).
        pub fn unit(&mut self) -> f64 {
            (self.next_u64() >> 11) as f64 / (1u64 << 53) as f64
        }

        /// Standard normal (Box–Muller).
        pub fn normal(&mut self) -> f64 {
            let u = self.unit().max(1e-300);
            let v = self.unit();
            (-2.0 * u.ln()).sqrt() * (std::f64::consts::TAU * v).cos()
        }
    }

    /// True position of a synthetic fix.
    #[derive(Debug, Clone, Copy, PartialEq)]
    pub struct Truth {
        /// Time, s.
        pub t_s: f64,
        /// Position.
        pub pos: LonLat,
        /// Distance along the source polyline, m.
        pub along_m: f64,
    }

    /// Positions every `dt_s` seconds along a polyline, at `speed(along_m)` m/s.
    pub fn sample_polyline(
        points: &[LonLat],
        dt_s: f64,
        mut speed: impl FnMut(f64) -> f64,
    ) -> Vec<Truth> {
        let mut cum = vec![0.0];
        for w in points.windows(2) {
            cum.push(cum.last().expect("seeded") + w[0].haversine_m(w[1]));
        }
        let total = *cum.last().expect("two points");
        let at = |d: f64| {
            let i = cum.partition_point(|&x| x <= d).clamp(1, cum.len() - 1);
            let span = cum[i] - cum[i - 1];
            let t = if span > 0.0 {
                (d - cum[i - 1]) / span
            } else {
                0.0
            };
            let (a, b) = (points[i - 1], points[i]);
            LonLat::new(a.lon + (b.lon - a.lon) * t, a.lat + (b.lat - a.lat) * t)
        };
        let (mut out, mut d, mut t) = (Vec::new(), 0.0, 0.0);
        loop {
            out.push(Truth {
                t_s: t,
                pos: at(d),
                along_m: d,
            });
            if d >= total {
                break;
            }
            d = (d + speed(d).max(0.1) * dt_s).min(total);
            t += dt_s;
        }
        out
    }

    /// GPS error model.
    #[derive(Debug, Clone, Copy, PartialEq)]
    pub struct Noise {
        /// Standard deviation per axis (east, north), metres.
        pub sigma_m: f64,
        /// Correlation time of the error (first-order Gauss–Markov), seconds; 0 for
        /// independent errors.
        pub tau_s: f64,
        /// Rate of outlier episodes, per second.
        pub outlier_rate_hz: f64,
        /// Offset during an outlier episode (random direction), metres.
        pub outlier_m: f64,
        /// Duration of an outlier episode, seconds (uniform between the two).
        pub outlier_len_s: (f64, f64),
    }

    /// Noisy fixes from true positions.
    pub fn add_noise(truth: &[Truth], noise: &Noise, rng: &mut Rng) -> Vec<Fix> {
        let plane = LocalPlane::new(truth[truth.len() / 2].pos);
        let (mut ex, mut ey) = (noise.sigma_m * rng.normal(), noise.sigma_m * rng.normal());
        let mut last_t = truth.first().map_or(0.0, |p| p.t_s);
        let mut outlier: Option<(f64, f64, f64)> = None;
        truth
            .iter()
            .map(|p| {
                let dt = p.t_s - last_t;
                last_t = p.t_s;
                let rho = if noise.tau_s > 0.0 {
                    (-dt / noise.tau_s).exp()
                } else {
                    0.0
                };
                let k = (1.0 - rho * rho).sqrt() * noise.sigma_m;
                ex = rho * ex + k * rng.normal();
                ey = rho * ey + k * rng.normal();
                if let Some((end, _, _)) = outlier
                    && p.t_s >= end
                {
                    outlier = None;
                }
                if outlier.is_none() && rng.unit() < noise.outlier_rate_hz * dt.max(0.0) {
                    let (a, b) = noise.outlier_len_s;
                    let dir = std::f64::consts::TAU * rng.unit();
                    outlier = Some((
                        p.t_s + a + (b - a) * rng.unit(),
                        noise.outlier_m * dir.cos(),
                        noise.outlier_m * dir.sin(),
                    ));
                }
                let (ox, oy) = outlier.map_or((0.0, 0.0), |(_, x, y)| (x, y));
                let [x, y] = plane.xy(p.pos);
                Fix {
                    t_s: p.t_s,
                    pos: plane.lonlat([x + ex + ox, y + ey + oy]),
                }
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::synth::{Noise, Rng, add_noise, sample_polyline};
    use super::*;
    use crate::graph::tests::{toy, toy_plane};
    use crate::routing::{Distance, Router, Style, StylePolicy};

    fn route_points(
        g: &PisteGraph,
        a: [f64; 2],
        b: [f64; 2],
    ) -> (crate::routing::Route, Vec<LonLat>) {
        let r = Router::new(
            g,
            Distance {
                style: Style::Classic,
                policy: StylePolicy::default(),
            },
        )
        .route(toy_plane().lonlat(a), toy_plane().lonlat(b), 20.0)
        .unwrap();
        let pts = r.polyline(g);
        (r, pts)
    }

    #[test]
    fn emission_mixture_has_a_heavy_tail() {
        let p = MatchParams::default();
        assert!(p.log_emission(0.0) > p.log_emission(10.0));
        // At 50 m the normal alone (σ = 10 m) gives e^-12.5; the mixture keeps it within
        // a few nats of the 30 m value.
        assert!(p.log_emission(30.0) - p.log_emission(50.0) < 2.0);
    }

    #[test]
    fn matches_a_noisy_trace_on_the_toy_graph() {
        let g = toy();
        // 0 → 3 through node 1: e0 then e2.
        let (_, pts) = route_points(&g, [0.0, 0.0], [1000.0, -500.0]);
        let truth = sample_polyline(&pts, 1.0, |_| 4.0);
        let noise = Noise {
            sigma_m: 10.0,
            tau_s: 0.0,
            outlier_rate_hz: 0.0,
            outlier_m: 0.0,
            outlier_len_s: (0.0, 0.0),
        };
        let fixes = add_noise(&truth, &noise, &mut Rng(7));
        let mut m = Matcher::new(&g, MatchParams::default());
        let online: Vec<MatchState> = fixes.iter().map(|f| m.push(*f)).collect();
        let offline = m.decode();
        let edge_of = |t: &synth::Truth| if t.along_m < 1000.0 { 0 } else { 2 };
        let score = |states: &[MatchState]| {
            states
                .iter()
                .zip(&truth)
                .filter(|(s, t)| matches!(s, MatchState::OnTrail(c) if c.edge == edge_of(t)))
                .count() as f64
                / truth.len() as f64
        };
        let (on, off) = (score(&online), score(&offline));
        println!("toy: online {on:.3}, Viterbi {off:.3}");
        assert!(on > 0.9, "{on}");
        assert!(off >= on - 0.01 && off > 0.97, "{off}");
        assert_eq!(m.breaks(), 0);
    }

    #[test]
    fn leaving_the_network_goes_off_trail() {
        let g = toy();
        // Along e0 for 400 m, then 300 m straight south into the void (no trail).
        let plane = toy_plane();
        let mut fixes = Vec::new();
        for k in 0..100 {
            fixes.push(Fix {
                t_s: k as f64,
                pos: plane.lonlat([4.0 * k as f64, 0.0]),
            });
        }
        for k in 0..75 {
            fixes.push(Fix {
                t_s: 100.0 + k as f64,
                pos: plane.lonlat([400.0, -4.0 * k as f64]),
            });
        }
        let mut m = Matcher::new(&g, MatchParams::default());
        let states: Vec<MatchState> = fixes.iter().map(|f| m.push(*f)).collect();
        assert!(matches!(states[50], MatchState::OnTrail(s) if s.edge == 0));
        // Off by the end (300 m south of e0, 600 m from e2).
        assert!(states[170..].iter().all(|s| *s == MatchState::OffTrail));
    }

    #[test]
    fn hysteresis_needs_the_delay_both_ways() {
        let mut d = DeviationDetector::new(ROUTE_HYSTERESIS);
        // A 10 s spike at 60 m: no alert.
        for t in 0..10 {
            assert_eq!(d.push(t as f64, 60.0), None);
        }
        assert_eq!(d.push(10.0, 5.0), None);
        // 15 s above: alert exactly at the 15th second.
        let mut ev = None;
        for t in 20..=40 {
            if let Some(e) = d.push(t as f64, 45.0) {
                ev = Some(e);
                break;
            }
        }
        assert_eq!(ev, Some(AlertEvent::Raised(35.0)));
        assert!(d.alerting());
        // Between exit and enter: stays alerting.
        assert_eq!(d.push(41.0, 20.0), None);
        for t in 42..47 {
            assert_eq!(d.push(t as f64, 10.0), None);
        }
        assert_eq!(d.push(47.0, 10.0), Some(AlertEvent::Cleared(47.0)));
    }

    #[test]
    fn route_monitor_detects_a_wrong_branch_but_not_noise() {
        let g = toy();
        // Plan: 0 → 2 (e0, e1). Truth A follows it, truth B turns south at node 1.
        let (_, plan) = route_points(&g, [0.0, 0.0], [2000.0, 0.0]);
        let (_, wrong) = route_points(&g, [0.0, 0.0], [1500.0, -500.0]);
        let noise = Noise {
            sigma_m: 10.0,
            tau_s: 20.0,
            outlier_rate_hz: 0.02,
            outlier_m: 30.0,
            outlier_len_s: (2.0, 8.0),
        };
        let mut rng = Rng(11);
        let run = |pts: &[LonLat], rng: &mut Rng| {
            let truth = sample_polyline(pts, 1.0, |_| 4.0);
            let fixes = add_noise(&truth, &noise, rng);
            let mut mon = RouteMonitor::new(&g, &plan, MatchParams::default(), ROUTE_HYSTERESIS);
            let events: Vec<AlertEvent> = fixes.iter().filter_map(|f| mon.push(*f).event).collect();
            (truth, events)
        };
        for _ in 0..5 {
            let (_, ev) = run(&plan, &mut rng);
            assert!(ev.is_empty(), "false alert {ev:?}");
        }
        let (truth, ev) = run(&wrong, &mut rng);
        let Some(AlertEvent::Raised(t)) = ev.first().copied() else {
            panic!("missed deviation: {ev:?}");
        };
        // The branch starts at node 1 (1000 m, t = 250 s): 40 m from the plan 10 s later,
        // plus the 15 s hold, plus the time the matcher needs to leave e0 / e1 under
        // correlated noise.
        let turn = truth.iter().find(|p| p.along_m >= 1000.0).unwrap().t_s;
        assert!(t > turn + 20.0 && t - turn < 50.0, "turn {turn} alert {t}");
    }

    #[test]
    fn corridor_monitor() {
        let plane = toy_plane();
        let plan: Vec<LonLat> = (0..=10)
            .map(|k| plane.lonlat([100.0 * k as f64, 0.0]))
            .collect();
        let mut mon = CorridorMonitor::new(&plan, CORRIDOR_HYSTERESIS);
        let mut first = None;
        for k in 0..200 {
            // 4 m/s east, drifting north 0.5 m/s after 100 s: 50 m reached at t = 200 s.
            let t = k as f64;
            let y = if t > 100.0 { 0.5 * (t - 100.0) } else { 0.0 };
            let out = mon.push(Fix {
                t_s: t,
                pos: plane.lonlat([4.0 * t, y]),
            });
            if first.is_none() {
                first = out.event;
            }
        }
        assert_eq!(first, None);
        for k in 200..260 {
            let t = k as f64;
            let out = mon.push(Fix {
                t_s: t,
                pos: plane.lonlat([4.0 * t.min(250.0), 0.5 * (t - 100.0)]),
            });
            if first.is_none() {
                first = out.event;
            }
        }
        // Beyond 50 m from t = 200 s (boundary) or 201 s, alert 20 s later.
        let Some(AlertEvent::Raised(t)) = first else {
            panic!("{first:?}")
        };
        assert!((220.0..=221.0).contains(&t), "{t}");
    }

    #[test]
    fn polyline_distance() {
        let plane = toy_plane();
        let pl = [plane.lonlat([0.0, 0.0]), plane.lonlat([100.0, 0.0])];
        let idx = PolylineIndex::new(&pl);
        assert!((idx.distance_m(plane.lonlat([50.0, 20.0])) - 20.0).abs() < 0.05);
        assert!((idx.distance_m(plane.lonlat([130.0, 40.0])) - 50.0).abs() < 0.1);
    }
}
