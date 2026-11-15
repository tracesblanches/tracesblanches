//! Writes the test pack of the Android app (instrumented and JVM tests, Q8 #108):
//! synthetic pistes, piste status and refuges around Cervières, built by the pack builder
//! of the pipeline (offline, no basemap download), plus what the validated styles of
//! `styles/` read from a Plan IGN basemap: a synthetic `basemap/plan-ign.pmtiles` (forest,
//! road and water in the IGN source layers), glyph ranges (empty: no text is drawn) and a
//! sprite with the two refuge icons. The data follow the fields and values the styles
//! filter on (`uses`, `difficulty`, `state`, `type_label`...).
//!
//! `cargo run -p skimap-ffi --example test_pack -- <pack directory> [2]`
//!
//! The optional `2` writes version 2 of the same pack (another piste status, another
//! refuge capacity, a later `pack_version`): the server publishes both, the app updates
//! from one to the other by a delta (K2). `voice-en` writes a voice language pack apart
//! (VO6): site `test-voice-en`, a clip manifest and two clips, nothing else. Deterministic apart from the build times written
//! in the manifest. About 40 kB.

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
    let report = match std::env::args().nth(2).as_deref() {
        None | Some("1") => write_test_pack(&out, 1)?,
        Some("2") => write_test_pack(&out, 2)?,
        Some("voice-en") => write_voice_pack(&out, "en")?,
        Some(v) => return Err(format!("unknown variant {v}").into()),
    };
    println!("{report}");
    Ok(())
}

/// Version of each variant of the test pack.
pub(crate) const VERSIONS: [&str; 2] = ["20261115T060000Z", "20261116T060000Z"];

