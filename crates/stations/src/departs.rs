//! Trail starts of every census domain (X9, #143): where a skier arriving by car begins.
//!
//! No source publishes the starts. They are deduced, for every domain alike, from:
//! - the OSM points of the station cards (`pois.geojson`): nordic lodges (`foyer`), pass
//!   desks (`ticket`), ski schools and car parks (not private) near the domain;
//! - the trail network of the domain (OpenSnowMap planet extract, ways of
//!   `domaines_ways.json`): each point is snapped to the nearest point of the network, a point
//!   near a dead end of the network (trailhead) counts more;
//! - points within [`CLUSTER_M`] of each other make one start (a car park beside a lodge);
//! - the part of the network a start serves (#166): connected component of the routing
//!   graph of the domain ways ([`Reach`], the graph of the packs), weighed by its nordic
//!   km. A point on a piece too short for a loop of the default outing ([`LOOP_KM`])
//!   counts less in proportion, and the largest part always gets a start
//!   ([`Served`], said in `departs.json`).
//!
//! The best [`MAX_STARTS`] starts are kept, main first; the pack of the site then orders
//! them by the loops the planner finds from each (#166, `build::rank` of `skimap-pack`),
//! the registry keeps this order. A domain without any such point
//! gets the network point nearest to its nearest lodge, desk or car park, else to its
//! nearest tourist office or ski school (the village), else the census representative point (on the network), with the method said.
//!
//! Output: `departs.json` (census id → starts, the shape of
//! [`skimap_common::site::Start`] plus method and score) and `departs.geojson` (one point
//! per start, for map checks), in `data/out/france/stations/`. The site registry reads
//! `departs.json` (`skimap registre`).

use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use skimap_common::geom::LonLat;
use skimap_common::layout::DataLayout;
use skimap_common::output::{feature, point, round7, write_geojson, write_json};
use skimap_common::site::Start;
use skimap_osm::extract::CONNECTOR_MAX_M;
use skimap_osm::graph::{EdgeKind, Graph};
use skimap_osm::pistes::PisteSet;

use crate::domains::{Domain, DomainWays, Local};
use crate::error::{Result, StationsError};

/// Output file (census id → starts).
pub const FILE: &str = "departs.json";
/// Map of the starts.
pub const GEOJSON: &str = "departs.geojson";
/// Starts kept per domain.
pub const MAX_STARTS: usize = 3;
/// Points closer than this make one start, m.
pub const CLUSTER_M: f64 = 500.0;
/// A dead end of the network within this distance of a point makes it a trailhead, m.
pub const TRAILHEAD_M: f64 = 150.0;
/// A start scoring less than this share of the main one is dropped (a lone car park far
/// from a lodge with its own car parks).
pub const MIN_SHARE: f64 = 0.25;

/// Largest distance of the point of a [`Method::Near`] start to the network, m.
pub const NEAR_MAX_M: f64 = 1000.0;

/// Nordic km of network a start must serve for a loop of the app's default outing (2 h,
/// classic). Measured on the loops the planner accepts (#166, the morning run from every
/// start of 12 packs of 10 massifs, 17/11/2026, `build::rank` of `skimap-pack`): the
/// length of a loop on stretches travelled once is 5.6 to 17.3 km, 11.5 km median (47
/// loops; first decile 7.7 km). A part of the network shorter than the first decile is too
/// short for nine loops in ten: a point serving fewer km weighs that share of its score
/// (the median, 11.5 km, would also lower parts that do hold loops, and drop their starts
/// under [`MIN_SHARE`]). The pack then ranks the kept starts by the planner itself; this
/// weight chooses which starts are kept and their order in the registry.
pub const LOOP_KM: f64 = 7.7;

/// Weight of a kind of point and its largest distance to the network, m.
pub fn kind_rule(kind: &str) -> Option<(f64, f64)> {
    match kind {
        "foyer" => Some((5.0, 300.0)),
        "ticket" => Some((4.0, 300.0)),
        "ski_school" => Some((2.0, 300.0)),
        // The point of a car park area is its centre, often far inside a large lot.
        "parking" => Some((2.0, 300.0)),
        _ => None,
    }
}

/// An OSM point of `pois.geojson`, as read here.
#[derive(Debug, Clone)]
pub struct Point {
    /// Kind (`foyer`, `ticket`, `parking`…).
    pub kind: String,
    /// Name.
    pub name: Option<String>,
    /// Position.
    pub pos: LonLat,
    /// Car park places.
    pub capacity: Option<f64>,
    /// Car park access.
    pub access: Option<String>,
    /// OSM page.
    pub url: String,
    /// Domains of the point.
    pub domains: Vec<String>,
    /// Score added to the point (a car park cleared in winter, X9b).
    pub bonus: f64,
}

/// Reads `pois.geojson`.
pub fn read_points(v: &Value) -> Vec<Point> {
    let s = |p: &Value, k: &str| {
        p.get(k)
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|x| !x.is_empty())
            .map(str::to_owned)
    };
    v["features"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|f| {
            let p = &f["properties"];
            let c = f["geometry"]["coordinates"].as_array()?;
            Some(Point {
                kind: s(p, "kind")?,
                name: s(p, "name"),
                pos: LonLat::new(c.first()?.as_f64()?, c.get(1)?.as_f64()?),
                capacity: s(p, "capacity").and_then(|c| c.parse().ok()),
                access: s(p, "access"),
                url: s(p, "url").unwrap_or_default(),
                domains: p["domains"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(|d| d["id"].as_str().map(str::to_owned))
                    .collect(),
                bonus: 0.0,
            })
        })
        .collect()
}

/// A trail way of a domain: its name and its nodes.
#[derive(Debug, Clone)]
pub struct Way {
    /// `piste:name` or `name`.
    pub name: Option<String>,
    /// Node ids and positions.
    pub points: Vec<(i64, LonLat)>,
}

impl Way {
    /// The way of a piste of the planet extract.
    pub fn of(p: &skimap_osm::pistes::Piste) -> Self {
        Self {
            name: p.way_name.clone(),
            points: p.points.clone(),
        }
    }
}

/// How a start was found.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Method {
    /// From lodges, pass desks, ski schools or car parks next to the network.
    Points,
    /// Network point nearest to a lodge, desk or car park of the domain farther than the
    /// distances of [`kind_rule`].
    Near,
    /// Network point nearest to the village (tourist office or ski school).
    Village,
    /// End of an SRN route (X9b): the dead end of the routes nearest to the village, else
    /// the beginning of the longest route (published routes start where one parks).
    RouteEnd,
    /// Census representative point (on the network); for an SRN area, its representative
    /// point (on a route).
    Centre,
}

/// A start of a domain.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Depart {
    /// The start as the sites use it.
    #[serde(flatten)]
    pub start: Start,
    /// How it was found.
    pub method: Method,
    /// Score (sum of the weights of its points).
    pub score: f64,
    /// OSM pages of its points.
    pub points: Vec<String>,
    /// Part of the trail network it serves and why it ranks there (census domains, #166).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub network: Option<Served>,
}

