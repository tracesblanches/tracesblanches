//! Ready-made inputs of the assistant for a site (starts and proof outings from the site
//! table of `skimap-common`), shared by `skimap
//! assist` and `skimap build-all`: archived past days ([`history_paths`]), three suggestion
//! requests ([`suggestion_requests`]) and the outings of the core proof routes
//! ([`proof_outings`]).

use std::path::PathBuf;

use chrono::{NaiveDate, NaiveTime};
use skimap_common::layout::DataLayout;
use skimap_common::site::Site;
use skimap_core::graph::Difficulty;
use skimap_core::routing::Style;

use crate::equipment::Outing;
use crate::error::{Error, Result};
use crate::inputs::Paths;
use crate::suggest::Request;

/// Inputs of past days archived in `assist/history/` (`skimap history`): every
/// `meteo_*.json` (oldest first), every `snow_<date>/` and `bera_<date>.json` (each day
/// takes its own). `with_forecast` adds the current forecast (`meteo-hourly`) and snow map.
pub fn history_paths(layout: &DataLayout, site: &Site, with_forecast: bool) -> Result<Paths> {
    let h = layout.layer_dir(site, "assist")?.join("history");
    let mut entries: Vec<PathBuf> = std::fs::read_dir(&h)
        .map_err(|source| Error::Io {
            path: h.clone(),
            source,
        })?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .collect();
    entries.sort();
    let mut paths = Paths::default();
    for e in entries {
        let n = e
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        if n.starts_with("meteo_") && n.ends_with(".json") && !n.ends_with(".meta.json") {
            paths.meteo.push(e);
        } else if n.starts_with("snow_") && e.is_dir() {
            paths.snow_dirs.push(e);
        } else if n.starts_with("bera_") && n.ends_with(".json") {
            paths.beras.push(e);
        }
    }
    if with_forecast {
        paths
            .meteo
            .push(layout.layer_dir(site, "meteo-hourly")?.join("hourly.json"));
        paths.snow_dirs.push(layout.layer_dir(site, "snow")?);
    }
    Ok(paths)
}

/// Three requests on `date`: « 2 h skating, back before 16 h », « 3 h classic » from the
/// main start, « 1 h 30 classic, green and blue » at 14:30 (from Les Fonts at Cervières).
pub fn suggestion_requests(site: &Site, date: NaiveDate) -> Vec<Request> {
    let at = |h: u32, m: u32| date.and_hms_opt(h, m, 0).expect("time");
    // Starts of the site table (`skimap_common::site::starts`): the main one, and the
    // second one for the easy request when the site has two (Les Fonts at Cervières);
    // otherwise the site centre.
    let starts = skimap_common::site::starts(site);
    let (sa, sb) = (&starts[0], starts.get(1).unwrap_or(&starts[0]));
    let (a, an, b, bn) = (
        sa.position(),
        sa.phrase.as_str(),
        sb.position(),
        sb.phrase.as_str(),
    );
    #[allow(clippy::too_many_arguments)]
    let req =
        |label: &str, start, name: &str, t, min, style, back, max: Option<Difficulty>| Request {
            label: label.into(),
            start,
            start_name: name.into(),
            start_time: t,
            budget_min: min,
            style,
            return_before: back,
            max_difficulty: max,
            max_results: 3,
        };
    vec![
        req(
            "2 h, skating, retour avant 16 h",
            a,
            an,
            at(10, 0),
            120.0,
            Style::Skating,
            NaiveTime::from_hms_opt(16, 0, 0),
            None,
        ),
        req(
            "3 h, classique",
            a,
            an,
            at(10, 0),
            180.0,
            Style::Classic,
            None,
            None,
        ),
        req(
            "1 h 30, classique, pistes vertes et bleues, départ 14 h 30",
            b,
            bn,
            at(14, 30),
            90.0,
            Style::Classic,
            None,
            Some(Difficulty::Easy),
        ),
    ]
}

/// Typical outings on the core proof routes (A6) on `date`, with the time core computed
/// for them (C5 classic time, C3 off-piste cost); a route file that does not exist is
/// skipped and named in the second list.
pub fn proof_outings(
    layout: &DataLayout,
    site: &Site,
    date: NaiveDate,
) -> Result<(Vec<Outing>, Vec<String>)> {
    let dir = layout.layer_dir(site, "routes")?;
    // Proof outings of the site table (`skimap_common::site::local`); none elsewhere.
    let list: Vec<(&str, &str, u32, u32, bool)> = skimap_common::site::local(site.id)
        .map(|l| {
            l.proof_outings
                .iter()
                .map(|o| {
                    let t =
                        skimap_common::site::parse_hhmm(&o.start_time).expect("site table time");
                    (
                        o.label.as_str(),
                        o.file.as_str(),
                        chrono::Timelike::hour(&t),
                        chrono::Timelike::minute(&t),
                        o.offpiste,
                    )
                })
                .collect()
        })
        .unwrap_or_default();
    let (mut outings, mut skipped) = (Vec::new(), Vec::new());
    for (label, file, h, m, off) in &list {
        let path = dir.join(file);
        if !path.is_file() {
            skipped.push(format!("{label} : {} absent", path.display()));
            continue;
        }
        let v: serde_json::Value = crate::layers::read_json(&path)?;
        let f = v["features"]
            .as_array()
            .and_then(|a| a.iter().find(|f| f["properties"]["kind"] == "route"))
            .ok_or_else(|| Error::format(&path, "no route feature"))?;
        let p = &f["properties"];
        let secs = p["eta_classic_s"]
            .as_f64()
            .or_else(|| p["summary"]["cost_s"].as_f64())
            .ok_or_else(|| Error::format(&path, "no time on the route"))?;
        outings.push(Outing {
            label: (*label).into(),
            start: date.and_hms_opt(*h, *m, 0).expect("time"),
            duration_min: secs / 60.0,
            route: crate::layers::line_coords(&f["geometry"]),
            offpiste: *off,
            kick_unreliable: false,
        });
    }
    Ok((outings, skipped))
}
