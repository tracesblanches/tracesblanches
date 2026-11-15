//! End-to-end on synthetic pipeline outputs, without network: build a pack (no basemaps),
//! read it back with the phone-side reader, export it for Locus Map / OsmAnd.

use std::path::Path;

use serde_json::{Value, json};
use skimap_common::crs::{Crs, transform_point};
use skimap_common::download::Downloader;
use skimap_common::geom::LonLat;
use skimap_common::grid::{GeoTransform, Grid};
use skimap_common::layout::DataLayout;
use skimap_common::site::CERVIERES;
use skimap_pack::Pack;
use skimap_pack::build::{BuildOptions, build, class_tiles};
use skimap_pack::export::{class_pixels, export};
use skimap_pack::tiles::{pixel_lat, pixel_lon, tile_pixel};

fn write(path: &Path, v: &Value) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, serde_json::to_vec(v).unwrap()).unwrap();
}

fn meta(personal: bool) -> Value {
    json!({"generated_at": "2026-11-14T22:00:00Z", "valid_until": "2026-12-14",
           "sources": [{"name": "src", "url": "https://example.org", "licence": "LO", "attribution": "Attr A",
                        "personal_use_only": personal}]})
}

fn fixture(root: &Path) {
    let out = root.join("out/cervieres");
    write(
        &out.join("pistes/pistes.geojson"),
        &json!({"type": "FeatureCollection", "features": [
            {"type": "Feature", "properties": {"name": "Souliers", "difficulty": "intermediate", "length_m": 322.9,
              "uses": ["nordic"], "grooming": "classic;skating", "osm_way_id": 1, "relations": [{"name": "Souliers"}]},
             "geometry": {"type": "LineString", "coordinates": [[6.761234567, 44.851], [6.762, 44.852]]}},
            {"type": "Feature", "properties": {"name": null, "difficulty": "novice", "uses": ["nordic"]},
             "geometry": {"type": "LineString", "coordinates": [[6.77, 44.86], [6.771, 44.861]]}}
        ]}),
    );
    write(&out.join("pistes/pistes.meta.json"), &meta(false));
    write(
        &out.join("zones/zones.geojson"),
        &json!({"type": "FeatureCollection", "features": [{
            "type": "Feature",
            "properties": {"id": "country:fr", "kind": "country", "name": "France", "source": "geo-api", "url": null,
                           "months": null, "practices": [], "designated": null, "area_ha": 1.0, "clipped": true, "note": null},
            "geometry": {"type": "Polygon", "coordinates": [[[6.7, 44.8], [6.8, 44.8], [6.8, 44.9], [6.7, 44.8]]]}}]}),
    );
    std::fs::write(
        out.join("zones/rules.json"),
        skimap_zones::rules::RULES_JSON,
    )
    .unwrap();
    write(&out.join("zones/zones.meta.json"), &meta(false));
    write(
        &out.join("refuges/refuges_info.geojson"),
        &json!({"type": "FeatureCollection", "features": [{
            "type": "Feature", "properties": {"name": "Cabane Bois & briquet <ok>", "type_label": "cabane non gardée", "places": 6,
              "last_comment": {"id": 54639, "point": 648, "date": "2026-06-07 18:44:00.749223+02", "photo": true}},
            "geometry": {"type": "Point", "coordinates": [6.75, 44.85]}}]}),
    );
    write(&out.join("refuges/refuges.meta.json"), &meta(false));
    write(
        &out.join("status/status.geojson"),
        &json!({"type": "FeatureCollection", "features": []}),
    );
    write(&out.join("status/status.meta.json"), &meta(true));
    // 5 m slope grid in Lambert-93 around the commune centre: a ramp 0..60° from west to east.
    let (x0, y0) = transform_point(Crs::Wgs84, Crs::Lambert93, 6.74, 44.87).unwrap();
    let (w, h) = (600usize, 500usize);
    let data: Vec<f32> = (0..w * h)
        .map(|i| {
            if i / w == h - 1 {
                f32::NAN
            } else {
                (i % w) as f32 * 60.0 / w as f32
            }
        })
        .collect();
    let grid = Grid::from_vec(
        w,
        h,
        GeoTransform::north_up(x0, y0, 5.0),
        Crs::Lambert93,
        data,
    )
    .unwrap();
    std::fs::create_dir_all(out.join("terrain")).unwrap();
    skimap_common::geotiff::write_f32(&out.join("terrain/slope_5m.tif"), &grid).unwrap();
    write(
        &out.join("terrain/terrain.meta.json"),
        &json!({"generated_at": "x", "valid_for": "relief permanent", "sources": []}),
    );
}

