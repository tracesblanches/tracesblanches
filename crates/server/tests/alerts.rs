//! Non-return alert (S2) over HTTP with a simulated clock: plan, deadline, alert to every
//! contact, retries, return after the alert, extension, cancellation, purge, validation,
//! rate limit.

mod common;

use std::sync::Arc;

use chrono::{DateTime, Duration, TimeZone, Utc};
use common::request;
use serde_json::{Value, json};
use skimap_server::alerts::Alerts;
use skimap_server::alerts::mail::RecordingMailer;
use skimap_server::app::App;
use skimap_server::clock::{Clock, ManualClock};
use skimap_server::store::Store;

struct Env {
    clock: Arc<ManualClock>,
    mail: Arc<RecordingMailer>,
    alerts: Arc<Alerts>,
    addr: std::net::SocketAddr,
    _server: skimap_server::http::Running,
    _dir: tempfile::TempDir,
}

fn env() -> Env {
    let dir = tempfile::tempdir().unwrap();
    // Sunday 15/11/2026, 09:00 in Paris.
    let clock = Arc::new(ManualClock::new(
        Utc.with_ymd_and_hms(2026, 11, 15, 8, 0, 0).unwrap(),
    ));
    let mail = Arc::new(RecordingMailer::default());
    let alerts = Arc::new(
        Alerts::open(
            &dir.path().join("server/skimap.sqlite"),
            mail.clone(),
            clock.clone(),
            "https://skimap.test",
        )
        .unwrap(),
    );
    let app = App::new(
        Store::open(dir.path().join("store")).unwrap(),
        Some(alerts.clone()),
        None,
        "https://skimap.test",
        "contact@example.org",
        clock.clone(),
    );
    let server = skimap_server::http::serve("127.0.0.1:0", 2, Arc::new(app)).unwrap();
    Env {
        clock,
        mail,
        alerts,
        addr: server.addr,
        _server: server,
        _dir: dir,
    }
}

fn plan(deadline: DateTime<Utc>) -> Value {
    json!({
        "skier_name": "Marie Dupont",
        "skier_phone": "+33 6 12 34 56 78",
        "contacts": ["paul@example.org", "Anne@Example.org"],
        "deadline": deadline.to_rfc3339(),
        "grace_minutes": 30,
        "site": "cervieres",
        "plan": "Boucle par le Bourget et le col des Thures, retour par la piste des Fonts. Seule, raquettes dans le sac.",
        "route": [[6.7250, 44.8589], [6.7600, 44.8550], [6.8157, 44.8447]],
        "position": {"lon": 6.7251, "lat": 44.8590, "accuracy_m": 8.0}
    })
}

fn post(e: &Env, path: &str, token: Option<&str>, body: &Value) -> common::Resp {
    let auth = token.map(|t| format!("Bearer {t}"));
    let mut h = vec![("Content-Type", "application/json")];
    if let Some(a) = &auth {
        h.push(("Authorization", a.as_str()));
    }
    request(e.addr, "POST", path, &h, body.to_string().as_bytes())
}

fn get_trip(e: &Env, id: &str, token: &str) -> common::Resp {
    request(
        e.addr,
        "GET",
        &format!("/v1/trips/{id}"),
        &[("Authorization", &format!("Bearer {token}"))],
        b"",
    )
}

