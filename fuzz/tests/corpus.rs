//! Replays the versioned inputs of every fuzz target on the stable toolchain (CI): the
//! hand-made seeds (`seeds/<target>/`), the inputs of the bugs found by fuzzing
//! (`regressions/<target>/`) and the minimised campaign corpus (`corpus/<target>/`).
//! `cargo test --release -- --ignored make_seeds` regenerates the seeds and regressions.

#![allow(
    clippy::disallowed_methods,
    reason = "tests write their inputs directly"
)]

use std::io::{Cursor, Write};
use std::path::{Path, PathBuf};

use skimap_common::crs::Crs;
use skimap_common::geom::LonLat;
use skimap_common::grid::{GeoTransform, Grid};
use skimap_core::graph::{Edge, EdgeAttrs, EdgeKind, GraphData, Node, PisteGraph, Profile};
use skimap_core::offpiste::{
    Encoding, SnowGrid, ZoneClass, encode_raster_with, encode_zone_classes,
};
use skimap_fuzz::{TARGETS, encode_archive};

fn dir(kind: &str, target: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join(kind)
        .join(target)
}

fn replay(kind: &str, required: bool) -> usize {
    let mut total = 0;
    for (name, f) in TARGETS {
        let d = dir(kind, name);
        let mut n = 0;
        if let Ok(rd) = std::fs::read_dir(&d) {
            let mut paths: Vec<PathBuf> = rd.filter_map(|e| Some(e.ok()?.path())).collect();
            paths.sort();
            for p in paths {
                let data = std::fs::read(&p).expect("input file");
                f(&data);
                n += 1;
            }
        }
        assert!(
            !required || n > 0,
            "no {kind} for {name} in {}",
            d.display()
        );
        total += n;
    }
    total
}

/// Every seed, regression and corpus input of every target runs without a panic.
#[test]
fn replay_seeds_regressions_and_corpus() {
    let seeds = replay("seeds", true);
    let regressions = replay("regressions", false);
    let corpus = replay("corpus", false);
    println!("replayed {seeds} seeds, {regressions} regressions, {corpus} corpus inputs");
    assert!(regressions >= 8, "{regressions} regression inputs");
}

fn write_input(kind: &str, target: &str, name: &str, data: &[u8]) {
    let d = dir(kind, target);
    std::fs::create_dir_all(&d).expect("input directory");
    std::fs::write(d.join(name), data).expect("input");
}

fn write_seed(target: &str, name: &str, data: &[u8]) {
    write_input("seeds", target, name, data);
}

/// SKR1 header (72 bytes) of a `w × h` raster of `code` cells with a geotransform.
fn skr1_header(code: u8, w: u32, h: u32, gt: [f64; 6]) -> Vec<u8> {
    let mut b = b"SKR1".to_vec();
    b.extend_from_slice(&[code, 0, 0, 0]);
    b.extend_from_slice(&w.to_le_bytes());
    b.extend_from_slice(&h.to_le_bytes());
    b.extend_from_slice(&2154u32.to_le_bytes());
    b.extend_from_slice(&255u32.to_le_bytes());
    for v in gt {
        b.extend_from_slice(&v.to_le_bytes());
    }
    b
}

