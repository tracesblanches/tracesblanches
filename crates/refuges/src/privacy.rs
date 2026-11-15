//! Personal data filter for the free texts of the sources (H1, #132).
//!
//! Rule of the project: only the phones and contacts of establishments are kept (staffed
//! hut, gîte, town hall, tourist office, ski patrol post); never a private person's
//! number, a personal e-mail or a signed comment. The sources mix both in free text
//! (refuges.info « Propriétaires » field: « Gardien : name mobile », « Tél : gîte number »,
//! « E-mail : private address »), so:
//!
//! - [`redact`] removes every phone number, every e-mail address and the names announced
//!   by a title or a role (« M. », « Mme », « Gardien : », « Privé : », first name +
//!   capitalised surname) from a free text;
//! - [`establishment_phones`] keeps a number only when its own line names an
//!   establishment (« Mairie », « Office de tourisme », « PC sécurité »...) or, for a staffed
//!   hut or a gîte, carries a phone label (« Tél », « SMS », « Réservation »), and never
//!   when the line names a person or the number is a fax;
//! - [`find_phones`] and [`find_emails`] let the tests check the written layers.

use serde::Serialize;

/// Replacement of a removed phone number.
pub const PHONE_REMOVED: &str = "[téléphone retiré]";
/// Replacement of a removed e-mail address.
pub const EMAIL_REMOVED: &str = "[e-mail retiré]";
/// Replacement of a removed person name.
pub const NAME_REMOVED: &str = "[nom retiré]";

/// A phone number found in a text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PhoneMatch {
    /// Byte range in the text.
    pub start: usize,
    /// End of the byte range (exclusive).
    pub end: usize,
    /// International form, digits only after `+` (`+33400003282`).
    pub e164: String,
}

/// A phone kept as an establishment contact.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Phone {
    /// International form (`+33400003282`), for the dialler.
    pub e164: String,
    /// Display form (`04 00 00 32 82` in France).
    pub display: String,
    /// Whom it reaches: `etablissement`, `mairie`, `office_tourisme`, `pc_securite`,
    /// `onf`, `club`, or `sms` (text messages only).
    pub label: String,
    /// Source of the number (`refuges.info`, `camptocamp`, `openstreetmap`).
    pub source: String,
}

fn is_sep(c: char) -> bool {
    matches!(c, ' ' | '.' | '-' | '\u{a0}' | '\u{202f}')
}

/// Digits of a candidate number, with the international prefix resolved, or `None`.
fn to_e164(raw: &str) -> Option<String> {
    let plus = raw.starts_with('+');
    let digits: String = raw.chars().filter(char::is_ascii_digit).collect();
    // « +33 (0)4 ... »: the (0) is dropped by the caller (parentheses are not kept).
    if plus || digits.starts_with("00") {
        let d = if plus { &digits[..] } else { &digits[2..] };
        if !(9..=13).contains(&d.len()) {
            return None;
        }
        let d = if let Some(rest) = d.strip_prefix("330") {
            format!("33{rest}")
        } else {
            d.to_owned()
        };
        return Some(format!("+{d}"));
    }
    if digits.len() == 10 && digits.starts_with('0') && !digits.starts_with("00") {
        // French (or Swiss) national number: 0X XX XX XX XX.
        return Some(format!("+33{}", &digits[1..]));
    }
    None
}

