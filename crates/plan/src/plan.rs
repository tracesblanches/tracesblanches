//! The day plan (J1, #110): the morning choices ([`morning`]) and the validated plan
//! ([`build`]), serialised for the phone ([`DayPlan`]).
//!
//! # Morning (without gloves)
//!
//! For a request (start, day, time available, technique, return time, level) on the layers
//! of the day (`skimap_assist::inputs::SiteData`):
//!
//! 1. briefing of the day (A1, `briefing::day`);
//! 2. snow quality (A3) and wax (A2) at the forecast point nearest to the start;
//! 3. best time to leave (A9) for the time available, unless a start time is given;
//! 4. loops for the request from that start time (A5, `suggest::suggest_for`), on the time
//!    of the group of the day when one is given ([`morning_for`], C11), else on the model
//!    time;
//! 5. equipment of each loop (A6) for that time, with the grip wax reliability of A2.
//!
//! # The plan (one loop validated)
//!
//! | Part | From |
//! |---|---|
//! | route: legs on the trail graph, length, climb, model time | A5 loop, C1 graph, C5 model of the day |
//! | key points with planned passage times (model pace) | start, junctions ([`crate::guidance`]), refuges within [`REFUGE_NEAR_M`], zones without network (P12), historic avalanche paths (P20), finish |
//! | limit to be back, turnaround | C7 deadline (light limit minus 30 min), return time asked, A8 latest turnaround at the planned pace (the group's with C11, [`crate::pace::with_pace`]) |
//! | planned announcements, each with its clips and whether the pack has them | phrase bank VO2, clips VO3 |
//! | plans B | [`crate::fallback`] |
//! | equipment, wax, briefing, best time, synthesis (A7 on the route), reasons of the loop | A6, A2, A1, A9, A7, A5 |
//! | weather hours of the day at the forecast point | P19, for the condition alerts of A8 on the phone |
//!
//! Passage times are at the model pace (C5): the pace A8 compares the skier with. The
//! planned return with margin is the model time × 1.2 (C7 factor), as A5 and A6 say it.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use chrono::{Duration, NaiveDate, NaiveDateTime, NaiveTime, Timelike};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use skimap_assist::alerts;
use skimap_assist::best_time::{self, BestTime};
use skimap_assist::briefing::{self, DayBriefing};
use skimap_assist::equipment::{self, Equipment};
use skimap_assist::inputs::SiteData;
use skimap_assist::layers::{Route as AssistRoute, WeatherPoint, densify};
use skimap_assist::suggest::{self, Suggestions};
use skimap_assist::synthesis;
use skimap_assist::wax::{self, DayWax};
use skimap_common::geom::LonLat;
use skimap_common::layout::DataLayout;
use skimap_common::meta::{FileMeta, LayerMeta, now_rfc3339};
use skimap_common::site::Site;
use skimap_core::calibration::ProfileModel;
use skimap_core::eta::EtaModel;
use skimap_core::graph::{Difficulty, PisteGraph};
use skimap_core::routing::Style;
use skimap_core::turnaround::{self, TurnaroundParams};
use skimap_voice::Event;
use skimap_voice::bank::{Announcement, Change, Side, ZoneRule};
use skimap_voice::clips::{DayMonth, Fixed};
use skimap_voice::pack::VoiceManifest;
use skimap_voice::quantity::Clock;

use crate::course::{Course, PlanLeg};
use crate::error::{Error, Result};
use crate::fallback::{self, Fallback, FallbackKind, MIN_EXIT_SAVING_S};
use crate::guidance::{self, Junction};
use crate::phrases::{self, ClipCheck, PLAN_PHRASES, Voice, never_safe};
use crate::speech::Priority;

/// Format of the plan file.
pub const FORMAT: &str = "skimap-plan";
/// Version of the format.
pub const VERSION: u32 = 1;
/// Output layer: `data/out/<site>/plan/`.
pub const LAYER: &str = "plan";
/// A refuge closer than this to the route is a key point (and recognised at a pause).
pub const REFUGE_NEAR_M: f64 = 150.0;
/// A turn is announced this far before its junction, metres.
pub const TURN_LEAD_M: f64 = 150.0;
/// Announced junctions closer than this to the first of a group are said with it, metres.
pub const TURN_MERGE_M: f64 = 120.0;
/// A zone without network is announced this far before it, metres.
pub const NO_NETWORK_LEAD_M: f64 = 200.0;
/// Historic avalanche paths less than this apart along the route are one stretch (one
/// announcement), metres.
pub const AVALANCHE_MERGE_M: f64 = 500.0;
/// Zones without network less than this apart along the route are one stretch, metres.
pub const NO_NETWORK_MERGE_M: f64 = 200.0;
/// A historic avalanche path is announced this far before it, metres.
pub const AVALANCHE_LEAD_M: f64 = 100.0;
/// Distance at which a protected zone is announced ahead, metres.
pub const ZONE_LEAD_M: f64 = 100.0;
/// Protected zones are tested every this many samples of 10 m along a route, and at every
/// sample where two tests disagree.
const ZONE_STEP: usize = 5;
/// Text of every plan: never « sans danger ».
pub const NEVER_SAFE: &str = "Ce plan n'est jamais une garantie : informations indicatives, lisez le BERA et les sources officielles, et décidez sur place.";

/// What the skier asks in the morning.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PlanRequest {
    /// Label (« 2 h skating, retour avant 16 h »).
    pub label: String,
    /// Start (and finish) position.
    pub start: LonLat,
    /// Name of the start.
    pub start_name: String,
    /// Day.
    pub date: NaiveDate,
    /// Start time; `None`: the best time of A9.
    pub start_time: Option<NaiveTime>,
    /// Earliest start tried by A9.
    pub earliest: NaiveTime,
    /// Time available, minutes (model time).
    pub budget_min: f64,
    /// Technique.
    pub style: Style,
    /// Back before (local), if any.
    pub return_before: Option<NaiveTime>,
    /// Hardest difficulty wanted.
    pub max_difficulty: Option<Difficulty>,
    /// Loops proposed.
    pub max_results: usize,
}

/// The morning screen: everything to choose a loop.
#[derive(Debug, Clone, Serialize)]
pub struct Morning {
    /// Request.
    pub request: PlanRequest,
    /// Start time used for the loops.
    pub start_time: NaiveDateTime,
    /// Briefing of the day (A1).
    pub briefing: DayBriefing,
    /// Forecast point used (nearest to the start).
    pub weather_point: Option<String>,
    /// Best time to leave (A9).
    pub best_time: Option<BestTime>,
    /// Wax (A2).
    pub wax: Option<DayWax>,
    /// Loops (A5).
    pub suggestions: Suggestions,
    /// Equipment of each loop (A6), in the order of the loops.
    pub equipment: Vec<Equipment>,
    /// Missing layers and limits.
    pub notes: Vec<String>,
    /// The group of the day (C11) the loops are chosen for, if any: [`build`] gives its
    /// time to the plan ([`crate::pace::with_pace`]).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub group: Option<ProfileModel>,
}

/// The morning choices of a request, on the model time (no group): [`morning_for`] with
/// `None`.
pub fn morning(data: &SiteData, req: &PlanRequest) -> Morning {
    morning_for(data, req, None)
}

