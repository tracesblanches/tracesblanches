//! Token counting of Pocket TTS, ported from pockettts.cpp (`src/text-prep.h`,
//! `src/tokenizer.h` at fab79722): the text preparation of the pack, then the SentencePiece
//! Unigram tokenizer (Viterbi per `▁` piece, byte fallback). Counts are those that
//! pockettts.cpp logs as "chunk 1/1: N tokens"; the pipeline checks every clip against
//! that log.

use std::collections::HashMap;
use std::path::Path;

use crate::gguf::{GgufError, Metadata};

const METASPACE: &str = "\u{2581}";
const TERMINAL: &[char] = &['.', '!', '?', '\u{2026}'];
const WEAK: &[char] = &[',', ';', ':', '-', '\u{2013}', '\u{2014}'];
const CLOSERS: &[char] = &['"', '\'', '\u{201d}', '\u{2019}', ')', ']', '\u{bb}'];

/// Text preparation rules of a pack (`pocket.text.*` in the GGUF).
#[derive(Debug, Clone, PartialEq)]
pub struct TextRules {
    /// Prefix 8 spaces to texts of fewer than 5 words.
    pub pad_with_spaces: bool,
    /// `;` becomes `,`.
    pub remove_semicolons: bool,
    /// Ensure a final sentence mark.
    pub append_terminal_punctuation: bool,
    /// Upper-case the first letter.
    pub capitalize_first_letter: bool,
    /// Single characters replaced (the French pack deletes quotes, `’`, `:` and brackets).
    pub replace: Vec<(char, String)>,
}

/// Pocket TTS text preparation and tokenizer of one pack.
#[derive(Debug, Clone)]
pub struct PocketText {
    /// Preparation rules.
    pub rules: TextRules,
    pieces: HashMap<String, (u32, f32)>,
    byte_ids: [Option<u32>; 256],
    max_len: usize,
    unk_score: f32,
}

impl PocketText {
    /// Loads the rules and the vocabulary of the pack model GGUF.
    pub fn from_gguf(path: &Path) -> Result<Self, GgufError> {
        let meta = Metadata::read(path)?;
        Self::from_metadata(&meta).map_err(|message| GgufError {
            path: path.display().to_string(),
            message,
        })
    }

    /// From already read metadata.
    pub fn from_metadata(m: &Metadata) -> Result<Self, String> {
        let from = m.strings("pocket.text.replace_from")?;
        let to = m.strings("pocket.text.replace_to")?;
        if from.len() != to.len() {
            return Err("replace_from and replace_to differ in length".into());
        }
        let mut replace = Vec::new();
        for (f, t) in from.iter().zip(to) {
            let mut cs = f.chars();
            // pockettts.cpp only applies single code point replacements.
            if let (Some(c), None) = (cs.next(), cs.next()) {
                replace.push((c, t));
            }
        }
        let rules = TextRules {
            pad_with_spaces: m.bool("pocket.text.pad_with_spaces")?,
            remove_semicolons: m.bool("pocket.text.remove_semicolons")?,
            append_terminal_punctuation: m.bool("pocket.text.append_terminal_punctuation")?,
            capitalize_first_letter: m.bool("pocket.text.capitalize_first_letter")?,
            replace,
        };
        let tokens = m.strings("tokenizer.ggml.tokens")?;
        let scores = m.f32s("tokenizer.ggml.scores")?;
        if tokens.len() != scores.len() || tokens.is_empty() {
            return Err("tokenizer tokens and scores differ in length".into());
        }
        Ok(Self::new(rules, &tokens, &scores))
    }

    /// From rules and a vocabulary (pieces in id order with their log probabilities).
    pub fn new(rules: TextRules, tokens: &[String], scores: &[f32]) -> Self {
        let mut pieces = HashMap::new();
        let mut byte_ids = [None; 256];
        let mut max_len = 0;
        let mut min = 0.0f32;
        for (i, (p, &s)) in tokens.iter().zip(scores).enumerate() {
            min = min.min(s);
            if let Some(b) = byte_piece(p) {
                byte_ids[usize::from(b)] = Some(i as u32);
                continue;
            }
            if matches!(p.as_str(), "<unk>" | "<s>" | "</s>" | "<pad>") {
                continue;
            }
            // First occurrence wins, as std::unordered_map::emplace.
            pieces.entry(p.clone()).or_insert((i as u32, s));
            max_len = max_len.max(p.len());
        }
        Self {
            rules,
            pieces,
            byte_ids,
            max_len,
            unk_score: min - 10.0,
        }
    }

