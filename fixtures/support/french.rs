//! The guard of the English texts (VO6, #129): the French left in an English sentence of
//! the app once the proper names of the data are taken out. Shared by the English tests of
//! the assistant and of the plan.

/// French words that never appear in an English sentence of the app.
const FRENCH_WORDS: &[&str] = &[
    "le",
    "la",
    "les",
    "des",
    "du",
    "de",
    "et",
    "sur",
    "pour",
    "avec",
    "sans",
    "dans",
    "au",
    "aux",
    "une",
    "un",
    "est",
    "pas",
    "ou",
    "à",
    "vers",
    "entre",
    "piste",
    "pistes",
    "neige",
    "retour",
    "boucle",
    "boucles",
    "heure",
    "jour",
    "environ",
    "fermé",
    "ouvert",
    "ouverte",
    "inconnu",
    "carte",
    "domaine",
    "itinéraire",
    "itinéraires",
    "affluence",
    "météo",
];

/// Letters of French words that English sentences do not have.
const FRENCH_LETTERS: &[char] = &[
    'é', 'è', 'ê', 'ë', 'à', 'â', 'î', 'ï', 'ô', 'ù', 'û', 'ç', 'œ', 'É', 'È', 'À', 'Ç', '«', '»',
];

/// The French left in an English text once the proper `names` are taken out: accented
/// letters, French function words.
pub fn french_in(text: &str, names: &[String]) -> Vec<String> {
    // « off-piste » is English.
    let mut t = text.replace("off-piste", " ").replace("Off-piste", " ");
    let mut names: Vec<&String> = names.iter().filter(|n| !n.is_empty()).collect();
    names.sort_by_key(|n| std::cmp::Reverse(n.len()));
    for n in names {
        t = t.replace(n.as_str(), " ");
    }
    let mut found = Vec::new();
    for c in FRENCH_LETTERS {
        if t.contains(*c) {
            found.push(c.to_string());
        }
    }
    for w in t.split(|c: char| !c.is_alphanumeric() && c != '\'') {
        let w = w.to_lowercase();
        let w = w.trim_start_matches("d'").trim_start_matches("l'");
        if FRENCH_WORDS.contains(&w) {
            found.push(w.to_owned());
        }
    }
    found
}
