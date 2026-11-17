//! France basemap (X20, #154): a light, low-zoom background of the whole of metropolitan
//! France and Corsica, shared by every site, so that the free mode moves smoothly over
//! the whole country without any pack installed.
//!
//! Precomputed by the pipeline ([`crate::build::france`]: Protomaps extract of the
//! France box, zooms 0 to [`MAXZOOM`], and the glyphs of its labels), published by the
//! server (`/v1/basemap/france.json`, files named by their SHA-256) and kept in the cache
//! of the phone, which draws it under the packs. This module is the description read by
//! the server and the phone (no network, no SQLite).
//!
//! ```text
//! <dir>/france.json                         this description
//! <dir>/france.pmtiles                      vector tiles, Protomaps schema
//! <dir>/fonts/<fontstack>/<range>.pbf       glyphs of the labels
//! ```

use std::path::Path;

use serde::{Deserialize, Serialize};
use skimap_common::geom::BBox;

use crate::error::{Error, Result};
use crate::manifest::{FileEntry, Source};
use crate::reader::check_relative;

/// `format` of the description.
pub const FORMAT: &str = "skimap-france-basemap";
/// `format_version` of the description.
pub const FORMAT_VERSION: u32 = 1;
/// Name of the description in its directory.
pub const DESCRIPTION_FILE: &str = "france.json";
/// Name of the tile archive in its directory.
pub const TILES_FILE: &str = "france.pmtiles";
/// Directory of the glyphs, `fonts/{fontstack}/{range}.pbf`.
pub const FONTS_DIR: &str = "fonts";
/// Highest zoom of the archive. Beyond it MapLibre enlarges the tiles of this zoom; the
/// packs draw their own basemap where they are installed.
pub const MAXZOOM: u8 = 6;
/// Metropolitan France and Corsica, with a margin (the neighbouring borders show).
pub const BBOX: BBox = BBox {
    west: -5.5,
    south: 41.2,
    east: 9.8,
    north: 51.3,
};
/// Font stacks of the labels of the France layers of the styles (`crates/styles`): those
/// of the Plan IGN labels, so one glyph template serves both.
pub const FONT_STACKS: [&str; 2] = ["Source Sans Pro Regular", "Source Sans Pro Semibold"];

/// Description of a France basemap (`france.json`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FranceBasemap {
    /// [`FORMAT`].
    pub format: String,
    /// [`FORMAT_VERSION`].
    pub format_version: u32,
    /// Protomaps daily build, `AAAAMMJJ.pmtiles`.
    pub build: String,
    /// Tile schema version of the build.
    pub schema: String,
    /// Date of the OSM data (`AAAA-MM-JJ`): the date shown, kept while the build is in use.
    pub data_date: String,
    /// Box of the extract, `[west, south, east, north]`.
    pub bbox: [f64; 4],
    /// Lowest zoom of the archive.
    pub minzoom: u8,
    /// Highest zoom of the archive.
    pub maxzoom: u8,
    /// Every file but this description, sorted by path.
    pub files: Vec<FileEntry>,
    /// Sources, licences and attributions.
    pub sources: Vec<Source>,
}

impl FranceBasemap {
    /// Total size of the files.
    pub fn bytes(&self) -> u64 {
        self.files.iter().map(|f| f.bytes).sum()
    }

    /// The tile archive entry.
    pub fn tiles(&self) -> Option<&FileEntry> {
        self.files.iter().find(|f| f.path == TILES_FILE)
    }

