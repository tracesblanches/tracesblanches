//! HTTP plumbing: a request and a reply independent of the server library, conditional and
//! partial responses of static files, and the blocking server loop (tiny_http, fixed
//! worker pool, file downloads on their own bounded threads).

use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use serde::Serialize;

/// Largest request body read (JSON of a trip plan, OAuth forms).
pub const MAX_BODY: usize = 64 * 1024;

/// An HTTP request, already read.
#[derive(Debug, Clone, Default)]
pub struct Req {
    /// `GET`, `HEAD`, `POST`, ...
    pub method: String,
    /// Decoded-as-is path, without the query string.
    pub path: String,
    /// Raw query string (after `?`), possibly empty.
    pub query: String,
    /// Headers, names in lowercase.
    pub headers: Vec<(String, String)>,
    /// Body (at most [`MAX_BODY`] bytes).
    pub body: Vec<u8>,
    /// Client address: `CF-Connecting-IP` (Cloudflare), else the first
    /// `X-Forwarded-For`, else the peer.
    pub client: String,
}

impl Req {
    /// A request without headers or body (tests).
    pub fn new(method: &str, target: &str) -> Self {
        let (path, query) = target.split_once('?').unwrap_or((target, ""));
        Self {
            method: method.to_owned(),
            path: path.to_owned(),
            query: query.to_owned(),
            client: "test".to_owned(),
            ..Default::default()
        }
    }

    /// Adds a header (tests).
    pub fn header(mut self, name: &str, value: &str) -> Self {
        self.headers
            .push((name.to_ascii_lowercase(), value.to_owned()));
        self
    }

    /// Sets the body (tests).
    pub fn body(mut self, body: impl Into<Vec<u8>>) -> Self {
        self.body = body.into();
        self
    }

    /// First header with this (lowercase) name.
    pub fn get(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(k, _)| k == name)
            .map(|(_, v)| v.as_str())
    }

    /// Value of a query parameter.
    pub fn param(&self, name: &str) -> Option<String> {
        crate::util::parse_query(&self.query)
            .into_iter()
            .find(|(k, _)| k == name)
            .map(|(_, v)| v)
    }

    /// `GET` or `HEAD`.
    pub fn is_read(&self) -> bool {
        self.method == "GET" || self.method == "HEAD"
    }

    /// Whether `If-None-Match` matches `etag` (quoted).
    fn not_modified(&self, etag: &str) -> bool {
        self.get("if-none-match").is_some_and(|v| {
            v.split(',')
                .any(|t| t.trim() == "*" || t.trim().trim_start_matches("W/") == etag)
        })
    }
}

/// Body of a reply.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Body {
    /// No body.
    Empty,
    /// In memory.
    Bytes(Vec<u8>),
    /// A slice of a file.
    File {
        /// File.
        path: PathBuf,
        /// First byte.
        offset: u64,
        /// Number of bytes.
        len: u64,
    },
}

/// An HTTP reply.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Reply {
    /// Status code.
    pub status: u16,
    /// Headers (no `Content-Length`: computed from the body).
    pub headers: Vec<(String, String)>,
    /// Body.
    pub body: Body,
}

impl Reply {
    /// A reply with a body in memory.
    pub fn bytes(status: u16, content_type: &str, body: impl Into<Vec<u8>>) -> Self {
        Self {
            status,
            headers: vec![("Content-Type".into(), content_type.into())],
            body: Body::Bytes(body.into()),
        }
    }

    /// Plain text.
    pub fn text(status: u16, body: &str) -> Self {
        Self::bytes(status, "text/plain; charset=utf-8", body)
    }

    /// HTML page.
    pub fn html(status: u16, body: &str) -> Self {
        Self::bytes(status, "text/html; charset=utf-8", body)
    }

    /// JSON value, compact.
    pub fn json<T: Serialize>(status: u16, v: &T) -> Self {
        let body = serde_json::to_vec(v).expect("JSON serialises");
        Self::bytes(status, "application/json", body)
    }

    /// JSON error `{"error": code, "message": message}`.
    pub fn error(status: u16, code: &str, message: impl Into<String>) -> Self {
        Self::json(
            status,
            &serde_json::json!({"error": code, "message": message.into()}),
        )
        .with("Cache-Control", "no-store")
    }

    /// Adds a header.
    pub fn with(mut self, name: &str, value: impl Into<String>) -> Self {
        self.headers.push((name.to_owned(), value.into()));
        self
    }

