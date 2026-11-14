//! Protected areas with their periods, bivouac and access rules.
//!
//! Owned tickets:
//! - P9 (#22): INPN WFS, GPU easements, Biodiv'sports with `period`, Swiss hunting reserves;
//!   a single layer with periods ([`layer`]).
//! - P10 (#23): rule schema (period, stay on routes, bivouac, fire with stoves, night access,
//!   core zones, parcel exceptions) and sourced rules for both sites ([`rules`],
//!   `rules/rules.json`).

pub mod error;
pub mod layer;
pub mod rules;

pub use error::{Error, Result};
pub use layer::{Months, Zone, ZoneKind, ZoneLayer, ZoneProps};
pub use rules::{Effect, Period, Rule, RuleSet, Topic};

/// Re-exported so that users of the schema (dates of [`rules::InForce`], geometries of
/// [`layer::Zone`]) need no direct dependency.
pub use chrono;
pub use geo;

/// Name of this crate, checked by the crate smoke test.
pub const CRATE: &str = env!("CARGO_PKG_NAME");

#[cfg(test)]
mod tests {
    #[test]
    fn crate_name() {
        assert!(super::CRATE.starts_with("skimap-"));
    }
}
