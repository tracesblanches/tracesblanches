//! Route suggestion from available time, level and technique (A5, #92): « 2 h, skating,
//! back before 16 h » → candidate loops ranked, with estimated time, climb, trail status
//! and the reasons of the choice.
//!
//! # Loops
//!
//! On the trail graph of C1 (`routes/piste_graph.json`, one-way trails respected) and with
//! the time model of C5 (`EtaModel`, style, temperature and snow class of the day):
//!
//! 1. the start is snapped to the network (300 m at most);
//! 2. every node is a possible turning point; its round trip is the fastest route from
//!    the start and back; the turning points whose round trip is 50 to 110 % of the
//!    budget are kept, and at most 40 are spread out by farthest-point sampling;
//! 3. the way back from each turning point is the fastest route with the OSM ways of the
//!    way out made 4 times more costly, so that it comes back by another trail when there
//!    is one not much longer; real loops are searched before out-and-backs (X16): when
//!    that way back still travels stretches of the way out again (5 % of the loop or
//!    more), the way back with those ways 30 times more costly is tried as well (a cycle
//!    whenever the network has one); when no simple real loop fits, figures of eight: two
//!    real loops from the start (petals, turning points at 15 to 55 % of the budget, at
//!    most 20) whose stretches are distinct and whose times add up to 60 to 125 % of the
//!    budget, the 20 closest to it;
//! 4. trails harder than the level cost 4 times more (`difficulty`: the easiest colour of
//!    the relations of a way, so a shared section of a green loop counts as green);
//! 5. loops whose model time is outside 75 to 110 % of the budget are dropped, then those
//!    that do not end before the return time and the C7 deadline (light limit minus 30
//!    min) with the C7 margin factor 1.2 on the model time;
//! 6. the real loops (less than a quarter of their length travelled twice: a stem from the
//!    start to the loop is allowed) are proposed, best score first: the simple ones, the
//!    figures of eight (said as such, with the distance at which they pass the start again)
//!    only when no simple loop fits; out-and-backs only when no real loop fits, and then it
//!    is said, with the longest real loop found if any; near-duplicates (more than 60 % of
//!    their edges in common) keep the best.
//!
//! # Score
//!
//! | Factor | Points |
//! |---|---|
//! | fit to the budget | −2 × relative gap |
//! | out-and-back share (length on stretches of the graph travelled twice) | −4 × share |
//! | status (A1 sources, by geometry for the ENJ, by name for Nordic France) | closed share × −10, open share × +1 |
//! | estimated snow (median along the loop, as A1) | as A1 |
//! | avalanche exposure ≥ « exposé » (P16–P18) | −2 with BERA ≥ 3, −0.5 with BERA not available or no BERA, else noted |
//! | weather of the outing hours | as A1 (gusts, cold, fog, snow, rain, sun) |
//! | km harder than the level asked | −2 × share |
//! | named trails | +0.1 each, at most +0.5 |
//!
//! Graph limits are reported, not hidden: network not reachable from the start, turning
//! points without a way back (one-way trails).
//!
//! # Time of a group (C11)
//!
//! [`suggest_for`] with the group of the day (`skimap_core::calibration::ProfileModel`,
//! prior or learned): the time of a loop is the group's median (moving pace per terrain,
//! stops), not the C5 model time. The budget, the fit to it, the planned arrival (× 1.2,
//! C7 margin), the return limits and the spoken summary use that time; the turning points
//! are chosen on the budget converted to model time with the group's flat pace. The model
//! time stays in `eta_min`; the group's range is in [`Suggestion::group`]. Without a group
//! ([`suggest`]) nothing changes.

use std::collections::{BTreeSet, HashMap, HashSet};

use chrono::{Duration, NaiveDate, NaiveDateTime, NaiveTime, Timelike};
use serde::Serialize;
use skimap_common::geom::LonLat;
use skimap_core::calibration::{PaceParams, ProfileModel, TERRAINS, Terrain, terrain_ratio};
use skimap_core::eta::{Conditions, EtaModel, SnowClass};
use skimap_core::graph::{Difficulty, EdgeAttrs, HalfEdge, PisteGraph, Snap};
use skimap_core::matching::PolylineIndex;
use skimap_core::routing::{CostModel, Leg, Piece, Route, Router, Style};
use skimap_core::turnaround::{self, TurnaroundParams};

use crate::briefing::{self, Factor, Reason, SnowStats};
use crate::inputs::SiteData;
use crate::layers::{DayWeather, OpenState, densify};
use crate::snow_quality::{self, Quality};

/// A request: « 2 h, skating, back before 16 h ».
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Request {
    /// Label (« 2 h skating, retour avant 16 h »).
    pub label: String,
    /// Start position.
    pub start: LonLat,
    /// Name of the start.
    pub start_name: String,
    /// Start time, local.
    pub start_time: NaiveDateTime,
    /// Time available, minutes (model time).
    pub budget_min: f64,
    /// Technique.
    pub style: Style,
    /// Back before (local), if any.
    pub return_before: Option<NaiveTime>,
    /// Hardest difficulty wanted, if any.
    pub max_difficulty: Option<Difficulty>,
    /// Number of suggestions.
    pub max_results: usize,
    /// Language of the texts.
    pub lang: crate::text::Lang,
}

/// A suggested loop.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Suggestion {
    /// Label: the three trails with the most length, in order.
    pub label: String,
    /// Trails in order of first appearance.
    pub names: Vec<String>,
    /// Length, km.
    pub length_km: f64,
    /// Climb, m.
    pub gain_m: f64,
    /// Descent, m.
    pub loss_m: f64,
    /// Model time (C5), minutes.
    pub eta_min: f64,
    /// Planned arrival (model time × 1.2), local.
    pub arrival: NaiveDateTime,
    /// Turning point.
    pub turn: LonLat,
    /// Share of the length on stretches of the graph already travelled (out-and-back).
    pub out_and_back_share: f64,
    /// A real loop: less than [`REAL_LOOP_MAX_SHARE`] of its length travelled twice.
    pub real_loop: bool,
    /// Km on open, closed and unknown trails.
    pub status_km: (f64, f64, f64),
    /// Snow along the loop.
    pub snow: Option<SnowStats>,
    /// Km at exposure « exposé » or more, and « très exposé ».
    pub exposure_km: (f64, f64),
    /// Score.
    pub score: f64,
    /// Reasons.
    pub reasons: Vec<Reason>,
    /// Spoken summary (voice bank): « Boucle de treize kilomètres, environ deux heures,
    /// retour vers midi quinze. »
    pub voice: skimap_voice::Event,
    /// Geometry.
    #[serde(skip)]
    pub line: Vec<LonLat>,
    /// Edges used.
    #[serde(skip)]
    pub edges: BTreeSet<usize>,
    /// Legs of the loop in travel order (way out, then way back): the route itself, for a
    /// day plan (`skimap-plan`) that follows it on the graph.
    #[serde(skip)]
    pub legs: Vec<Leg>,
    /// Time model of the loop (C5 with the conditions of the day), the one of `eta_min`
    /// and `profile`.
    #[serde(skip)]
    pub eta: EtaModel,
    /// Time profile along the loop (C5 model): distance, time from the start, time back to
    /// the start the same way reversed (for the turnaround of A8).
    #[serde(skip)]
    pub profile: Profile,
    /// Time of the group of the request (C11), if any.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub group: Option<GroupTime>,
    /// An option « si tout va bien » for a group with children: the loop fits the time
    /// asked at the group's median, not on the slow side it is planned on (shown after the
    /// loops proposed, never first).
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub if_all_goes_well: bool,
}

/// Time of a loop for a group (C11): the median of its prediction and the 80 % range.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct GroupTime {
    /// The group (« famille, enfants de 6 et 9 ans »).
    pub label: String,
    /// Outings learned.
    pub outings: u32,
    /// Median time, minutes.
    pub median_min: f64,
    /// 10 % quantile, minutes.
    pub lo_min: f64,
    /// 90 % quantile, minutes.
    pub hi_min: f64,
    /// Time the loop is planned on, minutes: the median, or the slow side of the range for
    /// a group with children (`ProfileModel::plan_quantile`).
    pub plan_min: f64,
    /// Moving time, minutes (expected).
    pub moving_min: f64,
    /// Stopped time, minutes (expected).
    pub stops_min: f64,
}

/// Cumulative distance and times along a route, at the end of each piece.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Profile {
    /// Distance from the start, m.
    pub dist_m: Vec<f64>,
    /// Model time from the start, s.
    pub out_s: Vec<f64>,
    /// Model time back to the start along the same way reversed, s.
    pub back_s: Vec<f64>,
}

