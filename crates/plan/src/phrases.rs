//! What the plan says: the announcements of the phrase bank (VO2, `skimap_voice::bank`),
//! including the plan's own fixed phrases ([`PLAN_PHRASES`], ids `fix.plan-*`), and the
//! piste names they say ([`piste`]).
//!
//! Never « sans danger », never « sûr » (tested on every phrase).

use skimap_core::graph::{Difficulty, EdgeAttrs};
use skimap_voice::bank::{Announcement, Pause, Segment};
use skimap_voice::clips::Fixed;
use skimap_voice::names::{Colour, PisteName};

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

/// Colour of a difficulty, as the announcements name the pistes.
pub fn colour(d: Option<Difficulty>) -> Option<Colour> {
    d.map(|d| match d {
        Difficulty::Novice => Colour::Green,
        Difficulty::Easy => Colour::Blue,
        Difficulty::Intermediate => Colour::Red,
        Difficulty::Advanced => Colour::Black,
    })
}

/// The piste name of an edge as the announcements say it: the first part of the name
/// (« A / B ») with the colour of the edge's difficulty.
pub fn piste(attrs: &EdgeAttrs) -> Option<PisteName> {
    let name = attrs.name.as_ref()?;
    name.split(" / ")
        .next()
        .map(|p| PisteName::new(p.trim(), colour(attrs.difficulty)))
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
    use std::collections::BTreeSet;

    #[test]
    fn plan_phrases_are_the_bank_plan_phrases() {
        // Every `fix.plan-*` phrase of the bank, once, and nothing else: the plan's own
        // (J1 to J3) and those of the proposals (C11).
        let bank: BTreeSet<String> = Fixed::ALL
            .iter()
            .map(|f| f.clip().id)
            .filter(|id| id.starts_with("fix.plan-"))
            .collect();
        let all: Vec<Fixed> = PLAN_PHRASES
            .into_iter()
            .chain(crate::pace::PROPOSAL_PHRASES)
            .chain(crate::pace::PROFILE_PHRASES)
            .collect();
        let ours: BTreeSet<String> = all.iter().map(|f| f.clip().id).collect();
        assert_eq!(ours.len(), all.len());
        assert_eq!(ours.len(), 16);
        assert_eq!(ours, bank);
        for p in all {
            let c = p.clip();
            // « Je vous propose la boucle courte, » is continued by « retour vers … ».
            let end = if p.continued() { "," } else { "." };
            // The profile proposal asks a question (X14).
            assert!(
                c.text.ends_with(end)
                    || (p == Fixed::PlanProfileMismatch && c.text.ends_with(" ?")),
                "{}",
                c.text
            );
            assert!(never_safe(&c.text), "{}", c.text);
            assert_eq!(announce(p).segments.len(), 1);
        }
        let texts: Vec<String> = crate::pace::PROPOSAL_PHRASES
            .iter()
            .map(|f| f.clip().text)
            .collect();
        assert_eq!(
            texts,
            [
                "Vous allez moins vite que prévu.",
                "Vous allez moins vite que prévu, avec plus de pauses.",
                "Vous faites plus de pauses que prévu.",
                "Je vous propose la boucle courte,",
                "Je vous propose la sortie la plus proche,",
                "Je vous propose de garder la boucle,",
                "Proposition acceptée.",
            ]
        );
    }

    #[test]
    fn never_safe_words() {
        assert!(never_safe("Itinéraire recalculé."));
        assert!(never_safe("Assurez-vous du BERA, c'est obscur."));
        assert!(!never_safe("Itinéraire sûr."));
        assert!(!never_safe("Une sortie sans danger"));
    }
}
