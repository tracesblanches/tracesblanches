//! Pistes known to exist but absent from OSM, traced on the ground (G1, #117).
//!
//! Two lists name pistes that OSM lacks:
//!
//! - the operator's bulletin (`status/`): the rows the join to OSM left unmatched
//!   (`stats.join.<source>_unmatched`), with their printed length, colour and activity;
//! - the official plan (`plan_pistes/`): the legend lines with no OSM piste of their name
//!   (`stats.not_transcribed`), and the traces already estimated from the plan
//!   (`certainty` = `incertain`), copied as they are.
//!
//! A piste not yet traced gets a geometry from, in order:
//!
//! 1. **Geotrek** ([`match_trek`]): a published itinerary near the site whose name holds
//!    every distinctive word of the piste's name ([`distinctive`]), of a winter practice
//!    of the same family (ski, or snowshoe / walking for a pedestrian track), and whose
//!    length agrees with the printed one (ratio within [`LENGTH_RATIO`], an out-and-back
//!    piste counting its itinerary twice). Method `geotrek`.
//! 2. **IGN roads and paths** ([`route_liaison`]): a link named after its ends
//!    (« Liaison A - B », « Liaison B par la route ») is routed on the BD TOPO sections
//!    between the toponyms of its ends (one end only: the place the site is named after),
//!    on roads only when its name says « par la route », preferring paths otherwise, and
//!    kept when its length agrees with the printed one. Method `estimated_route`.
//!
//! Everything written is `incertain` (dotted on the map, « Tracé incertain » on the
//! card). Each feature carries the licences of what it is made of (`licences`): the
//! source naming the piste and the source of its geometry. A published pack keeps only
//! the features whose every licence allows it ([`skimap_common::licence`]).

use std::cmp::Ordering;
use std::collections::{BTreeMap, BinaryHeap, HashMap};
use std::path::Path as FsPath;
use std::time::Duration;

use serde::Serialize;
use serde_json::{Value, json};
use skimap_common::crs::{from_lambert93, to_lambert93};
use skimap_common::download::{Downloader, Fetched, Request};
use skimap_common::geom::{BBox, LonLat};
use skimap_common::meta::{FileMeta, LayerMeta, SourceRef};
use skimap_common::output;

use crate::error::{Error, Result};
use crate::estimate::{Path, plausible_name};
use crate::labels::fold;
use crate::osmref::Landmark;
use crate::warp::{P2, dist};

/// Pipeline layer (directory and file stem).
pub const LAYER: &str = "pistes_estimees";
/// Accepted ratio between the traced length and the printed one (both ways).
pub const LENGTH_RATIO: f64 = 1.6;
/// Smallest score written.
pub const MIN_SCORE: f64 = 0.4;
/// Largest distance between a toponym (or the commune centre) and the network, m.
pub const MAX_SNAP_M: f64 = 500.0;
/// Cost factor of a road section for a link that does not say « par la route ».
pub const ROAD_PENALTY: f64 = 3.0;
/// Licence of the BD TOPO.
pub const BDTOPO_LICENCE: &str = "Licence Ouverte Etalab 2.0";
/// Cache lifetime of the Geotrek answers.
pub const GEOTREK_MAX_AGE: Duration = Duration::from_secs(30 * 86_400);

/// Words that do not tell a piste from another.
const NOT_DISTINCTIVE: &[&str] = &[
    "le",
    "la",
    "les",
    "l",
    "de",
    "du",
    "des",
    "d",
    "et",
    "en",
    "a",
    "au",
    "aux",
    "par",
    "piste",
    "pistes",
    "pieton",
    "pietons",
    "pietonne",
    "liaison",
    "boucle",
    "tour",
    "raquette",
    "raquettes",
    "ski",
    "fond",
    "nordique",
    "multiactivites",
    "multi",
    "activites",
    "itineraire",
    "sentier",
    "circuit",
    "depuis",
    "vers",
    "km",
    "m",
    "sur",
    "skating",
    "classique",
    "marche",
    "trail",
    "balade",
    "randonnee",
    "promenade",
];

/// Words of a name that tell the piste apart (folded, without lengths).
pub fn distinctive(name: &str) -> Vec<String> {
    fold(name)
        .split_whitespace()
        .filter(|w| !NOT_DISTINCTIVE.contains(w))
        .filter(|w| !w.chars().next().is_some_and(|c| c.is_ascii_digit()))
        .map(str::to_owned)
        .collect()
}

/// Family of a piste: what it is groomed for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Family {
    /// Cross-country ski.
    Ski,
    /// Pedestrian, snowshoe or multi-activity track.
    Pedestrian,
}

/// A piste named by a source and absent from OSM.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Known {
    /// Name as the source writes it (length removed).
    pub name: String,
    /// Difficulty colour (green, blue, red, black).
    pub colour: Option<String>,
    /// Ski or pedestrian.
    pub family: Family,
    /// Link between two places (« Liaison … »).
    pub liaison: bool,
    /// Printed length, km.
    pub length_km: Option<f64>,
    /// Which source names it (`nordicfrance`, `plan`…).
    pub named_by: String,
    /// Licences of the source naming it.
    pub licences: Vec<String>,
    /// Official plan, link only.
    pub plan_url: Option<String>,
}

/// Colour of a printed flag (« Vert », « Bleu », « Rouge », « Noir », or English).
pub fn colour_of_flag(flag: &str) -> Option<&'static str> {
    match fold(flag).as_str() {
        "vert" | "verte" | "green" => Some("green"),
        "bleu" | "bleue" | "blue" => Some("blue"),
        "rouge" | "red" => Some("red"),
        "noir" | "noire" | "black" => Some("black"),
        _ => None,
    }
}

