//! Pack updates without network: first install, daily delta, reuse by content, bad blob,
//! interrupted swap. The phone side is `update::apply`; the server side computes
//! `Update::between` the same way.

use std::fs;
use std::path::Path;

use serde_json::json;
use skimap_pack::Pack;
use skimap_pack::manifest::Manifest;
use skimap_pack::update::{
    Change, MISSING_CONTENTS, Update, apply, apply_bundle, previous_dir, read_bundle, recover,
    sha256_hex, staging_dir, write_bundle,
};

/// Writes a pack with these files and a `bera` + `pistes` layer, returns its manifest bytes.
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
    let layer = |id: &str, file: &str, until: &str| {
        json!({"id": id, "title": id, "kind": "data", "format": "json", "file": file,
               "origin": id, "generated_at": format!("{version}-gen"), "valid_from": null,
               "valid_until": until, "validity": "un jour", "sources": [],
               "personal_use_only": false, "bytes": 1, "visible": false})
    };
    let m = json!({
        "format": "skimap-pack", "format_version": 1, "pack_version": version,
        "generator": "test", "generated_at": "2026-11-15T00:00:00Z",
        "site": {"id": "cervieres", "name": "Cervières", "department": "05",
                 "bbox": [6.66, 44.77, 6.86, 44.92], "center": [6.77, 44.85]},
        "valid_for": {"until": "2026-11-16", "until_layer": "bera", "note": "n"},
        "personal_sources": false, "basemaps": [],
        "layers": [layer("pistes", "layers/pistes.json", "2026-12-31"),
                   layer("bera", "layers/bera.json", "2026-11-16")],
        "voice": {"status": "absent", "clips": 0, "bytes": 0, "note": "n"},
        "attributions": [], "files": entries, "total_bytes": total
    });
    let bytes = serde_json::to_vec_pretty(&m).unwrap();
    fs::write(dir.join("manifest.json"), &bytes).unwrap();
    bytes
}

fn manifest(bytes: &[u8]) -> Manifest {
    serde_json::from_slice(bytes).unwrap()
}

