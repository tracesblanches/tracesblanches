//! Refuges of the day plan (X21, #155): `Context::load` reads the flag written by the pack
//! (`plan_refuge`) as it is, without comparing the two layers; the pipeline's own files,
//! without the flag, are flagged when loaded (same hut in both sources kept once).

#![allow(
    clippy::disallowed_methods,
    reason = "tests write their inputs directly"
)]
use std::path::PathBuf;

use serde_json::{Value, json};
use skimap_common::layout::DataLayout;
use skimap_common::site::{SITES, Site};
use skimap_plan::plan::Context;

fn pt(lon: f64, props: Value) -> Value {
    json!({"type": "Feature", "properties": props,
           "geometry": {"type": "Point", "coordinates": [lon, 45.0]}})
}

/// A data root holding the two refuge layers of a site.
fn root(tag: &str, info: &Value, osm: &Value) -> (PathBuf, &'static Site) {
    let site = SITES.first().expect("an evaluation site");
    let root =
        std::env::temp_dir().join(format!("skimap-plan-refuges-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let dir = DataLayout::new(&root).layer_dir(site, "refuges").unwrap();
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("refuges_info.geojson"), info.to_string()).unwrap();
    std::fs::write(dir.join("osm.geojson"), osm.to_string()).unwrap();
    (root, site)
}

fn names(ctx: &Context) -> Vec<&str> {
    ctx.refuges
        .iter()
        .map(|r| r.name.as_deref().unwrap_or("-"))
        .collect()
}

#[test]
fn pack_flags_are_read_as_they_are() {
    // Flags that the comparison would not give (the OSM hut 24 m from the refuges.info
    // one flagged, a kind the plan does not use flagged): read, not recomputed.
    let info = json!({"features": [
        pt(6.0, json!({"name": " Refuge A ", "type_label": "refuge gardé", "plan_refuge": true})),
        pt(6.1, json!({"name": "Point d'eau", "type_label": "point d'eau", "plan_refuge": true})),
        pt(6.2, json!({"name": "Cabane", "type_label": "cabane non gardée", "plan_refuge": false})),
    ]});
    let osm = json!({"features": [
        pt(6.0003, json!({"kind": "alpine_hut", "tags": {"name": "Hut A"}, "plan_refuge": true})),
    ]});
    let (root, site) = root("flags", &info, &osm);
    let ctx = Context::load(&DataLayout::new(&root), site).unwrap();
    assert_eq!(names(&ctx), ["Refuge A", "Point d'eau", "Hut A"]);
    assert_eq!(ctx.refuges[2].kind, "alpine_hut");
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn pipeline_files_are_flagged_when_loaded() {
    let info = json!({"features": [
        pt(6.0, json!({"name": "Refuge A", "type_label": "refuge gardé"})),
        pt(6.1, json!({"name": "Point d'eau", "type_label": "point d'eau"})),
    ]});
    let osm = json!({"features": [
        pt(6.0003, json!({"kind": "alpine_hut", "tags": {"name": "Hut A"}})),
        pt(6.01, json!({"kind": "shelter", "tags": {"name": "Abri"}})),
        pt(6.0102, json!({"kind": "wilderness_hut"})),
    ]});
    let (root, site) = root("pipeline", &info, &osm);
    let ctx = Context::load(&DataLayout::new(&root), site).unwrap();
    // The OSM hut at the refuges.info one and the hut 16 m from the shelter: left out.
    assert_eq!(names(&ctx), ["Refuge A", "Abri"]);
    assert!(ctx.missing.iter().all(|m| !m.starts_with("refuges")));
    let _ = std::fs::remove_dir_all(&root);
}
