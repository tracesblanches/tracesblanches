//! `skimap history`: archives the date-dependent inputs of past days so that the assistant
//! (which never downloads) can be replayed on them, in `data/out/<site>/assist/history/`:
//!
//! - `meteo_<start>_<end>.json`: hourly weather of the historical forecast API
//!   (`skimap_meteo::hourly::history`, from three days before the first date; personal
//!   use only, behind `personal_sources`);
//! - `snow_<date>/`: the snow map of each date (P6, `skimap_snow::build`);
//! - `bera_<date>.json`: the BERA of each date (P8, `skimap_meteo::bera::build`).
//!
//! The snow and BERA builders write into a temporary data root whose terrain is a link to
//! the real one, so the current layers are left untouched; every download goes through
//! the shared downloader.

use std::path::Path;

use anyhow::{Result, bail};
use chrono::NaiveDate;
use skimap_common::layout::DataLayout;
use skimap_common::site::Site;

use crate::build::Ctx;

/// Directory of the archived inputs of the assistant.
pub fn history_dir(layout: &DataLayout, site: &Site) -> Result<std::path::PathBuf> {
    Ok(layout.layer_dir(site, "assist")?.join("history"))
}

fn copy_dir(from: &Path, to: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(to)?;
    for e in std::fs::read_dir(from)? {
        let e = e?;
        std::fs::copy(e.path(), to.join(e.file_name()))?;
    }
    Ok(())
}

/// Archives the weather, snow maps and BERA of `first..=last`; returns the lines to print.
pub fn history(
    ctx: &Ctx,
    site: &Site,
    first: NaiveDate,
    last: NaiveDate,
    snow: bool,
    bera: bool,
) -> Result<Vec<String>> {
    if last < first {
        bail!("{last} est avant {first}");
    }
    let dir = history_dir(&ctx.layout, site)?;
    let mut lines = Vec::new();
    let (path, file) =
        skimap_meteo::hourly::history(site, &ctx.dl, ctx.cfg.personal_sources, first, last, &dir)?;
    for p in &file.points {
        let rh = p
            .hours
            .iter()
            .filter(|h| h.relative_humidity_pct.is_some())
            .count();
        lines.push(format!(
            "météo {} : {} heures de {} à {}, {} sans température, {rh} avec humidité, altitude {} m",
            p.id,
            p.hours.len(),
            p.hours.first().map_or("-", |h| h.time.as_str()),
            p.hours.last().map_or("-", |h| h.time.as_str()),
            p.hours.iter().filter(|h| h.temperature_c.is_none()).count(),
            p.elevation_m
        ));
    }
    lines.push(format!("écrit {}", path.display()));
    if !snow && !bera {
        return Ok(lines);
    }
    let tmp = tempfile::tempdir()?;
    let tl = DataLayout::new(tmp.path());
    std::fs::create_dir_all(tl.out_dir(site))?;
    std::os::unix::fs::symlink(
        std::fs::canonicalize(ctx.layout.layer_dir(site, "terrain")?)?,
        tl.layer_dir(site, "terrain")?,
    )?;
    let mut d = first;
    while d <= last {
        if snow {
            let b = skimap_snow::build(site, &tl, &ctx.dl, Some(d))?;
            copy_dir(&tl.layer_dir(site, "snow")?, &dir.join(format!("snow_{d}")))?;
            lines.push(format!(
                "neige {d} : simulation du {}, {} cellules",
                b.date, b.cells_estimated
            ));
        }
        if bera {
            let b = skimap_meteo::bera::build(
                site,
                &tl,
                &ctx.dl,
                ctx.cfg.meteofrance_api_key.as_ref(),
                d,
            )?;
            let bd = tl.layer_dir(site, "bera")?;
            std::fs::copy(bd.join("bera.json"), dir.join(format!("bera_{d}.json")))?;
            std::fs::copy(bd.join("bera.meta.json"), dir.join("bera.meta.json"))?;
            lines.push(format!(
                "BERA {d} : {:?}, risque {:?}",
                b.status,
                b.bulletin.as_ref().and_then(|x| x.risk.max)
            ));
        }
        d += chrono::Duration::days(1);
    }
    Ok(lines)
}
