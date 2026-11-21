//! Navigation on a bearing in the fog (K7 #57, F26), and the guided outings without a day
//! plan: the bearing to a point and the off-piste route followed as a line (V1 #64,
//! [`crate::offpiste`]).
//!
//! The app's location service (module `:tracking`) follows a guided outing exactly as it
//! follows a day plan: it opens a [`GuidedSession`] on a directory written here
//! ([`guided_bearing`], [`crate::offpiste::OffpistePlanner::guide`]) instead of a
//! `TrackingSession`, feeds it the same batches of fixes and gets back the same
//! [`BatchOut`] (what to say, alerts, GPS mode, guidance of the banner and of the
//! lock-screen notification). The screen may stay off: every announcement is decided
//! here from the GPS alone (distance, bearing, direction of travel), never from the
//! compass of a phone in a pocket; the compass is only used by the screen
//! ([`fog_bearing`] with the heading of the phone).
//!
//! What is said is sober: the bearing and the distance at the start, when crossing 1 km,
//! 500 m, 200 m and 100 m, every [`GuidedSpec::period_s`] (3 min by default), a drift of
//! the direction of travel of more than [`DRIFT_DEG`] held [`DRIFT_S`], never two
//! announcements less than [`MIN_GAP_S`] apart, and the arrival.
//!
//! Every input is appended to `inputs.jsonl` (the format of the day plan's tracking): the
//! trace (GPX at the end), the recap of the outing (A10, [`crate::recap`]) and the state
//! after a restart of the process (inputs replayed, nothing said again).

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use jiff::Timestamp;
use serde::{Deserialize, Serialize};
use skimap_common::geom::LonLat;
use skimap_core::calibration::{ProfileModel, TERRAINS};
use skimap_core::eta::EtaModel;
use skimap_core::huts;
use skimap_core::routing::Style;
use skimap_pack::Pack;
use skimap_plan::gps::{GpsInput, GpsMode, GpsParams, GpsRequest, GpsState};
use skimap_plan::pace::duration_fr;
use skimap_plan::tracker::Input;
use skimap_voice::clips::Direction;

use crate::PackError;
use crate::card::Lang;
use crate::text;
use crate::tracking::{
    AlertKind, AlertView, BatchOut, GpsModeView, GpsRequestView, GuidanceView, INPUTS_FILE,
    NextTurn, PhaseView, RecapView, SpeechPriority, SpokenView, StepReport, TrackEvent,
    TrackingError, TurnDirection, back_view, direction_view,
};
use crate::tracking_back::{Backtrack, Target, TargetKind, WayKind};

/// Description of a guided outing, written in its directory.
pub const GUIDED_FILE: &str = "guided.json";
/// Arrived within this of the point, metres.
pub const ARRIVE_M: f64 = 30.0;
/// Drift of the direction of travel said above this, degrees…
pub const DRIFT_DEG: f64 = 45.0;
/// …held this long, seconds.
pub const DRIFT_S: f64 = 20.0;
/// Two announcements (other than the arrival) at least this far apart, seconds.
pub const MIN_GAP_S: f64 = 20.0;
/// Period of the reminders by default, seconds.
pub const DEFAULT_PERIOD_S: f64 = 180.0;
/// Distances announced when crossed, metres.
pub const STEPS_M: [f64; 4] = [1000.0, 500.0, 200.0, 100.0];
/// The direction of travel is measured over this distance, metres.
pub const COURSE_M: f64 = 25.0;
/// Moving above this speed, m/s.
pub const MOVING_MS: f64 = 0.6;
/// A point of the recorded trace every this, metres.
const TRACE_STEP_M: f64 = 10.0;

fn invalid(detail: impl std::fmt::Display) -> TrackingError {
    TrackingError::Invalid {
        detail: detail.to_string(),
    }
}

fn utc(ms: i64) -> Timestamp {
    Timestamp::from_millisecond(ms).ok().unwrap_or_default()
}

// ---------------------------------------------------------------------------------------
// Bearing and words
// ---------------------------------------------------------------------------------------

/// Initial great-circle bearing from `a` to `b`, degrees clockwise from true north, 0–360.
pub fn bearing_deg(a: LonLat, b: LonLat) -> f64 {
    let (p1, p2) = (a.lat.to_radians(), b.lat.to_radians());
    let dl = (b.lon - a.lon).to_radians();
    let y = dl.sin() * p2.cos();
    let x = p1.cos() * p2.sin() - p1.sin() * p2.cos() * dl.cos();
    y.atan2(x).to_degrees().rem_euclid(360.0)
}

/// Signed angle from `from` to `to`, degrees, −180 to 180 (positive: to the right).
pub fn relative_deg(from: f64, to: f64) -> f64 {
    let d = (to - from).rem_euclid(360.0);
    if d > 180.0 { d - 360.0 } else { d }
}

/// Eight-point compass word of a bearing.
pub fn cardinal(deg: f64, lang: Lang) -> &'static str {
    const FR: [&str; 8] = [
        "nord",
        "nord-est",
        "est",
        "sud-est",
        "sud",
        "sud-ouest",
        "ouest",
        "nord-ouest",
    ];
    const EN: [&str; 8] = [
        "north",
        "north-east",
        "east",
        "south-east",
        "south",
        "south-west",
        "west",
        "north-west",
    ];
    let i = ((deg.rem_euclid(360.0) + 22.5) / 45.0).floor() as usize % 8;
    match lang {
        Lang::Fr => FR[i],
        Lang::En => EN[i],
    }
}

/// « 850 mètres », « 1,2 kilomètre », « 3 kilomètres » (spoken and written).
pub fn distance_words(m: f64, lang: Lang) -> String {
    let m = m.max(0.0);
    if m < 1000.0 {
        let r = if m < 100.0 {
            (m / 5.0).round() * 5.0
        } else {
            (m / 10.0).round() * 10.0
        };
        match lang {
            Lang::Fr => format!("{r:.0} mètres"),
            Lang::En => format!("{r:.0} metres"),
        }
    } else {
        let km = (m / 100.0).round() / 10.0;
        let whole = km.fract() == 0.0;
        let n = if whole {
            format!("{km:.0}")
        } else {
            format!("{km:.1}")
        };
        match lang {
            Lang::Fr => {
                let unit = if km < 2.0 {
                    "kilomètre"
                } else {
                    "kilomètres"
                };
                format!("{} {unit}", n.replace('.', ","))
            }
            Lang::En => format!("{n} kilometres"),
        }
    }
}

/// Direction of the turn of a relative angle (positive: right).
pub fn turn_of(rel: f64) -> Direction {
    let a = rel.abs();
    match (a, rel > 0.0) {
        (a, _) if a < 20.0 => Direction::Straight,
        (a, true) if a < 60.0 => Direction::SlightRight,
        (a, false) if a < 60.0 => Direction::SlightLeft,
        (a, true) if a <= 135.0 => Direction::Right,
        (a, false) if a <= 135.0 => Direction::Left,
        _ => Direction::UTurn,
    }
}

/// Words of a turn: « tout droit », « légèrement à droite », « demi-tour ».
pub fn turn_words(d: Direction, lang: Lang) -> &'static str {
    match (d, lang) {
        (Direction::Straight, Lang::Fr) => "tout droit",
        (Direction::SlightRight | Direction::KeepRight, Lang::Fr) => "légèrement à droite",
        (Direction::SlightLeft | Direction::KeepLeft, Lang::Fr) => "légèrement à gauche",
        (Direction::Right, Lang::Fr) => "à droite",
        (Direction::Left, Lang::Fr) => "à gauche",
        (Direction::UTurn, Lang::Fr) => "demi-tour",
        (Direction::Straight, Lang::En) => "straight on",
        (Direction::SlightRight | Direction::KeepRight, Lang::En) => "bear right",
        (Direction::SlightLeft | Direction::KeepLeft, Lang::En) => "bear left",
        (Direction::Right, Lang::En) => "turn right",
        (Direction::Left, Lang::En) => "turn left",
        (Direction::UTurn, Lang::En) => "turn back",
    }
}

/// Bearing and distance from a position to a point, with the turn to make when the
/// heading of the phone (compass, screen on) or the direction of travel is known.
#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct BearingView {
    /// Distance, metres (great circle).
    pub distance_m: f64,
    /// Bearing to the point, degrees from true north.
    pub bearing_deg: f64,
    /// « sud-ouest ».
    pub cardinal: String,
    /// « 1,2 kilomètre ».
    pub distance_text: String,
    /// « cap 245°, sud-ouest ».
    pub bearing_text: String,
    /// Angle from the heading to the point, degrees (positive: right), when a heading was
    /// given.
    pub relative_deg: Option<f64>,
    /// Turn to make, when a heading was given.
    pub turn: Option<TurnDirection>,
    /// « légèrement à gauche », when a heading was given.
    pub turn_text: Option<String>,
}

