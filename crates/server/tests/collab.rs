//! Collaborative reports over HTTP with a simulated clock and SQLite in memory (V8 to V11):
//! report sent late from the offline queue, listing by box, confirmation, expiry, purge,
//! photo without EXIF, refusals, limits.

mod common;

use std::sync::Arc;

use common::request;
use jiff::civil::date;
use jiff::{SignedDuration, Timestamp};
use serde_json::{Value, json};
use skimap_server::app::App;
use skimap_server::clock::{Clock, ManualClock};
use skimap_server::collab::Collab;
use skimap_server::store::Store;

struct Env {
    clock: Arc<ManualClock>,
    collab: Arc<Collab>,
    addr: std::net::SocketAddr,
    _server: skimap_server::http::Running,
    _dir: tempfile::TempDir,
}

fn env() -> Env {
    let dir = tempfile::tempdir().unwrap();
    let clock = Arc::new(ManualClock::new(
        skimap_common::dates::utc(date(2026, 1, 10).at(8, 0, 0, 0)).unwrap(),
    ));
    let collab = Arc::new(Collab::new(
        skimap_collab::store::Store::in_memory().unwrap(),
    ));
    let mut app = App::new(
        Store::open(dir.path().join("store")).unwrap(),
        None,
        None,
        "https://skimap.test",
        "contact@example.org",
        clock.clone(),
    );
    app.collab = Some(collab.clone());
    let server = skimap_server::http::serve("127.0.0.1:0", 2, Arc::new(app)).unwrap();
    Env {
        clock,
        collab,
        addr: server.addr,
        _server: server,
        _dir: dir,
    }
}

const A: &str = "0f1e2d3c4b5a69788796a5b4c3d2e1f0";
const B: &str = "1f1e2d3c4b5a69788796a5b4c3d2e1f0";
const BOX: &str = "/v1/collab/reports?bbox=6.66,44.77,6.86,44.92";

fn post(e: &Env, path: &str, device: &str, body: &[u8], ctype: &str) -> common::Resp {
    request(
        e.addr,
        "POST",
        path,
        &[("Content-Type", ctype), ("X-Skimap-Device", device)],
        body,
    )
}

fn report(e: &Env, kind: &str, device: &str, seen: jiff::Timestamp) -> common::Resp {
    let body =
        json!({"kind": kind, "lon": 6.7712, "lat": 44.8456, "seen_at": seen, "device": device});
    post(
        e,
        "/v1/collab/reports",
        device,
        body.to_string().as_bytes(),
        "application/json",
    )
}

fn list(e: &Env) -> Vec<Value> {
    let r = common::get(e.addr, BOX);
    assert_eq!(r.status, 200);
    assert_eq!(r.header("cache-control"), Some("public, max-age=60"));
    r.json()["reports"].as_array().unwrap().clone()
}

/// A JPEG with an EXIF segment holding a GPS position and the phone's model, ~50 KB.
fn photo() -> Vec<u8> {
    let exif = b"Exif\0\0Pixel 8 Pro GPSLatitude=44.8456";
    let mut v = vec![0xFF, 0xD8, 0xFF, 0xE1, 0x00, exif.len() as u8 + 2];
    v.extend_from_slice(exif);
    v.extend_from_slice(&[
        0xFF, 0xC0, 0x00, 0x0B, 8, 0x01, 0xE0, 0x02, 0x80, 1, 1, 0x11, 0,
    ]);
    v.extend_from_slice(&[0xFF, 0xDA, 0x00, 0x08, 1, 1, 0, 0, 63, 0]);
    v.extend(std::iter::repeat_n(0x55u8, 50_000));
    v.extend_from_slice(&[0xFF, 0xD9]);
    v
}

