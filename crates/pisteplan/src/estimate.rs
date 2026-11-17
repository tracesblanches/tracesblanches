//! Estimated traces of the pistes of the plan that OSM lacks (G1, #117).
//!
//! A piste of the legend that neither the exact matching nor the learnt model carried onto
//! an OSM piste is looked for on the plan itself, then on the ground:
//!
//! 1. **Drawn lines without OSM piste.** The skeleton of the piste's ink is sampled every
//!    [`SAMPLE_PT`]; a sample is *explained* when an OSM piste way, carried onto the plan
//!    by the registration, runs within [`EXPLAINED_PT`] in the same direction (±30°).
//!    The other samples form runs, joined into lines across gaps of [`JOIN_GAP_PT`] (a
//!    label or a junction cuts the drawn line). Lines shorter than [`MIN_DRAWN_M`] on the
//!    ground are dropped.
//! 2. **Which line is the piste.** Its name written on the map beside a line of its ink
//!    (within [`LABEL_RADIUS_PT`]; key of the name equal, OCR similarity ≥ 0.8), else its
//!    departure box on the map when it is the only piste of its ink without OSM piste in
//!    that box and one line of that ink starts within [`BOX_RADIUS_PT`]. A line claimed
//!    by two pistes goes to the nearer anchor; without anchor nothing is estimated.
//! 3. **On the ground.** The line is carried onto the map by the registration's spline. Its
//!    corridor is the held-out error of the registration (p90 of the junctions left out,
//!    between [`MIN_CORRIDOR_M`] and [`MAX_CORRIDOR_M`]). The paths and tracks of the IGN
//!    BD TOPO (`troncon_de_route`, motorways and dual carriageways left out) in the
//!    corridor are the candidates; each point of the carried line picks the nearest
//!    candidate, its distance weighted by the slope of the section (BD TOPO heights: a
//!    groomed nordic piste rarely climbs more than 12 %), and a candidate point is kept
//!    when the nearest line point picked its section. The kept runs are the estimated
//!    geometry (`estimated_paths`) when they follow at least [`MIN_COVERED`] of the line;
//!    otherwise the carried line itself is the geometry (`estimated_drawing`).
//! 4. **Score** (0 to 1): anchor (map label 1, departure box 0.7) × registration quality
//!    (exp(−p90 / 400 m)) × share of the line followed (half that share for a drawing).
//!    Below [`MIN_SCORE`] nothing is written.
//! 5. **Guards** (measured on six massifs, #117): nothing is estimated on a plan whose
//!    registration no exact match confirms, nor for a legend line that does not read as a
//!    name ([`plausible_name`]).
//!
//! The result is always `incertain`: never shown as a sure piste (dotted on the map, the
//! card says « Tracé incertain, d'après le plan de la station »).

use std::collections::{BTreeMap, HashMap};

use rstar::primitives::{GeomWithData, Line};
use rstar::{PointDistance, RTree};
use serde::Serialize;
use serde_json::Value;
use skimap_common::crs::{from_lambert93, to_lambert93};
use skimap_common::download::{Downloader, Fetched, Request};
use skimap_common::geom::{BBox, LonLat};
use skimap_common::names::name_key;

use crate::build::Registration;
use crate::error::{Error, Result};
use crate::labels::{fold, similarity};
use crate::spec::LabelSource;
use crate::transcribe::{Decision, MAX_ANGLE_DEG};
use crate::transcript::{Estimate, MatchMethod, PisteKind};
use crate::warp::{P2, dist, quantile};

/// Spacing of the samples along the drawn lines, page points.
pub const SAMPLE_PT: f64 = 2.0;
/// A drawn sample within this distance of an OSM piste way (same direction) is explained
/// by OSM, page points.
pub const EXPLAINED_PT: f64 = 8.0;
/// Runs of unexplained samples closer than this are one line, page points.
pub const JOIN_GAP_PT: f64 = 6.0;
/// Shortest drawn line kept, metres on the ground.
pub const MIN_DRAWN_M: f64 = 200.0;
/// Largest distance between a name written on the map and its line, page points.
pub const LABEL_RADIUS_PT: f64 = 12.0;
/// Largest distance between a departure box and the start of its line, page points.
pub const BOX_RADIUS_PT: f64 = 30.0;
/// Bounds of the corridor half width, metres.
pub const MIN_CORRIDOR_M: f64 = 50.0;
/// Upper bound of the corridor half width, metres.
pub const MAX_CORRIDOR_M: f64 = 200.0;
/// Spacing of the samples along the candidate paths, metres.
pub const PATH_STEP_M: f64 = 10.0;
/// Smallest share of the line followed by paths for an `estimated_paths` geometry.
pub const MIN_COVERED: f64 = 0.6;
/// Smallest score written.
pub const MIN_SCORE: f64 = 0.3;
/// Grade above which a candidate section is penalised.
pub const GRADE_FREE: f64 = 0.12;
/// Anchor weights.
const ANCHOR_LABEL: f64 = 1.0;
const ANCHOR_BOX: f64 = 0.7;

