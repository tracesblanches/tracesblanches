//! Voice commands without a language model (V3 #66, VO5 #87, A11 #98).
//!
//! What the speech recognition heard is classified into the closed list of the core's
//! functions ([`Intent`]): normalisation ([`normalize::words`]: case, accents, punctuation,
//! elisions), a lexicon of French and English words and phrases mapped to concepts
//! ([`lexicon`]: synonyms, the spellings speech recognition writes), an edit distance for
//! the words heard slightly wrong, and a score per command over the concepts found (key
//! words, with words that rule a command out). Below [`MIN_SCORE`], or when two commands
//! are too close ([`MIN_MARGIN`]), nothing is chosen: the app says « je n'ai pas compris »
//! and the questions it knows ([`questions`]).
//!
//! The answer is never made here: the app asks the core (next direction, time left,
//! « ramène-moi »…) and says the phrases of the voice bank. No number, place or safety
//! advice comes out of this crate. A language model stays an option that is not enabled
//! (A11): it could only replace [`classify`], with the same closed output.

pub mod lexicon;
pub mod measure;
pub mod normalize;

use lexicon::Concept::{self, *};

/// Language of a question.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Lang {
    /// French.
    Fr,
    /// English.
    En,
}

/// The closed list of commands: each is one function of the core.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Intent {
    /// « Où suis-je ? »: on or off the route, distance and time left.
    WhereAmI,
    /// « Temps restant ? »: distance and time left (`say_remaining`).
    TimeLeft,
    /// « Prochaine direction ? » (`say_next`).
    NextDirection,
    /// « Ramène-moi » (`back_start`).
    BringMeBack,
    /// « Heure de demi-tour ? »: the turnaround countdown or time.
    TurnaroundTime,
    /// « Signale : trace faite » (community report).
    ReportTracksSet,
    /// « Signale : piste damée » (community report).
    ReportGroomed,
    /// « Combien de batterie ? »: level and autonomy.
    Battery,
    /// « Répète »: the last thing said, again.
    Repeat,
}

impl Intent {
    /// Every command, in the order of [`questions`].
    pub const ALL: [Intent; 9] = [
        Intent::WhereAmI,
        Intent::TimeLeft,
        Intent::NextDirection,
        Intent::BringMeBack,
        Intent::TurnaroundTime,
        Intent::ReportTracksSet,
        Intent::ReportGroomed,
        Intent::Battery,
        Intent::Repeat,
    ];

    /// Stable name (data sets, logs, Kotlin).
    pub fn name(self) -> &'static str {
        match self {
            Intent::WhereAmI => "where",
            Intent::TimeLeft => "time_left",
            Intent::NextDirection => "next_direction",
            Intent::BringMeBack => "bring_back",
            Intent::TurnaroundTime => "turnaround",
            Intent::ReportTracksSet => "report_tracks",
            Intent::ReportGroomed => "report_groomed",
            Intent::Battery => "battery",
            Intent::Repeat => "repeat",
        }
    }

    /// The command of a [`Intent::name`].
    pub fn from_name(s: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|i| i.name() == s)
    }

    /// The question that asks it, as listed after « je n'ai pas compris ».
    pub fn question(self, lang: Lang) -> &'static str {
        match (self, lang) {
            (Intent::WhereAmI, Lang::Fr) => "Où suis-je ?",
            (Intent::TimeLeft, Lang::Fr) => "Temps restant ?",
            (Intent::NextDirection, Lang::Fr) => "Prochaine direction ?",
            (Intent::BringMeBack, Lang::Fr) => "Ramène-moi.",
            (Intent::TurnaroundTime, Lang::Fr) => "Heure de demi-tour ?",
            (Intent::ReportTracksSet, Lang::Fr) => "Signale : trace faite.",
            (Intent::ReportGroomed, Lang::Fr) => "Signale : piste damée.",
            (Intent::Battery, Lang::Fr) => "Combien de batterie ?",
            (Intent::Repeat, Lang::Fr) => "Répète.",
            (Intent::WhereAmI, Lang::En) => "Where am I?",
            (Intent::TimeLeft, Lang::En) => "Time left?",
            (Intent::NextDirection, Lang::En) => "Next direction?",
            (Intent::BringMeBack, Lang::En) => "Take me back.",
            (Intent::TurnaroundTime, Lang::En) => "Turnaround time?",
            (Intent::ReportTracksSet, Lang::En) => "Report: tracks set.",
            (Intent::ReportGroomed, Lang::En) => "Report: trail groomed.",
            (Intent::Battery, Lang::En) => "How much battery?",
            (Intent::Repeat, Lang::En) => "Repeat.",
        }
    }

    /// Weights of the concepts for this command: a positive weight is a key word, a
    /// negative one rules the command out (« quelle heure » is not « temps restant »,
    /// « demi-tour » is not « ramène-moi »). Each concept counts once.
    fn weights(self) -> &'static [(Concept, f32)] {
        match self {
            Intent::WhereAmI => &[
                (Where, 0.7),
                (SelfRef, 0.6),
                (Here, 0.6),
                (Position, 1.5),
                (Direction, -1.0),
                (Side, -1.0),
                (Next, -0.8),
                (Back, -0.6),
                (Time, -0.5),
                (Report, -1.0),
            ],
            Intent::TimeLeft => &[
                (Time, 0.8),
                (Remain, 1.0),
                (Distance, 1.0),
                (Arrival, 0.8),
                (HowMuch, 0.3),
                (When, 0.3),
                (Back, -0.5),
                (Did, -1.2),
                (Turnaround, -2.0),
                (Battery, -2.0),
                (Repeat, -2.0),
            ],
            Intent::NextDirection => &[
                (Direction, 1.0),
                (Next, 1.0),
                (Side, 1.3),
                (Where, 0.3),
                (Time, -0.5),
                (Distance, -0.3),
                (Back, -0.6),
                (Turnaround, -2.0),
                (Report, -1.0),
            ],
            Intent::BringMeBack => &[
                (Back, 1.2),
                (Guide, 0.6),
                (When, -0.8),
                (Time, -0.5),
                (Turnaround, -1.5),
            ],
            Intent::TurnaroundTime => &[(Turnaround, 2.0), (When, 0.5)],
            Intent::ReportTracksSet => {
                &[(Tracks, 1.4), (Report, 0.8), (Done, 0.4), (Groomed, -1.0)]
            }
            Intent::ReportGroomed => &[(Groomed, 1.6), (Report, 0.8), (Done, 0.2)],
            Intent::Battery => &[(Battery, 2.0)],
            Intent::Repeat => &[(Repeat, 2.0), (Excuse, 1.0)],
        }
    }
}