/// The morning choices of a request for the group of the day (C11, prior or learned):
/// loops chosen on its time (`suggest::suggest_for`), equipment for that time; `None`:
/// on the model time, as [`morning`].
pub fn morning_for(data: &SiteData, req: &PlanRequest, group: Option<&ProfileModel>) -> Morning {
    let date = req.date;
    let point = data
        .weather
        .as_ref()
        .and_then(|w| w.nearest(req.start))
        .map(|(p, _)| p);
    let day = briefing::day(data, date, &briefing::Options { home: None });
    let quality = point.and_then(|p| day.snow_quality.iter().find(|q| q.point == p.id));
    let wax = match (point, quality) {
        (Some(p), Some(q)) => wax::day_wax(p, q),
        _ => None,
    };
    let best = point.map(|p| {
        best_time::best_time(
            p,
            quality,
            day.crowd.as_ref().map(|c| c.level),
            &best_time::Request {
                date,
                duration_min: req.budget_min,
                earliest: req.earliest,
                classes: Vec::new(),
            },
        )
    });
    let start_time = req
        .start_time
        .map(|t| date.and_time(t))
        .or_else(|| best.as_ref().and_then(|b| b.best.as_ref()).map(|s| s.start))
        .unwrap_or_else(|| date.and_time(req.earliest));
    let suggestions = suggest::suggest_for(
        data,
        &suggest::Request {
            label: req.label.clone(),
            start: req.start,
            start_name: req.start_name.clone(),
            start_time,
            budget_min: req.budget_min,
            style: req.style,
            return_before: req.return_before,
            max_difficulty: req.max_difficulty,
            max_results: req.max_results,
        },
        group,
    );
    let kick_unreliable = wax
        .as_ref()
        .is_some_and(|w| !w.morning.kick_reliable || !w.afternoon.kick_reliable);
    let equipment = suggestions
        .suggestions
        .iter()
        .map(|s| {
            equipment::equipment(
                data,
                &equipment::Outing {
                    label: s.label.clone(),
                    start: start_time,
                    // The group's median time when the loops are chosen for one.
                    duration_min: s.group.as_ref().map_or(s.eta_min, |g| g.median_min),
                    route: s.line.clone(),
                    offpiste: false,
                    kick_unreliable,
                },
            )
        })
        .collect();
    let mut notes: Vec<String> = data.missing.clone();
    if point.is_none() {
        notes.push(
            "pas de météo horaire : ni meilleur moment, ni fartage, ni alertes de conditions"
                .into(),
        );
    }
    if req.start_time.is_none() && best.as_ref().is_none_or(|b| b.best.is_none()) {
        notes.push(format!(
            "pas de meilleur moment calculé : départ à {}",
            hhmm(start_time)
        ));
    }
    notes.extend(suggestions.notes.iter().cloned());
    Morning {
        request: req.clone(),
        start_time,
        briefing: day,
        weather_point: point.map(|p| p.id.clone()),
        best_time: best,
        wax,
        suggestions,
        equipment,
        notes,
        group: group.cloned(),
    }
}

/// A named place.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Waypoint {
    /// Name (`None`: an unnamed shelter).
    pub name: Option<String>,
    /// Kind (`refuge gardé`, `cabane non gardée`, `shelter`...).
    pub kind: String,
    /// Position.
    pub pos: LonLat,
}

/// Inputs of the plan besides the assistant's layers: the refuges and the clips of the
/// pack.
#[derive(Debug, Clone, Default)]
pub struct Context {
    /// Refuges, huts and shelters of the site (`refuges` layer, P11).
    pub refuges: Vec<Waypoint>,
    /// Clips of the pack (`voice` layer, VO3).
    pub voice: Option<Voice>,
    /// What could not be read.
    pub missing: Vec<String>,
}

fn read_value(path: &Path) -> Result<Value> {
    let b = std::fs::read(path).map_err(|source| Error::Io {
        path: path.to_owned(),
        source,
    })?;
    serde_json::from_slice(&b).map_err(|source| Error::Json {
        context: path.display().to_string(),
        source,
    })
}

impl Context {
    /// Reads `refuges/refuges_info.geojson` (refuges gardés, cabanes non gardées, gîtes),
    /// `refuges/osm.geojson` (alpine huts, wilderness huts, shelters) and
    /// `voice/manifest.json` of a site; what is missing is noted, not fatal.
    pub fn load(layout: &DataLayout, site: &Site) -> Result<Self> {
        let mut ctx = Context::default();
        let rdir = layout.layer_dir(site, "refuges")?;
        let feats = |v: &Value| v["features"].as_array().cloned().unwrap_or_default();
        let pos = |f: &Value| {
            let c = f["geometry"]["coordinates"].as_array()?;
            Some(LonLat::new(c.first()?.as_f64()?, c.get(1)?.as_f64()?))
        };
        match read_value(&rdir.join("refuges_info.geojson")) {
            Ok(v) => {
                for f in feats(&v) {
                    let p = &f["properties"];
                    let kind = p["type_label"].as_str().unwrap_or_default();
                    if matches!(kind, "cabane non gardée" | "refuge gardé" | "gîte d'étape")
                        && let Some(pos) = pos(&f)
                    {
                        ctx.refuges.push(Waypoint {
                            name: p["name"].as_str().map(|s| s.trim().to_owned()),
                            kind: kind.to_owned(),
                            pos,
                        });
                    }
                }
            }
            Err(e) => ctx.missing.push(format!("refuges.info : {e}")),
        }
        match read_value(&rdir.join("osm.geojson")) {
            Ok(v) => {
                for f in feats(&v) {
                    let p = &f["properties"];
                    let kind = p["kind"].as_str().unwrap_or_default();
                    if matches!(kind, "alpine_hut" | "wilderness_hut" | "shelter")
                        && let Some(pos) = pos(&f)
                    {
                        // The same hut in both sources: keep the refuges.info one.
                        if ctx.refuges.iter().any(|r| r.pos.haversine_m(pos) < 50.0) {
                            continue;
                        }
                        ctx.refuges.push(Waypoint {
                            name: p["tags"]["name"].as_str().map(|s| s.trim().to_owned()),
                            kind: kind.to_owned(),
                            pos,
                        });
                    }
                }
            }
            Err(e) => ctx.missing.push(format!("refuges OSM : {e}")),
        }
        let vdir = layout.layer_dir(site, "voice")?;
        match VoiceManifest::read(&vdir) {
            Ok(m) => {
                ctx.voice = Some(Voice::new(
                    &m,
                    vdir.join("manifest.json").display().to_string(),
                ))
            }
            Err(e) => ctx.missing.push(format!("clips de la voix : {e}")),
        }
        Ok(ctx)
    }
}

/// The graph a plan was built on.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GraphRef {
    /// Site of the graph.
    pub site: String,
    /// Nodes.
    pub nodes: usize,
    /// Edges.
    pub edges: usize,
    /// FNV-1a 64 of the edge endpoints and lengths, hex.
    pub fingerprint: String,
}

impl GraphRef {
    /// Reference of a graph.
    pub fn of(g: &PisteGraph) -> Self {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        let mut eat = |x: u64| {
            for b in x.to_le_bytes() {
                h ^= u64::from(b);
                h = h.wrapping_mul(0x0100_0000_01b3);
            }
        };
        for (i, e) in g.edges().iter().enumerate() {
            eat(e.from as u64);
            eat(e.to as u64);
            eat(u64::from(e.oneway));
            eat((g.length_m(i) * 100.0).round() as u64);
        }
        Self {
            site: g.data().site.clone(),
            nodes: g.nodes().len(),
            edges: g.edges().len(),
            fingerprint: format!("{h:016x}"),
        }
    }
}

/// Kind of key point.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum KeyKind {
    /// Start.
    Start,
    /// Junction where something is said.
    Turn,
    /// Junction where the way is obvious (nothing said).
    Junction,
    /// Refuge, hut or shelter near the route.
    Refuge,
    /// Start of a zone without any network.
    NoNetworkStart,
    /// End of a zone without any network.
    NoNetworkEnd,
    /// Start of a historic avalanche path.
    AvalancheStart,
    /// End of a historic avalanche path.
    AvalancheEnd,
    /// Finish.
    Finish,
}

/// A key point of the route with its planned passage time.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct KeyPoint {
    /// Kind.
    pub kind: KeyKind,
    /// Distance along the route, metres.
    pub s: f64,
    /// Position.
    pub pos: LonLat,
    /// Name (trail taken, refuge, zone).
    pub name: Option<String>,
    /// Model time from the start, seconds.
    pub model_s: f64,
    /// Planned passage, local (start + model time).
    pub at: NaiveDateTime,
}

/// When a planned announcement is said.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Trigger {
    /// When the outing starts.
    Departure,
    /// When the skier reaches this distance along the route, metres.
    At {
        /// Distance.
        s: f64,
        /// Not said after this distance.
        until: f64,
    },
    /// When the skier comes near the finish.
    Arrival,
}

