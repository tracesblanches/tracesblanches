//! Hourly weather per site, P19 (#32): wind and gusts, apparent temperature and wind
//! chill, relative humidity (fartage, A2), visibility and fog, 0 °C isotherm, snowfall,
//! cloud cover. [`history`]: the same series for past days (historical forecast API), so
//! that the assistant can be replayed on them.
//!
//! **Source for now: Open-Meteo**, free for non-commercial use only (CC BY 4.0 data,
//! 10 000 calls a day; report 03). It is therefore behind `personal_sources`: [`build`]
//! refuses to run without it.
//!
//! Two calls per forecast point, through the shared downloader (source `open-meteo`, cached
//! one hour):
//! - `models=meteofrance_seamless` (AROME / ARPEGE blend) for temperature, apparent
//!   temperature, wind, gusts, direction, snowfall, precipitation, cloud cover, weather code;
//! - `models=best_match` for `visibility` and `freezing_level_height`, which the Météo-France
//!   models of Open-Meteo leave null (checked on 14/11/2026 at Cervières, like `snow_depth`
//!   in report 03). `snow_depth` is never used: snow depth comes from the snow layer (P6).
//!
//! Each hour records which model gave each variable ([`PointForecast::models`]).
//!
//! **Switch path to Météo-France AROME** (no code yet, documented in the README):
//! - API « AROME » of the Météo-France portal (key `METEOFRANCE_API_KEY`, header `apikey`),
//!   WCS 2.0.1 `https://public-api.meteofrance.fr/public/arome/1.0/wcs/MF-NWP-HIGHRES-AROME-001-FRANCE-WCS/`
//!   (`GetCapabilities` answers 401 without key, 14/11/2026), `GetCoverage` with a
//!   lat/long/time subset per parameter: small point extractions;
//! - or the open « Paquets AROME 0,01° » (Licence Ouverte, no key) on data.gouv.fr: hourly
//!   GRIB2 files of the whole EURW1S100 grid, which need a GRIB2 decoder (not in the
//!   workspace) and large downloads.
//!
//! Output (`data/out/<site>/meteo-hourly/`): `hourly.json` ([`HourlyFile`]) and
//! `meteo-hourly.meta.json`.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::Duration;

use chrono::NaiveDate;
use serde::{Deserialize, Serialize};
use skimap_common::download::{Downloader, Request};
use skimap_common::layout::DataLayout;
use skimap_common::site::Site;

use crate::error::{Error, Result};
use skimap_common::meta::{FileMeta, LayerMeta, SourceRef, now_rfc3339};
use skimap_common::output::write_json;

/// Layer name.
pub const LAYER: &str = "meteo-hourly";
/// Downloader source.
pub const SOURCE: &str = "open-meteo";
/// Forecast endpoint.
pub const FORECAST_URL: &str = "https://api.open-meteo.com/v1/forecast";
/// Forecast length, days.
pub const FORECAST_DAYS: u32 = 3;
/// Cache lifetime of a forecast.
pub const MAX_AGE: Duration = Duration::from_secs(3600);
/// Visibility below which an hour is foggy (WMO definition of fog), metres.
pub const FOG_VISIBILITY_M: f64 = 1000.0;

/// Variables taken from `meteofrance_seamless`.
pub const MF_VARIABLES: &[&str] = &[
    "temperature_2m",
    "relative_humidity_2m",
    "apparent_temperature",
    "wind_speed_10m",
    "wind_gusts_10m",
    "wind_direction_10m",
    "snowfall",
    "precipitation",
    "cloud_cover",
    "cloud_cover_low",
    "weather_code",
];
/// Variables taken from `best_match` (null in the Météo-France models of Open-Meteo).
pub const BEST_MATCH_VARIABLES: &[&str] = &["visibility", "freezing_level_height"];

/// A forecast point of a site.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct ForecastPoint {
    /// Identifier.
    pub id: &'static str,
    /// Name.
    pub name: &'static str,
    /// Latitude.
    pub lat: f64,
    /// Longitude.
    pub lon: f64,
    /// Where the coordinates come from.
    pub provenance: &'static str,
}