/// Bearing and distance from (`from_lon`, `from_lat`) to (`to_lon`, `to_lat`);
/// `heading_deg`: heading of the phone or direction of travel, degrees from true north.
#[uniffi::export]
pub fn fog_bearing(
    from_lon: f64,
    from_lat: f64,
    to_lon: f64,
    to_lat: f64,
    heading_deg: Option<f64>,
    lang: Lang,
) -> BearingView {
    let (a, b) = (LonLat::new(from_lon, from_lat), LonLat::new(to_lon, to_lat));
    let d = a.haversine_m(b);
    let br = bearing_deg(a, b);
    let rel = heading_deg.map(|h| relative_deg(h, br));
    let turn = rel.map(turn_of);
    BearingView {
        distance_m: d,
        bearing_deg: br,
        cardinal: cardinal(br, lang).to_owned(),
        distance_text: distance_words(d, lang),
        bearing_text: bearing_words(br, lang),
        relative_deg: rel,
        turn: turn.map(direction_view),
        turn_text: turn.map(|t| turn_words(t, lang).to_owned()),
    }
}

/// « cap 245°, sud-ouest ».
fn bearing_words(br: f64, lang: Lang) -> String {
    let deg = br.round().rem_euclid(360.0);
    match lang {
        Lang::Fr => format!("cap {deg:.0}°, {}", cardinal(br, lang)),
        Lang::En => format!("bearing {deg:.0}°, {}", cardinal(br, lang)),
    }
}

/// Spoken bearing: « cap 245 degrés, sud-ouest ».
fn bearing_spoken(br: f64, lang: Lang) -> String {
    let deg = br.round().rem_euclid(360.0);
    match lang {
        Lang::Fr => format!("cap {deg:.0} degrés, {}", cardinal(br, lang)),
        Lang::En => format!("bearing {deg:.0} degrees, {}", cardinal(br, lang)),
    }
}

// ---------------------------------------------------------------------------------------
// Points to go to
// ---------------------------------------------------------------------------------------

/// A point the skier may go to on a bearing (or off-piste).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, uniffi::Record)]
pub struct FogTarget {
    /// `refuge`, `parking`, `start` (start of the area's loops), `point` (chosen on the map).
    pub kind: String,
    /// Name.
    pub name: String,
    /// Longitude.
    pub lon: f64,
    /// Latitude.
    pub lat: f64,
    /// From the position given, metres.
    pub distance_m: Option<f64>,
}

pub(crate) fn point_of(f: &geojson::Feature) -> Option<LonLat> {
    match f.geometry.as_ref()?.value {
        geojson::GeometryValue::Point { ref coordinates } if coordinates.len() >= 2 => {
            Some(LonLat::new(coordinates[0], coordinates[1]))
        }
        _ => None,
    }
}

pub(crate) fn prop<'a>(f: &'a geojson::Feature, k: &str) -> Option<&'a str> {
    f.properties.as_ref()?.get(k)?.as_str()
}

/// Refuges of a pack, with their name: those of the day plan, flagged in the
/// `refuges-info` and `refuges-osm` layers every pack holds ([`huts::PLAN_FLAG`], X21
/// #155: the same hut in both sources once), read as they are; layers of a pack built
/// before the flag are flagged here ([`huts::mark`]). A pack with neither layer falls back
/// to the merged sheets (`refuges`) when it has them.
fn plan_refuges(pack: &Pack) -> Result<Vec<(LonLat, Option<String>)>, PackError> {
    let read = |id: &str| -> Result<serde_json::Value, PackError> {
        if pack.layer(id).is_ok() {
            Ok(pack.read_json(id)?)
        } else {
            Ok(serde_json::Value::Null)
        }
    };
    let (mut info, mut osm) = (read("refuges-info")?, read("refuges-osm")?);
    let name = |v: &serde_json::Value| {
        v.as_str()
            .map(str::trim)
            .filter(|n| !n.is_empty())
            .map(str::to_owned)
    };
    let mut out = Vec::new();
    if info.is_null() && osm.is_null() {
        if pack.layer("refuges").is_ok() {
            for f in pack.read_geojson("refuges")?.features {
                let Some(p) = point_of(&f) else { continue };
                let n = prop(&f, "name").or_else(|| prop(&f, "kind_label"));
                out.push((p, n.map(str::to_owned)));
            }
        }
        return Ok(out);
    }
    if !(huts::marked(&info) && huts::marked(&osm)) {
        huts::mark(&mut info, &mut osm);
    }
    let flagged = |v: &serde_json::Value| -> Vec<(LonLat, serde_json::Value)> {
        v["features"]
            .as_array()
            .map_or(&[][..], Vec::as_slice)
            .iter()
            .filter(|f| huts::flagged(f))
            .filter_map(|f| Some((huts::point(f)?, f["properties"].clone())))
            .collect()
    };
    for (p, props) in flagged(&info) {
        out.push((p, name(&props["name"])));
    }
    for (p, props) in flagged(&osm) {
        out.push((
            p,
            name(&props["tags"]["name"]).or_else(|| name(&props["name"])),
        ));
    }
    Ok(out)
}

/// Two unnamed points of the same kind closer than this are one (the ways of one car park
/// mapped apart), metres.
pub const SAME_PLACE_M: f64 = 150.0;
/// An unnamed point is told by the nearest named place within this, metres.
pub const PLACE_MAX_M: f64 = 5000.0;

/// A point to go to as the pack gives it: kind, name when it has one, position.
#[derive(Debug, Clone, PartialEq)]
pub struct RawTarget {
    /// `refuge`, `parking`, `start`, `point`.
    pub kind: String,
    /// Name in the data, `None` when the data has none.
    pub name: Option<String>,
    /// Position.
    pub at: LonLat,
}

/// Named places of a pack an unnamed point is told by: the named targets `raw`, the named
/// points of interest of the pack (car parks, desks, offices) and its stations.
fn named_places(pack: &Pack, raw: &[RawTarget]) -> Result<Vec<(LonLat, String)>, PackError> {
    let mut out: Vec<(LonLat, String)> = raw
        .iter()
        .filter_map(|t| Some((t.at, t.name.clone()?)))
        .collect();
    for id in ["stations-pois", "stations"] {
        if pack.layer(id).is_err() {
            continue;
        }
        for f in pack.read_geojson(id)?.features {
            let Some(p) = point_of(&f) else { continue };
            if let Some(n) = prop(&f, "name").map(str::trim).filter(|n| !n.is_empty()) {
                out.push((p, n.to_owned()));
            }
        }
    }
    Ok(out)
}

/// The targets of `raw`, each told apart from the others (#181). A point without a name
/// is called by its kind and the nearest named place of `places` within
/// [`PLACE_MAX_M`] (« Parking à 300 mètres au sud du Plan », « Parking près du
/// foyer »), else by its kind and the centre of the site (`site`: name, centre); « près
/// de » shared by two points gives way to the distance and the direction. A point closer
/// than [`SAME_PLACE_M`] to a point of the same kind already kept, unnamed or of the same
/// name, is dropped (the same car park mapped as several ways, the same hut in two
/// sources). A label still shared gets its rank (« (2) »).
pub fn label_targets(
    raw: Vec<RawTarget>,
    places: &[(LonLat, String)],
    site: Option<(&str, LonLat)>,
    lang: Lang,
) -> Vec<FogTarget> {
    let tx = text::of(lang);
    // (target, label, precise label of an unnamed point)
    let mut kept: Vec<(RawTarget, String, Option<String>)> = Vec::new();
    // Named first, so that an unnamed way of a named car park is dropped for it.
    let (named, unnamed): (Vec<RawTarget>, Vec<RawTarget>) =
        raw.into_iter().partition(|t| t.name.is_some());
    for t in named {
        let name = t.name.clone().unwrap_or_default();
        if !kept.iter().any(|(k, n, _)| {
            k.kind == t.kind && *n == name && k.at.haversine_m(t.at) < SAME_PLACE_M
        }) {
            kept.push((t, name, None));
        }
    }
    for t in unnamed {
        if kept
            .iter()
            .any(|(k, _, _)| k.kind == t.kind && k.at.haversine_m(t.at) < SAME_PLACE_M)
        {
            continue;
        }
        let (name, precise) = unnamed_names(&t, places, site, lang);
        kept.push((t, name, Some(precise)));
    }
    let count = |kept: &[(RawTarget, String, Option<String>)], kind: &str, name: &str| {
        kept.iter()
            .filter(|(k, n, _)| k.kind == kind && n == name)
            .count()
    };
    let shared: Vec<bool> = kept
        .iter()
        .map(|(t, n, _)| count(&kept, &t.kind, n) > 1)
        .collect();
    for ((_, name, precise), shared) in kept.iter_mut().zip(shared) {
        if shared && let Some(p) = precise.take() {
            *name = p;
        }
    }
    let mut seen: std::collections::HashMap<(String, String), usize> =
        std::collections::HashMap::new();
    kept.into_iter()
        .map(|(t, name, _)| {
            let n = seen.entry((t.kind.clone(), name.clone())).or_insert(0);
            *n += 1;
            let name = if *n == 1 {
                name
            } else {
                tx.fog_rank(&name, *n)
            };
            FogTarget {
                kind: t.kind,
                name,
                lon: t.at.lon,
                lat: t.at.lat,
                distance_m: None,
            }
        })
        .collect()
}

