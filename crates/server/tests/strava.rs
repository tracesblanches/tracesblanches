//! Strava OAuth exchange (S1) against a fake Strava: the app sends only the code or the
//! refresh token, the server adds the client secret, Strava's answer comes back as is.

mod common;

use std::sync::{Arc, Mutex};

use common::{get, request};
use serde_json::json;
use skimap_server::app::App;
use skimap_server::clock::SystemClock;
use skimap_server::store::Store;
use skimap_server::strava::{Strava, StravaConfig};

const SECRET: &str = "s3cr3t-of-the-strava-app";

#[derive(Debug, Clone)]
struct Call {
    path: String,
    body: String,
    authorization: Option<String>,
}

/// A fake `www.strava.com`: `/oauth/token` and `/oauth/revoke`.
fn fake_strava() -> (String, Arc<Mutex<Vec<Call>>>) {
    let server = tiny_http::Server::http("127.0.0.1:0").unwrap();
    let base = format!("http://{}", server.server_addr().to_ip().unwrap());
    let calls = Arc::new(Mutex::new(Vec::new()));
    let c2 = calls.clone();
    std::thread::spawn(move || {
        for mut rq in server.incoming_requests() {
            let mut body = String::new();
            rq.as_reader().read_to_string(&mut body).unwrap();
            let auth = rq
                .headers()
                .iter()
                .find(|h| h.field.equiv("Authorization"))
                .map(|h| h.value.as_str().to_owned());
            let path = rq.url().to_owned();
            c2.lock().unwrap().push(Call {
                path: path.clone(),
                body: body.clone(),
                authorization: auth.clone(),
            });
            let has = |kv: &str| body.split('&').any(|p| p == kv);
            let (status, out) = match path.as_str() {
                "/oauth/token" if !has(&format!("client_secret={SECRET}")) => {
                    (401, json!({"message": "Authorization Error"}))
                }
                "/oauth/token" if has("grant_type=authorization_code") && has("code=good-code") => {
                    (
                        200,
                        json!({"token_type": "Bearer", "expires_at": 1_800_000_000, "expires_in": 21600,
                           "refresh_token": "r1", "access_token": "a1",
                           "athlete": {"id": 42, "firstname": "Marie"}}),
                    )
                }
                "/oauth/token" if has("grant_type=authorization_code") => (
                    400,
                    json!({"message": "Bad Request", "errors": [{"resource": "AuthorizationCode", "field": "code", "code": "invalid"}]}),
                ),
                "/oauth/token" if has("grant_type=refresh_token") && has("refresh_token=r1") => (
                    200,
                    json!({"token_type": "Bearer", "access_token": "a2", "expires_at": 1_800_021_600,
                           "expires_in": 21600, "refresh_token": "r2"}),
                ),
                "/oauth/revoke" => {
                    let expected = format!(
                        "Basic {}",
                        skimap_server::util::base64(format!("12345:{SECRET}").as_bytes())
                    );
                    if auth.as_deref() == Some(expected.as_str()) && has("token=r2") {
                        (200, json!(null))
                    } else {
                        (401, json!({"message": "Unauthorized"}))
                    }
                }
                _ => (404, json!({"message": "Record Not Found"})),
            };
            let body = if out.is_null() {
                String::new()
            } else {
                out.to_string()
            };
            let _ = rq.respond(
                tiny_http::Response::from_string(body)
                    .with_status_code(status)
                    .with_header(
                        tiny_http::Header::from_bytes(
                            &b"Content-Type"[..],
                            &b"application/json; charset=utf-8"[..],
                        )
                        .unwrap(),
                    ),
            );
        }
    });
    (base, calls)
}

fn app(strava_base: Option<&str>) -> (skimap_server::http::Running, tempfile::TempDir) {
    let dir = tempfile::tempdir().unwrap();
    let strava = strava_base.map(|b| {
        let cfg = StravaConfig {
            client_id: "12345".into(),
            client_secret: SECRET.into(),
            base_url: b.into(),
            scope: "activity:write".into(),
            app_redirect: "skimap://strava/callback".into(),
            public_url: "https://skimap.test".into(),
        };
        (Strava::new(cfg.clone()).unwrap(), cfg)
    });
    let app = App::new(
        Store::open(dir.path().join("store")).unwrap(),
        None,
        strava,
        "https://skimap.test",
        "contact@example.org",
        Arc::new(SystemClock),
    );
    (
        skimap_server::http::serve("127.0.0.1:0", 2, Arc::new(app)).unwrap(),
        dir,
    )
}

