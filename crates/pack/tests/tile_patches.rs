//! PMTiles sent tile by tile (#160): the patch of a few changed tiles rebuilds the new
//! archive byte for byte, travels in a bundle of format 3 much smaller than the file, and
//! a phone whose old file is not the expected one refuses it without touching its pack.

#![allow(
    clippy::disallowed_methods,
    reason = "tests write their inputs directly"
)]
use std::collections::HashMap;
use std::fs;
use std::path::Path;

use pmtiles::{PmTilesWriter, TileCoord, TileType};
use serde_json::json;
use skimap_pack::manifest::Manifest;
use skimap_pack::patch;
use skimap_pack::update::{
    BUNDLE_MAGIC, BUNDLE_MAGIC_V3, PATCH_FAILED, TilePatches, UPDATE_FORMAT_VERSION, Update,
    apply_bundle, read_bundle, sha256_hex, tile_patches, write_bundle, write_bundle_patched,
};

/// Bytes of a tile that do not compress (a tile of a real archive is compressed already).
fn noise(seed: u64, len: usize) -> Vec<u8> {
    let mut x = seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1;
    (0..len)
        .map(|_| {
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            x as u8
        })
        .collect()
}

/// An archive of zooms 0 to 12: one tile per zoom up to 11, then a 90 x 90 block of zoom
/// 12 (8 100 tiles, enough for leaf directories), a column of identical "sea" tiles
/// (stored once). `edit` changes the contents of a tile id (`None`: absent).
fn archive(edit: &dyn Fn(u8, u32, u32, Vec<u8>) -> Option<Vec<u8>>) -> Vec<u8> {
    let mut tiles = Vec::new();
    for z in 0..12u8 {
        tiles.push((z, 0u32, 0u32));
    }
    for x in 0..90u32 {
        for y in 0..90u32 {
            tiles.push((12, 2000 + x, 1400 + y));
        }
    }
    let mut out = Vec::new();
    {
        let mut w = PmTilesWriter::new(TileType::Mvt)
            .min_zoom(0)
            .max_zoom(12)
            .metadata(r#"{"name":"test"}"#)
            .create(std::io::Cursor::new(&mut out))
            .unwrap();
        for (z, x, y) in tiles {
            let base = if y == 1400 {
                noise(1, 300)
            } else {
                noise(
                    u64::from(z) << 40 | u64::from(x) << 20 | u64::from(y),
                    200 + (x as usize % 50),
                )
            };
            if let Some(b) = edit(z, x, y, base) {
                w.add_tile(TileCoord::new(z, x, y).unwrap(), &b).unwrap();
            }
        }
        w.finalize().unwrap();
    }
    out
}

fn old_archive() -> Vec<u8> {
    archive(&|_, _, _, b| Some(b))
}

/// Three tiles changed (one at zoom 5, two at zoom 12), one removed, two added (zoom 12,
/// outside the old block's rows... inside its columns: new ids).
fn new_archive() -> Vec<u8> {
    archive(&|z, x, y, b| match (z, x, y) {
        (5, 0, 0) | (12, 2010, 1410) | (12, 2050, 1480) => Some(noise(99 + u64::from(x), 260)),
        (12, 2020, 1420) => None,
        _ => Some(b),
    })
}

#[test]
fn patch_rebuilds_the_archive_byte_for_byte() {
    let (old, new) = (old_archive(), new_archive());
    assert_ne!(old, new);
    let (p, d) = patch::diff(&old, &new).unwrap();
    assert_eq!(
        (d.tiles_changed, d.tiles_removed, d.tiles_added),
        (3, 1, 0),
        "{d:?}"
    );
    assert_eq!(d.by_zoom[&5].changed, 1);
    assert_eq!(d.by_zoom[&12].changed, 2);
    assert_eq!(d.copied_bytes + d.sent_bytes, new.len() as u64);
    // A few tiles and the directories, not the 1.8 MB archive.
    assert!(
        p.len() * 20 < new.len(),
        "patch {} of {}",
        p.len(),
        new.len()
    );
    let mut out = Vec::new();
    let n = patch::apply(
        &mut p.as_slice(),
        &mut std::io::Cursor::new(&old),
        old.len() as u64,
        &mut out,
        new.len() as u64,
    )
    .unwrap();
    assert_eq!(n, new.len() as u64);
    assert_eq!(out, new);

    // Identical archives: copies only.
    let (same, d) = patch::diff(&old, &old).unwrap();
    assert_eq!(d.sent_bytes, 0);
    assert!(same.len() < 32, "{}", same.len());

    // Refusals: another old file of the same size gives other bytes (the update checks
    // the SHA-256), a wrong length, a cut patch, trailing bytes, not a patch.
    let apply = |p: &[u8], old: &[u8], len: u64| {
        let mut out = Vec::new();
        patch::apply(
            &mut &p[..],
            &mut std::io::Cursor::new(old),
            old.len() as u64,
            &mut out,
            len,
        )
        .map(|_| out)
    };
    let mut other = old.clone();
    other[200_000] ^= 1;
    assert_ne!(apply(&p, &other, new.len() as u64).unwrap(), new);
    assert!(apply(&p, &old, new.len() as u64 + 1).is_err());
    assert!(apply(&p[..p.len() - 1], &old, new.len() as u64).is_err());
    let mut longer = p.clone();
    longer.push(0);
    assert!(apply(&longer, &old, new.len() as u64).is_err());
    assert!(apply(b"SKMPBND2xxxx", &old, new.len() as u64).is_err());
    assert!(
        apply(&p, &old[..1000], new.len() as u64).is_err(),
        "copy beyond the old file"
    );
    // Not PMTiles: no patch (the file travels whole).
    assert!(patch::diff(&[7u8; 5000], &new).is_err());
}

/// Writes a pack with these files, returns its manifest bytes.
fn make_pack(dir: &Path, version: &str, files: &[(&str, &[u8])]) -> Vec<u8> {
    let _ = fs::remove_dir_all(dir);
    let mut entries = Vec::new();
    let mut total = 0u64;
    for (path, body) in files {
        let p = dir.join(path);
        fs::create_dir_all(p.parent().unwrap()).unwrap();
        fs::write(&p, body).unwrap();
        entries.push(json!({"path": path, "bytes": body.len(), "sha256": sha256_hex(body)}));
        total += body.len() as u64;
    }
    let m = json!({
        "format": "skimap-pack", "format_version": 1, "pack_version": version,
        "generator": "test", "generated_at": "2026-11-15T00:00:00Z",
        "site": {"id": "cervieres", "name": "Cervières", "department": "05",
                 "bbox": [6.66, 44.77, 6.86, 44.92], "center": [6.77, 44.85]},
        "valid_for": {"until": "2026-11-16", "until_layer": null, "note": "n"},
        "personal_sources": false, "basemaps": [], "layers": [],
                "attributions": [], "files": entries, "total_bytes": total
    });
    let bytes = serde_json::to_vec_pretty(&m).unwrap();
    fs::write(dir.join("manifest.json"), &bytes).unwrap();
    bytes
}

fn manifest(bytes: &[u8]) -> Manifest {
    serde_json::from_slice(bytes).unwrap()
}

fn read_tree(dir: &Path) -> Vec<(String, Vec<u8>)> {
    let mut out = Vec::new();
    fn walk(root: &Path, d: &Path, out: &mut Vec<(String, Vec<u8>)>) {
        for e in fs::read_dir(d).unwrap() {
            let p = e.unwrap().path();
            if p.is_dir() {
                walk(root, &p, out);
            } else {
                let rel = p.strip_prefix(root).unwrap().to_string_lossy().into_owned();
                out.push((rel, fs::read(&p).unwrap()));
            }
        }
    }
    walk(dir, dir, &mut out);
    out.sort();
    out
}

/// Every content of the server's packs by SHA-256 (the server's blob store).
fn store(packs: &[&Path]) -> HashMap<String, Vec<u8>> {
    let mut out = HashMap::new();
    for p in packs {
        for (_, b) in read_tree(p) {
            out.insert(sha256_hex(&b), b);
        }
    }
    out
}

fn write(
    path: &Path,
    update: &Update,
    patches: &TilePatches,
    blobs: &HashMap<String, Vec<u8>>,
) -> u64 {
    let mut f = fs::File::create(path).unwrap();
    write_bundle_patched(&mut f, update, patches, |sha| {
        Ok(std::io::Cursor::new(blobs[sha].clone()))
    })
    .unwrap()
}

#[test]
fn bundle_of_format_3_sends_the_changed_tiles_only() {
    let t = tempfile::tempdir().unwrap();
    let p = |n: &str| t.path().join(n);
    let (a, b) = (old_archive(), new_archive());
    let snow1 = archive(&|z, x, y, b| (z == 12 && x < 2010).then(|| noise(u64::from(y), b.len())));
    let m1 = make_pack(
        &p("server/v1"),
        "v1",
        &[
            ("basemap/osm.pmtiles", &a),
            ("layers/snow.pmtiles", &snow1),
            ("layers/bera.json", b"{\"risk\":2}"),
        ],
    );
    let m2 = make_pack(
        &p("server/v2"),
        "v2",
        &[
            ("basemap/osm.pmtiles", &b),
            // Not a readable archive: travels whole.
            ("layers/snow.pmtiles", &[3u8; 4000]),
            ("layers/bera.json", b"{\"risk\":3}"),
        ],
    );
    let blobs = store(&[&p("server/v1"), &p("server/v2")]);
    let (v1, v2) = (manifest(&m1), manifest(&m2));
    let u = Update::between(Some(&v1), &v2, &m2);
    let patches =
        tile_patches(&u, &v1, |sha| Ok(std::io::Cursor::new(blobs[sha].clone()))).unwrap();
    assert_eq!(patches.0.len(), 1, "the basemap only");
    let (pinfo, _) = &patches.0[&sha256_hex(&b)];
    assert_eq!(pinfo.from_sha256, sha256_hex(&a));
    assert_eq!((pinfo.tiles_changed, pinfo.tiles_removed), (3, 1));

    // The bundle with whole files is the format 2 bundle of before: an app of format 2
    // reads it as it did.
    let whole = write(&p("whole.skb"), &u, &TilePatches::default(), &blobs);
    let mut f = fs::File::create(p("whole2.skb")).unwrap();
    let again = write_bundle(&mut f, &u, |sha| {
        Ok(std::io::Cursor::new(blobs[sha].clone()))
    })
    .unwrap();
    assert_eq!(again, whole);
    assert_eq!(
        fs::read(p("whole.skb")).unwrap(),
        fs::read(p("whole2.skb")).unwrap()
    );
    assert_eq!(&fs::read(p("whole.skb")).unwrap()[..8], BUNDLE_MAGIC);
    let (h, _) = read_bundle(&p("whole.skb")).unwrap();
    assert_eq!(h.format_version, 2);
    assert!(h.fetch.iter().all(|b| b.patch.is_none()));

    let patched = write(&p("patched.skb"), &u, &patches, &blobs);
    assert_eq!(&fs::read(p("patched.skb")).unwrap()[..8], BUNDLE_MAGIC_V3);
    let (h, _) = read_bundle(&p("patched.skb")).unwrap();
    assert_eq!(h.format_version, UPDATE_FORMAT_VERSION);
    assert!(
        patched * 10 < whole,
        "patched bundle {patched} bytes, whole {whole}"
    );

    // The phone at v1 installs it: the pack is the server's v2 byte for byte.
    let phone = p("phone/cervieres");
    make_pack(
        &phone,
        "v1",
        &[
            ("basemap/osm.pmtiles", &a),
            ("layers/snow.pmtiles", &snow1),
            ("layers/bera.json", b"{\"risk\":2}"),
        ],
    );
    assert_eq!(fs::read(phone.join("manifest.json")).unwrap(), m1);
    let r = apply_bundle(&phone, &p("patched.skb")).unwrap();
    assert_eq!(r.version, "v2");
    assert_eq!(read_tree(&phone), read_tree(&p("server/v2")));

    // A phone whose old basemap differs (same size, one byte): the rebuilt file does not
    // match the manifest, refused, pack untouched; the bundle with whole files installs.
    make_pack(
        &phone,
        "v1",
        &[
            ("basemap/osm.pmtiles", &a),
            ("layers/snow.pmtiles", &snow1),
            ("layers/bera.json", b"{\"risk\":2}"),
        ],
    );
    fs::write(phone.join("manifest.json"), &m1).unwrap();
    let mut bad = a.clone();
    bad[300_000] ^= 0xff;
    fs::write(phone.join("basemap/osm.pmtiles"), &bad).unwrap();
    let before = read_tree(&phone);
    let e = apply_bundle(&phone, &p("patched.skb"))
        .unwrap_err()
        .to_string();
    assert!(e.starts_with(&format!("pack: {PATCH_FAILED}")), "{e}");
    assert_eq!(read_tree(&phone), before);
    assert!(!skimap_pack::update::staging_dir(&phone).exists());
    apply_bundle(&phone, &p("whole.skb")).unwrap();
    assert_eq!(read_tree(&phone), read_tree(&p("server/v2")));

    // A phone without the old basemap (a pack of another version that has the rest):
    // refused before anything is written.
    make_pack(
        &phone,
        "v0",
        &[
            ("basemap/osm.pmtiles", &[1u8; 10]),
            ("layers/snow.pmtiles", &snow1),
            ("layers/bera.json", b"{\"risk\":2}"),
        ],
    );
    let before = read_tree(&phone);
    let e = apply_bundle(&phone, &p("patched.skb"))
        .unwrap_err()
        .to_string();
    assert!(e.starts_with(&format!("pack: {PATCH_FAILED}")), "{e}");
    assert_eq!(read_tree(&phone), before);
}
