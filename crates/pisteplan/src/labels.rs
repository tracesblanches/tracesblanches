//! Labels of the plan: names on the map (text layer or OCR), the legend of the pistes
//! (name, ink, length, departure) and name comparison.

use crate::image::{Rgb, lab};
use crate::input::TextBox;
use crate::spec::PlanSpec;

/// Lower-case ASCII words of a text (accents removed, punctuation as spaces).
pub fn fold(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        let c = match c {
            'à' | 'â' | 'ä' | 'á' | 'À' | 'Â' | 'Ä' => 'a',
            'é' | 'è' | 'ê' | 'ë' | 'É' | 'È' | 'Ê' | 'Ë' => 'e',
            'î' | 'ï' | 'Î' | 'Ï' => 'i',
            'ô' | 'ö' | 'Ô' | 'Ö' => 'o',
            'ù' | 'û' | 'ü' | 'Ù' | 'Û' | 'Ü' => 'u',
            'ç' | 'Ç' => 'c',
            '’' | '\'' | '-' | '/' | '.' | ',' | '(' | ')' | ':' | '•' | '«' | '»' => ' ',
            c => c,
        };
        if c.is_alphanumeric() {
            out.extend(c.to_lowercase());
        } else {
            out.push(' ');
        }
    }
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Levenshtein distance between two strings (characters).
pub fn levenshtein(a: &str, b: &str) -> usize {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    let mut prev: Vec<usize> = (0..=b.len()).collect();
    let mut cur = vec![0; b.len() + 1];
    for i in 1..=a.len() {
        cur[0] = i;
        for j in 1..=b.len() {
            let sub = prev[j - 1] + usize::from(a[i - 1] != b[j - 1]);
            cur[j] = sub.min(prev[j] + 1).min(cur[j - 1] + 1);
        }
        std::mem::swap(&mut prev, &mut cur);
    }
    prev[b.len()]
}

/// Similarity of two folded names, 0 to 1 (1 − normalised edit distance).
pub fn similarity(a: &str, b: &str) -> f64 {
    let (a, b) = (fold(a), fold(b));
    let n = a.chars().count().max(b.chars().count());
    if n == 0 {
        return 0.0;
    }
    1.0 - levenshtein(&a, &b) as f64 / n as f64
}

/// Splits a trailing elevation: « MOUTHE - 938 m » → (« MOUTHE », 938), « Lasseron 2703 m »
/// → (« Lasseron », 2703).
pub fn split_elevation(text: &str) -> (String, Option<f64>) {
    let t = text.trim();
    let Some(body) = t
        .strip_suffix(" m")
        .or_else(|| t.strip_suffix("m"))
        .or_else(|| t.strip_suffix(" M"))
    else {
        return (t.to_owned(), None);
    };
    let body = body.trim_end();
    let digits_start = body
        .char_indices()
        .rev()
        .take_while(|(_, c)| c.is_ascii_digit() || *c == ' ' || *c == '.')
        .last()
        .map(|(i, _)| i);
    match digits_start {
        Some(i) => {
            let num: String = body[i..].chars().filter(char::is_ascii_digit).collect();
            let name = body[..i].trim_end().trim_end_matches(['-', '–']).trim_end();
            match num.parse::<f64>() {
                Ok(v) if (100.0..5000.0).contains(&v) && !name.is_empty() => {
                    (name.to_owned(), Some(v))
                }
                _ => (t.to_owned(), None),
            }
        }
        None => (t.to_owned(), None),
    }
}

/// Splits a trailing length: « Chalet Pin 11,0 km » → (« Chalet Pin », 11.0).
pub fn split_length(text: &str) -> Option<(String, f64)> {
    let t = text.trim();
    let body = t.strip_suffix("km")?.trim_end();
    let i = body.rfind(' ')?;
    let num = body[i + 1..].replace(',', ".");
    let v: f64 = num.parse().ok()?;
    let name = body[..i].trim().trim_end_matches(['•', '-', '·']).trim();
    (!name.is_empty()).then(|| (name.to_owned(), v))
}

