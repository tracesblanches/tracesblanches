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
use skimap_styles::schema::Snapshot;

/// Sites of the proof packs (`styles/pack-schema.json`).
const SITES: [&str; 3] = [
    "cervieres",
    "chapelle-des-bois",
    "srn-domaine-geotrek-geo-pnr-vercors-fr-29403-1",
];
use skimap_styles::styles_dir;
use skimap_styles::validate::{unchecked_sources, validate, validate_with};

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
    assert_eq!(snapshot.sites(), SITES);
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
            "pistes-unknown",
            "pistes-closed",
            "pistes-oneway",
            "pistes-label",
            "osm-parking",
            "osm-centre",
            "srn-routes-line",
            "fee-zones-line",
            "snow-groomed-2",
            "position-dot",
        ] {
            assert!(fond.contains(&id.to_string()), "fond {}: {id}", t.name());
        }
        for id in [
            "ign-contour",
            "ign-contour-label",
            "slope-1",
            "slope-4",
            "exposure-3",
            "avalanche-areas-line",
            "ign-forest",
            "zones-line",
            "zones-bivouac",
            "huts-info",
            "huts-dot-info",
            "no-network-fill",
            "pistes-line",
            "pistes-unknown",
            "srn-areas-line",
            "srn-routes-line",
            "srn-routes-label",
            "snow-srn-0",
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

/// X6 (#140): in SRN, the avalanche exposure and the slope classes are never both drawn
/// at full strength at one zoom: exposure up to z13, slope from z13 (fade 12.5 to 13.5).
#[test]
fn srn_draws_exposure_and_slope_at_different_zooms() {
    // Opacity of an `interpolate linear zoom` expression at zoom z.
    fn at(e: &Value, z: f64) -> f64 {
        let a = e.as_array().unwrap();
        let stops: Vec<(f64, f64)> = a[3..]
            .chunks(2)
            .map(|c| (c[0].as_f64().unwrap(), c[1].as_f64().unwrap()))
            .collect();
        if z <= stops[0].0 {
            return stops[0].1;
        }
        for w in stops.windows(2) {
            let ((z0, v0), (z1, v1)) = (w[0], w[1]);
            if z <= z1 {
                return v0 + (v1 - v0) * (z - z0) / (z1 - z0);
            }
        }
        stops.last().unwrap().1
    }
    for t in Theme::ALL {
        let s = style(Kind::Srn, t);
        let get = |id: &str| s["layers"][layer_index(&s, id)].clone();
        let (e, sl) = (get("exposure-3"), get("slope-4"));
        let full_e = t.palette().exposure_opacity;
        let full_s = t.palette().slope_opacity;
        for k in 100..=180 {
            let z = f64::from(k) / 10.0;
            let ev = if z < e["minzoom"].as_f64().unwrap() || z >= e["maxzoom"].as_f64().unwrap() {
                0.0
            } else {
                at(&e["paint"]["fill-opacity"], z) / full_e
            };
            let sv = if z < sl["minzoom"].as_f64().unwrap() {
                0.0
            } else {
                at(&sl["paint"]["fill-opacity"], z) / full_s
            };
            assert!(
                ev + sv <= 1.0 + 1e-9,
                "{} z{z}: exposure {ev}, slope {sv}",
                t.name()
            );
        }
    }
}

/// X7 (#141): a piste whose state is unknown is never drawn like an open piste: the full
/// colour line and its casing exclude it, the neutral band draws it.
#[test]
fn unknown_state_is_never_drawn_as_open() {
    for k in [Kind::Fond, Kind::Srn] {
        for t in Theme::ALL {
            let s = style(k, t);
            let get = |id: &str| s["layers"][layer_index(&s, id)].clone();
            let known = json!([
                "match",
                ["coalesce", ["get", "state"], ""],
                ["open", "closed"],
                true,
                false
            ]);
            for id in ["pistes-line", "pistes-casing"] {
                let f = get(id)["filter"].to_string();
                assert!(f.contains(&known.to_string()), "{id}: {f}");
            }
            for id in ["pistes-unknown", "pistes-unknown-casing"] {
                let f = get(id)["filter"].to_string();
                assert!(f.contains(&json!(["!", known]).to_string()), "{id}: {f}");
            }
            // The neutral band is grey in both themes.
            let band = get("pistes-unknown-casing")["paint"]["line-color"][4]
                .as_str()
                .unwrap()
                .to_string();
            assert_eq!(band, t.palette().unknown);
        }
    }
}

/// Sources a pack may lack are declared, and each one is checked against at least one pack
/// of the snapshot or a declared schema (no layer goes unchecked in CI).
#[test]
fn optional_sources_are_declared_and_checked() {
    let snapshot = Snapshot::load().expect("styles/pack-schema.json");
    for (name, s) in all() {
        let declared: Vec<&str> = s["metadata"]["skimap:optional_sources"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_str().unwrap())
            .collect();
        for id in ["avalanche-areas", "avalanche-exposure"] {
            if s["sources"].get(id).is_some() {
                assert!(declared.contains(&id), "{name}: {id} not optional");
            }
        }
        for id in ["pistes", "ign", "zones"] {
            assert!(!declared.contains(&id), "{name}: {id} optional");
        }
        let unchecked = unchecked_sources(&s, &snapshot.packs);
        assert!(unchecked.is_empty(), "{name}: unchecked {unchecked:?}");
    }
    // A public pack (no avalanche-areas, no avalanche-exposure, no SRN layers): valid with
    // the layers of the absent sources left out.
    let public: Vec<_> = snapshot
        .packs
        .iter()
        .filter(|p| p.site.starts_with("srn-"))
        .cloned()
        .collect();
    assert_eq!(public.len(), 1);
    for (name, s) in all() {
        let errors = validate_with(&s, &public, &snapshot.packs);
        assert!(errors.is_empty(), "{name}:\n{}", errors.join("\n"));
    }
}

/// Index of the layer `id` of a style.
/// The estimated traces of the pistes absent from OSM (G1, #117) are in the six styles,
/// dotted, uncertain only, optional (personal packs), checked against their declared
/// schema.
#[test]
fn estimated_pistes_are_dotted_and_optional() {
    for (name, s) in all() {
        let i = layer_index(&s, "pistes-estimees");
        let l = &s["layers"][i];
        assert_eq!(l["source"], "pistes-estimees", "{name}");
        assert_eq!(
            l["filter"],
            json!(["==", ["get", "certainty"], "incertain"]),
            "{name}"
        );
        assert_eq!(l["paint"]["line-dasharray"], json!([0.1, 2]), "{name}");
        assert_eq!(l["layout"]["line-cap"], "round", "{name}");
        assert_eq!(l["metadata"]["skimap:group"], "pistes", "{name}");
        let optional = s["metadata"]["skimap:optional_sources"].as_array().unwrap();
        assert!(optional.contains(&json!("pistes-estimees")), "{name}");
        assert!(
            skimap_styles::declared::declared_layer("layers/pistes-estimees.geojson").is_some()
        );
    }
}

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
            "no skimap:group",
            Box::new(|s| {
                let i = layer_index(s, "huts-info");
                s["layers"][i]["metadata"] = json!({});
            }),
        ),
        (
            "a safety group cannot be hidden",
            Box::new(|s| {
                let groups = s["metadata"]["skimap:groups"].as_array_mut().unwrap();
                let g = groups.iter_mut().find(|g| g["id"] == "pistes").unwrap();
                g["toggleable"] = json!(true);
            }),
        ),
        (
            "group refuges: toggleable must be true",
            Box::new(|s| {
                let groups = s["metadata"]["skimap:groups"].as_array_mut().unwrap();
                let g = groups.iter_mut().find(|g| g["id"] == "refuges").unwrap();
                g["toggleable"] = json!(false);
            }),
        ),
        (
            "state never has the value",
            Box::new(|s| {
                let i = layer_index(s, "pistes-closed");
                s["layers"][i]["filter"] = json!(["==", ["get", "state"], "ferme"]);
            }),
        ),
        (
            "source-layer nope absent from the tiles of france",
            Box::new(|s| {
                let i = layer_index(s, "france-forest");
                s["layers"][i]["source-layer"] = json!("nope");
            }),
        ),
        (
            "kind never has the value",
            Box::new(|s| {
                let i = layer_index(s, "france-glacier");
                s["layers"][i]["filter"] = json!(["==", ["get", "kind"], "glacière"]);
            }),
        ),
        (
            "shared source absent from the France basemap",
            Box::new(|s| {
                s["sources"]["france"]["url"] = json!("pmtiles://france/autre.pmtiles");
            }),
        ),
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
                add_raster(s);
                let i = layer_index(s, "snow-raster");
                s["layers"][i]["paint"]["raster-resampling"] = json!("linear");
            }),
        ),
        (
            "rasters go under lines",
            Box::new(|s| {
                add_raster(s);
                let i = layer_index(s, "snow-raster");
                let r = s["layers"].as_array_mut().unwrap().remove(i);
                s["layers"].as_array_mut().unwrap().push(r);
            }),
        ),
        (
            "feature-state in filter",
            Box::new(|s| {
                let i = layer_index(s, "fee-zones-line");
                s["layers"][i]["filter"] = json!(["==", ["feature-state", "valid"], true]);
            }),
        ),
        (
            "is not a source of the style",
            Box::new(|s| {
                s["metadata"]["skimap:optional_sources"]
                    .as_array_mut()
                    .unwrap()
                    .push(json!("nowhere"));
            }),
        ),
        (
            "source pistes: file absent from the srn-domaine",
            Box::new(|s| {
                // pistes is required: a pack without it is refused.
                s["sources"]["pistes"]["data"] = json!("layers/pistes-absent.geojson");
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
                s["layers"][i]["metadata"] = json!({"skimap:group": "pistes"});
            }),
        ),
    ];
    let mut packs = snapshot.packs.clone();
    for p in &mut packs {
        p.raster
            .insert("rasters/snow-raster.pmtiles".into(), Default::default());
    }
    for (expected, break_it) in cases {
        let mut s = base.clone();
        break_it(&mut s);
        let errors = validate(&s, &packs);
        assert!(
            errors.iter().any(|e| e.contains(expected)),
            "expected an error containing {expected:?}, got {errors:#?}"
        );
    }
}

/// A palette raster source and layer, right above the background (the styles draw no
/// raster since the snow became polygons; the validator still checks rasters).
fn add_raster(s: &mut Value) {
    s["sources"]["snow-raster"] =
        json!({"type": "raster", "url": "pmtiles://rasters/snow-raster.pmtiles", "tileSize": 256});
    let l = json!({"id": "snow-raster", "type": "raster", "source": "snow-raster",
                   "metadata": {"skimap:group": "neige"},
                   "paint": {"raster-resampling": "nearest"}});
    s["layers"].as_array_mut().unwrap().insert(1, l);
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
    let committed = Snapshot::load().expect("styles/pack-schema.json");
    let fresh = Snapshot::from_packs(
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("../../data/out"),
        &committed.sites(),
    )
    .expect("packs");
    assert!(
        fresh == committed,
        "styles/pack-schema.json is stale: cargo run --release -p skimap-styles --example write_styles -- --schema"
    );
}
