//! French number words, as spoken: "vingt et un", "quatre-vingts", "deux cent trois".
//!
//! Traditional spelling (hyphens below one hundred, "et" for 21, 31, 41, 51, 61 and 71,
//! plural "cents" / "quatre-vingts" only at the end of the number). The spelling only
//! matters for the text-to-speech engine, which reads it as written.

/// Grammatical gender of the counted noun: "un mètre", "une minute", "vingt et une heures".
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Gender {
    /// Masculine noun (mètre, kilomètre, degré).
    Masculine,
    /// Feminine noun (minute, heure, piste).
    Feminine,
}

const UNITS: [&str; 17] = [
    "zéro", "un", "deux", "trois", "quatre", "cinq", "six", "sept", "huit", "neuf", "dix", "onze",
    "douze", "treize", "quatorze", "quinze", "seize",
];
const TENS: [&str; 7] = [
    "",
    "dix",
    "vingt",
    "trente",
    "quarante",
    "cinquante",
    "soixante",
];

/// Words for `n` (0 to 999 999 999), with `un` / `une` agreeing with `gender`.
///
/// Panics if `n` is one billion or more: nothing the voice says is that large.
pub fn cardinal(n: u32, gender: Gender) -> String {
    assert!(n < 1_000_000_000, "number too large for speech: {n}");
    if n == 0 {
        return UNITS[0].to_owned();
    }
    let millions = n / 1_000_000;
    let thousands = (n / 1000) % 1000;
    let rest = n % 1000;
    let mut parts: Vec<String> = Vec::new();
    if millions > 0 {
        let w = below_thousand(millions, Gender::Masculine, false);
        parts.push(if millions == 1 {
            "un million".to_owned()
        } else {
            format!("{w} millions")
        });
    }
    if thousands > 0 {
        // "mille" is invariable; "un mille" is just "mille"; "cent" and "vingt" do not take
        // the plural before "mille" ("deux cent mille", "quatre-vingt mille").
        if thousands == 1 {
            parts.push("mille".to_owned());
        } else {
            parts.push(format!(
                "{} mille",
                below_thousand(thousands, Gender::Masculine, false)
            ));
        }
    }
    if rest > 0 {
        parts.push(below_thousand(rest, gender, true));
    }
    parts.join(" ")
}

/// 1..=999. `last` tells whether the group ends the number (plural of cent / quatre-vingts).
fn below_thousand(n: u32, gender: Gender, last: bool) -> String {
    debug_assert!((1..1000).contains(&n));
    let hundreds = n / 100;
    let rest = n % 100;
    let mut out = String::new();
    if hundreds > 0 {
        if hundreds > 1 {
            out.push_str(UNITS[hundreds as usize]);
            out.push(' ');
        }
        out.push_str("cent");
        if hundreds > 1 && rest == 0 && last {
            out.push('s');
        }
    }
    if rest > 0 {
        if !out.is_empty() {
            out.push(' ');
        }
        out.push_str(&below_hundred(rest, gender, last));
    }
    out
}

/// 1..=99.
fn below_hundred(n: u32, gender: Gender, last: bool) -> String {
    debug_assert!((1..100).contains(&n));
    let one = |g: Gender| match g {
        Gender::Masculine => "un",
        Gender::Feminine => "une",
    };
    if n == 1 {
        return one(gender).to_owned();
    }
    if n <= 16 {
        return UNITS[n as usize].to_owned();
    }
    if n < 20 {
        return format!("dix-{}", UNITS[(n - 10) as usize]);
    }
    let (tens, unit) = (n / 10, n % 10);
    match tens {
        2..=6 => {
            let t = TENS[tens as usize];
            match unit {
                0 => t.to_owned(),
                1 => format!("{t} et {}", one(gender)),
                u => format!("{t}-{}", UNITS[u as usize]),
            }
        }
        7 => match unit {
            1 => "soixante et onze".to_owned(),
            u => format!("soixante-{}", below_hundred(10 + u, gender, last)),
        },
        _ => {
            // 80..=99
            let r = n - 80;
            if r == 0 {
                if last {
                    "quatre-vingts".to_owned()
                } else {
                    "quatre-vingt".to_owned()
                }
            } else {
                format!("quatre-vingt-{}", below_hundred(r, gender, last))
            }
        }
    }
}