/// Names of an unnamed point by its kind and the nearest named place: the short one
/// (« près de » within [`SAME_PLACE_M`]) and the precise one (distance and direction).
fn unnamed_names(
    t: &RawTarget,
    places: &[(LonLat, String)],
    site: Option<(&str, LonLat)>,
    lang: Lang,
) -> (String, String) {
    let tx = text::of(lang);
    let what = tx.fog_kind(&t.kind);
    let nearest = places
        .iter()
        .map(|(p, n)| (p.haversine_m(t.at), *p, n.as_str()))
        .filter(|(d, _, _)| *d <= PLACE_MAX_M)
        .min_by(|a, b| a.0.total_cmp(&b.0));
    let Some((d, p, place)) = nearest.or_else(|| site.map(|(n, c)| (c.haversine_m(t.at), c, n)))
    else {
        return (what.to_owned(), what.to_owned());
    };
    let precise = tx.fog_off(what, &distance_words(d, lang), bearing_deg(p, t.at), place);
    if d < SAME_PLACE_M {
        (tx.fog_near(what, place), precise)
    } else {
        (precise.clone(), precise)
    }
}

/// Points of the pack to go to as the data gives them: its refuges and shelters, its
/// parkings, the starts of its loops.
fn raw_targets(pack: &Pack, lang: Lang) -> Result<Vec<RawTarget>, PackError> {
    let mut out = Vec::new();
    for (p, name) in plan_refuges(pack)? {
        out.push(RawTarget {
            kind: "refuge".into(),
            name,
            at: p,
        });
    }
    if pack.layer("stations-pois").is_ok() {
        for f in pack.read_geojson("stations-pois")?.features {
            if prop(&f, "kind") != Some("parking") {
                continue;
            }
            let Some(p) = point_of(&f) else { continue };
            let name = prop(&f, "name")
                .map(str::trim)
                .filter(|n| !n.is_empty())
                .map(str::to_owned);
            out.push(RawTarget {
                kind: "parking".into(),
                name,
                at: p,
            });
        }
    }
    for s in crate::tracking_day::day_starts(pack.manifest().site.id.clone(), lang) {
        out.push(RawTarget {
            kind: "start".into(),
            name: Some(s.name),
            at: LonLat::new(s.lon, s.lat),
        });
    }
    Ok(out)
}

/// Name and centre of the site of a pack.
fn site_of(pack: &Pack) -> (String, LonLat) {
    let s = &pack.manifest().site;
    (s.name.clone(), LonLat::new(s.center[0], s.center[1]))
}

/// Points of the pack in `pack_dir` to go to: its refuges and shelters, its parkings, the
/// starts of its loops, each told apart from the others ([`label_targets`]); nearest
/// first when a position is given, at most `limit` (0: all).
#[uniffi::export]
pub fn fog_targets(
    pack_dir: String,
    lon: Option<f64>,
    lat: Option<f64>,
    lang: Lang,
    limit: u32,
) -> Result<Vec<FogTarget>, PackError> {
    let pack = Pack::open(PathBuf::from(&pack_dir))?;
    let raw = raw_targets(&pack, lang)?;
    let places = named_places(&pack, &raw)?;
    let (site, centre) = site_of(&pack);
    let mut out = label_targets(raw, &places, Some((&site, centre)), lang);
    if let (Some(lon), Some(lat)) = (lon, lat) {
        let here = LonLat::new(lon, lat);
        for t in &mut out {
            t.distance_m = Some(here.haversine_m(LonLat::new(t.lon, t.lat)));
        }
        out.sort_by(|a, b| a.distance_m.total_cmp(&b.distance_m));
    } else {
        out.sort_by(|a, b| a.kind.cmp(&b.kind).then(a.name.cmp(&b.name)));
    }
    if limit > 0 {
        out.truncate(limit as usize);
    }
    Ok(out)
}

trait TotalCmp {
    fn total_cmp(&self, o: &Self) -> std::cmp::Ordering;
}

impl TotalCmp for Option<f64> {
    fn total_cmp(&self, o: &Self) -> std::cmp::Ordering {
        self.unwrap_or(f64::INFINITY)
            .total_cmp(&o.unwrap_or(f64::INFINITY))
    }
}

/// The point touched on the map (`lon`, `lat`) as a target, named by the nearest named
/// place of the pack in `pack_dir` (« Point à 400 mètres à l'ouest du refuge… »); by its
/// kind alone without a pack.
#[uniffi::export]
pub fn fog_point(pack_dir: Option<String>, lon: f64, lat: f64, lang: Lang) -> FogTarget {
    let t = RawTarget {
        kind: "point".into(),
        name: None,
        at: LonLat::new(lon, lat),
    };
    let name = match pack_dir.and_then(|d| Pack::open(PathBuf::from(d)).ok()) {
        Some(pack) => {
            let raw = raw_targets(&pack, lang).unwrap_or_default();
            let places = named_places(&pack, &raw).unwrap_or_default();
            let (site, centre) = site_of(&pack);
            unnamed_names(&t, &places, Some((&site, centre)), lang).0
        }
        None => text::of(lang).fog_kind("point").to_owned(),
    };
    FogTarget {
        kind: t.kind,
        name,
        lon,
        lat,
        distance_m: None,
    }
}

// ---------------------------------------------------------------------------------------
// Guided outing: description written in its directory
// ---------------------------------------------------------------------------------------

/// What a guided outing follows.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GuidedKind {
    /// Bearing and distance to a point (K7).
    Bearing,
    /// A line (off-piste route, V1).
    Line,
}

/// Description of a guided outing (`guided.json`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GuidedSpec {
    /// Bearing or line.
    pub kind: GuidedKind,
    /// Label (« Cap vers Refuge de la Blanche », « Hors piste Le Laus → col d'Izoard »).
    pub label: String,
    /// The point to reach (end of the line).
    pub target: FogTarget,
    /// The line, `[lon, lat]` (line mode).
    #[serde(default)]
    pub line: Vec<[f64; 2]>,
    /// Model time (C5) of the line per terrain (up, flat, down), seconds.
    #[serde(default)]
    pub model_s: [f64; TERRAINS],
    /// The group (C11, `ProfileModel` JSON) whose pace gives the times.
    #[serde(default)]
    pub group_json: Option<String>,
    /// Period of the reminders, seconds.
    pub period_s: f64,
    /// `fr` or `en`.
    pub lang: String,
    /// Site of the pack, when known.
    #[serde(default)]
    pub site_id: Option<String>,
    /// Pack directory (relief of the recap), when known.
    #[serde(default)]
    pub pack_dir: Option<String>,
    /// Written at (UTC, RFC 3339).
    pub created_at: String,
}

impl GuidedSpec {
    fn lang(&self) -> Lang {
        if self.lang == "en" {
            Lang::En
        } else {
            Lang::Fr
        }
    }

    fn model(&self) -> Option<ProfileModel> {
        self.group_json
            .as_deref()
            .and_then(|j| serde_json::from_str(j).ok())
    }

    /// Writes it into `dir` (created).
    pub fn write(&self, dir: &Path) -> Result<(), TrackingError> {
        std::fs::create_dir_all(dir).map_err(|e| invalid(format!("{}: {e}", dir.display())))?;
        let json = serde_json::to_string_pretty(self).map_err(invalid)?;
        skimap_common::output::write_text(&dir.join(GUIDED_FILE), &json).map_err(invalid)
    }

    /// Reads the description of `dir`.
    pub fn read(dir: &Path) -> Result<Self, TrackingError> {
        let p = dir.join(GUIDED_FILE);
        let text =
            std::fs::read_to_string(&p).map_err(|e| invalid(format!("{}: {e}", p.display())))?;
        serde_json::from_str(&text).map_err(invalid)
    }
}