/// Settings of the estimation (defaults: the constants above).
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct Params {
    /// [`EXPLAINED_PT`].
    pub explained_pt: f64,
    /// [`LABEL_RADIUS_PT`].
    pub label_radius_pt: f64,
    /// [`BOX_RADIUS_PT`].
    pub box_radius_pt: f64,
    /// [`MIN_COVERED`].
    pub min_covered: f64,
    /// [`MIN_SCORE`].
    pub min_score: f64,
}

impl Default for Params {
    fn default() -> Self {
        Self {
            explained_pt: EXPLAINED_PT,
            label_radius_pt: LABEL_RADIUS_PT,
            box_radius_pt: BOX_RADIUS_PT,
            min_covered: MIN_COVERED,
            min_score: MIN_SCORE,
        }
    }
}

/// BD TOPO natures that cannot carry a nordic piste.
pub const EXCLUDED_NATURES: [&str; 6] = [
    "Type autoroutier",
    "Route à 2 chaussées",
    "Bretelle",
    "Rond-point",
    "Bac ou liaison maritime",
    "Escalier",
];

/// A candidate path or track (Lambert-93, heights when the source has them).
#[derive(Debug, Clone)]
pub struct Path {
    /// Source identifier (`ign/<cleabs>`).
    pub id: String,
    /// BD TOPO nature.
    pub nature: String,
    /// Points, Lambert-93 metres.
    pub xy: Vec<P2>,
    /// Mean absolute grade (0 when unknown).
    pub grade: f64,
}

fn grade_of(xy: &[P2], z: &[Option<f64>]) -> f64 {
    let (mut climb, mut len) = (0.0, 0.0);
    for i in 1..xy.len() {
        let l = dist(xy[i - 1], xy[i]);
        if let (Some(a), Some(b)) = (z[i - 1], z[i]) {
            climb += (b - a).abs();
            len += l;
        }
    }
    if len > 0.0 { climb / len } else { 0.0 }
}

/// Parses a page of BD TOPO `troncon_de_route` (GeoJSON, WGS84 with heights).
pub fn parse_paths(v: &Value) -> Result<Vec<Path>> {
    let feats = v
        .get("features")
        .and_then(Value::as_array)
        .ok_or_else(|| Error::Data("WFS answer without features".into()))?;
    let mut out = Vec::new();
    for f in feats {
        let p = &f["properties"];
        let nature = p["nature"].as_str().unwrap_or("").to_owned();
        if EXCLUDED_NATURES.contains(&nature.as_str())
            || p["etat_de_l_objet"]
                .as_str()
                .is_some_and(|e| e != "En service")
        {
            continue;
        }
        let g = &f["geometry"];
        let lines: Vec<&Value> = match g["type"].as_str() {
            Some("LineString") => vec![&g["coordinates"]],
            Some("MultiLineString") => g["coordinates"].as_array().into_iter().flatten().collect(),
            _ => continue,
        };
        for l in lines {
            let mut xy = Vec::new();
            let mut z = Vec::new();
            for c in l.as_array().into_iter().flatten() {
                let (Some(lon), Some(lat)) = (c[0].as_f64(), c[1].as_f64()) else {
                    continue;
                };
                let (x, y) = to_lambert93(LonLat::new(lon, lat))?;
                xy.push([x, y]);
                z.push(c.get(2).and_then(Value::as_f64).filter(|h| *h > -100.0));
            }
            if xy.len() < 2 {
                continue;
            }
            out.push(Path {
                id: format!("ign/{}", p["cleabs"].as_str().unwrap_or_default()),
                nature: nature.clone(),
                grade: grade_of(&xy, &z),
                xy,
            });
        }
    }
    Ok(out)
}

/// IGN Géoplateforme WFS (BD TOPO).
pub const IGN_WFS_URL: &str = crate::osmref::IGN_WFS_URL;

/// Features per WFS page.
const WFS_PAGE: usize = 5000;
/// The Géoplateforme answers `400` past two pages (`STARTINDEX` ≥ 10 000): a box with
/// more features is split in four, down to this depth.
const WFS_MAX_SPLIT: u32 = 6;

fn wfs_page(dl: &Downloader, b: &BBox, start: usize) -> Result<(Value, Fetched)> {
    let url = format!(
        "{IGN_WFS_URL}?SERVICE=WFS&VERSION=2.0.0&REQUEST=GetFeature&TYPENAMES=BDTOPO_V3:troncon_de_route&BBOX={:.5},{:.5},{:.5},{:.5},urn:ogc:def:crs:EPSG::4326&COUNT={WFS_PAGE}&STARTINDEX={start}&OUTPUTFORMAT=application/json",
        b.south, b.west, b.north, b.east,
    );
    let f = dl.fetch(&Request::get("ign", &url))?;
    let v: Value = f.json()?;
    Ok((v, f))
}

