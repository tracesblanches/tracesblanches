//! Download of the France basemap into the phone cache (X20, #154), phone side, no
//! network: the app fetches `/v1/basemap/france.json`, asks [`france_plan`] which files to
//! download (`/v1/basemap/files/<sha256>` into a work directory, named by their SHA-256),
//! then [`install_france`] checks them and puts them in place, the description last. The
//! basemap in place stays readable at every step.

use std::collections::HashSet;
use std::path::Path;

use skimap_pack::france::{DESCRIPTION_FILE, FranceBasemap};
use skimap_pack::reader::sha256_file;

use crate::sync::SyncError;

/// A file of the France basemap to download.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct FranceFile {
    /// Path in the basemap directory.
    pub path: String,
    /// SHA-256: the name of the file on the server and in the work directory.
    pub sha256: String,
    /// Size.
    pub bytes: u64,
}

/// What [`france_plan`] found.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct FrancePlan {
    /// Build of the description offered.
    pub build: String,
    /// Date of its data.
    pub data_date: String,
    /// Files to download (absent from the phone, or different).
    pub files: Vec<FranceFile>,
    /// Bytes to download.
    pub download_bytes: u64,
    /// Whether the phone already has exactly this description.
    pub up_to_date: bool,
}

fn invalid(e: impl std::fmt::Display) -> SyncError {
    SyncError::Invalid {
        detail: e.to_string(),
    }
}

/// The installed description of `dir`, if any.
fn installed(dir: &Path) -> Option<FranceBasemap> {
    FranceBasemap::read(dir).ok()
}

/// Files of `description` the basemap in `dir` lacks (by the installed description: no
/// hashing of the files in place).
#[uniffi::export]
pub fn france_plan(description: String, dir: String) -> Result<FrancePlan, SyncError> {
    let new = FranceBasemap::parse(description.as_bytes()).map_err(invalid)?;
    let dir = Path::new(&dir);
    let have: HashSet<(String, String, u64)> = installed(dir)
        .map(|d| {
            d.files
                .into_iter()
                .filter(|f| std::fs::metadata(dir.join(&f.path)).is_ok_and(|m| m.len() == f.bytes))
                .map(|f| (f.path, f.sha256, f.bytes))
                .collect()
        })
        .unwrap_or_default();
    let files: Vec<FranceFile> = new
        .files
        .iter()
        .filter(|f| !have.contains(&(f.path.clone(), f.sha256.clone(), f.bytes)))
        .map(|f| FranceFile {
            path: f.path.clone(),
            sha256: f.sha256.clone(),
            bytes: f.bytes,
        })
        .collect();
    let up_to_date = files.is_empty() && installed(dir).as_ref() == Some(&new);
    Ok(FrancePlan {
        build: new.build.clone(),
        data_date: new.data_date.clone(),
        download_bytes: files.iter().map(|f| f.bytes).sum(),
        files,
        up_to_date,
    })
}

