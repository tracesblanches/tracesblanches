//! The API seen by Kotlin, on the test pack (always) and on the real packs of both sites
//! (`--ignored`, needs `data/out/<site>/pack/`): the validated styles of `styles/`
//! instantiated on one and several packs, the catalogue, and the sync of a pack from
//! bundles made exactly as the server makes them.

#![allow(
    clippy::disallowed_methods,
    reason = "tests write their inputs directly"
)]
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::Instant;

use jiff::Timestamp;
use serde_json::{Value, json};
use skimap_ffi::sync::{
    SyncError, install_bundle, record_sync, remove_pack, sync_plan, sync_plan_full, sync_state,
    verify_bundle,
};
use skimap_ffi::{Lang, LegendKind, PackError, list_packs, open_packs};
use skimap_pack::Pack;
use skimap_pack::update::{Update, write_bundle};

#[path = "../examples/test_pack.rs"]
#[allow(dead_code)]
mod test_pack;

/// The six styles validated by the user.
const STYLES: [&str; 6] = [
    "fond-clair",
    "fond-sombre",
    "srn-clair",
    "srn-sombre",
    "guidage-clair",
    "guidage-sombre",
];

fn style_json(name: &str) -> String {
    let p = Path::new(env!("CARGO_MANIFEST_DIR")).join(format!("../../styles/{name}.json"));
    std::fs::read_to_string(p).unwrap()
}

/// Local path of a `file://` URL (percent-decoded), template tokens left as they are.
fn file_path(url: &str) -> PathBuf {
    let rest = url.strip_prefix("file://").expect(url);
    let bytes = rest.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' {
            let h = std::str::from_utf8(&bytes[i + 1..i + 3]).unwrap();
            out.push(u8::from_str_radix(h, 16).unwrap());
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    PathBuf::from(String::from_utf8(out).unwrap())
}

/// Every URL of an instantiated style points at a file that exists in one of `roots`;
/// returns the number of URLs checked.
fn check_urls(style: &Value, roots: &[&Path]) -> usize {
    let mut n = 0;
    let inside = |p: &Path| {
        assert!(
            roots.iter().any(|r| p.starts_with(r)),
            "{} outside the packs",
            p.display()
        );
        assert!(p.exists(), "missing {}", p.display());
    };
    for src in style["sources"].as_object().unwrap().values() {
        if let Some(u) = src["url"].as_str() {
            inside(&file_path(u.strip_prefix("pmtiles://").unwrap_or(u)));
            n += 1;
        }
        if let Some(d) = src["data"].as_str() {
            inside(&file_path(d));
            n += 1;
        }
    }
    if let Some(g) = style["glyphs"].as_str() {
        let g = g
            .replace("{fontstack}", "Source Sans Pro Bold")
            .replace("{range}", "0-255");
        inside(&file_path(&g));
        n += 1;
    }
    if let Some(s) = style["sprite"].as_str() {
        inside(&file_path(&format!("{s}.json")));
        inside(&file_path(&format!("{s}@2x.png")));
        n += 1;
    }
    n
}

fn layer_ids(style: &Value) -> Vec<&str> {
    style["layers"]
        .as_array()
        .unwrap()
        .iter()
        .map(|l| l["id"].as_str().unwrap())
        .collect()
}

/// The layers of `json` without the pack suffix, every source of a layer present.
fn check_layers(style: &Value) {
    let sources = style["sources"].as_object().unwrap();
    for l in style["layers"].as_array().unwrap() {
        if let Some(s) = l["source"].as_str() {
            assert!(
                sources.contains_key(s),
                "layer {} without source {s}",
                l["id"]
            );
        }
    }
}

fn test_pack(dir: &Path, variant: u8) {
    test_pack::write_test_pack(dir, variant).unwrap();
}

