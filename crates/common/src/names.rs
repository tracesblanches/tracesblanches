//! Comparison of piste names across sources (OSM, Nordic France widget, town hall page):
//! lower-case words without accents, articles, generic words and lengths.

/// Lower-case words of a name without accents, articles and generic words
/// (« Le Chemin du Sioux » → `sioux`; « L'Izoard » → `izoard`; « Le Gibera 1.5km » →
/// `gibera`).
pub fn name_key(name: &str) -> Vec<String> {
    const STOP: &[&str] = &[
        "le", "la", "les", "l", "de", "du", "des", "d", "chemin", "piste", "route", "col", "et",
        "a", "au", "aux", "en",
    ];
    let folded: String = name
        .chars()
        .map(|c| match c {
            'à' | 'â' | 'ä' | 'À' | 'Â' => 'a',
            'é' | 'è' | 'ê' | 'ë' | 'É' | 'È' | 'Ê' => 'e',
            'î' | 'ï' | 'Î' => 'i',
            'ô' | 'ö' | 'Ô' => 'o',
            'ù' | 'û' | 'ü' | 'Û' => 'u',
            'ç' | 'Ç' => 'c',
            c if c.is_alphanumeric() => c.to_ascii_lowercase(),
            _ => ' ',
        })
        .collect();
    folded
        .split_whitespace()
        .filter(|w| !STOP.contains(w))
        // « 1.5km », « 16km », « 3 », « km »: lengths written in the names of the mairie.
        .filter(|w| !w.chars().next().is_some_and(|c| c.is_ascii_digit()) && *w != "km")
        .map(str::to_owned)
        .collect()
}

/// Whether two names designate the same piste: same significant words, or every word of
/// the shorter name in the longer one.
pub fn same_name(a: &str, b: &str) -> bool {
    name_score(a, b).is_some()
}

/// Strength of a [`same_name`] match, to choose among several candidates: `None` when the
/// names differ, else twice the number of shared words, plus one when the keys are equal
/// (« Izoard 16km » is closer to « L'Izoard » than to « Piéton multiactivités Izoard »).
pub fn name_score(a: &str, b: &str) -> Option<usize> {
    let (ka, kb) = (name_key(a), name_key(b));
    if ka.is_empty() || kb.is_empty() {
        return None;
    }
    let (short, long) = if ka.len() <= kb.len() {
        (&ka, &kb)
    } else {
        (&kb, &ka)
    };
    short
        .iter()
        .all(|w| long.contains(w))
        .then(|| 2 * short.len() + usize::from(ka == kb))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_match_across_sources() {
        assert_eq!(name_key("Le Chemin du Sioux"), vec!["sioux"]);
        assert_eq!(name_key("L’Aigalan 8km"), vec!["aigalan"]);
        assert!(same_name("Le Sioux", "Le Chemin du Sioux"));
        assert!(same_name("L'Izoard", "Izoard 16km"));
        assert!(same_name("Le Gibera 1.5km", "Le Gibera"));
        assert!(!same_name("Le Savare", "Le Cancel"));
        assert!(!same_name("Piéton les Fonts", "L'Aigalan"));
        assert!(!same_name("", "Le Cancel"));
    }

    #[test]
    fn scores_prefer_the_closest_name() {
        let izoard = name_score("Izoard 16km", "L'Izoard").unwrap();
        let pieton = name_score("Izoard 16km", "Piéton multiactivités Izoard").unwrap();
        assert!(izoard > pieton);
        let by_road = "Liaison Cervières-les Aittes par route 3.5 km";
        let by_quarry = "Liaison Cervières-les Aittes par carrière 2.5 km";
        let nf_road = "Liaison les Aittes par la route";
        let nf_quarry = "Liaison les Aittes par la carrière";
        assert!(
            name_score(by_quarry, nf_quarry).unwrap() > name_score(by_quarry, nf_road).unwrap()
        );
        assert!(name_score(by_road, nf_road).is_some());
        assert!(name_score(by_road, nf_quarry).is_none());
    }
}
