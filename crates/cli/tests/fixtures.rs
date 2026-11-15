//! The reference data set itself (Q1, #101): integrity against its manifest, licences,
//! absence of the personal-use and unlicensed sources, and the pipeline chain P2 → C1
//! rebuilt from its reduced inputs without network.

#[path = "../../../fixtures/support/mod.rs"]
mod fixtures;

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use serde_json::Value;
use sha2::{Digest, Sha256};
use skimap_common::layout::DataLayout;
use skimap_common::meta::SourceRef;
use skimap_common::site::SITES;
use skimap_osm::extract::{self, Planet};

fn manifest() -> Value {
    let p = fixtures::require(fixtures::root().join("manifest.json"));
    serde_json::from_str(&std::fs::read_to_string(p).unwrap()).unwrap()
}

fn files_under(dir: &Path, base: &Path, out: &mut BTreeSet<String>) {
    for e in std::fs::read_dir(dir).unwrap() {
        let p = e.unwrap().path();
        if p.is_dir() {
            files_under(&p, base, out);
        } else {
            out.insert(p.strip_prefix(base).unwrap().to_string_lossy().into_owned());
        }
    }
}

#[test]
fn every_file_matches_the_manifest() {
    let m = manifest();
    let root = fixtures::root();
    let mut listed = BTreeSet::new();
    let mut total = 0u64;
    for f in m["files"].as_array().unwrap() {
        let rel = f["path"].as_str().unwrap();
        let bytes = std::fs::read(fixtures::require(root.join(rel))).unwrap();
        assert_eq!(
            bytes.len() as u64,
            f["bytes"].as_u64().unwrap(),
            "{rel}: size"
        );
        assert_eq!(
            hex::encode(Sha256::digest(&bytes)),
            f["sha256"].as_str().unwrap(),
            "{rel}: content changed without `skimap fixtures`"
        );
        total += bytes.len() as u64;
        listed.insert(rel.to_owned());
    }
    assert_eq!(total, m["total_bytes"].as_u64().unwrap());
    let mut found = BTreeSet::new();
    for d in ["inputs", "data"] {
        files_under(&root.join(d), &root, &mut found);
    }
    let unlisted: Vec<_> = found.difference(&listed).collect();
    assert!(
        unlisted.is_empty(),
        "files not in the manifest: {unlisted:?}"
    );
    println!(
        "{} files, {total} bytes ({:.2} MB)",
        listed.len(),
        total as f64 / 1e6
    );
}

#[test]
fn every_licence_is_described() {
    let text =
        std::fs::read_to_string(fixtures::require(fixtures::root().join("LICENCES.md"))).unwrap();
    // Markdown files of the repository stay under 100 lines.
    assert!(text.lines().count() < 100);
    let m = manifest();
    for f in m["files"].as_array().unwrap() {
        let key = f["licence"].as_str().unwrap();
        assert!(
            text.contains(&format!("`{key}`")),
            "licence `{key}` of {} not described in LICENCES.md",
            f["path"]
        );
    }
}

/// Personal-use sources (Nordic France, the town hall page, ENJ, Open-Meteo), the CLPA
/// (INRAE; DDT 05 republication pending) and Biodiv'sports (no licence) never enter the
/// set.
#[test]
fn no_personal_or_unlicensed_source() {
    let root = fixtures::root();
    let mut found = BTreeSet::new();
    files_under(&root.join("data"), &root, &mut found);
    for rel in &found {
        for banned in [
            "/status/",
            "/meteo-hourly/",
            "/assist/",
            "/avalanche-historic/",
            "/bera/",
            "links.json",
            "massif.tif",
        ] {
            assert!(!rel.contains(banned), "{rel} must not be in the fixtures");
        }
        if rel.ends_with(".meta.json") {
            let v: Value = serde_json::from_slice(&std::fs::read(root.join(rel)).unwrap()).unwrap();
            for s in v["sources"].as_array().unwrap() {
                assert_ne!(s["personal_use_only"], true, "{rel}: {}", s["name"]);
                let name = s["name"].as_str().unwrap();
                for banned in [
                    "Biodiv",
                    "Nordic France",
                    "Open-Meteo",
                    "ENJ",
                    "CLPA",
                    "Mairie",
                ] {
                    assert!(!name.contains(banned), "{rel}: source {name}");
                }
            }
        }
        if rel.ends_with("zones.geojson") {
            let text = std::fs::read_to_string(root.join(rel)).unwrap();
            assert!(!text.contains("biodiv"), "{rel}: Biodiv'sports area");
        }
    }
}

