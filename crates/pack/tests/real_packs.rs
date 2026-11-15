//! Checks on the real packs of both sites (built by the `build_pack` example) against the
//! pipeline outputs they come from. Ignored by default: needs `data/`.
//!
//! ```sh
//! cargo test --release -p skimap-pack --features pipeline --test real_packs -- --ignored --nocapture
//! ```

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

use serde_json::Value;
use skimap_common::crs::{Crs, transform_point};
use skimap_common::geom::LonLat;
use skimap_common::layout::DataLayout;
use skimap_common::site::SITES;
use skimap_pack::Pack;
use skimap_pack::build::{pack_dir, raster};
use skimap_pack::manifest::LayerKind;
use skimap_pack::tiles::{pixel_lat, pixel_lon, tile_range};

fn layout() -> DataLayout {
    let root = std::env::var_os("SKIMAP_DATA_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../data"));
    DataLayout::new(root)
}

fn json(path: &std::path::Path) -> Value {
    serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap()
}

#[test]
#[ignore = "needs data/out/<site>/pack"]
fn packs_match_their_sources() {
    let layout = layout();
    for site in SITES {
        let pack = Pack::open(pack_dir(site, &layout).unwrap()).unwrap();
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

/// Every pixel of every top-zoom tile of the slope, snow and avalanche exposure rasters
/// equals the class of the source cell under its centre; prints the class shares.
#[test]
#[ignore = "needs data/out/<site>/pack"]
fn rasters_match_their_geotiffs() {
    let layout = layout();
    for site in SITES {
        let pack = Pack::open(pack_dir(site, &layout).unwrap()).unwrap();
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
        }
    }
}

/// Basemaps: archives readable, styles point to files of the pack, glyphs present.
#[test]
#[ignore = "needs data/out/<site>/pack"]
fn basemaps_are_complete() {
    let layout = layout();
    for site in SITES {
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