fn paths_box(
    dl: &Downloader,
    b: &BBox,
    depth: u32,
    out: &mut Vec<Value>,
    first: &mut Option<Fetched>,
) -> Result<()> {
    let count = |v: &Value| v["features"].as_array().map_or(0, Vec::len);
    let (v0, f0) = wfs_page(dl, b, 0)?;
    first.get_or_insert(f0);
    if count(&v0) < WFS_PAGE {
        out.push(v0);
        return Ok(());
    }
    let (v1, _) = wfs_page(dl, b, WFS_PAGE)?;
    if count(&v1) < WFS_PAGE || depth >= WFS_MAX_SPLIT {
        out.push(v0);
        out.push(v1);
        return Ok(());
    }
    // More than two pages: four quarters.
    let (mlat, mlon) = ((b.south + b.north) / 2.0, (b.west + b.east) / 2.0);
    for q in [
        BBox::new(b.south, b.west, mlat, mlon),
        BBox::new(b.south, mlon, mlat, b.east),
        BBox::new(mlat, b.west, b.north, mlon),
        BBox::new(mlat, mlon, b.north, b.east),
    ] {
        paths_box(dl, &q, depth + 1, out, first)?;
    }
    Ok(())
}

/// Paths and tracks of `b` from the BD TOPO (IGN WFS, source `ign`, pages of 5 000; a box
/// of more than two pages is split in four), each section once.
pub fn paths_ign(dl: &Downloader, b: &BBox) -> Result<(Vec<Path>, Fetched)> {
    let mut pages = Vec::new();
    let mut first = None;
    paths_box(dl, b, 0, &mut pages, &mut first)?;
    let mut seen = std::collections::HashSet::new();
    let mut out = Vec::new();
    for v in &pages {
        for p in parse_paths(v)? {
            let key = (p.id.clone(), p.xy[0][0].to_bits(), p.xy[0][1].to_bits());
            if seen.insert(key) {
                out.push(p);
            }
        }
    }
    let first = first.ok_or_else(|| Error::Data("no WFS page".into()))?;
    Ok((out, first))
}

/// A line of the plan drawn in a piste ink where OSM has no piste.
#[derive(Debug, Clone)]
pub struct DrawnLine {
    /// Ink index.
    pub ink: usize,
    /// Runs of samples, page points.
    pub runs: Vec<Vec<P2>>,
    /// Length on the ground, m.
    pub length_m: f64,
}

impl DrawnLine {
    /// Smallest distance from `p` to the line, page points.
    pub fn distance_pt(&self, p: P2) -> f64 {
        self.runs
            .iter()
            .flatten()
            .map(|&q| dist(p, q))
            .fold(f64::INFINITY, f64::min)
    }

    /// Smallest distance from a rectangle to the line, page points.
    pub fn distance_rect_pt(&self, r: &[f64; 4]) -> f64 {
        self.runs
            .iter()
            .flatten()
            .map(|q| {
                let dx = (r[0] - q[0]).max(q[0] - r[2]).max(0.0);
                let dy = (r[1] - q[1]).max(q[1] - r[3]).max(0.0);
                dx.hypot(dy)
            })
            .fold(f64::INFINITY, f64::min)
    }
}

type Seg = GeomWithData<Line<P2>, P2>;

/// OSM piste ways carried onto the plan, segments with their unit direction.
fn osm_on_plan(reg: &Registration) -> RTree<Seg> {
    let mut segs = Vec::new();
    for w in &reg.osm.ways {
        let pts: Vec<P2> = w.xy.iter().map(|&q| reg.warp.to_plan(q)).collect();
        for s in pts.windows(2) {
            let l = dist(s[0], s[1]);
            if l > 0.0 {
                let u = [(s[1][0] - s[0][0]) / l, (s[1][1] - s[0][1]) / l];
                segs.push(GeomWithData::new(Line::new(s[0], s[1]), u));
            }
        }
    }
    RTree::bulk_load(segs)
}

fn resample(pts: &[P2], step: f64) -> Vec<P2> {
    let mut out = Vec::new();
    if pts.is_empty() {
        return out;
    }
    out.push(pts[0]);
    let mut carry = 0.0;
    for s in pts.windows(2) {
        let l = dist(s[0], s[1]);
        let mut t = step - carry;
        while t <= l {
            let f = t / l;
            out.push([
                s[0][0] + (s[1][0] - s[0][0]) * f,
                s[0][1] + (s[1][1] - s[0][1]) * f,
            ]);
            t += step;
        }
        carry = (carry + l) % step;
    }
    if out.last() != pts.last() {
        out.extend(pts.last().copied());
    }
    out
}

fn find(parent: &mut [usize], i: usize) -> usize {
    let mut r = i;
    while parent[r] != r {
        r = parent[r];
    }
    let mut c = i;
    while parent[c] != r {
        let n = parent[c];
        parent[c] = r;
        c = n;
    }
    r
}

fn ground_length(reg: &Registration, runs: &[Vec<P2>]) -> f64 {
    runs.iter()
        .map(|r| {
            let m: Vec<P2> = r.iter().map(|&p| reg.warp.to_map(p)).collect();
            m.windows(2).map(|w| dist(w[0], w[1])).sum::<f64>()
        })
        .sum()
}

