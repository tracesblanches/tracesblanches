//! Following the plan during the outing (J2 #111, J3 #112, J4 #113, E1 #125): fixes and
//! events in, what to say, which route to follow and which GPS mode to use out. Nothing to
//! touch when all goes well.
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
//! 5. asks the speech policy ([`crate::speech`]) whether something is due.
//!
//! No allocation on that path (tested by counting the allocator). Once a minute: the
//! condition alerts and turnaround of A8 (`alerts::Monitor`: hourly weather, minutes of
//! delay) and the limit of the plan (return time asked or C7 deadline: a late start or a
//! slow pace that misses it shortens the loop). On events only (allocating): off the route for
//! [`TrackerParams::reroute_after_s`] → a new route on the trail graph to the junction
//! ahead that costs the least extra time (or the finish); a closed trail → the plan B of
//! the morning that goes around it, else a route computed then, else « no other way
//! known »; fog or rising wind → the exit saving the most time; late → the longest exit
//! that still ends in time (shortened loop); no time left → the fastest way home (safety).
//! A refusal (one gesture) or skiing on along the previous route past the divergence puts
//! the previous route back; the same cause does not switch again.

use std::borrow::Cow;
use std::collections::BTreeSet;

use chrono::{DateTime, NaiveDateTime, Utc};
use serde::{Deserialize, Serialize};
use skimap_assist::alerts;
use skimap_assist::layers::WeatherPoint;
use skimap_assist::suggest::{loop_voice, node_snap};
use skimap_common::geom::LonLat;
use skimap_core::graph::{LocalPlane, PisteGraph, Snap};
use skimap_core::matching::{AlertEvent, DeviationDetector, Hysteresis, ROUTE_HYSTERESIS};
use skimap_core::turnaround::TurnaroundParams;
use skimap_voice::Event;
use skimap_voice::bank::{Announcement, Side};
use skimap_voice::clips::Direction;

use crate::course::{Course, PlanLeg};
use crate::error::{Error, Result};
use crate::fallback::{self, FallbackKind};
use crate::gps::{self, GpsInput, GpsParams, GpsRequest, GpsState};
use crate::guidance::{self, Junction};
use crate::line::turn_deg;
use crate::phrases::{PlanPhrase, Voice};
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
}

impl Input {
    /// Time of the input.
    pub fn t(&self) -> DateTime<Utc> {
        match self {
            Input::Fix { t, .. }
            | Input::Activity { t, .. }
            | Input::Screen { t, .. }
            | Input::Closed { t, .. }
            | Input::Refuse { t } => *t,
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
            | Output::Recap { t, .. } => *t,
        }
    }
}

/// Where the geometry of a route comes from.
#[derive(Debug, Clone)]
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
            p,
        })
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
            Input::Closed { ways, .. } => self.closure(t, ways, out),
            Input::Refuse { .. } => self.revert(t, false, out),
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
                    let back = start
                        + chrono::Duration::seconds(
                            (model * TurnaroundParams::default().eta_factor) as i64,
                        );
                    let ev = loop_voice(length / 1000.0, model, back);
                    self.offer(
                        t,
                        Priority::Comfort,
                        "departure",
                        "loop_summary",
                        ev.announce(),
                        None,
                    );
                    let point: Cow<'a, WeatherPoint> = match &self.plan.weather {
                        Some(w) => Cow::Borrowed(w),
                        None => Cow::Owned(WeatherPoint {
                            id: "none".into(),
                            name: "sans météo".into(),
                            position: self.plan.request.start,
                            elevation_m: 0.0,
                            hours: Vec::new(),
                        }),
                    };
                    self.monitor = Some(alerts::Monitor::with(
                        start,
                        Cow::Owned(profile),
                        point,
                        alerts::Params::default(),
                    ));
                }
                SessionEvent::Paused { .. } => self.speech.set_paused(true),
                SessionEvent::Refuge { name, .. } => {
                    if !self.refuges.contains(&name) {
                        self.refuges.push(name);
                    }
                }
                SessionEvent::Resumed { .. } => self.speech.set_paused(false),
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
                        PlanPhrase::Finished.announce(),
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
        let comma = |x: String| x.replace('.', ",");
        let text = format!(
            "{} : {} km en {} min de mouvement et {} min d'arrêt ; {} % de l'itinéraire suivi ({} km){} ; {} changement(s) d'itinéraire, {} annonce(s){}",
            match end {
                End::Arrived => "Sortie terminée à l'arrivée",
                End::Abandoned => "Sortie interrompue loin de l'arrivée",
            },
            comma(format!("{:.1}", tot.distance_m / 1000.0)),
            (moving / 60.0).round(),
            (tot.paused_s / 60.0).round(),
            (done * 100.0).round(),
            comma(format!("{:.1}", len / 1000.0)),
            pace.map(|p| format!(
                ", temps réel / temps du modèle : {}",
                comma(format!("{p:.2}"))
            ))
            .unwrap_or_default(),
            self.switches,
            self.spoken,
            if self.refuges.is_empty() {
                String::new()
            } else {
                format!(", arrêts : {}", self.refuges.join(", "))
            }
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
        // 2. Session.
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
            self.return_time(t, out);
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
                self.offer(t, pr, &key, &kind, ann, Some(120.0));
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
                    self.switch_plan_b(t, i, cause, PlanPhrase::Shortcut, Priority::Direction, out);
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
        let arrival = |model_total: f64| local(t + pace * f * (model_total - here));
        let pick = self
            .exits_ahead()
            .into_iter()
            .map(|i| (i, self.plan.plans_b[i].fallback.model_s))
            .filter(|(_, m)| arrival(*m) <= limit)
            .max_by(|a, b| a.1.total_cmp(&b.1));
        if let Some((i, _)) = pick {
            self.switch_plan_b(t, i, cause, PlanPhrase::Shortened, Priority::Schedule, out);
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
        let arrival = local(t + pace * f * (self.model_total() - here));
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
                self.switch_plan_b(t, i, cause, PlanPhrase::Shortened, Priority::Schedule, out);
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
            (PlanPhrase::HomeNow.announce(), "home_now")
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
                PlanPhrase::ClosedAhead,
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
            let a = PlanPhrase::ClosedAhead.announce();
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
                PlanPhrase::ClosedNoDetour.announce(),
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
            let mut a = PlanPhrase::Rerouted.announce();
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
        if let Some(m) = &mut self.monitor {
            m.replace_route(Cow::Owned(new.view().to_profile()));
        }
        let prev = std::mem::replace(&mut self.active, new);
        self.pending = Some(Pending {
            prev,
            prev_cursor: self.cursor,
            cause: cause.clone(),
            s_div,
        });
        self.cursor = cursor;
        self.cursor_t = self.last_fix.map_or(f64::NEG_INFINITY, |(t, _)| t);
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
        let own = plan::announcements(
            self.g,
            &course,
            &junctions,
            self.voice,
            None,
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
        phrase: PlanPhrase,
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
        self.offer(t, priority, "switch", &format!("{phrase:?}"), a, None);
    }

    fn revert(&mut self, t: f64, implicit: bool, out: &mut Vec<Output>) {
        let Some(pd) = self.pending.take() else {
            return;
        };
        if let Some(m) = &mut self.monitor {
            m.replace_route(Cow::Owned(pd.prev.view().to_profile()));
        }
        self.speech.withdraw("switch");
        self.active = pd.prev;
        self.cursor = pd.prev_cursor;
        self.cursor_t = f64::NEG_INFINITY;
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
            PlanPhrase::Kept.announce(),
            Some(60.0),
        );
    }
}
