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
//! 2. every node is a possible turning point; its outbound time is the fastest route from
//!    the start; the turning points whose outbound time is 20 to 60 % of the budget are
//!    kept, and at most 40 are spread out by farthest-point sampling;
//! 3. the way back from each turning point is the fastest route with the OSM ways of the
//!    way out made 4 times more costly, so that it comes back by another trail when
//!    there is one (otherwise it is an out-and-back, said as such);
//! 4. trails harder than the level cost 4 times more (`difficulty`: the easiest colour of
//!    the relations of a way, so a shared section of a green loop counts as green);
//! 5. loops whose model time is outside 75 to 110 % of the budget are dropped, then those
//!    that do not end before the return time and the C7 deadline (light limit minus 30
//!    min) with the C7 margin factor 1.2 on the model time; near-duplicates (more than 60
//!    % of their edges in common) keep the best.
//!
//! # Score
//!
//! | Factor | Points |
//! |---|---|
//! | fit to the budget | −2 × relative gap |
//! | out-and-back share (length on the same way twice) | −2 × share |
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

use std::collections::{BTreeSet, HashSet};

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
    /// Share of the length on a way already travelled (out-and-back).
    pub out_and_back_share: f64,
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
    /// retour vers midi et quart. »
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
        let median = params.predict(self.model_by_terrain(ratio)).median_s;
        let f = if o_acc > 0.0 { median / o_acc } else { 1.0 };
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

