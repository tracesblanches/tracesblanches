//! Following the plan during the outing (J2 #111, J3 #112, J4 #113, E1 #125, C11 #130):
//! fixes and events in, what to say, which route to follow and which GPS mode to use out.
//! Nothing to touch when all goes well.
//!
//! Everything that does not depend on the position or the real time is in the plan's flat
//! table ([`crate::track`], computed on the workstation). For each fix the phone only:
//!
//! 1. projects the fix on the route followed, in a window ahead of the cursor
//!    ([`RouteView::project_window`]; after a gap or off the route, the grid cells around
//!    the fix), and moves the cursor forward (never back);
//! 2. feeds the session ([`crate::session`]: start, pause, refuge, end) and the deviation
//!    hysteresis (C2: 40 m for 15 s, back under 15 m for 5 s);
//! 3. fires the decision points whose trigger the cursor passed (turns that are not
//!    obvious, zones without network, avalanche paths);
//! 4. decides the GPS mode ([`crate::gps::decide`]) from the distance to the next decision
//!    point;
//! 5. asks the speech policy ([`crate::speech`]) whether something is due;
//! 6. while moving, adds the time and the model time progressed per terrain to the pace of
//!    the day (C11, [`DayPace`]); stops are counted when the skier moves again.
//!
//! No allocation on that path (tested by counting the allocator). Once a minute: the
//! condition alerts and turnaround of A8 (`alerts::Monitor`: hourly weather, minutes of
//! delay; with a group on the plan, the turnaround at the group's pace,
//! `alerts::Monitor::for_group`) and the limit of the plan (return time asked or C7 deadline: a late start or a
//! slow pace that misses it shortens the loop; with a group on the plan, its median arrival
//! at the pace of the day) and the proposals of C11 when the group is durably later than
//! planned ([`crate::pace`]: never imposed, answered by [`Input::Answer`]). On events only
//! (allocating): off the route for [`TrackerParams::reroute_after_s`] → a new route on the
//! trail graph to the junction ahead that costs the least extra time (or the finish), with
//! the announcements of the plan's routes ([`plan::announcements`]: with the layers of the
//! day, [`Tracker::with_layers`], zones without network, avalanche paths, protected zones;
//! X11); a
//! closed trail → the plan B of the morning that goes around it, else a route computed
//! then, else « no other way known »; fog or rising wind → the exit saving the most time;
//! late → the longest exit that still ends in time (shortened loop); no time left → the
//! fastest way home (safety).
//! A refusal (one gesture) or skiing on along the previous route past the divergence puts
//! the previous route back; the same cause does not switch again.

use std::borrow::Cow;
use std::collections::BTreeSet;

use chrono::{DateTime, NaiveDateTime, Utc};
use serde::{Deserialize, Serialize};
use skimap_assist::alerts;
use skimap_assist::inputs::SiteData;
use skimap_assist::layers::WeatherPoint;
use skimap_assist::suggest::{loop_voice, node_snap};
use skimap_common::geom::LonLat;
use skimap_core::calibration::{
    self as calibration, DayPace, GroupProfile, OutingObs, PaceParams, Prediction, ProfileModel,
    TERRAINS, Terrain, terrain_ratio,
};
use skimap_core::graph::{LocalPlane, PisteGraph, Snap};
use skimap_core::matching::{AlertEvent, DeviationDetector, Hysteresis, ROUTE_HYSTERESIS};
use skimap_core::turnaround::TurnaroundParams;
use skimap_voice::Event;
use skimap_voice::bank::{Announcement, Side};
use skimap_voice::clips::{Direction, Fixed};

use crate::course::{Course, PlanLeg};
use crate::error::{Error, Result};
use crate::fallback::{self, FallbackKind};
use crate::gps::{self, GpsInput, GpsParams, GpsRequest, GpsState};
use crate::guidance::{self, Junction};
use crate::line::turn_deg;
use crate::pace::{
    self, PaceReason, PaceRoute, ProfileFitParams, ProfileProposal, Proposal, ProposalKind,
    ProposeParams, proposal_announcement, proposal_text_in,
};
use crate::phrases::{self, Voice};
use crate::plan::{self, DayPlan, PlannedAnnouncement, Trigger};
use crate::session::{Activity, End, Phase, Session, SessionEvent, SessionParams};
use crate::speech::{Candidate, Priority, Speech, SpeechParams, Spoken};
use crate::track::{RouteTable, RouteView, TrackView};

/// Weather alerts of A8, serialisable.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Condition {
    /// Wind rising.
    WindRising,
    /// Fog coming.
    Fog,
    /// Snow starting.
    Snow,
    /// Temperature dropping.
    TemperatureDrop,
}

impl Condition {
    fn of(k: alerts::Kind) -> Option<Self> {
        Some(match k {
            alerts::Kind::WindRising => Condition::WindRising,
            alerts::Kind::Fog => Condition::Fog,
            alerts::Kind::Snow => Condition::Snow,
            alerts::Kind::TemperatureDrop => Condition::TemperatureDrop,
            _ => return None,
        })
    }
}

/// Settings.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TrackerParams {
    /// Speech policy.
    pub speech: SpeechParams,
    /// Start, pause, end.
    pub session: SessionParams,
    /// GPS modes.
    pub gps: GpsParams,
    /// Off the route: alert above this, metres…
    pub enter_m: f64,
    /// …for this long, seconds.
    pub enter_s: f64,
    /// Back under this, metres…
    pub exit_m: f64,
    /// …for this long, seconds.
    pub exit_s: f64,
    /// A fix farther than this from the route does not move the cursor, metres.
    pub on_route_m: f64,
    /// Off the route and near a trail this long: new route, seconds.
    pub reroute_after_s: f64,
    /// Conditions that switch to the exit.
    pub shortcut_on: Vec<Condition>,
    /// Model time an exit must save to be taken for the conditions or the return time,
    /// seconds.
    pub min_saving_s: f64,
    /// A switch is accepted once the skier is this far past the divergence on it, metres.
    pub accept_m: f64,
    /// Skiing on the previous route this far past the divergence refuses the switch,
    /// metres.
    pub refuse_m: f64,
    /// A8 and the return time are checked this often, seconds.
    pub slow_every_s: f64,
    /// Proposals when the group is durably slower than planned (C11).
    #[serde(default)]
    pub propose: ProposeParams,
    /// Proposal to choose another group profile (X14).
    #[serde(default)]
    pub profile: ProfileFitParams,
}

impl Default for TrackerParams {
    fn default() -> Self {
        let h: Hysteresis = ROUTE_HYSTERESIS;
        Self {
            speech: SpeechParams::default(),
            session: SessionParams::default(),
            gps: GpsParams::default(),
            enter_m: h.enter_m,
            enter_s: h.enter_s,
            exit_m: h.exit_m,
            exit_s: h.exit_s,
            on_route_m: 40.0,
            reroute_after_s: 60.0,
            shortcut_on: vec![Condition::Fog, Condition::WindRising],
            min_saving_s: 600.0,
            accept_m: 150.0,
            refuse_m: 150.0,
            slow_every_s: 60.0,
            propose: ProposeParams::default(),
            profile: ProfileFitParams::default(),
        }
    }
}

/// An input.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Input {
    /// A GPS fix.
    Fix {
        /// Time.
        t: DateTime<Utc>,
        /// Position.
        pos: LonLat,
    },
    /// Activity recognised by the phone, or the significant motion sensor.
    Activity {
        /// Time.
        t: DateTime<Utc>,
        /// Activity.
        activity: Activity,
    },
    /// Screen on (interactive) or off.
    Screen {
        /// Time.
        t: DateTime<Utc>,
        /// On.
        on: bool,
    },
    /// Trails closed (OSM ways), from the status sources or a report.
    Closed {
        /// Time.
        t: DateTime<Utc>,
        /// OSM ways.
        ways: Vec<i64>,
    },
    /// The skier refuses the last switch (one gesture).
    Refuse {
        /// Time.
        t: DateTime<Utc>,
    },
    /// The skier answers the last proposal (C11): accept or refuse, one gesture.
    Answer {
        /// Time.
        t: DateTime<Utc>,
        /// Accepted.
        accept: bool,
    },
    /// Steps counted by the phone since the previous report (Android's step counter or step
    /// detector, delivered with the fixes in time order): a skier striding climbs, a skier
    /// stopped does not (X10).
    Steps {
        /// Time.
        t: DateTime<Utc>,
        /// Steps since the previous report.
        steps: u32,
    },
    /// The skier answers the proposal to choose another group profile (X14): one gesture.
    ProfileAnswer {
        /// Time.
        t: DateTime<Utc>,
        /// Accepted.
        accept: bool,
    },
}

impl Input {
    /// Time of the input.
    pub fn t(&self) -> DateTime<Utc> {
        match self {
            Input::Fix { t, .. }
            | Input::Activity { t, .. }
            | Input::Screen { t, .. }
            | Input::Closed { t, .. }
            | Input::Refuse { t }
            | Input::Answer { t, .. }
            | Input::Steps { t, .. }
            | Input::ProfileAnswer { t, .. } => *t,
        }
    }
}

/// Which route is followed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RouteRef {
    /// The validated loop.
    Plan,
    /// A plan B of the morning.
    PlanB(usize),
    /// A route computed during the outing (n-th).
    Live(usize),
}

/// Why the route changed.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Cause {
    /// Closed trails.
    Closure {
        /// OSM ways.
        ways: Vec<i64>,
    },
    /// Late for a limit (C7 deadline or return time asked).
    Late {
        /// The limit.
        limit: NaiveDateTime,
    },
    /// Conditions turning (A8).
    Conditions(Condition),
    /// Off the route for a while.
    Deviation,
    /// No time left before the deadline.
    NoTime,
    /// A proposal accepted (C11: slower than planned).
    Pace,
}

impl Cause {
    fn same(&self, other: &Cause) -> bool {
        match (self, other) {
            (Cause::Closure { ways: a }, Cause::Closure { ways: b }) => {
                a.iter().any(|w| b.contains(w))
            }
            (Cause::Late { .. }, Cause::Late { .. }) => true,
            (a, b) => a == b,
        }
    }
}

/// An alert raised (whether said or not, see [`Output::Spoken`]).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Alert {
    /// Off the route (C2).
    OffRoute,
    /// Back on the route.
    BackOnRoute,
    /// Weather (A8).
    Condition(Condition),
    /// Turnaround moved earlier (A8), or late for the return time asked.
    TurnaroundAdvanced,
    /// Turn around now (A8).
    TurnaroundNow,
    /// A closed trail on the route.
    Closed,
    /// A closed trail on the route and no way around it.
    ClosedNoDetour,
}

/// Recap of the outing (J3; the spoken and compared recap is A10).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Recap {
    /// How it ended.
    pub end: End,
    /// Start, local.
    pub started: NaiveDateTime,
    /// End, local.
    pub finished: NaiveDateTime,
    /// Time moving, minutes.
    pub moving_min: f64,
    /// Time stopped, minutes.
    pub paused_min: f64,
    /// Distance skied (coarse smoothed track of the session), km.
    pub distance_km: f64,
    /// Route followed at the end.
    pub route: RouteRef,
    /// Its length, km.
    pub route_km: f64,
    /// Share of it done, from the cursor.
    pub done_share: f64,
    /// Model time of the part done, minutes.
    pub model_min: f64,
    /// Elapsed over model time of the part done.
    pub pace: Option<f64>,
    /// Switches of route.
    pub switches: usize,
    /// Groups spoken.
    pub spoken: usize,
    /// Refuges where the skier stopped.
    pub refuges: Vec<String>,
    /// Text, French.
    pub text: String,
}