/// Phone numbers of a text: French national form (`04 00 00 32 82`, `0600000503`,
/// `04.00.00.32.82`), international form (`+33 4 92 ...`, `0033 4 ...`, `+41 ...`).
/// Coordinates, years and postal codes are not numbers (they do not start with `0` or
/// `+`, or are too short).
pub fn find_phones(text: &str) -> Vec<PhoneMatch> {
    let chars: Vec<(usize, char)> = text.char_indices().collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        let (start, c) = chars[i];
        let prev_alnum = i > 0 && chars[i - 1].1.is_alphanumeric();
        if prev_alnum || !(c == '+' || c == '0') {
            i += 1;
            continue;
        }
        // Collect digits separated by at most one separator, and an optional « (0) ».
        let mut j = if c == '+' { i + 1 } else { i };
        let mut raw = String::new();
        if c == '+' {
            raw.push('+');
        }
        let mut last_digit_end = i;
        let mut ndig = 0;
        while j < chars.len() {
            let ch = chars[j].1;
            if ch.is_ascii_digit() {
                raw.push(ch);
                ndig += 1;
                last_digit_end = j + 1;
                j += 1;
            } else if is_sep(ch) && j + 1 < chars.len() && chars[j + 1].1.is_ascii_digit() {
                j += 1;
            } else if ch == '(' && text[chars[j].0..].starts_with("(0)") {
                j += 3;
            } else if is_sep(ch)
                && text[chars[j].0..].len() > 4
                && text[chars[j].0 + 1..].starts_with("(0)")
            {
                j += 1;
            } else {
                break;
            }
            if ndig > 15 {
                break;
            }
        }
        let next_alnum = last_digit_end < chars.len() && chars[last_digit_end].1.is_alphanumeric();
        if ndig >= 9
            && !next_alnum
            && let Some(e164) = to_e164(&raw)
        {
            let end = chars.get(last_digit_end).map_or(text.len(), |(b, _)| *b);
            out.push(PhoneMatch { start, end, e164 });
            i = last_digit_end;
            continue;
        }
        i = last_digit_end.max(i + 1);
    }
    out
}

/// Display form of an international number: French numbers in the national form
/// (`04 00 00 32 82`), others as `+CC rest`.
pub fn display_phone(e164: &str) -> String {
    let d = e164.trim_start_matches('+');
    if let Some(rest) = d.strip_prefix("33")
        && rest.len() == 9
    {
        let n = format!("0{rest}");
        return n
            .as_bytes()
            .chunks(2)
            .map(|c| String::from_utf8_lossy(c).into_owned())
            .collect::<Vec<_>>()
            .join(" ");
    }
    e164.to_owned()
}

fn is_email_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '%' | '+' | '-')
}

/// E-mail addresses of a text, as byte ranges.
pub fn find_emails(text: &str) -> Vec<(usize, usize)> {
    let b = text.as_bytes();
    let mut out = Vec::new();
    for (at, _) in text.match_indices('@') {
        let mut s = at;
        while s > 0 && is_email_char(b[s - 1] as char) {
            s -= 1;
        }
        let mut e = at + 1;
        while e < b.len() && (b[e].is_ascii_alphanumeric() || matches!(b[e], b'.' | b'-')) {
            e += 1;
        }
        while e > at + 1 && b[e - 1] == b'.' {
            e -= 1;
        }
        let domain = &text[at + 1..e];
        let tld_ok = domain.rsplit_once('.').is_some_and(|(h, t)| {
            !h.is_empty() && t.len() >= 2 && t.chars().all(|c| c.is_ascii_alphabetic())
        });
        if s < at && tld_ok {
            out.push((s, e));
        }
    }
    out
}

/// Whether a text still holds a phone number or an e-mail address.
pub fn has_contact(text: &str) -> bool {
    !find_phones(text).is_empty() || !find_emails(text).is_empty()
}

fn lower(s: &str) -> String {
    s.to_lowercase()
}