/// Printed length in a name (« Le Gibera 1.5km », « Izoard 16 km »), km.
pub fn length_in_name(name: &str) -> Option<f64> {
    let t = name.replace(',', ".");
    let words: Vec<&str> = t.split_whitespace().collect();
    for (i, w) in words.iter().enumerate() {
        let lw = w.to_lowercase();
        let num = if let Some(n) = lw.strip_suffix("km") {
            n.to_owned()
        } else if words
            .get(i + 1)
            .is_some_and(|n| n.eq_ignore_ascii_case("km"))
        {
            lw.clone()
        } else {
            continue;
        };
        if let Ok(v) = num.parse::<f64>() {
            return Some(v);
        }
    }
    None
}

/// Name without its printed length.
pub fn strip_length(name: &str) -> String {
    let words: Vec<&str> = name.split_whitespace().collect();
    let mut out: Vec<&str> = Vec::new();
    let mut i = 0;
    while i < words.len() {
        let w = words[i].to_lowercase().replace(',', ".");
        let numeric = |s: &str| !s.is_empty() && s.parse::<f64>().is_ok();
        if w.strip_suffix("km").is_some_and(numeric) || w.strip_suffix('m').is_some_and(numeric) {
            i += 1;
            continue;
        }
        if numeric(&w)
            && words
                .get(i + 1)
                .is_some_and(|n| n.eq_ignore_ascii_case("km"))
        {
            i += 2;
            continue;
        }
        out.push(words[i]);
        i += 1;
    }
    out.join(" ")
}

fn family_of(name: &str, activity: &str, flag: &str) -> Family {
    let n = fold(name);
    let a = fold(activity);
    let f = fold(flag);
    let ski = a.contains("ski") || colour_of_flag(flag).is_some();
    let walk = n.starts_with("pieton")
        || f.contains("raquette")
        || a.contains("raquette")
        || a.contains("marche")
        || a.contains("pied");
    if walk && !ski {
        Family::Pedestrian
    } else {
        Family::Ski
    }
}

/// Finds, anywhere in `v`, the object naming the piste `name` (`name` or `piste` field).
fn find_row<'a>(v: &'a Value, name: &str) -> Option<&'a Value> {
    match v {
        Value::Object(o) => {
            let hit = ["name", "piste"]
                .iter()
                .any(|k| o.get(*k).and_then(Value::as_str) == Some(name));
            if hit {
                return Some(v);
            }
            o.values().find_map(|x| find_row(x, name))
        }
        Value::Array(a) => a.iter().find_map(|x| find_row(x, name)),
        _ => None,
    }
}

/// The operator's pistes the join to OSM left unmatched (`status.json`,
/// `status.meta.json`). Licences: those of the status sources that are not OSM.
pub fn from_status(status: &Value, meta: &LayerMeta) -> Vec<Known> {
    let licences: Vec<String> = meta
        .sources
        .iter()
        .filter(|s| !s.licence.starts_with("ODbL"))
        .map(|s| {
            if s.personal_use_only && !s.licence.to_lowercase().starts_with("aucune") {
                format!(
                    "aucune licence de réutilisation (usage personnel) : {}",
                    s.licence
                )
            } else {
                s.licence.clone()
            }
        })
        .collect();
    let mut out = Vec::new();
    let Some(join) = meta.stats.get("join").and_then(Value::as_object) else {
        return out;
    };
    for (key, names) in join {
        let Some(source) = key.strip_suffix("_unmatched") else {
            continue;
        };
        for raw in names
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
        {
            let row = status
                .get("sources")
                .and_then(|s| s.get(source))
                .and_then(|s| find_row(s, raw));
            let txt = |k: &str| {
                row.and_then(|r| r.get(k))
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_owned()
            };
            let (activity, flag) = (txt("activity"), txt("flag"));
            let name = strip_length(raw);
            if distinctive(&name).is_empty() {
                continue;
            }
            out.push(Known {
                family: family_of(&name, &activity, &flag),
                liaison: fold(&name).starts_with("liaison"),
                colour: colour_of_flag(&flag).map(str::to_owned),
                length_km: row
                    .and_then(|r| r.get("length_km"))
                    .and_then(Value::as_f64)
                    .or_else(|| length_in_name(raw)),
                named_by: source.to_owned(),
                licences: licences.clone(),
                plan_url: None,
                name,
            });
        }
    }
    out
}

/// The legend lines of the plan with no OSM piste of their name and no estimated trace
/// (`plan_pistes.meta.json`).
pub fn from_plan(meta: &LayerMeta, plan_url: Option<&str>) -> Vec<Known> {
    let estimated: Vec<String> = meta.stats["estimation"]["estimated"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|e| e["name"].as_str().map(fold))
        .collect();
    let licences: Vec<String> = meta
        .sources
        .iter()
        .filter(|s| s.personal_use_only)
        .map(|s| {
            format!(
                "aucune licence de réutilisation (usage personnel) : {}",
                s.licence
            )
        })
        .collect();
    meta.stats["not_transcribed"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|d| {
            d["reason"]
                .as_str()
                .is_some_and(|r| r.starts_with("aucune piste OSM"))
        })
        .filter_map(|d| {
            let name = d["name"].as_str()?.to_owned();
            if !plausible_name(&name) || estimated.contains(&fold(&name)) {
                return None;
            }
            let ink = d["ink"].as_str().unwrap_or("");
            Some(Known {
                family: Family::Ski,
                liaison: fold(&name).starts_with("liaison"),
                colour: colour_of_flag(ink).map(str::to_owned),
                length_km: None,
                named_by: "plan".to_owned(),
                licences: licences.clone(),
                plan_url: plan_url.map(str::to_owned),
                name,
            })
        })
        .collect()
}

