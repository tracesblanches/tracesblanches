//! Publishable hourly weather per site from the open Météo-France AROME packages (X17,
//! #151; same `meteo-hourly` layer as P19, #32).
//!
//! **Source**: « Paquets AROME - Résolution 0,025° » of Météo-France on data.gouv.fr,
//! Licence Ouverte 2.0 (Etalab), no key and no quota published. The files are served by
//! the public bucket `meteofrance-pnt.s3.rbx.io.cloud.ovh.net` (the URLs of the dataset
//! resources); its S3 listing gives the runs. Eight runs a day (00, 03 … 21 UTC), forecast
//! hours 0 to 51, GRIB2 with CCSDS packing (template 5.42, decoded by the pure Rust `grib`
//! and `rust-aec` crates), grid EURW1S40 (0.025°, 55.4N 37.5N 12W 16E), with missing
//! values outside the trapezoidal native domain (bitmap).
//!
//! Package `SP1` (9 files per run, 00-06 h … 49-51 h, about 470 MB together): T(2m),
//! HU(2m), FF(10m), DD(10m), FF_RAF(10m) (maximum over the hour), NEBUL, PRECIP, NEIGE,
//! GRAUPEL (accumulated since the start of the run). The grid elevation (`ALTITUDE`) is
//! read once per run from `SP2` 00-06 h (48 MB): the constant-fields file of the dataset
//! uses the packing 5.50002, which the decoder does not read.
//!
//! Not in the open surface packages, hence `None` in `hourly.json`: visibility and fog,
//! 0 °C isotherm (only on isobaric / height levels, 444 to 700 MB per group of hours),
//! WMO weather code, low cloud cover (`SP2`, 375 MB per run, not read by the assistant).
//! Apparent temperature is computed (Steadman 1994 formula of the Australian Bureau of
//! Meteorology, without radiation), wind chill as in P19.
//!
//! Each point takes the nearest grid point (no altitude correction): `elevation_m` is the
//! model elevation of that grid point.
//!
//! [`refresh`] is the server entry point: it finds the latest complete run, decodes each
//! file once for all the points of all the given sites and rewrites their layer only when
//! the run changed (idempotent). [`build`] does the same for one site.

use std::collections::BTreeMap;
use std::io::BufReader;
use std::path::{Path, PathBuf};
use std::time::Duration;

use chrono::{DateTime, NaiveDateTime, TimeZone, Utc};
use grib::LatLons;
use serde::Serialize;
use skimap_common::download::{CachePolicy, Downloader, Request};
use skimap_common::layout::DataLayout;
use skimap_common::meta::{FileMeta, LayerMeta, SourceRef, now_rfc3339};
use skimap_common::output::write_json;
use skimap_common::site::Site;

use crate::error::{Error, Result};
use crate::hourly::{ForecastPoint, Hour, HourlyFile, LAYER, PointForecast, days, wind_chill};

/// Downloader source (public bucket of the Météo-France numerical weather prediction).
pub const SOURCE: &str = "meteofrance-pnt";
/// Public bucket serving the files of the data.gouv.fr dataset.
pub const BUCKET_URL: &str = "https://meteofrance-pnt.s3.rbx.io.cloud.ovh.net";
/// Dataset page (licence, documentation).
pub const DATASET_URL: &str = "https://www.data.gouv.fr/datasets/paquets-arome-resolution-0-025deg";
/// Licence of the data.
pub const LICENCE: &str = "Licence Ouverte / Open Licence 2.0 (Etalab)";
/// Attribution to display.
pub const ATTRIBUTION: &str = "Météo-France, modèle AROME 0,025° (Licence Ouverte 2.0)";
/// Model name recorded for each variable in `PointForecast::models`.
pub const MODEL: &str = "meteofrance_arome_0025";
/// Groups of forecast hours of one run, one file each.
pub const GROUPS: [(u32, u32); 9] = [
    (0, 6),
    (7, 12),
    (13, 18),
    (19, 24),
    (25, 30),
    (31, 36),
    (37, 42),
    (43, 48),
    (49, 51),
];
/// Cache lifetime of a bucket listing (a new run appears every three hours).
pub const LIST_MAX_AGE: Duration = Duration::from_secs(10 * 60);
/// Runs looked at, newest first, before giving up (the bucket keeps about two days).
pub const MAX_RUNS_TRIED: usize = 8;
/// Previous runs, six hours apart, whose first hours (1 to 6) give the past of the series:
/// the assistant needs 24 hours before 06:00 of a day (snow quality, fartage), and a run
/// of 09 UTC starts at 10:00 the day before at the latest.
pub const PAST_RUNS: u32 = 5;
/// Snow depth per water equivalent: 1 mm of water gives 0.7 cm of snow (the conversion
/// Open-Meteo documents for its `snowfall`, so that the assistant reads the same unit).
pub const SNOW_CM_PER_MM: f64 = 0.7;

/// Variables read from the GRIB files.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
pub enum Var {
    /// Temperature at 2 m, K.
    T2,
    /// Relative humidity at 2 m, %.
    Rh2,
    /// Wind speed at 10 m, m/s.
    Wind,
    /// Wind direction at 10 m, degrees.
    Dir,
    /// Maximum gust at 10 m over the previous hour, m/s.
    Gust,
    /// Total cloud cover, %.
    Cloud,
    /// Total precipitation accumulated since the run start, kg/m².
    Precip,
    /// Snow accumulated since the run start, kg/m² of water.
    Snow,
    /// Graupel accumulated since the run start, kg/m² of water.
    Graupel,
    /// Elevation of the grid point, m.
    Altitude,
}

