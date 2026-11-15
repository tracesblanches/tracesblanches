//! Downloader tests against a local HTTP server (P1, #14): no redundant request, rate
//! limiting, conditional requests, retries, secrets kept out of the cache.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use skimap_common::config::{Config, RateLimit, Secret};
use skimap_common::download::{CachePolicy, Downloader, Origin, Request, SourcePolicy, USER_AGENT};
use skimap_common::error::Error;

/// One request seen by the server.
#[derive(Debug, Clone)]
struct Seen {
    at: Instant,
    method: String,
    url: String,
    headers: HashMap<String, String>,
    body: String,
}

impl Seen {
    fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .get(&name.to_ascii_lowercase())
            .map(String::as_str)
    }
}

/// What the handler answers.
struct Reply {
    status: u16,
    body: Vec<u8>,
    headers: Vec<(&'static str, String)>,
}

impl Reply {
    fn ok(body: &str) -> Self {
        Self {
            status: 200,
            body: body.as_bytes().to_vec(),
            headers: Vec::new(),
        }
    }
    fn status(status: u16) -> Self {
        Self {
            status,
            body: Vec::new(),
            headers: Vec::new(),
        }
    }
    fn with(mut self, name: &'static str, value: impl Into<String>) -> Self {
        self.headers.push((name, value.into()));
        self
    }
}

type Handler = Box<dyn Fn(&Seen, usize) -> Reply + Send + Sync>;

/// Local HTTP server recording every request.
struct TestServer {
    base: String,
    seen: Arc<Mutex<Vec<Seen>>>,
    server: Arc<tiny_http::Server>,
    thread: Option<thread::JoinHandle<()>>,
}

impl TestServer {
    fn start(handler: impl Fn(&Seen, usize) -> Reply + Send + Sync + 'static) -> Self {
        let handler: Handler = Box::new(handler);
        let server = Arc::new(tiny_http::Server::http("127.0.0.1:0").unwrap());
        let port = server.server_addr().to_ip().unwrap().port();
        let seen = Arc::new(Mutex::new(Vec::new()));
        let (srv, log) = (server.clone(), seen.clone());
        let thread = thread::spawn(move || {
            for mut rq in srv.incoming_requests() {
                let mut body = String::new();
                let _ = rq.as_reader().read_to_string(&mut body);
                let s = Seen {
                    at: Instant::now(),
                    method: rq.method().as_str().to_owned(),
                    url: rq.url().to_owned(),
                    headers: rq
                        .headers()
                        .iter()
                        .map(|h| {
                            (
                                h.field.as_str().as_str().to_ascii_lowercase(),
                                h.value.as_str().to_owned(),
                            )
                        })
                        .collect(),
                    body,
                };
                let n = {
                    let mut l = log.lock().unwrap();
                    l.push(s.clone());
                    l.len()
                };
                let r = handler(&s, n);
                let mut resp = tiny_http::Response::from_data(r.body).with_status_code(r.status);
                for (k, v) in r.headers {
                    resp.add_header(
                        tiny_http::Header::from_bytes(k.as_bytes(), v.as_bytes()).unwrap(),
                    );
                }
                let _ = rq.respond(resp);
            }
        });
        Self {
            base: format!("http://127.0.0.1:{port}"),
            seen,
            server,
            thread: Some(thread),
        }
    }

    fn url(&self, path: &str) -> String {
        format!("{}{path}", self.base)
    }

    fn seen(&self) -> Vec<Seen> {
        self.seen.lock().unwrap().clone()
    }