/// Builds variant 1 or 2 of the test pack into `out` (replaced if it exists). Returns a
/// one-line summary.
pub(crate) fn write_test_pack(out: &Path, variant: u8) -> Res<String> {
    let parent = out.parent().ok_or("pack directory without parent")?;
    std::fs::create_dir_all(parent)?;
    let work = parent.join(".test-pack-work");
    if work.exists() {
        std::fs::remove_dir_all(&work)?;
    }
    pipeline_outputs(&work.join("out/cervieres"), variant)?;
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
    add_basemap(&pack, VERSIONS[usize::from(variant - 1)])?;
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

/// Trail-pass zones of the test pack (schema of `crates/ffi/src/fees.rs`, no producer in
/// `main` yet): the site pass over the area and a national pass beyond it.
fn add_fee_zones(pack: &Path, m: &mut Manifest) -> Res<()> {
    let b = CERVIERES.bbox;
    let rect = |w: f64, s: f64, e: f64, n: f64| {
        json!({"type": "Polygon",
        "coordinates": [[[w, s], [e, s], [e, n], [w, n], [w, s]]]})
    };
    let fc = json!({"type": "FeatureCollection", "features": [
        {"type": "Feature", "geometry": rect(b.west, b.south, b.east, b.north),
         "properties": {"fee_id": "test-site", "name": "Redevance du domaine de test", "issuer": "Foyer de test",
                        "kind": "site", "prices": [{"label": "Journée adulte", "price_eur": 9.5, "date": "2026-11-01"}]}},
        {"type": "Feature", "geometry": rect(b.west - 1.0, b.south - 1.0, b.east + 1.0, b.north + 1.0),
         "properties": {"fee_id": "nordic-pass-national", "name": "Nordic Pass national", "issuer": "Nordic France",
                        "kind": "national", "prices": [{"label": "Saison adulte", "price_eur": 240.0, "date": "2026-11-01"}]}}
    ]});
    let file = "layers/fee-zones.geojson";
    write_json(&pack.join(file), &fc)?;
    let mut layer = serde_json::to_value(m.layer("status").ok_or("no status layer")?)?;
    layer["id"] = json!("fee-zones");
    layer["title"] = json!("Zones de validité des redevances (test)");
    layer["file"] = json!(file);
    layer["meta"] = Value::Null;
    layer["origin"] = json!("crates/ffi/examples/test_pack.rs");
    layer["personal_use_only"] = json!(false);
    layer["sources"] = json!([]);
    layer["bytes"] = json!(std::fs::metadata(pack.join(file))?.len());
    layer["features"] = json!(2);
    layer["geometry_types"] = json!(["Polygon"]);
    layer["visible"] = json!(false);
    m.layers.push(serde_json::from_value(layer)?);
    Ok(())
}

/// Writes a voice language pack of the test site into `out` (replaced if it exists): the
/// layout of the clips of a site pack (`voice/manifest.json`, `voice/clips/*.opus`) in a
/// pack of its own, site `test-voice-<lang>`. The clips are not real Opus files.
pub(crate) fn write_voice_pack(out: &Path, lang: &str) -> Res<String> {
    if out.exists() {
        std::fs::remove_dir_all(out)?;
    }
    let clips = out.join("voice/clips");
    std::fs::create_dir_all(&clips)?;
    let texts = [
        ("tourner-a-droite", "Turn right"),
        ("refuge-du-test", "Test hut"),
    ];
    for (id, _) in texts {
        std::fs::write(
            clips.join(format!("{id}.opus")),
            format!("OggS fake clip {id} {lang}"),
        )?;
    }
    write_json(
        &out.join("voice/manifest.json"),
        &json!({"lang": lang, "clips": texts.iter().map(|(id, t)| json!({"id": id, "text": t,
                                                                          "file": format!("clips/{id}.opus")}))
                                         .collect::<Vec<_>>()}),
    )?;
    let files = list_files(out)?;
    let bytes: u64 = files.iter().map(|f| f.bytes).sum();
    let mut m: Manifest = serde_json::from_value(json!({
        "format": skimap_pack::manifest::FORMAT,
        "format_version": skimap_pack::manifest::FORMAT_VERSION,
        "pack_version": VERSIONS[0], "generator": "skimap-ffi test_pack", "generated_at": "2026-11-15T06:00:00Z",
        "site": {"id": format!("test-voice-{lang}"), "name": format!("Pack de test, voix {lang}"), "department": "05",
                 "bbox": [CERVIERES.bbox.west, CERVIERES.bbox.south, CERVIERES.bbox.east, CERVIERES.bbox.north],
                 "center": [CERVIERES.commune_center.lon, CERVIERES.commune_center.lat]},
        "valid_for": {"until": null, "until_layer": null, "note": "voix"},
        "personal_sources": false, "basemaps": [], "layers": [],
        "voice": {"status": "present", "manifest": "voice/manifest.json", "clips_dir": "voice/clips",
                  "clips": texts.len(), "bytes": bytes, "sources": [], "note": format!("voix de test, {lang}")},
        "missing": [], "attributions": [], "files": [], "total_bytes": 0
    }))?;
    m.files = files;
    m.total_bytes = bytes;
    write_json(&out.join(MANIFEST_FILE), &serde_json::to_value(&m)?)?;
    let p = skimap_pack::Pack::open(out)?;
    let problems = p.verify()?;
    if !problems.is_empty() {
        return Err(format!("voice pack does not verify: {problems:?}").into());
    }
    Ok(format!(
        "voice pack {}: {} files, {bytes} bytes",
        out.display(),
        m.files.len()
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
fn pipeline_outputs(dir: &Path, variant: u8) -> Res<()> {
    // Six pistes around the commune centre (6.7746, 44.8586), one per difficulty, a
    // pedestrian trail and a one-way piste.
    let (x, y) = (6.7746, 44.8586);
    // OSM way ids 1 to 6: the piste status joins on them (`osm_way_ids` of the status).
    let mut way = 0;
    let mut piste = |name: &str, difficulty: Value, uses: Value, oneway: bool, c: &[(f64, f64)]| {
        way += 1;
        json!({"type": "Feature",
               "properties": {"name": name, "osm_way_id": way, "difficulty": difficulty, "uses": uses, "oneway": oneway,
                              "grooming": "classic;skating", "classic": true, "skating": true,
                              "length_m": 1000.0},
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

    let status = |name: &str, state: &str, way: i64, c: &[(f64, f64)]| {
        let coords: Vec<Value> = c.iter().map(|(a, b)| json!([a, b])).collect();
        json!({"type": "Feature",
               "properties": {"name": name, "state": state, "polled_at": "2026-11-15T01:45:36Z",
                              "source": "test", "itinerary": name, "osm_way_ids": [way]},
               "geometry": {"type": "MultiLineString", "coordinates": [coords]}})
    };
    // Version 2: Le Laus opens.
    let laus = if variant == 2 { "open" } else { "closed" };
    let status = json!({"type": "FeatureCollection", "features": [
        status("Les Fonts", "open", 2, &[(x - 0.03, y - 0.002), (x, y), (x + 0.03, y + 0.002)]),
        status("Le Laus", laus, 3, &[(x - 0.03, y + 0.008), (x, y + 0.012), (x + 0.03, y + 0.010)]),
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
            point(x - 0.015, y + 0.016, json!({"name": "Refuge du test", "type_label": "refuge gardé",
                                                "places": if variant == 2 { 24 } else { 18 }})),
            point(x + 0.022, y - 0.004, json!({"name": "Cabane du test", "type_label": "cabane non gardée", "places": 6})),
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

// --- Minimal Mapbox Vector Tile encoder (protobuf), enough for a few tagged layers. ----

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

/// One feature of geometry type `kind` (2 line, 3 polygon) with command integers `geom`
/// and tags (key index, value index).
fn feature(kind: u64, geom: &[u32], tags: &[u32]) -> Vec<u8> {
    let mut f = Vec::new();
    if !tags.is_empty() {
        let mut packed = Vec::new();
        for t in tags {
            varint(&mut packed, u64::from(*t));
        }
        field_bytes(&mut f, 2, &packed);
    }
    field_varint(&mut f, 3, kind);
    let mut packed = Vec::new();
    for g in geom {
        varint(&mut packed, u64::from(*g));
    }
    field_bytes(&mut f, 4, &packed);
    f
}

/// A layer whose features all carry `symbo` = `symbo` (key 0, value 0).
fn layer(name: &str, symbo: &str, features: &[Vec<u8>]) -> Vec<u8> {
    let mut l = Vec::new();
    field_varint(&mut l, 15, 2);
    field_bytes(&mut l, 1, name.as_bytes());
    for f in features {
        field_bytes(&mut l, 2, f);
    }
    field_bytes(&mut l, 3, b"symbo");
    let mut v = Vec::new();
    field_bytes(&mut v, 1, symbo.as_bytes());
    field_bytes(&mut l, 4, &v);
    field_varint(&mut l, 5, 4096);
    l
}

/// Tile of the synthetic Plan IGN: closed forest over the west half (`ocs_vegetation_surf`),
/// a lake in the south-east quarter (`hydro_surf`), a main road along the tile border
/// (`routier_route`), as the IGN layers and `symbo` values the styles filter on.
fn tile() -> Vec<u8> {
    let move_to = 1 | (1 << 3);
    let line_to = |n: u32| 2 | (n << 3);
    let close = 7 | (1 << 3);
    // Clockwise in tile coordinates (y down): exterior ring of a rectangle.
    let rect = |x0: i32, y0: i32, x1: i32, y1: i32| {
        vec![
            move_to,
            zigzag(x0),
            zigzag(y0),
            line_to(3),
            zigzag(x1 - x0),
            0,
            0,
            zigzag(y1 - y0),
            zigzag(x0 - x1),
            0,
            close,
        ]
    };
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
    let tags = [0, 0];
    let mut t = Vec::new();
    field_bytes(
        &mut t,
        3,
        &layer(
            "ocs_vegetation_surf",
            "ZONE_FORET_FERMEE_CONI",
            &[feature(3, &rect(-64, -64, 2048, 4160), &tags)],
        ),
    );
    field_bytes(
        &mut t,
        3,
        &layer(
            "hydro_surf",
            "SURFACE_D_EAU",
            &[feature(3, &rect(2600, 2600, 3600, 3600), &tags)],
        ),
    );
    field_bytes(
        &mut t,
        3,
        &layer(
            "routier_route",
            "PRINCIPALE_1",
            &[feature(2, &border, &tags)],
        ),
    );
    t
}

/// Empty glyph range (`glyphs { stacks { name, range } }`): MapLibre loads it and draws no
/// text, instead of failing on a missing file.
fn empty_glyphs(font: &str, range: &str) -> Vec<u8> {
    let mut stack = Vec::new();
    field_bytes(&mut stack, 1, font.as_bytes());
    field_bytes(&mut stack, 2, range.as_bytes());
    let mut out = Vec::new();
    field_bytes(&mut out, 1, &stack);
    out
}

/// Fonts of the styles (`text-font`), and the glyph ranges of the texts of the test pack.
const FONTS: [&str; 4] = [
    "Source Sans Pro Bold",
    "Source Sans Pro Semibold",
    "Source Sans Pro Regular",
    "Source Sans Pro Italic",
];
const GLYPH_RANGES: [&str; 2] = ["0-255", "8192-8447"];

/// Sprite with the refuge icons of the styles (`Refugegard`, `Refugenongard`, `Abri`):
/// 16-pixel squares, brown, dark brown and grey, at pixel ratio `ratio`.
fn sprite(ratio: u32) -> Res<(Vec<u8>, Value)> {
    let size = 16 * ratio;
    let names = ["Refugegard", "Refugenongard", "Abri"];
    let width = size * names.len() as u32;
    let palette = [
        [0, 0, 0, 0],
        [0x8a, 0x4b, 0x08, 255],
        [0x4a, 0x2a, 0x05, 255],
        [0x6b, 0x6b, 0x6b, 255],
    ];
    let mut indices = vec![0u8; (width * size) as usize];
    for (k, _) in names.iter().enumerate() {
        for y in 0..size {
            for x in 0..size {
                let border = x < ratio || y < ratio || x >= size - ratio || y >= size - ratio;
                indices[(y * width + k as u32 * size + x) as usize] =
                    if border { 0 } else { k as u8 + 1 };
            }
        }
    }
    let png = skimap_pack::png::encode_indexed(width, size, &palette, &indices)?;
    let mut index = serde_json::Map::new();
    for (k, n) in names.iter().enumerate() {
        index.insert(
            (*n).to_owned(),
            json!({"x": k as u32 * size, "y": 0, "width": size, "height": size, "pixelRatio": ratio}),
        );
    }
    Ok((png, Value::Object(index)))
}

fn gzip(data: &[u8]) -> Res<Vec<u8>> {
    let mut e = GzEncoder::new(Vec::new(), Gz::best());
    e.write_all(data)?;
    Ok(e.finish()?)
}

/// Writes `basemap/plan-ign.pmtiles`, its fonts and sprite, and a stand-in for the
/// basemap style, adds the basemap to the manifest, sets the version and lists the files
/// again.
fn add_basemap(pack: &Path, version: &str) -> Res<()> {
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

    let path = pack.join("basemap/plan-ign.pmtiles");
    let dir = pack.join("basemap/plan-ign");
    std::fs::create_dir_all(&dir)?;
    let f = std::fs::File::create(&path)?;
    let mut w = PmTilesWriter::new(TileType::Mvt)
        .precompressed(Compression::Gzip)
        .min_zoom(8)
        .max_zoom(14)
        .bounds(bbox.west, bbox.south, bbox.east, bbox.north)
        .center(c.lon, c.lat)
        .center_zoom(12)
        .metadata(
            json!({"vector_layers": [{"id": "ocs_vegetation_surf", "fields": {"symbo": "String"}},
                                     {"id": "hydro_surf", "fields": {"symbo": "String"}},
                                     {"id": "routier_route", "fields": {"symbo": "String"}}]})
            .to_string(),
        )
        .create(f)?;
    for id in &ids {
        w.add_tile(TileId::new(*id)?.into(), &data)?;
    }
    w.finalize()?;
    for font in FONTS {
        for range in GLYPH_RANGES {
            let p = dir.join(format!("fonts/{font}/{range}.pbf"));
            std::fs::create_dir_all(p.parent().ok_or("no parent")?)?;
            std::fs::write(p, empty_glyphs(font, range))?;
        }
    }
    std::fs::create_dir_all(dir.join("sprite"))?;
    for (ratio, suffix) in [(1, ""), (2, "@2x")] {
        let (png, index) = sprite(ratio)?;
        std::fs::write(dir.join(format!("sprite/PlanIgn{suffix}.png")), png)?;
        write_json(&dir.join(format!("sprite/PlanIgn{suffix}.json")), &index)?;
    }
    // The manifest requires a basemap style; the app draws with the styles of `styles/`.
    let style = json!({
        "version": 8,
        "name": "skimap test basemap",
        "sources": {"ign": {"type": "vector", "url": "pmtiles://basemap/plan-ign.pmtiles"}},
        "layers": [{"id": "background", "type": "background", "paint": {"background-color": "#ffffff"}}]
    });
    write_json(&dir.join("style.json"), &style)?;

    let mpath = pack.join(MANIFEST_FILE);
    let mut m: Manifest = serde_json::from_slice(&std::fs::read(&mpath)?)?;
    m.site.id = "test".to_owned();
    m.site.name = "Pack de test".to_owned();
    m.pack_version = version.to_owned();
    let bytes = list_files(pack)?
        .iter()
        .filter(|f| f.path.starts_with("basemap/"))
        .map(|f| f.bytes)
        .sum();
    m.basemaps = vec![Basemap {
        id: "plan-ign".to_owned(),
        title: "Plan IGN de test".to_owned(),
        tiles: "basemap/plan-ign.pmtiles".to_owned(),
        style: "basemap/plan-ign/style.json".to_owned(),
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
    add_fee_zones(pack, &mut m)?;
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
        let line = write_test_pack(&out, 1).unwrap();
        assert!(line.contains("\"pistes\""), "{line}");
        let p = skimap_pack::Pack::open(&out).unwrap();
        assert!(p.verify().unwrap().is_empty());
        assert_eq!(p.manifest().pack_version, VERSIONS[0]);
        let t = p.tiles("plan-ign").unwrap();
        assert_eq!(t.zoom_range(), (8, 14));
        let (tx, ty, _, _) =
            skimap_pack::tiles::tile_pixel(skimap_common::geom::LonLat::new(6.7746, 44.8586), 14);
        assert_eq!(t.tile(14, tx, ty).unwrap().unwrap(), tile());
        assert!(!tmp.path().join("packs/.test-pack-work").exists());
        let v2 = tmp.path().join("packs/test-v2");
        write_test_pack(&v2, 2).unwrap();
        let p2 = skimap_pack::Pack::open(&v2).unwrap();
        assert_eq!(p2.manifest().pack_version, VERSIONS[1]);
        assert_ne!(
            std::fs::read(out.join("layers/status.geojson")).unwrap(),
            std::fs::read(v2.join("layers/status.geojson")).unwrap()
        );
        let voice = tmp.path().join("packs/test-voice-en");
        write_voice_pack(&voice, "en").unwrap();
        let pv = skimap_pack::Pack::open(&voice).unwrap();
        assert_eq!(pv.manifest().site.id, "test-voice-en");
        assert_eq!(pv.manifest().files.len(), 3);
    }
}
