//! Helpers of the integration tests: a raw HTTP/1.1 client and synthetic packs.

#![allow(dead_code)]

use std::io::{Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::path::Path;

use serde_json::json;
use skimap_pack::update::sha256_hex;

/// A response as read from the wire.
#[derive(Debug)]
pub struct Resp {
    pub status: u16,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

impl Resp {
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(name))
            .map(|(_, v)| v.as_str())
    }

    pub fn json(&self) -> serde_json::Value {
        serde_json::from_slice(&self.body)
            .unwrap_or_else(|e| panic!("{e}: {}", String::from_utf8_lossy(&self.body)))
    }
}

/// One request on a fresh connection, `Connection: close`; reads the whole response.
pub fn request(
    addr: SocketAddr,
    method: &str,
    target: &str,
    headers: &[(&str, &str)],
    body: &[u8],
) -> Resp {
    let mut s = TcpStream::connect(addr).unwrap();
    let mut head = format!("{method} {target} HTTP/1.1\r\nHost: test\r\nConnection: close\r\n");
    for (k, v) in headers {
        head.push_str(&format!("{k}: {v}\r\n"));
    }
    if !body.is_empty() || method == "POST" {
        head.push_str(&format!("Content-Length: {}\r\n", body.len()));
    }
    head.push_str("\r\n");
    s.write_all(head.as_bytes()).unwrap();
    s.write_all(body).unwrap();
    let mut raw = Vec::new();
    s.read_to_end(&mut raw).unwrap();
    let split = raw
        .windows(4)
        .position(|w| w == b"\r\n\r\n")
        .expect("end of headers");
    let head = String::from_utf8_lossy(&raw[..split]).into_owned();
    let mut lines = head.split("\r\n");
    let status: u16 = lines
        .next()
        .unwrap()
        .split(' ')
        .nth(1)
        .unwrap()
        .parse()
        .unwrap();
    let headers: Vec<(String, String)> = lines
        .filter_map(|l| l.split_once(':'))
        .map(|(k, v)| (k.trim().to_owned(), v.trim().to_owned()))
        .collect();
    let body = raw[split + 4..].to_vec();
    let r = Resp {
        status,
        headers,
        body,
    };
    if method != "HEAD"
        && let Some(n) = r.header("content-length")
    {
        assert_eq!(n.parse::<usize>().unwrap(), r.body.len(), "Content-Length");
    }
    r
}

pub fn get(addr: SocketAddr, target: &str) -> Resp {
    request(addr, "GET", target, &[], b"")
}

/// Writes a pack (manifest with sizes and SHA-256) holding these files and two layers,
/// `pistes` (`layers/pistes.json`) and `bera` (`layers/bera.json`).
pub fn make_pack(dir: &Path, version: &str, files: &[(&str, &[u8])]) -> Vec<u8> {
    let _ = std::fs::remove_dir_all(dir);
    let mut entries = Vec::new();
    let mut total = 0u64;
    for (path, body) in files {
        let p = dir.join(path);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(&p, body).unwrap();
        entries.push(json!({"path": path, "bytes": body.len(), "sha256": sha256_hex(body)}));
        total += body.len() as u64;
    }
    let layer = |id: &str, file: &str, until: &str| {
        json!({"id": id, "title": id, "kind": "data", "format": "json", "file": file,
               "origin": id, "generated_at": "2026-11-15T00:00:00Z", "valid_from": null,
               "valid_until": until, "validity": "un jour", "sources": [],
               "personal_use_only": false, "bytes": 1, "visible": false})
    };
    let m = json!({
        "format": "skimap-pack", "format_version": 1, "pack_version": version,
        "generator": "test", "generated_at": "2026-11-15T00:00:00Z",
        "site": {"id": "cervieres", "name": "Cervières", "department": "05",
                 "bbox": [6.66, 44.77, 6.86, 44.92], "center": [6.77, 44.85]},
        "valid_for": {"until": "2026-11-16", "until_layer": "bera", "note": "n"},
        "personal_sources": false, "basemaps": [],
        "layers": [layer("pistes", "layers/pistes.json", "2026-12-31"),
                   layer("bera", "layers/bera.json", "2026-11-16")],
        "voice": {"status": "absent", "clips": 0, "bytes": 0, "note": "n"},
        "attributions": [], "files": entries, "total_bytes": total
    });
    let bytes = serde_json::to_vec_pretty(&m).unwrap();
    std::fs::write(dir.join("manifest.json"), &bytes).unwrap();
    bytes
}