fn lang_code(lang: Lang) -> String {
    match lang {
        Lang::Fr => "fr".into(),
        Lang::En => "en".into(),
    }
}

/// Writes in `dir` the guided outing to `target` on a bearing (K7); `group_json`: the
/// group whose pace gives the time left (C11); `period_s`: reminders, 0 for the default.
#[uniffi::export]
pub fn guided_bearing(
    dir: String,
    target: FogTarget,
    group_json: Option<String>,
    period_s: f64,
    lang: Lang,
    site_id: Option<String>,
    pack_dir: Option<String>,
) -> Result<(), TrackingError> {
    let label = match lang {
        Lang::Fr => format!("Cap vers {}", target.name),
        Lang::En => format!("Bearing to {}", target.name),
    };
    GuidedSpec {
        kind: GuidedKind::Bearing,
        label,
        target,
        line: Vec::new(),
        model_s: [0.0; TERRAINS],
        group_json,
        period_s: if period_s > 0.0 {
            period_s
        } else {
            DEFAULT_PERIOD_S
        },
        lang: lang_code(lang),
        site_id,
        pack_dir,
        created_at: skimap_common::dates::rfc3339(Timestamp::now()),
    }
    .write(Path::new(&dir))
}

/// Whether `dir` holds a guided outing (else a day plan).
#[uniffi::export]
pub fn is_guided(dir: String) -> bool {
    Path::new(&dir).join(GUIDED_FILE).is_file()
}

/// Time to cover `model_s` (C5, per terrain) at the pace of `model` (C11), else the model
/// time: (median, 10 % and 90 % quantiles), seconds.
pub fn personal_time(model: Option<&ProfileModel>, model_s: [f64; TERRAINS]) -> (f64, f64, f64) {
    match model {
        Some(m) => {
            let p = m.params.predict(model_s);
            (p.median_s, p.lo_s, p.hi_s)
        }
        None => {
            let t: f64 = model_s.iter().sum();
            (t, t, t)
        }
    }
}

/// Model time (C5) of a straight line of `d` metres off-piste, on the flat.
pub fn bearing_model_s(d: f64) -> [f64; TERRAINS] {
    let eta = EtaModel::new(Style::Classic);
    let v = eta.speed_ms(0.0, eta.default_elev_m, true).max(0.3);
    [0.0, d / v, 0.0]
}

// ---------------------------------------------------------------------------------------
// The session
// ---------------------------------------------------------------------------------------

/// A guided outing followed by the location service, call for call like a
/// `TrackingSession`.
#[derive(uniffi::Object)]
pub struct GuidedSession {
    engine: Mutex<Engine>,
    dir: PathBuf,
    replayed: u32,
}

impl std::fmt::Debug for GuidedSession {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("GuidedSession")
            .field("dir", &self.dir)
            .finish_non_exhaustive()
    }
}

/// Opens the guided outing written in `dir`; inputs already recorded there are replayed
/// (nothing said again).
#[uniffi::export]
pub fn open_guided(dir: String) -> Result<Arc<GuidedSession>, TrackingError> {
    let d = PathBuf::from(&dir);
    let spec = GuidedSpec::read(&d)?;
    let log = d.join(INPUTS_FILE);
    let past = crate::recap::read_inputs(&log)?;
    let mut e = Engine::new(spec, log);
    for i in &past {
        e.feed(i, true);
    }
    e.out.clear();
    Ok(Arc::new(GuidedSession {
        engine: Mutex::new(e),
        dir: d,
        replayed: u32::try_from(past.len()).unwrap_or(u32::MAX),
    }))
}

impl GuidedSession {
    fn with<T>(&self, f: impl FnOnce(&mut Engine) -> T) -> T {
        let mut e = self
            .engine
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        f(&mut e)
    }
}

#[uniffi::export]
impl GuidedSession {
    /// Directory of the outing.
    pub fn dir(&self) -> String {
        self.dir.display().to_string()
    }

    /// Inputs replayed when the session opened.
    pub fn replayed(&self) -> u32 {
        self.replayed
    }

    /// A batch of fixes, packed as for `TrackingSession::push_fixes`.
    pub fn push_fixes(&self, packed: Vec<u8>) -> Result<BatchOut, TrackingError> {
        self.push_batch(packed, Vec::new())
    }

    /// A batch of fixes and the steps counted over the same time (X10), as for
    /// `TrackingSession::push_batch`: the steps are recorded with the fixes, in time order
    /// (a report at the time of a fix after it); the bearing does not use them.
    pub fn push_batch(
        &self,
        packed: Vec<u8>,
        steps: Vec<StepReport>,
    ) -> Result<BatchOut, TrackingError> {
        let fixes = unpack(&packed)?;
        self.with(|e| {
            let mut inputs: Vec<Input> = fixes
                .iter()
                .map(|(t, p)| Input::Fix {
                    t: utc(*t),
                    pos: *p,
                })
                .collect();
            if !steps.is_empty() {
                inputs.extend(steps.iter().map(|s| Input::Steps {
                    t: utc(s.t_ms),
                    steps: s.steps,
                }));
                inputs.sort_by_key(Input::t);
            }
            e.log(&inputs)?;
            for i in &inputs {
                e.feed(i, false);
            }
            Ok(e.batch())
        })
    }

    /// An input other than a fix (the screen state drives the GPS mode; the others are
    /// recorded).
    pub fn push_event(&self, event: TrackEvent) -> Result<BatchOut, TrackingError> {
        self.with(|e| {
            let i = crate::recap::event_input(&event);
            e.log(std::slice::from_ref(&i))?;
            e.feed(&i, false);
            Ok(e.batch())
        })
    }

    /// « Ramène-moi »: back along the way skied, from the last fix.
    pub fn back_start(&self, t_ms: i64) -> Result<BatchOut, TrackingError> {
        self.with(|e| {
            e.back_start(t_ms);
            Ok(e.batch())
        })
    }

    /// Ends « ramène-moi ».
    pub fn back_stop(&self) -> Result<BatchOut, TrackingError> {
        self.with(|e| {
            e.back = None;
            Ok(e.batch())
        })
    }

    /// Bearing and distance (or the next turn of the line), to say now.
    pub fn say_next(&self, t_ms: i64) -> Option<SpokenView> {
        self.with(|e| e.say_next(t_ms))
    }

    /// Distance and time left, to say now.
    pub fn say_remaining(&self, t_ms: i64) -> Option<SpokenView> {
        self.with(|e| e.say_remaining(t_ms))
    }

    /// Guidance now.
    pub fn guidance(&self) -> GuidanceView {
        self.with(|e| e.guidance())
    }

    /// Nothing for the group to learn from a guided outing (its times are off the trail
    /// graph): an empty JSON object.
    pub fn observation(&self) -> String {
        "{}".to_owned()
    }

    /// Writes the recorded trace as GPX next to the description; its path.
    pub fn write_gpx(&self) -> Result<String, TrackingError> {
        self.with(|e| crate::recap::write_gpx(&e.log_path, &e.spec.label))
    }
}

fn unpack(packed: &[u8]) -> Result<Vec<(i64, LonLat)>, TrackingError> {
    const N: usize = crate::tracking::FIX_BYTES;
    if !packed.len().is_multiple_of(N) {
        return Err(TrackingError::BadFixes {
            bytes: packed.len() as u64,
        });
    }
    Ok(packed
        .as_chunks::<N>()
        .0
        .iter()
        .map(|c| {
            let mut b8 = [0u8; 8];
            b8.copy_from_slice(&c[0..8]);
            let t = i64::from_le_bytes(b8);
            b8.copy_from_slice(&c[8..16]);
            let lon = f64::from_le_bytes(b8);
            b8.copy_from_slice(&c[16..24]);
            let lat = f64::from_le_bytes(b8);
            (t, LonLat::new(lon, lat))
        })
        .collect())
}

fn mode_view(m: GpsMode) -> GpsModeView {
    match m {
        GpsMode::Idle => GpsModeView::Idle,
        GpsMode::Pocket => GpsModeView::Pocket,
        GpsMode::Approach => GpsModeView::Approach,
        GpsMode::Deviation => GpsModeView::Deviation,
        GpsMode::Interaction => GpsModeView::Interaction,
        GpsMode::Pause => GpsModeView::Pause,
        GpsMode::Refuge => GpsModeView::Refuge,
        GpsMode::Waking => GpsModeView::Waking,
    }
}

fn gps_view(r: &GpsRequest) -> GpsRequestView {
    GpsRequestView {
        mode: mode_view(r.mode),
        gnss: r.gnss,
        interval_ms: (r.interval_s * 1000.0).round() as i64,
        max_delay_ms: (r.max_delay_s * 1000.0).round() as i64,
        significant_motion: r.significant_motion,
    }
}

