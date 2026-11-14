//! Météo-France API clients.
//!
//! Owned tickets:
//! - P8 (#21): avalanche bulletin (BERA) per massif, XML parsing, Queyras extraction; Jura:
//!   Météo-France warnings and SLF link.
//! - P19 (#32): hourly AROME weather per site (wind, wind chill, visibility / fog, 0 °C
//!   isotherm, forecast snowfall).
//!
//! The API key comes from `skimap_common::config::Config` (`METEOFRANCE_API_KEY`) and must
//! never be logged. Pipeline side: may use `skimap_common::download`.

/// Name of this crate, checked by the crate smoke test.
pub const CRATE: &str = env!("CARGO_PKG_NAME");

#[cfg(test)]
mod tests {
    #[test]
    fn crate_name() {
        assert!(super::CRATE.starts_with("skimap-"));
    }
}
