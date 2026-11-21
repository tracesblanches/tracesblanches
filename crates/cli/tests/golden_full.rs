//! Golden fingerprints of every layer of both sites on the complete local data (Q2,
//! #102), full-data category (weekly job, after `skimap build-all`):
//!
//! ```sh
//! cargo test --release -p skimap-cli --test golden_full -- --ignored full_data_
//! SKIMAP_BLESS=1 cargo test --release -p skimap-cli --test golden_full -- --ignored full_data_
//! ```
//!
//! References: `fixtures/golden/full/<site>.json` for a build with the personal sources,
//! `fixtures/golden/full-public/<site>.json` for one without (the weekly build: no
//! historic avalanche layer, no CLPA cell raised, no Biodiv'sports zone, fewer pack
//! layers), whichever the data says it was built with (fixtures/support/mod.rs);
//! figures only, no data. The figures
//! come from the files the builders write (`<layer>.meta.json` statistics, reports and
//! summaries); the layers of the CI fixtures have their own fingerprints in each crate.
//!
//! The weekly job rebuilds from refreshed sources, so each tolerance is the change that
//! the source can bring in a week without a code change; anything beyond is reviewed:
//!
//! - OSM (pistes, graph, lengths on the pistes): `Rel(0.02)`, a trail added or removed
//!   (≥ 1 km on the 68 km of Cervières) must be looked at;
//! - static sources (LiDAR HD terrain and canopy, IGN slope map, DDT 05 avalanche map of
//!   16/02/2026, protected areas): deterministic figures, `Abs` at the rounding of the file
//!   or exact counts;
//! - ARCEP: quarterly release, named in the fingerprint; within a release deterministic,
//!   `Abs(0.005)` (values rounded to 0.01);
//! - community huts (refuges.info, OSM, Camptocamp): `Abs(2)` places;
//! - packs: basemaps change daily (Protomaps) or monthly (Plan IGN), `Rel(0.05)` bytes.
//!
//! Research report figures behind them: report 06 (coverage: Cervières 22.7 % without
//! any network, Chapelle-des-Bois 10.2 %), report 07 (5.78 km of the 34.5 km nordic
//! network of Cervières in CLPA footprints, 38.1 % of slopes ≥ 30°), report 05 (zones).

#[path = "../../../fixtures/support/mod.rs"]
mod fixtures;

use fixtures::golden::{Golden, Tol};
use serde_json::Value;
use skimap_common::site::{SITES, Site};

const OSM: Tol = Tol::Rel(0.02);
const STATIC: Tol = Tol::Abs(1e-6);
const COUNT: Tol = Tol::Abs(0.0);
const ARCEP: Tol = Tol::Abs(0.005);
const HUTS: Tol = Tol::Abs(2.0);

