//! Values a producing crate declares for a property, accepted by the validator even when
//! the two proof packs have none (a national park, a nature reserve elsewhere in France).

use std::collections::BTreeSet;

use skimap_zones::ZoneKind;

/// Every zone kind of `skimap-zones` (its `kind` property).
pub fn zone_kinds() -> Vec<&'static str> {
    use ZoneKind::*;
    let all = [
        Appb,
        ReserveNaturelleNationale,
        ReserveNaturelleRegionale,
        ParcNaturelRegional,
        ParcNational,
        SiteClasse,
        SiteInscrit,
        Spr,
        MonumentHistorique,
        MonumentAbords,
        CaptageProtection,
        ForetProtection,
        Natura2000Sic,
        Natura2000Zps,
        ReserveBiologique,
        BiodivSports,
        DistrictFranc,
        Country,
        Department,
        Canton,
        Commune,
    ];
    // Exhaustive on purpose: a new kind in skimap-zones fails to compile here.
    for k in all {
        match k {
            Appb
            | ReserveNaturelleNationale
            | ReserveNaturelleRegionale
            | ParcNaturelRegional
            | ParcNational
            | SiteClasse
            | SiteInscrit
            | Spr
            | MonumentHistorique
            | MonumentAbords
            | CaptageProtection
            | ForetProtection
            | Natura2000Sic
            | Natura2000Zps
            | ReserveBiologique
            | BiodivSports
            | DistrictFranc
            | Country
            | Department
            | Canton
            | Commune => {}
        }
    }
    all.iter().map(|k| k.as_str()).collect()
}

/// Declared values (JSON text, as in the inventory) of `key` in the pack file `path`:
/// zone kinds and the fields derived from the rules (`skimap-zones`), piste states
/// (`skimap-pack`).
pub fn declared_values(path: &str, key: &str) -> BTreeSet<String> {
    let text = |v: Vec<&str>| {
        v.into_iter()
            .map(|k| serde_json::Value::from(k).to_string())
            .collect()
    };
    match (path, key) {
        ("layers/zones.geojson", "kind") => text(zone_kinds()),
        ("layers/zones.geojson", "bivouac") => text(
            skimap_zones::fields::BIVOUAC_VALUES
                .iter()
                .map(|v| v.as_str())
                .collect(),
        ),
        ("layers/zones.geojson", "access") => text(
            skimap_zones::fields::ACCESS_VALUES
                .iter()
                .map(|v| v.as_str())
                .collect(),
        ),
        ("layers/pistes.geojson", "state") => text(vec!["open", "closed", "unknown"]),
        _ => BTreeSet::new(),
    }
}
