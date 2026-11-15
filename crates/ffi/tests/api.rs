//! The API seen by Kotlin, on the test pack (always) and on the real packs of both sites
//! (`--ignored`, needs `data/out/<site>/pack/`).

use std::path::{Path, PathBuf};
use std::time::Instant;

use serde_json::Value;
use skimap_ffi::{PackError, list_packs, map_symbols, open_pack};

#[path = "../examples/test_pack.rs"]
#[allow(dead_code)]
mod test_pack;

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

/// Every URL of a composed style points at a file of the pack that exists.
fn check_urls(style: &Value, root: &Path, font: Option<&str>) -> usize {
    let mut n = 0;
    let inside = |p: &Path| {
        assert!(
            p.starts_with(root),
            "{} outside {}",
            p.display(),
            root.display()
        );
        assert!(p.is_file(), "missing {}", p.display());
    };
    for (id, src) in style["sources"].as_object().unwrap() {
        if let Some(u) = src["url"].as_str() {
            let u = u.strip_prefix("pmtiles://").unwrap_or(u);
            inside(&file_path(u));
            n += 1;
        }
        if let Some(d) = src["data"].as_str() {
            assert!(id.starts_with("skimap-"), "{id}");
            inside(&file_path(d));
            n += 1;
        }
    }
    if let Some(g) = style["glyphs"].as_str() {
        let font = font.expect("glyphs without overlay font");
        let g = g.replace("{fontstack}", font).replace("{range}", "0-255");
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

fn style_layers(style: &Value) -> Vec<&str> {
    style["layers"]
        .as_array()
        .unwrap()
        .iter()
        .map(|l| l["id"].as_str().unwrap())
        .collect()
}

#[test]
fn test_pack_through_the_api() {
    let tmp = tempfile::tempdir().unwrap();
    let packs = tmp.path().join("packs");
    let dir = packs.join("test");
    test_pack::write_test_pack(&dir).unwrap();
    // A broken pack and a directory without manifest next to it.
    std::fs::create_dir_all(packs.join("broken")).unwrap();
    std::fs::write(packs.join("broken/manifest.json"), b"{").unwrap();
    std::fs::create_dir_all(packs.join("empty")).unwrap();

    let list = list_packs(packs.display().to_string());
    assert_eq!(list.len(), 2, "{list:?}");
    let broken = list.iter().find(|e| e.site_id == "broken").unwrap();
    assert!(broken.error.is_some());
    let ok = list.iter().find(|e| e.site_name == "Pack de test").unwrap();
    assert_eq!(ok.error, None);
    assert_eq!(ok.dir, dir.display().to_string());
    assert!(list_packs("/nonexistent/skimap".into()).is_empty());

    let view = open_pack(ok.dir.clone(), None).unwrap();
    let h = view.handle.clone();
    let info = h.info();
    assert_eq!(view.info, info);
    assert_eq!(view.style, h.map_style(None).unwrap());
    assert_eq!(view.legend, h.legend());
    assert_eq!(view.symbols, map_symbols());
    assert_eq!(info.site_id, "test");
    assert_eq!(info.basemaps.len(), 1);
    assert_eq!(info.basemaps[0].id, "test");
    let overlays: Vec<(&str, u64)> = info
        .overlays
        .iter()
        .map(|o| (o.id.as_str(), o.features))
        .collect();
    assert_eq!(
        overlays,
        [
            ("status", 2),
            ("pistes", 6),
            ("refuges-camptocamp", 1),
            ("refuges-osm", 2),
            ("refuges-info", 2)
        ]
    );
    assert!(info.overlays[0].personal_use_only);
    let s = info.status.clone().unwrap();
    assert_eq!((s.open, s.closed, s.unknown), (1, 1, 0));
    assert_eq!(s.polled_at.as_deref(), Some("2026-11-15T01:45:36Z"));
    assert_eq!(info.valid_until.as_deref(), Some("2027-04-30"));
    assert!(
        info.attributions
            .contains(&"Fond de test synthétique".to_owned())
    );

    let st = h.map_style(None).unwrap();
    assert_eq!(st.basemap.as_deref(), Some("test"));
    assert_eq!(st.font, None, "the test basemap has no glyphs");
    let v: Value = serde_json::from_str(&st.json).unwrap();
    assert_eq!(v["version"], 8);
    assert_eq!(check_urls(&v, &dir, None), 6);
    let ids = style_layers(&v);
    // Basemap first, then lines, then symbols, refuges on top; no labels without glyphs.
    assert_eq!(&ids[..3], ["background", "snow", "grid"]);
    let pos = |id: &str| ids.iter().position(|x| *x == id).expect(id);
    assert!(pos("skimap-status-halo") < pos("skimap-pistes-line"));
    assert!(pos("skimap-pistes-line") < pos("skimap-pistes-difficulty"));
    assert!(pos("skimap-pistes-difficulty") < pos("skimap-refuges-info-icon"));
    assert!(!ids.iter().any(|x| x.ends_with("-label")));
    assert_eq!(st.overlays.len(), 5);
    assert!(
        st.overlays
            .iter()
            .all(|o| o.style_layers.iter().all(|l| ids.contains(&l.as_str())))
    );
    // Every image the style asks for is a symbol the app draws.
    let symbols: Vec<String> = map_symbols().into_iter().map(|s| s.id).collect();
    let text = st.json.clone();
    for id in &symbols {
        assert!(text.contains(id.as_str()), "{id} unused");
    }
    assert!(st.attribution_short.starts_with("Fond de test synthétique"));
    // Switches of the panel "Couches": secondary elements only, existing layers only.
    let toggles: Vec<&str> = st.toggles.iter().map(|t| t.id.as_str()).collect();
    assert_eq!(toggles, ["refuges", "water"]);
    for t in &st.toggles {
        assert!(t.visible_by_default);
        assert!(
            t.style_layers.iter().all(|l| ids.contains(&l.as_str())),
            "{t:?}"
        );
        assert!(
            t.style_layers
                .iter()
                .all(|l| !l.starts_with("skimap-pistes") && !l.starts_with("skimap-status"))
        );
    }

    let legend = h.legend();
    assert!(legend.iter().any(|e| e.label.contains("Rouge")));
    assert!(legend.iter().any(|e| e.label.contains("fermé")));
    for e in &legend {
        if let Some(s) = &e.symbol {
            assert!(symbols.contains(s), "{s}");
        }
    }

    match open_pack(ok.dir.clone(), Some("plan-ign".into())) {
        Err(PackError::UnknownBasemap { id }) => assert_eq!(id, "plan-ign"),
        other => panic!("{other:?}"),
    }
    match h.map_style(Some("plan-ign".into())) {
        Err(PackError::UnknownBasemap { id }) => assert_eq!(id, "plan-ign"),
        other => panic!("{other:?}"),
    }
    match open_pack(packs.join("empty").display().to_string(), None) {
        Err(PackError::NotFound { .. }) => {}
        other => panic!("{other:?}"),
    }
    match open_pack(packs.join("broken").display().to_string(), None) {
        Err(PackError::Invalid { detail }) => assert!(detail.contains("JSON"), "{detail}"),
        other => panic!("{other:?}"),
    }
}

#[test]
fn pack_without_status_has_no_status_layer() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().join("p");
    test_pack::write_test_pack(&dir).unwrap();
    // Drop the status layer from the manifest, as a pack built without personal sources.
    let mpath = dir.join("manifest.json");
    let mut m: Value = serde_json::from_slice(&std::fs::read(&mpath).unwrap()).unwrap();
    m["layers"]
        .as_array_mut()
        .unwrap()
        .retain(|l| l["id"] != "status");
    std::fs::write(&mpath, serde_json::to_vec(&m).unwrap()).unwrap();
    let h = open_pack(dir.display().to_string(), None).unwrap().handle;
    assert!(h.info().status.is_none());
    let st = h.map_style(None).unwrap();
    assert!(!st.json.contains("skimap-status"));
    assert!(
        h.legend()
            .iter()
            .any(|e| e.label == "État des pistes absent de ce pack")
    );
}

#[test]
fn piste_names_are_points_with_their_shape() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().join("p");
    test_pack::write_test_pack(&dir).unwrap();
    // Give the test basemap glyphs: labels need a font.
    let font = dir.join("basemap/test/fonts/Noto Sans Medium");
    std::fs::create_dir_all(&font).unwrap();
    std::fs::write(font.join("0-255.pbf"), b"").unwrap();
    let spath = dir.join("basemap/test/style.json");
    let mut st: Value = serde_json::from_slice(&std::fs::read(&spath).unwrap()).unwrap();
    st["glyphs"] = "basemap/test/fonts/{fontstack}/{range}.pbf".into();
    std::fs::write(&spath, serde_json::to_vec(&st).unwrap()).unwrap();

    let h = open_pack(dir.display().to_string(), None).unwrap().handle;
    let style = h.map_style(None).unwrap();
    assert_eq!(style.font.as_deref(), Some("Noto Sans Medium"));
    let v: Value = serde_json::from_str(&style.json).unwrap();
    let labels = &v["sources"]["skimap-pistes-labels"]["data"]["features"];
    let mut names: Vec<(&str, &str)> = labels
        .as_array()
        .unwrap()
        .iter()
        .map(|f| {
            (
                f["properties"]["name"].as_str().unwrap(),
                f["properties"]["difficulty"].as_str().unwrap(),
            )
        })
        .collect();
    names.sort_unstable();
    assert_eq!(
        names,
        [
            ("Bleue des Fonts", "easy"),
            ("Noire de l'Izoard", "advanced"),
            ("Rouge du Laus", "intermediate"),
            ("Sans couleur", ""),
            ("Sentier raquettes", "easy"),
            ("Verte du village", "novice")
        ]
    );
    // The label of the blue piste is halfway along it: its middle vertex (6.7746, 44.8586).
    let blue = labels
        .as_array()
        .unwrap()
        .iter()
        .find(|f| f["properties"]["name"] == "Bleue des Fonts")
        .unwrap();
    let c = blue["geometry"]["coordinates"].as_array().unwrap();
    assert!((c[0].as_f64().unwrap() - 6.7746).abs() < 1e-4, "{c:?}");
    let layer = v["layers"]
        .as_array()
        .unwrap()
        .iter()
        .find(|l| l["id"] == "skimap-pistes-label")
        .unwrap();
    assert_eq!(layer["source"], "skimap-pistes-labels");
    assert!(
        layer["layout"]["icon-image"]
            .to_string()
            .contains("skimap-difficulty-easy")
    );
}

