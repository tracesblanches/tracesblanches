//! Limits of the HTTP server under many phones (#152): a slow download does not hold the
//! workers, small replies are not held back by Nagle's algorithm, idle connections are
//! closed, downloads beyond the cap are refused with a retry delay.

#![allow(
    clippy::disallowed_methods,
    reason = "tests write their inputs directly"
)]

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use skimap_server::http::{Handler, Limits, Reply, Req, file_reply};

/// `/big`: a large file (a bundle); `/index`: a 25 kB file (a site index); `/small`:
/// 25 kB in memory.
struct Files {
    big: PathBuf,
    index: PathBuf,
}

impl Handler for Files {
    fn handle(&self, req: &Req) -> Reply {
        match req.path.as_str() {
            "/big" => file_reply(req, &self.big, "\"big\"", "application/octet-stream", "c"),
            "/index" => file_reply(req, &self.index, "\"i\"", "application/json", "c"),
            _ => Reply::bytes(200, "application/json", vec![b'x'; 25_000]),
        }
    }
}

fn big_file(dir: &tempfile::TempDir) -> PathBuf {
    let p = dir.path().join("big");
    std::fs::write(&p, vec![7u8; 64 << 20]).unwrap();
    p
}

fn index_file(dir: &tempfile::TempDir) -> PathBuf {
    let p = dir.path().join("index.json");
    std::fs::write(&p, vec![b'x'; 25_000]).unwrap();
    p
}

/// Sends `GET target` on `s` (keep-alive) and reads the response; its status.
fn get(s: &mut BufReader<TcpStream>, target: &str) -> std::io::Result<u16> {
    s.get_mut()
        .write_all(format!("GET {target} HTTP/1.1\r\nHost: t\r\n\r\n").as_bytes())?;
    let mut line = String::new();
    s.read_line(&mut line)?;
    let status = line
        .split(' ')
        .nth(1)
        .and_then(|x| x.parse().ok())
        .unwrap_or(0);
    let mut len = 0usize;
    loop {
        line.clear();
        s.read_line(&mut line)?;
        if line.trim_end().is_empty() {
            break;
        }
        if let Some((k, v)) = line.split_once(':')
            && k.eq_ignore_ascii_case("content-length")
        {
            len = v.trim().parse().unwrap();
        }
    }
    let mut body = vec![0u8; len];
    s.read_exact(&mut body)?;
    Ok(status)
}

fn connect(addr: SocketAddr) -> BufReader<TcpStream> {
    let s = TcpStream::connect(addr).unwrap();
    s.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
    BufReader::new(s)
}

#[test]
fn a_stalled_download_does_not_block_other_requests() {
    let dir = tempfile::tempdir().unwrap();
    let handler = Arc::new(Files {
        big: big_file(&dir),
        index: index_file(&dir),
    });
    // One worker: before #152 it wrote the whole download itself.
    let running = skimap_server::http::serve("127.0.0.1:0", 1, handler).unwrap();
    let addr = running.addr;
    // A phone that asks for a bundle and stops reading (weak network).
    let mut stalled = TcpStream::connect(addr).unwrap();
    stalled
        .write_all(b"GET /big HTTP/1.1\r\nHost: t\r\n\r\n")
        .unwrap();
    std::thread::sleep(Duration::from_millis(300));
    let t = Instant::now();
    let status = get(&mut connect(addr), "/small");
    assert!(
        matches!(status, Ok(200)),
        "index during a stalled download: {status:?} after {:?}",
        t.elapsed()
    );
    drop(stalled);
    running.stop();
}

#[test]
fn small_replies_are_not_delayed_by_nagle() {
    let dir = tempfile::tempdir().unwrap();
    let handler = Arc::new(Files {
        big: big_file(&dir),
        index: index_file(&dir),
    });
    let running = skimap_server::http::serve("127.0.0.1:0", 2, handler).unwrap();
    let mut s = connect(running.addr);
    get(&mut s, "/small").unwrap();
    // 20 indexes on one keep-alive connection: well under a millisecond each without
    // delayed acknowledgements, 40 ms each with them.
    let t = Instant::now();
    for _ in 0..20 {
        assert_eq!(get(&mut s, "/small").unwrap(), 200);
    }
    let el = t.elapsed();
    assert!(el < Duration::from_millis(200), "20 indexes took {el:?}");
    running.stop();
}

#[test]
fn idle_connections_are_closed() {
    let dir = tempfile::tempdir().unwrap();
    let handler = Arc::new(Files {
        big: big_file(&dir),
        index: index_file(&dir),
    });
    let limits = Limits {
        workers: 1,
        io_timeout: Duration::from_millis(300),
        ..Limits::default()
    };
    let running = skimap_server::http::serve_with("127.0.0.1:0", limits, handler).unwrap();
    // A silent connection, and one that sent a request then nothing more (keep-alive).
    let mut silent = TcpStream::connect(running.addr).unwrap();
    let mut kept = connect(running.addr);
    assert_eq!(get(&mut kept, "/small").unwrap(), 200);
    std::thread::sleep(Duration::from_millis(1000));
    for s in [&mut silent, kept.get_mut()] {
        s.set_read_timeout(Some(Duration::from_secs(2))).unwrap();
        let mut b = [0u8; 256];
        // Closed by the server: end of stream (possibly after a 408 reply).
        let mut n = s.read(&mut b).unwrap();
        while n > 0 {
            n = s.read(&mut b).unwrap();
        }
    }
    running.stop();
}

#[test]
fn downloads_beyond_the_cap_wait_for_a_retry() {
    let dir = tempfile::tempdir().unwrap();
    let handler = Arc::new(Files {
        big: big_file(&dir),
        index: index_file(&dir),
    });
    let limits = Limits {
        workers: 2,
        max_downloads: 1,
        ..Limits::default()
    };
    let running = skimap_server::http::serve_with("127.0.0.1:0", limits, handler).unwrap();
    let mut stalled = TcpStream::connect(running.addr).unwrap();
    stalled
        .write_all(b"GET /big HTTP/1.1\r\nHost: t\r\n\r\n")
        .unwrap();
    std::thread::sleep(Duration::from_millis(300));
    let mut s = connect(running.addr);
    s.get_mut()
        .write_all(b"GET /big HTTP/1.1\r\nHost: t\r\n\r\n")
        .unwrap();
    let mut head = String::new();
    while !head.ends_with("\r\n\r\n") {
        s.read_line(&mut head).unwrap();
    }
    assert!(head.starts_with("HTTP/1.1 503"), "{head}");
    assert!(head.contains("Retry-After: 30"), "{head}");
    // The in-memory replies and the small files still flow.
    assert_eq!(get(&mut connect(running.addr), "/small").unwrap(), 200);
    assert_eq!(get(&mut connect(running.addr), "/index").unwrap(), 200);
    drop(stalled);
    running.stop();
}
