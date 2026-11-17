//! France basemap builder (X20, #154): Protomaps extract of [`france::BBOX`] up to
//! [`france::MAXZOOM`], and the glyphs of its labels, written to a directory described by
//! `france.json` ([`FranceBasemap`]).
//!
//! The extract reads the planet archive by HTTP range requests ([`crate::build::extract`],
//! the downloader caches each range). Every daily build changes the low zooms (they take
//! in edits made anywhere, [`osm`] module doc), so the build in use is kept until it is
//! [`osm::MAX_AGE_DAYS`] older than the latest one or the tile schema changed
//! ([`renews`]): the phone downloads it again at most once a week, and the date shown is
//! the date of the build in use.
//!
//! Licence: ODbL, « Produced Work » of OpenStreetMap (Protomaps); glyphs from the Plan
//! IGN style (Source Sans Pro, SIL OFL 1.1).

use std::collections::BTreeSet;
use std::path::Path;
use std::time::Instant;

use serde_json::Value;
use skimap_common::download::{Downloader, Request};
use skimap_common::layout::DataLayout;

use crate::build::extract::{ExtractStats, HttpRanges};
use crate::build::osm::{self, BuildInfo};
use crate::build::{assets, copy_file, create_dir, list_files, plan_ign, source, write_json};
use crate::error::{Error, Result};
use crate::france::{
    self, BBOX, DESCRIPTION_FILE, FONT_STACKS, FORMAT, FORMAT_VERSION, FranceBasemap, TILES_FILE,
};
use crate::reader::TileArchive;

/// What [`build`] did.
#[derive(Debug, Clone)]
pub struct Built {
    /// The description written (or kept).
    pub basemap: FranceBasemap,
    /// Whether the build in use was kept (nothing rewritten).
    pub kept: bool,
    /// The extract, when one was made in this call (absent when it was cached).
    pub extract: Option<ExtractStats>,
    /// Seconds spent.
    pub seconds: f64,
}

/// Whether the latest build replaces the build of the description in use.
pub fn renews(current: Option<&FranceBasemap>, latest: &BuildInfo, maxzoom: u8) -> bool {
    let Some(cur) = current else {
        return true;
    };
    if cur.maxzoom != maxzoom || cur.format_version != FORMAT_VERSION {
        return true;
    }
    let cur_build = BuildInfo {
        key: cur.build.clone(),
        size: 0,
        uploaded: String::new(),
        version: cur.schema.clone(),
    };
    // Same rules as the site extracts, without the box comparison: the low zooms change
    // with every build, only the age and the schema count.
    let unchanged = osm::BoxChange {
        zoom: maxzoom,
        tiles: 1,
        changed: 0,
    };
    !matches!(
        osm::decide(Some(&cur_build), latest, Some(unchanged)),
        osm::Decision::Kept | osm::Decision::Same
    )
}

/// `AAAAMMJJ.pmtiles` -> `AAAA-MM-JJ`.
fn data_date(key: &str) -> String {
    let d = key.trim_end_matches(".pmtiles");
    if d.len() == 8 && d.bytes().all(|b| b.is_ascii_digit()) {
        format!("{}-{}-{}", &d[..4], &d[4..6], &d[6..])
    } else {
        d.to_owned()
    }
}

