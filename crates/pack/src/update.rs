//! Pack updates (K2, S0): from the pack a phone holds to a newer version, file by file.
//!
//! A pack version is its manifest: every file with its size and SHA-256. An [`Update`]
//! lists the contents of the new version that the old one does not have (by SHA-256, each
//! once), the files that go away, and the change and validity of every layer. The server
//! (`skimap-server`) serves it as JSON and each content as an immutable blob; the phone
//! downloads the listed blobs into a directory of its own (`<blobs>/<sha256>`), then
//! [`apply`] builds the new pack next to the old one and swaps them. No network here: the
//! download is the app's business (Kotlin, K2). Format: `docs/mise-a-jour.md`.
//!
//! Files whose contents the old pack already has are hard-linked from it (copied when the
//! file system refuses links), so a daily update that changes a few layers writes only
//! those. A pack is never changed in place: an interrupted update leaves the old pack
//! intact, and [`recover`] (called by [`apply`]) finishes or rolls back an interrupted swap.
//!
//! A PMTiles file that changed by a few tiles travels as a patch against its previous
//! version ([`Patch`], [`crate::patch`], format 3, #160) when that is smaller than the
//! file: the phone rebuilds the new file from the old one and checks its SHA-256.

use std::collections::{HashMap, HashSet};
use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use skimap_common::output::{AtomicFile, copy_file, write_bytes};

use crate::error::{Error, Result};
use crate::manifest::{FORMAT, FORMAT_VERSION, MANIFEST_FILE, Manifest, PackValidity};
use crate::reader::{Pack, check_relative, sha256_file};

/// Value of [`Update::format`].
pub const UPDATE_FORMAT: &str = "skimap-pack-update";
/// Update format version read by this crate (versions 1 to this one).
/// Version 2 (#160): in a bundle, a blob may be stored gzip-compressed
/// ([`Blob::gzip_bytes`]). Version 3 (#160): a PMTiles blob may be stored as a patch
/// against a file of the installed version ([`Blob::patch`]). A bundle is written at the
/// lowest version that reads it: 3 only when it holds a patch ([`write_bundle_patched`]).
pub const UPDATE_FORMAT_VERSION: u32 = 3;
/// Version of the bundles without patches ([`write_bundle`]).
pub const UPDATE_FORMAT_VERSION_WHOLE: u32 = 2;

/// What it takes to go from one pack version (or from nothing) to another.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Update {
    /// Always [`UPDATE_FORMAT`].
    pub format: String,
    /// [`UPDATE_FORMAT_VERSION_WHOLE`] (2), or [`UPDATE_FORMAT_VERSION`] (3) in a bundle with a
    /// patch.
    pub format_version: u32,
    /// Site identifier.
    pub site: String,
    /// Version the update starts from; `None`: from nothing (first download).
    pub from_version: Option<String>,
    /// Version the update leads to.
    pub to_version: String,
    /// The new `manifest.json` (its `paths` is `["manifest.json"]`).
    pub manifest: Blob,
    /// Contents of the new version absent from the old one, each once, in the order of
    /// their first file in the new manifest.
    pub fetch: Vec<Blob>,
    /// Bytes to download: [`Update::manifest`] (unless already up to date) and
    /// [`Update::fetch`].
    pub fetch_bytes: u64,
    /// Files of the new version whose contents the old version already has.
    pub reused_files: u64,
    /// Paths of the old version absent from the new one.
    pub removed: Vec<String>,
    /// Number of files of the new version (without the manifest).
    pub files: u64,
    /// Size of the new version (`Manifest::total_bytes`).
    pub total_bytes: u64,
    /// Validity of the new version as a whole.
    pub valid_for: PackValidity,
    /// Every basemap, layer and the voice clips of either version, with their change.
    pub changes: Vec<ItemChange>,
    /// In a bundle only: the manifest is stored gzip-compressed, this many bytes.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub manifest_gzip_bytes: Option<u64>,
}

/// One content to download, addressed by its SHA-256.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Blob {
    /// SHA-256, lowercase hex: the blob's name.
    pub sha256: String,
    /// Size in bytes.
    pub bytes: u64,
    /// Paths of the new version with these contents.
    pub paths: Vec<String>,
    /// In a bundle only: the blob is stored gzip-compressed, this many bytes (text
    /// contents, [`compressible`]); [`Blob::bytes`] and [`Blob::sha256`] are those of the
    /// inflated contents.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gzip_bytes: Option<u64>,
    /// In a bundle of format 3 only: the blob is stored as a patch against a file of the
    /// installed version ([`crate::patch`]); [`Blob::bytes`] and [`Blob::sha256`] are those
    /// of the rebuilt file.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub patch: Option<Patch>,
}

/// A blob stored as a patch (format 3, #160).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Patch {
    /// SHA-256 of the old contents the patch applies to (a file of the installed version,
    /// at the same path).
    pub from_sha256: String,
    /// Stored length of the patch in the bundle.
    pub bytes: u64,
    /// Tile ids only in the new archive.
    pub tiles_added: u64,
    /// Tile ids in both, with other contents.
    pub tiles_changed: u64,
    /// Tile ids only in the old archive.
    pub tiles_removed: u64,
}

/// How an item changed between the two versions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Change {
    /// Only in the new version.
    Added,
    /// In both, with different contents.
    Changed,
    /// In both, same contents.
    Unchanged,
    /// Only in the old version.
    Removed,
}

