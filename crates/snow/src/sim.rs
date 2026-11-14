//! Météo-France "Données de Modèle de Simulation Nivologique" (data.gouv, Licence Ouverte).
//!
//! One gzipped CSV per year, updated daily for the current year (the last date is J-1):
//! `num_massif;dat;altitude;exposition;pente;neigetot;equivalent_eau;ecoulement`.
//! Per massif and day: altitude bands every 300 m, 17 (aspect, slope) combinations per
//! band: flat (`exposition = -1`, `pente = 0`) and the 8 aspects 0, 45, …, 315° at 20° and
//! 40°. `neigetot` is the total snow depth in cm (descriptif des champs, 2025-06-25). The
//! state is at 06 UTC of `dat` (`ecoulement` is accumulated over ]J-1 06 h, J 06 h]).
//!
//! [`MassifDay::depth`] projects a band table onto a pixel: linear in altitude between
//! bands, linear in aspect between the two neighbouring octants, linear in slope between
//! flat, 20° and 40° (clamped at 40°). [`MassifDay::depth_range`] gives the minimum and
//! maximum of the same function over altitude ± 150 m, slope ± 10° and aspect ± 45°: the
//! sensitivity of the simulation to the local terrain, not the model error.

use std::collections::BTreeMap;
use std::io::{BufRead, BufReader};

use chrono::NaiveDate;
use skimap_common::download::{CachePolicy, Downloader, Request};

use crate::error::{Error, Result};

/// Downloader source (data.gouv resources hosted by Météo-France).
pub const SOURCE: &str = "datagouv";
/// data.gouv dataset page.
pub const DATASET_URL: &str =
    "https://www.data.gouv.fr/datasets/donnees-de-modele-de-simulation-nivologique";
/// (aspect, slope) combinations per altitude band.
pub const N_COMBOS: usize = 17;
/// Half-width of the altitude stencil of [`MassifDay::depth_range`], metres.
pub const RANGE_ALT_M: f32 = 150.0;
/// Half-width of the slope stencil, degrees.
pub const RANGE_SLOPE_DEG: f32 = 10.0;
/// Half-width of the aspect stencil, degrees.
pub const RANGE_ASPECT_DEG: f32 = 45.0;

/// URL of the yearly file.
pub fn year_url(year: i32) -> String {
    format!(
        "https://meteofrance.s3.sbg.io.cloud.ovh.net/data/REF_CC/NIVO/donnees_nivologiques_{year}.csv.gz"
    )
}

/// Downloads (or reads from cache) the gzipped file of a year. The file of the current year
/// changes every day: it is revalidated (conditional request) when `current` is set.
pub fn fetch_year(dl: &Downloader, year: i32, current: bool) -> Result<Vec<u8>> {
    let mut req = Request::get(SOURCE, &year_url(year));
    if current {
        req = req.cache(CachePolicy::Revalidate);
    } else {
        req = req.max_age(std::time::Duration::from_secs(30 * 86_400));
    }
    Ok(dl.fetch(&req)?.bytes()?)
}

/// Index of an (exposition, pente) pair in a band: 0 = flat, `1 + 2k` = octant `k` at 20°,
/// `2 + 2k` = octant `k` at 40°.
pub fn combo_index(exposition: i32, pente: i32) -> Option<usize> {
    if exposition == -1 && pente == 0 {
        return Some(0);
    }
    if !(0..360).contains(&exposition) || exposition % 45 != 0 {
        return None;
    }
    let k = (exposition / 45) as usize;
    match pente {
        20 => Some(1 + 2 * k),
        40 => Some(2 + 2 * k),
        _ => None,
    }
}

/// Snow depths of one massif on one day.
#[derive(Debug, Clone, PartialEq)]
pub struct MassifDay {
    /// Band altitudes, metres, ascending.
    pub bands: Vec<f32>,
    /// Depth (cm) per band and combination ([`combo_index`]); `NaN` if missing.
    pub depth: Vec<[f32; N_COMBOS]>,
}

impl MassifDay {
    fn band_value(&self, b: usize, slope: f32, aspect: f32) -> f32 {
        let v = &self.depth[b];
        let flat = v[0];
        // NaN slope or aspect, zero slope or negative aspect: flat.
        if slope.is_nan() || slope <= 0.0 || aspect.is_nan() || aspect < 0.0 {
            return flat;
        }
        let a = aspect.rem_euclid(360.0) / 45.0;
        let k0 = (a.floor() as usize) % 8;
        let k1 = (k0 + 1) % 8;
        let f = a - a.floor();
        let v20 = (1.0 - f) * v[1 + 2 * k0] + f * v[1 + 2 * k1];
        let v40 = (1.0 - f) * v[2 + 2 * k0] + f * v[2 + 2 * k1];
        let s = slope.min(40.0);
        if s <= 20.0 {
            let t = s / 20.0;
            (1.0 - t) * flat + t * v20
        } else {
            let t = (s - 20.0) / 20.0;
            (1.0 - t) * v20 + t * v40
        }
    }