fn real_pack(site: &str) -> PathBuf {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../data/out");
    root.join(site).join("pack")
}

#[test]
#[ignore = "needs the real packs in data/out/<site>/pack/"]
fn real_packs() {
    for (site, n_pistes) in [("cervieres", 196), ("chapelle-des-bois", 691)] {
        let dir = std::fs::canonicalize(real_pack(site)).unwrap();
        let t = Instant::now();
        let h = open_pack(dir.display().to_string(), None).unwrap().handle;
        let open_ms = t.elapsed().as_secs_f64() * 1e3;
        let info = h.info();
        assert_eq!(info.site_id, site);
        let pistes = info.overlays.iter().find(|o| o.id == "pistes").unwrap();
        assert_eq!(pistes.features, n_pistes);
        let ids: Vec<&str> = info.basemaps.iter().map(|b| b.id.as_str()).collect();
        assert_eq!(ids, ["plan-ign", "osm"]);
        for (b, font) in [
            ("plan-ign", "Source Sans Pro Bold"),
            ("osm", "Noto Sans Medium"),
        ] {
            let t = Instant::now();
            let st = h.map_style(Some(b.into())).unwrap();
            let ms = t.elapsed().as_secs_f64() * 1e3;
            assert_eq!(st.font.as_deref(), Some(font));
            let v: Value = serde_json::from_str(&st.json).unwrap();
            let n = check_urls(&v, &dir, Some(font));
            let ids = style_layers(&v);
            assert!(ids.contains(&"skimap-pistes-label"));
            let labels = v["sources"]["skimap-pistes-labels"]["data"]["features"]
                .as_array()
                .unwrap()
                .len();
            assert!(labels > 10, "{labels} piste labels");
            let toggles: Vec<(&str, bool)> = st
                .toggles
                .iter()
                .map(|t| (t.id.as_str(), t.visible_by_default))
                .collect();
            assert_eq!(
                toggles,
                [
                    ("refuges", true),
                    ("water", true),
                    ("no-network", false),
                    ("place-names", true)
                ]
            );
            for t in &st.toggles {
                assert!(
                    t.style_layers.iter().all(|l| ids.contains(&l.as_str())),
                    "{t:?}"
                );
            }
            println!(
                "{site} {b}: {} basemap symbol layers switched by place-names",
                st.toggles[3].style_layers.len()
            );
            println!("{site} {b}: {labels} piste name points");
            println!(
                "{site} {b}: open {open_ms:.1} ms, style {ms:.1} ms, {} layers, {n} URLs checked, \
                 status {:?}, footer {:?}",
                ids.len(),
                info.status,
                st.attribution_short
            );
        }
    }
}

