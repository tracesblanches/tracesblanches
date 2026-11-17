//! Publication of the France basemap (X20, #154): one for every site, precomputed by the
//! pipeline (`skimap_pack::build::france`), served as static files.
//!
//! ```text
//! <store>/public/basemap/france.json            description (short cache, ETag)
//! <store>/public/basemap/files/<sha256>         its files, immutable (name = SHA-256)
//! <store>/basemap/france.prev.json              previous description: its files are kept
//!                                               one more publication (downloads in flight)
//! ```
//!
//! The files are written, then the description that names them (by rename): a phone never
//! sees a description whose files are missing. A description equal to the published one
//! publishes nothing (the phone downloads nothing).

use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use serde::Serialize;
use skimap_pack::france::{DESCRIPTION_FILE, FranceBasemap};

use crate::store::{Store, valid_sha};

/// What [`publish`] did.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct FrancePublished {
    /// `published` or `unchanged`.
    pub status: String,
    /// Protomaps build of the description.
    pub build: String,
    /// Date of the data.
    pub data_date: String,
    /// Files of the description, bytes of all of them.
    pub files: usize,
    /// Total size.
    pub bytes: u64,
    /// Files written this time (the others were already published).
    pub written: usize,
    /// Files of older descriptions removed.
    pub removed: usize,
}

/// Served path of the description.
pub fn description_path(store: &Store) -> PathBuf {
    store.public().join("basemap").join(DESCRIPTION_FILE)
}

/// Served path of a file of the description, `None` for a malformed name.
pub fn file_path(store: &Store, sha: &str) -> Option<PathBuf> {
    valid_sha(sha).then(|| files_dir(store).join(sha))
}

fn files_dir(store: &Store) -> PathBuf {
    store.public().join("basemap").join("files")
}

fn prev_path(store: &Store) -> PathBuf {
    store.root().join("basemap").join("france.prev.json")
}

fn write_atomic(path: &Path, bytes: &[u8]) -> Result<()> {
    if let Some(p) = path.parent() {
        fs::create_dir_all(p)?;
    }
    let tmp = path.with_extension("tmp");
    fs::write(&tmp, bytes).with_context(|| format!("{}", tmp.display()))?;
    fs::rename(&tmp, path).with_context(|| format!("{}", path.display()))
}

fn read(path: &Path) -> Result<Option<FranceBasemap>> {
    match fs::read(path) {
        Ok(b) => Ok(Some(
            FranceBasemap::parse(&b).with_context(|| format!("{}", path.display()))?,
        )),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e.into()),
    }
}

