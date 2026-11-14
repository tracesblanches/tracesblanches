//! Protected areas with their periods, bivouac and access rules.
//!
//! Owned tickets:
//! - P9 (#22): INPN WFS, GPU easements, Biodiv'sports with `period`, Swiss hunting reserves;
//!   a single layer with periods.
//! - P10 (#23): rule schema (period, stay on routes, bivouac, fire, night access, core zones)
//!   and sourced rules for both sites.
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