/// An announcement planned in the morning.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PlannedAnnouncement {
    /// Key (unique in a route).
    pub key: String,
    /// What it is (`turn`, `no_network`...).
    pub kind: String,
    /// When.
    pub trigger: Trigger,
    /// Priority.
    pub priority: Priority,
    /// Event of the phrase bank, when it is one.
    pub event: Option<Event>,
    /// Clips to play.
    pub announcement: Announcement,
    /// Text.
    pub text: String,
}

/// A reason of the loop's score (A5).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PlanReason {
    /// Factor (`status`, `snow`, `wind`...).
    pub factor: String,
    /// Points.
    pub points: f64,
    /// Text.
    pub text: String,
}

/// The validated loop.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PlanRoute {
    /// Label (three main trails).
    pub label: String,
    /// Trails in order.
    pub names: Vec<String>,
    /// Legs on the trail graph.
    pub legs: Vec<PlanLeg>,
    /// Length, km.
    pub length_km: f64,
    /// Climb, m.
    pub gain_m: f64,
    /// Descent, m.
    pub loss_m: f64,
    /// Model time, minutes.
    pub model_min: f64,
    /// Rank among the loops proposed (1 is the best).
    pub rank: usize,
    /// Score of A5.
    pub score: f64,
    /// Reasons of the score.
    pub reasons: Vec<PlanReason>,
    /// Spoken summary (A5).
    pub voice: Event,
}

/// Limit to be back and turnaround.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Turnaround {
    /// Sunset, local.
    pub sunset: Option<NaiveDateTime>,
    /// C7 deadline (light limit minus 30 min), local.
    pub deadline: Option<NaiveDateTime>,
    /// Return time asked, local.
    pub return_before: Option<NaiveDateTime>,
    /// The earlier of the two.
    pub limit: Option<NaiveDateTime>,
    /// Planned arrival at the model pace.
    pub arrival: NaiveDateTime,
    /// Planned arrival with the C7 margin (model time × 1.2).
    pub arrival_with_margin: NaiveDateTime,
    /// Limit minus the arrival with margin, minutes.
    pub margin_min: Option<f64>,
    /// Latest turnaround at the planned pace (A8), the group's when the plan has one (C11,
    /// [`crate::pace::with_pace`]): `None` when the loop ends before the deadline.
    pub latest_turnaround: Option<NaiveDateTime>,
    /// Latest passage at each junction with an exit so that the exit still ends before
    /// the limit (model time × 1.2 from there): (junction index, time).
    pub latest_exit: Vec<(usize, NaiveDateTime)>,
}

/// A plan B with its announcements.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PlanB {
    /// The plan B.
    pub fallback: Fallback,
    /// What is said when the tracker switches to it.
    pub switch: Announcement,
    /// Planned announcements along it (the shared part included).
    pub announcements: Vec<PlannedAnnouncement>,
}

/// Text and voice of one part of the morning.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Said {
    /// Text, French.
    pub text: Vec<String>,
    /// Voice events.
    pub voice: Vec<Event>,
}

/// An equipment item.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Item {
    /// Item.
    pub label: String,
    /// Why.
    pub reason: String,
}

/// The day plan.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DayPlan {
    /// [`FORMAT`].
    pub format: String,
    /// [`VERSION`].
    pub version: u32,
    /// Site identifier.
    pub site: String,
    /// Day.
    pub date: NaiveDate,
    /// Generation time (UTC, RFC 3339).
    pub generated_at: String,
    /// Graph of the legs.
    pub graph: GraphRef,
    /// Request.
    pub request: PlanRequest,
    /// Planned start, local.
    pub departure: NaiveDateTime,
    /// The loop.
    pub route: PlanRoute,
    /// The other loops proposed: label, km, model minutes, score.
    pub alternatives: Vec<(String, f64, f64, f64)>,
    /// Limit and turnaround.
    pub turnaround: Turnaround,
    /// Key points in order.
    pub key_points: Vec<KeyPoint>,
    /// Junctions of the loop.
    pub junctions: Vec<Junction>,
    /// Planned announcements of the loop.
    pub announcements: Vec<PlannedAnnouncement>,
    /// Plans B.
    pub plans_b: Vec<PlanB>,
    /// Refuges near the loop.
    pub refuges: Vec<Waypoint>,
    /// Equipment (A6).
    pub equipment: Vec<Item>,
    /// Briefing (A1), best time (A9), wax (A2), synthesis on the route (A7), equipment.
    pub morning: Vec<(String, Said)>,
    /// Time model of the day (C5).
    pub eta: EtaModel,
    /// Weather hours of the day at the forecast point (A8 on the phone).
    pub weather: Option<WeatherPoint>,
    /// Clips needed by the announcements and whether the pack has them.
    pub clips: ClipCheck,
    /// Missing inputs and limits.
    pub notes: Vec<String>,
    /// [`NEVER_SAFE`].
    pub never_safe: String,
    /// Estimated time for the group of the day (C11), when one was chosen
    /// ([`crate::pace::with_pace`]).
    #[serde(default)]
    pub pace: Option<crate::pace::PlanPace>,
}

fn hhmm(t: NaiveDateTime) -> String {
    format!("{} h {:02}", t.hour(), t.minute())
}

fn secs(s: f64) -> Duration {
    Duration::milliseconds((s * 1000.0).round() as i64)
}

fn km(m: f64) -> String {
    format!("{:.1}", m / 1000.0).replace('.', ",")
}

/// Runs of a predicate along a line, sampled every 10 m: (start, end) in metres; runs
/// separated by less than `gap_m` are merged, runs shorter than `min_m` dropped.
fn runs(
    line: &[LonLat],
    gap_m: f64,
    min_m: f64,
    inside: impl Fn(LonLat) -> bool,
) -> Vec<(f64, f64)> {
    let mut out: Vec<(f64, f64)> = Vec::new();
    let mut s = 0.0;
    let mut cur: Option<f64> = None;
    for (p, len) in densify(line, 10.0) {
        if inside(p) {
            if cur.is_none() {
                cur = Some(s);
            }
        } else if let Some(a) = cur.take() {
            out.push((a, s));
        }
        s += len;
    }
    if let Some(a) = cur {
        out.push((a, s));
    }
    let mut merged: Vec<(f64, f64)> = Vec::new();
    for r in out {
        match merged.last_mut() {
            Some(m) if r.0 - m.1 < gap_m => m.1 = r.1,
            _ => merged.push(r),
        }
    }
    merged.retain(|(a, b)| b - a >= min_m);
    merged
}

/// What [`announcements`] reads besides the route.
pub struct Hazards<'a> {
    /// Layers of the day (zones without network, historic avalanche paths, BERA, zones and
    /// rules).
    pub data: &'a SiteData,
    /// Day.
    pub date: NaiveDate,
    /// Protected zones already told for the plan: the morning says them for the loop (A7,
    /// « Votre itinéraire traverse une zone protégée. »). `None`: no protected zone said
    /// along the route (the loop); `Some(zones)`: the route's protected zones that are not
    /// among them, at their entry (plans B, routes computed during the outing, X11).
    pub zones_told: Option<&'a BTreeSet<String>>,
    /// Stretch of the route searched for those protected zones, metres (the rest is the
    /// loop's: before the divergence of a plan B, after its return).
    pub zones_range: (f64, f64),
}

/// A protected zone crossed by a route, with its rules for a day outing (C8 at noon of the
/// day, as A7 says them: zones that are not a country, department, canton or commune;
/// rules of access, leaving the routes and fire), and the stretches of the route inside it.
#[derive(Debug, Clone, PartialEq)]
pub struct ZoneCrossing {
    /// Zone identifier.
    pub id: String,
    /// Zone name.
    pub name: String,
    /// Rules said.
    pub rules: Vec<ZoneRule>,
    /// End of the period of the first rule, if seasonal.
    pub until: Option<DayMonth>,
    /// Stretches along the route, metres (closer than [`NO_NETWORK_MERGE_M`] merged).
    pub runs: Vec<(f64, f64)>,
}