/// The catalogue embedded in the app (written by `examples/catalog.rs`).
#[test]
fn embedded_catalogue() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../android/app/src/main/assets/catalog.json");
    let c = skimap_ffi::Catalog::parse(&std::fs::read_to_string(path).unwrap()).unwrap();
    assert_eq!(c.len(), 219);
    let names = |v: Vec<skimap_ffi::Domain>| v.into_iter().map(|d| d.name).collect::<Vec<_>>();
    let cerv = names(c.search("cervieres".into(), vec![], 5));
    assert!(cerv.iter().any(|n| n.contains("Cervières")), "{cerv:?}");
    let chap = names(c.search("chapelle des bois".into(), vec![], 5));
    assert!(chap.iter().any(|n| n.contains("Chapelle")), "{chap:?}");
    // From Chapelle-des-Bois (6.1118, 46.5952), the nearest area is within 10 km.
    let n = c.nearest(6.1118, 46.5952, vec![], 3);
    assert!(n[0].km < 10.0, "{:?}", n[0]);
    // Metropolitan France only: every border point in its bounding box.
    let pts: Vec<f64> = c.outline().into_iter().flatten().collect();
    assert!(pts.len() > 2_000);
    for xy in pts.chunks(2) {
        assert!(
            (-5.3..=9.7).contains(&xy[0]) && (41.2..=51.2).contains(&xy[1]),
            "{xy:?}"
        );
    }
}