#[test]
fn build_read_export() {
    let tmp = tempfile::tempdir().unwrap();
    fixture(tmp.path());
    let layout = DataLayout::new(tmp.path());
    let dl = Downloader::builder(layout.cache_root())
        .offline(true)
        .build()
        .unwrap();
    let opts = BuildOptions {
        plan_ign: false,
        osm: false,
        ign_slopes: false,
        ..BuildOptions::default()
    };
    let report = build(&CERVIERES, &layout, &dl, &opts).unwrap();
    assert!(
        report
            .missing
            .iter()
            .any(|m| m.id == "status" && m.reason.contains("personnel"))
    );
    assert!(report.missing.iter().any(|m| m.id == "no-network"));

    let pack = Pack::open(&report.pack_dir).unwrap();
    assert!(pack.verify().unwrap().is_empty());
    let m = pack.manifest();
    assert_eq!(m.format_version, 1);
    assert_eq!(m.valid_for.until.as_deref(), Some("2026-12-14"));
    assert_eq!(m.voice.status, "absent");
    assert!(m.attributions.contains(&"Attr A".to_owned()));
    assert_eq!(m.total_bytes, m.files.iter().map(|f| f.bytes).sum::<u64>());

    // GeoJSON: rounded coordinates, properties kept.
    let pistes = pack.read_geojson("pistes").unwrap();
    assert_eq!(pistes.features.len(), 2);
    let text = String::from_utf8(pack.read_file("layers/pistes.geojson").unwrap()).unwrap();
    assert!(text.contains("6.761235") && !text.contains("6.761234567"));
    // Zones carry their rules.
    let zones = pack.read_geojson("zones").unwrap();
    let props = zones.features[0].properties.as_ref().unwrap();
    assert!(props["rules_text"].as_str().unwrap().contains("Bivouac"));

    // Slope classes: vector polygons; drawn back at z15 (export rasteriser), the class
    // under each pixel centre is the class of the source cell almost everywhere (the
    // smoothing moves the boundaries by a fraction of a cell).
    let l = pack.layer("slope").unwrap();
    assert!(l.raster.is_none());
    assert_eq!(l.format, "pmtiles");
    let info = l.classes.as_ref().unwrap();
    assert_eq!((info.minzoom, info.maxzoom), (10, 13));
    assert_eq!(info.levels[0].significant_min_coverage, 1.0);
    let classes = class_tiles::catalog()
        .into_iter()
        .find(|s| s.id == "slope")
        .unwrap();
    let grid =
        skimap_common::geotiff::read_f32(&tmp.path().join("out/cervieres/terrain/slope_5m.tif"))
            .unwrap();
    let archive = pack.tiles("slope").unwrap();
    let mut cache = std::collections::HashMap::new();
    let classify = |v: f32| -> Option<u8> {
        if v.is_nan() {
            return None;
        }
        let k = classes
            .classes
            .iter()
            .position(|c| v < c.upper)
            .unwrap_or(4) as u8;
        (k >= 1).then_some(k)
    };
    let (mut same, mut total) = (0usize, 0usize);
    let mut seen = [0usize; 5];
    for i in 0..400 {
        let p = LonLat::new(
            6.742 + (i % 20) as f64 * 0.0018,
            44.851 + (i / 20) as f64 * 0.0009,
        );
        let (tx, ty, px, py) = tile_pixel(p, 15);
        let lon = pixel_lon(f64::from(tx * 256 + px) + 0.5, 15);
        let lat = pixel_lat(f64::from(ty * 256 + py) + 0.5, 15);
        let (gx, gy) = transform_point(Crs::Wgs84, Crs::Lambert93, lon, lat).unwrap();
        let want = classify(grid.value_at(gx, gy).copied().unwrap_or(f32::NAN));
        let got = class_pixels(&archive, info, 15, tx, ty, &mut cache)
            .unwrap()
            .map(|t| t[(py * 256 + px) as usize])
            .filter(|&c| c <= 4);
        total += 1;
        same += usize::from(got == want);
        if let Some(c) = got {
            seen[usize::from(c)] += 1;
        }
    }
    assert!(
        same * 100 >= total * 97,
        "{same} / {total} pixels in the source class"
    );
    assert!(
        seen[1..].iter().all(|&n| n > 0),
        "every class drawn: {seen:?}"
    );
    // Outside the polygons.
    let (tx, ty, _, _) = tile_pixel(LonLat::new(6.0, 44.0), 15);
    assert!(
        class_pixels(&archive, info, 15, tx, ty, &mut cache)
            .unwrap()
            .is_none()
    );

    // Export.
    let ex = export(&pack, &tmp.path().join("export")).unwrap();
    let names: Vec<&str> = ex.files.iter().map(|f| f.path.as_str()).collect();
    for want in [
        "kml/pistes.kml",
        "gpx/pistes-rouges.gpx",
        "gpx/pistes-vertes.gpx",
        "gpx/refuges-refuges-info.gpx",
        "kml/zones-protegees.kml",
        "locus/pentes.mbtiles",
        "osmand/pentes.sqlitedb",
        "LISEZMOI.md",
    ] {
        assert!(names.contains(&want), "{want} missing from {names:?}");
    }
    let gpx =
        std::fs::read_to_string(tmp.path().join("export/gpx/refuges-refuges-info.gpx")).unwrap();
    // The comment is a date only (no text, no author: refuges crate, privacy).
    assert!(gpx.contains("Dernier commentaire sur refuges.info : 2026-06-07"));
    assert!(gpx.contains("Cabane Bois &amp; briquet &lt;ok&gt;"));
    let c = rusqlite::Connection::open(tmp.path().join("export/osmand/pentes.sqlitedb")).unwrap();
    let (minz, maxz): (u8, u8) = c
        .query_row("SELECT min(z), max(z) FROM tiles", [], |r| {
            Ok((r.get(0)?, r.get(1)?))
        })
        .unwrap();
    assert_eq!((minz, maxz), (10, 16));
}