/// Inputs of the bugs found by fuzzing (Q3, #103), rebuilt by hand so that each one
/// states its bug; replayed by `replay_seeds_regressions_and_corpus` and by the CI fuzz job
/// under the sanitizer and the libFuzzer allocation limit.
fn write_regressions() {
    let gt = [1_000_000.0, 5.0, 0.0, 6_420_000.0, 0.0, -5.0];
    // NaN origin accepted by `GeoTransform::from_gdal` (SKR1 round trip changed the grid).
    let mut nan = skr1_header(1, 2, 1, [f64::NAN, 5.0, 0.0, 6_420_000.0, 0.0, -5.0]);
    nan.extend_from_slice(&[1, 2]);
    write_input("regressions", "skr1", "nan-origin", &nan);
    // (2^32 − 1)² cells of 2 bytes: the byte count overflowed.
    write_input(
        "regressions",
        "skr1",
        "byte-count-overflow",
        &skr1_header(2, u32::MAX, u32::MAX, gt),
    );
    // 4 × 10⁹ × 0 cells: the row-delta encoder reserved 4 GB for one row.
    write_input(
        "regressions",
        "skr1",
        "empty-wide-row-delta",
        &skr1_header(0x81, u32::MAX, 0, gt),
    );
    let mut files = offpiste_files();
    files[0].1 = skr1_header(2, u32::MAX, u32::MAX, gt);
    let refs: Vec<(&str, &[u8])> = files.iter().map(|(n, b)| (*n, b.as_slice())).collect();
    write_input(
        "regressions",
        "offpiste_grid",
        "byte-count-overflow",
        &encode_archive(&refs),
    );
    // More stations than elevations: elevation lookups out of bounds.
    let g = graph_json().replacen(
        "\"elev_m\":[1800.0,1812.0,1805.0]",
        "\"elev_m\":[1800.0,1812.0]",
        1,
    );
    assert!(
        g.contains("[1800.0,1812.0]"),
        "profile not found in the seed graph"
    );
    write_input(
        "regressions",
        "piste_graph",
        "short-elevations",
        g.as_bytes(),
    );
    // PMTiles header asking for 3 GB of metadata: the file backend allocated it.
    let mut big = pmtiles_archive();
    big[32..40].copy_from_slice(&3_000_000_000u64.to_le_bytes());
    write_input("regressions", "tile_archive", "metadata-3gb", &big);
    write_input("regressions", "pmtiles_vendor", "metadata-3gb", &big);
    // Zooms 32 and above: `1u32 << z` overflowed in the tile arithmetic.
    let mut z = pmtiles_archive();
    z[100] = 32;
    z[101] = 40;
    write_input("regressions", "tile_archive", "zoom-32", &z);
    let files = skimap_fuzz::parse_archive(&pack_seed());
    let mut pack: Vec<(String, Vec<u8>)> = files;
    for (n, b) in &mut pack {
        if n == "manifest.json" {
            let mut m: serde_json::Value = serde_json::from_slice(b).expect("manifest");
            for l in m["layers"].as_array_mut().expect("layers") {
                if !l["raster"].is_null() {
                    l["raster"]["maxzoom"] = 40.into();
                }
            }
            *b = serde_json::to_vec(&m).expect("manifest");
        }
    }
    let refs: Vec<(&str, &[u8])> = pack
        .iter()
        .map(|(n, b)| (n.as_str(), b.as_slice()))
        .collect();
    write_input(
        "regressions",
        "pack",
        "raster-maxzoom-40",
        &encode_archive(&refs),
    );
}

fn gzip(b: &[u8]) -> Vec<u8> {
    let mut e = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
    e.write_all(b).expect("gzip");
    e.finish().expect("gzip")
}

fn grid<T: Copy>(w: usize, h: usize, f: impl Fn(usize, usize) -> T) -> Grid<T> {
    let data = (0..w * h).map(|i| f(i % w, i / w)).collect();
    Grid::from_vec(
        w,
        h,
        GeoTransform::north_up(1_000_000.0, 6_420_000.0, 5.0),
        Crs::Lambert93,
        data,
    )
    .expect("shape")
}

fn zone_table() -> String {
    encode_zone_classes(&[
        ZoneClass {
            zone_ids: vec![],
            representative: LonLat::new(6.75, 44.85),
            cells: 40,
        },
        ZoneClass {
            zone_ids: vec!["country:fr".into(), "inpn:FR9301502".into()],
            representative: LonLat::new(6.751, 44.851),
            cells: 8,
        },
    ])
    .expect("table")
}