/// Part of the trail network a start serves ([`Reach`]).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Served {
    /// Nordic km of the part (connected component of the routing graph).
    pub km: f64,
    /// Nordic km of the whole domain network.
    pub domain_km: f64,
    /// Whether it is the largest part of the domain network.
    pub main: bool,
    /// Why the start is there: its score lowered by a short part, or kept on the largest
    /// part that no better start served.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

/// Parts of the trail network of a domain a skier links without leaving the trails:
/// connected components of the routing graph of its ways ([`Graph::build`] with the
/// repairs of the packs, [`CONNECTOR_MAX_M`]), with their nordic km. Hike and snowshoe
/// ways link parts (the planner uses them) but add no km.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Reach {
    /// Component of each way (index into the ways of the domain), `None` without edge.
    pub way_component: Vec<Option<usize>>,
    /// Nordic km of each component.
    pub component_km: Vec<f64>,
}

impl Reach {
    /// Components of `set`, the pistes of the domain ways in the order of its ways.
    pub fn of(set: &PisteSet) -> Self {
        let g = Graph::build(set, CONNECTOR_MAX_M);
        let (comp, n) = g.components();
        let mut component_km = vec![0.0; n];
        let mut way_component = vec![None; set.pistes.len()];
        for e in &g.edges {
            if let EdgeKind::Piste { piste } = e.kind {
                let c = comp[e.from];
                way_component[piste] = Some(c);
                if set.pistes[piste].is_nordic() {
                    component_km[c] += e.length_m / 1000.0;
                }
            }
        }
        Self {
            way_component,
            component_km,
        }
    }

    /// Nordic km of the whole network.
    pub fn total_km(&self) -> f64 {
        self.component_km.iter().sum()
    }

    /// The largest component, by nordic km (none without nordic km).
    pub fn main(&self) -> Option<usize> {
        self.component_km
            .iter()
            .enumerate()
            .filter(|(_, km)| **km > 0.0)
            .max_by(|a, b| a.1.total_cmp(b.1).then(b.0.cmp(&a.0)))
            .map(|(i, _)| i)
    }

    /// Component of way `w`.
    pub fn component(&self, w: usize) -> Option<usize> {
        self.way_component.get(w).copied().flatten()
    }

    /// Nordic km of the component of way `w`.
    pub fn km(&self, w: usize) -> f64 {
        self.component(w).map_or(0.0, |c| self.component_km[c])
    }

    /// Weight of a start on way `w`: the share of a default-outing loop its part holds,
    /// at most 1 ([`LOOP_KM`]).
    pub fn factor(&self, w: usize) -> f64 {
        (self.km(w) / LOOP_KM).min(1.0)
    }

    /// What a start on way `w` serves, with `reason`.
    pub fn served(&self, w: usize, reason: Option<String>) -> Served {
        let r1 = |x: f64| (x * 10.0).round() / 10.0;
        Served {
            km: r1(self.km(w)),
            domain_km: r1(self.total_km()),
            main: self.component(w).is_some() && self.component(w) == self.main(),
            reason,
        }
    }
}

/// What a start at `pos` serves: the part of the network of its nearest way (`ways` in
/// the order of `reach`); `None` without network.
pub fn serving(ways: &[Way], reach: &Reach, pos: LonLat) -> Option<Served> {
    let local = Local::new(pos);
    let net = Net::new(ways, &local);
    net.nearest(local.xy(pos))
        .map(|s| reach.served(s.way, None))
}

/// `x` with one decimal and a decimal comma.
fn km_fr(x: f64) -> String {
    format!("{x:.1}").replace('.', ",")
}

/// `departs.json`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DepartsFile {
    /// Generation instant.
    pub generated_at: String,
    /// Date of the OSM trail data.
    pub osm_valid_at: Option<String>,
    /// How the starts are found.
    pub notes: Vec<String>,
    /// Census id → starts, main first.
    pub domains: BTreeMap<String, Vec<Depart>>,
    /// SRN area id (`srn_areas.geojson`) → starts, best first (X9b): every start found
    /// along the routes, the registry keeps the best of each site or section
    /// ([`crate::departs_srn::pick`]).
    #[serde(default)]
    pub srn: BTreeMap<String, Vec<Depart>>,
}

impl DepartsFile {
    /// Reads `departs.json`.
    pub fn read(path: &Path) -> Result<Self> {
        let b = std::fs::read(path).map_err(|e| skimap_common::Error::io(path, e))?;
        serde_json::from_slice(&b).map_err(|e| StationsError::format(FILE, e.to_string()))
    }
}

/// A segment of the network: ends (local metres) and way index.
type Segment = ((f64, f64), (f64, f64), usize);

pub(crate) struct Net {
    /// Vertices: position (local metres) and whether a dead end of the network.
    pub(crate) vertices: Vec<((f64, f64), bool)>,
    /// Segments.
    pub(crate) segments: Vec<Segment>,
}

/// A point snapped to the network.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Snap {
    /// Nearest point of the network (local metres).
    pub(crate) xy: (f64, f64),
    /// Way of that point.
    pub(crate) way: usize,
    /// Distance, m.
    pub(crate) dist: f64,
}

pub(crate) fn d2(a: (f64, f64), b: (f64, f64)) -> f64 {
    ((a.0 - b.0).powi(2) + (a.1 - b.1).powi(2)).sqrt()
}

fn project(p: (f64, f64), a: (f64, f64), b: (f64, f64)) -> (f64, f64) {
    let (dx, dy) = (b.0 - a.0, b.1 - a.1);
    let l2 = dx * dx + dy * dy;
    let t = if l2 == 0.0 {
        0.0
    } else {
        (((p.0 - a.0) * dx + (p.1 - a.1) * dy) / l2).clamp(0.0, 1.0)
    };
    (a.0 + t * dx, a.1 + t * dy)
}

impl Net {
    pub(crate) fn new(ways: &[Way], local: &Local) -> Self {
        let mut deg: HashMap<i64, u32> = HashMap::new();
        for w in ways {
            let n = w.points.len();
            for (i, (id, _)) in w.points.iter().enumerate() {
                *deg.entry(*id).or_default() += if i == 0 || i + 1 == n { 1 } else { 2 };
            }
        }
        let mut seen: HashMap<i64, ()> = HashMap::new();
        let mut net = Net {
            vertices: Vec::new(),
            segments: Vec::new(),
        };
        for (wi, w) in ways.iter().enumerate() {
            let xy: Vec<(f64, f64)> = w.points.iter().map(|(_, p)| local.xy(*p)).collect();
            for (k, (id, _)) in w.points.iter().enumerate() {
                if seen.insert(*id, ()).is_none() {
                    net.vertices.push((xy[k], deg.get(id) == Some(&1)));
                }
            }
            for pair in xy.windows(2) {
                net.segments.push((pair[0], pair[1], wi));
            }
            if xy.len() == 1 {
                net.segments.push((xy[0], xy[0], wi));
            }
        }
        net
    }