/// An entry of the plan's legend of pistes.
#[derive(Debug, Clone, PartialEq)]
pub struct LegendEntry {
    /// Name.
    pub name: String,
    /// Ink of the dash (index into `spec.inks`).
    pub ink: Option<usize>,
    /// Length printed, km.
    pub length_km: Option<f64>,
    /// Departure (title of the box or sector) listing the piste.
    pub departure: Option<String>,
    /// Box of the entry, page points.
    pub rect: [f64; 4],
}

/// Nearest ink of the legend dash left of a text box: majority over the pixels of the
/// band `[x0 − 16 pt, x0 − 1 pt] × [y0, y1]` within 1.5 × the ink threshold.
pub fn dash_ink(spec: &PlanSpec, img: &Rgb, rect: [f64; 4], pt_per_px: f64) -> Option<usize> {
    let refs: Vec<(usize, [f32; 3])> = spec
        .inks
        .iter()
        .enumerate()
        .flat_map(|(i, ink)| ink.refs.iter().map(move |&c| (i, lab(c))))
        .collect();
    let to_px = |v: f64| (v / pt_per_px).round().max(0.0) as usize;
    let (x0, x1) = (to_px(rect[0] - 16.0), to_px(rect[0] - 1.0));
    let (y0, y1) = (to_px(rect[1]), to_px(rect[3]));
    let mut votes = vec![0usize; spec.inks.len()];
    for y in y0..y1.min(img.h) {
        for x in x0..x1.min(img.w) {
            let l = lab(img.get(x, y));
            let mut best = (usize::MAX, f32::MAX);
            for &(i, r) in &refs {
                let d =
                    ((l[0] - r[0]).powi(2) + (l[1] - r[1]).powi(2) + (l[2] - r[2]).powi(2)).sqrt();
                if d <= 1.5 * spec.inks[i].max_de && d < best.1 {
                    best = (i, d);
                }
            }
            if best.0 != usize::MAX {
                votes[best.0] += 1;
            }
        }
    }
    let (i, &n) = votes.iter().enumerate().max_by_key(|(_, n)| **n)?;
    (n >= 4).then_some(i)
}

/// Title of a departure box, « Le Glacier - 1080 m », possibly followed by a map label
/// the text layer put on the same line (« … 1080 m Bois Cernois Roz »).
pub fn box_title(text: &str) -> Option<(String, f64)> {
    let t: Vec<&str> = text.split_whitespace().collect();
    (1..t.len().saturating_sub(1)).find_map(|i| {
        let d = t[i];
        if t[i + 1] != "m" || !(3..=4).contains(&d.len()) || !d.chars().all(|c| c.is_ascii_digit())
        {
            return None;
        }
        let name = t[..i].join(" ");
        let name = name.trim_end_matches(['-', '–']).trim_end();
        (!name.is_empty()).then(|| (name.to_owned(), d.parse().unwrap_or(0.0)))
    })
}

/// Joins text lines on the same baseline separated by a small gap (the text layer cuts
/// « Le Gy 2,6 » from « km »).
pub fn join_baselines(lines: &[TextBox]) -> Vec<TextBox> {
    let mut out: Vec<TextBox> = Vec::new();
    let mut used = vec![false; lines.len()];
    for i in 0..lines.len() {
        if used[i] {
            continue;
        }
        let mut cur = lines[i].clone();
        loop {
            let next = (0..lines.len()).find(|&j| {
                !used[j]
                    && j != i
                    && (lines[j].rect[1] - cur.rect[1]).abs() < 0.5
                    && (lines[j].rect[3] - cur.rect[3]).abs() < 0.5
                    && lines[j].rect[0] >= cur.rect[2] - 0.5
                    && lines[j].rect[0] - cur.rect[2] < 3.0
            });
            match next {
                Some(j) => {
                    used[j] = true;
                    cur.text = format!("{} {}", cur.text, lines[j].text);
                    cur.rect[2] = lines[j].rect[2];
                }
                None => break,
            }
        }
        used[i] = true;
        out.push(cur);
    }
    out
}