/// Text of a fixture file (the OSM extract decompressed); `None` for rasters.
fn text_of(rel: &str) -> Option<String> {
    let bytes = std::fs::read(fixtures::root().join(rel)).unwrap();
    if rel.ends_with(".tif") {
        return None;
    }
    if rel.ends_with(".gz") {
        let mut s = String::new();
        std::io::Read::read_to_string(&mut flate2::read::GzDecoder::new(&bytes[..]), &mut s)
            .unwrap();
        return Some(s);
    }
    Some(String::from_utf8(bytes).unwrap_or_else(|_| panic!("{rel}: not UTF-8")))
}

/// No e-mail address, no phone number, no free comment of a third party anywhere in the
/// set (the huts layers keep allow-listed fields only, the OSM extract has no contact
/// tags), golden references included.
#[test]
fn no_personal_data_in_the_fixtures() {
    let root = fixtures::root();
    let mut files = BTreeSet::new();
    for d in ["inputs", "data", "golden"] {
        if root.join(d).is_dir() {
            files_under(&root.join(d), &root, &mut files);
        }
    }
    files.insert("manifest.json".into());
    files.insert("LICENCES.md".into());
    let mut scanned = 0;
    for rel in &files {
        let Some(text) = text_of(rel) else { continue };
        scanned += 1;
        if let Some(m) = fixtures::privacy::personal_pattern(&text) {
            panic!("{rel}: personal data {m:?}");
        }
        if rel.contains("/refuges/") && rel.ends_with(".geojson") {
            for banned in [
                "last_comment",
                "\"remark\"",
                "\"owner\"",
                "\"access_period\"",
                "\"phone\"",
                "\"website\"",
                "\"description\"",
            ] {
                assert!(!text.contains(banned), "{rel}: {banned}");
            }
        }
    }
    assert!(scanned >= 40, "{scanned}");
    let osm = text_of("inputs/osm/planet_pistes_sites.osm.gz").unwrap();
    for k in [
        "phone",
        "email",
        "contact:",
        "website",
        "description",
        "note",
        "fixme",
    ] {
        assert!(!osm.contains(&format!("k=\"{k}")), "OSM tag {k}");
    }
}

#[test]
fn personal_patterns_are_detected() {
    use fixtures::privacy::personal_pattern as p;
    for yes in [
        "tel 06 12 34 56 78 le soir",
        "0612345678",
        "06.12.34.56.78",
        "(+33 6 12 34 56 78)",
        "+33612345678",
        "+41 79 123 45 67",
        "0041791234567",
        "079 123 45 67",
        "écrire à jean.dupont@free.fr.",
        "x_y@gmail.com",
    ] {
        assert!(p(yes).is_some(), "{yes}");
    }
    for no in [
        "0.123456789",
        "[6.664691,44.795402]",
        "sha256 4a68841c0612345678ab",
        "1234567890",
        "2026-11-15T01:44:07Z",
        "ref:refuges.info",
        "@media screen",
        "contact@",
        "10 000 m",
        "osm_id 0612345678901",
    ] {
        assert_eq!(p(no), None, "{no}");
    }
}

/// The complete hut layers do hold such data: the allow-lists are what keeps it out.
#[test]
#[ignore = "full-data: data/out/<site>/refuges"]
fn full_data_hut_layers_hold_personal_data_that_the_fixtures_drop() {
    let data = fixtures::full_data();
    let found = SITES.iter().any(|s| {
        let p = data
            .join("out")
            .join(s.id)
            .join("refuges/refuges_info.geojson");
        fixtures::privacy::personal_pattern(&std::fs::read_to_string(p).unwrap()).is_some()
    });
    assert!(found);
}

