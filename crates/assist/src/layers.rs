//! Typed readers of the layer outputs (`data/out/<site>/<layer>/`) used by the assistant.
//!
//! The assistant runs on the phone: it never downloads, it reads the files the pipeline
//! wrote. Each reader mirrors the documented format of its layer (crate READMEs and
//! `<layer>.meta.json`) and keeps only the fields the assistant uses; unknown fields are
//! ignored so that a layer can grow without breaking the phone.
//!
//! | Layer | File | Reader |
//! |---|---|---|
//! | `meteo-hourly` (P19) | `hourly.json` | [`Weather::load`] (several files merged: history then forecast) |
//! | `bera` (P8) | `bera.json` | [`Bera::load`] |
//! | `status` (P13) | `status.json`, `status.geojson` | [`Status::load`] |
//! | `domaines` (P3) | `domaines.geojson`, `domaines_ways.json` | [`load_domains`] |
//! | `pistes` (P2) | `pistes.geojson` | [`load_pistes`] |
//! | `snow` (P6) | `snow_depth*.tif`, `snow.meta.json` | [`SnowMap::load`] |
//! | `avalanche-historic` (P20) | `emprises.geojson`, `summary.json` | [`Clpa::load`] |
//! | `coverage` (P12) | `no_network.geojson` | [`Areas::load`] |
//! | `zones` (P9, P10) | `zones.geojson`, `rules.json` | [`load_zones`] |
//! | a planned route | any GeoJSON with a LineString | [`Route::load`] |

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use chrono::{DateTime, NaiveDate, NaiveDateTime, Utc};
use geo::{BoundingRect, Contains, Coord, LineString, MultiPolygon, Point, Polygon, Rect};
use serde::Deserialize;
use serde_json::Value;
use skimap_common::geom::LonLat;
use skimap_common::grid::Grid;
use skimap_common::meta::{LayerMeta, SourceRef};

use crate::error::{Error, Result};

/// Reads and parses a JSON file.
pub fn read_json<T: serde::de::DeserializeOwned>(path: &Path) -> Result<T> {
    let bytes = std::fs::read(path).map_err(|source| Error::Io {
        path: path.to_owned(),
        source,
    })?;
    serde_json::from_slice(&bytes).map_err(|e| Error::format(path, e))
}

/// Sources of a layer, read from its `<layer>.meta.json` (empty when absent).
pub fn layer_sources(dir: &Path, layer: &str) -> Vec<SourceRef> {
    LayerMeta::read(&dir.join(LayerMeta::file_name(layer)))
        .map(|m| m.sources)
        .unwrap_or_default()
}

// ---------------------------------------------------------------------------------------
// Weather (meteo-hourly, P19)

#[derive(Debug, Deserialize)]
struct RawHourlyFile {
    site: String,
    generated_at: String,
    points: Vec<RawPoint>,
}

#[derive(Debug, Deserialize)]
struct RawPoint {
    id: String,
    name: String,
    lat: f64,
    lon: f64,
    elevation_m: f64,
    hours: Vec<RawHour>,
}

#[derive(Debug, Deserialize)]
struct RawHour {
    time: String,
    temperature_c: Option<f64>,
    apparent_temperature_c: Option<f64>,
    wind_chill_c: Option<f64>,
    wind_kmh: Option<f64>,
    gust_kmh: Option<f64>,
    wind_direction_deg: Option<f64>,
    visibility_m: Option<f64>,
    fog: Option<bool>,
    freezing_level_m: Option<f64>,
    snowfall_cm: Option<f64>,
    precipitation_mm: Option<f64>,
    cloud_cover_pct: Option<f64>,
    cloud_cover_low_pct: Option<f64>,
    weather_code: Option<i32>,
    /// Not in the P19 layer yet (requested); present in the history files of the
    /// `history` example.
    #[serde(default)]
    relative_humidity_pct: Option<f64>,
}

/// One hour of weather at a forecast point (local legal time, Europe/Paris).
#[derive(Debug, Clone, PartialEq)]
pub struct WeatherHour {
    /// Local time at the start of the hour.
    pub time: NaiveDateTime,
    /// Air temperature at 2 m, °C.
    pub temperature_c: Option<f64>,
    /// Apparent temperature (Open-Meteo), °C.
    pub apparent_temperature_c: Option<f64>,
    /// Wind chill (JAG-TI), °C.
    pub wind_chill_c: Option<f64>,
    /// Mean wind at 10 m, km/h.
    pub wind_kmh: Option<f64>,
    /// Gusts, km/h.
    pub gust_kmh: Option<f64>,
    /// Visibility, m.
    pub visibility_m: Option<f64>,
    /// Fog (visibility < 1000 m or WMO 45/48).
    pub fog: Option<bool>,
    /// 0 °C isotherm, m.
    pub freezing_level_m: Option<f64>,
    /// Snowfall during the hour, cm.
    pub snowfall_cm: Option<f64>,
    /// Precipitation during the hour, mm.
    pub precipitation_mm: Option<f64>,
    /// Total cloud cover, %.
    pub cloud_cover_pct: Option<f64>,
    /// Relative humidity at 2 m, %.
    pub relative_humidity_pct: Option<f64>,
    /// WMO weather code.
    pub weather_code: Option<i32>,
}

/// Hourly series of one forecast point, all files merged.
#[derive(Debug, Clone, PartialEq)]
pub struct WeatherPoint {
    /// Identifier (`le-laus`).
    pub id: String,
    /// Name.
    pub name: String,
    /// Position.
    pub position: LonLat,
    /// Elevation of the model grid point, m.
    pub elevation_m: f64,
    /// Hours, sorted, one per local time.
    pub hours: Vec<WeatherHour>,
}

/// One file merged into a [`Weather`].
#[derive(Debug, Clone, PartialEq)]
pub struct WeatherFile {
    /// Path.
    pub path: PathBuf,
    /// Generation time written in the file.
    pub generated_at: String,
    /// First and last hour of the file.
    pub span: Option<(NaiveDateTime, NaiveDateTime)>,
}

