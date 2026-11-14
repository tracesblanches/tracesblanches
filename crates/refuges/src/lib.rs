//! Huts and shelters: refuges.info, OSM, Camptocamp.
//!
//! Owned tickets:
//! - P11 (#24): merge by `ref:refuges.info`, unstaffed capacity from Camptocamp, last dated
//!   comment; separate layers per licence.
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
