//! Load generator for `skimap-server` (#152): realistic phone traffic against a running
//! server, with the server's memory, threads and open files read from `/proc`.
//!
//! ```sh
//! load <addr> <server pid> <store public dir> mix  <connections> <seconds>
//! load <addr> <server pid> <store public dir> slow <slow downloads> <seconds>
//! load <addr> <server pid> <store public dir> idle <idle connections> <seconds>
//! ```
//!
//! - `mix`: keep-alive connections, each looping over what phones send: catalog and site
//!   indexes with `If-None-Match` (304), indexes without (200), a 1 MiB range of a bundle
//!   (resumed download), a France basemap file.
//! - `slow`: downloads of whole bundles read at 64 KiB/s (a phone on a weak network, or a
//!   Cloudflare fetch on a cache miss behind it) while a probe asks for the catalog.
//! - `idle`: connections opened and left silent (keep-alive pools, dead phones) while a
//!   probe asks for the catalog.
//!
//! Standard library only: the numbers are those of the server, not of a client stack.

#![allow(
    clippy::disallowed_methods,
    clippy::print_stdout,
    reason = "developer tool, not a pipeline output"
)]
use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpStream;
use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};

/// One response: status, `ETag`, body bytes.
struct Resp {
    status: u16,
    etag: Option<String>,
    bytes: u64,
}

/// Sends `GET target` with `headers` on `s` and reads the response, the body at most
/// `rate` bytes per second (0: as fast as possible).
fn get(
    s: &mut BufReader<TcpStream>,
    target: &str,
    headers: &[(&str, String)],
    rate: u64,
) -> Result<Resp> {
    let mut rq = format!("GET {target} HTTP/1.1\r\nHost: load\r\nUser-Agent: skimap-load\r\n");
    for (k, v) in headers {
        rq.push_str(&format!("{k}: {v}\r\n"));
    }
    rq.push_str("\r\n");
    s.get_mut().write_all(rq.as_bytes())?;
    let mut line = String::new();
    if s.read_line(&mut line)? == 0 {
        bail!("connection closed");
    }
    let status: u16 = line
        .split_whitespace()
        .nth(1)
        .context("status line")?
        .parse()?;
    let (mut len, mut etag) = (0u64, None);
    loop {
        line.clear();
        s.read_line(&mut line)?;
        let l = line.trim_end();
        if l.is_empty() {
            break;
        }
        if let Some((k, v)) = l.split_once(':') {
            match k.to_ascii_lowercase().as_str() {
                "content-length" => len = v.trim().parse()?,
                "etag" => etag = Some(v.trim().to_owned()),
                _ => {}
            }
        }
    }
    let mut buf = vec![0u8; 64 * 1024];
    let mut left = len;
    while left > 0 {
        let n = usize::try_from(left.min(buf.len() as u64))?;
        let t = Instant::now();
        s.read_exact(&mut buf[..n])?;
        left -= n as u64;
        if rate > 0 {
            let want = Duration::from_secs_f64(n as f64 / rate as f64);
            if let Some(d) = want.checked_sub(t.elapsed()) {
                std::thread::sleep(d);
            }
        }
    }
    Ok(Resp {
        status,
        etag,
        bytes: len,
    })
}

fn connect(addr: &str) -> Result<BufReader<TcpStream>> {
    let s = TcpStream::connect(addr)?;
    s.set_read_timeout(Some(Duration::from_secs(10)))?;
    s.set_write_timeout(Some(Duration::from_secs(10)))?;
    s.set_nodelay(true)?;
    Ok(BufReader::with_capacity(64 * 1024, s))
}

/// What the server holds: resident memory (KiB), threads, open files.
fn proc_stats(pid: u32) -> String {
    let status = std::fs::read_to_string(format!("/proc/{pid}/status")).unwrap_or_default();
    let field = |name: &str| {
        status
            .lines()
            .find(|l| l.starts_with(name))
            .and_then(|l| l.split_whitespace().nth(1))
            .unwrap_or("?")
            .to_owned()
    };
    let fds = std::fs::read_dir(format!("/proc/{pid}/fd")).map_or(0, |d| d.count());
    format!(
        "rss_kib={} hwm_kib={} threads={} fds={fds}",
        field("VmRSS:"),
        field("VmHWM:"),
        field("Threads:")
    )
}

fn percentiles(mut v: Vec<f64>) -> String {
    if v.is_empty() {
        return "n=0".into();
    }
    v.sort_by(f64::total_cmp);
    let p = |q: f64| v[((v.len() - 1) as f64 * q) as usize];
    format!(
        "n={} p50={:.2}ms p99={:.2}ms max={:.2}ms",
        v.len(),
        p(0.5),
        p(0.99),
        v[v.len() - 1]
    )
}

/// Targets of the published store.
struct Targets {
    indexes: Vec<String>,
    bundles: Vec<(String, u64)>,
    basemap: Vec<String>,
}

