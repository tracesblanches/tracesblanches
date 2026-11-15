//! Rate of good classification on a set of questions, with its confusion matrix (A11).
//!
//! A set is a TSV file: `label<TAB>lang<TAB>text`, label an [`Intent::name`] or `unknown`
//! (a question outside the list, which must be refused), lang `fr` or `en`; `#` starts a
//! comment line.

use std::collections::BTreeMap;
use std::fmt;

use crate::{Intent, Lang, classify};

/// One question of a set.
#[derive(Debug, Clone, PartialEq)]
pub struct Case {
    /// Expected command, `None`: must not be understood.
    pub expected: Option<Intent>,
    /// Language.
    pub lang: Lang,
    /// Text, as said.
    pub text: String,
}

/// Parses a set; an error names the bad line.
pub fn parse(tsv: &str) -> Result<Vec<Case>, String> {
    tsv.lines()
        .enumerate()
        .filter(|(_, l)| !l.trim().is_empty() && !l.starts_with('#'))
        .map(|(n, l)| {
            let mut f = l.splitn(3, '\t');
            let (Some(label), Some(lang), Some(text)) = (f.next(), f.next(), f.next()) else {
                return Err(format!("line {}: expected label, lang, text", n + 1));
            };
            let expected = match label {
                "unknown" => None,
                s => {
                    Some(Intent::from_name(s).ok_or_else(|| format!("line {}: label {s}", n + 1))?)
                }
            };
            let lang = match lang {
                "fr" => Lang::Fr,
                "en" => Lang::En,
                s => return Err(format!("line {}: lang {s}", n + 1)),
            };
            Ok(Case {
                expected,
                lang,
                text: text.to_string(),
            })
        })
        .collect()
}

/// Name of a class of the matrix (`unknown` for none).
pub fn class_name(i: Option<Intent>) -> &'static str {
    i.map_or("unknown", Intent::name)
}

/// Results on a set.
#[derive(Debug, Clone, Default)]
pub struct Report {
    /// (expected, predicted) → count.
    pub matrix: BTreeMap<(Option<Intent>, Option<Intent>), usize>,
    /// Per language: (right, total).
    pub per_lang: BTreeMap<&'static str, (usize, usize)>,
    /// Wrong answers: (case, predicted).
    pub errors: Vec<(Case, Option<Intent>)>,
    /// Questions classified.
    pub total: usize,
    /// Right ones.
    pub right: usize,
    /// A command of the list taken for another command of the list (the worst error: the
    /// app does something else than asked).
    pub wrong_command: usize,
    /// A question outside the list taken for a command.
    pub false_accept: usize,
    /// A command of the list refused (« je n'ai pas compris »: the skier asks again).
    pub false_reject: usize,
}

impl Report {
    /// Rate of good classification.
    pub fn accuracy(&self) -> f64 {
        if self.total == 0 {
            0.0
        } else {
            self.right as f64 / self.total as f64
        }
    }
}

/// Classifies every case in its own language as default (the phone's language).
pub fn evaluate(cases: &[Case]) -> Report {
    let mut r = Report::default();
    for c in cases {
        let got = classify(&c.text, c.lang).intent;
        *r.matrix.entry((c.expected, got)).or_default() += 1;
        let lang = match c.lang {
            Lang::Fr => "fr",
            Lang::En => "en",
        };
        let e = r.per_lang.entry(lang).or_default();
        e.1 += 1;
        r.total += 1;
        if got == c.expected {
            e.0 += 1;
            r.right += 1;
        } else {
            match (c.expected, got) {
                (Some(_), Some(_)) => r.wrong_command += 1,
                (None, Some(_)) => r.false_accept += 1,
                (Some(_), None) => r.false_reject += 1,
                (None, None) => {}
            }
            r.errors.push((c.clone(), got));
        }
    }
    r
}

/// Short column header of a class.
fn short(i: Option<Intent>) -> &'static str {
    match i {
        None => "unk",
        Some(Intent::WhereAmI) => "where",
        Some(Intent::TimeLeft) => "time",
        Some(Intent::NextDirection) => "next",
        Some(Intent::BringMeBack) => "back",
        Some(Intent::TurnaroundTime) => "turn",
        Some(Intent::ReportTracksSet) => "track",
        Some(Intent::ReportGroomed) => "groom",
        Some(Intent::Battery) => "batt",
        Some(Intent::Repeat) => "rep",
    }
}

impl fmt::Display for Report {
    /// The rates, the confusion matrix (rows: expected, columns: predicted) and the errors.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(
            f,
            "accuracy {}/{} = {:.1} %",
            self.right,
            self.total,
            100.0 * self.accuracy()
        )?;
        for (lang, (ok, n)) in &self.per_lang {
            writeln!(
                f,
                "  {lang}: {ok}/{n} = {:.1} %",
                100.0 * *ok as f64 / *n as f64
            )?;
        }
        writeln!(
            f,
            "  wrong command {}, outside question accepted {}, command refused {}",
            self.wrong_command, self.false_accept, self.false_reject
        )?;
        let classes: Vec<Option<Intent>> = Intent::ALL
            .into_iter()
            .map(Some)
            .chain(std::iter::once(None))
            .collect();
        write!(f, "{:>15}", "expected\\got")?;
        for c in &classes {
            write!(f, " {:>5}", short(*c))?;
        }
        writeln!(f)?;
        for e in &classes {
            write!(f, "{:>15}", class_name(*e))?;
            for p in &classes {
                let n = self.matrix.get(&(*e, *p)).copied().unwrap_or(0);
                if n == 0 {
                    write!(f, " {:>5}", ".")?;
                } else {
                    write!(f, " {n:>5}")?;
                }
            }
            writeln!(f)?;
        }
        for (c, got) in &self.errors {
            writeln!(
                f,
                "  {} -> {}: {}",
                class_name(c.expected),
                class_name(*got),
                c.text
            )?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_and_counts() {
        let cases = parse("# c\nwhere\tfr\tOù suis-je ?\nunknown\ten\tPlay music\n\n").unwrap();
        assert_eq!(cases.len(), 2);
        let r = evaluate(&cases);
        assert_eq!((r.right, r.total), (2, 2));
        assert!(r.to_string().contains("accuracy 2/2"));
        assert!(parse("where\tde\tWo bin ich").is_err());
        assert!(parse("nowhere\tfr\tx").is_err());
        assert!(parse("where fr").is_err());
    }
}