/// Lines drawn in each piste ink where no OSM piste way runs (step 1).
pub fn drawn_lines(reg: &Registration) -> Vec<DrawnLine> {
    drawn_lines_with(reg, &Params::default())
}

/// [`drawn_lines`] with the settings `prm`.
pub fn drawn_lines_with(reg: &Registration, prm: &Params) -> Vec<DrawnLine> {
    let osm = osm_on_plan(reg);
    let cos_max = MAX_ANGLE_DEG.to_radians().cos();
    let k = reg.input.pt_per_px;
    let mut out = Vec::new();
    for (ink, g) in &reg.graphs {
        let mut runs: Vec<Vec<P2>> = Vec::new();
        for e in &g.edges {
            let pts: Vec<P2> = e.pts.iter().map(|p| [p[0] * k, p[1] * k]).collect();
            let s = resample(&pts, SAMPLE_PT);
            let mut cur: Vec<P2> = Vec::new();
            for i in 0..s.len() {
                let (a, b) = (s[i.saturating_sub(2)], s[(i + 2).min(s.len() - 1)]);
                let l = dist(a, b);
                let t = if l > 0.0 {
                    [(b[0] - a[0]) / l, (b[1] - a[1]) / l]
                } else {
                    [1.0, 0.0]
                };
                let p = s[i];
                let explained = !reg.spec.on_map(p[0], p[1])
                    || osm
                        .locate_within_distance(p, prm.explained_pt * prm.explained_pt)
                        .any(|g| (g.data[0] * t[0] + g.data[1] * t[1]).abs() >= cos_max);
                if explained {
                    if cur.len() >= 2 {
                        runs.push(std::mem::take(&mut cur));
                    }
                    cur.clear();
                } else {
                    cur.push(p);
                }
            }
            if cur.len() >= 2 {
                runs.push(cur);
            }
        }
        // Runs whose ends are close are one line.
        let mut parent: Vec<usize> = (0..runs.len()).collect();
        let ends: Vec<[P2; 2]> = runs.iter().map(|r| [r[0], r[r.len() - 1]]).collect();
        let tree = RTree::bulk_load(
            ends.iter()
                .enumerate()
                .flat_map(|(i, e)| [GeomWithData::new(e[0], i), GeomWithData::new(e[1], i)])
                .collect::<Vec<_>>(),
        );
        for (i, e) in ends.iter().enumerate() {
            for p in e {
                for n in tree.locate_within_distance(*p, JOIN_GAP_PT * JOIN_GAP_PT) {
                    let (a, b) = (find(&mut parent, i), find(&mut parent, n.data));
                    if a != b {
                        parent[a] = b;
                    }
                }
            }
        }
        let mut groups: BTreeMap<usize, Vec<Vec<P2>>> = BTreeMap::new();
        for (i, r) in runs.into_iter().enumerate() {
            let root = find(&mut parent, i);
            groups.entry(root).or_default().push(r);
        }
        for runs in groups.into_values() {
            let length_m = ground_length(reg, &runs);
            if length_m >= MIN_DRAWN_M {
                out.push(DrawnLine {
                    ink: *ink,
                    runs,
                    length_m,
                });
            }
        }
    }
    out
}

/// Where a piste was found on the plan.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Anchor {
    Label(f64),
    Box(f64),
}

impl Anchor {
    fn weight(self) -> f64 {
        match self {
            Anchor::Label(_) => ANCHOR_LABEL,
            Anchor::Box(_) => ANCHOR_BOX,
        }
    }
    fn distance(self) -> f64 {
        match self {
            Anchor::Label(d) | Anchor::Box(d) => d,
        }
    }
    fn name(self) -> &'static str {
        match self {
            Anchor::Label(_) => "map_label",
            Anchor::Box(_) => "legend_box",
        }
    }
}

/// Whether a text line of the map writes the piste's name.
fn writes_name(src: LabelSource, text: &str, name: &str) -> bool {
    let (kt, kn) = (name_key(text), name_key(name));
    match src {
        LabelSource::TextLayer => (!kn.is_empty() && kt == kn) || fold(text) == fold(name),
        LabelSource::Ocr => {
            !kn.is_empty()
                && similarity(&kt.join(" "), &kn.join(" ")).max(similarity(text, name)) >= 0.8
        }
    }
}

fn inside(r: &[f64; 4], p: P2) -> bool {
    p[0] >= r[0] && p[0] < r[2] && p[1] >= r[1] && p[1] < r[3]
}

/// An estimated trace.
#[derive(Debug, Clone, Serialize)]
pub struct Estimated {
    /// Index of the decision (piste of the plan) in the decisions given.
    pub decision: usize,
    /// Method (`estimated_paths` or `estimated_drawing`).
    pub method: MatchMethod,
    /// Score, 0 to 1.
    pub score: f64,
    /// Geometry, Lambert-93 lines.
    #[serde(skip)]
    pub lines: Vec<Vec<P2>>,
    /// Geometry length, m.
    pub length_m: f64,
    /// The plan's line of the piste, page points (local diagnostics).
    #[serde(skip)]
    pub drawn: Vec<Vec<P2>>,
    /// Details.
    pub estimate: Estimate,
    /// Paths used (`ign/<cleabs>`).
    pub paths: Vec<String>,
}

