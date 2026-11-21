//! The SQLite store of the reports, in memory, with a simulated time (V8, V10).

#![cfg(feature = "server")]

use jiff::{SignedDuration, Timestamp};
use skimap_collab::rules::{State, Vote};
use skimap_collab::store::{NewReport, Refusal, Store};

const A: &str = "device-aaaaaaaaaaaaaaaa";
const B: &str = "device-bbbbbbbbbbbbbbbb";
const C: &str = "device-cccccccccccccccc";
const BOX: [f64; 4] = [6.6, 44.7, 6.9, 44.95];

fn t0() -> Timestamp {
    "2026-01-10T09:00:00Z".parse().unwrap()
}

fn report(kind: &str, device: &str, seen: Timestamp) -> NewReport {
    NewReport {
        kind: kind.into(),
        lon: 6.77,
        lat: 44.85,
        seen_at: seen,
        device: device.into(),
    }
}

fn jpeg() -> Vec<u8> {
    let exif = b"Exif\0\0GPS 44.85";
    let mut v = vec![0xFF, 0xD8, 0xFF, 0xE1, 0x00, exif.len() as u8 + 2];
    v.extend_from_slice(exif);
    v.extend_from_slice(&[
        0xFF, 0xC0, 0x00, 0x0B, 8, 0x01, 0xE0, 0x02, 0x80, 1, 1, 0x11, 0,
    ]);
    v.extend_from_slice(&[
        0xFF, 0xDA, 0x00, 0x08, 1, 1, 0, 0, 63, 0, 0x12, 0x34, 0xFF, 0xD9,
    ]);
    v
}

#[test]
fn ice_expires_unless_confirmed_and_is_purged_with_its_photo() {
    let s = Store::in_memory().unwrap();
    let now = t0();
    let ice = s.create(&report("ice", A, now), now).unwrap().unwrap();
    assert_eq!(ice.state, State::Active);
    assert_eq!(ice.expires_at, now + SignedDuration::from_hours(24));
    let obstacle = s.create(&report("obstacle", A, now), now).unwrap().unwrap();
    s.put_photo(&ice.id, A, &jpeg(), now).unwrap().unwrap();
    // Photo hidden until another device confirms.
    assert!(s.photo(&ice.id, now).unwrap().is_none());
    assert_eq!(s.list(BOX, now).unwrap().len(), 2);

    // After 24 h the ice is gone from the listing, the obstacle stays.
    let later = now + SignedDuration::from_hours(24);
    let shown: Vec<String> = s
        .list(BOX, later)
        .unwrap()
        .into_iter()
        .map(|r| r.kind)
        .collect();
    assert_eq!(shown, vec!["obstacle"]);
    let p = s.purge(later).unwrap();
    assert_eq!((p.reports, p.photos), (1, 1));
    assert!(s.view(&ice.id, later).unwrap().is_none());
    assert!(s.view(&obstacle.id, later).unwrap().is_some());
}