/// Publishes the France basemap of `dir` (`france.json` and its files, checked).
pub fn publish(store: &Store, dir: &Path) -> Result<FrancePublished> {
    let desc = FranceBasemap::read(dir).with_context(|| format!("{}", dir.display()))?;
    desc.verify(dir)
        .with_context(|| format!("{}", dir.display()))?;
    let lock_file = fs::File::create(store.root().join(".lock"))?;
    lock_file.lock()?;
    let served = description_path(store);
    let current = read(&served)?;
    let mut out = FrancePublished {
        status: "unchanged".into(),
        build: desc.build.clone(),
        data_date: desc.data_date.clone(),
        files: desc.files.len(),
        bytes: desc.bytes(),
        written: 0,
        removed: 0,
    };
    if current.as_ref() == Some(&desc) {
        return Ok(out);
    }
    let files = files_dir(store);
    fs::create_dir_all(&files)?;
    for f in &desc.files {
        if !valid_sha(&f.sha256) {
            bail!("{}: SHA-256 not in lowercase hex", f.path);
        }
        let to = files.join(&f.sha256);
        if !to.is_file() {
            let tmp = to.with_extension("tmp");
            fs::copy(dir.join(&f.path), &tmp)
                .with_context(|| format!("{}", dir.join(&f.path).display()))?;
            fs::rename(&tmp, &to)?;
            out.written += 1;
        }
    }
    let body = serde_json::to_vec_pretty(&desc)?;
    // The description in use becomes the previous one; its files stay one publication.
    if let Some(cur) = &current {
        write_atomic(&prev_path(store), &serde_json::to_vec_pretty(cur)?)?;
    }
    write_atomic(&served, &body)?;
    out.status = "published".into();
    let keep: HashSet<String> = desc
        .files
        .iter()
        .chain(current.iter().flat_map(|c| c.files.iter()))
        .map(|f| f.sha256.clone())
        .collect();
    for e in fs::read_dir(&files)? {
        let e = e?;
        let name = e.file_name().to_string_lossy().into_owned();
        if !keep.contains(&name) {
            fs::remove_file(e.path())?;
            out.removed += 1;
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use skimap_pack::france::{FORMAT, FORMAT_VERSION, TILES_FILE};
    use skimap_pack::manifest::{FileEntry, Source};

    fn basemap(dir: &Path, tiles: &[u8], build: &str) {
        fs::create_dir_all(dir.join("fonts/A")).unwrap();
        fs::write(dir.join(TILES_FILE), tiles).unwrap();
        fs::write(dir.join("fonts/A/0-255.pbf"), b"glyphs").unwrap();
        let entry = |rel: &str| {
            let (bytes, sha256) = skimap_pack::reader::sha256_file(&dir.join(rel)).unwrap();
            FileEntry {
                path: rel.to_owned(),
                bytes,
                sha256,
            }
        };
        let d = FranceBasemap {
            format: FORMAT.into(),
            format_version: FORMAT_VERSION,
            build: build.into(),
            schema: "4".into(),
            data_date: "2026-11-17".into(),
            bbox: [-5.5, 41.2, 9.8, 51.3],
            minzoom: 0,
            maxzoom: 6,
            files: vec![entry("fonts/A/0-255.pbf"), entry(TILES_FILE)],
            sources: vec![Source::new("Protomaps", "https://x", "ODbL", "© OSM")],
        };
        fs::write(dir.join(DESCRIPTION_FILE), serde_json::to_vec(&d).unwrap()).unwrap();
    }

    #[test]
    fn publishes_files_then_description_and_keeps_the_previous_one() {
        let tmp = tempfile::tempdir().unwrap();
        let store = Store::open(tmp.path().join("store")).unwrap();
        let (a, b, c) = (
            tmp.path().join("a"),
            tmp.path().join("b"),
            tmp.path().join("c"),
        );
        basemap(&a, b"tiles a", "20261110.pmtiles");
        basemap(&b, b"tiles b", "20261117.pmtiles");
        basemap(&c, b"tiles c", "20261124.pmtiles");

        let r = publish(&store, &a).unwrap();
        assert_eq!(
            (r.status.as_str(), r.written, r.removed),
            ("published", 2, 0)
        );
        let served = read(&description_path(&store)).unwrap().unwrap();
        for f in &served.files {
            assert!(file_path(&store, &f.sha256).unwrap().is_file());
        }
        // Same again: nothing.
        let r = publish(&store, &a).unwrap();
        assert_eq!((r.status.as_str(), r.written), ("unchanged", 0));
        // New tiles, same glyphs: one file written, the old tiles kept one publication.
        let r = publish(&store, &b).unwrap();
        assert_eq!(
            (r.status.as_str(), r.written, r.removed),
            ("published", 1, 0)
        );
        let r = publish(&store, &c).unwrap();
        assert_eq!((r.written, r.removed), (1, 1));
        assert_eq!(fs::read_dir(files_dir(&store)).unwrap().count(), 3);
        assert!(file_path(&store, "../x").is_none());
    }

    #[test]
    fn refuses_files_that_differ_from_the_description() {
        let tmp = tempfile::tempdir().unwrap();
        let store = Store::open(tmp.path().join("store")).unwrap();
        let a = tmp.path().join("a");
        basemap(&a, b"tiles a", "20261110.pmtiles");
        fs::write(a.join(TILES_FILE), b"other").unwrap();
        assert!(publish(&store, &a).is_err());
        assert!(!description_path(&store).exists());
    }
}
