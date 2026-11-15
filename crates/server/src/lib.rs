//! `skimap-server`: distribution server of the offline packs, on the oracle k3s cluster.
//!
//! Owned tickets:
//! - S0 (#50): catalog of the sites and packs, static files of the packs with byte ranges,
//!   updates between pack versions (format in `skimap_pack::update`), health check,
//!   privacy page (K14, #122), publication of the pipeline's packs (`publish`).
//! - S1 (#62): Strava OAuth token exchange with the client secret kept here ([`strava`]).
//! - S2 (#60): non-return alert, server-side timer, e-mail to the contacts ([`alerts`]).
//!
//! Blocking, like the rest of the workspace: tiny_http with a fixed pool of worker threads,
//! SQLite (rusqlite) for the alerts, reqwest blocking for Strava, a small SMTP client over
//! rustls for the e-mails. The server only distributes and relays: the phone does all
//! the work offline.

pub mod alerts;
pub mod app;
pub mod clock;
pub mod config;
pub mod http;
pub mod privacy;
pub mod ratelimit;
pub mod store;
pub mod strava;
pub mod util;

/// Name of this crate.
pub const CRATE: &str = env!("CARGO_PKG_NAME");