impl Profile {
    /// Profile of pieces with a time model.
    pub fn of(eta: &EtaModel, pieces: &[Piece<'_>]) -> Self {
        let mut p = Profile {
            dist_m: vec![0.0],
            out_s: vec![0.0],
            back_s: vec![0.0],
        };
        for pc in pieces {
            let (d, o, b) = (
                *p.dist_m.last().expect("seeded"),
                *p.out_s.last().expect("seeded"),
                *p.back_s.last().expect("seeded"),
            );
            p.dist_m.push(d + pc.dist_m);
            p.out_s
                .push(o + eta.piece_time_s(pc.attrs, pc.dist_m, pc.dz_m, pc.elev_m));
            p.back_s
                .push(b + eta.piece_time_s(pc.attrs, pc.dist_m, -pc.dz_m, pc.elev_m));
        }
        p
    }

    /// Index of the last station at or before a distance.
    pub fn index_at(&self, along_m: f64) -> usize {
        self.dist_m
            .partition_point(|d| *d <= along_m)
            .saturating_sub(1)
    }

    /// Total model time, s.
    pub fn total_s(&self) -> f64 {
        *self.out_s.last().unwrap_or(&0.0)
    }

    /// Total length, m.
    pub fn length_m(&self) -> f64 {
        *self.dist_m.last().unwrap_or(&0.0)
    }

    /// Model time per terrain class (C11, [`Terrain::of_times`] on each piece with the
    /// bound `ratio` = [`terrain_ratio`] of the model), seconds.
    pub fn model_by_terrain(&self, ratio: f64) -> [f64; TERRAINS] {
        let mut m = [0.0; TERRAINS];
        for i in 1..self.out_s.len() {
            let o = self.out_s[i] - self.out_s[i - 1];
            let b = self.back_s[i] - self.back_s[i - 1];
            m[Terrain::of_times(o, b, ratio) as usize] += o.max(0.0);
        }
        m
    }

    /// The profile at the pace of a group (C11): each piece at the group's median progress
    /// pace on its terrain (out and back), then every time scaled so that the total is the
    /// median of the group's prediction (its stops included). The turnaround of A8 runs
    /// on it unchanged.
    pub fn for_group(&self, params: &PaceParams, ratio: f64) -> Profile {
        self.for_group_at(params, ratio, 0.5)
    }

    /// [`Profile::for_group`] scaled to the quantile `q` of the group's total time instead
    /// of its median (a group with children is planned on the slow side of its range:
    /// `ProfileModel::plan_quantile`).
    pub fn for_group_at(&self, params: &PaceParams, ratio: f64, q: f64) -> Profile {
        let k = |t: Terrain| 1.0 / params.progress_pace(t);
        let mut p = Profile {
            dist_m: self.dist_m.clone(),
            out_s: Vec::with_capacity(self.out_s.len()),
            back_s: Vec::with_capacity(self.back_s.len()),
        };
        let (mut o_acc, mut b_acc) = (0.0, 0.0);
        p.out_s.push(0.0);
        p.back_s.push(0.0);
        for i in 1..self.out_s.len() {
            let o = (self.out_s[i] - self.out_s[i - 1]).max(0.0);
            let b = (self.back_s[i] - self.back_s[i - 1]).max(0.0);
            o_acc += o * k(Terrain::of_times(o, b, ratio));
            b_acc += b * k(Terrain::of_times(b, o, ratio));
            p.out_s.push(o_acc);
            p.back_s.push(b_acc);
        }
        let total = params.predict(self.model_by_terrain(ratio)).quantile_s(q);
        let f = if o_acc > 0.0 { total / o_acc } else { 1.0 };
        for x in p.out_s.iter_mut().chain(p.back_s.iter_mut()) {
            *x *= f;
        }
        p
    }
}

/// Result of a request.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Suggestions {
    /// Request.
    pub request: Request,
    /// The group whose time the loops are chosen on (C11), if any.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub group: Option<String>,
    /// Best first.
    pub suggestions: Vec<Suggestion>,
    /// Deadline of C7 (local).
    pub deadline: Option<NaiveDateTime>,
    /// Loops evaluated.
    pub evaluated: usize,
    /// Notes (graph limits, data).
    pub notes: Vec<String>,
    /// Text, French.
    pub text: Vec<String>,
}

/// Share of its length a loop may travel twice and still be a real loop: a stem from the
/// start to the loop, travelled out and back, of up to an eighth of the length.
pub const REAL_LOOP_MAX_SHARE: f64 = 0.25;

/// Cost factor on the ways of the way out for the way back of the cycle search: another
/// trail is taken whenever the network has one that is not 30 times longer.
const AVOID_CYCLE: f64 = 30.0;

/// Cost factor on the ways of the way out for the way back of the fallback loops: another
/// trail when it is not 4 times longer, else an out-and-back.
const AVOID_LOOP: f64 = 4.0;

/// Points per share of the length travelled twice (X16: a loop that is half an
/// out-and-back loses 2 points, as much as gusts of 60 km/h).
const OUT_AND_BACK_POINTS: f64 = -4.0;

/// Cost of the loop search: C5 time, style preferences, level and ways to avoid.
struct LoopCost<'a> {
    eta: EtaModel,
    max_difficulty: Option<Difficulty>,
    avoid: &'a HashSet<i64>,
    /// Factor on the ways to avoid.
    avoid_factor: f64,
}

/// A loop to evaluate: its routes in travel order (way out and way back, or the two of
/// each petal of a figure of eight) and its turning point.
struct Candidate {
    parts: Vec<Route>,
    turn: usize,
    /// A figure of eight: two loops from the start, back at the start in between.
    eight: bool,
}

/// Share of the length of legs on stretches of the graph already travelled
/// (out-and-back): on each edge, the overlap with the intervals travelled before, so that
/// an OSM way split into several edges and travelled once counts once.
fn twice_share(legs: &[Leg]) -> f64 {
    let mut seen: HashMap<usize, Vec<(f64, f64)>> = HashMap::new();
    let (mut twice, mut total) = (0.0, 0.0);
    for l in legs {
        let (a, b) = (l.from_m.min(l.to_m), l.from_m.max(l.to_m));
        total += b - a;
        let prev = seen.entry(l.edge).or_default();
        // Disjoint intervals: the overlap is the sum of the intersections.
        for (x, y) in prev.iter() {
            twice += (b.min(*y) - a.max(*x)).max(0.0);
        }
        let (mut lo, mut hi) = (a, b);
        prev.retain(|(x, y)| {
            let touch = *x <= hi && *y >= lo;
            if touch {
                lo = lo.min(*x);
                hi = hi.max(*y);
            }
            !touch
        });
        prev.push((lo, hi));
    }
    if total > 0.0 { twice / total } else { 0.0 }
}

/// Near-duplicates: more than 60 % of their edges in common.
fn similar(a: &Suggestion, b: &Suggestion) -> bool {
    let inter = a.edges.intersection(&b.edges).count() as f64;
    let union = a.edges.union(&b.edges).count() as f64;
    union > 0.0 && inter / union > 0.6
}

/// Adds the loops of `from` (best first) that are not near-duplicates of the kept ones,
/// until `max` are kept.
fn keep_distinct(kept: &mut Vec<Suggestion>, from: Vec<Suggestion>, max: usize) {
    for l in from {
        if !kept.iter().any(|k| similar(k, &l)) {
            kept.push(l);
        }
        if kept.len() >= max {
            break;
        }
    }
}

impl CostModel for LoopCost<'_> {
    fn piece_cost(&self, attrs: &EdgeAttrs, dist_m: f64, dz_m: f64, elev_m: Option<f64>) -> f64 {
        self.eta.piece_cost(attrs, dist_m, dz_m, elev_m)
    }

    fn edge_factor(&self, attrs: &EdgeAttrs) -> f64 {
        let mut f = self.eta.edge_factor(attrs);
        if let (Some(max), Some(d)) = (self.max_difficulty, attrs.difficulty)
            && d > max
        {
            f *= 4.0;
        }
        if attrs.osm_way_id.is_some_and(|w| self.avoid.contains(&w)) {
            f *= self.avoid_factor;
        }
        f
    }
}

/// Snap at a node of the graph (on its first incident edge), for routing to or from a
/// node.
pub fn node_snap(g: &PisteGraph, node: usize) -> Option<Snap> {
    let h: HalfEdge = *g.incident_edges(node).first()?;
    let e = g.edge(h.edge);
    let along_m = if e.from == node {
        0.0
    } else {
        g.length_m(h.edge)
    };
    Some(Snap {
        edge: h.edge,
        along_m,
        pos: g.nodes()[node].pos,
        dist_m: 0.0,
    })
}

