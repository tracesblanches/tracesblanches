//! Algorithm core shared by the server and the phone.
//!
//! Owned tickets, one module each so that agents can work in parallel:
//! - [`routing`]: C1 (#33) trail graph and on-trail routing.
//! - [`matching`]: C2 (#34) map matching and deviation detection; C10 (#42) validation on
//!   real traces under forest.
//! - [`offpiste`]: C3 (#35) off-piste routing on a cost grid; C4 (#36) benchmark on the
//!   Pixel 8.
//! - [`eta`]: C5 (#37) estimated time model.
//! - [`calibration`]: C11 (#130) pace learned per group (moving pace by terrain, stops,
//!   detours; offline and online Bayesian update), base of C6 (#38).
//! - [`turnaround`]: C7 (#39) turnaround time before nightfall.
//! - [`zone_eval`]: C8 (#40) zones and rules active at a position and a date.
//! - [`battery`]: C9 (#41) battery forecast model.
//!
//! Shared modules: [`graph`] (typed trail graph with elevation profiles, used by C1, C2,
//! C5), [`ephemeris`] (sun times and French legal time, used by C7 and C8), [`error`];
//! [`huts`] (refuges of the day plan, flagged in the packs, X21 #155);
//! [`dem`] (feature `pipeline`, pipeline side only) samples the terrain model along the graph.
//!
//! Runs on the phone (through UniFFI or a native binary, decision K0 #49): no networking,
//! no dependency on the `download` feature of `skimap-common`.

pub mod battery;
pub mod calibration;
#[cfg(feature = "pipeline")]
pub mod dem;
pub mod ephemeris;
pub mod error;
pub mod eta;
pub mod graph;
pub mod huts;
pub mod matching;
pub mod offpiste;
pub mod routing;
pub mod turnaround;
pub mod zone_eval;