/// Change of one basemap, layer or of the voice clips.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ItemChange {
    /// `basemap`, `vector`, `raster`, `data` (layer kinds) or `voice`.
    pub kind: String,
    /// Basemap or layer identifier; `voice` for the clips.
    pub id: String,
    /// Change.
    pub change: Change,
    /// Main file in the new version (in the old one when removed).
    pub file: String,
    /// Size of the main file in the new version (0 when removed).
    pub bytes: u64,
    /// Generation time of the layer's source (layers only).
    #[serde(default)]
    pub generated_at: Option<String>,
    /// Start of validity (layers only).
    #[serde(default)]
    pub valid_from: Option<String>,
    /// End of validity (layers only).
    #[serde(default)]
    pub valid_until: Option<String>,
    /// Validity in words (layers only).
    #[serde(default)]
    pub validity: Option<String>,
}

/// Outcome of [`apply`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Applied {
    /// Version now installed.
    pub version: String,
    /// Files written from downloaded blobs.
    pub fetched_files: u64,
    /// Files linked or copied from the old version.
    pub reused_files: u64,
    /// Files of the old version left out.
    pub removed_files: u64,
}

/// SHA-256 of a byte string, lowercase hex.
pub fn sha256_hex(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}

/// One basemap, layer or the voice clips of a manifest, with its main file.
struct Item {
    kind: String,
    id: String,
    file: String,
    sha256: Option<String>,
    bytes: u64,
}

/// Basemaps, layers and voice clips of a manifest, in manifest order.
fn items(m: &Manifest) -> Vec<Item> {
    let files: HashMap<&str, (&str, u64)> = m
        .files
        .iter()
        .map(|f| (f.path.as_str(), (f.sha256.as_str(), f.bytes)))
        .collect();
    let item = |kind: String, id: &str, file: &str| {
        let (sha256, bytes) = files
            .get(file)
            .map_or((None, 0), |(s, b)| (Some((*s).to_owned()), *b));
        Item {
            kind,
            id: id.to_owned(),
            file: file.to_owned(),
            sha256,
            bytes,
        }
    };
    let mut out: Vec<Item> = m
        .basemaps
        .iter()
        .map(|b| item("basemap".to_owned(), &b.id, &b.tiles))
        .collect();
    for l in &m.layers {
        let kind = serde_json::to_value(l.kind)
            .ok()
            .and_then(|v| v.as_str().map(str::to_owned))
            .unwrap_or_default();
        out.push(item(kind, &l.id, &l.file));
    }
    if let Some(vm) = &m.voice.manifest {
        out.push(item("voice".to_owned(), "voice", vm));
    }
    out
}

impl Update {
    /// The update from `old` (`None`: nothing installed) to `new`, whose `manifest.json`
    /// is `new_manifest` (its exact bytes: the blob the phone downloads).
    pub fn between(old: Option<&Manifest>, new: &Manifest, new_manifest: &[u8]) -> Self {
        let have: HashSet<&str> = old
            .map(|o| o.files.iter().map(|f| f.sha256.as_str()).collect())
            .unwrap_or_default();
        let mut fetch: Vec<Blob> = Vec::new();
        let mut index: HashMap<&str, usize> = HashMap::new();
        let mut reused = 0u64;
        for f in &new.files {
            if have.contains(f.sha256.as_str()) {
                reused += 1;
                continue;
            }
            match index.get(f.sha256.as_str()) {
                Some(&i) => fetch[i].paths.push(f.path.clone()),
                None => {
                    index.insert(&f.sha256, fetch.len());
                    fetch.push(Blob {
                        sha256: f.sha256.clone(),
                        bytes: f.bytes,
                        paths: vec![f.path.clone()],
                        gzip_bytes: None,
                        patch: None,
                    });
                }
            }
        }
        let new_paths: HashSet<&str> = new.files.iter().map(|f| f.path.as_str()).collect();
        let removed: Vec<String> = old
            .map(|o| {
                o.files
                    .iter()
                    .filter(|f| !new_paths.contains(f.path.as_str()))
                    .map(|f| f.path.clone())
                    .collect()
            })
            .unwrap_or_default();
        let up_to_date = old.is_some_and(|o| o.pack_version == new.pack_version);
        let manifest = Blob {
            sha256: sha256_hex(new_manifest),
            bytes: new_manifest.len() as u64,
            paths: vec![MANIFEST_FILE.to_owned()],
            gzip_bytes: None,
            patch: None,
        };
        let fetch_bytes = fetch.iter().map(|b| b.bytes).sum::<u64>()
            + if up_to_date { 0 } else { manifest.bytes };
        Update {
            format: UPDATE_FORMAT.to_owned(),
            format_version: UPDATE_FORMAT_VERSION_WHOLE,
            site: new.site.id.clone(),
            from_version: old.map(|o| o.pack_version.clone()),
            to_version: new.pack_version.clone(),
            manifest,
            fetch,
            fetch_bytes,
            reused_files: reused,
            removed,
            files: new.files.len() as u64,
            total_bytes: new.total_bytes,
            valid_for: new.valid_for.clone(),
            changes: changes(old, new),
            manifest_gzip_bytes: None,
        }
    }