/// Protected zones crossed by a line (sampled every 10 m) between the distances `range`
/// on a day; nothing without the zones layer. The zones of the layer that are not a country, department, canton or
/// commune are tested along the line; the rules of each zone crossed are those of C8 at
/// its first point on the line (one evaluation per zone: the big zones are not tested at
/// every point).
pub fn protected_zones(
    line: &[LonLat],
    range: (f64, f64),
    data: &SiteData,
    date: NaiveDate,
) -> Vec<ZoneCrossing> {
    use skimap_core::zone_eval::Verdict;
    use skimap_zones::layer::ZoneKind;
    use skimap_zones::rules::{Period, Topic};
    let Some((layer, rules)) = &data.zones else {
        return Vec::new();
    };
    let protected: Vec<&skimap_zones::layer::Zone> = layer
        .zones
        .iter()
        .filter(|z| {
            !matches!(
                z.kind(),
                ZoneKind::Country | ZoneKind::Department | ZoneKind::Canton | ZoneKind::Commune
            )
        })
        .collect();
    // Stretches of the line inside each zone (closer than NO_NETWORK_MERGE_M merged),
    // and the first point inside: tested every ZONE_STEP samples of 10 m, and at every
    // sample between two tests that disagree (the boundaries to 10 m; a zone crossed on
    // less than the step between two tests outside is missed).
    let samples: Vec<(LonLat, f64, f64)> = {
        let mut s = 0.0;
        densify(line, 10.0)
            .into_iter()
            .map(|(p, len)| {
                let x = (p, s, len);
                s += len;
                x
            })
            .filter(|(_, s, len)| s + len >= range.0 && *s <= range.1)
            .collect()
    };
    // Per zone: its stretches along the line and its first point on it.
    type Inside = (Vec<(f64, f64)>, Option<LonLat>);
    let mut inside: Vec<Inside> = vec![(Vec::new(), None); protected.len()];
    for (k, z) in protected.iter().enumerate() {
        let at = |i: usize| z.contains(samples[i].0);
        let mut hits: Vec<usize> = Vec::new();
        let mut prev: Option<(usize, bool)> = None;
        let mut i = 0;
        while i < samples.len() {
            let here = at(i);
            if let Some((j, was)) = prev
                && was != here
            {
                hits.extend((j + 1..i).filter(|&m| at(m)));
            }
            if here {
                hits.push(i);
            }
            prev = Some((i, here));
            i += if i + 1 == samples.len() {
                1
            } else {
                ZONE_STEP.min(samples.len() - 1 - i)
            };
        }
        // Between two tests inside, the samples are inside.
        hits.sort_unstable();
        hits.dedup();
        let (runs, first) = &mut inside[k];
        for w in 0..hits.len() {
            let m = hits[w];
            let (p, s, len) = samples[m];
            first.get_or_insert(p);
            let filled = w > 0 && m - hits[w - 1] <= ZONE_STEP;
            match runs.last_mut() {
                Some(run) if filled || s - run.1 < NO_NETWORK_MERGE_M => run.1 = s + len,
                _ => runs.push((s, s + len)),
            }
        }
    }
    let noon = skimap_assist::sun::local_to_utc(date.and_hms_opt(12, 0, 0).expect("noon"));
    let mut out = Vec::new();
    for (z, (mut runs, first)) in protected.into_iter().zip(inside) {
        runs.retain(|(a, b)| b - a >= 30.0);
        let Some(p) = first.filter(|_| !runs.is_empty()) else {
            continue;
        };
        // Its rules of a day outing, as A7 says them: access, leaving the routes, fire.
        let ev = skimap_core::zone_eval::evaluate(layer, rules, p, noon);
        let mut said = Vec::new();
        let mut until = None;
        for r in ev
            .active_rules()
            .filter(|r| r.zones.iter().any(|id| id == z.id()))
        {
            let zr = match (r.topic, r.verdict) {
                (Topic::Access, Verdict::Forbidden) => ZoneRule::NoEntry,
                (Topic::Access, Verdict::AvoidRecommended) => ZoneRule::Quiet,
                (Topic::LeaveRoutes, Verdict::Forbidden | Verdict::ForbiddenInPlaces) => {
                    ZoneRule::StayOnRoutes
                }
                (Topic::Fire, Verdict::Forbidden) => ZoneRule::NoFire,
                _ => continue,
            };
            if !said.contains(&zr) {
                said.push(zr);
            }
            if until.is_none()
                && let Some(Period::Yearly { to, .. }) = rules.get(&r.rule).map(|x| &x.period)
            {
                until = Some(DayMonth {
                    day: to.day as u8,
                    month: to.month as u8,
                });
            }
        }
        if said.is_empty() {
            continue;
        }
        out.push(ZoneCrossing {
            id: z.id().to_owned(),
            name: z.props.name.clone(),
            rules: said,
            until,
            runs,
        });
    }
    out.sort_by(|a, b| a.runs[0].0.total_cmp(&b.runs[0].0));
    out
}

fn planned(
    key: String,
    kind: &str,
    trigger: Trigger,
    priority: Priority,
    event: Event,
) -> PlannedAnnouncement {
    let announcement = event.announce();
    PlannedAnnouncement {
        key,
        kind: kind.into(),
        trigger,
        priority,
        text: announcement.text(),
        event: Some(event),
        announcement,
    }
}