/// Identifies a GRIB2 field from discipline, parameter category and number, product
/// template, first fixed surface (type, value). Returns the variable and the forecast hour
/// it describes (end of the period for a statistical field), or `None` for a field that is
/// not used.
pub fn identify(
    discipline: u8,
    category: u8,
    number: u8,
    template: u16,
    surface: (u8, f64),
) -> Option<Var> {
    if discipline != 0 {
        return None;
    }
    let height = |h: f64| surface.0 == 103 && (surface.1 - h).abs() < 1e-6;
    let v = match (template, category, number) {
        (0, 0, 0) if height(2.0) => Var::T2,
        (0, 1, 1) if height(2.0) => Var::Rh2,
        (0, 2, 1) if height(10.0) => Var::Wind,
        (0, 2, 0) if height(10.0) => Var::Dir,
        (8, 2, 22) if height(10.0) => Var::Gust,
        (0, 6, 1) => Var::Cloud,
        (8, 1, 52) => Var::Precip,
        (8, 1, 66) => Var::Snow,
        (8, 1, 75) => Var::Graupel,
        (0, 3, 6) => Var::Altitude,
        _ => return None,
    };
    Some(v)
}

/// Hours from a GRIB time unit (code table 4.4) and value.
fn hours_of(unit: u8, value: u32) -> Option<u32> {
    match unit {
        0 => value.is_multiple_of(60).then_some(value / 60),
        1 => Some(value),
        2 => Some(value * 24),
        10 => Some(value * 3),
        11 => Some(value * 6),
        12 => Some(value * 12),
        _ => None,
    }
}

/// Forecast hour of a field: the forecast time for an instantaneous field (template 4.0),
/// its end (start + length of the time range, octets 49 to 53; forecast time: octets 18 to 22) for a statistical one
/// (template 4.8). `payload` is section 4 from octet 6.
pub fn forecast_hour(template: u16, payload: &[u8]) -> Option<u32> {
    // Octet n of section 4 is payload[n - 6].
    let unit = *payload.get(18 - 6)?;
    let value = u32::from_be_bytes(payload.get(19 - 6..23 - 6)?.try_into().ok()?);
    let start = hours_of(unit, value)?;
    match template {
        0 => Some(start),
        8 => {
            let unit = *payload.get(49 - 6)?;
            let len = u32::from_be_bytes(payload.get(50 - 6..54 - 6)?.try_into().ok()?);
            Some(start + hours_of(unit, len)?)
        }
        _ => None,
    }
}

/// Value of one variable at one forecast hour at every requested point.
#[derive(Debug, Clone, PartialEq)]
pub struct Field {
    /// Variable.
    pub var: Var,
    /// Forecast hour.
    pub hour: u32,
    /// One value per point (`None`: missing value of the grid, or point outside it).
    pub values: Vec<Option<f64>>,
}

/// Grid point nearest to each requested point, in the order of the decoded values.
#[derive(Debug, Clone, PartialEq)]
pub struct PointIndex {
    /// Index in the decoded values, `None` when the point is outside the grid.
    pub index: Vec<Option<usize>>,
    /// Latitude and longitude of the chosen grid point.
    pub grid_latlon: Vec<Option<(f64, f64)>>,
    /// Number of points of the grid.
    pub grid_size: usize,
}

impl PointIndex {
    /// Nearest grid point of each `(lat, lon)` among the grid coordinates (in value order).
    /// The grid is regular: the search walks the coordinates once.
    pub fn new(
        latlons: impl Iterator<Item = (f64, f64)>,
        points: &[(f64, f64)],
        grid_size: usize,
    ) -> Self {
        // Per point: value index, squared distance, grid coordinates.
        type Best = Option<(usize, f64, (f64, f64))>;
        let mut best: Vec<Best> = vec![None; points.len()];
        for (i, (lat, lon)) in latlons.enumerate() {
            for (k, (plat, plon)) in points.iter().enumerate() {
                let dlat = lat - plat;
                if dlat.abs() > 0.1 {
                    continue;
                }
                let dlon = (lon - plon) * plat.to_radians().cos();
                if dlon.abs() > 0.1 {
                    continue;
                }
                let d = dlat * dlat + dlon * dlon;
                if best[k].is_none_or(|(_, bd, _)| d < bd) {
                    best[k] = Some((i, d, (lat, lon)));
                }
            }
        }
        Self {
            index: best.iter().map(|b| b.map(|(i, _, _)| i)).collect(),
            grid_latlon: best.iter().map(|b| b.map(|(_, _, ll)| ll)).collect(),
            grid_size,
        }
    }
}