    /// Snow depth (cm) at an altitude (m), slope (degrees) and aspect (degrees from north,
    /// negative or `NaN` = flat). Altitudes outside the bands are clamped to the nearest
    /// band. `NaN` when the altitude is `NaN` or a needed value is missing.
    pub fn depth(&self, alt: f32, slope: f32, aspect: f32) -> f32 {
        let n = self.bands.len();
        if n == 0 || alt.is_nan() {
            return f32::NAN;
        }
        if alt <= self.bands[0] {
            return self.band_value(0, slope, aspect);
        }
        if alt >= self.bands[n - 1] {
            return self.band_value(n - 1, slope, aspect);
        }
        let i = self.bands.partition_point(|b| *b <= alt) - 1;
        let t = (alt - self.bands[i]) / (self.bands[i + 1] - self.bands[i]);
        (1.0 - t) * self.band_value(i, slope, aspect) + t * self.band_value(i + 1, slope, aspect)
    }

    /// Minimum and maximum of [`MassifDay::depth`] over altitude ± 150 m, slope ± 10°
    /// (≥ 0) and aspect ± 45°. The function is multilinear between the lattice nodes, so
    /// evaluating the box corners and the nodes inside the box gives the exact extremes.
    pub fn depth_range(&self, alt: f32, slope: f32, aspect: f32) -> (f32, f32) {
        if alt.is_nan() || self.bands.is_empty() {
            return (f32::NAN, f32::NAN);
        }
        let mut alts = vec![alt - RANGE_ALT_M, alt + RANGE_ALT_M];
        alts.extend(
            self.bands
                .iter()
                .copied()
                .filter(|b| *b > alt - RANGE_ALT_M && *b < alt + RANGE_ALT_M),
        );
        let s = if slope.is_nan() { 0.0 } else { slope };
        let (s0, s1) = ((s - RANGE_SLOPE_DEG).max(0.0), s + RANGE_SLOPE_DEG);
        let mut slopes = vec![s0, s1];
        slopes.extend([20.0f32, 40.0].into_iter().filter(|n| *n > s0 && *n < s1));
        let aspects: Vec<f32> = if aspect >= 0.0 {
            let (a0, a1) = (aspect - RANGE_ASPECT_DEG, aspect + RANGE_ASPECT_DEG);
            let mut v = vec![a0, a1];
            let first = (a0 / 45.0).floor() as i32 + 1;
            let last = (a1 / 45.0).ceil() as i32 - 1;
            v.extend((first..=last).map(|k| k as f32 * 45.0));
            v
        } else {
            // Flat cell: any aspect once the slope is not zero.
            (0..8).map(|k| k as f32 * 45.0).collect()
        };
        let (mut lo, mut hi) = (f32::INFINITY, f32::NEG_INFINITY);
        for &a in &alts {
            for &sl in &slopes {
                for &asp in &aspects {
                    let v = self.depth(a, sl, asp);
                    if v.is_nan() {
                        return (f32::NAN, f32::NAN);
                    }
                    lo = lo.min(v);
                    hi = hi.max(v);
                }
            }
        }
        (lo, hi)
    }
}

/// Parsed simulation, keyed by (massif, date).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Simulation {
    /// Tables by (massif, date).
    pub days: BTreeMap<(u16, NaiveDate), MassifDay>,
}

fn field<'a>(it: &mut impl Iterator<Item = &'a str>, name: &str) -> Result<&'a str> {
    it.next()
        .ok_or_else(|| Error::parse("simulation CSV", format!("missing field {name}")))
}

fn num<T: std::str::FromStr>(s: &str, name: &str) -> Result<T>
where
    T::Err: std::fmt::Display,
{
    s.trim()
        .parse()
        .map_err(|e| Error::parse("simulation CSV", format!("{name} {s:?}: {e}")))
}

impl Simulation {
    /// Parses a gzipped yearly file, keeping `massifs` and the dates accepted by `keep`.
    pub fn parse_gz(
        gz: &[u8],
        massifs: &[u16],
        keep: impl Fn(NaiveDate) -> bool,
    ) -> Result<Simulation> {
        let reader = BufReader::with_capacity(1 << 20, flate2::read::GzDecoder::new(gz));
        Self::parse_reader(reader, massifs, keep)
    }