/// A piste of the plan for which nothing is estimated, and why.
#[derive(Debug, Clone, Serialize)]
pub struct NotEstimated {
    /// Name as read.
    pub name: String,
    /// Why.
    pub reason: String,
}

/// Outcome of the estimation of a plan.
#[derive(Debug, Clone, Default, Serialize)]
pub struct Outcome {
    /// Estimated traces.
    pub estimated: Vec<Estimated>,
    /// Pistes left without trace.
    pub not_estimated: Vec<NotEstimated>,
    /// Drawn lines without OSM piste (all inks).
    pub drawn_lines: usize,
    /// Corridor half width, m.
    pub corridor_m: f64,
    /// Candidate paths read.
    pub paths: usize,
}

/// Corridor half width of a registration: p90 of the held-out junction errors, bounded.
pub fn corridor_m(reg: &Registration) -> f64 {
    quantile(&reg.held_out_m, 0.9)
        .unwrap_or(MAX_CORRIDOR_M)
        .clamp(MIN_CORRIDOR_M, MAX_CORRIDOR_M)
}

/// Words of a legend line that name no piste (« Plan des pistes », « 5 km »).
const NOT_A_NAME: [&str; 14] = [
    "piste", "pistes", "plan", "km", "des", "les", "la", "le", "du", "de", "et", "boucle",
    "depart", "liaison",
];

/// Whether a legend line reads as the name of a piste: a word that is not
/// [`NOT_A_NAME`], of at least four letters or capitalised (« Le Gy »). Measured on six massifs (#117): fragments of the text
/// layer or of the OCR (« iè e », « km sa », « des pistes ») were estimated as pistes and
/// all were wrong; an estimated trace is shown under its name, a fragment never is.
pub fn plausible_name(name: &str) -> bool {
    name.split(|c: char| !c.is_alphabetic()).any(|w| {
        let n = w.chars().count();
        let capital = w.chars().next().is_some_and(char::is_uppercase);
        (n >= 4 || (capital && n >= 2)) && !NOT_A_NAME.contains(&fold(w).as_str())
    })
}

/// Pistes to estimate: not transcribed, a piste ink (ski, link, shared track), a name.
fn wanted(reg: &Registration, d: &Decision) -> Option<usize> {
    let ink = d.entry.ink?;
    if d.status.is_some() || !plausible_name(&d.entry.name_read) {
        return None;
    }
    let kind = reg.spec.inks.get(ink)?.kind;
    matches!(
        kind,
        PisteKind::Ski | PisteKind::Liaison | PisteKind::MultiActivity
    )
    .then_some(ink)
}