/// Cost of the loop search: C5 time, style preferences, level and ways to avoid.
struct LoopCost<'a> {
    eta: EtaModel,
    max_difficulty: Option<Difficulty>,
    avoid: &'a HashSet<i64>,
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
            f *= 4.0;
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
    let group_label = group.map(|g| g.profile.label_fr());
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
        notes.push("pas de graphe des pistes (couche routes)".into());
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
        },
    );
    let Some(start) = g.snap(req.start, 300.0) else {
        notes.push(format!(
            "pas de piste à moins de 300 m de {}",
            req.start_name
        ));
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
        notes.push(format!(
            "{} nœuds sur {} ({} km de pistes) ne sont pas atteignables depuis {} : réseau \
             séparé ou sens uniques dans le graphe des pistes (C1)",
            unreachable.len(),
            n,
            fmt(km, 1),
            req.start_name
        ));
    }
    // Turning points: out and fastest way back between 50 and 110 % of the budget (the
    // way back by another trail is longer), spread out. With a group: the model time of
    // that round trip at the group's time on the flat (the costs of the routers carry
    // style factors that its pace would multiply).
    let back_s = |v: usize| -> Option<f64> {
        let r = time_router.route_snaps(node_snap(g, v)?, start).ok()?;
        Some(r.cost)
    };
    let group_round_s = |params: &PaceParams, v: usize| -> Option<f64> {
        let (r_out, _) = out[v].as_ref()?;
        let r_back = time_router.route_snaps(node_snap(g, v)?, start).ok()?;
        let m = eta.time_s(&route_pieces(g, r_out)).total_s
            + eta.time_s(&route_pieces(g, &r_back)).total_s;
        Some(params.predict([0.0, m, 0.0]).median_s)
    };
    let window = 0.5 * budget_s..=1.1 * budget_s;
    let mut cands: Vec<usize> = (0..n)
        .filter(|v| match group {
            None => out[*v]
                .as_ref()
                .is_some_and(|(_, t)| back_s(*v).is_some_and(|b| window.contains(&(t + b)))),
            Some(gm) => group_round_s(&gm.params, *v).is_some_and(|x| window.contains(&x)),
        })
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
    while chosen.len() < 40 && !cands.is_empty() {
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
    let deadline = turnaround::deadline(date, req.start, &TurnaroundParams::default())
        .map(|d| skimap_core::ephemeris::local_time(d.deadline));
    let limit = match (req.return_before.map(|t| date.and_time(t)), deadline) {
        (Some(a), Some(b)) => Some(a.min(b)),
        (a, b) => a.or(b),
    };
    let states = edge_states(data, g, date);
    let bera = data.bera_for(date).and_then(|b| b.level());
    let map = data.snow_for(date);
    let mut no_way_back = 0;
    let mut too_late = 0;
    let (mut too_short, mut too_long) = (0, 0);
    let mut loops: Vec<Suggestion> = Vec::new();
    let mut evaluated = 0;
    for v in chosen {
        let (r_out, _) = out[v].clone().expect("reachable");
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
            },
        );
        let Some(sv) = node_snap(g, v) else { continue };
        let Ok(r_back) = back_router.route_snaps(sv, start) else {
            no_way_back += 1;
            continue;
        };
        evaluated += 1;
        let mut pieces = route_pieces(g, &r_out);
        pieces.extend(route_pieces(g, &r_back));
        let b = eta.time_s(&pieces);
        let profile = Profile::of(&eta, &pieces);
        // Time of the loop: the group's median (C11), else the model time.
        let gp = group.map(|g| g.params.predict(profile.model_by_terrain(ratio)));
        let time_s = gp.map_or(b.total_s, |p| p.median_s);
        if time_s < 0.75 * budget_s {
            too_short += 1;
            continue;
        }
        if time_s > 1.1 * budget_s {
            too_long += 1;
            continue;
        }
        let planned = time_s * TurnaroundParams::default().eta_factor;
        let arrival = req.start_time + Duration::seconds(planned as i64);
        if limit.is_some_and(|l| arrival > l) {
            too_late += 1;
            continue;
        }
        let mut line = r_out.polyline(g);
        line.extend(r_back.polyline(g));
        let legs: Vec<_> = r_out.legs.iter().chain(&r_back.legs).copied().collect();
        // Out-and-back share: length on a way travelled twice.
        let mut seen: HashSet<i64> = HashSet::new();
        let mut twice = 0.0;
        let mut total = 0.0;
        let (mut open_km, mut closed_km, mut unknown_km) = (0.0, 0.0, 0.0);
        for l in &legs {
            let len = l.length_m();
            total += len;
            if let Some(w) = g.edge(l.edge).attrs.osm_way_id
                && !seen.insert(w)
            {
                twice += len;
            }
            match states[l.edge] {
                OpenState::Open => open_km += len / 1000.0,
                OpenState::Closed => closed_km += len / 1000.0,
                OpenState::Unknown => unknown_km += len / 1000.0,
            }
        }
        let share_twice = if total > 0.0 { twice / total } else { 0.0 };
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
            "boucle sans nom".to_owned()
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
        let style = match req.style {
            Style::Classic => "classique",
            Style::Skating => "skating",
        };
        let time_text = match (group, gp) {
            (Some(g), Some(p)) => format!(
                "environ {} pour {} ({} à {}, {} ; modèle C5, {} : {})",
                hm(p.median_s),
                g.profile.label_fr(),
                hm(p.lo_s),
                hm(p.hi_s),
                match g.outings {
                    0 => "a priori".to_owned(),
                    1 => "une sortie apprise".to_owned(),
                    n => format!("{n} sorties apprises"),
                },
                style,
                hm(b.total_s)
            ),
            _ => format!("environ {} (modèle C5, {style})", hm(b.total_s)),
        };
        briefing::reason(
            &mut reasons,
            Factor::Data,
            -2.0 * gap.abs(),
            format!(
                "{} km, D+ {} m, {time_text}, retour prévu vers {} h {:02}",
                fmt(total / 1000.0, 1),
                b.gain_m.round(),
                arrival.hour(),
                arrival.minute()
            ),
        );
        if share_twice >= 0.05 {
            briefing::reason(
                &mut reasons,
                Factor::Data,
                -2.0 * share_twice,
                format!(
                    "{} % du parcours en aller-retour sur le même chemin",
                    (share_twice * 100.0).round()
                ),
            );
        }
        let len_km = total / 1000.0;
        if closed_km > 0.05 {
            briefing::reason(
                &mut reasons,
                Factor::Status,
                -10.0 * closed_km / len_km,
                format!("{} km sur des pistes fermées", fmt(closed_km, 1)),
            );
        }
        if open_km > 0.05 {
            briefing::reason(
                &mut reasons,
                Factor::Status,
                open_km / len_km,
                format!("{} km sur des pistes ouvertes", fmt(open_km, 1)),
            );
        }
        if unknown_km > 0.05 && closed_km + open_km > 0.0 {
            briefing::reason(
                &mut reasons,
                Factor::Status,
                0.0,
                format!("{} km sans état connu", fmt(unknown_km, 1)),
            );
        }
        if closed_km + open_km == 0.0 {
            briefing::reason(
                &mut reasons,
                Factor::Status,
                0.0,
                "état des pistes inconnu".into(),
            );
        }
        briefing::snow_reasons(&mut reasons, &snow, None);
        if exp.0 >= 0.05 {
            let km = fmt(exp.0, 1);
            let (p, why) = match bera {
                Some(l) if l >= 3 => (-2.0, format!("BERA {l} sur 5")),
                Some(l) => (0.0, format!("BERA {l} sur 5")),
                None if data.no_bera() => (-0.5, "pas de BERA dans ce massif".into()),
                None => (-0.5, "BERA non disponible".into()),
            };
            briefing::reason(
                &mut reasons,
                Factor::Avalanche,
                p,
                format!("{km} km en terrain exposé aux avalanches (modèle), {why}"),
            );
        }
        if let Some(w) = &weather
            && w.hours > 0
        {
            briefing::weather_reasons(&mut reasons, w);
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
                    format!(
                        "{} km sur des pistes plus difficiles que demandé",
                        fmt(hard, 1)
                    ),
                );
            }
        }
        let named = names.len().min(5) as f64 * 0.1;
        if named > 0.0 {
            briefing::reason(
                &mut reasons,
                Factor::Data,
                named,
                format!("pistes : {}", names.join(", ")),
            );
        }
        let score = reasons.iter().map(|r| r.points).sum();
        let voice = loop_voice(len_km, time_s, arrival);
        let group_time = match (group, gp) {
            (Some(g), Some(p)) => Some(GroupTime {
                label: g.profile.label_fr(),
                outings: g.outings,
                median_min: (p.median_s / 60.0).round(),
                lo_min: (p.lo_s / 60.0).round(),
                hi_min: (p.hi_s / 60.0).round(),
                moving_min: (p.moving_s / 60.0).round(),
                stops_min: (p.stop_s / 60.0).round(),
            }),
            _ => None,
        };
        loops.push(Suggestion {
            label,
            names,
            length_km: (len_km * 100.0).round() / 100.0,
            gain_m: b.gain_m.round(),
            loss_m: b.loss_m.round(),
            eta_min: (b.total_s / 60.0).round(),
            arrival,
            turn: g.nodes()[v].pos,
            out_and_back_share: (share_twice * 100.0).round() / 100.0,
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
        });
    }
    if no_way_back > 0 {
        notes.push(format!(
            "{no_way_back} points de demi-tour sans retour possible vers le départ (sens uniques \
             du graphe C1)"
        ));
    }
    if too_short + too_long > 0 {
        notes.push(format!(
            "{evaluated} boucles évaluées, {too_short} trop courtes et {too_long} trop longues \
             (hors de 75 à 110 % du temps demandé{})",
            group_label
                .as_ref()
                .map(|l| format!(", au rythme du groupe : {l}"))
                .unwrap_or_default()
        ));
    }
    if too_late > 0 {
        notes.push(format!(
            "{too_late} boucles écartées : retour après {}",
            limit
                .map(|l| format!("{} h {:02}", l.hour(), l.minute()))
                .unwrap_or_default()
        ));
    }
    loops.sort_by(|a, b| b.score.total_cmp(&a.score));
    let mut kept: Vec<Suggestion> = Vec::new();
    for l in loops {
        let similar = kept.iter().any(|k| {
            let inter = k.edges.intersection(&l.edges).count() as f64;
            let union = k.edges.union(&l.edges).count() as f64;
            union > 0.0 && inter / union > 0.6
        });
        if !similar {
            kept.push(l);
        }
        if kept.len() >= req.max_results {
            break;
        }
    }
    if kept.is_empty() {
        notes.push("aucune boucle ne tient dans le temps et l'heure de retour demandés".into());
    } else {
        if kept
            .iter()
            .all(|k| k.snow.is_some_and(|s| s.depth_cm() < 20.0))
        {
            notes.push(
                "neige estimée insuffisante sur toutes les boucles : rien n'est skiable ce jour"
                    .into(),
            );
        }
        if kept.iter().all(|k| k.status_km.1 > 0.5 * k.length_km) {
            notes.push("pistes fermées sur la plus grande partie de chaque boucle".into());
        }
    }
    let mut text = vec![format!(
        "{} depuis {} le {} à {} h {:02}{}{}",
        req.label,
        req.start_name,
        briefing::date_fr(date),
        req.start_time.hour(),
        req.start_time.minute(),
        deadline
            .map(|d| format!(
                " (heure limite de retour C7 : {} h {:02})",
                d.hour(),
                d.minute()
            ))
            .unwrap_or_default(),
        group_label
            .as_ref()
            .map(|l| format!(", temps du groupe : {l}"))
            .unwrap_or_default()
    )];
    for (i, s) in kept.iter().enumerate() {
        let why: Vec<String> = s
            .reasons
            .iter()
            .map(|r| {
                if r.points == 0.0 {
                    r.text.clone()
                } else {
                    format!("{} [{:+.2}]", r.text, r.points).replace('.', ",")
                }
            })
            .collect();
        text.push(format!(
            "{}. {} (score {}) : {}.",
            i + 1,
            s.label,
            fmt(s.score, 2),
            why.join(" ; ")
        ));
    }
    if !notes.is_empty() {
        text.push(format!("Limites : {}.", notes.join(" ; ")));
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

fn fmt(x: f64, d: usize) -> String {
    format!("{x:.d$}").replace('.', ",")
}

/// « 1 h 50 ».
pub fn hm(seconds: f64) -> String {
    let m = (seconds / 60.0).round() as i64;
    if m < 60 {
        format!("{m} min")
    } else {
        format!("{} h {:02}", m / 60, m % 60)
    }
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
            "Boucle de treize kilomètres, environ deux heures, retour vers midi et quart."
        );
    }
    use skimap_common::site::CHAPELLE_DES_BOIS;
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
        let p = |lon: f64, lat: f64| LonLat::new(lon, lat);
        let pts = [
            p(6.10, 46.60),
            p(6.12, 46.60),
            p(6.12, 46.62),
            p(6.10, 46.62),
            p(6.08, 46.60),
        ];
        let nodes = pts
            .iter()
            .map(|q| Node {
                pos: *q,
                osm_node_id: None,
            })
            .collect();
        let edge = |a: usize, b: usize, name: &str, way: i64, d: Difficulty| Edge {
            from: a,
            to: b,
            oneway: false,
            points: vec![pts[a], pts[b]],
            attrs: attrs(name, way, d),
            profile: None,
        };
        let g = PisteGraph::new(GraphData {
            site: "test".into(),
            nodes,
            edges: vec![
                edge(0, 1, "Sud", 1, Difficulty::Easy),
                edge(1, 2, "Est", 2, Difficulty::Easy),
                edge(2, 3, "Nord", 3, Difficulty::Easy),
                edge(3, 0, "Ouest", 4, Difficulty::Advanced),
                edge(0, 4, "Antenne", 5, Difficulty::Easy),
            ],
            elevation_source: None,
        })
        .unwrap();
        SiteData {
            site: CHAPELLE_DES_BOIS,
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
        // for it; whatever is proposed fits its time, not the model's.
        let family = ProfileModel::new(GroupProfile::family(2, &[6, 9], Style::Classic));
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
    }
}