fn planet() -> Planet {
    let data = skimap_osm::parse::read_file(
        &fixtures::input("osm/planet_pistes_sites.osm.gz"),
        &extract::france_aoi(),
    )
    .unwrap();
    let set = skimap_osm::pistes::classify(&data);
    let state = std::fs::read_to_string(fixtures::input("osm/planet_pistes-state.txt")).unwrap();
    Planet {
        data,
        set,
        source: SourceRef::new(
            "OpenSnowMap planet_pistes.osm.gz (fixtures)",
            extract::PLANET_URL,
            extract::LICENCE,
            extract::ATTRIBUTION,
        ),
        valid_at: Some(state.trim().to_owned()),
    }
}

/// P2 then C1 from the reduced inputs: the cut planet gives the pistes layer, the terrain
/// cells read by the profiles give the elevations; the trail graph is byte-identical to
/// the versioned `routes/piste_graph.json` that the phone-side tests read.
#[test]
fn trail_graph_rebuilt_from_the_reduced_inputs() {
    let planet = planet();
    let tmp = tempfile::tempdir().unwrap();
    let layout = DataLayout::new(tmp.path());
    for site in SITES {
        extract::build_from(site, &layout, &planet).unwrap();
        let terrain = layout.layer_dir(site, "terrain").unwrap();
        std::fs::create_dir_all(&terrain).unwrap();
        std::fs::copy(
            fixtures::input(&format!("{}/dem_profile_cells_5m.tif", site.id)),
            terrain.join("dem_5m.tif"),
        )
        .unwrap();
        std::fs::copy(
            fixtures::layer_file(site.id, "terrain", "terrain.meta.json"),
            terrain.join("terrain.meta.json"),
        )
        .unwrap();
        let (_, b) = skimap_core::routing::build(site, &layout).unwrap();
        println!(
            "{}: {} nodes, {} edges, {:.1} km, profiles on {} edges",
            site.id, b.nodes, b.edges, b.length_km, b.profiles.edges_with_profile
        );
        let got = std::fs::read(
            layout
                .layer_dir(site, "routes")
                .unwrap()
                .join("piste_graph.json"),
        )
        .unwrap();
        let want =
            std::fs::read(fixtures::layer_file(site.id, "routes", "piste_graph.json")).unwrap();
        assert!(
            got == want,
            "{}: piste_graph.json differs from the fixture",
            site.id
        );
    }
}

/// The versioned set is the one `skimap fixtures` writes today from the complete local
/// data: a pipeline change that alters an extracted layer shows up here (weekly job).
#[test]
#[ignore = "full-data: data/cache and data/out (skimap fixtures)"]
fn full_data_fixtures_are_up_to_date() {
    let data = fixtures::full_data();
    let tmp = tempfile::tempdir().unwrap();
    let out: PathBuf = tmp.path().join("fixtures");
    let st = std::process::Command::new(env!("CARGO_BIN_EXE_skimap"))
        .arg("--data-dir")
        .arg(&data)
        .arg("fixtures")
        .arg("--out")
        .arg(&out)
        .status()
        .unwrap();
    assert!(st.success(), "skimap fixtures failed");
    let fresh: Value =
        serde_json::from_str(&std::fs::read_to_string(out.join("manifest.json")).unwrap()).unwrap();
    let old = manifest();
    let key = |v: &Value| -> BTreeSet<(String, String)> {
        v["files"]
            .as_array()
            .unwrap()
            .iter()
            .map(|f| {
                (
                    f["path"].as_str().unwrap().to_owned(),
                    f["sha256"].as_str().unwrap().to_owned(),
                )
            })
            .collect()
    };
    let (a, b) = (key(&old), key(&fresh));
    // A metadata file that differs only by its build times and fetch stamps (any rebuild
    // of the layer changes them) is up to date.
    let root = fixtures::root();
    let same_meta = |p: &str| {
        p.ends_with(".meta.json") && {
            let read = |f: PathBuf| -> Value {
                serde_json::from_str(&std::fs::read_to_string(f).unwrap()).unwrap()
            };
            strip_volatile(read(root.join(p))) == strip_volatile(read(out.join(p)))
        }
    };
    let stale: BTreeSet<String> = a
        .symmetric_difference(&b)
        .map(|(p, _)| p.clone())
        .filter(|p| !same_meta(p))
        .collect();
    assert!(
        stale.is_empty(),
        "fixtures out of date (run `skimap fixtures` and review the golden tests): {stale:?}"
    );
}

