//! Fuzz targets of everything the phone reads from a downloaded file (Q3, #103).
//!
//! Each target is a function here, called by `fuzz_targets/<name>.rs` under libFuzzer and
//! by `tests/corpus.rs` on the versioned corpus (stable toolchain, CI). A target must
//! never panic, abort or allocate without bound on any input: every failure is an `Err`.
//!
//! ```sh
//! cd fuzz
//! cargo +nightly fuzz run skr1 -- -max_total_time=600   # campaign on one target
//! cargo test --release                                  # replay the corpus (stable)
//! cargo test --release -- --ignored make_seeds          # regenerate the seeds
//! ```
//!
//! Multi-file inputs (`pack`, `offpiste_grid`) are small archives ([`parse_archive`]):
//! `name_len: u8, name, data_len: u32 LE, data`, repeated.

use std::io::Read;
use std::path::{Path, PathBuf};

use skimap_common::geom::LonLat;

/// Largest decompressed size accepted from a gzip layer (a "zip bomb" is an error).
pub const MAX_INFLATE: u64 = 64 << 20;

/// A fuzz target.
pub type Target = fn(&[u8]);

/// Every target, by name.
pub const TARGETS: &[(&str, Target)] = &[
    ("skr1", skr1),
    ("zone_classes", zone_classes),
    ("piste_graph", piste_graph),
    ("offpiste_grid", offpiste_grid),
    ("pack", pack),
    ("pack_update", pack_update),
    ("pmtiles_vendor", pmtiles_vendor),
    ("tile_archive", tile_archive),
    ("png", png),
    ("geotiff", geotiff),
    ("bera", bera),
];

// ---------------------------------------------------------------------------------------
// Archives of files
// ---------------------------------------------------------------------------------------

/// Whether an archive entry name is a safe relative path (ASCII letters, digits, `._-/`,
/// no empty, `.` or `..` component).
fn safe_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 96
        && name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"._-/".contains(&b))
        && name
            .split('/')
            .all(|c| !c.is_empty() && c != "." && c != "..")
}

/// Files of an archive (entries with an unsafe name are skipped; a truncated last entry
/// keeps what is there).
pub fn parse_archive(mut data: &[u8]) -> Vec<(String, Vec<u8>)> {
    let mut out = Vec::new();
    while let Some((&n, rest)) = data.split_first() {
        let n = usize::from(n);
        if rest.len() < n + 4 {
            break;
        }
        let name = String::from_utf8_lossy(&rest[..n]).into_owned();
        let len = u32::from_le_bytes([rest[n], rest[n + 1], rest[n + 2], rest[n + 3]]) as usize;
        let body = &rest[n + 4..];
        let len = len.min(body.len());
        if safe_name(&name) {
            out.push((name, body[..len].to_vec()));
        }
        data = &body[len..];
    }
    out
}

/// Archive of files, the inverse of [`parse_archive`].
pub fn encode_archive(files: &[(&str, &[u8])]) -> Vec<u8> {
    let mut out = Vec::new();
    for (name, data) in files {
        out.push(u8::try_from(name.len()).expect("short name"));
        out.extend_from_slice(name.as_bytes());
        out.extend_from_slice(&u32::try_from(data.len()).expect("small file").to_le_bytes());
        out.extend_from_slice(data);
    }
    out
}