fn spoken(t_ms: i64, text: String, priority: SpeechPriority, kind: &str) -> SpokenView {
    SpokenView {
        t_ms,
        text,
        segments: Vec::new(),
        all_clips: false,
        priority,
        kinds: vec![kind.to_owned()],
    }
}

/// What the session keeps between fixes.
struct Engine {
    spec: GuidedSpec,
    lang: Lang,
    model: Option<ProfileModel>,
    log_path: PathBuf,
    target: LonLat,
    /// Line followed (line mode), built at the first fix.
    line: Option<Backtrack>,
    /// « Ramène-moi ».
    back: Option<Backtrack>,
    trace: Vec<LonLat>,
    first: Option<(f64, LonLat)>,
    last: Option<(f64, LonLat)>,
    /// Distance at the first fix, metres.
    initial_m: f64,
    /// Direction of travel, degrees, and where it was measured from.
    course: Option<f64>,
    anchor: Option<(f64, LonLat)>,
    speed: f64,
    skied_m: f64,
    moving_s: f64,
    last_said: f64,
    last_period: f64,
    drift_since: Option<f64>,
    steps_said: usize,
    arrived: bool,
    screen_on: bool,
    gps: GpsState,
    last_gps: Option<GpsRequest>,
    /// Outputs since the last batch.
    out: BatchOut,
}

fn empty_batch(g: GuidanceView) -> BatchOut {
    BatchOut {
        spoken: Vec::new(),
        alerts: Vec::new(),
        switches: Vec::new(),
        proposals: Vec::new(),
        answered: Vec::new(),
        profile_proposals: Vec::new(),
        profile_chosen: None,
        session: Vec::new(),
        gps: None,
        recap: None,
        guidance: g,
    }
}

impl BatchOut {
    fn clear(&mut self) {
        self.spoken.clear();
        self.alerts.clear();
        self.session.clear();
        self.gps = None;
        self.recap = None;
    }
}

impl Engine {
    fn new(spec: GuidedSpec, log_path: PathBuf) -> Self {
        let lang = spec.lang();
        let model = spec.model();
        let target = LonLat::new(spec.target.lon, spec.target.lat);
        let mut e = Self {
            spec,
            lang,
            model,
            log_path,
            target,
            line: None,
            back: None,
            trace: Vec::new(),
            first: None,
            last: None,
            initial_m: 0.0,
            course: None,
            anchor: None,
            speed: 0.0,
            skied_m: 0.0,
            moving_s: 0.0,
            last_said: f64::NEG_INFINITY,
            last_period: f64::NEG_INFINITY,
            drift_since: None,
            steps_said: 0,
            arrived: false,
            screen_on: false,
            gps: GpsState::default(),
            last_gps: None,
            out: empty_batch(placeholder()),
        };
        e.out.guidance = e.guidance();
        e
    }

    fn log(&self, inputs: &[Input]) -> Result<(), TrackingError> {
        let mut buf = String::new();
        for i in inputs {
            buf.push_str(&serde_json::to_string(i).map_err(invalid)?);
            buf.push('\n');
        }
        skimap_common::output::append_bytes(&self.log_path, buf.as_bytes()).map_err(invalid)
    }

    fn batch(&mut self) -> BatchOut {
        let g = self.guidance();
        let mut b = std::mem::replace(&mut self.out, empty_batch(g.clone()));
        b.guidance = g;
        b
    }

    fn say(&mut self, t: f64, text: String, priority: SpeechPriority, kind: &str, quiet: bool) {
        self.last_said = t;
        if !quiet {
            self.out
                .spoken
                .push(spoken((t * 1000.0) as i64, text, priority, kind));
        }
    }

    fn distance(&self) -> f64 {
        if let Some(b) = &self.back {
            return b.remaining_m();
        }
        match (&self.line, self.last) {
            (Some(l), _) => l.remaining_m(),
            (None, Some((_, p))) => p.haversine_m(self.target),
            (None, None) => f64::INFINITY,
        }
    }

    /// Time left, seconds (median, 10 %, 90 %), at the group's pace when one is given.
    fn remaining(&self) -> (f64, f64, f64) {
        if let Some(b) = &self.back {
            let s = b.remaining_s();
            return (s, s, s);
        }
        let d = self.distance();
        if !d.is_finite() {
            return (0.0, 0.0, 0.0);
        }
        let model_s = match (&self.line, self.spec.kind) {
            (Some(l), _) if l.length_m() > 0.0 => {
                let f = (l.remaining_m() / l.length_m()).clamp(0.0, 1.0);
                self.spec.model_s.map(|m| m * f)
            }
            (None, GuidedKind::Line) => self.spec.model_s,
            _ => bearing_model_s(d),
        };
        personal_time(self.model.as_ref(), model_s)
    }

    fn feed(&mut self, i: &Input, quiet: bool) {
        match i {
            Input::Fix { t, pos } => self.fix(t.as_millisecond() as f64 / 1000.0, *pos, quiet),
            Input::Screen { on, .. } => self.screen_on = *on,
            _ => {}
        }
    }

    fn fix(&mut self, t: f64, pos: LonLat, quiet: bool) {
        if self.arrived || self.last.is_some_and(|(lt, _)| t <= lt) {
            return;
        }
        if let Some((lt, lp)) = self.last {
            let d = lp.haversine_m(pos);
            let dt = t - lt;
            if dt > 0.0 {
                let v = d / dt;
                self.speed = if self.speed > 0.0 {
                    0.7 * self.speed + 0.3 * v
                } else {
                    v
                };
                if (MOVING_MS..15.0).contains(&v) {
                    self.moving_s += dt;
                    self.skied_m += d;
                }
            }
        }
        if self
            .trace
            .last()
            .is_none_or(|p| p.haversine_m(pos) >= TRACE_STEP_M)
        {
            self.trace.push(pos);
        }
        // Direction of travel over the last COURSE_M.
        match self.anchor {
            Some((_, a)) if a.haversine_m(pos) >= COURSE_M => {
                self.course = Some(bearing_deg(a, pos));
                self.anchor = Some((t, pos));
            }
            Some((at, _)) if t - at > 60.0 => {
                self.course = None;
                self.anchor = Some((t, pos));
            }
            None => self.anchor = Some((t, pos)),
            _ => {}
        }
        let first = self.first.is_none();
        if first {
            self.first = Some((t, pos));
            if self.spec.kind == GuidedKind::Line {
                // Backtrack follows the reverse of its trace from `here`: give it the line
                // reversed to follow it forward.
                let rev: Vec<LonLat> = self
                    .spec
                    .line
                    .iter()
                    .rev()
                    .map(|c| LonLat::new(c[0], c[1]))
                    .collect();
                self.line = Backtrack::new(&rev, pos);
            }
            self.initial_m = match &self.line {
                Some(l) => l.length_m(),
                None => pos.haversine_m(self.target),
            };
            if !quiet {
                self.out.session.push(crate::tracking::SessionChange {
                    t_ms: (t * 1000.0) as i64,
                    what: "started".into(),
                    name: None,
                });
            }
        }
        self.last = Some((t, pos));
        let t_ms = (t * 1000.0) as i64;
        // « Ramène-moi » takes over the announcements.
        if let Some(b) = self.back.as_mut() {
            let st = b.fix(t, pos);
            self.line_step(t, st, quiet, true);
        } else if self.spec.kind == GuidedKind::Line {
            if let Some(l) = self.line.as_mut() {
                let st = l.fix(t, pos);
                if first {
                    let a = l.start_announcement();
                    let text = format!("{}. {}", self.spec.label, a.in_lang(self.lang).text());
                    self.say(t, text, SpeechPriority::Direction, "guided_start", quiet);
                }
                self.line_step(t, st, quiet, false);
            }
        } else {
            self.bearing_step(t, pos, first, quiet);
        }
        self.gps_step(t, quiet);
        let _ = t_ms;
    }

    fn line_step(&mut self, t: f64, st: crate::tracking_back::BackStep, quiet: bool, back: bool) {
        let t_ms = (t * 1000.0) as i64;
        if let Some(off) = st.off
            && !quiet
        {
            self.out.alerts.push(AlertView {
                t_ms,
                kind: if off {
                    AlertKind::OffRoute
                } else {
                    AlertKind::BackOnRoute
                },
            });
        }
        for (a, kind) in st.say {
            let pr = if kind == "off_route" {
                SpeechPriority::Safety
            } else {
                SpeechPriority::Direction
            };
            self.say(t, a.in_lang(self.lang).text(), pr, kind, quiet);
        }
        if st.arrived {
            if back {
                self.back = None;
            } else {
                self.arrive(t, quiet);
            }
        }
    }