/// A pack built without `personal_sources` holds no personal-use or unauthorised source,
/// neither in the manifest (sources, attributions) nor in the data, even from pipeline
/// outputs built with the option (or older metadata that does not mark them).
#[test]
fn public_pack_has_no_restricted_source() {
    let tmp = tempfile::tempdir().unwrap();
    fixture(tmp.path());
    let out = tmp.path().join("out/cervieres");
    // Zones with a Biodiv'sports zone, its source not marked (older metadata).
    write(
        &out.join("zones/zones.geojson"),
        &json!({"type": "FeatureCollection", "features": [{
            "type": "Feature",
            "properties": {"id": "biodiv:1", "kind": "biodiv_sports", "name": "Aigle royal", "source": "biodiv-sports", "url": null,
                           "months": [1, 2], "practices": ["terrestre"], "designated": null, "area_ha": 1.0, "clipped": true, "note": null},
            "geometry": {"type": "Polygon", "coordinates": [[[6.7, 44.8], [6.8, 44.8], [6.8, 44.9], [6.7, 44.8]]]}}]}),
    );
    write(
        &out.join("zones/zones.meta.json"),
        &json!({"generated_at": "x", "sources": [
            {"name": "INPN", "url": "https://inpn.mnhn.fr", "licence": "LO", "attribution": "PatriNat"},
            {"name": "Biodiv'sports", "url": "https://biodiv-sports.fr", "licence": "?", "attribution": "Biodiv'sports (LPO AuRA)"}]}),
    );
    // Avalanche exposure fused with the CLPA (marked personal), and a core raster made
    // from it.
    let meta_clpa = json!({"generated_at": "x", "sources": [
        {"name": "DDT 05, aléa avalanche dans les Hautes-Alpes (CLPA republiée)", "url": "https://www.data.gouv.fr",
         "licence": "LO", "attribution": "DDT des Hautes-Alpes ; CLPA INRAE", "personal_use_only": true}]});
    write(
        &out.join("avalanche-exposure/avalanche-exposure.meta.json"),
        &meta_clpa,
    );
    write(
        &out.join("avalanche-exposure/segments.geojson"),
        &json!({"type": "FeatureCollection", "features": []}),
    );
    std::fs::create_dir_all(out.join("offpiste")).unwrap();
    std::fs::write(out.join("offpiste/exposure_5m.skr"), b"SKR1 test").unwrap();
    write(
        &out.join("offpiste/offpiste.meta.json"),
        &json!({"sources": []}),
    );
    write(
        &out.join("avalanche-historic/avalanche-historic.meta.json"),
        &meta_clpa,
    );
    write(
        &out.join("avalanche-historic/emprises.geojson"),
        &json!({"type": "FeatureCollection", "features": [{"type": "Feature",
            "properties": {"source": "CLPA"}, "geometry": {"type": "Point", "coordinates": [6.75, 44.85]}}]}),
    );
    // Voice clips with a Biodiv'sports zone name.
    std::fs::create_dir_all(out.join("voice/clips")).unwrap();
    std::fs::write(out.join("voice/clips/z.opus"), b"x").unwrap();
    write(
        &out.join("voice/manifest.json"),
        &json!({"clips": [{"text": "Aigle royal"}]}),
    );
    write(
        &out.join("voice/voice.meta.json"),
        &json!({"sources": [{"name": "Biodiv'sports", "url": "https://biodiv-sports.fr", "licence": "?", "attribution": "B"}]}),
    );

    let layout = DataLayout::new(tmp.path());
    let dl = Downloader::builder(layout.cache_root())
        .offline(true)
        .build()
        .unwrap();
    for personal in [false, true] {
        let opts = BuildOptions {
            plan_ign: false,
            osm: false,
            personal_sources: personal,
            ..BuildOptions::default()
        };
        let report = build(&CERVIERES, &layout, &dl, &opts).unwrap();
        let pack = Pack::open(&report.pack_dir).unwrap();
        let m = pack.manifest();
        let ids: Vec<&str> = m.layers.iter().map(|l| l.id.as_str()).collect();
        if personal {
            // With the option everything is there, marked personal use only.
            for id in [
                "zones",
                "avalanche-areas",
                "avalanche-segments",
                "offpiste-exposure",
                "status",
            ] {
                let l = m.layer(id).unwrap_or_else(|| panic!("{id} in {ids:?}"));
                assert!(l.personal_use_only, "{id}");
            }
            assert_eq!(m.voice.status, "present");
            continue;
        }
        for id in [
            "zones",
            "avalanche-areas",
            "avalanche-segments",
            "offpiste-exposure",
            "status",
        ] {
            assert!(!ids.contains(&id), "{id} in the public pack");
        }
        assert_eq!(m.voice.status, "absent");
        assert_restricted_free(&pack);
    }
}