    /// Value of a header (tests).
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(name))
            .map(|(_, v)| v.as_str())
    }

    /// Body length.
    pub fn len(&self) -> u64 {
        match &self.body {
            Body::Empty => 0,
            Body::Bytes(b) => b.len() as u64,
            Body::File { len, .. } => *len,
        }
    }

    /// Whether the body is empty.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Body bytes (tests; reads files).
    pub fn body_bytes(&self) -> Vec<u8> {
        match &self.body {
            Body::Empty => Vec::new(),
            Body::Bytes(b) => b.clone(),
            Body::File { path, offset, len } => {
                let mut f = File::open(path).expect("open");
                f.seek(SeekFrom::Start(*offset)).expect("seek");
                let mut out = Vec::new();
                f.take(*len).read_to_end(&mut out).expect("read");
                out
            }
        }
    }
}

/// Parses a single-range `Range` header against a length: `(start, end)` inclusive.
/// `None`: not a single byte range (served whole); `Some(Err(()))`: unsatisfiable.
#[allow(clippy::result_unit_err)]
pub fn parse_range(h: &str, len: u64) -> Option<Result<(u64, u64), ()>> {
    let spec = h.trim().strip_prefix("bytes=")?;
    if spec.contains(',') {
        return None;
    }
    let (a, b) = spec.split_once('-')?;
    let (a, b) = (a.trim(), b.trim());
    if len == 0 {
        return Some(Err(()));
    }
    let r = if a.is_empty() {
        let n: u64 = b.parse().ok()?;
        if n == 0 {
            return Some(Err(()));
        }
        (len.saturating_sub(n), len - 1)
    } else {
        let s: u64 = a.parse().ok()?;
        let e: u64 = if b.is_empty() {
            len - 1
        } else {
            b.parse::<u64>().ok()?.min(len - 1)
        };
        (s, e)
    };
    if r.0 > r.1 || r.0 >= len {
        Some(Err(()))
    } else {
        Some(Ok(r))
    }
}

/// Serves a file (or the bytes of `inline`) with a strong `ETag`, conditional requests
/// (`If-None-Match` → 304) and single byte ranges (`Range`, `If-Range` → 206 or 416).
pub fn file_reply(
    req: &Req,
    path: &Path,
    etag: &str,
    content_type: &str,
    cache_control: &str,
) -> Reply {
    let len = match std::fs::metadata(path) {
        Ok(m) if m.is_file() => m.len(),
        _ => return Reply::error(404, "not_found", "no such file"),
    };
    let base = |status: u16, body: Body| Reply {
        status,
        headers: vec![
            ("Content-Type".into(), content_type.into()),
            ("ETag".into(), etag.into()),
            ("Accept-Ranges".into(), "bytes".into()),
            ("Cache-Control".into(), cache_control.into()),
            ("Access-Control-Allow-Origin".into(), "*".into()),
            (
                "Access-Control-Expose-Headers".into(),
                "Content-Range, Content-Length, ETag, Accept-Ranges".into(),
            ),
        ],
        body,
    };
    if req.not_modified(etag) {
        return base(304, Body::Empty);
    }
    let range_applies = req.get("if-range").is_none_or(|v| v.trim() == etag);
    let range = req
        .get("range")
        .filter(|_| range_applies)
        .and_then(|h| parse_range(h, len));
    match range {
        None => base(
            200,
            Body::File {
                path: path.to_owned(),
                offset: 0,
                len,
            },
        ),
        Some(Ok((s, e))) => base(
            206,
            Body::File {
                path: path.to_owned(),
                offset: s,
                len: e - s + 1,
            },
        )
        .with("Content-Range", format!("bytes {s}-{e}/{len}")),
        Some(Err(())) => Reply {
            status: 416,
            headers: vec![
                ("Content-Range".into(), format!("bytes */{len}")),
                ("Content-Type".into(), "text/plain; charset=utf-8".into()),
            ],
            body: Body::Bytes(b"416 Range Not Satisfiable\n".to_vec()),
        },
    }
}

/// Something that answers requests.
pub trait Handler: Send + Sync + 'static {
    /// Answers one request.
    fn handle(&self, req: &Req) -> Reply;
}

fn client_of(headers: &[(String, String)], peer: Option<&SocketAddr>) -> String {
    let get = |n: &str| {
        headers
            .iter()
            .find(|(k, _)| k == n)
            .map(|(_, v)| v.trim().to_owned())
    };
    get("cf-connecting-ip")
        .or_else(|| {
            get("x-forwarded-for").and_then(|v| v.split(',').next().map(|s| s.trim().to_owned()))
        })
        .filter(|s| !s.is_empty())
        .or_else(|| peer.map(|p| p.ip().to_string()))
        .unwrap_or_else(|| "unknown".to_owned())
}