#[test]
fn confirmation_extends_and_shows_the_photo_reputation_moves() {
    let s = Store::in_memory().unwrap();
    let now = t0();
    let r = s.create(&report("ice", A, now), now).unwrap().unwrap();
    s.put_photo(&r.id, A, &jpeg(), now).unwrap().unwrap();
    // Own report: no vote.
    assert!(matches!(
        s.vote(&r.id, A, Vote::Confirm, now).unwrap(),
        Err(Refusal::Forbidden(_))
    ));
    let t = now + SignedDuration::from_hours(20);
    let v = s.vote(&r.id, B, Vote::Confirm, t).unwrap().unwrap();
    assert_eq!(
        (v.confirmations, v.expires_at),
        (1, t + SignedDuration::from_hours(24))
    );
    assert!(v.photo);
    let photo = s.photo(&r.id, t).unwrap().unwrap();
    assert!(
        !String::from_utf8_lossy(&photo).contains("Exif"),
        "EXIF removed"
    );
    assert!((s.reputation(A).unwrap().unwrap() - 1.1).abs() < 1e-9);
    // Confirming twice counts once.
    s.vote(&r.id, B, Vote::Confirm, t).unwrap().unwrap();
    assert!((s.reputation(A).unwrap().unwrap() - 1.1).abs() < 1e-9);
    // Still there at 30 h thanks to the confirmation, purged at 44 h.
    assert_eq!(
        s.list(BOX, now + SignedDuration::from_hours(30))
            .unwrap()
            .len(),
        1
    );
    assert_eq!(
        s.purge(now + SignedDuration::from_hours(43))
            .unwrap()
            .reports,
        0
    );
    assert_eq!(
        s.purge(now + SignedDuration::from_hours(44))
            .unwrap()
            .reports,
        1
    );
    // A second photo is refused, a photo of someone else's report too.
    let r2 = s.create(&report("track", A, now), now).unwrap().unwrap();
    assert!(matches!(
        s.put_photo(&r2.id, B, &jpeg(), now).unwrap(),
        Err(Refusal::Forbidden(_))
    ));
    s.put_photo(&r2.id, A, &jpeg(), now).unwrap().unwrap();
    assert!(matches!(
        s.put_photo(&r2.id, A, &jpeg(), now).unwrap(),
        Err(Refusal::Conflict(_))
    ));
    assert!(matches!(
        s.put_photo(&r2.id, A, b"not a jpeg", now).unwrap(),
        Err(Refusal::Invalid(_))
    ));
}

#[test]
fn two_denials_refute_the_author_loses_reputation_and_gets_held() {
    let s = Store::in_memory().unwrap();
    let mut now = t0();
    for round in 0..2 {
        let r = s.create(&report("obstacle", A, now), now).unwrap().unwrap();
        s.vote(&r.id, B, Vote::Deny, now).unwrap().unwrap();
        let v = s.vote(&r.id, C, Vote::Deny, now).unwrap().unwrap();
        assert_eq!(v.state, State::Refuted, "round {round}");
        // Not listed, and no more votes on it.
        assert!(s.list(BOX, now).unwrap().iter().all(|x| x.id != r.id));
        assert_eq!(
            s.vote(&r.id, B, Vote::Confirm, now).unwrap(),
            Err(Refusal::NotFound)
        );
        now += SignedDuration::from_mins(10);
    }
    assert!((s.reputation(A).unwrap().unwrap() - 0.4).abs() < 1e-9);
    // Held: not shown until someone confirms.
    let r = s.create(&report("ice", A, now), now).unwrap().unwrap();
    assert_eq!(r.state, State::Held);
    assert!(s.list(BOX, now).unwrap().is_empty());
    let v = s.vote(&r.id, B, Vote::Confirm, now).unwrap().unwrap();
    assert_eq!(v.state, State::Active);
    assert_eq!(s.list(BOX, now).unwrap().len(), 1);
    assert_eq!(s.purge(now).unwrap().reports, 2, "refuted ones purged");
}

