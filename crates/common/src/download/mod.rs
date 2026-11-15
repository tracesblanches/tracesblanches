//! Shared downloader of the pipeline (P1, #14): disk cache, conditional requests
//! (`ETag` / `If-Modified-Since`), per-source rate limits persisted across runs, retries
//! with exponential backoff, a generic User-Agent.
//!
//! Model: **blocking** (`reqwest::blocking`). Requests to one source are serialised (one
//! in flight at a time, which also keeps us polite); different sources may be fetched in
//! parallel from different threads (`std::thread::scope`, rayon). See
//! `docs/architecture.md`.
//!
//! ```no_run
//! use skimap_common::config::Config;
//! use skimap_common::download::{Downloader, Request};
//!
//! let cfg = Config::from_env()?;
//! let dl = Downloader::from_config(&cfg)?;
//! let f = dl.fetch(&Request::get("geo-api", "https://geo.api.gouv.fr/communes/05027"))?;
//! println!("{} bytes from {:?}", f.meta.size, f.origin);
//! # Ok::<(), skimap_common::error::Error>(())
//! ```

mod cache;
mod ratelimit;
mod sources;

use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, SystemTime};

use reqwest::StatusCode;
use reqwest::header::{
    CONTENT_TYPE, ETAG, HeaderMap, IF_MODIFIED_SINCE, IF_NONE_MATCH, LAST_MODIFIED, RETRY_AFTER,
};

pub use cache::CacheMeta;
pub use ratelimit::{Clock, SystemClock};
pub use sources::{SourcePolicy, known_sources};

use crate::config::{Config, RateLimit, Secret, check_source_name};
use crate::error::{Error, Result};
use cache::Entry;
use ratelimit::{Limiter, unix_ms};

/// User-Agent sent with every request: generic, no personal data.
pub const USER_AGENT: &str = concat!(
    "skimap/",
    env!("CARGO_PKG_VERSION"),
    " (offline nordic ski map data pipeline)"
);

/// HTTP method.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Method {
    /// GET.
    Get,
    /// POST with a body (Overpass queries, search APIs).
    Post,
}

impl Method {
    fn as_str(self) -> &'static str {
        match self {
            Method::Get => "GET",
            Method::Post => "POST",
        }
    }
}

/// How a request uses the cache.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CachePolicy {
    /// Serve from cache while younger than `max_age`, then revalidate (conditional
    /// request if `ETag` / `Last-Modified` are known).
    #[default]
    Normal,
    /// Always ask the server, conditionally when possible.
    Revalidate,
    /// Ignore the cache and download again (unconditional request).
    Reload,
    /// Never touch the network: cached entry or [`Error::NotCached`].
    Offline,
}

/// Where a [`Fetched`] body came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Origin {
    /// Cache, still fresh: no request sent.
    Cache,
    /// Cache, confirmed by the server with `304 Not Modified`.
    NotModified,
    /// Downloaded now.
    Downloaded,
}

/// A request to one source.
#[derive(Debug, Clone)]
pub struct Request {
    source: String,
    method: Method,
    url: String,
    body: Option<Vec<u8>>,
    headers: Vec<(String, String)>,
    secret_headers: Vec<(String, Secret)>,
    secret_query: Vec<(String, Secret)>,
    cache: CachePolicy,
    max_age: Option<Duration>,
}

impl Request {
    fn new(source: &str, method: Method, url: &str) -> Self {
        Self {
            source: source.to_owned(),
            method,
            url: url.to_owned(),
            body: None,
            headers: Vec::new(),
            secret_headers: Vec::new(),
            secret_query: Vec::new(),
            cache: CachePolicy::Normal,
            max_age: None,
        }
    }

    /// GET `url` from `source`.
    pub fn get(source: &str, url: &str) -> Self {
        Self::new(source, Method::Get, url)
    }

    /// POST `body` with `content_type` to `url`.
    pub fn post(source: &str, url: &str, body: impl Into<Vec<u8>>, content_type: &str) -> Self {
        let mut r = Self::new(source, Method::Post, url);
        r.body = Some(body.into());
        r.headers
            .push(("content-type".to_owned(), content_type.to_owned()));
        r
    }

    /// POST an `application/x-www-form-urlencoded` body (e.g. Overpass `data=`).
    pub fn post_form(source: &str, url: &str, pairs: &[(&str, &str)]) -> Self {
        let body = pairs
            .iter()
            .map(|(k, v)| format!("{}={}", form_encode(k), form_encode(v)))
            .collect::<Vec<_>>()
            .join("&");
        Self::post(source, url, body, "application/x-www-form-urlencoded")
    }