/// Planned announcements of a route: the loop summary at the departure, a turn before
/// every junction that is not obvious, zones without network and historic avalanche paths
/// ahead (with `hazards`), the protected zones not told for the plan
/// ([`Hazards::zones_told`]), the arrival. The one function of the loop, its plans B and
/// the routes computed during the outing (X11).
pub fn announcements(
    g: &PisteGraph,
    c: &Course,
    junctions: &[Junction],
    voice: Option<&Voice>,
    hazards: Option<&Hazards<'_>>,
    departure: NaiveDateTime,
) -> Vec<PlannedAnnouncement> {
    let mut out = Vec::new();
    let model = c.model_s();
    let back = departure + secs(model * TurnaroundParams::default().eta_factor);
    out.push(planned(
        "departure".into(),
        "loop_summary",
        Trigger::Departure,
        Priority::Comfort,
        suggest::loop_voice(c.length_m() / 1000.0, model, back),
    ));
    // Turns; announced junctions less than TURN_MERGE_M after the first of a group are
    // said with it, each with its own distance (« Dans cent cinquante mètres, restez à
    // droite. Dans deux cents mètres, tournez à gauche. »).
    let turn_of = |j: &Junction, at: f64| {
        let attrs = &g.edge(j.edge_out).attrs;
        let piste = match voice {
            Some(v) => v.piste(attrs),
            None => attrs.name.as_ref().map(|n| {
                skimap_voice::names::PisteName::new(
                    n.split(" / ").next().unwrap_or(n).trim(),
                    crate::phrases::colour(attrs.difficulty),
                )
            }),
        };
        Event::Turn {
            metres: j.s - at,
            direction: j
                .direction
                .unwrap_or(skimap_voice::clips::Direction::Straight),
            piste,
        }
    };
    let mut prev_end = 0.0;
    let mut i = 0;
    while i < junctions.len() {
        let j = &junctions[i];
        if !j.announced() {
            prev_end = j.s_end;
            i += 1;
            continue;
        }
        let at = (j.s - TURN_LEAD_M).max(prev_end + 10.0).max(0.0).min(j.s);
        let mut group = vec![i];
        let mut k = i + 1;
        while k < junctions.len() && junctions[k].s - j.s <= TURN_MERGE_M {
            if junctions[k].announced() {
                group.push(k);
            }
            k += 1;
        }
        let last = &junctions[*group.last().expect("one")];
        let mut p = planned(
            format!("turn.{i}"),
            "turn",
            Trigger::At {
                s: at,
                until: last.s_end + 30.0,
            },
            Priority::Direction,
            turn_of(j, at),
        );
        for &g2 in &group[1..] {
            p.announcement = p.announcement.then(turn_of(&junctions[g2], at).announce());
        }
        p.text = p.announcement.text();
        out.push(p);
        prev_end = junctions[k - 1].s_end;
        i = k;
    }
    if let Some(h) = hazards {
        let pts = c.line().points();
        if let Some(n) = &h.data.no_network {
            for (k, (a, b)) in runs(pts, NO_NETWORK_MERGE_M, 100.0, |p| n.find(p).is_some())
                .into_iter()
                .enumerate()
            {
                let at = (a - NO_NETWORK_LEAD_M).max(0.0);
                out.push(planned(
                    format!("no_network.{k}"),
                    "no_network",
                    Trigger::At { s: at, until: b },
                    Priority::Safety,
                    Event::NoNetworkAhead {
                        metres: a - at,
                        length_m: Some(b - a),
                    },
                ));
                out.push(planned(
                    format!("no_network_end.{k}"),
                    "no_network_end",
                    Trigger::At {
                        s: b,
                        until: b + 300.0,
                    },
                    Priority::Comfort,
                    Event::NoNetworkExit,
                ));
            }
        }
        if let Some(cl) = &h.data.clpa {
            let level = h.data.bera_for(h.date).and_then(|b| b.level());
            // A stretch whose avalanche paths were all crossed earlier on the route (the
            // way back of an out-and-back) is not said again.
            let mut said: BTreeSet<String> = BTreeSet::new();
            for (k, (a, b)) in runs(pts, AVALANCHE_MERGE_M, 30.0, |p| cl.areas.find(p).is_some())
                .into_iter()
                .enumerate()
            {
                let line = c.line();
                let labels: BTreeSet<String> = (0..=((b - a) / 10.0) as usize)
                    .filter_map(|i| cl.areas.find(line.point_at(a + 10.0 * i as f64)))
                    .map(|x| x.label.clone())
                    .collect();
                if !labels.is_empty() && labels.is_subset(&said) {
                    continue;
                }
                said.extend(labels);
                let at = (a - AVALANCHE_LEAD_M).max(0.0);
                let attrs = &g.edge(c.legs()[c.leg_at(a)].edge).attrs;
                out.push(planned(
                    format!("avalanche.{k}"),
                    "avalanche_path",
                    Trigger::At { s: at, until: b },
                    Priority::Safety,
                    Event::AvalancheSegment {
                        bera_level: level,
                        piste: voice.and_then(|v| v.piste(attrs)),
                    },
                ));
            }
        }
        if let Some(told) = h.zones_told {
            // At the first entry of each protected zone not told for the plan: its name,
            // its rules (no entry: safety, else as a direction).
            let mut entries: Vec<(f64, f64, ZoneCrossing)> =
                protected_zones(c.line().points(), h.zones_range, h.data, h.date)
                    .into_iter()
                    .filter(|z| !told.contains(&z.id))
                    .filter_map(|z| z.runs.first().copied().map(|(a, b)| (a, b, z)))
                    .collect();
            entries.sort_by(|x, y| x.0.total_cmp(&y.0));
            for (k, (a, b, z)) in entries.into_iter().enumerate() {
                let priority = if z.rules.contains(&ZoneRule::NoEntry) {
                    Priority::Safety
                } else {
                    Priority::Direction
                };
                out.push(planned(
                    format!("zone.{k}"),
                    "protected_zone",
                    Trigger::At {
                        s: (a - ZONE_LEAD_M).max(0.0),
                        until: b,
                    },
                    priority,
                    Event::ProtectedZone {
                        entered: false,
                        zone: Some(z.name),
                        rules: z.rules,
                        until: z.until,
                    },
                ));
            }
        }
    }
    out.push(planned(
        "arrival".into(),
        "arrive",
        Trigger::Arrival,
        Priority::Direction,
        Event::Arrive { metres: None },
    ));
    out.sort_by(|a, b| {
        let key = |t: &Trigger| match t {
            Trigger::Departure => -1.0,
            Trigger::At { s, .. } => *s,
            Trigger::Arrival => f64::INFINITY,
        };
        key(&a.trigger).total_cmp(&key(&b.trigger))
    });
    out
}

/// The announcement of a switch to a plan B: its phrase, then the turn at the divergence
/// (or « turn back » when the plan B starts behind).
pub fn switch_announcement(phrase: Fixed, first_turn: Option<Event>) -> Announcement {
    let mut a = phrases::announce(phrase);
    if let Some(e) = first_turn {
        a = a.then(e.announce());
    }
    a
}

/// The turn at the divergence of a plan B: the junction of the new route at `s_div`.
pub fn turn_at(
    g: &PisteGraph,
    junctions: &[Junction],
    s_div: f64,
    metres: Option<f64>,
    voice: Option<&Voice>,
) -> Option<Event> {
    let j = junctions.iter().find(|j| {
        (j.s - s_div).abs() < guidance::CLUSTER_M || (j.s <= s_div && s_div <= j.s_end)
    })?;
    let attrs = &g.edge(j.edge_out).attrs;
    Some(Event::Turn {
        metres: metres.unwrap_or(0.0),
        direction: j
            .direction
            .unwrap_or(skimap_voice::clips::Direction::Straight),
        piste: voice.and_then(|v| v.piste(attrs)),
    })
}

