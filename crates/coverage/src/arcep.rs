//! ARCEP "Mon Réseau Mobile" theoretical coverage maps: releases, operators, archives.
//!
//! One 7z archive per operator, technology and use for metropolitan France, holding one
//! GeoPackage in Lambert-93 (EPSG:2154), one feature per département and coverage level
//! (TBC very good, BC good, CL limited). 2G/3G voice maps are published twice a year
//! (T2 and T4). Documentation:
//! <https://data.arcep.fr/mobile/couvertures_theoriques/documentation_couverture.md>.

use std::fmt;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use sevenz_rust2::{ArchiveReader, Password};
use skimap_common::download::{Downloader, Request};

use crate::error::{Error, Result};

/// Root of the coverage files.
pub const BASE_URL: &str = "https://data.arcep.fr/mobile/couvertures_theoriques";

/// Mobile network operators of metropolitan France.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Operator {
    /// Orange (`OF`, MCC-MNC 20801).
    Orange,
    /// SFR (`SFR0`, 20810).
    Sfr,
    /// Free Mobile (`FREE`, 20815). No 2G map since 2024 T4: its 2G/3G map is 3G.
    Free,
    /// Bouygues Telecom (`BOUY`, 20820).
    Bouygues,
}

impl Operator {
    /// The four operators.
    pub const ALL: [Operator; 4] = [
        Operator::Orange,
        Operator::Sfr,
        Operator::Free,
        Operator::Bouygues,
    ];

    /// Code used in the file names.
    pub fn code(self) -> &'static str {
        match self {
            Operator::Orange => "OF",
            Operator::Sfr => "SFR0",
            Operator::Free => "FREE",
            Operator::Bouygues => "BOUY",
        }
    }

    /// Commercial name.
    pub fn name(self) -> &'static str {
        match self {
            Operator::Orange => "Orange",
            Operator::Sfr => "SFR",
            Operator::Free => "Free",
            Operator::Bouygues => "Bouygues",
        }
    }
}

/// A publication quarter of the 2G/3G voice maps.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Release {
    /// Year.
    pub year: u16,
    /// Quarter, 1..=4 (2G/3G voice maps: 2 and 4).
    pub quarter: u8,
}

impl Release {
    /// Latest release checked on 14/11/2026 (T2 2026, published 24/09/2026).
    pub const LATEST_KNOWN: Release = Release {
        year: 2026,
        quarter: 2,
    };

    /// `2026_T2`.
    pub fn tag(self) -> String {
        format!("{}_T{}", self.year, self.quarter)
    }

    /// Last day of the quarter (the `date` of the features), ISO.
    pub fn quarter_end(self) -> String {
        let md = match self.quarter {
            1 => "03-31",
            2 => "06-30",
            3 => "09-30",
            _ => "12-31",
        };
        format!("{}-{md}", self.year)
    }

    /// GeoPackage name inside the archive.
    pub fn gpkg_name(self, op: Operator) -> String {
        format!("{}_couv_Metropole_{}_2G3G_voix.gpkg", self.tag(), op.code())
    }

    /// Archive name.
    pub fn archive_name(self, op: Operator) -> String {
        format!("{}.7z", self.gpkg_name(op))
    }

    /// Download URL (redirects to the OVH object storage).
    pub fn url(self, op: Operator) -> String {
        format!(
            "{BASE_URL}/{}/Metropole/00_Metropole/{}",
            self.tag(),
            self.archive_name(op)
        )
    }
}

impl fmt::Display for Release {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.tag())
    }
}

/// The archive of an operator: from `archive_dir` when it holds it (archives already
/// downloaded, e.g. `research/raw/arcep/dl`), otherwise through the shared downloader
/// (source `arcep`, cached).
pub fn archive(
    dl: &Downloader,
    release: Release,
    op: Operator,
    archive_dir: Option<&Path>,
) -> Result<PathBuf> {
    if let Some(d) = archive_dir {
        let p = d.join(release.archive_name(op));
        if p.is_file() {
            return Ok(p);
        }
    }
    Ok(dl.fetch(&Request::get("arcep", &release.url(op)))?.path)
}

/// Provenance of an extracted GeoPackage, to re-extract when the archive changes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct Stamp {
    archive: PathBuf,
    archive_len: u64,
    archive_mtime: u64,
    gpkg_len: u64,
}