/// Weather of a site: one or more `hourly.json` files merged (later files win on the
/// same hour, so pass history first and the forecast last).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Weather {
    /// Points.
    pub points: Vec<WeatherPoint>,
    /// Files read.
    pub files: Vec<WeatherFile>,
}

fn parse_local(s: &str) -> Option<NaiveDateTime> {
    NaiveDateTime::parse_from_str(s, "%Y-%m-%dT%H:%M")
        .or_else(|_| NaiveDateTime::parse_from_str(s, "%Y-%m-%dT%H:%M:%S"))
        .ok()
}

/// Weather summary of a day at a point.
#[derive(Debug, Clone, PartialEq, Default, serde::Serialize)]
pub struct DayWeather {
    /// Hours of the window with data.
    pub hours: usize,
    /// Mean temperature of the window, °C.
    pub temperature_mean_c: Option<f64>,
    /// Minimum and maximum temperature of the window, °C.
    pub temperature_min_c: Option<f64>,
    /// Maximum temperature of the window, °C.
    pub temperature_max_c: Option<f64>,
    /// Maximum mean wind, km/h.
    pub wind_max_kmh: Option<f64>,
    /// Maximum gust, km/h.
    pub gust_max_kmh: Option<f64>,
    /// Minimum wind chill, °C.
    pub wind_chill_min_c: Option<f64>,
    /// Hours with fog.
    pub fog_hours: usize,
    /// Snowfall during the window, cm.
    pub snowfall_cm: f64,
    /// Rain during the window (precipitation of hours above 0.5 °C without snowfall), mm.
    pub rain_mm: f64,
    /// Mean cloud cover, %.
    pub cloud_mean_pct: Option<f64>,
    /// Lowest 0 °C isotherm, m.
    pub freezing_level_min_m: Option<f64>,
    /// Mean relative humidity, %.
    pub humidity_mean_pct: Option<f64>,
}

fn mean(v: &[f64]) -> Option<f64> {
    (!v.is_empty()).then(|| v.iter().sum::<f64>() / v.len() as f64)
}

impl WeatherPoint {
    /// Hours from `from` (included) to `to` (excluded).
    pub fn between(&self, from: NaiveDateTime, to: NaiveDateTime) -> &[WeatherHour] {
        let a = self.hours.partition_point(|h| h.time < from);
        let b = self.hours.partition_point(|h| h.time < to);
        &self.hours[a..b.max(a)]
    }

    /// Hours of `date` from `h0` h (included) to `h1` h (excluded).
    pub fn window(&self, date: NaiveDate, h0: u32, h1: u32) -> &[WeatherHour] {
        let at = |h: u32| {
            date.and_hms_opt(0, 0, 0).expect("midnight") + chrono::Duration::hours(i64::from(h))
        };
        self.between(at(h0), at(h1))
    }

    /// Whether every hour of the window `date` `h0`..`h1` is present.
    pub fn covers(&self, date: NaiveDate, h0: u32, h1: u32) -> bool {
        self.window(date, h0, h1).len() == (h1 - h0) as usize
    }

    /// Snowfall between two instants, cm (missing hours count as 0).
    pub fn snowfall_between(&self, from: NaiveDateTime, to: NaiveDateTime) -> f64 {
        self.between(from, to)
            .iter()
            .filter_map(|h| h.snowfall_cm)
            .sum()
    }

    /// Summary of the skiing window of a day (`h0`..`h1`, local hours).
    pub fn summary(&self, date: NaiveDate, h0: u32, h1: u32) -> DayWeather {
        summarise(self.window(date, h0, h1))
    }

    /// Summary of the hours from `from` (included) to `to` (excluded).
    pub fn summary_between(&self, from: NaiveDateTime, to: NaiveDateTime) -> DayWeather {
        summarise(self.between(from, to))
    }
}

fn summarise(hs: &[WeatherHour]) -> DayWeather {
    let t: Vec<f64> = hs.iter().filter_map(|h| h.temperature_c).collect();
    let cc: Vec<f64> = hs.iter().filter_map(|h| h.cloud_cover_pct).collect();
    let rh: Vec<f64> = hs.iter().filter_map(|h| h.relative_humidity_pct).collect();
    let max = |f: fn(&WeatherHour) -> Option<f64>| hs.iter().filter_map(f).reduce(f64::max);
    let min = |f: fn(&WeatherHour) -> Option<f64>| hs.iter().filter_map(f).reduce(f64::min);
    DayWeather {
        hours: hs.len(),
        temperature_mean_c: mean(&t),
        temperature_min_c: min(|h| h.temperature_c),
        temperature_max_c: max(|h| h.temperature_c),
        wind_max_kmh: max(|h| h.wind_kmh),
        gust_max_kmh: max(|h| h.gust_kmh),
        wind_chill_min_c: min(|h| h.wind_chill_c),
        fog_hours: hs.iter().filter(|h| h.fog == Some(true)).count(),
        snowfall_cm: hs.iter().filter_map(|h| h.snowfall_cm).sum(),
        rain_mm: hs
            .iter()
            .filter(|h| {
                h.snowfall_cm.unwrap_or(0.0) <= 0.0 && h.temperature_c.is_some_and(|t| t > 0.5)
            })
            .filter_map(|h| h.precipitation_mm)
            .sum(),
        cloud_mean_pct: mean(&cc),
        freezing_level_min_m: min(|h| h.freezing_level_m),
        humidity_mean_pct: mean(&rh),
    }
}

