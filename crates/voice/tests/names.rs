//! Spoken names and clip identifiers (VO2, VO3), contracts left open by the existing tests
//! (Q4, #104: survivors of `cargo mutants -p skimap-voice` in `names.rs`): every colour
//! and difficulty, every folded accent, the expanded abbreviations, Roman numerals,
//! whitespace.

use skimap_voice::names::{Colour, PisteName, name_clip_id, name_clip_text, slug, spoken_name};

#[test]
fn colours_from_osm() {
    let c = |colour: Option<&str>, diff: Option<&str>| Colour::from_osm(colour, diff);
    for (v, want) in [
        ("green", Colour::Green),
        ("#008000", Colour::Green),
        ("GREEN", Colour::Green),
        ("blue", Colour::Blue),
        ("#0000FF", Colour::Blue),
        ("red", Colour::Red),
        ("#ff0000", Colour::Red),
        ("black", Colour::Black),
        ("#000000", Colour::Black),
    ] {
        assert_eq!(c(Some(v), Some("expert")), Some(want), "{v}");
    }
    for (d, want) in [
        ("novice", Colour::Green),
        ("easy", Colour::Blue),
        ("intermediate", Colour::Red),
        ("advanced", Colour::Black),
        ("expert", Colour::Black),
    ] {
        assert_eq!(c(Some("orange"), Some(d)), Some(want), "{d}");
        assert_eq!(c(None, Some(d)), Some(want), "{d}");
    }
    assert_eq!(c(Some("orange"), Some("freeride")), None);
    assert_eq!(c(None, None), None);
    assert_eq!(
        Colour::ALL.map(Colour::word),
        ["verte", "bleue", "rouge", "noire"]
    );
}

#[test]
fn every_accent_is_folded_in_slugs() {
    for (name, want) in [
        ("àáâãäå ÀÁÂÃÄÅ", "aaaaaa-aaaaaa"),
        ("æ Æ ç Ç", "ae-ae-c-c"),
        ("èéêë ÈÉÊË", "eeee-eeee"),
        ("ìíîï ÌÍÎÏ", "iiii-iiii"),
        ("ñ Ñ", "n-n"),
        ("òóôõöø ÒÓÔÕÖØ", "oooooo-oooooo"),
        ("œ Œ", "oe-oe"),
        ("ùúûü ÙÚÛÜ", "uuuu-uuuu"),
        ("ýÿ ÝŸ", "yy-yy"),
        ("ß", "ss"),
        (
            "  Refuge des Fonts de Cervières !",
            "refuge-des-fonts-de-cervieres",
        ),
    ] {
        assert_eq!(slug(name), want, "{name}");
    }
    assert_eq!(name_clip_id("Chalet de l'Œuf"), "name.chalet-de-l-oeuf");
    let p = PisteName::new("Les Prés d'Haut", Some(Colour::Red));
    assert_eq!(p.clip_id(), "piste.les-pres-d-haut.rouge");
    assert_eq!(p.clip_text(), "Les Prés d'Haut, piste rouge.");
    let p = PisteName::new("Liaison", None);
    assert_eq!(
        (p.clip_id(), p.clip_text()),
        ("piste.liaison".into(), "Liaison.".into())
    );
}

#[test]
fn spoken_abbreviations_numbers_and_spaces() {
    for (raw, want) in [
        ("St Laurent", "Saint Laurent"),
        ("St. Laurent", "Saint Laurent"),
        ("Ste Croix", "Sainte Croix"),
        ("Ste. Croix", "Sainte Croix"),
        ("Mt Risoux", "Mont Risoux"),
        ("Mt. Risoux", "Mont Risoux"),
        ("Rte des Lacs", "Route des Lacs"),
        ("La Molasse I", "La Molasse un"),
        ("La Molasse II", "La Molasse deux"),
        ("La Molasse III", "La Molasse trois"),
        ("La Molasse IV", "La Molasse quatre"),
        ("La Molasse V", "La Molasse cinq"),
        ("La Molasse VI", "La Molasse six"),
        ("La Molasse VII", "La Molasse sept"),
        ("La Molasse VIII", "La Molasse huit"),
        ("La Molasse IX", "La Molasse neuf"),
        ("La Molasse X", "La Molasse dix"),
        ("Saugy2", "Saugy deux"),
        ("Piste\tdes\u{a0}Crêtes", "Piste des Crêtes"),
        ("Chemin  du   Sioux", "Chemin du Sioux"),
        ("Bois des Ayes (du)", "Bois des Ayes"),
    ] {
        assert_eq!(spoken_name(raw), want, "{raw}");
        assert_eq!(name_clip_text(raw), format!("{want}."), "{raw}");
    }
    // A number of a million or more stays in digits.
    assert_eq!(spoken_name("Piste 1000000"), "Piste 1000000");
    assert!(spoken_name("Piste 999999").split(" ").count() > 2);
}
