//! The texts shown to the skier carry no internal code (#177): no ticket or model code
//! (« modèle C5 », « fartage A2 », « (C7) »), no point identifier (« (depart-1) »).

use jiff::civil::date;
use skimap_assist::text::{self, Lang};
use skimap_assist::wax::SnowType;

/// A token such as `C5`, `A2`, `C11`: one capital and one or two digits. `V05` is the
/// name of a grip wax (Swix V05 Polar), not a code.
fn codes(s: &str) -> Vec<String> {
    s.split(|c: char| !c.is_alphanumeric())
        .filter(|w| {
            let mut ch = w.chars();
            ch.next().is_some_and(|c| c.is_ascii_uppercase())
                && (2..=3).contains(&w.len())
                && ch.all(|c| c.is_ascii_digit())
        })
        .filter(|w| *w != "V05")
        .map(str::to_owned)
        .collect()
}

fn plain(s: &str) {
    assert!(codes(s).is_empty(), "internal code in « {s} »");
}

#[test]
fn day_texts_have_no_internal_code() {
    let at = date(2026, 11, 21).at(9, 0, 0, 0);
    for lang in [Lang::Fr, Lang::En] {
        let t = text::of(lang);
        plain(&t.s_time_group_slow(
            "1 h",
            "famille",
            "33 min",
            "1 h 57",
            "a priori",
            "1 h 34",
            "classique",
            "37 min",
        ));
        plain(&t.s_time_group(
            "1 h",
            "famille",
            "33 min",
            "1 h 57",
            "a priori",
            "classique",
            "37 min",
        ));
        plain(&t.s_time_model("37 min", "classique"));
        plain(t.eq_kick_unreliable());
        plain(&t.s_no_way_back(2));
        plain(&t.s_unreachable(3, 10, 1.5, "Parking"));
        plain(&t.s_title(
            "2 h",
            "Parking",
            at.date(),
            at,
            Some(at + jiff::SignedDuration::from_hours(7)),
            None,
        ));
        plain(&t.wax_model(SnowType::TransformedMoist, -2.0, -5.0, 1.0));
    }
}

#[test]
fn snow_quality_names_the_place_not_its_identifier() {
    let summary = vec!["Au plat (1001 m) : neige transformée.".to_owned()];
    let fr = text::of(Lang::Fr).briefing_snow_quality("Parking Le Rouge Gazon", &summary);
    assert_eq!(
        fr,
        "Qualité de neige (Parking Le Rouge Gazon) : Au plat (1001 m) : neige transformée."
    );
    // A point without a name is not shown by its identifier.
    let fr = text::of(Lang::Fr).briefing_snow_quality("", &summary);
    assert_eq!(
        fr,
        "Qualité de neige : Au plat (1001 m) : neige transformée."
    );
    let en = text::of(Lang::En).briefing_snow_quality("", &summary);
    assert_eq!(en, "Snow quality: Au plat (1001 m) : neige transformée.");
}