#[test]
fn offline_report_confirmed_expires_and_is_purged() {
    let e = env();
    let now = e.clock.now();
    // Seen two hours ago without network, sent now: the time of the observation is kept.
    let r = report(&e, "ice", A, now - SignedDuration::from_hours(2));
    assert_eq!(r.status, 201, "{}", String::from_utf8_lossy(&r.body));
    let v = r.json();
    let id = v["id"].as_str().unwrap().to_owned();
    assert_eq!(v["seen_at"], json!(now - SignedDuration::from_hours(2)));
    assert_eq!(v["expires_at"], json!(now + SignedDuration::from_hours(22)));
    assert_eq!(v["state"], "active");
    assert_eq!(list(&e).len(), 1);
    // Outside the box: nothing.
    let r = common::get(e.addr, "/v1/collab/reports?bbox=5.9,46.5,6.2,46.7");
    assert!(r.json()["reports"].as_array().unwrap().is_empty());

    // Photo: EXIF removed, shown once someone else confirmed.
    let r = post(
        &e,
        &format!("/v1/collab/reports/{id}/photo"),
        A,
        &photo(),
        "image/jpeg",
    );
    assert_eq!(r.status, 204, "{}", String::from_utf8_lossy(&r.body));
    assert_eq!(
        common::get(e.addr, &format!("/v1/collab/photos/{id}.jpg")).status,
        404
    );
    e.clock.advance(SignedDuration::from_hours(20));
    let r = post(
        &e,
        &format!("/v1/collab/reports/{id}/confirm"),
        B,
        b"",
        "application/json",
    );
    assert_eq!(r.status, 200);
    assert_eq!(r.json()["confirmations"], 1);
    let p = common::get(e.addr, &format!("/v1/collab/photos/{id}.jpg"));
    assert_eq!(
        (p.status, p.header("content-type")),
        (200, Some("image/jpeg"))
    );
    assert!(!String::from_utf8_lossy(&p.body).contains("GPS"));
    assert!(!String::from_utf8_lossy(&p.body).contains("Pixel"));
    assert_eq!(p.body.len(), photo().len() - 41);

    // Without the confirmation it would have expired at +22 h; now at +44 h.
    e.clock.advance(SignedDuration::from_hours(10));
    assert_eq!(list(&e).len(), 1);
    e.clock.advance(SignedDuration::from_hours(14));
    assert!(list(&e).is_empty());
    e.collab.purge(e.clock.as_ref());
    assert_eq!(
        common::get(e.addr, &format!("/v1/collab/reports/{id}")).status,
        404
    );
    assert_eq!(e.collab.store.stats().unwrap().photos, 0);
}

#[test]
fn refusals_and_limits() {
    let e = env();
    let now = e.clock.now();
    // Ice seen 25 h ago: the queue drops it (410).
    assert_eq!(
        report(&e, "ice", A, now - SignedDuration::from_hours(25)).status,
        410
    );
    assert_eq!(report(&e, "avalanche", A, now).status, 400);
    assert_eq!(report(&e, "ice", "x", now).status, 400);
    let r = post(
        &e,
        "/v1/collab/reports",
        A,
        b"{\"kind\":\"ice\",\"text\":\"free\"}",
        "application/json",
    );
    assert_eq!(r.status, 400);
    assert_eq!(
        common::get(e.addr, "/v1/collab/reports?bbox=0,40,10,52").status,
        400,
        "too large a box"
    );
    assert_eq!(common::get(e.addr, "/v1/collab/reports").status, 400);
    // A vote on one's own report.
    let id = report(&e, "track", A, now).json()["id"]
        .as_str()
        .unwrap()
        .to_owned();
    let r = post(
        &e,
        &format!("/v1/collab/reports/{id}/deny"),
        A,
        b"",
        "application/json",
    );
    assert_eq!(r.status, 403);
    // A photo too large for the server (over 64 KiB): refused before reading it all.
    let mut big = photo();
    big.resize(70_000, 0);
    let r = post(
        &e,
        &format!("/v1/collab/reports/{id}/photo"),
        A,
        &big,
        "image/jpeg",
    );
    assert_eq!(r.status, 413);
    // 12 reports per device and hour.
    for _ in 0..11 {
        assert_eq!(report(&e, "groomed", A, now).status, 201);
    }
    let r = report(&e, "groomed", A, now);
    assert_eq!((r.status, r.header("retry-after")), (429, Some("600")));
    // Delete one's own report.
    let r = request(
        e.addr,
        "DELETE",
        &format!("/v1/collab/reports/{id}"),
        &[("X-Skimap-Device", B)],
        b"",
    );
    assert_eq!(r.status, 404);
    let r = request(
        e.addr,
        "DELETE",
        &format!("/v1/collab/reports/{id}"),
        &[("X-Skimap-Device", A)],
        b"",
    );
    assert_eq!(r.status, 204);
    // Rules, for the app and the documentation.
    let rules = common::get(e.addr, "/v1/collab/rules").json();
    assert_eq!(rules["kinds"].as_array().unwrap().len(), 6);
    assert_eq!(rules["free_text"], false);
}

#[test]
fn without_the_service_503() {
    let dir = tempfile::tempdir().unwrap();
    let app = App::new(
        Store::open(dir.path().join("store")).unwrap(),
        None,
        None,
        "https://skimap.test",
        "contact@example.org",
        Arc::new(ManualClock::new(Timestamp::now())),
    );
    let server = skimap_server::http::serve("127.0.0.1:0", 1, Arc::new(app)).unwrap();
    assert_eq!(common::get(server.addr, BOX).status, 503);
    server.stop();
}