#[test]
fn offline_queue_times_and_rate_limits() {
    let s = Store::in_memory().unwrap();
    let now = t0();
    // Seen 30 h ago, sent now: kept with the real time of the observation.
    let r = s
        .create(
            &report("track", A, now - SignedDuration::from_hours(30)),
            now,
        )
        .unwrap()
        .unwrap();
    assert_eq!(r.seen_at, now - SignedDuration::from_hours(30));
    assert_eq!(r.expires_at, now + SignedDuration::from_hours(18));
    // Ice seen 25 h ago: already expired, dropped.
    assert_eq!(
        s.create(&report("ice", A, now - SignedDuration::from_hours(25)), now)
            .unwrap(),
        Err(Refusal::Gone)
    );
    assert!(matches!(
        s.create(&report("ice", A, now + SignedDuration::from_hours(1)), now)
            .unwrap(),
        Err(Refusal::Invalid(_))
    ));
    assert!(matches!(
        s.create(&report("free text", A, now), now).unwrap(),
        Err(Refusal::Invalid(_))
    ));
    assert!(matches!(
        s.create(&report("ice", "short", now), now).unwrap(),
        Err(Refusal::Invalid(_))
    ));
    let mut far = report("ice", A, now);
    far.lon = -73.9;
    far.lat = 40.7;
    assert!(matches!(
        s.create(&far, now).unwrap(),
        Err(Refusal::Invalid(_))
    ));
    // 12 per hour per device: the first counted above, 11 more, then refused.
    for i in 0..11 {
        s.create(
            &report("groomed", A, now),
            now + SignedDuration::from_secs(i),
        )
        .unwrap()
        .unwrap();
    }
    assert!(matches!(
        s.create(
            &report("groomed", A, now),
            now + SignedDuration::from_mins(1)
        )
        .unwrap(),
        Err(Refusal::RateLimited(_))
    ));
    // Another device is not limited; an hour later the first is free again.
    s.create(&report("groomed", B, now), now).unwrap().unwrap();
    s.create(
        &report("groomed", A, now + SignedDuration::from_hours(1)),
        now + SignedDuration::from_hours(1),
    )
    .unwrap()
    .unwrap();
}

#[test]
fn flag_delete_forget_and_no_identifier_stored_in_clear() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("collab.sqlite");
    let now = t0();
    let id = {
        let s = Store::open(&path).unwrap();
        let r = s.create(&report("ice", A, now), now).unwrap().unwrap();
        s.put_photo(&r.id, A, &jpeg(), now).unwrap().unwrap();
        s.vote(&r.id, B, Vote::Confirm, now).unwrap().unwrap();
        assert!(s.photo(&r.id, now).unwrap().is_some());
        s.flag_photo(&r.id, C, now).unwrap().unwrap();
        assert!(s.photo(&r.id, now).unwrap().is_none());
        assert!(!s.view(&r.id, now).unwrap().unwrap().photo);
        r.id
    };
    // Reopened: same salt, same hashes; the device identifier is nowhere in the file.
    let s = Store::open(&path).unwrap();
    assert!(s.reputation(A).unwrap().is_some());
    let raw = std::fs::read(&path).unwrap();
    let wal = std::fs::read(path.with_extension("sqlite-wal")).unwrap_or_default();
    for bytes in [&raw, &wal] {
        assert!(!bytes.windows(A.len()).any(|w| w == A.as_bytes()));
    }
    assert_eq!(s.delete(&id, B).unwrap(), Err(Refusal::NotFound), "not B's");
    s.delete(&id, A).unwrap().unwrap();
    assert!(s.view(&id, now).unwrap().is_none());
    let p = s.purge(now + SignedDuration::from_hours(24 * 61)).unwrap();
    assert_eq!(p.devices, 3);
    assert_eq!(s.stats().unwrap().devices, 0);
}

#[test]
fn listing_ten_thousand_reports_is_bounded_and_fast() {
    let mut s = Store::in_memory().unwrap();
    s.limits.reports_per_hour = u32::MAX;
    s.limits.reports_per_day = u32::MAX;
    let now = t0();
    let t = std::time::Instant::now();
    for i in 0..10_000 {
        let mut r = report("obstacle", &format!("device-{i:016}"), now);
        r.lon = 5.0 + f64::from(i % 100) * 0.03;
        r.lat = 44.0 + f64::from(i / 100) * 0.03;
        s.create(&r, now).unwrap().unwrap();
    }
    let insert = t.elapsed();
    let t = std::time::Instant::now();
    let shown = s.list(BOX, now).unwrap();
    let list = t.elapsed();
    // Box of 0.3° × 0.25°: 10 × 8 points of the grid.
    assert_eq!(shown.len(), 80);
    let all = s.list([0.0, 40.0, 10.0, 52.0], now).unwrap();
    assert_eq!(all.len(), 500, "bounded");
    eprintln!("10 000 reports inserted in {insert:?}, one site listed in {list:?}");
    assert!(list < std::time::Duration::from_millis(200));
}
