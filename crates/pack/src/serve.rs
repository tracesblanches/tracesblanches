//! Tiny static file server for the viewer (P15), with HTTP range requests.
//!
//! PMTiles are read by byte ranges: `python3 -m http.server` ignores the `Range` header
//! and the PMTiles client of the viewer refuses it. This server answers `GET` and
//! `HEAD` with `206 Partial Content` for single ranges, keeps connections alive, and
//! serves nothing outside its root (no `..`, no absolute paths; symbolic links inside
//! the root are followed, such as a `data` link to the data directory).
//!
//! Standard library only; meant for a developer machine (`127.0.0.1`, or the phone
//! through `adb reverse`), not for the Internet.

use std::fs::File;
use std::io::{BufRead, BufReader, Read, Seek, SeekFrom, Write};
use std::net::{TcpListener, TcpStream, ToSocketAddrs};
use std::path::{Component, Path, PathBuf};

use crate::error::{Error, Result};

/// Content type by file extension.
pub fn content_type(path: &Path) -> &'static str {
    match path.extension().and_then(|e| e.to_str()).unwrap_or("") {
        "html" => "text/html; charset=utf-8",
        "js" | "mjs" => "text/javascript; charset=utf-8",
        "css" => "text/css; charset=utf-8",
        "json" => "application/json",
        "geojson" => "application/geo+json",
        "png" => "image/png",
        "pbf" => "application/x-protobuf",
        "md" | "txt" => "text/plain; charset=utf-8",
        "gpx" | "kml" => "application/xml",
        "opus" => "audio/ogg",
        _ => "application/octet-stream",
    }
}

/// Decodes `%XX` escapes of a URL path.
pub fn percent_decode(s: &str) -> Option<String> {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' {
            let h = s.get(i + 1..i + 3)?;
            out.push(u8::from_str_radix(h, 16).ok()?);
            i += 3;
        } else {
            out.push(b[i]);
            i += 1;
        }
    }
    String::from_utf8(out).ok()
}

/// File under `root` for a request target, `None` if it leaves the root.
pub fn resolve(root: &Path, target: &str) -> Option<PathBuf> {
    let path = target.split(['?', '#']).next().unwrap_or("");
    let decoded = percent_decode(path)?;
    let rel = decoded.trim_start_matches('/');
    let mut out = root.to_path_buf();
    for c in Path::new(rel).components() {
        match c {
            Component::Normal(p) => out.push(p),
            Component::CurDir => {}
            _ => return None,
        }
    }
    if out.is_dir() {
        out.push("index.html");
    }
    Some(out)
}

/// Parses a single-range `Range` header against a file length: `(start, end)` inclusive.
/// `Err(())`: unsatisfiable or malformed.
#[allow(clippy::result_unit_err)]
pub fn parse_range(h: &str, len: u64) -> std::result::Result<(u64, u64), ()> {
    let spec = h.trim().strip_prefix("bytes=").ok_or(())?;
    if spec.contains(',') || len == 0 {
        return Err(());
    }
    let (a, b) = spec.split_once('-').ok_or(())?;
    let (a, b) = (a.trim(), b.trim());
    let (start, end) = if a.is_empty() {
        let n: u64 = b.parse().map_err(|_| ())?;
        if n == 0 {
            return Err(());
        }
        (len.saturating_sub(n), len - 1)
    } else {
        let s: u64 = a.parse().map_err(|_| ())?;
        let e: u64 = if b.is_empty() {
            len - 1
        } else {
            b.parse::<u64>().map_err(|_| ())?.min(len - 1)
        };
        (s, e)
    };
    if start > end || start >= len {
        return Err(());
    }
    Ok((start, end))
}

fn respond_status(w: &mut TcpStream, code: u16, reason: &str, extra: &str) -> std::io::Result<()> {
    let body = format!("{code} {reason}\n");
    write!(
        w,
        "HTTP/1.1 {code} {reason}\r\nContent-Type: text/plain\r\nContent-Length: {}\r\n{extra}\r\n{body}",
        body.len()
    )
}

fn handle(root: &Path, stream: TcpStream) -> std::io::Result<()> {
    let mut reader = BufReader::new(stream.try_clone()?);
    let mut w = stream;
    loop {
        let mut line = String::new();
        if reader.read_line(&mut line)? == 0 {
            return Ok(());
        }
        let mut parts = line.split_whitespace();
        let (method, target, version) = (
            parts.next().unwrap_or("").to_owned(),
            parts.next().unwrap_or("").to_owned(),
            parts.next().unwrap_or("HTTP/1.0").to_owned(),
        );
        let mut range = None;
        let mut close = version == "HTTP/1.0";
        loop {
            let mut h = String::new();
            if reader.read_line(&mut h)? == 0 {
                return Ok(());
            }
            let h = h.trim_end();
            if h.is_empty() {
                break;
            }
            if let Some((k, v)) = h.split_once(':') {
                match k.trim().to_ascii_lowercase().as_str() {
                    "range" => range = Some(v.trim().to_owned()),
                    "connection" => close = v.trim().eq_ignore_ascii_case("close"),
                    _ => {}
                }
            }
        }
        let head = method == "HEAD";
        if method != "GET" && !head {
            respond_status(&mut w, 405, "Method Not Allowed", "Allow: GET, HEAD\r\n")?;
        } else {
            match resolve(root, &target).filter(|p| p.is_file()) {
                None => respond_status(&mut w, 404, "Not Found", "")?,
                Some(p) => serve_file(&mut w, &p, range.as_deref(), head)?,
            }
        }
        w.flush()?;
        if close {
            return Ok(());
        }
    }
}