/// An empty scratch directory private to this process (in `/dev/shm` when there is one).
fn scratch(tag: &str) -> PathBuf {
    let base = if Path::new("/dev/shm").is_dir() {
        PathBuf::from("/dev/shm")
    } else {
        std::env::temp_dir()
    };
    let dir = base.join(format!(
        "skimap-fuzz-{tag}-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("scratch directory");
    dir
}

fn write_files(dir: &Path, files: &[(String, Vec<u8>)]) {
    for (name, data) in files {
        let p = dir.join(name);
        if let Some(parent) = p.parent()
            && std::fs::create_dir_all(parent).is_err()
        {
            continue;
        }
        // A name may collide with a directory created for another entry.
        let _ = std::fs::write(&p, data);
    }
}

/// Inflates a gzip stream, at most [`MAX_INFLATE`] bytes.
fn gunzip(data: &[u8]) -> Option<Vec<u8>> {
    let mut out = Vec::new();
    flate2::read::GzDecoder::new(data)
        .take(MAX_INFLATE + 1)
        .read_to_end(&mut out)
        .ok()?;
    (out.len() as u64 <= MAX_INFLATE).then_some(out)
}

// ---------------------------------------------------------------------------------------
// Targets
// ---------------------------------------------------------------------------------------

/// SKR1 rasters (`u8` and `u16`, raw and row-delta, plain or gzipped as in the pack):
/// decoding never panics, and what decodes re-encodes to the same grid in both layouts.
pub fn skr1(data: &[u8]) {
    use skimap_core::offpiste::{Encoding, decode_raster, encode_raster_with};
    fn check<T: skimap_core::offpiste::RasterValue + std::fmt::Debug>(bytes: &[u8]) {
        if let Ok(g) = decode_raster::<T>(bytes) {
            let nodata = *g
                .nodata()
                .expect("decoded rasters carry their no-data value");
            for enc in [Encoding::Raw, Encoding::RowDelta] {
                let b = encode_raster_with(&g, nodata, enc).expect("a decoded raster encodes");
                let back = decode_raster::<T>(&b).expect("an encoded raster decodes");
                assert!(back == g, "SKR1 round trip changed the raster");
            }
        }
    }
    check::<u8>(data);
    check::<u16>(data);
    if let Some(raw) = gunzip(data) {
        check::<u8>(&raw);
        check::<u16>(&raw);
    }
}

/// Zone class table: parsing never panics; a parsed table re-encodes and parses back to
/// the same classes.
pub fn zone_classes(data: &[u8]) {
    use skimap_core::offpiste::{encode_zone_classes, parse_zone_classes};
    let Ok(text) = std::str::from_utf8(data) else {
        return;
    };
    let Ok(classes) = parse_zone_classes(text) else {
        return;
    };
    if let Ok(again) = encode_zone_classes(&classes) {
        let back = parse_zone_classes(&again).expect("an encoded table parses");
        assert_eq!(back.len(), classes.len());
        for (a, b) in classes.iter().zip(&back) {
            assert_eq!(a.zone_ids, b.zone_ids);
            assert_eq!(a.cells, b.cells);
        }
    }
}

/// Trail graph (`piste-graph.json`): loading never panics, nor does what the phone does
/// with it: lengths, stations, snapping, components, fastest route, map matching.
pub fn piste_graph(data: &[u8]) {
    use skimap_core::eta::EtaModel;
    use skimap_core::graph::PisteGraph;
    use skimap_core::matching::{Fix, MatchParams, Matcher};
    use skimap_core::routing::{Router, Style};
    let Ok(text) = std::str::from_utf8(data) else {
        return;
    };
    let Ok(g) = PisteGraph::from_json(text) else {
        return;
    };
    let ne = g.edges().len();
    let _ = g.total_length_m();
    let _ = g.components();
    for e in 0..ne.min(32) {
        let l = g.length_m(e);
        let _ = g.stations(e, 0.0, l);
        let _ = g.stations(e, l, l * 0.5);
        let _ = g.sub_polyline(e, l * 0.25, l * 0.75);
        let _ = g.point_at(e, l * 0.5);
    }
    let first = g.nodes()[0].pos;
    let last = g.nodes()[g.nodes().len() - 1].pos;
    let _ = g.snap(first, 1e4);
    let _ = g.candidates(last, 100.0, 8);
    if ne == 0 {
        return;
    }
    for style in [Style::Classic, Style::Skating] {
        let r = Router::new(&g, EtaModel::new(style));
        if let Ok(route) = r.route(first, last, 1e5) {
            let _ = route.polyline(&g);
            let _ = route.gain_loss(&g);
            let _ = route.profile_coverage(&g);
            let _ = route.names(&g);
            let _ = r.model().time_s(&route.pieces(&g));
        }
    }
    let mut m = Matcher::new(&g, MatchParams::default());
    for (k, p) in [first, last, first].into_iter().enumerate() {
        let _ = m.push(Fix {
            t_s: k as f64,
            pos: p,
        });
    }
    let _ = m.decode();
}

/// Off-piste grid of a pack (four SKR1 rasters, the class table, optional snow and
/// exposure): assembling never panics, nor do snapping and a route across it.
pub fn offpiste_grid(data: &[u8]) {
    use skimap_core::offpiste::{
        CostParams, ExposureFactors, Heuristic, OffpisteGrid, Router, SnowGrid, Window, ZonePolicy,
        decode_raster, parse_zone_classes,
    };
    let files = parse_archive(data);
    let get = |n: &str| files.iter().find(|f| f.0 == n).map(|f| f.1.as_slice());
    let (Some(e), Some(s), Some(z), Some(p), Some(c)) = (
        get("elevation"),
        get("slope"),
        get("zones"),
        get("pistes"),
        get("classes"),
    ) else {
        return;
    };
    let (Ok(e), Ok(s), Ok(z), Ok(p)) = (
        decode_raster::<u16>(e),
        decode_raster::<u8>(s),
        decode_raster::<u8>(z),
        decode_raster::<u8>(p),
    ) else {
        return;
    };
    let Ok(classes) = std::str::from_utf8(c)
        .map_err(|_| ())
        .and_then(|t| parse_zone_classes(t).map_err(|_| ()))
    else {
        return;
    };
    let n = classes.len();
    let Ok(grid) = OffpisteGrid::new(e, s, z, p, classes) else {
        return;
    };
    let snow = get("snow")
        .and_then(|b| decode_raster::<u8>(b).ok())
        .map(|grid| SnowGrid { grid });
    let overlays: Vec<_> = get("exposure")
        .and_then(|b| decode_raster::<u8>(b).ok())
        .map(|g| ExposureFactors::default().overlay(g))
        .into_iter()
        .collect();
    let at = "2026-02-15T10:00:00Z".parse().expect("date");
    let policy = ZonePolicy::open(n, at);
    let params = CostParams::default();
    let r = Router {
        grid: &grid,
        snow: snow.as_ref(),
        overlays: &overlays,
        policy: &policy,
        params: &params,
    };
    let (w, h) = (grid.width(), grid.height());
    if w == 0 || h == 0 {
        return;
    }
    let win = Window::full(&grid);
    if win.cells() <= 1 << 20 {
        let _ = r.route_cells(win, (0, 0), (w - 1, h - 1), Heuristic::AStar);
        let _ = r.explore(win, (w / 2, h / 2));
    }
    if let (Ok(a), Ok(b)) = (grid.center(0, 0), grid.center(w - 1, h - 1)) {
        let _ = r.route(a, b);
        let _ = grid.cell_of(a);
    }
    let _ = r.cell_factor(w - 1, h - 1);
}

/// A whole pack directory: opening, verifying, reading every layer as the phone does
/// (JSON, GeoJSON, tiles and raster values, gzipped SKR1, zone classes), `pack.meta.json`
/// and the voice manifest.
pub fn pack(data: &[u8]) {
    use skimap_pack::Pack;
    let files = parse_archive(data);
    let dir = scratch("pack");
    write_files(&dir, &files);
    if let Ok(b) = std::fs::read(dir.join("pack.meta.json")) {
        let _ = serde_json::from_slice::<skimap_common::meta::LayerMeta>(&b);
    }
    let Ok(p) = Pack::open(&dir) else {
        return;
    };
    let _ = p.verify();
    let m = p.manifest().clone();
    for l in m.layers.iter().take(64) {
        let _ = p.layer(&l.id);
        match l.format.as_str() {
            "json" => {
                let _ = p.read_json(&l.id);
            }
            "geojson" => {
                let _ = p.read_geojson(&l.id);
            }
            "pmtiles" => {
                if let Ok(t) = p.tiles(&l.id) {
                    read_archive(&t);
                }
                let [w, s, e, n] = m.site.bbox;
                for q in [
                    LonLat::new(m.site.center[0], m.site.center[1]),
                    LonLat::new((w + e) / 2.0, (s + n) / 2.0),
                ] {
                    let _ = p.raster_value_at(&l.id, q);
                }
            }
            "skr+gzip" => {
                if let Ok(b) = p.read_file(&l.file)
                    && let Some(raw) = gunzip(&b)
                {
                    let _ = skimap_core::offpiste::decode_raster::<u8>(&raw);
                    let _ = skimap_core::offpiste::decode_raster::<u16>(&raw);
                }
            }
            "tsv" => {
                if let Ok(b) = p.read_file(&l.file)
                    && let Ok(t) = std::str::from_utf8(&b)
                {
                    let _ = skimap_core::offpiste::parse_zone_classes(t);
                }
            }
            _ => {
                let _ = p.read_file(&l.file);
            }
        }
    }
    for b in m.basemaps.iter().take(8) {
        if let Ok(t) = p.tiles(&b.id) {
            read_archive(&t);
        }
        let _ = p.read_file(&b.style);
    }
    if let Some(rel) = &m.voice.manifest
        && let Ok(path) = p.path(rel)
        && let Some(parent) = path.parent()
        && path.file_name().is_some_and(|n| n == "manifest.json")
        && let Ok(v) = skimap_voice::pack::VoiceManifest::read(parent)
    {
        let _ = v.get("fixed.reassurance");
    }
}

/// A pack update as the phone receives it (K2, S0): the installed pack (`old/<path>`
/// entries), a downloaded bundle (`bundle`), the site index (`index`) and the sync state
/// (`state`). Reading and installing the bundle never panics, and never writes outside
/// the pack directory and its two siblings (`.<site>.new`, `.<site>.old`); the index and
/// the state never make the sync plan panic.
pub fn pack_update(data: &[u8]) {
    use skimap_pack::sync::{SiteIndex, SyncState, plan, plan_full, read_state, state_path};
    use skimap_pack::update::{apply_bundle, read_bundle};
    let files = parse_archive(data);
    let root = scratch("update");
    let area = root.join("area");
    let pack_dir = area.join("cervieres");
    std::fs::create_dir_all(&pack_dir).expect("pack directory");
    let old: Vec<(String, Vec<u8>)> = files
        .iter()
        .filter_map(|(n, b)| Some((n.strip_prefix("old/")?.to_owned(), b.clone())))
        .collect();
    write_files(&pack_dir, &old);
    let get = |n: &str| files.iter().find(|f| f.0 == n).map(|f| f.1.as_slice());
    if let Some(st) = get("state") {
        std::fs::write(state_path(&pack_dir), st).expect("sync state file");
    }
    if let Some(b) = get("bundle") {
        let bundle = root.join("bundle.skb");
        std::fs::write(&bundle, b).expect("bundle file");
        let _ = read_bundle(&bundle);
        if apply_bundle(&pack_dir, &bundle).is_ok()
            && let Ok(p) = skimap_pack::Pack::open(&pack_dir)
        {
            let _ = p.verify();
        }
        // Nothing outside the pack directory and its siblings.
        for e in std::fs::read_dir(&root).expect("scratch").flatten() {
            let n = e.file_name();
            assert!(
                n == "area" || n == "bundle.skb",
                "written outside the pack: {n:?}"
            );
        }
        for e in std::fs::read_dir(&area).expect("area").flatten() {
            let n = e.file_name();
            assert!(
                n == "cervieres"
                    || n == ".cervieres.new"
                    || n == ".cervieres.old"
                    || n == ".cervieres.sync.json",
                "written outside the pack: {n:?}"
            );
        }
    }
    let state: Option<SyncState> = get("state").and_then(|b| serde_json::from_slice(b).ok());
    if let Some(i) = get("index")
        && let Ok(index) = serde_json::from_slice::<SiteIndex>(i)
    {
        let _ = plan(&index, state.as_ref());
        let _ = plan_full(&index);
    }
    let _ = read_state(&pack_dir);
}

/// Exercises an opened tile archive: header, metadata, the tiles at the centre of its
/// bounds at every zoom (at most 8), palette values there.
fn read_archive(t: &skimap_pack::TileArchive) {
    let (z0, z1) = t.zoom_range();
    let [w, s, e, n] = t.bounds();
    let _ = t.tile_type();
    let _ = t.addressed_tiles();
    let _ = t.metadata();
    let _ = t.tile_compression();
    let c = LonLat::new((w + e) / 2.0, (s + n) / 2.0);
    for z in z0..=z1.min(z0.saturating_add(7)) {
        let (x, y) = skimap_common::crs::tile_for(c, z);
        let _ = t.tile(z, x, y);
        let _ = t.raw_tile(z, x, y);
        let _ = t.palette_index_at(c, z, Some(0));
    }
}

/// The patched vendor PMTiles reader on an archive in memory: header, metadata, every
/// directory entry (at most 4096) and their tiles.
pub fn pmtiles_vendor(data: &[u8]) {
    use pmtiles::{PmTilesReader, TileCoord, TileId};
    let Ok(r) = PmTilesReader::try_from_source(bytes::Bytes::copy_from_slice(data)) else {
        return;
    };
    let h = r.get_header();
    let _ = h.n_addressed_tiles();
    let _ = r.get_metadata();
    let mut ids = Vec::new();
    for e in r.entries().take(4096) {
        match e {
            Ok(e) => ids.extend(e.iter_coords().take(4)),
            Err(_) => break,
        }
    }
    for id in ids.into_iter().take(64) {
        let _ = r.get_tile(id);
        let _ = r.get_tile_decompressed(id);
    }
    for z in h.min_zoom..=h.max_zoom.min(h.min_zoom.saturating_add(4)) {
        if let Ok(c) = TileCoord::new(z, 0, 0) {
            let _ = r.get_tile_decompressed(c);
        }
    }
    if data.len() >= 8 {
        let raw = u64::from_le_bytes(data[data.len() - 8..].try_into().expect("8 bytes"));
        if let Ok(id) = TileId::new(raw) {
            let _ = r.get_tile(id);
        }
    }
}

/// The pack's `TileArchive` (positioned reads on a file) on the input written to disk.
pub fn tile_archive(data: &[u8]) {
    let dir = scratch("tiles");
    let p = dir.join("t.pmtiles");
    std::fs::write(&p, data).expect("scratch file");
    if let Ok(t) = skimap_pack::TileArchive::open(&p) {
        read_archive(&t);
    }
}

/// Palette PNG tiles of the raster layers.
pub fn png(data: &[u8]) {
    if let Ok(img) = skimap_pack::png::decode_indexed(data) {
        let _ = img.index_at(0, 0);
        let _ = img.index_at(img.width.saturating_sub(1), img.height.saturating_sub(1));
        assert_eq!(
            img.indices.len() as u64,
            u64::from(img.width) * u64::from(img.height)
        );
    }
}

/// GeoTIFF readers of `skimap_common::geotiff`.
pub fn geotiff(data: &[u8]) {
    use skimap_common::geotiff::{decode_any, decode_f32, decode_u8};
    let _ = decode_f32(data);
    let _ = decode_u8(data);
    let _ = decode_any(data);
}

/// BERA XML bulletins.
pub fn bera(data: &[u8]) {
    let _ = skimap_meteo::bera::looks_like_bera(data);
    if let Ok(text) = std::str::from_utf8(data) {
        if let Ok(bs) = skimap_meteo::bera::parse_xml(text) {
            for b in &bs {
                let _ = skimap_meteo::bera::parse_massif(text, b.massif_id);
            }
        }
        let _ = skimap_meteo::xml::parse(text);
    }
}
