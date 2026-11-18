//! No Overpass request (X5, #139): every OSM element the pipeline needs comes from the
//! local Geofabrik extracts (`skimap_osm::local`). The public Overpass instances refused
//! connections or answered 502/504 for hours (15 to 18/11/2026); a request to one of them
//! in the code of a crate would bring that failure back into `build-all`, `skimap refresh`
//! or the plans of the pistes.
//!
//! Scanned: the `src` and `examples` directories of every crate, outside the trailing
//! `#[cfg(test)] mod … {` of a file. Forbidden: a downloader request on the `overpass`
//! source and the addresses of the Overpass instances used so far, except as the
//! provenance of a saved Overpass dump read from disk (`OVERPASS_URL` of
//! `skimap_avalanche::historic`, never fetched).

use std::path::{Path, PathBuf};

/// Patterns of an Overpass request.
const FORBIDDEN: [&str; 7] = [
    "post_form(\"overpass\"",
    "Request::get(\"overpass\"",
    "Request::post(\"overpass\"",
    "maps.mail.ru",
    "overpass.kumi.systems",
    "overpass.private.coffee",
    "OVERPASS_MIRROR",
];

/// The only file allowed to name the main instance: the provenance of a saved dump.
const DUMP_PROVENANCE: &str = "crates/avalanche/src/historic/mod.rs";

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
        for pat in FORBIDDEN {
            if code.contains(pat) {
                found.push(format!("{rel}: {pat}"));
            }
        }
        if code.contains("overpass-api.de") && rel != DUMP_PROVENANCE {
            found.push(format!("{rel}: overpass-api.de"));
        }
    }
    assert!(
        found.is_empty(),
        "Overpass requests left:\n{}",
        found.join("\n")
    );
}
