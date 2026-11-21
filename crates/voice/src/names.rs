//! Names of a site (pistes, refuges, places, zones) as spoken, and their phrase ids.

use serde::{Deserialize, Serialize};

use crate::numbers::{Gender, cardinal};

/// Difficulty colour of a nordic piste, as signposted in France.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Colour {
    /// Verte (very easy; OSM `piste:difficulty=novice`).
    Green,
    /// Bleue (easy; `easy`).
    Blue,
    /// Rouge (difficult; `intermediate`).
    Red,
    /// Noire (very difficult; `advanced`, `expert`).
    Black,
}

impl Colour {
    /// All colours.
    pub const ALL: [Colour; 4] = [Colour::Green, Colour::Blue, Colour::Red, Colour::Black];

    /// Feminine adjective, agreeing with "piste": "verte", "bleue", "rouge", "noire".
    pub fn word(self) -> &'static str {
        match self {
            Colour::Green => "verte",
            Colour::Blue => "bleue",
            Colour::Red => "rouge",
            Colour::Black => "noire",
        }
    }

    /// From an OSM `colour` value (only the four difficulty colours) or, failing that, an
    /// OSM `piste:difficulty` value.
    pub fn from_osm(colour: Option<&str>, difficulty: Option<&str>) -> Option<Self> {
        let by_colour = match colour.map(str::to_ascii_lowercase).as_deref() {
            Some("green") | Some("#008000") => Some(Colour::Green),
            Some("blue") | Some("#0000ff") => Some(Colour::Blue),
            Some("red") | Some("#ff0000") => Some(Colour::Red),
            Some("black") | Some("#000000") => Some(Colour::Black),
            _ => None,
        };
        by_colour.or(match difficulty {
            Some("novice") => Some(Colour::Green),
            Some("easy") => Some(Colour::Blue),
            Some("intermediate") => Some(Colour::Red),
            Some("advanced") | Some("expert") => Some(Colour::Black),
            _ => None,
        })
    }
}

/// A named piste with its colour, as announced: "Greffier, piste bleue."
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct PisteName {
    /// Name as in the source data (OSM `name`).
    pub name: String,
    /// Difficulty colour, if known.
    pub colour: Option<Colour>,
}

impl PisteName {
    /// New piste name.
    pub fn new(name: impl Into<String>, colour: Option<Colour>) -> Self {
        Self {
            name: name.into(),
            colour,
        }
    }

    /// Clip id, e.g. `piste.greffier.bleue`.
    pub fn clip_id(&self) -> String {
        match self.colour {
            Some(c) => format!("piste.{}.{}", slug(&self.name), c.word()),
            None => format!("piste.{}", slug(&self.name)),
        }
    }

    /// Clip text, e.g. "Greffier, piste bleue."
    pub fn clip_text(&self) -> String {
        let n = spoken_name(&self.name);
        match self.colour {
            Some(c) => format!("{n}, piste {}.", c.word()),
            None => format!("{n}."),
        }
    }
}

/// Clip id of any other name (refuge, place, zone, site), e.g. `name.refuge-du-col`.
pub fn name_clip_id(name: &str) -> String {
    format!("name.{}", slug(name))
}

/// Clip text of any other name: the spoken name with a final period.
pub fn name_clip_text(name: &str) -> String {
    format!("{}.", spoken_name(name))
}

/// ASCII slug: lower case, accents folded, every other run of characters turned into `-`.
/// Names that differ only by case, accents or punctuation share a slug (and a clip).
pub fn slug(name: &str) -> String {
    let mut out = String::new();
    let mut dash = false;
    for c in name.chars().flat_map(fold_accent) {
        let c = c.to_ascii_lowercase();
        if c.is_ascii_alphanumeric() {
            if dash && !out.is_empty() {
                out.push('-');
            }
            out.push(c);
            dash = false;
        } else {
            dash = true;
        }
    }
    if out.is_empty() {
        // Names without any Latin letter or digit: stable hexadecimal of the code points.
        out = name.chars().map(|c| format!("{:x}", c as u32)).collect();
    }
    out
}

fn fold_accent(c: char) -> Vec<char> {
    let s = match c {
        'à' | 'á' | 'â' | 'ã' | 'ä' | 'å' => "a",
        'À' | 'Á' | 'Â' | 'Ã' | 'Ä' | 'Å' => "A",
        'æ' => "ae",
        'Æ' => "AE",
        'ç' => "c",
        'Ç' => "C",
        'è' | 'é' | 'ê' | 'ë' => "e",
        'È' | 'É' | 'Ê' | 'Ë' => "E",
        'ì' | 'í' | 'î' | 'ï' => "i",
        'Ì' | 'Í' | 'Î' | 'Ï' => "I",
        'ñ' => "n",
        'Ñ' => "N",
        'ò' | 'ó' | 'ô' | 'õ' | 'ö' | 'ø' => "o",
        'Ò' | 'Ó' | 'Ô' | 'Õ' | 'Ö' | 'Ø' => "O",
        'œ' => "oe",
        'Œ' => "OE",
        'ù' | 'ú' | 'û' | 'ü' => "u",
        'Ù' | 'Ú' | 'Û' | 'Ü' => "U",
        'ý' | 'ÿ' => "y",
        'Ý' | 'Ÿ' => "Y",
        'ß' => "ss",
        _ => return vec![c],
    };
    s.chars().collect()
}

