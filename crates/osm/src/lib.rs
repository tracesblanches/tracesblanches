//! Nordic ski trails extracted from OSM, and cross-country ski areas.
//!
//! Owned tickets:
//! - P2 (#15): extraction of nordic trails from OpenSnowMap `planet_pistes.osm.gz`.
//! - P3 (#16): grouping trails into ski areas (France first), matching names.
//!
//! Pipeline side: uses `skimap_common::download`.

pub mod error;
pub mod extract;
pub mod graph;
pub mod output;
pub mod parse;
pub mod pistes;

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
