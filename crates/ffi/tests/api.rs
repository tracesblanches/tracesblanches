//! The API seen by Kotlin, on the test pack (always) and on the real packs of both sites
//! (`--ignored`, needs `data/out/<site>/pack/`): the validated styles of `styles/`
//! instantiated on one and several packs, the catalogue, and the sync of a pack from
//! bundles made exactly as the server makes them.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::Instant;

use serde_json::{Value, json};
use skimap_ffi::sync::{
    SyncError, install_bundle, record_sync, remove_pack, sync_plan, sync_plan_full, sync_state,
    verify_bundle,
};
use skimap_ffi::{LegendKind, PackError, list_packs, open_packs};
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

    for name in STYLES {
        let original: Value = serde_json::from_str(&style_json(name)).unwrap();
        let view = open_packs(vec![dir.display().to_string()], style_json(name), false).unwrap();
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
        // Attributions of what is drawn, from the manifest.
        assert!(
            st.attributions
                .contains(&"Fond de test synthétique".to_owned())
        );
        assert!(st.attributions.contains(&"pistes synthétiques".to_owned()));
    }
    // Piste status of the test pack.
    let view = open_packs(
        vec![dir.display().to_string()],
        style_json("fond-clair"),
        false,
    )
    .unwrap();
    let s = view.info.status.unwrap();
    assert_eq!((s.open, s.closed), (1, 1));
    // The same set, another style: one call.
    let st = view.set.map_style(style_json("srn-sombre")).unwrap();
    assert_eq!(st.name, "skimap test srn-sombre");
    assert!(
        open_packs(vec![dir.display().to_string()], "{}".into(), false).is_err(),
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
    let e = open_packs(vec![dir.display().to_string()], style.to_string(), false).unwrap_err();
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
    let view = open_packs(dirs, style_json("fond-clair"), true).unwrap();
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

/// The server catalog, with and without voice packs.
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
    assert_eq!(sites[0].voice[0].lang, "en");
    let mut no_voice = c.clone();
    no_voice["sites"][0]
        .as_object_mut()
        .unwrap()
        .remove("voice");
    assert!(
        skimap_ffi::sync::parse_server_catalog(no_voice.to_string()).unwrap()[0]
            .voice
            .is_empty()
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
            let view =
                open_packs(vec![dir.display().to_string()], style_json(name), false).unwrap();
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
