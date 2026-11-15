//! Voice: French phrase bank and pre-generated clips.
//!
//! Owned tickets:
//! - VO2 (#84): parameterised French guidance and alert phrases with natural rounding
//!   ("dans deux cents mètres", "environ une heure"), chunks of at most 50 tokens split at
//!   sentence ends; one phrase per core event.
//! - VO3 (#85): pre-generated Ogg/Opus clips per pack (Pocket TTS on the workstation, outside
//!   the phone) for fixed phrases, numbers, distances, durations and all names of a pack.
//!
//! Phone side (no networking, no subprocess): [`bank`] (events → announcements),
//! [`clips`], [`mod@manifest`], [`names`], [`numbers`], [`quantity`]. Workstation side:
//! [`pack`] (generation of the voice layer), [`sources`] (names of the built layers and their
//! provenance), [`tts`] (pockettts.cpp), [`audio`] (trim,
//! level, Ogg/Opus), [`tokenizer`] and [`gguf`] (exact Pocket TTS token counts).
//!
//! An announcement is a list of clips, each a complete spoken unit. The phone plays the
//! clips of the pack in order with the pauses of the announcement; if one clip is missing
//! (a text the pack did not foresee), it speaks [`bank::Announcement::text`] with the
//! Android voice instead.

pub mod audio;
pub mod bank;
pub mod clips;
pub mod error;
pub mod gguf;
pub mod manifest;
pub mod names;
pub mod numbers;
pub mod pack;
pub mod quantity;
pub mod sources;
pub mod tokenizer;
pub mod tts;

pub use bank::{Announcement, Event};
pub use error::{Error, Result};
pub use manifest::{Manifest, PackNames, manifest, manifest_split};
pub use pack::{VoiceConfig, assemble, build, build_site};
pub use sources::{SiteNames, site_names};

/// Name of this crate, checked by the crate smoke test.
pub const CRATE: &str = env!("CARGO_PKG_NAME");

#[cfg(test)]
mod tests {
    #[test]
    fn crate_name() {
        assert!(super::CRATE.starts_with("skimap-"));
    }
}