    /// Text as the model reads it (`tp_prepare`); `None` for an empty text.
    pub fn prepare(&self, input: &str) -> Option<String> {
        let r = &self.rules;
        let mut text: Vec<char> = strip(&input.chars().collect::<Vec<_>>());
        if !r.replace.is_empty() {
            let mut t: Vec<char> = Vec::with_capacity(text.len());
            for &c in &text {
                match r.replace.iter().find(|(f, _)| *f == c) {
                    Some((_, to)) => t.extend(to.chars()),
                    None => t.push(c),
                }
            }
            let words: Vec<Vec<char>> = split_space(&t);
            let mut joined: Vec<char> = Vec::new();
            for w in words {
                if !joined.is_empty() {
                    joined.push(' ');
                }
                joined.extend(w);
            }
            // ([.!?…])\s*[,;:] -> \1
            text = Vec::with_capacity(joined.len());
            let mut i = 0;
            while i < joined.len() {
                text.push(joined[i]);
                if TERMINAL.contains(&joined[i]) {
                    let mut j = i + 1;
                    while j < joined.len() && is_space(joined[j]) {
                        j += 1;
                    }
                    if j < joined.len() && matches!(joined[j], ',' | ';' | ':') {
                        i = j;
                    }
                }
                i += 1;
            }
        }
        if text.is_empty() {
            return None;
        }
        for c in &mut text {
            if *c == '\n' || *c == '\r' {
                *c = ' ';
            }
        }
        // tp_replace("  ", " "): one left-to-right pass.
        let mut t = Vec::with_capacity(text.len());
        let mut i = 0;
        while i < text.len() {
            if i + 1 < text.len() && text[i] == ' ' && text[i + 1] == ' ' {
                t.push(' ');
                i += 2;
            } else {
                t.push(text[i]);
                i += 1;
            }
        }
        let mut text = t;
        if r.remove_semicolons {
            for c in &mut text {
                if *c == ';' {
                    *c = ',';
                }
            }
        }
        if r.capitalize_first_letter {
            text[0] = upper(text[0]);
        }
        if r.append_terminal_punctuation {
            text = terminal_punctuation(text);
        }
        if r.pad_with_spaces && split_space(&text).len() < 5 {
            let mut p = vec![' '; 8];
            p.extend(text);
            text = p;
        }
        Some(text.into_iter().collect())
    }

    /// Token ids of an already prepared text (`tok_encode`).
    pub fn encode(&self, text: &str) -> Vec<u32> {
        let mut norm = String::from(METASPACE);
        norm.push_str(&text.replace(' ', METASPACE));
        let mut out = Vec::new();
        let mut start = 0;
        while start < norm.len() {
            let next = norm
                .get(start + METASPACE.len()..)
                .and_then(|rest| rest.find(METASPACE))
                .map_or(norm.len(), |k| start + METASPACE.len() + k);
            self.encode_piece(&norm[start..next], &mut out);
            start = next;
        }
        out
    }