/// An output.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Output {
    /// Something said.
    Spoken {
        /// Time.
        t: DateTime<Utc>,
        /// What.
        spoken: Spoken,
    },
    /// A decision on the outing.
    Session {
        /// Time of the decision (the event carries the time it refers to).
        t: DateTime<Utc>,
        /// Decision.
        event: SessionEvent,
    },
    /// An alert raised.
    Alert {
        /// Time.
        t: DateTime<Utc>,
        /// Alert.
        alert: Alert,
    },
    /// Switch to another route.
    Switched {
        /// Time.
        t: DateTime<Utc>,
        /// Previous route.
        from: RouteRef,
        /// New route.
        to: RouteRef,
        /// Why.
        cause: Cause,
        /// Where it leaves the previous route, metres along it.
        s_div: f64,
    },
    /// Previous route put back.
    Reverted {
        /// Time.
        t: DateTime<Utc>,
        /// Route followed again.
        to: RouteRef,
        /// Cause of the switch refused.
        cause: Cause,
        /// Refused by skiing on (true) or by a gesture.
        implicit: bool,
    },
    /// New GPS request for the client.
    Gps {
        /// Time.
        t: DateTime<Utc>,
        /// Request.
        request: GpsRequest,
    },
    /// End of the outing.
    Recap {
        /// Time.
        t: DateTime<Utc>,
        /// Recap.
        recap: Recap,
    },
    /// A proposal (C11), to accept or refuse ([`Input::Answer`]).
    Proposal {
        /// Time.
        t: DateTime<Utc>,
        /// Proposal.
        proposal: Box<Proposal>,
    },
    /// The answer to a proposal.
    Answered {
        /// Time.
        t: DateTime<Utc>,
        /// What was proposed.
        kind: ProposalKind,
        /// Accepted.
        accept: bool,
        /// Applied (an accepted exit already passed is not).
        applied: bool,
    },
    /// The pace of the day fits another group profile better (X14): choose it? Answered
    /// by [`Input::ProfileAnswer`].
    ProfileProposal {
        /// Time.
        t: DateTime<Utc>,
        /// Proposal.
        proposal: Box<ProfileProposal>,
    },
    /// The answer to the profile proposal: accepted, the tracker plans on that profile
    /// from now on (arrival, proposals).
    ProfileAnswered {
        /// Time.
        t: DateTime<Utc>,
        /// Accepted.
        accept: bool,
        /// The profile chosen (accepted).
        profile: Option<GroupProfile>,
    },
}

/// The estimated arrival at the pace of the day (C11), from the last fix.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Eta {
    /// Time of the estimate (local).
    pub at: NaiveDateTime,
    /// Arrival, median (local).
    pub arrival: NaiveDateTime,
    /// 10 % quantile.
    pub arrival_lo: NaiveDateTime,
    /// 90 % quantile.
    pub arrival_hi: NaiveDateTime,
    /// Arrival planned for the group.
    pub planned: NaiveDateTime,
    /// What remains.
    pub remaining: Prediction,
    /// Progress pace of the day on the flat, relative to C5.
    pub pace: f64,
    /// Stops so far and their total, seconds.
    pub stops: (u32, f64),
}

impl Output {
    /// Time.
    pub fn t(&self) -> DateTime<Utc> {
        match self {
            Output::Spoken { t, .. }
            | Output::Session { t, .. }
            | Output::Alert { t, .. }
            | Output::Switched { t, .. }
            | Output::Reverted { t, .. }
            | Output::Gps { t, .. }
            | Output::Recap { t, .. }
            | Output::Proposal { t, .. }
            | Output::Answered { t, .. }
            | Output::ProfileProposal { t, .. }
            | Output::ProfileAnswered { t, .. } => *t,
        }
    }
}

/// Where the geometry of a route comes from. Two at most live (the route followed and the
/// one a switch can go back to): the view of the table is kept inline, read on every fix.
#[derive(Debug, Clone)]
#[allow(clippy::large_enum_variant)]
enum Geo<'a> {
    /// The plan's table.
    Table(RouteView<'a>),
    /// Computed on the phone (flat block owned here).
    Owned(Vec<u8>),
}

#[derive(Debug, Clone)]
struct Active<'a> {
    rf: RouteRef,
    geo: Geo<'a>,
    legs: Vec<PlanLeg>,
    /// Announcements of a route computed on the phone (its decisions' `src` is MAX).
    own: Vec<PlannedAnnouncement>,
    junctions: Vec<Junction>,
    next: usize,
}

impl<'a> Active<'a> {
    #[inline]
    fn view(&self) -> RouteView<'_> {
        match &self.geo {
            Geo::Table(v) => *v,
            Geo::Owned(b) => RouteView::trusted(b).expect("block built here"),
        }
    }
}

#[derive(Debug, Clone)]
struct Pending<'a> {
    prev: Active<'a>,
    prev_cursor: f64,
    cause: Cause,
    s_div: f64,
}

/// The arrival planned for the group, from the start.
#[derive(Debug, Clone, Copy)]
struct Reference {
    t0: f64,
    total: Prediction,
    /// Planned arrival, seconds (moved by an accepted proposal): the time planned on
    /// (the median; the slow side of the range with children).
    arrival: f64,
    /// Median arrival, seconds (the delays of the proposals are on the medians).
    median: f64,
}

#[derive(Debug, Clone)]
struct PendingProposal {
    t: f64,
    kind: ProposalKind,
    exit: Option<usize>,
    arrival: f64,
    median: f64,
    delay: f64,
}

#[derive(Debug, Clone, Default)]
struct Proposals {
    /// Every 5 min of moving time: (moving time, progress), a ring.
    marks: [(f64, f64); 6],
    mark_n: usize,
    since: Option<f64>,
    last_t: Option<f64>,
    count: u32,
    /// Delay of the last proposal not accepted.
    last_delay: Option<f64>,
    pending: Option<PendingProposal>,
}

fn secs_of(t: DateTime<Utc>) -> f64 {
    t.timestamp_millis() as f64 / 1000.0
}

fn utc_of(t: f64) -> DateTime<Utc> {
    DateTime::<Utc>::from_timestamp_millis((t * 1000.0).round() as i64).expect("time in range")
}

fn local(t: f64) -> NaiveDateTime {
    skimap_core::ephemeris::local_time(utc_of(t))
}

/// The tracker of one outing.
pub struct Tracker<'a> {
    plan: &'a DayPlan,
    track: TrackView<'a>,
    g: &'a PisteGraph,
    voice: Option<&'a Voice>,
    p: TrackerParams,
    plane: LocalPlane,
    active: Active<'a>,
    pending: Option<Pending<'a>>,
    cursor: f64,
    /// Time the cursor last moved, seconds.
    cursor_t: f64,
    d_route: f64,
    /// Distance to the route smoothed over about 10 s (GPS mode).
    d_smooth: f64,
    detector: DeviationDetector,
    deviating_since: Option<f64>,
    rerouted_in_deviation: bool,
    monitor: Option<alerts::Monitor<'a>>,
    session: Session,
    speech: Speech,
    gps: GpsState,
    last_request: Option<GpsRequest>,
    screen_on: bool,
    closed: BTreeSet<i64>,
    refused: Vec<Cause>,
    last_fix: Option<(f64, LonLat)>,
    last_slow: f64,
    last_late: f64,
    live: usize,
    switches: usize,
    spoken: usize,
    refuges: Vec<String>,
    live_key: Option<(String, f64)>,
    session_events: Vec<SessionEvent>,
    /// C11: the group's parameters, the terrain ratio of the model, the pace of the day.
    pace_base: PaceParams,
    ratio: f64,
    /// Quantile of the total time planned on (0.5; the slow side with children).
    plan_q: f64,
    /// The slow side planned on over the median, in the morning (1 without children).
    slow_ratio: f64,
    day: DayPace,
    pace_route: PaceRoute,
    pace_last: Option<(f64, f64)>,
    progress_total: f64,
    stops_s: Vec<f64>,
    reference: Option<Reference>,
    props: Proposals,
    /// A switch other than a reroute happened (no more proposals).
    switched_other: bool,
    /// Layers of the day for the routes computed during the outing (X11), and the
    /// protected zones of the loop (said in the morning, A7), once needed.
    layers: Option<&'a SiteData>,
    loop_zones: Option<BTreeSet<String>>,
    /// When the terrain at the cursor was last given to the session, seconds.
    climb_t: f64,
    /// X14: model time progressed per terrain while moving, the profiles the day is
    /// compared with, the proposal waiting (time, index), asked once, the profile chosen.
    model_done: [f64; TERRAINS],
    profiles: Vec<ProfileModel>,
    /// A lone adult of the plan's technique: the profile a plan without one is compared to.
    adult: PaceParams,
    profile_pending: Option<(f64, usize)>,
    profile_asked: bool,
    profile_chosen: Option<GroupProfile>,
}

impl<'a> Tracker<'a> {
    /// Tracker of a plan, its flat table (`plan_<…>.trk`, [`plan::track`]) and its graph;
    /// `voice` (the pack's clips) names the pistes of the routes computed during the
    /// outing as the pack does.
    pub fn new(
        plan: &'a DayPlan,
        track: TrackView<'a>,
        g: &'a PisteGraph,
        voice: Option<&'a Voice>,
        p: TrackerParams,
    ) -> Result<Self> {
        plan.check_graph(g)?;
        let view = track
            .find(0)
            .ok_or_else(|| Error::Invalid("track without the loop".into()))?;
        let course = Course::new(g, &plan.route.legs, &plan.eta)?;
        let start = course.line().point_at(0.0);
        if start.haversine_m(track.origin) > 1.0 {
            return Err(Error::Invalid("track of another plan".into()));
        }
        let finish = course.line().point_at(course.length_m());
        let ratio = terrain_ratio(&plan.eta);
        let pace_route = PaceRoute::of(&course, ratio);
        let pace_base = pace::base_params(plan);
        let plan_q = plan.pace.as_ref().map_or(0.5, |p| p.plan_q);
        let slow_ratio = match &plan.pace {
            Some(p) if p.plan_q > 0.5 && p.prediction.median_s > 0.0 => {
                p.prediction.quantile_s(p.plan_q) / p.prediction.median_s
            }
            _ => 1.0,
        };
        // With children, proposals sooner (C11).
        let mut p = p;
        if plan.pace.as_ref().is_some_and(|x| x.profile.has_children()) {
            p.propose = p.propose.with_children();
        }
        let day = DayPace::new(&pace_base, pace_route.total());
        let refuges = plan
            .refuges
            .iter()
            .map(|r| (r.name.clone(), r.pos))
            .collect();
        let hyst = Hysteresis {
            enter_m: p.enter_m,
            enter_s: p.enter_s,
            exit_m: p.exit_m,
            exit_s: p.exit_s,
        };
        Ok(Self {
            plan,
            track,
            g,
            voice,
            plane: LocalPlane::new(track.origin),
            active: Active {
                rf: RouteRef::Plan,
                geo: Geo::Table(view),
                legs: plan.route.legs.clone(),
                own: Vec::new(),
                junctions: plan.junctions.clone(),
                next: 0,
            },
            pending: None,
            cursor: 0.0,
            cursor_t: f64::NEG_INFINITY,
            d_route: f64::INFINITY,
            d_smooth: 0.0,
            detector: DeviationDetector::new(hyst),
            deviating_since: None,
            rerouted_in_deviation: false,
            monitor: None,
            session: Session::new(p.session, start, finish, refuges),
            speech: Speech::new(p.speech),
            gps: GpsState::default(),
            last_request: None,
            screen_on: false,
            closed: BTreeSet::new(),
            refused: Vec::new(),
            last_fix: None,
            last_slow: f64::NEG_INFINITY,
            last_late: f64::NEG_INFINITY,
            live: 0,
            switches: 0,
            spoken: 0,
            refuges: Vec::new(),
            live_key: None,
            session_events: Vec::with_capacity(4),
            pace_base,
            ratio,
            plan_q,
            slow_ratio,
            day,
            pace_route,
            pace_last: None,
            progress_total: 0.0,
            stops_s: Vec::with_capacity(64),
            reference: None,
            props: Proposals::default(),
            switched_other: false,
            layers: None,
            loop_zones: None,
            climb_t: f64::NEG_INFINITY,
            model_done: [0.0; TERRAINS],
            profiles: pace::profile_presets(plan.request.style),
            adult: PaceParams::prior(&GroupProfile::solo(
                plan.request.style,
                skimap_core::calibration::Level::Intermediate,
            )),
            profile_pending: None,
            profile_asked: false,
            profile_chosen: None,
            p,
        })
    }