/// Assigns drawn lines to the pistes left by the matching (step 2): decision index →
/// (line index, anchor).
fn assign(
    reg: &Registration,
    prm: &Params,
    decisions: &[Decision],
    lines: &[DrawnLine],
    not: &mut Vec<NotEstimated>,
) -> BTreeMap<usize, (usize, Anchor)> {
    let wanted_ds: Vec<(usize, usize)> = decisions
        .iter()
        .enumerate()
        .filter_map(|(i, d)| wanted(reg, d).map(|ink| (i, ink)))
        .collect();
    let map_area = reg.spec.map_area;
    // Candidates of each piste: (line, anchor).
    let mut cands: Vec<(usize, usize, Anchor)> = Vec::new();
    for &(i, ink) in &wanted_ds {
        let d = &decisions[i];
        let name = &d.entry.name_read;
        let labels: Vec<&crate::input::TextBox> = reg
            .map_labels
            .iter()
            .filter(|t| {
                let c = t.center();
                !inside(&d.entry.rect, c) && writes_name(reg.spec.labels, &t.text, name)
            })
            .collect();
        let mut best: Option<(usize, Anchor)> = None;
        for t in &labels {
            for (li, l) in lines.iter().enumerate().filter(|(_, l)| l.ink == ink) {
                let dd = l.distance_rect_pt(&t.rect);
                if dd <= prm.label_radius_pt && best.is_none_or(|(_, a)| dd < a.distance()) {
                    best = Some((li, Anchor::Label(dd)));
                }
            }
        }
        if best.is_none() {
            // Departure box on the map: the only piste of its ink without OSM piste
            // listed beside it, and one line of that ink starting near it.
            let c = [
                (d.entry.rect[0] + d.entry.rect[2]) / 2.0,
                (d.entry.rect[1] + d.entry.rect[3]) / 2.0,
            ];
            let on_map = inside(&map_area, c);
            let alone = wanted_ds
                .iter()
                .filter(|&&(j, ink2)| {
                    j != i && ink2 == ink && {
                        let r = &decisions[j].entry.rect;
                        dist(c, [(r[0] + r[2]) / 2.0, (r[1] + r[3]) / 2.0])
                            <= 4.0 * prm.box_radius_pt
                    }
                })
                .count()
                == 0;
            if on_map && alone {
                let near: Vec<(usize, f64)> = lines
                    .iter()
                    .enumerate()
                    .filter(|(_, l)| l.ink == ink)
                    .map(|(li, l)| (li, l.distance_rect_pt(&d.entry.rect)))
                    .filter(|&(_, dd)| dd <= prm.box_radius_pt)
                    .collect();
                if near.len() == 1 {
                    best = Some((near[0].0, Anchor::Box(near[0].1)));
                } else if near.len() > 1 {
                    not.push(NotEstimated {
                        name: name.clone(),
                        reason: format!(
                            "{} lignes de l'encre partent de l'encadré du départ : ambigu",
                            near.len()
                        ),
                    });
                    continue;
                }
            }
        }
        match best {
            Some((li, a)) => cands.push((i, li, a)),
            None => not.push(NotEstimated {
                name: name.clone(),
                reason: if labels.is_empty() {
                    "nom non écrit sur la carte et pas d'encadré de départ propre à la piste : ligne du plan non retrouvée".to_owned()
                } else {
                    "nom écrit sur la carte, mais aucune ligne de l'encre sans piste OSM à côté".to_owned()
                },
            }),
        }
    }
    // A line claimed by two pistes goes to the better anchor (label first, then nearer).
    let mut by_line: HashMap<usize, Vec<(usize, Anchor)>> = HashMap::new();
    for (i, li, a) in cands {
        by_line.entry(li).or_default().push((i, a));
    }
    let mut out = BTreeMap::new();
    for (li, mut v) in by_line {
        v.sort_by(|x, y| {
            y.1.weight()
                .total_cmp(&x.1.weight())
                .then(x.1.distance().total_cmp(&y.1.distance()))
        });
        out.insert(v[0].0, (li, v[0].1));
        for &(i, _) in &v[1..] {
            not.push(NotEstimated {
                name: decisions[i].entry.name_read.clone(),
                reason: format!(
                    "la ligne trouvée est aussi celle de « {} », plus proche de son nom",
                    decisions[v[0].0].entry.name_read
                ),
            });
        }
    }
    out
}

type PathPoint = GeomWithData<P2, (usize, usize)>;

/// Follows a carried line on the candidate paths (step 3): kept runs of path samples,
/// share of the line followed, paths used.
pub fn follow(line: &[Vec<P2>], paths: &[Path], corridor: f64) -> (Vec<Vec<P2>>, f64, Vec<usize>) {
    let drawn: Vec<P2> = line.iter().flat_map(|r| resample(r, PATH_STEP_M)).collect();
    if drawn.is_empty() {
        return (Vec::new(), 0.0, Vec::new());
    }
    let dtree = RTree::bulk_load(
        drawn
            .iter()
            .enumerate()
            .map(|(i, &p)| GeomWithData::new(p, i))
            .collect::<Vec<_>>(),
    );
    // Path samples near the line.
    let mut samples: Vec<Vec<P2>> = Vec::with_capacity(paths.len());
    let mut pts: Vec<PathPoint> = Vec::new();
    for (pi, p) in paths.iter().enumerate() {
        let s = resample(&p.xy, PATH_STEP_M);
        let near = s.iter().any(|&q| {
            dtree
                .locate_within_distance(q, corridor * corridor)
                .next()
                .is_some()
        });
        if near {
            for (j, &q) in s.iter().enumerate() {
                pts.push(GeomWithData::new(q, (pi, j)));
            }
        }
        samples.push(s);
    }
    let ptree = RTree::bulk_load(pts);
    let penalty = |pi: usize| 1.0 + 5.0 * (paths[pi].grade - GRADE_FREE).max(0.0);
    // Each line point picks the nearest path (distance × slope penalty).
    let picks: Vec<Option<(usize, f64)>> = drawn
        .iter()
        .map(|&p| {
            ptree
                .locate_within_distance(p, corridor * corridor)
                .map(|q| (q.data.0, q.geom().distance_2(&p).sqrt() * penalty(q.data.0)))
                .min_by(|a, b| a.1.total_cmp(&b.1))
        })
        .collect();
    let covered = picks
        .iter()
        .filter(|p| p.is_some_and(|(_, d)| d <= corridor / 2.0))
        .count() as f64
        / drawn.len() as f64;
    // A path sample is kept when the nearest line point picked its path.
    let mut out = Vec::new();
    let mut used = Vec::new();
    for (pi, s) in samples.iter().enumerate() {
        let mut run: Vec<P2> = Vec::new();
        let mut any = false;
        for &q in s {
            let keep = dtree
                .nearest_neighbor(&q)
                .filter(|n| n.geom().distance_2(&q).sqrt() <= corridor)
                .is_some_and(|n| picks[n.data].is_some_and(|(p, _)| p == pi));
            if keep {
                run.push(q);
            } else {
                if run.len() >= 2 {
                    out.push(std::mem::take(&mut run));
                    any = true;
                }
                run.clear();
            }
        }
        if run.len() >= 2 {
            out.push(run);
            any = true;
        }
        if any {
            used.push(pi);
        }
    }
    (out, covered, used)
}

