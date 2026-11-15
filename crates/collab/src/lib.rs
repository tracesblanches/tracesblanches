//! `skimap-collab`: the collaborative and sport parts of skimap.
//!
//! Owned tickets:
//! - V8 (#71): the collaborative server's data, in SQLite ([`store`], feature `server`):
//!   no account, an anonymous device identifier rotated by the phone, stored hashed; no
//!   address; photos without metadata ([`jpeg`]) deleted with their report.
//! - V9 (#72): one-gesture reports of a fixed list of kinds ([`rules::Kind`]), no free text.
//! - V10 (#73): confirmation, expiry and reputation ([`rules`]), pure functions of the time.
//! - V11 (#74): the offline queue lives on the phone; the server takes the real time of
//!   the observation ([`rules::accept_observed`]).
//! - V12 (#75): the draft of an OSM note the user reviews and sends ([`osm_note`]).
//! - V13 (#76): challenges computed on the phone from the outings ([`challenges`],
//!   feature `phone`).
//! - V15 (#78): import of the Strava export archive, read on the phone ([`strava_export`],
//!   [`track`], feature `phone`); the outings feed the calibration (C11 `OutingObs`).

pub mod jpeg;
pub mod rules;

#[cfg(feature = "server")]
pub mod store;


/// Name of this crate.
pub const CRATE: &str = env!("CARGO_PKG_NAME");