/// Ink named by the colour word of a legend line « Piste verte : le Gibera » (the word
/// right after « Piste »; a colour word inside a name, « Combe Verte », names nothing).
pub fn colour_word(spec: &PlanSpec, text: &str) -> Option<usize> {
    let f = fold(text);
    let mut words = f.split(' ');
    if words.next() != Some("piste") {
        return None;
    }
    let w = match words.next()? {
        "de" => words.next()?,
        w => w,
    };
    let name = [
        ("verte", "green"),
        ("bleue", "blue"),
        ("rouge", "red"),
        ("noire", "black"),
        ("liaison", "orange"),
    ]
    .iter()
    .find(|(c, _)| *c == w)?
    .1;
    spec.inks.iter().position(|i| i.name == name)
}

/// Legend of a plan whose boxes list « Name 12,3 km » lines under a « Departure - 1088 m »
/// title (ENJ plans), or « Piste verte : le Gibera • 1,5 km » lines (Cervières).
pub fn legend(spec: &PlanSpec, lines: &[TextBox], img: &Rgb, pt_per_px: f64) -> Vec<LegendEntry> {
    let lines = &join_baselines(lines);
    let titles: Vec<(&TextBox, String)> = lines
        .iter()
        .filter_map(|l| box_title(&l.text).map(|(name, _)| (l, name)))
        .collect();
    let mut out: Vec<LegendEntry> = Vec::new();
    for (i, l) in lines.iter().enumerate() {
        let mut text = l.text.clone();
        let mut rect = l.rect;
        // « (Multi-activités) 11,0 km » continues the line above.
        if text.starts_with('(')
            && let Some(prev) = lines[..i].iter().rev().find(|p| {
                (p.rect[0] - l.rect[0]).abs() < 2.0
                    && l.rect[1] - p.rect[3] < 4.0
                    && l.rect[1] > p.rect[1]
            })
        {
            text = format!("{} {}", prev.text, text);
            rect = [
                prev.rect[0],
                prev.rect[1],
                rect[2].max(prev.rect[2]),
                rect[3],
            ];
        }
        let Some((raw_name, km)) = split_length(&text) else {
            continue;
        };
        // « Piste verte : le Gibera » → « le Gibera ».
        let name = match raw_name.split_once(':') {
            Some((_, n)) => n.trim().to_owned(),
            None => raw_name.clone(),
        };
        if name.chars().filter(|c| c.is_alphabetic()).count() < 3 {
            continue;
        }
        let ink = colour_word(spec, &raw_name).or_else(|| dash_ink(spec, img, rect, pt_per_px));
        // Departure: nearest title above, left-aligned with the box (within 30 pt).
        let departure = titles
            .iter()
            .filter(|(t, _)| {
                t.rect[3] <= rect[1] + 1.0
                    && rect[1] - t.rect[3] < 140.0
                    && rect[0] - t.rect[0] > -5.0
                    && rect[0] - t.rect[0] < 110.0
            })
            .min_by(|a, b| (rect[1] - a.0.rect[3]).total_cmp(&(rect[1] - b.0.rect[3])))
            .map(|(_, n)| n.clone());
        out.push(LegendEntry {
            name,
            ink,
            length_km: Some(km),
            departure,
            rect,
        });
    }
    out
}

/// Colour words of a legend line and the ink they name.
const COLOUR_WORDS: &[(&str, &str)] = &[
    ("verte", "green"),
    ("bleue", "blue"),
    ("rouge", "red"),
    ("noire", "black"),
    ("lineaire", "purple"),
    ("liaison", "orange"),
];