    /// Adds a public header (not logged; part of the cache key only for `Range`, and
    /// `Content-Type` with a body).
    pub fn header(mut self, name: &str, value: &str) -> Self {
        self.headers.push((name.to_owned(), value.to_owned()));
        self
    }

    /// Asks for `len` bytes from `offset` (`Range: bytes=offset-(offset+len-1)`). Each
    /// range is its own cache entry; the server must answer `206 Partial Content`.
    pub fn range(self, offset: u64, len: u64) -> Self {
        let end = offset + len.max(1) - 1;
        self.header("Range", &format!("bytes={offset}-{end}"))
    }

    /// Value of the `Range` header, if any (last one wins, as for the request sent).
    pub fn range_header(&self) -> Option<&str> {
        self.header_value("range")
    }

    /// Value of a public header, if any (last one wins, as for the request sent).
    fn header_value(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .rev()
            .find(|(k, _)| k.eq_ignore_ascii_case(name))
            .map(|(_, v)| v.as_str())
    }

    /// Adds a secret header (API key); never logged, never cached.
    pub fn secret_header(mut self, name: &str, value: &Secret) -> Self {
        self.secret_headers.push((name.to_owned(), value.clone()));
        self
    }

    /// Adds a secret query parameter, appended only when sending: the cache key, the
    /// metadata, the logs and the errors use the URL without it.
    pub fn secret_query(mut self, name: &str, value: &Secret) -> Self {
        self.secret_query.push((name.to_owned(), value.clone()));
        self
    }

    /// Sets the cache policy.
    pub fn cache(mut self, policy: CachePolicy) -> Self {
        self.cache = policy;
        self
    }

    /// Overrides the cache lifetime of the source for this request.
    pub fn max_age(mut self, d: Duration) -> Self {
        self.max_age = Some(d);
        self
    }

    /// Source name.
    pub fn source(&self) -> &str {
        &self.source
    }

    /// Public URL (without secret query parameters).
    pub fn url(&self) -> &str {
        &self.url
    }

    fn send_url(&self) -> String {
        let mut url = self.url.clone();
        for (k, v) in &self.secret_query {
            url.push(if url.contains('?') { '&' } else { '?' });
            url.push_str(&form_encode(k));
            url.push('=');
            url.push_str(&form_encode(v.expose()));
        }
        url
    }
}

/// `application/x-www-form-urlencoded` encoding of one component.
fn form_encode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'*' | b'-' | b'.' | b'_' => {
                out.push(b as char)
            }
            b' ' => out.push('+'),
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

/// A body available on disk.
#[derive(Debug, Clone)]
pub struct Fetched {
    /// Path of the cached body (stable until the next download of the same request).
    pub path: PathBuf,
    /// Cache metadata.
    pub meta: CacheMeta,
    /// Where the body came from.
    pub origin: Origin,
}

impl Fetched {
    /// Reads the whole body.
    pub fn bytes(&self) -> Result<Vec<u8>> {
        std::fs::read(&self.path).map_err(|e| Error::io(&self.path, e))
    }

    /// Reads the body as UTF-8 text.
    pub fn text(&self) -> Result<String> {
        String::from_utf8(self.bytes()?).map_err(|e| {
            Error::io(
                &self.path,
                std::io::Error::new(std::io::ErrorKind::InvalidData, e),
            )
        })
    }

    /// Parses the body as JSON.
    pub fn json<T: serde::de::DeserializeOwned>(&self) -> Result<T> {
        serde_json::from_slice(&self.bytes()?).map_err(|e| Error::json(self.meta.url.clone(), e))
    }
}

/// Builder of a [`Downloader`].
pub struct DownloaderBuilder {
    cache_root: PathBuf,
    policies: Vec<SourcePolicy>,
    overrides: BTreeMap<String, Vec<RateLimit>>,
    clock: Arc<dyn Clock>,
    offline: bool,
}

impl DownloaderBuilder {
    /// Adds or replaces the policy of a source.
    pub fn policy(mut self, p: SourcePolicy) -> Self {
        self.policies.retain(|q| q.name != p.name);
        self.policies.push(p);
        self
    }

    /// Rate-limit overrides per source (from [`Config::rate_limits`]).
    pub fn rate_limit_overrides(mut self, o: BTreeMap<String, Vec<RateLimit>>) -> Self {
        self.overrides = o;
        self
    }

    /// Replaces the clock (tests).
    pub fn clock(mut self, clock: Arc<dyn Clock>) -> Self {
        self.clock = clock;
        self
    }

