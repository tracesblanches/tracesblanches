//! The styles of `styles/`: generated, valid against what the packs offer, no orphan
//! layer, contrast. The packs themselves are absent from CI: the validator then reads
//! the snapshot `styles/pack-schema.json`; the `--ignored` tests read the real packs
//! (`data/out/<site>/pack`) and check the snapshot against them.

use std::path::{Path, PathBuf};

use serde_json::{Value, json};
use skimap_styles::generate::{Kind, file_name, style, style_text};
use skimap_styles::inventory::Inventory;
use skimap_styles::orphan;
use skimap_styles::palette::Theme;
use skimap_styles::schema::{SITES, Snapshot};
use skimap_styles::styles_dir;
use skimap_styles::validate::validate;

fn all() -> Vec<(String, Value)> {
    Kind::ALL
        .iter()
        .flat_map(|&k| {
            Theme::ALL
                .iter()
                .map(move |&t| (file_name(k, t), style(k, t)))
        })
        .collect()
}

#[test]
fn committed_styles_are_the_generated_ones() {
    for kind in Kind::ALL {
        for theme in Theme::ALL {
            let path = styles_dir().join(file_name(kind, theme));
            let committed = std::fs::read_to_string(&path).unwrap_or_default();
            assert!(
                committed == style_text(kind, theme),
                "{} differs from the generator: cargo run -p skimap-styles --example write_styles",
                path.display()
            );
        }
    }
}

#[test]
fn styles_are_valid_against_the_pack_schema() {
    let snapshot = Snapshot::load().expect("styles/pack-schema.json");
    assert_eq!(snapshot.packs.len(), SITES.len());
    for (name, s) in all() {
        let errors = validate(&s, &snapshot.packs);
        assert!(errors.is_empty(), "{name}:\n{}", errors.join("\n"));
    }
}

#[test]
fn every_style_has_its_content() {
    let ids = |k, t| -> Vec<String> {
        style(k, t)["layers"]
            .as_array()
            .unwrap()
            .iter()
            .map(|l| l["id"].as_str().unwrap().to_string())
            .collect()
    };
    for t in Theme::ALL {
        let fond = ids(Kind::Fond, t);
        let srn = ids(Kind::Srn, t);
        let guidage = ids(Kind::Guidage, t);
        for id in [
            "pistes-line",
            "status-closed",
            "pistes-oneway",
            "pistes-label",
            "osm-parking",
            "osm-centre",
            "position-dot",
        ] {
            assert!(fond.contains(&id.to_string()), "fond {}: {id}", t.name());
        }
        for id in [
            "ign-contour",
            "ign-contour-label",
            "slope",
            "avalanche-exposure",
            "avalanche-areas-line",
            "ign-forest",
            "zones-line",
            "zones-bivouac",
            "huts-info",
            "huts-dot-info",
            "no-network-fill",
            "pistes-line",
        ] {
            assert!(srn.contains(&id.to_string()), "srn {}: {id}", t.name());
        }
        for id in ["route-line", "decision", "decision-label", "position-dot"] {
            assert!(
                guidage.contains(&id.to_string()),
                "guidage {}: {id}",
                t.name()
            );
        }
        // Pistes second in SRN: narrower than in fond.
        let width = |k, id: &str| {
            let s = style(k, t);
            let l = s["layers"]
                .as_array()
                .unwrap()
                .iter()
                .find(|l| l["id"] == id)
                .unwrap()
                .clone();
            l["paint"]["line-width"]
                .as_array()
                .unwrap()
                .last()
                .unwrap()
                .as_f64()
                .unwrap()
        };
        assert!(width(Kind::Srn, "pistes-line") < width(Kind::Fond, "pistes-line"));
        // Guidage is minimal; fond lighter than SRN.
        assert!(guidage.len() < fond.len() && fond.len() < srn.len());
    }
}

/// Index of the layer `id` of a style.
fn layer_index(s: &Value, id: &str) -> usize {
    s["layers"]
        .as_array()
        .unwrap()
        .iter()
        .position(|l| l["id"] == id)
        .unwrap()
}

/// Breaks a style in one way.
type Breaker = Box<dyn Fn(&mut Value)>;