/// Bounding box (WGS84) of Lambert-93 lines, grown by `margin_m`.
fn bbox_of(lines: &[&[P2]], margin_m: f64) -> Option<BBox> {
    let pts: Vec<P2> = lines.iter().flat_map(|l| l.iter().copied()).collect();
    if pts.is_empty() {
        return None;
    }
    let (mut x0, mut y0, mut x1, mut y1) = (f64::MAX, f64::MAX, f64::MIN, f64::MIN);
    for p in &pts {
        x0 = x0.min(p[0]);
        y0 = y0.min(p[1]);
        x1 = x1.max(p[0]);
        y1 = y1.max(p[1]);
    }
    let a = from_lambert93(x0 - margin_m, y0 - margin_m).ok()?;
    let b = from_lambert93(x1 + margin_m, y1 + margin_m).ok()?;
    let c = from_lambert93(x0 - margin_m, y1 + margin_m).ok()?;
    let d = from_lambert93(x1 + margin_m, y0 - margin_m).ok()?;
    Some(BBox::new(
        a.lat.min(d.lat),
        a.lon.min(c.lon),
        b.lat.max(c.lat),
        b.lon.max(d.lon),
    ))
}

fn polyline_m(l: &[P2]) -> f64 {
    l.windows(2).map(|w| dist(w[0], w[1])).sum()
}

/// Estimates the traces of the pistes the matching left (see the module documentation).
/// `paths_of` gives the candidate paths of a box (BD TOPO through the downloader in the
/// pipeline, fixed paths in the tests).
pub fn estimate(
    reg: &Registration,
    decisions: &[Decision],
    paths_of: &mut dyn FnMut(&BBox) -> Result<Vec<Path>>,
) -> Result<Outcome> {
    estimate_with(reg, decisions, &Params::default(), paths_of)
}