fn respond(req: tiny_http::Request, reply: Reply) -> std::io::Result<()> {
    let status = tiny_http::StatusCode(reply.status);
    let headers: Vec<tiny_http::Header> = reply
        .headers
        .iter()
        .filter_map(|(k, v)| tiny_http::Header::from_bytes(k.as_bytes(), v.as_bytes()).ok())
        .chain(tiny_http::Header::from_bytes(&b"Server"[..], &b"skimap-server"[..]).ok())
        .collect();
    match reply.body {
        Body::Empty => req.respond(
            tiny_http::Response::new(status, headers, std::io::empty(), Some(0), None)
                .with_chunked_threshold(usize::MAX),
        ),
        Body::Bytes(b) => {
            let n = b.len();
            req.respond(
                tiny_http::Response::new(status, headers, std::io::Cursor::new(b), Some(n), None)
                    .with_chunked_threshold(usize::MAX),
            )
        }
        Body::File { path, offset, len } => {
            let mut f = File::open(&path)?;
            f.seek(SeekFrom::Start(offset))?;
            let n = usize::try_from(len).unwrap_or(usize::MAX);
            req.respond(
                tiny_http::Response::new(status, headers, f.take(len), Some(n), None)
                    .with_chunked_threshold(usize::MAX),
            )
        }
    }
}

fn read_request(req: &mut tiny_http::Request) -> Result<Req, Reply> {
    let headers: Vec<(String, String)> = req
        .headers()
        .iter()
        .map(|h| {
            (
                h.field.as_str().as_str().to_ascii_lowercase(),
                h.value.as_str().to_owned(),
            )
        })
        .collect();
    let target = req.url().to_owned();
    let (path, query) = target.split_once('?').unwrap_or((&target, ""));
    if req.body_length().is_some_and(|n| n > MAX_BODY) {
        return Err(Reply::error(413, "too_large", "request body too large"));
    }
    let mut body = Vec::new();
    req.as_reader()
        .take(MAX_BODY as u64 + 1)
        .read_to_end(&mut body)
        .map_err(|_| Reply::error(400, "bad_request", "unreadable body"))?;
    if body.len() > MAX_BODY {
        return Err(Reply::error(413, "too_large", "request body too large"));
    }
    Ok(Req {
        method: req.method().as_str().to_ascii_uppercase(),
        path: path.to_owned(),
        query: query.to_owned(),
        client: client_of(&headers, req.remote_addr()),
        headers,
        body,
    })
}

/// Limits of the server loop ([`serve_with`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Limits {
    /// Threads answering requests (`SKIMAP_SERVER_WORKERS`).
    pub workers: usize,
    /// Downloads of large files (bundles, basemap archives: over [`SMALL_FILE`]) sent at
    /// the same time, each on its own thread; beyond, 503 with `Retry-After`
    /// (`SKIMAP_SERVER_MAX_DOWNLOADS`). Small files (catalog, indexes) are never refused:
    /// up to four times as many on their own threads, beyond that written by the worker.
    pub max_downloads: usize,
    /// A connection silent this long (no request, no byte of the body, no byte of the
    /// reply accepted) is closed (`SKIMAP_SERVER_IO_TIMEOUT_SECS`).
    pub io_timeout: Duration,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            workers: 16,
            max_downloads: 512,
            io_timeout: Duration::from_secs(60),
        }
    }
}

/// Largest file counted as small: a catalog or a site index (tens of kB), not a bundle.
pub const SMALL_FILE: u64 = 1 << 20;

/// File replies being written on their own threads: large ones at most `max`, small ones
/// at most `4 * max`.
struct Downloads {
    large: AtomicUsize,
    small: AtomicUsize,
    max: usize,
}

/// One slot, given back on drop.
struct Slot(Arc<Downloads>, bool);

impl Drop for Slot {
    fn drop(&mut self) {
        self.0.counter(self.1).fetch_sub(1, Ordering::SeqCst);
    }
}

impl Downloads {
    fn counter(&self, large: bool) -> &AtomicUsize {
        if large { &self.large } else { &self.small }
    }

    fn take(self: &Arc<Self>, large: bool) -> Option<Slot> {
        let max = if large { self.max } else { 4 * self.max };
        self.counter(large)
            .try_update(Ordering::SeqCst, Ordering::SeqCst, |n| {
                (n < max).then_some(n + 1)
            })
            .ok()
            .map(|_| Slot(Arc::clone(self), large))
    }
}