#[test]
fn test_pack_with_every_validated_style() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().join("packs/test");
    test_pack(&dir, 1);
    let dir = std::fs::canonicalize(&dir).unwrap();
    let entries = list_packs(tmp.path().join("packs").display().to_string());
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].site_id, "test");

    let mut closed_rows = 0;
    for name in STYLES {
        let original: Value = serde_json::from_str(&style_json(name)).unwrap();
        let view = open_packs(
            vec![dir.display().to_string()],
            style_json(name),
            false,
            Lang::Fr,
        )
        .unwrap();
        let st = &view.style;
        let v: Value = serde_json::from_str(&st.json).unwrap();
        let (kind, theme) = name.split_once('-').unwrap();
        assert_eq!(st.name, format!("skimap test {kind}-{theme}"));
        assert_eq!((st.style_id.as_str(), st.theme.as_str()), (kind, theme));
        assert!(check_urls(&v, &[&dir]) >= 4, "{name}");
        check_layers(&v);
        // The drawing is the style's: every layer kept is the original one, unchanged
        // apart from its URLs.
        let originals: HashMap<&str, &Value> = original["layers"]
            .as_array()
            .unwrap()
            .iter()
            .map(|l| (l["id"].as_str().unwrap(), l))
            .collect();
        for l in v["layers"].as_array().unwrap() {
            assert_eq!(
                &l,
                originals.get(l["id"].as_str().unwrap()).unwrap(),
                "{name}"
            );
        }
        // Without the OSM basemap, slope and exposure, their layers are left out.
        let ids = layer_ids(&v);
        let pistes = if kind == "guidage" {
            "pistes-context"
        } else {
            "pistes-line"
        };
        assert!(ids.contains(&pistes), "{name}");
        assert!(ids.contains(&"ign-forest"), "{name}");
        assert!(!ids.iter().any(|i| i.starts_with("osm-")), "{name}");
        assert!(!st.missing.iter().any(|m| m == "test: ign"), "{name}");
        if kind != "guidage" {
            assert!(
                st.missing.contains(&"test: osm".to_owned()),
                "{:?}",
                st.missing
            );
        }
        // Groups of the style, safety ones not toggleable, snow hidden by default.
        let groups: Vec<(&str, bool, bool)> = st
            .groups
            .iter()
            .map(|g| (g.id.as_str(), g.toggleable, g.visible_by_default))
            .collect();
        assert!(groups.contains(&("pistes", false, true)), "{groups:?}");
        assert!(groups.contains(&("position", false, true)), "{groups:?}");
        for g in &st.groups {
            assert!(!g.style_layers.is_empty());
            assert!(g.style_layers.iter().all(|l| ids.contains(&l.as_str())));
        }
        match kind {
            "fond" => {
                assert!(groups.contains(&("refuges", true, true)), "{groups:?}");
                let huts = st
                    .overlays
                    .iter()
                    .find(|o| o.pack_layer == "refuges-info")
                    .unwrap();
                // The dark theme draws a light disc under the icons (`huts-disc-info`).
                assert_eq!(huts.style_layers.first().unwrap(), "huts-dot-info");
                assert_eq!(huts.style_layers.last().unwrap(), "huts-info");
            }
            "srn" => {
                assert!(groups.contains(&("points-eau", true, true)), "{groups:?}");
                // No zones layer in the test pack: no empty group.
                assert!(
                    !groups.iter().any(|g| g.0 == "reglementation"),
                    "{groups:?}"
                );
            }
            _ => {
                let rt: Vec<&str> = v["sources"]
                    .as_object()
                    .unwrap()
                    .keys()
                    .map(String::as_str)
                    .filter(|k| ["route", "decision", "position"].contains(k))
                    .collect();
                assert_eq!(rt.len(), 3, "runtime sources kept: {rt:?}");
            }
        }
        // Legend: the colours of the style's own `pistes-line` (guidage: grey context).
        if kind == "guidage" {
            assert!(st.legend.is_empty());
            continue;
        }
        let line = originals["pistes-line"]["paint"]["line-color"]
            .as_array()
            .unwrap();
        let easy = line
            .iter()
            .position(|x| x == "easy")
            .map(|i| line[i + 1].as_str().unwrap())
            .unwrap();
        let row = st.legend.iter().find(|e| e.key == "easy").unwrap();
        assert_eq!((row.color.as_str(), row.kind), (easy, LegendKind::Line));
        // A piste whose state is unknown has its own row (#141): the neutral band of
        // `pistes-unknown-casing` with a thin core, never the solid line of an open piste.
        let band = originals["pistes-unknown-casing"]["paint"]["line-color"]
            .as_array()
            .unwrap()
            .last()
            .unwrap()
            .as_str()
            .unwrap();
        let row = st
            .legend
            .iter()
            .find(|e| e.key == "state-unknown")
            .unwrap_or_else(|| panic!("{name}: no state-unknown row in {:?}", st.legend));
        assert_eq!(
            (row.group.as_str(), row.color.as_str(), row.kind),
            ("pistes", band, LegendKind::Band),
            "{name}"
        );
        assert_eq!(row.core.as_deref(), Some(easy), "{name}");
        assert_ne!(row.color, easy, "{name}: the band is not the open colour");
        // A closed piste (#177) is drawn as on the map: the piste colour veiled, broken
        // by dashes of the ground colour of `pistes-closed-dash`; never the plain dashes
        // of a marked off-piste trail.
        if let Some(dash) = originals.get("pistes-closed-dash") {
            let ground = dash["paint"]["line-color"].as_str().unwrap();
            let row = st
                .legend
                .iter()
                .find(|e| e.key == "closed")
                .unwrap_or_else(|| panic!("{name}: no closed row in {:?}", st.legend));
            assert_eq!(
                (row.color.as_str(), row.kind, row.core.as_deref()),
                (easy, LegendKind::Veiled, Some(ground)),
                "{name}"
            );
            closed_rows += 1;
            if let Some(b) = st.legend.iter().find(|e| e.key == "backcountry") {
                assert_ne!((b.kind, &b.core), (row.kind, &row.core), "{name}");
            }
        }
        // Every other row has no core.
        assert!(
            st.legend
                .iter()
                .filter(|e| e.key != "state-unknown" && e.key != "closed")
                .all(|e| e.core.is_none()),
            "{name}"
        );
        // Attributions of what is drawn, from the manifest.
        assert!(
            st.attributions
                .contains(&"Fond de test synthétique".to_owned())
        );
        assert!(st.attributions.contains(&"pistes synthétiques".to_owned()));
    }
    assert!(closed_rows > 0, "no style draws closed pistes");
    // Piste status of the test pack.
    let view = open_packs(
        vec![dir.display().to_string()],
        style_json("fond-clair"),
        false,
        Lang::Fr,
    )
    .unwrap();
    let s = view.info.status.unwrap();
    assert_eq!((s.open, s.closed), (1, 1));
    // The same set, another style: one call.
    let st = view.set.map_style(style_json("srn-sombre")).unwrap();
    assert_eq!(st.name, "skimap test srn-sombre");
    assert!(
        open_packs(
            vec![dir.display().to_string()],
            "{}".into(),
            false,
            Lang::Fr
        )
        .is_err(),
        "a style without sources is refused"
    );
}

/// A source that leaves the pack is refused.
#[test]
fn paths_outside_the_pack_are_refused() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().join("packs/test");
    test_pack(&dir, 1);
    let style = json!({"version": 8, "metadata": {}, "layers": [],
        "sources": {"x": {"type": "geojson", "data": "../../etc/passwd"}}});
    let e = open_packs(
        vec![dir.display().to_string()],
        style.to_string(),
        false,
        Lang::Fr,
    )
    .unwrap_err();
    assert!(matches!(e, PackError::Invalid { .. }), "{e:?}");
}