    pub(crate) fn nearest(&self, p: (f64, f64)) -> Option<Snap> {
        self.nearest_where(p, |_| true)
    }

    /// Nearest point of the ways `keep` accepts.
    pub(crate) fn nearest_where(
        &self,
        p: (f64, f64),
        keep: impl Fn(usize) -> bool,
    ) -> Option<Snap> {
        self.segments
            .iter()
            .filter(|(_, _, way)| keep(*way))
            .map(|(a, b, way)| {
                let xy = project(p, *a, *b);
                Snap {
                    xy,
                    way: *way,
                    dist: d2(p, xy),
                }
            })
            .min_by(|a, b| a.dist.total_cmp(&b.dist))
    }

    fn dead_end_within(&self, p: (f64, f64), m: f64) -> bool {
        self.vertices.iter().any(|(q, e)| *e && d2(p, *q) <= m)
    }
}

struct Candidate<'a> {
    point: &'a Point,
    xy: (f64, f64),
    snap: Snap,
    score: f64,
}

/// Points making one start: the best one (`head`, an index of the candidates), the others.
#[derive(Debug, Clone)]
struct Cluster {
    head: usize,
    score: f64,
    members: Vec<usize>,
    method: Method,
    /// Why it is kept when not for its score ([`Served::reason`]).
    reason: Option<String>,
    /// Its head is snapped to the largest part of the network, not to the nearest trail.
    main_only: bool,
}

/// Score of a point `dist` metres from the network (at most `max`): the weight of its kind
/// lowered with the distance, car park places, its bonus, +1 near a dead end.
fn point_score(p: &Point, dist: f64, max: f64, net: &Net, xy: (f64, f64)) -> f64 {
    let mut score = weight(p) * (1.0 - 0.5 * dist / max);
    if p.kind == "parking" {
        score += p.capacity.map_or(0.0, |c| (c / 50.0).min(2.0));
    }
    score += p.bonus;
    if net.dead_end_within(xy, TRAILHEAD_M) {
        score += 1.0;
    }
    score
}

/// The point of the largest part `main` of the network nearest to a lodge, desk or car
/// park of the place (not private, at most [`NEAR_MAX_M`] from that part), the best scored;
/// [`Method::Points`] when within the distance of its kind, [`Method::Near`] beyond.
fn near_main<'a>(
    r: &Reach,
    main: usize,
    net: &Net,
    local: &Local,
    points: &[&'a Point],
) -> Option<(Candidate<'a>, Method)> {
    points
        .iter()
        .filter(|p| !(p.kind == "parking" && private(p)))
        .filter_map(|p| {
            let (_, kmax) = kind_rule(&p.kind)?;
            let xy = local.xy(p.pos);
            let snap = net.nearest_where(xy, |w| r.component(w) == Some(main))?;
            (snap.dist <= NEAR_MAX_M).then(|| {
                let method = if snap.dist <= kmax {
                    Method::Points
                } else {
                    Method::Near
                };
                let score = point_score(p, snap.dist, NEAR_MAX_M, net, xy) * r.factor(snap.way);
                (
                    Candidate {
                        point: p,
                        xy,
                        snap,
                        score,
                    },
                    method,
                )
            })
        })
        .max_by(|a, b| {
            a.0.score
                .total_cmp(&b.0.score)
                .then(b.0.point.url.cmp(&a.0.point.url))
        })
}

pub(crate) fn private(p: &Point) -> bool {
    matches!(
        p.access.as_deref(),
        Some("private" | "no" | "customers" | "permit" | "residents")
    )
}

fn kind_fr(kind: &str) -> (&'static str, &'static str, &'static str) {
    // (name, phrase, English phrase pattern).
    match kind {
        "foyer" => ("Foyer nordique", "le foyer nordique", "nordic lodge"),
        "ticket" => ("Accueil des pistes", "l'accueil des pistes", "trail desk"),
        "ski_school" => ("École de ski", "l'école de ski", "ski school"),
        "parking" => ("Parking", "le parking", "car park"),
        _ => ("Départ des pistes", "le départ des pistes", "trailhead"),
    }
}

/// What a place is, for the words of its starts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlaceKind {
    /// Groomed nordic domain of the census.
    Domain,
    /// SRN play area (`srn_areas.geojson`, `kind: domaine`).
    SrnArea,
    /// SRN traverse (`kind: traversee`).
    Traverse,
}

/// A place whose starts are searched: a census domain or an SRN area.
#[derive(Debug, Clone)]
pub struct Place {
    /// Kind.
    pub kind: PlaceKind,
    /// Display name.
    pub name: String,
    /// Representative point (on the network).
    pub point: LonLat,
}

impl Place {
    /// A census domain.
    pub fn domain(d: &Domain) -> Self {
        Self {
            kind: PlaceKind::Domain,
            name: d.name.clone(),
            point: d.point,
        }
    }

    /// « du domaine X », « de la zone SRN X », « de la traversée X ».
    pub fn of_fr(&self) -> String {
        match self.kind {
            PlaceKind::Domain => format!("du domaine {}", self.name),
            PlaceKind::SrnArea => format!("de la zone SRN {}", self.name),
            PlaceKind::Traverse => format!("de la traversée {}", self.name),
        }
    }

    /// « the X area », « the X SRN area », « the X traverse ».
    pub fn the_en(&self) -> String {
        match self.kind {
            PlaceKind::Domain => format!("the {} area", self.name),
            PlaceKind::SrnArea => format!("the {} SRN area", self.name),
            PlaceKind::Traverse => format!("the {} traverse", self.name),
        }
    }

    /// What its network is made of: « pistes », « itinéraires ».
    pub fn trails_fr(&self) -> &'static str {
        match self.kind {
            PlaceKind::Domain => "pistes",
            PlaceKind::SrnArea | PlaceKind::Traverse => "itinéraires",
        }
    }

    /// One trail of its network, French and English: « piste », « itinéraire ».
    pub fn trail(&self) -> (&'static str, &'static str) {
        match self.kind {
            PlaceKind::Domain => ("piste", "trail"),
            PlaceKind::SrnArea | PlaceKind::Traverse => ("itinéraire", "route"),
        }
    }
}

/// Starts of one domain from its points (those of `pois.geojson` attached to it), its
/// trail ways and, for the fallbacks, its representative point.
pub fn domain_departs(
    d: &Domain,
    ways: &[Way],
    points: &[&Point],
    reach: Option<&Reach>,
) -> Vec<Depart> {
    let place = Place::domain(d);
    let mut out = clustered(&place, ways, points, MAX_STARTS, MIN_SHARE, reach);
    if out.is_empty() {
        let local = Local::new(d.point);
        let net = Net::new(ways, &local);
        let mut f = fallback(d, &net, &local, points);
        f.network = reach
            .filter(|r| r.main().is_some())
            .and_then(|r| serving(ways, r, f.start.position()));
        out.push(f);
    }
    number(&mut out);
    out
}

