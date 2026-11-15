//! Checks on the real packs of both sites against the pipeline outputs they come from.
//!
//! By default the packs are built from the versioned reference layers (`fixtures/`,
//! without basemaps, voice or off-piste rasters; the slope raster is the Horn slope of the
//! 1 km terrain window) into a temporary directory, then read back. The `full_data_`
//! variants read the packs of the complete local data, basemaps included (weekly job):
//!
//! ```sh
//! cargo test -p skimap-pack --features pipeline --test real_packs
//! cargo test --release -p skimap-pack --features pipeline --test real_packs -- --ignored full_data_ --nocapture
//! ```

#[path = "../../../fixtures/support/mod.rs"]
mod fixtures;

use std::collections::{BTreeMap, BTreeSet};

use serde_json::Value;
use skimap_common::crs::{Crs, transform_point};
use skimap_common::download::Downloader;
use skimap_common::geom::LonLat;
use skimap_common::layout::DataLayout;
use skimap_common::site::{SITES, Site};
use skimap_pack::Pack;
use skimap_pack::build::{BuildOptions, build, class_tiles, pack_dir, raster};
use skimap_pack::export::class_pixels;
use skimap_pack::manifest::LayerKind;
use skimap_pack::tiles::{pixel_lat, pixel_lon, tile_range};

fn full_layout() -> DataLayout {
    DataLayout::new(fixtures::full_data())
}

fn json(path: &std::path::Path) -> Value {
    serde_json::from_slice(&std::fs::read(path).unwrap_or_else(|e| panic!("{path:?}: {e}")))
        .unwrap()
}

/// Packs of both sites built from the fixtures in a temporary data root.
struct FixturePacks {
    _tmp: tempfile::TempDir,
    layout: DataLayout,
}

fn fixture_packs() -> FixturePacks {
    let tmp = tempfile::tempdir().unwrap();
    fixtures::copy_tree(&fixtures::data(), tmp.path());
    let layout = DataLayout::new(tmp.path());
    let dl = Downloader::builder(layout.cache_root())
        .offline(true)
        .build()
        .unwrap();
    for site in SITES.iter() {
        // Slope raster of the pack: Horn slope of the 1 km window (the 5 m slope of the
        // whole site, 36 MB, is not in the fixtures).
        let dem = skimap_common::geotiff::read_f32(&fixtures::input(&format!(
            "{}/dem_window_5m.tif",
            site.id
        )))
        .unwrap();
        let slope = skimap_terrain::slope::horn(&dem).slope;
        let dir = layout.layer_dir(site, "terrain").unwrap();
        skimap_common::geotiff::write_f32(&dir.join("slope_5m.tif"), &slope).unwrap();
        let opts = BuildOptions {
            plan_ign: false,
            osm: false,
            ign_slopes: false,
            voice: false,
            offpiste: false,
            ..BuildOptions::default()
        };
        let r = build(site, &layout, &dl, &opts).unwrap();
        println!(
            "{}: {} layers, missing {:?}",
            site.id,
            r.layers.len(),
            r.missing.iter().map(|m| m.id.as_str()).collect::<Vec<_>>()
        );
        for want in [
            "pistes",
            "zones",
            "zones-rules",
            "refuges-info",
            "refuges-osm",
            "refuges-camptocamp",
            "no-network",
            "coverage-area",
            "slope",
            "snow-depth",
        ] {
            assert!(
                r.layers.iter().any(|l| l == want),
                "{}: no {want} layer",
                site.id
            );
        }
    }
    FixturePacks { _tmp: tmp, layout }
}

