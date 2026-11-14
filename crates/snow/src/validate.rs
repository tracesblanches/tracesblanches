//! Validation of the estimated snow depth against Météo-France stations (P7, #20).
//!
//! For each station and each day of the winter seasons (1 November to 31 May) with a 06 UTC
//! observation, two estimates are compared with the measured depth:
//!
//! - **product**: the value of the 25 m cell containing the station, exactly as in the
//!   daily map (cell elevation, slope and aspect, massif polygon);
//! - **flat**: the simulation on flat ground at the station altitude, which isolates the
//!   simulation and the altitude interpolation from the terrain projection (snow stations
//!   measure on flat open ground).
//!
//! Bias is estimate − observation (cm); MAE and RMSE in cm. The range coverage is the share
//! of days whose observation lies inside [low, high] of the product.

use std::collections::BTreeMap;
use std::time::Instant;

use chrono::{Datelike, NaiveDate};
use serde::{Deserialize, Serialize};
use skimap_common::crs::to_lambert93;
use skimap_common::download::Downloader;
use skimap_common::geom::LonLat;
use skimap_common::layout::DataLayout;
use skimap_common::site::Site;
use skimap_terrain::Extent;

use crate::classes::{PRACTICES, Practice};
use crate::error::{Error, Result};
use crate::map::{estimate_cell, snow_terrain};
use crate::massifs;
use crate::sim::{Simulation, fetch_year};
use crate::stations::{self, STATIONS, StationDef};

/// Half-side of the terrain patch computed around a station, metres.
pub const PATCH_HALF_SIDE_M: f64 = 500.0;
/// Snow presence threshold for the presence agreement, cm.
pub const PRESENCE_CM: f32 = 5.0;

/// Error statistics of an estimate against observations.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Stats {
    /// Number of days.
    pub n: usize,
    /// Mean of estimate − observation, cm.
    pub bias_cm: f64,
    /// Mean absolute error, cm.
    pub mae_cm: f64,
    /// Root mean square error, cm.
    pub rmse_cm: f64,
    /// Mean observation, cm.
    pub mean_obs_cm: f64,
    /// Mean estimate, cm.
    pub mean_est_cm: f64,
}

/// Statistics of (estimate, observation) pairs.
pub fn stats(pairs: &[(f32, f32)]) -> Stats {
    let n = pairs.len();
    if n == 0 {
        return Stats::default();
    }
    let nf = n as f64;
    let (mut b, mut a, mut s, mut o, mut e) = (0.0, 0.0, 0.0, 0.0, 0.0);
    for &(est, obs) in pairs {
        let d = f64::from(est) - f64::from(obs);
        b += d;
        a += d.abs();
        s += d * d;
        o += f64::from(obs);
        e += f64::from(est);
    }
    Stats {
        n,
        bias_cm: b / nf,
        mae_cm: a / nf,
        rmse_cm: (s / nf).sqrt(),
        mean_obs_cm: o / nf,
        mean_est_cm: e / nf,
    }
}

/// Statistics of one month (all seasons together).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MonthStats {
    /// Month number (11, 12, 1, …, 5).
    pub month: u32,
    /// Product (25 m cell).
    pub product: Stats,
    /// Flat at the station altitude.
    pub flat: Stats,
}

/// Statistics of one season.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SeasonStats {
    /// Season label, e.g. `2024-2025`.
    pub season: String,
    /// Product (25 m cell).
    pub product: Stats,
    /// Flat at the station altitude.
    pub flat: Stats,
}

/// Agreement of the three classes of a practice (observed depth vs product).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ClassAgreement {
    /// Practice identifier.
    pub practice: String,
    /// Thresholds, cm.
    pub thresholds_cm: [f32; 2],
    /// Share of days with the same class.
    pub accuracy: f64,
    /// Confusion `[observed class][estimated class]`, days.
    pub confusion: [[usize; 3]; 3],
}

/// Validation of one station.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StationValidation {
    /// Station number.
    pub id: String,
    /// Station name.
    pub name: String,
    /// Station position.
    pub lon_lat: [f64; 2],
    /// Station altitude (Météo-France metadata), m.
    pub station_alti_m: f64,
    /// Massif of the station point (polygon).
    pub massif: u16,
    /// Massif name.
    pub massif_name: String,
    /// Elevation of the 25 m cell, m.
    pub cell_alti_m: f32,
    /// Slope of the 25 m cell, degrees.
    pub cell_slope_deg: f32,
    /// Aspect of the 25 m cell, degrees (-1 flat).
    pub cell_aspect_deg: f32,
    /// First and last compared day.
    pub period: [NaiveDate; 2],
    /// Product statistics over all days.
    pub product: Stats,
    /// Flat-at-station-altitude statistics over all days.
    pub flat: Stats,
    /// By month.
    pub by_month: Vec<MonthStats>,
    /// By season.
    pub by_season: Vec<SeasonStats>,
    /// Share of days with the observation inside [low, high].
    pub range_coverage: f64,
    /// Mean width of the range, cm.
    pub mean_range_width_cm: f64,
    /// Share of days where observation and product agree on snow ≥ 5 cm.
    pub presence_agreement: f64,
    /// Class agreement per practice.
    pub classes: Vec<ClassAgreement>,
}