/// The voice crate marks names as personal by the same source list as the pack.
#[test]
fn voice_and_pack_agree_on_personal_sources() {
    assert_eq!(
        skimap_voice::sources::PERSONAL_SOURCES,
        skimap_pack::build::layers::UNAUTHORISED_SOURCES
    );
    assert_eq!(
        skimap_voice::pack::PERSONAL_FILE,
        skimap_pack::build::VOICE_PERSONAL_FILE
    );
}

/// A voice layer with its list of personal clips (`personal.json`) goes into the published
/// pack without the clips of names read only from personal-use sources, and without those
/// sources; the personal pack has them all.
#[test]
fn public_pack_voice_leaves_out_personal_names() {
    let tmp = tempfile::tempdir().unwrap();
    fixture(tmp.path());
    let out = tmp.path().join("out/cervieres");
    let clip = |id: &str, kind: &str, text: &str| {
        json!({"id": id, "kind": kind, "text": text, "file": format!("clips/{id}.opus"),
               "duration_ms": 700, "bytes": 3, "tokens": 4, "lead_trim_ms": 90, "tail_trim_ms": 40,
               "gain_db": 1.0, "end_burst": false, "seed": 42, "onset_clipped": false})
    };
    let clips = [
        clip("fix.arrive", "fixed", "Vous arrivez à destination."),
        clip("name.casse-deserte", "name", "Casse Déserte."),
        clip("name.aigle-royal", "name", "Aigle royal."),
        clip(
            "piste.la-pinatelle.noire",
            "name",
            "La Pinatelle, piste noire.",
        ),
        clip("until.0430", "value", "En vigueur jusqu'au trente avril."),
    ];
    std::fs::create_dir_all(out.join("voice/clips")).unwrap();
    for c in &clips {
        let f = c["file"].as_str().unwrap();
        std::fs::write(out.join("voice").join(f), c["text"].as_str().unwrap()).unwrap();
    }
    write(
        &out.join("voice/manifest.json"),
        &json!({"format": 1, "voice": "estelle", "model": "m.gguf", "model_sha256": "e7", "seed": 42,
                "sample_rate": 24000, "opus_kbps": 24, "pauses_ms": [150, 400], "clips": clips}),
    );
    write(
        &out.join("voice/personal.json"),
        &json!({"note": "n", "sources": ["Biodiv'sports", "Plan des pistes - Cervières"],
                "clips": ["name.aigle-royal", "piste.la-pinatelle.noire", "until.0430"]}),
    );
    write(
        &out.join("voice/voice.meta.json"),
        &json!({"layer": "voice", "site": "cervieres", "generated_at": "2026-11-15T10:00:00Z",
                "sources": [
                    {"name": "Pocket TTS", "url": "https://huggingface.co/kyutai", "licence": "CC-BY-4.0", "attribution": "Kyutai"},
                    {"name": "INPN espaces protégés (PatriNat)", "url": "https://data.geopf.fr/wfs/ows", "licence": "other-open", "attribution": "PatriNat"},
                    // Not marked, recognised by name (older metadata).
                    {"name": "Biodiv'sports", "url": "https://biodiv-sports.fr", "licence": "?", "attribution": "Biodiv'sports"},
                    {"name": "Plan des pistes - Cervières", "url": "https://example.org/plan.pdf", "licence": "?", "attribution": "OT", "personal_use_only": true}],
                "files": [{"name": "manifest.json", "description": "m", "count": 5},
                          {"name": "clips/<id>.opus", "description": "c", "count": 5, "bytes": 15},
                          {"name": "personal.json", "description": "p", "count": 3}],
                "stats": {"clips": 5, "clips_personal": 3, "names_personal": {"places": 1}},
                "notes": []}),
    );
    let layout = DataLayout::new(tmp.path());
    let dl = Downloader::builder(layout.cache_root())
        .offline(true)
        .build()
        .unwrap();
    for personal in [false, true] {
        let opts = BuildOptions {
            plan_ign: false,
            osm: false,
            personal_sources: personal,
            ..BuildOptions::default()
        };
        let report = build(&CERVIERES, &layout, &dl, &opts).unwrap();
        let pack = Pack::open(&report.pack_dir).unwrap();
        let m = pack.manifest();
        assert_eq!(m.voice.status, "present", "{:?}", m.voice);
        let vm: Value = serde_json::from_slice(
            &std::fs::read(pack.path("voice/manifest.json").unwrap()).unwrap(),
        )
        .unwrap();
        let ids: Vec<&str> = vm["clips"]
            .as_array()
            .unwrap()
            .iter()
            .map(|c| c["id"].as_str().unwrap())
            .collect();
        let files: Vec<String> = std::fs::read_dir(pack.path("voice/clips").unwrap())
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        if personal {
            assert_eq!(m.voice.clips, 5);
            assert_eq!(ids.len(), 5);
            assert!(
                m.voice
                    .sources
                    .iter()
                    .any(skimap_pack::build::layers::unauthorised)
            );
            continue;
        }
        assert_eq!(m.voice.clips, 2, "{:?}", m.voice);
        assert_eq!(ids, ["fix.arrive", "name.casse-deserte"]);
        let mut files = files;
        files.sort();
        assert_eq!(files, ["fix.arrive.opus", "name.casse-deserte.opus"]);
        let names: Vec<&str> = m.voice.sources.iter().map(|s| s.name.as_str()).collect();
        assert_eq!(names, ["Pocket TTS", "INPN espaces protégés (PatriNat)"]);
        let meta: Value = serde_json::from_slice(
            &std::fs::read(pack.path("voice/voice.meta.json").unwrap()).unwrap(),
        )
        .unwrap();
        assert_eq!(meta["stats"]["clips"], 2);
        assert_eq!(meta["stats"]["clips_personal"], 0);
        assert_eq!(meta["stats"]["public_pack_clips_removed"], 3);
        assert_eq!(meta["files"][1]["count"], 2);
        assert_eq!(
            meta["files"].as_array().unwrap().len(),
            2,
            "no personal.json"
        );
        assert!(m.files.iter().all(|f| !f.path.ends_with("personal.json")));
        // Nothing of the personal names anywhere in the published voice.
        for f in m.files.iter().filter(|f| f.path.starts_with("voice/")) {
            let text =
                String::from_utf8_lossy(&std::fs::read(pack.path(&f.path).unwrap()).unwrap())
                    .to_lowercase();
            for bad in [
                "aigle",
                "pinatelle",
                "biodiv",
                "plan des pistes",
                "until.0430",
            ] {
                assert!(!text.contains(bad), "{} contains {bad}", f.path);
            }
        }
        assert!(
            !m.voice
                .sources
                .iter()
                .any(skimap_pack::build::layers::unauthorised)
        );
    }
}