fn targets(public: &Path) -> Result<Targets> {
    let mut indexes: Vec<String> = std::fs::read_dir(public.join("sites"))?
        .filter_map(|e| e.ok())
        .map(|e| format!("/v1/sites/{}/index.json", e.file_name().to_string_lossy()))
        .collect();
    indexes.sort();
    let mut bundles: Vec<(String, u64)> = std::fs::read_dir(public.join("bundles"))?
        .filter_map(|e| e.ok())
        .filter_map(|e| {
            let len = e.metadata().ok()?.len();
            Some((
                format!("/v1/bundles/{}", e.file_name().to_string_lossy()),
                len,
            ))
        })
        .collect();
    bundles.sort();
    let mut basemap: Vec<String> = std::fs::read_dir(public.join("basemap/files"))?
        .filter_map(|e| e.ok())
        .map(|e| format!("/v1/basemap/files/{}", e.file_name().to_string_lossy()))
        .collect();
    basemap.sort();
    if indexes.is_empty() || bundles.is_empty() || basemap.is_empty() {
        bail!("empty store: publish packs and the France basemap first");
    }
    Ok(Targets {
        indexes,
        bundles,
        basemap,
    })
}

/// A small deterministic generator (no dependency).
struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
}

fn mix(addr: &str, pid: u32, t: Arc<Targets>, conns: usize, secs: u64) -> Result<()> {
    let stop = Arc::new(AtomicBool::new(false));
    let lat = Arc::new(Mutex::new(Vec::<(&'static str, f64)>::new()));
    let (reqs, bytes, errors) = (
        Arc::new(AtomicU64::new(0)),
        Arc::new(AtomicU64::new(0)),
        Arc::new(AtomicU64::new(0)),
    );
    let t0 = Instant::now();
    let handles: Vec<_> = (0..conns)
        .map(|i| {
            let (addr, t, stop, lat) = (addr.to_owned(), t.clone(), stop.clone(), lat.clone());
            let (reqs, bytes, errors) = (reqs.clone(), bytes.clone(), errors.clone());
            std::thread::spawn(move || {
                let mut rng = Rng(0x9E37_79B9_7F4A_7C15 ^ (i as u64 + 1));
                let mut etags = std::collections::HashMap::<String, String>::new();
                let mut mine = Vec::new();
                let mut s = None;
                while !stop.load(Ordering::Relaxed) {
                    let r = rng.next() % 100;
                    let kind = match r {
                        0..30 => "catalog",
                        30..75 => "index",
                        75..90 => "bundle_range",
                        _ => "basemap_file",
                    };
                    let (target, headers): (String, Vec<(&str, String)>) = match r {
                        0..30 => {
                            let x = "/v1/catalog.json".to_owned();
                            let h = etags
                                .get(&x)
                                .map(|e| vec![("If-None-Match", e.clone())])
                                .unwrap_or_default();
                            (x, h)
                        }
                        30..75 => {
                            let x = t.indexes[(rng.next() as usize) % t.indexes.len()].clone();
                            let h = etags
                                .get(&x)
                                .filter(|_| r < 65)
                                .map(|e| vec![("If-None-Match", e.clone())])
                                .unwrap_or_default();
                            (x, h)
                        }
                        75..90 => {
                            let (x, len) = &t.bundles[(rng.next() as usize) % t.bundles.len()];
                            let start = rng.next() % len.saturating_sub(1 << 20).max(1);
                            let end = (start + (1 << 20) - 1).min(len - 1);
                            (x.clone(), vec![("Range", format!("bytes={start}-{end}"))])
                        }
                        _ => (
                            t.basemap[(rng.next() as usize) % t.basemap.len()].clone(),
                            Vec::new(),
                        ),
                    };
                    if s.is_none() {
                        s = connect(&addr).ok();
                    }
                    let Some(c) = s.as_mut() else {
                        errors.fetch_add(1, Ordering::Relaxed);
                        continue;
                    };
                    let q = Instant::now();
                    match get(c, &target, &headers, 0) {
                        Ok(resp) if resp.status < 400 => {
                            let kind = match (kind, resp.status) {
                                ("catalog", 304) => "catalog_304",
                                ("index", 304) => "index_304",
                                ("index", _) => "index_200",
                                (k, _) => k,
                            };
                            let ms = q.elapsed().as_secs_f64() * 1e3;
                            if ms > 1000.0 {
                                eprintln!(
                                    "slow: connection {i} {target} {} ms={ms:.0} at t={:.1}s",
                                    resp.status,
                                    t0.elapsed().as_secs_f64()
                                );
                            }
                            mine.push((kind, ms));
                            reqs.fetch_add(1, Ordering::Relaxed);
                            bytes.fetch_add(resp.bytes, Ordering::Relaxed);
                            if let Some(e) = resp.etag {
                                etags.insert(target, e);
                            }
                        }
                        _ => {
                            errors.fetch_add(1, Ordering::Relaxed);
                            s = None;
                        }
                    }
                }
                lat.lock().expect("lock").extend(mine);
            })
        })
        .collect();
    let mut peak = String::new();
    while t0.elapsed() < Duration::from_secs(secs) {
        std::thread::sleep(Duration::from_millis(500));
        peak = proc_stats(pid);
    }
    stop.store(true, Ordering::Relaxed);
    for h in handles {
        let _ = h.join();
    }
    let el = t0.elapsed().as_secs_f64();
    let n = reqs.load(Ordering::Relaxed);
    let all = std::mem::take(&mut *lat.lock().expect("lock"));
    println!(
        "mix conns={conns} secs={el:.1} requests={n} req_per_s={:.0} MiB_per_s={:.1} errors={} latency: {}",
        n as f64 / el,
        bytes.load(Ordering::Relaxed) as f64 / el / 1048576.0,
        errors.load(Ordering::Relaxed),
        percentiles(all.iter().map(|(_, v)| *v).collect())
    );
    let mut kinds: Vec<&str> = all.iter().map(|(k, _)| *k).collect();
    kinds.sort_unstable();
    kinds.dedup();
    for k in kinds {
        let v = all
            .iter()
            .filter(|(x, _)| *x == k)
            .map(|(_, v)| *v)
            .collect();
        println!("  {k}: {}", percentiles(v));
    }
    println!("server during load: {peak}");
    Ok(())
}

/// Asks for the catalog every 100 ms on a fresh connection while `busy` holds, and reports
/// the latencies (a probe that gets nothing within 10 s counts as a timeout).
fn probe(addr: &str, secs: u64) -> String {
    let t0 = Instant::now();
    let (mut lat, mut timeouts) = (Vec::new(), 0);
    while t0.elapsed() < Duration::from_secs(secs) {
        let q = Instant::now();
        match connect(addr).and_then(|mut c| get(&mut c, "/v1/catalog.json", &[], 0)) {
            Ok(r) if r.status == 200 => lat.push(q.elapsed().as_secs_f64() * 1e3),
            _ => timeouts += 1,
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    format!("probe catalog {} failed={timeouts}", percentiles(lat))
}

fn slow(addr: &str, pid: u32, t: Arc<Targets>, n: usize, secs: u64) -> Result<()> {
    let stop = Arc::new(AtomicBool::new(false));
    let (ok, refused) = (Arc::new(AtomicU64::new(0)), Arc::new(AtomicU64::new(0)));
    let handles: Vec<_> = (0..n)
        .map(|i| {
            let (addr, t, stop) = (addr.to_owned(), t.clone(), stop.clone());
            let (ok, refused) = (ok.clone(), refused.clone());
            std::thread::spawn(move || {
                let (x, _) = &t.bundles[i % t.bundles.len()];
                while !stop.load(Ordering::Relaxed) {
                    match connect(&addr).and_then(|mut c| get(&mut c, x, &[], 64 * 1024)) {
                        Ok(r) if r.status == 200 => {
                            ok.fetch_add(1, Ordering::Relaxed);
                        }
                        _ => {
                            refused.fetch_add(1, Ordering::Relaxed);
                            std::thread::sleep(Duration::from_millis(500));
                        }
                    }
                }
            })
        })
        .collect();
    std::thread::sleep(Duration::from_secs(2));
    let p = probe(addr, secs);
    let stats = proc_stats(pid);
    stop.store(true, Ordering::Relaxed);
    println!(
        "slow downloads={n} at 64KiB/s: {p}; downloads finished={} failed_or_refused={}",
        ok.load(Ordering::Relaxed),
        refused.load(Ordering::Relaxed)
    );
    println!("server during load: {stats}");
    // The slow readers stop at their next read timeout or bundle end: not waited for.
    drop(handles);
    Ok(())
}

fn idle(addr: &str, pid: u32, n: usize, secs: u64) -> Result<()> {
    let mut held = Vec::with_capacity(n);
    let mut failed = 0;
    for _ in 0..n {
        match TcpStream::connect(addr) {
            Ok(s) => held.push(s),
            Err(_) => failed += 1,
        }
    }
    std::thread::sleep(Duration::from_secs(1));
    let before = proc_stats(pid);
    let p = probe(addr, secs);
    let after = proc_stats(pid);
    let alive = held
        .iter()
        .filter(|s| {
            s.set_nonblocking(true).ok();
            let mut b = [0u8; 1];
            // Still open: would block; closed by the server: Ok(0).
            matches!((&**s).read(&mut b), Err(e) if e.kind() == std::io::ErrorKind::WouldBlock)
        })
        .count();
    println!(
        "idle connections={n} (connect failed {failed}), still open after {secs}s={alive}: {p}"
    );
    println!("server after connect: {before}");
    println!("server after {secs}s: {after}");
    Ok(())
}

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let [addr, pid, public, scenario, n, secs] = args.as_slice() else {
        bail!("usage: load <addr> <server pid> <store public dir> mix|slow|idle <n> <seconds>");
    };
    let (pid, n, secs): (u32, usize, u64) = (pid.parse()?, n.parse()?, secs.parse()?);
    let t = Arc::new(targets(Path::new(public))?);
    println!("server at rest: {}", proc_stats(pid));
    match scenario.as_str() {
        "mix" => mix(addr, pid, t, n, secs),
        "slow" => slow(addr, pid, t, n, secs),
        "idle" => idle(addr, pid, n, secs),
        _ => bail!("scenario: mix, slow or idle"),
    }
}