/// Both checks below on the packs built from the fixtures (one build: the raster tiles
/// dominate the time of this test), and the golden fingerprint of each pack (Q2, #102):
/// layers with their feature counts, files, sampled raster classes.
///
/// References `fixtures/golden/pack/<site>.json`; update with `SKIMAP_BLESS=1 cargo test
/// -p skimap-pack --features pipeline --test real_packs` and review the diff. Exact:
/// fixed layers, deterministic tiling and classification; a changed class count is a
/// changed raster in the pack.
#[test]
fn fixture_packs_match_their_sources_and_geotiffs() {
    let p = fixture_packs();
    for site in SITES.iter() {
        sources(&p.layout, site);
        let classes = rasters(&p.layout, site);
        let pack = Pack::open(pack_dir(site, &p.layout).unwrap()).unwrap();
        let m = pack.manifest();
        let mut k = fixtures::golden::Golden::new(&format!("pack/{}", site.id));
        k.count("files", m.files.len())
            .count("layers", m.layers.len())
            .text("format", m.format.clone())
            .count("format_version", m.format_version)
            .count("attributions", m.attributions.len());
        for l in &m.layers {
            k.text(&format!("layer.{}.kind", l.id), format!("{:?}", l.kind));
            if let Some(n) = l.features {
                k.count(&format!("layer.{}.features", l.id), n);
            }
        }
        for (id, counts) in classes {
            for (class, n) in counts {
                k.count(&format!("raster.{id}.class_{class}"), n);
            }
        }
        k.check();
    }
    // Slope and avalanche exposure are vector class polygons in the pack.
    class_polygons(&p.layout);
    zone_rule_fields(&p.layout);
}

#[test]
#[ignore = "full-data: data/out/<site>/pack"]
fn full_data_packs_match_their_sources() {
    let layout = full_layout();
    for site in SITES.iter() {
        sources(&layout, site);
    }
}

/// Every pixel of every top-zoom tile of the slope, snow and avalanche exposure rasters
/// equals the class of the source cell under its centre; prints the class shares.
#[test]
#[ignore = "full-data: data/out/<site>/pack"]
fn full_data_rasters_match_their_geotiffs() {
    let layout = full_layout();
    for site in SITES.iter() {
        let _ = rasters(&layout, site);
    }
}

fn sources(layout: &DataLayout, site: &Site) {
    {
        let pack = Pack::open(pack_dir(site, layout).unwrap()).unwrap();
        let m = pack.manifest();
        println!(
            "== {} : pack {} , {} files, {:.2} MB",
            site.id,
            m.pack_version,
            m.files.len(),
            m.total_bytes as f64 / 1e6
        );
        assert_eq!(pack.verify().unwrap(), Vec::<String>::new());
        let out = layout.out_dir(site);
        // Every GeoJSON layer keeps all the features of its source.
        for l in m.layers.iter().filter(|l| l.format == "geojson") {
            if l.id == "piste-labels" {
                // Derived from the pistes of the pack: every name is an itinerary or a
                // name of the nordic ways, every nordic itinerary has its lines.
                let pistes = pack.read_geojson("pistes").unwrap();
                let mut names = BTreeSet::new();
                for f in &pistes.features {
                    let p = f.properties.as_ref().unwrap();
                    let nordic = p["uses"]
                        .as_array()
                        .is_some_and(|u| u.iter().any(|x| x == "nordic"));
                    if !nordic {
                        continue;
                    }
                    for r in p["relations"].as_array().into_iter().flatten() {
                        names.extend(r["name"].as_str().map(str::to_owned));
                    }
                    names.extend(p["name"].as_str().map(str::to_owned));
                }
                let labels = pack.read_geojson("piste-labels").unwrap();
                let got: BTreeSet<String> = labels
                    .features
                    .iter()
                    .filter_map(|f| f.properties.as_ref()?["name"].as_str().map(str::to_owned))
                    .collect();
                println!(
                    "piste-labels: {} lines, {} names",
                    labels.features.len(),
                    got.len()
                );
                assert!(!got.is_empty());
                assert!(got.is_subset(&names), "{:?}", got.difference(&names));
                continue;
            }
            let src = json(&out.join(&l.origin));
            let n = src["features"].as_array().unwrap().len() as u64;
            assert_eq!(l.features, Some(n), "{}", l.id);
            let packed = pack.read_geojson(&l.id).unwrap();
            assert_eq!(packed.features.len() as u64, n, "{}", l.id);
        }
        // Zone rules: the pack's join equals the zones crate's own list (zones.meta.json).
        let meta = json(&out.join("zones/zones.meta.json"));
        let mut want: BTreeSet<(String, String)> = BTreeSet::new();
        for r in meta["stats"]["rules"].as_array().unwrap() {
            for z in r["zones"].as_array().unwrap() {
                want.insert((
                    r["rule"].as_str().unwrap().to_owned(),
                    z.as_str().unwrap().to_owned(),
                ));
            }
        }
        let mut got = BTreeSet::new();
        for f in pack.read_geojson("zones").unwrap().features {
            let p = f.properties.unwrap();
            for r in p["rules"].as_array().unwrap() {
                got.insert((
                    r["id"].as_str().unwrap().to_owned(),
                    p["id"].as_str().unwrap().to_owned(),
                ));
            }
        }
        println!(
            "zone-rule pairs: pack {} , zones.meta.json {}",
            got.len(),
            want.len()
        );
        assert_eq!(got, want);
    }
}

