//! Following the day plan on the phone (K4 #54, K5 #55, K6 #56, K8 #58, K13 #116, E1
//! #125, C11 #130): the app's single location service feeds the core's tracker
//! (`skimap_plan::tracker`) with its batches of fixes and gets back what to say (clips of
//! the pack, assembled), the alerts, the switches and proposals to show, the GPS request
//! to apply, and the guidance of the thin banner and of the lock-screen notification
//! (next direction and distance, time left, arrival).
//!
//! The tracker borrows the plan, its flat table, the trail graph and the pack's clips. A
//! [`TrackingSession`] owns them on a thread of its own, where the tracker lives for the
//! whole outing; every call is a message to that thread and its answer (no `unsafe`, no
//! copy of the plan per call). The thread sleeps on its channel between batches: nothing
//! runs between two GPS batches.
//!
//! Fixes cross the FFI packed (`push_fixes`: 24 bytes per fix, one call per batch, no
//! object per fix on the Kotlin side). Every input is appended to `inputs.jsonl` next to
//! the plan, one write per batch: the recorded trace (GPX at the end), the outing the
//! group learns from (C11, `pace::digest`), and the state of the outing when the process
//! is restarted (the inputs are replayed through a new tracker, a few milliseconds).
//!
//! « Ramène-moi » ([`TrackingSession::back_start`]) guides back along the way skied
//! ([`crate::tracking_back`]); the tracker keeps running for the safety alerts.

use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, Mutex};

use chrono::{DateTime, NaiveDateTime, Timelike, Utc};
use skimap_assist::inputs::{Paths, SiteData};
use skimap_common::geom::LonLat;
use skimap_core::graph::PisteGraph;
use skimap_pack::Pack;
use skimap_plan::course::Course;
use skimap_plan::gps::{GpsInput, GpsMode, GpsParams, GpsRequest, GpsState};
use skimap_plan::guidance::{self, Junction};
use skimap_plan::pace::{ProposalKind, clock_fr, duration_fr};
use skimap_plan::phrases::Voice;
use skimap_plan::plan::DayPlan;
use skimap_plan::session::{Activity, End, Phase, SessionEvent};
use skimap_plan::speech::{Priority, Spoken};
use skimap_plan::track::TrackView;
use skimap_plan::tracker::{
    Alert, Cause, Condition, Input, Output, RouteRef, Tracker, TrackerParams,
};
use skimap_voice::Event;
use skimap_voice::bank::Announcement;
use skimap_voice::clips::Direction;
use skimap_voice::pack::VoiceManifest;

use crate::tracking_back::Backtrack;
use crate::tracking_day::{PACK_FILE, PLAN_FILE, PackView, TRACK_FILE};

/// Inputs recorded next to the plan (JSON lines of `skimap_plan::tracker::Input`).
pub const INPUTS_FILE: &str = "inputs.jsonl";
/// Directory under the plan's where the pack's layers are linked in the pipeline layout
/// ([`PackView`]) for the tracker ([`Tracker::with_layers`]).
pub const LAYERS_DIR: &str = "layers";
/// Bytes of one packed fix: epoch milliseconds (i64), longitude, latitude (f64), little
/// endian.
pub const FIX_BYTES: usize = 24;

/// Errors returned to Kotlin (`TrackingException`).
#[derive(Debug, thiserror::Error, uniffi::Error)]
pub enum TrackingError {
    /// Unreadable plan, pack or input.
    #[error("{detail}")]
    Invalid {
        /// What is wrong.
        detail: String,
    },
    /// No assistant layers for this site (the morning needs the two evaluation sites'
    /// layers).
    #[error("pas de plan de journée pour le site {site}")]
    Unsupported {
        /// Site of the pack.
        site: String,
    },
    /// `validate` before `morning`.
    #[error("aucune boucle calculée")]
    NoMorning,
    /// The session's thread is gone.
    #[error("suivi arrêté")]
    Closed,
}

fn invalid(detail: impl std::fmt::Display) -> TrackingError {
    TrackingError::Invalid {
        detail: detail.to_string(),
    }
}

/// Epoch milliseconds of a local (French legal) time.
pub fn epoch_ms(local: NaiveDateTime) -> i64 {
    skimap_assist::sun::local_to_utc(local).timestamp_millis()
}

fn utc(ms: i64) -> DateTime<Utc> {
    DateTime::<Utc>::from_timestamp_millis(ms).unwrap_or_default()
}

fn ms(t: DateTime<Utc>) -> i64 {
    t.timestamp_millis()
}

fn hhmm(t: NaiveDateTime) -> String {
    format!("{} h {:02}", t.hour(), t.minute())
}

/// The trail graph of a pack (`piste-graph` layer).
pub fn load_graph(pack_dir: &Path) -> Result<PisteGraph, TrackingError> {
    let pack = Pack::open(pack_dir.to_path_buf()).map_err(invalid)?;
    let l = pack.layer("piste-graph").map_err(invalid)?;
    let text = std::fs::read_to_string(pack.path(&l.file).map_err(invalid)?).map_err(invalid)?;
    PisteGraph::from_json(&text).map_err(invalid)
}

/// Activity recognised by the phone, or the significant motion sensor.
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum TrackActivity {
    /// Still.
    Still,
    /// On foot.
    OnFoot,
    /// In a vehicle.
    InVehicle,
    /// Unknown.
    Unknown,
    /// Significant motion sensor fired.
    SignificantMotion,
}

/// An input other than a fix.
#[derive(Debug, Clone, PartialEq, uniffi::Enum)]
pub enum TrackEvent {
    /// Activity.
    Activity {
        /// Epoch milliseconds.
        t_ms: i64,
        /// Activity.
        activity: TrackActivity,
    },
    /// Screen on (interactive) or off.
    Screen {
        /// Epoch milliseconds.
        t_ms: i64,
        /// On.
        on: bool,
    },
    /// Refuses the last switch (one gesture).
    Refuse {
        /// Epoch milliseconds.
        t_ms: i64,
    },
    /// Answers the last proposal (one gesture).
    Answer {
        /// Epoch milliseconds.
        t_ms: i64,
        /// Accepted.
        accept: bool,
    },
    /// Steps counted by the phone since the previous report (X10).
    Steps {
        /// Epoch milliseconds.
        t_ms: i64,
        /// Steps since the previous report.
        steps: u32,
    },
    /// Trails closed (OSM ways).
    Closed {
        /// Epoch milliseconds.
        t_ms: i64,
        /// OSM ways.
        ways: Vec<i64>,
    },
}

