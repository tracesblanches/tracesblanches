//! Voice: French and English phrases of the announcements, read by the phone's speech
//! synthesis (Android `TextToSpeech`, offline).
//!
//! Owned tickets:
//! - VO2 (#84): parameterised French guidance and alert phrases with natural rounding
//!   ("dans deux cents mètres", "environ une heure"); one phrase per core event.
//! - VO6 (#129): the same phrases in English ([`phrase`], [`en`]), same ids: the
//!   announcements are said in the app's language ([`Lang`]).
//!
//! [`bank`] (events → announcements), [`clips`] (the phrases with their values: each one
//! a complete spoken unit with an id, `turn.left.200`, `fix.arrive`), [`names`],
//! [`numbers`], [`quantity`], [`mod@vocabulary`] (the finite vocabulary on the rounding
//! grids). An announcement is a list of phrases with the pause after each; the phone
//! speaks [`bank::Announcement::text`].

pub mod bank;
pub mod clips;
pub mod en;
/// Languages of the voice and of the app's texts (VO6): [`skimap_common::lang`].
pub mod lang {
    pub use skimap_common::lang::Lang;
}
pub mod names;
pub mod numbers;
pub mod phrase;
pub mod quantity;
pub mod vocabulary;

pub use bank::{Announcement, Event};
pub use lang::Lang;
pub use vocabulary::{SiteNames, Vocabulary, vocabulary, vocabulary_in};

/// Name of this crate, checked by the crate smoke test.
pub const CRATE: &str = env!("CARGO_PKG_NAME");

#[cfg(test)]
mod tests {
    #[test]
    fn crate_name() {
        assert!(super::CRATE.starts_with("skimap-"));
    }
}