/// Checks the rasters of the pack of `site`; returns the sampled class counts by raster.
fn rasters(layout: &DataLayout, site: &Site) -> Vec<(String, BTreeMap<u8, u64>)> {
    let mut all = Vec::new();
    {
        let pack = Pack::open(pack_dir(site, layout).unwrap()).unwrap();
        for spec in raster::catalog() {
            let Ok(l) = pack.layer(spec.id) else {
                continue;
            };
            assert_eq!(l.kind, LayerKind::Raster);
            let grid = raster::read_grid(&spec, &layout.out_dir(site).join(&l.origin)).unwrap();
            let archive = pack.tiles(spec.id).unwrap();
            let z = spec.maxzoom;
            let bbox = raster::grid_bbox_wgs84(&grid).unwrap();
            let (x0, y0, x1, y1) = tile_range(bbox, z);
            let (mut pixels, mut mismatches) = (0u64, 0u64);
            let mut counts: BTreeMap<u8, u64> = BTreeMap::new();
            for tx in x0..=x1 {
                for ty in y0..=y1 {
                    let tile = archive.tile(z, tx, ty).unwrap();
                    let img = tile.map(|t| skimap_pack::png::decode_indexed(&t).unwrap());
                    for py in (0..256u32).step_by(3) {
                        for px in (0..256u32).step_by(3) {
                            let lon = pixel_lon(f64::from(tx * 256 + px) + 0.5, z);
                            let lat = pixel_lat(f64::from(ty * 256 + py) + 0.5, z);
                            let (gx, gy) =
                                transform_point(Crs::Wgs84, grid.crs(), lon, lat).unwrap();
                            let want =
                                spec.classify(grid.value_at(gx, gy).copied().unwrap_or(f32::NAN));
                            let got = img
                                .as_ref()
                                .map_or(spec.nodata(), |i| i.index_at(px, py).unwrap());
                            pixels += 1;
                            mismatches += u64::from(got != want);
                            *counts.entry(got).or_default() += 1;
                        }
                    }
                }
            }
            let valid: u64 = counts
                .iter()
                .filter(|(c, _)| **c != spec.nodata())
                .map(|(_, n)| n)
                .sum();
            let shares: Vec<String> = counts
                .iter()
                .filter(|(c, _)| **c != spec.nodata())
                .map(|(c, n)| {
                    format!(
                        "{}: {:.1} %",
                        spec.classes[usize::from(*c)].label,
                        100.0 * *n as f64 / valid as f64
                    )
                })
                .collect();
            println!(
                "{} {} z{z}: {pixels} pixels checked, {mismatches} mismatches ; {}",
                site.id,
                spec.id,
                shares.join(", ")
            );
            assert_eq!(mismatches, 0);
            // Reader API at the commune centre agrees with the tiles.
            let c = site.commune_center;
            let v = pack
                .raster_value_at(spec.id, LonLat::new(c.lon, c.lat))
                .unwrap();
            println!("  value at the commune centre: {v:?}");
            all.push((spec.id.to_owned(), counts));
        }
    }
    all
}

