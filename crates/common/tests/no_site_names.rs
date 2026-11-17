//! No site is a special case (X27, #161): the code of the app and of the pipeline names no
//! site. Every site-specific value lives in a data table (`crates/*/data/*.json`, the site
//! registry, the catalogue); the evaluation sites only appear in test data (tests,
//! fixtures, goldens, benches, examples, the `#[cfg(test)]` modules).
//!
//! Scanned: `crates/`, `android/*/src/main`, `deploy/`, `styles/`. Not scanned: test data
//! (directories `tests`, `fixtures`, `goldens`, `benches`, `examples`, `oracle`, `demo`, the
//! Android benchmark module, the trailing `#[cfg(test)] mod … {` of a Rust file), data
//! tables (directories `data`, the zone rules, the Android catalogue, the style schema
//! snapshot) and documentation (`.md`, `.html`).

use std::path::{Path, PathBuf};

/// Names of the evaluation sites, lower case, accents folded.
const NAMES: [&str; 3] = ["cervieres", "chapelle-des-bois", "chapelle_des_bois"];

/// Path components that mark test data.
const TEST_DIRS: [&str; 8] = [
    "tests", "fixtures", "goldens", "benches", "examples", "oracle", "demo", "target",
];

/// Data tables (site-specific values belong there), relative to the workspace.
const DATA_FILES: [&str; 4] = [
    "android/app/src/main/assets/catalog.json",
    "styles/pack-schema.json",
    "crates/zones/rules/rules.json",
    "android/benchmark",
];

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn fold(line: &str) -> String {
    line.to_lowercase()
        .replace(['è', 'é', 'ê'], "e")
        .replace(' ', "-")
}

fn scanned(rel: &Path) -> bool {
    let s = rel.to_string_lossy().replace('\\', "/");
    if DATA_FILES
        .iter()
        .any(|d| s == *d || s.starts_with(&format!("{d}/")))
    {
        return false;
    }
    if rel.components().any(|c| {
        TEST_DIRS.contains(&c.as_os_str().to_string_lossy().as_ref()) || c.as_os_str() == "data"
    }) {
        return false;
    }
    let ext = rel.extension().and_then(|e| e.to_str()).unwrap_or("");
    !matches!(
        ext,
        "md" | "html" | "png" | "jpg" | "webp" | "pmtiles" | "tif" | "gz" | "zip"
    )
}

fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for e in entries.flatten() {
        let p = e.path();
        if p.is_dir() {
            walk(&p, out);
        } else {
            out.push(p);
        }
    }
}

/// Lines naming a site in one file: `(line number, line)`.
fn offending(path: &Path, text: &str) -> Vec<(usize, String)> {
    let rust = path.extension().is_some_and(|e| e == "rs");
    let lines: Vec<&str> = text.lines().collect();
    let mut out = Vec::new();
    for (i, l) in lines.iter().enumerate() {
        if rust && l.starts_with("#[cfg(test)]") {
            let next = lines
                .get(i + 1)
                .map_or("", |n| n.trim_start_matches("pub(crate) "));
            if next.starts_with("mod ") && next.ends_with('{') {
                break;
            }
        }
        let f = fold(l);
        if NAMES.iter().any(|n| f.contains(n)) {
            out.push((i + 1, l.trim().to_owned()));
        }
    }
    out
}

#[test]
fn no_site_name_outside_test_data() {
    let root = root();
    let mut files = Vec::new();
    walk(&root.join("crates"), &mut files);
    walk(&root.join("deploy"), &mut files);
    walk(&root.join("styles"), &mut files);
    for m in std::fs::read_dir(root.join("android")).unwrap().flatten() {
        walk(&m.path().join("src/main"), &mut files);
    }
    assert!(files.len() > 500, "{} files: wrong root?", files.len());
    let mut found = Vec::new();
    for f in &files {
        let rel = f.strip_prefix(&root).unwrap();
        if !scanned(rel) {
            continue;
        }
        let Ok(text) = std::fs::read_to_string(f) else {
            continue;
        };
        for (n, l) in offending(rel, &text) {
            found.push(format!("{}:{n}: {l}", rel.display()));
        }
    }
    assert!(
        found.is_empty(),
        "site names outside test data (move the value to a data table, or make the comment generic):\n{}",
        found.join("\n")
    );
}

#[test]
fn the_guard_sees_a_site_name() {
    let code = "fn f() {}\n// Cervières\nlet s = \"chapelle-des-bois\";\n#[cfg(test)]\nmod tests {\n    // cervieres\n}\n";
    let found = offending(Path::new("x.rs"), code);
    assert_eq!(found.iter().map(|f| f.0).collect::<Vec<_>>(), [2, 3]);
    // The common noun is not a site name.
    assert!(offending(Path::new("x.rs"), "\"chapelle\", \"village\"").is_empty());
    assert!(!scanned(Path::new("crates/x/tests/a.rs")));
    assert!(!scanned(Path::new("crates/common/data/sites.json")));
    assert!(scanned(Path::new("crates/x/src/a.rs")));
    assert!(scanned(Path::new("deploy/skimap-job")));
}
