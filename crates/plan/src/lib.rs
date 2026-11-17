//! Day plan (J1 to J4, Q9, E1, C11): the morning choices turned into a plan the phone follows
//! without any manipulation when all goes well.
//!
//! - [`plan`] (J1): [`plan::morning`] gathers the briefing (A1), best time (A9), wax (A2),
//!   loops (A5) and equipment (A6) of a request; [`plan::build`] turns the loop chosen
//!   into a [`plan::DayPlan`]: route on the trail graph (C1), planned passage times (C5),
//!   turnaround (C7), planned announcements with the clips of the pack (VO2, VO3), plans
//!   B, equipment and reasons. Serialised as JSON, read back on the phone.
//! - [`fallback`] (J2): plans B computed in the morning at every useful junction.
//! - [`track`] (E1): the flat table of the loop and its plans B (geometry, grid index,
//!   decision points, time profile) computed on the workstation, read in place on the
//!   phone.
//! - [`tracker`] (J2, J3, E1): fixes and events in, decisions out: start, pause, refuge,
//!   end with recap ([`session`]), deviation (C2), condition alerts (A8), switch to a plan
//!   B and its refusal, GPS mode ([`gps`]); no allocation for a fix where all goes well.
//! - [`speech`] (J4): hierarchy, rate, grouping; safety never lost.
//! - [`scenario`] (Q9): synthetic outings on the real graphs replayed with a simulated
//!   clock, in closed loop with the GPS modes and the switches.
//! - [`pace`] (C11): the estimated time learned per group on the plan's routes (morning
//!   range, the proposals of the tracker when the group is durably slower, the digest of a
//!   past outing for the learning); [`pace_eval`]: its errors against C5 alone on
//!   synthetic outings.
//!
//! Shared: [`course`] (a route followed), [`guidance`] (junctions, turns, obvious ways),
//! [`line`](mod@line) (geometry along a route), [`phrases`] (what the plan says), [`presets`]
//! (requests on archived days of both sites), [`error`].
//!
//! Runs on the phone: no networking. Never « sans danger », never « sûr ».

pub mod course;
pub mod error;
pub mod fallback;
pub mod gps;
pub mod guidance;
pub mod line;
pub mod outing;
pub mod pace;
pub mod pace_eval;
pub mod phrases;
pub mod plan;
pub mod presets;
pub mod scenario;
pub mod session;
pub mod speech;
pub mod text;
pub mod track;
pub mod tracker;

pub use error::{Error, Result};
