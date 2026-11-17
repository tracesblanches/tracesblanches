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
/// (`skimap-pack`), SRN itinerary activities and area kinds (`skimap-srn`), and the
/// enumerated properties of the layers of [`declared_layer`].
pub fn declared_values(path: &str, key: &str) -> BTreeSet<String> {
    let text = |v: Vec<&str>| {
        v.into_iter()
            .map(|k| serde_json::Value::from(k).to_string())
            .collect()
    };
    // The SRN layers of every licence group have the schema of the ODbL one (#169).
    let path = srn_base(path).unwrap_or(path);
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
        // Any other enumerated property of a layer whose schema a producer declares
        // ([`declared_layer`]): a pack holds only some of the values (four estimated
        // traces may all be blue), the style draws all of them.
        _ => declared_layer(path)
            .and_then(|l| l["properties"][key].as_array().cloned())
            .map(|vs| vs.iter().map(serde_json::Value::to_string).collect())
            .unwrap_or_default(),
    }
}

/// Schema of a pack layer documented before any crate produces it, in the format of
/// `metadata.skimap:runtime_sources` (geometry, properties with their type or values):
/// the trail-pass zones `layers/fee-zones.geojson` (H2, #133), documented in
/// `android/docs/telechargement.md` and read by the app (`crates/ffi/src/fees.rs`), and
/// the estimated traces `layers/pistes-estimees.geojson` (G1, #117: personal packs only,
/// the proof packs are public).
pub fn declared_layer(path: &str) -> Option<serde_json::Value> {
    match path {
        // SRN areas and itineraries of the CC BY-SA and personal licence groups (#169,
        // `skimap-pack` `layers::srn_specs`): the features of `srn_areas.geojson` and
        // `srn_routes.geojson` (`skimap-srn`) of that group, same properties.
        p if srn_base(p).is_some_and(|b| b == "layers/srn-routes.geojson") => {
            Some(serde_json::json!({
                "geometry": ["LineString", "MultiLineString"],
                "properties": {
                    "activity": ["srn", "easy_ski_touring", "snowshoe", "ski_touring"],
                    "name": "string",
                    "licence": "string",
                    "attribution": "string",
                    "provider": "string",
                    "url": "string"
                }
            }))
        }
        p if srn_base(p).is_some_and(|b| b == "layers/srn-areas.geojson") => {
            Some(serde_json::json!({
                "geometry": ["Polygon", "MultiPolygon"],
                "properties": {
                    "kind": ["domaine", "traversee"],
                    "name": "string",
                    "licences": "array"
                }
            }))
        }
        // Estimated traces of the pistes absent from OSM (G1, #117), `skimap-pack`
        // `layers::estimated_pistes`: personal packs of the sites whose plan has some.
        "layers/pistes-estimees.geojson" => Some(serde_json::json!({
            "geometry": "LineString",
            "properties": {
                "name": "string",
                "colour": ["green", "blue", "red", "black"],
                "difficulty": ["novice", "easy", "intermediate", "advanced"],
                "certainty": ["incertain"],
                "method": ["estimated_paths", "estimated_drawing"],
                "score": "number",
                "plan_url": "string"
            }
        })),
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

/// The ODbL SRN layer (`layers/srn-routes.geojson`, `layers/srn-areas.geojson`) of the
/// file of another licence group (`layers/srn-routes-cc-by-sa.geojson`,
/// `layers/srn-areas-personnel.geojson`, #169); `None` for any other path (the ODbL layer
/// itself included).
fn srn_base(path: &str) -> Option<&'static str> {
    ["layers/srn-routes.geojson", "layers/srn-areas.geojson"]
        .into_iter()
        .find(|base| {
            let stem = base.trim_end_matches(".geojson");
            crate::generate::SRN_GROUPS
                .iter()
                .filter(|s| !s.is_empty())
                .any(|s| path == format!("{stem}{s}.geojson"))
        })
}