fn offpiste_files() -> Vec<(&'static str, Vec<u8>)> {
    let (w, h) = (8, 6);
    let e = encode_raster_with(
        &grid(w, h, |c, r| 18_000 + 7 * c as u16 + 3 * r as u16),
        u16::MAX,
        Encoding::RowDelta,
    )
    .expect("elevation");
    let s = encode_raster_with(&grid(w, h, |c, _| (c * 9) as u8), u8::MAX, Encoding::Raw)
        .expect("slope");
    let z = encode_raster_with(&grid(w, h, |c, _| u8::from(c > 4)), u8::MAX, Encoding::Raw)
        .expect("zones");
    let p = encode_raster_with(
        &grid(w, h, |_, r| if r == 2 { 2 } else { 0 }),
        u8::MAX,
        Encoding::RowDelta,
    )
    .expect("pistes");
    let snow = SnowGrid {
        grid: grid(2, 2, |c, _| SnowGrid::pack(Some(c as u8), Some(2))),
    };
    let sn = encode_raster_with(&snow.grid, 0xFF, Encoding::Raw).expect("snow");
    let ex = encode_raster_with(&grid(w, h, |c, r| ((c + r) % 4) as u8), 255, Encoding::Raw)
        .expect("exposure");
    vec![
        ("elevation", e),
        ("slope", s),
        ("zones", z),
        ("pistes", p),
        ("classes", zone_table().into_bytes()),
        ("snow", sn),
        ("exposure", ex),
    ]
}

fn graph_json() -> String {
    let at = |x: f64, y: f64| LonLat::new(6.75 + x * 1e-5, 44.85 + y * 1e-5);
    let attrs = EdgeAttrs {
        kind: EdgeKind::Piste,
        name: Some("Le Bourget".into()),
        osm_way_id: Some(42),
        classic: Some(true),
        skating: Some(false),
        backcountry: false,
        nordic: true,
        hike: false,
        difficulty: None,
        difficulty_max: None,
    };
    let nodes = vec![
        Node {
            pos: at(0.0, 0.0),
            osm_node_id: Some(1),
        },
        Node {
            pos: at(100.0, 0.0),
            osm_node_id: Some(2),
        },
        Node {
            pos: at(100.0, 80.0),
            osm_node_id: None,
        },
    ];
    let edge = |from: usize, to: usize, oneway, profile: bool| {
        let points = vec![nodes[from].pos, nodes[to].pos];
        let len = points[0].haversine_m(points[1]) as f32;
        Edge {
            from,
            to,
            oneway,
            points,
            attrs: attrs.clone(),
            profile: profile.then(|| Profile {
                dist_m: vec![0.0, len / 2.0, len],
                elev_m: vec![1800.0, 1812.0, 1805.0],
                filled: 0,
            }),
        }
    };
    let edges = vec![
        edge(0, 1, false, true),
        edge(1, 2, true, false),
        edge(2, 0, false, true),
    ];
    PisteGraph::new(GraphData {
        site: "seed".into(),
        nodes,
        edges,
        elevation_source: Some("seed".into()),
    })
    .expect("graph")
    .to_json()
    .expect("json")
}

fn palette_png() -> Vec<u8> {
    let palette = [[0, 0, 0, 0], [200, 30, 30, 255], [30, 200, 30, 180]];
    // 256 × 256 like the pack tiles: bands of the three classes.
    let idx: Vec<u8> = (0..256 * 256).map(|i| ((i % 256) / 86) as u8).collect();
    skimap_pack::png::encode_indexed(256, 256, &palette, &idx).expect("png")
}