/// Free mode: two packs in one style, the order of the style kept across packs.
#[test]
fn two_packs_together_without_seam() {
    let tmp = tempfile::tempdir().unwrap();
    let a = tmp.path().join("packs/a");
    let b = tmp.path().join("packs/b");
    test_pack(&a, 1);
    test_pack(&b, 2);
    let dirs = vec![a.display().to_string(), b.display().to_string()];
    let view = open_packs(dirs, style_json("fond-clair"), true, Lang::Fr).unwrap();
    assert_eq!(view.set.len(), 2);
    assert_eq!(view.info.site_id, "libre");
    assert_eq!(view.style.name, "skimap libre fond-clair");
    let s = view.info.status.as_ref().unwrap();
    assert_eq!((s.open, s.closed), (3, 1));
    let v: Value = serde_json::from_str(&view.style.json).unwrap();
    check_layers(&v);
    let ids = layer_ids(&v);
    let pos = |id: &str| {
        ids.iter()
            .position(|x| *x == id)
            .unwrap_or_else(|| panic!("{id}"))
    };
    // Each layer right after itself for the second pack; background and position once.
    assert_eq!(pos("pistes-line~p1"), pos("pistes-line") + 1);
    assert_eq!(pos("ign-forest~p1"), pos("ign-forest") + 1);
    assert!(pos("ign-forest~p1") < pos("pistes-casing"));
    assert!(pos("pistes-line~p1") < pos("pistes-label"));
    assert_eq!(
        ids.iter().filter(|i| i.starts_with("background")).count(),
        1
    );
    assert_eq!(
        ids.iter().filter(|i| i.starts_with("position-dot")).count(),
        1
    );
    assert!(v["sources"]["position"].is_object());
    assert!(v["sources"].get("position~p1").is_none());
    let refuges = view
        .style
        .groups
        .iter()
        .find(|g| g.id == "refuges")
        .unwrap();
    assert!(refuges.style_layers.contains(&"huts-info~p1".to_owned()));
    let huts = view
        .style
        .overlays
        .iter()
        .find(|o| o.pack_layer == "refuges-info")
        .unwrap();
    assert_eq!(huts.style_layers.len(), 4);
    // A single pack in free mode is still the free mode.
    let one = open_packs(
        vec![a.display().to_string()],
        style_json("fond-clair"),
        true,
        Lang::Fr,
    )
    .unwrap();
    assert_eq!(one.style.name, "skimap libre fond-clair");
}

/// Trail-pass zones (H2): one per pass over the packs of the set, with their sites.
#[test]
fn fee_zones_over_the_packs() {
    let tmp = tempfile::tempdir().unwrap();
    let a = tmp.path().join("packs/a");
    let b = tmp.path().join("packs/b");
    test_pack(&a, 1);
    test_pack(&b, 2);
    let view = open_packs(
        vec![a.display().to_string(), b.display().to_string()],
        style_json("fond-clair"),
        true,
        Lang::Fr,
    )
    .unwrap();
    let zones = view.set.fee_zones().unwrap();
    let ids: Vec<&str> = zones.iter().map(|z| z.fee_id.as_str()).collect();
    assert_eq!(ids, ["test-site", "nordic-pass-national"]);
    // Both packs are the site `test`: listed once.
    assert_eq!(zones[0].sites, ["test"]);
    assert_eq!(zones[0].kind, "site");
    assert_eq!(zones[1].prices[0].price_eur, 240.0);
    assert_eq!(zones[1].prices[0].date.as_deref(), Some("2026-11-01"));
    // The validated styles draw the layer (group `redevance`): its source points into the pack.
    let v: Value = serde_json::from_str(&view.style.json).unwrap();
    assert!(
        v["sources"]
            .as_object()
            .unwrap()
            .values()
            .any(|s| s["data"].as_str().unwrap_or("").contains("fee-zones"))
    );
}

/// The catalogue embedded in the app (written by `examples/catalog.rs`).
#[test]
fn embedded_catalogue() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../android/app/src/main/assets/catalog.json");
    let c = skimap_ffi::Catalog::parse(&std::fs::read_to_string(path).unwrap()).unwrap();
    assert_eq!(c.len(), 219);
    let names = |v: Vec<skimap_ffi::Domain>| v.into_iter().map(|d| d.name).collect::<Vec<_>>();
    let cerv = names(c.search("cervieres".into(), vec![], vec![], 5));
    assert!(cerv.iter().any(|n| n.contains("Cervières")), "{cerv:?}");
    let chap = names(c.search("chapelle des bois".into(), vec![], vec![], 5));
    assert!(chap.iter().any(|n| n.contains("Chapelle")), "{chap:?}");
    // From Chapelle-des-Bois (6.1118, 46.5952), the nearest area is within 10 km.
    let n = c.nearest(6.1118, 46.5952, vec![], vec![], 3);
    assert!(n[0].km < 10.0, "{:?}", n[0]);
    let pts: Vec<f64> = c.outline().into_iter().flatten().collect();
    assert!(pts.len() > 2_000);
    for xy in pts.chunks(2) {
        assert!(
            (-5.3..=9.7).contains(&xy[0]) && (41.2..=51.2).contains(&xy[1]),
            "{xy:?}"
        );
    }
}

// --- Sync ------------------------------------------------------------------------------

/// Writes the bundle from `old` to `new` as the server does (`Update::between`,
/// `write_bundle`) into `dir`; returns its index entry.
fn bundle(old: Option<&Path>, new: &Path, dir: &Path) -> Value {
    let new_pack = Pack::open(new).unwrap();
    let raw = std::fs::read(new.join("manifest.json")).unwrap();
    let old_pack = old.map(|o| Pack::open(o).unwrap());
    let u = Update::between(
        old_pack.as_ref().map(Pack::manifest),
        new_pack.manifest(),
        &raw,
    );
    let by_sha: HashMap<String, PathBuf> = new_pack
        .manifest()
        .files
        .iter()
        .map(|f| (f.sha256.clone(), new.join(&f.path)))
        .collect();
    let manifest_sha = u.manifest.sha256.clone();
    let mut out = Vec::new();
    write_bundle(&mut out, &u, |sha| {
        let p = if sha == manifest_sha {
            new.join("manifest.json")
        } else {
            by_sha[sha].clone()
        };
        Ok(std::fs::File::open(p).unwrap())
    })
    .unwrap();
    let sha = skimap_pack::update::sha256_hex(&out);
    std::fs::create_dir_all(dir).unwrap();
    std::fs::write(dir.join(format!("{sha}.skb")), &out).unwrap();
    json!({"from": old_pack.map(|p| p.manifest().pack_version.clone()),
           "to": new_pack.manifest().pack_version, "sha256": sha, "bytes": out.len(),
           "url": format!("/v1/bundles/{sha}.skb"), "blobs": u.fetch.len()})
}

