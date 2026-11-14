//! Nordic ski trails extracted from OSM, and cross-country ski areas.
//!
//! Owned tickets:
//! - P2 (#15): extraction of nordic trails from OpenSnowMap `planet_pistes.osm.gz`
//!   (ways and relations `piste:type=nordic` / `hike`, names and colours from relations,
//!   `piste:grooming`, `piste:oneway`), GeoJSON and graph for both sites.
//! - P3 (#16): grouping trails into ski areas (France first), matching with Nordic France /
//!   SkiRadar names.
//!
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