/// A name rewritten for speech:
/// - typographic apostrophes become `'` (the French pack deletes `’`), quotes are dropped;
/// - a parenthesis, a bracket or a colon becomes a comma clause (the French pack deletes
///   `:` and brackets), `" - "` becomes `", "`;
/// - digits and Roman numerals after a word become French words ("Saugy2" → "Saugy deux",
///   "La Molasse II" → "La Molasse deux");
/// - `St`, `Ste`, `Mt`, `Rte` are expanded; known acronyms (`APPB`, `GTJ`...) are kept
///   in capitals, any other word in capitals of two letters or more is capitalised
///   ("BAZAN" → "Bazan", "COTE" → "Cote");
/// - an inverted article in brackets is dropped ("Bois des Ayes (du)" → "Bois des Ayes");
/// - whitespace is collapsed and trailing punctuation removed.
pub fn spoken_name(raw: &str) -> String {
    let raw = drop_inverted_article(raw);
    let mut s = String::with_capacity(raw.len());
    for c in raw.chars() {
        match c {
            '’' | '‘' | '`' | 'ʼ' => s.push('\''),
            '"' | '“' | '”' | '„' | '«' | '»' => {}
            '(' | '[' | ':' => s.push_str(", "),
            ')' | ']' => s.push(' '),
            '_' => s.push(' '),
            c if c.is_whitespace() => s.push(' '),
            c => s.push(c),
        }
    }
    // " - " and " – " separate two places: a comma.
    for dash in [" - ", " – ", " — "] {
        s = s.replace(dash, ", ");
    }
    // Split letters from digits: "Saugy2" -> "Saugy 2".
    let mut spaced = String::with_capacity(s.len());
    let mut prev: Option<char> = None;
    for c in s.chars() {
        if let Some(p) = prev
            && ((p.is_alphabetic() && c.is_ascii_digit())
                || (p.is_ascii_digit() && c.is_alphabetic()))
        {
            spaced.push(' ');
        }
        spaced.push(c);
        prev = Some(c);
    }
    let words: Vec<&str> = spaced.split(' ').filter(|w| !w.is_empty()).collect();
    let mut out: Vec<String> = Vec::with_capacity(words.len());
    for (i, w) in words.iter().enumerate() {
        let (core, mut tail) = split_trailing_punct(w);
        // The period of an abbreviation goes with it ("St. Laurent" → "Saint Laurent"):
        // `split_trailing_punct` has moved it to the tail.
        if matches!(core, "St" | "Ste" | "Mt" | "Rte") {
            tail = tail.strip_prefix('.').unwrap_or(tail);
        }
        let spoken = if !core.is_empty() && core.chars().all(|c| c.is_ascii_digit()) {
            core.parse::<u32>()
                .ok()
                .filter(|&n| n < 1_000_000)
                .map(|n| cardinal(n, Gender::Masculine))
                .unwrap_or_else(|| core.to_owned())
        } else if i > 0 && is_roman(core) {
            cardinal(roman_value(core), Gender::Masculine)
        } else {
            match core {
                "St" => "Saint".to_owned(),
                "Ste" => "Sainte".to_owned(),
                "Mt" => "Mont".to_owned(),
                "Rte" => "Route".to_owned(),
                _ if ACRONYMS.contains(&core) => core.to_owned(),
                _ if core.chars().count() >= 2
                    && core.chars().any(char::is_alphabetic)
                    && core.chars().all(|c| !c.is_lowercase()) =>
                {
                    capitalise_word(core)
                }
                _ => core.to_owned(),
            }
        };
        out.push(format!("{spoken}{tail}"));
    }
    let mut joined = out.join(" ");
    // Commas produced by the rewriting: no space before, one after, no doubles.
    while joined.contains(" ,") {
        joined = joined.replace(" ,", ",");
    }
    while joined.contains(",,") {
        joined = joined.replace(",,", ",");
    }
    let joined = joined.trim_start_matches([',', ' ']).to_owned();
    joined
        .trim_end_matches([',', '.', ';', ':', ' '])
        .to_owned()
}

/// "Bois des Ayes (du)" (INPN style) -> "Bois des Ayes".
fn drop_inverted_article(raw: &str) -> &str {
    let t = raw.trim_end();
    if let Some(open) = t.rfind('(')
        && t.ends_with(')')
    {
        let inner = t[open + 1..t.len() - 1].trim().to_lowercase();
        let articles = [
            "du", "de", "de la", "de l'", "des", "le", "la", "les", "l'", "d'",
        ];
        if articles.contains(&inner.as_str()) {
            return t[..open].trim_end();
        }
    }
    raw
}