/// Forecast points of an evaluation site: the starts of its nordic sectors, from the
/// site table of `skimap-common` (`skimap_common::site::local`); empty for another site
/// (see [`points`]).
pub fn forecast_points(site_id: &str) -> &'static [ForecastPoint] {
    static TABLE: std::sync::LazyLock<BTreeMap<String, Vec<ForecastPoint>>> =
        std::sync::LazyLock::new(|| {
            skimap_common::site::evaluation_records()
                .iter()
                .filter_map(|r| {
                    let starts = &r.local.as_ref()?.starts;
                    let pts = starts
                        .iter()
                        .map(|s| ForecastPoint {
                            id: s.id.as_str(),
                            name: s.name.as_str(),
                            lat: s.lat,
                            lon: s.lon,
                            provenance: s.provenance.as_str(),
                        })
                        .collect();
                    Some((r.id.clone(), pts))
                })
                .collect()
        });
    TABLE.get(site_id).map_or(&[], Vec::as_slice)
}

/// Forecast points of any site: [`forecast_points`], or else one point at the site centre
/// (`skimap_common::site::starts`: representative point of a census domain).
pub fn points(site: &Site) -> Vec<ForecastPoint> {
    let pts = forecast_points(site.id);
    if !pts.is_empty() {
        return pts.to_vec();
    }
    skimap_common::site::starts(site)
        .into_iter()
        .map(|s| ForecastPoint {
            id: Box::leak(s.id.into_boxed_str()),
            name: Box::leak(s.name.into_boxed_str()),
            lat: s.lat,
            lon: s.lon,
            provenance: Box::leak(s.provenance.into_boxed_str()),
        })
        .collect()
}

/// Request of one model for one point.
pub fn request(p: &ForecastPoint, model: &str, variables: &[&str]) -> Request {
    Request::get(
        SOURCE,
        &format!(
            "{FORECAST_URL}?latitude={}&longitude={}&hourly={}&models={model}\
             &timezone=Europe%2FParis&forecast_days={FORECAST_DAYS}&wind_speed_unit=kmh",
            p.lat,
            p.lon,
            variables.join(",")
        ),
    )
    .max_age(MAX_AGE)
}

/// An Open-Meteo answer: elevation of the grid point (DEM 90 m), hourly series.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct Series {
    /// Elevation used for the downscaling, metres.
    pub elevation: f64,
    /// Offset of the local times, seconds.
    pub utc_offset_seconds: i64,
    /// Hourly values: `time` (local ISO) and one array per variable.
    pub hourly: BTreeMap<String, serde_json::Value>,
}

impl Series {
    /// Parses an answer.
    pub fn from_json(body: &[u8]) -> Result<Self> {
        serde_json::from_slice(body).map_err(|e| Error::format(SOURCE, e.to_string()))
    }