    fn bearing_step(&mut self, t: f64, pos: LonLat, first: bool, quiet: bool) {
        let l = self.lang;
        let d = pos.haversine_m(self.target);
        let br = bearing_deg(pos, self.target);
        let name = self.spec.target.name.clone();
        if d <= ARRIVE_M {
            self.arrive(t, quiet);
            return;
        }
        if first {
            let text = match l {
                Lang::Fr => format!(
                    "{name} : {}, {}.",
                    distance_words(d, l),
                    bearing_spoken(br, l)
                ),
                Lang::En => format!(
                    "{name}: {}, {}.",
                    distance_words(d, l),
                    bearing_spoken(br, l)
                ),
            };
            self.say(t, text, SpeechPriority::Direction, "fog_start", quiet);
            self.last_period = t;
            self.steps_said = STEPS_M.iter().filter(|s| d <= **s).count();
            return;
        }
        let moving = self.speed >= MOVING_MS;
        let rel = self.course.map(|c| relative_deg(c, br));
        // Drift of the direction of travel, held.
        let drifting = moving && rel.is_some_and(|r| r.abs() > DRIFT_DEG);
        if drifting {
            self.drift_since.get_or_insert(t);
        } else {
            self.drift_since = None;
        }
        let gap_ok = t - self.last_said >= MIN_GAP_S;
        let crossed = STEPS_M.iter().filter(|s| d <= **s).count();
        let turn = rel.map(|r| turn_words(turn_of(r), l));
        if crossed > self.steps_said && gap_ok {
            self.steps_said = crossed;
            let text = match l {
                Lang::Fr => format!(
                    "{name} à {}, {}.",
                    distance_words(d, l),
                    bearing_spoken(br, l)
                ),
                Lang::En => format!(
                    "{name} in {}, {}.",
                    distance_words(d, l),
                    bearing_spoken(br, l)
                ),
            };
            self.say(t, text, SpeechPriority::Direction, "fog_step", quiet);
            self.last_period = t;
            return;
        }
        self.steps_said = self.steps_said.max(crossed);
        if let (Some(since), Some(tw)) = (self.drift_since, turn)
            && t - since >= DRIFT_S
            && t - self.last_said >= 3.0 * MIN_GAP_S
        {
            let text = match l {
                Lang::Fr => format!("Écart de cap : {tw}, {}.", bearing_spoken(br, l)),
                Lang::En => format!("Off bearing: {tw}, {}.", bearing_spoken(br, l)),
            };
            self.say(t, text, SpeechPriority::Direction, "fog_drift", quiet);
            self.drift_since = Some(t);
            self.last_period = t;
            return;
        }
        if t - self.last_period >= self.spec.period_s && gap_ok {
            let tail = match (turn, moving) {
                (Some(tw), true) => format!(", {tw}"),
                _ => String::new(),
            };
            let text = match l {
                Lang::Fr => format!(
                    "{name} : {}, {}{tail}.",
                    distance_words(d, l),
                    bearing_spoken(br, l)
                ),
                Lang::En => format!(
                    "{name}: {}, {}{tail}.",
                    distance_words(d, l),
                    bearing_spoken(br, l)
                ),
            };
            self.say(t, text, SpeechPriority::Schedule, "fog_period", quiet);
            self.last_period = t;
        }
    }

    fn arrive(&mut self, t: f64, quiet: bool) {
        if self.arrived {
            return;
        }
        self.arrived = true;
        let l = self.lang;
        let name = self.spec.target.name.clone();
        let elapsed = self.first.map_or(0.0, |(t0, _)| t - t0);
        let text = match l {
            Lang::Fr => format!(
                "Arrivée : {name}. {} en {}.",
                distance_words(self.skied_m, l),
                duration_fr(elapsed)
            ),
            Lang::En => format!(
                "Arrived: {name}. {} in {} min.",
                distance_words(self.skied_m, l),
                (elapsed / 60.0).round()
            ),
        };
        self.say(
            t,
            text.clone(),
            SpeechPriority::Direction,
            "finished",
            quiet,
        );
        if !quiet {
            self.out.session.push(crate::tracking::SessionChange {
                t_ms: (t * 1000.0) as i64,
                what: "arrived".into(),
                name: Some(name),
            });
            self.out.recap = Some(RecapView {
                arrived: true,
                distance_km: self.skied_m / 1000.0,
                moving_min: self.moving_s / 60.0,
                paused_min: ((elapsed - self.moving_s) / 60.0).max(0.0),
                text,
            });
        }
    }

    fn back_start(&mut self, t_ms: i64) {
        let Some((t, here)) = self.last else { return };
        self.back = Backtrack::new(&self.trace, here);
        if let Some(b) = self.back.as_mut() {
            let a = b.start_announcement();
            self.say(
                (t_ms as f64 / 1000.0).max(t),
                a.in_lang(self.lang).text(),
                SpeechPriority::Direction,
                "back",
                false,
            );
        }
    }

    fn gps_step(&mut self, t: f64, quiet: bool) {
        let (d_next, since, d_route) = if let Some(b) = self.back.as_ref().or(self.line.as_ref()) {
            (
                b.next_turn().map(|(_, d)| d),
                b.since_turn_m(),
                b.distance_m(),
            )
        } else {
            // Approaching the point is approaching a decision: the last 200 m are precise.
            (Some(self.distance()), None, 0.0)
        };
        let input = GpsInput {
            t,
            running: !self.arrived,
            d_next,
            since_point: since,
            speed_ms: self.speed,
            d_route,
            paused: false,
            refuge: false,
            waking: false,
            screen_on: self.screen_on,
        };
        let (st, req) = skimap_plan::gps::decide(&self.gps, &input, &GpsParams::default());
        self.gps = st;
        if self.last_gps.is_none_or(|r| r != req) {
            if !quiet {
                self.out.gps = Some(gps_view(&req));
            }
            self.last_gps = Some(req);
        }
    }

    fn phase(&self) -> PhaseView {
        if self.arrived {
            PhaseView::Finished
        } else if self.first.is_none() {
            PhaseView::Waiting
        } else if self.speed < MOVING_MS / 2.0 {
            PhaseView::Paused
        } else {
            PhaseView::Moving
        }
    }

    fn next(&self) -> Option<NextTurn> {
        if let Some(b) = self.back.as_ref().or(self.line.as_ref()) {
            return b.next_turn().map(|(d, m)| NextTurn {
                direction: direction_view(d),
                distance_m: m,
                name: None,
            });
        }
        let (_, pos) = self.last?;
        let br = bearing_deg(pos, self.target);
        let dir = self
            .course
            .map_or(Direction::Straight, |c| turn_of(relative_deg(c, br)));
        Some(NextTurn {
            direction: direction_view(dir),
            distance_m: pos.haversine_m(self.target),
            name: Some(bearing_words(br, self.lang)),
        })
    }

    fn guidance(&self) -> GuidanceView {
        let d = self.distance();
        let remaining_m = if d.is_finite() { d } else { 0.0 };
        let (med, lo, hi) = self.remaining();
        let now = self.last.map_or_else(
            || Timestamp::now().as_millisecond() as f64 / 1000.0,
            |(t, _)| t,
        );
        let at = |s: f64| skimap_core::ephemeris::local_time(utc(((now + s) * 1000.0) as i64));
        let label = match (self.spec.kind, self.last) {
            (GuidedKind::Bearing, Some((_, p))) => format!(
                "{} · {}",
                self.spec.target.name,
                bearing_words(bearing_deg(p, self.target), self.lang)
            ),
            _ => self.spec.label.clone(),
        };
        let length_m = if self.initial_m > 0.0 {
            self.initial_m
        } else {
            remaining_m
        };
        GuidanceView {
            phase: self.phase(),
            route: if self.back.is_some() {
                "back".into()
            } else {
                match self.spec.kind {
                    GuidedKind::Bearing => "fog".into(),
                    GuidedKind::Line => "offpiste".into(),
                }
            },
            label,
            progress_m: (length_m - remaining_m).max(0.0),
            length_m,
            remaining_m,
            next: self.next(),
            remaining_s: med,
            remaining_text: text::of(self.lang).duration(med),
            arrival: text::of(self.lang).clock5(at(med)),
            arrival_ms: ((now + med) * 1000.0) as i64,
            arrival_range: (self.model.is_some() && hi > lo + 60.0)
                .then(|| text::of(self.lang).arrival_range(at(lo), at(hi))),
            limit: None,
            off_route_m: self
                .back
                .as_ref()
                .or(self.line.as_ref())
                .map_or(0.0, Backtrack::distance_m),
            gps_mode: self
                .last_gps
                .map_or(GpsModeView::Idle, |r| mode_view(r.mode)),
            proposal: None,
            switch: None,
            profile_proposal: None,
            back: self.back.as_ref().map(|b| {
                let start = Target {
                    kind: TargetKind::Start,
                    pos: b.end(),
                    name: None,
                    recorded: None,
                };
                back_view(b, &start, WayKind::Trace, self.last.map(|(_, p)| p))
            }),
        }
    }