impl Weather {
    /// Reads and merges `hourly.json` files; later files win on the same hour.
    pub fn load(paths: &[PathBuf]) -> Result<Self> {
        let mut points: BTreeMap<String, (WeatherPoint, BTreeMap<NaiveDateTime, WeatherHour>)> =
            BTreeMap::new();
        let mut order: Vec<String> = Vec::new();
        let mut files = Vec::new();
        for path in paths {
            let raw: RawHourlyFile = read_json(path)?;
            let _ = raw.site;
            let mut span: Option<(NaiveDateTime, NaiveDateTime)> = None;
            for p in raw.points {
                let entry = points.entry(p.id.clone()).or_insert_with(|| {
                    order.push(p.id.clone());
                    (
                        WeatherPoint {
                            id: p.id.clone(),
                            name: p.name.clone(),
                            position: LonLat::new(p.lon, p.lat),
                            elevation_m: p.elevation_m,
                            hours: Vec::new(),
                        },
                        BTreeMap::new(),
                    )
                });
                for h in p.hours {
                    let time = parse_local(&h.time)
                        .ok_or_else(|| Error::format(path, format!("bad time {}", h.time)))?;
                    span = Some(match span {
                        None => (time, time),
                        Some((a, b)) => (a.min(time), b.max(time)),
                    });
                    entry.1.insert(
                        time,
                        WeatherHour {
                            time,
                            temperature_c: h.temperature_c,
                            apparent_temperature_c: h.apparent_temperature_c,
                            wind_chill_c: h.wind_chill_c,
                            wind_kmh: h.wind_kmh,
                            gust_kmh: h.gust_kmh,
                            visibility_m: h.visibility_m,
                            fog: h.fog,
                            freezing_level_m: h.freezing_level_m,
                            snowfall_cm: h.snowfall_cm,
                            precipitation_mm: h.precipitation_mm,
                            cloud_cover_pct: h.cloud_cover_pct,
                            relative_humidity_pct: h.relative_humidity_pct,
                            weather_code: h.weather_code,
                        },
                    );
                    let _ = (h.wind_direction_deg, h.cloud_cover_low_pct);
                }
            }
            files.push(WeatherFile {
                path: path.clone(),
                generated_at: raw.generated_at,
                span,
            });
        }
        let points = order
            .into_iter()
            .filter_map(|id| points.remove(&id))
            .map(|(mut p, hours)| {
                p.hours = hours.into_values().collect();
                p
            })
            .collect();
        Ok(Weather { points, files })
    }

    /// Point by identifier.
    pub fn point(&self, id: &str) -> Option<&WeatherPoint> {
        self.points.iter().find(|p| p.id == id)
    }

    /// Nearest point to a position and its distance, m.
    pub fn nearest(&self, p: LonLat) -> Option<(&WeatherPoint, f64)> {
        self.points
            .iter()
            .map(|w| (w, w.position.haversine_m(p)))
            .min_by(|a, b| a.1.total_cmp(&b.1))
    }

    /// Last hour available over all points.
    pub fn last_hour(&self) -> Option<NaiveDateTime> {
        self.points
            .iter()
            .filter_map(|p| p.hours.last().map(|h| h.time))
            .max()
    }
}

// ---------------------------------------------------------------------------------------
// BERA (P8)

/// Risk block of a bulletin (as published by Météo-France).
#[derive(Debug, Clone, Default, PartialEq, Deserialize, serde::Serialize)]
pub struct BeraRisk {
    /// Risk of the lower band or first area.
    pub risk1: Option<u8>,
    /// Localisation of risk 1 (`<2100`, `E`...).
    pub loc1: Option<String>,
    /// Altitude separating the bands, m.
    pub altitude: Option<i32>,
    /// Risk of the upper band or second area.
    pub risk2: Option<u8>,
    /// Localisation of risk 2.
    pub loc2: Option<String>,
    /// Maximum risk of the day.
    pub max: Option<u8>,
    /// Text of the risk.
    pub comment: Option<String>,
}

/// Most dangerous aspects.
#[derive(Debug, Clone, Default, PartialEq, Deserialize, serde::Serialize)]
pub struct BeraAspects {
    /// North.
    pub n: bool,
    /// North-east.
    pub ne: bool,
    /// East.
    pub e: bool,
    /// South-east.
    pub se: bool,
    /// South.
    pub s: bool,
    /// South-west.
    pub sw: bool,
    /// West.
    pub w: bool,
    /// North-west.
    pub nw: bool,
}

impl BeraAspects {
    /// French short labels of the flagged aspects, clockwise from north.
    pub fn labels(&self) -> Vec<&'static str> {
        [
            (self.n, "nord"),
            (self.ne, "nord-est"),
            (self.e, "est"),
            (self.se, "sud-est"),
            (self.s, "sud"),
            (self.sw, "sud-ouest"),
            (self.w, "ouest"),
            (self.nw, "nord-ouest"),
        ]
        .into_iter()
        .filter_map(|(f, l)| f.then_some(l))
        .collect()
    }
}

/// The parts of a bulletin the assistant uses.
#[derive(Debug, Clone, PartialEq, Deserialize, serde::Serialize)]
pub struct BeraBulletin {
    /// Massif name.
    pub massif: String,
    /// Validity (`DATEVALIDITE`).
    pub date_validity: Option<String>,
    /// Risk.
    #[serde(default)]
    pub risk: BeraRisk,
    /// Aspects.
    #[serde(default)]
    pub aspects: BeraAspects,
    /// Accidental triggering text.
    pub accidental: Option<String>,
    /// Natural avalanches text.
    pub natural: Option<String>,
    /// Snow quality text.
    pub snow_quality: Option<String>,
}

/// A link of the BERA layer.
#[derive(Debug, Clone, PartialEq, Deserialize, serde::Serialize)]
pub struct Link {
    /// Label.
    pub label: String,
    /// URL.
    pub url: String,
}

/// Status of the BERA of a site on a date.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum BeraStatus {
    /// A bulletin was read.
    Bulletin,
    /// The massif has a bulletin but it could not be read offline.
    Unavailable,
    /// The massif has no BERA (Jura).
    NoBera,
}

/// `bera.json` of a site.
#[derive(Debug, Clone, PartialEq, Deserialize, serde::Serialize)]
pub struct Bera {
    /// Site.
    pub site: String,
    /// Date of the bulletin requested.
    pub date: String,
    /// Massif name.
    pub massif: Option<String>,
    /// Status.
    pub status: BeraStatus,
    /// PDF of the official bulletin.
    pub pdf_url: Option<String>,
    /// Bulletin, when read.
    pub bulletin: Option<BeraBulletin>,
    /// Message of the layer.
    pub notice: String,
    /// Links (official bulletin, Jura alternatives).
    #[serde(default)]
    pub links: Vec<Link>,
}

