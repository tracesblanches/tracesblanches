//! Avalanche terrain exposure.
//!
//! Owned tickets:
//! - P16 (#29): run the auto-ATES reference implementation (GPL, Python) on Cervières as an
//!   oracle; reference layer and runtime measured. The oracle is tooling, never the hot path.
//! - P17 (#30): Rust reimplementation (release areas, runout, forest), compared with P16.
//! - P18 (#31): validation against known avalanches (CLPA, EPA) in the Queyras.
//!
//! Never phrase any output as "safe". Pipeline side: may use `skimap_common::download`.

/// Name of this crate, checked by the crate smoke test.
pub const CRATE: &str = env!("CARGO_PKG_NAME");

#[cfg(test)]
mod tests {
    #[test]
    fn crate_name() {
        assert!(super::CRATE.starts_with("skimap-"));
    }
}