/// [`estimate`] with the settings `prm`.
pub fn estimate_with(
    reg: &Registration,
    decisions: &[Decision],
    prm: &Params,
    paths_of: &mut dyn FnMut(&BBox) -> Result<Vec<Path>>,
) -> Result<Outcome> {
    let mut out = Outcome {
        corridor_m: corridor_m(reg),
        ..Outcome::default()
    };
    // A registration that no exact match confirms is not trusted to carry a line onto
    // the ground: on the plans measured (#117) every trace estimated on such a plan was
    // wrong (lines carried hundreds of metres away).
    if !decisions
        .iter()
        .any(|d| d.status.is_some() && d.method == Some(MatchMethod::Exact))
    {
        for d in decisions.iter().filter(|d| wanted(reg, d).is_some()) {
            out.not_estimated.push(NotEstimated {
                name: d.entry.name_read.clone(),
                reason: "calage du plan confirmé par aucune piste appariée exactement : pas de tracé estimé".to_owned(),
            });
        }
        return Ok(out);
    }
    let lines = drawn_lines_with(reg, prm);
    out.drawn_lines = lines.len();
    let assigned = assign(reg, prm, decisions, &lines, &mut out.not_estimated);
    if assigned.is_empty() {
        return Ok(out);
    }
    let corridor = out.corridor_m;
    let carried: BTreeMap<usize, Vec<Vec<P2>>> = assigned
        .iter()
        .map(|(&i, &(li, _))| {
            (
                i,
                lines[li]
                    .runs
                    .iter()
                    .map(|r| r.iter().map(|&p| reg.warp.to_map(p)).collect())
                    .collect(),
            )
        })
        .collect();
    let all: Vec<&[P2]> = carried.values().flatten().map(Vec::as_slice).collect();
    let paths = match bbox_of(&all, corridor + 100.0) {
        Some(b) => paths_of(&b)?,
        None => Vec::new(),
    };
    out.paths = paths.len();
    let quality = (-corridor / 400.0).exp();
    for (&i, &(li, anchor)) in &assigned {
        let line = &carried[&i];
        let (pieces, covered, used) = follow(line, &paths, corridor);
        let (method, geom, sources, fit) = if covered >= prm.min_covered && !pieces.is_empty() {
            (
                MatchMethod::EstimatedPaths,
                pieces,
                vec!["ign_bdtopo".to_owned()],
                covered,
            )
        } else {
            (
                MatchMethod::EstimatedDrawing,
                line.iter().map(|r| resample(r, PATH_STEP_M)).collect(),
                vec!["plan_drawing".to_owned()],
                0.5,
            )
        };
        let score = anchor.weight() * quality * fit;
        if score < prm.min_score {
            out.not_estimated.push(NotEstimated {
                name: decisions[i].entry.name_read.clone(),
                reason: format!(
                    "tracé estimé trop incertain : score {score:.3} < {} (ligne suivie sur {:.0} % par les chemins)",
                    prm.min_score,
                    covered * 100.0
                ),
            });
            continue;
        }
        out.estimated.push(Estimated {
            decision: i,
            method,
            score,
            length_m: geom.iter().map(|l| polyline_m(l)).sum(),
            lines: geom,
            drawn: lines[li].runs.clone(),
            estimate: Estimate {
                anchor: anchor.name().to_owned(),
                corridor_m: corridor.round(),
                drawn_covered: (covered * 1000.0).round() / 1000.0,
                drawn_length_m: lines[li].length_m.round(),
                sources,
            },
            paths: used.iter().map(|&p| paths[p].id.clone()).collect(),
        });
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resample_keeps_ends_and_spacing() {
        let r = resample(&[[0.0, 0.0], [10.0, 0.0]], 2.0);
        assert_eq!(r.len(), 6);
        assert_eq!(r[5], [10.0, 0.0]);
    }

    #[test]
    fn follow_picks_the_path_under_the_line_and_skips_the_steep_one() {
        // Carried line along y = 20 from x = 0 to 1000; a track along y = 0 (20 m off),
        // a steep path along y = 30 (10 m off but climbing 40 %: 10 m × 2.4 > 20 m), and a road far away.
        let line = vec![vec![[0.0, 20.0], [1000.0, 20.0]]];
        let paths = vec![
            Path {
                id: "ign/track".into(),
                nature: "Chemin".into(),
                xy: vec![[-50.0, 0.0], [1050.0, 0.0]],
                grade: 0.03,
            },
            Path {
                id: "ign/steep".into(),
                nature: "Sentier".into(),
                xy: vec![[0.0, 30.0], [1000.0, 30.0]],
                grade: 0.40,
            },
            Path {
                id: "ign/far".into(),
                nature: "Route à 1 chaussée".into(),
                xy: vec![[0.0, 900.0], [1000.0, 900.0]],
                grade: 0.0,
            },
        ];
        let (pieces, covered, used) = follow(&line, &paths, 80.0);
        assert_eq!(used, vec![0]);
        assert!(covered > 0.95, "{covered}");
        let len: f64 = pieces.iter().map(|p| polyline_m(p)).sum();
        // The track is kept under the line and up to the corridor beyond its ends.
        assert!((980.0..=1160.0).contains(&len), "{len}");
    }

    #[test]
    fn follow_reports_a_line_without_paths() {
        let line = vec![vec![[0.0, 0.0], [500.0, 0.0]]];
        let paths = vec![Path {
            id: "ign/x".into(),
            nature: "Chemin".into(),
            xy: vec![[0.0, 500.0], [500.0, 500.0]],
            grade: 0.0,
        }];
        let (pieces, covered, used) = follow(&line, &paths, 80.0);
        assert!(pieces.is_empty() && used.is_empty());
        assert_eq!(covered, 0.0);
    }

    #[test]
    fn bd_topo_paths_are_parsed_with_their_grade() {
        let v = serde_json::json!({"features": [
            {"geometry": {"type": "LineString", "coordinates": [[6.10, 46.60, 1000.0], [6.101, 46.60, 1010.0]]},
             "properties": {"cleabs": "A", "nature": "Chemin", "etat_de_l_objet": "En service"}},
            {"geometry": {"type": "LineString", "coordinates": [[6.10, 46.60, 1000.0], [6.101, 46.60, 1000.0]]},
             "properties": {"cleabs": "B", "nature": "Type autoroutier", "etat_de_l_objet": "En service"}},
            {"geometry": {"type": "LineString", "coordinates": [[6.10, 46.60], [6.101, 46.60]]},
             "properties": {"cleabs": "C", "nature": "Sentier", "etat_de_l_objet": "En projet"}}
        ]});
        let p = parse_paths(&v).unwrap();
        assert_eq!(p.len(), 1);
        assert_eq!(p[0].id, "ign/A");
        // 10 m over ~77 m.
        assert!((p[0].grade - 0.13).abs() < 0.01, "{}", p[0].grade);
    }

    #[test]
    fn fragments_are_not_names() {
        for bad in ["iè e", "km sa", "des pistes", "Plan des pistes", "5,5 km"] {
            assert!(!plausible_name(bad), "{bad}");
        }
        for ok in [
            "La Lanterne",
            "Bougaud",
            "Pré Tardy",
            "Liaison Mouthe - Chez Liadet",
            "Le Gy",
        ] {
            assert!(plausible_name(ok), "{ok}");
        }
    }

    #[test]
    fn names_on_the_map() {
        assert!(writes_name(LabelSource::TextLayer, "Gy", "Le Gy"));
        assert!(writes_name(
            LabelSource::TextLayer,
            "La Lanterne",
            "La Lanterne"
        ));
        assert!(!writes_name(
            LabelSource::TextLayer,
            "Lanternes",
            "La Lanterne"
        ));
        assert!(writes_name(LabelSource::Ocr, "la lanteme", "La Lanterne"));
    }
}
