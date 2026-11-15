//! What the plan says: the announcements of the phrase bank (VO2, `skimap_voice::bank`),
//! including the plan's own fixed phrases ([`PLAN_PHRASES`], ids `fix.plan-*`), and the
//! check of their clips against the pack ([`ClipCheck::missing`]: those go to the
//! fallback voice of the phone).
//!
//! Never « sans danger », never « sûr » (tested on every phrase).

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};
use skimap_core::graph::{Difficulty, EdgeAttrs};
use skimap_voice::bank::{Announcement, Pause, Segment};
use skimap_voice::clips::{Clip, Fixed};
use skimap_voice::names::{Colour, PisteName, slug};
use skimap_voice::pack::VoiceManifest;

/// The plan's phrases in the bank (`Fixed::Plan*`, ids `fix.plan-*`): switch to a plan B,
/// refusal, recomputed route, going home, end of the outing.
pub const PLAN_PHRASES: [Fixed; 8] = [
    Fixed::PlanClosedAhead,
    Fixed::PlanClosedNoDetour,
    Fixed::PlanShortcut,
    Fixed::PlanShortened,
    Fixed::PlanRerouted,
    Fixed::PlanKept,
    Fixed::PlanHomeNow,
    Fixed::PlanFinished,
];

/// Announcement of a fixed phrase alone, as a message of its own (long pause after).
pub fn announce(phrase: Fixed) -> Announcement {
    Announcement {
        segments: vec![Segment {
            clip: phrase.clip(),
            pause: Pause::Long,
        }],
    }
}

/// Colour of a difficulty, as the voice packs name the pistes.
pub fn colour(d: Option<Difficulty>) -> Option<Colour> {
    d.map(|d| match d {
        Difficulty::Novice => Colour::Green,
        Difficulty::Easy => Colour::Blue,
        Difficulty::Intermediate => Colour::Red,
        Difficulty::Advanced => Colour::Black,
    })
}

/// Clip coverage of announcements against the clips of a pack.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClipCheck {
    /// Voice manifest read (`<site>/voice/manifest.json`), if any.
    pub manifest: Option<String>,
    /// Every clip the announcements need.
    pub required: BTreeSet<String>,
    /// Clips absent from the pack (or with another text): those announcements go to the
    /// fallback voice of the phone.
    pub missing: BTreeSet<String>,
}

/// Names as spoken by a pack: piste clips by slug, read from the voice manifest.
#[derive(Debug, Clone, Default)]
pub struct Voice {
    /// Clip id → text of the pack.
    clips: BTreeMap<String, String>,
    /// Slug → piste clip ids of the pack.
    pistes: BTreeMap<String, Vec<String>>,
    /// Manifest path.
    source: Option<String>,
}

impl Voice {
    /// Voice of a pack manifest.
    pub fn new(m: &VoiceManifest, source: impl Into<String>) -> Self {
        let mut clips = BTreeMap::new();
        let mut pistes: BTreeMap<String, Vec<String>> = BTreeMap::new();
        for c in &m.clips {
            clips.insert(c.id.clone(), c.text.clone());
            if let Some(rest) = c.id.strip_prefix("piste.") {
                let slug = rest.split('.').next().unwrap_or(rest).to_owned();
                pistes.entry(slug).or_default().push(c.id.clone());
            }
        }
        Self {
            clips,
            pistes,
            source: Some(source.into()),
        }
    }

    /// The piste name of an edge as the pack speaks it: the first part of the name (« A /
    /// B ») that has a clip, with the colour of the clip when the pack has it under another
    /// colour; else the first part with the colour of the edge's difficulty.
    pub fn piste(&self, attrs: &EdgeAttrs) -> Option<PisteName> {
        let name = attrs.name.as_ref()?;
        let col = colour(attrs.difficulty);
        for part in name.split(" / ") {
            let p = PisteName::new(part.trim(), col);
            if let Some(q) = self.as_packed(&p) {
                return Some(q);
            }
            if let Some(ids) = self.pistes.get(&slug(part)) {
                // Another colour in the pack: the pack's (from the relations).
                for id in ids {
                    let c = Colour::ALL
                        .into_iter()
                        .find(|c| id.ends_with(&format!(".{}", c.word())));
                    if let Some(q) = self.as_packed(&PisteName::new(part.trim(), c)) {
                        return Some(q);
                    }
                }
            }
        }
        name.split(" / ")
            .next()
            .map(|p| PisteName::new(p.trim(), col))
    }

