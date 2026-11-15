//! Nordic backcountry ski (SRN) areas of France outside groomed domains: traverses
//! (GTJ, Hauts-Plateaux du Vercors…), plateaus and marked itineraries.
//!
//! Owned ticket: G5 (#131). Sources: OSM (ODbL), Geotrek instances of parks and
//! départements, Camptocamp (CC BY-SA 3.0), DATAtourisme (Licence Ouverte 2.0); the
//! groomed domains come from the census (P3, `data/out/france/domaines/`).
//!
//! Output (`data/out/france/srn/`): `srn_areas.geojson`, `srn_routes.geojson`,
//! `srn_hints.geojson`, `srn.meta.json`; see `docs/srn.md`.
//!
//! Pipeline side: uses `skimap_common::download`.

pub mod areas;
pub mod build;
pub mod camptocamp;
pub mod datatourisme;
pub mod error;
pub mod geotrek;
pub mod osm;
pub mod route;

pub use build::{SrnOutput, build, build_from};
pub use error::{Result, SrnError};

/// Name of this crate, checked by the crate smoke test.
pub const CRATE: &str = env!("CARGO_PKG_NAME");

#[cfg(test)]
mod tests {
    #[test]
    fn crate_name() {
        assert!(super::CRATE.starts_with("skimap-"));
    }
}