const TITLES: &[&str] = &[
    "m.",
    "mr",
    "mr.",
    "mme",
    "mme.",
    "mmes",
    "mlle",
    "monsieur",
    "madame",
    "mademoiselle",
];
const ROLES: &[&str] = &[
    "gardien",
    "gardienne",
    "gardiens",
    "gardiennes",
    "propriétaire",
    "propriétaires",
    "privé",
    "contact",
    "responsable",
    "gérant",
    "gérante",
    "gérants",
    "famille",
    "chez",
];
/// Words that end a name: institutions and phone labels.
const STOP: &[&str] = &[
    "mairie",
    "commune",
    "office",
    "onf",
    "pc",
    "refuge",
    "club",
    "caf",
    "ffcam",
    "association",
    "syndicat",
    "parc",
    "tél",
    "tél.",
    "tel",
    "tel.",
    "téléphone",
    "telephone",
    "sms",
    "site",
    "mail",
    "e-mail",
    "email",
    "fax",
    "réservation",
    "reservation",
    "le",
    "la",
    "les",
    "du",
    "de",
    "des",
    "l",
    "d",
    "au",
    "aux",
    "état",
    "etat",
    "canton",
    "groupement",
    "gîte",
    "gite",
    "auberge",
    "hôtel",
    "hotel",
    "chalet",
];
/// Acronyms that are not surnames.
const ACRONYMS: &[&str] = &[
    "ONF", "CAF", "FFCAM", "GTJ", "PNR", "VTT", "SMS", "GR", "GRP", "OT", "RD", "RN", "PC", "TEL",
    "ESF", "FFS", "CC", "SNCF", "EDF", "PGHM", "CRS", "SDIS", "WC", "GPS", "IGN", "CHF", "EUR",
    "TTC", "PDF", "URL", "NB", "OK", "FERME", "FERMÉ", "OUVERT",
];
/// First words that make a capitalised pair a place, not a person.
const PLACE_WORDS: &[&str] = &[
    "lac", "col", "mont", "refuge", "cabane", "chalet", "abri", "gîte", "gite", "pic", "pointe",
    "route", "rue", "chemin", "bois", "forêt", "foret", "station", "domaine", "pas", "plan",
    "combe", "crêt", "cret", "tête", "tete", "roc", "grand", "grande", "petit", "petite", "saint",
    "sainte", "notre", "vallée", "vallee", "plateau", "massif", "fort", "camp", "piste", "sentier",
    "secteur", "parking", "commune", "mairie",
];

#[derive(Debug, Clone, Copy)]
struct Word<'a> {
    start: usize,
    end: usize,
    text: &'a str,
}

fn words(text: &str) -> Vec<Word<'_>> {
    let mut out = Vec::new();
    let mut start = None;
    for (i, c) in text.char_indices() {
        let part = c.is_alphanumeric() || c == '-' || c == '\'' || c == '.';
        match (part, start) {
            (true, None) => start = Some(i),
            (false, Some(s)) => {
                out.push(Word {
                    start: s,
                    end: i,
                    text: &text[s..i],
                });
                start = None;
            }
            _ => {}
        }
    }
    if let Some(s) = start {
        out.push(Word {
            start: s,
            end: text.len(),
            text: &text[s..],
        });
    }
    // A trailing full stop ends a sentence, not a word (except titles like « M. »).
    for w in &mut out {
        let tl = lower(w.text);
        if w.text.len() > 1 && w.text.ends_with('.') && !TITLES.contains(&tl.as_str()) {
            w.end -= 1;
            w.text = &w.text[..w.text.len() - 1];
        }
    }
    out.retain(|w| !w.text.is_empty());
    out
}

fn is_cap(w: &str) -> bool {
    let mut cs = w.chars();
    match cs.next() {
        Some(c) if c.is_uppercase() => {
            cs.clone().count() >= 1 && cs.all(|c| c.is_alphabetic() || c == '-' || c == '\'')
        }
        _ => false,
    }
}

fn is_cap_lower(w: &str) -> bool {
    is_cap(w) && w.chars().skip(1).any(|c| c.is_lowercase())
}

fn is_all_caps_name(w: &str) -> bool {
    w.chars().filter(|c| c.is_alphabetic()).count() >= 3
        && w.chars().all(|c| c.is_uppercase() || c == '-' || c == '\'')
        && !ACRONYMS.contains(&w)
}

fn is_name_word(w: &str) -> bool {
    (is_cap(w) || is_all_caps_name(w))
        && !STOP.contains(&lower(w).as_str())
        && !ACRONYMS.contains(&w)
}