/// What the phone does after downloading: puts each listed blob (and the manifest) under
/// its SHA-256, taking them from the server's copy of the new pack.
fn download(update: &Update, server_pack: &Path, blobs: &Path) {
    fs::create_dir_all(blobs).unwrap();
    for b in &update.fetch {
        fs::copy(server_pack.join(&b.paths[0]), blobs.join(&b.sha256)).unwrap();
    }
    fs::copy(
        server_pack.join("manifest.json"),
        blobs.join(&update.manifest.sha256),
    )
    .unwrap();
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

const BASEMAP: &[u8] = &[7u8; 50_000];

#[test]
fn first_install_then_daily_delta() {
    let t = tempfile::tempdir().unwrap();
    let server_v1 = t.path().join("server/v1");
    let server_v2 = t.path().join("server/v2");
    let m1 = make_pack(
        &server_v1,
        "20261115T020000Z",
        &[
            ("basemap/osm.pmtiles", BASEMAP),
            ("layers/pistes.json", b"{\"pistes\":1}"),
            ("layers/bera.json", b"{\"risk\":2}"),
            ("layers/old.json", b"{\"gone\":true}"),
            ("fonts/a.pbf", b"same"),
        ],
    );
    let m2 = make_pack(
        &server_v2,
        "20261116T170000Z",
        &[
            ("basemap/osm.pmtiles", BASEMAP),
            ("layers/pistes.json", b"{\"pistes\":1}"),
            ("layers/bera.json", b"{\"risk\":3}"),
            ("fonts/a.pbf", b"same"),
            // New path, contents the old pack has: reused, not downloaded.
            ("fonts/b.pbf", b"same"),
            ("layers/new.json", b"{\"new\":1}"),
            // Two new files with the same contents: one blob.
            ("layers/dup1.json", b"dup"),
            ("layers/dup2.json", b"dup"),
        ],
    );
    let (man1, man2) = (manifest(&m1), manifest(&m2));

    // First download: everything, the 50 kB basemap included.
    let phone = t.path().join("phone/cervieres");
    let full = Update::between(None, &man1, &m1);
    assert_eq!(full.from_version, None);
    assert_eq!(full.fetch.len(), 5);
    assert_eq!(full.fetch_bytes, man1.total_bytes + m1.len() as u64);
    download(&full, &server_v1, &t.path().join("blobs1"));
    let a = apply(&phone, &full, &t.path().join("blobs1")).unwrap();
    assert_eq!((a.fetched_files, a.reused_files), (5, 0));
    assert_eq!(read_tree(&phone), read_tree(&server_v1));
    assert!(Pack::open(&phone).unwrap().verify().unwrap().is_empty());

    // Daily update: only the changed BERA, the new file and one copy of the duplicate.
    let up = Update::between(Some(&man1), &man2, &m2);
    assert_eq!(up.from_version.as_deref(), Some("20261115T020000Z"));
    let fetched: Vec<&str> = up.fetch.iter().map(|b| b.paths[0].as_str()).collect();
    assert_eq!(
        fetched,
        ["layers/bera.json", "layers/new.json", "layers/dup1.json"]
    );
    assert_eq!(up.fetch[2].paths, ["layers/dup1.json", "layers/dup2.json"]);
    assert_eq!(up.reused_files, 4);
    assert_eq!(up.removed, ["layers/old.json"]);
    assert_eq!(
        up.fetch_bytes,
        (b"{\"risk\":3}".len() + b"{\"new\":1}".len() + 3 + m2.len()) as u64
    );
    let bera = up.changes.iter().find(|c| c.id == "bera").unwrap();
    assert_eq!(bera.change, Change::Changed);
    assert_eq!(bera.valid_until.as_deref(), Some("2026-11-16"));
    let pistes = up.changes.iter().find(|c| c.id == "pistes").unwrap();
    assert_eq!(pistes.change, Change::Unchanged);

    let blobs = t.path().join("blobs2");
    download(&up, &server_v2, &blobs);
    let a = apply(&phone, &up, &blobs).unwrap();
    assert_eq!(a.version, "20261116T170000Z");
    assert_eq!(
        (a.fetched_files, a.reused_files, a.removed_files),
        (4, 4, 1)
    );
    assert_eq!(read_tree(&phone), read_tree(&server_v2));
    assert!(Pack::open(&phone).unwrap().verify().unwrap().is_empty());
    assert!(!staging_dir(&phone).exists() && !previous_dir(&phone).exists());

    // Same update again: already installed, nothing done.
    assert_eq!(apply(&phone, &up, &blobs).unwrap().fetched_files, 0);
    // Up to date: nothing to download, nothing done.
    let none = Update::between(Some(&man2), &man2, &m2);
    assert!(none.is_up_to_date() && none.fetch.is_empty() && none.fetch_bytes == 0);
    apply(&phone, &none, &blobs).unwrap();
    assert_eq!(read_tree(&phone), read_tree(&server_v2));
}

#[test]
fn corrupt_blob_leaves_the_old_pack() {
    let t = tempfile::tempdir().unwrap();
    let phone = t.path().join("phone/cervieres");
    let m1 = make_pack(&phone, "20261115T020000Z", &[("layers/bera.json", b"1")]);
    let server = t.path().join("server");
    let m2 = make_pack(&server, "20261116T020000Z", &[("layers/bera.json", b"2")]);
    let up = Update::between(Some(&manifest(&m1)), &manifest(&m2), &m2);
    let blobs = t.path().join("blobs");
    download(&up, &server, &blobs);
    fs::write(blobs.join(&up.fetch[0].sha256), b"3").unwrap();
    let err = apply(&phone, &up, &blobs).unwrap_err().to_string();
    assert!(err.contains("blob"), "{err}");
    assert_eq!(fs::read(phone.join("layers/bera.json")).unwrap(), b"1");
    assert_eq!(fs::read(phone.join("manifest.json")).unwrap(), m1);
    assert!(!staging_dir(&phone).exists());
    // Missing blob: same.
    fs::remove_file(blobs.join(&up.fetch[0].sha256)).unwrap();
    assert!(apply(&phone, &up, &blobs).is_err());
    assert_eq!(fs::read(phone.join("manifest.json")).unwrap(), m1);
}

#[test]
fn interrupted_swaps_recover() {
    let t = tempfile::tempdir().unwrap();
    let phone = t.path().join("cervieres");
    // Crash after the old pack was moved away, before the new one was moved in.
    let m2 = make_pack(&staging_dir(&phone), "v2", &[("a", b"2")]);
    make_pack(&previous_dir(&phone), "v1", &[("a", b"1")]);
    recover(&phone).unwrap();
    assert_eq!(fs::read(phone.join("manifest.json")).unwrap(), m2);
    assert!(!staging_dir(&phone).exists() && !previous_dir(&phone).exists());

    // Crash while the staging directory was written (no manifest yet): old pack back.
    let phone = t.path().join("chapelle");
    let m1 = make_pack(&previous_dir(&phone), "v1", &[("a", b"1")]);
    fs::create_dir_all(staging_dir(&phone).join("layers")).unwrap();
    fs::write(staging_dir(&phone).join("layers/x"), b"half").unwrap();
    recover(&phone).unwrap();
    assert_eq!(fs::read(phone.join("manifest.json")).unwrap(), m1);
    assert!(!staging_dir(&phone).exists() && !previous_dir(&phone).exists());
}

#[test]
fn manifest_must_match_the_update() {
    let t = tempfile::tempdir().unwrap();
    let server = t.path().join("server");
    let m = make_pack(&server, "20261116T020000Z", &[("a", b"x")]);
    let mut up = Update::between(None, &manifest(&m), &m);
    let blobs = t.path().join("blobs");
    download(&up, &server, &blobs);
    up.to_version = "20261117T000000Z".into();
    let err = apply(&t.path().join("phone"), &up, &blobs)
        .unwrap_err()
        .to_string();
    assert!(err.contains("update says"), "{err}");
}

/// Writes the bundle of `update`, taking blobs from the server's copy of the new pack.
fn bundle(update: &Update, server_pack: &Path, m: &[u8], out: &Path) -> u64 {
    let by_sha: std::collections::HashMap<String, Vec<u8>> = update
        .fetch
        .iter()
        .map(|b| {
            (
                b.sha256.clone(),
                fs::read(server_pack.join(&b.paths[0])).unwrap(),
            )
        })
        .chain([(update.manifest.sha256.clone(), m.to_vec())])
        .collect();
    let mut f = fs::File::create(out).unwrap();
    write_bundle(&mut f, update, |sha| {
        Ok(std::io::Cursor::new(by_sha[sha].clone()))
    })
    .unwrap()
}

#[test]
fn bundles_install_in_one_file_and_fall_back_when_contents_are_missing() {
    let t = tempfile::tempdir().unwrap();
    let p = |n: &str| t.path().join(n);
    let files = |bera: &'static [u8], pistes: &'static [u8]| {
        [
            ("basemap/osm.pmtiles", BASEMAP),
            ("layers/bera.json", bera),
            ("layers/pistes.json", pistes),
        ]
    };
    let mb = make_pack(&p("server/base"), "v1", &files(b"1", b"p"));
    let m2 = make_pack(&p("server/v2"), "v2", &files(b"2", b"p"));
    let m3 = make_pack(&p("server/v3"), "v3", &files(b"3", b"q"));
    let (b1, b2, b3) = (manifest(&mb), manifest(&m2), manifest(&m3));

    // First install: the base bundle, one file, blobs read in place.
    let phone = p("phone/cervieres");
    let full = Update::between(None, &b1, &mb);
    let n = bundle(&full, &p("server/base"), &mb, &p("base.skb"));
    assert_eq!(n, fs::metadata(p("base.skb")).unwrap().len());
    let (hdr, offsets) = read_bundle(&p("base.skb")).unwrap();
    assert_eq!(offsets.len(), 4);
    let gz = hdr.manifest_gzip_bytes.unwrap();
    assert!(gz < full.manifest.bytes, "{gz} >= {}", full.manifest.bytes);
    assert_eq!(
        Update {
            manifest_gzip_bytes: None,
            ..hdr
        },
        full
    );
    apply_bundle(&phone, &p("base.skb")).unwrap();
    assert_eq!(read_tree(&phone), read_tree(&p("server/base")));

    // Deltas from the base: small, one file each.
    let d13 = Update::between(Some(&b1), &b3, &m3);
    bundle(&d13, &p("server/v3"), &m3, &p("d13.skb"));
    let d13_len = fs::metadata(p("d13.skb")).unwrap().len();
    assert!(d13_len < 4000, "{d13_len}");
    // A truncated download is refused before anything is touched.
    let bytes = fs::read(p("d13.skb")).unwrap();
    fs::write(p("cut.skb"), &bytes[..bytes.len() - 1]).unwrap();
    let e = apply_bundle(&phone, &p("cut.skb")).unwrap_err().to_string();
    assert!(e.contains("incomplete"), "{e}");
    assert_eq!(read_tree(&phone), read_tree(&p("server/base")));
    // A delta built from the base also applies to a later version on the same base.
    let d12 = Update::between(Some(&b1), &b2, &m2);
    bundle(&d12, &p("server/v2"), &m2, &p("d12.skb"));
    apply_bundle(&phone, &p("d12.skb")).unwrap();
    assert_eq!(read_tree(&phone), read_tree(&p("server/v2")));
    apply_bundle(&phone, &p("d13.skb")).unwrap();
    assert_eq!(read_tree(&phone), read_tree(&p("server/v3")));

    // A delta needing contents the phone no longer has (BERA back to the base's):
    // refused with MISSING_CONTENTS, pack untouched; the app installs the base bundle.
    let m4 = make_pack(&p("server/v4"), "v4", &files(b"1", b"q"));
    let d14 = Update::between(Some(&b1), &manifest(&m4), &m4);
    bundle(&d14, &p("server/v4"), &m4, &p("d14.skb"));
    let e = apply_bundle(&phone, &p("d14.skb")).unwrap_err().to_string();
    assert!(e.starts_with(&format!("pack: {MISSING_CONTENTS}")), "{e}");
    assert_eq!(read_tree(&phone), read_tree(&p("server/v3")));
}

/// An hourly forecast as the pipeline writes it (indented JSON, many similar hours).
fn weather_json(run: u32) -> Vec<u8> {
    let hours: Vec<serde_json::Value> = (0..81)
        .map(|h| {
            json!({"time": format!("2026-11-17T{:02}:00", h % 24),
                   "temperature_c": -3.5 + f64::from((h + run) % 9),
                   "wind_kmh": 12.0, "gust_kmh": 20.0 + f64::from(h % 5), "snowfall_cm": 0.0,
                   "cloud_cover_pct": f64::from((h * 7 + run) % 100)})
        })
        .collect();
    serde_json::to_vec_pretty(&json!({"points": [{"id": "centre", "hours": hours}]})).unwrap()
}

#[test]
fn text_blobs_are_compressed_in_bundles_and_checked_once_inflated() {
    let t = tempfile::tempdir().unwrap();
    let p = |n: &str| t.path().join(n);
    let (w1, w2) = (weather_json(1), weather_json(2));
    let m1 = make_pack(
        &p("server/v1"),
        "v1",
        &[
            ("basemap/osm.pmtiles", BASEMAP),
            ("layers/bera.json", b"1"),
            ("layers/meteo-hourly.json", &w1),
        ],
    );
    let m2 = make_pack(
        &p("server/v2"),
        "v2",
        &[
            ("basemap/osm.pmtiles", BASEMAP),
            ("layers/bera.json", b"1"),
            ("layers/meteo-hourly.json", &w2),
        ],
    );
    let (b1, b2) = (manifest(&m1), manifest(&m2));
    let phone = p("phone/cervieres");
    let full = Update::between(None, &b1, &m1);
    bundle(&full, &p("server/v1"), &m1, &p("base.skb"));
    let raw = fs::read(p("base.skb")).unwrap();
    assert_eq!(&raw[..8], b"SKMPBND2");
    let (hdr, _) = read_bundle(&p("base.skb")).unwrap();
    assert_eq!(hdr.format_version, 2);
    // The weather is stored compressed, the basemap (PMTiles) and the one-byte BERA as is.
    let stored = |u: &Update, path: &str| {
        u.fetch
            .iter()
            .find(|b| b.paths.iter().any(|x| x == path))
            .unwrap()
            .gzip_bytes
    };
    let gz = stored(&hdr, "layers/meteo-hourly.json").unwrap();
    assert!(gz * 5 < w1.len() as u64, "{gz} vs {}", w1.len());
    assert_eq!(stored(&hdr, "basemap/osm.pmtiles"), None);
    assert_eq!(stored(&hdr, "layers/bera.json"), None);
    apply_bundle(&phone, &p("base.skb")).unwrap();
    assert_eq!(read_tree(&phone), read_tree(&p("server/v1")));

    // The weather of the next run: the delta is the header, the manifest and the
    // compressed weather.
    let d = Update::between(Some(&b1), &b2, &m2);
    let n = bundle(&d, &p("server/v2"), &m2, &p("d12.skb"));
    assert!(n < w2.len() as u64 / 2, "{n} vs {}", w2.len());
    // A damaged compressed blob is refused, the installed pack untouched.
    let mut bad = fs::read(p("d12.skb")).unwrap();
    let k = bad.len() - 40;
    bad[k] ^= 0x55;
    fs::write(p("bad.skb"), &bad).unwrap();
    apply_bundle(&phone, &p("bad.skb")).unwrap_err();
    assert_eq!(read_tree(&phone), read_tree(&p("server/v1")));
    apply_bundle(&phone, &p("d12.skb")).unwrap();
    assert_eq!(read_tree(&phone), read_tree(&p("server/v2")));
}

#[test]
fn bundles_of_format_version_1_still_install() {
    let t = tempfile::tempdir().unwrap();
    let p = |n: &str| t.path().join(n);
    let w = weather_json(1);
    let m = make_pack(
        &p("server/v1"),
        "v1",
        &[
            ("basemap/osm.pmtiles", BASEMAP),
            ("layers/meteo-hourly.json", &w),
        ],
    );
    // As written before #160: plain JSON header, manifest gzip, every blob as is.
    let mut u = Update::between(None, &manifest(&m), &m);
    u.format_version = 1;
    let mut gz = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::best());
    std::io::Write::write_all(&mut gz, &m).unwrap();
    let manifest_gz = gz.finish().unwrap();
    u.manifest_gzip_bytes = Some(manifest_gz.len() as u64);
    let header = serde_json::to_vec(&u).unwrap();
    let mut out = b"SKMPBND1".to_vec();
    out.extend((header.len() as u64).to_le_bytes());
    out.extend(&header);
    out.extend(&manifest_gz);
    for b in &u.fetch {
        out.extend(fs::read(p("server/v1").join(&b.paths[0])).unwrap());
    }
    fs::write(p("v1.skb"), &out).unwrap();
    let phone = p("phone/cervieres");
    apply_bundle(&phone, &p("v1.skb")).unwrap();
    assert_eq!(read_tree(&phone), read_tree(&p("server/v1")));
}