/// Reads every used field of a GRIB2 file at the given points. The nearest grid points
/// are `index` when given (computed on another file of the same grid; the grid size is
/// checked), else computed from the first used field (all fields of a package share the
/// grid).
pub fn extract(
    path: &Path,
    points: &[(f64, f64)],
    index: Option<&PointIndex>,
) -> Result<(Vec<Field>, PointIndex)> {
    let ctx = |e: &dyn std::fmt::Display| Error::format(path.display().to_string(), e.to_string());
    let f = std::fs::File::open(path)
        .map_err(|e| skimap_common::Error::io(path, e))
        .map_err(Error::from)?;
    let grib2 = grib::from_reader(BufReader::new(f)).map_err(|e| ctx(&e))?;
    let mut fields = Vec::new();
    let mut index: Option<PointIndex> = index.cloned();
    for (_, sub) in grib2.iter() {
        let pd = sub.prod_def();
        let template = pd.prod_tmpl_num();
        let (Some(cat), Some(num), Some((s1, _))) = (
            pd.parameter_category(),
            pd.parameter_number(),
            pd.fixed_surfaces(),
        ) else {
            continue;
        };
        let Some(var) = identify(
            sub.indicator().discipline,
            cat,
            num,
            template,
            (s1.surface_type, s1.value()),
        ) else {
            continue;
        };
        let payload: Vec<u8> = pd.iter().copied().collect();
        let hour = forecast_hour(template, &payload)
            .ok_or_else(|| ctx(&format!("unreadable forecast time of {var:?}")))?;
        let size = sub.grid_def().num_points() as usize;
        if index.is_none() {
            index = Some(PointIndex::new(
                sub.latlons().map_err(|e| ctx(&e))?,
                points,
                size,
            ));
        }
        let idx = index.as_ref().expect("set above");
        if idx.grid_size != size {
            return Err(ctx(&format!(
                "grid of {size} points, expected {}",
                idx.grid_size
            )));
        }
        let decoder = grib::Grib2SubmessageDecoder::from(sub).map_err(|e| ctx(&e))?;
        let all: Vec<f32> = decoder.dispatch().map_err(|e| ctx(&e))?.collect();
        if all.len() != size {
            return Err(ctx(&format!("{} values for {size} points", all.len())));
        }
        let values = idx
            .index
            .iter()
            .map(|i| {
                i.and_then(|i| all.get(i).copied())
                    .filter(|v| v.is_finite())
                    .map(f64::from)
            })
            .collect();
        fields.push(Field { var, hour, values });
    }
    let index = index.ok_or_else(|| ctx(&"no used field in the file"))?;
    Ok((fields, index))
}

/// Run identifier as in the bucket paths, `2026-11-15T09:00:00Z`.
pub fn run_id(run: DateTime<Utc>) -> String {
    run.format("%Y-%m-%dT%H:%M:%SZ").to_string()
}

/// URL of one file of a run: package (`SP1`, `SP2`) and group of hours.
pub fn file_url(run: DateTime<Utc>, package: &str, (a, b): (u32, u32)) -> String {
    let id = run_id(run);
    format!(
        "{BUCKET_URL}/pnt/{id}/arome/0025/{package}/arome__0025__{package}__{a:02}H{b:02}H__{id}.grib2"
    )
}

/// Request of a bucket listing (S3 ListObjectsV2).
pub fn list_request(prefix: &str, delimiter: bool) -> Request {
    let d = if delimiter { "&delimiter=/" } else { "" };
    Request::get(
        SOURCE,
        &format!("{BUCKET_URL}/?list-type=2&prefix={prefix}{d}"),
    )
    .max_age(LIST_MAX_AGE)
}

/// Texts of the `<tag>` elements of an S3 listing (keys or common prefixes).
pub fn xml_values(xml: &str, tag: &str) -> Vec<String> {
    let open = format!("<{tag}>");
    let close = format!("</{tag}>");
    let mut out = Vec::new();
    let mut rest = xml;
    while let Some(i) = rest.find(&open) {
        rest = &rest[i + open.len()..];
        let Some(j) = rest.find(&close) else { break };
        out.push(rest[..j].to_owned());
        rest = &rest[j + close.len()..];
    }
    out
}

/// Runs of the bucket, newest first, from the `pnt/<run>/` prefixes of a listing.
pub fn runs_in_listing(xml: &str) -> Vec<DateTime<Utc>> {
    let mut runs: Vec<DateTime<Utc>> = xml_values(xml, "Prefix")
        .iter()
        .filter_map(|p| {
            let id = p.strip_prefix("pnt/")?.strip_suffix('/')?;
            let t = NaiveDateTime::parse_from_str(id, "%Y-%m-%dT%H:%M:%SZ").ok()?;
            Some(Utc.from_utc_datetime(&t))
        })
        .collect();
    runs.sort_unstable_by(|a, b| b.cmp(a));
    runs
}

/// Whether a listing of `pnt/<run>/arome/0025/` holds every file needed from the run.
pub fn run_complete(xml: &str, run: DateTime<Utc>) -> bool {
    let keys = xml_values(xml, "Key");
    let has = |url: String| {
        let key = url.trim_start_matches(BUCKET_URL).trim_start_matches('/');
        keys.iter().any(|k| k == key)
    };
    GROUPS.iter().all(|g| has(file_url(run, "SP1", *g))) && has(file_url(run, "SP2", GROUPS[0]))
}

/// Latest run whose files are all published.
pub fn latest_run(dl: &Downloader) -> Result<DateTime<Utc>> {
    let listing = dl.fetch(&list_request("pnt/", true))?.text()?;
    let runs = runs_in_listing(&listing);
    if runs.is_empty() {
        return Err(Error::format(SOURCE, "no run in the bucket listing"));
    }
    for run in runs.into_iter().take(MAX_RUNS_TRIED) {
        let prefix = format!("pnt/{}/arome/0025/", run_id(run));
        let xml = dl.fetch(&list_request(&prefix, false))?.text()?;
        if run_complete(&xml, run) {
            return Ok(run);
        }
    }
    Err(Error::format(
        SOURCE,
        format!("no complete AROME 0.025° run among the last {MAX_RUNS_TRIED}"),
    ))
}

/// Series of one point: value of each variable at each forecast hour.
pub type PointSeries = BTreeMap<(Var, u32), f64>;

fn round1(x: f64) -> f64 {
    (x * 10.0).round() / 10.0
}