/// Byte ranges of the person names of a text.
fn find_names(text: &str) -> Vec<(usize, usize)> {
    let ws = words(text);
    let mut out = Vec::new();
    let mut i = 0;
    while i < ws.len() {
        let wl = lower(ws[i].text);
        let gap_ok = |a: usize, b: usize| {
            // Between two words: spaces, « : », « . », « - », « , » only.
            text[ws[a].end..ws[b].start]
                .chars()
                .all(|c| c.is_whitespace() || matches!(c, ':' | '.' | '-' | ','))
        };
        // Title or role, then 1 to 3 name words (optionally joined by « et » / « & »).
        if TITLES.contains(&wl.as_str()) || ROLES.contains(&wl.as_str()) {
            let mut j = i + 1;
            let mut last = None;
            while j < ws.len() && j <= i + 4 && gap_ok(j - 1, j) {
                let w = ws[j].text;
                if is_name_word(w) {
                    last = Some(j);
                    j += 1;
                } else if (w == "et" || w == "&")
                    && j + 1 < ws.len()
                    && is_name_word(ws[j + 1].text)
                {
                    j += 1;
                } else {
                    break;
                }
            }
            // A role needs two name words (« Gardien : Jean Dupont ») or one surname in
            // capitals; a title needs one.
            if let Some(l) = last {
                let n = l - i;
                let title = TITLES.contains(&wl.as_str());
                if title || n >= 2 || is_all_caps_name(ws[l].text) {
                    out.push((ws[i + 1].start, ws[l].end));
                    i = l + 1;
                    continue;
                }
            }
        }
        // First name (and « et » first name) followed by a surname in capitals.
        if is_cap_lower(ws[i].text)
            && !PLACE_WORDS.contains(&wl.as_str())
            && !STOP.contains(&wl.as_str())
        {
            let mut j = i + 1;
            if j + 1 < ws.len()
                && (ws[j].text == "et" || ws[j].text == "&")
                && is_cap_lower(ws[j + 1].text)
            {
                j += 2;
            }
            if j < ws.len() && gap_ok(j - 1, j) && is_all_caps_name(ws[j].text) {
                out.push((ws[i].start, ws[j].end));
                i = j + 1;
                continue;
            }
        }
        i += 1;
    }
    out
}

/// Removes phone numbers, e-mail addresses and person names from a free text. Returns
/// `None` when nothing but whitespace and punctuation is left.
pub fn redact(text: &str) -> Option<String> {
    let mut spans: Vec<(usize, usize, &str)> = Vec::new();
    for (s, e) in find_emails(text) {
        spans.push((s, e, EMAIL_REMOVED));
    }
    for p in find_phones(text) {
        spans.push((p.start, p.end, PHONE_REMOVED));
    }
    for (s, e) in find_names(text) {
        spans.push((s, e, NAME_REMOVED));
    }
    spans.sort_by_key(|s| (s.0, std::cmp::Reverse(s.1)));
    let mut out = String::with_capacity(text.len());
    let mut pos = 0;
    for (s, e, r) in spans {
        if s < pos {
            continue;
        }
        out.push_str(&text[pos..s]);
        out.push_str(r);
        pos = e;
    }
    out.push_str(&text[pos..]);
    let t = out.trim();
    let left = t
        .replace(PHONE_REMOVED, "")
        .replace(EMAIL_REMOVED, "")
        .replace(NAME_REMOVED, "");
    if left.chars().any(char::is_alphanumeric) {
        Some(t.to_owned())
    } else {
        None
    }
}

/// Lower-case words of a line.
fn lower_words(line: &str) -> Vec<String> {
    words(line).iter().map(|w| lower(w.text)).collect()
}