/// No restricted source in the manifest, the attributions, the copied metadata nor the
/// data of the pack.
fn assert_restricted_free(pack: &Pack) {
    let m = pack.manifest();
    for l in &m.layers {
        assert!(!l.personal_use_only, "{} personal", l.id);
        for s in &l.sources {
            assert!(
                !skimap_pack::build::layers::unauthorised(s),
                "{}: {}",
                l.id,
                s.name
            );
        }
    }
    for a in &m.attributions {
        let a = a.to_lowercase();
        for bad in [
            "biodiv",
            "clpa",
            "enj",
            "nordic france",
            "open-meteo",
            "mairie",
        ] {
            assert!(!a.contains(bad), "attribution {a}");
        }
    }
    for f in &m.files {
        let path = pack.path(&f.path).unwrap();
        if f.path.ends_with(".meta.json") {
            let meta: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
            for s in meta["sources"].as_array().into_iter().flatten() {
                let name = s["name"].as_str().unwrap_or("").to_lowercase();
                assert!(s["personal_use_only"] != true, "{}: {name}", f.path);
                for bad in skimap_pack::build::layers::UNAUTHORISED_SOURCES {
                    assert!(!name.contains(bad), "{}: {name}", f.path);
                }
            }
        } else if f.path.ends_with(".geojson") || f.path.ends_with(".json") {
            let text = std::fs::read_to_string(&path).unwrap().to_lowercase();
            for bad in [
                "biodiv-sports",
                "biodiv'sports",
                "\"clpa\"",
                "\"source\":\"enj\"",
                "nordicfrance:",
            ] {
                assert!(!text.contains(bad), "{} contains {bad}", f.path);
            }
        }
    }
}