fn post(addr: std::net::SocketAddr, path: &str, body: serde_json::Value) -> common::Resp {
    request(
        addr,
        "POST",
        path,
        &[("Content-Type", "application/json")],
        body.to_string().as_bytes(),
    )
}

#[test]
fn exchange_refresh_revoke_through_the_server() {
    let (base, calls) = fake_strava();
    let (server, _d) = app(Some(&base));
    let addr = server.addr;

    let cfg = get(addr, "/v1/strava/config").json();
    assert_eq!(
        cfg,
        json!({"configured": true, "client_id": "12345",
               "authorize_url": "https://www.strava.com/oauth/mobile/authorize",
               "redirect_uri": "https://skimap.test/v1/strava/callback", "scope": "activity:write"})
    );

    // Strava sends the browser back to the server, which hands the code to the app.
    let r = get(
        addr,
        "/v1/strava/callback?state=k2&code=good-code&scope=read,activity:write",
    );
    assert_eq!(r.status, 302);
    assert_eq!(
        r.header("location"),
        Some("skimap://strava/callback?state=k2&code=good-code&scope=read%2Cactivity%3Awrite")
    );

    // Code → tokens.
    let r = post(addr, "/v1/strava/token", json!({"code": "good-code"}));
    assert_eq!(r.status, 200, "{}", String::from_utf8_lossy(&r.body));
    let t = r.json();
    assert_eq!(
        (t["access_token"].as_str(), t["refresh_token"].as_str()),
        (Some("a1"), Some("r1"))
    );
    assert_eq!(t["athlete"]["id"], 42);
    assert_eq!(r.header("cache-control"), Some("no-store"));
    let c = calls.lock().unwrap().last().cloned().unwrap();
    assert_eq!(c.path, "/oauth/token");
    assert_eq!(
        c.body,
        format!(
            "client_id=12345&client_secret={SECRET}&code=good-code&grant_type=authorization_code"
        )
    );

    // Strava's refusal is passed through.
    let r = post(addr, "/v1/strava/token", json!({"code": "expired"}));
    assert_eq!(r.status, 400);
    assert_eq!(r.json()["errors"][0]["code"], "invalid");

    // Refresh.
    let r = post(addr, "/v1/strava/refresh", json!({"refresh_token": "r1"}));
    assert_eq!(r.status, 200);
    assert_eq!(r.json()["access_token"], "a2");

    // Revoke: HTTP Basic with id and secret.
    let r = post(
        addr,
        "/v1/strava/revoke",
        json!({"token": "r2", "token_type_hint": "refresh_token"}),
    );
    assert_eq!(r.status, 200);
    let c = calls.lock().unwrap().last().cloned().unwrap();
    assert_eq!(c.path, "/oauth/revoke");
    assert_eq!(c.body, "token=r2&token_type_hint=refresh_token");
    assert!(c.authorization.unwrap().starts_with("Basic "));

    // Bad input never reaches Strava.
    let before = calls.lock().unwrap().len();
    assert_eq!(
        post(
            addr,
            "/v1/strava/token",
            json!({"code": "a&client_secret=x"})
        )
        .status,
        400
    );
    assert_eq!(post(addr, "/v1/strava/token", json!({})).status, 400);
    assert_eq!(calls.lock().unwrap().len(), before);

    // The secret never comes back to a client.
    for path in ["/v1/strava/config", "/v1/catalog", "/privacy"] {
        assert!(!String::from_utf8_lossy(&get(addr, path).body).contains(SECRET));
    }
    server.stop();
}

#[test]
fn strava_down_or_not_configured() {
    // Nothing listens there.
    let free = std::net::TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap();
    let (server, _d) = app(Some(&format!("http://{free}")));
    let r = post(server.addr, "/v1/strava/token", json!({"code": "c"}));
    assert_eq!(
        (r.status, r.json()["error"].as_str()),
        (502, Some("strava_unreachable"))
    );
    server.stop();

    let (server, _d) = app(None);
    let cfg = get(server.addr, "/v1/strava/config").json();
    assert_eq!(cfg["configured"], false);
    assert_eq!(
        post(server.addr, "/v1/strava/token", json!({"code": "c"})).status,
        503
    );
    // No alert service either (serve without SMTP_HOST): plans are refused, not queued.
    let r = post(
        server.addr,
        "/v1/trips",
        json!({"contacts": ["a@example.org"]}),
    );
    assert_eq!(
        (r.status, r.json()["error"].as_str()),
        (503, Some("unavailable"))
    );
    assert_eq!(get(server.addr, "/healthz").status, 200);
    server.stop();
}