    /// The name with the spelling of the pack's clip of the same id (names that differ by
    /// case or accents share one clip, « La pinatelle » is spoken « La Pinatelle »).
    fn as_packed(&self, p: &PisteName) -> Option<PisteName> {
        let text = self.clips.get(&p.clip_id())?;
        if *text == p.clip_text() {
            return Some(p.clone());
        }
        let suffix = match p.colour {
            Some(c) => format!(", piste {}.", c.word()),
            None => ".".into(),
        };
        let q = PisteName::new(text.strip_suffix(&suffix)?, p.colour);
        (q.clip_text() == *text).then_some(q)
    }

    /// Whether the pack has this clip with this text.
    pub fn has(&self, c: &Clip) -> bool {
        self.clips.get(&c.id).is_some_and(|t| *t == c.text)
    }

    /// Adds the clips of an announcement to a coverage check.
    pub fn check(&self, a: &Announcement, into: &mut ClipCheck) {
        if into.manifest.is_none() {
            into.manifest = self.source.clone();
        }
        for s in &a.segments {
            into.required.insert(s.clip.id.clone());
            if !self.has(&s.clip) {
                into.missing.insert(s.clip.id.clone());
            }
        }
    }
}

/// Words never said or written by the plan.
pub const NEVER: [&str; 4] = ["sans danger", "sûr", "sûre", "sécurisé"];

/// Whether a French text avoids the words of [`NEVER`] (as whole words).
pub fn never_safe(text: &str) -> bool {
    let t = text.to_lowercase();
    NEVER.iter().all(|w| {
        t.match_indices(w).all(|(i, _)| {
            let before = t[..i].chars().next_back();
            let after = t[i + w.len()..].chars().next();
            before.is_some_and(char::is_alphabetic) || after.is_some_and(char::is_alphabetic)
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use skimap_voice::tokenizer::PocketText;

    #[test]
    fn plan_phrases_are_the_bank_plan_phrases() {
        // Every `fix.plan-*` phrase of the bank, once, and nothing else.
        let bank: BTreeSet<String> = Fixed::ALL
            .iter()
            .map(|f| f.clip().id)
            .filter(|id| id.starts_with("fix.plan-"))
            .collect();
        let ours: BTreeSet<String> = PLAN_PHRASES.iter().map(|f| f.clip().id).collect();
        assert_eq!(ours.len(), PLAN_PHRASES.len());
        assert_eq!(ours, bank);
        for p in PLAN_PHRASES {
            let c = p.clip();
            assert!(c.text.ends_with('.'), "{}", c.text);
            assert!(never_safe(&c.text), "{}", c.text);
            assert_eq!(announce(p).segments.len(), 1);
        }
    }

    #[test]
    #[ignore = "needs the French Pocket TTS model (docs/voix.md)"]
    fn plan_phrases_fit_the_chunk_budget() {
        // Exact Pocket TTS counts with the model's tokenizer (`VoiceConfig::from_env`).
        let cfg = skimap_voice::VoiceConfig::from_env().unwrap();
        let t = PocketText::from_gguf(&cfg.tts.model).unwrap();
        for p in PLAN_PHRASES {
            let n = t.count(&p.clip().text);
            eprintln!("{:?}: {n} tokens", p.clip().id);
            assert!(n <= skimap_voice::pack::MAX_TOKENS, "{p:?}: {n} tokens");
        }
    }

    #[test]
    fn never_safe_words() {
        assert!(never_safe("Itinéraire recalculé."));
        assert!(never_safe("Assurez-vous du BERA, c'est obscur."));
        assert!(!never_safe("Itinéraire sûr."));
        assert!(!never_safe("Une sortie sans danger"));
    }
}