/// Parses an OCR reading of a legend line « Piste <couleur> : <nom> • <longueur> km »:
/// the colour word is the token most similar to a known one (≥ 0.75); the name is what
/// follows, without the length. Returns (ink name, name, length km).
pub fn parse_colour_line(text: &str) -> Option<(&'static str, String, Option<f64>)> {
    let f = fold(text);
    let toks: Vec<&str> = f.split(' ').collect();
    let first = *toks.first()?;
    if similarity(first, "piste") < 0.6 {
        return None;
    }
    // The colour word may be glued to the next word (« bleueIe ») or cut by an unread
    // accent (« lin alre »).
    let (pos, ink) = toks
        .iter()
        .enumerate()
        .skip(1)
        .take(2)
        .filter_map(|(i, t)| {
            let pair = toks.get(i + 1).map(|n| format!("{t}{n}"));
            COLOUR_WORDS
                .iter()
                .filter_map(|(w, ink)| {
                    if t.starts_with(w) || similarity(t, w) >= 0.75 {
                        Some((i, *ink))
                    } else if pair.as_deref().is_some_and(|p| similarity(p, w) >= 0.75) {
                        Some((i + 1, *ink))
                    } else {
                        None
                    }
                })
                .next()
        })
        .next()?;
    let mut rest: Vec<String> = toks[pos + 1..].iter().map(|s| (*s).to_owned()).collect();
    if ink == "orange" && rest.first().is_some_and(|t| t == "de") {
        rest.remove(0);
    }
    // Length: digits glued to the last word (« pinatelle75km ») or separate tokens.
    let joined = rest.join(" ");
    let cut = joined
        .char_indices()
        .find(|(_, c)| c.is_ascii_digit())
        .map_or(joined.len(), |(i, _)| i);
    let name = joined[..cut].trim().to_owned();
    let digits: String = text
        .chars()
        .rev()
        .skip_while(|c| !c.is_ascii_digit())
        .take_while(|c| c.is_ascii_digit() || *c == ',' || *c == '.')
        .collect::<String>()
        .chars()
        .rev()
        .collect();
    let km = digits.replace(',', ".").parse::<f64>().ok();
    (name.chars().filter(|c| c.is_alphabetic()).count() >= 3).then_some((ink, name, km))
}

/// Legend of a picture plan read by OCR (lines off the map with a colour word); the
/// departure is the sector of the legend column holding the line.
pub fn legend_ocr(spec: &PlanSpec, lines: &[TextBox]) -> Vec<LegendEntry> {
    lines
        .iter()
        .filter(|l| {
            let c = l.center();
            !spec.on_map(c[0], c[1])
        })
        .flat_map(split_entries)
        .filter_map(|l| {
            let (ink, name, km) = parse_colour_line(&l.text)?;
            let c = l.center();
            Some(LegendEntry {
                name,
                ink: spec.inks.iter().position(|i| i.name == ink),
                length_km: km,
                departure: spec
                    .sectors
                    .iter()
                    .find(|(r, _)| c[0] >= r[0] && c[0] < r[2] && c[1] >= r[1] && c[1] < r[3])
                    .map(|(_, n)| (*n).to_owned()),
                rect: l.rect,
            })
        })
        .collect()
}

/// Splits an OCR line holding two legend entries side by side (two columns read as one
/// line) at the second « Piste » word; the boxes are cut in proportion of the characters.
pub fn split_entries(l: &TextBox) -> Vec<TextBox> {
    let words: Vec<&str> = l.text.split_whitespace().collect();
    let cut = words
        .iter()
        .enumerate()
        .skip(1)
        .find(|(_, w)| similarity(w, "piste") >= 0.6 && w.len() <= 6)
        .map(|(i, _)| i);
    let Some(i) = cut else {
        return vec![l.clone()];
    };
    let (a, b) = (words[..i].join(" "), words[i..].join(" "));
    let f = a.len() as f64 / (a.len() + b.len()).max(1) as f64;
    let xm = l.rect[0] + (l.rect[2] - l.rect[0]) * f;
    vec![
        TextBox {
            text: a,
            rect: [l.rect[0], l.rect[1], xm, l.rect[3]],
        },
        TextBox {
            text: b,
            rect: [xm, l.rect[1], l.rect[2], l.rect[3]],
        },
    ]
}