/// A published Geotrek itinerary.
#[derive(Debug, Clone)]
pub struct Trek {
    /// Name.
    pub name: String,
    /// Practice name (`trek_practice`).
    pub practice: String,
    /// Published 2D length, m (the geometry's when absent).
    pub length_m: f64,
    /// Lines (WGS84).
    pub lines: Vec<Vec<LonLat>>,
    /// Out-and-back (`route` = 2).
    pub out_and_back: bool,
    /// API page of the itinerary.
    pub url: String,
    /// Publisher (instance names).
    pub publisher: String,
    /// Licence statement of the instance.
    pub licence: String,
}

fn lines_length_m(lines: &[Vec<LonLat>]) -> f64 {
    lines
        .iter()
        .map(|l| l.windows(2).map(|w| w[0].haversine_m(w[1])).sum::<f64>())
        .sum()
}

/// Parses one page of `trek/` (fields `id,name,practice,length_2d,route,geometry`).
pub fn parse_treks(
    api: &str,
    publisher: &str,
    licence: &str,
    practices: &HashMap<u64, String>,
    v: &Value,
) -> Vec<Trek> {
    let line = |c: &Value| -> Vec<LonLat> {
        c.as_array()
            .into_iter()
            .flatten()
            .filter_map(|p| Some(LonLat::new(p.get(0)?.as_f64()?, p.get(1)?.as_f64()?)))
            .collect()
    };
    v["results"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|t| {
            let id = t["id"].as_u64()?;
            let g = &t["geometry"];
            let lines: Vec<Vec<LonLat>> = match g["type"].as_str()? {
                "LineString" => vec![line(&g["coordinates"])],
                "MultiLineString" => g["coordinates"].as_array()?.iter().map(line).collect(),
                _ => return None,
            };
            let lines: Vec<Vec<LonLat>> = lines.into_iter().filter(|l| l.len() >= 2).collect();
            if lines.is_empty() {
                return None;
            }
            let practice = t["practice"]
                .as_u64()
                .and_then(|p| practices.get(&p).cloned())
                .unwrap_or_default();
            Some(Trek {
                name: t["name"].as_str()?.trim().to_owned(),
                practice,
                length_m: t["length_2d"]
                    .as_f64()
                    .unwrap_or_else(|| lines_length_m(&lines)),
                out_and_back: t["route"].as_u64() == Some(2),
                lines,
                url: format!("{api}/trek/{id}/"),
                publisher: publisher.to_owned(),
                licence: licence.to_owned(),
            })
        })
        .collect()
}

/// Winter practice family of a Geotrek practice name; `None` for a summer practice.
pub fn practice_family(practice: &str) -> Option<Family> {
    let p = fold(practice);
    if p.contains("raquette") || p.contains("snow") || p.contains("neige") {
        Some(Family::Pedestrian)
    } else if p.contains("ski") || p.contains("nordique") || p.contains("fond") {
        Some(Family::Ski)
    } else {
        None
    }
}