/// Validation of a site.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Validation {
    /// Site identifier.
    pub site: String,
    /// Seasons (first and last start year).
    pub seasons: [i32; 2],
    /// All stations of the site pooled: product.
    pub site_product: Stats,
    /// All stations of the site pooled: flat at the station altitude.
    pub site_flat: Stats,
    /// All stations of the site pooled, by month.
    pub site_by_month: Vec<MonthStats>,
    /// Stations.
    pub stations: Vec<StationValidation>,
    /// Wall-clock seconds of the validation.
    pub seconds: f64,
}

/// (estimate, observation) pairs of the product and of the flat estimate.
type PairLists = (Vec<(f32, f32)>, Vec<(f32, f32)>);

/// Month statistics in season order (November to May).
fn month_stats(by_month: BTreeMap<u32, PairLists>) -> Vec<MonthStats> {
    let order = |m: u32| if m >= 11 { m - 11 } else { m + 1 };
    let mut months: Vec<MonthStats> = by_month
        .into_iter()
        .map(|(month, (p, f))| MonthStats {
            month,
            product: stats(&p),
            flat: stats(&f),
        })
        .collect();
    months.sort_by_key(|m| order(m.month));
    months
}

/// Season start year of a date (November–May), `None` in summer.
pub fn season_of(d: NaiveDate) -> Option<i32> {
    match d.month() {
        11 | 12 => Some(d.year()),
        1..=5 => Some(d.year() - 1),
        _ => None,
    }
}