    /// Local times.
    pub fn times(&self) -> Vec<String> {
        self.hourly
            .get("time")
            .and_then(|v| v.as_array())
            .map(|a| {
                a.iter()
                    .filter_map(|t| t.as_str().map(str::to_owned))
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Values of a variable (`None` where null or absent).
    pub fn values(&self, var: &str) -> Vec<Option<f64>> {
        self.hourly
            .get(var)
            .and_then(|v| v.as_array())
            .map(|a| a.iter().map(serde_json::Value::as_f64).collect())
            .unwrap_or_default()
    }
}

/// Wind chill index (Environment Canada / JAG-TI 2001, the « température ressentie » of
/// Météo-France): defined for `t ≤ 10 °C` and wind `≥ 4.8 km/h`, otherwise the air
/// temperature.
pub fn wind_chill(t_c: f64, wind_kmh: f64) -> f64 {
    if t_c > 10.0 || wind_kmh < 4.8 {
        return t_c;
    }
    let v = wind_kmh.powf(0.16);
    13.12 + 0.6215 * t_c - 11.37 * v + 0.3965 * t_c * v
}

/// One hour at one point.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Hour {
    /// Local time (Europe/Paris), `YYYY-MM-DDTHH:MM`.
    pub time: String,
    /// Air temperature at 2 m, °C.
    pub temperature_c: Option<f64>,
    /// Relative humidity at 2 m, % (A2 fartage).
    #[serde(default)]
    pub relative_humidity_pct: Option<f64>,
    /// Apparent temperature (Open-Meteo, Steadman), °C.
    pub apparent_temperature_c: Option<f64>,
    /// Wind chill index computed from temperature and wind, °C.
    pub wind_chill_c: Option<f64>,
    /// Mean wind at 10 m, km/h.
    pub wind_kmh: Option<f64>,
    /// Gusts at 10 m, km/h.
    pub gust_kmh: Option<f64>,
    /// Wind direction, degrees from north.
    pub wind_direction_deg: Option<f64>,
    /// Visibility, metres.
    pub visibility_m: Option<f64>,
    /// Fog: visibility below 1000 m or WMO weather code 45/48.
    pub fog: Option<bool>,
    /// Altitude of the 0 °C isotherm, metres.
    pub freezing_level_m: Option<f64>,
    /// Snowfall over the hour, cm.
    pub snowfall_cm: Option<f64>,
    /// Precipitation over the hour, mm.
    pub precipitation_mm: Option<f64>,
    /// Total cloud cover, %.
    pub cloud_cover_pct: Option<f64>,
    /// Low cloud cover, %.
    pub cloud_cover_low_pct: Option<f64>,
    /// WMO weather code.
    pub weather_code: Option<i32>,
}

/// Summary of one day at one point.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Day {
    /// Local date.
    pub date: String,
    /// Snowfall over the day, cm.
    pub snowfall_cm: f64,
    /// Mean cloud cover between 09:00 and 16:00, % (sunshine proxy).
    pub daytime_cloud_cover_pct: Option<f64>,
    /// Strongest gust, km/h.
    pub max_gust_kmh: Option<f64>,
    /// Lowest wind chill, °C.
    pub min_wind_chill_c: Option<f64>,
    /// Hours with fog between 09:00 and 16:00.
    pub daytime_fog_hours: usize,
    /// Lowest freezing level, metres.
    pub min_freezing_level_m: Option<f64>,
}

/// Forecast of one point.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PointForecast {
    /// Point identifier.
    pub id: String,
    /// Point name.
    pub name: String,
    /// Latitude.
    pub lat: f64,
    /// Longitude.
    pub lon: f64,
    /// Elevation of the forecast (Open-Meteo DEM), metres.
    pub elevation_m: f64,
    /// Model of each variable.
    pub models: BTreeMap<String, String>,
    /// Hours.
    pub hours: Vec<Hour>,
    /// Days.
    pub days: Vec<Day>,
}

/// Content of `hourly.json`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HourlyFile {
    /// Site identifier.
    pub site: String,
    /// Generation time, RFC 3339 UTC.
    pub generated_at: String,
    /// Points.
    pub points: Vec<PointForecast>,
    /// Attribution to display.
    pub attribution: String,
}

fn min_opt(v: impl Iterator<Item = Option<f64>>) -> Option<f64> {
    v.flatten().reduce(f64::min)
}

fn max_opt(v: impl Iterator<Item = Option<f64>>) -> Option<f64> {
    v.flatten().reduce(f64::max)
}