#[cfg(test)]
mod tests {
    #[test]
    fn box_titles_and_split_lines() {
        assert_eq!(
            box_title("Le Glacier - 1080 m Bois Cernois Roz"),
            Some(("Le Glacier".to_owned(), 1080.0))
        );
        assert_eq!(
            box_title("Chapelle-des-Bois - 1088 m"),
            Some(("Chapelle-des-Bois".to_owned(), 1088.0))
        );
        assert_eq!(box_title("Chalet Pin 11,0 km"), None);
        let b = |t: &str, r: [f64; 4]| TextBox {
            text: t.into(),
            rect: r,
        };
        let j = join_baselines(&[
            b("Le Gy 2,6", [434.4, 579.9, 456.6, 589.1]),
            b("Autre", [434.4, 588.9, 470.0, 598.1]),
            b("km", [457.9, 579.9, 465.5, 589.1]),
        ]);
        assert_eq!(j.len(), 2);
        assert_eq!(j[0].text, "Le Gy 2,6 km");
        assert_eq!(split_length(&j[0].text), Some(("Le Gy".into(), 2.6)));
    }

    #[test]
    fn colour_words_only_after_piste() {
        let s = &crate::spec::chapelle_des_bois();
        let ink = |n: &str| s.inks.iter().position(|i| i.name == n);
        assert_eq!(colour_word(s, "Combe Verte 8,4 km"), None);
        assert_eq!(colour_word(s, "Piste verte : le Gibera"), ink("green"));
        assert_eq!(colour_word(s, "Piste de liaison Cervières"), ink("orange"));
        assert_eq!(colour_word(s, "Piste débutant 1,0 km"), None);
    }

    #[test]
    fn parses_ocr_legend_lines() {
        assert_eq!(
            parse_colour_line("Piste noire: la Pinatelle75km"),
            Some(("black", "la pinatelle".to_owned(), Some(75.0)))
        );
        assert_eq!(
            parse_colour_line("Piste verte leGbera1.5 km"),
            Some(("green", "legbera".to_owned(), Some(1.5)))
        );
        let (ink, name, _) = parse_colour_line("Pste bleue:LAigalan8 km").unwrap();
        assert_eq!((ink, name.as_str()), ("blue", "laigalan"));
        assert_eq!(parse_colour_line("Ce secteur est damé"), None);
    }

    use super::*;

    #[test]
    fn folds_and_compares_names() {
        assert_eq!(fold("Le Pré d’Haut"), "le pre d haut");
        assert_eq!(fold("CHAPELLE-DES-BOIS"), "chapelle des bois");
        assert_eq!(levenshtein("kitten", "sitting"), 3);
        assert!(similarity("Lasseron", "Lasser0n") > 0.85);
        assert!(similarity("Le Laus", "Les Fonts") < 0.5);
    }

    #[test]
    fn splits_elevations_and_lengths() {
        assert_eq!(
            split_elevation("MOUTHE - 938 m"),
            ("MOUTHE".into(), Some(938.0))
        );
        assert_eq!(
            split_elevation("Lasseron 2703 m"),
            ("Lasseron".into(), Some(2703.0))
        );
        assert_eq!(
            split_elevation("Chez Petit Pierre - Téléski - 906 m"),
            ("Chez Petit Pierre - Téléski".into(), Some(906.0))
        );
        assert_eq!(split_elevation("Le Laus"), ("Le Laus".into(), None));
        assert_eq!(
            split_length("Chalet Pin 11,0 km"),
            Some(("Chalet Pin".into(), 11.0))
        );
        assert_eq!(
            split_length("Les Tourbières 6 km"),
            Some(("Les Tourbières".into(), 6.0))
        );
        assert_eq!(
            split_length("Piste verte : le Gibera • 1,5 km"),
            Some(("Piste verte : le Gibera".into(), 1.5))
        );
        assert_eq!(split_length("km"), None);
    }
}