    /// The layers of the day (the pack's: zones without network, historic avalanche paths,
    /// BERA, zones and rules): a route computed during the outing announces them as the
    /// plan's routes do ([`plan::announcements`], X11), and the protected zones the loop
    /// does not cross.
    pub fn with_layers(mut self, data: &'a SiteData) -> Self {
        self.layers = Some(data);
        self
    }

    /// The group profiles the pace of the day is compared with (X14): the app's (its saved
    /// groups), instead of [`pace::profile_presets`].
    pub fn with_profiles(mut self, models: Vec<ProfileModel>) -> Self {
        self.profiles = models;
        self
    }

    /// The group profiles the pace of the day is compared with.
    pub fn profiles(&self) -> &[ProfileModel] {
        &self.profiles
    }

    /// The profile chosen during the outing (X14), if any.
    pub fn profile_chosen(&self) -> Option<&GroupProfile> {
        self.profile_chosen.as_ref()
    }

    /// Route followed.
    pub fn route(&self) -> RouteRef {
        self.active.rf
    }

    /// Legs of the route followed.
    pub fn legs(&self) -> &[PlanLeg] {
        &self.active.legs
    }

    /// Cursor along the route followed, metres.
    pub fn progress(&self) -> f64 {
        self.cursor
    }

    /// Distance from the last fix to the route followed, metres.
    pub fn distance_to_route(&self) -> f64 {
        self.d_route
    }

    /// Distance along the route to the next decision point, metres (`None`: none left):
    /// the client may show it; the GPS mode is already decided from it.
    pub fn next_decision_m(&self) -> Option<f64> {
        let v = self.active.view();
        (self.active.next.saturating_sub(2)..v.decisions())
            .map(|i| f64::from(v.decision(i).s_point))
            .find(|s| *s > self.cursor)
            .map(|s| s - self.cursor)
    }

    /// Phase of the outing.
    pub fn phase(&self) -> &Phase {
        self.session.phase()
    }

    /// The speech policy (dropped candidates).
    pub fn speech(&self) -> &Speech {
        &self.speech
    }

    /// GPS request in force.
    pub fn gps_request(&self) -> Option<GpsRequest> {
        self.last_request
    }

    fn hysteresis(&self) -> Hysteresis {
        Hysteresis {
            enter_m: self.p.enter_m,
            enter_s: self.p.enter_s,
            exit_m: self.p.exit_m,
            exit_s: self.p.exit_s,
        }
    }

    fn offer(
        &mut self,
        t: f64,
        priority: Priority,
        key: &str,
        kind: &str,
        a: Announcement,
        ttl: Option<f64>,
    ) {
        self.speech.offer(Candidate {
            t,
            priority,
            key: key.into(),
            kind: kind.into(),
            announcement: a,
            expires: ttl.map(|x| t + x),
        });
    }

    /// Feeds an input; appends what happened to `out`, in order (nothing appended and
    /// nothing allocated for a fix where all goes well).
    pub fn push(&mut self, input: &Input, out: &mut Vec<Output>) {
        let t = secs_of(input.t());
        match input {
            Input::Fix { pos, .. } => self.fix(t, *pos, out),
            Input::Activity { activity, .. } => {
                let mut ev = std::mem::take(&mut self.session_events);
                self.session.activity(t, *activity, &mut ev);
                self.session_events(t, &mut ev, out);
                self.session_events = ev;
                self.gps_mode(t, out);
            }
            Input::Screen { on, .. } => {
                self.screen_on = *on;
                self.gps_mode(t, out);
            }
            Input::Steps { steps, .. } => {
                let mut ev = std::mem::take(&mut self.session_events);
                self.session.steps(t, *steps, &mut ev);
                self.session_events(t, &mut ev, out);
                self.session_events = ev;
                self.gps_mode(t, out);
            }
            Input::Closed { ways, .. } => self.closure(t, ways, out),
            Input::Refuse { .. } => self.revert(t, false, out),
            Input::Answer { accept, .. } => self.answer(t, *accept, out),
            Input::ProfileAnswer { accept, .. } => self.profile_answer(t, *accept, out),
        }
        if let Some(s) = self.speech.tick(t) {
            self.spoken += 1;
            out.push(Output::Spoken {
                t: utc_of(t),
                spoken: s,
            });
        }
    }

    /// Feeds a batch of inputs (the fixes of a GPS batch, up to 60 s), in one pass.
    pub fn push_batch(&mut self, inputs: &[Input], out: &mut Vec<Output>) {
        for i in inputs {
            self.push(i, out);
        }
    }

