//! Languages of the voice (VO6): French and English.
//!
//! The clip ids do not depend on the language (`turn.left.200`, `fix.arrive`,
//! `piste.greffier.bleue`); each language has its own texts and its own clips, generated
//! by the Pocket TTS model of that language and shipped in a voice pack per site and
//! language. The phrase bank builds announcements in French; [`crate::phrase`] gives the
//! same announcement in another language, clip by clip.

use serde::{Deserialize, Serialize};

/// A language of the voice.
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
    /// Every language with a voice.
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