/// Establishment named by a line, as the label of its phone.
fn institution(line: &str) -> Option<&'static str> {
    const PHRASES: &[(&str, &str)] = &[
        ("office de tourisme", "office_tourisme"),
        ("office du tourisme", "office_tourisme"),
        ("bureau d'information", "office_tourisme"),
        ("pc sécurité", "pc_securite"),
        ("pc securite", "pc_securite"),
        ("poste de secours", "pc_securite"),
        ("office national des forêts", "onf"),
        ("club alpin", "club"),
    ];
    const WORDS: &[(&str, &str)] = &[
        ("mairie", "mairie"),
        ("commune", "mairie"),
        ("pisteurs", "pc_securite"),
        ("onf", "onf"),
        ("caf", "club"),
        ("ffcam", "club"),
    ];
    let l = lower(line);
    if let Some((_, v)) = PHRASES.iter().find(|(k, _)| l.contains(k)) {
        return Some(v);
    }
    let ws = lower_words(line);
    WORDS
        .iter()
        .find(|(k, _)| ws.iter().any(|w| w == k))
        .map(|(_, v)| *v)
}

/// Phone label of a line (« Tél », « SMS », « Réservation », « Accueil »).
fn has_phone_label(line: &str) -> Option<&'static str> {
    const WORDS: &[(&str, &str)] = &[
        ("sms", "sms"),
        ("tél", "etablissement"),
        ("tél.", "etablissement"),
        ("tel", "etablissement"),
        ("tel.", "etablissement"),
        ("téléphone", "etablissement"),
        ("telephone", "etablissement"),
        ("réservation", "etablissement"),
        ("réservations", "etablissement"),
        ("reservation", "etablissement"),
        ("accueil", "etablissement"),
    ];
    let ws = lower_words(line);
    WORDS
        .iter()
        .find(|(k, _)| ws.iter().any(|w| w == k))
        .map(|(_, v)| *v)
}

fn names_a_person(line: &str) -> bool {
    let l = lower(line);
    let w: Vec<String> = words(line).iter().map(|w| lower(w.text)).collect();
    w.iter().any(|x| TITLES.contains(&x.as_str()))
        || [
            "gardien",
            "gardienne",
            "gardiens",
            "gérant",
            "gérante",
            "responsable",
        ]
        .iter()
        .any(|r| w.iter().any(|x| x == r))
        || l.contains("portable")
        || l.contains("perso")
        || !find_names(line).is_empty()
}

/// Phones of a free text that reach an establishment.
///
/// `establishment`: the record itself is an establishment (staffed hut, open gîte): a
/// number on a line labelled « Tél », « SMS », « Réservation », « Accueil » is its phone.
/// Otherwise (unstaffed hut) only a line naming an institution (town hall, tourist office,
/// patrol post, ONF, club) gives a phone. A line that names a person, and a fax, give none.
pub fn establishment_phones(text: &str, establishment: bool, source: &str) -> Vec<Phone> {
    let mut out: Vec<Phone> = Vec::new();
    for line in text.split(['\n', '\r']) {
        let phones = find_phones(line);
        if phones.is_empty() || names_a_person(line) {
            continue;
        }
        let label = match (institution(line), has_phone_label(line)) {
            (Some(i), _) => i,
            (None, Some(l)) if establishment => l,
            _ => continue,
        };
        for p in phones {
            let mut from = p.start.saturating_sub(16);
            while !line.is_char_boundary(from) {
                from -= 1;
            }
            let before = lower(&line[from..p.start]);
            if before.contains("fax") {
                continue;
            }
            if out.iter().any(|x| x.e164 == p.e164) {
                continue;
            }
            out.push(Phone {
                display: display_phone(&p.e164),
                e164: p.e164,
                label: label.to_owned(),
                source: source.to_owned(),
            });
        }
    }
    out
}

/// A phone from a structured field of an establishment (Camptocamp `phone`, OSM `phone`).
/// A field holding only a French number without its leading 0 (« 3 84 33 59 45 ») is
/// read as that number.
pub fn structured_phone(raw: &str, label: &str, source: &str) -> Vec<Phone> {
    raw.split([';', ','])
        .filter_map(|part| {
            let part = part.trim();
            let e164 = match find_phones(part).into_iter().next() {
                Some(p) => p.e164,
                None => {
                    let only_number = part.chars().all(|c| c.is_ascii_digit() || is_sep(c));
                    let d: String = part.chars().filter(char::is_ascii_digit).collect();
                    if only_number && d.len() == 9 && !d.starts_with('0') {
                        format!("+33{d}")
                    } else {
                        return None;
                    }
                }
            };
            Some(Phone {
                display: display_phone(&e164),
                e164,
                label: label.to_owned(),
                source: source.to_owned(),
            })
        })
        .collect()
}