/// Numbers the starts `depart-1`, `depart-2`…
pub fn number(out: &mut [Depart]) {
    for (i, o) in out.iter_mut().enumerate() {
        o.start.id = format!("depart-{}", i + 1);
    }
}

/// Starts of a place from its points next to its network: clusters of points, the
/// best `max` kept, each at least `min_share` of the best score. Empty without such
/// points (the caller's fallbacks).
///
/// With the `reach` of a domain network (#166): each point weighs its [`Reach::factor`]
/// (a point on a piece too short for a default-outing loop counts less), and when no kept
/// start serves the largest part of the network, the best start on it is kept: the best
/// cluster on it, else the point of that part nearest to a lodge, desk or car park of the
/// place (at most [`NEAR_MAX_M`] away). It comes first when the first start serves fewer
/// than [`LOOP_KM`], last otherwise (in place of the last one when `max` are kept).
pub fn clustered(
    place: &Place,
    ways: &[Way],
    points: &[&Point],
    max: usize,
    min_share: f64,
    reach: Option<&Reach>,
) -> Vec<Depart> {
    let local = Local::new(place.point);
    let net = Net::new(ways, &local);
    let reach = reach.filter(|r| r.main().is_some());
    let mut cands: Vec<Candidate> = Vec::new();
    for p in points {
        let Some((_, max)) = kind_rule(&p.kind) else {
            continue;
        };
        if p.kind == "parking" && private(p) {
            continue;
        }
        let xy = local.xy(p.pos);
        let Some(snap) = net.nearest(xy) else {
            continue;
        };
        if snap.dist > max {
            continue;
        }
        let factor = reach.map_or(1.0, |r| r.factor(snap.way));
        cands.push(Candidate {
            point: p,
            xy,
            snap,
            score: point_score(p, snap.dist, max, &net, xy) * factor,
        });
    }
    cands.sort_by(|a, b| {
        b.score
            .total_cmp(&a.score)
            .then(a.point.url.cmp(&b.point.url))
    });
    // Greedy clusters around the best points.
    let mut clusters: Vec<Cluster> = Vec::new();
    for (i, c) in cands.iter().enumerate() {
        match clusters
            .iter_mut()
            .find(|cl| d2(cands[cl.head].xy, c.xy) <= CLUSTER_M)
        {
            Some(cl) => {
                cl.score += 0.5 * c.score;
                cl.members.push(i);
            }
            None => clusters.push(Cluster {
                head: i,
                score: c.score,
                members: vec![i],
                method: Method::Points,
                reason: None,
                main_only: false,
            }),
        }
    }
    clusters.sort_by(|a, b| b.score.total_cmp(&a.score));
    let ranked = clusters.clone();
    let best = clusters.first().map_or(0.0, |c| c.score);
    clusters.retain(|c| c.score >= min_share * best);
    clusters.truncate(max);
    if let Some(r) = reach
        && let Some(main) = r.main()
        && max > 0
    {
        let on_main =
            |c: &Cluster, cands: &[Candidate]| r.component(cands[c.head].snap.way) == Some(main);
        if !clusters.iter().any(|c| on_main(c, &cands)) {
            let lead = format!(
                "réseau principal du domaine ({} km nordiques sur {})",
                km_fr(r.component_km[main]),
                km_fr(r.total_km())
            );
            let first_km = clusters.first().map(|c| r.km(cands[c.head].snap.way));
            let why_first = match first_km {
                Some(km) if km < LOOP_KM => format!(
                    " ; placé en tête : le meilleur départ ne dessert que {} km, moins que les {LOOP_KM} km d'une boucle de 2 h",
                    km_fr(km)
                ),
                _ => String::new(),
            };
            let kept = match ranked.iter().find(|c| on_main(c, &cands)).cloned() {
                Some(mut c) => {
                    c.reason = Some(format!(
                        "{lead} : aucun départ mieux noté ne le dessert{why_first}"
                    ));
                    Some(c)
                }
                None => near_main(r, main, &net, &local, points).map(|(c, method)| {
                    let i = cands.len();
                    cands.push(c);
                    Cluster {
                        head: i,
                        score: cands[i].score,
                        members: vec![i],
                        method,
                        main_only: true,
                        reason: Some(match method {
                            Method::Near => format!(
                                "{lead} : aucun foyer, accueil ni parking à moins de 300 m de lui, point du réseau principal le plus proche de l'un d'eux{why_first}"
                            ),
                            _ => format!(
                                "{lead} : point du réseau principal le plus proche d'un foyer, accueil ou parking à moins de 300 m, plus près encore d'une autre partie{why_first}"
                            ),
                        }),
                    }
                }),
            };
            if let Some(c) = kept {
                let head = c.head;
                if first_km.is_none_or(|km| km < LOOP_KM) {
                    clusters.insert(0, c);
                    clusters.truncate(max);
                } else {
                    clusters.truncate(max - 1);
                    clusters.push(c);
                }
                // The others again at least `min_share` of the best of them all.
                let best = clusters.iter().map(|c| c.score).fold(0.0, f64::max);
                clusters.retain(|c| c.head == head || c.score >= min_share * best);
            }
        }
        // The first start serves a default-outing loop when a kept one does.
        let km = |c: &Cluster| r.km(cands[c.head].snap.way);
        if let Some(first) = clusters.first().map(km)
            && first < LOOP_KM
            && let Some(i) = clusters.iter().position(|c| km(c) >= LOOP_KM)
        {
            let mut c = clusters.remove(i);
            c.reason = Some(format!(
                "réseau de {} km nordiques : placé en tête, le départ le mieux noté ne dessert que {} km, moins que les {LOOP_KM} km d'une boucle de 2 h",
                km_fr(km(&c)),
                km_fr(first)
            ));
            clusters.insert(0, c);
        }
    }

    let mut out: Vec<Depart> = Vec::new();
    for cl in &clusters {
        let ms: Vec<&Candidate> = cl.members.iter().map(|i| &cands[*i]).collect();
        // Named after the best named lodge or desk, else the best named point, else the
        // kind of the head.
        let named = ms.iter().filter(|c| c.point.name.is_some()).max_by(|a, b| {
            let r = |c: &Candidate| weight(c.point);
            r(a).total_cmp(&r(b)).then(b.score.total_cmp(&a.score))
        });
        let h = &cands[cl.head];
        let way_name = ways[h.snap.way].name.clone();
        let (kname, kphrase, ken) = kind_fr(&h.point.kind);
        let (trail_fr, _) = place.trail();
        let (name, phrase, phrase_en) = match named.and_then(|c| c.point.name.clone()) {
            Some(n) => {
                let n = named_as(named.map_or("", |c| c.point.kind.as_str()), &n);
                (format!("{n} ({})", place.name), n.clone(), None)
            }
            None => (
                format!("{kname} ({})", place.name),
                format!("{kphrase} {}", place.of_fr()),
                Some(format!("{} {ken}", place.the_en())),
            ),
        };
        let pos = local.lonlat(h.snap.xy);
        let trail = way_name
            .map(|w| format!(", {trail_fr} « {w} »"))
            .unwrap_or_default();
        let provenance = if cl.main_only {
            format!(
                "déduit (X9) : point du réseau principal le plus proche de {} ({}, à {:.0} m du réseau){trail}",
                h.point.kind, h.point.url, h.snap.dist
            )
        } else {
            format!(
                "déduit (X9) : point du réseau le plus proche de {} ({}){trail}",
                ms.iter()
                    .map(|c| c.point.kind.as_str())
                    .collect::<Vec<_>>()
                    .join(", "),
                h.point.url,
            )
        };
        let network = reach.map(|r| {
            let reason = cl.reason.clone().or_else(|| {
                let f = r.factor(h.snap.way);
                // Said when the factor shows (1,00 is no weighting).
                (f < 0.995).then(|| {
                    format!(
                        "réseau de {} km nordiques seulement (moins des {LOOP_KM} km d'une boucle de 2 h) : score × {}",
                        km_fr(r.km(h.snap.way)),
                        format!("{f:.2}").replace('.', ",")
                    )
                })
            });
            r.served(h.snap.way, reason)
        });
        out.push(Depart {
            start: Start {
                id: String::new(),
                name,
                phrase,
                phrase_en,
                lon: round7(pos.lon),
                lat: round7(pos.lat),
                provenance,
                // The walk from the car park to the itinerary (SRN, #168).
                access_m: matches!(place.kind, PlaceKind::SrnArea | PlaceKind::Traverse)
                    .then(|| h.snap.dist.round()),
            },
            method: cl.method,
            score: (cl.score * 100.0).round() / 100.0,
            points: ms.iter().map(|c| c.point.url.clone()).collect(),
            network,
        });
    }
    // Two starts named alike: the later ones take the name of their trail, else their
    // distance and direction from the main start.
    for i in 1..out.len() {
        if !out[..i]
            .iter()
            .any(|o| o.start.phrase == out[i].start.phrase)
        {
            continue;
        }
        let trail = net
            .nearest(local.xy(out[i].start.position()))
            .and_then(|s| ways[s.way].name.clone())
            .clone()
            .filter(|w| {
                !out[..i]
                    .iter()
                    .any(|o| o.start.name.ends_with(&format!(" — {w}")))
            });
        let (t_fr, t_en) = place.trail();
        let (fr, en) = match trail {
            Some(w) => (format!("{t_fr} « {w} »"), format!("« {w} » {t_en}")),
            None => {
                let (x, y) = local.xy(out[i].start.position());
                let (x0, y0) = local.xy(out[0].start.position());
                let (dir_fr, dir_en) = direction(x - x0, y - y0);
                let km = d2((x, y), (x0, y0)) / 1000.0;
                (
                    format!("{} km {dir_fr}", format!("{km:.1}").replace('.', ",")),
                    format!("{km:.1} km {dir_en}"),
                )
            }
        };
        let o = &mut out[i].start;
        o.name = format!("{} — {fr}", o.name);
        o.phrase = format!("{} ({fr})", o.phrase);
        o.phrase_en = Some(format!(
            "{} ({en})",
            o.phrase_en.clone().unwrap_or_else(|| o.phrase.clone())
        ));
    }
    out
}