impl TrackEvent {
    fn input(&self) -> Input {
        match self {
            TrackEvent::Activity { t_ms, activity } => Input::Activity {
                t: utc(*t_ms),
                activity: match activity {
                    TrackActivity::Still => Activity::Still,
                    TrackActivity::OnFoot => Activity::OnFoot,
                    TrackActivity::InVehicle => Activity::InVehicle,
                    TrackActivity::Unknown => Activity::Unknown,
                    TrackActivity::SignificantMotion => Activity::SignificantMotion,
                },
            },
            TrackEvent::Screen { t_ms, on } => Input::Screen {
                t: utc(*t_ms),
                on: *on,
            },
            TrackEvent::Refuse { t_ms } => Input::Refuse { t: utc(*t_ms) },
            TrackEvent::Answer { t_ms, accept } => Input::Answer {
                t: utc(*t_ms),
                accept: *accept,
            },
            TrackEvent::Steps { t_ms, steps } => Input::Steps {
                t: utc(*t_ms),
                steps: *steps,
            },
            TrackEvent::Closed { t_ms, ways } => Input::Closed {
                t: utc(*t_ms),
                ways: ways.clone(),
            },
        }
    }
}

/// Steps counted by the phone over one report (X10): Android's step counter, summed over
/// about 10 s, delivered with the batch of fixes it overlaps ([`TrackingSession::push_batch`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Record)]
pub struct StepReport {
    /// End of the report, epoch milliseconds.
    pub t_ms: i64,
    /// Steps since the previous report (0: none, the phone still counts).
    pub steps: u32,
}

/// GPS mode decided by the core (report 09, `skimap_plan::gps`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum GpsModeView {
    /// Before the start, after the end.
    Idle,
    /// In the pocket on the route.
    Pocket,
    /// Approaching a decision point.
    Approach,
    /// Off the route.
    Deviation,
    /// Screen on.
    Interaction,
    /// Stopped.
    Pause,
    /// Stopped at a refuge.
    Refuge,
    /// Moving again after a pause.
    Waking,
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

/// What the location service applies.
#[derive(Debug, Clone, Copy, PartialEq, uniffi::Record)]
pub struct GpsRequestView {
    /// Mode.
    pub mode: GpsModeView,
    /// GNSS on (high accuracy); off: passive.
    pub gnss: bool,
    /// Fix interval, milliseconds (also the minimum interval).
    pub interval_ms: i64,
    /// Batch delay (`setMaxUpdateDelayMillis`), milliseconds.
    pub max_delay_ms: i64,
    /// Arm the significant motion sensor.
    pub significant_motion: bool,
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

/// Priority of what is said (J4).
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum SpeechPriority {
    /// Safety: never lost.
    Safety,
    /// Direction.
    Direction,
    /// Schedule.
    Schedule,
    /// Comfort.
    Comfort,
}

fn priority_view(p: Priority) -> SpeechPriority {
    match p {
        Priority::Safety => SpeechPriority::Safety,
        Priority::Direction => SpeechPriority::Direction,
        Priority::Schedule => SpeechPriority::Schedule,
        Priority::Comfort => SpeechPriority::Comfort,
    }
}

/// One clip of an announcement.
#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct ClipSegment {
    /// Clip id.
    pub id: String,
    /// Text.
    pub text: String,
    /// Ogg/Opus file of the pack, when the pack has this clip with this text.
    pub file: Option<String>,
    /// Silence after it, milliseconds (none after the last).
    pub pause_ms: u32,
}

/// Something to say.
#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct SpokenView {
    /// Epoch milliseconds.
    pub t_ms: i64,
    /// Text (the fallback voice reads it when a clip is missing).
    pub text: String,
    /// Clips in order.
    pub segments: Vec<ClipSegment>,
    /// Every clip is in the pack.
    pub all_clips: bool,
    /// Highest priority of the items said.
    pub priority: SpeechPriority,
    /// What each item is (`turn`, `off_route`, `fog`, `proposal`…).
    pub kinds: Vec<String>,
}

/// Kind of alert (K6): drives the vibration pattern.
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum AlertKind {
    /// Off the route (or off the way back).
    OffRoute,
    /// Back on it.
    BackOnRoute,
    /// Wind rising.
    WindRising,
    /// Fog coming.
    Fog,
    /// Snow starting.
    Snow,
    /// Temperature dropping.
    TemperatureDrop,
    /// Turnaround moved earlier, or late for the return asked.
    TurnaroundAdvanced,
    /// Turn around now.
    TurnaroundNow,
    /// A closed trail ahead (a way around it is taken).
    Closed,
    /// A closed trail ahead, no way around it known.
    ClosedNoDetour,
}

/// An alert raised.
#[derive(Debug, Clone, Copy, PartialEq, uniffi::Record)]
pub struct AlertView {
    /// Epoch milliseconds.
    pub t_ms: i64,
    /// Kind.
    pub kind: AlertKind,
}

fn alert_kind(a: &Alert) -> AlertKind {
    match a {
        Alert::OffRoute => AlertKind::OffRoute,
        Alert::BackOnRoute => AlertKind::BackOnRoute,
        Alert::Condition(Condition::WindRising) => AlertKind::WindRising,
        Alert::Condition(Condition::Fog) => AlertKind::Fog,
        Alert::Condition(Condition::Snow) => AlertKind::Snow,
        Alert::Condition(Condition::TemperatureDrop) => AlertKind::TemperatureDrop,
        Alert::TurnaroundAdvanced => AlertKind::TurnaroundAdvanced,
        Alert::TurnaroundNow => AlertKind::TurnaroundNow,
        Alert::Closed => AlertKind::Closed,
        Alert::ClosedNoDetour => AlertKind::ClosedNoDetour,
    }
}

/// A change of route (J2).
#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct SwitchView {
    /// Epoch milliseconds.
    pub t_ms: i64,
    /// Route followed now: `plan`, `plan_b`, `live`.
    pub route: String,
    /// Why, French.
    pub cause: String,
    /// The previous route was put back (refusal).
    pub reverted: bool,
    /// The skier may refuse it with one gesture.
    pub refusable: bool,
}

fn route_name(r: RouteRef) -> String {
    match r {
        RouteRef::Plan => "plan".into(),
        RouteRef::PlanB(_) => "plan_b".into(),
        RouteRef::Live(_) => "live".into(),
    }
}