fn round2(x: f64) -> f64 {
    (x * 100.0).round() / 100.0
}

/// Apparent temperature (Steadman 1994, Australian Bureau of Meteorology): air temperature
/// °C, relative humidity %, wind at 10 m in m/s.
pub fn apparent_temperature(t_c: f64, rh_pct: f64, wind_ms: f64) -> f64 {
    let e = rh_pct / 100.0 * 6.105 * (17.27 * t_c / (237.7 + t_c)).exp();
    t_c + 0.33 * e - 0.70 * wind_ms - 4.00
}

/// Hours of one point from its series: local time (Europe/Paris), units of `hourly.json`.
/// Hourly precipitation and snow are the differences of the accumulations; the hour 0 of
/// the run (analysis) has neither those nor a gust.
pub fn hours(run: DateTime<Utc>, s: &PointSeries) -> Vec<Hour> {
    let last = s.keys().map(|(_, h)| *h).max().unwrap_or(0);
    let get = |v: Var, h: u32| s.get(&(v, h)).copied();
    let cum = |v: Var, h: u32| {
        if h == 0 { Some(0.0) } else { get(v, h) }
    };
    let step = |v: Var, h: u32| -> Option<f64> {
        if h == 0 {
            return None;
        }
        Some((cum(v, h)? - cum(v, h - 1)?).max(0.0))
    };
    (0..=last)
        .filter(|h| get(Var::T2, *h).is_some())
        .map(|h| {
            let at = run + chrono::Duration::hours(i64::from(h));
            let t = get(Var::T2, h).map(|k| round1(k - 273.15));
            let rh = get(Var::Rh2, h).map(|x| x.round().clamp(0.0, 100.0));
            let w_ms = get(Var::Wind, h);
            let w = w_ms.map(|x| round1(x * 3.6));
            let snow = match (step(Var::Snow, h), step(Var::Graupel, h)) {
                (Some(s), g) => Some(round2((s + g.unwrap_or(0.0)) * SNOW_CM_PER_MM)),
                (None, _) => None,
            };
            Hour {
                time: skimap_core::ephemeris::local_time(at)
                    .format("%Y-%m-%dT%H:%M")
                    .to_string(),
                temperature_c: t,
                relative_humidity_pct: rh,
                apparent_temperature_c: match (t, rh, w_ms) {
                    (Some(t), Some(rh), Some(w)) => Some(round1(apparent_temperature(t, rh, w))),
                    _ => None,
                },
                wind_chill_c: match (t, w) {
                    (Some(t), Some(w)) => Some(round1(wind_chill(t, w))),
                    _ => None,
                },
                wind_kmh: w,
                gust_kmh: get(Var::Gust, h).filter(|_| h > 0).map(|x| round1(x * 3.6)),
                wind_direction_deg: get(Var::Dir, h).map(f64::round),
                visibility_m: None,
                fog: None,
                freezing_level_m: None,
                snowfall_cm: snow,
                precipitation_mm: step(Var::Precip, h).map(round1),
                cloud_cover_pct: get(Var::Cloud, h).map(|x| x.round().clamp(0.0, 100.0)),
                cloud_cover_low_pct: None,
                weather_code: None,
            }
        })
        .collect()
}

/// Model of each variable of `hourly.json` (absent variables are not listed).
pub fn models(run: DateTime<Utc>) -> BTreeMap<String, String> {
    let m = format!("{MODEL} (run {})", run_id(run));
    let mut out = BTreeMap::new();
    for v in [
        "temperature_2m",
        "relative_humidity_2m",
        "wind_speed_10m",
        "wind_gusts_10m",
        "wind_direction_10m",
        "snowfall",
        "precipitation",
        "cloud_cover",
    ] {
        out.insert(v.to_owned(), m.clone());
    }
    out.insert(
        "apparent_temperature".to_owned(),
        "computed (Steadman 1994, BoM)".to_owned(),
    );
    out.insert("wind_chill".to_owned(), "computed (JAG-TI)".to_owned());
    out
}

/// Decoded run: series and grid elevation of each requested point.
#[derive(Debug, Clone, PartialEq)]
pub struct DecodedRun {
    /// Run (UTC).
    pub run: DateTime<Utc>,
    /// Series of each point.
    pub series: Vec<PointSeries>,
    /// Series of each point in each previous run read (00-06 h only), newest first.
    pub past: Vec<(DateTime<Utc>, Vec<PointSeries>)>,
    /// Elevation of the nearest grid point, m.
    pub elevation_m: Vec<Option<f64>>,
    /// Coordinates of the nearest grid point.
    pub grid_latlon: Vec<Option<(f64, f64)>>,
}