    fn say_next(&mut self, t_ms: i64) -> Option<SpokenView> {
        let l = self.lang;
        if let Some(b) = self.back.as_ref().or(self.line.as_ref()) {
            let text = match b.next_turn() {
                Some((d, m)) => skimap_voice::Event::Turn {
                    metres: m,
                    direction: d,
                    piste: None,
                }
                .announce()
                .in_lang(l)
                .text(),
                None => skimap_voice::Event::Arrive {
                    metres: Some(b.remaining_m()),
                }
                .announce()
                .in_lang(l)
                .text(),
            };
            return Some(spoken(t_ms, text, SpeechPriority::Direction, "next"));
        }
        let (_, pos) = self.last?;
        let br = bearing_deg(pos, self.target);
        let d = pos.haversine_m(self.target);
        let turn = self
            .course
            .map(|c| format!(", {}", turn_words(turn_of(relative_deg(c, br)), l)))
            .unwrap_or_default();
        let text = match l {
            Lang::Fr => format!(
                "{} : {}, {}{turn}.",
                self.spec.target.name,
                distance_words(d, l),
                bearing_spoken(br, l)
            ),
            Lang::En => format!(
                "{}: {}, {}{turn}.",
                self.spec.target.name,
                distance_words(d, l),
                bearing_spoken(br, l)
            ),
        };
        Some(spoken(t_ms, text, SpeechPriority::Direction, "next"))
    }

    fn say_remaining(&mut self, t_ms: i64) -> Option<SpokenView> {
        let d = self.distance();
        if !d.is_finite() {
            return None;
        }
        let (med, _, _) = self.remaining();
        let a = skimap_voice::Event::Remaining {
            metres: d,
            seconds: med,
        }
        .announce();
        Some(spoken(
            t_ms,
            a.in_lang(self.lang).text(),
            SpeechPriority::Schedule,
            "remaining",
        ))
    }
}

fn placeholder() -> GuidanceView {
    GuidanceView {
        phase: PhaseView::Waiting,
        route: String::new(),
        label: String::new(),
        progress_m: 0.0,
        length_m: 0.0,
        remaining_m: 0.0,
        next: None,
        remaining_s: 0.0,
        remaining_text: String::new(),
        arrival: String::new(),
        arrival_ms: 0,
        arrival_range: None,
        limit: None,
        off_route_m: 0.0,
        gps_mode: GpsModeView::Idle,
        proposal: None,
        switch: None,
        profile_proposal: None,
        back: None,
    }
}

/// Visibility under which a forecast hour is foggy enough to offer the bearing, metres.
pub const FOG_VISIBILITY_M: f64 = 300.0;

