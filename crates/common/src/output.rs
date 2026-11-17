//! Writing pipeline files, always atomically and never through a shared inode.
//!
//! Every writer creates the parent directory, writes a fresh temporary file next to the
//! target (`<file>.<pid>.<n>.tmp`, created exclusively) and renames it over the target
//! after `fsync`. So a reader never sees a half-written file, even after a crash or a power
//! cut, and a target that is a hard link (working copies made with `cp -al`, layer files
//! linked into a pack) gets a new inode: its other names keep their content. JSON is
//! pretty-printed and ends with a newline.
//!
//! - [`write_bytes`], [`write_text`], [`write_json`], [`write_geojson`]: whole files.
//! - [`AtomicFile`]: a streamed file (`Write + Seek`: GeoTIFF, PMTiles encoders), renamed
//!   into place by [`AtomicFile::commit`], removed if dropped uncommitted.
//! - [`copy_file`]: copy onto a new inode (`std::fs::copy` truncates the target in place).
//! - [`append_bytes`], [`open_append`]: append to a journal, after breaking a hard link
//!   ([`break_link`]).
//! - [`create_log`]: a log written as it goes, on a new inode.
//! - [`open_lock`]: a lock file, never written.
//!
//! The direct writers of `std` (`std::fs::write`, `File::create`, `std::fs::copy`,
//! `OpenOptions::write` / `append`) are denied by `clippy.toml` everywhere else.
#![allow(
    clippy::disallowed_methods,
    reason = "the one module allowed to open files for writing"
)]

use std::fs::{File, OpenOptions};
use std::io::{BufWriter, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use serde::Serialize;
use serde_json::{Value, json};

use crate::error::{Error, Result};
use crate::geom::LonLat;

/// Fresh temporary file name next to `path`: `<file name>.<pid>.<n>.tmp`, unique within
/// the process and across processes (concurrent writers never share it); it contains
/// `.tmp`, which the orphan sweeps of the pipeline recognise.
pub fn tmp_path(path: &Path) -> PathBuf {
    static N: AtomicU64 = AtomicU64::new(0);
    let n = N.fetch_add(1, Ordering::Relaxed);
    let mut name = path.file_name().unwrap_or_default().to_os_string();
    name.push(format!(".{}.{n}.tmp", std::process::id()));
    path.with_file_name(name)
}

fn ensure_parent(path: &Path) -> Result<()> {
    if let Some(dir) = path.parent()
        && !dir.as_os_str().is_empty()
    {
        std::fs::create_dir_all(dir).map_err(|e| Error::io(dir, e))?;
    }
    Ok(())
}

/// A file written under a temporary name and renamed over its target by
/// [`commit`](Self::commit); dropped uncommitted, the temporary file is removed and the
/// target is untouched.
pub struct AtomicFile {
    path: PathBuf,
    tmp: PathBuf,
    w: Option<BufWriter<File>>,
}

impl AtomicFile {
    /// Creates the temporary file of `path` (parent directories created).
    pub fn create(path: &Path) -> Result<Self> {
        ensure_parent(path)?;
        let tmp = tmp_path(path);
        // `create_new`: never opens (and so never truncates) an existing inode.
        let f = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&tmp)
            .map_err(|e| Error::io(&tmp, e))?;
        Ok(Self {
            path: path.to_owned(),
            tmp,
            w: Some(BufWriter::with_capacity(1 << 16, f)),
        })
    }

    /// Final path.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Temporary path, for error messages.
    pub fn tmp(&self) -> &Path {
        &self.tmp
    }

    fn writer(&mut self) -> &mut BufWriter<File> {
        self.w.as_mut().expect("AtomicFile used after commit")
    }

    /// Flushes, syncs and renames the file into place; returns its size in bytes.
    pub fn commit(self) -> Result<u64> {
        let path = self.path.clone();
        self.commit_as(&path)
    }

    /// Like [`commit`](Self::commit), under another name (known once written: a file
    /// named by its hash), in the same file system as the path given to
    /// [`create`](Self::create).
    pub fn commit_as(mut self, path: &Path) -> Result<u64> {
        let tmp = self.tmp.clone();
        let io = |e| Error::io(&tmp, e);
        let w = self.w.take().expect("AtomicFile committed twice");
        let f = match w.into_inner() {
            Ok(f) => f,
            Err(e) => {
                let _ = std::fs::remove_file(&tmp);
                return Err(io(e.into_error()));
            }
        };
        let done = (|| {
            // On disk before the rename: a crash never leaves the new name on empty
            // content.
            f.sync_all().map_err(io)?;
            let len = f.metadata().map_err(io)?.len();
            std::fs::rename(&tmp, path).map_err(|e| Error::io(path, e))?;
            Ok(len)
        })();
        if done.is_err() {
            let _ = std::fs::remove_file(&tmp);
        }
        done
    }
}

impl Write for AtomicFile {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.writer().write(buf)
    }

    fn write_all(&mut self, buf: &[u8]) -> std::io::Result<()> {
        self.writer().write_all(buf)
    }

    fn flush(&mut self) -> std::io::Result<()> {
        self.writer().flush()
    }
}

