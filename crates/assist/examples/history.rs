//! Workstation side: archives the date-dependent inputs of past days so that the assistant
//! (which never downloads) can be replayed on them. Writes
//! `data/out/<site>/assist/history/`:
//!
//! - `meteo_<start>_<end>.json`: hourly weather in the format of the `meteo-hourly` layer
//!   (P19), from the **Open-Meteo historical forecast API**
//!   (`historical-forecast-api.open-meteo.com`, the archived first hours of each run of the
//!   same models, « same models, same parameters, same response format », from 2022),
//!   plus `relative_humidity_pct` per hour; the start is 3 days before the first date (snow
//!   quality needs history). Same terms as the forecast API (free for non-commercial use,
//!   CC BY 4.0): behind `personal_sources`, like P19.
//! - `snow_<date>/`: the snow map of the date (P6, `skimap_snow::build`, Météo-France
//!   simulation, Licence Ouverte).
//! - `bera_<date>.json`: the BERA of the date (P8, `skimap_meteo::bera::build`).
//!
//! Every download goes through the shared downloader (source `open-meteo`, quotas and
//! cache). The P19, P6 and P8 builders write into a temporary data root whose cache and
//! terrain are links to the real ones, so the current layers are left untouched.
//!
//! ```sh
//! SKIMAP_PERSONAL_SOURCES=1 cargo run --release -p skimap-assist --example history -- \
//!     cervieres 2026-01-04 2026-01-10
//! ```

use std::path::Path;
use std::time::Duration;

use chrono::NaiveDate;
use skimap_common::config::Config;
use skimap_common::download::{Downloader, Request};
use skimap_common::layout::DataLayout;
use skimap_common::meta::{FileMeta, LayerMeta, SourceRef, now_rfc3339};
use skimap_common::output::write_json;
use skimap_meteo::hourly::{
    BEST_MATCH_VARIABLES, HourlyFile, MF_VARIABLES, SOURCE, Series, forecast_points, merge,
};

/// Historical forecast endpoint of Open-Meteo.
const HISTORICAL_URL: &str = "https://historical-forecast-api.open-meteo.com/v1/forecast";

fn request(
    p: &skimap_meteo::hourly::ForecastPoint,
    model: &str,
    vars: &[&str],
    start: NaiveDate,
    end: NaiveDate,
    max_age: Duration,
) -> Request {
    Request::get(
        SOURCE,
        &format!(
            "{HISTORICAL_URL}?latitude={}&longitude={}&hourly={}&models={model}\
             &timezone=Europe%2FParis&start_date={start}&end_date={end}&wind_speed_unit=kmh",
            p.lat,
            p.lon,
            vars.join(",")
        ),
    )
    .max_age(max_age)
}