#[test]
fn alert_goes_out_after_deadline_and_grace_then_return_is_told() {
    let e = env();
    let deadline = e.clock.now() + Duration::hours(7); // 16:00 in Paris
    let r = post(&e, "/v1/trips", None, &plan(deadline));
    assert_eq!(r.status, 201, "{}", String::from_utf8_lossy(&r.body));
    let c = r.json();
    let (id, token) = (
        c["id"].as_str().unwrap().to_owned(),
        c["token"].as_str().unwrap().to_owned(),
    );
    assert_eq!(c["trip"]["state"], "active");
    assert_eq!(c["trip"]["alert_at"], "2026-11-15T15:30:00Z");

    // Someone else cannot read it.
    assert_eq!(get_trip(&e, &id, "wrong").status, 404);
    assert_eq!(
        request(e.addr, "GET", &format!("/v1/trips/{id}"), &[], b"").status,
        404
    );
    assert_eq!(get_trip(&e, &id, &token).json()["state"], "active");

    // The phone sends a position on the way.
    e.clock.advance(Duration::hours(3));
    let r = post(
        &e,
        &format!("/v1/trips/{id}/position"),
        Some(&token),
        &json!({"lon": 6.7701, "lat": 44.8561, "accuracy_m": 12}),
    );
    assert_eq!(r.status, 200);

    // The phone dies. Deadline passes, grace not yet: nothing.
    e.clock.set(deadline + Duration::minutes(29));
    let rep = e.alerts.tick().unwrap();
    assert_eq!((rep.alerted, rep.sent), (0, 0));
    assert!(e.mail.sent().is_empty());

    // Grace over: one alert per contact.
    e.clock.set(deadline + Duration::minutes(30));
    let rep = e.alerts.tick().unwrap();
    assert_eq!((rep.alerted, rep.sent), (1, 2));
    let sent = e.mail.sent();
    let to: Vec<&str> = sent.iter().map(|m| m.to.as_str()).collect();
    assert_eq!(to, ["paul@example.org", "anne@example.org"]);
    let m = &sent[0];
    assert_eq!(
        m.subject,
        "Alerte skimap : Marie Dupont n'a pas confirmé son retour"
    );
    for needle in [
        "dimanche 15/11/2026 à 16:00 (heure de Paris)",
        "30 minutes",
        "+33 6 12 34 56 78",
        "112",
        "col des Thures",
        "Secteur : Cervières (05)",
        "Départ prévu : 44.85890, 6.72500",
        "Arrivée prévue : 44.84470, 6.81570",
        "Dernière position reçue : 44.85610, 6.77010",
        "le dimanche 15/11/2026 à 12:00 (heure de Paris), précision 12 m",
        "https://www.openstreetmap.org/?mlat=44.85610&mlon=6.77010",
    ] {
        assert!(
            m.body.contains(needle),
            "missing {needle:?} in:\n{}",
            m.body
        );
    }
    // Ticking again sends nothing more.
    assert_eq!(e.alerts.tick().unwrap(), Default::default());
    let st = get_trip(&e, &id, &token).json();
    assert_eq!(st["state"], "alerted");
    assert_eq!(st["notifications"][0]["status"], "sent");

    // Back two hours later, with network: the contacts are told.
    e.clock.advance(Duration::hours(2));
    let r = post(
        &e,
        &format!("/v1/trips/{id}/checkin"),
        Some(&token),
        &json!({}),
    );
    assert_eq!(r.status, 200);
    assert_eq!(r.json()["state"], "returned");
    let rep = e.alerts.tick().unwrap();
    assert_eq!(rep.sent, 2);
    let sent = e.mail.sent();
    assert_eq!(sent.len(), 4);
    assert_eq!(
        sent[2].subject,
        "skimap : Marie Dupont est rentré(e), alerte levée"
    );
    assert!(
        sent[2]
            .body
            .contains("dimanche 15/11/2026 à 18:30 (heure de Paris)"),
        "{}",
        sent[2].body
    );
    // Checking in twice: conflict.
    assert_eq!(
        post(
            &e,
            &format!("/v1/trips/{id}/checkin"),
            Some(&token),
            &json!({})
        )
        .status,
        409
    );

    // Kept 7 days after the return, then deleted.
    e.clock.advance(Duration::days(7) - Duration::minutes(1));
    assert_eq!(e.alerts.tick().unwrap().purged, 0);
    e.clock.advance(Duration::minutes(2));
    assert_eq!(e.alerts.tick().unwrap().purged, 1);
    assert_eq!(get_trip(&e, &id, &token).status, 404);
}