/// Basemaps: archives readable, styles point to files of the pack, glyphs present (the
/// basemaps, 40 MB a site, are not built from the fixtures).
#[test]
#[ignore = "full-data: data/out/<site>/pack"]
fn full_data_basemaps_are_complete() {
    let layout = full_layout();
    for site in SITES.iter() {
        let pack = Pack::open(pack_dir(site, &layout).unwrap()).unwrap();
        let m = pack.manifest();
        for b in &m.basemaps {
            let a = pack.tiles(&b.id).unwrap();
            assert_eq!(a.tile_type(), pmtiles::TileType::Mvt);
            let c = site.commune_center;
            let (tx, ty) = skimap_common::crs::tile_for(c, 14);
            let t = a
                .tile(14, tx, ty)
                .unwrap()
                .expect("z14 tile at the commune centre");
            assert!(t.len() > 100);
            let style: Value = serde_json::from_slice(&pack.read_file(&b.style).unwrap()).unwrap();
            let url = style["sources"]
                .as_object()
                .unwrap()
                .values()
                .next()
                .unwrap()["url"]
                .as_str()
                .unwrap();
            assert_eq!(url, format!("pmtiles://{}", b.tiles));
            let glyphs = style["glyphs"].as_str().unwrap();
            let mut stacks = 0;
            for stack in skimap_pack::build::assets::fontstacks(&style) {
                if stack.contains("Devanagari") {
                    continue;
                }
                for r in skimap_pack::build::assets::GLYPH_RANGES {
                    let rel = glyphs.replace("{fontstack}", &stack).replace("{range}", r);
                    assert!(pack.path(&rel).unwrap().is_file(), "{rel}");
                }
                stacks += 1;
            }
            println!(
                "{} {}: z{}-{}, {} tiles, {:.2} MB, {} font stacks, data {:?}",
                site.id,
                b.id,
                b.minzoom,
                b.maxzoom,
                b.tile_count,
                b.bytes as f64 / 1e6,
                stacks,
                b.data_date
            );
        }
    }
}
/// The class polygons (slope, avalanche exposure) drawn back at z15 against the GeoTIFF
/// classes under the pixel centres (one pixel in nine): agreement, and the share of every
/// threshold level in the polygons and in the source; the manifest's own measurements
/// (no significant zone lost).
#[test]
#[ignore = "full-data: data/out/<site>/pack"]
fn full_data_class_polygons_match_their_geotiffs() {
    class_polygons(&full_layout());
}

/// See [`full_data_class_polygons_match_their_geotiffs`].
fn class_polygons(layout: &DataLayout) {
    for site in SITES.iter() {
        let pack = Pack::open(pack_dir(site, layout).unwrap()).unwrap();
        for spec in class_tiles::catalog() {
            let Ok(l) = pack.layer(spec.id) else {
                continue;
            };
            let info = l.classes.as_ref().expect("class layer");
            for lv in &info.levels {
                assert_eq!(lv.significant_below_half, 0, "{} {}", spec.id, lv.label);
                assert!(lv.largest_removed_m2 < info.min_inner_patch_m2, "{lv:?}");
            }
            let grid = raster::read_as(spec.read, &layout.out_dir(site).join(&l.origin)).unwrap();
            let archive = pack.tiles(spec.id).unwrap();
            let z = 15;
            let bbox = raster::grid_bbox_wgs84(&grid).unwrap();
            let (x0, y0, x1, y1) = tile_range(bbox, z);
            let n = spec.classes.len();
            let mut cache = std::collections::HashMap::new();
            let (mut pixels, mut same) = (0u64, 0u64);
            let (mut vec_levels, mut raw_levels) = (vec![0u64; n], vec![0u64; n]);
            for tx in x0..=x1 {
                for ty in y0..=y1 {
                    let t = class_pixels(&archive, info, z, tx, ty, &mut cache).unwrap();
                    for py in (1..256u32).step_by(3) {
                        for px in (1..256u32).step_by(3) {
                            let lon = pixel_lon(f64::from(tx * 256 + px) + 0.5, z);
                            let lat = pixel_lat(f64::from(ty * 256 + py) + 0.5, z);
                            let (gx, gy) =
                                transform_point(Crs::Wgs84, grid.crs(), lon, lat).unwrap();
                            let v = grid.value_at(gx, gy).copied().unwrap_or(f32::NAN);
                            if v.is_nan() {
                                continue;
                            }
                            let raw = spec
                                .classes
                                .iter()
                                .position(|c| v < c.upper)
                                .unwrap_or(n - 1);
                            let raw = (raw >= spec.drawn_from).then_some(raw);
                            let got = t
                                .as_ref()
                                .map(|t| t[(py * 256 + px) as usize])
                                .filter(|&c| {
                                    c >= spec.value_base && usize::from(c - spec.value_base) < n
                                })
                                .map(|c| usize::from(c - spec.value_base));
                            pixels += 1;
                            same += u64::from(got == raw);
                            for k in 1..n {
                                vec_levels[k] += u64::from(got.is_some_and(|c| c >= k));
                                raw_levels[k] += u64::from(raw.is_some_and(|c| c >= k));
                            }
                        }
                    }
                }
            }
            let agree = same as f64 / pixels as f64;
            println!(
                "{} {}: {pixels} pixels, {:.2} % in the source class",
                site.id,
                spec.id,
                100.0 * agree
            );
            assert!(agree > 0.9, "{agree}");
            for k in 1..n {
                let (a, b) = (vec_levels[k] as f64, raw_levels[k] as f64);
                let dev = (a - b) / b.max(1.0);
                println!(
                    "   {:<24} polygons {:.3} % of the pixels, source {:.3} %, {:+.2} %",
                    spec.level_labels[k - 1],
                    100.0 * a / pixels as f64,
                    100.0 * b / pixels as f64,
                    100.0 * dev
                );
                assert!(dev.abs() < 0.08, "{} level {k}: {dev}", spec.id);
            }
        }
    }
}