/// Merges the two model answers of a point into hours and days.
pub fn merge(p: &ForecastPoint, mf: &Series, best: &Series) -> Result<PointForecast> {
    let times = mf.times();
    if times.is_empty() || best.times() != times {
        return Err(Error::format(
            SOURCE,
            "the two models do not share the same hours",
        ));
    }
    let col = |s: &Series, v: &str| {
        let mut x = s.values(v);
        x.resize(times.len(), None);
        x
    };
    let t = col(mf, "temperature_2m");
    let rh = col(mf, "relative_humidity_2m");
    let at = col(mf, "apparent_temperature");
    let w = col(mf, "wind_speed_10m");
    let g = col(mf, "wind_gusts_10m");
    let d = col(mf, "wind_direction_10m");
    let sf = col(mf, "snowfall");
    let pr = col(mf, "precipitation");
    let cc = col(mf, "cloud_cover");
    let cl = col(mf, "cloud_cover_low");
    let wc = col(mf, "weather_code");
    let vis = col(best, "visibility");
    let fz = col(best, "freezing_level_height");
    let hours: Vec<Hour> = (0..times.len())
        .map(|i| {
            let code = wc[i].map(|c| c as i32);
            let fog = match (vis[i], code) {
                (Some(v), c) => Some(v < FOG_VISIBILITY_M || matches!(c, Some(45 | 48))),
                (None, Some(c)) => Some(matches!(c, 45 | 48)),
                (None, None) => None,
            };
            Hour {
                time: times[i].clone(),
                temperature_c: t[i],
                relative_humidity_pct: rh[i],
                apparent_temperature_c: at[i],
                wind_chill_c: match (t[i], w[i]) {
                    (Some(t), Some(w)) => Some((wind_chill(t, w) * 10.0).round() / 10.0),
                    _ => None,
                },
                wind_kmh: w[i],
                gust_kmh: g[i],
                wind_direction_deg: d[i],
                visibility_m: vis[i],
                fog,
                freezing_level_m: fz[i],
                snowfall_cm: sf[i],
                precipitation_mm: pr[i],
                cloud_cover_pct: cc[i],
                cloud_cover_low_pct: cl[i],
                weather_code: code,
            }
        })
        .collect();
    let mut by_day: BTreeMap<String, Vec<&Hour>> = BTreeMap::new();
    for h in &hours {
        by_day
            .entry(h.time.chars().take(10).collect())
            .or_default()
            .push(h);
    }
    let daytime = |h: &Hour| {
        h.time
            .get(11..13)
            .and_then(|x| x.parse::<u32>().ok())
            .is_some_and(|hh| (9..=16).contains(&hh))
    };
    let days = by_day
        .into_iter()
        .map(|(date, hs)| {
            let dt: Vec<&&Hour> = hs.iter().filter(|h| daytime(h)).collect();
            let cc: Vec<f64> = dt.iter().filter_map(|h| h.cloud_cover_pct).collect();
            Day {
                date,
                snowfall_cm: (hs.iter().filter_map(|h| h.snowfall_cm).sum::<f64>() * 10.0).round()
                    / 10.0,
                daytime_cloud_cover_pct: (!cc.is_empty())
                    .then(|| (cc.iter().sum::<f64>() / cc.len() as f64).round()),
                max_gust_kmh: max_opt(hs.iter().map(|h| h.gust_kmh)),
                min_wind_chill_c: min_opt(hs.iter().map(|h| h.wind_chill_c)),
                daytime_fog_hours: dt.iter().filter(|h| h.fog == Some(true)).count(),
                min_freezing_level_m: min_opt(hs.iter().map(|h| h.freezing_level_m)),
            }
        })
        .collect();
    let mut models = BTreeMap::new();
    for v in MF_VARIABLES {
        models.insert((*v).to_owned(), "meteofrance_seamless".to_owned());
    }
    for v in BEST_MATCH_VARIABLES {
        models.insert((*v).to_owned(), "best_match".to_owned());
    }
    models.insert("wind_chill".to_owned(), "computed (JAG-TI)".to_owned());
    Ok(PointForecast {
        id: p.id.to_owned(),
        name: p.name.to_owned(),
        lat: p.lat,
        lon: p.lon,
        elevation_m: mf.elevation,
        models,
        hours,
        days,
    })
}

