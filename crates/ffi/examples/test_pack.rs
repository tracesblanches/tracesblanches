//! Writes the test pack of the Android app (instrumented and JVM tests, Q8 #108):
//! synthetic pistes, piste status and refuges around Cervières, built by the pack builder
//! of the pipeline (offline, no basemap download), plus a synthetic vector basemap (two
//! MVT layers in a PMTiles archive) so that the map draws basemap tiles too.
//!
//! `cargo run -p skimap-ffi --example test_pack -- <pack directory>`
//!
//! Deterministic apart from the build times written in the manifest. About 30 kB.

use std::io::Write as _;
use std::path::{Path, PathBuf};

use flate2::Compression as Gz;
use flate2::write::GzEncoder;
use pmtiles::{Compression, PmTilesWriter, TileCoord, TileId, TileType};
use serde_json::{Value, json};
use skimap_common::download::Downloader;
use skimap_common::geom::BBox;
use skimap_common::layout::DataLayout;
use skimap_common::meta::SourceRef;
use skimap_common::site::CERVIERES;
use skimap_pack::build::{BuildOptions, build, list_files};
use skimap_pack::manifest::{Basemap, MANIFEST_FILE, Manifest};
use skimap_pack::tiles::tiles_covering;

type Res<T> = Result<T, Box<dyn std::error::Error>>;

fn main() -> Res<()> {
    let out = std::env::args_os()
        .nth(1)
        .map(PathBuf::from)
        .ok_or("usage: test_pack <pack directory>")?;
    let report = write_test_pack(&out)?;
    println!("{report}");
    Ok(())
}

/// Builds the test pack into `out` (replaced if it exists). Returns a one-line summary.
pub(crate) fn write_test_pack(out: &Path) -> Res<String> {
    let parent = out.parent().ok_or("pack directory without parent")?;
    std::fs::create_dir_all(parent)?;
    let work = parent.join(".test-pack-work");
    if work.exists() {
        std::fs::remove_dir_all(&work)?;
    }
    pipeline_outputs(&work.join("out/cervieres"))?;
    let layout = DataLayout::new(&work);
    let dl = Downloader::builder(layout.cache_root())
        .offline(true)
        .build()?;
    let opts = BuildOptions {
        plan_ign: false,
        osm: false,
        ign_slopes: false,
        voice: false,
        offpiste: false,
        personal_sources: true,
        ..BuildOptions::default()
    };
    let report = build(&CERVIERES, &layout, &dl, &opts)?;
    let pack = report.pack_dir;
    add_basemap(&pack)?;
    if out.exists() {
        std::fs::remove_dir_all(out)?;
    }
    std::fs::rename(&pack, out)?;
    std::fs::remove_dir_all(&work)?;

    let p = skimap_pack::Pack::open(out)?;
    let problems = p.verify()?;
    if !problems.is_empty() {
        return Err(format!("test pack does not verify: {problems:?}").into());
    }
    let m = p.manifest();
    Ok(format!(
        "test pack {}: {} files, {} bytes, layers {:?}, basemaps {:?}",
        out.display(),
        m.files.len(),
        m.total_bytes,
        m.layers.iter().map(|l| l.id.as_str()).collect::<Vec<_>>(),
        m.basemaps.iter().map(|b| b.id.as_str()).collect::<Vec<_>>()
    ))
}

fn write_json(path: &Path, v: &Value) -> Res<()> {
    std::fs::create_dir_all(path.parent().ok_or("no parent")?)?;
    std::fs::write(path, serde_json::to_vec_pretty(v)?)?;
    Ok(())
}

fn meta(personal: bool, attribution: &str) -> Value {
    json!({"generated_at": "2026-11-15T00:00:00Z", "valid_until": "2027-04-30",
           "sources": [{"name": "synthétique", "url": "https://example.org/skimap-test",
                        "licence": "domaine public", "attribution": attribution,
                        "personal_use_only": personal}]})
}

fn line(coords: &[(f64, f64)]) -> Value {
    let c: Vec<Value> = coords.iter().map(|(x, y)| json!([x, y])).collect();
    json!({"type": "LineString", "coordinates": c})
}