fn copy_dir(from: &Path, to: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(to)?;
    for e in std::fs::read_dir(from)? {
        let e = e?;
        std::fs::copy(e.path(), to.join(e.file_name()))?;
    }
    Ok(())
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let usage = "usage: history <site> <first date> <last date> [--no-snow] [--no-bera]";
    let site = skimap_common::site::site(args.first().ok_or(usage)?)?;
    let parse = |s: Option<&String>| -> Result<NaiveDate, Box<dyn std::error::Error>> {
        Ok(NaiveDate::parse_from_str(s.ok_or(usage)?, "%Y-%m-%d")?)
    };
    let (first, last) = (parse(args.get(1))?, parse(args.get(2))?);
    let cfg = Config::from_env()?;
    if !cfg.personal_sources {
        return Err(
            "Open-Meteo is for non-commercial use only: set SKIMAP_PERSONAL_SOURCES=1".into(),
        );
    }
    let layout = cfg.layout();
    let dl = Downloader::from_config(&cfg)?;
    let hist = layout.layer_dir(site, "assist")?.join("history");
    DataLayout::ensure(&hist)?;

    // Weather.
    let start = first - chrono::Duration::days(3);
    let today = chrono::Utc::now().date_naive();
    // Past data no longer changes; the last two days may still be completed.
    let max_age = if (today - last).num_days() > 2 {
        Duration::from_secs(365 * 86_400)
    } else {
        Duration::from_secs(3600)
    };
    let mut mf_vars: Vec<&str> = MF_VARIABLES.to_vec();
    mf_vars.push("relative_humidity_2m");
    let mut points = Vec::new();
    for p in forecast_points(site.id) {
        let mf = Series::from_json(
            &dl.fetch(&request(
                p,
                "meteofrance_seamless",
                &mf_vars,
                start,
                last,
                max_age,
            ))?
            .bytes()?,
        )?;
        let best = Series::from_json(
            &dl.fetch(&request(
                p,
                "best_match",
                BEST_MATCH_VARIABLES,
                start,
                last,
                max_age,
            ))?
            .bytes()?,
        )?;
        let pf = merge(p, &mf, &best)?;
        let rh = mf.values("relative_humidity_2m");
        let mut v = serde_json::to_value(&pf)?;
        for (i, h) in v["hours"]
            .as_array_mut()
            .ok_or("hours")?
            .iter_mut()
            .enumerate()
        {
            h["relative_humidity_pct"] = rh.get(i).copied().flatten().into();
        }
        let n = pf.hours.len();
        let missing_t = pf
            .hours
            .iter()
            .filter(|h| h.temperature_c.is_none())
            .count();
        eprintln!(
            "{} {}: {n} hours from {} to {}, {missing_t} without temperature, elevation {} m",
            site.id,
            p.id,
            pf.hours.first().map(|h| h.time.as_str()).unwrap_or("-"),
            pf.hours.last().map(|h| h.time.as_str()).unwrap_or("-"),
            pf.elevation_m
        );
        points.push(v);
    }
    let file = HourlyFile {
        site: site.id.to_owned(),
        generated_at: now_rfc3339(),
        points: Vec::new(),
        attribution: "Weather data by Open-Meteo.com (CC BY 4.0), modèles Météo-France".into(),
    };
    let mut fv = serde_json::to_value(&file)?;
    fv["points"] = points.into();
    let name = format!("meteo_{start}_{last}.json");
    write_json(&hist.join(&name), &fv)?;
    LayerMeta {
        tickets: vec!["A1 (#88)".into(), "A2 (#89)".into(), "A3 (#90)".into(), "A7 (#94)".into()],
        valid_from: Some(start.to_string()),
        valid_until: Some(last.to_string()),
        sources: vec![
            SourceRef::new(
                "Open-Meteo, API historical forecast (meteofrance_seamless et best_match)",
                HISTORICAL_URL,
                "CC BY 4.0 ; API gratuite réservée à l'usage non commercial",
                "Weather data by Open-Meteo.com",
            )
            .personal_use_only(),
        ],
        files: vec![FileMeta::new(
            &name,
            "météo horaire archivée (format HourlyFile de P19, plus relative_humidity_pct)",
        )],
        notes: vec![
            "Premières heures de chaque run archivées par Open-Meteo, mêmes modèles et variables que P19.".into(),
            "Heures locales Europe/Paris.".into(),
        ],
        ..LayerMeta::new("meteo-hourly", site.id)
    }
    .write(&hist)?;
    eprintln!("wrote {}", hist.join(&name).display());

    // Snow maps and BERA, built in a temporary data root.
    let tmp = tempfile::tempdir()?;
    let tl = DataLayout::new(tmp.path());
    std::fs::create_dir_all(tl.out_dir(site))?;
    std::os::unix::fs::symlink(
        std::fs::canonicalize(layout.layer_dir(site, "terrain")?)?,
        tl.layer_dir(site, "terrain")?,
    )?;
    let mut d = first;
    while d <= last {
        if !args.iter().any(|a| a == "--no-snow") {
            let b = skimap_snow::build(site, &tl, &dl, Some(d))?;
            let _ = b;
            copy_dir(
                &tl.layer_dir(site, "snow")?,
                &hist.join(format!("snow_{d}")),
            )?;
            eprintln!("snow {d}: done");
        }
        if !args.iter().any(|a| a == "--no-bera") {
            let b = skimap_meteo::bera::build(site, &tl, &dl, cfg.meteofrance_api_key.as_ref(), d)?;
            let dir = tl.layer_dir(site, "bera")?;
            std::fs::copy(dir.join("bera.json"), hist.join(format!("bera_{d}.json")))?;
            std::fs::copy(dir.join("bera.meta.json"), hist.join("bera.meta.json"))?;
            eprintln!(
                "bera {d}: {:?}, level {:?}",
                b.status,
                b.bulletin.as_ref().and_then(|x| x.risk.max)
            );
        }
        d += chrono::Duration::days(1);
    }
    Ok(())
}