/// Itineraries of every API of `apis` (`(root, publisher, licence)`) inside `b`, with the
/// first answer of each API (sources to cite). An API that cannot be read is reported.
pub fn fetch_treks(
    dl: &Downloader,
    apis: &[(String, String, String)],
    b: &BBox,
) -> (Vec<Trek>, Vec<(String, Fetched)>, Vec<String>) {
    let (mut treks, mut fetched, mut errors) = (Vec::new(), Vec::new(), Vec::new());
    let get = |u: &str| -> Result<(Value, Fetched)> {
        let f = dl.fetch(&Request::get("geotrek", u).max_age(GEOTREK_MAX_AGE))?;
        Ok((f.json()?, f))
    };
    for (api, publisher, licence) in apis {
        let practices: HashMap<u64, String> =
            match get(&format!("{api}/trek_practice/?language=fr")) {
                Ok((v, _)) => v["results"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(|p| Some((p["id"].as_u64()?, p["name"].as_str()?.to_owned())))
                    .collect(),
                Err(e) => {
                    errors.push(format!("{api} : {e}"));
                    continue;
                }
            };
        let mut url = Some(format!(
            "{api}/trek/?language=fr&in_bbox={:.4},{:.4},{:.4},{:.4}&fields=id,name,practice,length_2d,route,geometry&page_size=100",
            b.west, b.south, b.east, b.north
        ));
        let mut first = true;
        while let Some(u) = url.take() {
            match get(&u) {
                Ok((v, f)) => {
                    treks.extend(parse_treks(api, publisher, licence, &practices, &v));
                    if first {
                        fetched.push((publisher.clone(), f));
                        first = false;
                    }
                    url = v["next"].as_str().map(str::to_owned);
                }
                Err(e) => errors.push(format!("{u} : {e}")),
            }
        }
    }
    (treks, fetched, errors)
}

/// Agreement of a traced length with a printed one: `exp(−|ln ratio|)`, 1 when equal;
/// `None` when the ratio exceeds [`LENGTH_RATIO`]; 0.6 without a printed length.
pub fn length_agreement(traced_km: f64, printed_km: Option<f64>) -> Option<f64> {
    let Some(p) = printed_km.filter(|p| *p > 0.0) else {
        return Some(0.6);
    };
    if traced_km <= 0.0 {
        return None;
    }
    let r = (traced_km / p).ln().abs();
    (r <= LENGTH_RATIO.ln()).then(|| (-r).exp())
}

/// Share of the points of `lines` inside `b`.
fn share_inside(lines: &[Vec<LonLat>], b: &BBox) -> f64 {
    let (mut n, mut inside) = (0usize, 0usize);
    for p in lines.iter().flatten() {
        n += 1;
        if p.lat >= b.south && p.lat <= b.north && p.lon >= b.west && p.lon <= b.east {
            inside += 1;
        }
    }
    if n == 0 {
        0.0
    } else {
        inside as f64 / n as f64
    }
}

/// Best Geotrek itinerary for `k` (index, score) among `treks`, those with half their
/// points or more inside `b`.
pub fn match_trek(k: &Known, treks: &[Trek], b: &BBox) -> Option<(usize, f64)> {
    let words = distinctive(&k.name);
    if words.is_empty() {
        return None;
    }
    let mut best: Option<(usize, f64)> = None;
    for (i, t) in treks.iter().enumerate() {
        if practice_family(&t.practice) != Some(k.family) {
            continue;
        }
        let tw = distinctive(&t.name);
        if !words.iter().all(|w| tw.contains(w)) {
            continue;
        }
        if share_inside(&t.lines, b) < 0.5 {
            continue;
        }
        let one_way = t.length_m / 1000.0;
        let agree = [
            one_way,
            if t.out_and_back {
                one_way
            } else {
                2.0 * one_way
            },
        ]
        .into_iter()
        .filter_map(|l| length_agreement(l, k.length_km))
        .fold(None, |a: Option<f64>, x| Some(a.map_or(x, |a| a.max(x))));
        let Some(agree) = agree else { continue };
        // Words of the itinerary not in the piste's name lower the score a little.
        let extra = tw.iter().filter(|w| !words.contains(w)).count() as f64;
        let s = agree * 0.95_f64.powf(extra);
        if best.is_none_or(|(_, b)| s > b) {
            best = Some((i, s));
        }
    }
    best.filter(|(_, s)| *s >= MIN_SCORE)
}

/// Ends and way of a link name: « Liaison A - B par la route » → (`[A, B]`, `route`).
pub fn parse_liaison(name: &str) -> Option<(Vec<String>, Option<String>)> {
    let rest = name.trim();
    let lower = rest.to_lowercase();
    if !lower.starts_with("liaison") {
        return None;
    }
    let rest = rest["liaison".len()..].trim();
    let (ends, via) = match rest.to_lowercase().find(" par ") {
        Some(i) => (&rest[..i], Some(rest[i + 5..].trim().to_owned())),
        None => (rest, None),
    };
    let ends: Vec<String> = ends
        .split(['-', '–', '—'])
        .map(|s| s.trim().to_owned())
        .filter(|s| !distinctive(s).is_empty())
        .collect();
    if ends.is_empty() || ends.len() > 2 {
        return None;
    }
    Some((ends, via))
}

/// Toponym named `name` nearest to `near` (every distinctive word of `name` in the
/// toponym's name; inhabited places first, then the fewest other words).
pub fn find_place<'a>(name: &str, places: &'a [Landmark], near: P2) -> Option<&'a Landmark> {
    let words = distinctive(name);
    if words.is_empty() {
        return None;
    }
    let ok: Vec<&Landmark> = places
        .iter()
        .filter(|l| {
            let lw = distinctive(&l.name);
            words.iter().all(|w| lw.contains(w))
        })
        .collect();
    let rank = |l: &&Landmark| {
        (
            u8::from(l.kind != crate::transcript::LandmarkKind::Place),
            distinctive(&l.name).len().saturating_sub(words.len()),
            dist(l.xy, near),
        )
    };
    ok.into_iter()
        .min_by(|a, b| rank(a).partial_cmp(&rank(b)).unwrap_or(Ordering::Equal))
}

/// Inhabited place the site is named after: a toponym every distinctive word of which is
/// in the site's name (« Village - Col » → « Village »), the most words first, then the
/// nearest to `near`.
pub fn place_of_site<'a>(
    site_name: &str,
    places: &'a [Landmark],
    near: P2,
) -> Option<&'a Landmark> {
    let words = distinctive(site_name);
    places
        .iter()
        .filter(|l| l.kind == crate::transcript::LandmarkKind::Place)
        .filter_map(|l| {
            let lw = distinctive(&l.name);
            (!lw.is_empty() && lw.iter().all(|w| words.contains(w))).then_some((l, lw.len()))
        })
        .min_by(|a, b| {
            b.1.cmp(&a.1)
                .then(dist(a.0.xy, near).total_cmp(&dist(b.0.xy, near)))
        })
        .map(|(l, _)| l)
}

/// Road and path graph of the BD TOPO sections (ends joined at 1 m).
pub struct Network<'a> {
    paths: &'a [Path],
    nodes: Vec<P2>,
    /// node → (neighbour, path index, forward).
    adj: Vec<Vec<(usize, usize, bool)>>,
}

fn is_road(nature: &str) -> bool {
    nature.starts_with("Route")
}

impl<'a> Network<'a> {
    /// The graph of `paths`.
    pub fn new(paths: &'a [Path]) -> Self {
        let mut index: HashMap<(i64, i64), usize> = HashMap::new();
        let mut nodes = Vec::new();
        let mut adj: Vec<Vec<(usize, usize, bool)>> = Vec::new();
        let mut node = |p: P2, nodes: &mut Vec<P2>, adj: &mut Vec<Vec<_>>| {
            let k = (p[0].round() as i64, p[1].round() as i64);
            *index.entry(k).or_insert_with(|| {
                nodes.push(p);
                adj.push(Vec::new());
                nodes.len() - 1
            })
        };
        for (i, p) in paths.iter().enumerate() {
            let (Some(&a), Some(&b)) = (p.xy.first(), p.xy.last()) else {
                continue;
            };
            let na = node(a, &mut nodes, &mut adj);
            let nb = node(b, &mut nodes, &mut adj);
            adj[na].push((nb, i, true));
            adj[nb].push((na, i, false));
        }
        Self { paths, nodes, adj }
    }