    /// Parses CSV text (see [`Simulation::parse_gz`]).
    pub fn parse_reader(
        reader: impl BufRead,
        massifs: &[u16],
        keep: impl Fn(NaiveDate) -> bool,
    ) -> Result<Simulation> {
        // (massif, date) -> altitude -> combos
        let mut acc: BTreeMap<(u16, NaiveDate), BTreeMap<i32, [f32; N_COMBOS]>> = BTreeMap::new();
        let mut date_cache: Option<(u32, NaiveDate, bool)> = None;
        for (i, line) in reader.lines().enumerate() {
            let line = line.map_err(|e| Error::parse("simulation CSV", e))?;
            if i == 0 {
                if !line.starts_with("num_massif;dat;altitude;exposition;pente;neigetot") {
                    return Err(Error::parse(
                        "simulation CSV",
                        format!("unexpected header {line:?}"),
                    ));
                }
                continue;
            }
            if line.is_empty() {
                continue;
            }
            let mut it = line.split(';');
            let m: u16 = num(field(&mut it, "num_massif")?, "num_massif")?;
            if !massifs.contains(&m) {
                continue;
            }
            let dat: u32 = num(field(&mut it, "dat")?, "dat")?;
            let (date, kept) = match date_cache {
                Some((d, nd, k)) if d == dat => (nd, k),
                _ => {
                    let nd =
                        NaiveDate::from_ymd_opt((dat / 10_000) as i32, dat / 100 % 100, dat % 100)
                            .ok_or_else(|| {
                                Error::parse("simulation CSV", format!("bad date {dat}"))
                            })?;
                    let k = keep(nd);
                    date_cache = Some((dat, nd, k));
                    (nd, k)
                }
            };
            if !kept {
                continue;
            }
            let alt: i32 = num(field(&mut it, "altitude")?, "altitude")?;
            let expo: i32 = num(field(&mut it, "exposition")?, "exposition")?;
            let pente: i32 = num(field(&mut it, "pente")?, "pente")?;
            let raw = field(&mut it, "neigetot")?.trim();
            let depth: f32 = if raw.is_empty() {
                f32::NAN
            } else {
                num(raw, "neigetot")?
            };
            let Some(k) = combo_index(expo, pente) else {
                return Err(Error::parse(
                    "simulation CSV",
                    format!("unknown combination exposition={expo} pente={pente}"),
                ));
            };
            acc.entry((m, date))
                .or_default()
                .entry(alt)
                .or_insert([f32::NAN; N_COMBOS])[k] = depth;
        }
        let days = acc
            .into_iter()
            .map(|(key, bands)| {
                let (alts, depth): (Vec<f32>, Vec<[f32; N_COMBOS]>) =
                    bands.into_iter().map(|(a, v)| (a as f32, v)).unzip();
                (key, MassifDay { bands: alts, depth })
            })
            .collect();
        Ok(Simulation { days })
    }

    /// Adds the days of another parse.
    pub fn extend(&mut self, other: Simulation) {
        self.days.extend(other.days);
    }

    /// Table of a massif on a date.
    pub fn get(&self, massif: u16, date: NaiveDate) -> Option<&MassifDay> {
        self.days.get(&(massif, date))
    }