/// `SiteIndex` of a server holding `base` and deltas to `latest`.
fn index(latest: &Path, base: &Value, deltas: &[Value]) -> String {
    let m = Pack::open(latest).unwrap().manifest().clone();
    json!({"format": "skimap-site-index", "format_version": 1, "site": m.site,
           "latest": {"version": m.pack_version, "published_at": "2026-11-16T06:00:00Z",
                      "manifest_sha256": "", "total_bytes": m.total_bytes,
                      "files": m.files.len(), "valid_for": m.valid_for, "personal_sources": false},
           "base_version": base["to"], "base": base, "deltas": deltas, "changes": []})
    .to_string()
}

fn tree(dir: &Path) -> Vec<(String, String)> {
    let p = Pack::open(dir).unwrap();
    assert!(p.verify().unwrap().is_empty());
    let mut v: Vec<(String, String)> = p
        .manifest()
        .files
        .iter()
        .map(|f| (f.path.clone(), f.sha256.clone()))
        .collect();
    v.sort();
    v
}

/// First download, delta, missing contents, corrupt download, state: what the app does
/// with the bundles it downloads.
#[test]
fn sync_from_bundles() {
    let tmp = tempfile::tempdir().unwrap();
    let (v1, v2) = (
        tmp.path().join("src/v1/test"),
        tmp.path().join("src/v2/test"),
    );
    test_pack(&v1, 1);
    test_pack(&v2, 2);
    let server = tmp.path().join("server");
    let base = bundle(None, &v1, &server);
    let delta = bundle(Some(&v1), &v2, &server);
    let idx = index(&v2, &base, std::slice::from_ref(&delta));
    let phone = tmp.path().join("phone/packs/test");
    let get = |b: &skimap_ffi::sync::BundleInfo| {
        server
            .join(b.url.trim_start_matches("/v1/bundles/"))
            .display()
            .to_string()
    };
    let p = phone.display().to_string();

    // First download: base then delta.
    let plan = sync_plan(idx.clone(), p.clone()).unwrap();
    assert_eq!(plan.installed_version, None);
    assert_eq!(plan.bundles.len(), 2);
    assert_eq!(
        plan.download_bytes,
        base["bytes"].as_u64().unwrap() + delta["bytes"].as_u64().unwrap()
    );
    for b in &plan.bundles {
        verify_bundle(get(b), b.sha256.clone(), b.bytes).unwrap();
        let r = install_bundle(p.clone(), get(b)).unwrap();
        assert_eq!(r.version, b.to);
    }
    assert_eq!(tree(&phone), tree(&v2));
    assert_eq!(sync_state(p.clone()), None, "no state before record_sync");
    record_sync(p.clone(), idx.clone(), Some("\"etag-1\"".into())).unwrap();
    let st = sync_state(p.clone()).unwrap();
    assert_eq!(st.version, test_pack::VERSIONS[1]);
    assert_eq!(st.index_etag.as_deref(), Some("\"etag-1\""));
    assert!(
        sync_plan(idx.clone(), p.clone())
            .unwrap()
            .bundles
            .is_empty()
    );
    assert_eq!(
        list_packs(tmp.path().join("phone/packs").display().to_string()).len(),
        1
    );

    // A pack at v1 copied by hand, without state: the delta only.
    remove_pack(p.clone()).unwrap();
    assert!(!phone.exists() && sync_state(p.clone()).is_none());
    copy_dir(&v1, &phone);
    let plan = sync_plan(idx.clone(), p.clone()).unwrap();
    assert_eq!(
        plan.installed_version.as_deref(),
        Some(test_pack::VERSIONS[0])
    );
    assert_eq!(plan.bundles.len(), 1);
    assert_eq!(plan.bundles[0].sha256, delta["sha256"].as_str().unwrap());
    let r = install_bundle(p.clone(), get(&plan.bundles[0])).unwrap();
    assert!(r.reused_files > r.fetched_files, "{r:?}");
    assert_eq!(tree(&phone), tree(&v2));

    // A delta for contents the phone no longer has (a file it reuses deleted):
    // MissingContents, nothing written, then the full plan.
    remove_pack(p.clone()).unwrap();
    copy_dir(&v1, &phone);
    std::fs::remove_file(phone.join("layers/refuges-camptocamp.geojson")).unwrap();
    let plan = sync_plan(idx.clone(), p.clone()).unwrap();
    assert_eq!(plan.bundles.len(), 1);
    let e = install_bundle(p.clone(), get(&plan.bundles[0])).unwrap_err();
    assert!(matches!(e, SyncError::MissingContents { .. }), "{e:?}");
    assert_eq!(
        Pack::open(&phone).unwrap().manifest().pack_version,
        test_pack::VERSIONS[0],
        "the installed version is left as it was"
    );
    std::fs::remove_dir_all(&phone).unwrap();
    // A truncated download is refused before anything is written.
    let full = sync_plan_full(idx.clone(), p.clone()).unwrap();
    assert_eq!(full.bundles.len(), 2);
    let cut = tmp.path().join("cut.skb");
    let bytes = std::fs::read(get(&full.bundles[0])).unwrap();
    std::fs::write(&cut, &bytes[..bytes.len() / 2]).unwrap();
    let e = verify_bundle(
        cut.display().to_string(),
        full.bundles[0].sha256.clone(),
        full.bundles[0].bytes,
    )
    .unwrap_err();
    assert!(matches!(e, SyncError::Corrupt { .. }), "{e:?}");
    let e = install_bundle(p.clone(), cut.display().to_string()).unwrap_err();
    assert!(matches!(e, SyncError::Corrupt { .. }), "{e:?}");
    assert!(!phone.join("manifest.json").exists());
    // The delta alone on an empty phone: contents missing.
    let e = install_bundle(p.clone(), get(&full.bundles[1])).unwrap_err();
    assert!(matches!(e, SyncError::MissingContents { .. }), "{e:?}");
    for b in &full.bundles {
        install_bundle(p.clone(), get(b)).unwrap();
    }
    assert_eq!(tree(&phone), tree(&v2));
}