/// Keys that are never written: comment text and author, custodian phone, e-mails, raw
/// phone tags (phones are only written as checked [`Phone`] lists under `phones`).
pub const FORBIDDEN_KEYS: &[&str] = &[
    "text",
    "author",
    "texte_commentaire",
    "auteur_commentaire",
    "phone_custodian",
    "phone",
    "contact:phone",
    "email",
    "contact:email",
];

/// Personal data found in a written JSON value: forbidden keys, and strings outside the
/// `phones` lists holding a phone number, an e-mail address or a titled name (« M. X »).
/// Empty when the value is clean.
pub fn findings(v: &serde_json::Value) -> Vec<String> {
    fn walk(v: &serde_json::Value, path: &str, out: &mut Vec<String>) {
        use serde_json::Value;
        match v {
            Value::String(s) => {
                if has_contact(s) {
                    out.push(format!("{path}: contact in « {s} »"));
                }
                if !find_names(s).is_empty() {
                    out.push(format!("{path}: name in « {s} »"));
                }
            }
            Value::Array(a) => {
                for (i, x) in a.iter().enumerate() {
                    walk(x, &format!("{path}[{i}]"), out);
                }
            }
            Value::Object(m) => {
                for (k, x) in m {
                    if FORBIDDEN_KEYS.contains(&k.as_str()) {
                        out.push(format!("{path}.{k}: forbidden key"));
                    } else if k != "phones" {
                        walk(x, &format!("{path}.{k}"), out);
                    }
                }
            }
            _ => {}
        }
    }
    let mut out = Vec::new();
    walk(v, "", &mut out);
    out
}

/// URL given after « Site officiel » in a free text.
pub fn official_site(text: &str) -> Option<String> {
    let l = lower(text);
    let at = l.find("site officiel")?;
    let rest = &text[at..];
    let start = rest.find("http")?;
    let url: String = rest[start..]
        .chars()
        .take_while(|c| !c.is_whitespace() && !matches!(c, '"' | '<' | '>'))
        .collect();
    let url = url.trim_end_matches(['.', ',', ';', ')']).to_owned();
    (url.len() > 10).then_some(url)
}

#[cfg(test)]
mod tests {
    use super::*;

    // Every number and name below is invented (no real person in the fixtures).

    #[test]
    fn phones_found_in_all_forms() {
        let t = "Tél : 03 00 00 00 01. Autre 0600000002, +33 (0)4 00 00 00 03, 0033 4 00 00 00 04, 04.00.00.00.05 ; +41 21 000 00 06";
        let p: Vec<String> = find_phones(t).into_iter().map(|p| p.e164).collect();
        assert_eq!(
            p,
            vec![
                "+33300000001",
                "+33600000002",
                "+33400000003",
                "+33400000004",
                "+33400000005",
                "+41210000006"
            ]
        );
        assert_eq!(display_phone("+33400000003"), "04 00 00 00 03");
    }

    #[test]
    fn not_phones() {
        for t in [
            "Coordonnées suisses : 503575 / 157600",
            "Coordonnées GPS Suisse : 5 04154 / 155 769",
            "25240 Chaux-Neuve, 2026-11-15, 1500 m, 44 €",
            "Carte SwissTopo 1:25000 n° 1241",
            "ref 1234567890",
        ] {
            assert!(find_phones(t).is_empty(), "{t}");
        }
    }

    #[test]
    fn emails() {
        let t = "Mail : contact@exemple-gite.fr. Sinon jean.dupont@free.fr";
        let e: Vec<&str> = find_emails(t).into_iter().map(|(s, e)| &t[s..e]).collect();
        assert_eq!(e, vec!["contact@exemple-gite.fr", "jean.dupont@free.fr"]);
        assert!(find_emails("@ 2 rue").is_empty());
    }