/// (file under `out/<site>/`, JSON pointer, tolerance).
const FIGURES: &[(&str, &str, Tol)] = &[
    ("pistes/pistes.meta.json", "/stats/summary/km_total", OSM),
    ("pistes/pistes.meta.json", "/stats/summary/km_nordic", OSM),
    (
        "pistes/pistes.meta.json",
        "/stats/summary/km_hike_only",
        OSM,
    ),
    ("pistes/pistes.meta.json", "/stats/summary/ways", OSM),
    ("pistes/pistes.meta.json", "/stats/summary/relations", OSM),
    ("pistes/pistes.meta.json", "/stats/summary/graph_nodes", OSM),
    ("pistes/pistes.meta.json", "/stats/summary/graph_edges", OSM),
    (
        "pistes/pistes.meta.json",
        "/stats/summary/graph_components",
        COUNT,
    ),
    ("routes/routes.meta.json", "/stats/length_km", OSM),
    ("routes/routes.meta.json", "/stats/oneway_km", OSM),
    ("routes/routes.meta.json", "/stats/gain_forward_m", OSM),
    (
        "routes/routes.meta.json",
        "/stats/profiles/edges_without",
        OSM,
    ),
    (
        "terrain/slope_ign_comparison.json",
        "/exact_agreement",
        STATIC,
    ),
    (
        "terrain/slope_ign_comparison.json",
        "/within_one_class",
        STATIC,
    ),
    (
        "terrain/slope_ign_comparison.json",
        "/steep30/ours_fraction",
        STATIC,
    ),
    (
        "terrain/slope_ign_comparison.json",
        "/steep30/ign_fraction",
        STATIC,
    ),
    ("terrain/slope_ign_comparison.json", "/steep30/f1", STATIC),
    ("zones/build_report.json", "/zones", COUNT),
    (
        "zones/zones.meta.json",
        "/stats/areas_ha/country:fr",
        Tol::Abs(1e-4),
    ),
    ("coverage/build_report.json", "/release", COUNT),
    ("coverage/build_report.json", "/analysed_km2", ARCEP),
    ("coverage/build_report.json", "/any_operator_pct", ARCEP),
    ("coverage/build_report.json", "/no_network_km2", ARCEP),
    ("coverage/build_report.json", "/no_network_pct", ARCEP),
    ("coverage/build_report.json", "/no_network_polygons", COUNT),
    (
        "refuges/refuges.meta.json",
        "/stats/refuges_info/points",
        HUTS,
    ),
    ("refuges/refuges.meta.json", "/stats/osm/elements", HUTS),
    (
        "refuges/refuges.meta.json",
        "/stats/camptocamp/in_box",
        HUTS,
    ),
    ("avalanche-exposure/summary.json", "/valid_cells", COUNT),
    ("avalanche-exposure/summary.json", "/class_shares/0", STATIC),
    ("avalanche-exposure/summary.json", "/class_shares/1", STATIC),
    ("avalanche-exposure/summary.json", "/class_shares/2", STATIC),
    ("avalanche-exposure/summary.json", "/ates_shares/0", STATIC),
    ("avalanche-exposure/summary.json", "/ates_shares/4", STATIC),
    (
        "avalanche-exposure/summary.json",
        "/clpa_cells_raised",
        COUNT,
    ),
    (
        "avalanche-exposure/summary.json",
        "/all_lines/length_km",
        OSM,
    ),
    (
        "avalanche-exposure/summary.json",
        "/all_lines/not_low_share",
        OSM,
    ),
    ("avalanche-historic/summary.json", "/clpa_in_commune", COUNT),
    (
        "avalanche-historic/summary.json",
        "/clpa_in_commune_km2",
        Tol::Abs(0.001),
    ),
    (
        "avalanche-historic/summary.json",
        "/networks/0/length_km",
        OSM,
    ),
    (
        "avalanche-historic/summary.json",
        "/networks/0/inside_clpa_km",
        OSM,
    ),
    (
        "offpiste/offpiste.meta.json",
        "/stats/grid/share_slope_ge_30_of_valid",
        STATIC,
    ),
    ("offpiste/offpiste.meta.json", "/stats/zones/classes", COUNT),
    (
        "offpiste/offpiste.meta.json",
        "/stats/zones/check_mismatches",
        COUNT,
    ),
    (
        "offpiste/offpiste.meta.json",
        "/stats/pistes/cells_groomed",
        OSM,
    ),
    ("pack/pack.meta.json", "/stats/layers", COUNT),
    ("pack/pack.meta.json", "/stats/total_bytes", Tol::Rel(0.05)),
];

/// Layers only a build with the personal sources writes (the DDT 05 CLPA, personal use).
const PERSONAL_LAYERS: &[&str] = &["avalanche-historic/"];

fn fingerprint(site: &Site) {
    let out = fixtures::full_data().join("out").join(site.id);
    let personal = fixtures::full_data_personal(site.id);
    let golden = if personal { "full" } else { "full-public" };
    println!("{}: {golden} (personal sources: {personal})", site.id);
    let mut k = Golden::new(&format!("{golden}/{}", site.id));
    let mut cache: std::collections::HashMap<&str, Value> = std::collections::HashMap::new();
    for (file, pointer, tol) in FIGURES {
        if !personal && PERSONAL_LAYERS.iter().any(|l| file.starts_with(l)) {
            continue;
        }
        let v = cache.entry(file).or_insert_with(|| {
            let p = out.join(file);
            serde_json::from_str(
                &std::fs::read_to_string(&p).unwrap_or_else(|e| panic!("{}: {e}", p.display())),
            )
            .unwrap_or_else(|e| panic!("{}: {e}", p.display()))
        });
        let key = format!("{file}#{pointer}");
        match v.pointer(pointer) {
            Some(Value::Number(n)) => {
                k.num(&key, n.as_f64().unwrap(), *tol);
            }
            Some(Value::String(s)) => {
                k.text(&key, s.as_str());
            }
            other => panic!("{}: {key} is {other:?}", site.id),
        }
    }
    k.check();
}

#[test]
#[ignore = "full-data: data/out/<site>/ after skimap build-all"]
fn full_data_layer_fingerprints() {
    for site in SITES {
        fingerprint(site);
    }
}