fn copy_dir(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).unwrap();
    for e in std::fs::read_dir(from).unwrap() {
        let e = e.unwrap();
        let p = e.path();
        if p.is_dir() {
            copy_dir(&p, &to.join(e.file_name()));
        } else {
            std::fs::copy(&p, to.join(e.file_name())).unwrap();
        }
    }
}

/// The server catalog; the `voice` field of an older server (voice packs, VO6) is ignored.
#[test]
fn server_catalog() {
    let c = json!({"format": "skimap-catalog", "format_version": 1, "sites": [
        {"site": {"id": "test", "name": "Pack de test", "department": "05",
                  "bbox": [6.66, 44.77, 6.86, 44.92], "center": [6.77, 44.86]},
         "index_url": "/v1/sites/test/index.json",
         "latest": {"version": "v2", "published_at": "2026-11-16T06:00:00Z", "manifest_sha256": "",
                    "total_bytes": 24000, "files": 23, "valid_for": {"until": null, "note": ""},
                    "personal_sources": false},
         "first_download_bytes": 25000,
         "voice": [{"lang": "en", "index_url": "/v1/sites/test/voice/en/index.json",
                    "latest": {"version": "v1", "published_at": "", "manifest_sha256": "",
                               "total_bytes": 1, "files": 1, "valid_for": {"until": null, "note": ""},
                               "personal_sources": false},
                    "first_download_bytes": 4000000}]}]});
    let sites = skimap_ffi::sync::parse_server_catalog(c.to_string()).unwrap();
    assert_eq!(sites.len(), 1);
    assert_eq!(sites[0].site_id, "test");
    assert_eq!(sites[0].bbox, [6.66, 44.77, 6.86, 44.92]);
    assert_eq!(sites[0].first_download_bytes, 25000);
    let mut no_voice = c.clone();
    no_voice["sites"][0]
        .as_object_mut()
        .unwrap()
        .remove("voice");
    assert_eq!(
        skimap_ffi::sync::parse_server_catalog(no_voice.to_string()).unwrap(),
        sites
    );
    assert!(skimap_ffi::sync::parse_server_catalog("{\"format\": \"x\"}".into()).is_err());
}

fn real_pack(site: &str) -> PathBuf {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../data/out");
    std::fs::canonicalize(root.join(site).join("pack")).unwrap()
}

/// The six styles on the real packs, alone and together: every source of the styles is
/// in the packs, every URL resolves, timings.
#[test]
#[ignore = "needs the real packs in data/out/<site>/pack/"]
fn real_packs_with_every_style() {
    let sites = ["cervieres", "chapelle-des-bois"];
    for site in sites {
        let dir = real_pack(site);
        for name in STYLES {
            let t = Instant::now();
            let view = open_packs(
                vec![dir.display().to_string()],
                style_json(name),
                false,
                Lang::Fr,
            )
            .unwrap();
            let ms = t.elapsed().as_secs_f64() * 1e3;
            let v: Value = serde_json::from_str(&view.style.json).unwrap();
            let n = check_urls(&v, &[&dir]);
            check_layers(&v);
            let original: Value = serde_json::from_str(&style_json(name)).unwrap();
            // Every layer is kept, except those of a source the style declares optional
            // (`skimap:optional_sources`) and the pack does not have.
            let optional: Vec<&str> = original["metadata"]["skimap:optional_sources"]
                .as_array()
                .map(|a| a.iter().filter_map(|x| x.as_str()).collect())
                .unwrap_or_default();
            let kept: std::collections::HashSet<&str> = v["layers"]
                .as_array()
                .unwrap()
                .iter()
                .filter_map(|l| l["id"].as_str())
                .collect();
            for l in original["layers"].as_array().unwrap() {
                let id = l["id"].as_str().unwrap();
                if !kept.contains(id) {
                    let src = l["source"].as_str().unwrap_or("");
                    assert!(
                        optional.contains(&src),
                        "{site} {name}: layer {id} dropped but its source {src} is not optional"
                    );
                }
            }
            let missing: Vec<&String> = view
                .style
                .missing
                .iter()
                .filter(|m| !optional.iter().any(|o| m.ends_with(&format!(": {o}"))))
                .collect();
            assert!(missing.is_empty(), "{site} {name}: {missing:?}");
            let t = Instant::now();
            let again = view.set.map_style(style_json(name)).unwrap();
            let restyle_ms = t.elapsed().as_secs_f64() * 1e3;
            assert_eq!(again.json, view.style.json);
            println!(
                "{site} {name}: open + style {ms:.1} ms, style again {restyle_ms:.1} ms, {} layers, \
                 {n} URLs checked, {} groups, legend {} rows, {} attributions",
                v["layers"].as_array().unwrap().len(),
                view.style.groups.len(),
                view.style.legend.len(),
                view.style.attributions.len()
            );
        }
    }
    let dirs: Vec<PathBuf> = sites.iter().map(|s| real_pack(s)).collect();
    let roots: Vec<&Path> = dirs.iter().map(PathBuf::as_path).collect();
    for name in STYLES {
        let t = Instant::now();
        let view = open_packs(
            dirs.iter().map(|d| d.display().to_string()).collect(),
            style_json(name),
            true,
            Lang::Fr,
        )
        .unwrap();
        let ms = t.elapsed().as_secs_f64() * 1e3;
        let v: Value = serde_json::from_str(&view.style.json).unwrap();
        check_urls(&v, &roots);
        check_layers(&v);
        println!(
            "free mode {name}: open + style {ms:.1} ms, {} layers, {} bytes of style",
            v["layers"].as_array().unwrap().len(),
            view.style.json.len()
        );
    }
}