/// Zones of the packs: the simple fields derived from the real rules.
#[test]
#[ignore = "full-data: data/out/<site>/pack"]
fn full_data_zones_have_simple_rule_fields() {
    zone_rule_fields(&full_layout());
}

/// Bivouac and access fields of every packed zone, and those of four zones of the reports.
fn zone_rule_fields(layout: &DataLayout) {
    let bivouac: BTreeSet<&str> = skimap_zones::fields::BIVOUAC_VALUES
        .iter()
        .map(|v| v.as_str())
        .collect();
    let access: BTreeSet<&str> = skimap_zones::fields::ACCESS_VALUES
        .iter()
        .map(|v| v.as_str())
        .collect();
    let mut seen: BTreeMap<String, String> = BTreeMap::new();
    for site in SITES.iter() {
        let pack = Pack::open(pack_dir(site, layout).unwrap()).unwrap();
        let Ok(zones) = pack.read_geojson("zones") else {
            continue;
        };
        let mut counts: BTreeMap<(String, String), u64> = BTreeMap::new();
        for f in zones.features {
            let p = f.properties.unwrap();
            let b = p["bivouac"].as_str().unwrap();
            let a = p["access"].as_str().unwrap();
            assert!(bivouac.contains(b) && access.contains(a), "{p:?}");
            *counts.entry((b.to_owned(), a.to_owned())).or_default() += 1;
            seen.insert(
                p["id"].as_str().unwrap().to_owned(),
                format!("{b} {} / {a} {}", p["bivouac_period"], p["access_period"]),
            );
        }
        println!("{}: (bivouac, access) -> zones {counts:?}", site.id);
    }
    for (id, want) in [
        ("country:fr", "consent_required null / unknown null"),
        (
            "inpn:FR3800988",
            "forbidden \"du 15 décembre au 30 juin\" / routes_only \"du 15 décembre au 30 juin\"",
        ),
        (
            "inpn:FR3800137",
            "forbidden null / routes_only_in_places null",
        ),
        (
            "dept:25",
            "forbidden_in_places null / routes_only_in_places null",
        ),
    ] {
        println!("  {id}: {:?}", seen.get(id));
        assert_eq!(seen.get(id).map(String::as_str), Some(want), "{id}");
    }
}

/// Pistes of the packs: a state on every way, joined by OSM way from the status layer.
#[test]
#[ignore = "full-data: data/out/<site>/pack"]
fn full_data_pistes_carry_the_state_of_their_ways() {
    let layout = full_layout();
    for site in SITES.iter() {
        let pack = Pack::open(pack_dir(site, &layout).unwrap()).unwrap();
        let pistes = pack.read_geojson("pistes").unwrap();
        let mut by_state: BTreeMap<String, u64> = BTreeMap::new();
        let mut sources: BTreeMap<String, u64> = BTreeMap::new();
        let mut state_of = BTreeMap::new();
        for f in &pistes.features {
            let p = f.properties.as_ref().unwrap();
            let s = p["state"].as_str().unwrap().to_owned();
            *by_state.entry(s.clone()).or_default() += 1;
            for k in ["colour_source", "oneway_source", "name_source"] {
                if let Some(v) = p[k].as_str() {
                    *sources.entry(format!("{k}={v}")).or_default() += 1;
                }
            }
            if let Some(id) = p["osm_way_id"].as_i64() {
                state_of.insert(id, (s, p["uses"].clone()));
            }
        }
        println!("{} : states {by_state:?} ; {sources:?}", site.id);
        if !pack.manifest().personal_sources {
            assert_eq!(by_state.len(), 1);
            continue;
        }
        // Every ski way named by a ski itinerary has its state.
        let status = pack.read_geojson("status").unwrap();
        let mut named = 0;
        for f in status.features {
            let p = f.properties.unwrap();
            if p["osm_practice"] != "nordic" {
                continue;
            }
            for id in p["osm_way_ids"].as_array().into_iter().flatten() {
                let (s, _) = &state_of[&id.as_i64().unwrap()];
                assert_ne!(s, "unknown", "way {id}");
                named += 1;
            }
        }
        println!("   {named} (itinerary, way) pairs with a state");
        assert!(named > 0);
    }
}

