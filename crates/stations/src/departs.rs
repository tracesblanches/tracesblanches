//! Trail starts of every census domain (X9, #143): where a skier arriving by car begins.
//!
//! No source publishes the starts. They are deduced, for every domain alike, from:
//! - the OSM points of the station cards (`pois.geojson`): nordic lodges (`foyer`), pass
//!   desks (`ticket`), ski schools and car parks (not private) near the domain;
//! - the trail network of the domain (OpenSnowMap planet extract, ways of
//!   `domaines_ways.json`): each point is snapped to the nearest point of the network, a point
//!   near a dead end of the network (trailhead) counts more;
//! - points within [`CLUSTER_M`] of each other make one start (a car park beside a lodge).
//!
//! The best [`MAX_STARTS`] starts are kept, main first. A domain without any such point
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
    /// Census representative point (on the network).
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
}

impl DepartsFile {
    /// Reads `departs.json`.
    pub fn read(path: &Path) -> Result<Self> {
        let b = std::fs::read(path).map_err(|e| skimap_common::Error::io(path, e))?;
        serde_json::from_slice(&b).map_err(|e| StationsError::format(FILE, e.to_string()))
    }
}

struct Net {
    /// Vertices: position (local metres) and whether a dead end of the network.
    vertices: Vec<((f64, f64), bool)>,
    /// Segments: ends (local metres) and way index.
    segments: Vec<((f64, f64), (f64, f64), usize)>,
}

/// A point snapped to the network.
#[derive(Debug, Clone, Copy)]
struct Snap {
    /// Nearest point of the network (local metres).
    xy: (f64, f64),
    /// Way of that point.
    way: usize,
    /// Distance, m.
    dist: f64,
}

