//! Avalanche terrain exposure.
//!
//! Owned tickets:
//! - P16 (#29): run the AutoATES v3.0 reference implementation (Python, MIT, with
//!   AvaFrame com4FlowPy) on Cervières as an oracle (`oracle/autoates_oracle.py`, outside
//!   the pipeline); compared cell by cell in [`exposure::oracle`].
//! - P17 (#30): Rust reimplementation (release areas, Flow-Py runout, forest, dev8 classes,
//!   three exposure classes, piste runs), see [`exposure`].
//! - P18 (#31): validation against known avalanches (CLPA; EPA behind personal sources),
//!   see [`exposure::validation`].
//! - P20 (#99): historic avalanches (CLPA republished by the DDT 05) and the piste sections
//!   crossing them, see [`historic`].
//!
//! Never phrase any output as "safe". Pipeline side: may use `skimap_common::download`.

pub mod error;
pub mod exposure;
pub mod historic;

pub use error::{Error, Result};

/// Name of this crate, checked by the crate smoke test.
pub const CRATE: &str = env!("CARGO_PKG_NAME");

#[cfg(test)]
mod tests {
    #[test]
    fn crate_name() {
        assert!(super::CRATE.starts_with("skimap-"));
    }
}