fn file_len_mtime(p: &Path) -> Result<(u64, u64)> {
    let m = std::fs::metadata(p).map_err(|e| skimap_common::Error::io(p, e))?;
    let mtime = m
        .modified()
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map_or(0, |d| d.as_secs());
    Ok((m.len(), mtime))
}

/// Extracts `name` from the 7z archive into `dest_dir`, unless an extraction of the same
/// archive is already there. Returns the GeoPackage path and the time spent extracting
/// (`None` when reused).
pub fn extract(archive: &Path, name: &str, dest_dir: &Path) -> Result<(PathBuf, Option<Duration>)> {
    std::fs::create_dir_all(dest_dir).map_err(|e| skimap_common::Error::io(dest_dir, e))?;
    let dest = dest_dir.join(name);
    let stamp_path = dest_dir.join(format!("{name}.source.json"));
    let (archive_len, archive_mtime) = file_len_mtime(archive)?;
    if dest.is_file()
        && let Ok(s) = std::fs::read_to_string(&stamp_path)
        && let Ok(stamp) = serde_json::from_str::<Stamp>(&s)
        && stamp.archive_len == archive_len
        && stamp.archive_mtime == archive_mtime
        && file_len_mtime(&dest)?.0 == stamp.gpkg_len
    {
        return Ok((dest, None));
    }
    let t0 = Instant::now();
    let part = dest_dir.join(format!("{name}.part"));
    let mut reader =
        ArchiveReader::open(archive, Password::empty()).map_err(|e| Error::archive(archive, e))?;
    let mut found = false;
    let mut io_error: Option<std::io::Error> = None;
    reader
        .for_each_entries(|entry, r| {
            // Only the expected file name is written, never a path from the archive.
            if entry.is_directory() || entry.name() != name {
                return Ok(true);
            }
            found = true;
            let res = std::fs::File::create(&part).and_then(|f| {
                let mut w = std::io::BufWriter::with_capacity(1 << 20, f);
                let mut buf = vec![0u8; 1 << 20];
                loop {
                    let n = r.read(&mut buf)?;
                    if n == 0 {
                        break;
                    }
                    w.write_all(&buf[..n])?;
                }
                w.flush()
            });
            if let Err(e) = res {
                io_error = Some(e);
            }
            Ok(true)
        })
        .map_err(|e| Error::archive(archive, e))?;
    if let Some(e) = io_error {
        return Err(skimap_common::Error::io(&part, e).into());
    }
    if !found {
        return Err(Error::archive(archive, format!("no entry named {name}")));
    }
    std::fs::rename(&part, &dest).map_err(|e| skimap_common::Error::io(&dest, e))?;
    let stamp = Stamp {
        archive: archive.to_owned(),
        archive_len,
        archive_mtime,
        gpkg_len: file_len_mtime(&dest)?.0,
    };
    let s = serde_json::to_string_pretty(&stamp).expect("stamp serialises");
    std::fs::write(&stamp_path, s).map_err(|e| skimap_common::Error::io(&stamp_path, e))?;
    Ok((dest, Some(t0.elapsed())))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_and_urls() {
        let r = Release::LATEST_KNOWN;
        assert_eq!(r.tag(), "2026_T2");
        assert_eq!(r.quarter_end(), "2026-06-30");
        assert_eq!(
            r.gpkg_name(Operator::Sfr),
            "2026_T2_couv_Metropole_SFR0_2G3G_voix.gpkg"
        );
        assert_eq!(
            r.url(Operator::Bouygues),
            "https://data.arcep.fr/mobile/couvertures_theoriques/2026_T2/Metropole/00_Metropole/2026_T2_couv_Metropole_BOUY_2G3G_voix.gpkg.7z"
        );
    }

    #[test]
    fn extraction_and_reuse() {
        let dir = tempfile::tempdir().unwrap();
        let src = dir.path().join("src");
        std::fs::create_dir_all(&src).unwrap();
        std::fs::write(src.join("x.gpkg"), b"hello geopackage").unwrap();
        let archive = dir.path().join("x.gpkg.7z");
        sevenz_rust2::compress_to_path(&src, &archive).unwrap();
        let out = dir.path().join("out");
        let (p, t) = extract(&archive, "x.gpkg", &out).unwrap();
        assert!(t.is_some());
        assert_eq!(std::fs::read(&p).unwrap(), b"hello geopackage");
        let (_, t) = extract(&archive, "x.gpkg", &out).unwrap();
        assert!(t.is_none(), "second call reuses the extraction");
        assert!(extract(&archive, "missing.gpkg", &out).is_err());
    }
}
