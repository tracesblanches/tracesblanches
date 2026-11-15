//! Values a producing crate declares for a property, accepted by the validator even when
//! the two proof packs have none (a national park, a nature reserve elsewhere in France),
//! and the schema of a layer no pack has yet ([`declared_layer`]).

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
/// (`skimap-pack`), SRN itinerary activities and area kinds (`skimap-srn`).
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
        // `skimap_srn::route::Activity::as_str` (crates/srn/src/route.rs).
        ("layers/srn-routes.geojson", "activity") => {
            text(vec!["srn", "easy_ski_touring", "snowshoe", "ski_touring"])
        }
        // `skimap_srn::areas` (crates/srn/README.md): `domaine` or `traversee`.
        ("layers/srn-areas.geojson", "kind") => text(vec!["domaine", "traversee"]),
        // android/docs/telechargement.md (H2): kinds of trail-pass zones.
        ("layers/fee-zones.geojson", "kind") => text(vec![
            "site",
            "groupement",
            "departement",
            "region",
            "national",
        ]),
        _ => BTreeSet::new(),
    }
}

/// Schema of a pack layer documented before any crate produces it, in the format of
/// `metadata.skimap:runtime_sources` (geometry, properties with their type or values):
/// the trail-pass zones `layers/fee-zones.geojson` (H2, #133), documented in
/// `android/docs/telechargement.md` and read by the app (`crates/ffi/src/fees.rs`).
pub fn declared_layer(path: &str) -> Option<serde_json::Value> {
    match path {
        "layers/fee-zones.geojson" => Some(serde_json::json!({
            "geometry": "Polygon",
            "properties": {
                "fee_id": "string",
                "name": "string",
                "issuer": "string",
                "kind": ["site", "groupement", "departement", "region", "national"],
                "prices": "array"
            }
        })),
        _ => None,
    }
}
