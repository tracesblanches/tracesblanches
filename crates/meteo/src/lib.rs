//! Météo-France API clients.
//!
//! Owned tickets:
//! - P8 (#21): avalanche bulletin (BERA) per massif, XML parsing, Queyras extraction; Jura:
//!   Météo-France warnings and SLF link. See [`bera`].
//! - P19 (#32): hourly weather per site (wind, wind chill, visibility / fog, 0 °C
//!   isotherm, forecast snowfall). See [`hourly`].
//! - X17 (#151): the same layer from the open Météo-France AROME packages (Licence
//!   Ouverte, no key), publishable. See [`arome`].
//!
//! The API key comes from `skimap_common::config::Config` (`METEOFRANCE_API_KEY`) and must
//! never be logged. Pipeline side: uses `skimap_common::download`. Layer metadata:
//! `skimap_common::meta`.

pub mod arome;
pub mod bera;
pub mod error;
pub mod hourly;
pub mod xml;

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
