//! Estimated snow cover map.
//!
//! Owned tickets:
//! - P6 (#19): per pixel (massif, elevation, aspect, slope), snow depth interpolated from the
//!   Météo-France snowpack simulation (J-1); three classes per activity plus a range.
//! - P7 (#20): validation against Météo-France stations (Cervières 1614 m, Arvieux, Mouthe,
//!   La Boissaude) over a past season.
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