#[test]
fn return_in_time_extension_and_cancellation() {
    let e = env();
    let now = e.clock.now();
    let c = post(&e, "/v1/trips", None, &plan(now + Duration::hours(2))).json();
    let (id, token) = (c["id"].as_str().unwrap(), c["token"].as_str().unwrap());
    e.clock.advance(Duration::hours(1));
    assert_eq!(
        post(
            &e,
            &format!("/v1/trips/{id}/checkin"),
            Some(token),
            &json!({})
        )
        .status,
        200
    );
    e.clock.advance(Duration::hours(5));
    assert_eq!(e.alerts.tick().unwrap().alerted, 0);

    // Extension: the alert moves.
    let c = post(
        &e,
        "/v1/trips",
        None,
        &plan(e.clock.now() + Duration::hours(1)),
    )
    .json();
    let (id, token) = (c["id"].as_str().unwrap(), c["token"].as_str().unwrap());
    let later = e.clock.now() + Duration::hours(3);
    let r = post(
        &e,
        &format!("/v1/trips/{id}/extend"),
        Some(token),
        &json!({"deadline": later.to_rfc3339()}),
    );
    assert_eq!(r.status, 200);
    e.clock.advance(Duration::hours(2));
    assert_eq!(e.alerts.tick().unwrap().alerted, 0);
    e.clock.advance(Duration::hours(1) + Duration::minutes(30));
    assert_eq!(e.alerts.tick().unwrap().alerted, 1);
    // After the alert, no extension and no cancellation: confirm the return.
    assert_eq!(
        post(
            &e,
            &format!("/v1/trips/{id}/extend"),
            Some(token),
            &json!({"deadline": (e.clock.now() + Duration::hours(1)).to_rfc3339()})
        )
        .status,
        409
    );
    assert_eq!(
        request(
            e.addr,
            "DELETE",
            &format!("/v1/trips/{id}"),
            &[("Authorization", &format!("Bearer {token}"))],
            b""
        )
        .status,
        409
    );

    // Cancellation before the deadline: never alerted.
    let c = post(
        &e,
        "/v1/trips",
        None,
        &plan(e.clock.now() + Duration::hours(1)),
    )
    .json();
    let (id, token) = (c["id"].as_str().unwrap(), c["token"].as_str().unwrap());
    let r = request(
        e.addr,
        "DELETE",
        &format!("/v1/trips/{id}"),
        &[("Authorization", &format!("Bearer {token}"))],
        b"",
    );
    assert_eq!(
        (r.status, r.json()["state"].as_str()),
        (200, Some("cancelled"))
    );
    e.clock.advance(Duration::hours(3));
    assert_eq!(e.alerts.tick().unwrap().alerted, 0);
}

