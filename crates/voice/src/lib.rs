//! Voice: French phrase bank and pre-generated clips.
//!
//! Owned tickets:
//! - VO2 (#84): parameterised French guidance and alert phrases with natural rounding
//!   ("dans deux cents mètres", "environ une heure"), chunks of at most 50 tokens split at
//!   sentence ends; one phrase per core event.
//! - VO3 (#85): pre-generated Ogg/Opus clips per pack (Pocket TTS on the workstation, outside
//!   the phone) for fixed phrases, numbers, distances, durations and all names of a pack.
//!
//! Runs on the phone (phrase bank): no networking.

/// Name of this crate, checked by the crate smoke test.
pub const CRATE: &str = env!("CARGO_PKG_NAME");

#[cfg(test)]
mod tests {
    #[test]
    fn crate_name() {
        assert!(super::CRATE.starts_with("skimap-"));
    }
}