/// Pipeline outputs read by the pack builder: pistes, status, refuges.
fn pipeline_outputs(dir: &Path) -> Res<()> {
    // Six pistes around the commune centre (6.7746, 44.8586), one per difficulty, a
    // pedestrian trail and a one-way piste.
    let (x, y) = (6.7746, 44.8586);
    let piste = |name: &str, difficulty: Value, uses: Value, oneway: bool, c: &[(f64, f64)]| {
        json!({"type": "Feature",
               "properties": {"name": name, "difficulty": difficulty, "uses": uses, "oneway": oneway,
                              "grooming": "classic;skating", "length_m": 1000.0},
               "geometry": line(c)})
    };
    let pistes = json!({"type": "FeatureCollection", "features": [
        piste("Verte du village", json!("novice"), json!(["nordic"]), false,
              &[(x - 0.03, y - 0.010), (x - 0.01, y - 0.012), (x + 0.01, y - 0.010)]),
        piste("Bleue des Fonts", json!("easy"), json!(["nordic"]), false,
              &[(x - 0.03, y - 0.002), (x, y), (x + 0.03, y + 0.002)]),
        piste("Rouge du Laus", json!("intermediate"), json!(["nordic"]), true,
              &[(x - 0.03, y + 0.008), (x, y + 0.012), (x + 0.03, y + 0.010)]),
        piste("Noire de l'Izoard", json!("advanced"), json!(["nordic"]), false,
              &[(x - 0.02, y + 0.020), (x + 0.02, y + 0.024)]),
        piste("Sans couleur", Value::Null, json!(["nordic"]), false,
              &[(x + 0.012, y - 0.020), (x + 0.03, y - 0.016)]),
        piste("Sentier raquettes", json!("easy"), json!(["hike"]), false,
              &[(x - 0.03, y - 0.022), (x - 0.005, y - 0.026)]),
    ]});
    write_json(&dir.join("pistes/pistes.geojson"), &pistes)?;
    write_json(
        &dir.join("pistes/pistes.meta.json"),
        &meta(false, "pistes synthétiques"),
    )?;

    let status = |name: &str, state: &str, c: &[(f64, f64)]| {
        let coords: Vec<Value> = c.iter().map(|(a, b)| json!([a, b])).collect();
        json!({"type": "Feature",
               "properties": {"name": name, "state": state, "polled_at": "2026-11-15T01:45:36Z"},
               "geometry": {"type": "MultiLineString", "coordinates": [coords]}})
    };
    let status = json!({"type": "FeatureCollection", "features": [
        status("Les Fonts", "open", &[(x - 0.03, y - 0.002), (x, y), (x + 0.03, y + 0.002)]),
        status("Le Laus", "closed", &[(x - 0.03, y + 0.008), (x, y + 0.012), (x + 0.03, y + 0.010)]),
    ]});
    write_json(&dir.join("status/status.geojson"), &status)?;
    write_json(
        &dir.join("status/status.meta.json"),
        &meta(true, "état synthétique"),
    )?;

    let point = |lon: f64, lat: f64, props: Value| {
        json!({"type": "Feature", "properties": props,
               "geometry": {"type": "Point", "coordinates": [lon, lat]}})
    };
    write_json(
        &dir.join("refuges/refuges_info.geojson"),
        &json!({"type": "FeatureCollection", "features": [
            point(x - 0.015, y + 0.016, json!({"name": "Cabane du test", "type_label": "cabane non gardée", "places": 6})),
            point(x + 0.015, y - 0.006, json!({"name": "Source", "type_label": "point d'eau"})),
        ]}),
    )?;
    write_json(
        &dir.join("refuges/osm.geojson"),
        &json!({"type": "FeatureCollection", "features": [
            point(x + 0.02, y + 0.016, json!({"kind": "wilderness_hut", "tags": {"name": "Abri OSM"}})),
            point(x - 0.02, y - 0.016, json!({"kind": "bus_shelter", "tags": {"name": "Arrêt de bus"}})),
        ]}),
    )?;
    write_json(
        &dir.join("refuges/camptocamp.geojson"),
        &json!({"type": "FeatureCollection", "features": [
            point(x + 0.005, y - 0.017, json!({"title": "Gîte Camptocamp", "waypoint_type": "gite"})),
        ]}),
    )?;
    write_json(
        &dir.join("refuges/refuges.meta.json"),
        &meta(false, "refuges synthétiques"),
    )?;
    Ok(())
}