/// Builds (or keeps) the France basemap in `out` (`france.json`, `france.pmtiles`,
/// `fonts/`). `wanted_build`: a Protomaps build `AAAAMMJJ`, else the latest.
pub fn build(
    layout: &DataLayout,
    dl: &Downloader,
    out: &Path,
    wanted_build: Option<&str>,
    maxzoom: u8,
) -> Result<Built> {
    let t = Instant::now();
    let latest = osm::select_build(dl, wanted_build)?;
    let current = FranceBasemap::read(out)
        .ok()
        .filter(|d| d.verify(out).is_ok());
    if wanted_build.is_none() && !renews(current.as_ref(), &latest, maxzoom) {
        let basemap = current.ok_or_else(|| Error::Pack("France basemap vanished".into()))?;
        tracing::info!(
            in_use = basemap.build,
            latest = latest.key,
            "France basemap kept"
        );
        return Ok(Built {
            basemap,
            kept: true,
            extract: None,
            seconds: t.elapsed().as_secs_f64(),
        });
    }

    // Extract, cached by build and zoom.
    let cache = layout.cache_dir(osm::SOURCE)?.join("france");
    create_dir(&cache)?;
    let stem = latest.key.trim_end_matches(".pmtiles");
    let cached = cache.join(format!("france-{stem}-z{maxzoom}.pmtiles"));
    let mut stats = None;
    if !cached.is_file() {
        let tmp = cached.with_extension("pmtiles.part");
        let url = format!("{}{}", osm::BUILD_BASE, latest.key);
        let s = crate::build::extract::extract(
            &HttpRanges {
                dl,
                source: osm::SOURCE,
                url: &url,
            },
            BBOX,
            maxzoom,
            &tmp,
        )?;
        tracing::info!(url, ?s, "France extract");
        std::fs::rename(&tmp, &cached).map_err(|e| Error::io(&tmp, e))?;
        stats = Some(s);
    }

    // Written next to `out`, then swapped in.
    let parent = out
        .parent()
        .ok_or_else(|| Error::Pack(format!("{} has no parent", out.display())))?;
    create_dir(parent)?;
    let tmp = parent.join(format!(
        ".{}.tmp",
        out.file_name().and_then(|n| n.to_str()).unwrap_or("france")
    ));
    if tmp.exists() {
        std::fs::remove_dir_all(&tmp).map_err(|e| Error::io(&tmp, e))?;
    }
    copy_file(&cached, &tmp.join(TILES_FILE))?;
    let archive = TileArchive::open(tmp.join(TILES_FILE))?;
    let (minzoom, maxz) = archive.zoom_range();

    let ign_style: Value = dl
        .fetch(&Request::get(plan_ign::SOURCE, plan_ign::STYLE_URL))?
        .json()?;
    let glyphs_url = ign_style
        .get("glyphs")
        .and_then(Value::as_str)
        .ok_or_else(|| Error::Pack("IGN style without glyphs".into()))?;
    let stacks: BTreeSet<String> = FONT_STACKS.iter().map(|s| (*s).to_owned()).collect();
    assets::fetch_glyphs(
        dl,
        plan_ign::SOURCE,
        glyphs_url,
        &stacks,
        &tmp.join(france::FONTS_DIR),
    )?;

    let basemap = FranceBasemap {
        format: FORMAT.to_owned(),
        format_version: FORMAT_VERSION,
        build: latest.key.clone(),
        schema: latest.version.clone(),
        data_date: data_date(&latest.key),
        bbox: [BBOX.west, BBOX.south, BBOX.east, BBOX.north],
        minzoom,
        maxzoom: maxz,
        files: list_files(&tmp)?,
        sources: vec![
            source(
                &format!(
                    "Protomaps, build quotidien {} (schéma {}), emprise de la France, zooms {minzoom} à {maxz}",
                    latest.key, latest.version
                ),
                &format!("{}{}", osm::BUILD_BASE, latest.key),
                "ODbL 1.0 (Produced Work d'OpenStreetMap)",
                "© les contributeurs d'OpenStreetMap, Protomaps",
            ),
            source(
                "Glyphes du style Plan IGN (Source Sans Pro)",
                glyphs_url,
                "SIL OFL 1.1",
                "",
            ),
        ],
    };
    basemap.validate()?;
    write_json(&tmp.join(DESCRIPTION_FILE), &basemap)?;
    if out.exists() {
        std::fs::remove_dir_all(out).map_err(|e| Error::io(out, e))?;
    }
    std::fs::rename(&tmp, out).map_err(|e| Error::io(&tmp, e))?;
    Ok(Built {
        basemap,
        kept: false,
        extract: stats,
        seconds: t.elapsed().as_secs_f64(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::manifest::FileEntry;

    fn info(key: &str, version: &str) -> BuildInfo {
        BuildInfo {
            key: key.to_owned(),
            size: 0,
            uploaded: String::new(),
            version: version.to_owned(),
        }
    }

    fn described(key: &str, version: &str) -> FranceBasemap {
        FranceBasemap {
            format: FORMAT.to_owned(),
            format_version: FORMAT_VERSION,
            build: key.to_owned(),
            schema: version.to_owned(),
            data_date: data_date(key),
            bbox: [0.0; 4],
            minzoom: 0,
            maxzoom: 7,
            files: vec![FileEntry {
                path: TILES_FILE.to_owned(),
                bytes: 1,
                sha256: "0".repeat(64),
            }],
            sources: Vec::new(),
        }
    }

    #[test]
    fn renewed_weekly_or_on_schema_change() {
        let cur = described("20261110.pmtiles", "4");
        assert!(renews(None, &info("20261110.pmtiles", "4"), 7));
        assert!(!renews(Some(&cur), &info("20261110.pmtiles", "4"), 7));
        assert!(!renews(Some(&cur), &info("20261116.pmtiles", "4"), 7));
        assert!(renews(Some(&cur), &info("20261117.pmtiles", "4"), 7));
        assert!(renews(Some(&cur), &info("20261111.pmtiles", "5"), 7));
        assert!(renews(Some(&cur), &info("20261111.pmtiles", "4"), 8));
    }

    #[test]
    fn dates_of_builds() {
        assert_eq!(data_date("20261117.pmtiles"), "2026-11-17");
        assert_eq!(data_date("x.pmtiles"), "x");
    }
}