    /// Last date available for every massif of `massifs`.
    pub fn latest_common_date(&self, massifs: &[u16]) -> Option<NaiveDate> {
        massifs
            .iter()
            .map(|m| {
                self.days
                    .keys()
                    .filter(|(mm, _)| mm == m)
                    .map(|(_, d)| *d)
                    .max()
            })
            .collect::<Option<Vec<_>>>()?
            .into_iter()
            .min()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use approx::assert_abs_diff_eq;

    /// Band table where depth = base + 10·k for octant k at 20°, + 100 more at 40°.
    fn table(base: f32) -> [f32; N_COMBOS] {
        let mut v = [0.0; N_COMBOS];
        v[0] = base;
        for k in 0..8 {
            v[1 + 2 * k] = base + 10.0 * k as f32;
            v[2 + 2 * k] = base + 10.0 * k as f32 + 100.0;
        }
        v
    }

    fn day() -> MassifDay {
        MassifDay {
            bands: vec![1200.0, 1500.0, 1800.0],
            depth: vec![table(0.0), table(30.0), table(90.0)],
        }
    }

    #[test]
    fn combos() {
        assert_eq!(combo_index(-1, 0), Some(0));
        assert_eq!(combo_index(0, 20), Some(1));
        assert_eq!(combo_index(0, 40), Some(2));
        assert_eq!(combo_index(315, 40), Some(16));
        assert_eq!(combo_index(30, 20), None);
        assert_eq!(combo_index(45, 30), None);
    }

    #[test]
    fn interpolation() {
        let d = day();
        // Flat, on a band and between bands.
        assert_eq!(d.depth(1500.0, 0.0, -1.0), 30.0);
        assert_abs_diff_eq!(d.depth(1650.0, 0.0, -1.0), 60.0);
        // Clamped outside the bands.
        assert_eq!(d.depth(900.0, 0.0, -1.0), 0.0);
        assert_eq!(d.depth(2500.0, 0.0, -1.0), 90.0);
        // 20° slope facing east (octant 2): base + 20.
        assert_abs_diff_eq!(d.depth(1500.0, 20.0, 90.0), 50.0);
        // 10° slope facing east: halfway between flat (30) and 20° (50).
        assert_abs_diff_eq!(d.depth(1500.0, 10.0, 90.0), 40.0);
        // 30° facing 112.5° (between octants 2 and 3): v20 = 55, v40 = 155 -> 105.
        assert_abs_diff_eq!(d.depth(1500.0, 30.0, 112.5), 105.0);
        // Steeper than 40° is clamped; aspect 337.5 between octant 7 (70) and 0 (0).
        assert_abs_diff_eq!(d.depth(1500.0, 60.0, 337.5), 30.0 + 35.0 + 100.0);
        assert!(d.depth(f32::NAN, 0.0, 0.0).is_nan());
    }

    #[test]
    fn range_contains_value_and_is_exact_on_simple_cases() {
        let d = day();
        let (lo, hi) = d.depth_range(1500.0, 0.0, -1.0);
        // Flat: slope 0..10 any aspect, alt 1350..1650: min = flat at 1350 = 15, max at
        // 1650 slope 10 facing 315 (octant 7): flat 60, v20 = (30+70 + 90+70)/2 = 130 ->
        // (60 + 130) / 2 = 95.
        assert_abs_diff_eq!(lo, 15.0);
        assert_abs_diff_eq!(hi, 95.0);
        for (alt, s, a) in [
            (1400.0, 25.0, 100.0),
            (1790.0, 5.0, 10.0),
            (1210.0, 39.0, 359.0),
        ] {
            let v = d.depth(alt, s, a);
            let (lo, hi) = d.depth_range(alt, s, a);
            assert!(lo <= v && v <= hi, "{lo} {v} {hi}");
            // Brute force over a fine grid of the same box.
            let (mut blo, mut bhi) = (f32::INFINITY, f32::NEG_INFINITY);
            for i in 0..=30 {
                for j in 0..=20 {
                    for k in 0..=90 {
                        let aa = alt - 150.0 + 10.0 * i as f32;
                        let ss = (s - 10.0 + j as f32).max(0.0);
                        let asp = a - 45.0 + k as f32;
                        let x = d.depth(aa, ss, asp);
                        blo = blo.min(x);
                        bhi = bhi.max(x);
                    }
                }
            }
            assert_abs_diff_eq!(lo, blo, epsilon = 1e-3);
            assert_abs_diff_eq!(hi, bhi, epsilon = 1e-3);
        }
    }

    #[test]
    fn parse_csv() {
        let mut csv = String::from(
            "num_massif;dat;altitude;exposition;pente;neigetot;equivalent_eau;ecoulement\n",
        );
        for (m, dat) in [
            (17, 20260115),
            (55, 20260115),
            (17, 20260116),
            (3, 20260115),
        ] {
            for alt in [1500, 1200] {
                csv.push_str(&format!("{m};{dat};{alt};-1;0;{};0;0\n", alt / 100));
                for k in 0..8 {
                    csv.push_str(&format!("{m};{dat};{alt};{};20;{};0;0\n", 45 * k, k));
                    csv.push_str(&format!("{m};{dat};{alt};{};40;{};0;0\n", 45 * k, 10 + k));
                }
            }
        }
        let d16 = NaiveDate::from_ymd_opt(2026, 1, 16).unwrap();
        let sim = Simulation::parse_reader(csv.as_bytes(), &[17, 55], |d| d != d16).unwrap();
        assert_eq!(sim.days.len(), 2);
        let d15 = NaiveDate::from_ymd_opt(2026, 1, 15).unwrap();
        let t = sim.get(17, d15).unwrap();
        assert_eq!(t.bands, vec![1200.0, 1500.0]);
        assert_eq!(t.depth[1][0], 15.0);
        assert_eq!(t.depth[0][combo_index(90, 40).unwrap()], 12.0);
        assert_eq!(sim.latest_common_date(&[17, 55]), Some(d15));
        assert!(sim.get(3, d15).is_none());
        assert!(Simulation::parse_reader("bad header\n".as_bytes(), &[17], |_| true).is_err());
    }
}
