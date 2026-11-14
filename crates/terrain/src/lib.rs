//! IGN elevation per site: slope and aspect.
//!
//! Owned tickets:
//! - P5 (#18): download RGE ALTI 5 m or LiDAR HD over both sites, compute slope and aspect on
//!   a `skimap_common::grid::Grid`, compare with the IGN `SLOPES.MOUNTAIN` layer.
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
