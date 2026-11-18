//! No Overpass request (X5, #139): every OSM element the pipeline needs comes from the
//! local Geofabrik extracts (`skimap_osm::local`). The public Overpass instances refused
//! connections or answered 502/504 for hours (15 to 18/11/2026); a request to one of them
//! in the code of a crate would bring that failure back into `build-all`, `skimap refresh`
//! or the plans of the pistes.
//!
//! Scanned: the `src` and `examples` directories of every crate, outside the trailing
//! `#[cfg(test)] mod … {` of a file. Forbidden: a downloader request on the `overpass`
//! source that may reach the network (allowed: a read of an answer already in the cache,
//! `CachePolicy::Offline` in the same statement) and the addresses of the mirrors used
//! so far. The main instance's address stays as a cache key or a provenance (a saved
//! dump read from disk), never fetched.

use std::path::{Path, PathBuf};

/// Downloader requests on the Overpass source.
const REQUESTS: [&str; 3] = [
    "post_form(\"overpass\"",
    "Request::get(\"overpass\"",
    "Request::post(\"overpass\"",
];

/// Overpass mirrors.
const MIRRORS: [&str; 4] = [
    "maps.mail.ru",
    "overpass.kumi.systems",
    "overpass.private.coffee",
    "OVERPASS_MIRROR",
];

/// Whether every request of `code` on the Overpass source is an offline cache read (the
/// statement, up to its `;`, says `CachePolicy::Offline`).
fn network_requests(code: &str) -> Vec<&'static str> {
    let mut out = Vec::new();
    for pat in REQUESTS {
        for (i, _) in code.match_indices(pat) {
            let stmt = code[i..].split(';').next().unwrap_or_default();
            if !stmt.contains("CachePolicy::Offline") {
                out.push(pat);
            }
        }
    }
    out
}

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .expect("workspace root")
        .to_owned()
}

fn rust_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(rd) = std::fs::read_dir(dir) else {
        return;
    };
    for e in rd.flatten() {
        let p = e.path();
        if p.is_dir() {
            rust_files(&p, out);
        } else if p.extension().is_some_and(|x| x == "rs") {
            out.push(p);
        }
    }
}

/// The file without its trailing test module.
fn without_tests(text: &str) -> &str {
    text.find("#[cfg(test)]\nmod ").map_or(text, |i| &text[..i])
}

#[test]
fn offline_reads_are_told_from_requests() {
    let read = r#"dl.fetch(&Request::post_form("overpass", U, &[("data", &q)])
            .cache(skimap_common::download::CachePolicy::Offline));"#;
    let req = r#"dl.fetch(&Request::post_form("overpass", U, &[("data", &q)]))?;"#;
    assert!(network_requests(read).is_empty());
    assert_eq!(network_requests(req).len(), 1);
}

#[test]
fn no_crate_sends_an_overpass_request() {
    let root = root();
    let mut files = Vec::new();
    let crates = std::fs::read_dir(root.join("crates")).expect("crates/");
    for c in crates.flatten() {
        rust_files(&c.path().join("src"), &mut files);
        rust_files(&c.path().join("examples"), &mut files);
    }
    assert!(files.len() > 100, "{} files scanned", files.len());
    let mut found = Vec::new();
    for f in &files {
        let text = std::fs::read_to_string(f).unwrap();
        let code = without_tests(&text);
        let rel = f
            .strip_prefix(&root)
            .unwrap()
            .to_string_lossy()
            .into_owned();
        for pat in network_requests(code) {
            found.push(format!("{rel}: {pat}"));
        }
        for pat in MIRRORS {
            if code.contains(pat) {
                found.push(format!("{rel}: {pat}"));
            }
        }
    }
    assert!(
        found.is_empty(),
        "Overpass requests left:\n{}",
        found.join("\n")
    );
}