/// Downloads (through the cache) and decodes the files of a run at the given points: the
/// nine `SP1` groups, the elevation of `SP2` 00-06 h and the `SP1` 00-06 h file of the
/// [`PAST_RUNS`] previous runs that are still published, one thread per file.
pub fn decode_run(
    dl: &Downloader,
    run: DateTime<Utc>,
    points: &[(f64, f64)],
) -> Result<DecodedRun> {
    // (run, URL): the current run first, then the past runs.
    let mut files: Vec<(DateTime<Utc>, String)> = GROUPS
        .iter()
        .map(|g| (run, file_url(run, "SP1", *g)))
        .collect();
    files.push((run, file_url(run, "SP2", GROUPS[0])));
    for k in 1..=PAST_RUNS {
        let r = run - chrono::Duration::hours(6 * i64::from(k));
        files.push((r, file_url(r, "SP1", GROUPS[0])));
    }
    // Files of a run never change once published: the cache is enough. A past run already
    // removed from the bucket only shortens the past.
    let mut paths: Vec<(DateTime<Utc>, PathBuf)> = Vec::new();
    for (r, u) in &files {
        match dl.fetch(&Request::get(SOURCE, u).cache(CachePolicy::Normal)) {
            Ok(f) => paths.push((*r, f.path)),
            Err(e) if *r != run => {
                tracing::warn!(url = %u, error = %e, "past AROME run unavailable");
            }
            Err(e) => return Err(e.into()),
        }
    }
    // The nearest grid points are searched once, on the first file.
    let (first, index) = extract(&paths[0].1, points, None)?;
    let rest: Vec<Result<(Vec<Field>, PointIndex)>> = std::thread::scope(|s| {
        let handles: Vec<_> = paths[1..]
            .iter()
            .map(|(_, p)| {
                let index = &index;
                s.spawn(move || extract(p, points, Some(index)))
            })
            .collect();
        handles
            .into_iter()
            .map(|h| h.join().expect("decoder thread"))
            .collect()
    });
    let mut by_run: BTreeMap<DateTime<Utc>, Vec<PointSeries>> = BTreeMap::new();
    let mut elevation_m = vec![None; points.len()];
    let grid_latlon = index.grid_latlon.clone();
    let mut all = vec![(run, first)];
    for ((r, _), d) in paths[1..].iter().zip(rest) {
        all.push((*r, d?.0));
    }
    for (r, fields) in all {
        let series = by_run
            .entry(r)
            .or_insert_with(|| vec![PointSeries::new(); points.len()]);
        for f in fields {
            for (i, v) in f.values.iter().enumerate() {
                let Some(v) = v else { continue };
                if f.var == Var::Altitude {
                    elevation_m[i] = Some(*v);
                } else {
                    series[i].insert((f.var, f.hour), *v);
                }
            }
        }
    }
    let series = by_run.remove(&run).unwrap_or_default();
    let past = by_run.into_iter().rev().collect();
    Ok(DecodedRun {
        run,
        series,
        past,
        elevation_m,
        grid_latlon,
    })
}

/// Fills a series of hours with older ones: hours before its first hour are prepended,
/// and an hour present in both keeps its own values, missing ones taken from the older
/// hour (the analysis hour 0 of a run has no precipitation, gust nor cloud cover; the
/// previous run has them at its hour 6).
pub fn fill_with_older(newer: Vec<Hour>, older: &[Hour]) -> Vec<Hour> {
    let Some(start) = newer.first().map(|h| h.time.clone()) else {
        return older.to_vec();
    };
    let mut out: Vec<Hour> = older.iter().filter(|h| h.time < start).cloned().collect();
    for mut h in newer {
        if let Some(o) = older.iter().find(|o| o.time == h.time) {
            macro_rules! fill {
                ($($f:ident),*) => { $( if h.$f.is_none() { h.$f = o.$f.clone(); } )* };
            }
            fill!(
                temperature_c,
                relative_humidity_pct,
                apparent_temperature_c,
                wind_chill_c,
                wind_kmh,
                gust_kmh,
                wind_direction_deg,
                snowfall_cm,
                precipitation_mm,
                cloud_cover_pct
            );
        }
        out.push(h);
    }
    out
}

/// Hours of point `i` of a decoded run: the run, then its past runs, newest first.
pub fn point_hours(run: &DecodedRun, i: usize) -> Vec<Hour> {
    let mut hs = hours(run.run, &run.series[i]);
    for (r, series) in &run.past {
        // Hours 1 to 6 of a past run: its analysis hour has no accumulations.
        let mut older = hours(*r, &series[i]);
        older.retain(|h| h.precipitation_mm.is_some());
        hs = fill_with_older(hs, &older);
    }
    hs
}

/// Run recorded in the `meteo-hourly` metadata of a site, if it came from this source.
pub fn recorded_run(layout: &DataLayout, site: &Site) -> Option<String> {
    let dir = layout.layer_dir(site, LAYER).ok()?;
    let m = LayerMeta::read(&dir.join(LayerMeta::file_name(LAYER))).ok()?;
    m.parameters
        .get("arome_run")
        .and_then(|v| v.as_str())
        .map(str::to_owned)
}

/// What [`refresh`] did for one site.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SiteRefresh {
    /// Site identifier.
    pub site: String,
    /// `written`, `unchanged` when the layer already holds the run, or `failed` when the
    /// weather of the site cannot be made from the run ([`SiteRefresh::error`]; its
    /// previous layer, if any, is kept).
    pub status: &'static str,
    /// Points written.
    pub points: usize,
    /// Hours per point.
    pub hours: usize,
    /// Why the site failed.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

impl SiteRefresh {
    fn failed(site: &Site, error: String) -> Self {
        Self {
            site: site.id.to_owned(),
            status: "failed",
            points: 0,
            hours: 0,
            error: Some(error),
        }
    }
}

/// Report of [`refresh`].
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct RefreshReport {
    /// Run used.
    pub run: String,
    /// One entry per site.
    pub sites: Vec<SiteRefresh>,
}