fn cause_fr(c: &Cause) -> String {
    match c {
        Cause::Closure { .. } => "piste fermée sur l'itinéraire".into(),
        Cause::Late { limit } => format!("retard sur la limite de {}", hhmm(*limit)),
        Cause::Conditions(Condition::Fog) => "brouillard".into(),
        Cause::Conditions(Condition::WindRising) => "vent qui se lève".into(),
        Cause::Conditions(Condition::Snow) => "neige".into(),
        Cause::Conditions(Condition::TemperatureDrop) => "froid".into(),
        Cause::Deviation => "écart de l'itinéraire".into(),
        Cause::NoTime => "plus le temps de finir la boucle avant la nuit".into(),
        Cause::Pace => "proposition acceptée".into(),
    }
}

/// A proposal (C11), to accept or refuse with one gesture.
#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct ProposalView {
    /// Number in the outing.
    pub id: u32,
    /// Epoch milliseconds.
    pub t_ms: i64,
    /// `short_loop`, `nearest_exit`, `keep`.
    pub kind: String,
    /// Text, French.
    pub text: String,
}

/// A decision on the outing.
#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct SessionChange {
    /// Epoch milliseconds.
    pub t_ms: i64,
    /// `started`, `paused`, `refuge`, `resumed`, `near_finish`, `arrived`, `abandoned`.
    pub what: String,
    /// Refuge name.
    pub name: Option<String>,
}

/// End of the outing.
#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct RecapView {
    /// Back at the finish (else abandoned away from it).
    pub arrived: bool,
    /// Distance skied, km.
    pub distance_km: f64,
    /// Moving, minutes.
    pub moving_min: f64,
    /// Stopped, minutes.
    pub paused_min: f64,
    /// Text, French.
    pub text: String,
}

/// Direction of a turn (arrow of the banner).
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum TurnDirection {
    /// Turn left.
    Left,
    /// Turn right.
    Right,
    /// Bear left.
    SlightLeft,
    /// Bear right.
    SlightRight,
    /// Keep left.
    KeepLeft,
    /// Keep right.
    KeepRight,
    /// Straight on.
    Straight,
    /// Turn back.
    UTurn,
}

pub(crate) fn direction_view(d: Direction) -> TurnDirection {
    match d {
        Direction::Left => TurnDirection::Left,
        Direction::Right => TurnDirection::Right,
        Direction::SlightLeft => TurnDirection::SlightLeft,
        Direction::SlightRight => TurnDirection::SlightRight,
        Direction::KeepLeft => TurnDirection::KeepLeft,
        Direction::KeepRight => TurnDirection::KeepRight,
        Direction::Straight => TurnDirection::Straight,
        Direction::UTurn => TurnDirection::UTurn,
    }
}

/// The next turn said.
#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct NextTurn {
    /// Direction.
    pub direction: TurnDirection,
    /// Distance along the route, metres.
    pub distance_m: f64,
    /// Trail taken.
    pub name: Option<String>,
}

/// Phase of the outing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum PhaseView {
    /// Before the start.
    Waiting,
    /// Skiing.
    Moving,
    /// Stopped.
    Paused,
    /// Stopped at a refuge.
    Refuge,
    /// Over.
    Finished,
}

/// What the thin banner and the lock-screen notification show.
#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct GuidanceView {
    /// Phase.
    pub phase: PhaseView,
    /// Route followed: `plan`, `plan_b`, `live`, `back` (« ramène-moi »).
    pub route: String,
    /// Label of the loop.
    pub label: String,
    /// Along the route, metres.
    pub progress_m: f64,
    /// Length of the route, metres.
    pub length_m: f64,
    /// Left, metres.
    pub remaining_m: f64,
    /// Next turn said.
    pub next: Option<NextTurn>,
    /// Time left at the pace of the day (median), seconds; before the start: planned.
    pub remaining_s: f64,
    /// « 1 h 05 » (rounded to 5 min).
    pub remaining_text: String,
    /// Arrival, « 15 h 20 ».
    pub arrival: String,
    /// Arrival, epoch milliseconds.
    pub arrival_ms: i64,
    /// « 15 h 05 à 15 h 40 » (80 %), once the outing started with a group.
    pub arrival_range: Option<String>,
    /// Limit to be back, « 16 h 30 ».
    pub limit: Option<String>,
    /// Distance to the route, metres (infinite before the first fix).
    pub off_route_m: f64,
    /// GPS mode in force.
    pub gps_mode: GpsModeView,
    /// Proposal waiting for an answer.
    pub proposal: Option<ProposalView>,
    /// A switch the skier may refuse.
    pub switch: Option<SwitchView>,
}

/// What a call produced, in order of kind.
#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct BatchOut {
    /// To say, in order.
    pub spoken: Vec<SpokenView>,
    /// Alerts raised.
    pub alerts: Vec<AlertView>,
    /// Changes of route.
    pub switches: Vec<SwitchView>,
    /// Proposals.
    pub proposals: Vec<ProposalView>,
    /// Answers taken into account: (accepted, applied).
    pub answered: Vec<bool>,
    /// Decisions on the outing.
    pub session: Vec<SessionChange>,
    /// GPS request to apply (the last one of the call), when it changed.
    pub gps: Option<GpsRequestView>,
    /// The outing is over.
    pub recap: Option<RecapView>,
    /// Guidance after the call.
    pub guidance: GuidanceView,
}

/// Clips of the pack: id → file, with the text the pack says.
struct Clips {
    manifest: Option<VoiceManifest>,
    dir: PathBuf,
}

impl Clips {
    fn segment(&self, a: &Announcement) -> (Vec<ClipSegment>, bool) {
        let n = a.segments.len();
        let mut all = true;
        let segs = a
            .segments
            .iter()
            .enumerate()
            .map(|(i, s)| {
                let file = self
                    .manifest
                    .as_ref()
                    .and_then(|m| m.get(&s.clip.id))
                    .filter(|e| e.text == s.clip.text)
                    .map(|e| self.dir.join(&e.file).display().to_string());
                all &= file.is_some();
                ClipSegment {
                    id: s.clip.id.clone(),
                    text: s.clip.text.clone(),
                    file,
                    pause_ms: if i + 1 < n { s.pause.ms() } else { 0 },
                }
            })
            .collect();
        (segs, all)
    }

    fn spoken(
        &self,
        t_ms: i64,
        a: &Announcement,
        priority: Priority,
        kinds: Vec<String>,
    ) -> SpokenView {
        let (segments, all_clips) = self.segment(a);
        SpokenView {
            t_ms,
            text: a.text(),
            segments,
            all_clips,
            priority: priority_view(priority),
            kinds,
        }
    }

    fn of(&self, t_ms: i64, s: &Spoken) -> SpokenView {
        let priority = s
            .items
            .iter()
            .map(|c| c.priority)
            .min_by_key(|p| *p as u8)
            .unwrap_or(Priority::Comfort);
        self.spoken(
            t_ms,
            &s.announcement,
            priority,
            s.items.iter().map(|c| c.kind.clone()).collect(),
        )
    }
}