    /// Nearest node to `p` with a section usable by the route (a road when
    /// `roads_only`).
    fn nearest(&self, p: P2, roads_only: bool) -> Option<(usize, f64)> {
        self.nodes
            .iter()
            .enumerate()
            .map(|(i, n)| (i, dist(*n, p)))
            .filter(|(i, _)| {
                self.adj[*i]
                    .iter()
                    .any(|e| !roads_only || is_road(&self.paths[e.1].nature))
            })
            .min_by(|a, b| a.1.total_cmp(&b.1))
    }

    /// Cheapest route from `a` to `b` (ends snapped within [`MAX_SNAP_M`]): roads only
    /// when `roads_only`, else roads cost [`ROAD_PENALTY`] times their length. Returns
    /// the line (Lambert-93) and its length, m.
    pub fn route(&self, a: P2, b: P2, roads_only: bool) -> Option<(Vec<P2>, f64)> {
        #[derive(PartialEq)]
        struct St(f64, usize);
        impl Eq for St {}
        impl PartialOrd for St {
            fn partial_cmp(&self, o: &Self) -> Option<Ordering> {
                Some(self.cmp(o))
            }
        }
        impl Ord for St {
            fn cmp(&self, o: &Self) -> Ordering {
                o.0.total_cmp(&self.0)
            }
        }
        let (s, ds) = self.nearest(a, roads_only)?;
        let (t, dt) = self.nearest(b, roads_only)?;
        if ds > MAX_SNAP_M || dt > MAX_SNAP_M {
            return None;
        }
        let mut best = vec![f64::INFINITY; self.nodes.len()];
        let mut prev: Vec<Option<(usize, usize, bool)>> = vec![None; self.nodes.len()];
        let mut heap = BinaryHeap::new();
        best[s] = 0.0;
        heap.push(St(0.0, s));
        while let Some(St(c, u)) = heap.pop() {
            if u == t {
                break;
            }
            if c > best[u] {
                continue;
            }
            for &(v, pi, fwd) in &self.adj[u] {
                let p = &self.paths[pi];
                let road = is_road(&p.nature);
                if roads_only && !road {
                    continue;
                }
                let len: f64 = p.xy.windows(2).map(|w| dist(w[0], w[1])).sum();
                let w = if !roads_only && road {
                    len * ROAD_PENALTY
                } else {
                    len
                };
                if c + w < best[v] {
                    best[v] = c + w;
                    prev[v] = Some((u, pi, fwd));
                    heap.push(St(c + w, v));
                }
            }
        }
        if !best[t].is_finite() {
            return None;
        }
        let mut parts: Vec<Vec<P2>> = Vec::new();
        let mut v = t;
        while let Some((u, pi, fwd)) = prev[v] {
            let mut xy = self.paths[pi].xy.clone();
            if !fwd {
                xy.reverse();
            }
            parts.push(xy);
            v = u;
        }
        parts.reverse();
        let mut line: Vec<P2> = Vec::new();
        for p in parts {
            for q in p {
                if line.last().is_none_or(|l| dist(*l, q) > 0.01) {
                    line.push(q);
                }
            }
        }
        let len = line.windows(2).map(|w| dist(w[0], w[1])).sum();
        (line.len() >= 2).then_some((line, len))
    }
}

/// Route of a link (`k.liaison`): line (Lambert-93), length (m), score and the names
/// of its ends. A link naming one end starts from the place the site is named after
/// ([`place_of_site`]), else from the commune centre.
pub fn route_liaison(
    k: &Known,
    net: &Network<'_>,
    places: &[Landmark],
    site_name: &str,
    centre: P2,
) -> Option<(Vec<P2>, f64, f64, Vec<String>)> {
    let (ends, via) = parse_liaison(&k.name)?;
    let mut pts: Vec<(P2, String)> = Vec::new();
    for e in &ends {
        let p = find_place(e, places, centre)?;
        pts.push((p.xy, p.name.clone()));
    }
    if pts.len() == 1 {
        let start = place_of_site(site_name, places, centre).map_or_else(
            || (centre, "centre de la commune".to_owned()),
            |p| (p.xy, p.name.clone()),
        );
        pts.insert(0, start);
    }
    let roads_only = via.as_deref().is_some_and(|v| fold(v).contains("route"));
    let (line, len) = net.route(pts[0].0, pts[1].0, roads_only)?;
    let agree = length_agreement(len / 1000.0, k.length_km)?;
    let s = 0.8 * agree;
    (s >= MIN_SCORE).then(|| (line, len, s, pts.into_iter().map(|p| p.1).collect()))
}

/// What [`build`] did.
#[derive(Debug, Clone, Default, Serialize)]
pub struct Report {
    /// Pistes named and absent from OSM, by the source naming them.
    pub known: BTreeMap<String, usize>,
    /// Traces copied from the plan's estimation.
    pub from_plan_estimate: usize,
    /// Traced from Geotrek.
    pub geotrek: usize,
    /// Routed on the IGN sections.
    pub routed: usize,
    /// Each piste and what became of it.
    pub pistes: Vec<Value>,
    /// Sources that could not be read.
    pub errors: Vec<String>,
}

/// Inputs of [`build`] read from the network.
pub struct Ground<'a> {
    /// Geotrek itineraries near the site.
    pub treks: &'a [Trek],
    /// BD TOPO sections of the site.
    pub paths: &'a [Path],
    /// BD TOPO toponyms of the site.
    pub places: &'a [Landmark],
}

fn ll_line(xy: &[P2]) -> Result<Vec<LonLat>> {
    xy.iter()
        .map(|p| from_lambert93(p[0], p[1]).map_err(Error::from))
        .collect()
}

fn read_json(path: &FsPath) -> Result<Option<Value>> {
    if !path.is_file() {
        return Ok(None);
    }
    let t = std::fs::read_to_string(path).map_err(|e| Error::Io {
        path: path.to_path_buf(),
        source: e,
    })?;
    serde_json::from_str(&t).map(Some).map_err(|e| Error::Json {
        context: path.display().to_string(),
        source: e,
    })
}

