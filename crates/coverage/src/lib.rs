//! Areas with no mobile network at all, not even for 112 (ARCEP).
//!
//! Owned tickets:
//! - P12 (#25): union of the voice coverage of the four operators, difference with the site
//!   area; quarterly refresh. Expected: Cervières 51 km², Chapelle-des-Bois 18 km².
//!
//! ARCEP polygons are in Lambert-93 (EPSG:2154): use `skimap_common::crs`.
//! Pipeline side: may use `skimap_common::download`.

/// Name of this crate, checked by the crate smoke test.
pub const CRATE: &str = env!("CARGO_PKG_NAME");

#[cfg(test)]
mod tests {
    #[test]
    fn crate_name() {
        assert!(super::CRATE.starts_with("skimap-"));
    }
}