/// The route followed and its junctions, recomputed when the route changes.
struct RouteCache {
    route: RouteRef,
    legs_len: usize,
    length_m: f64,
    model_s: f64,
    junctions: Vec<Junction>,
    course: Option<Course>,
}

/// Everything the tracker borrows, owned by the session's thread.
struct Owned {
    plan: DayPlan,
    trk: Vec<u8>,
    graph: PisteGraph,
    voice: Option<Voice>,
    clips: Clips,
    /// The pack's layers of the day (X11): a route computed during the outing announces
    /// zones without network, avalanche paths and protected zones.
    layers: Option<SiteData>,
    log: PathBuf,
}

enum Cmd {
    Fixes(Vec<u8>, Vec<StepReport>),
    Event(TrackEvent),
    BackStart(i64),
    BackStop,
    SayNext(i64),
    SayRemaining(i64),
    Guidance,
    Observation,
    Gpx,
}

enum Reply {
    Out(Box<BatchOut>),
    Said(Option<SpokenView>),
    Guidance(Box<GuidanceView>),
    Text(String),
}

/// The two ends of the session's thread: calls out, answers back.
type Chan = (Sender<Cmd>, Receiver<Result<Reply, TrackingError>>);

/// The tracker of one outing, on its own thread.
#[derive(uniffi::Object)]
pub struct TrackingSession {
    chan: Mutex<Chan>,
    plan_dir: PathBuf,
    replayed: u32,
    layers: Vec<String>,
}

impl std::fmt::Debug for TrackingSession {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TrackingSession")
            .field("plan_dir", &self.plan_dir)
            .finish_non_exhaustive()
    }
}

fn read_inputs(path: &Path) -> Result<Vec<Input>, TrackingError> {
    let text = match std::fs::read_to_string(path) {
        Ok(t) => t,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(invalid(format!("{}: {e}", path.display()))),
    };
    // A line cut by the end of the process is dropped (the batch was not processed).
    Ok(text
        .lines()
        .filter_map(|l| serde_json::from_str(l).ok())
        .collect())
}

/// Opens the tracking of the plan in `plan_dir` (written by « Valider la journée »).
/// Inputs already recorded there (the process was restarted during the outing) are
/// replayed first.
#[uniffi::export]
pub fn open_tracking(plan_dir: String) -> Result<Arc<TrackingSession>, TrackingError> {
    let dir = PathBuf::from(&plan_dir);
    let read = |n: &str| std::fs::read(dir.join(n)).map_err(|e| invalid(format!("{n}: {e}")));
    let plan = DayPlan::from_json(&String::from_utf8_lossy(&read(PLAN_FILE)?)).map_err(invalid)?;
    let trk = read(TRACK_FILE)?;
    let pack_dir = PathBuf::from(String::from_utf8_lossy(&read(PACK_FILE)?).trim());
    let graph = load_graph(&pack_dir)?;
    let layers = load_layers(&pack_dir, &dir.join(LAYERS_DIR));
    TrackingSession::start_with(plan, trk, graph, Some(pack_dir.join("voice")), layers, dir)
}

/// The layers of the pack in `pack_dir` for the tracker (X11), loaded by the morning's
/// loaders on the pack's files linked under `work` ([`PackView`]). An error (a pack without
/// the layers, or of a site without them) leaves the outing followed without them: the
/// reason is the error's text.
pub fn load_layers(pack_dir: &Path, work: &Path) -> Result<SiteData, String> {
    let pack = Pack::open(pack_dir.to_path_buf()).map_err(|e| e.to_string())?;
    let site = skimap_common::site::site(&pack.manifest().site.id).map_err(|e| e.to_string())?;
    let view = PackView::build(&pack, work).map_err(|e| e.to_string())?;
    SiteData::load(&view.layout, site, &Paths::default()).map_err(|e| e.to_string())
}

/// What the tracker has of the layers: `no_network`, `avalanche_paths`, `zones`, and the
/// reason of each one missing.
fn layers_note(l: &Result<SiteData, String>) -> Vec<String> {
    match l {
        Err(e) => vec![format!("sans couches : {e}")],
        Ok(d) => {
            let mut v = Vec::new();
            v.push(
                if d.no_network.is_some() {
                    "no_network"
                } else {
                    "-no_network"
                }
                .into(),
            );
            v.push(
                if d.clpa.is_some() {
                    "avalanche_paths"
                } else {
                    "-avalanche_paths"
                }
                .into(),
            );
            v.push(if d.zones.is_some() { "zones" } else { "-zones" }.into());
            v
        }
    }
}

impl TrackingSession {
    /// A session on a plan, its table and graph, with the clips of the voice layer in
    /// `voice_dir` (`None`: the Android voice says everything); inputs are recorded in
    /// `plan_dir` and those already there are replayed first.
    pub fn start(
        plan: DayPlan,
        trk: Vec<u8>,
        graph: PisteGraph,
        voice_dir: Option<PathBuf>,
        plan_dir: PathBuf,
    ) -> Result<Arc<Self>, TrackingError> {
        Self::start_with(
            plan,
            trk,
            graph,
            voice_dir,
            Err("non demandées".into()),
            plan_dir,
        )
    }

    /// [`TrackingSession::start`] with the layers of the day for the tracker
    /// ([`Tracker::with_layers`], X11), or why there are none.
    pub fn start_with(
        plan: DayPlan,
        trk: Vec<u8>,
        graph: PisteGraph,
        voice_dir: Option<PathBuf>,
        layers: Result<SiteData, String>,
        plan_dir: PathBuf,
    ) -> Result<Arc<Self>, TrackingError> {
        let note = layers_note(&layers);
        let manifest = voice_dir.as_ref().and_then(|v| VoiceManifest::read(v).ok());
        let voice = manifest
            .as_ref()
            .zip(voice_dir.as_ref())
            .map(|(m, v)| Voice::new(m, v.join("manifest.json").display().to_string()));
        let log = plan_dir.join(INPUTS_FILE);
        let past = read_inputs(&log)?;
        let replayed = u32::try_from(past.len()).unwrap_or(u32::MAX);
        let owned = Owned {
            plan,
            trk,
            graph,
            voice,
            clips: Clips {
                manifest,
                dir: voice_dir.unwrap_or_default(),
            },
            layers: layers.ok(),
            log,
        };
        let (tx, rx) = channel::<Cmd>();
        let (rtx, rrx) = channel::<Result<Reply, TrackingError>>();
        std::thread::Builder::new()
            .name("skimap-tracker".into())
            .spawn(move || serve(owned, past, rx, rtx))
            .map_err(invalid)?;
        // The thread answers once the tracker is built (or why it is not).
        match rrx.recv() {
            Ok(Ok(_)) => {}
            Ok(Err(e)) => return Err(e),
            Err(_) => return Err(TrackingError::Closed),
        }
        Ok(Arc::new(Self {
            chan: Mutex::new((tx, rrx)),
            plan_dir,
            replayed,
            layers: note,
        }))
    }
}

