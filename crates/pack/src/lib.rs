//! Per-site offline pack.
//!
//! Owned tickets:
//! - P4 (#17): per-site basemap (Protomaps PMTiles extract, Plan IGN v2 vector style).
//! - P14 (#27): versioned pack format (PMTiles, GeoJSON layers, metadata: dates, sources,
//!   attributions), read by the Rust core.
//! - P15 (#28): local MapLibre GL JS viewer showing every layer of a pack.
//!
//! Read on the phone: keep the reader free of networking (no `download` feature).

/// Name of this crate, checked by the crate smoke test.
pub const CRATE: &str = env!("CARGO_PKG_NAME");

#[cfg(test)]
mod tests {
    #[test]
    fn crate_name() {
        assert!(super::CRATE.starts_with("skimap-"));
    }
}