/// Installs `description` into `dir`: every file of [`france_plan`] taken from `work`
/// (`<work>/<sha256>`, checked: length and SHA-256), the description written last, the
/// files it no longer names removed. A file that fails is deleted from `work`
/// ([`SyncError::Corrupt`]: download it again).
#[uniffi::export]
pub fn install_france(description: String, dir: String, work: String) -> Result<(), SyncError> {
    let plan = france_plan(description.clone(), dir.clone())?;
    let new = FranceBasemap::parse(description.as_bytes()).map_err(invalid)?;
    let (dir, work) = (Path::new(&dir), Path::new(&work));
    for f in &plan.files {
        let src = work.join(&f.sha256);
        let (n, h) = sha256_file(&src).map_err(invalid)?;
        if n != f.bytes || h != f.sha256 {
            let _ = std::fs::remove_file(&src);
            return Err(SyncError::Corrupt {
                detail: format!(
                    "{}: {n} bytes, SHA-256 {h}; expected {} bytes, {}",
                    src.display(),
                    f.bytes,
                    f.sha256
                ),
            });
        }
    }
    for f in &plan.files {
        let to = dir.join(&f.path);
        if let Some(p) = to.parent() {
            std::fs::create_dir_all(p).map_err(|e| invalid(format!("{}: {e}", p.display())))?;
        }
        let src = work.join(&f.sha256);
        // Same file system (the work directory sits next to the basemap); a copy else.
        if std::fs::rename(&src, &to).is_err() {
            std::fs::copy(&src, &to).map_err(|e| invalid(format!("{}: {e}", to.display())))?;
            let _ = std::fs::remove_file(&src);
        }
    }
    let tmp = dir.join(format!("{DESCRIPTION_FILE}.tmp"));
    std::fs::write(&tmp, description.as_bytes())
        .map_err(|e| invalid(format!("{}: {e}", tmp.display())))?;
    std::fs::rename(&tmp, dir.join(DESCRIPTION_FILE))
        .map_err(|e| invalid(format!("{}: {e}", dir.display())))?;
    // Files no longer named.
    let keep: HashSet<std::path::PathBuf> = new
        .files
        .iter()
        .map(|f| dir.join(&f.path))
        .chain(std::iter::once(dir.join(DESCRIPTION_FILE)))
        .collect();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        for e in std::fs::read_dir(&d).into_iter().flatten().flatten() {
            let p = e.path();
            if p.is_dir() {
                stack.push(p);
            } else if !keep.contains(&p) {
                let _ = std::fs::remove_file(&p);
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use skimap_pack::france::{FORMAT, FORMAT_VERSION, TILES_FILE};
    use skimap_pack::manifest::{FileEntry, Source};

    /// A description of `files` (path, contents), written into `server` by SHA-256.
    fn offer(server: &Path, files: &[(&str, &[u8])], build: &str) -> String {
        std::fs::create_dir_all(server).unwrap();
        let entries = files
            .iter()
            .map(|(p, b)| {
                let f = server.join("tmp");
                std::fs::write(&f, b).unwrap();
                let (bytes, sha256) = sha256_file(&f).unwrap();
                std::fs::rename(&f, server.join(&sha256)).unwrap();
                FileEntry {
                    path: (*p).to_owned(),
                    bytes,
                    sha256,
                }
            })
            .collect();
        serde_json::to_string(&FranceBasemap {
            format: FORMAT.into(),
            format_version: FORMAT_VERSION,
            build: build.into(),
            schema: "4".into(),
            data_date: "2026-11-17".into(),
            bbox: [-5.5, 41.2, 9.8, 51.3],
            minzoom: 0,
            maxzoom: 6,
            files: entries,
            sources: vec![Source::new("Protomaps", "https://x", "ODbL", "© OSM")],
        })
        .unwrap()
    }

    /// Downloads the plan from `server` into `work` (what the app does over HTTP).
    fn download(plan: &FrancePlan, server: &Path, work: &Path) {
        std::fs::create_dir_all(work).unwrap();
        for f in &plan.files {
            std::fs::copy(server.join(&f.sha256), work.join(&f.sha256)).unwrap();
        }
    }

    #[test]
    fn first_download_then_only_what_changed() {
        let tmp = tempfile::tempdir().unwrap();
        let (server, work, dir) = (
            tmp.path().join("server"),
            tmp.path().join("work"),
            tmp.path().join("france"),
        );
        let d1 = offer(
            &server,
            &[(TILES_FILE, b"tiles 1"), ("fonts/A/0-255.pbf", b"glyphs")],
            "20261110.pmtiles",
        );
        let s = |p: &Path| p.display().to_string();
        let p = france_plan(d1.clone(), s(&dir)).unwrap();
        assert_eq!(
            (p.files.len(), p.download_bytes, p.up_to_date),
            (2, 13, false)
        );
        download(&p, &server, &work);
        install_france(d1.clone(), s(&dir), s(&work)).unwrap();
        let installed = FranceBasemap::read(&dir).unwrap();
        installed.verify(&dir).unwrap();
        assert!(france_plan(d1, s(&dir)).unwrap().up_to_date);

        // New tiles: only they are downloaded; the old ones are gone.
        let d2 = offer(
            &server,
            &[(TILES_FILE, b"tiles 2"), ("fonts/A/0-255.pbf", b"glyphs")],
            "20261117.pmtiles",
        );
        let p = france_plan(d2.clone(), s(&dir)).unwrap();
        assert_eq!(p.files.len(), 1);
        assert_eq!(p.files[0].path, TILES_FILE);
        download(&p, &server, &work);
        install_france(d2.clone(), s(&dir), s(&work)).unwrap();
        assert_eq!(std::fs::read(dir.join(TILES_FILE)).unwrap(), b"tiles 2");
        FranceBasemap::read(&dir).unwrap().verify(&dir).unwrap();
        assert!(france_plan(d2, s(&dir)).unwrap().up_to_date);
    }

    #[test]
    fn a_corrupt_download_is_refused_and_the_basemap_kept() {
        let tmp = tempfile::tempdir().unwrap();
        let (server, work, dir) = (
            tmp.path().join("server"),
            tmp.path().join("work"),
            tmp.path().join("france"),
        );
        let s = |p: &Path| p.display().to_string();
        let d1 = offer(&server, &[(TILES_FILE, b"tiles 1")], "20261110.pmtiles");
        let p = france_plan(d1.clone(), s(&dir)).unwrap();
        download(&p, &server, &work);
        install_france(d1, s(&dir), s(&work)).unwrap();
        let d2 = offer(&server, &[(TILES_FILE, b"tiles 2")], "20261117.pmtiles");
        let p = france_plan(d2.clone(), s(&dir)).unwrap();
        std::fs::write(work.join(&p.files[0].sha256), b"tiles X").unwrap();
        let e = install_france(d2, s(&dir), s(&work)).unwrap_err();
        assert!(matches!(e, SyncError::Corrupt { .. }), "{e:?}");
        assert!(!work.join(&p.files[0].sha256).exists());
        assert_eq!(std::fs::read(dir.join(TILES_FILE)).unwrap(), b"tiles 1");
        FranceBasemap::read(&dir).unwrap().verify(&dir).unwrap();
        assert!(france_plan("{".into(), s(&dir)).is_err());
    }
}
