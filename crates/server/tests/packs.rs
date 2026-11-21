//! Publication and synchronisation end to end, over HTTP: catalog and index with ETag (a
//! sync without change is one 304), bundles with byte ranges, the phone-side plan and
//! `apply_bundle` of the pack crate, base rebuild, retention, garbage collection.

#![allow(
    clippy::disallowed_methods,
    reason = "tests write their inputs directly"
)]
mod common;

use std::net::SocketAddr;
use std::path::Path;
use std::sync::Arc;

use common::{get, make_pack, request};
use jiff::SignedDuration;
use jiff::civil::date;
use skimap_pack::Pack;
use skimap_pack::sync::{
    SiteIndex, SyncState, plan, plan_full, plan_whole_files, read_state, write_state,
};
use skimap_pack::update::{MISSING_CONTENTS, apply_bundle};
use skimap_server::app::App;
use skimap_server::clock::{Clock, ManualClock};
use skimap_server::store::{Retention, Store};

const BASEMAP: &[u8] = &[42u8; 100_000];
const OTHER_BASEMAP: &[u8] = &[7u8; 100_000];

/// Every file under `dir` with the SHA-256 of its contents.
fn tree(dir: &Path) -> Vec<(String, String)> {
    fn walk(root: &Path, d: &Path, out: &mut Vec<(String, String)>) {
        for e in std::fs::read_dir(d).unwrap() {
            let p = e.unwrap().path();
            if p.is_dir() {
                walk(root, &p, out);
            } else {
                out.push((
                    p.strip_prefix(root).unwrap().to_string_lossy().into_owned(),
                    skimap_pack::update::sha256_hex(&std::fs::read(&p).unwrap()),
                ));
            }
        }
    }
    let mut out = Vec::new();
    walk(dir, dir, &mut out);
    out.sort();
    out
}

/// Requests and bytes of one sync.
#[derive(Debug, Default, PartialEq, Eq)]
struct Cost {
    requests: u32,
    bytes: u64,
}

/// What the app does: revalidate the index, download the bundles of the plan (falling
/// back to the base when contents are missing), apply them, record the state.
fn sync(addr: SocketAddr, phone: &Path, work: &Path) -> Cost {
    let state = read_state(phone).unwrap();
    let mut cost = Cost::default();
    let etag = state.as_ref().and_then(|s| s.index_etag.clone());
    let hdr: Vec<(&str, &str)> = etag
        .as_deref()
        .map(|e| ("If-None-Match", e))
        .into_iter()
        .collect();
    let r = request(addr, "GET", "/v1/sites/cervieres/index.json", &hdr, b"");
    cost.requests += 1;
    cost.bytes += r.body.len() as u64;
    if r.status == 304 {
        return cost;
    }
    assert_eq!(r.status, 200);
    let etag = r.header("etag").map(str::to_owned);
    let index: SiteIndex = serde_json::from_slice(&r.body).unwrap();
    let mut todo = plan(&index, state.as_ref());
    let mut fell_back = false;
    let mut i = 0;
    std::fs::create_dir_all(work).unwrap();
    while i < todo.len() {
        let b = &todo[i];
        let r = get(addr, &b.url);
        cost.requests += 1;
        cost.bytes += r.body.len() as u64;
        assert_eq!((r.status, r.body.len() as u64), (200, b.bytes));
        let path = work.join(&b.sha256);
        std::fs::write(&path, &r.body).unwrap();
        match apply_bundle(phone, &path) {
            Ok(_) => i += 1,
            Err(e) if e.to_string().contains(MISSING_CONTENTS) && !fell_back => {
                fell_back = true;
                todo = plan_full(&index);
                i = 0;
            }
            Err(e) => panic!("{e}"),
        }
    }
    write_state(
        phone,
        &SyncState {
            version: index.latest.version.clone(),
            base_version: index.base_version.clone(),
            index_etag: etag,
        },
    )
    .unwrap();
    cost
}

fn index(addr: SocketAddr) -> SiteIndex {
    serde_json::from_slice(&get(addr, "/v1/sites/cervieres/index.json").body).unwrap()
}

type Files = [(&'static str, &'static [u8]); 5];

fn files(bera: &'static [u8], meta: &'static [u8], basemap: &'static [u8]) -> Files {
    [
        ("basemap/osm.pmtiles", basemap),
        ("basemap/osm/fonts/Noto Sans Regular/0-255.pbf", b"glyphs"),
        ("layers/pistes.json", b"{\"pistes\":1}"),
        ("layers/bera.json", bera),
        ("pack.meta.json", meta),
    ]
}