/// A broken copy of `fond-clair` must be refused, each fault with its message.
#[test]
fn the_validator_refuses_broken_styles() {
    let snapshot = Snapshot::load().expect("styles/pack-schema.json");
    let base = style(Kind::Fond, Theme::Clair);
    let cases: Vec<(&str, Breaker)> = vec![
        (
            "absent from the tiles",
            Box::new(|s| {
                let i = layer_index(s, "ign-forest");
                s["layers"][i]["source-layer"] = json!("vegetation");
            }),
        ),
        (
            "property foret absent",
            Box::new(|s| {
                let i = layer_index(s, "ign-forest");
                s["layers"][i]["filter"] = json!(["==", ["get", "foret"], "x"]);
            }),
        ),
        (
            "never has the value",
            Box::new(|s| {
                let i = layer_index(s, "ign-forest");
                s["layers"][i]["filter"] = json!(["==", ["get", "symbo"], "ZONE_FORET_TROPICALE"]);
            }),
        ),
        (
            "legacy filter",
            Box::new(|s| {
                let i = layer_index(s, "ign-forest");
                s["layers"][i]["filter"] = json!(["==", "symbo", "ZONE_BOISEE"]);
            }),
        ),
        (
            "orphan source",
            Box::new(|s| {
                s["sources"]["zones"] = json!({"type": "geojson", "data": "layers/zones.geojson"});
            }),
        ),
        (
            "file absent from the cervieres pack",
            Box::new(|s| {
                s["sources"]["pistes"]["data"] = json!("layers/pistes-2.geojson");
            }),
        ),
        (
            "font Noto Sans Regular absent",
            Box::new(|s| {
                let i = layer_index(s, "pistes-label");
                s["layers"][i]["layout"]["text-font"] = json!(["Noto Sans Regular"]);
            }),
        ),
        (
            "one constant font",
            Box::new(|s| {
                let i = layer_index(s, "pistes-label");
                s["layers"][i]["layout"]
                    .as_object_mut()
                    .unwrap()
                    .remove("text-font");
            }),
        ),
        (
            "icon Refuge absent",
            Box::new(|s| {
                let i = layer_index(s, "huts-info");
                s["layers"][i]["layout"]["icon-image"] = json!("Refuge");
            }),
        ),
        (
            "raster-resampling nearest",
            Box::new(|s| {
                let i = layer_index(s, "snow-groomed");
                s["layers"][i]["paint"]["raster-resampling"] = json!("linear");
            }),
        ),
        (
            "rasters go under lines",
            Box::new(|s| {
                let i = layer_index(s, "snow-groomed");
                let r = s["layers"].as_array_mut().unwrap().remove(i);
                s["layers"].as_array_mut().unwrap().push(r);
            }),
        ),
        (
            "contrast",
            Box::new(|s| {
                let i = layer_index(s, "pistes-line");
                s["layers"][i]["paint"]["line-color"] = json!("#eeeeee");
            }),
        ),
        (
            "text contrast",
            Box::new(|s| {
                let i = layer_index(s, "pistes-label");
                s["layers"][i]["paint"]["text-color"] = json!("#bbbbbb");
            }),
        ),
        (
            "outside the supported subset",
            Box::new(|s| {
                let i = layer_index(s, "ign-forest");
                s["layers"][i]["filter"] =
                    json!(["within", {"type": "Polygon", "coordinates": []}]);
            }),
        ),
        (
            "has features only",
            Box::new(|s| {
                let i = layer_index(s, "ign-contour");
                s["layers"][i]["maxzoom"] = json!(9);
                s["layers"][i]["minzoom"] = json!(5);
            }),
        ),
        (
            "fill on geometries",
            Box::new(|s| {
                let i = layer_index(s, "pistes-line");
                s["layers"][i]["type"] = json!("fill");
                s["layers"][i]["paint"] = json!({"fill-color": "#000000"});
                s["layers"][i]["layout"] = json!({});
                s["layers"][i]["metadata"] = json!({});
            }),
        ),
    ];
    for (expected, break_it) in cases {
        let mut s = base.clone();
        break_it(&mut s);
        let errors = validate(&s, &snapshot.packs);
        assert!(
            errors.iter().any(|e| e.contains(expected)),
            "expected an error containing {expected:?}, got {errors:#?}"
        );
    }
}

fn pack_dir(site: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../data/out")
        .join(site)
        .join("pack")
}

#[test]
#[ignore = "needs the real packs data/out/<site>/pack"]
fn styles_are_valid_against_the_real_packs() {
    let packs: Vec<Inventory> = SITES
        .iter()
        .map(|s| Inventory::from_pack(&pack_dir(s)).expect("pack"))
        .collect();
    let dirs: Vec<PathBuf> = SITES.iter().map(|s| pack_dir(s)).collect();
    let dirs: Vec<&Path> = dirs.iter().map(PathBuf::as_path).collect();
    for (name, s) in all() {
        let errors = validate(&s, &packs);
        assert!(errors.is_empty(), "{name}:\n{}", errors.join("\n"));
        let counts = orphan::matches(&s, &dirs).expect("evaluation");
        let orphans = orphan::orphans(&s, &dirs).expect("evaluation");
        println!(
            "{name}: {} layers, features drawn per layer: {counts:?}",
            s["layers"].as_array().unwrap().len()
        );
        assert!(
            orphans.is_empty(),
            "{name}: layers drawing nothing: {orphans:?}"
        );
    }
}

#[test]
#[ignore = "needs the real packs data/out/<site>/pack"]
fn the_snapshot_is_the_real_packs() {
    let fresh = Snapshot::from_packs(&Path::new(env!("CARGO_MANIFEST_DIR")).join("../../data/out"))
        .expect("packs");
    let committed = Snapshot::load().expect("styles/pack-schema.json");
    assert!(
        fresh == committed,
        "styles/pack-schema.json is stale: cargo run --release -p skimap-styles --example write_styles -- --schema"
    );
}