fn read_meta(path: &FsPath) -> Result<Option<LayerMeta>> {
    Ok(if path.is_file() {
        Some(LayerMeta::read(path)?)
    } else {
        None
    })
}

/// The pistes absent from OSM named by the status and plan outputs of `out`
/// (`data/out/<site>/`), and the plan's estimated traces as features.
pub fn known_of(out: &FsPath) -> Result<(Vec<Known>, Vec<Value>, Vec<SourceRef>)> {
    let mut known = Vec::new();
    let mut copied = Vec::new();
    let mut sources = Vec::new();
    if let (Some(st), Some(meta)) = (
        read_json(&out.join("status").join("status.json"))?,
        read_meta(&out.join("status").join("status.meta.json"))?,
    ) {
        let k = from_status(&st, &meta);
        if !k.is_empty() {
            sources.extend(
                meta.sources
                    .iter()
                    .filter(|s| !s.licence.starts_with("ODbL"))
                    .cloned(),
            );
        }
        known.extend(k);
    }
    let pdir = out.join("plan_pistes");
    if let Some(meta) = read_meta(&pdir.join("plan_pistes.meta.json"))? {
        let fc = read_json(&pdir.join("plan_pistes.geojson"))?.unwrap_or(Value::Null);
        let plan_url = fc["features"]
            .as_array()
            .into_iter()
            .flatten()
            .find_map(|f| {
                f["properties"]["source_plan_url"]
                    .as_str()
                    .map(str::to_owned)
            });
        let plan_licences: Vec<String> = meta
            .sources
            .iter()
            .filter(|s| s.personal_use_only)
            .map(|s| {
                format!(
                    "aucune licence de réutilisation (usage personnel) : {}",
                    s.licence
                )
            })
            .collect();
        for f in fc["features"].as_array().into_iter().flatten() {
            let p = &f["properties"];
            let est = p["certainty"] == "incertain"
                || p["method"]
                    .as_str()
                    .is_some_and(|m| m.starts_with("estimated_"));
            if !est {
                continue;
            }
            let mut f = f.clone();
            let mut licences = plan_licences.clone();
            licences.push(BDTOPO_LICENCE.to_owned());
            f["properties"]["licences"] = json!(licences);
            f["properties"]["named_by"] = json!("plan");
            copied.push(f);
        }
        let k = from_plan(&meta, plan_url.as_deref());
        if !k.is_empty() || !copied.is_empty() {
            sources.extend(meta.sources.iter().filter(|s| s.personal_use_only).cloned());
        }
        known.extend(k);
    }
    // One entry per piste: the operator's (printed length) before the plan's.
    let mut seen: Vec<Vec<String>> = copied
        .iter()
        .filter_map(|f| f["properties"]["name"].as_str().map(distinctive))
        .collect();
    known.retain(|k| {
        let d = distinctive(&k.name);
        if seen.contains(&d) {
            false
        } else {
            seen.push(d);
            true
        }
    });
    Ok((known, copied, sources))
}

/// Traces `known` on `ground`: the features (WGS84) and the report.
pub fn trace(
    known: &[Known],
    ground: &Ground<'_>,
    b: &BBox,
    site_name: &str,
    centre: LonLat,
) -> Result<(Vec<Value>, Report)> {
    let mut rep = Report::default();
    let net = Network::new(ground.paths);
    let (cx, cy) = to_lambert93(centre)?;
    let mut features = Vec::new();
    for k in known {
        *rep.known.entry(k.named_by.clone()).or_default() += 1;
        let base = json!({
            "name": k.name, "colour": k.colour, "kind": if k.liaison { "liaison" } else if k.family == Family::Pedestrian { "pedestrian" } else { "ski" },
            "certainty": "incertain", "named_by": k.named_by,
            "length_km_announced": k.length_km, "plan_url": k.plan_url,
        });
        if let Some((i, s)) = match_trek(k, ground.treks, b) {
            let t = &ground.treks[i];
            let mut p = base.clone();
            let mut licences = k.licences.clone();
            licences.push(t.licence.clone());
            p["method"] = json!("geotrek");
            p["score"] = json!((s * 1000.0).round() / 1000.0);
            p["length_m"] = json!(lines_length_m(&t.lines).round());
            p["source_name"] = json!(t.name);
            p["source_url"] = json!(t.url);
            p["source_publisher"] = json!(t.publisher);
            p["licences"] = json!(licences);
            let coords: Vec<Value> = t
                .lines
                .iter()
                .map(|l| Value::Array(l.iter().map(|q| output::position(*q)).collect()))
                .collect();
            rep.pistes.push(json!({"name": k.name, "named_by": k.named_by, "method": "geotrek", "score": p["score"], "length_km_announced": k.length_km, "length_m": p["length_m"], "source": t.name, "url": t.url}));
            features.push(output::feature(
                json!({"type": "MultiLineString", "coordinates": coords}),
                p,
            ));
            rep.geotrek += 1;
            continue;
        }
        if k.liaison
            && let Some((line, len, s, ends)) =
                route_liaison(k, &net, ground.places, site_name, [cx, cy])
        {
            let mut p = base.clone();
            let mut licences = k.licences.clone();
            licences.push(BDTOPO_LICENCE.to_owned());
            p["method"] = json!("estimated_route");
            p["score"] = json!((s * 1000.0).round() / 1000.0);
            p["length_m"] = json!(len.round());
            p["route_ends"] = json!(ends);
            p["licences"] = json!(licences);
            let ll = ll_line(&line)?;
            rep.pistes.push(json!({"name": k.name, "named_by": k.named_by, "method": "estimated_route", "score": p["score"], "length_km_announced": k.length_km, "length_m": p["length_m"], "ends": ends}));
            features.push(output::feature(
                    json!({"type": "MultiLineString", "coordinates": [ll.iter().map(|q| output::position(*q)).collect::<Vec<_>>()]}),
                    p,
                ));
            rep.routed += 1;
            continue;
        }
        rep.pistes.push(json!({"name": k.name, "named_by": k.named_by, "method": null,
            "reason": if k.liaison { "liaison : extrémités introuvables parmi les toponymes IGN, ou longueur sans accord avec celle annoncée" } else { "aucun itinéraire Geotrek d'hiver du même nom et d'une longueur proche près du site" }}));
    }
    Ok((features, rep))
}