/// Whether the forecast of the day plan in `plan_dir` (`plan.json`, hourly weather of
/// the morning) gives fog or a visibility under [`FOG_VISIBILITY_M`] in the hour around
/// `now_ms`: the app then offers the bearing on its own (K7), without any setting.
#[uniffi::export]
pub fn fog_expected(plan_dir: String, now_ms: i64) -> bool {
    let path = Path::new(&plan_dir).join(crate::tracking_day::PLAN_FILE);
    let Ok(json) = std::fs::read_to_string(path) else {
        return false;
    };
    let Ok(p) = skimap_plan::plan::DayPlan::from_json(&json) else {
        return false;
    };
    let now = skimap_core::ephemeris::local_time(utc(now_ms));
    p.weather.as_ref().is_some_and(|w| {
        w.hours.iter().any(|h| {
            h.time.duration_since(now).as_mins().abs() <= 60
                && (h.fog == Some(true) || h.visibility_m.is_some_and(|v| v < FOG_VISIBILITY_M))
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tracking::pack_fixes;

    const T0: i64 = 1_771_146_000_000; // 2026-02-15 09:00 UTC

    /// `metres` north (`dlat`) and east (`dlon`) of (6, 45).
    fn off(north: f64, east: f64) -> LonLat {
        LonLat::new(
            6.0 + east / (111_195.0 * 45f64.to_radians().cos()),
            45.0 + north / 111_195.0,
        )
    }

    fn raw(kind: &str, name: Option<&str>, at: LonLat) -> RawTarget {
        RawTarget {
            kind: kind.into(),
            name: name.map(str::to_owned),
            at,
        }
    }

    /// #181: the car parks and huts without a name are told apart by the nearest named
    /// place (« près de » within 150 m, else distance and direction), in French and in
    /// English; the ways of one car park are one; a named car park keeps its name.
    #[test]
    fn unnamed_targets_are_told_apart() {
        let places = vec![
            (off(0.0, 0.0), "Le Laus".to_owned()),
            (off(3000.0, 0.0), "Refuge Napoléon".to_owned()),
        ];
        let targets = || {
            vec![
                raw("parking", None, off(50.0, 0.0)),
                // Another way of the same car park, 60 m away: dropped.
                raw("parking", None, off(110.0, 0.0)),
                raw("parking", None, off(-300.0, 0.0)),
                raw("parking", None, off(3000.0, 1200.0)),
                raw("parking", Some("Parking des Vignes"), off(1000.0, -1000.0)),
                // An unnamed way of the named car park: dropped for it.
                raw("parking", None, off(1000.0, -1050.0)),
                raw("refuge", None, off(0.0, -800.0)),
                raw("point", None, off(0.0, 8000.0)),
            ]
        };
        let names = |lang| -> Vec<String> {
            label_targets(
                targets(),
                &places,
                Some(("Cervières", off(-9000.0, 0.0))),
                lang,
            )
            .into_iter()
            .map(|t| t.name)
            .collect()
        };
        assert_eq!(
            names(Lang::Fr),
            [
                "Parking des Vignes",
                "Parking près du Laus",
                "Parking à 300 mètres au sud du Laus",
                "Parking à 1,2 kilomètre à l'est du refuge Napoléon",
                "Refuge à 800 mètres à l'ouest du Laus",
                "Point à 12 kilomètres au nord-est de Cervières",
            ]
        );
        assert_eq!(
            names(Lang::En),
            [
                "Parking des Vignes",
                "Car park near Le Laus",
                "Car park 300 metres south of Le Laus",
                "Car park 1.2 kilometres east of Refuge Napoléon",
                "Refuge 800 metres west of Le Laus",
                "Point 12 kilometres north-east of Cervières",
            ]
        );
    }

    /// Beyond 5 km of any named place, the centre of the site; two « près de » of one
    /// place give way to the distance and the direction; a label still shared gets its
    /// rank, a named hut in two sources is one.
    #[test]
    fn shared_labels_are_made_distinct() {
        let places = vec![(off(0.0, 0.0), "Accueil".to_owned())];
        let t = label_targets(
            vec![
                raw("parking", None, off(0.0, 100.0)),
                raw("parking", None, off(0.0, -100.0)),
                raw("parking", None, off(20_000.0, 0.0)),
                raw("refuge", Some("Cabane"), off(5000.0, 0.0)),
                raw("refuge", Some("Cabane"), off(5000.0, 40.0)),
                raw("refuge", Some("Cabane"), off(-5000.0, 0.0)),
            ],
            &places,
            Some(("Cervières", off(10_000.0, 0.0))),
            Lang::Fr,
        );
        let names: Vec<&str> = t.iter().map(|t| t.name.as_str()).collect();
        assert_eq!(
            names,
            [
                "Cabane",
                "Cabane (2)",
                "Parking à 100 mètres à l'est de l'accueil",
                "Parking à 100 mètres à l'ouest de l'accueil",
                "Parking à 10 kilomètres au nord de Cervières",
            ]
        );
        let mut labels: Vec<(&str, &str)> = t
            .iter()
            .map(|t| (t.kind.as_str(), t.name.as_str()))
            .collect();
        labels.sort_unstable();
        labels.dedup();
        assert_eq!(labels.len(), t.len(), "every label is distinct");
        // Without a pack, the point on the map is called by its kind.
        assert_eq!(fog_point(None, 6.0, 45.0, Lang::Fr).name, "Point");
        assert_eq!(fog_point(None, 6.0, 45.0, Lang::En).kind, "point");
    }

    #[test]
    fn bearings_and_words() {
        let a = LonLat::new(6.0, 45.0);
        assert!((bearing_deg(a, LonLat::new(6.0, 45.01)) - 0.0).abs() < 0.01);
        assert!((bearing_deg(a, LonLat::new(6.01, 45.0)) - 90.0).abs() < 0.1);
        assert!((bearing_deg(a, LonLat::new(6.0, 44.99)) - 180.0).abs() < 0.01);
        assert!((bearing_deg(a, LonLat::new(5.99, 45.0)) - 270.0).abs() < 0.1);
        assert_eq!(cardinal(244.0, Lang::Fr), "sud-ouest");
        assert_eq!(cardinal(350.0, Lang::En), "north");
        assert_eq!(relative_deg(350.0, 10.0), 20.0);
        assert_eq!(relative_deg(10.0, 350.0), -20.0);
        assert_eq!(distance_words(853.0, Lang::Fr), "850 mètres");
        assert_eq!(distance_words(1230.0, Lang::Fr), "1,2 kilomètre");
        assert_eq!(distance_words(3000.0, Lang::Fr), "3 kilomètres");
        assert_eq!(distance_words(1230.0, Lang::En), "1.2 kilometres");
        assert_eq!(turn_of(5.0), Direction::Straight);
        assert_eq!(turn_of(-40.0), Direction::SlightLeft);
        assert_eq!(turn_of(100.0), Direction::Right);
        assert_eq!(turn_of(170.0), Direction::UTurn);
        let v = fog_bearing(6.0, 45.0, 5.99, 44.99, Some(180.0), Lang::Fr);
        assert_eq!(v.cardinal, "sud-ouest");
        assert!(v.bearing_text.starts_with("cap 2"), "{}", v.bearing_text);
        assert_eq!(v.turn, Some(TurnDirection::SlightRight));
        assert_eq!(v.turn_text.as_deref(), Some("légèrement à droite"));
    }

    /// A straight walk towards a point 1.3 km north, at 1.2 m/s, a fix every 5 s; then a
    /// drift to the east.
    fn walk(dir: &Path, period_s: f64) -> Vec<BatchOut> {
        let target = FogTarget {
            kind: "refuge".into(),
            name: "Refuge test".into(),
            lon: 6.0,
            lat: 45.0 + 1300.0 / 111_195.0,
            distance_m: None,
        };
        guided_bearing(
            dir.display().to_string(),
            target,
            None,
            period_s,
            Lang::Fr,
            None,
            None,
        )
        .unwrap();
        let s = open_guided(dir.display().to_string()).unwrap();
        let mut outs = Vec::new();
        // 6 m per 5 s towards the point, except 15 fixes to the east (off bearing).
        let kx = 111_195.0 * 45f64.to_radians().cos();
        let mut lat = 45.0;
        let mut lon = 6.0;
        let mut t = T0;
        for i in 0..260 {
            let br = if (100..115).contains(&i) {
                90f64
            } else {
                bearing_deg(
                    LonLat::new(lon, lat),
                    LonLat::new(6.0, 45.0 + 1300.0 / 111_195.0),
                )
            };
            lat += 6.0 * br.to_radians().cos() / 111_195.0;
            lon += 6.0 * br.to_radians().sin() / kx;
            t += 5000;
            outs.push(s.push_fixes(pack_fixes(&[(t, lon, lat)])).unwrap());
        }
        outs
    }

    #[test]
    fn bearing_walk_says_little_and_arrives() {
        let dir = tempfile::tempdir().unwrap();
        let outs = walk(dir.path(), 180.0);
        let said: Vec<&SpokenView> = outs.iter().flat_map(|o| &o.spoken).collect();
        let kinds: Vec<&str> = said.iter().map(|s| s.kinds[0].as_str()).collect();
        eprintln!(
            "{:#?}",
            said.iter()
                .map(|s| (&s.kinds[0], &s.text))
                .collect::<Vec<_>>()
        );
        assert_eq!(kinds.first(), Some(&"fog_start"));
        assert!(
            said[0].text.contains("cap 0 degrés, nord"),
            "{}",
            said[0].text
        );
        assert!(kinds.contains(&"fog_step"), "{kinds:?}");
        assert!(kinds.contains(&"fog_drift"), "{kinds:?}");
        assert_eq!(kinds.last(), Some(&"finished"));
        // Sober: about 1300 m at 1.2 m/s (18 min) gives fewer than ten announcements.
        assert!(said.len() <= 10, "{}", said.len());
        // Never two within MIN_GAP_S, except the arrival.
        for w in said.windows(2) {
            if w[1].kinds[0] != "finished" {
                assert!(w[1].t_ms - w[0].t_ms >= (MIN_GAP_S * 1000.0) as i64);
            }
        }
        let recap = outs.iter().find_map(|o| o.recap.clone()).unwrap();
        assert!(recap.arrived);
        assert!(
            (recap.distance_km - 1.35).abs() < 0.2,
            "{}",
            recap.distance_km
        );
        // The drift is said as a turn to the left (the point is to the north-west).
        let drift = said.iter().find(|s| s.kinds[0] == "fog_drift").unwrap();
        assert!(drift.text.contains("gauche"), "{}", drift.text);
        // GPS: precise in the last 200 m (approach).
        assert!(
            outs.iter()
                .filter_map(|o| o.gps)
                .any(|g| g.mode == GpsModeView::Approach)
        );
        let g = &outs[50].guidance;
        assert_eq!(g.route, "fog");
        assert!(g.label.starts_with("Refuge test · cap"), "{}", g.label);
        assert!(g.remaining_s > 0.0 && g.remaining_m > 0.0);
    }

    #[test]
    fn replay_says_nothing_again_and_keeps_the_state() {
        let dir = tempfile::tempdir().unwrap();
        let _ = walk(dir.path(), 180.0);
        let s = open_guided(dir.path().display().to_string()).unwrap();
        assert!(s.replayed() > 200);
        let g = s.guidance();
        assert_eq!(g.phase, PhaseView::Finished);
        let gpx = s.write_gpx().unwrap();
        let text = std::fs::read_to_string(gpx).unwrap();
        assert_eq!(text.matches("<trkpt").count(), 260);
    }

    #[test]
    fn personal_time_uses_the_group() {
        let fam = ProfileModel::new(skimap_core::calibration::GroupProfile::app_default());
        let m = bearing_model_s(1000.0);
        let (alone, _, _) = personal_time(None, m);
        let (family, lo, hi) = personal_time(Some(&fam), m);
        assert!(family > alone, "{family} {alone}");
        assert!(lo < family && family < hi);
    }

    #[test]
    fn line_mode_follows_the_route() {
        let dir = tempfile::tempdir().unwrap();
        // An L: 600 m north then 600 m east.
        let mut line = Vec::new();
        for i in 0..=60 {
            line.push([6.0, 45.0 + f64::from(i) * 10.0 / 111_195.0]);
        }
        let top = 45.0 + 600.0 / 111_195.0;
        let kx = 111_195.0 * 45f64.to_radians().cos();
        for i in 1..=60 {
            line.push([6.0 + f64::from(i) * 10.0 / kx, top]);
        }
        let end = *line.last().unwrap();
        GuidedSpec {
            kind: GuidedKind::Line,
            label: "Hors piste test".into(),
            target: FogTarget {
                kind: "point".into(),
                name: "B".into(),
                lon: end[0],
                lat: end[1],
                distance_m: None,
            },
            line: line.clone(),
            model_s: [0.0, 1000.0, 0.0],
            group_json: None,
            period_s: DEFAULT_PERIOD_S,
            lang: "fr".into(),
            site_id: None,
            pack_dir: None,
            created_at: skimap_common::dates::rfc3339(Timestamp::now()),
        }
        .write(dir.path())
        .unwrap();
        let s = open_guided(dir.path().display().to_string()).unwrap();
        let mut said = Vec::new();
        let mut recap = None;
        let mut t = T0;
        for c in &line {
            t += 8000;
            let o = s.push_fixes(pack_fixes(&[(t, c[0], c[1])])).unwrap();
            said.extend(
                o.spoken
                    .iter()
                    .map(|x| (x.kinds[0].clone(), x.text.clone())),
            );
            recap = recap.or(o.recap);
        }
        eprintln!("{said:#?}");
        assert_eq!(said[0].0, "guided_start");
        assert!(
            said.iter()
                .any(|(k, t)| k == "turn" && t.contains("droite")),
            "{said:?}"
        );
        assert!(recap.is_some_and(|r| r.arrived));
        assert_eq!(s.guidance().route, "offpiste");
    }
}
