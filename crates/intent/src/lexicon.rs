//! Words and phrases of the commands, French and English, mapped to concepts.
//!
//! A heard word stands for a lexicon word when it is the same or within the edit tolerance
//! of [`crate::normalize::matches`] (same first letter). Phrases (« demi tour », « how
//! long », « encore une fois ») are matched before single words, longest first, and their
//! words are not looked at again.

use crate::Lang;

/// What a word says, for the scoring of [`crate::classify`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Concept {
    /// « où », « where ».
    Where,
    /// The speaker: « je suis », « on », « nous », « am I », « we ».
    SelfRef,
    /// « ici », « here ».
    Here,
    /// « position », « localisation », « endroit », « location ».
    Position,
    /// « temps », « durée », « long », « time ».
    Time,
    /// « reste », « restant », « encore », « left », « remaining », « to go ».
    Remain,
    /// « loin », « kilomètres », « distance », « far ».
    Distance,
    /// « arrivée », « arriver », « de retour », « get there », « arrival ».
    Arrival,
    /// « prochaine », « suivante », « next ».
    Next,
    /// « direction », « tourner », « virage », « chemin », « aller », « turn », « way », « go ».
    Direction,
    /// « gauche », « droite », « left », « right ».
    Side,
    /// « ramène », « rentrer », « retour », « départ », « back », « home », « start ».
    Back,
    /// « guide », « take », « bring », « lead ».
    Guide,
    /// « demi-tour », « turnaround », « turn around », « turn back ».
    Turnaround,
    /// « quand », « heure », « limite », « when », « what time », « until ».
    When,
    /// « signale », « signalement », « report ».
    Report,
    /// « trace », « traces », « tracks ».
    Tracks,
    /// « damée », « dameuse », « damage », « groomed », « groomer ».
    Groomed,
    /// « faite », « done », « set ».
    Done,
    /// What was done so far: « fait », « did » (« combien de kilomètres j'ai fait »).
    Did,
    /// « batterie », « autonomie », « battery », « charge ».
    Battery,
    /// « répète », « redis », « encore une fois », « repeat », « say again ».
    Repeat,
    /// « pardon », « sorry »: a repeat when nothing else is asked.
    Excuse,
    /// « combien », « how much », « how many ».
    HowMuch,
    /// Outside the closed list: weather, tomorrow, music, calls, wax, places…
    OutOfDomain,
}

use Concept::*;

/// (words, language, concepts). `None`: the word is the same in both languages.
pub(crate) type Entry = (&'static str, Option<Lang>, &'static [Concept]);

const FR: Option<Lang> = Some(Lang::Fr);
const EN: Option<Lang> = Some(Lang::En);