/// Fetches the forecast of every point of the site and writes
/// `data/out/<site>/meteo-hourly/`. Requires `personal_sources` (Open-Meteo is free for
/// non-commercial use only).
pub fn build(
    site: &Site,
    layout: &DataLayout,
    dl: &Downloader,
    personal_sources: bool,
) -> Result<HourlyFile> {
    if !personal_sources {
        return Err(Error::PersonalSourcesDisabled(SOURCE));
    }
    let pts = points(site);
    if pts.is_empty() {
        return Err(Error::format(
            "site",
            format!("no forecast point for {}", site.id),
        ));
    }
    let mut points = Vec::new();
    for p in &pts {
        let mf = Series::from_json(
            &dl.fetch(&request(p, "meteofrance_seamless", MF_VARIABLES))?
                .bytes()?,
        )?;
        let best = Series::from_json(
            &dl.fetch(&request(p, "best_match", BEST_MATCH_VARIABLES))?
                .bytes()?,
        )?;
        points.push(merge(p, &mf, &best)?);
    }
    let generated_at = now_rfc3339();
    let file = HourlyFile {
        site: site.id.to_owned(),
        generated_at: generated_at.clone(),
        points,
        attribution: "Weather data by Open-Meteo.com (CC BY 4.0), modèles Météo-France".into(),
    };
    let dir = layout.layer_dir(site, LAYER)?;
    DataLayout::ensure(&dir)?;
    write_json(&dir.join("hourly.json"), &file)?;
    let first = file.points.first();
    LayerMeta {
        tickets: vec!["P19 (#32)".to_owned()],
        generated_at,
        valid_from: first.and_then(|p| p.hours.first()).map(|h| h.time.clone()),
        valid_until: first.and_then(|p| p.hours.last()).map(|h| h.time.clone()),
        sources: vec![
            SourceRef::new(
                "Open-Meteo, API forecast (meteofrance_seamless et best_match)",
                FORECAST_URL,
                "CC BY 4.0 ; API gratuite réservée à l'usage non commercial",
                "Weather data by Open-Meteo.com",
            )
            .personal_use_only(),
        ],
        files: vec![FileMeta::new(
            "hourly.json",
            "prévision horaire par point (HourlyFile)",
        )],
        notes: vec![
            "Heures locales Europe/Paris.".into(),
            "visibility et freezing_level_height viennent de best_match : nuls dans les \
             modèles Météo-France d'Open-Meteo."
                .into(),
            "snow_depth n'est pas utilisé (nul avec meteofrance_seamless, rapport 03).".into(),
        ],
        ..LayerMeta::new(LAYER, site.id)
    }
    .write(&dir)?;
    Ok(file)
}

/// Historical forecast endpoint of Open-Meteo: the archived first hours of each run of the
/// same models (« same models, same parameters, same response format », from 2022).
pub const HISTORICAL_URL: &str = "https://historical-forecast-api.open-meteo.com/v1/forecast";
/// Days of weather fetched before the first date of a history (the snow quality model of
/// the assistant needs the days before).
pub const HISTORY_LEAD_DAYS: i64 = 3;

/// Request of one model for one point over past dates (historical forecast API). Past
/// data no longer changes: kept a year in the cache, one hour if `last` is within two
/// days of `today` (the last runs may still be archived).
pub fn history_request(
    p: &ForecastPoint,
    model: &str,
    variables: &[&str],
    start: NaiveDate,
    last: NaiveDate,
    today: NaiveDate,
) -> Request {
    let max_age = if (today - last).num_days() > 2 {
        Duration::from_secs(365 * 86_400)
    } else {
        MAX_AGE
    };
    Request::get(
        SOURCE,
        &format!(
            "{HISTORICAL_URL}?latitude={}&longitude={}&hourly={}&models={model}\
             &timezone=Europe%2FParis&start_date={start}&end_date={last}&wind_speed_unit=kmh",
            p.lat,
            p.lon,
            variables.join(",")
        ),
    )
    .max_age(max_age)
}