impl TrackingSession {
    fn call(&self, cmd: Cmd) -> Result<Reply, TrackingError> {
        let chan = self
            .chan
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        chan.0.send(cmd).map_err(|_| TrackingError::Closed)?;
        chan.1.recv().map_err(|_| TrackingError::Closed)?
    }

    fn out(&self, cmd: Cmd) -> Result<BatchOut, TrackingError> {
        match self.call(cmd)? {
            Reply::Out(o) => Ok(*o),
            _ => Err(TrackingError::Closed),
        }
    }

    fn said(&self, cmd: Cmd) -> Result<Option<SpokenView>, TrackingError> {
        match self.call(cmd)? {
            Reply::Said(s) => Ok(s),
            _ => Err(TrackingError::Closed),
        }
    }

    fn text(&self, cmd: Cmd) -> Result<String, TrackingError> {
        match self.call(cmd)? {
            Reply::Text(s) => Ok(s),
            _ => Err(TrackingError::Closed),
        }
    }
}

#[uniffi::export]
impl TrackingSession {
    /// Directory of the plan.
    pub fn plan_dir(&self) -> String {
        self.plan_dir.display().to_string()
    }

    /// Inputs replayed when the session opened (a restart during the outing).
    pub fn replayed(&self) -> u32 {
        self.replayed
    }

    /// What the tracker has of the pack's layers (X11): `no_network`, `avalanche_paths`,
    /// `zones` (`-` before a missing one), or why it has none.
    pub fn layers(&self) -> Vec<String> {
        self.layers.clone()
    }

    /// A batch of fixes, packed ([`FIX_BYTES`] each: epoch ms i64, lon f64, lat f64, little
    /// endian), in time order.
    pub fn push_fixes(&self, packed: Vec<u8>) -> Result<BatchOut, TrackingError> {
        self.push_batch(packed, Vec::new())
    }

    /// A batch of fixes (as [`TrackingSession::push_fixes`]) and the steps counted over the
    /// same time (X10), given to the tracker in time order (a report at the time of a fix
    /// after it): one call per batch.
    pub fn push_batch(
        &self,
        packed: Vec<u8>,
        steps: Vec<StepReport>,
    ) -> Result<BatchOut, TrackingError> {
        if !packed.len().is_multiple_of(FIX_BYTES) {
            return Err(invalid(format!(
                "{} octets : pas des positions",
                packed.len()
            )));
        }
        self.out(Cmd::Fixes(packed, steps))
    }

    /// An input other than a fix.
    pub fn push_event(&self, event: TrackEvent) -> Result<BatchOut, TrackingError> {
        self.out(Cmd::Event(event))
    }

    /// « Ramène-moi »: guidance back along the way skied, from the last fix.
    pub fn back_start(&self, t_ms: i64) -> Result<BatchOut, TrackingError> {
        self.out(Cmd::BackStart(t_ms))
    }

    /// Ends « ramène-moi »: the route of the tracker again.
    pub fn back_stop(&self) -> Result<BatchOut, TrackingError> {
        self.out(Cmd::BackStop)
    }

    /// The next direction, to say now (headset button, volume key).
    pub fn say_next(&self, t_ms: i64) -> Result<Option<SpokenView>, TrackingError> {
        self.said(Cmd::SayNext(t_ms))
    }

    /// Time and distance left, to say now.
    pub fn say_remaining(&self, t_ms: i64) -> Result<Option<SpokenView>, TrackingError> {
        self.said(Cmd::SayRemaining(t_ms))
    }

    /// Guidance now.
    pub fn guidance(&self) -> Result<GuidanceView, TrackingError> {
        match self.call(Cmd::Guidance)? {
            Reply::Guidance(g) => Ok(*g),
            _ => Err(TrackingError::Closed),
        }
    }

    /// What the outing tells about the group (C11), JSON for `group_learn`.
    pub fn observation(&self) -> Result<String, TrackingError> {
        self.text(Cmd::Observation)
    }

    /// Writes the recorded trace as GPX next to the plan; returns its path.
    pub fn write_gpx(&self) -> Result<String, TrackingError> {
        self.text(Cmd::Gpx)
    }
}

/// The session's thread: builds the tracker on the owned data, replays the past inputs,
/// then answers the calls until the session is dropped.
fn serve(
    owned: Owned,
    past: Vec<Input>,
    rx: Receiver<Cmd>,
    tx: Sender<Result<Reply, TrackingError>>,
) {
    let view = match TrackView::new(&owned.trk) {
        Ok(v) => v,
        Err(e) => {
            let _ = tx.send(Err(invalid(e)));
            return;
        }
    };
    let tracker = match Tracker::new(
        &owned.plan,
        view,
        &owned.graph,
        owned.voice.as_ref(),
        TrackerParams::default(),
    ) {
        Ok(t) => match &owned.layers {
            Some(l) => t.with_layers(l),
            None => t,
        },
        Err(e) => {
            let _ = tx.send(Err(invalid(e)));
            return;
        }
    };
    let mut e = Engine::new(&owned, tracker);
    for i in &past {
        e.feed(i);
    }
    // The outputs of the past rebuild what the screens show (route, proposal, switch,
    // GPS mode); they were said before the restart.
    let _ = e.collect();
    if e.open_log().is_err() {
        // Without a log the outing is followed all the same; nothing is learned.
        e.log = None;
    }
    let _ = tx.send(Ok(Reply::Text(String::new())));
    for cmd in rx {
        let reply = e.handle(cmd);
        if tx.send(reply).is_err() {
            break;
        }
    }
}

struct Engine<'a> {
    o: &'a Owned,
    tracker: Tracker<'a>,
    out: Vec<Output>,
    inputs: Vec<Input>,
    log: Option<std::io::BufWriter<std::fs::File>>,
    cache: RouteCache,
    /// The way skied: a point every 10 m.
    trace: Vec<LonLat>,
    last: Option<(f64, LonLat)>,
    back: Option<Backtrack>,
    /// What the way back said per fix since the last call.
    back_steps: Vec<(i64, crate::tracking_back::BackStep)>,
    back_gps: GpsState,
    last_gps: Option<GpsRequest>,
    proposal: Option<ProposalView>,
    switch: Option<SwitchView>,
    /// Where the refusable switch leaves the previous route, metres: accepted (no longer
    /// refusable) once the skier is `accept_m` past it.
    switch_div: f64,
    finished: bool,
}