/// A running server: its address and the worker threads.
pub struct Running {
    server: Arc<tiny_http::Server>,
    stopping: Arc<AtomicBool>,
    /// Bound address.
    pub addr: SocketAddr,
    workers: Vec<std::thread::JoinHandle<()>>,
}

impl Running {
    /// Stops accepting requests and waits for the workers.
    pub fn stop(self) {
        self.stopping.store(true, Ordering::SeqCst);
        for _ in &self.workers {
            self.server.unblock();
        }
        for w in self.workers {
            let _ = w.join();
        }
    }

    /// Waits for the workers (forever, in production).
    pub fn join(self) {
        for w in self.workers {
            let _ = w.join();
        }
    }
}

/// The listening socket. Linux copies its `TCP_NODELAY` and its receive and send timeouts
/// to every accepted connection, which tiny_http does not let us reach:
/// - without `TCP_NODELAY`, a reply written as headers then body waits for the delayed
///   acknowledgement of the headers (40 ms per index or catalog of a few tens of kB);
/// - without timeouts, a connection that never sends another request keeps its thread
///   and its two descriptors forever.
fn listener(addr: &str, io_timeout: Duration) -> anyhow::Result<std::net::TcpListener> {
    use std::net::ToSocketAddrs;
    let sa = addr
        .to_socket_addrs()?
        .next()
        .ok_or_else(|| anyhow::anyhow!("{addr}: no address"))?;
    let s = socket2::Socket::new(
        socket2::Domain::for_address(sa),
        socket2::Type::STREAM,
        Some(socket2::Protocol::TCP),
    )?;
    s.set_reuse_address(true)?;
    s.set_tcp_nodelay(true)?;
    s.set_read_timeout(Some(io_timeout))?;
    s.set_write_timeout(Some(io_timeout))?;
    s.bind(&sa.into())?;
    s.listen(1024)?;
    Ok(s.into())
}

/// Binds `addr` and answers requests with `handler` on `workers` threads (other limits by
/// default).
pub fn serve(addr: &str, workers: usize, handler: Arc<dyn Handler>) -> anyhow::Result<Running> {
    serve_with(
        addr,
        Limits {
            workers,
            ..Limits::default()
        },
        handler,
    )
}

/// Binds `addr` and answers requests with `handler` within `limits`.
///
/// Blocking on purpose: the workspace is blocking and a fixed pool bounds memory. The
/// workers only compute replies; a file (bundle, basemap file, catalog, index) is written
/// on its own thread, so phones on a weak network, or Cloudflare filling its cache, never
/// hold the workers that answer the catalog, the indexes and the alerts.
pub fn serve_with(
    addr: &str,
    limits: Limits,
    handler: Arc<dyn Handler>,
) -> anyhow::Result<Running> {
    let server = Arc::new(
        tiny_http::Server::from_listener(listener(addr, limits.io_timeout)?, None)
            .map_err(|e| anyhow::anyhow!("binding {addr}: {e}"))?,
    );
    let addr = server
        .server_addr()
        .to_ip()
        .ok_or_else(|| anyhow::anyhow!("not an IP listener"))?;
    let stopping = Arc::new(AtomicBool::new(false));
    let downloads = Arc::new(Downloads {
        large: AtomicUsize::new(0),
        small: AtomicUsize::new(0),
        max: limits.max_downloads.max(1),
    });
    let workers = (0..limits.workers.max(1))
        .map(|_| {
            let server = Arc::clone(&server);
            let handler = Arc::clone(&handler);
            let stopping = Arc::clone(&stopping);
            let downloads = Arc::clone(&downloads);
            std::thread::spawn(move || {
                loop {
                    let mut rq = match server.recv() {
                        Ok(rq) => rq,
                        Err(_) if stopping.load(Ordering::SeqCst) => break,
                        Err(e) => {
                            // Accept error (e.g. out of file descriptors): keep the worker.
                            tracing::warn!(%e, "accept");
                            std::thread::sleep(std::time::Duration::from_millis(100));
                            continue;
                        }
                    };
                    let t = Instant::now();
                    let method = rq.method().as_str().to_owned();
                    let path = rq.url().split('?').next().unwrap_or("").to_owned();
                    let mut reply = match read_request(&mut rq) {
                        Ok(req) => handler.handle(&req),
                        Err(r) => r,
                    };
                    let large = match reply.body {
                        Body::File { len, .. } => Some(len > SMALL_FILE),
                        _ => None,
                    };
                    if let Some(large) = large {
                        match downloads.take(large) {
                            Some(slot) => {
                                let sent = std::thread::Builder::new()
                                    .name("download".into())
                                    .stack_size(256 * 1024)
                                    .spawn(move || {
                                        let _slot = slot;
                                        send(rq, reply, &method, &path, t);
                                    });
                                if let Err(e) = sent {
                                    // The request is dropped with the closure: tiny_http
                                    // answers 500 and closes the connection.
                                    tracing::warn!(%e, "download thread");
                                }
                                continue;
                            }
                            None if large => {
                                reply = Reply::error(
                                    503,
                                    "busy",
                                    "too many downloads in progress, try again later",
                                )
                                .with("Retry-After", "30");
                            }
                            // A small file: the socket buffer takes most of it at once.
                            None => {}
                        }
                    }
                    send(rq, reply, &method, &path, t);
                }
            })
        })
        .collect();
    Ok(Running {
        server,
        stopping,
        addr,
        workers,
    })
}