/// Free mode: two packs at once, every source and layer kept apart, one summary.
#[test]
fn two_packs_together() {
    let tmp = tempfile::tempdir().unwrap();
    let a = tmp.path().join("packs/a");
    let b = tmp.path().join("packs/b");
    test_pack::write_test_pack(&a).unwrap();
    test_pack::write_test_pack(&b).unwrap();
    let view = skimap_ffi::open_packs(
        vec![
            a.display().to_string(),
            b.display().to_string(),
            "/nonexistent".into(),
        ],
        None,
    )
    .unwrap();
    assert_eq!(view.set.len(), 2);
    assert_eq!(view.info.site_id, "libre");
    let pistes = view
        .info
        .overlays
        .iter()
        .find(|o| o.id == "pistes")
        .unwrap();
    assert_eq!(pistes.features, 12);
    let s = view.info.status.clone().unwrap();
    assert_eq!((s.open, s.closed), (2, 2));
    let v: Value = serde_json::from_str(&view.style.json).unwrap();
    assert_eq!(v["name"], "skimap libre test");
    assert_eq!(check_urls(&v, tmp.path(), None), 12, "files of both packs");
    let ids = style_layers(&v);
    assert!(ids.contains(&"skimap-pistes-line") && ids.contains(&"skimap-pistes-line~p1"));
    assert!(ids.contains(&"grid") && ids.contains(&"grid~p1"));
    assert!(!ids.contains(&"background~p1"), "one background only");
    let refuges = view
        .style
        .toggles
        .iter()
        .find(|t| t.id == "refuges")
        .unwrap();
    assert_eq!(refuges.style_layers.len(), 6);
    let card_layers = &view
        .style
        .overlays
        .iter()
        .find(|o| o.pack_layer == "refuges-info")
        .unwrap()
        .style_layers;
    assert!(card_layers.iter().any(|l| l.ends_with("~p1")));
    // One pack: the same as opening it alone.
    let one = skimap_ffi::open_packs(vec![a.display().to_string()], None).unwrap();
    let alone = open_pack(a.display().to_string(), None).unwrap();
    assert_eq!(one.style, alone.style);
    assert_eq!(one.info, alone.info);
    assert!(skimap_ffi::open_packs(vec!["/nonexistent".into()], None).is_err());
}

#[test]
#[ignore = "needs the real packs in data/out/<site>/pack/"]
fn real_packs_free_mode() {
    let dirs: Vec<PathBuf> = ["cervieres", "chapelle-des-bois"]
        .iter()
        .map(|s| std::fs::canonicalize(real_pack(s)).unwrap())
        .collect();
    let root = std::fs::canonicalize(real_pack("cervieres").join("../..")).unwrap();
    for b in ["plan-ign", "osm"] {
        let t = Instant::now();
        let view = skimap_ffi::open_packs(
            dirs.iter().map(|d| d.display().to_string()).collect(),
            Some(b.into()),
        )
        .unwrap();
        let ms = t.elapsed().as_secs_f64() * 1e3;
        let v: Value = serde_json::from_str(&view.style.json).unwrap();
        assert_eq!(v["name"], format!("skimap libre {b}"));
        let n = check_urls(&v, &root, view.style.font.as_deref());
        let layers = style_layers(&v).len();
        let s = view.info.status.clone().unwrap();
        assert_eq!((s.open, s.closed), (2, 48));
        println!(
            "free mode {b}: open + style {ms:.1} ms, {layers} layers, {n} URLs checked, bounds {:?}, json {} bytes",
            view.info.bounds,
            view.style.json.len()
        );
    }
}