#[test]
fn publish_and_sync() {
    let t = tempfile::tempdir().unwrap();
    let out = t.path().join("out/cervieres/pack");
    let clock = Arc::new(ManualClock::new(
        skimap_common::dates::utc(date(2026, 11, 15).at(2, 0, 0, 0)).unwrap(),
    ));
    let store = Store::open(t.path().join("store")).unwrap();
    let retention = Retention {
        recent_versions: 2,
        rebase_fraction: 0.10,
    };
    let publish = |version: &str, f: &Files| {
        make_pack(&out, version, f);
        let r = store.publish(&out, clock.now(), retention).unwrap();
        clock.advance(SignedDuration::from_hours(12));
        r
    };

    let r = publish("v01", &files(b"{\"risk\":1}", b"1", BASEMAP));
    assert_eq!(
        (r.status.as_str(), r.rebased, r.new_blobs),
        ("published", true, 6)
    );
    // Rebuilt with nothing new but its build time: not published again.
    let r = publish("v02", &files(b"{\"risk\":1}", b"2", BASEMAP));
    assert_eq!((r.status.as_str(), r.latest.as_str()), ("unchanged", "v01"));
    let expected_v1 = t.path().join("expected/v01");
    make_pack(&expected_v1, "v01", &files(b"{\"risk\":1}", b"1", BASEMAP));

    let app = App::new(
        Store::open(t.path().join("store")).unwrap(),
        None,
        None,
        "https://skimap.test",
        "contact@example.org",
        clock.clone(),
    );
    let server = skimap_server::http::serve("127.0.0.1:0", 4, Arc::new(app)).unwrap();
    let addr = server.addr;
    assert_eq!(get(addr, "/healthz").status, 200);
    assert!(String::from_utf8_lossy(&get(addr, "/privacy").body).contains("contact@example.org"));

    // Catalog.
    let c = get(addr, "/v1/catalog.json").json();
    assert_eq!(c["sites"][0]["site"]["id"], "cervieres");
    assert_eq!(c["sites"][0]["latest"]["version"], "v01");
    assert_eq!(c["sites"][0]["index_url"], "/v1/sites/cervieres/index.json");

    // First sync: index + base bundle = 2 requests; the pack is the pipeline's.
    let phone = t.path().join("phone/cervieres");
    let cost = sync(addr, &phone, &t.path().join("dl1"));
    assert_eq!(cost.requests, 2);
    assert_eq!(tree(&phone), tree(&expected_v1));
    assert!(Pack::open(&phone).unwrap().verify().unwrap().is_empty());

    // Nothing new: one request, 304, empty body.
    let cost = sync(addr, &phone, &t.path().join("dl"));
    assert_eq!(
        cost,
        Cost {
            requests: 1,
            bytes: 0
        }
    );

    // Bundles: immutable, resumable.
    let base_url = index(addr).base.url;
    let full = get(addr, &base_url).body;
    let r = request(addr, "GET", &base_url, &[("Range", "bytes=1000-")], b"");
    assert_eq!(r.status, 206);
    assert_eq!(r.body, full[1000..]);
    assert_eq!(
        r.header("cache-control"),
        Some(skimap_server::app::IMMUTABLE)
    );
    let r = request(addr, "HEAD", &base_url, &[], b"");
    assert_eq!(r.header("content-length").unwrap(), full.len().to_string());
    let etag = r.header("etag").unwrap().to_owned();
    assert_eq!(
        request(addr, "GET", &base_url, &[("If-None-Match", &etag)], b"").status,
        304
    );
    let r = request(addr, "GET", &base_url, &[("Range", "bytes=99999999-")], b"");
    assert_eq!(r.status, 416);
    let none = format!("/v1/bundles/{}.skb", "0".repeat(64));
    assert_eq!(get(addr, &none).status, 404);
    assert_eq!(get(addr, "/v1/bundles/..%2Findex.json").status, 404);
    assert_eq!(get(addr, "/v1/sites/nowhere/index.json").status, 404);
    assert_eq!(get(addr, "/v1/sites/..%2F..%2Fx/index.json").status, 404);
    assert_eq!(
        request(addr, "PUT", "/v1/catalog.json", &[], b"").status,
        405
    );

    // The next day: new BERA. One delta bundle, small.
    let r = publish("v03", &files(b"{\"risk\":3}", b"3", BASEMAP));
    assert_eq!((r.status.as_str(), r.rebased), ("published", false));
    assert_eq!(r.changed, ["bera"]);
    assert!(r.daily_delta_bytes.unwrap() < 5000, "{r:?}");
    let cost = sync(addr, &phone, &t.path().join("dl2"));
    assert_eq!(cost.requests, 2);
    assert!(cost.bytes < 10_000, "{cost:?}");
    assert_eq!(tree(&phone), tree(&out));
    let bera = index(addr)
        .changes
        .into_iter()
        .find(|c| c.id == "bera")
        .unwrap();
    assert_eq!(bera.change, skimap_pack::update::Change::Changed);
    assert_eq!(bera.valid_until.as_deref(), Some("2026-11-16"));

    // The phone stays offline while four versions go by: its version is no longer kept,
    // it gets the delta from the base (one bundle), not the whole pack.
    for (v, x) in [
        ("v04", &b"4"[..]),
        ("v05", &b"5"[..]),
        ("v06", &b"6"[..]),
        ("v07", &b"7"[..]),
    ] {
        let r = publish(v, &files(x, x, BASEMAP));
        assert!(!r.rebased);
    }
    let from: Vec<_> = index(addr)
        .deltas
        .iter()
        .map(|d| d.from.clone().unwrap())
        .collect();
    assert_eq!(from, ["v01", "v05", "v06"]);
    let cost = sync(addr, &phone, &t.path().join("dl3"));
    assert_eq!(cost.requests, 2);
    assert!(cost.bytes < 10_000, "{cost:?}");
    assert_eq!(tree(&phone), tree(&out));

    // New basemap (more than a tenth of the pack): the base is rebuilt; the phone, on a
    // kept version, still gets a delta.
    let r = publish("v08", &files(b"8", b"8", OTHER_BASEMAP));
    assert!(r.rebased, "{r:?}");
    assert_eq!(r.base, "v08");
    let cost = sync(addr, &phone, &t.path().join("dl4"));
    assert_eq!(cost.requests, 2);
    assert!(cost.bytes > 100_000);
    assert_eq!(tree(&phone), tree(&out));
    assert!(!publish("v09", &files(b"9", b"9", OTHER_BASEMAP)).rebased);
    // A second phone installed at v01 long ago (another base): base + delta.
    let old_phone = t.path().join("phone2/cervieres");
    make_pack(&old_phone, "v01", &files(b"{\"risk\":1}", b"1", BASEMAP));
    write_state(
        &old_phone,
        &SyncState {
            version: "v01".into(),
            base_version: "v01".into(),
            index_etag: None,
        },
    )
    .unwrap();
    let cost = sync(addr, &old_phone, &t.path().join("dl5"));
    assert_eq!(cost.requests, 3);
    assert_eq!(tree(&old_phone), tree(&out));

    // Bundles of the previous index stay one more publication, then go.
    let prev = index(addr);
    let r = publish("v10", &files(b"10", b"10", OTHER_BASEMAP));
    assert!(r.files_removed > 0, "{r:?}");
    for d in &prev.deltas {
        assert_eq!(
            get(addr, &d.url).status,
            200,
            "previous index's bundles kept"
        );
    }
    publish("v11", &files(b"11", b"11", OTHER_BASEMAP));
    for d in &prev.deltas {
        assert_eq!(get(addr, &d.url).status, 404);
    }
    assert_eq!(get(addr, &index(addr).base.url).status, 200);

    // A pack that does not verify is refused.
    std::fs::write(out.join("layers/bera.json"), b"tampered").unwrap();
    let e = store.publish(&out, clock.now(), retention).unwrap_err();
    assert!(format!("{e:#}").contains("does not verify"), "{e:#}");

    server.stop();
}