const TRACE_STEP_M: f64 = 10.0;
/// A fix this far from the previous one…
const TRACE_JUMP_M: f64 = 1000.0;
/// …faster than this, m/s, is not skied: the trace restarts there.
const TRACE_JUMP_MS: f64 = 60.0;

impl<'a> Engine<'a> {
    fn new(o: &'a Owned, tracker: Tracker<'a>) -> Self {
        let mut e = Self {
            o,
            tracker,
            out: Vec::with_capacity(16),
            inputs: Vec::with_capacity(64),
            log: None,
            cache: RouteCache {
                route: RouteRef::Live(usize::MAX),
                legs_len: 0,
                length_m: 0.0,
                model_s: 0.0,
                junctions: Vec::new(),
                course: None,
            },
            trace: Vec::with_capacity(8192),
            last: None,
            back: None,
            back_steps: Vec::new(),
            back_gps: GpsState::default(),
            last_gps: None,
            proposal: None,
            switch: None,
            switch_div: 0.0,
            finished: false,
        };
        e.refresh_route();
        e
    }

    fn open_log(&mut self) -> std::io::Result<()> {
        let f = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.o.log)?;
        self.log = Some(std::io::BufWriter::new(f));
        Ok(())
    }

    fn refresh_route(&mut self) {
        let rf = self.tracker.route();
        let legs = self.tracker.legs();
        if rf == self.cache.route && legs.len() == self.cache.legs_len {
            return;
        }
        let course = Course::new(&self.o.graph, legs, &self.o.plan.eta).ok();
        let junctions = course
            .as_ref()
            .map(|c| guidance::junctions(&self.o.graph, c))
            .unwrap_or_default();
        self.cache = RouteCache {
            route: rf,
            legs_len: legs.len(),
            length_m: course.as_ref().map_or(0.0, Course::length_m),
            model_s: course.as_ref().map_or(0.0, Course::model_s),
            junctions,
            course,
        };
    }

    /// One input through the tracker (and the way back), outputs into `self.out`.
    fn feed(&mut self, i: &Input) {
        if let Input::Fix { t, pos } = i {
            let ts = t.timestamp_millis() as f64 / 1000.0;
            // A jump no skier makes (the first fixes of a phone are often a cached position
            // from elsewhere): the way skied starts again from here.
            if let Some((t0, p0)) = self.last {
                let d = p0.haversine_m(*pos);
                if d > TRACE_JUMP_M && d > TRACE_JUMP_MS * (ts - t0).max(1.0) {
                    self.trace.clear();
                }
            }
            match self.trace.last() {
                Some(p) if p.haversine_m(*pos) < TRACE_STEP_M => {}
                _ => self.trace.push(*pos),
            }
            self.last = Some((ts, *pos));
            if let Some(b) = self.back.as_mut() {
                let st = b.fix(ts, *pos);
                if st.off.is_some() || !st.say.is_empty() || st.arrived {
                    self.back_steps.push((t.timestamp_millis(), st));
                }
            }
        }
        self.tracker.push(i, &mut self.out);
    }

    fn record(&mut self, inputs: &[Input]) {
        if let Some(w) = self.log.as_mut() {
            for i in inputs {
                if let Ok(line) = serde_json::to_string(i) {
                    let _ = writeln!(w, "{line}");
                }
            }
            let _ = w.flush();
        }
    }

    fn handle(&mut self, cmd: Cmd) -> Result<Reply, TrackingError> {
        match cmd {
            Cmd::Fixes(b, steps) => {
                let mut inputs = std::mem::take(&mut self.inputs);
                inputs.clear();
                for c in b.as_chunks::<FIX_BYTES>().0 {
                    let word = |i: usize| {
                        let mut w = [0u8; 8];
                        w.copy_from_slice(&c[i..i + 8]);
                        w
                    };
                    let t = i64::from_le_bytes(word(0));
                    let lon = f64::from_le_bytes(word(8));
                    let lat = f64::from_le_bytes(word(16));
                    if !(lon.is_finite() && lat.is_finite()) {
                        continue;
                    }
                    inputs.push(Input::Fix {
                        t: utc(t),
                        pos: LonLat::new(lon, lat),
                    });
                }
                if !steps.is_empty() {
                    inputs.extend(steps.iter().map(|s| Input::Steps {
                        t: utc(s.t_ms),
                        steps: s.steps,
                    }));
                    // Stable: a report at the time of a fix comes after it.
                    inputs.sort_by_key(Input::t);
                }
                for i in &inputs {
                    self.feed(i);
                }
                self.record(&inputs);
                self.inputs = inputs;
                Ok(Reply::Out(Box::new(self.collect())))
            }
            Cmd::Event(ev) => {
                let i = ev.input();
                if matches!(ev, TrackEvent::Refuse { .. }) {
                    self.switch = None;
                }
                if matches!(ev, TrackEvent::Answer { .. }) {
                    self.proposal = None;
                }
                self.feed(&i);
                self.record(std::slice::from_ref(&i));
                Ok(Reply::Out(Box::new(self.collect())))
            }
            Cmd::BackStart(t_ms) => {
                let here = self.last.map(|(_, p)| p);
                self.back = here.and_then(|h| Backtrack::new(&self.trace, h));
                self.back_gps = GpsState::default();
                let mut out = self.collect();
                let t = t_ms as f64 / 1000.0;
                if let Some(b) = self.back.as_mut() {
                    let a = b.start_announcement();
                    out.spoken.push(self.o.clips.spoken(
                        t_ms,
                        &a,
                        Priority::Direction,
                        vec!["back".into()],
                    ));
                    let (st, req) = back_gps(&self.back_gps, b, t, self.last.map(|(_, p)| p));
                    self.back_gps = st;
                    out.gps = Some(gps_view(&req));
                    self.last_gps = Some(req);
                }
                out.guidance = self.guidance();
                Ok(Reply::Out(Box::new(out)))
            }
            Cmd::BackStop => {
                self.back = None;
                let mut out = self.collect();
                if let Some(r) = self.tracker.gps_request() {
                    out.gps = Some(gps_view(&r));
                    self.last_gps = Some(r);
                }
                Ok(Reply::Out(Box::new(out)))
            }
            Cmd::SayNext(t_ms) => Ok(Reply::Said(self.say_next(t_ms))),
            Cmd::SayRemaining(t_ms) => Ok(Reply::Said(self.say_remaining(t_ms))),
            Cmd::Guidance => Ok(Reply::Guidance(Box::new(self.guidance()))),
            Cmd::Observation => serde_json::to_string(&self.tracker.pace_observation())
                .map(Reply::Text)
                .map_err(invalid),
            Cmd::Gpx => self.gpx().map(Reply::Text),
        }
    }

    /// Turns the tracker's outputs (and the way back) into the batch result.
    fn collect(&mut self) -> BatchOut {
        let mut b = BatchOut {
            spoken: Vec::new(),
            alerts: Vec::new(),
            switches: Vec::new(),
            proposals: Vec::new(),
            answered: Vec::new(),
            session: Vec::new(),
            gps: None,
            recap: None,
            guidance: placeholder_guidance(),
        };
        let backing = self.back.is_some();
        for o in self.out.drain(..) {
            match o {
                Output::Spoken { t, spoken } => {
                    // Going back, only safety is said from the tracker.
                    if !backing || spoken.items.iter().any(|c| c.priority == Priority::Safety) {
                        b.spoken.push(self.o.clips.of(ms(t), &spoken));
                    }
                }
                Output::Alert { t, alert } => {
                    if !(backing && matches!(alert, Alert::OffRoute | Alert::BackOnRoute)) {
                        b.alerts.push(AlertView {
                            t_ms: ms(t),
                            kind: alert_kind(&alert),
                        });
                    }
                }
                Output::Switched {
                    t,
                    to,
                    cause,
                    s_div,
                    ..
                } => {
                    self.switch_div = s_div;
                    let s = SwitchView {
                        t_ms: ms(t),
                        route: route_name(to),
                        refusable: !matches!(cause, Cause::NoTime | Cause::Deviation),
                        cause: cause_fr(&cause),
                        reverted: false,
                    };
                    self.switch = s.refusable.then(|| s.clone());
                    b.switches.push(s);
                }
                Output::Reverted { t, to, cause, .. } => {
                    self.switch = None;
                    b.switches.push(SwitchView {
                        t_ms: ms(t),
                        route: route_name(to),
                        cause: cause_fr(&cause),
                        reverted: true,
                        refusable: false,
                    });
                }
                Output::Gps { request, .. } => {
                    if !backing {
                        b.gps = Some(gps_view(&request));
                        self.last_gps = Some(request);
                    }
                }
                Output::Session { t, event } => {
                    let (what, name) = match &event {
                        SessionEvent::Started { .. } => ("started", None),
                        SessionEvent::Paused { .. } => ("paused", None),
                        SessionEvent::Refuge { name, .. } => ("refuge", Some(name.clone())),
                        SessionEvent::Resumed { .. } => ("resumed", None),
                        SessionEvent::NearFinish { .. } => ("near_finish", None),
                        SessionEvent::Finished { end, .. } => match end {
                            End::Arrived => ("arrived", None),
                            End::Abandoned => ("abandoned", None),
                        },
                    };
                    b.session.push(SessionChange {
                        t_ms: ms(t),
                        what: what.into(),
                        name,
                    });
                }
                Output::Recap { recap, .. } => {
                    self.finished = true;
                    b.recap = Some(RecapView {
                        arrived: recap.end == End::Arrived,
                        distance_km: recap.distance_km,
                        moving_min: recap.moving_min,
                        paused_min: recap.paused_min,
                        text: recap.text,
                    });
                }
                Output::Proposal { t, proposal } => {
                    let p = ProposalView {
                        id: proposal.id,
                        t_ms: ms(t),
                        kind: match proposal.kind {
                            ProposalKind::ShortLoop => "short_loop",
                            ProposalKind::NearestExit => "nearest_exit",
                            ProposalKind::Keep => "keep",
                        }
                        .into(),
                        text: proposal.text.clone(),
                    };
                    self.proposal = Some(p.clone());
                    b.proposals.push(p);
                }
                Output::Answered { accept, .. } => {
                    self.proposal = None;
                    b.answered.push(accept);
                }
            }
        }
        self.refresh_route();
        self.expire();
        self.back_step(&mut b);
        b.guidance = self.guidance();
        b
    }

    /// A switch is no longer refusable once accepted by skiing on it; a proposal not
    /// answered expires (the tracker's own rules, `TrackerParams`).
    fn expire(&mut self) {
        let p = TrackerParams::default();
        if self.switch.is_some() && self.tracker.progress() > self.switch_div + p.accept_m {
            self.switch = None;
        }
        if let (Some(pr), Some((t, _))) = (&self.proposal, self.last)
            && t - pr.t_ms as f64 / 1000.0 >= p.propose.ttl_s
        {
            self.proposal = None;
        }
    }

    /// The way back since the last call: its announcements, alerts and GPS mode.
    fn back_step(&mut self, b: &mut BatchOut) {
        let mut arrived = false;
        for (t_ms, step) in self.back_steps.drain(..) {
            if let Some(off) = step.off {
                b.alerts.push(AlertView {
                    t_ms,
                    kind: if off {
                        AlertKind::OffRoute
                    } else {
                        AlertKind::BackOnRoute
                    },
                });
            }
            for (a, kind) in step.say {
                let pr = if kind == "off_route" {
                    Priority::Safety
                } else {
                    Priority::Direction
                };
                b.spoken
                    .push(self.o.clips.spoken(t_ms, &a, pr, vec![kind.to_owned()]));
            }
            arrived |= step.arrived;
        }
        let (Some(bt), Some((t, pos))) = (self.back.as_ref(), self.last) else {
            return;
        };
        let (st, req) = back_gps(&self.back_gps, bt, t, Some(pos));
        if self.last_gps.is_none_or(|r| r != req) {
            b.gps = Some(gps_view(&req));
            self.last_gps = Some(req);
        }
        self.back_gps = st;
        if arrived {
            // Arrived: the tracker's route and GPS mode again.
            self.back = None;
            if let Some(r) = self.tracker.gps_request() {
                b.gps = Some(gps_view(&r));
                self.last_gps = Some(r);
            }
        }
    }

    fn phase(&self) -> PhaseView {
        match self.tracker.phase() {
            Phase::Waiting => PhaseView::Waiting,
            Phase::Moving => PhaseView::Moving,
            Phase::Paused {
                refuge: Some(_), ..
            } => PhaseView::Refuge,
            Phase::Paused { .. } => PhaseView::Paused,
            Phase::Finished => PhaseView::Finished,
        }
    }

    fn next_turn(&self) -> Option<NextTurn> {
        if let Some(b) = &self.back {
            return b.next_turn().map(|(d, m)| NextTurn {
                direction: direction_view(d),
                distance_m: m,
                name: None,
            });
        }
        let s = self.tracker.progress();
        self.cache
            .junctions
            .iter()
            .find(|j| j.announced() && j.s > s - 5.0)
            .and_then(|j| {
                Some(NextTurn {
                    direction: direction_view(j.direction?),
                    distance_m: (j.s - s).max(0.0),
                    name: j.name_out.clone(),
                })
            })
    }

    fn guidance(&self) -> GuidanceView {
        let p = &self.o.plan;
        let limit = p.turnaround.limit.map(hhmm);
        let planned_back = p.pace.as_ref().map_or(p.turnaround.arrival, |x| x.planned);
        let s = self.tracker.progress();
        let mut remaining_m = (self.cache.length_m - s).max(0.0);
        let now = self.last.map(|(t, _)| t);
        let (mut remaining_s, mut arrival, arrival_range) = match self.tracker.eta() {
            Some(e) => (
                e.remaining.median_s,
                e.arrival,
                Some(format!(
                    "{} à {}",
                    clock_fr(e.arrival_lo),
                    clock_fr(e.arrival_hi)
                )),
            ),
            None => {
                let rest = match &self.cache.course {
                    Some(c) => (self.cache.model_s - c.model_s_at(s)).max(0.0),
                    None => self.cache.model_s,
                };
                (rest, planned_back, None)
            }
        };
        let mut route = route_name(self.tracker.route());
        if let Some(b) = &self.back {
            route = "back".into();
            remaining_m = b.remaining_m();
            remaining_s = b.remaining_s();
            if let Some(t) = now {
                arrival =
                    skimap_core::ephemeris::local_time(utc(((t + remaining_s) * 1000.0) as i64));
            }
        }
        GuidanceView {
            phase: self.phase(),
            route,
            label: p.route.label.clone(),
            progress_m: s,
            length_m: self.cache.length_m,
            remaining_m,
            next: self.next_turn(),
            remaining_s,
            remaining_text: duration_fr(remaining_s),
            arrival: clock_fr(arrival),
            arrival_ms: epoch_ms(arrival),
            arrival_range: if self.back.is_some() {
                None
            } else {
                arrival_range
            },
            limit,
            off_route_m: self
                .back
                .as_ref()
                .map_or(self.tracker.distance_to_route(), Backtrack::distance_m),
            gps_mode: self
                .last_gps
                .map_or(GpsModeView::Idle, |r| mode_view(r.mode)),
            proposal: self.proposal.clone(),
            switch: self.switch.clone(),
        }
    }

    fn say_next(&self, t_ms: i64) -> Option<SpokenView> {
        let g = self.guidance();
        let ev = match &g.next {
            Some(n) => Event::Turn {
                metres: n.distance_m,
                direction: match n.direction {
                    TurnDirection::Left => Direction::Left,
                    TurnDirection::Right => Direction::Right,
                    TurnDirection::SlightLeft => Direction::SlightLeft,
                    TurnDirection::SlightRight => Direction::SlightRight,
                    TurnDirection::KeepLeft => Direction::KeepLeft,
                    TurnDirection::KeepRight => Direction::KeepRight,
                    TurnDirection::Straight => Direction::Straight,
                    TurnDirection::UTurn => Direction::UTurn,
                },
                piste: None,
            },
            None => Event::Arrive {
                metres: Some(g.remaining_m),
            },
        };
        Some(self.o.clips.spoken(
            t_ms,
            &ev.announce(),
            Priority::Direction,
            vec!["next".into()],
        ))
    }

    fn say_remaining(&self, t_ms: i64) -> Option<SpokenView> {
        let g = self.guidance();
        let a = Event::Remaining {
            metres: g.remaining_m,
            seconds: g.remaining_s,
        }
        .announce();
        Some(
            self.o
                .clips
                .spoken(t_ms, &a, Priority::Schedule, vec!["remaining".into()]),
        )
    }

    fn gpx(&self) -> Result<String, TrackingError> {
        let inputs = read_inputs(&self.o.log)?;
        let mut s = String::from(
            "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<gpx version=\"1.1\" creator=\"skimap\" xmlns=\"http://www.topografix.com/GPX/1/1\">\n<trk><name>",
        );
        s.push_str(&xml_escape(&self.o.plan.route.label));
        s.push_str("</name><type>nordic_skiing</type><trkseg>\n");
        for i in &inputs {
            if let Input::Fix { t, pos } = i {
                s.push_str(&format!(
                    "<trkpt lat=\"{:.7}\" lon=\"{:.7}\"><time>{}</time></trkpt>\n",
                    pos.lat,
                    pos.lon,
                    t.format("%Y-%m-%dT%H:%M:%S%.3fZ")
                ));
            }
        }
        s.push_str("</trkseg></trk>\n</gpx>\n");
        let path = self.o.log.with_file_name("trace.gpx");
        std::fs::write(&path, s).map_err(invalid)?;
        Ok(path.display().to_string())
    }
}