/// Hourly weather of past days, from [`HISTORY_LEAD_DAYS`] before `first` to `last`, in
/// the [`HourlyFile`] format, written as `meteo_<start>_<last>.json` with a
/// `meteo-hourly.meta.json` in `dir` (the assistant reads `data/out/<site>/assist/history/`).
/// Requires `personal_sources` (Open-Meteo: non-commercial use only). Returns the file
/// path and the content.
pub fn history(
    site: &Site,
    dl: &Downloader,
    personal_sources: bool,
    first: NaiveDate,
    last: NaiveDate,
    dir: &Path,
) -> Result<(PathBuf, HourlyFile)> {
    if !personal_sources {
        return Err(Error::PersonalSourcesDisabled(SOURCE));
    }
    if last < first {
        return Err(Error::format("dates", format!("{last} before {first}")));
    }
    let pts = points(site);
    if pts.is_empty() {
        return Err(Error::format(
            "site",
            format!("no forecast point for {}", site.id),
        ));
    }
    let start = first - chrono::Duration::days(HISTORY_LEAD_DAYS);
    let today = chrono::Utc::now().date_naive();
    let mut points = Vec::new();
    for p in &pts {
        let fetch = |model: &str, vars: &[&str]| -> Result<Series> {
            Series::from_json(
                &dl.fetch(&history_request(p, model, vars, start, last, today))?
                    .bytes()?,
            )
        };
        let mf = fetch("meteofrance_seamless", MF_VARIABLES)?;
        let best = fetch("best_match", BEST_MATCH_VARIABLES)?;
        points.push(merge(p, &mf, &best)?);
    }
    let generated_at = now_rfc3339();
    let file = HourlyFile {
        site: site.id.to_owned(),
        generated_at: generated_at.clone(),
        points,
        attribution: "Weather data by Open-Meteo.com (CC BY 4.0), modèles Météo-France".into(),
    };
    DataLayout::ensure(dir)?;
    let name = format!("meteo_{start}_{last}.json");
    let path = dir.join(&name);
    write_json(&path, &file)?;
    LayerMeta {
        tickets: vec![
            "P19 (#32)".into(),
            "A1 (#88)".into(),
            "A2 (#89)".into(),
            "A3 (#90)".into(),
        ],
        generated_at,
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
            name,
            "météo horaire archivée (format HourlyFile de P19, humidité relative comprise)",
        )],
        notes: vec![
            "Premières heures de chaque run archivées par Open-Meteo, mêmes modèles et variables que la prévision P19.".into(),
            "Heures locales Europe/Paris.".into(),
        ],
        ..LayerMeta::new(LAYER, site.id)
    }
    .write(dir)?;
    Ok((path, file))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn series(vars: &[(&str, &str)], elevation: f64) -> Series {
        let mut j = format!(
            r#"{{"elevation":{elevation},"utc_offset_seconds":3600,"hourly":{{"time":["2027-01-10T08:00","2027-01-10T09:00","2027-01-10T10:00","2027-01-11T10:00"]"#
        );
        for (k, v) in vars {
            j.push_str(&format!(r#","{k}":{v}"#));
        }
        j.push_str("}}");
        Series::from_json(j.as_bytes()).unwrap()
    }

    #[test]
    fn wind_chill_reference_values() {
        // Environment Canada wind chill table: -20 °C and 30 km/h give -33 (rounded).
        assert_eq!(wind_chill(-20.0, 30.0).round(), -33.0);
        // -10 °C, 20 km/h: -18 (table).
        assert_eq!(wind_chill(-10.0, 20.0).round(), -18.0);
        assert_eq!(wind_chill(12.0, 30.0), 12.0, "above 10 °C");
        assert_eq!(wind_chill(-5.0, 3.0), -5.0, "calm");
    }

    #[test]
    fn merge_models() {
        let mf = series(
            &[
                ("temperature_2m", "[-5.0,-4.0,-3.0,0.0]"),
                ("relative_humidity_2m", "[95,90,null,60]"),
                ("apparent_temperature", "[-9.0,-8.0,-7.0,-2.0]"),
                ("wind_speed_10m", "[20.0,20.0,null,10.0]"),
                ("wind_gusts_10m", "[40.0,55.0,30.0,20.0]"),
                ("snowfall", "[0.7,0.7,0.0,1.0]"),
                ("cloud_cover", "[100,90,10,0]"),
                ("weather_code", "[71,71,45,0]"),
                ("visibility", "[null,null,null,null]"),
            ],
            1750.0,
        );
        let best = series(
            &[
                ("visibility", "[24000.0,800.0,20000.0,30000.0]"),
                ("freezing_level_height", "[1200.0,1300.0,1500.0,2000.0]"),
            ],
            1750.0,
        );
        let p = &forecast_points("cervieres")[0];
        let f = merge(p, &mf, &best).unwrap();
        assert_eq!(f.elevation_m, 1750.0);
        assert_eq!(f.hours.len(), 4);
        let h = &f.hours;
        assert_eq!(h[0].fog, Some(false));
        assert_eq!(h[0].relative_humidity_pct, Some(95.0));
        assert_eq!(h[2].relative_humidity_pct, None);
        assert_eq!(h[1].fog, Some(true), "visibility 800 m");
        assert_eq!(h[2].fog, Some(true), "WMO 45");
        assert_eq!(h[2].wind_chill_c, None, "wind missing");
        assert_eq!(
            h[0].wind_chill_c,
            Some((wind_chill(-5.0, 20.0) * 10.0).round() / 10.0)
        );
        assert_eq!(
            h[1].visibility_m,
            Some(800.0),
            "from best_match, not the null MF value"
        );
        assert_eq!(f.days.len(), 2);
        assert_eq!(f.days[0].snowfall_cm, 1.4);
        assert_eq!(
            f.days[0].daytime_cloud_cover_pct,
            Some(50.0),
            "09:00 and 10:00"
        );
        assert_eq!(f.days[0].max_gust_kmh, Some(55.0));
        assert_eq!(f.days[0].daytime_fog_hours, 2);
        assert_eq!(f.days[0].min_freezing_level_m, Some(1200.0));
        assert_eq!(f.models["visibility"], "best_match");
        assert_eq!(f.models["wind_speed_10m"], "meteofrance_seamless");
        let other = series(&[], 1750.0);
        let mut shifted = other.clone();
        shifted
            .hourly
            .insert("time".into(), serde_json::json!(["2027-01-10T07:00"]));
        assert!(merge(p, &mf, &shifted).is_err());
    }

    #[test]
    fn points_and_requests() {
        assert_eq!(forecast_points("cervieres").len(), 2);
        assert_eq!(forecast_points("chapelle-des-bois").len(), 1);
        let r = request(
            &forecast_points("chapelle-des-bois")[0],
            "best_match",
            &["visibility"],
        );
        assert!(r.url().contains("latitude=46.6032&longitude=6.1125"));
        assert!(r.url().contains("hourly=visibility&models=best_match"));
        assert!(r.url().contains("wind_speed_unit=kmh"));
        let d = |s: &str| NaiveDate::parse_from_str(s, "%Y-%m-%d").unwrap();
        let p = &forecast_points("cervieres")[0];
        let h = history_request(
            p,
            "meteofrance_seamless",
            MF_VARIABLES,
            d("2026-01-01"),
            d("2026-01-10"),
            d("2026-11-15"),
        );
        assert!(h.url().starts_with(HISTORICAL_URL));
        assert!(
            h.url()
                .contains("start_date=2026-01-01&end_date=2026-01-10")
        );
        assert!(h.url().contains("relative_humidity_2m"));
    }

    #[test]
    fn requires_personal_sources() {
        let dir = tempfile::tempdir().unwrap();
        let layout = DataLayout::new(dir.path());
        let dl = Downloader::builder(layout.cache_root())
            .offline(true)
            .build()
            .unwrap();
        let e = build(&skimap_common::site::CERVIERES, &layout, &dl, false).unwrap_err();
        assert!(matches!(e, Error::PersonalSourcesDisabled("open-meteo")));
        assert!(!layout.out_root().exists());
    }
}