/// Validates the snow estimate of a site against its stations over the seasons starting
/// in `first..=last` (e.g. 2020..=2025 for winters 2020-2021 to 2025-2026), and writes
/// `data/out/<site>/snow/validation.json`.
pub fn validate(
    site: &Site,
    layout: &DataLayout,
    dl: &Downloader,
    first: i32,
    last: i32,
) -> Result<Validation> {
    let t0 = Instant::now();
    let defs: Vec<StationDef> = STATIONS
        .iter()
        .copied()
        .filter(|d| d.site == site.id)
        .collect();
    if defs.is_empty() {
        return Err(Error::Missing(format!(
            "no validation station for {}",
            site.id
        )));
    }
    let series = stations::fetch(dl, &defs)?;
    let all_massifs = massifs::fetch(dl)?;

    // Terrain cell and massif of each station.
    let mut cells = Vec::new();
    for d in &defs {
        let s = series
            .get(d.id)
            .ok_or_else(|| Error::Missing(format!("station {} not in the hourly files", d.id)))?;
        let (x, y) = to_lambert93(LonLat::new(s.lon, s.lat))?;
        let t = snow_terrain(&Extent::around(x, y, PATCH_HALF_SIDE_M), dl, &all_massifs)?;
        let (alt, slope, aspect, code) = t
            .at(x, y)
            .ok_or_else(|| Error::Missing(format!("station {} outside its patch", d.id)))?;
        let poly = massifs::massif_at(&all_massifs, x, y)
            .ok_or_else(|| Error::Missing(format!("station {} outside every massif", d.id)))?;
        if poly.code != code {
            return Err(Error::Missing(format!(
                "station {}: cell massif {code} differs from point massif {}",
                d.id, poly.code
            )));
        }
        cells.push((d, s, alt, slope, aspect, poly.code, poly.name.clone()));
    }

    // Simulation of the needed massifs over the seasons.
    let mut codes: Vec<u16> = cells.iter().map(|c| c.5).collect();
    codes.sort_unstable();
    codes.dedup();
    let current_year = chrono::Utc::now().year();
    let mut sim = Simulation::default();
    for year in first..=last + 1 {
        let gz = fetch_year(dl, year, year == current_year)?;
        let keep = |d: NaiveDate| season_of(d).is_some_and(|s| (first..=last).contains(&s));
        sim.extend(Simulation::parse_gz(&gz, &codes, keep)?);
    }

    let mut out = Vec::new();
    let mut pooled: BTreeMap<u32, PairLists> = BTreeMap::new();
    for (d, s, alt, slope, aspect, code, name) in cells {
        let mut all = Vec::new();
        let mut flat_all = Vec::new();
        let mut by_month: BTreeMap<u32, PairLists> = BTreeMap::new();
        let mut by_season: BTreeMap<i32, PairLists> = BTreeMap::new();
        let (mut inside, mut width) = (0usize, 0.0f64);
        let mut presence = 0usize;
        let mut confusion = vec![[[0usize; 3]; 3]; PRACTICES.len()];
        let mut period: Option<[NaiveDate; 2]> = None;
        for (&date, &obs) in &s.depth_cm {
            let Some(season) = season_of(date) else {
                continue;
            };
            if !(first..=last).contains(&season) {
                continue;
            }
            let Some(day) = sim.get(code, date) else {
                continue;
            };
            let e = estimate_cell(day, alt, slope, aspect);
            let flat = day.depth(s.alti as f32, 0.0, -1.0);
            if e.depth.is_nan() || flat.is_nan() {
                continue;
            }
            period = Some(match period {
                None => [date, date],
                Some([a, _]) => [a, date],
            });
            all.push((e.depth, obs));
            flat_all.push((flat, obs));
            let pm = pooled.entry(date.month()).or_default();
            pm.0.push((e.depth, obs));
            pm.1.push((flat, obs));
            let m = by_month.entry(date.month()).or_default();
            m.0.push((e.depth, obs));
            m.1.push((flat, obs));
            let ss = by_season.entry(season).or_default();
            ss.0.push((e.depth, obs));
            ss.1.push((flat, obs));
            if e.low <= obs && obs <= e.high {
                inside += 1;
            }
            width += f64::from(e.high - e.low);
            if (obs >= PRESENCE_CM) == (e.depth >= PRESENCE_CM) {
                presence += 1;
            }
            for (k, p) in PRACTICES.iter().enumerate() {
                if let (Some(co), Some(ce)) = (p.class(obs), p.class(e.depth)) {
                    confusion[k][usize::from(co)][usize::from(ce)] += 1;
                }
            }
        }
        let n = all.len();
        let Some(period) = period else {
            return Err(Error::Missing(format!(
                "no overlapping day for station {}",
                d.id
            )));
        };
        let months = month_stats(by_month);
        out.push(StationValidation {
            id: d.id.into(),
            name: s.name.clone(),
            lon_lat: [s.lon, s.lat],
            station_alti_m: s.alti,
            massif: code,
            massif_name: name,
            cell_alti_m: alt,
            cell_slope_deg: slope,
            cell_aspect_deg: aspect,
            period,
            product: stats(&all),
            flat: stats(&flat_all),
            by_month: months,
            by_season: by_season
                .into_iter()
                .map(|(y, (p, f))| SeasonStats {
                    season: format!("{y}-{}", y + 1),
                    product: stats(&p),
                    flat: stats(&f),
                })
                .collect(),
            range_coverage: inside as f64 / n as f64,
            mean_range_width_cm: width / n as f64,
            presence_agreement: presence as f64 / n as f64,
            classes: PRACTICES
                .iter()
                .zip(&confusion)
                .map(|(p, c): (&Practice, &[[usize; 3]; 3])| {
                    let total: usize = c.iter().flatten().sum();
                    let diag: usize = (0..3).map(|i| c[i][i]).sum();
                    ClassAgreement {
                        practice: p.id.into(),
                        thresholds_cm: p.thresholds_cm,
                        accuracy: diag as f64 / total.max(1) as f64,
                        confusion: *c,
                    }
                })
                .collect(),
        });
    }
    let pooled_product: Vec<(f32, f32)> = pooled
        .values()
        .flat_map(|(p, _)| p.iter().copied())
        .collect();
    let pooled_flat: Vec<(f32, f32)> = pooled
        .values()
        .flat_map(|(_, f)| f.iter().copied())
        .collect();
    let v = Validation {
        site: site.id.into(),
        seasons: [first, last],
        site_product: stats(&pooled_product),
        site_flat: stats(&pooled_flat),
        site_by_month: month_stats(pooled),
        stations: out,
        seconds: t0.elapsed().as_secs_f64(),
    };
    let dir = layout.layer_dir(site, crate::LAYER)?;
    DataLayout::ensure(&dir)?;
    let path = dir.join("validation.json");
    std::fs::write(&path, serde_json::to_vec_pretty(&v)?).map_err(|e| Error::io(&path, e))?;
    Ok(v)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn statistics() {
        let s = stats(&[(10.0, 0.0), (0.0, 10.0), (5.0, 5.0)]);
        assert_eq!(s.n, 3);
        assert!(s.bias_cm.abs() < 1e-12);
        assert!((s.mae_cm - 20.0 / 3.0).abs() < 1e-12);
        assert!((s.rmse_cm - (200.0f64 / 3.0).sqrt()).abs() < 1e-12);
        assert_eq!(stats(&[]).n, 0);
    }

    #[test]
    fn seasons() {
        let d = |y, m, dd| NaiveDate::from_ymd_opt(y, m, dd).unwrap();
        assert_eq!(season_of(d(2025, 11, 1)), Some(2025));
        assert_eq!(season_of(d(2026, 5, 31)), Some(2025));
        assert_eq!(season_of(d(2026, 6, 1)), None);
        assert_eq!(season_of(d(2026, 10, 31)), None);
    }
}