impl Seek for AtomicFile {
    fn seek(&mut self, pos: SeekFrom) -> std::io::Result<u64> {
        self.writer().seek(pos)
    }
}

impl Drop for AtomicFile {
    fn drop(&mut self) {
        if let Some(w) = self.w.take() {
            drop(w);
            let _ = std::fs::remove_file(&self.tmp);
        }
    }
}

impl std::fmt::Debug for AtomicFile {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AtomicFile")
            .field("path", &self.path)
            .field("tmp", &self.tmp)
            .finish()
    }
}

/// Writes bytes atomically (temporary file then rename).
pub fn write_bytes(path: &Path, body: &[u8]) -> Result<()> {
    let mut f = AtomicFile::create(path)?;
    f.write_all(body).map_err(|e| Error::io(&f.tmp, e))?;
    f.commit().map(|_| ())
}

/// Writes text atomically.
pub fn write_text(path: &Path, text: &str) -> Result<()> {
    write_bytes(path, text.as_bytes())
}

/// Writes a value as pretty JSON followed by a newline, atomically.
pub fn write_json<T: Serialize + ?Sized>(path: &Path, value: &T) -> Result<()> {
    let mut body =
        serde_json::to_vec_pretty(value).map_err(|e| Error::json(path.display().to_string(), e))?;
    body.push(b'\n');
    write_bytes(path, &body)
}

/// Copies `from` to `to` atomically, onto a new inode (`std::fs::copy` would truncate
/// and rewrite an existing `to` in place, through all its hard links); returns the size
/// in bytes. Parent directories of `to` are created.
pub fn copy_file(from: &Path, to: &Path) -> Result<u64> {
    let mut src = File::open(from).map_err(|e| Error::io(from, e))?;
    let mut f = AtomicFile::create(to)?;
    // The bare file: `std::io::copy` between two files uses the kernel copy.
    std::io::copy(&mut src, f.writer().get_mut()).map_err(|e| Error::io(from, e))?;
    f.commit()
}

/// Gives `path` its own inode when it is a hard link (copy then rename), so that writing
/// to it in place no longer reaches its other names; returns whether it was a link. A
/// missing file is not an error.
pub fn break_link(path: &Path) -> Result<bool> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        match std::fs::metadata(path) {
            Ok(m) if m.is_file() && m.nlink() > 1 => {
                copy_file(path, path)?;
                Ok(true)
            }
            Ok(_) => Ok(false),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(false),
            Err(e) => Err(Error::io(path, e)),
        }
    }
    #[cfg(not(unix))]
    {
        let _ = path;
        Ok(false)
    }
}

/// Appends `body` to the journal `path` (created with its parents), synced. A journal
/// that is a hard link is first copied onto its own inode ([`break_link`]): its other
/// names never see the new lines.
pub fn append_bytes(path: &Path, body: &[u8]) -> Result<()> {
    let io = |e| Error::io(path, e);
    let mut f = open_append(path)?;
    f.write_all(body).map_err(io)?;
    f.sync_all().map_err(io)
}

/// Opens the journal `path` for appending (created with its parents), after giving it
/// its own inode if it is a hard link ([`break_link`]); for a journal kept open.
pub fn open_append(path: &Path) -> Result<File> {
    ensure_parent(path)?;
    break_link(path)?;
    OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .map_err(|e| Error::io(path, e))
}

/// Opens a log written as it goes (readable while being written) on a new inode: an
/// existing file is unlinked first, so a hard link to the previous log keeps it.
pub fn create_log(path: &Path) -> Result<File> {
    ensure_parent(path)?;
    match std::fs::remove_file(path) {
        Ok(()) => {}
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => return Err(Error::io(path, e)),
    }
    OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(|e| Error::io(path, e))
}

/// Opens (creates if needed) a lock file, for `File::lock`; its content is never written
/// nor truncated.
pub fn open_lock(path: &Path) -> Result<File> {
    ensure_parent(path)?;
    OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(path)
        .map_err(|e| Error::io(path, e))
}

/// Rounds a coordinate to 7 decimals (≈ 1 cm, the precision of OSM).
pub fn round7(v: f64) -> f64 {
    (v * 1e7).round() / 1e7
}

/// GeoJSON position `[lon, lat]`, rounded to 7 decimals.
pub fn position(p: LonLat) -> Value {
    json!([round7(p.lon), round7(p.lat)])
}

/// GeoJSON LineString geometry.
pub fn line_string(points: impl IntoIterator<Item = LonLat>) -> Value {
    json!({
        "type": "LineString",
        "coordinates": points.into_iter().map(position).collect::<Vec<_>>(),
    })
}

/// GeoJSON Point geometry.
pub fn point(p: LonLat) -> Value {
    json!({ "type": "Point", "coordinates": position(p) })
}

/// GeoJSON Feature.
pub fn feature(geometry: Value, properties: Value) -> Value {
    json!({ "type": "Feature", "geometry": geometry, "properties": properties })
}