/// A PMTiles archive of 60 x 60 tiles of zoom 12 that do not compress; `changed` tiles
/// (by index) get other contents.
fn archive(changed: &[u32]) -> Vec<u8> {
    let mut out = Vec::new();
    {
        let mut w = pmtiles::PmTilesWriter::new(pmtiles::TileType::Mvt)
            .min_zoom(12)
            .max_zoom(12)
            .create(std::io::Cursor::new(&mut out))
            .unwrap();
        for i in 0..3600u32 {
            let seed = u64::from(i) + if changed.contains(&i) { 1 << 32 } else { 0 };
            let mut x = seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1;
            let tile: Vec<u8> = (0..300)
                .map(|_| {
                    x ^= x << 13;
                    x ^= x >> 7;
                    x ^= x << 17;
                    x as u8
                })
                .collect();
            let c = pmtiles::TileCoord::new(12, 2000 + i % 60, 1400 + i / 60).unwrap();
            w.add_tile(c, &tile).unwrap();
        }
        w.finalize().unwrap();
    }
    out
}

/// Tile-by-tile update (#160): a basemap changed by a few tiles is published twice, whole
/// in `deltas` (what an app of format 2 reads, unchanged) and as a patch in
/// `patched_deltas`; a phone of format 3 installs the patched one, a phone of format 2
/// the whole one, both end with the pipeline's pack byte for byte.
#[test]
fn patched_deltas_send_the_changed_tiles() {
    let t = tempfile::tempdir().unwrap();
    let out = t.path().join("out/cervieres/pack");
    let store = Store::open(t.path().join("store")).unwrap();
    let retention = Retention {
        recent_versions: 2,
        // No rebase: the deltas are from the base.
        rebase_fraction: 1.0,
    };
    let now = skimap_common::dates::utc(date(2026, 11, 17).at(2, 0, 0, 0)).unwrap();
    let (a, b) = (archive(&[]), archive(&[5, 1000, 3599]));
    let pack = |basemap: &[u8], bera: &[u8]| -> Vec<(&'static str, Vec<u8>)> {
        vec![
            ("basemap/osm.pmtiles", basemap.to_vec()),
            ("layers/pistes.json", b"{\"pistes\":1}".to_vec()),
            ("layers/bera.json", bera.to_vec()),
            ("pack.meta.json", bera.to_vec()),
        ]
    };
    let make = |version: &str, files: &[(&'static str, Vec<u8>)]| {
        let f: Vec<(&str, &[u8])> = files.iter().map(|(p, b)| (*p, b.as_slice())).collect();
        make_pack(&out, version, &f);
    };
    make("v01", &pack(&a, b"1"));
    store.publish(&out, now, retention).unwrap();
    let phone = t.path().join("phone/cervieres");
    let old_phone = t.path().join("old-phone/cervieres");
    let clock = Arc::new(ManualClock::new(now));
    let app = App::new(
        Store::open(t.path().join("store")).unwrap(),
        None,
        None,
        "https://skimap.test",
        "contact@example.org",
        clock,
    );
    let server = skimap_server::http::serve("127.0.0.1:0", 4, Arc::new(app)).unwrap();
    let addr = server.addr;
    // Installs the bundles of `todo`; returns the bytes downloaded.
    let install = |dir: &Path, todo: Vec<skimap_pack::sync::BundleRef>| -> u64 {
        let mut n = 0;
        for b in todo {
            let body = get(addr, &b.url).body;
            n += body.len() as u64;
            let p = t.path().join(&b.sha256);
            std::fs::write(&p, &body).unwrap();
            apply_bundle(dir, &p).unwrap();
        }
        n
    };
    install(&phone, plan(&index(addr), None));
    install(&old_phone, plan_whole_files(&index(addr), None));
    assert_eq!(tree(&phone), tree(&out));

    make("v02", &pack(&b, b"2"));
    let r = store
        .publish(&out, now + SignedDuration::from_hours(24), retention)
        .unwrap();
    let (whole, patched) = (r.daily_delta_bytes.unwrap(), r.daily_patched_bytes.unwrap());
    assert!(patched * 20 < whole, "{patched} vs {whole}");
    let i = index(addr);
    assert_eq!(i.patched_deltas.len(), 1);
    assert_eq!(i.patched_deltas[0].from.as_deref(), Some("v01"));
    // What an app of format 2 reads: the whole delta, a bundle of format 2.
    assert_eq!(i.deltas.len(), 1);
    assert_eq!(&get(addr, &i.deltas[0].url).body[..8], b"SKMPBND2");
    assert_eq!(&get(addr, &i.patched_deltas[0].url).body[..8], b"SKMPBND3");
    let state = |v: &str| SyncState {
        version: v.into(),
        base_version: i.base_version.clone(),
        index_etag: None,
    };
    let new_bytes = install(&phone, plan(&i, Some(&state("v01"))));
    let old_bytes = install(&old_phone, plan_whole_files(&i, Some(&state("v01"))));
    assert_eq!((new_bytes, old_bytes), (patched, whole));
    assert_eq!(tree(&phone), tree(&out));
    assert_eq!(tree(&old_phone), tree(&out));
    server.stop();
}

#[test]
fn publish_many_sites_writes_one_catalog() {
    let t = tempfile::tempdir().unwrap();
    let store = Store::open(t.path().join("store")).unwrap();
    let retention = Retention {
        recent_versions: 2,
        rebase_fraction: 0.10,
    };
    let ids = ["cervieres", "gerardmer", "les-rousses-478721195"];
    let mut packs = Vec::new();
    for id in ids {
        let dir = t.path().join(format!("out/{id}/pack"));
        make_pack(&dir, "v01", &files(b"{\"risk\":1}", b"1", BASEMAP));
        // Same contents under another site identifier.
        let path = dir.join("manifest.json");
        let mut m: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        m["site"]["id"] = id.into();
        std::fs::write(&path, serde_json::to_vec_pretty(&m).unwrap()).unwrap();
        packs.push(dir);
    }
    // A directory without a pack fails alone.
    packs.push(t.path().join("out/absent/pack"));
    let now = skimap_common::dates::utc(date(2026, 11, 15).at(2, 0, 0, 0)).unwrap();
    let reports = store.publish_many(&packs, now, retention).unwrap();
    assert_eq!(reports.len(), 4);
    for r in &reports[..3] {
        assert_eq!(r.as_ref().unwrap().status, "published");
    }
    assert!(reports[3].is_err());
    assert_eq!(store.sites().unwrap(), ids);
    let catalog: serde_json::Value =
        serde_json::from_slice(&std::fs::read(store.catalog_path()).unwrap()).unwrap();
    let listed: Vec<&str> = catalog["sites"]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| s["site"]["id"].as_str().unwrap())
        .collect();
    assert_eq!(listed, ids);
    // Nothing changed: nothing published again.
    let again = store.publish_many(&packs[..3], now, retention).unwrap();
    assert!(
        again
            .iter()
            .all(|r| r.as_ref().unwrap().status == "unchanged")
    );
}