    fn encode_piece(&self, s: &str, out: &mut Vec<u32>) {
        let b = s.as_bytes();
        let n = b.len();
        let mut best = vec![-1e30f32; n + 1];
        let mut from: Vec<isize> = vec![-1; n + 1];
        let mut id: Vec<i64> = vec![-1; n + 1];
        best[0] = 0.0;
        let mut i = 0;
        while i < n {
            let cl = utf8_len(b[i]);
            if best[i] > -1e30 {
                let mut single = false;
                let mut j = i + cl;
                while j <= n && j - i <= self.max_len {
                    if let Some(&(pid, score)) = s.get(i..j).and_then(|p| self.pieces.get(p)) {
                        let sc = best[i] + score;
                        if from[j] < 0 || sc > best[j] {
                            best[j] = sc;
                            from[j] = i as isize;
                            id[j] = i64::from(pid);
                        }
                        if j - i == cl {
                            single = true;
                        }
                    }
                    if j == n {
                        break;
                    }
                    j += utf8_len(b[j]);
                }
                if !single && i + cl <= n {
                    let sc = best[i] + self.unk_score;
                    if from[i + cl] < 0 || sc > best[i + cl] {
                        best[i + cl] = sc;
                        from[i + cl] = i as isize;
                        id[i + cl] = -1;
                    }
                }
            }
            i += cl;
        }
        let mut path = Vec::new();
        let mut j = n;
        while j > 0 {
            let f = from[j];
            if f < 0 {
                // Unreachable end (cannot happen with byte fallback); emit raw bytes.
                break;
            }
            path.push((f as usize, j));
            j = f as usize;
        }
        for &(a, j) in path.iter().rev() {
            if id[j] >= 0 {
                out.push(id[j] as u32);
            } else {
                for &byte in &b[a..j] {
                    if let Some(t) = self.byte_ids[usize::from(byte)] {
                        out.push(t);
                    }
                }
            }
        }
    }

    /// Tokens pockettts.cpp feeds the model for `text` when it fits one chunk: the chunk is
    /// the prepared, trimmed text, prepared again before encoding.
    pub fn count(&self, text: &str) -> usize {
        let Some(p) = self.prepare(text) else {
            return 0;
        };
        let trimmed: String = strip(&p.chars().collect::<Vec<_>>()).into_iter().collect();
        self.prepare(&trimmed)
            .map_or(0, |again| self.encode(&again).len())
    }
}

fn byte_piece(p: &str) -> Option<u8> {
    let h = p.strip_prefix("<0x")?.strip_suffix('>')?;
    if h.len() != 2 {
        return None;
    }
    u8::from_str_radix(h, 16).ok()
}

fn utf8_len(c: u8) -> usize {
    if c < 0x80 {
        1
    } else if c >> 5 == 0x6 {
        2
    } else if c >> 4 == 0xe {
        3
    } else if c >> 3 == 0x1e {
        4
    } else {
        1
    }
}

/// Python `str.isspace`, as `tp_space`.
fn is_space(c: char) -> bool {
    let c = c as u32;
    (0x09..=0x0d).contains(&c)
        || (0x1c..=0x20).contains(&c)
        || c == 0x85
        || c == 0xa0
        || c == 0x1680
        || (0x2000..=0x200a).contains(&c)
        || c == 0x2028
        || c == 0x2029
        || c == 0x202f
        || c == 0x205f
        || c == 0x3000
}

fn strip(s: &[char]) -> Vec<char> {
    let a = s.iter().position(|&c| !is_space(c)).unwrap_or(s.len());
    let b = s.iter().rposition(|&c| !is_space(c)).map_or(a, |i| i + 1);
    s[a..b.max(a)].to_vec()
}

fn split_space(s: &[char]) -> Vec<Vec<char>> {
    s.split(|&c| is_space(c))
        .filter(|w| !w.is_empty())
        .map(<[char]>::to_vec)
        .collect()
}

/// `tp_upper`: Latin-1, Latin Extended-A, Greek and Cyrillic lower case letters.
fn upper(c: char) -> char {
    let u = c as u32;
    let up = if c.is_ascii_lowercase() || ((0xe0..=0xfe).contains(&u) && u != 0xf7) {
        u - 32
    } else if u == 0xff {
        0x178
    } else if (0x100..=0x17f).contains(&u) {
        let odd_lower = (0x100..=0x137).contains(&u) || (0x14a..=0x177).contains(&u);
        let even_lower = (0x139..=0x148).contains(&u) || (0x179..=0x17e).contains(&u);
        if (odd_lower && u & 1 == 1) || (even_lower && u & 1 == 0) {
            u - 1
        } else {
            u
        }
    } else if (0x3b1..=0x3c9).contains(&u) && u != 0x3c2 {
        u - 32
    } else if u == 0x3c2 {
        0x3a3
    } else if (0x430..=0x44f).contains(&u) {
        u - 32
    } else if (0x450..=0x45f).contains(&u) {
        u - 80
    } else {
        u
    };
    char::from_u32(up).unwrap_or(c)
}

