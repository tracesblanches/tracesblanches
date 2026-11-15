//! `skimap assist`: the assistant layer of a site (A1 to A9), `data/out/<site>/assist/`.

use std::path::PathBuf;

use anyhow::{Result, bail};
use chrono::NaiveDate;
use skimap_common::geom::LonLat;
use skimap_common::layout::DataLayout;
use skimap_common::site::Site;

/// Options of `skimap assist`.
#[derive(Debug, Clone, Default)]
pub struct AssistArgs {
    /// First day (default: today, legal time).
    pub date: Option<NaiveDate>,
    /// Days after the first one.
    pub days: u32,
    /// Home point, for the distances of the briefing.
    pub home: Option<LonLat>,
    /// Planned route (GeoJSON with a LineString).
    pub route: Option<PathBuf>,
    /// The three suggestion requests of the site ([`skimap_assist::presets`]).
    pub suggest: bool,
    /// Outings on the core proof routes.
    pub outings: bool,
    /// Past days archived by `skimap history`: `past`, or `with-forecast`.
    pub history: Option<String>,
}

/// Parses `lon,lat`.
pub fn parse_lon_lat(s: &str) -> Result<LonLat> {
    let (lon, lat) = s
        .split_once(',')
        .ok_or_else(|| anyhow::anyhow!("`{s}` : attendu lon,lat"))?;
    Ok(LonLat::new(lon.trim().parse()?, lat.trim().parse()?))
}

/// Runs the assistant; returns the output and the lines to print (texts of the day).
pub fn assist(
    layout: &DataLayout,
    site: &Site,
    a: &AssistArgs,
) -> Result<(skimap_assist::Output, Vec<String>)> {
    let date = a
        .date
        .unwrap_or_else(|| skimap_core::ephemeris::local_time(chrono::Utc::now()).date());
    let mut opts = skimap_assist::Options {
        date: Some(date),
        extra_days: a.days,
        home: a.home,
        ..Default::default()
    };
    let mut lines = Vec::new();
    match a.history.as_deref() {
        None => {}
        Some(h @ ("past" | "with-forecast")) => {
            opts.paths = skimap_assist::presets::history_paths(layout, site, h == "with-forecast")?;
        }
        Some(other) => bail!("--history {other} : past ou with-forecast"),
    }
    opts.paths.route = a.route.clone();
    if a.suggest {
        opts.requests = skimap_assist::presets::suggestion_requests(site, date);
    }
    if a.outings {
        let (outings, skipped) = skimap_assist::presets::proof_outings(layout, site, date)?;
        opts.outings = outings;
        lines.extend(skipped.into_iter().map(|s| format!("sortie ignorée : {s}")));
    }
    let out = skimap_assist::build(site, layout, &opts)?;
    for d in &out.briefing.days {
        lines.push(format!("--- {}", d.date));
        lines.extend(d.text.iter().cloned());
    }
    for w in &out.wax {
        lines.push(format!("Fartage {} {} : {}", w.date, w.point, w.text));
    }
    for s in &out.synthesis {
        lines.push(format!("Synthèse {} : {}", s.date, s.text()));
    }
    for s in &out.suggestions {
        lines.extend(s.text.iter().cloned());
    }
    for b in &out.best_times {
        lines.push(format!(
            "Départ {} {} : {}",
            b.request.date, b.point, b.text
        ));
    }
    for e in &out.equipment {
        lines.push(format!("Équipement : {}", e.text));
    }
    for f in &out.files {
        lines.push(format!("écrit {}", f.display()));
    }
    Ok((out, lines))
}