/// What [`write`] writes: the outputs of [`known_of`] and the sources of the ground.
pub struct Inputs {
    /// Pistes to trace.
    pub known: Vec<Known>,
    /// Estimated traces of the plan, copied.
    pub copied: Vec<Value>,
    /// Sources to cite.
    pub sources: Vec<SourceRef>,
    /// Sources that could not be read.
    pub errors: Vec<String>,
}

/// Writes `out/pistes_estimees/` from the plan's estimated traces and the pistes named
/// by the status and the plan, traced on `ground` (site box `b`, commune centre).
pub fn write(
    out: &FsPath,
    site: &str,
    inputs: Inputs,
    ground: &Ground<'_>,
    b: &BBox,
    site_name: &str,
    centre: LonLat,
) -> Result<Report> {
    let Inputs {
        known,
        copied,
        mut sources,
        errors,
    } = inputs;
    let (traced, mut rep) = trace(&known, ground, b, site_name, centre)?;
    rep.errors = errors;
    rep.from_plan_estimate = copied.len();
    let mut features = copied;
    features.extend(traced);
    let dir = out.join(LAYER);
    let file = format!("{LAYER}.geojson");
    output::write_geojson(&dir.join(&file), &features)?;
    sources.dedup_by(|a, b| a.name == b.name && a.url == b.url);
    let mut meta = LayerMeta::new(LAYER, site);
    meta.tickets = vec!["G1 (#117)".to_owned()];
    meta.crs = Some("EPSG:4326 (WGS84, longitude, latitude)".to_owned());
    meta.sources = sources;
    meta.files = vec![
        FileMeta::new(
            &file,
            "pistes connues absentes d'OSM, tracé estimé (certainty = incertain) : tracés estimés d'après le plan officiel, itinéraires Geotrek du même nom, liaisons routées sur les tronçons IGN",
        )
        .licence("par entité (`licences`) : source qui nomme la piste et source de la géométrie ; usage personnel dès qu'une des deux l'est")
        .count(features.len())
        .properties([
            ("name", "nom de la piste dans la source qui la nomme"),
            ("colour", "couleur de difficulté annoncée (green, blue, red, black)"),
            ("kind", "ski, liaison, pedestrian (piéton, raquettes)"),
            ("certainty", "toujours incertain"),
            ("method", "estimated_paths, estimated_drawing (plan), geotrek (itinéraire publié du même nom), estimated_route (liaison routée sur les tronçons IGN)"),
            ("score", "confiance de 0 à 1 (accord des longueurs, des noms, du calage)"),
            ("length_m", "longueur du tracé"),
            ("length_km_announced", "longueur annoncée par l'exploitant"),
            ("named_by", "source qui nomme la piste (bulletin de l'exploitant, plan)"),
            ("source_name", "nom de l'itinéraire Geotrek repris"),
            ("source_url", "adresse de l'itinéraire Geotrek repris"),
            ("route_ends", "toponymes IGN des extrémités d'une liaison routée"),
            ("plan_url", "adresse du plan officiel (lien seulement)"),
            ("licences", "licences de la source qui nomme la piste et de la géométrie"),
        ]),
    ];
    meta.notes = vec![
        "Aucun tracé n'est sûr : pointillés sur la carte, « Tracé incertain » sur la fiche. Un pack publié ne garde que les entités dont toutes les licences le permettent.".to_owned(),
    ];
    meta.stats = serde_json::to_value(&rep).map_err(|e| Error::Json {
        context: "report".into(),
        source: e,
    })?;
    meta.write(&dir)?;
    Ok(rep)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn path(id: &str, nature: &str, xy: &[P2]) -> Path {
        Path {
            id: id.into(),
            nature: nature.into(),
            xy: xy.to_vec(),
            grade: 0.0,
        }
    }

    #[test]
    fn names_lengths_and_colours() {
        assert_eq!(length_in_name("Le Gibera 1.5km"), Some(1.5));
        assert_eq!(length_in_name("Liaison par route 3,5 km"), Some(3.5));
        assert_eq!(length_in_name("Le Gy"), None);
        assert_eq!(strip_length("Le Gibera 1.5km"), "Le Gibera");
        assert_eq!(strip_length("Liaison A-B 1.5m"), "Liaison A-B");
        assert_eq!(
            strip_length("Liaison par route 3.5 km"),
            "Liaison par route"
        );
        assert_eq!(colour_of_flag("Bleu"), Some("blue"));
        assert_eq!(colour_of_flag("raquettes"), None);
        assert_eq!(distinctive("Piéton les Fonts"), vec!["fonts"]);
        assert_eq!(
            distinctive("Les Fonts de Cervières en raquettes"),
            vec!["fonts", "cervieres"]
        );
    }

    #[test]
    fn liaison_names() {
        assert_eq!(
            parse_liaison("Liaison les Aittes par la route"),
            Some((vec!["les Aittes".to_owned()], Some("la route".to_owned())))
        );
        assert_eq!(
            parse_liaison("Liaison Village-le Haut"),
            Some((vec!["Village".to_owned(), "le Haut".to_owned()], None))
        );
        assert_eq!(parse_liaison("La Combe"), None);
    }

    fn known(name: &str, family: Family, km: Option<f64>) -> Known {
        Known {
            name: name.into(),
            colour: None,
            family,
            liaison: name.starts_with("Liaison"),
            length_km: km,
            named_by: "op".into(),
            licences: vec!["aucune licence".into()],
            plan_url: None,
        }
    }

    fn trek(name: &str, practice: &str, km: f64) -> Trek {
        Trek {
            name: name.into(),
            practice: practice.into(),
            length_m: km * 1000.0,
            lines: vec![vec![LonLat::new(6.0, 45.0), LonLat::new(6.01, 45.01)]],
            out_and_back: false,
            url: "u".into(),
            publisher: "p".into(),
            licence: "aucune licence affichée".into(),
        }
    }

    #[test]
    fn trek_needs_the_name_a_winter_practice_and_the_length() {
        let b = BBox::new(44.9, 5.9, 45.1, 6.1);
        let treks = vec![
            trek("La cascade du Val", "A pied", 6.0),
            trek("Le Val en raquettes", "Raquette", 6.5),
            trek("Le Mont en raquettes", "Raquette", 6.0),
        ];
        let k = known("Piéton le Val", Family::Pedestrian, Some(6.0));
        assert_eq!(match_trek(&k, &treks, &b).map(|m| m.0), Some(1));
        // Too long.
        let k = known("Piéton le Val", Family::Pedestrian, Some(2.0));
        assert_eq!(match_trek(&k, &treks, &b), None);
        // A ski piste does not take a snowshoe itinerary.
        let k = known("Le Val", Family::Ski, Some(6.0));
        assert_eq!(match_trek(&k, &treks, &b), None);
        // Outside the site.
        let far = BBox::new(40.0, 0.0, 40.1, 0.1);
        let k = known("Piéton le Val", Family::Pedestrian, Some(6.0));
        assert_eq!(match_trek(&k, &treks, &far), None);
    }

    #[test]
    fn a_linear_itinerary_counts_twice_for_an_out_and_back_piste() {
        let b = BBox::new(44.9, 5.9, 45.1, 6.1);
        let treks = vec![trek("Le Val en raquettes", "Raquette", 11.0)];
        let k = known("Piéton le Val", Family::Pedestrian, Some(22.0));
        let (_, s) = match_trek(&k, &treks, &b).unwrap();
        assert!(s > 0.9, "{s}");
    }

    #[test]
    fn route_follows_roads_or_prefers_paths() {
        // Square A(0,0) – B(1000,0): road along the bottom (via (500,-300)), path on top
        // (via (500, 400)).
        let paths = vec![
            path("r1", "Route à 1 chaussée", &[[0.0, 0.0], [500.0, -300.0]]),
            path(
                "r2",
                "Route à 1 chaussée",
                &[[500.0, -300.0], [1000.0, 0.0]],
            ),
            path("p1", "Chemin", &[[0.0, 0.0], [500.0, 400.0]]),
            path("p2", "Sentier", &[[500.0, 400.0], [1000.0, 0.0]]),
        ];
        let net = Network::new(&paths);
        let (line, _) = net.route([0.0, 0.0], [1000.0, 10.0], true).unwrap();
        assert!(line.iter().any(|p| p[1] < -100.0), "{line:?}");
        let (line, len) = net.route([0.0, 0.0], [1000.0, 10.0], false).unwrap();
        assert!(line.iter().any(|p| p[1] > 100.0), "{line:?}");
        assert!((len - 2.0 * 640.3).abs() < 1.0, "{len}");
        // Too far from the network.
        assert!(net.route([0.0, 5000.0], [1000.0, 0.0], false).is_none());
    }

    #[test]
    fn status_unmatched_rows_become_known_pistes() {
        let status = json!({"sources": {"op": {"sectors": [{"pistes": [
            {"name": "Piéton le Val", "length_km": 6.0, "activity": "6km - Raquettes , Marche/trail", "flag": "raquettes"},
            {"name": "Liaison le Bas par la route", "length_km": 3.5, "activity": "3.5km - Marche/trail , Ski de fond", "flag": null}
        ]}]}, "mairie": {"sectors": [{"rows": [{"piste": "Liaison A-le Bas 1.5km"}]}]}}});
        let mut meta = LayerMeta::new("status", "s");
        meta.sources = vec![
            SourceRef::new("Op", "https://op", "aucune licence, HTML seulement", "Op")
                .personal_use_only(),
            SourceRef::new("OSM", "https://osm", "ODbL 1.0", "OSM"),
        ];
        meta.stats = json!({"join": {"op_unmatched": ["Piéton le Val", "Liaison le Bas par la route"],
                                      "mairie_unmatched": ["Liaison A-le Bas 1.5km"], "op_rows": 2}});
        let k = from_status(&status, &meta);
        assert_eq!(k.len(), 3);
        let val = k.iter().find(|k| k.name == "Piéton le Val").unwrap();
        assert_eq!(val.family, Family::Pedestrian);
        assert_eq!(val.length_km, Some(6.0));
        assert!(!val.liaison);
        let bas = k
            .iter()
            .find(|k| k.name == "Liaison le Bas par la route")
            .unwrap();
        assert_eq!(bas.family, Family::Ski);
        assert!(bas.liaison);
        let m = k.iter().find(|k| k.named_by == "mairie").unwrap();
        assert_eq!(m.name, "Liaison A-le Bas");
        assert_eq!(m.length_km, Some(1.5));
        assert_eq!(
            m.licences,
            vec!["aucune licence, HTML seulement".to_owned()]
        );
    }
}
