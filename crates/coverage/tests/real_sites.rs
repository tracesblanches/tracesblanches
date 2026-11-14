//! Checks of the coverage layers of the two real sites against research report 06
//! (2G/3G voice, T2 2026, French part of the boxes). Ignored by default: they read
//! `data/out/<site>/coverage/` and the extracted GeoPackages.
//!
//! ```sh
//! cargo run --release -p skimap-coverage --example build_coverage -- --archives research/raw/arcep/dl
//! cargo test -p skimap-coverage --test real_sites -- --ignored
//! ```

use std::path::PathBuf;

use geo::{Coord, Rect};
use skimap_coverage::{GeoPackage, Operator, Release};

fn data_dir() -> PathBuf {
    std::env::var("SKIMAP_DATA_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/../../data")))
}

fn report(site: &str) -> serde_json::Value {
    let p = data_dir()
        .join("out")
        .join(site)
        .join("coverage")
        .join("build_report.json");
    serde_json::from_str(
        &std::fs::read_to_string(&p).unwrap_or_else(|e| panic!("{}: {e}", p.display())),
    )
    .unwrap()
}

fn check(
    site: &str,
    analysed: f64,
    shares: [(&str, f64); 4],
    any: f64,
    none_km2: f64,
    none_pct: f64,
) {
    let r = report(site);
    let close = |what: &str, got: f64, want: f64, tol: f64| {
        println!("{site} {what}: {got} (report 06: {want})");
        assert!((got - want).abs() <= tol, "{site} {what}: {got} vs {want}");
    };
    close(
        "analysed km²",
        r["analysed_km2"].as_f64().unwrap(),
        analysed,
        0.5,
    );
    for (name, pct) in shares {
        let o = r["operators"]
            .as_array()
            .unwrap()
            .iter()
            .find(|o| o["operator"] == name)
            .unwrap();
        close(
            &format!("{name} %"),
            o["covered_pct"].as_f64().unwrap(),
            pct,
            0.1,
        );
    }
    close(
        "any operator %",
        r["any_operator_pct"].as_f64().unwrap(),
        any,
        0.1,
    );
    close(
        "no network km²",
        r["no_network_km2"].as_f64().unwrap(),
        none_km2,
        0.1,
    );
    close(
        "no network %",
        r["no_network_pct"].as_f64().unwrap(),
        none_pct,
        0.1,
    );
    let fc: geojson::FeatureCollection = std::fs::read_to_string(
        data_dir()
            .join("out")
            .join(site)
            .join("coverage")
            .join("no_network.geojson"),
    )
    .unwrap()
    .parse()
    .unwrap();
    let total_ha: f64 = fc
        .features
        .iter()
        .map(|f| f.property("area_ha").unwrap().as_f64().unwrap())
        .sum();
    close(
        "sum of polygon areas km²",
        (total_ha / 100.0 * 10.0).round() / 10.0,
        none_km2,
        0.1,
    );
}

#[test]
#[ignore = "needs data/out/cervieres/coverage"]
fn cervieres_matches_report_06() {
    check(
        "cervieres",
        225.6,
        [
            ("Bouygues", 69.4),
            ("SFR", 69.3),
            ("Free", 54.2),
            ("Orange", 37.7),
        ],
        77.3,
        51.2,
        22.7,
    );
}

#[test]
#[ignore = "needs data/out/chapelle-des-bois/coverage"]
fn chapelle_des_bois_matches_report_06() {
    check(
        "chapelle-des-bois",
        172.7,
        [
            ("Bouygues", 77.7),
            ("SFR", 84.8),
            ("Free", 76.9),
            ("Orange", 73.2),
        ],
        89.8,
        17.6,
        10.2,
    );
}

#[test]
#[ignore = "needs the extracted ARCEP GeoPackages in data/cache/arcep/gpkg"]
fn real_geopackage_layout() {
    let p = data_dir()
        .join("cache/arcep/gpkg")
        .join(Release::LATEST_KNOWN.gpkg_name(Operator::Orange));
    let g = GeoPackage::open(&p).unwrap();
    assert_eq!(g.epsg(), Some(2154));
    assert!(g.has_rtree());
    // Whole of metropolitan France: one feature per département and level (287).
    let all = Rect::new(Coord { x: 0.0, y: 6.0e6 }, Coord { x: 1.3e6, y: 7.2e6 });
    let feats = g.features_in(all, "niveau").unwrap();
    println!("{} features in {}", feats.len(), g.table());
    assert_eq!(feats.len(), 287);
    let mut levels: Vec<_> = feats.iter().map(|f| f.0.clone()).collect();
    levels.sort();
    levels.dedup();
    assert_eq!(levels, ["BC", "CL", "TBC"]);
}