/// State of every edge from the status sources of the day.
fn edge_states(data: &SiteData, g: &PisteGraph, date: NaiveDate) -> Vec<OpenState> {
    let mut states = vec![OpenState::Unknown; g.edges().len()];
    let Some(st) = data.status.as_ref() else {
        return states;
    };
    if briefing::status_for(data, date) == "none" {
        return states;
    }
    let enj: Vec<(PolylineIndex, OpenState)> = st
        .itineraries
        .iter()
        .filter(|i| i.source == "enj" && i.line.len() >= 2)
        .map(|i| (PolylineIndex::new(&i.line), i.state))
        .collect();
    for (e, s) in states.iter_mut().enumerate() {
        let mid = g.point_at(e, g.length_m(e) / 2.0);
        let mut found: Vec<OpenState> = enj
            .iter()
            .filter(|(idx, _)| idx.distance_m(mid) <= 30.0)
            .map(|(_, st)| *st)
            .collect();
        // Nordic France pistes: the OSM ways joined to them by the status layer.
        if let Some(way) = g.edge(e).attrs.osm_way_id {
            for it in st.itineraries.iter().filter(|i| i.source != "enj") {
                if it.osm_way_ids.contains(&way) {
                    found.push(it.state);
                }
            }
        }
        *s = if found.contains(&OpenState::Open) {
            OpenState::Open
        } else if found.contains(&OpenState::Closed) {
            OpenState::Closed
        } else {
            OpenState::Unknown
        };
    }
    states
}

fn snow_class(q: Option<Quality>) -> SnowClass {
    match q {
        Some(Quality::Fresh | Quality::Windblown) => SnowClass::NewSnow,
        Some(Quality::Transformed | Quality::Wet | Quality::Slush) => SnowClass::Wet,
        Some(Quality::Hard | Quality::Icy) => SnowClass::Icy,
        _ => SnowClass::Groomed,
    }
}

fn route_pieces<'g>(g: &'g PisteGraph, r: &Route) -> Vec<Piece<'g>> {
    r.pieces(g)
}

/// Suggestions for a request on the data of its day, on the C5 model time.
pub fn suggest(data: &SiteData, req: &Request) -> Suggestions {
    suggest_for(data, req, None)
}