/// Below this score no command is chosen.
pub const MIN_SCORE: f32 = 1.0;
/// The best command must lead the second by this much, else nothing is chosen.
pub const MIN_MARGIN: f32 = 0.15;
/// Weight of a word outside the closed list (weather, tomorrow, music…), for every command.
const OUT_OF_DOMAIN: f32 = -1.5;

/// What [`classify`] found.
#[derive(Debug, Clone, PartialEq)]
pub struct Classification {
    /// The command, `None` when not understood.
    pub intent: Option<Intent>,
    /// Language of the question (the default when nothing tells).
    pub lang: Lang,
    /// Score of the best command.
    pub score: f32,
    /// The second best command and its score.
    pub runner_up: Option<(Intent, f32)>,
    /// Concepts found, in order of the words.
    pub concepts: Vec<Concept>,
}

/// Concepts of the words of `text`, with the language votes (French, English).
fn concepts(words: &[String]) -> (Vec<Concept>, usize, usize) {
    let mut found = Vec::new();
    let (mut fr, mut en) = (0, 0);
    let mut i = 0;
    while i < words.len() {
        let hit = lexicon::LEXICON
            .iter()
            .filter_map(|e| {
                let parts: Vec<&str> = e.0.split(' ').collect();
                let n = parts.len();
                (i + n <= words.len()
                    && parts
                        .iter()
                        .zip(&words[i..i + n])
                        .all(|(p, w)| same_start(w, p) && normalize::matches(w, p)))
                .then(|| (n, exact(&words[i..i + n], &parts), e))
            })
            // Longest phrase, then an exact match before a near one.
            .max_by_key(|(n, ex, _)| (*n, *ex));
        match hit {
            Some((n, _, (_, lang, cs))) => {
                found.extend_from_slice(cs);
                match lang {
                    Some(Lang::Fr) => fr += n,
                    Some(Lang::En) => en += n,
                    None => {}
                }
                i += n;
            }
            None => {
                let w = words[i].as_str();
                fr += usize::from(lexicon::FR_WORDS.contains(&w));
                en += usize::from(lexicon::EN_WORDS.contains(&w));
                i += 1;
            }
        }
    }
    (found, fr, en)
}

/// Fuzzy matches keep the first letter: « where » is not « there », « on » not « ou ».
fn same_start(heard: &str, word: &str) -> bool {
    heard.as_bytes().first() == word.as_bytes().first()
}

fn exact(words: &[String], parts: &[&str]) -> bool {
    words.iter().zip(parts).all(|(w, p)| w == p)
}