/// Weight of a pass desk of the alpine lifts (its name says so).
pub const ALPINE_DESK_WEIGHT: f64 = 1.0;

/// Whether a desk name is that of the alpine lifts.
pub fn alpine(name: &str) -> bool {
    let n = name.to_lowercase();
    [
        "remont",
        "téléski",
        "teleski",
        "télésiège",
        "telesiege",
        "téléphérique",
        "telepherique",
    ]
    .iter()
    .any(|k| n.contains(k))
}

/// Name of a start from the name of its point: a car park named after a place is said
/// « Parking <name> ».
fn named_as(kind: &str, name: &str) -> String {
    let n = name.to_lowercase();
    let says_park = n.contains("parking")
        || n.contains("park")
        || n.split_whitespace().next().is_some_and(|w| {
            w.len() <= 3 && w.starts_with('p') && w[1..].chars().all(|c| c.is_ascii_digit())
        });
    if kind == "parking" && !says_park {
        format!("Parking {name}")
    } else {
        name.to_owned()
    }
}

/// Direction of a vector (east, north), French and English.
fn direction(dx: f64, dy: f64) -> (&'static str, &'static str) {
    const DIRS: [(&str, &str); 8] = [
        ("au nord", "north"),
        ("au nord-est", "north-east"),
        ("à l'est", "east"),
        ("au sud-est", "south-east"),
        ("au sud", "south"),
        ("au sud-ouest", "south-west"),
        ("à l'ouest", "west"),
        ("au nord-ouest", "north-west"),
    ];
    let bearing = dx.atan2(dy).to_degrees().rem_euclid(360.0);
    DIRS[((bearing + 22.5) / 45.0) as usize % 8]
}

/// Weight of a point for the starts: that of its kind, less for an alpine desk.
fn weight(p: &Point) -> f64 {
    match kind_rule(&p.kind) {
        Some(_) if p.kind == "ticket" && p.name.as_deref().is_some_and(alpine) => {
            ALPINE_DESK_WEIGHT
        }
        Some((w, _)) => w,
        None => 0.0,
    }
}

fn fallback(d: &Domain, net: &Net, local: &Local, points: &[&Point]) -> Depart {
    let nearest = |kinds: &dyn Fn(&Point) -> bool| {
        points
            .iter()
            .filter(|p| kinds(p))
            .filter_map(|p| net.nearest(local.xy(p.pos)).map(|s| (p, s)))
            .min_by(|a, b| a.1.dist.total_cmp(&b.1.dist))
    };
    let near =
        nearest(&|p: &Point| kind_rule(&p.kind).is_some() && !(p.kind == "parking" && private(p)))
            .filter(|(_, s)| s.dist <= NEAR_MAX_M);
    let village = nearest(&|p: &Point| matches!(p.kind.as_str(), "tourist_office" | "ski_school"));
    let (pos, method, provenance, score, urls) = match (near, village) {
        (Some((p, s)), _) => (
            local.lonlat(s.xy),
            Method::Near,
            format!(
                "déduit (X9) : point du réseau le plus proche de {} ({}, à {:.0} m du réseau)",
                p.kind, p.url, s.dist
            ),
            0.0,
            vec![p.url.clone()],
        ),
        (None, Some((p, s))) => (
            local.lonlat(s.xy),
            Method::Village,
            format!(
                "déduit (X9) : point du réseau le plus proche du village ({}, {})",
                p.kind, p.url
            ),
            0.0,
            vec![p.url.clone()],
        ),
        (None, None) => (
            d.point,
            Method::Centre,
            "point représentatif du domaine (recensement P3 : point du réseau le plus proche du barycentre) ; aucun foyer, accueil ni parking OSM près des pistes".to_owned(),
            0.0,
            Vec::new(),
        ),
    };
    Depart {
        start: Start {
            id: String::new(),
            name: format!("Départ des pistes ({})", d.name),
            phrase: format!("le départ des pistes du domaine {}", d.name),
            phrase_en: Some(format!("the {} area trailhead", d.name)),
            lon: round7(pos.lon),
            lat: round7(pos.lat),
            provenance,
            access_m: None,
        },
        method,
        score,
        points: urls,
        network: None,
    }
}