    /// Forces [`CachePolicy::Offline`] for every request.
    pub fn offline(mut self, offline: bool) -> Self {
        self.offline = offline;
        self
    }

    /// Builds the downloader and its HTTP client.
    pub fn build(self) -> Result<Downloader> {
        for p in &self.policies {
            check_source_name(&p.name)?;
        }
        let client = http_client()?;
        let mut policies: HashMap<String, SourcePolicy> = self
            .policies
            .into_iter()
            .map(|p| (p.name.clone(), p))
            .collect();
        for (name, limits) in self.overrides {
            check_source_name(&name)?;
            policies
                .entry(name.clone())
                .or_insert_with(|| SourcePolicy::new(name))
                .limits = limits;
        }
        Ok(Downloader {
            cache_root: self.cache_root,
            client,
            policies,
            states: Mutex::new(HashMap::new()),
            clock: self.clock,
            offline: self.offline,
        })
    }
}

fn http_client() -> Result<reqwest::blocking::Client> {
    let roots = rustls::RootCertStore {
        roots: webpki_roots::TLS_SERVER_ROOTS.to_vec(),
    };
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let mut tls = rustls::ClientConfig::builder_with_provider(provider)
        .with_safe_default_protocol_versions()
        .map_err(|e| Error::Config(format!("TLS setup: {e}")))?
        .with_root_certificates(roots)
        .with_no_client_auth();
    tls.alpn_protocols = vec![b"h2".to_vec(), b"http/1.1".to_vec()];
    reqwest::blocking::Client::builder()
        .user_agent(USER_AGENT)
        .tls_backend_preconfigured(tls)
        .connect_timeout(Duration::from_secs(30))
        .timeout(None)
        .build()
        .map_err(|e| Error::Config(format!("HTTP client setup: {}", e.without_url())))
}

/// The shared downloader. Cheap to share by reference between threads.
pub struct Downloader {
    cache_root: PathBuf,
    client: reqwest::blocking::Client,
    policies: HashMap<String, SourcePolicy>,
    states: Mutex<HashMap<String, Arc<Mutex<Limiter>>>>,
    clock: Arc<dyn Clock>,
    offline: bool,
}

/// Outcome of one attempt.
enum Attempt {
    Done(Fetched),
    Retry(Error, Option<Duration>),
    Fatal(Error),
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|p| p.into_inner())
}

impl Downloader {
    /// Builder with the [`known_sources`] policies, caching under `cache_root`.
    pub fn builder(cache_root: impl Into<PathBuf>) -> DownloaderBuilder {
        DownloaderBuilder {
            cache_root: cache_root.into(),
            policies: known_sources(),
            overrides: BTreeMap::new(),
            clock: Arc::new(SystemClock),
            offline: false,
        }
    }

    /// Downloader from the process configuration (cache in `<data>/cache`, overrides).
    pub fn from_config(cfg: &Config) -> Result<Self> {
        Self::builder(cfg.layout().cache_root())
            .rate_limit_overrides(cfg.rate_limits.clone())
            .build()
    }

    /// Effective policy of a source (default policy for an unknown source).
    pub fn policy(&self, source: &str) -> SourcePolicy {
        self.policies
            .get(source)
            .cloned()
            .unwrap_or_else(|| SourcePolicy::new(source))
    }

    /// Root of the cache.
    pub fn cache_root(&self) -> &Path {
        &self.cache_root
    }

    fn limiter(&self, policy: &SourcePolicy, dir: &Path) -> Result<Arc<Mutex<Limiter>>> {
        let mut states = lock(&self.states);
        if let Some(s) = states.get(&policy.name) {
            return Ok(s.clone());
        }
        let l = Limiter::load(policy.limits.clone(), Some(dir.join("ratelimit.json")))?;
        let s = Arc::new(Mutex::new(l));
        states.insert(policy.name.clone(), s.clone());
        Ok(s)
    }

    fn now_unix(&self) -> u64 {
        unix_ms(self.clock.now()) / 1000
    }

