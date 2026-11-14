//! Trail status snapshot from the ENJ (Espace Nordique Jurassien) API.
//!
//! Owned tickets:
//! - P13 (#26): scheduled polling of the ENJ Geotrek API (field `route`: 1 closed, 2 open;
//!   `labels`), once for all users; one week of history in season.
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