    fn session_events(&mut self, t: f64, ev: &mut Vec<SessionEvent>, out: &mut Vec<Output>) {
        for e in ev.drain(..) {
            out.push(Output::Session {
                t: utc_of(t),
                event: e.clone(),
            });
            match e {
                SessionEvent::Started { t: t0, .. } => {
                    let (model, length, profile) = {
                        let v = self.active.view();
                        (
                            f64::from(v.model_s_at(v.length())),
                            f64::from(v.length()),
                            v.to_profile(),
                        )
                    };
                    let start = local(t0);
                    let total = self.pace_base.predict(self.pace_route.total());
                    self.reference = Some(Reference {
                        t0,
                        total,
                        arrival: t0 + self.planned_s(&total),
                        median: t0 + total.median_s,
                    });
                    self.pace_last = None;
                    // The group's time when the plan has one, else the model's with the
                    // margin of C7.
                    let (said, back_s) = match &self.plan.pace {
                        Some(_) => {
                            let s = self.planned_s(&total);
                            (s, s)
                        }
                        None => (model, model * TurnaroundParams::default().eta_factor),
                    };
                    let back = start + chrono::Duration::seconds(back_s as i64);
                    let ev = loop_voice(length / 1000.0, said, back);
                    self.offer(
                        t,
                        Priority::Comfort,
                        "departure",
                        "loop_summary",
                        ev.announce(),
                        None,
                    );
                    let p = alerts::Params::default();
                    let point: Cow<'a, WeatherPoint> = match &self.plan.weather {
                        Some(w) => Cow::Borrowed(w),
                        // No weather: the C7 deadline at the start still holds.
                        None => Cow::Owned(WeatherPoint {
                            id: "none".into(),
                            name: "sans météo".into(),
                            position: self.plan.request.start,
                            elevation_m: 0.0,
                            hours: Vec::new(),
                        }),
                    };
                    self.monitor = Some(
                        match &self.plan.pace {
                            // The group of the plan (C11): the turnaround at the time it is
                            // planned on, with its light margin.
                            Some(pace) => alerts::Monitor::for_model(
                                start,
                                &profile,
                                point,
                                p,
                                &pace::plan_model(pace),
                                self.ratio,
                            ),
                            None => alerts::Monitor::with(start, Cow::Owned(profile), point, p),
                        }
                        .in_lang(self.plan.lang),
                    );
                }
                SessionEvent::Paused { t: tp } => {
                    self.speech.set_paused(true);
                    // Its beginning was counted as moving.
                    self.day.unmove(t - tp);
                    self.pace_last = None;
                }
                SessionEvent::Refuge { name, .. } => {
                    if !self.refuges.contains(&name) {
                        self.refuges.push(name);
                    }
                }
                SessionEvent::Resumed { paused_s, .. } => {
                    self.speech.set_paused(false);
                    self.day.stop(paused_s);
                    self.stops_s.push(paused_s);
                    self.pace_last = None;
                }
                SessionEvent::NearFinish { .. } => {
                    if let Some(a) = self
                        .plan
                        .announcements
                        .iter()
                        .find(|a| a.trigger == Trigger::Arrival)
                    {
                        let a = a.announcement.clone();
                        self.offer(t, Priority::Direction, "arrival", "arrive", a, None);
                    }
                }
                SessionEvent::Finished { t: t1, end } => {
                    self.speech.set_paused(false);
                    self.offer(
                        t,
                        Priority::Comfort,
                        "finished",
                        "finished",
                        phrases::announce(Fixed::PlanFinished),
                        None,
                    );
                    let recap = self.recap(t1, end);
                    out.push(Output::Recap {
                        t: utc_of(t),
                        recap,
                    });
                }
            }
        }
    }

    fn recap(&self, t1: f64, end: End) -> Recap {
        let tot = self.session.totals();
        let t0 = tot.started.unwrap_or(t1);
        let v = self.active.view();
        let model = f64::from(v.model_s_at(self.cursor as f32));
        let moving = (t1 - t0 - tot.paused_s).max(0.0);
        let pace = (model > 60.0).then(|| (t1 - t0) / model);
        let len = f64::from(v.length());
        let done = if len > 0.0 { self.cursor / len } else { 0.0 };
        let text = crate::text::of(self.plan.lang).recap(
            end,
            tot.distance_m / 1000.0,
            (moving / 60.0).round(),
            (tot.paused_s / 60.0).round(),
            (done * 100.0).round(),
            len / 1000.0,
            pace,
            self.switches,
            self.spoken,
            &self.refuges,
        );
        Recap {
            end,
            started: local(t0),
            finished: local(t1),
            moving_min: (moving / 60.0).round(),
            paused_min: (tot.paused_s / 60.0).round(),
            distance_km: (tot.distance_m / 10.0).round() / 100.0,
            route: self.active.rf,
            route_km: (len / 10.0).round() / 100.0,
            done_share: (done * 100.0).round() / 100.0,
            model_min: (model / 60.0).round(),
            pace: pace.map(|p| (p * 100.0).round() / 100.0),
            switches: self.switches,
            spoken: self.spoken,
            refuges: self.refuges.clone(),
            text,
        }
    }

    /// Projection on a route from a cursor: (distance along, distance to the route).
    #[inline]
    fn locate(
        v: &RouteView<'_>,
        q: [f32; 2],
        cursor: f64,
        dt: f64,
        on_route_m: f64,
    ) -> (Option<f64>, f64) {
        // At most what a skier can do since the last fix (v_max 7 m/s, with margin): an
        // out-and-back has the way back a few metres away, further along.
        let c = cursor as f32;
        let ahead = 30.0 + 10.0 * dt.clamp(0.0, 600.0) as f32;
        let win = v.project_forward(q, c - 50.0, c + ahead, 15.0);
        match win {
            Some((s, d)) if f64::from(d) <= on_route_m => (Some(f64::from(s)), f64::from(d)),
            _ => {
                // Off the window: the cells around the fix (a gap, a short cut, a deviation).
                let grid = v.project_grid(q, c);
                let d_win = win.map_or(f64::INFINITY, |(_, d)| f64::from(d));
                match grid {
                    Some((s, d))
                        if f64::from(d) <= on_route_m && s >= c - 50.0 && s <= c + ahead =>
                    {
                        (Some(f64::from(s)), f64::from(d))
                    }
                    Some((_, d)) => (None, d_win.min(f64::from(d))),
                    None => (None, d_win),
                }
            }
        }
    }

    fn fix(&mut self, t: f64, pos: LonLat, out: &mut Vec<Output>) {
        let xy = self.plane.xy(pos);
        let q = [xy[0] as f32, xy[1] as f32];
        let dt = self.last_fix.map_or(1.0, |(t0, _)| t - t0);
        self.last_fix = Some((t, pos));
        let running = self.session.running();
        let paused = self.session.paused();
        // 1. Projection and cursor.
        let (s, d) = {
            let v = self.active.view();
            if running {
                // The window grows with the time since the cursor last moved.
                let since = if self.cursor_t.is_finite() {
                    t - self.cursor_t
                } else {
                    dt
                };
                Self::locate(&v, q, self.cursor, since.max(dt), self.p.on_route_m)
            } else {
                match v.project_window(q, 0.0, 600.0) {
                    Some((s, d)) => (Some(f64::from(s)), f64::from(d)),
                    None => (None, f64::INFINITY),
                }
            }
        };
        self.d_route = d;
        if d.is_finite() {
            let k = 1.0 - (-dt.max(0.0) / 10.0).exp();
            self.d_smooth += (d.min(1000.0) - self.d_smooth) * k;
        }
        if running
            && !paused
            && let Some(s) = s
            && s > self.cursor
        {
            self.cursor = s;
            self.cursor_t = t;
        }

        if let Some(pd) = &mut self.pending
            && running
            && !paused
        {
            let v = pd.prev.view();
            if let (Some(s), _) = Self::locate(&v, q, pd.prev_cursor, dt, self.p.on_route_m) {
                pd.prev_cursor = pd.prev_cursor.max(s);
            }
        }
        // 2. Session (on a climb, a pause from the fixes alone needs a longer window: X10;
        // the terrain at the cursor every 10 s, from the plan's model times).
        if running && t - self.climb_t >= 10.0 {
            self.climb_t = t;
            let m = self
                .pace_route
                .between(self.cursor - 50.0, self.cursor + 50.0);
            let terrain = Terrain::ALL
                .into_iter()
                .filter(|t| m[*t as usize] > 0.0)
                .max_by(|a, b| m[*a as usize].total_cmp(&m[*b as usize]))
                .unwrap_or(Terrain::Flat);
            self.session.set_terrain(terrain);
        }
        let mut ev = std::mem::take(&mut self.session_events);
        self.session.fix_on(t, pos, Some(d), s, &mut ev);
        if !ev.is_empty() {
            if !running
                && let Some(s) = s
                && ev.iter().any(|e| matches!(e, SessionEvent::Started { .. }))
            {
                // The outing starts where the skier is now on the route.
                self.cursor = s;
                self.cursor_t = t;
            }
            self.session_events(t, &mut ev, out);
        }
        self.session_events = ev;
        if !self.session.running() {
            self.gps_mode(t, out);
            return;
        }
        let paused = self.session.paused();
        if !paused {
            self.pace_fix(t);
        }
        // Pending switch: accepted, or refused by skiing on.
        if let Some(pd) = &self.pending {
            let d_old = {
                let v = pd.prev.view();
                Self::locate(&v, q, pd.prev_cursor, dt, self.p.on_route_m).1
            };
            if self.cursor >= pd.s_div + self.p.accept_m && d < 30.0 {
                self.pending = None;
            } else if pd.prev_cursor >= pd.s_div + self.p.refuse_m && d > 60.0 && d_old < 30.0 {
                self.revert(t, true, out);
            }
        }
        // 3. Deviation.
        if !paused {
            match self.detector.push(t, self.d_route) {
                Some(AlertEvent::Raised(_)) => {
                    self.deviating_since = Some(t);
                    self.rerouted_in_deviation = false;
                    out.push(Output::Alert {
                        t: utc_of(t),
                        alert: Alert::OffRoute,
                    });
                    let ev = self.off_route_event(q);
                    self.offer(
                        t,
                        Priority::Direction,
                        "off_route",
                        "off_route",
                        ev.announce(),
                        Some(60.0),
                    );
                }
                Some(AlertEvent::Cleared(_)) => {
                    self.deviating_since = None;
                    out.push(Output::Alert {
                        t: utc_of(t),
                        alert: Alert::BackOnRoute,
                    });
                    let piste = self.piste_at(self.cursor);
                    self.offer(
                        t,
                        Priority::Direction,
                        "off_route",
                        "back_on_route",
                        Event::BackOnRoute { piste }.announce(),
                        Some(60.0),
                    );
                }
                None => {}
            }
            if let Some(since) = self.deviating_since
                && !self.rerouted_in_deviation
                && t - since >= self.p.reroute_after_s
                && let Some(snap) = self.g.snap(pos, 40.0)
            {
                self.rerouted_in_deviation = true;
                self.reroute(t, snap, out);
            }
        }
        // 4. Decision points.
        if !paused {
            self.decisions(t);
        }
        // 5. A8 and the return time, once a minute.
        if t - self.last_slow >= self.p.slow_every_s {
            self.last_slow = t;
            let along = {
                let v = self.active.view();
                self.cursor * f64::from(v.scale())
            };
            let found = match &mut self.monitor {
                Some(m) => m.update(local(t), along),
                None => Vec::new(),
            };
            for a in found {
                self.a8(t, a, out);
            }
            self.unknown_stops();
            self.return_time(t, out);
            self.pace_check(t, out);
            self.profile_check(t, out);
        }
        // 6. GPS mode.
        self.gps_mode(t, out);
    }

    fn decisions(&mut self, t: f64) {
        let deviating = self.deviating_since.is_some();
        loop {
            let (d, n) = {
                let v = self.active.view();
                if self.active.next >= v.decisions() {
                    break;
                }
                (v.decision(self.active.next), v.decisions())
            };
            let _ = n;
            if f64::from(d.s_trigger) > self.cursor {
                break;
            }
            self.active.next += 1;
            if f64::from(d.s_until) < self.cursor || (deviating && d.priority != Priority::Safety) {
                continue;
            }
            let a = match d.src {
                0 => self.plan.announcements.get(d.ann as usize),
                u32::MAX => self.active.own.get(d.ann as usize),
                k => self
                    .plan
                    .plans_b
                    .get(k as usize - 1)
                    .and_then(|b| b.announcements.get(d.ann as usize)),
            };
            if let Some(a) = a {
                let (key, kind, ann, pr) = (
                    a.key.clone(),
                    a.kind.clone(),
                    a.announcement.clone(),
                    a.priority,
                );
                // Due before the point is passed: at the current speed, a short window (a
                // turn « dans vingt mètres » downhill) is said at once rather than after the
                // grouping wait of the speech policy (it would be withdrawn unsaid).
                let ttl = match self.session.speed(30.0) {
                    Some(v) if v > 0.5 && pr != Priority::Safety => {
                        ((f64::from(d.s_until) - self.cursor) / v).clamp(2.0, 120.0)
                    }
                    _ => 120.0,
                };
                self.offer(t, pr, &key, &kind, ann, Some(ttl));
                if pr != Priority::Safety {
                    // Withdrawn if not said before the point is passed.
                    self.live_key = Some((key, f64::from(d.s_until)));
                }
            }
        }
        if self
            .live_key
            .as_ref()
            .is_some_and(|(_, until)| self.cursor > *until)
            && let Some((k, _)) = self.live_key.take()
        {
            self.speech.withdraw(&k);
        }
    }

    fn gps_mode(&mut self, t: f64, out: &mut Vec<Output>) {
        let (d_next, since_point) = {
            let v = self.active.view();
            let mut next = None;
            let mut last = None;
            for i in self.active.next.saturating_sub(2)..v.decisions() {
                let d = v.decision(i);
                let sp = f64::from(d.s_point);
                if sp > self.cursor {
                    next = Some(sp - self.cursor);
                    break;
                }
                last = Some(self.cursor - sp);
            }
            (next, last)
        };
        let input = GpsInput {
            t,
            running: self.session.running(),
            d_next,
            since_point,
            speed_ms: self.session.speed(30.0).unwrap_or(0.0),
            d_route: self.d_smooth,
            paused: self.session.paused(),
            refuge: self.session.at_refuge(),
            waking: self.session.waking(),
            screen_on: self.screen_on,
        };
        let (state, req) = gps::decide(&self.gps, &input, &self.p.gps);
        self.gps = state;
        let changed = self
            .last_request
            .is_none_or(|r| r.mode != req.mode || (r.max_delay_s - req.max_delay_s).abs() >= 10.0);
        if changed {
            self.last_request = Some(req);
            out.push(Output::Gps {
                t: utc_of(t),
                request: req,
            });
        }
    }

    fn piste_at(&self, s: f64) -> Option<skimap_voice::names::PisteName> {
        let course = Course::new(self.g, &self.active.legs, &self.plan.eta).ok()?;
        let attrs = &self.g.edge(course.legs()[course.leg_at(s)].edge).attrs;
        match self.voice {
            Some(v) => v.piste(attrs),
            None => attrs.name.as_ref().map(|n| {
                skimap_voice::names::PisteName::new(
                    n.split(" / ").next().unwrap_or(n).trim(),
                    crate::phrases::colour(attrs.difficulty),
                )
            }),
        }
    }

    fn off_route_event(&self, q: [f32; 2]) -> Event {
        let v = self.active.view();
        let c = self.cursor as f32;
        let target = v
            .project_window(q, c - 200.0, c + 500.0)
            .map_or(c, |(s, _)| s);
        let tp = v.point_at(target);
        let heading = self.session.heading(30.0).or_else(|| {
            let (a, b) = (v.point_at(c), v.point_at(c + 40.0));
            let (dx, dy) = (f64::from(b[0] - a[0]), f64::from(b[1] - a[1]));
            (dx.hypot(dy) >= 1.0).then(|| dx.atan2(dy).to_degrees().rem_euclid(360.0))
        });
        let (dx, dy) = (f64::from(tp[0] - q[0]), f64::from(tp[1] - q[1]));
        let to_route = dx.atan2(dy).to_degrees().rem_euclid(360.0);
        let side = match heading {
            Some(h) => {
                let rb = turn_deg(h, to_route);
                if rb.abs() <= 45.0 {
                    Side::Ahead
                } else if rb.abs() >= 135.0 {
                    Side::Behind
                } else if rb > 0.0 {
                    Side::Right
                } else {
                    Side::Left
                }
            }
            None => Side::Behind,
        };
        Event::OffRoute {
            side,
            metres: self.d_route,
        }
    }

    fn snap_here(&self) -> Option<Snap> {
        let (_, p) = self.last_fix?;
        self.g.snap(p, 300.0)
    }

    fn course(&self) -> Option<Course> {
        Course::new(self.g, &self.active.legs, &self.plan.eta).ok()
    }

    /// Elapsed over model time so far (A8's pace), when meaningful.
    fn pace(&self, t: f64) -> Option<f64> {
        let t0 = self.session.totals().started?;
        let model = f64::from(self.active.view().model_s_at(self.cursor as f32));
        (model >= 1200.0).then(|| (t - t0) / model)
    }

    fn model_here(&self) -> f64 {
        f64::from(self.active.view().model_s_at(self.cursor as f32))
    }

    fn model_total(&self) -> f64 {
        let v = self.active.view();
        f64::from(v.model_s_at(v.length()))
    }

    fn full(&self, b: &plan::PlanB) -> Vec<PlanLeg> {
        b.fallback.full_legs(&self.plan.route.legs)
    }

    /// The exits of the morning still ahead on the route followed.
    fn exits_ahead(&self) -> Vec<usize> {
        let Some(c) = self.course() else {
            return Vec::new();
        };
        self.plan
            .plans_b
            .iter()
            .enumerate()
            .filter(|(_, b)| b.fallback.kind == FallbackKind::Exit && b.fallback.exists())
            .filter(|(_, b)| b.fallback.s_div >= self.cursor + 20.0)
            .filter(|(_, b)| self.shares_prefix(&c, &self.full(b), b.fallback.s_div))
            .filter(|(_, b)| !self.uses_closed(&self.full(b), b.fallback.s_div))
            .map(|(i, _)| i)
            .collect()
    }

    fn shares_prefix(&self, c: &Course, legs: &[PlanLeg], s_div: f64) -> bool {
        let pre = c.prefix(s_div);
        legs.len() >= pre.len() && legs[..pre.len()] == pre[..]
    }

    fn uses_closed(&self, legs: &[PlanLeg], from_s: f64) -> bool {
        if self.closed.is_empty() {
            return false;
        }
        match Course::new(self.g, legs, &self.plan.eta) {
            Ok(c) => !c.ways_from(self.g, from_s).is_disjoint(&self.closed),
            Err(_) => true,
        }
    }

    fn refused(&self, cause: &Cause) -> bool {
        self.refused.iter().any(|r| r.same(cause))
    }

    fn a8(&mut self, t: f64, a: alerts::Alert, out: &mut Vec<Output>) {
        let kind = a.kind;
        let ann = a.event.announce();
        if let Some(c) = Condition::of(kind) {
            out.push(Output::Alert {
                t: utc_of(t),
                alert: Alert::Condition(c),
            });
            self.offer(
                t,
                Priority::Safety,
                &format!("{c:?}"),
                &format!("{c:?}"),
                ann,
                None,
            );
            let cause = Cause::Conditions(c);
            if self.p.shortcut_on.contains(&c) && !self.refused(&cause) {
                let here = self.model_here();
                let rem = self.model_total() - here;
                let best = self
                    .exits_ahead()
                    .into_iter()
                    .map(|i| (i, self.plan.plans_b[i].fallback.model_s - here))
                    .filter(|(_, r)| rem - r >= self.p.min_saving_s)
                    .min_by(|a, b| a.1.total_cmp(&b.1));
                if let Some((i, _)) = best {
                    self.switch_plan_b(t, i, cause, Fixed::PlanShortcut, Priority::Direction, out);
                }
            }
            return;
        }
        match kind {
            alerts::Kind::TurnaroundAdvanced => {
                out.push(Output::Alert {
                    t: utc_of(t),
                    alert: Alert::TurnaroundAdvanced,
                });
                let limit = self.plan.turnaround.deadline;
                let cause = Cause::Late {
                    limit: limit.unwrap_or(local(t)),
                };
                if !self.refused(&cause)
                    && let Some(l) = limit
                    && self.shorten(t, l, cause, out)
                {
                    return;
                }
                self.offer(
                    t,
                    Priority::Schedule,
                    "turnaround",
                    "turnaround_advanced",
                    ann,
                    None,
                );
            }
            alerts::Kind::TurnaroundNow => {
                out.push(Output::Alert {
                    t: utc_of(t),
                    alert: Alert::TurnaroundNow,
                });
                self.home_now(t, ann, out);
            }
            _ => {}
        }
    }

    /// Switches to the longest exit ahead that ends before `limit` at the observed pace.
    fn shorten(
        &mut self,
        t: f64,
        limit: NaiveDateTime,
        cause: Cause,
        out: &mut Vec<Output>,
    ) -> bool {
        let pace = self.pace(t).unwrap_or(1.0).max(1.0);
        let f = TurnaroundParams::default().eta_factor;
        let here = self.model_here();
        let group = self.plan.pace.is_some();
        let paused_for = self.remaining(t).1;
        // With the group of the day (C11): its arrival at the pace of the day once it says
        // something (`limit_prediction`; median, the slow side with children); else the
        // observed pace with the C7 margin.
        let arrival = |i: usize, model_total: f64| {
            if group {
                self.exit_rest(i)
                    .map(|rest| local(t + self.planned_s(&self.limit_prediction(rest, paused_for))))
            } else {
                Some(local(t + pace * f * (model_total - here)))
            }
        };
        let pick = self
            .exits_ahead()
            .into_iter()
            .map(|i| (i, self.plan.plans_b[i].fallback.model_s))
            .filter(|(i, m)| arrival(*i, *m).is_some_and(|a| a <= limit))
            .max_by(|a, b| a.1.total_cmp(&b.1));
        if let Some((i, _)) = pick {
            self.switch_plan_b(t, i, cause, Fixed::PlanShortened, Priority::Schedule, out);
            return true;
        }
        false
    }

    /// Return time asked: shorten when the arrival at the observed pace misses it.
    fn return_time(&mut self, t: f64, out: &mut Vec<Output>) {
        // The limit of the plan: the return time asked or the C7 deadline, the earlier.
        let Some(limit) = self.plan.turnaround.limit else {
            return;
        };
        if t - self.last_late < 600.0 {
            return;
        }
        let here = self.model_here();
        let started = self.session.totals().started.unwrap_or(t);
        // The observed pace once meaningful (A8's rule: 20 min of model time, 10 min and
        // 10 % late), else the model's: a late start alone can miss the limit.
        let ap = alerts::Params::default();
        let late_s = (t - started) - here;
        let pace = match self.pace(t) {
            Some(p) if late_s >= ap.delay_min * 60.0 && late_s >= ap.delay_frac * here => p,
            _ => 1.0,
        };
        let f = TurnaroundParams::default().eta_factor;
        let arrival = if self.plan.pace.is_some() {
            // The group of the day (C11): its arrival at the pace of the day once it says
            // something (`limit_prediction`; median, the slow side with children; stops
            // included; the C7 deadline keeps its margin).
            let paused_for = self.remaining(t).1;
            let rest = self.pace_route.rest(self.cursor);
            local(t + self.planned_s(&self.limit_prediction(rest, paused_for)))
        } else {
            local(t + pace * f * (self.model_total() - here))
        };
        if arrival <= limit {
            return;
        }
        self.last_late = t;
        let cause = Cause::Late { limit };
        if self.refused(&cause) {
            return;
        }
        out.push(Output::Alert {
            t: utc_of(t),
            alert: Alert::TurnaroundAdvanced,
        });
        if !self.shorten(t, limit, cause.clone(), out) {
            // Nothing ends in time: the exit that ends first, if it saves enough (the
            // deadline itself is A8's « turn around now »).
            let rem = self.model_total() - here;
            let best = self
                .exits_ahead()
                .into_iter()
                .map(|i| (i, self.plan.plans_b[i].fallback.model_s - here))
                .filter(|(_, r)| rem - r >= self.p.min_saving_s)
                .min_by(|a, b| a.1.total_cmp(&b.1));
            if let Some((i, _)) = best {
                self.switch_plan_b(t, i, cause, Fixed::PlanShortened, Priority::Schedule, out);
            }
        }
    }

    /// Closed ways, minus the one under the skier (a closed edge cannot be left).
    fn closed_but_here(&self, from: &Snap) -> BTreeSet<i64> {
        let mut c = self.closed.clone();
        if let Some(w) = self.g.edge(from.edge).attrs.osm_way_id {
            c.remove(&w);
        }
        c
    }

    /// No time left: the fastest way home from here, said as safety.
    fn home_now(&mut self, t: f64, a8: Announcement, out: &mut Vec<Output>) {
        let (Some(from), Some(c)) = (self.snap_here(), self.course()) else {
            self.offer(
                t,
                Priority::Safety,
                "turnaround_now",
                "turnaround_now",
                a8,
                None,
            );
            return;
        };
        let closed = self.closed_but_here(&from);
        let r = fallback::route(
            self.g,
            self.plan.eta,
            self.plan.request.max_difficulty,
            &closed,
            from,
            c.end_snap(self.g),
        );
        let Some(r) = r else {
            self.offer(
                t,
                Priority::Safety,
                "turnaround_now",
                "turnaround_now",
                a8,
                None,
            );
            return;
        };
        let mut legs = c.prefix(self.cursor);
        let s_div = self.cursor;
        legs.extend(r.legs.iter().map(|l| PlanLeg::from(*l)));
        let behind = self.starts_behind(&r.legs);
        let (phrase, kind) = if behind {
            (a8, "turnaround_now")
        } else {
            (phrases::announce(Fixed::PlanHomeNow), "home_now")
        };
        if self.install_live(t, legs, Cause::NoTime, s_div, out) && !behind {
            let a = match self.first_turn_event(s_div) {
                Some(e) => phrase.then(e.announce()),
                None => phrase,
            };
            self.offer(t, Priority::Safety, "turnaround_now", kind, a, None);
        } else {
            self.offer(t, Priority::Safety, "turnaround_now", kind, phrase, None);
        }
    }

    fn starts_behind(&self, legs: &[skimap_core::routing::Leg]) -> bool {
        let Some(l) = legs.first() else { return false };
        let len = self.g.length_m(l.edge);
        let ahead = if l.to_m >= l.from_m {
            (l.from_m + 40.0).min(len)
        } else {
            (l.from_m - 40.0).max(0.0)
        };
        let (a, b) = (
            self.g.point_at(l.edge, l.from_m),
            self.g.point_at(l.edge, ahead),
        );
        let h_route = (a.haversine_m(b) >= 1.0).then(|| crate::line::bearing_deg(a, b));
        let h_skier = self.session.heading(30.0);
        matches!((h_route, h_skier), (Some(r), Some(s)) if turn_deg(s, r).abs() > 120.0)
    }

    fn first_turn_event(&self, s_div: f64) -> Option<Event> {
        let j = self
            .active
            .junctions
            .iter()
            .find(|j| j.s_end >= s_div - 1.0 && j.direction.is_some())?;
        let metres = (j.s - self.cursor).max(0.0);
        let attrs = &self.g.edge(j.edge_out).attrs;
        Some(Event::Turn {
            metres,
            direction: j.direction.unwrap_or(Direction::Straight),
            piste: self.voice.and_then(|v| v.piste(attrs)),
        })
    }

    fn closure(&mut self, t: f64, ways: &[i64], out: &mut Vec<Output>) {
        self.closed.extend(ways.iter().copied());
        if !self.session.running() && self.session.phase() != &Phase::Waiting {
            return;
        }
        let Some(c) = self.course() else { return };
        let ahead = c.ways_from(self.g, self.cursor);
        let hit: Vec<i64> = ways.iter().copied().filter(|w| ahead.contains(w)).collect();
        if hit.is_empty() {
            return;
        }
        let cause = Cause::Closure { ways: hit.clone() };
        if self.refused(&cause) {
            return;
        }
        out.push(Output::Alert {
            t: utc_of(t),
            alert: Alert::Closed,
        });
        // A detour of the morning ahead that avoids every closed way.
        let pick = self
            .plan
            .plans_b
            .iter()
            .enumerate()
            .filter(|(_, b)| {
                matches!(&b.fallback.kind, FallbackKind::Detour { ways, .. } if ways.iter().any(|w| hit.contains(w)))
            })
            .filter(|(_, b)| b.fallback.exists() && b.fallback.s_div >= self.cursor)
            .filter(|(_, b)| self.shares_prefix(&c, &self.full(b), b.fallback.s_div))
            .filter(|(_, b)| !self.uses_closed(&self.full(b), b.fallback.s_div))
            .min_by(|a, b| a.1.fallback.s_div.total_cmp(&b.1.fallback.s_div))
            .map(|(i, _)| i);
        if let Some(i) = pick {
            self.switch_plan_b(
                t,
                i,
                cause,
                Fixed::PlanClosedAhead,
                Priority::Direction,
                out,
            );
            return;
        }
        // A route now: from here to the first junction after the last closed way, else
        // to the finish.
        let last_closed = c
            .legs()
            .iter()
            .enumerate()
            .filter(|(_, l)| {
                self.g
                    .edge(l.edge)
                    .attrs
                    .osm_way_id
                    .is_some_and(|w| self.closed.contains(&w))
            })
            .map(|(i, _)| c.leg_bounds()[i].1)
            .fold(f64::NEG_INFINITY, f64::max);
        let from = self.snap_here().unwrap_or_else(|| c.start_snap(self.g));
        let closed = self.closed_but_here(&from);
        let from = fallback::open_snap(self.g, from, &closed);
        let md = self.plan.request.max_difficulty;
        let mut legs_found: Option<Vec<PlanLeg>> = None;
        if let Some(j) = self
            .active
            .junctions
            .iter()
            .find(|j| j.s >= last_closed - 0.5)
            && let Some(k) = c.nodes().iter().position(|n| (n.s - j.s).abs() < 1e-6)
            && let Some(to) = fallback::node_snap_open(self.g, c.nodes()[k].node, &closed)
            && let Some(r) = fallback::route(self.g, self.plan.eta, md, &closed, from, to)
        {
            let mut legs = c.prefix(self.cursor);
            legs.extend(r.legs.iter().map(|l| PlanLeg::from(*l)));
            legs.extend(c.suffix_from_node(k));
            legs_found = Some(legs);
        }
        if legs_found.is_none()
            && let Some(r) = fallback::route(
                self.g,
                self.plan.eta,
                md,
                &closed,
                from,
                fallback::open_snap(self.g, c.end_snap(self.g), &closed),
            )
        {
            let mut legs = c.prefix(self.cursor);
            legs.extend(r.legs.iter().map(|l| PlanLeg::from(*l)));
            legs_found = Some(legs);
        }
        let s_div = self.cursor;
        let done = match legs_found {
            Some(legs) => self.install_live(t, legs, cause, s_div, out),
            None => false,
        };
        if done {
            let a = phrases::announce(Fixed::PlanClosedAhead);
            let a = match self.first_turn_event(s_div) {
                Some(e) => a.then(e.announce()),
                None => a,
            };
            self.offer(t, Priority::Direction, "switch", "closed_ahead", a, None);
        } else {
            out.push(Output::Alert {
                t: utc_of(t),
                alert: Alert::ClosedNoDetour,
            });
            self.offer(
                t,
                Priority::Direction,
                "switch",
                "closed_no_detour",
                phrases::announce(Fixed::PlanClosedNoDetour),
                None,
            );
        }
    }

    fn reroute(&mut self, t: f64, snap: Snap, out: &mut Vec<Output>) {
        let Some(c) = self.course() else { return };
        let closed = self.closed_but_here(&snap);
        let md = self.plan.request.max_difficulty;
        // Rejoin the junction ahead (next 5 km) that costs the least extra time.
        let here = c.model_s_at(self.cursor);
        let mut best: Option<(f64, Vec<PlanLeg>)> = None;
        let mut tried = 0;
        for (k, n) in c.nodes().iter().enumerate() {
            if n.s <= self.cursor + 50.0 || n.s >= self.cursor + 5000.0 || tried >= 12 {
                continue;
            }
            if !self
                .active
                .junctions
                .iter()
                .any(|j| (j.s - n.s).abs() < 1e-6)
            {
                continue;
            }
            tried += 1;
            let Some(to) = node_snap(self.g, n.node) else {
                continue;
            };
            let Some(r) = fallback::route(self.g, self.plan.eta, md, &closed, snap, to) else {
                continue;
            };
            let extra = r.cost - (c.model_s_at(n.s) - here);
            if best.as_ref().is_none_or(|(e, _)| extra < *e) {
                let mut legs = c.prefix(self.cursor);
                legs.extend(r.legs.iter().map(|l| PlanLeg::from(*l)));
                legs.extend(c.suffix_from_node(k));
                best = Some((extra, legs));
            }
        }
        if best.is_none()
            && let Some(r) =
                fallback::route(self.g, self.plan.eta, md, &closed, snap, c.end_snap(self.g))
        {
            let mut legs = c.prefix(self.cursor);
            legs.extend(r.legs.iter().map(|l| PlanLeg::from(*l)));
            best = Some((0.0, legs));
        }
        let Some((_, legs)) = best else { return };
        let s_div = self.cursor;
        let first = legs
            .get(c.prefix(self.cursor).len())
            .map(|l| skimap_core::routing::Leg::from(*l));
        let behind = first.is_some_and(|l| self.starts_behind(&[l]));
        if self.install_live(t, legs, Cause::Deviation, s_div, out) {
            // Not refusable by skiing on: the skier is already off the previous route.
            self.pending = None;
            let mut a = phrases::announce(Fixed::PlanRerouted);
            if behind {
                a = a.then(
                    Event::Turn {
                        metres: 0.0,
                        direction: Direction::UTurn,
                        piste: None,
                    }
                    .announce(),
                );
            }
            self.offer(
                t,
                Priority::Direction,
                "off_route",
                "rerouted",
                a,
                Some(60.0),
            );
        }
    }

    fn install(
        &mut self,
        t: f64,
        mut new: Active<'a>,
        cause: Cause,
        s_div: f64,
        out: &mut Vec<Output>,
    ) {
        let from = self.active.rf;
        let to = new.rf;
        if let Some((k, _)) = self.live_key.take() {
            self.speech.withdraw(&k);
        }
        // Decisions already behind the cursor are not said again.
        let cursor = {
            let v = new.view();
            self.cursor.min(f64::from(v.length()))
        };
        {
            let v = new.view();
            let mut k = 0;
            while k < v.decisions() && f64::from(v.decision(k).s_trigger) < cursor + 1.0 {
                k += 1;
            }
            new.next = k;
        }
        if self.monitor.is_some() {
            let profile = self.monitor_profile(new.view().to_profile());
            if let Some(m) = &mut self.monitor {
                m.replace_route(Cow::Owned(profile));
            }
        }
        let prev = std::mem::replace(&mut self.active, new);
        if cause != Cause::Deviation {
            self.switched_other = true;
        }
        self.props.pending = None;
        self.pending = Some(Pending {
            prev,
            prev_cursor: self.cursor,
            cause: cause.clone(),
            s_div,
        });
        self.cursor = cursor;
        self.cursor_t = self.last_fix.map_or(f64::NEG_INFINITY, |(t, _)| t);
        self.pace_route_again();
        self.detector = DeviationDetector::new(self.hysteresis());
        self.deviating_since = None;
        self.switches += 1;
        out.push(Output::Switched {
            t: utc_of(t),
            from,
            to,
            cause,
            s_div,
        });
    }

    fn install_live(
        &mut self,
        t: f64,
        legs: Vec<PlanLeg>,
        cause: Cause,
        s_div: f64,
        out: &mut Vec<Output>,
    ) -> bool {
        let Ok(course) = Course::new(self.g, &legs, &self.plan.eta) else {
            return false;
        };
        self.live += 1;
        let junctions = guidance::junctions(self.g, &course);
        // The same announcements as the plan's routes (X11): with the layers of the day,
        // zones without network, avalanche paths and the protected zones the loop does
        // not cross.
        if let Some(data) = self.layers
            && self.loop_zones.is_none()
            && let Ok(c) = Course::new(self.g, &self.plan.route.legs, &self.plan.eta)
        {
            self.loop_zones = Some(
                plan::protected_zones(
                    c.line().points(),
                    (0.0, f64::INFINITY),
                    data,
                    self.plan.date,
                )
                .into_iter()
                .map(|z| z.id)
                .collect(),
            );
        }
        let hazards = self.layers.map(|data| plan::Hazards {
            data,
            date: self.plan.date,
            zones_told: self.loop_zones.as_ref(),
            zones_range: (s_div, f64::INFINITY),
        });
        let own = plan::announcements(
            self.g,
            &course,
            &junctions,
            self.voice,
            hazards.as_ref(),
            self.plan.departure,
        );
        let refs: Vec<(u32, u32, &PlannedAnnouncement)> = own
            .iter()
            .enumerate()
            .map(|(i, a)| (u32::MAX, i as u32, a))
            .collect();
        let table = RouteTable::new(-1, s_div, &course, &self.plane, &refs);
        let new = Active {
            rf: RouteRef::Live(self.live),
            geo: Geo::Owned(table.bytes()),
            legs,
            own,
            junctions,
            next: 0,
        };
        self.install(t, new, cause, s_div, out);
        true
    }

    fn switch_plan_b(
        &mut self,
        t: f64,
        i: usize,
        cause: Cause,
        phrase: Fixed,
        priority: Priority,
        out: &mut Vec<Output>,
    ) {
        let Some(view) = self.track.find(i as i32 + 1) else {
            return;
        };
        let legs = self.full(&self.plan.plans_b[i]);
        let junctions = match Course::new(self.g, &legs, &self.plan.eta) {
            Ok(c) => guidance::junctions(self.g, &c),
            Err(_) => return,
        };
        let new = Active {
            rf: RouteRef::PlanB(i),
            geo: Geo::Table(view),
            legs,
            own: Vec::new(),
            junctions,
            next: 0,
        };
        let s_div = self.plan.plans_b[i].fallback.s_div;
        self.install(t, new, cause, s_div, out);
        let a = crate::plan::switch_announcement(phrase, self.first_turn_event(s_div));
        // Kind: the phrase without the bank's `Plan` prefix (« Shortcut », « Shortened »).
        let name = format!("{phrase:?}");
        let kind = name.strip_prefix("Plan").unwrap_or(&name);
        self.offer(t, priority, "switch", kind, a, None);
    }

    /// Installs plan B `i` (no announcement); false if it cannot be followed.
    fn install_plan_b(&mut self, t: f64, i: usize, cause: Cause, out: &mut Vec<Output>) -> bool {
        let Some(view) = self.track.find(i as i32 + 1) else {
            return false;
        };
        let legs = self.full(&self.plan.plans_b[i]);
        let junctions = match Course::new(self.g, &legs, &self.plan.eta) {
            Ok(c) => guidance::junctions(self.g, &c),
            Err(_) => return false,
        };
        let new = Active {
            rf: RouteRef::PlanB(i),
            geo: Geo::Table(view),
            legs,
            own: Vec::new(),
            junctions,
            next: 0,
        };
        let s_div = self.plan.plans_b[i].fallback.s_div;
        self.install(t, new, cause, s_div, out);
        true
    }

    /// The model time per terrain of the route followed (after a change of route).
    /// The time planned on for a prediction of the day (C11): the median; with children,
    /// the median times the morning's ratio of the slow side to the median (the day's own
    /// spread starts wider than the morning's, its online priors being capped: its slow
    /// side would shorten every outing at the start).
    #[inline]
    fn planned_s(&self, p: &Prediction) -> f64 {
        if self.plan_q > 0.5 {
            p.median_s * self.slow_ratio
        } else {
            p.median_s
        }
    }

    /// The profile A8 follows on a route: at the time the plan's group is planned on when
    /// it has one (C11, as `alerts::Monitor::for_model`), else the model's.
    fn monitor_profile(
        &self,
        p: skimap_assist::suggest::Profile,
    ) -> skimap_assist::suggest::Profile {
        match &self.plan.pace {
            Some(pace) => p.for_group_at(&pace.params, self.ratio, self.plan_q),
            None => p,
        }
    }

    fn pace_route_again(&mut self) {
        if let Ok(c) = Course::new(self.g, &self.active.legs, &self.plan.eta) {
            self.pace_route = PaceRoute::of(&c, self.ratio);
        }
        self.pace_last = self.last_fix.map(|(t, _)| (t, self.cursor));
    }

    /// A fix while moving: the pace of the day (no allocation).
    #[inline]
    fn pace_fix(&mut self, t: f64) {
        let c = self.cursor;
        if let Some((t0, s0)) = self.pace_last {
            let (dt, ds) = (t - t0, c - s0);
            // A long gap without progress is not moving (GNSS off, a stop not decided).
            if dt > 0.0 && ds >= 0.0 && !(dt > 60.0 && ds < 30.0) {
                let d = self.pace_route.between(s0, c);
                self.day.moving(dt, d);
                for (m, x) in self.model_done.iter_mut().zip(d) {
                    *m += x.max(0.0);
                }
                self.progress_total += ds;
            }
        }
        self.pace_last = Some((t, c));
    }

    /// Model time per terrain left on plan B `i` from the cursor (allocates: an event).
    fn exit_rest(&self, i: usize) -> Option<[f64; TERRAINS]> {
        let legs = self.full(&self.plan.plans_b[i]);
        let c = Course::new(self.g, &legs, &self.plan.eta).ok()?;
        Some(PaceRoute::of(&c, self.ratio).rest(self.cursor))
    }

    /// The group's planned moving time from the start to the cursor (its own pace per
    /// terrain), seconds.
    fn group_s_here(&self) -> f64 {
        let at = self.pace_route.at(self.cursor);
        at.iter()
            .zip(&self.pace_base.pace)
            .map(|(m, n)| m * (-n.m).exp())
            .sum()
    }

    /// Remaining time on plan B `i` from the cursor at the pace of the day (allocates: an
    /// event).
    fn exit_prediction(&self, i: usize, paused_for: Option<f64>) -> Option<Prediction> {
        Some(self.day.predict(self.exit_rest(i)?, paused_for))
    }

    /// The remaining time a limit is checked on (a change of route for the return time or
    /// the C7 deadline): at the pace of the day once it is meaningful (A8's rule for a
    /// group: 20 min of the group's planned time done, [`Tracker::group_s_here`]), at the
    /// group's own pace before. The first minutes say nothing of the day: a stop not
    /// decided yet (90 s) counts as moving without progress, and a family stopping two
    /// minutes after the start would read as three times slower than planned and be sent
    /// on a shorter loop at once. A late start still counts (the rest from now).
    fn limit_prediction(&self, rest: [f64; TERRAINS], paused_for: Option<f64>) -> Prediction {
        if self.group_s_here() >= 1200.0 {
            self.day.predict(rest, paused_for)
        } else {
            self.day.predict_prior(rest, paused_for)
        }
    }

    /// The pace of the day.
    pub fn day_pace(&self) -> &DayPace {
        &self.day
    }

    /// What the outing tells about the group so far (for the learning, C11).
    pub fn pace_observation(&self) -> OutingObs {
        let tot = self.session.totals();
        let end = tot
            .finished
            .or(self.last_fix.map(|(t, _)| t))
            .unwrap_or(0.0);
        let elapsed = tot.started.map_or(0.0, |s| (end - s).max(0.0));
        self.day
            .observation(self.progress_total, tot.distance_m, &self.stops_s, elapsed)
    }

    /// Remaining time at the pace of the day from the last fix (no allocation).
    fn remaining(&self, t: f64) -> (Prediction, Option<f64>) {
        let paused_for = match self.session.phase() {
            Phase::Paused { since, .. } => Some((t - since).max(0.0)),
            _ => None,
        };
        let rem = self.pace_route.rest(self.cursor);
        (self.day.predict(rem, paused_for), paused_for)
    }

    /// The estimated arrival at the pace of the day (C11), once the outing started.
    pub fn eta(&self) -> Option<Eta> {
        let r = self.reference?;
        let (t, _) = self.last_fix?;
        if !self.session.running() {
            return None;
        }
        let (p, _) = self.remaining(t);
        Some(Eta {
            at: local(t),
            arrival: local(t + p.median_s),
            arrival_lo: local(t + p.lo_s),
            arrival_hi: local(t + p.hi_s),
            planned: local(r.arrival),
            remaining: p,
            pace: self.day.pace(Terrain::Flat),
            stops: self.day.stops(),
        })
    }

    /// Once a minute: a proposal when the group is durably late on its plan (C11).
    fn pace_check(&mut self, t: f64, out: &mut Vec<Output>) {
        let pp = self.p.propose;
        if self
            .props
            .pending
            .as_ref()
            .is_some_and(|p| t - p.t >= pp.ttl_s)
        {
            self.props.pending = None;
        }
        let Some(r) = self.reference else { return };
        let moving = self.day.moving_total_s();
        let last_mark = match self.props.mark_n {
            0 => None,
            n => Some(self.props.marks[(n - 1) % self.props.marks.len()].0),
        };
        if last_mark.is_none_or(|m| moving - m >= 300.0) {
            let k = self.props.mark_n % self.props.marks.len();
            self.props.marks[k] = (moving, self.progress_total);
            self.props.mark_n += 1;
        }
        // Progress over the last `recent_moving_s` of moving (the newest mark that old).
        let recent = self
            .props
            .marks
            .iter()
            .take(self.props.mark_n.min(self.props.marks.len()))
            .filter(|(m, _)| moving - m >= pp.recent_moving_s)
            .max_by(|a, b| a.0.total_cmp(&b.0))
            .map(|(_, p)| self.progress_total - p);
        if !pp.enabled
            || !self.session.running()
            || self.session.paused()
            || self.props.pending.is_some()
            || self.profile_pending.is_some()
            || self.pending.is_some()
            || self.switched_other
        {
            return;
        }
        let (pred, paused_for) = self.remaining(t);
        // Late on the median of the day against the median planned; confirmed when nine
        // chances in ten late, with children when more likely than not (sooner, C11).
        let delay = t + pred.median_s - r.median;
        let lo_delay = if self.plan_q > 0.5 {
            delay
        } else {
            t + pred.lo_s - r.median
        };
        let thr = pp.min_delay_s.max(pp.delay_frac * r.total.median_s);
        if delay >= thr && lo_delay > 0.0 {
            self.props.since.get_or_insert(t);
        } else if delay < pp.clear_frac * thr {
            self.props.since = None;
        }
        let Some(since) = self.props.since else {
            return;
        };
        if t - since < pp.durable_s
            || t - r.t0 < pp.min_elapsed_s
            || self.progress_total < pp.min_progress_m
            || recent.is_none_or(|p| p < pp.recent_progress_m)
            || self.props.count >= pp.max
            || self.props.last_t.is_some_and(|l| t - l < pp.spacing_s)
            || self
                .props
                .last_delay
                .is_some_and(|d| delay < d + pp.worse_s)
            || pred.median_s < pp.min_rest_s
            || delay < thr
        {
            return;
        }
        if self.profile_offer(t, self.p.profile.min_log_ratio_late, out) {
            return;
        }
        self.propose(t, r, pred, paused_for, delay, out);
    }

    fn propose(
        &mut self,
        t: f64,
        r: Reference,
        pred: Prediction,
        paused_for: Option<f64>,
        delay: f64,
        out: &mut Vec<Output>,
    ) {
        let pp = self.p.propose;
        let here = self.cursor;
        // The exits of the morning ahead, at the pace of the day.
        let mut cands: Vec<(usize, Prediction, f64)> = Vec::new();
        for i in self.exits_ahead() {
            if let Some(p) = self.exit_prediction(i, paused_for) {
                cands.push((i, p, self.plan.plans_b[i].fallback.model_s));
            }
        }
        // On the time planned on (median; the slow side with children).
        let loop_arr = t + self.planned_s(&pred);
        let short = cands
            .iter()
            .filter(|c| t + self.planned_s(&c.1) <= r.arrival + pp.slack_s)
            .filter(|c| loop_arr - (t + self.planned_s(&c.1)) >= 600.0)
            .max_by(|a, b| a.2.total_cmp(&b.2));
        let nearest = cands
            .iter()
            .min_by(|a, b| self.planned_s(&a.1).total_cmp(&self.planned_s(&b.1)))
            .filter(|c| loop_arr - (t + self.planned_s(&c.1)) >= pp.min_saving_s);
        let (kind, pick) = match (short, nearest) {
            (Some(c), _) => (ProposalKind::ShortLoop, Some(c)),
            (None, Some(c)) => (ProposalKind::NearestExit, Some(c)),
            (None, None) => (ProposalKind::Keep, None),
        };
        let p = pick.map_or(pred, |c| c.1);
        // Why: moving slower than the group's pace, more stops than its rate.
        let base = self.pace_base;
        let done = self.pace_route.at(here);
        let expected_moving: f64 = (0..done.len())
            .map(|c| done[c] * (-base.pace[c].m).exp())
            .sum();
        let moving = self.day.moving_total_s().max(1.0);
        let (_, stopped) = self.day.stops();
        let stopped = stopped + paused_for.unwrap_or(0.0);
        let expected_stop = base.stops.mean() * moving / 3600.0 * base.stop_mean_s();
        let more_pauses = stopped - expected_stop >= 300.0 && stopped >= 1.5 * expected_stop;
        let slower = expected_moving / moving < 0.85;
        let reason = match (slower, more_pauses) {
            (true, true) => PaceReason::SlowerAndPauses,
            (false, true) => PaceReason::Pauses,
            _ => PaceReason::Slower,
        };
        let expected_elapsed = base.predict(done).median_s;
        let elapsed = (t - r.t0).max(1.0);
        let slower_pct =
            (((1.0 - expected_elapsed / elapsed) * 100.0 / 5.0).round() * 5.0).clamp(0.0, 95.0);
        let (arr, lo, hi) = (
            local(t + self.planned_s(&p)),
            local(t + p.lo_s),
            local(t + p.hi_s),
        );
        let saved = pick.map(|c| (self.planned_s(&pred) - self.planned_s(&c.1)) / 60.0);
        self.props.count += 1;
        let proposal = Proposal {
            id: self.props.count,
            kind,
            reason,
            route: pick.map(|c| RouteRef::PlanB(c.0)),
            slower_pct,
            stops_min: ((stopped / 60.0).round(), (expected_stop / 60.0).round()),
            planned: local(r.arrival),
            loop_arrival: local(loop_arr),
            arrival: arr,
            arrival_lo: lo,
            arrival_hi: hi,
            delay_min: (delay / 60.0).round(),
            text: proposal_text_in(
                reason,
                slower_pct,
                kind,
                (arr, lo, hi),
                saved,
                self.plan.lang,
            ),
            announcement: proposal_announcement(reason, kind, arr),
        };
        self.offer(
            t,
            Priority::Schedule,
            "proposal",
            "proposal",
            proposal.announcement.clone(),
            Some(pp.ttl_s),
        );
        out.push(Output::Proposal {
            t: utc_of(t),
            proposal: Box::new(proposal),
        });
        self.props.last_t = Some(t);
        // The next one only if it gets worse (an acceptance clears it).
        self.props.last_delay = Some(delay);
        self.props.pending = Some(PendingProposal {
            t,
            kind,
            exit: pick.map(|c| c.0),
            arrival: t + self.planned_s(&p),
            median: t + p.median_s,
            delay,
        });
    }

    /// Once a minute (X14): on a plan without a group (and no profile chosen), the stops
    /// still to come are those of the profiles compared ([`Tracker::profiles`]) weighted by
    /// how well each explains the day so far ([`pace::day_fit`]; equal weights before any
    /// moving), not those of a lone adult: a family nobody described is not predicted with
    /// an adult's stops, nor an adult with a family's.
    fn unknown_stops(&mut self) {
        if self.plan.pace.is_some() || self.profile_chosen.is_some() || self.profiles.is_empty() {
            return;
        }
        let moving = self.day.moving_total_s();
        let (stops, _) = self.day.stops();
        let fit = |m: &ProfileModel| pace::day_fit(&m.params, self.model_done, moving, stops);
        let best = self
            .profiles
            .iter()
            .map(fit)
            .fold(f64::NEG_INFINITY, f64::max);
        if let Some((rate, len)) = calibration::stop_mixture(
            self.profiles
                .iter()
                .map(|m| ((fit(m) - best).exp(), &m.params)),
        ) {
            self.day.set_stop_prior(rate, len);
        }
    }

    /// Once a minute (X14): when the pace of the day fits another group profile much better
    /// than the one planned on (a lone adult when the plan has none), propose it, once.
    fn profile_check(&mut self, t: f64, out: &mut Vec<Output>) {
        let fp = self.p.profile;
        if let Some((t0, _)) = self.profile_pending
            && t - t0 >= fp.ttl_s
        {
            self.profile_pending = None;
        }
        if self.props.pending.is_some() || self.pending.is_some() {
            return;
        }
        self.profile_offer(t, fp.min_log_ratio, out);
    }

    /// The profile proposal (X14) if one fits the day `min_ratio` better than the profile
    /// planned on; true when made. Before a proposal of a shorter way (C11), the group being
    /// durably late already says the profile does not fit: a lower ratio
    /// ([`ProfileFitParams::min_log_ratio_late`]) is asked, and the shorter way waits for
    /// the answer (planned again on the profile chosen).
    fn profile_offer(&mut self, t: f64, min_ratio: f64, out: &mut Vec<Output>) -> bool {
        let fp = self.p.profile;
        if !fp.enabled
            || self.profile_asked
            || self.profile_pending.is_some()
            || self.profiles.is_empty()
            || !self.session.running()
            || self.session.paused()
        {
            return false;
        }
        let moving = self.day.moving_total_s();
        let (stops, _) = self.day.stops();
        if moving < fp.min_moving_s || self.progress_total < fp.min_progress_m {
            return false;
        }
        let current = self.plan.pace.as_ref().map(|p| &p.profile);
        // The profile planned on; without one, a lone adult (the plan's « nobody described »
        // is too wide to be contradicted: the question is whether the day is an adult's).
        let reference = match &self.plan.pace {
            Some(_) => self.pace_base,
            None => self.adult,
        };
        let here = pace::day_fit(&reference, self.model_done, moving, stops);
        let best = self
            .profiles
            .iter()
            .enumerate()
            .filter(|(_, m)| current != Some(&m.profile))
            .map(|(i, m)| {
                (
                    i,
                    pace::day_fit(&m.params, self.model_done, moving, stops) - here,
                )
            })
            .max_by(|a, b| a.1.total_cmp(&b.1));
        let Some((i, ratio)) = best.filter(|b| b.1 >= min_ratio) else {
            return false;
        };
        self.profile_asked = true;
        self.profile_pending = Some((t, i));
        let profile = self.profiles[i].profile.clone();
        let announcement = phrases::announce(Fixed::PlanProfileMismatch);
        self.offer(
            t,
            Priority::Schedule,
            "profile",
            "profile_proposal",
            announcement.clone(),
            Some(fp.ttl_s),
        );
        out.push(Output::ProfileProposal {
            t: utc_of(t),
            proposal: Box::new(ProfileProposal {
                index: i,
                text: pace::profile_text(&profile),
                profile,
                log_ratio: ratio,
                announcement,
            }),
        });
        true
    }

    /// The answer to the profile proposal (X14): accepted, the day is planned on that
    /// profile's parameters from now on (its pace of the day rebuilt on what was skied, its
    /// proposals with children sooner).
    fn profile_answer(&mut self, t: f64, accept: bool, out: &mut Vec<Output>) {
        let Some((_, i)) = self.profile_pending.take() else {
            return;
        };
        self.speech.withdraw("profile");
        let mut chosen = None;
        if accept {
            let m = self.profiles[i].clone();
            let moving = self.day.moving_total_s();
            let mut day = DayPace::new(&m.params, self.pace_route.total());
            day.moving(moving, self.model_done);
            for s in &self.stops_s {
                day.stop(*s);
            }
            self.day = day;
            self.pace_base = m.params;
            self.plan_q = m.plan_quantile();
            if m.profile.has_children() {
                self.p.propose = self.p.propose.with_children();
            }
            self.offer(
                t,
                Priority::Schedule,
                "profile",
                "profile_accepted",
                phrases::announce(Fixed::PlanProposalAccepted),
                Some(60.0),
            );
            self.profile_chosen = Some(m.profile.clone());
            chosen = Some(m.profile);
        }
        out.push(Output::ProfileAnswered {
            t: utc_of(t),
            accept,
            profile: chosen,
        });
    }

    fn answer(&mut self, t: f64, accept: bool, out: &mut Vec<Output>) {
        let Some(pd) = self.props.pending.take() else {
            return;
        };
        self.speech.withdraw("proposal");
        let mut applied = false;
        if accept {
            match pd.exit {
                Some(i)
                    if self.active.rf == RouteRef::Plan
                        && self.pending.is_none()
                        && self.plan.plans_b[i].fallback.s_div >= self.cursor =>
                {
                    let s_div = self.plan.plans_b[i].fallback.s_div;
                    if self.install_plan_b(t, i, Cause::Pace, out) {
                        applied = true;
                        let mut a = phrases::announce(Fixed::PlanProposalAccepted);
                        if let Some(e) = self.first_turn_event(s_div) {
                            a = a.then(e.announce());
                        }
                        self.offer(
                            t,
                            Priority::Direction,
                            "switch",
                            "proposal_accepted",
                            a,
                            None,
                        );
                    }
                }
                Some(_) => {}
                None => {
                    applied = true;
                    self.offer(
                        t,
                        Priority::Schedule,
                        "proposal",
                        "proposal_accepted",
                        phrases::announce(Fixed::PlanProposalAccepted),
                        Some(60.0),
                    );
                }
            }
            if applied && let Some(r) = &mut self.reference {
                // The new return time is the plan now.
                r.arrival = pd.arrival;
                r.median = pd.median;
                self.props.last_delay = None;
            }
        } else {
            self.props.last_delay = Some(pd.delay);
            self.offer(
                t,
                Priority::Direction,
                "proposal",
                "kept",
                phrases::announce(Fixed::PlanKept),
                Some(60.0),
            );
        }
        out.push(Output::Answered {
            t: utc_of(t),
            kind: pd.kind,
            accept,
            applied,
        });
    }

    fn revert(&mut self, t: f64, implicit: bool, out: &mut Vec<Output>) {
        let Some(pd) = self.pending.take() else {
            return;
        };
        if self.monitor.is_some() {
            let profile = self.monitor_profile(pd.prev.view().to_profile());
            if let Some(m) = &mut self.monitor {
                m.replace_route(Cow::Owned(profile));
            }
        }
        self.speech.withdraw("switch");
        self.active = pd.prev;
        self.cursor = pd.prev_cursor;
        self.cursor_t = f64::NEG_INFINITY;
        self.pace_route_again();
        // Decisions passed meanwhile on the previous route are not said late.
        let next = {
            let v = self.active.view();
            let mut k = self.active.next;
            while k < v.decisions() && f64::from(v.decision(k).s_until) < self.cursor {
                k += 1;
            }
            k
        };
        self.active.next = next;
        self.detector = DeviationDetector::new(self.hysteresis());
        self.deviating_since = None;
        self.refused.push(pd.cause.clone());
        out.push(Output::Reverted {
            t: utc_of(t),
            to: self.active.rf,
            cause: pd.cause,
            implicit,
        });
        self.offer(
            t,
            Priority::Direction,
            "switch",
            "kept",
            phrases::announce(Fixed::PlanKept),
            Some(60.0),
        );
    }
}