    /// Whether the phone already holds [`Update::to_version`].
    pub fn is_up_to_date(&self) -> bool {
        self.from_version.as_deref() == Some(self.to_version.as_str())
    }
}

fn changes(old: Option<&Manifest>, new: &Manifest) -> Vec<ItemChange> {
    let before = old.map(items).unwrap_or_default();
    let after = items(new);
    let key = |i: &Item| (i.kind.clone(), i.id.clone());
    let before_by_key: HashMap<(String, String), &Item> =
        before.iter().map(|i| (key(i), i)).collect();
    let after_keys: HashSet<(String, String)> = after.iter().map(key).collect();
    let layer = |id: &str| new.layer(id).or_else(|| old.and_then(|o| o.layer(id)));
    let mut out = Vec::new();
    for i in &after {
        let change = match before_by_key.get(&key(i)) {
            None => Change::Added,
            Some(b) if b.sha256 == i.sha256 => Change::Unchanged,
            Some(_) => Change::Changed,
        };
        out.push(item_change(i, change, i.bytes, layer));
    }
    for i in before.iter().filter(|i| !after_keys.contains(&key(i))) {
        out.push(item_change(i, Change::Removed, 0, layer));
    }
    out
}

fn item_change<'a>(
    i: &Item,
    change: Change,
    bytes: u64,
    layer: impl Fn(&str) -> Option<&'a crate::manifest::Layer>,
) -> ItemChange {
    let l = if i.kind == "basemap" || i.kind == "voice" {
        None
    } else {
        layer(&i.id)
    };
    ItemChange {
        kind: i.kind.clone(),
        id: i.id.clone(),
        change,
        file: i.file.clone(),
        bytes,
        generated_at: l.and_then(|l| l.generated_at.clone()),
        valid_from: l.and_then(|l| l.valid_from.clone()),
        valid_until: l.and_then(|l| l.valid_until.clone()),
        validity: l.and_then(|l| l.validity.clone()),
    }
}

/// Parses and checks a manifest: format, version, paths inside the pack, no duplicate.
pub fn parse_manifest(bytes: &[u8], context: &str) -> Result<Manifest> {
    let m: Manifest = serde_json::from_slice(bytes).map_err(|e| Error::json(context, e))?;
    if m.format != FORMAT {
        return Err(Error::Pack(format!(
            "{context}: format {:?}, expected {FORMAT:?}",
            m.format
        )));
    }
    if m.format_version == 0 || m.format_version > FORMAT_VERSION {
        return Err(Error::Pack(format!(
            "{context}: format version {} not supported (this reader reads 1 to {FORMAT_VERSION})",
            m.format_version
        )));
    }
    let mut seen = HashSet::new();
    for f in &m.files {
        check_relative(&f.path)?;
        if f.path == MANIFEST_FILE {
            return Err(Error::Pack(format!("{context}: lists {MANIFEST_FILE}")));
        }
        if !seen.insert(f.path.as_str()) {
            return Err(Error::Pack(format!("{context}: {} listed twice", f.path)));
        }
        if f.sha256.len() != 64 || !f.sha256.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Err(Error::Pack(format!(
                "{context}: bad SHA-256 for {}",
                f.path
            )));
        }
    }
    Ok(m)
}

/// Directory the new version is built in, next to `pack_dir`.
pub fn staging_dir(pack_dir: &Path) -> PathBuf {
    sibling(pack_dir, "new")
}

/// Where the old version sits during the swap, next to `pack_dir`.
pub fn previous_dir(pack_dir: &Path) -> PathBuf {
    sibling(pack_dir, "old")
}

fn sibling(pack_dir: &Path, suffix: &str) -> PathBuf {
    let name = pack_dir
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "pack".to_owned());
    pack_dir.with_file_name(format!(".{name}.{suffix}"))
}

fn remove_dir(p: &Path) -> Result<()> {
    match fs::remove_dir_all(p) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(Error::io(p, e)),
    }
}

/// Finishes or rolls back an interrupted swap of `pack_dir`, then removes leftovers.
///
/// The manifest is the last file written into the staging directory: a staging directory
/// with a manifest is complete. Without a pack in `pack_dir`, a complete staging
/// directory becomes the pack, else the previous version comes back.
pub fn recover(pack_dir: &Path) -> Result<()> {
    let staging = staging_dir(pack_dir);
    let previous = previous_dir(pack_dir);
    if !pack_dir.join(MANIFEST_FILE).exists() {
        let from = if staging.join(MANIFEST_FILE).exists() {
            Some(&staging)
        } else if previous.join(MANIFEST_FILE).exists() {
            Some(&previous)
        } else {
            None
        };
        if let Some(from) = from {
            remove_dir(pack_dir)?;
            fs::rename(from, pack_dir).map_err(|e| Error::io(from, e))?;
        }
    }
    remove_dir(&staging)?;
    remove_dir(&previous)
}

fn link_or_copy(src: &Path, dst: &Path) -> Result<()> {
    if fs::hard_link(src, dst).is_ok() {
        return Ok(());
    }
    copy_file(src, dst)?;
    Ok(())
}

