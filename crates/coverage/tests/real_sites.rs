//! Checks of the coverage layers of the two real sites against research report 06
//! (2G/3G voice, T2 2026, French part of the boxes).
//!
//! By default on the versioned layers (`fixtures/`); the `full_data_` variants read the
//! complete local data, including the extracted ARCEP GeoPackages (weekly job):
//!
//! ```sh
//! cargo test -p skimap-coverage --test real_sites
//! cargo test -p skimap-coverage --test real_sites -- --ignored full_data_
//! ```

#[path = "../../../fixtures/support/mod.rs"]
mod fixtures;

use std::path::{Path, PathBuf};

use geo::{Coord, Rect};
use skimap_coverage::{GeoPackage, Operator, Release};

fn coverage_dir(root: &Path, site: &str) -> PathBuf {
    root.join("out").join(site).join("coverage")
}

fn report(root: &Path, site: &str) -> serde_json::Value {
    let p = coverage_dir(root, site).join("build_report.json");
    serde_json::from_str(
        &std::fs::read_to_string(&p).unwrap_or_else(|e| panic!("{}: {e}", p.display())),
    )
    .unwrap()
}

fn check(
    root: &Path,
    site: &str,
    analysed: f64,
    shares: [(&str, f64); 4],
    any: f64,
    none_km2: f64,
    none_pct: f64,
) {
    let r = report(root, site);
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
    let p = coverage_dir(root, site).join("no_network.geojson");
    let fc: geojson::FeatureCollection = std::fs::read_to_string(&p)
        .unwrap_or_else(|e| panic!("{}: {e}", p.display()))
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
fn cervieres_matches_report_06() {
    cervieres(&fixtures::data());
}

#[test]
#[ignore = "full-data: data/out/cervieres/coverage"]
fn full_data_cervieres_matches_report_06() {
    cervieres(&fixtures::full_data());
}

fn cervieres(root: &Path) {
    check(
        root,
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
fn chapelle_des_bois_matches_report_06() {
    chapelle_des_bois(&fixtures::data());
}

#[test]
#[ignore = "full-data: data/out/chapelle-des-bois/coverage"]
fn full_data_chapelle_des_bois_matches_report_06() {
    chapelle_des_bois(&fixtures::full_data());
}

fn chapelle_des_bois(root: &Path) {
    check(
        root,
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

/// The whole-France GeoPackages (about 1 GB each) are not in the fixtures.
#[test]
#[ignore = "full-data: the extracted ARCEP GeoPackages in data/cache/arcep/gpkg"]
fn full_data_real_geopackage_layout() {
    let p = fixtures::full_data()
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