    /// Fetches a request, from the cache when allowed.
    pub fn fetch(&self, req: &Request) -> Result<Fetched> {
        check_source_name(&req.source)?;
        let policy = self.policy(&req.source);
        let dir = self.cache_root.join(&req.source);
        std::fs::create_dir_all(&dir).map_err(|e| Error::io(&dir, e))?;
        let key = cache::key(
            req.method.as_str(),
            &req.url,
            req.body.as_deref().map(|bytes| cache::KeyBody {
                bytes,
                content_type: req.header_value("content-type"),
            }),
            req.range_header(),
        );
        let entry = Entry::new(&dir, &key);
        let mode = if self.offline {
            CachePolicy::Offline
        } else {
            req.cache
        };

        if mode == CachePolicy::Offline {
            let meta = entry.load()?.ok_or_else(|| Error::NotCached {
                url: req.url.clone(),
            })?;
            return Ok(self.done(req, &entry, meta, Origin::Cache));
        }

        // One request in flight per source: the lock covers the freshness check, the rate
        // limiter, the request and the retries.
        let state = self.limiter(&policy, &dir)?;
        let mut limiter = lock(&state);

        let cached = entry.load()?;
        let max_age = req.max_age.unwrap_or(policy.max_age);
        if mode == CachePolicy::Normal
            && let Some(meta) = &cached
            && self.now_unix().saturating_sub(meta.validated_at_unix) < max_age.as_secs()
        {
            let meta = meta.clone();
            return Ok(self.done(req, &entry, meta, Origin::Cache));
        }
        let conditional = if mode == CachePolicy::Reload {
            None
        } else {
            cached
        };

        let mut attempt = 0u32;
        loop {
            let sent = limiter.acquire(&req.source, self.clock.as_ref(), policy.max_wait)?;
            let outcome = self.attempt(req, &policy, conditional.as_ref(), &entry);
            limiter.finish(sent, unix_ms(self.clock.now()))?;
            match outcome {
                Attempt::Done(f) => return Ok(f),
                Attempt::Fatal(e) => return Err(e),
                Attempt::Retry(e, retry_after) => {
                    if attempt >= policy.retries {
                        return Err(e);
                    }
                    let backoff = policy
                        .backoff_base
                        .saturating_mul(1u32 << attempt.min(16))
                        .min(policy.backoff_max);
                    let wait = retry_after.map_or(backoff, |ra| ra.max(backoff));
                    if wait > policy.max_wait {
                        return Err(Error::RateLimited {
                            source_name: req.source.clone(),
                            wait,
                        });
                    }
                    tracing::warn!(
                        source = %req.source,
                        url = %req.url,
                        attempt = attempt + 1,
                        wait_ms = wait.as_millis() as u64,
                        error = %e,
                        "retrying"
                    );
                    self.clock.sleep(wait);
                    attempt += 1;
                }
            }
        }
    }

    fn done(&self, req: &Request, entry: &Entry, meta: CacheMeta, origin: Origin) -> Fetched {
        tracing::info!(source = %req.source, url = %req.url, ?origin, size = meta.size, "fetched");
        Fetched {
            path: entry.body.clone(),
            meta,
            origin,
        }
    }