/// Writes `reply` and logs the request.
fn send(rq: tiny_http::Request, reply: Reply, method: &str, path: &str, t: Instant) {
    let (status, len) = (reply.status, reply.len());
    if let Err(e) = respond(rq, reply) {
        tracing::debug!(%e, "response not sent");
    }
    tracing::info!(
        %method, %path, status, bytes = len,
        ms = t.elapsed().as_millis() as u64, "request"
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ranges() {
        assert_eq!(parse_range("bytes=0-99", 1000), Some(Ok((0, 99))));
        assert_eq!(parse_range("bytes=900-", 1000), Some(Ok((900, 999))));
        assert_eq!(parse_range("bytes=-100", 1000), Some(Ok((900, 999))));
        assert_eq!(parse_range("bytes=990-2000", 1000), Some(Ok((990, 999))));
        assert_eq!(parse_range("bytes=1000-", 1000), Some(Err(())));
        assert_eq!(parse_range("bytes=5-1", 1000), Some(Err(())));
        assert_eq!(parse_range("bytes=0-1,5-6", 1000), None);
        assert_eq!(parse_range("items=0-1", 1000), None);
        assert_eq!(parse_range("bytes=x-1", 1000), None);
    }

    #[test]
    fn client_address() {
        let h = |v: &[(&str, &str)]| -> Vec<(String, String)> {
            v.iter()
                .map(|(a, b)| (a.to_string(), b.to_string()))
                .collect()
        };
        let peer: SocketAddr = "10.42.0.7:5000".parse().unwrap();
        assert_eq!(
            client_of(&h(&[("cf-connecting-ip", "1.2.3.4")]), Some(&peer)),
            "1.2.3.4"
        );
        assert_eq!(
            client_of(&h(&[("x-forwarded-for", "5.6.7.8, 10.0.0.1")]), Some(&peer)),
            "5.6.7.8"
        );
        assert_eq!(client_of(&[], Some(&peer)), "10.42.0.7");
    }

    #[test]
    fn conditional_and_partial() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("f");
        std::fs::write(&p, b"0123456789").unwrap();
        let e = "\"abc\"";
        let r = file_reply(&Req::new("GET", "/f"), &p, e, "x", "c");
        assert_eq!((r.status, r.body_bytes()), (200, b"0123456789".to_vec()));
        let r = file_reply(
            &Req::new("GET", "/f").header("Range", "bytes=2-4"),
            &p,
            e,
            "x",
            "c",
        );
        assert_eq!((r.status, r.body_bytes()), (206, b"234".to_vec()));
        assert_eq!(r.header("content-range"), Some("bytes 2-4/10"));
        let r = file_reply(
            &Req::new("GET", "/f").header("Range", "bytes=20-"),
            &p,
            e,
            "x",
            "c",
        );
        assert_eq!(
            (r.status, r.header("content-range")),
            (416, Some("bytes */10"))
        );
        let r = file_reply(
            &Req::new("GET", "/f").header("If-None-Match", e),
            &p,
            e,
            "x",
            "c",
        );
        assert_eq!(r.status, 304);
        // If-Range with another validator: the whole file.
        let r = file_reply(
            &Req::new("GET", "/f")
                .header("Range", "bytes=2-4")
                .header("If-Range", "\"old\""),
            &p,
            e,
            "x",
            "c",
        );
        assert_eq!(r.status, 200);
        let r = file_reply(
            &Req::new("GET", "/f"),
            &dir.path().join("nope"),
            e,
            "x",
            "c",
        );
        assert_eq!(r.status, 404);
    }
}