fn d2(a: (f64, f64), b: (f64, f64)) -> f64 {
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
    fn new(ways: &[Way], local: &Local) -> Self {
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

    fn nearest(&self, p: (f64, f64)) -> Option<Snap> {
        self.segments
            .iter()
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

fn private(p: &Point) -> bool {
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

/// Starts of one domain from its points (those of `pois.geojson` attached to it), its
/// trail ways and, for the fallbacks, its representative point.
pub fn domain_departs(d: &Domain, ways: &[Way], points: &[&Point]) -> Vec<Depart> {
    let local = Local::new(d.point);
    let net = Net::new(ways, &local);
    let mut cands: Vec<Candidate> = Vec::new();
    for p in points {
        let Some((_, max)) = kind_rule(&p.kind) else {
            continue;
        };
        let w = weight(p);
        if p.kind == "parking" && private(p) {
            continue;
        }
        let xy = local.xy(p.pos);
        let Some(snap) = net.nearest(xy) else {
            continue;
        };
        let dist = snap.dist;
        if dist > max {
            continue;
        }
        let mut score = w * (1.0 - 0.5 * dist / max);
        if p.kind == "parking" {
            score += p.capacity.map_or(0.0, |c| (c / 50.0).min(2.0));
        }
        if net.dead_end_within(xy, TRAILHEAD_M) {
            score += 1.0;
        }
        cands.push(Candidate {
            point: p,
            xy,
            snap,
            score,
        });
    }
    cands.sort_by(|a, b| {
        b.score
            .total_cmp(&a.score)
            .then(a.point.url.cmp(&b.point.url))
    });
    // Greedy clusters around the best points.
    let mut clusters: Vec<(usize, f64, Vec<usize>)> = Vec::new();
    for (i, c) in cands.iter().enumerate() {
        match clusters
            .iter_mut()
            .find(|(h, _, _)| d2(cands[*h].xy, c.xy) <= CLUSTER_M)
        {
            Some(cl) => {
                cl.1 += 0.5 * c.score;
                cl.2.push(i);
            }
            None => clusters.push((i, c.score, vec![i])),
        }
    }
    clusters.sort_by(|a, b| b.1.total_cmp(&a.1));
    let best = clusters.first().map_or(0.0, |c| c.1);
    clusters.retain(|c| c.1 >= MIN_SHARE * best);
    clusters.truncate(MAX_STARTS);

    let mut out: Vec<Depart> = Vec::new();
    for (head, score, members) in &clusters {
        let ms: Vec<&Candidate> = members.iter().map(|i| &cands[*i]).collect();
        // Named after the best named lodge or desk, else the best named point, else the
        // kind of the head.
        let named = ms.iter().filter(|c| c.point.name.is_some()).max_by(|a, b| {
            let r = |c: &Candidate| weight(c.point);
            r(a).total_cmp(&r(b)).then(b.score.total_cmp(&a.score))
        });
        let h = &cands[*head];
        let way_name = ways[h.snap.way].name.clone();
        let (kname, kphrase, ken) = kind_fr(&h.point.kind);
        let (name, phrase, phrase_en) = match named.and_then(|c| c.point.name.clone()) {
            Some(n) => {
                let n = named_as(&named.map_or("", |c| c.point.kind.as_str()), &n);
                (format!("{n} ({})", d.name), n.clone(), None)
            }
            None => (
                format!("{kname} ({})", d.name),
                format!("{kphrase} du domaine {}", d.name),
                Some(format!("the {} area {ken}", d.name)),
            ),
        };
        let pos = local.lonlat(h.snap.xy);
        out.push(Depart {
            start: Start {
                id: String::new(),
                name,
                phrase,
                phrase_en,
                lon: round7(pos.lon),
                lat: round7(pos.lat),
                provenance: format!(
                    "déduit (X9) : point du réseau le plus proche de {} ({}){}",
                    ms.iter()
                        .map(|c| c.point.kind.as_str())
                        .collect::<Vec<_>>()
                        .join(", "),
                    h.point.url,
                    way_name
                        .map(|w| format!(", piste « {w} »"))
                        .unwrap_or_default()
                ),
            },
            method: Method::Points,
            score: (score * 100.0).round() / 100.0,
            points: ms.iter().map(|c| c.point.url.clone()).collect(),
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
        let (fr, en) = match trail {
            Some(w) => (format!("piste « {w} »"), format!("« {w} » trail")),
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
    if out.is_empty() {
        out.push(fallback(d, &net, &local, points));
    }
    for (i, o) in out.iter_mut().enumerate() {
        o.start.id = format!("depart-{}", i + 1);
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
        },
        method,
        score,
        points: urls,
    }
}

/// Starts of every domain.
pub fn all_departs(
    ds: &[Domain],
    domain_ways: &DomainWays,
    geometry: &HashMap<i64, Way>,
    points: &[Point],
) -> BTreeMap<String, Vec<Depart>> {
    let mut by_domain: BTreeMap<&str, Vec<&Point>> = BTreeMap::new();
    for p in points {
        for d in &p.domains {
            by_domain.entry(d.as_str()).or_default().push(p);
        }
    }
    ds.iter()
        .map(|d| {
            let ways: Vec<Way> = domain_ways
                .get(&d.id)
                .into_iter()
                .flatten()
                .filter_map(|w| geometry.get(w).cloned())
                .collect();
            let pts = by_domain.get(d.id.as_str()).cloned().unwrap_or_default();
            (d.id.clone(), domain_departs(d, &ways, &pts))
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
}

/// Geometry of the trail ways of the planet extract.
pub fn planet_ways(planet: &skimap_osm::extract::Planet) -> HashMap<i64, Way> {
    planet
        .set
        .pistes
        .iter()
        .map(|p| {
            (
                p.way_id,
                Way {
                    name: p.way_name.clone(),
                    points: p.points.clone(),
                },
            )
        })
        .collect()
}

/// Writes `departs.json` and `departs.geojson` in `dir` from the census, the points of
/// `dir/pois.geojson` and the trail geometry.
pub fn write(
    dir: &Path,
    ds: &[Domain],
    domain_ways: &DomainWays,
    geometry: &HashMap<i64, Way>,
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
    let domains = all_departs(ds, domain_ways, geometry, &points);
    let mut by_method: BTreeMap<String, usize> = BTreeMap::new();
    let mut feats = Vec::new();
    let mut starts = 0;
    for (id, list) in &domains {
        if let Some(m) = list.first() {
            *by_method
                .entry(
                    serde_json::to_value(m.method)
                        .ok()
                        .and_then(|v| v.as_str().map(str::to_owned))
                        .unwrap_or_default(),
                )
                .or_default() += 1;
        }
        for (rank, s) in list.iter().enumerate() {
            starts += 1;
            feats.push(feature(
                point(s.start.position()),
                json!({"domain": id, "rank": rank + 1, "name": s.start.name, "method": s.method, "score": s.score, "points": s.points}),
            ));
        }
    }
    let path = dir.join(FILE);
    write_json(
        &path,
        &DepartsFile {
            generated_at: chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
            osm_valid_at,
            notes: notes(),
            domains,
        },
    )?;
    write_geojson(&dir.join(GEOJSON), &feats)?;
    Ok(DepartsOutput {
        path,
        by_method,
        starts,
    })
}

/// Builds the starts of every domain: census, `pois.geojson` of the stations layer, trail
/// geometry of the planet extract.
pub fn build(
    layout: &DataLayout,
    dl: &skimap_common::download::Downloader,
) -> Result<DepartsOutput> {
    let census_dir = layout.france_layer_dir(crate::CENSUS_LAYER)?;
    let (ds, ways) = crate::domains::load(&census_dir)?;
    let planet = skimap_osm::extract::load_planet(dl)?;
    let geometry = planet_ways(&planet);
    let dir = layout.france_layer_dir(crate::LAYER)?;
    write(&dir, &ds, &ways, &geometry, planet.valid_at.clone())
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
        let s = domain_departs(&d, &ways(), &pts);
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
        let s = domain_departs(&d, &ways(), &pts);
        assert_eq!(s.len(), 1);
        // The far lodge, on the nearest point of the network.
        assert_eq!(s[0].method, Method::Near);
        assert_eq!((s[0].start.lon, s[0].start.lat), (6.01, 46.01));
        // Only the private car park and the village.
        let s = domain_departs(&d, &ways(), &[&private, &office]);
        assert_eq!(s[0].method, Method::Village);
        assert_eq!((s[0].start.lon, s[0].start.lat), (6.0205, 46.01));
        // Nothing at all: the census point.
        let s = domain_departs(&d, &ways(), &[]);
        assert_eq!(s[0].method, Method::Centre);
        assert_eq!((s[0].start.lon, s[0].start.lat), (6.01, 46.01));
    }

    #[test]
    fn same_names_take_the_trail_name() {
        let d = domain();
        let a = pt("parking", None, 6.0011, 46.0101);
        let b = pt("parking", None, 6.0279, 46.0101);
        let s = domain_departs(&d, &ways(), &[&a, &b]);
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
        let s = domain_departs(&d, &ways(), &[&desk, &lodge]);
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
        let s = domain_departs(&d, &w, &[&a, &b]);
        assert_eq!(s.len(), 2);
        assert_eq!(s[1].start.name, "Parking (Les Plans) — 2,1 km à l'est");
        assert_eq!(
            s[1].start.phrase_en.as_deref(),
            Some("the Les Plans area car park (2.1 km east)")
        );
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