/// Writes the `meteo-hourly` layer of a site from a decoded run (`offset`: index of the
/// site's first point in the decoded points).
fn write_site(
    site: &Site,
    layout: &DataLayout,
    pts: &[ForecastPoint],
    run: &DecodedRun,
    offset: usize,
) -> Result<HourlyFile> {
    let mut points = Vec::new();
    for (k, p) in pts.iter().enumerate() {
        let i = offset + k;
        let hs = point_hours(run, i);
        if hs.is_empty() {
            return Err(Error::format(
                SOURCE,
                format!(
                    "no value at {} ({}, {}): outside the AROME grid",
                    p.id, p.lat, p.lon
                ),
            ));
        }
        points.push(PointForecast {
            id: p.id.to_owned(),
            name: p.name.to_owned(),
            lat: p.lat,
            lon: p.lon,
            elevation_m: run.elevation_m[i].map_or(f64::NAN, f64::round),
            models: models(run.run),
            days: days(&hs),
            hours: hs,
        });
    }
    if points.iter().any(|p| p.elevation_m.is_nan()) {
        return Err(Error::format(SOURCE, "grid elevation missing (SP2)"));
    }
    let generated_at = now_rfc3339();
    let file = HourlyFile {
        site: site.id.to_owned(),
        generated_at: generated_at.clone(),
        points,
        attribution: ATTRIBUTION.to_owned(),
    };
    let dir = layout.layer_dir(site, LAYER)?;
    DataLayout::ensure(&dir)?;
    write_json(&dir.join("hourly.json"), &file)?;
    let first = file.points.first();
    let grid: Vec<serde_json::Value> = pts
        .iter()
        .enumerate()
        .map(|(k, p)| {
            let i = offset + k;
            serde_json::json!({
                "point": p.id,
                "grid_lat": run.grid_latlon[i].map(|x| x.0),
                "grid_lon": run.grid_latlon[i].map(|x| x.1),
                "grid_elevation_m": run.elevation_m[i],
            })
        })
        .collect();
    LayerMeta {
        tickets: vec!["P19 (#32)".to_owned(), "X17 (#151)".to_owned()],
        generated_at,
        valid_from: first.and_then(|p| p.hours.first()).map(|h| h.time.clone()),
        valid_until: first.and_then(|p| p.hours.last()).map(|h| h.time.clone()),
        sources: vec![
            SourceRef::new(
                "Météo-France, paquets AROME 0,025° (SP1, altitude de SP2)",
                DATASET_URL,
                LICENCE,
                ATTRIBUTION,
            )
            .producer("Météo-France")
            .edition(format!("run {}", run_id(run.run)))
            .note(format!(
                "fichiers {BUCKET_URL}/pnt/{}/arome/0025/",
                run_id(run.run)
            )),
        ],
        files: vec![FileMeta::new(
            "hourly.json",
            "prévision horaire par point (HourlyFile), heures locales Europe/Paris",
        )],
        notes: vec![
            "Source publiable (Licence Ouverte 2.0), sans clé.".into(),
            "Point de grille le plus proche (0,025°), sans correction d'altitude : \
             elevation_m est l'altitude du modèle."
                .into(),
            "Absents des paquets de surface ouverts : visibilité, brouillard, isotherme 0 °C, \
             code temps OMM, nébulosité basse (valeurs nulles)."
                .into(),
            "Neige en cm = (NEIGE + GRAUPEL) en mm d'eau × 0,7 ; précipitations et neige \
             horaires = différences des cumuls depuis le début du run."
                .into(),
            "Heures passées (historique de 24 h demandé par l'assistant) : heures 1 à 6 des \
             runs précédents (past_runs), toutes les 6 h."
                .into(),
            "Température apparente calculée (Steadman 1994, BoM).".into(),
        ],
        parameters: serde_json::json!({
            "arome_run": run_id(run.run),
            "past_runs": run.past.iter().map(|(r, _)| run_id(*r)).collect::<Vec<_>>(),
            "grid": "EURW1S40 0.025°",
            "points": grid,
        }),
        ..LayerMeta::new(LAYER, site.id)
    }
    .write(&dir)?;
    Ok(file)
}

/// Brings the `meteo-hourly` layer of each site to the latest complete AROME run:
/// downloads it once (cache), decodes each file once for every point of every site, and
/// rewrites a site only when its layer does not hold that run yet. Idempotent: a second
/// call within the same run writes nothing. Publishable (no `personal_sources` needed).
/// A site whose weather cannot be made (no forecast point, a point outside the grid)
/// is reported `failed` with the reason and the others are written (#174); an error of
/// the run itself (listing, download, decoding) fails the whole call.
pub fn refresh(sites: &[&Site], layout: &DataLayout, dl: &Downloader) -> Result<RefreshReport> {
    let run = latest_run(dl)?;
    refresh_run(sites, layout, dl, run)
}

/// [`refresh`] with a given run.
pub fn refresh_run(
    sites: &[&Site],
    layout: &DataLayout,
    dl: &Downloader,
    run: DateTime<Utc>,
) -> Result<RefreshReport> {
    let id = run_id(run);
    let mut todo: Vec<(&Site, Vec<ForecastPoint>)> = Vec::new();
    let mut report = RefreshReport {
        run: id.clone(),
        sites: Vec::new(),
    };
    for site in sites {
        if recorded_run(layout, site).as_deref() == Some(id.as_str()) {
            report.sites.push(SiteRefresh {
                site: site.id.to_owned(),
                status: "unchanged",
                points: 0,
                hours: 0,
                error: None,
            });
            continue;
        }
        let pts = crate::hourly::points(site);
        if pts.is_empty() {
            report.sites.push(SiteRefresh::failed(
                site,
                format!("no forecast point for {}", site.id),
            ));
            continue;
        }
        todo.push((site, pts));
    }
    if todo.is_empty() {
        return Ok(report);
    }
    let all: Vec<(f64, f64)> = todo
        .iter()
        .flat_map(|(_, pts)| pts.iter().map(|p| (p.lat, p.lon)))
        .collect();
    let decoded = decode_run(dl, run, &all)?;
    report.sites.extend(write_decoded(&todo, layout, &decoded));
    Ok(report)
}