/// Writes a FeatureCollection with one feature per line (compact, diff-friendly),
/// atomically.
pub fn write_geojson(path: &Path, features: &[Value]) -> Result<()> {
    let mut w = AtomicFile::create(path)?;
    let tmp = w.tmp.clone();
    let io = |e| Error::io(&tmp, e);
    w.write_all(b"{\"type\":\"FeatureCollection\",\"features\":[\n")
        .map_err(io)?;
    for (i, feat) in features.iter().enumerate() {
        if i > 0 {
            w.write_all(b",\n").map_err(io)?;
        }
        serde_json::to_writer(&mut w, feat)
            .map_err(|e| Error::json(path.display().to_string(), e))?;
    }
    w.write_all(b"\n]}\n").map_err(io)?;
    w.commit().map(|_| ())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn writers() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("a/b/x.json");
        write_json(&p, &json!({"k": 1})).unwrap();
        assert_eq!(std::fs::read_to_string(&p).unwrap(), "{\n  \"k\": 1\n}\n");
        // `x.geojson` and `x.json` in the same directory do not share a temporary file.
        let g = dir.path().join("a/b/x.geojson");
        let f = feature(point(LonLat::new(6.123456789, 44.9)), json!({"n": "a"}));
        write_geojson(&g, &[f.clone(), f]).unwrap();
        let text = std::fs::read_to_string(&g).unwrap();
        assert!(text.starts_with("{\"type\":\"FeatureCollection\",\"features\":[\n"));
        assert!(text.contains("[6.1234568,44.9]"));
        let v: Value = serde_json::from_str(&text).unwrap();
        assert_eq!(v["features"].as_array().unwrap().len(), 2);
        let names: Vec<_> = std::fs::read_dir(dir.path().join("a/b"))
            .unwrap()
            .map(|e| e.unwrap().file_name().into_string().unwrap())
            .collect();
        assert!(names.iter().all(|n| !n.contains(".tmp")), "{names:?}");
        write_bytes(&dir.path().join("raw.xml"), b"<a/>").unwrap();
        assert_eq!(
            line_string([LonLat::new(1.0, 2.0)])["coordinates"],
            json!([[1.0, 2.0]])
        );
    }

    /// `path` and a hard link `other` to it holding `before`.
    fn linked(dir: &Path, before: &[u8]) -> (PathBuf, PathBuf) {
        let path = dir.join("out.bin");
        let other = dir.join("copy.bin");
        std::fs::write(&path, before).unwrap();
        std::fs::hard_link(&path, &other).unwrap();
        (path, other)
    }

    #[test]
    fn writers_never_reach_a_hard_link() {
        let d = tempfile::tempdir().unwrap();
        let (path, other) = linked(d.path(), b"old");
        write_bytes(&path, b"new").unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), b"new");
        assert_eq!(std::fs::read(&other).unwrap(), b"old");

        let d = tempfile::tempdir().unwrap();
        let (path, other) = linked(d.path(), b"old");
        let mut f = AtomicFile::create(&path).unwrap();
        f.write_all(b"streamed").unwrap();
        // Uncommitted: nothing visible yet.
        assert_eq!(std::fs::read(&path).unwrap(), b"old");
        assert_eq!(f.commit().unwrap(), 8);
        assert_eq!(std::fs::read(&path).unwrap(), b"streamed");
        assert_eq!(std::fs::read(&other).unwrap(), b"old");

        let d = tempfile::tempdir().unwrap();
        let (path, other) = linked(d.path(), b"old");
        let src = d.path().join("src.bin");
        std::fs::write(&src, b"copied").unwrap();
        assert_eq!(copy_file(&src, &path).unwrap(), 6);
        assert_eq!(std::fs::read(&path).unwrap(), b"copied");
        assert_eq!(std::fs::read(&other).unwrap(), b"old");

        let d = tempfile::tempdir().unwrap();
        let (path, other) = linked(d.path(), b"line 1\n");
        append_bytes(&path, b"line 2\n").unwrap();
        append_bytes(&path, b"line 3\n").unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), b"line 1\nline 2\nline 3\n");
        assert_eq!(std::fs::read(&other).unwrap(), b"line 1\n");

        let d = tempfile::tempdir().unwrap();
        let (path, other) = linked(d.path(), b"old log");
        let mut f = create_log(&path).unwrap();
        f.write_all(b"new log").unwrap();
        drop(f);
        assert_eq!(std::fs::read(&path).unwrap(), b"new log");
        assert_eq!(std::fs::read(&other).unwrap(), b"old log");
    }

    #[test]
    fn dropped_uncommitted_leaves_the_target() {
        let d = tempfile::tempdir().unwrap();
        let path = d.path().join("x");
        std::fs::write(&path, b"kept").unwrap();
        let mut f = AtomicFile::create(&path).unwrap();
        f.write_all(b"half").unwrap();
        drop(f);
        assert_eq!(std::fs::read(&path).unwrap(), b"kept");
        assert_eq!(std::fs::read_dir(d.path()).unwrap().count(), 1);
    }
}