/// Starts of every domain, from the pistes of the planet extract (geometry and routing
/// graph of the domain ways, [`Reach`]).
pub fn all_departs(
    ds: &[Domain],
    domain_ways: &DomainWays,
    pistes: &PisteSet,
    points: &[Point],
) -> BTreeMap<String, Vec<Depart>> {
    let mut by_domain: BTreeMap<&str, Vec<&Point>> = BTreeMap::new();
    for p in points {
        for d in &p.domains {
            by_domain.entry(d.as_str()).or_default().push(p);
        }
    }
    let by_way: HashMap<i64, &skimap_osm::pistes::Piste> =
        pistes.pistes.iter().map(|p| (p.way_id, p)).collect();
    ds.iter()
        .map(|d| {
            let own: Vec<skimap_osm::pistes::Piste> = domain_ways
                .get(&d.id)
                .into_iter()
                .flatten()
                .filter_map(|w| by_way.get(w).map(|p| (*p).clone()))
                .collect();
            let ways: Vec<Way> = own.iter().map(Way::of).collect();
            let reach = Reach::of(&pistes.subset(own));
            let pts = by_domain.get(d.id.as_str()).cloned().unwrap_or_default();
            (d.id.clone(), domain_departs(d, &ways, &pts, Some(&reach)))
        })
        .collect()
}

/// Notes of `departs.json`.
pub fn notes() -> Vec<String> {
    vec![
        format!(
            "Départs déduits pour chaque domaine (X9) : foyers nordiques, accueils (ticket), écoles de ski et parkings non privés OSM rattachés au domaine (pois.geojson), à moins de 300 m du réseau ; chaque point est ramené au point du réseau le plus proche ; poids foyer 5, accueil 4, école 2, parking 2 + places/50 (≤ 2) ; +1 près d'une extrémité du réseau (< {TRAILHEAD_M:.0} m) ; points à moins de {CLUSTER_M:.0} m réunis ; au plus {MAX_STARTS} départs, chacun au moins {:.0} % du score du premier.",
            MIN_SHARE * 100.0
        ),
        format!("Sans tel point : point du réseau le plus proche du foyer, de l'accueil ou du parking le plus proche du réseau (à moins de {NEAR_MAX_M:.0} m, method = near), sinon de l'office de tourisme ou de l'école de ski (method = village), sinon point représentatif du recensement (method = centre). Caisses des remontées mécaniques (nom) : poids {ALPINE_DESK_WEIGHT}."),
        "Réseau : ways du domaine (domaines_ways.json) lus dans l'extrait OpenSnowMap planet_pistes (ODbL).".to_owned(),
        format!("Parties du réseau (#166) : composantes connexes du graphe de routage des ways du domaine (celui des packs : raccords de moins de {CONNECTOR_MAX_M:.0} m, croisements), pesées en km nordiques (les pistes de randonnée ou raquettes relient sans compter). Chaque point pèse son score × km de sa partie / {LOOP_KM} (au plus 1) : une boucle de la sortie par défaut (2 h) demande environ {LOOP_KM} km. La plus grande partie a toujours un départ : le meilleur groupe de points qui la dessert, sinon le point de cette partie le plus proche d'un foyer, accueil ou parking (à moins de {NEAR_MAX_M:.0} m) ; il passe en tête quand le premier départ dessert moins de {LOOP_KM} km, sinon en dernier. Un départ dont la partie atteint {LOOP_KM} km passe en tête devant un premier départ qui n'y arrive pas. network : km de la partie desservie, km du domaine, plus grande partie ou non, raison."),
    ]
}

/// What [`build`] wrote.
#[derive(Debug, Clone)]
pub struct DepartsOutput {
    /// `departs.json`.
    pub path: PathBuf,
    /// Domains per method of their main start.
    pub by_method: BTreeMap<String, usize>,
    /// Starts written.
    pub starts: usize,
    /// SRN areas and traverses per method of their best start (X9b).
    pub srn_by_method: BTreeMap<String, usize>,
    /// SRN starts written.
    pub srn_starts: usize,
    /// Car parks near SRN areas left out as closed in winter.
    pub srn_winter_closed: usize,
}

/// Starts of the SRN areas (X9b) given to [`write()`].
#[derive(Debug, Clone, Default)]
pub struct SrnStarts {
    /// Area id → starts, best first.
    pub areas: BTreeMap<String, Vec<Depart>>,
    /// Car parks left out as closed in winter.
    pub winter_closed: usize,
}

fn method_name(m: Method) -> String {
    serde_json::to_value(m)
        .ok()
        .and_then(|v| v.as_str().map(str::to_owned))
        .unwrap_or_default()
}

/// Geometry of the trail ways of the planet extract.
pub fn planet_ways(planet: &skimap_osm::extract::Planet) -> HashMap<i64, Way> {
    planet
        .set
        .pistes
        .iter()
        .map(|p| (p.way_id, Way::of(p)))
        .collect()
}