/// A France basemap directory (X20): description, archive and the Regular glyphs.
fn france_dir(dir: &Path) {
    use skimap_pack::france::{FONTS_DIR, FORMAT, FORMAT_VERSION, FranceBasemap, TILES_FILE};
    use skimap_pack::manifest::{FileEntry, Source};
    std::fs::create_dir_all(dir.join(FONTS_DIR).join("Source Sans Pro Regular")).unwrap();
    std::fs::write(dir.join(TILES_FILE), b"PMTiles").unwrap();
    std::fs::write(
        dir.join(FONTS_DIR)
            .join("Source Sans Pro Regular/0-255.pbf"),
        b"glyphs",
    )
    .unwrap();
    let entry = |rel: &str| {
        let (bytes, sha256) = skimap_pack::reader::sha256_file(&dir.join(rel)).unwrap();
        FileEntry {
            path: rel.to_owned(),
            bytes,
            sha256,
        }
    };
    let d = FranceBasemap {
        format: FORMAT.to_owned(),
        format_version: FORMAT_VERSION,
        build: "20261117.pmtiles".to_owned(),
        schema: "4.15.2".to_owned(),
        data_date: "2026-11-17".to_owned(),
        bbox: [-5.5, 41.2, 9.8, 51.3],
        minzoom: 0,
        maxzoom: 6,
        files: vec![
            entry("fonts/Source Sans Pro Regular/0-255.pbf"),
            entry(TILES_FILE),
        ],
        sources: vec![Source::new(
            "Protomaps",
            "https://build.protomaps.com/20261117.pmtiles",
            "ODbL 1.0",
            "© les contributeurs d'OpenStreetMap, Protomaps",
        )],
    };
    std::fs::write(
        dir.join(skimap_pack::france::DESCRIPTION_FILE),
        serde_json::to_vec(&d).unwrap(),
    )
    .unwrap();
}

/// Free mode without any pack (X20): the France basemap alone, with its glyphs, every
/// style; without France basemap either, an error.
#[test]
fn france_basemap_alone_without_pack() {
    use skimap_ffi::open_map;
    let tmp = tempfile::tempdir().unwrap();
    let fr = tmp.path().join("france");
    france_dir(&fr);
    for name in STYLES {
        let view = open_map(
            vec![],
            Some(fr.display().to_string()),
            style_json(name),
            true,
            Lang::Fr,
        )
        .unwrap();
        assert_eq!(view.set.len(), 0);
        assert_eq!(view.info.site_id, "libre");
        // Not « all the installed areas »: there is none (#162).
        assert_eq!(view.info.site_name, "France, aucun domaine installé");
        assert_eq!(
            view.info.validity_note,
            "Aucun pack installé : fond de France seul, données du 2026-11-17."
        );
        assert_eq!(view.info.basemaps[0].id, "france");
        assert!(view.info.bounds.west < -5.0 && view.info.bounds.east > 9.5);
        let v: Value = serde_json::from_str(&view.style.json).unwrap();
        check_layers(&v);
        let ids = layer_ids(&v);
        assert!(ids.contains(&"france-water"), "{name}: {ids:?}");
        assert!(ids.contains(&"france-place"), "{name}: {ids:?}");
        // Only the background, the France basemap and the runtime sources.
        for l in v["layers"].as_array().unwrap() {
            let src = l["source"].as_str();
            assert!(
                matches!(
                    src,
                    None | Some("france" | "position" | "selection" | "route" | "decision")
                ),
                "{name}: {} from {src:?}",
                l["id"]
            );
        }
        let url = v["sources"]["france"]["url"].as_str().unwrap();
        let path = file_path(url.strip_prefix("pmtiles://").unwrap());
        assert_eq!(path, fr.join("france.pmtiles"));
        let g = v["glyphs"]
            .as_str()
            .unwrap()
            .replace("{fontstack}", "Source Sans Pro Regular")
            .replace("{range}", "0-255");
        assert!(file_path(&g).is_file(), "{g}");
        assert!(v.get("sprite").is_none());
        assert_eq!(
            view.style.attributions,
            vec!["© les contributeurs d'OpenStreetMap, Protomaps".to_owned()]
        );
    }
    let e = open_map(vec![], None, style_json("fond-clair"), true, Lang::Fr).unwrap_err();
    assert!(matches!(e, PackError::Invalid { .. }), "{e:?}");
    // A France basemap in area mode is not a pack.
    assert!(
        open_map(
            vec![],
            Some(fr.display().to_string()),
            style_json("fond-clair"),
            false,
            Lang::Fr
        )
        .is_err()
    );
}

/// With packs, the France basemap is drawn once, under every pack; without it, its
/// layers are left out and named in `missing`.
#[test]
fn france_basemap_under_the_packs() {
    use skimap_ffi::open_map;
    let tmp = tempfile::tempdir().unwrap();
    let fr = tmp.path().join("france");
    france_dir(&fr);
    let a = tmp.path().join("packs/a");
    let b = tmp.path().join("packs/b");
    test_pack(&a, 1);
    test_pack(&b, 2);
    let dirs = vec![a.display().to_string(), b.display().to_string()];
    let view = open_map(
        dirs.clone(),
        Some(fr.display().to_string()),
        style_json("srn-clair"),
        true,
        Lang::Fr,
    )
    .unwrap();
    let v: Value = serde_json::from_str(&view.style.json).unwrap();
    check_layers(&v);
    check_urls(&v, &[tmp.path()]);
    let ids = layer_ids(&v);
    let pos = |id: &str| {
        ids.iter()
            .position(|x| *x == id)
            .unwrap_or_else(|| panic!("{id}"))
    };
    assert_eq!(
        ids.iter().filter(|i| i.starts_with("france-water")).count(),
        1
    );
    assert!(v["sources"].get("france~p1").is_none());
    assert!(pos("background") < pos("france-water"));
    assert!(pos("france-road") < pos("ign-forest"));
    assert!(pos("france-place") < pos("ign-place-commune-low"));
    // The glyphs stay those of the packs.
    assert!(v["glyphs"].as_str().unwrap().contains("/packs/a/"));
    // Free mode: the camera may go anywhere in France.
    assert!(view.info.bounds.west <= -5.5 && view.info.bounds.north >= 51.3);
    assert!(view.info.basemaps.iter().any(|b| b.id == "france"));
    assert!(view.style.missing.iter().all(|m| !m.contains("france")));
    assert!(
        view.style
            .attributions
            .iter()
            .any(|a| a.contains("Protomaps"))
    );

    // A France basemap that does not open is left out when packs open.
    let broken = open_map(
        dirs.clone(),
        Some(tmp.path().join("nowhere").display().to_string()),
        style_json("srn-clair"),
        true,
        Lang::Fr,
    )
    .unwrap();
    let v: Value = serde_json::from_str(&broken.style.json).unwrap();
    assert!(layer_ids(&v).iter().all(|i| !i.starts_with("france")));
    assert!(v["sources"].get("france").is_none());
    assert!(broken.style.missing.iter().any(|m| m.contains("france")));
    // Same through open_packs (no France basemap).
    let old = open_packs(dirs, style_json("srn-clair"), true, Lang::Fr).unwrap();
    assert_eq!(old.style.json, broken.style.json);
}