/// Where the downloaded blobs are, each as stored: as is, gzip-compressed
/// ([`Blob::gzip_bytes`]) or as a patch ([`Blob::patch`]).
struct Source<'a> {
    at: Stored<'a>,
    /// SHA-256 of the blobs stored gzip-compressed (the manifest, text contents).
    gzipped: HashSet<String>,
    /// Blobs stored as patches, by SHA-256 of the rebuilt contents.
    patches: HashMap<String, Patch>,
}

enum Stored<'a> {
    /// One file per blob, named by its SHA-256.
    Dir(&'a Path),
    /// A bundle file and the offset and stored length of each blob in it.
    Bundle(&'a Path, BundleIndex),
}

/// A writer that hashes and counts what goes through it.
struct Hashing<W: Write> {
    inner: W,
    hash: Sha256,
    bytes: u64,
}

impl<W: Write> Write for Hashing<W> {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        let n = self.inner.write(buf)?;
        self.hash.update(&buf[..n]);
        self.bytes += n as u64;
        Ok(n)
    }
    fn flush(&mut self) -> std::io::Result<()> {
        self.inner.flush()
    }
}

/// The error of a patch that cannot be applied (message starting with [`PATCH_FAILED`]).
fn patch_failed(path: &Path, what: impl std::fmt::Display) -> Error {
    let what = what.to_string();
    let what = what.strip_prefix("pack: ").unwrap_or(&what);
    Error::Pack(format!("{PATCH_FAILED}: {}: {what}", path.display()))
}

fn bad_blob(sha: &str, bytes: u64, got: u64, h: &str) -> Error {
    Error::Pack(format!(
        "blob {sha}: {got} bytes, SHA-256 {h}; manifest says {bytes} bytes"
    ))
}

impl<'a> Source<'a> {
    fn new(at: Stored<'a>, update: &Update) -> Self {
        let mut gzipped: HashSet<String> = update
            .fetch
            .iter()
            .filter(|b| b.gzip_bytes.is_some())
            .map(|b| b.sha256.clone())
            .collect();
        if update.manifest_gzip_bytes.is_some() {
            gzipped.insert(update.manifest.sha256.clone());
        }
        let patches = update
            .fetch
            .iter()
            .filter_map(|b| b.patch.clone().map(|p| (b.sha256.clone(), p)))
            .collect();
        Source {
            at,
            gzipped,
            patches,
        }
    }

    fn has(&self, sha: &str) -> bool {
        match &self.at {
            Stored::Dir(d) => d.join(sha).is_file(),
            Stored::Bundle(_, offsets) => offsets.contains_key(sha),
        }
    }

    fn path(&self) -> &Path {
        match &self.at {
            Stored::Dir(d) | Stored::Bundle(d, _) => d,
        }
    }

    /// The stored bytes of blob `sha`, never more than `limit`.
    fn stored(&self, sha: &str, limit: u64) -> Result<Box<dyn Read>> {
        use std::io::{Seek, SeekFrom};
        match &self.at {
            Stored::Dir(d) => {
                let p = d.join(sha);
                let f = fs::File::open(&p).map_err(|e| Error::io(&p, e))?;
                Ok(Box::new(f.take(limit)))
            }
            Stored::Bundle(path, offsets) => {
                let &(off, len) = offsets
                    .get(sha)
                    .ok_or_else(|| Error::Pack(format!("blob {sha} not in the bundle")))?;
                let mut f = fs::File::open(path).map_err(|e| Error::io(path, e))?;
                f.seek(SeekFrom::Start(off))
                    .map_err(|e| Error::io(path, e))?;
                Ok(Box::new(f.take(len.min(limit))))
            }
        }
    }

    /// The contents of blob `sha` (not a patch), inflated when stored gzip-compressed;
    /// never more than `limit` bytes.
    fn contents(&self, sha: &str, limit: u64) -> Result<Box<dyn Read>> {
        let stored = self.stored(sha, u64::MAX)?;
        Ok(if self.gzipped.contains(sha) {
            Box::new(flate2::read::GzDecoder::new(stored).take(limit))
        } else {
            Box::new(stored.take(limit))
        })
    }

    fn read(&self, sha: &str) -> Result<Vec<u8>> {
        let mut out = Vec::new();
        self.contents(sha, 64 << 20)?
            .read_to_end(&mut out)
            .map_err(|e| Error::io(self.path(), e))?;
        Ok(out)
    }

    /// Writes the blob `sha` (`bytes` long) to `dst`, checking size and SHA-256. A patch
    /// is applied to the file of the installed version with its old contents (`local`).
    fn install(
        &self,
        sha: &str,
        bytes: u64,
        dst: &Path,
        local: &HashMap<&str, PathBuf>,
    ) -> Result<()> {
        let patch = self.patches.get(sha);
        if let (Stored::Dir(d), None, false) = (&self.at, patch, self.gzipped.contains(sha)) {
            let src = d.join(sha);
            let (n, h) = sha256_file(&src)?;
            if n != bytes || h != sha {
                return Err(bad_blob(sha, bytes, n, &h));
            }
            return link_or_copy(&src, dst);
        }
        // Removed if dropped (a read error, a mismatch below).
        let mut out = Hashing {
            inner: AtomicFile::create(dst)?,
            hash: Sha256::new(),
            bytes: 0,
        };
        if let Some(p) = patch {
            let old_path = local.get(p.from_sha256.as_str()).ok_or_else(|| {
                patch_failed(dst, format!("no installed file has {}", p.from_sha256))
            })?;
            let mut old = fs::File::open(old_path).map_err(|e| patch_failed(old_path, e))?;
            let old_len = old.metadata().map_err(|e| patch_failed(old_path, e))?.len();
            // One byte more than announced: a longer patch is refused, not cut.
            let mut input = std::io::BufReader::new(self.stored(sha, p.bytes + 1)?);
            crate::patch::apply(&mut input, &mut old, old_len, &mut out, bytes)
                .map_err(|e| patch_failed(dst, e))?;
        } else {
            // One byte more than announced: a longer blob is refused, not cut.
            let mut input = self.contents(sha, bytes + 1)?;
            std::io::copy(&mut input, &mut out).map_err(|e| Error::io(self.path(), e))?;
        }
        let got = hex::encode(out.hash.finalize_reset());
        if out.bytes != bytes || got != sha {
            let e = bad_blob(sha, bytes, out.bytes, &got);
            return Err(if patch.is_some() {
                patch_failed(dst, e)
            } else {
                e
            });
        }
        out.inner.commit()?;
        Ok(())
    }
}

