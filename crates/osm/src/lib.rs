//! Nordic ski trails extracted from OSM, and cross-country ski areas.
//!
//! Owned tickets:
//! - P2 (#15): extraction of nordic trails from OpenSnowMap `planet_pistes.osm.gz`.
//! - P3 (#16): grouping trails into ski areas (France first), matching names.
//!
//! Pipeline side: uses `skimap_common::download`.

// Unit tests write their inputs directly; the code itself goes through
// `skimap_common::output` (clippy.toml, #164).
#![cfg_attr(test, allow(clippy::disallowed_methods))]

pub mod census;
pub mod domains;
pub mod error;
pub mod extract;
pub mod graph;
pub mod local;
pub mod parse;
pub mod pistes;
pub mod srn_ways;

pub use error::{OsmError, Result};

/// Name of this crate, checked by the crate smoke test.
pub const CRATE: &str = env!("CARGO_PKG_NAME");

#[cfg(test)]
mod tests {
    #[test]
    fn crate_name() {
        assert!(super::CRATE.starts_with("skimap-"));
    }
}