/// Builds the plan of the loop `choice` of a morning (`data` keeps its planned route: the
/// synthesis of A7 is made on the loop, then the previous route is put back).
pub fn build(data: &mut SiteData, m: &Morning, choice: usize, ctx: &Context) -> Result<DayPlan> {
    let s = m
        .suggestions
        .suggestions
        .get(choice)
        .ok_or_else(|| {
            Error::Missing(format!(
                "boucle {} : {} proposées",
                choice + 1,
                m.suggestions.suggestions.len()
            ))
        })?
        .clone();
    let date = m.request.date;
    let departure = m.start_time;
    let eta = s.eta;
    let voice = ctx.voice.as_ref();
    // Synthesis (A7) on the loop.
    let previous = data.route.replace(AssistRoute {
        name: s.label.clone(),
        line: s.line.clone(),
    });
    let synth = synthesis::synthesise(data, date);
    data.route = previous;
    let data: &SiteData = data;
    let g = data
        .graph
        .as_ref()
        .ok_or_else(|| Error::Missing("graphe des pistes (couche routes)".into()))?;
    let legs: Vec<PlanLeg> = s.legs.iter().map(|l| PlanLeg::from(*l)).collect();
    let course = Course::new(g, &legs, &eta)?;
    let junctions = guidance::junctions(g, &course);
    let hazards = Hazards {
        data,
        date,
        zones_told: None,
        zones_range: (0.0, f64::INFINITY),
    };
    let anns = announcements(g, &course, &junctions, voice, Some(&hazards), departure);
    // The plans B say the protected zones the loop does not cross (the morning says the
    // loop's, A7).
    let loop_zones: BTreeSet<String> =
        protected_zones(course.line().points(), (0.0, f64::INFINITY), data, date)
            .into_iter()
            .map(|z| z.id)
            .collect();
    // Limit and turnaround (with the light margin of the morning's group: an hour with
    // children, C11).
    let tp = m
        .group
        .as_ref()
        .map_or_else(TurnaroundParams::default, |g| g.profile.turnaround_params());
    let dl = turnaround::deadline(date, m.request.start, &tp);
    let local = skimap_core::ephemeris::local_time;
    let deadline = dl.map(|d| local(d.deadline));
    let sunset = dl.map(|d| local(d.sunset));
    let return_before = m.request.return_before.map(|t| date.and_time(t));
    let limit = match (deadline, return_before) {
        (Some(a), Some(b)) => Some(a.min(b)),
        (a, b) => a.or(b),
    };
    let model = course.model_s();
    let arrival = departure + secs(model);
    let arrival_with_margin = departure + secs(model * tp.eta_factor);
    let point = data
        .weather
        .as_ref()
        .and_then(|w| w.nearest(m.request.start))
        .map(|(p, _)| p);
    let latest_turnaround = point.and_then(|p| {
        alerts::Monitor::new(departure, course.profile(), p, alerts::Params::default())
            .planned_turnaround()
    });
    // Plans B.
    let lp = fallback::Loop {
        graph: g,
        course: &course,
        junctions: &junctions,
        eta,
        max_difficulty: m.request.max_difficulty,
        min_exit_saving_s: MIN_EXIT_SAVING_S,
        budget_s: limit.map(|l| (l - departure).num_seconds() as f64 / tp.eta_factor),
    };
    let mut plans_b = Vec::new();
    let mut latest_exit = Vec::new();
    for f in lp.all() {
        let (switch, anns_b) = if f.exists() {
            let cb = Course::new(g, &f.full_legs(&legs), &eta)?;
            let jb = guidance::junctions(g, &cb);
            let phrase = match f.kind {
                FallbackKind::Exit => Fixed::PlanShortcut,
                FallbackKind::Detour { .. } => Fixed::PlanClosedAhead,
            };
            let first = turn_at(g, &jb, f.s_div, None, voice);
            if f.kind == FallbackKind::Exit
                && let (Some(l), Some(j)) = (limit, f.junction)
            {
                // Latest passage at the junction for the exit to end before the limit.
                let rest = cb.model_s() - cb.model_s_at(f.s_div);
                latest_exit.push((j, l - secs(rest * tp.eta_factor)));
            }
            // Only what lies after the divergence: the part shared with the loop is the
            // loop's.
            // Its own part (before, the loop; after a return, the loop again).
            let back = f
                .s_rejoin
                .map_or(f64::INFINITY, |r| cb.length_m() - (course.length_m() - r));
            let hazards_b = Hazards {
                data,
                date,
                zones_told: Some(&loop_zones),
                zones_range: (f.s_div, back),
            };
            let mut after = announcements(g, &cb, &jb, voice, Some(&hazards_b), departure);
            after.retain(|a| match a.trigger {
                Trigger::Departure => false,
                Trigger::At { s, .. } => s >= f.s_div - TURN_LEAD_M,
                Trigger::Arrival => true,
            });
            (switch_announcement(phrase, first), after)
        } else {
            (phrases::announce(Fixed::PlanClosedNoDetour), Vec::new())
        };
        plans_b.push(PlanB {
            fallback: f,
            switch,
            announcements: anns_b,
        });
    }
    // Key points.
    let at = |s: f64| departure + secs(course.model_s_at(s));
    let mut keys = vec![KeyPoint {
        kind: KeyKind::Start,
        s: 0.0,
        pos: course.line().point_at(0.0),
        name: Some(m.request.start_name.clone()),
        model_s: 0.0,
        at: departure,
    }];
    for j in &junctions {
        keys.push(KeyPoint {
            kind: if j.announced() {
                KeyKind::Turn
            } else {
                KeyKind::Junction
            },
            s: j.s,
            pos: j.pos,
            name: j.name_out.clone(),
            model_s: course.model_s_at(j.s),
            at: at(j.s),
        });
    }
    let mut refuges = Vec::new();
    for r in &ctx.refuges {
        if let Some((sr, d)) = course.line().project(r.pos, 0.0, course.length_m())
            && d <= REFUGE_NEAR_M
        {
            refuges.push(r.clone());
            keys.push(KeyPoint {
                kind: KeyKind::Refuge,
                s: sr,
                pos: r.pos,
                name: r.name.clone(),
                model_s: course.model_s_at(sr),
                at: at(sr),
            });
        }
    }
    let pts = course.line().points();
    if let Some(n) = &data.no_network {
        for (a, b) in runs(pts, NO_NETWORK_MERGE_M, 100.0, |p| n.find(p).is_some()) {
            for (kind, s) in [(KeyKind::NoNetworkStart, a), (KeyKind::NoNetworkEnd, b)] {
                keys.push(KeyPoint {
                    kind,
                    s,
                    pos: course.line().point_at(s),
                    name: None,
                    model_s: course.model_s_at(s),
                    at: at(s),
                });
            }
        }
    }
    if let Some(cl) = &data.clpa {
        for (a, b) in runs(pts, AVALANCHE_MERGE_M, 30.0, |p| cl.areas.find(p).is_some()) {
            let name = cl
                .areas
                .find(course.line().point_at(a + 5.0))
                .map(|x| x.label.clone());
            for (kind, s) in [(KeyKind::AvalancheStart, a), (KeyKind::AvalancheEnd, b)] {
                keys.push(KeyPoint {
                    kind,
                    s,
                    pos: course.line().point_at(s),
                    name: name.clone(),
                    model_s: course.model_s_at(s),
                    at: at(s),
                });
            }
        }
    }
    keys.push(KeyPoint {
        kind: KeyKind::Finish,
        s: course.length_m(),
        pos: course.line().point_at(course.length_m()),
        name: Some(m.request.start_name.clone()),
        model_s: model,
        at: arrival,
    });
    keys.sort_by(|a, b| a.s.total_cmp(&b.s));
    // Clips: every planned announcement, every switch, and the fixed phrases the tracker
    // may say during the outing (alerts, deviation, end).
    let mut clips = ClipCheck::default();
    let mut check = |a: &Announcement| {
        if let Some(v) = voice {
            v.check(a, &mut clips);
        } else {
            for s in &a.segments {
                clips.required.insert(s.clip.id.clone());
                clips.missing.insert(s.clip.id.clone());
            }
        }
    };
    for a in &anns {
        check(&a.announcement);
    }
    for b in &plans_b {
        check(&b.switch);
        for a in &b.announcements {
            check(&a.announcement);
        }
    }
    let clock = |t: NaiveDateTime| Clock::floor_quarter(t.hour() as u8, t.minute() as u8);
    let mut runtime: Vec<Announcement> = [
        Change::WindRising,
        Change::Fog,
        Change::Snow,
        Change::TemperatureDrop,
    ]
    .into_iter()
    .map(|c| Event::ConditionChange(c).announce())
    .collect();
    for side in [Side::Left, Side::Right, Side::Behind, Side::Ahead] {
        runtime.push(
            Event::OffRoute {
                side,
                metres: 100.0,
            }
            .announce(),
        );
    }
    runtime.push(Event::BackOnRoute { piste: None }.announce());
    runtime.push(Fixed::TurnaroundNow.clip_announcement());
    if let Some(d) = deadline {
        runtime.push(
            Event::TurnaroundAdvanced {
                deadline: clock(d - Duration::hours(1)),
            }
            .announce(),
        );
    }
    for p in PLAN_PHRASES {
        runtime.push(phrases::announce(p));
    }
    // Proposals (C11).
    for p in crate::pace::PROPOSAL_PHRASES {
        runtime.push(phrases::announce(p));
    }
    for a in &runtime {
        check(a);
    }
    // Morning parts.
    let mut morning_parts = vec![(
        "briefing".to_owned(),
        Said {
            text: m.briefing.text.clone(),
            voice: m.briefing.voice.clone(),
        },
    )];
    if let Some(b) = &m.best_time {
        morning_parts.push((
            "best_time".into(),
            Said {
                text: vec![b.text.clone()],
                voice: b.voice().into_iter().collect(),
            },
        ));
    }
    if let Some(w) = &m.wax {
        morning_parts.push((
            "wax".into(),
            Said {
                text: vec![w.text.clone()],
                voice: vec![w.voice()],
            },
        ));
    }
    morning_parts.push((
        "synthesis".into(),
        Said {
            text: synth.sentences.clone(),
            voice: synth.voice.clone(),
        },
    ));
    let eq = m.equipment.get(choice);
    if let Some(e) = eq {
        morning_parts.push((
            "equipment".into(),
            Said {
                text: vec![e.text.clone()],
                voice: vec![e.voice()],
            },
        ));
    }
    let equipment_items = eq
        .map(|e| {
            e.items
                .iter()
                .map(|i| Item {
                    label: i.label.to_owned(),
                    reason: i.reason.clone(),
                })
                .collect()
        })
        .unwrap_or_default();
    let (gain, loss) = course.gain_loss(g);
    let reasons = s
        .reasons
        .iter()
        .map(|r| PlanReason {
            factor: serde_json::to_value(r.factor)
                .ok()
                .and_then(|v| v.as_str().map(str::to_owned))
                .unwrap_or_default(),
            points: r.points,
            text: r.text.clone(),
        })
        .collect();
    let alternatives = m
        .suggestions
        .suggestions
        .iter()
        .enumerate()
        .filter(|(i, _)| *i != choice)
        .map(|(_, x)| {
            (
                x.label.clone(),
                x.length_km,
                x.eta_min,
                (x.score * 100.0).round() / 100.0,
            )
        })
        .collect();
    // Weather of the day (and the evening before, for the hour lookups of A8).
    let weather = point.map(|p| {
        let from = (date - Duration::days(1))
            .and_hms_opt(12, 0, 0)
            .expect("noon");
        let to = (date + Duration::days(1))
            .and_hms_opt(1, 0, 0)
            .expect("1 h");
        WeatherPoint {
            hours: p.between(from, to).to_vec(),
            ..p.clone()
        }
    });
    let mut notes: Vec<String> = m.notes.clone();
    notes.extend(ctx.missing.iter().cloned());
    if !clips.missing.is_empty() {
        notes.push(format!(
            "{} clips sur {} absents du pack : ces annonces passent par la voix de secours du téléphone",
            clips.missing.len(),
            clips.required.len()
        ));
    }
    let mut plan = DayPlan {
        format: FORMAT.into(),
        version: VERSION,
        site: data.site.id.into(),
        date,
        generated_at: now_rfc3339(),
        graph: GraphRef::of(g),
        request: m.request.clone(),
        departure,
        route: PlanRoute {
            label: s.label.clone(),
            names: s.names.clone(),
            legs,
            length_km: (course.length_m() / 10.0).round() / 100.0,
            gain_m: gain.round(),
            loss_m: loss.round(),
            model_min: (model / 60.0 * 10.0).round() / 10.0,
            rank: choice + 1,
            score: (s.score * 100.0).round() / 100.0,
            reasons,
            voice: s.voice.clone(),
        },
        alternatives,
        turnaround: Turnaround {
            sunset,
            deadline,
            return_before,
            limit,
            arrival,
            arrival_with_margin,
            margin_min: limit
                .map(|l| ((l - arrival_with_margin).num_seconds() as f64 / 60.0).round()),
            latest_turnaround,
            latest_exit,
        },
        key_points: keys,
        junctions,
        announcements: anns,
        plans_b,
        refuges,
        equipment: equipment_items,
        morning: morning_parts,
        eta,
        weather,
        clips,
        notes,
        never_safe: NEVER_SAFE.into(),
        pace: None,
    };
    // The group of the morning: its time and the turnaround at its pace (C11).
    if let Some(group) = &m.group {
        crate::pace::with_pace(&mut plan, g, group)?;
    }
    debug_assert!(never_safe(&plan.text_fr()));
    Ok(plan)
}