/// A pack built without `personal_sources` from the real outputs (built with the option)
/// holds no personal-use or unauthorised source: manifest, attributions, copied
/// metadata, data.
#[test]
#[ignore = "full-data: data/out/<site>"]
fn full_data_public_pack_from_real_outputs_has_no_restricted_source() {
    let real = full_layout();
    for site in SITES.iter() {
        let tmp = tempfile::tempdir().unwrap();
        let out = tmp.path().join("out").join(site.id);
        std::fs::create_dir_all(&out).unwrap();
        for e in std::fs::read_dir(real.out_dir(site)).unwrap() {
            let e = e.unwrap();
            if e.file_name() == "pack" || e.file_name() == "export" {
                continue;
            }
            std::os::unix::fs::symlink(e.path(), out.join(e.file_name())).unwrap();
        }
        let l = DataLayout::new(tmp.path());
        let dl = skimap_common::download::Downloader::builder(l.cache_root())
            .offline(true)
            .build()
            .unwrap();
        let opts = BuildOptions {
            plan_ign: false,
            osm: false,
            personal_sources: false,
            ..BuildOptions::default()
        };
        let r = build(site, &l, &dl, &opts).unwrap();
        let pack = Pack::open(&r.pack_dir).unwrap();
        let ids: Vec<&str> = pack
            .manifest()
            .layers
            .iter()
            .map(|l| l.id.as_str())
            .collect();
        println!("{} public pack: {} layers {ids:?}", site.id, ids.len());
        for m in &r.missing {
            println!("   left out {} : {}", m.id, m.reason);
        }
        assert_restricted_free(&pack);
        assert_public_voice(&pack, &real, site);
    }
}