/// Suggestions for a request on the data of its day, on the time of `group` when given
/// (C11, see the module), else on the C5 model time ([`suggest`]).
pub fn suggest_for(data: &SiteData, req: &Request, group: Option<&ProfileModel>) -> Suggestions {
    let mut notes = Vec::new();
    let t = crate::text::of(req.lang);
    let group_label = group.map(|g| t.group_label(&g.profile));
    let empty = |notes: Vec<String>| Suggestions {
        request: req.clone(),
        group: group_label.clone(),
        suggestions: Vec::new(),
        deadline: None,
        evaluated: 0,
        text: notes.clone(),
        notes,
    };
    let Some(g) = data.graph.as_ref() else {
        notes.push(t.s_no_graph().into());
        return empty(notes);
    };
    let date = req.start_time.date();
    // Conditions of the day for the time model.
    let wp = data
        .weather
        .as_ref()
        .and_then(|w| w.nearest(req.start))
        .map(|(p, _)| p);
    let temp = wp
        .and_then(|p| {
            p.between(req.start_time, req.start_time + Duration::hours(1))
                .first()
                .and_then(|h| h.temperature_c)
        })
        .unwrap_or(-5.0);
    let quality =
        wp.and_then(|p| snow_quality::day_quality(p, date, &snow_quality::Params::default(), None));
    let flat = quality.as_ref().map(|q| q.flat);
    let mut eta = EtaModel::new(req.style);
    eta.conditions = Conditions {
        snow: snow_class(flat),
        temperature_c: temp,
    };
    let budget_s = req.budget_min * 60.0;
    let ratio = terrain_ratio(&eta);
    let no_avoid = HashSet::new();
    let out_router = Router::new(
        g,
        LoopCost {
            eta,
            max_difficulty: req.max_difficulty,
            avoid: &no_avoid,
            avoid_factor: AVOID_LOOP,
        },
    );
    let Some(start) = g.snap(req.start, 300.0) else {
        notes.push(t.s_no_trail_near(&req.start_name));
        return empty(notes);
    };
    // Outbound routes to every node.
    // Times for the choice of the turning points: the cost of a router on the C5 time
    // with the style factors only (1 on most trails), precomputed per edge; the level
    // factors would inflate it. The exact time is computed for the loops evaluated.
    let time_router = Router::new(g, eta);
    let n = g.nodes().len();
    let mut out: Vec<Option<(Route, f64)>> = vec![None; n];
    for (v, slot) in out.iter_mut().enumerate() {
        if let Some(s) = node_snap(g, v)
            && let Ok(r) = out_router.route_snaps(start, s)
        {
            let t = time_router.route_snaps(start, s).map_or(r.cost, |x| x.cost);
            *slot = Some((r, t));
        }
    }
    let unreachable: Vec<usize> = (0..n).filter(|v| out[*v].is_none()).collect();
    if !unreachable.is_empty() {
        let km: f64 = (0..g.edges().len())
            .filter(|e| {
                let ed = g.edge(*e);
                out[ed.from].is_none() && out[ed.to].is_none()
            })
            .map(|e| g.length_m(e) / 1000.0)
            .sum();
        notes.push(t.s_unreachable(unreachable.len(), n, km, &req.start_name));
    }
    // Round trip of every node: out and fastest way back. With a group: the model time of
    // that round trip at the group's time on the flat (the costs of the routers carry
    // style factors that its pace would multiply).
    let round_s: Vec<Option<f64>> = (0..n)
        .map(|v| {
            let (r_out, t) = out[v].as_ref()?;
            let r_back = time_router.route_snaps(node_snap(g, v)?, start).ok()?;
            Some(match group {
                None => t + r_back.cost,
                Some(gm) => {
                    let m = eta.time_s(&route_pieces(g, r_out)).total_s
                        + eta.time_s(&route_pieces(g, &r_back)).total_s;
                    gm.params.predict([0.0, m, 0.0]).median_s
                }
            })
        })
        .collect();
    // Turning points whose round trip is `lo` to `hi` times the budget, at most `k`, spread
    // out by farthest-point sampling from the farthest one.
    let spread = |lo: f64, hi: f64, k: usize| -> Vec<usize> {
        let window = lo * budget_s..=hi * budget_s;
        let mut cands: Vec<usize> = (0..n)
            .filter(|v| round_s[*v].is_some_and(|x| window.contains(&x)))
            .collect();
        let mut chosen: Vec<usize> = Vec::new();
        if let Some(far) = cands.iter().copied().max_by(|a, b| {
            out[*a]
                .as_ref()
                .unwrap()
                .1
                .total_cmp(&out[*b].as_ref().unwrap().1)
        }) {
            chosen.push(far);
        }
        while chosen.len() < k && !cands.is_empty() {
            let pos = |v: usize| g.nodes()[v].pos;
            let next = cands
                .iter()
                .copied()
                .filter(|v| !chosen.contains(v))
                .max_by(|a, b| {
                    let da = chosen
                        .iter()
                        .map(|c| pos(*c).haversine_m(pos(*a)))
                        .fold(f64::MAX, f64::min);
                    let db = chosen
                        .iter()
                        .map(|c| pos(*c).haversine_m(pos(*b)))
                        .fold(f64::MAX, f64::min);
                    da.total_cmp(&db)
                });
            match next {
                Some(v) => chosen.push(v),
                None => break,
            }
            cands.retain(|v| !chosen.contains(v));
        }
        chosen
    };
    // The loops: turning points whose round trip is 50 to 110 % of the budget (the way
    // back by another trail is longer); the petals of the figures of eight: 15 to 55 %.
    let chosen = spread(0.5, 1.1, 40);
    let petal_points = spread(0.15, 0.55, 20);
    // With children: the group's light margin (C7 plus caution, C11).
    let tp = group.map_or_else(TurnaroundParams::default, |g| g.profile.turnaround_params());
    let deadline = turnaround::deadline(date, req.start, &tp)
        .map(|d| skimap_core::ephemeris::local_time(d.deadline));
    let limit = match (req.return_before.map(|t| date.and_time(t)), deadline) {
        (Some(a), Some(b)) => Some(a.min(b)),
        (a, b) => a.or(b),
    };
    let states = edge_states(data, g, date);
    let bera = data.bera_for(date).and_then(|b| b.level());
    let map = data.snow_for(date);
    let plan_q = group.map_or(0.5, ProfileModel::plan_quantile);
    // Way out to `v` and way back with the ways of the way out made `factor` times more
    // costly; `None` when there is no way back (one-way trails).
    let loop_from = |v: usize, factor: f64| -> Option<Vec<Route>> {
        let (r_out, _) = out[v].clone()?;
        let avoid: HashSet<i64> = r_out
            .legs
            .iter()
            .filter_map(|l| g.edge(l.edge).attrs.osm_way_id)
            .collect();
        let back_router = Router::new(
            g,
            LoopCost {
                eta,
                max_difficulty: req.max_difficulty,
                avoid: &avoid,
                avoid_factor: factor,
            },
        );
        let r_back = back_router.route_snaps(node_snap(g, v)?, start).ok()?;
        Some(vec![r_out, r_back])
    };
    // Time a loop is planned on: the group's (C11: the median, the slow side of its range
    // with children), else the model time.
    let plan_time = |pieces: &[Piece<'_>]| -> f64 {
        match group {
            Some(gm) => gm
                .params
                .predict(Profile::of(&eta, pieces).model_by_terrain(ratio))
                .quantile_s(plan_q),
            None => eta.time_s(pieces).total_s,
        }
    };
    // Candidates, real loops searched before out-and-backs (X16): for each turning point,
    // the way back with the ways of the way out 4 times more costly; when it still comes
    // back on stretches of the way out (5 % of the loop or more), the way back that avoids
    // them as much as the network allows (a cycle when there is one). Figures of eight,
    // made of two real loops from the start (petals), only when no simple real loop fits.
    let mut cands: Vec<Candidate> = Vec::new();
    let mut no_way_back = 0;
    for &v in &chosen {
        let Some(weak) = loop_from(v, AVOID_LOOP) else {
            no_way_back += 1;
            continue;
        };
        let legs: Vec<Leg> = weak.iter().flat_map(|r| r.legs.iter().copied()).collect();
        let strong = if twice_share(&legs) >= 0.05 {
            loop_from(v, AVOID_CYCLE).filter(|s| s[1].legs != weak[1].legs)
        } else {
            None
        };
        cands.push(Candidate {
            parts: weak,
            turn: v,
            eight: false,
        });
        if let Some(parts) = strong {
            cands.push(Candidate {
                parts,
                turn: v,
                eight: false,
            });
        }
    }
    let mut too_late = 0;
    let (mut too_short, mut too_long) = (0, 0);
    // Real loops (less than a quarter of their length twice), simple or figures of eight,
    // and out-and-backs.
    let mut real: Vec<Suggestion> = Vec::new();
    let mut eights: Vec<Suggestion> = Vec::new();
    let mut out_and_back: Vec<Suggestion> = Vec::new();
    // Loops that fit at the median of a group with children but not on its slow side.
    let mut hopeful: Vec<Suggestion> = Vec::new();
    // The longest real loop too short for the budget: its time and length.
    let mut longest_real: Option<(f64, f64)> = None;
    let mut petals_searched = false;
    let mut next = 0;
    loop {
        if next == cands.len() {
            if petals_searched || !real.is_empty() {
                break;
            }
            petals_searched = true;
            // Petals: real loops shorter than the budget (each loop once), combined two
            // by two when their stretches are distinct and their times add up to about
            // the budget (the 20 closest to it).
            let mut petals: Vec<(Vec<Route>, usize, f64)> = Vec::new();
            let mut petal_edges: Vec<BTreeSet<usize>> = Vec::new();
            for &v in &petal_points {
                let Some(parts) = loop_from(v, AVOID_CYCLE) else {
                    continue;
                };
                let legs: Vec<Leg> = parts.iter().flat_map(|r| r.legs.iter().copied()).collect();
                let edges: BTreeSet<usize> = legs.iter().map(|l| l.edge).collect();
                if twice_share(&legs) >= REAL_LOOP_MAX_SHARE || petal_edges.contains(&edges) {
                    continue;
                }
                let pieces: Vec<Piece<'_>> =
                    parts.iter().flat_map(|r| route_pieces(g, r)).collect();
                let t = plan_time(&pieces);
                if t < 0.75 * budget_s && longest_real.is_none_or(|(x, _)| t > x) {
                    longest_real = Some((t, legs.iter().map(Leg::length_m).sum::<f64>() / 1000.0));
                }
                petals.push((parts, v, t));
                petal_edges.push(edges);
            }
            let mut pairs: Vec<(f64, Candidate)> = Vec::new();
            for (i, (a, va, ta)) in petals.iter().enumerate() {
                for (b, vb, tb) in &petals[i + 1..] {
                    let gap = (ta + tb) / budget_s - 1.0;
                    if gap > 0.25 {
                        continue;
                    }
                    // The longer petal first.
                    let (first, second, turn) = if ta >= tb { (a, b, *va) } else { (b, a, *vb) };
                    let parts: Vec<Route> = first.iter().chain(second).cloned().collect();
                    let legs: Vec<Leg> =
                        parts.iter().flat_map(|r| r.legs.iter().copied()).collect();
                    if twice_share(&legs) >= REAL_LOOP_MAX_SHARE {
                        continue;
                    }
                    if gap >= -0.4 {
                        pairs.push((
                            gap.abs(),
                            Candidate {
                                parts,
                                turn,
                                eight: true,
                            },
                        ));
                    } else if longest_real.is_none_or(|(x, _)| ta + tb > x) {
                        longest_real = Some((
                            ta + tb,
                            legs.iter().map(Leg::length_m).sum::<f64>() / 1000.0,
                        ));
                    }
                }
            }
            pairs.sort_by(|a, b| a.0.total_cmp(&b.0));
            cands.extend(pairs.into_iter().take(20).map(|(_, c)| c));
            continue;
        }
        let c = &cands[next];
        next += 1;
        let legs: Vec<Leg> = c
            .parts
            .iter()
            .flat_map(|r| r.legs.iter().copied())
            .collect();
        let pieces: Vec<Piece<'_>> = c.parts.iter().flat_map(|r| route_pieces(g, r)).collect();
        let b = eta.time_s(&pieces);
        let profile = Profile::of(&eta, &pieces);
        // Time of the loop: the group's (C11: the median, the slow side of its range with
        // children), else the model time.
        let gp = group.map(|g| g.params.predict(profile.model_by_terrain(ratio)));
        let time_s = gp.map_or(b.total_s, |p| p.quantile_s(plan_q));
        // Out-and-back share: length on stretches travelled twice.
        let share_twice = twice_share(&legs);
        let real_loop = share_twice < REAL_LOOP_MAX_SHARE;
        let total: f64 = legs.iter().map(Leg::length_m).sum();
        if time_s < 0.75 * budget_s {
            too_short += 1;
            if real_loop && longest_real.is_none_or(|(t, _)| time_s > t) {
                longest_real = Some((time_s, total / 1000.0));
            }
            continue;
        }
        // Too long on the slow side, in the time at the median: « si tout va bien ».
        let if_all_goes_well = time_s > 1.1 * budget_s
            && gp.is_some_and(|p| (0.75 * budget_s..=1.1 * budget_s).contains(&p.median_s));
        if time_s > 1.1 * budget_s && !if_all_goes_well {
            too_long += 1;
            continue;
        }
        // Back: with children, the slow side of the range (its own margin); else the time
        // with the margin of C7.
        let planned = if plan_q > 0.5 {
            time_s
        } else {
            time_s * TurnaroundParams::default().eta_factor
        };
        let arrival = req.start_time + Duration::seconds(planned as i64);
        if limit.is_some_and(|l| arrival > l) {
            too_late += 1;
            continue;
        }
        let mut line: Vec<LonLat> = Vec::new();
        for r in &c.parts {
            line.extend(r.polyline(g));
        }
        let (mut open_km, mut closed_km, mut unknown_km) = (0.0, 0.0, 0.0);
        for l in &legs {
            let len = l.length_m();
            match states[l.edge] {
                OpenState::Open => open_km += len / 1000.0,
                OpenState::Closed => closed_km += len / 1000.0,
                OpenState::Unknown => unknown_km += len / 1000.0,
            }
        }
        // Trail names in order of first appearance (each part of « A / B »), with the
        // length on each; the label takes the three longest.
        let mut names: Vec<String> = Vec::new();
        let mut name_len: Vec<f64> = Vec::new();
        for l in &legs {
            if let Some(n) = &g.edge(l.edge).attrs.name {
                for part in n.split(" / ") {
                    match names.iter().position(|x| x == part) {
                        Some(i) => name_len[i] += l.length_m(),
                        None => {
                            names.push(part.to_owned());
                            name_len.push(l.length_m());
                        }
                    }
                }
            }
        }
        let mut top: Vec<usize> = (0..names.len()).collect();
        top.sort_by(|a, b| name_len[*b].total_cmp(&name_len[*a]));
        top.truncate(3);
        top.sort();
        let label = if top.is_empty() {
            t.s_unnamed_loop().to_owned()
        } else {
            top.iter()
                .map(|i| names[*i].clone())
                .collect::<Vec<_>>()
                .join(" – ")
        };
        let added = match (map, wp) {
            (Some(m), Some(p)) => {
                p.snowfall_between(m.valid_at.naive_utc() + Duration::hours(1), req.start_time)
            }
            _ => 0.0,
        };
        let snow = map.and_then(|m| briefing::snow_stats(m, &line, added));
        let mut exp = (0.0, 0.0);
        for (p, len) in densify(&line, 10.0) {
            match data.exposure_at(p) {
                Some(3) => {
                    exp.0 += len / 1000.0;
                    exp.1 += len / 1000.0;
                }
                Some(2) => exp.0 += len / 1000.0,
                _ => {}
            }
        }
        let weather: Option<DayWeather> = wp.map(|p| p.summary_between(req.start_time, arrival));
        let mut reasons = Vec::new();
        let gap = time_s / budget_s - 1.0;
        let style = t.style(req.style);
        let time_text = match (group, gp) {
            (Some(g), Some(p)) if plan_q > 0.5 => t.s_time_group_slow(
                &t.hm(p.median_s),
                &t.group_label(&g.profile),
                &t.hm(p.lo_s),
                &t.hm(p.hi_s),
                &t.learned(g.outings),
                &t.hm(time_s),
                style,
                &t.hm(b.total_s),
            ),
            (Some(g), Some(p)) => t.s_time_group(
                &t.hm(p.median_s),
                &t.group_label(&g.profile),
                &t.hm(p.lo_s),
                &t.hm(p.hi_s),
                &t.learned(g.outings),
                style,
                &t.hm(b.total_s),
            ),
            _ => t.s_time_model(&t.hm(b.total_s), style),
        };
        briefing::reason(
            &mut reasons,
            Factor::Data,
            -2.0 * gap.abs(),
            t.s_loop(total / 1000.0, b.gain_m.round(), &time_text, arrival),
        );
        if share_twice >= 0.05 {
            briefing::reason(
                &mut reasons,
                Factor::Data,
                OUT_AND_BACK_POINTS * share_twice,
                t.s_out_and_back((share_twice * 100.0).round()),
            );
        }
        if c.eight {
            let first: f64 = c.parts[..2].iter().map(Route::length_m).sum();
            briefing::reason(&mut reasons, Factor::Data, 0.0, t.s_eight(first / 1000.0));
        }
        let len_km = total / 1000.0;
        if closed_km > 0.05 {
            briefing::reason(
                &mut reasons,
                Factor::Status,
                -10.0 * closed_km / len_km,
                t.s_closed_km(closed_km),
            );
        }
        if open_km > 0.05 {
            briefing::reason(
                &mut reasons,
                Factor::Status,
                open_km / len_km,
                t.s_open_km(open_km),
            );
        }
        if unknown_km > 0.05 && closed_km + open_km > 0.0 {
            briefing::reason(
                &mut reasons,
                Factor::Status,
                0.0,
                t.s_unknown_km(unknown_km),
            );
        }
        if closed_km + open_km == 0.0 {
            briefing::reason(
                &mut reasons,
                Factor::Status,
                0.0,
                t.s_status_unknown().into(),
            );
        }
        briefing::snow_reasons(&mut reasons, &snow, None, t);
        if exp.0 >= 0.05 {
            let (p, why) = match bera {
                Some(l) if l >= 3 => (-2.0, t.s_bera(l)),
                Some(l) => (0.0, t.s_bera(l)),
                None if data.no_bera() => (-0.5, t.s_no_bera_range().into()),
                None => (-0.5, t.s_bera_unavailable().into()),
            };
            briefing::reason(&mut reasons, Factor::Avalanche, p, t.s_exposed(exp.0, &why));
        }
        if let Some(w) = &weather
            && w.hours > 0
        {
            briefing::weather_reasons(&mut reasons, w, t);
        }
        if let Some(max) = req.max_difficulty {
            let hard: f64 = legs
                .iter()
                .filter(|l| g.edge(l.edge).attrs.difficulty.is_some_and(|d| d > max))
                .map(|l| l.length_m() / 1000.0)
                .sum();
            if hard >= 0.05 {
                briefing::reason(
                    &mut reasons,
                    Factor::Data,
                    -2.0 * hard / len_km,
                    t.s_harder(hard),
                );
            }
        }
        let named = names.len().min(5) as f64 * 0.1;
        if named > 0.0 {
            briefing::reason(&mut reasons, Factor::Data, named, t.s_trails(&names));
        }
        if if_all_goes_well {
            briefing::reason(
                &mut reasons,
                Factor::Data,
                0.0,
                t.s_if_all_goes_well(
                    &gp.map(|p| t.hm(p.median_s)).unwrap_or_default(),
                    &t.hm(time_s),
                ),
            );
        }
        let score = reasons.iter().map(|r| r.points).sum();
        let voice = loop_voice(len_km, time_s, arrival);
        let group_time = match (group, gp) {
            (Some(g), Some(p)) => Some(GroupTime {
                label: t.group_label(&g.profile),
                outings: g.outings,
                median_min: (p.median_s / 60.0).round(),
                lo_min: (p.lo_s / 60.0).round(),
                hi_min: (p.hi_s / 60.0).round(),
                plan_min: (time_s / 60.0).round(),
                moving_min: (p.moving_s / 60.0).round(),
                stops_min: (p.stop_s / 60.0).round(),
            }),
            _ => None,
        };
        let target = if if_all_goes_well {
            &mut hopeful
        } else if real_loop && c.eight {
            &mut eights
        } else if real_loop {
            &mut real
        } else {
            &mut out_and_back
        };
        target.push(Suggestion {
            label,
            names,
            length_km: (len_km * 100.0).round() / 100.0,
            gain_m: b.gain_m.round(),
            loss_m: b.loss_m.round(),
            eta_min: (b.total_s / 60.0).round(),
            arrival,
            turn: g.nodes()[c.turn].pos,
            out_and_back_share: (share_twice * 100.0).round() / 100.0,
            real_loop,
            status_km: (open_km, closed_km, unknown_km),
            snow,
            exposure_km: exp,
            score,
            reasons,
            voice,
            line,
            edges: legs.iter().map(|l| l.edge).collect(),
            profile,
            legs,
            eta,
            group: group_time,
            if_all_goes_well,
        });
    }
    let evaluated = cands.len();
    if no_way_back > 0 {
        notes.push(t.s_no_way_back(no_way_back));
    }
    if too_short + too_long > 0 {
        notes.push(t.s_evaluated(evaluated, too_short, too_long, group_label.as_deref()));
    }
    if too_late > 0 {
        notes.push(t.s_too_late(too_late, limit));
    }
    // Simple real loops, best score first; figures of eight only when no simple loop fits,
    // out-and-backs only when no real loop does.
    let by_score = |a: &Suggestion, b: &Suggestion| b.score.total_cmp(&a.score);
    let mut kept: Vec<Suggestion> = Vec::new();
    for mut tier in [real, eights, out_and_back] {
        if kept.is_empty() {
            tier.sort_by(by_score);
            keep_distinct(&mut kept, tier, req.max_results);
        }
    }
    // With children, after the loops proposed: the best loop that needs all to go well,
    // if any (a real loop first).
    hopeful.sort_by(|a, b| b.real_loop.cmp(&a.real_loop).then(by_score(a, b)));
    if let Some(h) = hopeful
        .into_iter()
        .find(|l| !kept.iter().any(|k| similar(k, l)))
    {
        kept.push(h);
    }
    if kept.is_empty() {
        notes.push(t.s_none_fits().into());
    } else {
        if kept.iter().all(|k| !k.real_loop) {
            let longest = longest_real.map(|(time, km)| (km, t.hm(time)));
            notes.push(t.s_no_real_loop(
                &t.hm(0.75 * budget_s),
                &t.hm(1.1 * budget_s),
                &req.start_name,
                kept.len() == 1,
                longest.as_ref().map(|(km, h)| (*km, h.as_str())),
            ));
        }
        if kept
            .iter()
            .all(|k| k.snow.is_some_and(|s| s.depth_cm() < 20.0))
        {
            notes.push(t.s_no_snow_all().into());
        }
        if kept.iter().all(|k| k.status_km.1 > 0.5 * k.length_km) {
            notes.push(t.s_closed_most().into());
        }
    }
    let mut text = vec![t.s_title(
        &req.label,
        &req.start_name,
        date,
        req.start_time,
        deadline,
        group_label.as_deref(),
    )];
    for (i, s) in kept.iter().enumerate() {
        let why: Vec<String> = s
            .reasons
            .iter()
            .map(|r| {
                if r.points == 0.0 {
                    r.text.clone()
                } else {
                    t.reason_points(&r.text, r.points)
                }
            })
            .collect();
        text.push(t.s_line(i + 1, s.if_all_goes_well, &s.label, s.score, &why));
    }
    if !notes.is_empty() {
        text.push(t.s_limits(&notes));
    }
    Suggestions {
        request: req.clone(),
        group: group_label,
        suggestions: kept,
        deadline,
        evaluated,
        notes,
        text,
    }
}