/// The command asked by `text` (a transcription), in `default` language when nothing
/// tells.
pub fn classify(text: &str, default: Lang) -> Classification {
    let words = normalize::words(text);
    let (found, fr, en) = concepts(&words);
    let lang = match fr.cmp(&en) {
        std::cmp::Ordering::Greater => Lang::Fr,
        std::cmp::Ordering::Less => Lang::En,
        std::cmp::Ordering::Equal => default,
    };
    let ood = found.contains(&OutOfDomain);
    let mut scores: Vec<(Intent, f32)> = Intent::ALL
        .into_iter()
        .map(|it| {
            let s: f32 = it
                .weights()
                .iter()
                .filter(|(c, _)| found.contains(c))
                .map(|(_, w)| w)
                .sum();
            (it, if ood { s + OUT_OF_DOMAIN } else { s })
        })
        .collect();
    scores.sort_by(|a, b| b.1.total_cmp(&a.1));
    let (best, score) = scores[0];
    let runner_up = scores.get(1).copied();
    let clear = runner_up.is_none_or(|(_, s)| score - s >= MIN_MARGIN);
    Classification {
        intent: (score >= MIN_SCORE && clear).then_some(best),
        lang,
        score,
        runner_up,
        concepts: found,
    }
}

/// The questions understood, in `lang`: said after « je n'ai pas compris ».
pub fn questions(lang: Lang) -> Vec<&'static str> {
    Intent::ALL.iter().map(|i| i.question(lang)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_listed_questions_are_understood() {
        for lang in [Lang::Fr, Lang::En] {
            for it in Intent::ALL {
                let c = classify(it.question(lang), Lang::Fr);
                assert_eq!(c.intent, Some(it), "{:?}: {:?}", it.question(lang), c);
                assert_eq!(c.lang, lang, "{:?}", it.question(lang));
            }
        }
    }

    #[test]
    fn names_round_trip() {
        for it in Intent::ALL {
            assert_eq!(Intent::from_name(it.name()), Some(it));
        }
        assert_eq!(Intent::from_name("unknown"), None);
    }

    #[test]
    fn nothing_heard_is_not_understood() {
        for t in ["", "   ", "euh", "?!"] {
            assert_eq!(classify(t, Lang::Fr).intent, None, "{t:?}");
        }
    }

    #[test]
    fn speech_recognition_near_misses() {
        assert_eq!(
            classify("Ramènes moi", Lang::Fr).intent,
            Some(Intent::BringMeBack)
        );
        assert_eq!(classify("baterie", Lang::Fr).intent, Some(Intent::Battery));
        assert_eq!(
            classify("signal piste damer", Lang::Fr).intent,
            Some(Intent::ReportGroomed)
        );
    }

    #[test]
    fn close_but_different_questions() {
        // The time of day is not the time left; the turnaround is not « ramène-moi ».
        assert_eq!(classify("Quelle heure est-il ?", Lang::Fr).intent, None);
        assert_eq!(
            classify("When should I turn back?", Lang::En).intent,
            Some(Intent::TurnaroundTime)
        );
        assert_eq!(
            classify("Je tourne où ?", Lang::Fr).intent,
            Some(Intent::NextDirection)
        );
        assert_eq!(classify("Où skier demain ?", Lang::Fr).intent, None);
    }

    #[test]
    fn language_follows_the_words() {
        assert_eq!(classify("Where am I?", Lang::Fr).lang, Lang::En);
        assert_eq!(classify("Où suis-je ?", Lang::En).lang, Lang::Fr);
        assert_eq!(classify("position", Lang::En).lang, Lang::En);
    }
}

#[cfg(test)]
mod properties {
    use super::*;
    use proptest::prelude::*;

    proptest! {
        /// Any text classifies without panicking, to a command of the list or none,
        /// whatever the default language.
        #[test]
        fn any_text_is_classified(text in "\\PC{0,80}") {
            let c = classify(&text, Lang::Fr);
            prop_assert!(c.score.is_finite());
            if let Some(i) = c.intent {
                prop_assert!(Intent::ALL.contains(&i));
                prop_assert!(c.score >= MIN_SCORE);
            }
        }

        /// Case and punctuation do not change the command.
        #[test]
        fn case_and_punctuation_do_not_matter(i in 0usize..9, upper in any::<bool>(), p in "[ ?!.,:;]{0,3}") {
            let it = Intent::ALL[i];
            for lang in [Lang::Fr, Lang::En] {
                let q = it.question(lang);
                let t = if upper { q.to_uppercase() } else { q.to_lowercase() };
                let c = classify(&format!("{p}{t}{p}"), lang);
                prop_assert_eq!(c.intent, Some(it));
            }
        }
    }
}