    fn attempt(
        &self,
        req: &Request,
        policy: &SourcePolicy,
        cond: Option<&CacheMeta>,
        entry: &Entry,
    ) -> Attempt {
        let url = req.send_url();
        let mut rb = match req.method {
            Method::Get => self.client.get(&url),
            Method::Post => self
                .client
                .post(&url)
                .body(req.body.clone().unwrap_or_default()),
        };
        rb = rb.timeout(policy.timeout);
        for (k, v) in &req.headers {
            rb = rb.header(k.as_str(), v.as_str());
        }
        for (k, v) in &req.secret_headers {
            rb = rb.header(k.as_str(), v.expose());
        }
        if let Some(m) = cond {
            if let Some(etag) = &m.etag {
                rb = rb.header(IF_NONE_MATCH, etag.as_str());
            }
            if let Some(lm) = &m.last_modified {
                rb = rb.header(IF_MODIFIED_SINCE, lm.as_str());
            }
        }
        let http_err = |e: reqwest::Error| Error::Http {
            url: req.url.clone(),
            message: e.without_url().to_string(),
        };
        let mut resp = match rb.send() {
            Ok(r) => r,
            Err(e) => return Attempt::Retry(http_err(e), None),
        };
        let status = resp.status();
        let now = self.now_unix();

        if status == StatusCode::NOT_MODIFIED {
            let Some(m) = cond else {
                return Attempt::Fatal(Error::Status {
                    url: req.url.clone(),
                    status: 304,
                });
            };
            let mut meta = m.clone();
            meta.validated_at_unix = now;
            if let Err(e) = entry.store_meta(&meta) {
                return Attempt::Fatal(e);
            }
            return Attempt::Done(self.done(req, entry, meta, Origin::NotModified));
        }

        if status.is_success()
            && req.range_header().is_some()
            && status != StatusCode::PARTIAL_CONTENT
        {
            // The whole resource instead of the part asked for: never cache it under the
            // key of a range.
            return Attempt::Fatal(Error::Http {
                url: req.url.clone(),
                message: format!(
                    "status {} to a range request ({}): the server ignores Range",
                    status.as_u16(),
                    req.range_header().unwrap_or_default()
                ),
            });
        }
        if status.is_success() {
            let headers = resp.headers().clone();
            let tmp = cache::tmp_path(&entry.body);
            let (size, sha256) = match cache::store_body(&tmp, &mut resp) {
                Ok(v) => v,
                Err(e) => {
                    return Attempt::Retry(
                        Error::Http {
                            url: req.url.clone(),
                            message: format!("reading body: {e}"),
                        },
                        None,
                    );
                }
            };
            let meta = CacheMeta {
                url: req.url.clone(),
                method: req.method.as_str().to_owned(),
                status: status.as_u16(),
                etag: header_str(&headers, ETAG.as_str()),
                last_modified: header_str(&headers, LAST_MODIFIED.as_str()),
                content_type: header_str(&headers, CONTENT_TYPE.as_str()),
                fetched_at_unix: now,
                validated_at_unix: now,
                size,
                sha256,
                range: req.range_header().map(str::to_owned),
            };
            // Remove the old metadata first: a crash between the two renames leaves a body
            // without metadata, which is treated as not cached.
            let commit = (|| -> Result<()> {
                match std::fs::remove_file(&entry.meta) {
                    Ok(()) => {}
                    Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                    Err(e) => return Err(Error::io(&entry.meta, e)),
                }
                std::fs::rename(&tmp, &entry.body).map_err(|e| Error::io(&entry.body, e))?;
                entry.store_meta(&meta)
            })();
            if let Err(e) = commit {
                let _ = std::fs::remove_file(&tmp);
                return Attempt::Fatal(e);
            }
            return Attempt::Done(self.done(req, entry, meta, Origin::Downloaded));
        }

        let err = Error::Status {
            url: req.url.clone(),
            status: status.as_u16(),
        };
        let retryable = matches!(status.as_u16(), 408 | 429 | 500 | 502 | 503 | 504);
        if retryable {
            let retry_after = header_str(resp.headers(), RETRY_AFTER.as_str())
                .and_then(|v| parse_retry_after(&v, self.clock.now()));
            Attempt::Retry(err, retry_after)
        } else {
            Attempt::Fatal(err)
        }
    }
}

fn header_str(h: &HeaderMap, name: &str) -> Option<String> {
    h.get(name).and_then(|v| v.to_str().ok()).map(str::to_owned)
}

/// `Retry-After` as delay-seconds or HTTP-date.
fn parse_retry_after(v: &str, now: SystemTime) -> Option<Duration> {
    let v = v.trim();
    if let Ok(secs) = v.parse::<u64>() {
        return Some(Duration::from_secs(secs));
    }
    let at = httpdate::parse_http_date(v).ok()?;
    Some(at.duration_since(now).unwrap_or(Duration::ZERO))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn form_encoding() {
        assert_eq!(form_encode("a b&c=d/é"), "a+b%26c%3Dd%2F%C3%A9");
        let r = Request::post_form("overpass", "https://x/api", &[("data", "[out:json];")]);
        assert_eq!(r.body.as_deref(), Some(&b"data=%5Bout%3Ajson%5D%3B"[..]));
    }

    #[test]
    fn secret_query_only_in_send_url() {
        let s = Secret::new("k y");
        let r = Request::get("x", "https://h/p?a=1").secret_query("key", &s);
        assert_eq!(r.url(), "https://h/p?a=1");
        assert_eq!(r.send_url(), "https://h/p?a=1&key=k+y");
        assert!(!format!("{r:?}").contains("k y"));
    }

    #[test]
    fn retry_after_parsing() {
        let now = SystemTime::UNIX_EPOCH + Duration::from_secs(1_000_000_000);
        assert_eq!(
            parse_retry_after("120", now),
            Some(Duration::from_secs(120))
        );
        let later = httpdate::fmt_http_date(now + Duration::from_secs(30));
        assert_eq!(
            parse_retry_after(&later, now),
            Some(Duration::from_secs(30))
        );
        assert_eq!(parse_retry_after("soon", now), None);
    }

    #[test]
    fn user_agent_is_generic() {
        assert!(USER_AGENT.starts_with("skimap/"));
        assert!(!USER_AGENT.contains('@'));
    }
}