/// Installs `update` into `pack_dir`; the blobs it lists (and the new manifest) are in
/// `blobs`, each named by its SHA-256. `pack_dir` holds any earlier version of the pack,
/// or nothing for a first download: what counts is that every content of the new version
/// is either in the installed pack or in the update (checked before anything is
/// written). Every downloaded blob is checked (size and SHA-256); files taken from the
/// installed version are checked by size.
pub fn apply(pack_dir: &Path, update: &Update, blobs: &Path) -> Result<Applied> {
    apply_from(pack_dir, update, &Source::new(Stored::Dir(blobs), update))
}

fn apply_from(pack_dir: &Path, update: &Update, source: &Source<'_>) -> Result<Applied> {
    if update.format != UPDATE_FORMAT
        || update.format_version == 0
        || update.format_version > UPDATE_FORMAT_VERSION
    {
        return Err(Error::Pack(format!(
            "update format {:?} version {} not supported",
            update.format, update.format_version
        )));
    }
    recover(pack_dir)?;
    let old = if pack_dir.join(MANIFEST_FILE).exists() {
        Some(Pack::open(pack_dir)?)
    } else {
        None
    };
    if let Some(o) = &old
        && o.manifest().pack_version == update.to_version
    {
        return Ok(Applied {
            version: update.to_version.clone(),
            fetched_files: 0,
            reused_files: update.files,
            removed_files: 0,
        });
    }
    if update.is_up_to_date() {
        return Err(Error::Pack(format!(
            "update says {} is installed, found {}",
            update.to_version,
            old.as_ref()
                .map_or("nothing", |o| o.manifest().pack_version.as_str())
        )));
    }
    let manifest_bytes = source.read(&update.manifest.sha256)?;
    if sha256_hex(&manifest_bytes) != update.manifest.sha256 {
        return Err(Error::Pack(format!(
            "manifest {}: SHA-256 differs from the update",
            update.manifest.sha256
        )));
    }
    let new = parse_manifest(&manifest_bytes, MANIFEST_FILE)?;
    if new.pack_version != update.to_version || new.site.id != update.site {
        return Err(Error::Pack(format!(
            "manifest is {} {}, update says {} {}",
            new.site.id, new.pack_version, update.site, update.to_version
        )));
    }
    if let Some(o) = &old
        && o.manifest().site.id != new.site.id
    {
        return Err(Error::Pack(format!(
            "installed pack is {}, update is for {}",
            o.manifest().site.id,
            new.site.id
        )));
    }
    // Contents the installed pack has, with a file holding each (size checked).
    let local: HashMap<&str, PathBuf> = old
        .as_ref()
        .map(|o| {
            o.manifest()
                .files
                .iter()
                .filter(|f| fs::metadata(pack_dir.join(&f.path)).is_ok_and(|m| m.len() == f.bytes))
                .map(|f| (f.sha256.as_str(), pack_dir.join(&f.path)))
                .collect()
        })
        .unwrap_or_default();
    // Everything must be at hand before anything is written.
    let missing: Vec<&str> = new
        .files
        .iter()
        .filter(|f| !local.contains_key(f.sha256.as_str()) && !source.has(&f.sha256))
        .map(|f| f.path.as_str())
        .collect();
    if !missing.is_empty() {
        return Err(Error::Pack(format!(
            "{MISSING_CONTENTS}: {} files neither in the installed pack nor in the update (first: {}); install the base bundle",
            missing.len(),
            missing[0]
        )));
    }
    // A patch needs the file it applies to: without it, the bundle with whole files.
    if let Some((f, p)) = new.files.iter().find_map(|f| {
        source
            .patches
            .get(&f.sha256)
            .filter(|p| {
                !local.contains_key(f.sha256.as_str())
                    && !local.contains_key(p.from_sha256.as_str())
            })
            .map(|p| (f, p))
    }) {
        return Err(Error::Pack(format!(
            "{PATCH_FAILED}: {}: the installed pack has no file {} to patch; install the bundle with whole files",
            f.path, p.from_sha256
        )));
    }

    let staging = staging_dir(pack_dir);
    remove_dir(&staging)?;
    fs::create_dir_all(&staging).map_err(|e| Error::io(&staging, e))?;
    let result = (|| -> Result<Applied> {
        // Blobs already written into the staging directory: duplicates are linked.
        let mut written: HashMap<&str, PathBuf> = HashMap::new();
        let (mut fetched, mut reused) = (0u64, 0u64);
        for f in &new.files {
            let dst = staging.join(&f.path);
            if let Some(parent) = dst.parent() {
                fs::create_dir_all(parent).map_err(|e| Error::io(parent, e))?;
            }
            if let Some(src) = local.get(f.sha256.as_str()) {
                link_or_copy(src, &dst)?;
                reused += 1;
            } else if let Some(src) = written.get(f.sha256.as_str()) {
                link_or_copy(src, &dst)?;
                fetched += 1;
            } else {
                source.install(&f.sha256, f.bytes, &dst, &local)?;
                written.insert(&f.sha256, dst);
                fetched += 1;
            }
        }
        let m = staging.join(MANIFEST_FILE);
        write_bytes(&m, &manifest_bytes)?;
        let new_paths: HashSet<&str> = new.files.iter().map(|f| f.path.as_str()).collect();
        let removed = old
            .as_ref()
            .map(|o| {
                o.manifest()
                    .files
                    .iter()
                    .filter(|f| !new_paths.contains(f.path.as_str()))
                    .count() as u64
            })
            .unwrap_or(0);
        Ok(Applied {
            version: new.pack_version.clone(),
            fetched_files: fetched,
            reused_files: reused,
            removed_files: removed,
        })
    })();
    let applied = match result {
        Ok(a) => a,
        Err(e) => {
            let _ = fs::remove_dir_all(&staging);
            return Err(e);
        }
    };
    let previous = previous_dir(pack_dir);
    remove_dir(&previous)?;
    if pack_dir.exists() {
        fs::rename(pack_dir, &previous).map_err(|e| Error::io(pack_dir, e))?;
    }
    fs::rename(&staging, pack_dir).map_err(|e| Error::io(&staging, e))?;
    remove_dir(&previous)?;
    Ok(applied)
}