fn serve_file(w: &mut TcpStream, p: &Path, range: Option<&str>, head: bool) -> std::io::Result<()> {
    let mut f = File::open(p)?;
    let len = f.metadata()?.len();
    let common = format!(
        "Content-Type: {}\r\nAccept-Ranges: bytes\r\nCache-Control: no-cache\r\nAccess-Control-Allow-Origin: *\r\n",
        content_type(p)
    );
    let (status, start, end) = match range {
        None => ("200 OK", 0, len.saturating_sub(1)),
        Some(r) => match parse_range(r, len) {
            Ok((s, e)) => ("206 Partial Content", s, e),
            Err(()) => {
                return respond_status(
                    w,
                    416,
                    "Range Not Satisfiable",
                    &format!("Content-Range: bytes */{len}\r\n"),
                );
            }
        },
    };
    let n = if len == 0 { 0 } else { end - start + 1 };
    let mut hdr = format!("HTTP/1.1 {status}\r\n{common}Content-Length: {n}\r\n");
    if range.is_some() {
        hdr.push_str(&format!("Content-Range: bytes {start}-{end}/{len}\r\n"));
    }
    hdr.push_str("\r\n");
    w.write_all(hdr.as_bytes())?;
    if !head && n > 0 {
        f.seek(SeekFrom::Start(start))?;
        std::io::copy(&mut (&mut f).take(n), w)?;
    }
    Ok(())
}

/// Serves `root` on `addr` until the process is stopped (one thread per connection).
pub fn serve(root: &Path, addr: impl ToSocketAddrs) -> Result<()> {
    let listener = TcpListener::bind(addr).map_err(|e| Error::io(root, e))?;
    let local = listener.local_addr().map_err(|e| Error::io(root, e))?;
    tracing::info!(%local, root = %root.display(), "serving");
    for stream in listener.incoming() {
        let Ok(stream) = stream else { continue };
        let root = root.to_path_buf();
        std::thread::spawn(move || {
            if let Err(e) = handle(&root, stream) {
                tracing::debug!(%e, "connection closed");
            }
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ranges() {
        assert_eq!(parse_range("bytes=0-99", 1000), Ok((0, 99)));
        assert_eq!(parse_range("bytes=900-", 1000), Ok((900, 999)));
        assert_eq!(parse_range("bytes=-100", 1000), Ok((900, 999)));
        assert_eq!(parse_range("bytes=990-2000", 1000), Ok((990, 999)));
        assert_eq!(parse_range("bytes=1000-", 1000), Err(()));
        assert_eq!(parse_range("bytes=5-1", 1000), Err(()));
        assert_eq!(parse_range("bytes=0-1,5-6", 1000), Err(()));
        assert_eq!(parse_range("items=0-1", 1000), Err(()));
    }

    #[test]
    fn paths_stay_inside_the_root() {
        let root = Path::new("/srv/x");
        assert_eq!(
            resolve(root, "/a/b%20c.json?x=1"),
            Some(PathBuf::from("/srv/x/a/b c.json"))
        );
        assert_eq!(resolve(root, "/../etc/passwd"), None);
        assert_eq!(resolve(root, "/a/%2e%2e/%2e%2e/etc"), None);
        assert_eq!(resolve(root, "/%zz"), None);
    }

    #[test]
    fn serves_ranges_over_tcp() {
        let dir = tempfile::tempdir().unwrap();
        let data: Vec<u8> = (0..=255u8).cycle().take(5000).collect();
        std::fs::write(dir.path().join("a.pmtiles"), &data).unwrap();
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let root = dir.path().to_path_buf();
        std::thread::spawn(move || {
            for s in listener.incoming().take(1) {
                handle(&root, s.unwrap()).unwrap();
            }
        });
        let mut s = TcpStream::connect(addr).unwrap();
        // Two requests on one connection, then a 404 that closes it.
        write!(
            s,
            "GET /a.pmtiles HTTP/1.1\r\nHost: x\r\nRange: bytes=10-19\r\n\r\n"
        )
        .unwrap();
        write!(s, "HEAD /a.pmtiles HTTP/1.1\r\nHost: x\r\n\r\n").unwrap();
        write!(
            s,
            "GET /nope HTTP/1.1\r\nHost: x\r\nConnection: close\r\n\r\n"
        )
        .unwrap();
        let mut out = Vec::new();
        s.read_to_end(&mut out).unwrap();
        let text = String::from_utf8_lossy(&out);
        assert!(text.starts_with("HTTP/1.1 206 Partial Content\r\n"));
        assert!(text.contains("Content-Range: bytes 10-19/5000\r\n"));
        let body_at = text.find("\r\n\r\n").unwrap() + 4;
        assert_eq!(&out[body_at..body_at + 10], &data[10..20]);
        assert!(text.contains("HTTP/1.1 200 OK\r\n") && text.contains("Content-Length: 5000\r\n"));
        assert!(text.contains("HTTP/1.1 404 Not Found"));
    }
}