/// Writes the layer of each site of `todo` from `decoded` (its points in the order of
/// `todo`): a site that cannot be written is `failed`, the others are written.
fn write_decoded(
    todo: &[(&Site, Vec<ForecastPoint>)],
    layout: &DataLayout,
    decoded: &DecodedRun,
) -> Vec<SiteRefresh> {
    let mut out = Vec::new();
    let mut offset = 0;
    for (site, pts) in todo {
        let written = write_site(site, layout, pts, decoded, offset);
        offset += pts.len();
        out.push(match written {
            Ok(f) => SiteRefresh {
                site: site.id.to_owned(),
                status: "written",
                points: f.points.len(),
                hours: f.points.first().map_or(0, |p| p.hours.len()),
                error: None,
            },
            Err(e) => SiteRefresh::failed(site, e.to_string()),
        });
    }
    out
}

/// Publishable hourly weather of one site ([`refresh`] for one site); returns the layer.
pub fn build(site: &Site, layout: &DataLayout, dl: &Downloader) -> Result<HourlyFile> {
    build_run(site, layout, dl, None)
}

/// [`build`] at a given run (`None`: the latest complete one).
pub fn build_run(
    site: &Site,
    layout: &DataLayout,
    dl: &Downloader,
    run: Option<DateTime<Utc>>,
) -> Result<HourlyFile> {
    let report = match run {
        Some(run) => refresh_run(&[site], layout, dl, run)?,
        None => refresh(&[site], layout, dl)?,
    };
    if let Some(e) = report.sites.iter().find_map(|s| s.error.as_deref()) {
        return Err(Error::Site(e.to_owned()));
    }
    let path = layout.layer_dir(site, LAYER)?.join("hourly.json");
    let bytes = std::fs::read(&path).map_err(|e| skimap_common::Error::io(&path, e))?;
    serde_json::from_slice(&bytes)
        .map_err(|e| Error::format(path.display().to_string(), e.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run() -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 11, 15, 9, 0, 0).unwrap()
    }

    /// #174: a site whose weather cannot be made from the run (a point outside the grid)
    /// fails alone; the sites after it get their layer.
    #[test]
    fn a_site_whose_weather_cannot_be_made_fails_alone() {
        let t = tempfile::tempdir().unwrap();
        let layout = DataLayout::new(t.path());
        let a = skimap_common::site::site("cervieres").unwrap();
        let b = skimap_common::site::site("chapelle-des-bois").unwrap();
        let todo = vec![(a, crate::hourly::points(a)), (b, crate::hourly::points(b))];
        let n = todo.iter().map(|(_, p)| p.len()).sum::<usize>();
        let na = todo[0].1.len();
        let mut series = vec![PointSeries::new(); n];
        for s in &mut series[na..] {
            s.insert((Var::T2, 0), 270.0);
            s.insert((Var::T2, 1), 271.0);
        }
        let decoded = DecodedRun {
            run: run(),
            series,
            past: Vec::new(),
            elevation_m: (0..n).map(|i| (i >= na).then_some(1500.0)).collect(),
            grid_latlon: vec![None; n],
        };
        let r = write_decoded(&todo, &layout, &decoded);
        let status: Vec<(&str, &str)> = r.iter().map(|s| (s.site.as_str(), s.status)).collect();
        assert_eq!(status, [(a.id, "failed"), (b.id, "written")]);
        assert!(
            r[0].error
                .as_deref()
                .is_some_and(|e| e.contains("outside the AROME grid")),
            "{:?}",
            r[0].error
        );
        assert!(
            layout
                .layer_dir(b, LAYER)
                .unwrap()
                .join("hourly.json")
                .is_file()
        );
        assert_eq!(recorded_run(&layout, a), None);
    }

    #[test]
    fn identifies_fields() {
        assert_eq!(identify(0, 0, 0, 0, (103, 2.0)), Some(Var::T2));
        assert_eq!(identify(0, 0, 0, 0, (1, 0.0)), None, "skin temperature");
        assert_eq!(identify(0, 2, 22, 8, (103, 10.0)), Some(Var::Gust));
        assert_eq!(identify(0, 2, 23, 8, (103, 10.0)), None);
        assert_eq!(identify(0, 1, 52, 8, (1, 0.0)), Some(Var::Precip));
        assert_eq!(identify(2, 0, 0, 0, (1, 0.0)), None, "land-sea mask");
    }

    #[test]
    fn forecast_hours() {
        // Section 4 payload from octet 6: template 4.8, start 0 h, range 5 h.
        let mut p = vec![0u8; 60];
        p[18 - 6] = 1;
        p[49 - 6] = 1;
        p[50 - 6..54 - 6].copy_from_slice(&5u32.to_be_bytes());
        assert_eq!(forecast_hour(8, &p), Some(5));
        p[19 - 6..23 - 6].copy_from_slice(&3u32.to_be_bytes());
        assert_eq!(forecast_hour(0, &p), Some(3));
        p[18 - 6] = 0;
        p[19 - 6..23 - 6].copy_from_slice(&120u32.to_be_bytes());
        assert_eq!(forecast_hour(0, &p), Some(2), "minutes");
    }

    #[test]
    fn listing() {
        let xml = r#"<ListBucketResult><CommonPrefixes><Prefix>pnt/2026-11-15T06:00:00Z/</Prefix></CommonPrefixes><CommonPrefixes><Prefix>pnt/2026-11-15T09:00:00Z/</Prefix></CommonPrefixes></ListBucketResult>"#;
        assert_eq!(
            runs_in_listing(xml),
            vec![run(), run() - chrono::Duration::hours(3)]
        );
        let mut keys: String = GROUPS
            .iter()
            .map(|g| {
                format!(
                    "<Contents><Key>{}</Key></Contents>",
                    file_url(run(), "SP1", *g).trim_start_matches(&format!("{BUCKET_URL}/"))
                )
            })
            .collect();
        assert!(!run_complete(&keys, run()), "SP2 missing");
        keys.push_str(&format!(
            "<Key>pnt/{0}/arome/0025/SP2/arome__0025__SP2__00H06H__{0}.grib2</Key>",
            run_id(run())
        ));
        assert!(run_complete(&keys, run()));
        assert_eq!(
            file_url(run(), "SP1", (49, 51)),
            "https://meteofrance-pnt.s3.rbx.io.cloud.ovh.net/pnt/2026-11-15T09:00:00Z/arome/0025/SP1/arome__0025__SP1__49H51H__2026-11-15T09:00:00Z.grib2"
        );
    }

    #[test]
    fn nearest_grid_point() {
        let grid: Vec<(f64, f64)> = (0..4)
            .flat_map(|j| {
                (0..5).map(move |i| (45.0 - 0.025 * f64::from(j), 6.0 + 0.025 * f64::from(i)))
            })
            .collect();
        let idx = PointIndex::new(grid.iter().copied(), &[(44.96, 6.051), (50.0, 6.0)], 20);
        assert_eq!(idx.index, vec![Some(2 * 5 + 2), None]);
        assert_eq!(idx.grid_latlon[0], Some((44.95, 6.05)));
    }

    #[test]
    fn hours_from_series() {
        let mut s = PointSeries::new();
        for h in 0..=2 {
            s.insert((Var::T2, h), 268.15 + f64::from(h));
            s.insert((Var::Rh2, h), 90.0);
            s.insert((Var::Wind, h), 5.0);
            s.insert((Var::Dir, h), 270.4);
            s.insert((Var::Cloud, h), 80.0);
        }
        for (h, p, sn) in [(1, 1.0, 1.0), (2, 3.0, 2.5)] {
            s.insert((Var::Precip, h), p);
            s.insert((Var::Snow, h), sn);
            s.insert((Var::Graupel, h), 0.0);
            s.insert((Var::Gust, h), 10.0);
        }
        let hs = hours(run(), &s);
        assert_eq!(hs.len(), 3);
        // 09:00 UTC in November is 10:00 in Paris.
        assert_eq!(hs[0].time, "2026-11-15T10:00");
        assert_eq!(hs[0].temperature_c, Some(-5.0));
        assert_eq!(hs[0].precipitation_mm, None, "analysis hour");
        assert_eq!(hs[0].gust_kmh, None);
        assert_eq!(hs[1].precipitation_mm, Some(1.0));
        assert_eq!(hs[2].precipitation_mm, Some(2.0));
        assert_eq!(hs[2].snowfall_cm, Some(1.05), "1.5 mm water × 0.7");
        assert_eq!(hs[1].gust_kmh, Some(36.0));
        assert_eq!(hs[1].wind_kmh, Some(18.0));
        assert_eq!(hs[1].wind_direction_deg, Some(270.0));
        assert_eq!(hs[1].wind_chill_c, Some(round1(wind_chill(-4.0, 18.0))));
        assert_eq!(hs[0].fog, None);
        let d = days(&hs);
        assert_eq!(d.len(), 1);
        assert_eq!(d[0].snowfall_cm, 1.8);
    }

    #[test]
    fn past_runs_fill_the_series() {
        let h = |time: &str, t: Option<f64>, p: Option<f64>| Hour {
            time: time.into(),
            temperature_c: t,
            relative_humidity_pct: None,
            apparent_temperature_c: None,
            wind_chill_c: None,
            wind_kmh: None,
            gust_kmh: None,
            wind_direction_deg: None,
            visibility_m: None,
            fog: None,
            freezing_level_m: None,
            snowfall_cm: None,
            precipitation_mm: p,
            cloud_cover_pct: None,
            cloud_cover_low_pct: None,
            weather_code: None,
        };
        let newer = vec![
            h("2026-11-15T10:00", Some(1.0), None),
            h("2026-11-15T11:00", Some(2.0), Some(0.5)),
        ];
        let older = vec![
            h("2026-11-15T09:00", Some(0.0), Some(0.1)),
            h("2026-11-15T10:00", Some(9.0), Some(0.2)),
            h("2026-11-15T11:00", Some(9.0), Some(9.0)),
        ];
        let m = fill_with_older(newer, &older);
        assert_eq!(m.len(), 3);
        assert_eq!(m[0].time, "2026-11-15T09:00");
        assert_eq!(m[1].temperature_c, Some(1.0), "newer value kept");
        assert_eq!(m[1].precipitation_mm, Some(0.2), "missing value filled");
        assert_eq!(m[2].precipitation_mm, Some(0.5));
    }

    #[test]
    fn apparent_temperature_reference() {
        // BoM formula: 20 °C, 50 %, 2 m/s: e = 11.66 hPa, 20 + 3.85 - 1.4 - 4 = 18.4 °C.
        assert_eq!(round1(apparent_temperature(20.0, 50.0, 2.0)), 18.4);
    }
}