impl Bera {
    /// Reads `bera.json`.
    pub fn load(path: &Path) -> Result<Self> {
        read_json(path)
    }

    /// Date of the bulletin.
    pub fn date(&self) -> Option<NaiveDate> {
        NaiveDate::parse_from_str(&self.date, "%Y-%m-%d").ok()
    }

    /// Maximum level of the day, when a bulletin was read.
    pub fn level(&self) -> Option<u8> {
        self.bulletin.as_ref().and_then(|b| b.risk.max)
    }
}

// ---------------------------------------------------------------------------------------
// Status (P13)

/// Open state of an itinerary.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum OpenState {
    /// Open.
    Open,
    /// Closed.
    Closed,
    /// Not known for the date.
    Unknown,
}

/// Practice of an itinerary.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Practice {
    /// Cross-country skiing (classic and/or skating).
    Nordic,
    /// Snowshoes, walking, fat bike.
    Snowshoe,
    /// Shared multi-activity trail.
    Multi,
}

/// An itinerary of the status sources, with its state.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct Itinerary {
    /// Identifier (`enj:244720`, `nordicfrance:le-gibera`).
    pub id: String,
    /// Name.
    pub name: String,
    /// Source (`enj`, `nordicfrance`).
    pub source: String,
    /// Sector (Nordic France) or departure commune code (ENJ).
    pub sector: Option<String>,
    /// Practice.
    pub practice: Practice,
    /// State given by the source.
    pub state: OpenState,
    /// Length, km.
    pub length_km: Option<f64>,
    /// Difficulty or colour as published.
    pub difficulty: Option<String>,
    /// Comment of the source.
    pub comment: Option<String>,
    /// Geometry (ENJ: from `status.geojson`; Nordic France: OSM ways matched by name).
    #[serde(skip)]
    pub line: Vec<LonLat>,
}