trait ClipAnnouncement {
    fn clip_announcement(self) -> Announcement;
}

impl ClipAnnouncement for Fixed {
    fn clip_announcement(self) -> Announcement {
        Announcement {
            segments: vec![skimap_voice::bank::Segment {
                clip: self.clip(),
                pause: skimap_voice::bank::Pause::Short,
            }],
        }
    }
}

impl DayPlan {
    /// Reads a plan written by [`DayPlan::to_json`].
    pub fn from_json(s: &str) -> Result<Self> {
        let p: DayPlan = serde_json::from_str(s).map_err(|source| Error::Json {
            context: "plan".into(),
            source,
        })?;
        if p.format != FORMAT || p.version > VERSION {
            return Err(Error::Invalid(format!(
                "format {} version {}, attendu {FORMAT} ≤ {VERSION}",
                p.format, p.version
            )));
        }
        Ok(p)
    }

    /// JSON of the plan (compact: the `.txt` is the readable form).
    pub fn to_json(&self) -> Result<String> {
        serde_json::to_string(self).map_err(|source| Error::Json {
            context: "plan".into(),
            source,
        })
    }

    /// Checks that the plan follows a graph (same fingerprint, every edge known).
    pub fn check_graph(&self, g: &PisteGraph) -> Result<()> {
        let r = GraphRef::of(g);
        if r != self.graph {
            return Err(Error::GraphMismatch {
                plan: format!("{} {}", self.graph.site, self.graph.fingerprint),
                given: format!("{} {}", r.site, r.fingerprint),
            });
        }
        Ok(())
    }

    /// The plan in French, for reading it in the morning (and by hand).
    pub fn text_fr(&self) -> String {
        let mut t = Vec::new();
        let r = &self.route;
        t.push(format!(
            "Plan du {} — {} — départ {} à {}",
            briefing::date_fr(self.date),
            self.request.label,
            self.request.start_name,
            hhmm(self.departure)
        ));
        t.push(format!(
            "Boucle n° {} : {} — {} km, D+ {} m, environ {} min (modèle C5, {}), score {}",
            r.rank,
            r.label,
            format!("{:.1}", r.length_km).replace('.', ","),
            r.gain_m,
            r.model_min.round(),
            match self.request.style {
                Style::Classic => "classique",
                Style::Skating => "skating",
            },
            format!("{:.2}", r.score).replace('.', ",")
        ));
        t.push(format!("Pistes : {}", r.names.join(", ")));
        let ta = &self.turnaround;
        t.push(format!(
            "Arrivée prévue {} au rythme du modèle, {} avec la marge de 20 %{}{}",
            hhmm(ta.arrival),
            hhmm(ta.arrival_with_margin),
            ta.deadline
                .map(|d| format!(" ; limite de lumière moins 30 min (C7) : {}", hhmm(d)))
                .unwrap_or_default(),
            ta.return_before
                .map(|d| format!(" ; retour demandé avant {}", hhmm(d)))
                .unwrap_or_default()
        ));
        if let Some(p) = &self.pace {
            t.push(p.text.clone());
        }
        t.push(match ta.latest_turnaround {
            Some(x) if self.pace.is_some() => format!(
                "Demi-tour au plus tard à {} au rythme prévu du groupe (A8, C11)",
                hhmm(x)
            ),
            Some(x) => format!("Demi-tour au plus tard à {} au rythme prévu (A8)", hhmm(x)),
            None => "La boucle finit avant la limite de lumière au rythme prévu : pas d'heure de demi-tour imposée".into(),
        });
        t.push("Raisons :".into());
        for x in &r.reasons {
            t.push(format!("  - {} [{:+.2}]", x.text, x.points).replace('.', ","));
        }
        t.push("Points clés (heure de passage au rythme du modèle) :".into());
        for k in &self.key_points {
            let what = match k.kind {
                KeyKind::Start => "départ",
                KeyKind::Turn => "carrefour annoncé",
                KeyKind::Junction => "carrefour évident (rien n'est dit)",
                KeyKind::Refuge => "refuge",
                KeyKind::NoNetworkStart => "début de zone sans réseau",
                KeyKind::NoNetworkEnd => "fin de zone sans réseau",
                KeyKind::AvalancheStart => "début de zone d'avalanche connue (CLPA)",
                KeyKind::AvalancheEnd => "fin de zone d'avalanche connue",
                KeyKind::Finish => "arrivée",
            };
            t.push(format!(
                "  {} km  {}  {}{}",
                km(k.s),
                hhmm(k.at),
                what,
                k.name
                    .as_ref()
                    .map(|n| format!(" : {n}"))
                    .unwrap_or_default()
            ));
        }
        t.push("Annonces prévues :".into());
        for a in &self.announcements {
            let when = match a.trigger {
                Trigger::Departure => "au départ".to_owned(),
                Trigger::At { s, .. } => format!("à {} km", km(s)),
                Trigger::Arrival => "à l'arrivée".to_owned(),
            };
            t.push(format!("  {when} [{:?}] « {} »", a.priority, a.text));
        }
        t.push(format!("Plans B ({}) :", self.plans_b.len()));
        for b in &self.plans_b {
            t.push(format!(
                "  - {} — annonce « {} »",
                b.fallback.text,
                b.switch.text()
            ));
        }
        for (j, at) in &ta.latest_exit {
            t.push(format!(
                "  sortie du carrefour à {} km : y passer avant {} pour finir avant la limite",
                km(self.junctions[*j].s),
                hhmm(*at)
            ));
        }
        t.push("Matin :".into());
        for (k, s) in &self.morning {
            for l in &s.text {
                t.push(format!("  [{k}] {l}"));
            }
            let spoken: Vec<String> = s.voice.iter().map(|e| e.announce().text()).collect();
            if !spoken.is_empty() {
                t.push(format!("  [{k}, voix] {}", spoken.join(" ")));
            }
        }
        t.push("Équipement :".into());
        for i in &self.equipment {
            t.push(format!("  - {} : {}", i.label, i.reason));
        }
        if !self.refuges.is_empty() {
            t.push(format!(
                "Refuges à moins de {} m de la boucle : {}",
                REFUGE_NEAR_M,
                self.refuges
                    .iter()
                    .map(|r| r
                        .name
                        .clone()
                        .unwrap_or_else(|| format!("abri sans nom ({})", r.kind)))
                    .collect::<Vec<_>>()
                    .join(", ")
            ));
        }
        t.push(format!(
            "Clips : {} nécessaires, {} absents du pack{}",
            self.clips.required.len(),
            self.clips.missing.len(),
            if self.clips.missing.is_empty() {
                String::new()
            } else {
                format!(
                    " ({})",
                    self.clips
                        .missing
                        .iter()
                        .cloned()
                        .collect::<Vec<_>>()
                        .join(", ")
                )
            }
        ));
        for n in &self.notes {
            t.push(format!("Limite : {n}"));
        }
        t.push(self.never_safe.clone());
        t.join("\n") + "\n"
    }
}

