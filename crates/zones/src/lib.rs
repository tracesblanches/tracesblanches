//! Protected areas with their periods, bivouac and access rules.
//!
//! Owned tickets:
//! - P9 (#22): INPN WFS, GPU easements, Biodiv'sports with `period`, Swiss hunting reserves;
//!   a single layer with periods ([`layer`], built by `build`).
//! - P10 (#23): rule schema (period, stay on routes, bivouac, fire with stoves, night access,
//!   core zones, parcel exceptions) and sourced rules for both sites ([`rules`],
//!   `rules/rules.json`).
//!
//! - G4 (#120): national rules on every site and the status of the local rules
//!   ([`local`]).
//!
//! Without features the crate only holds the layer and rule schema, used on the phone by
//! `skimap_core::zone_eval` (C8). The `pipeline` feature adds the sources and
//! `build(site, &DataLayout, &Downloader)`, which writes `data/out/<site>/zones/`.

pub mod error;
pub mod fields;
pub mod geom;
pub mod layer;
pub mod local;
pub mod rules;

#[cfg(feature = "pipeline")]
mod build;
#[cfg(feature = "pipeline")]
pub mod sources;

#[cfg(feature = "pipeline")]
pub use build::{LAYER, RuleResolution, ZonesReport, build, build_with};
pub use error::{Error, Result};
pub use layer::{Months, Zone, ZoneKind, ZoneLayer, ZoneProps};
pub use rules::{Effect, Period, Rule, RuleSet, Topic};

pub use geo;
/// Re-exported so that users of the schema (dates of [`rules::InForce`], geometries of
/// [`layer::Zone`]) need no direct dependency.
pub use jiff;

/// Name of this crate, checked by the crate smoke test.
pub const CRATE: &str = env!("CARGO_PKG_NAME");

#[cfg(test)]
mod tests {
    #[test]
    fn crate_name() {
        assert!(super::CRATE.starts_with("skimap-"));
    }
}
