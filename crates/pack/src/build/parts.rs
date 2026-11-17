//! Reuse of the heavy parts of the previous pack (#164).
//!
//! A pack is rebuilt whenever one of its layers changes: every AROME run (four a day)
//! changes the weather of every site. Most of the pack does not depend on the weather,
//! and some parts are slow to make: the slope and avalanche exposure polygons
//! ([`super::class_tiles`], up to minutes for a large mountain site), the snow polygons,
//! the OSM basemap (Protomaps extract and comparison with the build in use) and the
//! Plan IGN basemap. Each of those parts has a key, a SHA-256 over everything its output
//! depends on (fingerprint of its input files, parameters, code revision
//! [`PARTS_REVISION`]), never a date. A new build whose part has the key of the previous
//! pack takes that part's files from the previous pack (hard link, copy when the file
//! system refuses) and its manifest entry, instead of making it again: same bytes, so the
//! phone downloads nothing for it, and the date shown stays the date of its data.
//!
//! The keys of the pack in place are in `data/out/<site>/pack.parts.json` (next to the
//! pack, not in it: the phone never sees it), written after the new pack is swapped in.
//! A part is taken over only when every one of its files in the previous pack still has
//! the size and SHA-256 recorded with the key (read again): a pack altered or replaced
//! since (interrupted build, copy from elsewhere) has its parts made again.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};

use crate::build::create_dir;
use crate::error::{Error, Result};
use crate::manifest::FileEntry;
use crate::reader::sha256_file;

/// File of the part keys, in the site directory next to `pack/`.
pub const PARTS_FILE: &str = "pack.parts.json";

/// Revision of the code of the reusable parts: bump it when a change of the code makes
/// different bytes from the same inputs (vectorisation, archive writing, basemap
/// assembly), so that every pack makes its parts again once.
pub const PARTS_REVISION: u32 = 1;

/// One part of a pack: its key, its files and its manifest entry.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Part {
    /// SHA-256 of the inputs ([`Key`]).
    pub key: String,
    /// Files of the part, relative to the pack root, with size and SHA-256.
    pub files: Vec<FileEntry>,
    /// Manifest entry of the part (`Layer` or `Basemap`, JSON).
    pub entry: Value,
}

/// Contents of [`PARTS_FILE`].
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Record {
    /// Pack version these parts belong to.
    pub pack_version: String,
    /// Parts by identifier (`basemap:osm`, `class:slope`…).
    pub parts: BTreeMap<String, Part>,
}

/// Key of a part: SHA-256 over labelled values, in the order given.
#[derive(Debug, Clone)]
pub struct Key(Sha256);

impl Key {
    /// A key for the part `id`, with the crate version and [`PARTS_REVISION`].
    pub fn new(id: &str) -> Self {
        let mut k = Self(Sha256::new());
        k.add("part", id);
        k.add("crate", env!("CARGO_PKG_VERSION"));
        k.add("revision", &PARTS_REVISION.to_string());
        k
    }

    /// Adds a labelled value.
    pub fn add(&mut self, label: &str, value: &str) -> &mut Self {
        for p in [label.as_bytes(), value.as_bytes()] {
            self.0.update((p.len() as u64).to_le_bytes());
            self.0.update(p);
        }
        self
    }

    /// Adds the fingerprint of a file (size and SHA-256), or its absence.
    pub fn add_file(&mut self, label: &str, path: &Path) -> Result<&mut Self> {
        let v = if path.is_file() {
            let (bytes, sha) = sha256_file(path)?;
            format!("{bytes} {sha}")
        } else {
            "absent".to_owned()
        };
        Ok(self.add(label, &v))
    }

    /// Hex digest.
    pub fn finish(self) -> String {
        hex::encode(self.0.finalize())
    }
}

/// The parts of the previous pack and those of the pack being built.
#[derive(Debug)]
pub struct Parts {
    previous_pack: PathBuf,
    previous: Record,
    next: Record,
    reused: Vec<String>,
}

impl Parts {
    /// Parts of the pack in place at `pack_dir` (`data/out/<site>/pack/`), from the
    /// [`PARTS_FILE`] next to it; none when it is absent or unreadable.
    pub fn open(pack_dir: &Path) -> Self {
        Self::open_at(pack_dir, &record_path(pack_dir))
    }

    /// Parts of the output directory `dir` whose keys are in `record` (the export keeps
    /// its own, [`crate::export`]).
    pub fn open_at(dir: &Path, record: &Path) -> Self {
        let previous = std::fs::read(record)
            .ok()
            .and_then(|b| serde_json::from_slice::<Record>(&b).ok())
            .unwrap_or_default();
        Self {
            previous_pack: dir.to_path_buf(),
            previous,
            next: Record::default(),
            reused: Vec::new(),
        }
    }

    /// No previous part: everything is made.
    pub fn none() -> Self {
        Self {
            previous_pack: PathBuf::new(),
            previous: Record::default(),
            next: Record::default(),
            reused: Vec::new(),
        }
    }