// --- Minimal Mapbox Vector Tile encoder (protobuf), enough for two layers. -------------

fn varint(out: &mut Vec<u8>, mut v: u64) {
    while v >= 0x80 {
        out.push((v as u8 & 0x7f) | 0x80);
        v >>= 7;
    }
    out.push(v as u8);
}

fn field_bytes(out: &mut Vec<u8>, field: u32, bytes: &[u8]) {
    varint(out, u64::from(field << 3 | 2));
    varint(out, bytes.len() as u64);
    out.extend_from_slice(bytes);
}

fn field_varint(out: &mut Vec<u8>, field: u32, v: u64) {
    varint(out, u64::from(field << 3));
    varint(out, v);
}

fn zigzag(v: i32) -> u32 {
    ((v << 1) ^ (v >> 31)) as u32
}

/// One feature of geometry type `kind` (2 line, 3 polygon) with command integers `geom`.
fn feature(kind: u64, geom: &[u32]) -> Vec<u8> {
    let mut f = Vec::new();
    field_varint(&mut f, 3, kind);
    let mut packed = Vec::new();
    for g in geom {
        varint(&mut packed, u64::from(*g));
    }
    field_bytes(&mut f, 4, &packed);
    f
}

fn layer(name: &str, features: &[Vec<u8>]) -> Vec<u8> {
    let mut l = Vec::new();
    field_varint(&mut l, 15, 2);
    field_bytes(&mut l, 1, name.as_bytes());
    for f in features {
        field_bytes(&mut l, 2, f);
    }
    field_varint(&mut l, 5, 4096);
    l
}

/// Tile: a `snow` polygon over the whole tile and a `grid` square along its border.
fn tile() -> Vec<u8> {
    let (a, b) = (-64, 4160);
    let move_to = 1 | (1 << 3);
    let line_to = |n: u32| 2 | (n << 3);
    let close = 7 | (1 << 3);
    // Clockwise in tile coordinates (y down): exterior ring.
    let polygon = [
        move_to,
        zigzag(a),
        zigzag(a),
        line_to(3),
        zigzag(b - a),
        0,
        0,
        zigzag(b - a),
        zigzag(a - b),
        0,
        close,
    ];
    let border = [
        move_to,
        0,
        0,
        line_to(4),
        zigzag(4096),
        0,
        0,
        zigzag(4096),
        zigzag(-4096),
        0,
        0,
        zigzag(-4096),
    ];
    let mut t = Vec::new();
    field_bytes(&mut t, 3, &layer("snow", &[feature(3, &polygon)]));
    field_bytes(&mut t, 3, &layer("grid", &[feature(2, &border)]));
    t
}

fn gzip(data: &[u8]) -> Res<Vec<u8>> {
    let mut e = GzEncoder::new(Vec::new(), Gz::best());
    e.write_all(data)?;
    Ok(e.finish()?)
}