/// Snapshot of the status layer.
#[derive(Debug, Clone, PartialEq)]
pub struct Status {
    /// Poll time.
    pub polled_at: DateTime<Utc>,
    /// Itineraries.
    pub itineraries: Vec<Itinerary>,
    /// Messages of the sources (mairie banner, Nordic France summary).
    pub messages: Vec<String>,
    /// Disagreements between two sources on the same piste.
    pub conflicts: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct RawStatus {
    polled_at: String,
    #[serde(default)]
    sources: BTreeMap<String, Value>,
}

#[derive(Debug, Deserialize)]
struct RawEnj {
    treks: Vec<RawTrek>,
}

#[derive(Debug, Deserialize)]
struct RawTrek {
    id: i64,
    name: String,
    practice: Option<String>,
    difficulty: Option<String>,
    length_m: Option<f64>,
    state: String,
    departure_city: Option<i64>,
}

#[derive(Debug, Deserialize)]
struct RawNf {
    #[serde(default)]
    sectors: Vec<RawNfSector>,
    open_summary: Option<String>,
    updated: Option<String>,
    snow_depth: Option<String>,
}

#[derive(Debug, Deserialize)]
struct RawNfSector {
    name: String,
    #[serde(default)]
    pistes: Vec<RawNfPiste>,
}

#[derive(Debug, Deserialize)]
struct RawNfPiste {
    name: String,
    activity: Option<String>,
    comment: Option<String>,
    flag: Option<String>,
    length_km: Option<f64>,
    state: String,
}

#[derive(Debug, Deserialize)]
struct RawMairie {
    message: Option<String>,
    modified_time: Option<String>,
    #[serde(default)]
    sectors: Vec<RawMairieSector>,
}

#[derive(Debug, Deserialize)]
struct RawMairieSector {
    #[serde(default)]
    rows: Vec<RawMairieRow>,
}

#[derive(Debug, Deserialize)]
struct RawMairieRow {
    piste: String,
    state: String,
}

fn state_of(s: &str) -> OpenState {
    let s = s.to_lowercase();
    if s == "open" || s.starts_with("ouvert") || s == "partially_open" {
        OpenState::Open
    } else if s == "closed" || s.starts_with("ferm") {
        OpenState::Closed
    } else {
        OpenState::Unknown
    }
}

/// Lower-case words of a name without accents, articles and generic words
/// (« Le Chemin du Sioux » → `sioux`; « L'Izoard » → `izoard`).
pub fn name_key(name: &str) -> Vec<String> {
    const STOP: &[&str] = &[
        "le", "la", "les", "l", "de", "du", "des", "d", "chemin", "piste", "route", "col", "et",
        "a", "au", "aux", "en",
    ];
    let folded: String = name
        .chars()
        .map(|c| match c {
            'à' | 'â' | 'ä' | 'À' | 'Â' => 'a',
            'é' | 'è' | 'ê' | 'ë' | 'É' | 'È' | 'Ê' => 'e',
            'î' | 'ï' | 'Î' => 'i',
            'ô' | 'ö' | 'Ô' => 'o',
            'ù' | 'û' | 'ü' | 'Û' => 'u',
            'ç' | 'Ç' => 'c',
            c if c.is_alphanumeric() => c.to_ascii_lowercase(),
            _ => ' ',
        })
        .collect();
    folded
        .split_whitespace()
        .filter(|w| !STOP.contains(w))
        // « 1.5km », « 16km », « 3 », « km »: lengths written in the names of the mairie.
        .filter(|w| !w.chars().next().is_some_and(|c| c.is_ascii_digit()) && *w != "km")
        .map(str::to_owned)
        .collect()
}

/// Whether two names designate the same piste: same significant words, or every word of
/// the shorter name in the longer one.
pub fn same_name(a: &str, b: &str) -> bool {
    let (ka, kb) = (name_key(a), name_key(b));
    if ka.is_empty() || kb.is_empty() {
        return false;
    }
    let (short, long) = if ka.len() <= kb.len() {
        (&ka, &kb)
    } else {
        (&kb, &ka)
    };
    short.iter().all(|w| long.contains(w))
}

fn practice_of(text: &str) -> Practice {
    let t = text.to_lowercase();
    let ski = t.contains("ski de fond") || t.contains("skating") || t.contains("classique");
    let foot = t.contains("raquette") || t.contains("marche") || t.contains("piéton");
    match (ski, foot) {
        (true, false) => Practice::Nordic,
        (true, true) => Practice::Multi,
        (false, true) => Practice::Snowshoe,
        (false, false) if t.contains("multi") => Practice::Multi,
        (false, false) => Practice::Nordic,
    }
}

impl Status {
    /// Reads `status.json` and, when present, the ENJ geometries of `status.geojson`.
    pub fn load(dir: &Path) -> Result<Self> {
        let path = dir.join("status.json");
        let raw: RawStatus = read_json(&path)?;
        let polled_at = DateTime::parse_from_rfc3339(&raw.polled_at)
            .map_err(|e| Error::format(&path, e))?
            .with_timezone(&Utc);
        let mut itineraries = Vec::new();
        let mut messages = Vec::new();
        let mut conflicts = Vec::new();
        if let Some(v) = raw.sources.get("enj") {
            let enj: RawEnj =
                serde_json::from_value(v.clone()).map_err(|e| Error::format(&path, e))?;
            let geoms = enj_geometries(&dir.join("status.geojson")).unwrap_or_default();
            for t in enj.treks {
                itineraries.push(Itinerary {
                    id: format!("enj:{}", t.id),
                    name: t.name,
                    source: "enj".into(),
                    sector: t.departure_city.map(|c| format!("commune ENJ {c}")),
                    practice: practice_of(t.practice.as_deref().unwrap_or("")),
                    state: state_of(&t.state),
                    length_km: t.length_m.map(|m| m / 1000.0),
                    difficulty: t.difficulty,
                    comment: None,
                    line: geoms.get(&t.id).cloned().unwrap_or_default(),
                });
            }
        }
        if let Some(v) = raw.sources.get("nordicfrance") {
            let nf: RawNf =
                serde_json::from_value(v.clone()).map_err(|e| Error::format(&path, e))?;
            messages.push(format!(
                "Nordic France : {} ouverts, mise à jour du {}, neige {}",
                nf.open_summary.as_deref().unwrap_or("?"),
                nf.updated.as_deref().unwrap_or("?"),
                nf.snow_depth.as_deref().unwrap_or("?")
            ));
            for s in nf.sectors {
                for p in s.pistes {
                    let key = name_key(&p.name).join("-");
                    itineraries.push(Itinerary {
                        id: format!("nordicfrance:{key}"),
                        practice: practice_of(p.activity.as_deref().unwrap_or("")),
                        name: p.name,
                        source: "nordicfrance".into(),
                        sector: Some(s.name.clone()),
                        state: state_of(&p.state),
                        length_km: p.length_km,
                        difficulty: p.flag,
                        comment: p.comment,
                        line: Vec::new(),
                    });
                }
            }
        }
        if let Some(v) = raw.sources.get("cervieres-mairie") {
            let m: RawMairie =
                serde_json::from_value(v.clone()).map_err(|e| Error::format(&path, e))?;
            if let Some(msg) = m.message {
                // First 100 characters, cut at a word.
                let mut short = String::new();
                for w in msg.split_whitespace() {
                    if short.chars().count() + w.chars().count() > 100 {
                        short.push_str(" …");
                        break;
                    }
                    if !short.is_empty() {
                        short.push(' ');
                    }
                    short.push_str(w);
                }
                messages.push(format!(
                    "Mairie ({}) : {short}",
                    m.modified_time.as_deref().unwrap_or("date inconnue")
                ));
            }
            for row in m.sectors.iter().flat_map(|s| &s.rows) {
                let st = state_of(&row.state);
                if let Some(it) = itineraries
                    .iter()
                    .find(|i| i.source == "nordicfrance" && same_name(&i.name, &row.piste))
                    && it.state != st
                    && st != OpenState::Unknown
                {
                    conflicts.push(format!(
                        "{} : Nordic France {:?}, mairie {:?}",
                        it.name, it.state, st
                    ));
                }
            }
        }
        Ok(Status {
            polled_at,
            itineraries,
            messages,
            conflicts,
        })
    }
}

fn enj_geometries(path: &Path) -> Result<BTreeMap<i64, Vec<LonLat>>> {
    let v: Value = read_json(path)?;
    let mut out = BTreeMap::new();
    for f in v["features"].as_array().into_iter().flatten() {
        let Some(id) = f["properties"]["id"].as_i64() else {
            continue;
        };
        let line = line_coords(&f["geometry"]);
        if !line.is_empty() {
            out.insert(id, line);
        }
    }
    Ok(out)
}

// ---------------------------------------------------------------------------------------
// Geometry helpers

fn coord(v: &Value) -> Option<LonLat> {
    Some(LonLat::new(v.get(0)?.as_f64()?, v.get(1)?.as_f64()?))
}

/// Coordinates of a LineString or MultiLineString (parts concatenated).
pub fn line_coords(geometry: &Value) -> Vec<LonLat> {
    match geometry["type"].as_str() {
        Some("LineString") => geometry["coordinates"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(coord)
            .collect(),
        Some("MultiLineString") => geometry["coordinates"]
            .as_array()
            .into_iter()
            .flatten()
            .flat_map(|l| l.as_array().into_iter().flatten().filter_map(coord))
            .collect(),
        _ => Vec::new(),
    }
}

fn ring(v: &Value) -> LineString<f64> {
    LineString::from(
        v.as_array()
            .into_iter()
            .flatten()
            .filter_map(coord)
            .map(|p| Coord { x: p.lon, y: p.lat })
            .collect::<Vec<_>>(),
    )
}

fn polygon(v: &Value) -> Option<Polygon<f64>> {
    let rings = v.as_array()?;
    let exterior = ring(rings.first()?);
    let interiors = rings[1..].iter().map(ring).collect();
    Some(Polygon::new(exterior, interiors))
}

/// Polygon or MultiPolygon geometry as a multipolygon (other types: empty).
pub fn multipolygon(geometry: &Value) -> MultiPolygon<f64> {
    match geometry["type"].as_str() {
        Some("Polygon") => {
            MultiPolygon::new(polygon(&geometry["coordinates"]).into_iter().collect())
        }
        Some("MultiPolygon") => MultiPolygon::new(
            geometry["coordinates"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(polygon)
                .collect(),
        ),
        _ => MultiPolygon::new(Vec::new()),
    }
}

/// A polygonal area with its bounding box, for fast point tests.
#[derive(Debug, Clone, PartialEq)]
pub struct Area {
    /// Label (CLPA index, kind).
    pub label: String,
    /// Geometry, WGS84.
    pub shape: MultiPolygon<f64>,
    /// Bounding box.
    pub bbox: Option<Rect<f64>>,
}

impl Area {
    /// Area from a geometry.
    pub fn new(label: impl Into<String>, shape: MultiPolygon<f64>) -> Self {
        let bbox = shape.bounding_rect();
        Self {
            label: label.into(),
            shape,
            bbox,
        }
    }

    /// Whether the area contains a point.
    pub fn contains(&self, p: LonLat) -> bool {
        let pt = Point::new(p.lon, p.lat);
        match self.bbox {
            Some(b)
                if p.lon < b.min().x
                    || p.lon > b.max().x
                    || p.lat < b.min().y
                    || p.lat > b.max().y =>
            {
                false
            }
            Some(_) => self.shape.contains(&pt),
            None => false,
        }
    }
}

/// Set of polygonal areas (CLPA paths, zones without network).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Areas {
    /// Areas.
    pub areas: Vec<Area>,
}

impl Areas {
    /// Reads a GeoJSON FeatureCollection of polygons; `keep` filters on the properties;
    /// `label` names each area.
    pub fn load_filtered(
        path: &Path,
        keep: impl Fn(&Value) -> bool,
        label: impl Fn(&Value) -> String,
    ) -> Result<Self> {
        let v: Value = read_json(path)?;
        let areas = v["features"]
            .as_array()
            .into_iter()
            .flatten()
            .filter(|f| keep(&f["properties"]))
            .map(|f| Area::new(label(&f["properties"]), multipolygon(&f["geometry"])))
            .filter(|a| !a.shape.0.is_empty())
            .collect();
        Ok(Areas { areas })
    }

    /// Reads every polygon of a GeoJSON file (coverage `no_network.geojson`).
    pub fn load(path: &Path) -> Result<Self> {
        Self::load_filtered(
            path,
            |_| true,
            |p| p["kind"].as_str().unwrap_or("").to_owned(),
        )
    }

    /// First area containing a point.
    pub fn find(&self, p: LonLat) -> Option<&Area> {
        self.areas.iter().find(|a| a.contains(p))
    }
}

/// Points every `step_m` metres along a line (vertices kept), with the length of line each
/// point stands for.
pub fn densify(line: &[LonLat], step_m: f64) -> Vec<(LonLat, f64)> {
    let mut out = Vec::new();
    for w in line.windows(2) {
        let (a, b) = (w[0], w[1]);
        let d = a.haversine_m(b);
        let n = (d / step_m).ceil().max(1.0) as usize;
        for i in 0..n {
            let f = i as f64 / n as f64;
            out.push((
                LonLat::new(a.lon + (b.lon - a.lon) * f, a.lat + (b.lat - a.lat) * f),
                d / n as f64,
            ));
        }
    }
    if let Some(last) = line.last() {
        out.push((*last, 0.0));
    }
    out
}

/// Length of a line, m.
pub fn line_length_m(line: &[LonLat]) -> f64 {
    line.windows(2).map(|w| w[0].haversine_m(w[1])).sum()
}

// ---------------------------------------------------------------------------------------
// Domains (P3) and pistes (P2)

/// A nordic domain of the site.
#[derive(Debug, Clone, PartialEq)]
pub struct Domain {
    /// Identifier (`site-13681086`).
    pub id: String,
    /// Name.
    pub name: String,
    /// Nordic kilometres (OSM).
    pub km_nordic: f64,
    /// Representative point.
    pub point: LonLat,
    /// Outline.
    pub area: Area,
    /// OSM ways.
    pub ways: Vec<i64>,
}

/// Reads `domaines.geojson` and `domaines_ways.json` of a site.
pub fn load_domains(dir: &Path) -> Result<Vec<Domain>> {
    let v: Value = read_json(&dir.join("domaines.geojson"))?;
    let ways: BTreeMap<String, Vec<i64>> =
        read_json(&dir.join("domaines_ways.json")).unwrap_or_default();
    let mut out = Vec::new();
    for f in v["features"].as_array().into_iter().flatten() {
        let p = &f["properties"];
        let id = p["id"].as_str().unwrap_or("").to_owned();
        let name = p["name"].as_str().unwrap_or(&id).to_owned();
        let (Some(lon), Some(lat)) = (p["lon"].as_f64(), p["lat"].as_f64()) else {
            continue;
        };
        out.push(Domain {
            ways: ways.get(&id).cloned().unwrap_or_default(),
            area: Area::new(name.clone(), multipolygon(&f["geometry"])),
            id,
            name,
            km_nordic: p["km_nordic"].as_f64().unwrap_or(0.0),
            point: LonLat::new(lon, lat),
        });
    }
    Ok(out)
}

/// A piste way of OSM.
#[derive(Debug, Clone, PartialEq)]
pub struct Piste {
    /// OSM way.
    pub way_id: i64,
    /// Name of the way.
    pub name: Option<String>,
    /// Names of the relations the way belongs to.
    pub relations: Vec<String>,
    /// Uses (`nordic`, `hike`, `snowshoe`).
    pub uses: Vec<String>,
    /// Geometry.
    pub line: Vec<LonLat>,
}

/// Reads `pistes.geojson`.
pub fn load_pistes(path: &Path) -> Result<Vec<Piste>> {
    let v: Value = read_json(path)?;
    Ok(v["features"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|f| {
            let p = &f["properties"];
            Some(Piste {
                way_id: p["osm_way_id"].as_i64()?,
                name: p["name"].as_str().map(str::to_owned),
                relations: p["relations"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(|r| r["name"].as_str().map(str::to_owned))
                    .collect(),
                uses: p["uses"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(|u| u.as_str().map(str::to_owned))
                    .collect(),
                line: line_coords(&f["geometry"]),
            })
        })
        .collect())
}

// ---------------------------------------------------------------------------------------
// Snow map (P6)

/// Estimated snow depth of a site at a date (three rasters, Lambert-93, 25 m).
#[derive(Debug, Clone)]
pub struct SnowMap {
    /// State described (06 h UTC of the simulation date).
    pub valid_at: DateTime<Utc>,
    /// Central value, cm.
    pub depth: Grid<f32>,
    /// Lower bound, cm.
    pub low: Option<Grid<f32>>,
    /// Upper bound, cm.
    pub high: Option<Grid<f32>>,
}

/// Snow depth at a point, cm.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SnowSample {
    /// Central value.
    pub depth_cm: f64,
    /// Lower bound.
    pub low_cm: Option<f64>,
    /// Upper bound.
    pub high_cm: Option<f64>,
}

impl SnowMap {
    /// Reads the snow layer directory.
    pub fn load(dir: &Path) -> Result<Self> {
        let meta = LayerMeta::read(&dir.join("snow.meta.json"))?;
        let valid_at = meta
            .valid_from
            .as_deref()
            .and_then(|s| DateTime::parse_from_rfc3339(s).ok())
            .map(|d| d.with_timezone(&Utc))
            .ok_or_else(|| Error::format(dir.join("snow.meta.json"), "no valid_from"))?;
        let read = |n: &str| skimap_common::geotiff::read_f32(&dir.join(n));
        Ok(SnowMap {
            valid_at,
            depth: read("snow_depth.tif")?,
            low: read("snow_depth_low.tif").ok(),
            high: read("snow_depth_high.tif").ok(),
        })
    }

    /// Snow at a position (`None` outside the raster or where it has no value).
    pub fn sample(&self, p: LonLat) -> Option<SnowSample> {
        let (x, y) = skimap_common::crs::to_lambert93(p).ok()?;
        let get = |g: &Grid<f32>| {
            g.value_at(x, y)
                .copied()
                .filter(|v| v.is_finite())
                .map(f64::from)
        };
        Some(SnowSample {
            depth_cm: get(&self.depth)?,
            low_cm: self.low.as_ref().and_then(get),
            high_cm: self.high.as_ref().and_then(get),
        })
    }

    /// Simulation date (UTC date of the 06 h state).
    pub fn date(&self) -> NaiveDate {
        self.valid_at.date_naive()
    }
}

// ---------------------------------------------------------------------------------------
// Avalanche exposure (P16–P18)

/// Reads a UInt8 class GeoTIFF (the `avalanche-exposure` classes: Lambert-93, square
/// cells, `GDAL_NODATA` 255). `skimap_common::geotiff` reads Float32 only.
pub fn read_class_u8(path: &Path) -> Result<Grid<u8>> {
    use tiff::decoder::{Decoder, DecodingResult, Limits};
    use tiff::tags::Tag;
    let bytes = std::fs::read(path).map_err(|source| Error::Io {
        path: path.to_owned(),
        source,
    })?;
    let e = |m: tiff::TiffError| Error::format(path, m);
    let mut dec = Decoder::new(std::io::Cursor::new(bytes))
        .map_err(e)?
        .with_limits(Limits::unlimited());
    let (w, h) = dec.dimensions().map_err(e)?;
    let scale = dec.get_tag_f64_vec(Tag::ModelPixelScaleTag).map_err(e)?;
    let tie = dec.get_tag_f64_vec(Tag::ModelTiepointTag).map_err(e)?;
    if scale.len() < 2 || tie.len() < 6 || (scale[0] - scale[1]).abs() > 1e-9 {
        return Err(Error::format(path, "georeferencing tags"));
    }
    let origin_x = tie[3] - tie[0] * scale[0];
    let origin_y = tie[4] + tie[1] * scale[1];
    let data = match dec.read_image().map_err(e)? {
        DecodingResult::U8(v) => v,
        _ => return Err(Error::format(path, "not a UInt8 raster")),
    };
    let gt = skimap_common::grid::GeoTransform::north_up(origin_x, origin_y, scale[0]);
    Ok(Grid::from_vec(
        w as usize,
        h as usize,
        gt,
        skimap_common::crs::Crs::Lambert93,
        data,
    )?
    .with_nodata(255))
}

// ---------------------------------------------------------------------------------------
// Avalanche paths (P20)

/// Historic avalanche paths (CLPA) of the site and the length of each piste inside them.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Clpa {
    /// Whether the site is covered by the CLPA (false in the Jura).
    pub covered: bool,
    /// CLPA paths.
    pub areas: Areas,
    /// Kilometres inside CLPA paths per piste name (`summary.json`).
    pub piste_km: BTreeMap<String, f64>,
}

impl Clpa {
    /// Reads the `avalanche-historic` layer directory.
    pub fn load(dir: &Path) -> Result<Self> {
        let summary: Value = read_json(&dir.join("summary.json"))?;
        let areas = Areas::load_filtered(
            &dir.join("emprises.geojson"),
            |p| p["source"].as_str().is_some_and(|s| s.contains("CLPA")),
            |p| format!("CLPA {}", p["ddt05_index"]),
        )?;
        let piste_km = summary["pistes"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|p| {
                Some((
                    p["name"].as_str()?.to_owned(),
                    p["inside_clpa_km"].as_f64()?,
                ))
            })
            .collect();
        Ok(Clpa {
            covered: summary["covered"].as_bool().unwrap_or(false),
            areas,
            piste_km,
        })
    }
}

// ---------------------------------------------------------------------------------------
// Zones and rules (P9, P10)

/// Reads `zones.geojson` and `rules.json` of a site.
pub fn load_zones(dir: &Path) -> Result<(skimap_zones::ZoneLayer, skimap_zones::RuleSet)> {
    let zp = dir.join("zones.geojson");
    let rp = dir.join("rules.json");
    let text = |p: &Path| {
        std::fs::read_to_string(p).map_err(|source| Error::Io {
            path: p.to_owned(),
            source,
        })
    };
    let layer = skimap_zones::ZoneLayer::from_geojson_str(&text(&zp)?)
        .map_err(|e| Error::format(&zp, e))?;
    let rules =
        skimap_zones::RuleSet::from_json_str(&text(&rp)?).map_err(|e| Error::format(&rp, e))?;
    Ok((layer, rules))
}

// ---------------------------------------------------------------------------------------
// Planned route

/// A planned route.
#[derive(Debug, Clone, PartialEq)]
pub struct Route {
    /// Name (file name, or `name` / `from`–`to` properties).
    pub name: String,
    /// Geometry.
    pub line: Vec<LonLat>,
}

impl Route {
    /// Reads the first LineString of a GeoJSON file whose `kind` is `route`, or else the
    /// first LineString.
    pub fn load(path: &Path) -> Result<Self> {
        let v: Value = read_json(path)?;
        let feats: Vec<&Value> = v["features"].as_array().into_iter().flatten().collect();
        let is_line = |f: &&Value| !line_coords(&f["geometry"]).is_empty();
        let f = feats
            .iter()
            .find(|f| is_line(f) && f["properties"]["kind"] == "route")
            .or_else(|| feats.iter().find(|f| is_line(f)))
            .ok_or_else(|| Error::format(path, "no LineString"))?;
        let p = &f["properties"];
        let name = match (p["name"].as_str(), p["from"].as_str(), p["to"].as_str()) {
            (Some(n), _, _) => n.to_owned(),
            (None, Some(a), Some(b)) => format!("{a} → {b}"),
            _ => path
                .file_stem()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_default(),
        };
        Ok(Route {
            name,
            line: line_coords(&f["geometry"]),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_match_across_sources() {
        assert_eq!(name_key("Le Chemin du Sioux"), vec!["sioux"]);
        assert_eq!(name_key("L’Aigalan 8km"), vec!["aigalan"]);
        assert!(same_name("Le Sioux", "Le Chemin du Sioux"));
        assert!(same_name("L'Izoard", "Izoard 16km"));
        assert!(same_name("Le Gibera 1.5km", "Le Gibera"));
        assert!(!same_name("Le Savare", "Le Cancel"));
        assert!(!same_name("Piéton les Fonts", "L'Aigalan"));
    }

    #[test]
    fn states_and_practices() {
        assert_eq!(state_of("Fermé"), OpenState::Closed);
        assert_eq!(state_of("open"), OpenState::Open);
        assert_eq!(state_of("partially_open"), OpenState::Open);
        assert_eq!(state_of("?"), OpenState::Unknown);
        assert_eq!(practice_of("8km - Ski de fond"), Practice::Nordic);
        assert_eq!(
            practice_of("3.5km - Marche/trail , Ski de fond"),
            Practice::Multi
        );
        assert_eq!(practice_of("Raquettes"), Practice::Snowshoe);
    }

    #[test]
    fn areas_and_densify() {
        let g: Value = serde_json::json!({"type":"Polygon","coordinates":[[[6.0,46.0],[6.01,46.0],[6.01,46.01],[6.0,46.01],[6.0,46.0]]]});
        let a = Area::new("x", multipolygon(&g));
        assert!(a.contains(LonLat::new(6.005, 46.005)));
        assert!(!a.contains(LonLat::new(6.02, 46.005)));
        let line = [LonLat::new(6.0, 46.0), LonLat::new(6.0, 46.009)];
        let pts = densify(&line, 100.0);
        let total: f64 = pts.iter().map(|p| p.1).sum();
        assert!((total - line_length_m(&line)).abs() < 1e-6);
        assert!(pts.len() >= 11);
    }

    #[test]
    fn weather_window_and_merge() {
        let dir = tempfile::tempdir().unwrap();
        let f1 = dir.path().join("a.json");
        let f2 = dir.path().join("b.json");
        let hour = |t: &str, temp: f64| serde_json::json!({"time": t, "temperature_c": temp, "snowfall_cm": 1.0});
        let file = |hours: Vec<Value>| serde_json::json!({"site":"s","generated_at":"x","points":[{"id":"p","name":"P","lat":46.0,"lon":6.0,"elevation_m":1000.0,"hours":hours}]});
        std::fs::write(
            &f1,
            file(vec![
                hour("2026-01-01T09:00", 1.0),
                hour("2026-01-01T10:00", 2.0),
            ])
            .to_string(),
        )
        .unwrap();
        std::fs::write(
            &f2,
            file(vec![
                hour("2026-01-01T10:00", 5.0),
                hour("2026-01-01T11:00", 6.0),
            ])
            .to_string(),
        )
        .unwrap();
        let w = Weather::load(&[f1, f2]).unwrap();
        let p = w.point("p").unwrap();
        assert_eq!(p.hours.len(), 3);
        assert_eq!(p.hours[1].temperature_c, Some(5.0), "later file wins");
        let d = NaiveDate::from_ymd_opt(2026, 1, 1).unwrap();
        let s = p.summary(d, 9, 12);
        assert_eq!(s.hours, 3);
        assert_eq!(s.temperature_max_c, Some(6.0));
        assert!((s.snowfall_cm - 3.0).abs() < 1e-9);
        assert!(p.covers(d, 9, 12));
        assert!(!p.covers(d, 8, 12));
    }
}