/// « 1 h 50 ».
pub fn hm(seconds: f64) -> String {
    crate::text::of(crate::text::Lang::Fr).hm(seconds)
}

/// GeoJSON features of the suggestions (for the viewer).
pub fn features(s: &Suggestions) -> Vec<serde_json::Value> {
    s.suggestions
        .iter()
        .enumerate()
        .map(|(i, x)| {
            skimap_common::output::feature(
                skimap_common::output::line_string(x.line.iter().copied()),
                serde_json::json!({
                    "rank": i + 1,
                    "request": s.request.label,
                    "label": x.label,
                    "names": x.names,
                    "length_km": x.length_km,
                    "gain_m": x.gain_m,
                    "eta_min": x.eta_min,
                    "arrival": x.arrival.format("%H:%M").to_string(),
                    "out_and_back_share": x.out_and_back_share,
                    "score": (x.score * 100.0).round() / 100.0,
                    "reasons": x.reasons.iter().map(|r| r.text.clone()).collect::<Vec<_>>(),
                }),
            )
        })
        .collect()
}

/// Spoken summary of a loop: length, model time, planned return.
pub fn loop_voice(length_km: f64, model_s: f64, arrival: NaiveDateTime) -> skimap_voice::Event {
    skimap_voice::Event::LoopSummary {
        km: length_km,
        seconds: model_s,
        back: skimap_voice::quantity::Clock {
            hour: arrival.hour() as u8,
            minute: arrival.minute() as u8,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn loop_summary_is_spoken() {
        let at = NaiveDate::from_ymd_opt(2026, 1, 5)
            .unwrap()
            .and_hms_opt(12, 14, 0)
            .unwrap();
        assert_eq!(
            loop_voice(12.8, 118.0 * 60.0, at).announce().text(),
            "Boucle de treize kilomètres, environ deux heures, retour vers midi quinze."
        );
    }
    /// Evaluation site of `data/sites.json`, test data.
    static CHAPELLE_DES_BOIS: std::sync::LazyLock<skimap_common::site::Site> =
        std::sync::LazyLock::new(|| {
            *skimap_common::site::site("chapelle-des-bois").expect("evaluation site")
        });
    use skimap_core::graph::{Edge, EdgeKind, GraphData, Node};

    fn attrs(name: &str, way: i64, difficulty: Difficulty) -> EdgeAttrs {
        EdgeAttrs {
            kind: EdgeKind::Piste,
            name: Some(name.into()),
            osm_way_id: Some(way),
            classic: Some(true),
            skating: Some(true),
            backcountry: false,
            nordic: true,
            hike: false,
            difficulty: Some(difficulty),
            difficulty_max: Some(difficulty),
        }
    }

    /// A square loop A-B-C-D (≈ 1.7 × 2.2 km sides) and a spur A-E, flat.
    fn data() -> SiteData {
        site(
            &[
                (6.10, 46.60),
                (6.12, 46.60),
                (6.12, 46.62),
                (6.10, 46.62),
                (6.08, 46.60),
            ],
            &[
                (0, 1, "Sud", 1, Difficulty::Easy),
                (1, 2, "Est", 2, Difficulty::Easy),
                (2, 3, "Nord", 3, Difficulty::Easy),
                (3, 0, "Ouest", 4, Difficulty::Advanced),
                (0, 4, "Antenne", 5, Difficulty::Easy),
            ],
        )
    }

    /// A flat graph: nodes (lon, lat) and straight edges (from, to, name, OSM way,
    /// difficulty).
    fn site(pts: &[(f64, f64)], edges: &[(usize, usize, &str, i64, Difficulty)]) -> SiteData {
        let pts: Vec<LonLat> = pts.iter().map(|(x, y)| LonLat::new(*x, *y)).collect();
        let nodes = pts
            .iter()
            .map(|q| Node {
                pos: *q,
                osm_node_id: None,
            })
            .collect();
        let g = PisteGraph::new(GraphData {
            site: "test".into(),
            nodes,
            edges: edges
                .iter()
                .map(|(a, b, name, way, d)| Edge {
                    from: *a,
                    to: *b,
                    oneway: false,
                    points: vec![pts[*a], pts[*b]],
                    attrs: attrs(name, *way, *d),
                    profile: None,
                })
                .collect(),
            elevation_source: None,
        })
        .unwrap();
        SiteData {
            site: *CHAPELLE_DES_BOIS,
            weather: None,
            snows: vec![],
            beras: vec![],
            status: None,
            domains: vec![],
            pistes: vec![],
            clpa: None,
            no_network: None,
            zones: None,
            route: None,
            graph: Some(g),
            exposure: None,
            missing: vec![],
            sources: vec![],
            files: vec![],
        }
    }

    fn req(min: f64, max: Option<Difficulty>) -> Request {
        Request {
            label: "test".into(),
            start: LonLat::new(6.10, 46.60),
            start_name: "A".into(),
            start_time: NaiveDate::from_ymd_opt(2027, 1, 12)
                .unwrap()
                .and_hms_opt(10, 0, 0)
                .unwrap(),
            budget_min: min,
            style: Style::Classic,
            return_before: NaiveTime::from_hms_opt(16, 0, 0),
            max_difficulty: max,
            max_results: 3,
            lang: crate::text::Lang::Fr,
        }
    }

    #[test]
    fn finds_the_loop_and_respects_level_and_time() {
        let d = data();
        let g = d.graph.as_ref().unwrap();
        let perimeter: f64 = (0..4).map(|e| g.length_m(e)).sum::<f64>() / 1000.0;
        // Model time of the loop at −5 °C on groomed snow, flat: about 10.6 km/h.
        let s = suggest(&d, &req(perimeter / 10.6 * 60.0, None));
        let best = s.suggestions.first().expect("a loop");
        assert!((best.length_km - perimeter).abs() < 0.2, "{best:?}");
        assert!(best.out_and_back_share < 0.05, "a real loop: {best:?}");
        assert_eq!(best.names.len(), 4);
        // Green and blue only: the black « Ouest » side makes the loop pay, said.
        let s2 = suggest(&d, &req(perimeter / 10.6 * 60.0, Some(Difficulty::Easy)));
        for b in &s2.suggestions {
            let on_black = b.names.iter().any(|n| n == "Ouest");
            let said = b
                .reasons
                .iter()
                .any(|r| r.text.contains("plus difficiles que demandé"));
            assert_eq!(on_black, said, "{b:?}");
        }
        // Ten minutes: nothing fits, said.
        let s3 = suggest(&d, &req(10.0, None));
        assert!(s3.suggestions.is_empty());
        assert!(
            s3.notes.iter().any(|n| n.contains("aucune boucle")),
            "{:?}",
            s3.notes
        );
        // The profile ends at the loop's model time.
        assert!((best.profile.total_s() / 60.0 - best.eta_min).abs() < 1.0);
        assert!((best.profile.length_m() / 1000.0 - best.length_km).abs() < 0.01);
    }

    #[test]
    fn loops_on_the_time_of_a_group() {
        use skimap_core::calibration::{GroupProfile, ProfileModel};
        let d = data();
        let g = d.graph.as_ref().unwrap();
        let perimeter: f64 = (0..4).map(|e| g.length_m(e)).sum::<f64>() / 1000.0;
        let budget = perimeter / 10.6 * 60.0;
        // Without a group: the same as before C11.
        assert_eq!(
            suggest_for(&d, &req(budget, None), None),
            suggest(&d, &req(budget, None))
        );
        let adult = suggest(&d, &req(budget, None));
        assert!(adult.group.is_none());
        assert!(adult.suggestions.iter().all(|s| s.group.is_none()));
        // A family (6 and 9 years) with the adult's budget: the whole loop is far too long
        // for it; whatever is proposed fits its time, not the model's. Planned on its median
        // here (`plan_q` 0.5); on the slow side by default (below).
        let mut family = ProfileModel::new(GroupProfile::family(2, &[6, 9], Style::Classic));
        family.plan_q = Some(0.5);
        let f = suggest_for(&d, &req(budget, None), Some(&family));
        assert_eq!(f.group.as_deref(), Some(family.profile.label_fr().as_str()));
        for s in &f.suggestions {
            let gt = s.group.as_ref().expect("group time");
            assert!(
                (0.75 * budget..=1.1 * budget).contains(&gt.median_min),
                "{gt:?}"
            );
            assert!(s.eta_min < 0.6 * budget, "{} min model", s.eta_min);
            assert!(s.length_km < 0.6 * perimeter, "{}", s.length_km);
            assert!(gt.lo_min < gt.median_min && gt.median_min < gt.hi_min);
            assert!(
                s.reasons[0].text.contains(&format!("pour {}", gt.label)),
                "{}",
                s.reasons[0].text
            );
        }
        assert!(f.text[0].contains("temps du groupe"), "{}", f.text[0]);
        // Given the family's time of the whole loop, it gets the loop, said with its time.
        let fam_s = family
            .params
            .predict(
                adult.suggestions[0]
                    .profile
                    .model_by_terrain(terrain_ratio(&adult.suggestions[0].eta)),
            )
            .median_s;
        let f2 = suggest_for(&d, &req(fam_s / 60.0, None), Some(&family));
        let best = f2.suggestions.first().expect("the loop for the family");
        assert!((best.length_km - perimeter).abs() < 0.2, "{best:?}");
        let gt = best.group.as_ref().unwrap();
        assert!((gt.median_min - fam_s / 60.0).abs() <= 1.0, "{gt:?}");
        assert!(gt.stops_min > 0.0 && gt.moving_min > best.eta_min);
        // Planned arrival on the group's time with the C7 margin; spoken the same way.
        let planned = best.arrival - req(0.0, None).start_time;
        assert!(
            (planned.num_seconds() as f64 - fam_s * 1.2).abs() < 2.0,
            "{planned}"
        );
        let skimap_voice::Event::LoopSummary { seconds, .. } = best.voice else {
            panic!("{:?}", best.voice)
        };
        assert!((seconds - fam_s).abs() < 1.0);
        // The A8 profile at the family's pace ends at the same median.
        let fp = best
            .profile
            .for_group(&family.params, terrain_ratio(&best.eta));
        assert!((fp.total_s() - fam_s).abs() < 1.0);
        // By default, with children: planned on the slow side (80 %). The loop that fits
        // the time asked at the median only is an option « si tout va bien », last.
        family.plan_q = None;
        let model_s = best.profile.model_by_terrain(terrain_ratio(&best.eta));
        let pr = family.params.predict(model_s);
        let slow = family.plan_s(&pr);
        assert!(slow > pr.median_s && slow < pr.hi_s);
        let f3 = suggest_for(&d, &req(fam_s / 60.0, None), Some(&family));
        let h = f3.suggestions.last().expect("the option");
        assert!(h.if_all_goes_well, "{h:?}");
        assert!((h.length_km - perimeter).abs() < 0.2);
        assert!(f3.suggestions.iter().filter(|s| s.if_all_goes_well).count() == 1);
        assert!(f3.text.iter().any(|t| t.contains("si tout va bien")));
        // With the slow side as the time asked: the loop, planned and said on it, back by
        // then (its own margin, not × 1.2), the A8 profile at that time.
        let f4 = suggest_for(&d, &req(slow / 60.0, None), Some(&family));
        let b4 = f4.suggestions.first().expect("the loop on the slow side");
        assert!(!b4.if_all_goes_well);
        assert!((b4.length_km - perimeter).abs() < 0.2);
        let gt = b4.group.as_ref().unwrap();
        assert!((gt.plan_min - slow / 60.0).abs() <= 1.0, "{gt:?}");
        assert!((gt.median_min - fam_s / 60.0).abs() <= 1.0, "{gt:?}");
        let planned = b4.arrival - req(0.0, None).start_time;
        assert!(
            (planned.num_seconds() as f64 - slow).abs() < 2.0,
            "{planned}"
        );
        assert!(
            b4.reasons[0].text.contains("le côté lent"),
            "{}",
            b4.reasons[0].text
        );
        let skimap_voice::Event::LoopSummary { seconds, .. } = b4.voice else {
            panic!("{:?}", b4.voice)
        };
        assert!((seconds - slow).abs() < 1.0);
        let fq = b4.profile.for_group_at(
            &family.params,
            terrain_ratio(&b4.eta),
            family.plan_quantile(),
        );
        assert!((fq.total_s() - slow).abs() < 1.0);
    }

    fn leg(edge: usize, from_m: f64, to_m: f64) -> Leg {
        Leg { edge, from_m, to_m }
    }

    #[test]
    fn travelled_twice_by_stretch_not_by_way() {
        // An OSM way split into two edges, travelled once: nothing twice.
        assert_eq!(twice_share(&[leg(0, 0.0, 100.0), leg(1, 0.0, 300.0)]), 0.0);
        // Out and back on the same edge: half of the length.
        assert_eq!(twice_share(&[leg(0, 0.0, 100.0), leg(0, 100.0, 0.0)]), 0.5);
        // Overlapping parts of an edge: 50 m of 200.
        assert_eq!(
            twice_share(&[leg(0, 0.0, 100.0), leg(0, 50.0, 150.0)]),
            0.25
        );
        // Three passes over intervals merged on the way: 0–100, 200–300, then 50–250
        // overlaps 50 + 50 m; then 0–300 overlaps all of it.
        let three = [
            leg(3, 0.0, 100.0),
            leg(3, 300.0, 200.0),
            leg(3, 50.0, 250.0),
        ];
        assert!((twice_share(&three) - 100.0 / 400.0).abs() < 1e-12);
        let four = [three.as_slice(), &[leg(3, 0.0, 300.0)]].concat();
        assert!((twice_share(&four) - 400.0 / 700.0).abs() < 1e-12);
        assert_eq!(twice_share(&[]), 0.0);
    }

    /// Time of `km` at the model pace on flat groomed snow at −5 °C, minutes.
    fn minutes(km: f64) -> f64 {
        let eta = EtaModel::new(Style::Classic);
        eta.piece_time_s(&attrs("x", 0, Difficulty::Easy), 1000.0 * km, 0.0, None) / 60.0
    }

    #[test]
    fn real_loops_before_out_and_backs() {
        // The square (A-B-C-D, 7.5 km) and a long spur A-F of 3.3 km.
        let d = site(
            &[
                (6.10, 46.60),
                (6.12, 46.60),
                (6.12, 46.62),
                (6.10, 46.62),
                (6.10, 46.57),
            ],
            &[
                (0, 1, "Sud", 1, Difficulty::Easy),
                (1, 2, "Est", 2, Difficulty::Easy),
                (2, 3, "Nord", 3, Difficulty::Easy),
                (3, 0, "Ouest", 4, Difficulty::Easy),
                (0, 4, "Antenne", 5, Difficulty::Easy),
            ],
        );
        let g = d.graph.as_ref().unwrap();
        let square: f64 = (0..4).map(|e| g.length_m(e)).sum::<f64>() / 1000.0;
        let spur = g.length_m(4) / 1000.0;
        // 2 × the spur fits the time asked better than the square (at 109 %): the square
        // is proposed all the same, the out-and-back is not.
        let s = suggest(&d, &req(minutes(square) / 1.09, None));
        assert!((2.0 * spur / square - 1.0 / 1.09).abs() < 0.1);
        assert!(!s.suggestions.is_empty(), "{:?}", s.text);
        for x in &s.suggestions {
            assert!(x.real_loop && x.out_and_back_share < 0.05, "{x:?}");
            assert!((x.length_km - square).abs() < 0.1);
        }
        assert!(!s.notes.iter().any(|n| n.contains("vraie boucle")));
        // The time of the out-and-back only: it is the one proposed, said as the only
        // option of the network; the square, longer, is not a loop « too short ».
        let s = suggest(&d, &req(minutes(2.0 * spur), None));
        let x = s.suggestions.first().expect("the out-and-back");
        assert!(!x.real_loop);
        assert!((x.out_and_back_share - 0.5).abs() < 0.01, "{x:?}");
        let r = x
            .reasons
            .iter()
            .find(|r| r.text.contains("aller-retour"))
            .unwrap();
        assert!((r.points - OUT_AND_BACK_POINTS * 0.5).abs() < 0.01, "{r:?}");
        assert!(r.text.starts_with("50 % du parcours"), "{}", r.text);
        let note = s
            .notes
            .iter()
            .find(|n| n.contains("vraie boucle"))
            .expect("said");
        assert!(
            note.starts_with("aucune vraie boucle trouvée de ")
                && note.contains(" depuis A : la boucle proposée est en partie en aller-retour")
                && !note.contains("la plus longue"),
            "{note}"
        );
        assert!(s.text.last().unwrap().contains(note.as_str()));
        // Score: the sum of the points of the reasons.
        let sum: f64 = x.reasons.iter().map(|r| r.points).sum();
        assert!((x.score - sum).abs() < 1e-12);
    }

    #[test]
    fn a_figure_of_eight_when_no_simple_loop_fits() {
        // Two triangles of about 4.2 km on both sides of the start A: no simple loop is
        // as long as both.
        let d = site(
            &[
                (6.10, 46.60),
                (6.12, 46.60),
                (6.11, 46.61),
                (6.08, 46.60),
                (6.09, 46.59),
            ],
            &[
                (0, 1, "Est 1", 1, Difficulty::Easy),
                (1, 2, "Est 2", 2, Difficulty::Easy),
                (2, 0, "Est 3", 3, Difficulty::Easy),
                (0, 3, "Ouest 1", 4, Difficulty::Easy),
                (3, 4, "Ouest 2", 5, Difficulty::Easy),
                (4, 0, "Ouest 3", 6, Difficulty::Easy),
            ],
        );
        let g = d.graph.as_ref().unwrap();
        let east: f64 = (0..3).map(|e| g.length_m(e)).sum::<f64>() / 1000.0;
        let west: f64 = (3..6).map(|e| g.length_m(e)).sum::<f64>() / 1000.0;
        let s = suggest(&d, &req(minutes(east + west), None));
        assert_eq!(s.suggestions.len(), 1, "{:?}", s.text);
        let x = &s.suggestions[0];
        assert!(x.real_loop && x.out_and_back_share == 0.0, "{x:?}");
        assert!((x.length_km - (east + west)).abs() < 0.05, "{x:?}");
        assert_eq!(x.names.len(), 6);
        let eight = x
            .reasons
            .iter()
            .find(|r| r.text.starts_with("en huit"))
            .expect("said as a figure of eight");
        assert_eq!(eight.points, 0.0);
        let first = if east >= west { east } else { west };
        assert!(
            eight.text.ends_with(&format!(
                "on y repasse après {} km",
                crate::text::of(crate::text::Lang::Fr).num(first, 1)
            )),
            "{}",
            eight.text
        );
        // The profile covers both petals.
        assert!((x.profile.length_m() / 1000.0 - x.length_km).abs() < 0.01);
        // The time of one triangle: a simple loop, no figure of eight.
        let s = suggest(&d, &req(minutes(east), None));
        assert!(!s.suggestions.is_empty(), "{:?}", s.text);
        for x in &s.suggestions {
            assert!(!x.reasons.iter().any(|r| r.text.starts_with("en huit")));
            assert!(x.length_km < 0.6 * (east + west), "{x:?}");
        }
    }

    #[test]
    fn out_and_back_only_on_a_line() {
        let d = site(
            &[(6.10, 46.60), (6.12, 46.60), (6.14, 46.60)],
            &[
                (0, 1, "Ligne", 1, Difficulty::Easy),
                (1, 2, "Ligne", 1, Difficulty::Easy),
            ],
        );
        let g = d.graph.as_ref().unwrap();
        let line = (g.length_m(0) + g.length_m(1)) / 1000.0;
        let s = suggest(&d, &req(minutes(2.0 * line), None));
        let x = s.suggestions.first().expect("the out-and-back");
        assert!(!x.real_loop);
        assert!((x.out_and_back_share - 0.5).abs() < 0.01);
        assert!((x.length_km - 2.0 * line).abs() < 0.05);
        assert!(
            s.notes.iter().any(|n| n.starts_with("aucune vraie boucle")),
            "{:?}",
            s.notes
        );
    }

    #[test]
    fn snow_classes_of_the_qualities() {
        use Quality::*;
        for (q, c) in [
            (Some(Fresh), SnowClass::NewSnow),
            (Some(Windblown), SnowClass::NewSnow),
            (Some(Transformed), SnowClass::Wet),
            (Some(Wet), SnowClass::Wet),
            (Some(Slush), SnowClass::Wet),
            (Some(Hard), SnowClass::Icy),
            (Some(Icy), SnowClass::Icy),
            (Some(OldDry), SnowClass::Groomed),
            (None, SnowClass::Groomed),
        ] {
            assert_eq!(snow_class(q), c, "{q:?}");
        }
    }

    /// Three pieces: 1 km up 100 m, 2 km flat, 1 km down 100 m.
    fn hill(a: &EdgeAttrs) -> Vec<Piece<'_>> {
        [(1000.0, 100.0), (2000.0, 0.0), (1000.0, -100.0)]
            .into_iter()
            .map(|(dist_m, dz_m)| Piece {
                attrs: a,
                dist_m,
                dz_m,
                elev_m: Some(1200.0),
            })
            .collect()
    }

    #[test]
    fn profile_out_and_back_of_pieces() {
        let eta = EtaModel::new(Style::Classic);
        let a = attrs("x", 1, Difficulty::Easy);
        let pieces = hill(&a);
        let p = Profile::of(&eta, &pieces);
        assert_eq!(p.dist_m, [0.0, 1000.0, 3000.0, 4000.0]);
        let t = |d, dz| eta.piece_time_s(&a, d, dz, Some(1200.0));
        let (up, flat, down) = (t(1000.0, 100.0), t(2000.0, 0.0), t(1000.0, -100.0));
        assert!(up > flat / 2.0 && flat / 2.0 > down, "{up} {flat} {down}");
        assert_eq!(p.out_s, [0.0, up, up + flat, up + flat + down]);
        // Back: each piece the other way.
        assert_eq!(p.back_s, [0.0, down, down + flat, down + flat + up]);
        assert_eq!(p.total_s(), up + flat + down);
        assert_eq!(p.length_m(), 4000.0);
        assert_eq!(p.index_at(0.0), 0);
        assert_eq!(p.index_at(2999.0), 1);
        assert_eq!(p.index_at(3000.0), 2);
        assert_eq!(p.index_at(9000.0), 3);
        // By terrain: the climb, the flat, the descent, each its own time.
        let ratio = terrain_ratio(&eta);
        let m = p.model_by_terrain(ratio);
        let near = |x: f64, y: f64| assert!((x - y).abs() < 1e-9, "{x} {y}");
        near(m[Terrain::Up as usize], up);
        near(m[Terrain::Flat as usize], flat);
        near(m[Terrain::Down as usize], down);
        assert_eq!(
            Profile::of(&eta, &[]).model_by_terrain(ratio),
            [0.0; TERRAINS]
        );
    }

    #[test]
    fn profile_at_the_pace_of_a_group() {
        use skimap_core::calibration::GroupProfile;
        let eta = EtaModel::new(Style::Classic);
        let a = attrs("x", 1, Difficulty::Easy);
        let p = Profile::of(&eta, &hill(&a));
        let ratio = terrain_ratio(&eta);
        let family = ProfileModel::new(GroupProfile::family(2, &[6, 9], Style::Classic));
        let params = &family.params;
        for q in [0.5, 0.8] {
            let g = p.for_group_at(params, ratio, q);
            assert_eq!(g.dist_m, p.dist_m);
            // The total is the group's prediction at that quantile.
            let total = params.predict(p.model_by_terrain(ratio)).quantile_s(q);
            assert!(
                (g.total_s() - total).abs() < 1e-6,
                "{} {total}",
                g.total_s()
            );
            // Each piece at the group's pace on its terrain: out, climb over flat; back,
            // descent (the climb taken down) over flat.
            let piece = |v: &[f64], i: usize| v[i + 1] - v[i];
            let k = |t: Terrain| 1.0 / params.progress_pace(t);
            let up_flat =
                (p.out_s[1] * k(Terrain::Up)) / ((p.out_s[2] - p.out_s[1]) * k(Terrain::Flat));
            assert!(
                (piece(&g.out_s, 0) / piece(&g.out_s, 1) - up_flat).abs() < 1e-9,
                "{up_flat}"
            );
            let down_flat =
                (p.back_s[1] * k(Terrain::Down)) / ((p.back_s[2] - p.back_s[1]) * k(Terrain::Flat));
            assert!(
                (piece(&g.back_s, 0) / piece(&g.back_s, 1) - down_flat).abs() < 1e-9,
                "{down_flat}"
            );
        }
        assert_ne!(
            p.for_group_at(params, ratio, 0.8).total_s(),
            p.for_group(params, ratio).total_s()
        );
        // Nothing to scale: zeros, not NaN.
        let empty = Profile::of(&eta, &[]).for_group(params, ratio);
        assert_eq!((empty.out_s, empty.back_s), (vec![0.0], vec![0.0]));
    }
}