/// Writes `basemap/test.pmtiles` and `basemap/test/style.json`, adds the basemap to the
/// manifest and lists the files again.
fn add_basemap(pack: &Path) -> Res<()> {
    let bbox = CERVIERES.bbox;
    let c = CERVIERES.commune_center;
    let near = BBox::new(c.lat - 0.03, c.lon - 0.04, c.lat + 0.03, c.lon + 0.04);
    let mut coords: Vec<(u8, u32, u32)> = tiles_covering(bbox, 8, 13);
    coords.extend(tiles_covering(near, 14, 14));
    let mut ids: Vec<u64> = coords
        .iter()
        .map(|&(z, x, y)| Ok(TileId::from(TileCoord::new(z, x, y)?).value()))
        .collect::<Res<_>>()?;
    ids.sort_unstable();
    ids.dedup();
    let data = gzip(&tile())?;

    let path = pack.join("basemap/test.pmtiles");
    std::fs::create_dir_all(pack.join("basemap/test"))?;
    let f = std::fs::File::create(&path)?;
    let mut w = PmTilesWriter::new(TileType::Mvt)
        .precompressed(Compression::Gzip)
        .min_zoom(8)
        .max_zoom(14)
        .bounds(bbox.west, bbox.south, bbox.east, bbox.north)
        .center(c.lon, c.lat)
        .center_zoom(12)
        .metadata(
            json!({"vector_layers": [{"id": "snow", "fields": {}}, {"id": "grid", "fields": {}}]})
                .to_string(),
        )
        .create(f)?;
    for id in &ids {
        w.add_tile(TileId::new(*id)?.into(), &data)?;
    }
    w.finalize()?;

    let style = json!({
        "version": 8,
        "name": "skimap test basemap",
        "sources": {"test": {"type": "vector", "url": "pmtiles://basemap/test.pmtiles",
                             "attribution": "Fond de test synthétique"}},
        "layers": [
            {"id": "background", "type": "background", "paint": {"background-color": "#dfe7ee"}},
            {"id": "snow", "type": "fill", "source": "test", "source-layer": "snow",
             "paint": {"fill-color": "#f4f7fa"}},
            {"id": "grid", "type": "line", "source": "test", "source-layer": "grid",
             "paint": {"line-color": "#9aa8b5", "line-width": 1}}
        ]
    });
    write_json(&pack.join("basemap/test/style.json"), &style)?;

    let mpath = pack.join(MANIFEST_FILE);
    let mut m: Manifest = serde_json::from_slice(&std::fs::read(&mpath)?)?;
    m.site.id = "test".to_owned();
    m.site.name = "Pack de test".to_owned();
    let bytes = std::fs::metadata(&path)?.len()
        + std::fs::metadata(pack.join("basemap/test/style.json"))?.len();
    m.basemaps = vec![Basemap {
        id: "test".to_owned(),
        title: "Fond de test".to_owned(),
        tiles: "basemap/test.pmtiles".to_owned(),
        style: "basemap/test/style.json".to_owned(),
        minzoom: 8,
        maxzoom: 14,
        tile_count: ids.len() as u64,
        bytes,
        data_date: None,
        sources: vec![SourceRef::new(
            "fond de test",
            "https://example.org/skimap-test",
            "domaine public",
            "Fond de test synthétique",
        )],
        notes: vec!["Fond synthétique des tests de l'app, sans données réelles.".to_owned()],
    }];
    m.attributions
        .insert(0, "Fond de test synthétique".to_owned());
    m.files = list_files(pack)?;
    m.total_bytes = m.files.iter().map(|f| f.bytes).sum();
    write_json(&mpath, &serde_json::to_value(&m)?)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn varints_and_zigzag() {
        let mut v = Vec::new();
        varint(&mut v, 300);
        assert_eq!(v, [0xac, 0x02]);
        assert_eq!(zigzag(0), 0);
        assert_eq!(zigzag(-1), 1);
        assert_eq!(zigzag(1), 2);
        assert_eq!(zigzag(-64), 127);
    }

    #[test]
    fn writes_a_pack_that_verifies() {
        let tmp = tempfile::tempdir().unwrap();
        let out = tmp.path().join("packs/test");
        let line = write_test_pack(&out).unwrap();
        assert!(line.contains("\"pistes\""), "{line}");
        let p = skimap_pack::Pack::open(&out).unwrap();
        assert!(p.verify().unwrap().is_empty());
        let t = p.tiles("test").unwrap();
        assert_eq!(t.zoom_range(), (8, 14));
        let (tx, ty, _, _) =
            skimap_pack::tiles::tile_pixel(skimap_common::geom::LonLat::new(6.7746, 44.8586), 14);
        assert_eq!(t.tile(14, tx, ty).unwrap().unwrap(), tile());
        assert!(!tmp.path().join("packs/.test-pack-work").exists());
    }
}