/// Phrases first (several words), then single words.
pub(crate) const LEXICON: &[Entry] = &[
    // --- Phrases ---
    ("demi tour", FR, &[Turnaround]),
    ("de retour", FR, &[Arrival]),
    ("par ou", FR, &[Direction]),
    ("encore une fois", FR, &[Repeat]),
    ("quel temps", FR, &[OutOfDomain]),
    ("week end", None, &[OutOfDomain]),
    ("turn around", EN, &[Turnaround]),
    ("turn back", EN, &[Turnaround]),
    ("u turn", EN, &[Turnaround]),
    ("which way", EN, &[Direction]),
    ("how long", EN, &[Time]),
    ("how far", EN, &[Distance]),
    ("how much", EN, &[HowMuch]),
    ("how many", EN, &[HowMuch]),
    ("what time", EN, &[When]),
    ("to go", EN, &[Remain]),
    ("get there", EN, &[Arrival]),
    ("get back", EN, &[Arrival]),
    ("say again", EN, &[Repeat]),
    ("one more time", EN, &[Repeat]),
    // --- Where am I ---
    ("ou", FR, &[Where]),
    ("where", EN, &[Where]),
    ("je", FR, &[SelfRef]),
    ("suis", FR, &[SelfRef]),
    ("sommes", FR, &[SelfRef]),
    ("nous", FR, &[SelfRef]),
    ("on", FR, &[SelfRef]),
    ("trouve", FR, &[SelfRef]),
    ("situe", FR, &[SelfRef, Position]),
    ("i", EN, &[SelfRef]),
    ("am", EN, &[SelfRef]),
    ("we", EN, &[SelfRef]),
    ("are", EN, &[SelfRef]),
    ("ici", FR, &[Here]),
    ("here", EN, &[Here]),
    ("position", None, &[Position]),
    ("localisation", FR, &[Position]),
    ("endroit", FR, &[Position]),
    ("location", EN, &[Position]),
    ("located", EN, &[Position]),
    // --- Time left ---
    ("temps", FR, &[Time]),
    ("duree", FR, &[Time]),
    ("long", None, &[Time]),
    ("longer", EN, &[Time, Remain]),
    ("time", EN, &[Time]),
    ("minutes", None, &[Time]),
    ("reste", FR, &[Remain]),
    ("restant", FR, &[Remain]),
    ("restante", FR, &[Remain]),
    ("restants", FR, &[Remain]),
    ("restantes", FR, &[Remain]),
    ("encore", FR, &[Remain]),
    ("beaucoup", FR, &[Remain]),
    ("left", EN, &[Remain, Side]),
    ("remaining", EN, &[Remain]),
    ("remain", EN, &[Remain]),
    ("further", EN, &[Distance, Remain]),
    ("loin", FR, &[Distance]),
    ("distance", None, &[Distance]),
    ("kilometres", None, &[Distance]),
    ("kilometre", None, &[Distance]),
    ("kilometers", EN, &[Distance]),
    ("km", None, &[Distance]),
    ("metres", None, &[Distance]),
    ("far", EN, &[Distance]),
    ("miles", EN, &[Distance]),
    ("arrive", None, &[Arrival]),
    ("arriver", FR, &[Arrival]),
    ("arrivee", FR, &[Arrival]),
    ("arrivera", FR, &[Arrival]),
    ("arrivons", FR, &[Arrival]),
    ("arrival", EN, &[Arrival]),
    ("finish", EN, &[Arrival]),
    ("fin", FR, &[Arrival]),
    ("end", EN, &[Arrival]),
    ("destination", None, &[Arrival]),
    ("combien", FR, &[HowMuch]),
    // --- Next direction ---
    ("prochaine", FR, &[Next]),
    ("prochain", FR, &[Next]),
    ("suivante", FR, &[Next]),
    ("suivant", FR, &[Next]),
    ("suite", FR, &[Next]),
    ("ensuite", FR, &[Next]),
    ("apres", FR, &[Next]),
    ("next", EN, &[Next]),
    ("direction", None, &[Direction]),
    ("directions", None, &[Direction]),
    ("tourner", FR, &[Direction]),
    ("tourne", FR, &[Direction]),
    ("virage", FR, &[Direction]),
    ("chemin", FR, &[Direction]),
    ("aller", FR, &[Direction]),
    ("vais", FR, &[Direction]),
    ("va", FR, &[Direction]),
    ("allons", FR, &[Direction]),
    ("prendre", FR, &[Direction]),
    ("prends", FR, &[Direction]),
    ("bifurcation", FR, &[Direction]),
    ("intersection", None, &[Direction]),
    ("carrefour", FR, &[Direction]),
    ("croisement", FR, &[Direction]),
    ("embranchement", FR, &[Direction]),
    ("turn", EN, &[Direction]),
    ("way", EN, &[Direction]),
    ("go", EN, &[Direction]),
    ("junction", EN, &[Direction]),
    ("fork", EN, &[Direction]),
    ("instruction", None, &[Direction]),
    ("gauche", FR, &[Side]),
    ("droite", FR, &[Side]),
    ("right", EN, &[Side]),
    // --- Bring me back ---
    ("ramene", FR, &[Back, Guide]),
    ("ramener", FR, &[Back, Guide]),
    ("ramenez", FR, &[Back, Guide]),
    ("rentrer", FR, &[Back]),
    ("rentre", FR, &[Back]),
    ("rentrons", FR, &[Back]),
    ("retour", FR, &[Back]),
    ("retourner", FR, &[Back]),
    ("revenir", FR, &[Back]),
    ("depart", FR, &[Back]),
    ("maison", FR, &[Back]),
    ("parking", None, &[Back]),
    ("voiture", FR, &[Back]),
    ("back", EN, &[Back]),
    ("home", EN, &[Back]),
    ("return", EN, &[Back]),
    ("start", EN, &[Back]),
    ("car", EN, &[Back]),
    ("guide", None, &[Guide]),
    ("emmene", FR, &[Guide]),
    ("conduis", FR, &[Guide]),
    ("take", EN, &[Guide]),
    ("bring", EN, &[Guide]),
    ("lead", EN, &[Guide]),
    // --- Turnaround time ---
    ("demitour", FR, &[Turnaround]),
    ("turnaround", EN, &[Turnaround]),
    ("quand", FR, &[When]),
    ("heure", FR, &[When]),
    ("limite", FR, &[When]),
    ("when", EN, &[When]),
    ("latest", EN, &[When]),
    ("deadline", EN, &[When]),
    ("until", EN, &[When]),
    // --- Reports (community) ---
    ("signale", FR, &[Report]),
    ("signaler", FR, &[Report]),
    ("signal", None, &[Report]),
    ("signalement", FR, &[Report]),
    ("report", EN, &[Report]),
    ("trace", FR, &[Tracks]),
    ("traces", FR, &[Tracks]),
    ("tracee", FR, &[Tracks]),
    ("tracees", FR, &[Tracks]),
    ("track", EN, &[Tracks]),
    ("tracks", EN, &[Tracks]),
    ("damee", FR, &[Groomed]),
    ("dame", FR, &[Groomed]),
    ("damees", FR, &[Groomed]),
    ("damer", FR, &[Groomed]),
    ("damage", FR, &[Groomed]),
    ("dameuse", FR, &[Groomed]),
    ("groomed", EN, &[Groomed]),
    ("groom", EN, &[Groomed]),
    ("grooming", EN, &[Groomed]),
    ("groomer", EN, &[Groomed]),
    ("faite", FR, &[Done]),
    ("faites", FR, &[Done]),
    ("fait", FR, &[Done, Did]),
    ("done", EN, &[Done]),
    ("set", EN, &[Done]),
    ("made", EN, &[Done]),
    ("did", EN, &[Did]),
    // --- Battery ---
    ("batterie", FR, &[Battery]),
    ("battery", EN, &[Battery]),
    ("autonomie", FR, &[Battery]),
    ("pourcentage", FR, &[Battery]),
    ("percentage", EN, &[Battery]),
    ("percent", EN, &[Battery]),
    ("charge", None, &[Battery]),
    // --- Repeat ---
    ("repete", FR, &[Repeat]),
    ("repeter", FR, &[Repeat]),
    ("repetez", FR, &[Repeat]),
    ("redis", FR, &[Repeat]),
    ("redire", FR, &[Repeat]),
    ("recommence", FR, &[Repeat]),
    ("repeat", EN, &[Repeat]),
    ("again", EN, &[Repeat]),
    ("pardon", None, &[Excuse]),
    ("sorry", EN, &[Excuse]),
    // --- Outside the closed list ---
    ("demain", FR, &[OutOfDomain]),
    ("tomorrow", EN, &[OutOfDomain]),
    ("meteo", FR, &[OutOfDomain]),
    ("weather", EN, &[OutOfDomain]),
    ("pleuvoir", FR, &[OutOfDomain]),
    ("pluie", FR, &[OutOfDomain]),
    ("rain", EN, &[OutOfDomain]),
    ("neiger", FR, &[OutOfDomain]),
    ("neige", FR, &[OutOfDomain]),
    ("snow", EN, &[OutOfDomain]),
    ("vent", FR, &[OutOfDomain]),
    ("wind", EN, &[OutOfDomain]),
    ("skier", FR, &[OutOfDomain]),
    ("ski", None, &[OutOfDomain]),
    ("musique", FR, &[OutOfDomain]),
    ("music", EN, &[OutOfDomain]),
    ("appelle", FR, &[OutOfDomain]),
    ("call", EN, &[OutOfDomain]),
    ("message", None, &[OutOfDomain]),
    ("text", EN, &[OutOfDomain]),
    ("sms", None, &[OutOfDomain]),
    ("blague", FR, &[OutOfDomain]),
    ("joke", EN, &[OutOfDomain]),
    ("fart", FR, &[OutOfDomain]),
    ("wax", EN, &[OutOfDomain]),
    ("lampe", FR, &[OutOfDomain]),
    ("flashlight", EN, &[OutOfDomain]),
    ("ouvre", FR, &[OutOfDomain]),
    ("open", EN, &[OutOfDomain]),
    ("carte", FR, &[OutOfDomain]),
    ("map", EN, &[OutOfDomain]),
    ("denivele", FR, &[OutOfDomain]),
    ("elevation", EN, &[OutOfDomain]),
    ("refuge", None, &[OutOfDomain]),
    ("bonjour", FR, &[OutOfDomain]),
    ("salut", FR, &[OutOfDomain]),
    ("hello", EN, &[OutOfDomain]),
    ("morning", EN, &[OutOfDomain]),
    ("merci", FR, &[OutOfDomain]),
    ("thank", EN, &[OutOfDomain]),
    ("thanks", EN, &[OutOfDomain]),
];

/// Function words that tell the language without carrying a command.
pub(crate) const FR_WORDS: &[&str] = &[
    "le", "la", "les", "de", "du", "des", "un", "une", "est", "c", "qu", "que", "quel", "quelle",
    "moi", "me", "ma", "mon", "tu", "il", "elle", "et", "a", "au", "s", "plait", "vient", "d",
    "etre", "dois", "faire", "peux", "veux", "voudrais", "jusqu", "l", "j", "t", "ce", "pour",
    "dans", "avant", "piste", "passee", "comment", "quoi", "donne", "dis", "niveau",
];
/// Function words that tell the language without carrying a command.
pub(crate) const EN_WORDS: &[&str] = &[
    "the", "a", "is", "what", "s", "my", "me", "to", "do", "should", "must", "have", "you", "can",
    "could", "please", "that", "it", "of", "at", "now", "exactly", "right", "tell", "us", "which",
    "trail", "how", "will", "there", "want", "just", "freshly", "classic", "or", "re",
];