    fn count(&self) -> usize {
        self.seen.lock().unwrap().len()
    }
}

impl Drop for TestServer {
    fn drop(&mut self) {
        self.server.unblock();
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

/// Test policy: no rate limit, no retry, one hour of cache.
fn policy(name: &str) -> SourcePolicy {
    SourcePolicy::new(name)
        .limits(Vec::new())
        .max_age(Duration::from_secs(3600))
        .max_wait(Duration::from_secs(5))
        .retries(0, Duration::from_millis(10), Duration::from_millis(100))
}

fn downloader(dir: &std::path::Path, p: SourcePolicy) -> Downloader {
    Downloader::builder(dir).policy(p).build().unwrap()
}

#[test]
fn fresh_cache_sends_no_request() {
    let srv = TestServer::start(|_, _| Reply::ok("hello").with("ETag", "\"v1\""));
    let dir = tempfile::tempdir().unwrap();
    let dl = downloader(dir.path(), policy("t"));
    let req = Request::get("t", &srv.url("/a"));

    let f1 = dl.fetch(&req).unwrap();
    assert_eq!(f1.origin, Origin::Downloaded);
    assert_eq!(f1.text().unwrap(), "hello");
    assert_eq!(f1.meta.etag.as_deref(), Some("\"v1\""));
    assert!(f1.path.starts_with(dir.path().join("t")));

    let f2 = dl.fetch(&req).unwrap();
    assert_eq!(f2.origin, Origin::Cache);
    assert_eq!(f2.text().unwrap(), "hello");

    // A new process (new downloader on the same cache) does not ask again either.
    let dl2 = downloader(dir.path(), policy("t"));
    assert_eq!(dl2.fetch(&req).unwrap().origin, Origin::Cache);

    assert_eq!(srv.count(), 1, "only the first fetch reaches the server");
    let seen = srv.seen();
    assert_eq!(seen[0].header("user-agent"), Some(USER_AGENT));
    assert_eq!(seen[0].method, "GET");
    assert_eq!(seen[0].url, "/a");
}

#[test]
fn concurrent_fetches_of_one_url_send_one_request() {
    let srv = TestServer::start(|_, _| {
        thread::sleep(Duration::from_millis(100));
        Reply::ok("slow")
    });
    let dir = tempfile::tempdir().unwrap();
    let dl = downloader(dir.path(), policy("t"));
    let url = srv.url("/same");
    let origins: Vec<Origin> = thread::scope(|s| {
        let hs: Vec<_> = (0..4)
            .map(|_| s.spawn(|| dl.fetch(&Request::get("t", &url)).unwrap().origin))
            .collect();
        hs.into_iter().map(|h| h.join().unwrap()).collect()
    });
    assert_eq!(srv.count(), 1);
    assert_eq!(
        origins.iter().filter(|o| **o == Origin::Downloaded).count(),
        1
    );
    assert_eq!(origins.iter().filter(|o| **o == Origin::Cache).count(), 3);
}

/// Server answering from `workers` threads after `delay`, recording the largest number of
/// requests it held at once.
fn parallel_server(workers: usize, delay: Duration) -> (String, Arc<Mutex<(usize, usize)>>) {
    let server = Arc::new(tiny_http::Server::http("127.0.0.1:0").unwrap());
    let port = server.server_addr().to_ip().unwrap().port();
    let gauge = Arc::new(Mutex::new((0usize, 0usize)));
    for _ in 0..workers {
        let (srv, g) = (server.clone(), gauge.clone());
        thread::spawn(move || {
            while let Ok(rq) = srv.recv() {
                {
                    let mut g = g.lock().unwrap();
                    g.0 += 1;
                    g.1 = g.1.max(g.0);
                }
                thread::sleep(delay);
                g.lock().unwrap().0 -= 1;
                let _ = rq.respond(tiny_http::Response::from_string("ok"));
            }
        });
    }
    (format!("http://127.0.0.1:{port}"), gauge)
}

#[test]
fn in_flight_requests_are_bounded_per_source() {
    let delay = Duration::from_millis(150);
    for (allowed, expect_max) in [(1, 1), (4, 4)] {
        let (base, gauge) = parallel_server(8, delay);
        let dir = tempfile::tempdir().unwrap();
        let dl = downloader(dir.path(), policy("t").max_in_flight(allowed));
        let t0 = Instant::now();
        thread::scope(|s| {
            for i in 0..8 {
                let (dl, url) = (&dl, format!("{base}/{i}"));
                s.spawn(move || dl.fetch(&Request::get("t", &url)).unwrap());
            }
        });
        let took = t0.elapsed();
        assert_eq!(
            gauge.lock().unwrap().1,
            expect_max,
            "max_in_flight {allowed}"
        );
        // 8 requests of 150 ms: 1.2 s one at a time, 0.3 s four at a time.
        let rounds = 8 / expect_max as u32;
        assert!(took >= delay * rounds, "{took:?}");
        assert!(
            took < delay * rounds + Duration::from_millis(600),
            "{took:?}"
        );
    }
}

#[test]
fn rate_limit_holds_with_several_requests_in_flight() {
    // 4 in flight but 5 requests per second: 10 requests need about 1 s.
    let (base, _) = parallel_server(8, Duration::from_millis(20));
    let dir = tempfile::tempdir().unwrap();
    let p = policy("t")
        .max_in_flight(4)
        .limits([RateLimit::new(5, Duration::from_secs(1))]);
    let dl = downloader(dir.path(), p);
    let t0 = Instant::now();
    thread::scope(|s| {
        for i in 0..10 {
            let (dl, url) = (&dl, format!("{base}/{i}"));
            s.spawn(move || dl.fetch(&Request::get("t", &url)).unwrap());
        }
    });
    assert!(
        t0.elapsed() >= Duration::from_millis(1000),
        "{:?}",
        t0.elapsed()
    );
}

#[test]
fn etag_revalidation_uses_304() {
    let version = Arc::new(Mutex::new("v1"));
    let v = version.clone();
    let srv = TestServer::start(move |s, _| {
        let cur = *v.lock().unwrap();
        let etag = format!("\"{cur}\"");
        if s.header("if-none-match") == Some(etag.as_str()) {
            Reply::status(304).with("ETag", etag)
        } else {
            Reply::ok(&format!("body {cur}")).with("ETag", etag)
        }
    });
    let dir = tempfile::tempdir().unwrap();
    let dl = downloader(dir.path(), policy("t").max_age(Duration::ZERO));
    let req = Request::get("t", &srv.url("/e"));

    assert_eq!(dl.fetch(&req).unwrap().origin, Origin::Downloaded);
    let f = dl.fetch(&req).unwrap();
    assert_eq!(f.origin, Origin::NotModified);
    assert_eq!(f.text().unwrap(), "body v1");

    *version.lock().unwrap() = "v2";
    let f = dl.fetch(&req).unwrap();
    assert_eq!(f.origin, Origin::Downloaded);
    assert_eq!(f.text().unwrap(), "body v2");
    assert_eq!(f.meta.etag.as_deref(), Some("\"v2\""));

    let seen = srv.seen();
    assert_eq!(seen.len(), 3);
    assert_eq!(seen[0].header("if-none-match"), None);
    assert_eq!(seen[1].header("if-none-match"), Some("\"v1\""));
    assert_eq!(seen[2].header("if-none-match"), Some("\"v1\""));
}

#[test]
fn last_modified_revalidation_uses_if_modified_since() {
    const LM: &str = "Wed, 11 Nov 2026 08:00:00 GMT";
    let srv = TestServer::start(|s, _| {
        if s.header("if-modified-since") == Some(LM) {
            Reply::status(304)
        } else {
            Reply::ok("dated").with("Last-Modified", LM)
        }
    });
    let dir = tempfile::tempdir().unwrap();
    let dl = downloader(dir.path(), policy("t"));
    let req = Request::get("t", &srv.url("/lm"));
    assert_eq!(dl.fetch(&req).unwrap().origin, Origin::Downloaded);
    // Revalidate ignores max_age but stays conditional.
    let f = dl
        .fetch(&req.clone().cache(CachePolicy::Revalidate))
        .unwrap();
    assert_eq!(f.origin, Origin::NotModified);
    assert_eq!(f.text().unwrap(), "dated");
    // Reload is unconditional.
    let f = dl.fetch(&req.clone().cache(CachePolicy::Reload)).unwrap();
    assert_eq!(f.origin, Origin::Downloaded);
    let seen = srv.seen();
    assert_eq!(seen.len(), 3);
    assert_eq!(seen[1].header("if-modified-since"), Some(LM));
    assert_eq!(seen[2].header("if-modified-since"), None);
}

#[test]
fn offline_mode_never_touches_the_network() {
    let srv = TestServer::start(|_, _| Reply::ok("x"));
    let dir = tempfile::tempdir().unwrap();
    let dl = downloader(dir.path(), policy("t"));
    let req = Request::get("t", &srv.url("/o"));
    let err = dl
        .fetch(&req.clone().cache(CachePolicy::Offline))
        .unwrap_err();
    assert!(matches!(err, Error::NotCached { .. }), "{err}");
    assert_eq!(srv.count(), 0);

    dl.fetch(&req).unwrap();
    let off = Downloader::builder(dir.path())
        .policy(policy("t").max_age(Duration::ZERO))
        .offline(true)
        .build()
        .unwrap();
    // Even stale, the entry is served without a request.
    assert_eq!(off.fetch(&req).unwrap().origin, Origin::Cache);
    assert_eq!(srv.count(), 1);
}

#[test]
fn rate_limit_spaces_requests() {
    let srv = TestServer::start(|_, _| Reply::ok("r"));
    let dir = tempfile::tempdir().unwrap();
    let window = Duration::from_millis(300);
    let dl = downloader(dir.path(), policy("t").limits([RateLimit::new(2, window)]));
    let start = Instant::now();
    for i in 0..5 {
        dl.fetch(&Request::get("t", &srv.url(&format!("/r{i}"))))
            .unwrap();
    }
    let elapsed = start.elapsed();
    // Requests at 0, 0, 300, 300, 600 ms.
    assert!(elapsed >= 2 * window, "elapsed {elapsed:?}");
    let seen = srv.seen();
    assert_eq!(seen.len(), 5);
    let tolerance = Duration::from_millis(50);
    for w in seen.windows(3) {
        let gap = w[2].at.duration_since(w[0].at);
        assert!(gap + tolerance >= window, "3 requests within {gap:?}");
    }
}

#[test]
fn cache_hits_do_not_consume_the_quota() {
    let srv = TestServer::start(|_, _| Reply::ok("q"));
    let dir = tempfile::tempdir().unwrap();
    let p = policy("t")
        .limits([RateLimit::per_hour(1)])
        .max_wait(Duration::from_millis(100));
    let dl = downloader(dir.path(), p);
    let req = Request::get("t", &srv.url("/q"));
    for _ in 0..10 {
        dl.fetch(&req).unwrap();
    }
    assert_eq!(srv.count(), 1);

    // A second URL would exceed the quota: fail fast, no request sent.
    let start = Instant::now();
    let err = dl.fetch(&Request::get("t", &srv.url("/q2"))).unwrap_err();
    match err {
        Error::RateLimited { source_name, wait } => {
            assert_eq!(source_name, "t");
            assert!(wait > Duration::from_secs(3500), "{wait:?}");
        }
        e => panic!("unexpected {e}"),
    }
    assert!(start.elapsed() < Duration::from_millis(100));
    assert_eq!(srv.count(), 1);
}

#[test]
fn rate_limit_state_survives_a_restart() {
    let srv = TestServer::start(|_, _| Reply::ok("p"));
    let dir = tempfile::tempdir().unwrap();
    let p = policy("t").limits([RateLimit::new(1, Duration::from_millis(500))]);
    let start = Instant::now();
    downloader(dir.path(), p.clone())
        .fetch(&Request::get("t", &srv.url("/p1")))
        .unwrap();
    assert!(dir.path().join("t/ratelimit.json").exists());
    downloader(dir.path(), p)
        .fetch(&Request::get("t", &srv.url("/p2")))
        .unwrap();
    assert!(
        start.elapsed() >= Duration::from_millis(500),
        "{:?}",
        start.elapsed()
    );
    let seen = srv.seen();
    let spacing = seen[1].at.duration_since(seen[0].at);
    assert!(
        spacing + Duration::from_millis(50) >= Duration::from_millis(500),
        "requests {spacing:?} apart at the server"
    );
}

#[test]
fn retries_with_backoff_on_5xx() {
    let srv = TestServer::start(|_, n| {
        if n <= 2 {
            Reply::status(503)
        } else {
            Reply::ok("finally")
        }
    });
    let dir = tempfile::tempdir().unwrap();
    let base = Duration::from_millis(40);
    let dl = downloader(
        dir.path(),
        policy("t").retries(3, base, Duration::from_secs(1)),
    );
    let start = Instant::now();
    let f = dl.fetch(&Request::get("t", &srv.url("/flaky"))).unwrap();
    assert_eq!(f.text().unwrap(), "finally");
    assert_eq!(srv.count(), 3);
    // Backoff 40 ms then 80 ms.
    assert!(start.elapsed() >= base * 3, "{:?}", start.elapsed());
    let seen = srv.seen();
    assert!(seen[2].at.duration_since(seen[1].at) >= base * 2 - Duration::from_millis(5));
}

#[test]
fn retries_are_bounded() {
    let srv = TestServer::start(|_, _| Reply::status(500));
    let dir = tempfile::tempdir().unwrap();
    let dl = downloader(
        dir.path(),
        policy("t").retries(2, Duration::from_millis(5), Duration::from_millis(20)),
    );
    let err = dl.fetch(&Request::get("t", &srv.url("/down"))).unwrap_err();
    assert!(matches!(err, Error::Status { status: 500, .. }), "{err}");
    assert_eq!(srv.count(), 3);
}

#[test]
fn retry_after_is_honoured() {
    let srv = TestServer::start(|_, n| {
        if n == 1 {
            Reply::status(429).with("Retry-After", "1")
        } else {
            Reply::ok("ok")
        }
    });
    let dir = tempfile::tempdir().unwrap();
    let dl = downloader(
        dir.path(),
        policy("t").retries(2, Duration::from_millis(10), Duration::from_secs(2)),
    );
    let start = Instant::now();
    dl.fetch(&Request::get("t", &srv.url("/busy"))).unwrap();
    assert!(
        start.elapsed() >= Duration::from_secs(1),
        "{:?}",
        start.elapsed()
    );
    assert_eq!(srv.count(), 2);
}

#[test]
fn long_retry_after_fails_fast() {
    let srv = TestServer::start(|_, _| Reply::status(429).with("Retry-After", "3600"));
    let dir = tempfile::tempdir().unwrap();
    let dl = downloader(
        dir.path(),
        policy("t")
            .retries(5, Duration::from_millis(10), Duration::from_secs(1))
            .max_wait(Duration::from_secs(1)),
    );
    let start = Instant::now();
    let err = dl
        .fetch(&Request::get("t", &srv.url("/later")))
        .unwrap_err();
    assert!(
        matches!(err, Error::RateLimited { wait, .. } if wait == Duration::from_secs(3600)),
        "{err}"
    );
    assert!(start.elapsed() < Duration::from_millis(500));
    assert_eq!(srv.count(), 1);
}

#[test]
fn client_errors_are_not_retried_nor_cached() {
    let srv = TestServer::start(|_, _| Reply::status(404));
    let dir = tempfile::tempdir().unwrap();
    let dl = downloader(
        dir.path(),
        policy("t").retries(3, Duration::from_millis(5), Duration::from_millis(5)),
    );
    let req = Request::get("t", &srv.url("/missing"));
    let err = dl.fetch(&req).unwrap_err();
    assert!(matches!(err, Error::Status { status: 404, .. }), "{err}");
    assert_eq!(srv.count(), 1);
    let err = dl.fetch(&req.cache(CachePolicy::Offline)).unwrap_err();
    assert!(matches!(err, Error::NotCached { .. }));
}

#[test]
fn transport_errors_are_retried_then_reported() {
    // A port that was free a moment ago: connection refused.
    let port = std::net::TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port();
    let dir = tempfile::tempdir().unwrap();
    let dl = downloader(
        dir.path(),
        policy("t").retries(1, Duration::from_millis(5), Duration::from_millis(5)),
    );
    let err = dl
        .fetch(&Request::get("t", &format!("http://127.0.0.1:{port}/x")))
        .unwrap_err();
    assert!(matches!(err, Error::Http { .. }), "{err}");
}

#[test]
fn secrets_reach_the_server_but_never_the_cache_or_errors() {
    const KEY: &str = "S3CRET-KEY-42";
    let srv = TestServer::start(|s, _| {
        if s.url.ends_with("/bad?apikey=S3CRET-KEY-42") {
            Reply::status(403)
        } else {
            Reply::ok("data")
        }
    });
    let dir = tempfile::tempdir().unwrap();
    let dl = downloader(dir.path(), policy("t"));
    let key = Secret::new(KEY);
    let req = Request::get("t", &srv.url("/api?x=1"))
        .secret_query("apikey", &key)
        .secret_header("apikey", &key);
    let f = dl.fetch(&req).unwrap();
    assert_eq!(f.meta.url, srv.url("/api?x=1"));

    let err = dl
        .fetch(&Request::get("t", &srv.url("/bad")).secret_query("apikey", &key))
        .unwrap_err();
    assert!(matches!(err, Error::Status { status: 403, .. }));
    assert!(!err.to_string().contains(KEY), "{err}");
    assert!(!format!("{err:?} {req:?}").contains(KEY));

    let seen = srv.seen();
    assert_eq!(seen[0].url, "/api?x=1&apikey=S3CRET-KEY-42");
    assert_eq!(seen[0].header("apikey"), Some(KEY));

    for entry in std::fs::read_dir(dir.path().join("t")).unwrap() {
        let path = entry.unwrap().path();
        let name = path.file_name().unwrap().to_string_lossy().into_owned();
        assert!(!name.contains(KEY));
        let content = std::fs::read(&path).unwrap();
        assert!(
            !String::from_utf8_lossy(&content).contains(KEY),
            "secret in {name}"
        );
    }
}

#[test]
fn post_bodies_are_part_of_the_cache_key() {
    let srv = TestServer::start(|s, _| Reply::ok(&format!("answer to {}", s.body)));
    let dir = tempfile::tempdir().unwrap();
    let dl = downloader(dir.path(), policy("overpass-test"));
    let url = srv.url("/api/interpreter");
    let q1 = Request::post_form("overpass-test", &url, &[("data", "node(1);out;")]);
    let q2 = Request::post_form("overpass-test", &url, &[("data", "node(2);out;")]);
    assert_eq!(
        dl.fetch(&q1).unwrap().text().unwrap(),
        "answer to data=node%281%29%3Bout%3B"
    );
    assert_eq!(dl.fetch(&q2).unwrap().origin, Origin::Downloaded);
    let again = dl.fetch(&q1).unwrap();
    assert_eq!(again.origin, Origin::Cache);
    assert_eq!(again.text().unwrap(), "answer to data=node%281%29%3Bout%3B");
    let again = dl.fetch(&q2).unwrap();
    assert_eq!(again.origin, Origin::Cache);
    assert_eq!(again.text().unwrap(), "answer to data=node%282%29%3Bout%3B");
    // Same bytes under another content type: another request, another entry.
    let raw = Request::post(
        "overpass-test",
        &url,
        "data=node%281%29%3Bout%3B",
        "text/plain",
    );
    assert_eq!(dl.fetch(&raw).unwrap().origin, Origin::Downloaded);
    assert_eq!(dl.fetch(&raw).unwrap().origin, Origin::Cache);
    let seen = srv.seen();
    assert_eq!(seen.len(), 3);
    assert_eq!(seen[2].header("content-type"), Some("text/plain"));
    assert_eq!(seen[0].method, "POST");
    assert_eq!(
        seen[0].header("content-type"),
        Some("application/x-www-form-urlencoded")
    );
}

#[test]
fn rate_limit_overrides_from_config() {
    let cfg = Config::from_vars([
        ("SKIMAP_DATA_DIR", "/nonexistent"),
        ("SKIMAP_RATE_LIMIT_DATATOURISME", "10/h"),
        ("SKIMAP_RATE_LIMIT_NEW_SOURCE", "1/5s"),
    ])
    .unwrap();
    let dl = Downloader::from_config(&cfg).unwrap();
    assert_eq!(dl.cache_root(), std::path::Path::new("/nonexistent/cache"));
    assert_eq!(
        dl.policy("datatourisme").limits,
        vec![RateLimit::per_hour(10)]
    );
    assert_eq!(
        dl.policy("new-source").limits,
        vec![RateLimit::new(1, Duration::from_secs(5))]
    );
    // Untouched known source keeps its documented quota.
    assert!(
        dl.policy("skitour")
            .limits
            .contains(&RateLimit::per_day(1000))
    );
}

#[test]
fn personal_sources_are_refused_unless_enabled() {
    let srv = TestServer::start(|_, _| Reply::ok("widget"));
    let dir = tempfile::tempdir().unwrap();
    let data = dir.path().to_str().unwrap();
    let url = srv.url("/station/3167");
    // Default configuration: refused before any request, even with a cached answer.
    let off = Config::from_vars([("SKIMAP_DATA_DIR", data)]).unwrap();
    let on =
        Config::from_vars([("SKIMAP_DATA_DIR", data), ("SKIMAP_PERSONAL_SOURCES", "1")]).unwrap();
    for source in [
        "nordicfrance",
        "enj",
        "cervieres-mairie",
        "open-meteo",
        "biodiv-sports",
        "geoide",
    ] {
        let dl = Downloader::from_config(&off).unwrap();
        let e = dl.fetch(&Request::get(source, &url)).unwrap_err();
        assert!(
            matches!(&e, Error::PersonalSourceRefused(s) if s == source),
            "{e}"
        );
        assert!(e.to_string().contains("personal use only"), "{e}");
    }
    assert_eq!(srv.count(), 0);
    let dl = Downloader::from_config(&on).unwrap();
    assert_eq!(
        dl.fetch(&Request::get("nordicfrance", &url))
            .unwrap()
            .origin,
        Origin::Downloaded
    );
    assert_eq!(srv.count(), 1);
    // Cached now; still refused without the option, cache included.
    let dl = Downloader::from_config(&off).unwrap();
    let cached = Request::get("nordicfrance", &url).cache(CachePolicy::Offline);
    assert!(matches!(
        dl.fetch(&cached),
        Err(Error::PersonalSourceRefused(_))
    ));
    // Other sources are unaffected.
    let dl = Downloader::from_config(&off).unwrap();
    dl.fetch(&Request::get("geo-api", &srv.url("/communes")))
        .unwrap();
    // A builder without the option refuses too; with it, allowed.
    let dl = Downloader::builder(dir.path().join("b")).build().unwrap();
    assert!(dl.fetch(&Request::get("enj", &url)).is_err());
    let dl = Downloader::builder(dir.path().join("b"))
        .personal_sources(true)
        .build()
        .unwrap();
    dl.fetch(&Request::get("enj", &url)).unwrap();
}

#[test]
fn invalid_source_names_are_rejected() {
    let dir = tempfile::tempdir().unwrap();
    let dl = downloader(dir.path(), policy("t"));
    let err = dl
        .fetch(&Request::get("../etc", "http://127.0.0.1:1/"))
        .unwrap_err();
    assert!(matches!(err, Error::Config(_)), "{err}");
}

/// Serves `data` with `Range: bytes=a-b` support (206) when `honour`, else whole (200).
fn range_server(data: &'static [u8], honour: bool) -> TestServer {
    TestServer::start(move |s, _| match s.header("range").filter(|_| honour) {
        Some(r) => {
            let (a, b) = r.trim_start_matches("bytes=").split_once('-').unwrap();
            let (a, b): (usize, usize) = (a.parse().unwrap(), b.parse().unwrap());
            let b = b.min(data.len() - 1);
            Reply {
                status: 206,
                body: data[a..=b].to_vec(),
                headers: vec![("Content-Range", format!("bytes {a}-{b}/{}", data.len()))],
            }
        }
        None => Reply {
            status: 200,
            body: data.to_vec(),
            headers: Vec::new(),
        },
    })
}

#[test]
fn range_requests_are_cached_apart() {
    let srv = range_server(b"0123456789abcdefghij", true);
    let dir = tempfile::tempdir().unwrap();
    let dl = downloader(dir.path(), policy("t"));
    let url = srv.url("/archive.pmtiles");
    let head = dl.fetch(&Request::get("t", &url).range(0, 4)).unwrap();
    let tail = dl.fetch(&Request::get("t", &url).range(10, 5)).unwrap();
    let whole = dl.fetch(&Request::get("t", &url)).unwrap();
    assert_eq!(head.bytes().unwrap(), b"0123");
    assert_eq!(tail.bytes().unwrap(), b"abcde");
    assert_eq!(whole.bytes().unwrap(), b"0123456789abcdefghij");
    assert_eq!(head.meta.range.as_deref(), Some("bytes=0-3"));
    assert_eq!(whole.meta.range, None);
    assert_ne!(head.path, tail.path);
    assert_eq!(srv.count(), 3);
    // Second time: all from the cache, each range with its own body.
    let again = dl.fetch(&Request::get("t", &url).range(10, 5)).unwrap();
    assert_eq!(again.origin, Origin::Cache);
    assert_eq!(again.bytes().unwrap(), b"abcde");
    let head = dl.fetch(&Request::get("t", &url).range(0, 4)).unwrap();
    assert_eq!(head.bytes().unwrap(), b"0123");
    assert_eq!(srv.count(), 3);
    let seen = srv.seen();
    assert_eq!(seen[0].header("range"), Some("bytes=0-3"));
    assert_eq!(seen[2].header("range"), None);
}

#[test]
fn range_ignored_by_the_server_is_an_error() {
    let srv = range_server(b"0123456789", false);
    let dir = tempfile::tempdir().unwrap();
    let dl = downloader(dir.path(), policy("t"));
    let req = Request::get("t", &srv.url("/f")).range(2, 3);
    let err = dl.fetch(&req).unwrap_err();
    assert!(err.to_string().contains("ignores Range"), "{err}");
    // Nothing cached under the key of the range.
    assert!(matches!(
        dl.fetch(&req.cache(CachePolicy::Offline)),
        Err(Error::NotCached { .. })
    ));
}