    /// When the previous pack has part `id` with `key` and all its files intact: links
    /// them into `root` and returns its manifest entry; else `None` (the part is made).
    pub fn reuse<T: DeserializeOwned>(&mut self, id: &str, key: &str, root: &Path) -> Option<T> {
        let part = self.previous.parts.get(id)?;
        if part.key != key {
            return None;
        }
        let entry: T = serde_json::from_value(part.entry.clone()).ok()?;
        for f in &part.files {
            let src = self.previous_pack.join(&f.path);
            match sha256_file(&src) {
                Ok((bytes, sha)) if bytes == f.bytes && sha == f.sha256 => {}
                _ => return None,
            }
        }
        for f in &part.files {
            if link_or_copy(&self.previous_pack.join(&f.path), &root.join(&f.path)).is_err() {
                return None;
            }
        }
        self.next.parts.insert(id.to_owned(), part.clone());
        self.reused.push(id.to_owned());
        Some(entry)
    }

    /// Records part `id`, just made into `root`: its key, its files (`files`: files
    /// relative to `root`; directories taken whole) and its manifest entry.
    pub fn record<T: Serialize>(
        &mut self,
        id: &str,
        key: String,
        root: &Path,
        files: &[&str],
        entry: &T,
    ) -> Result<()> {
        let mut out = Vec::new();
        for f in files {
            let p = root.join(f);
            if p.is_dir() {
                for e in crate::build::list_files(&p)? {
                    out.push(FileEntry {
                        path: format!("{f}/{}", e.path),
                        bytes: e.bytes,
                        sha256: e.sha256,
                    });
                }
            } else {
                let (bytes, sha256) = sha256_file(&p)?;
                out.push(FileEntry {
                    path: (*f).to_owned(),
                    bytes,
                    sha256,
                });
            }
        }
        let entry = serde_json::to_value(entry).map_err(|e| Error::json(id, e))?;
        self.next.parts.insert(
            id.to_owned(),
            Part {
                key,
                files: out,
                entry,
            },
        );
        Ok(())
    }

    /// Identifiers of the parts taken from the previous pack.
    pub fn reused(&self) -> &[String] {
        &self.reused
    }

    /// Writes the parts of the new pack, now at `pack_dir`.
    pub fn write(self, pack_dir: &Path, pack_version: &str) -> Result<()> {
        self.write_at(&record_path(pack_dir), pack_version)
    }

    /// Writes the parts of the new output to `record`.
    pub fn write_at(mut self, record: &Path, pack_version: &str) -> Result<()> {
        self.next.pack_version = pack_version.to_owned();
        crate::build::write_json(record, &self.next)?;
        Ok(())
    }
}

/// [`PARTS_FILE`] of a pack directory.
pub fn record_path(pack_dir: &Path) -> PathBuf {
    pack_dir
        .parent()
        .map_or_else(|| PathBuf::from(PARTS_FILE), |p| p.join(PARTS_FILE))
}

/// Hard link `from` at `to` (copy when the file system refuses), parents created.
fn link_or_copy(from: &Path, to: &Path) -> Result<()> {
    if let Some(p) = to.parent() {
        create_dir(p)?;
    }
    if to.exists() {
        std::fs::remove_file(to).map_err(|e| Error::io(to, e))?;
    }
    if std::fs::hard_link(from, to).is_err() {
        std::fs::copy(from, to).map_err(|e| Error::io(from, e))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_depend_on_every_value_and_its_label() {
        let k = |pairs: &[(&str, &str)]| {
            let mut k = Key::new("class:slope");
            for (l, v) in pairs {
                k.add(l, v);
            }
            k.finish()
        };
        assert_eq!(k(&[("a", "1")]), k(&[("a", "1")]));
        assert_ne!(k(&[("a", "1")]), k(&[("a", "2")]));
        assert_ne!(k(&[("a", "1")]), k(&[("b", "1")]));
        assert_ne!(k(&[("a", "1b")]), k(&[("a1", "b")]));
        assert_ne!(Key::new("x").finish(), Key::new("y").finish());
    }

    #[test]
    fn a_part_is_reused_only_with_its_key_and_intact_files() {
        let t = tempfile::tempdir().unwrap();
        let pack = t.path().join("pack");
        std::fs::create_dir_all(pack.join("layers")).unwrap();
        std::fs::write(pack.join("layers/a.pmtiles"), b"tiles").unwrap();
        let mut p = Parts::none();
        p.record(
            "class:a",
            "k1".into(),
            &pack,
            &["layers/a.pmtiles"],
            &"entry",
        )
        .unwrap();
        p.write(&pack, "v1").unwrap();

        let next = t.path().join("next");
        let mut p = Parts::open(&pack);
        assert_eq!(p.reuse::<String>("class:a", "k2", &next), None, "other key");
        assert_eq!(
            p.reuse::<String>("class:b", "k1", &next),
            None,
            "other part"
        );
        assert_eq!(
            p.reuse::<String>("class:a", "k1", &next).as_deref(),
            Some("entry")
        );
        assert_eq!(
            std::fs::read(next.join("layers/a.pmtiles")).unwrap(),
            b"tiles"
        );
        assert_eq!(p.reused(), ["class:a"]);

        // Altered since: made again.
        std::fs::write(pack.join("layers/a.pmtiles"), b"other").unwrap();
        let mut p = Parts::open(&pack);
        assert_eq!(
            p.reuse::<String>("class:a", "k1", &t.path().join("n2")),
            None
        );
    }
}
