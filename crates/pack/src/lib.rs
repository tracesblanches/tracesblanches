//! Per-site offline pack.
//!
//! Owned tickets:
//! - P4 (#17): per-site basemaps: Plan IGN v2 vector tiles and an OSM (Protomaps) extract,
//!   each a PMTiles archive with its MapLibre style, sprites and fonts ([`build::plan_ign`],
//!   [`build::osm`]).
//! - P14 (#27): versioned pack format ([`manifest`]), written by [`build::build`] from the
//!   pipeline outputs and read by [`reader::Pack`] on the phone.
//! - P15 (#28): local MapLibre GL JS viewer, `tools/viewer/index.html`.
//! - P21 (#100): export of a pack for Locus Map and OsmAnd ([`export`]).
//! - K2 (#52), S0 (#50), E1 (#125): pack updates file by file and their single-file
//!   bundles ([`update`], [`update::apply_bundle`]), synchronisation plan with the server
//!   ([`sync`]); phone side, no network.
//!
//! - X20 (#154): France basemap, light and shared by every site ([`france`],
//!   [`build::france`]).
//!
//! Without the `pipeline` feature the crate is the reader only: no HTTP stack, no SQLite.

// Unit tests write their inputs directly; the code itself goes through
// `skimap_common::output` (clippy.toml, #164).
#![cfg_attr(test, allow(clippy::disallowed_methods))]

pub mod error;
pub mod france;
pub mod manifest;
pub mod mvt;
pub mod patch;
pub mod pmtiles_v3;
pub mod png;
pub mod reader;
pub mod sync;
pub mod tiles;
pub mod update;
pub mod view;

#[cfg(feature = "pipeline")]
pub mod build;
#[cfg(feature = "pipeline")]
pub mod export;
#[cfg(feature = "pipeline")]
pub mod serve;

pub use error::{Error, Result};
pub use manifest::{FORMAT, FORMAT_VERSION, Manifest};
pub use reader::{Pack, TileArchive};

/// Name of this crate, checked by the crate smoke test.
pub const CRATE: &str = env!("CARGO_PKG_NAME");

#[cfg(test)]
mod tests {
    #[test]
    fn crate_name() {
        assert!(super::CRATE.starts_with("skimap-"));
    }
}