/// A store published when the voice came in packs (VO6, before the phone's own speech
/// synthesis): its voice packs, their indexes and bundles are removed at the next
/// publication, the catalog lists the site alone and their URLs answer 404.
#[test]
fn voice_packs_of_an_older_store_are_removed() {
    let t = tempfile::tempdir().unwrap();
    let clock = Arc::new(ManualClock::new(
        skimap_common::dates::utc(date(2026, 11, 15).at(2, 0, 0, 0)).unwrap(),
    ));
    let root = t.path().join("store");
    let store = Store::open(&root).unwrap();
    let retention = Retention::default();
    let site = t.path().join("out/cervieres/pack");
    make_pack(&site, "v01", &files(b"{\"risk\":1}", b"1", BASEMAP));
    store.publish(&site, clock.now(), retention).unwrap();
    // What the older server left: a served index of the English voice naming its bundle,
    // and its internal versions.
    let sha = "b".repeat(64);
    let served = root.join("public/sites/cervieres/voice/en");
    std::fs::create_dir_all(&served).unwrap();
    std::fs::write(served.join("index.json"), format!("{{\"base\":\"{sha}\"}}")).unwrap();
    let bundle = root.join(format!("public/bundles/{sha}.skb"));
    std::fs::write(&bundle, [1u8; 100]).unwrap();
    let internal = root.join("sites/cervieres/voice/en/versions");
    std::fs::create_dir_all(&internal).unwrap();
    std::fs::write(internal.join("v01.json"), b"{}").unwrap();

    clock.advance(SignedDuration::from_hours(12));
    make_pack(&site, "v02", &files(b"{\"risk\":2}", b"1", BASEMAP));
    store.publish(&site, clock.now(), retention).unwrap();
    assert!(!root.join("public/sites/cervieres/voice").exists());
    assert!(!root.join("sites/cervieres/voice").exists());
    assert!(!bundle.exists());
    assert_eq!(store.sites().unwrap(), ["cervieres"]);

    let app = App::new(
        Store::open(&root).unwrap(),
        None,
        None,
        "https://skimap.test",
        "contact@example.org",
        clock.clone(),
    );
    let server = skimap_server::http::serve("127.0.0.1:0", 4, Arc::new(app)).unwrap();
    let addr = server.addr;
    let c = get(addr, "/v1/catalog.json").json();
    assert_eq!(c["sites"].as_array().unwrap().len(), 1);
    assert!(c["sites"][0].get("voice").is_none(), "{c}");
    assert_eq!(index(addr).latest.version, "v02");
    assert_eq!(
        get(addr, "/v1/sites/cervieres/voice/en/index.json").status,
        404
    );
    server.stop();
}