/// Writes `departs.json` and `departs.geojson` in `dir` from the census, the points of
/// `dir/pois.geojson` and the pistes of the planet extract.
pub fn write(
    dir: &Path,
    ds: &[Domain],
    domain_ways: &DomainWays,
    pistes: &PisteSet,
    srn: SrnStarts,
    osm_valid_at: Option<String>,
) -> Result<DepartsOutput> {
    let pois_path = dir.join("pois.geojson");
    let b = std::fs::read(&pois_path).map_err(|e| {
        StationsError::Input(format!(
            "{} ({e}) : lancer d'abord `skimap stations`",
            pois_path.display()
        ))
    })?;
    let v: Value = serde_json::from_slice(&b)
        .map_err(|e| StationsError::format("pois.geojson", e.to_string()))?;
    let points = read_points(&v);
    let domains = all_departs(ds, domain_ways, pistes, &points);
    let mut by_method: BTreeMap<String, usize> = BTreeMap::new();
    let mut feats = Vec::new();
    let mut starts = 0;
    for (id, list) in &domains {
        if let Some(m) = list.first() {
            *by_method.entry(method_name(m.method)).or_default() += 1;
        }
        for (rank, s) in list.iter().enumerate() {
            starts += 1;
            feats.push(feature(
                point(s.start.position()),
                json!({"domain": id, "rank": rank + 1, "name": s.start.name, "method": s.method, "score": s.score, "points": s.points}),
            ));
        }
    }
    let mut srn_by_method: BTreeMap<String, usize> = BTreeMap::new();
    let mut srn_starts = 0;
    for (id, list) in &srn.areas {
        if let Some(m) = list.first() {
            *srn_by_method.entry(method_name(m.method)).or_default() += 1;
        }
        for (rank, s) in list.iter().enumerate() {
            srn_starts += 1;
            feats.push(feature(
                point(s.start.position()),
                json!({"srn": id, "rank": rank + 1, "name": s.start.name, "method": s.method, "score": s.score, "points": s.points}),
            ));
        }
    }
    let mut all_notes = notes();
    if !srn.areas.is_empty() {
        all_notes.extend(crate::departs_srn::notes());
    }
    let path = dir.join(FILE);
    write_json(
        &path,
        &DepartsFile {
            generated_at: chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
            osm_valid_at,
            notes: all_notes,
            domains,
            srn: srn.areas,
        },
    )?;
    write_geojson(&dir.join(GEOJSON), &feats)?;
    Ok(DepartsOutput {
        path,
        by_method,
        starts,
        srn_by_method,
        srn_starts,
        srn_winter_closed: srn.winter_closed,
    })
}