/// Start of the error message of [`apply`] / [`apply_bundle`] when contents are missing:
/// the update was made for a pack the phone no longer has; install the base bundle.
pub const MISSING_CONTENTS: &str = "missing contents";

/// Start of the error message of [`apply`] / [`apply_bundle`] when a patched file cannot be
/// rebuilt (the file it applies to is missing, or the result differs from the manifest):
/// the pack is left as it was; install the bundle of the same update with whole files
/// (`sync::plan_whole_files`).
pub const PATCH_FAILED: &str = "patch failed";

/// First bytes of a bundle written by this crate: the header is gzip-compressed JSON
/// (update format version 2, #160).
pub const BUNDLE_MAGIC: &[u8; 8] = b"SKMPBND2";
/// First bytes of a bundle of update format version 1 (header as plain JSON), still read.
pub const BUNDLE_MAGIC_V1: &[u8; 8] = b"SKMPBND1";
/// First bytes of a bundle of update format version 3 (#160): as [`BUNDLE_MAGIC`], and
/// some blobs are patches ([`Blob::patch`]). An app of format 2 refuses it ("not a skimap
/// bundle") and keeps its pack.
pub const BUNDLE_MAGIC_V3: &[u8; 8] = b"SKMPBND3";

/// Blobs of a bundle in order, after the header, with their stored length: the manifest
/// (unless the update says "up to date"; gzip-compressed when
/// [`Update::manifest_gzip_bytes`] is set), then [`Update::fetch`] (a patch when
/// [`Blob::patch`] is set, gzip-compressed when [`Blob::gzip_bytes`] is set).
pub fn bundle_blobs(update: &Update) -> Vec<(&str, u64)> {
    let mut out = Vec::with_capacity(update.fetch.len() + 1);
    if !update.is_up_to_date() {
        out.push((
            update.manifest.sha256.as_str(),
            update.manifest_gzip_bytes.unwrap_or(update.manifest.bytes),
        ));
    }
    out.extend(update.fetch.iter().map(|b| {
        let stored = match (&b.patch, b.gzip_bytes) {
            (Some(p), _) => p.bytes,
            (None, Some(gz)) => gz,
            (None, None) => b.bytes,
        };
        (b.sha256.as_str(), stored)
    }));
    out
}

/// Extensions of the pack files stored gzip-compressed in a bundle: text (JSON, GeoJSON,
/// TSV) and the font glyphs (protobuf), which the transport does not compress (a bundle is
/// served as binary). PMTiles, gzip rasters, PNG and Opus are compressed already.
pub const COMPRESSIBLE_EXTENSIONS: &[&str] = &["json", "geojson", "tsv", "txt", "csv", "pbf"];

/// Whether a blob is stored gzip-compressed in a bundle (when that makes it smaller):
/// every path it has ends with one of [`COMPRESSIBLE_EXTENSIONS`].
pub fn compressible(blob: &Blob) -> bool {
    !blob.paths.is_empty()
        && blob.paths.iter().all(|p| {
            Path::new(p)
                .extension()
                .and_then(|e| e.to_str())
                .is_some_and(|e| COMPRESSIBLE_EXTENSIONS.contains(&e))
        })
}

/// Gzip at the best level, without name nor time: the same input gives the same bytes.
fn gzip(raw: &[u8]) -> std::io::Result<Vec<u8>> {
    let mut gz = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::best());
    gz.write_all(raw)?;
    gz.finish()
}