/// The bearing targets come from the refuge layers every pack holds (`refuges-info`,
/// `refuges-osm`, flagged for the plan, X21), not from the merged sheets most packs lack
/// (#162): the huts of the plan, a hut of both sources once, a water point or a bus
/// shelter never.
#[test]
fn fog_targets_offer_the_refuges_of_every_pack() {
    let tmp = tempfile::tempdir().unwrap();
    test_pack(tmp.path(), 1);
    let pack = Pack::open(tmp.path()).unwrap();
    assert!(
        pack.layer("refuges").is_err(),
        "no merged sheets in this pack"
    );
    let targets =
        skimap_ffi::fog::fog_targets(tmp.path().display().to_string(), None, None, Lang::Fr, 0)
            .unwrap();
    let mut refuges: Vec<&str> = targets
        .iter()
        .filter(|t| t.kind == "refuge")
        .map(|t| t.name.as_str())
        .collect();
    refuges.sort_unstable();
    assert_eq!(refuges, ["Abri OSM", "Cabane du test", "Refuge du test"]);
}

/// Pack layers drawn by the instantiated style `v` of the pack in `dir`: the layers whose
/// file is the source of a style layer.
fn drawn_layers(v: &Value, pack: &Pack, dir: &Path, out: &mut std::collections::HashSet<String>) {
    let used: std::collections::HashSet<&str> = v["layers"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|l| l["source"].as_str())
        .collect();
    for (id, src) in v["sources"].as_object().unwrap() {
        if !used.contains(id.as_str()) {
            continue;
        }
        let Some(url) = src["url"].as_str().or_else(|| src["data"].as_str()) else {
            continue;
        };
        let Some(rest) = url.strip_prefix("pmtiles://").or(Some(url)) else {
            continue;
        };
        if !rest.starts_with("file://") {
            continue;
        }
        let path = file_path(rest);
        for l in &pack.manifest().layers {
            if path == dir.join(&l.file) {
                out.insert(l.id.clone());
            }
        }
    }
}

/// A kind of information none of whose map layers is drawn, with those layers.
type Undrawn = (String, Vec<String>);

/// Checks of K3 (#53) on one pack: every layer is in exactly one kind of information,
/// every kind has its date and its sources; every kind with a map layer is drawn by one of
/// the six styles (the piste state colours the pistes and is counted, with its time, in
/// the header: `PackInfo::status`). Returns the kinds, the number of layers drawn and the
/// kinds none of whose map layers is drawn, with those layers.
fn check_infos(site: &str, dir: &Path, pack: &Pack) -> (Vec<String>, usize, Vec<Undrawn>) {
    let mut drawn = std::collections::HashSet::new();
    let mut info = None;
    for name in STYLES {
        let view = open_packs(
            vec![dir.display().to_string()],
            style_json(name),
            false,
            Lang::Fr,
        )
        .unwrap();
        let v: Value = serde_json::from_str(&view.style.json).unwrap();
        drawn_layers(&v, pack, dir, &mut drawn);
        info = Some(view.info);
    }
    let info = info.unwrap();
    let infos = &info.infos;
    let mut undrawn = Vec::new();
    for l in &pack.manifest().layers {
        let n = infos.iter().filter(|i| i.layers.contains(&l.id)).count();
        assert_eq!(n, 1, "{site}: layer {} in {n} kinds", l.id);
    }
    for i in infos {
        assert!(
            skimap_ffi::INFO_KINDS.contains(&i.id.as_str()),
            "{site}: layer of no known kind: {} ({})",
            i.id,
            i.title
        );
        assert!(!i.sources.is_empty(), "{site} {}: no source", i.id);
        assert!(
            i.sources.iter().all(|s| !s.trim().is_empty()),
            "{site} {}: {:?}",
            i.id,
            i.sources
        );
        assert!(
            i.updated_at.parse::<Timestamp>().is_ok(),
            "{site} {}: update date {:?}",
            i.id,
            i.updated_at
        );
        if i.id == "status" {
            let s = info.status.as_ref().expect("the piste state in the header");
            assert!(s.polled_at.is_some(), "{site}: state without its time");
            continue;
        }
        if i.id == "bera" || i.id == "weather" {
            // Not map layers: the morning and the plan say them (checked on the sites).
            continue;
        }
        let map_layers: Vec<String> = i
            .layers
            .iter()
            .filter(|l| {
                pack.manifest()
                    .layer(l)
                    .is_some_and(|m| m.kind != skimap_pack::manifest::LayerKind::Data)
            })
            .cloned()
            .collect();
        if !map_layers.is_empty() && !map_layers.iter().any(|l| drawn.contains(l)) {
            undrawn.push((i.id.clone(), map_layers));
        }
    }
    (
        infos.iter().map(|i| i.id.clone()).collect(),
        drawn.len(),
        undrawn,
    )
}

/// K3 (#53) on the test pack: each kind of information with its date and sources.
#[test]
fn every_info_of_the_test_pack_is_dated_and_sourced() {
    let tmp = tempfile::tempdir().unwrap();
    test_pack(tmp.path(), 1);
    let pack = Pack::open(tmp.path()).unwrap();
    let dir = std::fs::canonicalize(tmp.path()).unwrap();
    let (kinds, drawn, undrawn) = check_infos("test", &dir, &pack);
    println!("test pack: {kinds:?}, {drawn} layers drawn");
    assert!(undrawn.is_empty(), "{undrawn:?}");
    assert!(kinds.contains(&"pistes".to_owned()), "{kinds:?}");
}