    /// Attributions to show, without repeats.
    pub fn attributions(&self) -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        for s in &self.sources {
            let a = s.attribution.trim();
            if !a.is_empty() && !out.iter().any(|x| x == a) {
                out.push(a.to_owned());
            }
        }
        out
    }

    /// Checks the format, the version, the file paths and that the archive is listed.
    pub fn validate(&self) -> Result<()> {
        if self.format != FORMAT {
            return Err(Error::Pack(format!(
                "France basemap: format {:?}, expected {FORMAT:?}",
                self.format
            )));
        }
        if self.format_version != FORMAT_VERSION {
            return Err(Error::Pack(format!(
                "France basemap: format version {}, expected {FORMAT_VERSION}",
                self.format_version
            )));
        }
        for f in &self.files {
            check_relative(&f.path)?;
            if f.path == DESCRIPTION_FILE {
                return Err(Error::Pack("France basemap lists its description".into()));
            }
            if f.sha256.len() != 64 || !f.sha256.bytes().all(|b| b.is_ascii_hexdigit()) {
                return Err(Error::Pack(format!(
                    "France basemap: bad SHA-256 for {}",
                    f.path
                )));
            }
        }
        if let Some(s) = self.sources.iter().find(|s| s.personal_use_only) {
            return Err(Error::Pack(format!(
                "France basemap with a personal-use source: {}",
                s.name
            )));
        }
        if self.tiles().is_none() {
            return Err(Error::Pack(format!("France basemap without {TILES_FILE}")));
        }
        Ok(())
    }

    /// Parses and validates a description.
    pub fn parse(bytes: &[u8]) -> Result<Self> {
        let d: Self =
            serde_json::from_slice(bytes).map_err(|e| Error::json(DESCRIPTION_FILE, e))?;
        d.validate()?;
        Ok(d)
    }

    /// Reads `dir/france.json`.
    pub fn read(dir: &Path) -> Result<Self> {
        let p = dir.join(DESCRIPTION_FILE);
        Self::parse(&std::fs::read(&p).map_err(|e| Error::io(&p, e))?)
    }

    /// Whether every file of `dir` matches the description (size and SHA-256).
    pub fn verify(&self, dir: &Path) -> Result<()> {
        for f in &self.files {
            let p = dir.join(&f.path);
            let (bytes, sha) = crate::reader::sha256_file(&p)?;
            if bytes != f.bytes || sha != f.sha256 {
                return Err(Error::Pack(format!(
                    "France basemap: {} differs from its description",
                    f.path
                )));
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> FranceBasemap {
        FranceBasemap {
            format: FORMAT.to_owned(),
            format_version: FORMAT_VERSION,
            build: "20261117.pmtiles".to_owned(),
            schema: "4.13.6".to_owned(),
            data_date: "2026-11-17".to_owned(),
            bbox: [BBOX.west, BBOX.south, BBOX.east, BBOX.north],
            minzoom: 0,
            maxzoom: MAXZOOM,
            files: vec![FileEntry {
                path: TILES_FILE.to_owned(),
                bytes: 3,
                sha256: "a".repeat(64),
            }],
            sources: vec![
                Source::new("a", "https://x", "ODbL", "© OSM"),
                Source::new("b", "https://y", "ODbL", "© OSM"),
            ],
        }
    }

    #[test]
    fn parses_what_it_writes() {
        let d = sample();
        let back = FranceBasemap::parse(&serde_json::to_vec(&d).unwrap()).unwrap();
        assert_eq!(back, d);
        assert_eq!(back.bytes(), 3);
        assert_eq!(back.attributions(), vec!["© OSM".to_owned()]);
    }

    #[test]
    fn refuses_bad_descriptions() {
        let mut d = sample();
        d.files[0].path = "../x".to_owned();
        assert!(d.validate().is_err());
        let mut d = sample();
        d.files.clear();
        assert!(d.validate().is_err());
        let mut d = sample();
        d.format_version = 99;
        assert!(d.validate().is_err());
        let mut d = sample();
        d.files[0].sha256 = "zz".to_owned();
        assert!(d.validate().is_err());
        let mut d = sample();
        d.sources[0].personal_use_only = true;
        assert!(d.validate().is_err());
    }

    #[test]
    fn verifies_files() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join(TILES_FILE), b"abc").unwrap();
        let mut d = sample();
        d.files[0].sha256 = crate::reader::sha256_file(&dir.path().join(TILES_FILE))
            .unwrap()
            .1;
        d.verify(dir.path()).unwrap();
        std::fs::write(dir.path().join(TILES_FILE), b"abd").unwrap();
        assert!(d.verify(dir.path()).is_err());
    }
}