/// Acronyms of the source data, spelled out by the voice.
const ACRONYMS: &[&str] = &[
    "APPB", "CAF", "CEN", "ENJ", "GAEC", "GR", "GRP", "GTJ", "JOJ", "ONF", "PNR", "RNN", "RNR",
    "SIC", "SRN", "VTT", "ZNIEFF", "ZPS",
];

fn split_trailing_punct(w: &str) -> (&str, &str) {
    let end = w
        .char_indices()
        .rev()
        .take_while(|(_, c)| matches!(c, ',' | '.' | ';' | ':' | '!' | '?'))
        .last()
        .map_or(w.len(), |(i, _)| i);
    (&w[..end], &w[end..])
}

fn is_roman(w: &str) -> bool {
    matches!(
        w,
        "I" | "II" | "III" | "IV" | "V" | "VI" | "VII" | "VIII" | "IX" | "X"
    )
}

fn roman_value(w: &str) -> u32 {
    match w {
        "I" => 1,
        "II" => 2,
        "III" => 3,
        "IV" => 4,
        "V" => 5,
        "VI" => 6,
        "VII" => 7,
        "VIII" => 8,
        "IX" => 9,
        _ => 10,
    }
}

/// "BAZAN" -> "Bazan", "PIET-THOUVEREZ" -> "Piet-Thouverez".
fn capitalise_word(w: &str) -> String {
    let mut out = String::with_capacity(w.len());
    let mut start = true;
    for c in w.chars() {
        if start {
            out.extend(c.to_uppercase());
        } else {
            out.extend(c.to_lowercase());
        }
        start = !c.is_alphabetic();
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spoken_names_from_the_two_sites() {
        let cases = [
            ("Greffier", "Greffier"),
            ("Liaison Le Laus - Les Fonts", "Liaison Le Laus, Les Fonts"),
            (
                "Transjurassienne (itinéraire course uniquement)",
                "Transjurassienne, itinéraire course uniquement",
            ),
            (
                "Thomassette Racine (Cret des Ages)",
                "Thomassette Racine, Cret des Ages",
            ),
            ("Chemin Saugy2", "Chemin Saugy deux"),
            ("La Molasse II", "La Molasse deux"),
            (
                "Restaurant d’Altitude “Les Anges”",
                "Restaurant d'Altitude Les Anges",
            ),
            ("Le Chalet BAZAN", "Le Chalet Bazan"),
            ("GTJ à ski de fond", "GTJ à ski de fond"),
            (
                "GTJ ski - 12 : Le Pré Poncet - Chapelle des Bois",
                "GTJ ski, douze, Le Pré Poncet, Chapelle des Bois",
            ),
            (
                "APPB CORNICHES CALCAIRES - COTE POUTIN",
                "APPB Corniches Calcaires, Cote Poutin",
            ),
            ("Piste JOJ", "Piste JOJ"),
            (
                "Fontaine de Brunissard (Rte des Ayes)",
                "Fontaine de Brunissard, Route des Ayes",
            ),
            ("  Refuge   Napoléon. ", "Refuge Napoléon"),
            ("Bois des Ayes (du)", "Bois des Ayes"),
            ("V", "V"),
        ];
        for (raw, want) in cases {
            assert_eq!(spoken_name(raw), want, "{raw}");
        }
    }

    #[test]
    fn slugs_and_ids() {
        assert_eq!(slug("Les Prés d'Haut"), "les-pres-d-haut");
        assert_eq!(slug("Les Près d'Haut"), "les-pres-d-haut");
        assert_eq!(slug("La Faîvre"), "la-faivre");
        assert_eq!(slug("Œuf"), "oeuf");
        assert_eq!(slug("…"), "2026");
        let p = PisteName::new("Greffier", Some(Colour::Black));
        assert_eq!(p.clip_id(), "piste.greffier.noire");
        assert_eq!(p.clip_text(), "Greffier, piste noire.");
        assert_eq!(
            PisteName::new("La Combette", None).clip_text(),
            "La Combette."
        );
        assert_eq!(
            name_clip_id("Refuge des Fonts de Cervières"),
            "name.refuge-des-fonts-de-cervieres"
        );
    }

    #[test]
    fn colours_from_osm() {
        assert_eq!(Colour::from_osm(Some("blue"), None), Some(Colour::Blue));
        assert_eq!(
            Colour::from_osm(Some("purple"), Some("easy")),
            Some(Colour::Blue)
        );
        assert_eq!(
            Colour::from_osm(None, Some("advanced")),
            Some(Colour::Black)
        );
        assert_eq!(Colour::from_osm(Some("orange"), None), None);
    }
}
