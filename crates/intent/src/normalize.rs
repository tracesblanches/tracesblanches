//! Text normalisation: what speech recognition writes (case, accents, punctuation,
//! elisions, hyphens) folded to plain lowercase ASCII words.

/// Lowercase ASCII words of `text`: accents removed (`é` → `e`, `œ` → `oe`), apostrophes,
/// hyphens and every other non-alphanumeric character are separators.
pub fn words(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    for c in text.chars().flat_map(char::to_lowercase) {
        match fold(c) {
            Some(s) => cur.push_str(s),
            None if c.is_ascii_alphanumeric() => cur.push(c),
            None => {
                if !cur.is_empty() {
                    out.push(std::mem::take(&mut cur));
                }
            }
        }
    }
    if !cur.is_empty() {
        out.push(cur);
    }
    out
}

/// ASCII form of a lowercase accented Latin letter (French, and the few others STT writes).
fn fold(c: char) -> Option<&'static str> {
    Some(match c {
        'à' | 'â' | 'ä' | 'á' | 'ã' | 'å' => "a",
        'ç' => "c",
        'é' | 'è' | 'ê' | 'ë' => "e",
        'î' | 'ï' | 'í' | 'ì' => "i",
        'ô' | 'ö' | 'ó' | 'ò' | 'õ' => "o",
        'ù' | 'û' | 'ü' | 'ú' => "u",
        'ÿ' | 'ý' => "y",
        'ñ' => "n",
        'œ' => "oe",
        'æ' => "ae",
        _ => return None,
    })
}

/// Levenshtein distance between two ASCII words, stopping early above `max` (returns
/// `max + 1` then).
pub fn distance(a: &str, b: &str, max: usize) -> usize {
    let (a, b) = (a.as_bytes(), b.as_bytes());
    if a.len().abs_diff(b.len()) > max {
        return max + 1;
    }
    let mut prev: Vec<usize> = (0..=b.len()).collect();
    let mut cur = vec![0; b.len() + 1];
    for (i, &ca) in a.iter().enumerate() {
        cur[0] = i + 1;
        let mut row_min = cur[0];
        for (j, &cb) in b.iter().enumerate() {
            let sub = prev[j] + usize::from(ca != cb);
            cur[j + 1] = sub.min(prev[j + 1] + 1).min(cur[j] + 1);
            row_min = row_min.min(cur[j + 1]);
        }
        if row_min > max {
            return max + 1;
        }
        std::mem::swap(&mut prev, &mut cur);
    }
    prev[b.len()].min(max + 1)
}

/// Edits allowed between a heard word and a lexicon word of this length: none under 5
/// letters (« où », « je », « set » are too close to other words), one from 5, two from 9.
pub fn tolerance(len: usize) -> usize {
    match len {
        0..=4 => 0,
        5..=8 => 1,
        _ => 2,
    }
}

/// The heard word `heard` stands for the lexicon word `word`.
pub fn matches(heard: &str, word: &str) -> bool {
    if heard == word {
        return true;
    }
    let tol = tolerance(word.len().min(heard.len()));
    tol > 0 && distance(heard, word, tol) <= tol
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn folds_accents_punctuation_elisions() {
        assert_eq!(words("Où suis-je ?"), ["ou", "suis", "je"]);
        assert_eq!(
            words("À quelle heure faire demi-tour ?"),
            ["a", "quelle", "heure", "faire", "demi", "tour"]
        );
        assert_eq!(words("C’est l'arrivée…"), ["c", "est", "l", "arrivee"]);
        assert_eq!(
            words("What's my battery at?"),
            ["what", "s", "my", "battery", "at"]
        );
        assert_eq!(words("  "), Vec::<String>::new());
        assert_eq!(words("Œuvre 12 km"), ["oeuvre", "12", "km"]);
    }

    #[test]
    fn distance_is_levenshtein_bounded() {
        assert_eq!(distance("ramene", "ramenes", 2), 1);
        assert_eq!(distance("batterie", "baterie", 2), 1);
        assert_eq!(distance("kitten", "sitting", 5), 3);
        assert_eq!(distance("kitten", "sitting", 1), 2);
        assert_eq!(distance("", "abc", 5), 3);
        assert_eq!(distance("abc", "abc", 0), 0);
    }

    #[test]
    fn short_words_must_be_exact() {
        assert!(!matches("ou", "on"));
        assert!(!matches("set", "sel"));
        assert!(matches("damer", "damee"));
        assert!(matches("ramenes", "ramene"));
        assert!(!matches("restaurant", "restant"));
    }
}