/// Builds the starts of every domain: census, `pois.geojson` of the stations layer, trail
/// geometry of the planet extract; then those of the SRN areas when the SRN layer exists
/// (`srn/srn_areas.geojson`, X9b), from the local OSM index.
pub fn build(
    layout: &DataLayout,
    dl: &skimap_common::download::Downloader,
) -> Result<DepartsOutput> {
    let census_dir = layout.france_layer_dir(crate::CENSUS_LAYER)?;
    let (ds, ways) = crate::domains::load(&census_dir)?;
    let planet = skimap_osm::extract::load_planet(dl)?;
    let srn = crate::departs_srn::build(layout, dl)?;
    let dir = layout.france_layer_dir(crate::LAYER)?;
    write(&dir, &ds, &ways, &planet.set, srn, planet.valid_at.clone())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn domain() -> Domain {
        let v = json!({"features": [{"geometry": {"type": "MultiPolygon", "coordinates": [[[
            [6.0, 46.0], [6.03, 46.0], [6.03, 46.02], [6.0, 46.02], [6.0, 46.0]
        ]]]}, "properties": {"id": "net-1", "name": "Les Plans", "lon": 6.01, "lat": 46.01,
            "commune": {"name": "C", "insee": "25000", "departement": "25"}}}]});
        crate::domains::parse(&v).unwrap().remove(0)
    }

    fn way(name: &str, ids: &[(i64, f64, f64)]) -> Way {
        Way {
            name: Some(name.into()),
            points: ids
                .iter()
                .map(|(i, lon, lat)| (*i, LonLat::new(*lon, *lat)))
                .collect(),
        }
    }

    fn pt(kind: &str, name: Option<&str>, lon: f64, lat: f64) -> Point {
        Point {
            kind: kind.into(),
            name: name.map(str::to_owned),
            pos: LonLat::new(lon, lat),
            capacity: None,
            access: None,
            url: format!("https://www.openstreetmap.org/node/{}", (lon * 1e4) as i64),
            domains: vec!["net-1".into()],
            bonus: 0.0,
        }
    }

    // Two trails: one from the lodge (west, dead end at 6.001) east to a junction, one loop
    // far east.
    fn ways() -> Vec<Way> {
        vec![
            way(
                "Les Gentianes",
                &[(1, 6.001, 46.01), (2, 6.005, 46.01), (3, 6.01, 46.01)],
            ),
            way(
                "Le Crêt",
                &[(3, 6.01, 46.01), (4, 6.02, 46.01), (5, 6.028, 46.01)],
            ),
        ]
    }

    #[test]
    fn lodge_and_car_park_make_one_start_on_the_network() {
        let d = domain();
        let lodge = pt("foyer", Some("Foyer des Plans"), 6.0012, 46.0102);
        let park = pt("parking", None, 6.0008, 46.0098);
        let far = pt("parking", None, 6.028, 46.0102);
        let pts = vec![&lodge, &park, &far];
        let s = domain_departs(&d, &ways(), &pts, None);
        assert_eq!(s[0].method, Method::Points);
        assert_eq!(s[0].start.name, "Foyer des Plans (Les Plans)");
        assert_eq!(s[0].start.phrase, "Foyer des Plans");
        // On the first trail, beside the lodge (projected on the segment).
        assert_eq!((s[0].start.lon, s[0].start.lat), (6.0012, 46.01));
        assert_eq!(s[0].points.len(), 2);
        // The far car park is a second start at the other dead end, named by its kind.
        assert_eq!(s.len(), 2);
        assert_eq!(s[1].start.phrase, "le parking du domaine Les Plans");
        assert_eq!(
            s[1].start.phrase_en.as_deref(),
            Some("the Les Plans area car park")
        );
        assert_eq!((s[1].start.lon, s[1].start.lat), (6.028, 46.01));
        assert_eq!(s[0].start.id, "depart-1");
    }

    #[test]
    fn private_and_far_points_are_ignored_and_the_village_is_the_fallback() {
        let d = domain();
        let mut private = pt("parking", None, 6.001, 46.0101);
        private.access = Some("private".into());
        let far = pt("foyer", None, 6.01, 46.016); // ~670 m from the trails
        let office = pt("tourist_office", None, 6.0205, 46.0115);
        let pts = vec![&private, &far, &office];
        let s = domain_departs(&d, &ways(), &pts, None);
        assert_eq!(s.len(), 1);
        // The far lodge, on the nearest point of the network.
        assert_eq!(s[0].method, Method::Near);
        assert_eq!((s[0].start.lon, s[0].start.lat), (6.01, 46.01));
        // Only the private car park and the village.
        let s = domain_departs(&d, &ways(), &[&private, &office], None);
        assert_eq!(s[0].method, Method::Village);
        assert_eq!((s[0].start.lon, s[0].start.lat), (6.0205, 46.01));
        // Nothing at all: the census point.
        let s = domain_departs(&d, &ways(), &[], None);
        assert_eq!(s[0].method, Method::Centre);
        assert_eq!((s[0].start.lon, s[0].start.lat), (6.01, 46.01));
    }

    #[test]
    fn same_names_take_the_trail_name() {
        let d = domain();
        let a = pt("parking", None, 6.0011, 46.0101);
        let b = pt("parking", None, 6.0279, 46.0101);
        let s = domain_departs(&d, &ways(), &[&a, &b], None);
        assert_eq!(s.len(), 2);
        assert_ne!(s[0].start.phrase, s[1].start.phrase);
        assert!(
            s[1].start.phrase.contains("piste « "),
            "{}",
            s[1].start.phrase
        );
    }

    #[test]
    fn alpine_desks_weigh_less_and_place_names_say_parking() {
        assert!(alpine("Caisse Remontées Mécaniques"));
        assert!(!alpine("Foyer nordique"));
        assert_eq!(named_as("parking", "Les Tourres"), "Parking Les Tourres");
        assert_eq!(named_as("parking", "P3"), "P3");
        assert_eq!(named_as("parking", "Parking du Riou"), "Parking du Riou");
        assert_eq!(named_as("foyer", "Les Tourres"), "Les Tourres");
        assert_eq!(direction(1.0, 1.0), ("au nord-est", "north-east"));
        assert_eq!(direction(0.0, -1.0), ("au sud", "south"));
        // An alpine desk on the network is beaten by a nordic lodge 2 km away.
        let d = domain();
        let desk = pt(
            "ticket",
            Some("Caisse remontées mécaniques"),
            6.0011,
            46.0101,
        );
        let lodge = pt("foyer", None, 6.0279, 46.0101);
        let s = domain_departs(&d, &ways(), &[&desk, &lodge], None);
        assert_eq!(s[0].start.phrase, "le foyer nordique du domaine Les Plans");
    }

    #[test]
    fn same_names_without_trail_take_the_direction() {
        let d = domain();
        let mut w = ways();
        for x in &mut w {
            x.name = None;
        }
        let a = pt("parking", None, 6.0011, 46.0101);
        let b = pt("parking", None, 6.0279, 46.0101);
        let s = domain_departs(&d, &w, &[&a, &b], None);
        assert_eq!(s.len(), 2);
        assert_eq!(s[1].start.name, "Parking (Les Plans) — 2,1 km à l'est");
        assert_eq!(
            s[1].start.phrase_en.as_deref(),
            Some("the Les Plans area car park (2.1 km east)")
        );
    }

    /// The two trails of [`ways`] as two parts of the network: 1 km and 20 km.
    fn split_reach() -> Reach {
        Reach {
            way_component: vec![Some(0), Some(1)],
            component_km: vec![1.0, 20.0],
        }
    }

    #[test]
    fn a_start_on_a_short_part_weighs_less() {
        let d = domain();
        let lodge = pt("foyer", Some("Foyer des Plans"), 6.0012, 46.0102);
        let park = pt("parking", None, 6.028, 46.0102);
        // Without parts: the lodge first.
        let s = domain_departs(&d, &ways(), &[&lodge, &park], None);
        assert_eq!(s[0].start.phrase, "Foyer des Plans");
        assert_eq!(s[0].network, None);
        // On a part of a twentieth of a loop, the lodge weighs a twentieth: the car park on
        // the 20 km part is the only start (the lodge under a quarter of its score).
        let r = Reach {
            way_component: vec![Some(0), Some(1)],
            component_km: vec![LOOP_KM / 20.0, 20.0],
        };
        let s = domain_departs(&d, &ways(), &[&lodge, &park], Some(&r));
        assert_eq!(s.len(), 1);
        assert_eq!((s[0].start.lon, s[0].start.lat), (6.028, 46.01));
        let n = s[0].network.as_ref().unwrap();
        assert_eq!((n.km, n.main), (20.0, true));
        assert!((n.domain_km - (20.0 + LOOP_KM / 20.0)).abs() < 0.1, "{n:?}");
        assert_eq!(n.reason, None);
    }

    #[test]
    fn the_largest_part_always_gets_a_start() {
        let d = domain();
        // Both points more than 300 m from the 20 km part (the lodge is on the 1 km one).
        let lodge = pt("foyer", Some("Foyer des Plans"), 6.0012, 46.0102);
        let park = pt("parking", Some("Les Prés"), 6.02, 46.0155);
        let s = domain_departs(&d, &ways(), &[&lodge, &park], Some(&split_reach()));
        // The lodge alone serves 1 km, less than a default loop: the point of the largest
        // part nearest to the best of them (the lodge, 690 m away) comes first; the lodge
        // on its 1 km part, a tenth of its score, falls under a quarter of it.
        assert_eq!(s.len(), 1, "{s:#?}");
        assert_eq!(s[0].method, Method::Near);
        assert_eq!(s[0].start.name, "Foyer des Plans (Les Plans)");
        assert_eq!((s[0].start.lon, s[0].start.lat), (6.01, 46.01));
        assert!(
            s[0].start
                .provenance
                .contains("réseau principal le plus proche de foyer"),
            "{}",
            s[0].start.provenance
        );
        let n = s[0].network.as_ref().unwrap();
        assert!(n.main);
        let why = n.reason.as_deref().unwrap();
        assert!(
            why.starts_with("réseau principal du domaine (20,0 km nordiques sur 21,0)"),
            "{why}"
        );
        assert!(why.contains("placé en tête"), "{why}");
        // Without the lodge: the car park, 610 m away.
        let s = domain_departs(&d, &ways(), &[&park], Some(&split_reach()));
        assert_eq!(s[0].start.name, "Parking Les Prés (Les Plans)");
        assert_eq!((s[0].start.lon, s[0].start.lat), (6.02, 46.01));
    }

    #[test]
    fn a_start_serving_a_loop_comes_before_one_that_does_not() {
        let d = domain();
        let lodge = pt("foyer", Some("Foyer des Plans"), 6.0012, 46.0102);
        let park = pt("parking", None, 6.028, 46.0102);
        // 80 % and 120 % of a loop: the lodge still scores more, the car park comes first.
        let r = Reach {
            way_component: vec![Some(0), Some(1)],
            component_km: vec![LOOP_KM * 0.8, LOOP_KM * 1.2],
        };
        let s = domain_departs(&d, &ways(), &[&lodge, &park], Some(&r));
        assert_eq!(s.len(), 2);
        assert_eq!((s[0].start.lon, s[0].start.lat), (6.028, 46.01));
        assert!(
            s[0].network
                .as_ref()
                .unwrap()
                .reason
                .as_deref()
                .unwrap()
                .contains("placé en tête")
        );
        assert_eq!(s[1].start.phrase, "Foyer des Plans");
    }

    #[test]
    fn reads_pois_geojson() {
        let v = json!({"features": [{"type": "Feature", "geometry": {"type": "Point", "coordinates": [6.1, 46.1]},
            "properties": {"kind": "parking", "name": " ", "capacity": "120", "access": "yes", "url": "u",
                "domains": [{"id": "net-1", "distance_m": 3.0}]}}]});
        let p = read_points(&v);
        assert_eq!(p.len(), 1);
        assert_eq!(p[0].name, None);
        assert_eq!(p[0].capacity, Some(120.0));
        assert_eq!(p[0].domains, vec!["net-1".to_owned()]);
    }
}