/// A layer metadata without the fields that change at every build: build and fetch times,
/// HTTP validators, hashes of the fetched bodies, timings.
fn strip_volatile(v: Value) -> Value {
    const VOLATILE: &[&str] = &[
        "generated_at",
        "fetched_at",
        "last_modified",
        "etag",
        "sha256",
        "timings",
        "timings_s",
        "seconds",
        "elapsed",
        "peak_rss_kib",
    ];
    match v {
        Value::Object(m) => Value::Object(
            m.into_iter()
                .filter(|(k, _)| !VOLATILE.contains(&k.as_str()))
                .map(|(k, v)| (k, strip_volatile(v)))
                .collect(),
        ),
        Value::Array(a) => Value::Array(a.into_iter().map(strip_volatile).collect()),
        v => v,
    }
}

/// The golden machinery of `fixtures/support/golden.rs` (Q2): creation by blessing,
/// acceptance within tolerance, failure with the differing keys and lines, bless that
/// rewrites only what is out of tolerance.
#[test]
fn golden_check_and_bless() {
    use fixtures::golden::{Golden, Tol};
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("g.json");
    let make = |eta: f64, km: f64, count: i64, line: &str| {
        let mut g = Golden::new("test");
        g.num("eta_s", eta, Tol::Rel(0.01))
            .num("km", km, Tol::Abs(0.1))
            .count("n", count)
            .lines("text", &format!("first\n{line}"));
        g
    };
    let missing = std::panic::catch_unwind(|| make(100.0, 1.0, 3, "x").check_at(&path, false));
    assert!(missing.is_err(), "a missing reference must fail");
    make(100.0, 1.0, 3, "x").check_at(&path, true);
    let written = std::fs::read_to_string(&path).unwrap();
    assert!(written.contains("\"eta_s\": 100.0") && written.contains("\"first\""));
    // Within tolerance: passes, and blessing keeps the reference unchanged.
    make(100.9, 1.05, 3, "x").check_at(&path, false);
    make(100.9, 1.05, 3, "x").check_at(&path, true);
    assert_eq!(std::fs::read_to_string(&path).unwrap(), written);
    // Out of tolerance: fails with each differing key and line, not the others.
    let err =
        std::panic::catch_unwind(|| make(102.0, 1.0, 4, "y").check_at(&path, false)).unwrap_err();
    let msg = err.downcast_ref::<String>().cloned().unwrap_or_default();
    assert!(msg.contains("3 of 4 values differ"), "{msg}");
    assert!(
        msg.contains("eta_s") && msg.contains("+2 (+2.000 %)"),
        "{msg}"
    );
    assert!(msg.contains("line 2") && msg.contains("golden x") && msg.contains("now    y"));
    assert!(!msg.contains("  km"), "{msg}");
    // Bless rewrites only the values out of tolerance; the others keep their text.
    let before = std::fs::read_to_string(&path).unwrap();
    make(102.0, 1.05, 4, "x").check_at(&path, true);
    let after = std::fs::read_to_string(&path).unwrap();
    let changed: Vec<(&str, &str)> = before
        .lines()
        .zip(after.lines())
        .filter(|(a, b)| a != b)
        .collect();
    assert_eq!(
        changed,
        [
            ("  \"eta_s\": 100.0,", "  \"eta_s\": 102.0,"),
            ("  \"n\": 3,", "  \"n\": 4,")
        ]
    );
    make(102.0, 1.05, 4, "y").check_at(&path, true);
    let v: Value = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    assert_eq!(v["eta_s"], 102.0);
    assert_eq!(v["km"], 1.0);
    assert_eq!(v["n"], 4);
    // A key that disappears is a difference too.
    let mut g = Golden::new("test");
    g.num("eta_s", 102.0, Tol::FLOAT);
    assert!(std::panic::catch_unwind(|| g.check_at(&path, false)).is_err());
}