fn pmtiles_archive() -> Vec<u8> {
    use pmtiles::{PmTilesWriter, TileCoord, TileType};
    let mut out = Cursor::new(Vec::new());
    let mut w = PmTilesWriter::new(TileType::Png)
        .min_zoom(10)
        .max_zoom(12)
        .bounds(6.74, 44.84, 6.76, 44.86)
        .center(6.75, 44.85)
        .center_zoom(11)
        .metadata(r#"{"name":"seed"}"#)
        .create(&mut out)
        .expect("writer");
    let tile = palette_png();
    for z in 10..=12u8 {
        let (x, y) = skimap_common::crs::tile_for(LonLat::new(6.75, 44.85), z);
        w.add_tile(TileCoord::new(z, x, y).expect("coord"), &tile)
            .expect("tile");
    }
    w.finalize().expect("finalize");
    out.into_inner()
}

fn geotiff_u8() -> Vec<u8> {
    use tiff::encoder::{TiffEncoder, colortype::Gray8};
    use tiff::tags::Tag;
    let mut out = Cursor::new(Vec::new());
    let mut enc = TiffEncoder::new(&mut out).expect("tiff");
    let mut img = enc.new_image::<Gray8>(4, 3).expect("image");
    let d = img.encoder();
    d.write_tag(Tag::ModelPixelScaleTag, &[5.0f64, 5.0, 0.0][..])
        .expect("tag");
    d.write_tag(
        Tag::ModelTiepointTag,
        &[0.0f64, 0.0, 0.0, 1_000_000.0, 6_420_000.0, 0.0][..],
    )
    .expect("tag");
    d.write_tag(
        Tag::GeoKeyDirectoryTag,
        &[1u16, 1, 0, 2, 1024, 0, 1, 1, 3072, 0, 1, 2154][..],
    )
    .expect("tag");
    d.write_tag(Tag::GdalNodata, "255").expect("tag");
    img.write_data(&[1u8, 2, 3, 255, 0, 1, 2, 3, 3, 2, 1, 0])
        .expect("data");
    out.into_inner()
}

/// SHA-256 of `b` as the pack reader computes it. One file per call: the tests run in
/// parallel and build the seed pack at the same time (a shared file gave one test the
/// hashes of the other's bytes).
fn sha(b: &[u8]) -> String {
    static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let n = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!("skimap-seed-sha-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("tmp");
    let p = dir.join(format!("f{n}"));
    std::fs::write(&p, b).expect("tmp");
    let h = skimap_pack::reader::sha256_file(&p).expect("sha").1;
    std::fs::remove_file(&p).expect("tmp");
    h
}

/// A small synthetic pack built from the crate types (manifest, `pack.meta.json`): no data of the real packs, so nothing of a source that may not be published.
fn pack_seed() -> Vec<u8> {
    use skimap_common::meta::{LayerMeta, SourceRef};
    use skimap_pack::manifest::{
        Basemap, FileEntry, Layer, LayerKind, LegendEntry, Manifest, PackValidity, RasterInfo,
        SiteInfo, ValueEncoding,
    };
    let source = || SourceRef {
        name: "seed".into(),
        producer: None,
        url: "https://example.org/seed".into(),
        licence: "CC0-1.0".into(),
        attribution: "seed".into(),
        personal_use_only: false,
        edition: None,
        note: None,
        fetched_at: None,
        last_modified: None,
        etag: None,
        sha256: None,
    };
    let mut meta = LayerMeta::new("pack", "seed");
    meta.generated_at = "2026-11-15T00:00:00Z".into();
    meta.sources.push(source());
    let meta = serde_json::to_vec(&meta).expect("pack.meta.json");
    let geo = br#"{"type":"FeatureCollection","features":[{"type":"Feature","geometry":{"type":"LineString","coordinates":[[6.75,44.85],[6.751,44.851]]},"properties":{"name":"Le Bourget"}}]}"#.to_vec();
    let files: Vec<(&str, Vec<u8>)> = vec![
        ("layers/pistes.geojson", geo),
        (
            "layers/bera.json",
            br#"{"date":"2027-01-15","level":2}"#.to_vec(),
        ),
        ("layers/piste-graph.json", graph_json().into_bytes()),
        ("rasters/slope.pmtiles", pmtiles_archive()),
        (
            "layers/offpiste-elevation.skr.gz",
            gzip(&offpiste_files()[0].1),
        ),
        (
            "layers/offpiste-zone-classes.tsv",
            zone_table().into_bytes(),
        ),
        ("basemap/osm.pmtiles", pmtiles_archive()),
        (
            "basemap/osm/style.json",
            br#"{"version":8,"sources":{},"layers":[]}"#.to_vec(),
        ),
    ];
    let bytes = |path: &str| {
        files
            .iter()
            .find(|f| f.0 == path)
            .map_or(0, |f| f.1.len() as u64)
    };
    let layer =
        |id: &str, kind: LayerKind, format: &str, file: &str, raster: Option<RasterInfo>| Layer {
            id: id.into(),
            title: id.into(),
            kind,
            format: format.into(),
            file: file.into(),
            meta: None,
            origin: format!("seed/{id}"),
            generated_at: Some("2026-11-15T00:00:00Z".into()),
            valid_from: Some("2026-11-15".into()),
            valid_until: None,
            validity: Some("seed".into()),
            sources: vec![source()],
            personal_use_only: false,
            bytes: bytes(file),
            features: None,
            geometry_types: vec![],
            visible: true,
            raster,
            classes: None,
            notes: vec![],
        };
    let slope = RasterInfo {
        minzoom: 10,
        maxzoom: 12,
        tile_size: 256,
        tile_count: 3,
        encoding: ValueEncoding::PaletteIndex,
        nodata_index: None,
        legend: (0..3)
            .map(|v| LegendEntry {
                value: v,
                label: format!("classe {v}"),
                color: "#c81e1eff".into(),
            })
            .collect(),
        source: "seed".into(),
        downsampling: "seed".into(),
    };
    let manifest = Manifest {
        format: skimap_pack::FORMAT.into(),
        format_version: skimap_pack::FORMAT_VERSION,
        pack_version: "20261115T000000Z".into(),
        generator: "skimap-fuzz seed".into(),
        generated_at: "2026-11-15T00:00:00Z".into(),
        site: SiteInfo {
            id: "cervieres".into(),
            name: "Cervières".into(),
            department: "05".into(),
            bbox: [6.74, 44.84, 6.76, 44.86],
            center: [6.75, 44.85],
            record: None,
            start_ranking: None,
        },
        valid_for: PackValidity {
            until: None,
            until_layer: None,
            note: "seed".into(),
        },
        personal_sources: false,
        basemaps: vec![Basemap {
            id: "osm".into(),
            title: "OSM".into(),
            tiles: "basemap/osm.pmtiles".into(),
            style: "basemap/osm/style.json".into(),
            minzoom: 10,
            maxzoom: 12,
            tile_count: 3,
            bytes: bytes("basemap/osm.pmtiles"),
            data_date: None,
            sources: vec![source()],
            notes: vec![],
        }],
        layers: vec![
            layer(
                "pistes",
                LayerKind::Vector,
                "geojson",
                "layers/pistes.geojson",
                None,
            ),
            layer("bera", LayerKind::Data, "json", "layers/bera.json", None),
            layer(
                "piste-graph",
                LayerKind::Data,
                "json",
                "layers/piste-graph.json",
                None,
            ),
            layer(
                "slope",
                LayerKind::Raster,
                "pmtiles",
                "rasters/slope.pmtiles",
                Some(slope),
            ),
            layer(
                "offpiste-elevation",
                LayerKind::Data,
                "skr+gzip",
                "layers/offpiste-elevation.skr.gz",
                None,
            ),
            layer(
                "offpiste-zone-classes",
                LayerKind::Data,
                "tsv",
                "layers/offpiste-zone-classes.tsv",
                None,
            ),
        ],
        missing: vec![],
        attributions: vec!["seed".into()],
        files: files
            .iter()
            .map(|(n, b)| FileEntry {
                path: (*n).into(),
                bytes: b.len() as u64,
                sha256: sha(b),
            })
            .collect(),
        total_bytes: files.iter().map(|(_, b)| b.len() as u64).sum(),
    };
    let manifest = serde_json::to_vec_pretty(&manifest).expect("manifest");
    let mut all: Vec<(&str, &[u8])> = vec![("manifest.json", &manifest), ("pack.meta.json", &meta)];
    all.extend(files.iter().map(|(n, b)| (*n, b.as_slice())));
    encode_archive(&all)
}

/// Bundles of `skimap_pack::update` from the seed pack: a first install, and a delta to a
/// version where one layer changed; with a site index and a sync state.
fn update_seeds() -> Vec<(&'static str, Vec<u8>)> {
    use skimap_pack::update::{Update, parse_manifest, write_bundle};
    let files = skimap_fuzz::parse_archive(&pack_seed());
    let manifest_bytes = files
        .iter()
        .find(|f| f.0 == "manifest.json")
        .expect("manifest")
        .1
        .clone();
    let manifest = parse_manifest(&manifest_bytes, "manifest.json").expect("seed manifest");
    let bundle = |update: &Update, files: &[(String, Vec<u8>)], manifest_bytes: &[u8]| {
        let mut out = Vec::new();
        write_bundle(&mut out, update, |sha| {
            if skimap_pack::update::sha256_hex(manifest_bytes) == sha {
                return Ok(Cursor::new(manifest_bytes.to_vec()));
            }
            let f = files
                .iter()
                .find(|f| skimap_pack::update::sha256_hex(&f.1) == sha)
                .expect("blob of the seed pack");
            Ok(Cursor::new(f.1.clone()))
        })
        .expect("bundle");
        out
    };
    let first = Update::between(None, &manifest, &manifest_bytes);
    let base = bundle(&first, &files, &manifest_bytes);
    // Next version: bera.json changed.
    let mut next_files = files.clone();
    let bera = br#"{"date":"2027-01-16","level":3}"#.to_vec();
    for f in &mut next_files {
        if f.0 == "layers/bera.json" {
            f.1 = bera.clone();
        }
    }
    let mut m: serde_json::Value = serde_json::from_slice(&manifest_bytes).expect("manifest");
    m["pack_version"] = "20261116T000000Z".into();
    let mut total = 0u64;
    for f in m["files"].as_array_mut().expect("files") {
        if f["path"] == "layers/bera.json" {
            f["bytes"] = bera.len().into();
            f["sha256"] = skimap_pack::update::sha256_hex(&bera).into();
        }
        total += f["bytes"].as_u64().expect("bytes");
    }
    m["total_bytes"] = total.into();
    let next_bytes = serde_json::to_vec_pretty(&m).expect("manifest");
    let next = parse_manifest(&next_bytes, "manifest.json").expect("next manifest");
    let delta_update = Update::between(Some(&manifest), &next, &next_bytes);
    let delta = bundle(&delta_update, &next_files, &next_bytes);
    let bref = |from: Option<&str>, to: &str, b: &[u8]| {
        let sha = skimap_pack::update::sha256_hex(b);
        serde_json::json!({"from": from, "to": to, "sha256": sha, "bytes": b.len(),
            "url": format!("/v1/bundles/{sha}.skb"), "blobs": 1})
    };
    let index = serde_json::json!({
        "format": "skimap-site-index", "format_version": 1,
        "site": m["site"].clone(),
        "latest": {"version": "20261116T000000Z", "published_at": "2026-11-16T00:00:00Z",
                   "manifest_sha256": skimap_pack::update::sha256_hex(&next_bytes),
                   "total_bytes": total, "files": next.files.len(),
                   "valid_for": m["valid_for"].clone(), "personal_sources": false},
        "base_version": manifest.pack_version.clone(),
        "base": bref(None, &manifest.pack_version, &base),
        "deltas": [bref(Some(&manifest.pack_version), "20261116T000000Z", &delta)],
        "changes": [],
    });
    let index = serde_json::to_vec(&index).expect("index");
    let state = serde_json::to_vec(&serde_json::json!({
        "version": manifest.pack_version, "base_version": manifest.pack_version, "index_etag": "\"x\""
    }))
    .expect("state");
    let install = encode_archive(&[("bundle", &base), ("index", &index)]);
    let mut entries: Vec<(String, Vec<u8>)> = files
        .iter()
        .map(|(n, b)| (format!("old/{n}"), b.clone()))
        .collect();
    entries.push(("bundle".into(), delta));
    entries.push(("index".into(), index));
    entries.push(("state".into(), state));
    let refs: Vec<(&str, &[u8])> = entries
        .iter()
        .map(|(n, b)| (n.as_str(), b.as_slice()))
        .collect();
    vec![("install", install), ("delta", encode_archive(&refs))]
}

/// The update seeds install: the first bundle into an empty directory, the delta onto the
/// seed pack (the seeds exercise the installer past its checks).
#[test]
fn update_seeds_apply() {
    for (name, seed) in update_seeds() {
        let files = skimap_fuzz::parse_archive(&seed);
        let root =
            std::env::temp_dir().join(format!("skimap-seed-update-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let pack_dir = root.join("cervieres");
        for (n, b) in &files {
            if let Some(rel) = n.strip_prefix("old/") {
                let p = pack_dir.join(rel);
                std::fs::create_dir_all(p.parent().expect("parent")).expect("dir");
                std::fs::write(p, b).expect("file");
            }
        }
        std::fs::create_dir_all(&pack_dir).expect("dir");
        let bundle = root.join("b.skb");
        std::fs::write(
            &bundle,
            &files.iter().find(|f| f.0 == "bundle").expect("bundle").1,
        )
        .expect("bundle");
        let applied = skimap_pack::update::apply_bundle(&pack_dir, &bundle).expect(name);
        assert_eq!(
            skimap_pack::Pack::open(&pack_dir)
                .expect("open")
                .verify()
                .expect("verify"),
            Vec::<String>::new(),
            "{name}"
        );
        println!("{name}: {applied:?}");
    }
}

/// The seed pack opens and verifies: the seed exercises the reader past its checks.
#[test]
fn pack_seed_is_valid() {
    let files = skimap_fuzz::parse_archive(&pack_seed());
    let dir = std::env::temp_dir().join(format!("skimap-seed-pack-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    for (n, b) in &files {
        let p = dir.join(n);
        std::fs::create_dir_all(p.parent().expect("parent")).expect("dir");
        std::fs::write(p, b).expect("file");
    }
    let p = skimap_pack::Pack::open(&dir).expect("open");
    assert_eq!(p.verify().expect("verify"), Vec::<String>::new());
    assert!(p.read_geojson("pistes").is_ok());
    assert!(
        p.raster_value_at("slope", LonLat::new(6.75, 44.85))
            .expect("value")
            .is_some()
    );
}

/// Writes the hand-made seeds and the regression inputs of every target.
#[test]
#[ignore = "regenerates fuzz/seeds and fuzz/regressions"]
fn make_seeds() {
    write_regressions();
    let off = offpiste_files();
    for (name, data) in &off {
        if *name != "classes" {
            write_seed("skr1", name, data);
        }
    }
    write_seed("skr1", "elevation.gz", &gzip(&off[0].1));
    write_seed("zone_classes", "table", zone_table().as_bytes());
    write_seed("piste_graph", "triangle", graph_json().as_bytes());
    let refs: Vec<(&str, &[u8])> = off.iter().map(|(n, b)| (*n, b.as_slice())).collect();
    write_seed("offpiste_grid", "grid", &encode_archive(&refs));
    write_seed("pack", "minimal", &pack_seed());
    for (name, seed) in update_seeds() {
        write_seed("pack_update", name, &seed);
    }
    write_seed("pmtiles_vendor", "png3", &pmtiles_archive());
    write_seed("tile_archive", "png3", &pmtiles_archive());
    write_seed("png", "palette", &palette_png());
    {
        use skimap_pack::mvt::{LayerWriter, Value, encode_tile};
        let mut l = LayerWriter::new("slope");
        // Exterior (positive area, y down) with a hole, and a second polygon.
        let outer = vec![[100, 100], [3000, 100], [3000, 3000], [100, 3000]];
        let hole = vec![[1000, 1000], [1000, 2000], [2000, 2000], [2000, 1000]];
        l.add_polygon(&[("class", Value::Int(2))], &[outer, hole]);
        l.add_polygon(
            &[
                ("class", Value::Int(3)),
                ("label", Value::Str("seed".into())),
            ],
            &[vec![[3500, 3500], [4000, 3500], [4000, 4000]]],
        );
        write_seed("mvt", "slope", &encode_tile(&[l], 4096).expect("tile"));
    }
    let g = grid(5, 4, |c, r| {
        if c == r {
            f32::NAN
        } else {
            c as f32 * 1.5 - r as f32
        }
    });
    let mut tif = Cursor::new(Vec::new());
    skimap_common::geotiff::encode_f32(&mut tif, &g).expect("geotiff");
    write_seed("geotiff", "f32", &tif.into_inner());
    write_seed("geotiff", "u8", &geotiff_u8());
    let meteo = Path::new(env!("CARGO_MANIFEST_DIR")).join("../crates/meteo/tests/fixtures");
    for f in [
        "bera_spec_belledonne_20231026.xml",
        "BRA.QUEYRAS.20230415135925.noimg.xml",
    ] {
        write_seed(
            "bera",
            f,
            &std::fs::read(meteo.join(f)).expect("BERA fixture"),
        );
    }
}
