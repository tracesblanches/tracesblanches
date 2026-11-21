//! The plan texts shown to the skier carry no internal code (#177): no « modèle C5 »,
//! « (C7) », « (A8, C11) ».

use jiff::civil::date;
use skimap_core::routing::Style;
use skimap_plan::text::{self, Lang};

/// A token such as `C5`, `A8`, `C11`: one capital and one or two digits.
fn codes(s: &str) -> Vec<String> {
    s.split(|c: char| !c.is_alphanumeric())
        .filter(|w| {
            let mut ch = w.chars();
            ch.next().is_some_and(|c| c.is_ascii_uppercase())
                && (2..=3).contains(&w.len())
                && ch.all(|c| c.is_ascii_digit())
        })
        .map(str::to_owned)
        .collect()
}

fn plain(s: &str) {
    assert!(codes(s).is_empty(), "internal code in « {s} »");
}

#[test]
fn plan_texts_have_no_internal_code() {
    let at = date(2026, 11, 21).at(9, 0, 0, 0);
    let later = at + jiff::SignedDuration::from_hours(2);
    for lang in [Lang::Fr, Lang::En] {
        let t = text::of(lang);
        plain(&t.p_loop(1, "La Verte", 3.7, 96.0, 60.0, Style::Classic, 1.25));
        plain(&t.p_arrival(at, later, Some(later), Some(later)));
        plain(&t.p_turnaround(Some(later), true));
        plain(&t.p_turnaround(Some(later), false));
        plain(&t.fb_exit("au carrefour", 2000.0, 3700.0, 600.0, "La Verte"));
        plain(&t.fb_detour(
            "La Bleu",
            "au carrefour",
            Some(1500.0),
            4000.0,
            -300.0,
            "La Verte",
        ));
        plain(&t.pc_text(
            "famille",
            "a priori",
            "1 h",
            "retour vers 11 h",
            3000.0,
            600.0,
            2200.0,
        ));
    }
}