    #[test]
    fn redaction_of_real_shapes() {
        // Shapes seen in refuges.info « Propriétaires » (names and numbers invented).
        assert_eq!(
            redact("Gardien : Jean Martin\r\n0600000009").as_deref(),
            Some("Gardien : [nom retiré]\r\n[téléphone retiré]")
        );
        let r =
            redact("Privé : Paul Durand. Téléphone : 03 00 00 00 08. E-mail : paul.durand@free.fr")
                .unwrap();
        assert!(
            !r.contains("Paul")
                && !r.contains("Durand")
                && !r.contains("free.fr")
                && !r.contains("08"),
            "{r}"
        );
        let r =
            redact("Anne et Luc PERRIN\r\n6 chemin des Sapins\r\nTél : 03 00 00 00 07").unwrap();
        assert!(!r.contains("Anne") && !r.contains("PERRIN"), "{r}");
        assert!(r.contains("6 chemin des Sapins"));
        assert_eq!(
            redact("M. Dupont a la clé").as_deref(),
            Some("M. [nom retiré] a la clé")
        );
        // Institutions and places stay.
        for t in [
            "Commune de Villar Saint Pancrace",
            "Office du Tourisme d'Arvieux ou mairie",
            "Contact PC sécurité",
            "Refuge forestier de la commune du Chenit.",
            "FERME DEfINITIVEMENT",
            "Cabane de bûcherons, construite en 1968",
            "Lac BLANC et Col du GALIBIER",
        ] {
            assert_eq!(redact(t).as_deref(), Some(t), "{t}");
        }
        assert_eq!(redact("0600000009"), None);
    }

    #[test]
    fn establishment_rules() {
        // Staffed hut: a named custodian's mobile on its own line is not kept.
        assert!(establishment_phones("Gardien : Jean Martin\r\n0600000009", true, "x").is_empty());
        // Gîte: labelled lines are its phone; fax and e-mail are not.
        let p = establishment_phones(
            "Anne et Luc PERRIN\r\nTél : 03 00 00 00 07\r\nFax : 03 00 00 00 06\r\nMail : x@y.fr",
            true,
            "refuges.info",
        );
        assert_eq!(p.len(), 1);
        assert_eq!(p[0].display, "03 00 00 00 07");
        assert_eq!(p[0].label, "etablissement");
        let p = establishment_phones("PAR SMS uniquement au 06 00 00 00 05", true, "x");
        assert_eq!(p[0].label, "sms");
        // Same line as a person's name: not kept.
        assert!(
            establishment_phones(
                "Privé : Paul Durand. Téléphone : 03 00 00 00 08.",
                true,
                "x"
            )
            .is_empty()
        );
        // Unstaffed hut: only institutions.
        let p = establishment_phones(
            "Mairie - 1, grande rue - 25240 Chaux-Neuve - 03 00 00 00 04",
            false,
            "x",
        );
        assert_eq!(p[0].label, "mairie");
        let p = establishment_phones("Contact PC sécurité 0700000003.", false, "x");
        assert_eq!(p[0].label, "pc_securite");
        assert!(establishment_phones("Tél. 06 00 00 00 02", false, "x").is_empty());
    }

    #[test]
    fn structured() {
        let p = structured_phone("3 00 00 00 05", "etablissement", "camptocamp");
        assert_eq!(p[0].display, "03 00 00 00 05");
        let p = structured_phone("+33 3 00 00 00 06;04 00 00 00 07", "x", "openstreetmap");
        assert_eq!(p.len(), 2);
        assert!(structured_phone("voir le site", "x", "y").is_empty());
        assert!(structured_phone("12345", "x", "y").is_empty());
    }

    #[test]
    fn site() {
        assert_eq!(
            official_site("Privé.\nSite officiel: http://www.exemple-gite.com.").as_deref(),
            Some("http://www.exemple-gite.com")
        );
        assert_eq!(official_site("rien"), None);
    }
}