/// The voice of a published pack is there (VO3) and says no name read only from a
/// personal-use source: the Biodiv'sports zones that no publishable source names (species,
/// « APPB … » as Biodiv'sports spells them), their end dates, the piste-plan names that OSM
/// does not have. Fails as soon as one such clip is in the published voice.
fn assert_public_voice(pack: &Pack, real: &DataLayout, site: &Site) {
    use skimap_voice::names::{PisteName, name_clip_id, name_clip_text, slug};
    let m = pack.manifest();
    assert_eq!(m.voice.status, "present", "{}", m.voice.note);
    let vm = json(&pack.path("voice/manifest.json").unwrap());
    let clips = vm["clips"].as_array().unwrap();
    assert_eq!(clips.len() as u64, m.voice.clips);
    // None of the clips the voice layer lists as personal.
    let listed_personal = json(&real.out_dir(site).join("voice/personal.json"));
    let listed_personal: BTreeSet<&str> = listed_personal["clips"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| c.as_str().unwrap())
        .collect();
    assert!(!listed_personal.is_empty());
    assert!(
        clips
            .iter()
            .all(|c| !listed_personal.contains(c["id"].as_str().unwrap()))
    );
    let ids: BTreeSet<&str> = clips.iter().map(|c| c["id"].as_str().unwrap()).collect();
    let texts: BTreeSet<&str> = clips.iter().map(|c| c["text"].as_str().unwrap()).collect();
    let files: BTreeSet<String> = std::fs::read_dir(pack.path("voice/clips").unwrap())
        .unwrap()
        .map(|e| format!("clips/{}", e.unwrap().file_name().to_string_lossy()))
        .collect();
    let listed: BTreeSet<String> = clips
        .iter()
        .map(|c| c["file"].as_str().unwrap().to_owned())
        .collect();
    assert_eq!(files, listed, "clip files = manifest entries");

    let out = real.out_dir(site);
    let feats = |v: &Value| v["features"].as_array().cloned().unwrap_or_default();
    // Names a publishable layer has: every zone not from Biodiv'sports, refuges, OSM pistes.
    let mut public = BTreeSet::new();
    let mut biodiv = BTreeSet::new();
    let (mut biodiv_dates, mut other_dates) = (BTreeSet::new(), BTreeSet::new());
    for f in feats(&json(&out.join("zones/zones.geojson"))) {
        let p = &f["properties"];
        let months: Vec<u8> = p["months"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|m| m.as_u64().map(|m| m as u8))
            .collect();
        let dates = skimap_voice::sources::period_ends(&months);
        let Some(n) = p["name"].as_str() else {
            continue;
        };
        if p["source"] == "biodiv-sports" {
            biodiv.insert(n.trim().to_owned());
            biodiv_dates.extend(dates);
        } else {
            public.insert(name_clip_id(n.trim()));
            other_dates.extend(dates);
        }
    }
    for f in feats(&json(&out.join("refuges/refuges_info.geojson"))) {
        if let Some(n) = f["properties"]["name"].as_str() {
            public.insert(name_clip_id(n.trim()));
        }
    }
    for f in feats(&json(&out.join("refuges/osm.geojson"))) {
        if let Some(n) = f["properties"]["tags"]["name"].as_str() {
            public.insert(name_clip_id(n.trim()));
        }
    }
    let mut osm_pistes = BTreeSet::new();
    for f in feats(&json(&out.join("pistes/pistes.geojson"))) {
        let p = &f["properties"];
        if let Some(n) = p["way_name"].as_str() {
            osm_pistes.insert(slug(n.trim()));
        }
        for r in p["relations"].as_array().into_iter().flatten() {
            if let Some(n) = r["name"].as_str() {
                osm_pistes.insert(slug(n.trim()));
            }
        }
    }
    let personal_only: Vec<&String> = biodiv
        .iter()
        .filter(|n| !public.contains(&name_clip_id(n)))
        .collect();
    assert!(
        !personal_only.is_empty(),
        "{}: Biodiv'sports names",
        site.id
    );
    for n in &personal_only {
        assert!(!ids.contains(name_clip_id(n).as_str()), "{}: {n}", site.id);
        assert!(
            !texts.contains(name_clip_text(n).as_str()),
            "{}: {n}",
            site.id
        );
    }
    let personal_dates: Vec<_> = biodiv_dates.difference(&other_dates).collect();
    for d in &personal_dates {
        let id = skimap_voice::clips::until(**d).id;
        assert!(!ids.contains(id.as_str()), "{}: {id}", site.id);
    }
    let mut plan_only = Vec::new();
    let plan = out.join("plan_pistes/plan_pistes.geojson");
    if plan.is_file() {
        for f in feats(&json(&plan)) {
            let Some(n) = f["properties"]["name"].as_str() else {
                continue;
            };
            let s = slug(n.trim());
            if osm_pistes.contains(&s) {
                continue;
            }
            let id = PisteName::new(n.trim(), None).clip_id();
            assert!(
                !ids.iter()
                    .any(|i| *i == id || i.starts_with(&format!("{id}."))),
                "{}: plan name {n}",
                site.id
            );
            plan_only.push(n.to_owned());
        }
    }
    // The publishable names are back in the published voice.
    let inpn = public.iter().filter(|id| ids.contains(id.as_str())).count();
    assert!(inpn > 10, "{}: {inpn} public names spoken", site.id);
    println!(
        "   voice: {} clips, {} o ; absent: {} Biodiv'sports names {personal_only:?}, dates {personal_dates:?}, plan names {plan_only:?} ; {inpn} public names",
        m.voice.clips,
        m.voice.bytes,
        personal_only.len()
    );
}

fn assert_restricted_free(pack: &Pack) {
    let m = pack.manifest();
    assert!(!m.personal_sources);
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
    assert!(
        m.voice
            .sources
            .iter()
            .all(|s| !skimap_pack::build::layers::unauthorised(s))
    );
    for a in &m.attributions {
        let a = a.to_lowercase();
        for bad in [
            "biodiv",
            "clpa",
            "espace nordique jurassien",
            "nordic france",
            "open-meteo",
            "commune de cervières",
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
                "\"kind\":\"biodiv_sports\"",
                "\"source\":\"enj\"",
                "nordicfrance:",
                "\"ddt05_index\"",
            ] {
                assert!(!text.contains(bad), "{} contains {bad}", f.path);
            }
        }
    }
}