/// Patches of the PMTiles of an update ([`tile_patches`]), by SHA-256 of the new contents:
/// what the bundle says of each and its bytes.
#[derive(Debug, Clone, Default)]
pub struct TilePatches(pub HashMap<String, (Patch, Vec<u8>)>);

impl TilePatches {
    /// Whether no file is patched.
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

/// Reads blob `sha` (`bytes` long) whole through `open`, checking size and SHA-256.
fn read_blob<R: Read>(
    open: &mut impl FnMut(&str) -> Result<R>,
    sha: &str,
    bytes: u64,
) -> Result<Vec<u8>> {
    let mut raw = Vec::with_capacity(bytes as usize);
    open(sha)?
        .take(bytes + 1)
        .read_to_end(&mut raw)
        .map_err(|e| Error::io("bundle", e))?;
    if raw.len() as u64 != bytes || sha256_hex(&raw) != sha {
        return Err(Error::Pack(format!(
            "blob {sha}: contents differ from the update"
        )));
    }
    Ok(raw)
}

/// Extension of the files sent as patches when smaller ([`tile_patches`]).
pub const PATCHED_EXTENSION: &str = "pmtiles";

/// The PMTiles files of `update` (from the version whose manifest is `old`) that travel
/// smaller as a patch against the file at the same path in `old` ([`crate::patch`]):
/// for each, the patch is made, applied to the old file and checked against the SHA-256
/// of the new one before it is kept, and kept only when shorter than the file. A file
/// that is not a readable PMTiles v3 archive travels whole. `open(sha)` gives the contents
/// of a blob of either version.
pub fn tile_patches<R: Read>(
    update: &Update,
    old: &Manifest,
    mut open: impl FnMut(&str) -> Result<R>,
) -> Result<TilePatches> {
    let old_at: HashMap<&str, &crate::manifest::FileEntry> =
        old.files.iter().map(|f| (f.path.as_str(), f)).collect();
    let mut out = TilePatches::default();
    for b in &update.fetch {
        let from = b
            .paths
            .iter()
            .filter(|p| {
                Path::new(p)
                    .extension()
                    .is_some_and(|e| e == PATCHED_EXTENSION)
            })
            .find_map(|p| old_at.get(p.as_str()).filter(|f| f.sha256 != b.sha256));
        let Some(from) = from else { continue };
        let before = read_blob(&mut open, &from.sha256, from.bytes)?;
        let after = read_blob(&mut open, &b.sha256, b.bytes)?;
        let Ok((patch, diff)) = crate::patch::diff(&before, &after) else {
            continue;
        };
        if patch.len() as u64 >= b.bytes {
            continue;
        }
        // Proof before publishing: the patch rebuilds the new file byte for byte.
        let mut rebuilt = Vec::with_capacity(after.len());
        crate::patch::apply(
            &mut patch.as_slice(),
            &mut std::io::Cursor::new(&before),
            before.len() as u64,
            &mut rebuilt,
            b.bytes,
        )?;
        if rebuilt != after {
            return Err(Error::Pack(format!(
                "patch of {}: the rebuilt file differs",
                b.paths[0]
            )));
        }
        out.0.insert(
            b.sha256.clone(),
            (
                Patch {
                    from_sha256: from.sha256.clone(),
                    bytes: patch.len() as u64,
                    tiles_added: diff.tiles_added,
                    tiles_changed: diff.tiles_changed,
                    tiles_removed: diff.tiles_removed,
                },
                patch,
            ),
        );
    }
    Ok(out)
}

/// Writes a bundle of format 2, every file whole: [`write_bundle_patched`] without
/// patches.
pub fn write_bundle<W: std::io::Write, R: std::io::Read>(
    w: &mut W,
    update: &Update,
    open: impl FnMut(&str) -> Result<R>,
) -> Result<u64> {
    write_bundle_patched(w, update, &TilePatches::default(), open)
}

/// Writes a bundle: the magic ([`BUNDLE_MAGIC_V3`] when it holds a patch, else
/// [`BUNDLE_MAGIC`]), the header length (u64 little-endian), the update as gzip-compressed
/// compact JSON (format version 3 with a patch, else 2), then each blob of
/// [`bundle_blobs`]: the manifest gzip-compressed (a few tens of kilobytes instead of a
/// few hundred; the phone inflates it once), the files of `patches` as their patch, the
/// text blobs ([`compressible`]: weather, assistant, vector layers) gzip-compressed when
/// that makes them smaller, every other blob as is (the heavy ones are compressed already:
/// PMTiles, SKR gzip, Opus). `open(sha)` gives the contents of a blob. Returns the bundle
/// length.
pub fn write_bundle_patched<W: std::io::Write, R: std::io::Read>(
    w: &mut W,
    update: &Update,
    patches: &TilePatches,
    mut open: impl FnMut(&str) -> Result<R>,
) -> Result<u64> {
    let io = |e| Error::io("bundle", e);
    let mut update = update.clone();
    let mut manifest_gz = Vec::new();
    if !update.is_up_to_date() {
        let raw = read_blob(&mut open, &update.manifest.sha256, update.manifest.bytes)
            .map_err(|_| Error::Pack("manifest blob differs from the update".into()))?;
        manifest_gz = gzip(&raw).map_err(io)?;
        update.manifest_gzip_bytes = Some(manifest_gz.len() as u64);
    }
    // Patches and compressed text first: their stored length goes in the header.
    let mut packed: HashMap<String, Vec<u8>> = HashMap::new();
    for b in &mut update.fetch {
        b.gzip_bytes = None;
        b.patch = None;
        if let Some((p, _)) = patches.0.get(&b.sha256) {
            b.patch = Some(p.clone());
            continue;
        }
        if !compressible(b) {
            continue;
        }
        let raw = read_blob(&mut open, &b.sha256, b.bytes)?;
        let gz = gzip(&raw).map_err(io)?;
        if gz.len() < raw.len() {
            b.gzip_bytes = Some(gz.len() as u64);
            packed.insert(b.sha256.clone(), gz);
        }
    }
    let patched = update.fetch.iter().any(|b| b.patch.is_some());
    update.format_version = if patched {
        UPDATE_FORMAT_VERSION
    } else {
        UPDATE_FORMAT_VERSION_WHOLE
    };
    let header = serde_json::to_vec(&update).map_err(|e| Error::json("bundle header", e))?;
    let header = gzip(&header).map_err(io)?;
    w.write_all(if patched {
        BUNDLE_MAGIC_V3
    } else {
        BUNDLE_MAGIC
    })
    .map_err(io)?;
    w.write_all(&(header.len() as u64).to_le_bytes())
        .map_err(io)?;
    w.write_all(&header).map_err(io)?;
    w.write_all(&manifest_gz).map_err(io)?;
    let mut total = 16 + header.len() as u64 + manifest_gz.len() as u64;
    for b in &update.fetch {
        if let Some(stored) = patches
            .0
            .get(&b.sha256)
            .map(|(_, p)| p)
            .or_else(|| packed.get(&b.sha256))
        {
            w.write_all(stored).map_err(io)?;
            total += stored.len() as u64;
            continue;
        }
        let mut r = open(&b.sha256)?.take(b.bytes);
        let n = std::io::copy(&mut r, w).map_err(io)?;
        if n != b.bytes {
            return Err(Error::Pack(format!(
                "blob {}: {n} bytes, expected {}",
                b.sha256, b.bytes
            )));
        }
        total += n;
    }
    Ok(total)
}

/// Offset and length of each blob of a bundle, by SHA-256.
pub type BundleIndex = HashMap<String, (u64, u64)>;

/// Reads the header of a bundle file and the position of each of its blobs; checks the
/// length (an interrupted download is refused).
pub fn read_bundle(path: &Path) -> Result<(Update, BundleIndex)> {
    let mut f = fs::File::open(path).map_err(|e| Error::io(path, e))?;
    let len = f.metadata().map_err(|e| Error::io(path, e))?.len();
    let mut head = [0u8; 16];
    f.read_exact(&mut head).map_err(|e| Error::io(path, e))?;
    let gzipped_header = match &head[..8] {
        m if m == BUNDLE_MAGIC || m == BUNDLE_MAGIC_V3 => true,
        m if m == BUNDLE_MAGIC_V1 => false,
        _ => {
            return Err(Error::Pack(format!(
                "{}: not a skimap bundle",
                path.display()
            )));
        }
    };
    let hlen = u64::from_le_bytes(head[8..].try_into().expect("8 bytes"));
    if hlen > 64 << 20 || 16 + hlen > len {
        return Err(Error::Pack(format!(
            "{}: bad bundle header length",
            path.display()
        )));
    }
    let mut header = vec![0u8; hlen as usize];
    f.read_exact(&mut header).map_err(|e| Error::io(path, e))?;
    if gzipped_header {
        let mut raw = Vec::new();
        flate2::read::GzDecoder::new(header.as_slice())
            .take(64 << 20)
            .read_to_end(&mut raw)
            .map_err(|e| Error::io(path, e))?;
        header = raw;
    }
    let update: Update =
        serde_json::from_slice(&header).map_err(|e| Error::json(path.display().to_string(), e))?;
    let mut offsets = HashMap::new();
    let mut at = 16 + hlen;
    for (sha, bytes) in bundle_blobs(&update) {
        offsets.insert(sha.to_owned(), (at, bytes));
        at += bytes;
    }
    if at != len {
        return Err(Error::Pack(format!(
            "{}: {len} bytes, the header describes {at} (incomplete download?)",
            path.display()
        )));
    }
    Ok((update, offsets))
}

/// Installs a downloaded bundle into `pack_dir`, reading each blob straight from the
/// bundle file (the manifest and the text blobs stored gzip-compressed are inflated on the
/// way, the patches applied to the installed files; nothing is unpacked first).
pub fn apply_bundle(pack_dir: &Path, bundle: &Path) -> Result<Applied> {
    let (update, offsets) = read_bundle(bundle)?;
    let source = Source::new(Stored::Bundle(bundle, offsets), &update);
    apply_from(pack_dir, &update, &source)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn siblings() {
        let p = Path::new("/data/packs/cervieres");
        assert_eq!(staging_dir(p), Path::new("/data/packs/.cervieres.new"));
        assert_eq!(previous_dir(p), Path::new("/data/packs/.cervieres.old"));
    }

    #[test]
    fn sha256_of_bytes() {
        assert_eq!(
            sha256_hex(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }
}