fn terminal_punctuation(text: Vec<char>) -> Vec<char> {
    let mut e = text.len();
    while e > 0 && (text[e - 1] == ' ' || CLOSERS.contains(&text[e - 1])) {
        e -= 1;
    }
    let mut core = text[..e].to_vec();
    let closers = strip(&text[e..]);
    match core.last() {
        None => text,
        Some(c) if TERMINAL.contains(c) => text,
        Some(c) if WEAK.contains(c) => {
            while core.last().is_some_and(|c| *c == ' ' || WEAK.contains(c)) {
                core.pop();
            }
            core.push('.');
            core.extend(closers);
            core
        }
        Some(_) => {
            let mut t = text;
            t.push('.');
            t
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn french_rules() -> TextRules {
        // Values of pocket-tts-french-Q8_0.gguf (kyutai 08ffc58).
        TextRules {
            pad_with_spaces: false,
            remove_semicolons: true,
            append_terminal_punctuation: true,
            capitalize_first_letter: true,
            replace: "\"\u{201c}\u{201d}\u{201e}\u{ab}\u{bb}\u{2019}\u{2018}:()[]"
                .chars()
                .map(|c| (c, String::new()))
                .collect(),
        }
    }

    fn toy() -> PocketText {
        let mut v: Vec<(String, f32)> = vec![
            ("<unk>".into(), 0.0),
            ("<s>".into(), 0.0),
            ("</s>".into(), 0.0),
        ];
        for b in 0..=255u8 {
            v.push((format!("<0x{b:02X}>"), 0.0));
        }
        for (p, s) in [
            ("▁", -2.0),
            ("▁a", -1.0),
            ("▁ab", -1.5),
            ("a", -3.0),
            ("b", -3.0),
            ("c", -3.0),
            (".", -1.0),
            ("é", -4.0),
        ] {
            v.push((p.into(), s));
        }
        let (t, s): (Vec<_>, Vec<_>) = v.into_iter().unzip();
        PocketText::new(french_rules(), &t, &s)
    }

    #[test]
    fn prepare_follows_the_french_rules() {
        let p = toy();
        assert_eq!(p.prepare("  bonjour  ").unwrap(), "Bonjour.");
        assert_eq!(p.prepare("à gauche,").unwrap(), "À gauche.");
        assert_eq!(
            p.prepare("Heure : seize heures").unwrap(),
            "Heure seize heures."
        );
        assert_eq!(
            p.prepare("d’Altitude “Les Anges”").unwrap(),
            "DAltitude Les Anges."
        );
        assert_eq!(
            p.prepare("Stop. , encore; oui").unwrap(),
            "Stop. encore, oui."
        );
        assert_eq!(p.prepare("Fin !").unwrap(), "Fin !");
        assert_eq!(p.prepare("   "), None);
    }

    #[test]
    fn viterbi_and_byte_fallback() {
        let p = toy();
        let id = |s: &str| p.pieces[s].0;
        // "▁ab" (-1.5) beats "▁a" + "b" (-4.0).
        assert_eq!(p.encode("ab"), vec![id("▁ab")]);
        assert_eq!(p.encode("ab c"), vec![id("▁ab"), id("▁"), id("c")]);
        // "z" has no piece: its byte.
        assert_eq!(
            p.encode("az"),
            vec![id("▁a"), p.byte_ids[b'z' as usize].unwrap()]
        );
        // A two-byte character without a piece gives two byte tokens.
        assert_eq!(p.encode("aè").len(), 3);
        assert_eq!(p.encode("aé"), vec![id("▁a"), id("é")]);
        // "ab" is prepared to "Ab.": ▁, A (no piece: one byte token), b, ".".
        assert_eq!(p.count("ab"), 4);
    }
}