fn xml_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

/// The GPS mode of the way back, decided by the core's rule (`gps::decide`) on the
/// distance to the next turn of the way back.
fn back_gps(
    prev: &GpsState,
    b: &Backtrack,
    t: f64,
    _pos: Option<LonLat>,
) -> (GpsState, GpsRequest) {
    let input = GpsInput {
        t,
        running: true,
        d_next: b.next_turn().map(|(_, d)| d),
        since_point: b.since_turn_m(),
        speed_ms: b.speed_ms(),
        d_route: b.distance_m(),
        paused: false,
        refuge: false,
        waking: false,
        screen_on: false,
    };
    skimap_plan::gps::decide(prev, &input, &GpsParams::default())
}

fn placeholder_guidance() -> GuidanceView {
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
        off_route_m: f64::INFINITY,
        gps_mode: GpsModeView::Idle,
        proposal: None,
        switch: None,
    }
}

/// Packs fixes for [`TrackingSession::push_fixes`] (tests and tools; the app packs them
/// in Kotlin).
pub fn pack_fixes(fixes: &[(i64, f64, f64)]) -> Vec<u8> {
    let mut v = Vec::with_capacity(fixes.len() * FIX_BYTES);
    for (t, lon, lat) in fixes {
        v.extend_from_slice(&t.to_le_bytes());
        v.extend_from_slice(&lon.to_le_bytes());
        v.extend_from_slice(&lat.to_le_bytes());
    }
    v
}