/// Writes `plan_<date>_<n>.json` and `.txt` (n = rank of the loop) and `plan.meta.json`
/// in `data/out/<site>/plan/`.
pub fn write(
    layout: &DataLayout,
    site: &Site,
    plan: &DayPlan,
    data: &SiteData,
) -> Result<Vec<PathBuf>> {
    let dir = layout.layer_dir(site, LAYER)?;
    DataLayout::ensure(&dir)?;
    let stem = format!(
        "plan_{}_{}_{}",
        plan.date,
        skimap_voice::names::slug(&plan.request.label),
        plan.route.rank
    );
    let j = dir.join(format!("{stem}.json"));
    let t = dir.join(format!("{stem}.txt"));
    skimap_common::output::write_text(&j, &(plan.to_json()? + "\n"))?;
    skimap_common::output::write_text(&t, &plan.text_fr())?;
    let g = data
        .graph
        .as_ref()
        .ok_or_else(|| Error::Missing("graphe des pistes (couche routes)".into()))?;
    let k = dir.join(format!("{stem}.trk"));
    skimap_common::output::write_bytes(&k, &track(plan, g)?.to_bytes())?;
    let mut meta = LayerMeta::new(LAYER, site.id);
    meta.tickets = vec!["J1 (#110)".into(), "J2 (#111)".into()];
    meta.valid_from = Some(plan.date.to_string());
    meta.valid_until = Some(plan.date.to_string());
    meta.validity = Some("le jour du plan".into());
    meta.crs = Some("EPSG:4326".into());
    meta.sources = data.sources.clone();
    meta.files = vec![
        FileMeta::new(
            "plan_<date>_<demande>_<rang>.json",
            "plan de journée (DayPlan, format skimap-plan) : itinéraire sur le graphe des pistes, points clés et heures de passage, demi-tour, annonces prévues et clips, plans B, équipement, météo du jour",
        ),
        FileMeta::new(
            "plan_<date>_<demande>_<rang>.txt",
            "le même plan en français, à relire",
        ),
        FileMeta::new(
            "plan_<date>_<demande>_<rang>.trk",
            "table à plat lue sans copie par le téléphone à chaque position (format SKTRK002, skimap_plan::track) : géométrie de la boucle et de chaque plan B dans le plan local du départ (un plan B ne garde que sa partie propre et lit dans la boucle le début et la fin qu'il partage avec elle), index en grille de 200 m, points de décision triés, profil de temps",
        ),
    ];
    meta.notes = vec![NEVER_SAFE.into()];
    meta.write(&dir)?;
    Ok(vec![j, t, k, dir.join(LayerMeta::file_name(LAYER))])
}

/// Whether two JSON values are equal, numbers within a relative `tol` (serde_json parses
/// floats within one unit in the last place without its `float_roundtrip` feature, so a
/// plan read back may differ from the one written by that much).
pub fn json_close(a: &Value, b: &Value, tol: f64) -> bool {
    match (a, b) {
        (Value::Number(x), Value::Number(y)) => match (x.as_f64(), y.as_f64()) {
            (Some(x), Some(y)) => (x - y).abs() <= tol * x.abs().max(y.abs()).max(1.0),
            _ => x == y,
        },
        (Value::Array(x), Value::Array(y)) => {
            x.len() == y.len() && x.iter().zip(y).all(|(a, b)| json_close(a, b, tol))
        }
        (Value::Object(x), Value::Object(y)) => {
            x.len() == y.len()
                && x.iter()
                    .all(|(k, v)| y.get(k).is_some_and(|w| json_close(v, w, tol)))
        }
        _ => a == b,
    }
}

/// The flat table the phone follows (E1): the loop and each plan B, computed here once
/// ([`crate::track`]). Announcements of a plan B before its divergence are the loop's. A
/// plan B keeps only its own part and refers to the loop for the legs it shares with it
/// (X13; the loop's own block is unchanged).
pub fn track(plan: &DayPlan, g: &PisteGraph) -> Result<crate::track::Track> {
    use crate::track::{RouteTable, Track};
    plan.check_graph(g)?;
    let course = Course::new(g, &plan.route.legs, &plan.eta)?;
    let origin = course.line().point_at(0.0);
    let plane = skimap_core::graph::LocalPlane::new(origin);
    let loop_anns: Vec<(u32, u32, &PlannedAnnouncement)> = plan
        .announcements
        .iter()
        .enumerate()
        .map(|(i, a)| (0, i as u32, a))
        .collect();
    // Legs of the loop each plan B shares at its head and at its tail (the course drops
    // legs of no length).
    let real = |legs: &[PlanLeg]| legs.iter().filter(|l| l.length_m() > 1e-6).count();
    let n = course.legs().len();
    let shares: Vec<(usize, usize)> = plan
        .plans_b
        .iter()
        .map(|b| {
            let f = &b.fallback;
            let head = real(&plan.route.legs[..f.prefix.min(plan.route.legs.len())]);
            let tail = f.rejoin.map_or(0, |r| {
                real(&plan.route.legs[r.min(plan.route.legs.len())..])
            });
            (head.min(n), tail.min(n))
        })
        .collect();
    let base = RouteTable::new(0, 0.0, &course, &plane, &loop_anns);
    let mut routes = vec![base];
    for (bi, b) in plan.plans_b.iter().enumerate() {
        let f = &b.fallback;
        if !f.exists() {
            continue;
        }
        let cb = Course::new(g, &f.full_legs(&plan.route.legs), &plan.eta)?;
        let mut anns: Vec<(u32, u32, &PlannedAnnouncement)> = loop_anns
            .iter()
            .copied()
            .filter(
                |(_, _, a)| matches!(a.trigger, Trigger::At { s, .. } if s < f.s_div - TURN_LEAD_M),
            )
            .collect();
        anns.extend(
            b.announcements
                .iter()
                .enumerate()
                .map(|(i, a)| ((bi + 1) as u32, i as u32, a)),
        );
        let (head, tail) = shares[bi];
        let id = (bi + 1) as i32;
        let table = RouteTable::on_loop(
            &routes[0], &course, id, f.s_div, &cb, &plane, &anns, head, tail,
        )
        .unwrap_or_else(|| RouteTable::new(id, f.s_div, &cb, &plane, &anns));
        routes.push(table);
    }
    Ok(Track { origin, routes })
}