/// K3 (#53), « toutes les couches visibles et datées », on every pack of the data directory
/// (`SKIMAP_DATA_DIR`): every layer in one kind of information, each with its update date
/// and its sources, each kind with a map layer drawn by the styles; both evaluation sites
/// together (free mode) keep the oldest dates. A kind not drawn is allowed only for layers
/// in the raster format of packs built before the snow became vector tiles (the builder
/// writes no raster layer any more, `raster::catalog` is empty; the styles draw the vector
/// tiles): those packs are listed, to rebuild.
#[test]
#[ignore = "needs the real packs in $SKIMAP_DATA_DIR/out/<site>/pack/"]
fn real_packs_every_info_shown_dated_and_sourced() {
    let root = std::env::var("SKIMAP_DATA_DIR").map_or_else(
        |_| Path::new(env!("CARGO_MANIFEST_DIR")).join("../../data"),
        PathBuf::from,
    );
    let mut dirs: Vec<PathBuf> = std::fs::read_dir(root.join("out"))
        .unwrap()
        .flatten()
        .map(|e| e.path().join("pack"))
        .filter(|p| p.join("manifest.json").is_file())
        .collect();
    dirs.sort();
    let mut seen: HashMap<String, usize> = HashMap::new();
    let mut stale = Vec::new();
    for dir in &dirs {
        let dir = &std::fs::canonicalize(dir).unwrap();
        let pack = Pack::open(dir).unwrap();
        let site = pack.manifest().site.id.clone();
        let (kinds, drawn, undrawn) = check_infos(&site, dir, &pack);
        for (kind, layers) in undrawn {
            let raster = layers.iter().all(|l| {
                pack.manifest()
                    .layer(l)
                    .is_some_and(|m| m.kind == skimap_pack::manifest::LayerKind::Raster)
            });
            assert!(
                raster,
                "{site} {kind}: none of {layers:?} drawn by the styles"
            );
            stale.push(format!("{site} ({kind}: {})", layers.join(", ")));
        }
        for k in &kinds {
            *seen.entry(k.clone()).or_default() += 1;
        }
        println!(
            "{site}: {} kinds, {drawn} layers drawn: {kinds:?}",
            kinds.len()
        );
    }
    let mut seen: Vec<_> = seen.into_iter().collect();
    seen.sort();
    println!("{} packs; packs per kind: {seen:?}", dirs.len());
    println!(
        "{} packs built before the vector snow tiles, to rebuild: {stale:?}",
        stale.len()
    );
    assert!(dirs.len() >= 30, "{} packs only", dirs.len());
    for k in [
        "pistes", "slopes", "snow", "zones", "refuges", "network", "bera", "weather",
    ] {
        assert!(seen.iter().any(|(s, _)| s == k), "no pack has {k}");
    }
    // The BERA and the weather, not drawn: the weather in the morning's briefing, the BERA
    // in the validated plan.
    for s in ["cervieres", "chapelle-des-bois"] {
        let dir = root.join("out").join(s).join("pack");
        let has_bera = Pack::open(&dir).unwrap().manifest().layer("bera").is_some();
        let work = tempfile::tempdir().unwrap();
        let planner = skimap_ffi::tracking_day::open_day_planner(
            dir.display().to_string(),
            work.path().join("view").display().to_string(),
            Lang::Fr,
        )
        .unwrap();
        let start = skimap_ffi::tracking_day::day_starts(s.into(), Lang::Fr).remove(0);
        let o = skimap_ffi::tracking_day::day_default_outing();
        let m = planner
            .morning(
                skimap_ffi::tracking_day::DayRequest {
                    label: "test".into(),
                    start,
                    date: "2026-11-16".into(),
                    start_time: Some("10:00".into()),
                    earliest: "08:00".into(),
                    budget_min: o.budget_min,
                    skating: o.skating,
                    return_before: None,
                    max_difficulty: None,
                },
                None,
            )
            .unwrap();
        // The validated plan says the BERA of the day, or that it is missing.
        let v = planner
            .validate(0, None, work.path().join("plan").display().to_string())
            .unwrap();
        // Its avalanche sentence (`[synthesis]`), not the closing « lisez le BERA ».
        let bera: Vec<&str> = v
            .text
            .lines()
            .filter(|l| l.contains("[synthesis]") && (l.contains("BERA") || l.contains("bulletin")))
            .collect();
        println!("{s}: BERA said {bera:?}");
        assert!(
            m.briefing.iter().any(|b| b.contains("°C")),
            "{s}: no weather"
        );
        if has_bera {
            assert!(!bera.is_empty(), "{s}: no BERA in {}", v.text);
        }
    }
    // Both evaluation sites together (free mode): the oldest dates of either.
    let two: Vec<PathBuf> = ["cervieres", "chapelle-des-bois"]
        .iter()
        .map(|s| root.join("out").join(s).join("pack"))
        .collect();
    let one: Vec<skimap_ffi::PackInfo> = two
        .iter()
        .map(|d| {
            open_packs(
                vec![d.display().to_string()],
                style_json("fond-clair"),
                false,
                Lang::Fr,
            )
            .unwrap()
            .info
        })
        .collect();
    let both = open_packs(
        two.iter().map(|d| d.display().to_string()).collect(),
        style_json("fond-clair"),
        true,
        Lang::Fr,
    )
    .unwrap()
    .info;
    assert_eq!(
        both.generated_at,
        one[0].generated_at.clone().min(one[1].generated_at.clone())
    );
    for i in &both.infos {
        let dates: Vec<&String> = one
            .iter()
            .filter_map(|p| p.infos.iter().find(|x| x.id == i.id))
            .map(|x| &x.updated_at)
            .collect();
        assert_eq!(Some(&i.updated_at), dates.iter().min().copied(), "{}", i.id);
    }
}
