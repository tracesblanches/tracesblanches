//! Languages of the app (VO6): French and English, the one language type of the core.
//!
//! The texts of the core (briefing, recap, cards, rules...) and the voice follow the
//! phone's language: French on a French phone, English for every other language
//! ([`Lang::of_phone`]). The clip ids of the voice do not depend on the language
//! (`turn.left.200`, `fix.arrive`); each language has its own clips, shipped in a voice
//! pack per site and language (`skimap_voice::phrase` gives an announcement in another
//! language, clip by clip).

use serde::{Deserialize, Serialize};

/// A language of the app and of the voice.
#[derive(
    Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize,
)]
#[serde(rename_all = "snake_case")]
pub enum Lang {
    /// French.
    Fr,
    /// English (the default of the app outside French-speaking phones).
    #[default]
    En,
}

impl Lang {
    /// Every language of the app.
    pub const ALL: [Lang; 2] = [Lang::Fr, Lang::En];

    /// ISO 639-1 code: `fr`, `en`.
    pub fn code(self) -> &'static str {
        match self {
            Lang::Fr => "fr",
            Lang::En => "en",
        }
    }

    /// From an ISO 639-1 code or a locale tag (`fr`, `fr-FR`, `en_GB`); `None` for any
    /// other language.
    pub fn from_code(code: &str) -> Option<Self> {
        let base = code
            .split(['-', '_'])
            .next()
            .unwrap_or_default()
            .to_ascii_lowercase();
        match base.as_str() {
            "fr" => Some(Lang::Fr),
            "en" => Some(Lang::En),
            _ => None,
        }
    }

    /// Language of the app for a phone language: French on a French phone, English for
    /// every other language (the default).
    pub fn of_phone(code: &str) -> Self {
        match Self::from_code(code) {
            Some(Lang::Fr) => Lang::Fr,
            _ => Lang::En,
        }
    }

    /// Name of the voice layer of a site in the pipeline outputs: `voice` (French, the
    /// first layer, kept where it was) and `voice-<code>` for the others.
    pub fn layer(self) -> String {
        match self {
            Lang::Fr => "voice".to_owned(),
            l => format!("voice-{}", l.code()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn codes() {
        assert_eq!(Lang::from_code("fr-FR"), Some(Lang::Fr));
        assert_eq!(Lang::from_code("en_GB"), Some(Lang::En));
        assert_eq!(Lang::from_code("de"), None);
        assert_eq!(Lang::of_phone("de-DE"), Lang::En);
        assert_eq!(Lang::of_phone("fr-CA"), Lang::Fr);
        assert_eq!(Lang::of_phone(""), Lang::En);
        assert_eq!(Lang::default(), Lang::En);
        assert_eq!(Lang::Fr.layer(), "voice");
        assert_eq!(Lang::En.layer(), "voice-en");
        for l in Lang::ALL {
            assert_eq!(Lang::from_code(l.code()), Some(l));
        }
    }
}