/// Signed number: "moins huit".
pub fn signed(n: i32, gender: Gender) -> String {
    if n < 0 {
        format!("moins {}", cardinal(n.unsigned_abs(), gender))
    } else {
        cardinal(n as u32, gender)
    }
}

/// Day of the month: "premier", "deux", "trente et un".
pub fn day_of_month(day: u8) -> String {
    if day == 1 {
        "premier".to_owned()
    } else {
        cardinal(u32::from(day), Gender::Masculine)
    }
}

/// French month name, 1 = janvier.
pub fn month_name(month: u8) -> &'static str {
    const MONTHS: [&str; 12] = [
        "janvier",
        "février",
        "mars",
        "avril",
        "mai",
        "juin",
        "juillet",
        "août",
        "septembre",
        "octobre",
        "novembre",
        "décembre",
    ];
    MONTHS[usize::from(month.clamp(1, 12) - 1)]
}

#[cfg(test)]
mod tests {
    use super::Gender::{Feminine as F, Masculine as M};
    use super::*;

    #[test]
    fn units_and_teens() {
        let expect = [
            (0, "zéro"),
            (1, "un"),
            (7, "sept"),
            (11, "onze"),
            (16, "seize"),
            (17, "dix-sept"),
            (19, "dix-neuf"),
        ];
        for (n, w) in expect {
            assert_eq!(cardinal(n, M), w, "{n}");
        }
        assert_eq!(cardinal(1, F), "une");
    }

    #[test]
    fn tens() {
        let expect = [
            (20, "vingt"),
            (21, "vingt et un"),
            (22, "vingt-deux"),
            (31, "trente et un"),
            (45, "quarante-cinq"),
            (60, "soixante"),
            (61, "soixante et un"),
            (70, "soixante-dix"),
            (71, "soixante et onze"),
            (72, "soixante-douze"),
            (77, "soixante-dix-sept"),
            (79, "soixante-dix-neuf"),
            (80, "quatre-vingts"),
            (81, "quatre-vingt-un"),
            (89, "quatre-vingt-neuf"),
            (90, "quatre-vingt-dix"),
            (91, "quatre-vingt-onze"),
            (97, "quatre-vingt-dix-sept"),
            (99, "quatre-vingt-dix-neuf"),
        ];
        for (n, w) in expect {
            assert_eq!(cardinal(n, M), w, "{n}");
        }
        assert_eq!(cardinal(21, F), "vingt et une");
        assert_eq!(cardinal(81, F), "quatre-vingt-une");
    }

    #[test]
    fn hundreds_and_thousands() {
        let expect = [
            (100, "cent"),
            (101, "cent un"),
            (180, "cent quatre-vingts"),
            (200, "deux cents"),
            (250, "deux cent cinquante"),
            (999, "neuf cent quatre-vingt-dix-neuf"),
            (1000, "mille"),
            (1001, "mille un"),
            (1500, "mille cinq cents"),
            (2000, "deux mille"),
            (21_000, "vingt et un mille"),
            (80_000, "quatre-vingt mille"),
            (200_000, "deux cent mille"),
            (1_000_000, "un million"),
            (2_300_000, "deux millions trois cent mille"),
        ];
        for (n, w) in expect {
            assert_eq!(cardinal(n, M), w, "{n}");
        }
    }

    #[test]
    fn signed_days_months() {
        assert_eq!(signed(-8, M), "moins huit");
        assert_eq!(signed(-21, M), "moins vingt et un");
        assert_eq!(signed(3, M), "trois");
        assert_eq!(day_of_month(1), "premier");
        assert_eq!(day_of_month(30), "trente");
        assert_eq!(month_name(8), "août");
        assert_eq!(month_name(12), "décembre");
    }

    #[test]
    fn every_number_below_ten_thousand_is_well_formed() {
        for n in 0..10_000 {
            let w = cardinal(n, M);
            assert!(!w.is_empty() && !w.contains("  ") && !w.starts_with(' ') && !w.ends_with(' '));
            assert!(
                !w.contains("un cent") && !w.contains("un mille"),
                "{n}: {w}"
            );
        }
    }
}