#[test]
fn failed_deliveries_are_retried_and_cancelled_on_return() {
    let e = env();
    let c = post(
        &e,
        "/v1/trips",
        None,
        &plan(e.clock.now() + Duration::hours(1)),
    )
    .json();
    let (id, token) = (
        c["id"].as_str().unwrap().to_owned(),
        c["token"].as_str().unwrap().to_owned(),
    );
    e.mail.fail_next(3);
    e.clock.advance(Duration::minutes(90));
    let rep = e.alerts.tick().unwrap();
    assert_eq!((rep.alerted, rep.sent, rep.retried), (1, 0, 2));
    // Not due yet (retry in 1 minute).
    e.clock.advance(Duration::seconds(30));
    assert_eq!(e.alerts.tick().unwrap().sent, 0);
    e.clock.advance(Duration::seconds(30));
    let rep = e.alerts.tick().unwrap();
    assert_eq!((rep.sent, rep.retried), (1, 1));
    // Paul (first in the outbox) failed again, Anne got it.
    let st = get_trip(&e, &id, &token).json();
    assert_eq!(st["notifications"][0]["status"], "pending");
    assert_eq!(st["notifications"][0]["attempts"], 2);
    assert_eq!(st["notifications"][1]["status"], "sent");
    // Return before the second contact got the alert: that alert is dropped, and only
    // the contact who was alerted hears of the return.
    assert_eq!(
        post(
            &e,
            &format!("/v1/trips/{id}/checkin"),
            Some(&token),
            &json!({})
        )
        .status,
        200
    );
    e.clock.advance(Duration::minutes(10));
    let rep = e.alerts.tick().unwrap();
    assert_eq!(rep.sent, 1);
    let sent = e.mail.sent();
    assert_eq!(sent.len(), 2);
    assert_eq!(sent[1].to, sent[0].to);
    assert!(sent[1].subject.contains("rentré"));
    assert_eq!(sent[1].to, "anne@example.org");
    let st = get_trip(&e, &id, &token).json();
    assert_eq!(st["notifications"][0]["status"], "cancelled");

    // A relay down for a day: given up after 24 h.
    let c = post(
        &e,
        "/v1/trips",
        None,
        &plan(e.clock.now() + Duration::hours(1)),
    )
    .json();
    let (id, token) = (
        c["id"].as_str().unwrap().to_owned(),
        c["token"].as_str().unwrap().to_owned(),
    );
    e.mail.fail_next(10_000);
    e.clock.advance(Duration::minutes(90));
    let mut failed = 0;
    for _ in 0..60 {
        failed += e.alerts.tick().unwrap().failed;
        e.clock.advance(Duration::minutes(30));
    }
    assert_eq!(failed, 2);
    let st = get_trip(&e, &id, &token).json();
    assert_eq!(st["notifications"][0]["status"], "failed");
    e.mail.fail_next(0);
}

#[test]
fn bad_plans_are_refused_and_creation_is_limited() {
    let e = env();
    let ok = plan(e.clock.now() + Duration::hours(2));
    for (i, (field, value)) in [
        ("contacts", json!([])),
        ("contacts", json!(["a@b.fr", "c@d.fr", "e@f.fr", "g@h.fr"])),
        ("contacts", json!(["pas une adresse"])),
        ("contacts", json!(["x@y.fr\r\nBcc: z@w.fr"])),
        (
            "deadline",
            json!((e.clock.now() - Duration::minutes(1)).to_rfc3339()),
        ),
        (
            "deadline",
            json!((e.clock.now() + Duration::days(8)).to_rfc3339()),
        ),
        ("grace_minutes", json!(500)),
        ("skier_name", json!("")),
        ("skier_phone", json!("06 <script>")),
        ("site", json!("nowhere")),
        ("route", json!([[200.0, 44.0]])),
    ]
    .into_iter()
    .enumerate()
    {
        let mut p = ok.clone();
        p[field] = value.clone();
        let ip = format!("198.51.100.{i}");
        let r = request(
            e.addr,
            "POST",
            "/v1/trips",
            &[("CF-Connecting-IP", &ip)],
            p.to_string().as_bytes(),
        );
        assert_eq!(
            r.status,
            400,
            "{field} = {value}: {}",
            String::from_utf8_lossy(&r.body)
        );
    }
    let r = request(
        e.addr,
        "POST",
        "/v1/trips",
        &[("CF-Connecting-IP", "198.51.100.99")],
        b"{not json",
    );
    assert_eq!(r.status, 400);
    // 10 plans an hour per client.
    let from = |ip: &str| {
        request(
            e.addr,
            "POST",
            "/v1/trips",
            &[("CF-Connecting-IP", ip)],
            ok.to_string().as_bytes(),
        )
    };
    for _ in 0..10 {
        assert_eq!(from("192.0.2.1").status, 201);
    }
    let r = from("192.0.2.1");
    assert_eq!(r.status, 429);
    assert_eq!(r.header("retry-after"), Some("600"));
    assert_eq!(from("192.0.2.2").status, 201);
    e.clock.advance(Duration::minutes(61));
    let ok = plan(e.clock.now() + Duration::hours(2));
    let r = request(
        e.addr,
        "POST",
        "/v1/trips",
        &[("CF-Connecting-IP", "192.0.2.1")],
        ok.to_string().as_bytes(),
    );
    assert_eq!(r.status, 201);
}
