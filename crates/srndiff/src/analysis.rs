//! Measurements of a profile in one direction of travel: slope distribution, steep
//! passages (climbs, descents, side-hill traverses), forest descents, flats, distance to
//! an exit, network, avalanche exposure, zones, altitude and aspect.
//!
//! The angle of the track is the elevation change over a centred window of
//! [`Params::sustain_m`](crate::rules::Params::sustain_m) metres, on the elevation
//! smoothed over three samples (30 m): a GPS track wanders across the fall line by a few
//! metres, and a single 10 m step on a 5 m terrain model is noisy.

use serde::{Deserialize, Serialize};

use crate::profile::{Profile, Sample};

/// Direction of travel.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Direction {
    /// In the order of the track.
    Forward,
    /// Reversed.
    Reverse,
}

/// Kind of a passage.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PassageKind {
    /// Climb.
    Climb,
    /// Descent.
    Descent,
    /// Traverse across a steep slope (side hill).
    Sidehill,
}

/// A steep passage, in the coordinates of the direction of travel.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Passage {
    /// Kind.
    pub kind: PassageKind,
    /// Start, km from the start of the direction.
    pub start_km: f64,
    /// Length, metres.
    pub length_m: f64,
    /// Mean angle of the track over the passage, degrees (absolute).
    pub mean_deg: f32,
    /// Steepest angle over the sustain window, degrees (absolute).
    pub max_deg: f32,
    /// Steepest terrain slope under the passage, degrees.
    pub terrain_max_deg: f32,
    /// Length in dense forest without an opening, metres.
    pub dense_forest_m: f64,
    /// Highest avalanche exposure class under the passage.
    pub exposure_max: Option<u8>,
    /// Mean aspect of the terrain (circular), degrees.
    pub aspect_deg: Option<f32>,
    /// Elevation at the start, metres.
    pub z_start: Option<f32>,
    /// Position of the start.
    pub lon: f64,
    /// Position of the start.
    pub lat: f64,
}

/// A stretch of the track (no network, exposure, zone, flat...).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Stretch {
    /// Start, km.
    pub start_km: f64,
    /// Length, metres.
    pub length_m: f64,
    /// Label (zone names, exposure class).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub label: String,
}

/// Length of track per angle band, km: index `i` covers `[BANDS[i], BANDS[i+1])`.
pub const BANDS: [f32; 8] = [3.0, 8.0, 12.0, 15.0, 20.0, 25.0, 30.0, 90.0];

/// Measurements in one direction.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Metrics {
    /// Direction.
    pub direction: Direction,
    /// Length, km.
    pub length_km: f64,
    /// Ascent, metres.
    pub ascent_m: f64,
    /// Descent, metres.
    pub descent_m: f64,
    /// Lowest and highest elevation, metres.
    pub z_min: Option<f32>,
    /// Highest elevation, metres.
    pub z_max: Option<f32>,
    /// Share of the length without elevation (outside the terrain model).
    pub unknown_elevation_share: f64,
    /// Munter time (rate 4 uphill and flat, 10 downhill), hours.
    pub munter_h: f64,
    /// Share of the length on coarse segments (straight lines between distant points).
    pub coarse_share: f64,
    /// Flat (|angle| < 3°), km.
    pub flat_km: f64,
    /// Longest flat stretch, km.
    pub longest_flat_km: f64,
    /// Climbing km per band of [`BANDS`] (from 3°).
    pub climb_km: Vec<f64>,
    /// Descending km per band of [`BANDS`].
    pub descent_km: Vec<f64>,
    /// Steepest sustained climb, degrees.
    pub max_climb_deg: f32,
    /// Steepest sustained descent, degrees.
    pub max_descent_deg: f32,
    /// Steepest terrain slope under the track, degrees (median of 3 samples).
    pub max_terrain_deg: f32,
    /// Climbs of at least 8°.
    pub climbs: Vec<Passage>,
    /// Descents of at least 8°.
    pub descents: Vec<Passage>,
    /// Side-hill traverses on terrain of at least 20°.
    pub sidehills: Vec<Passage>,
    /// Descending km in dense forest without an opening (of at least 8°).
    pub forest_descent_km: f64,
    /// Largest distance along the track to the nearest exit, km, and where.
    pub max_exit_km: f64,
    /// Where the largest distance to an exit is, km.
    pub max_exit_at_km: f64,
    /// Without any mobile network, km (`None`: not analysed).
    pub no_network_km: Option<f64>,
    /// Longest stretch without network.
    pub no_network: Vec<Stretch>,
    /// Network not analysed, km.
    pub network_unknown_km: f64,
    /// Stretches « exposé » or « très exposé ».
    pub exposed: Vec<Stretch>,
    /// Stretches « très exposé » (gaps under 50 m merged).
    pub high_exposed: Vec<Stretch>,
    /// « exposé » km.
    pub exposed_km: f64,
    /// « très exposé » km.
    pub high_exposed_km: f64,
    /// Exposure not evaluated, km.
    pub exposure_unknown_km: f64,
    /// Steepest terrain where the exposure is unknown, degrees.
    pub exposure_unknown_max_terrain_deg: f32,
    /// Stretches in a zone with an active restriction.
    pub zones: Vec<Stretch>,
    /// Access forbidden, km.
    pub forbidden_km: f64,
    /// Leaving the marked routes forbidden, km.
    pub leave_routes_forbidden_km: f64,
    /// Zones to avoid, km.
    pub avoid_km: f64,
    /// Zones not known, km.
    pub zones_unknown_km: f64,
    /// Open terrain (canopy cover below 10 %), km; `None` without canopy.
    pub open_km: Option<f64>,
    /// Longest open stretch, km.
    pub longest_open_km: Option<f64>,
}

/// Parameters of the measurement (subset of the rule parameters).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Measure {
    /// Window of the sustained angle, metres.
    pub sustain_m: f64,
    /// Forest cover from which a cell is forest, percent.
    pub forest_cover_pct: f32,
    /// A canopy lower than this within 10 m is an opening, metres.
    pub opening_h: f32,
    /// Minimum terrain slope of a side-hill traverse, degrees.
    pub sidehill_deg: f32,
    /// Track segments longer than this are straight lines between distant points, not
    /// the path skied: the elevation is interpolated between their ends and no side-hill
    /// is read on them, metres.
    pub coarse_m: f64,
}

fn angle(g: f64) -> f32 {
    g.atan().to_degrees() as f32
}

/// Elevation filled by linear interpolation over gaps (ends: nearest known value), then
/// smoothed by a centred mean of 3 samples. `None` when no sample has an elevation.
fn elevations(samples: &[Sample]) -> Option<Vec<f64>> {
    let known: Vec<usize> = (0..samples.len())
        .filter(|i| samples[*i].z.is_some())
        .collect();
    let (&first, &last) = (known.first()?, known.last()?);
    let mut z = vec![0.0; samples.len()];
    for (i, zi) in z.iter_mut().enumerate().take(first + 1) {
        let _ = i;
        *zi = f64::from(samples[first].z.unwrap_or_default());
    }
    for w in known.windows(2) {
        let (a, b) = (w[0], w[1]);
        let (za, zb) = (
            f64::from(samples[a].z.unwrap_or_default()),
            f64::from(samples[b].z.unwrap_or_default()),
        );
        for (k, zk) in z.iter_mut().enumerate().take(b + 1).skip(a) {
            let t = if b > a {
                (k - a) as f64 / (b - a) as f64
            } else {
                0.0
            };
            *zk = za + t * (zb - za);
        }
    }
    for zk in z.iter_mut().skip(last) {
        *zk = f64::from(samples[last].z.unwrap_or_default());
    }
    let n = z.len();
    Some(
        (0..n)
            .map(|i| {
                let lo = i.saturating_sub(1);
                let hi = (i + 1).min(n - 1);
                z[lo..=hi].iter().sum::<f64>() / (hi - lo + 1) as f64
            })
            .collect(),
    )
}

fn circular_mean(v: impl Iterator<Item = f32>) -> Option<f32> {
    let (mut sx, mut sy, mut n) = (0.0f64, 0.0f64, 0);
    for a in v.filter(|a| *a >= 0.0) {
        let r = f64::from(a).to_radians();
        sx += r.sin();
        sy += r.cos();
        n += 1;
    }
    (n > 0).then(|| (sx.atan2(sy).to_degrees().rem_euclid(360.0)) as f32)
}

fn median3(v: &[Option<f32>], i: usize) -> Option<f32> {
    let lo = i.saturating_sub(1);
    let hi = (i + 1).min(v.len() - 1);
    let mut w: Vec<f32> = v[lo..=hi].iter().flatten().copied().collect();
    if w.is_empty() {
        return None;
    }
    w.sort_by(f32::total_cmp);
    Some(w[w.len() / 2])
}

/// Runs of consecutive indices where `pred` holds, merging gaps shorter than `gap`
/// samples; returns `(first, last)` inclusive.
fn runs(n: usize, gap: usize, pred: impl Fn(usize) -> bool) -> Vec<(usize, usize)> {
    let mut out: Vec<(usize, usize)> = Vec::new();
    for i in 0..n {
        if !pred(i) {
            continue;
        }
        match out.last_mut() {
            Some((_, e)) if i - *e <= gap + 1 => *e = i,
            _ => out.push((i, i)),
        }
    }
    out
}

/// Measures a profile in one direction.
pub fn measure(profile: &Profile, direction: Direction, m: &Measure) -> Metrics {
    let mut samples: Vec<Sample> = profile.samples.clone();
    if direction == Direction::Reverse {
        samples.reverse();
        let total = samples.first().map_or(0.0, |s| s.s);
        for s in &mut samples {
            s.s = total - s.s;
        }
    }
    let n = samples.len();
    let step = profile.step_m.max(1.0);
    let seg = |i: usize| -> f64 {
        // Length represented by sample i (half of each neighbouring step).
        let a = if i > 0 {
            samples[i].s - samples[i - 1].s
        } else {
            0.0
        };
        let b = if i + 1 < n {
            samples[i + 1].s - samples[i].s
        } else {
            0.0
        };
        (a + b) / 2.0
    };
    let length_m = samples.last().map_or(0.0, |s| s.s);
    let km_at = |i: usize| samples[i].s / 1000.0;
    let mut z = elevations(&samples).unwrap_or_else(|| vec![0.0; n]);
    // Coarse segments: elevation interpolated between the ends of the segment.
    let coarse = |i: usize| f64::from(samples[i].seg_len) > m.coarse_m;
    let mut i = 0;
    while i < n {
        let mut j = i;
        while j + 1 < n && samples[j + 1].seg == samples[i].seg {
            j += 1;
        }
        if coarse(i) && j > i {
            let (za, zb) = (z[i], z[j]);
            let span = samples[j].s - samples[i].s;
            for k in i + 1..j {
                let t = (samples[k].s - samples[i].s) / span;
                z[k] = za + t * (zb - za);
            }
        }
        i = j + 1;
    }
    let coarse_m: f64 = (0..n).filter(|i| coarse(*i)).map(seg).sum();
    let has_z = samples.iter().any(|s| s.z.is_some());
    let unknown_z: f64 = (0..n).filter(|i| samples[*i].z.is_none()).map(seg).sum();

    // Sustained angle over a centred window.
    let k = ((m.sustain_m / step / 2.0).round() as usize).max(1);
    let ang: Vec<f32> = (0..n)
        .map(|i| {
            let (a, b) = (i.saturating_sub(k), (i + k).min(n - 1));
            let ds = samples[b].s - samples[a].s;
            if ds > 0.0 && has_z {
                angle((z[b] - z[a]) / ds)
            } else {
                0.0
            }
        })
        .collect();
    // Angle over 50 m for the distribution and the flats.
    let k50 = ((25.0 / step).round() as usize).max(1);
    let ang50: Vec<f32> = (0..n)
        .map(|i| {
            let (a, b) = (i.saturating_sub(k50), (i + k50).min(n - 1));
            let ds = samples[b].s - samples[a].s;
            if ds > 0.0 && has_z {
                angle((z[b] - z[a]) / ds)
            } else {
                0.0
            }
        })
        .collect();

    let (mut ascent, mut descent, mut munter_s) = (0.0, 0.0, 0.0);
    // Ascent and descent on the smoothed elevation, Munter time on 100 m pieces.
    for w in z.windows(2) {
        let d = w[1] - w[0];
        if d > 0.0 {
            ascent += d;
        } else {
            descent -= d;
        }
    }
    let piece = ((100.0 / step).round() as usize).max(1);
    let mut i = 0;
    while i + 1 < n {
        let j = (i + piece).min(n - 1);
        let dx = samples[j].s - samples[i].s;
        let dz = z[j] - z[i];
        let units = dx / 1000.0 + dz.abs() / 100.0;
        let rate = if dx > 0.0 && dz / dx < -0.02 {
            10.0
        } else {
            4.0
        };
        munter_s += units / rate * 3600.0;
        i = j;
    }

    let mut climb_km = vec![0.0; BANDS.len() - 1];
    let mut descent_km = vec![0.0; BANDS.len() - 1];
    let mut flat_km = 0.0;
    for (i, a) in ang50.iter().enumerate() {
        let l = seg(i) / 1000.0;
        if a.abs() < BANDS[0] {
            flat_km += l;
            continue;
        }
        let b = BANDS
            .windows(2)
            .position(|w| a.abs() >= w[0] && a.abs() < w[1]);
        if let Some(b) = b {
            if *a > 0.0 {
                climb_km[b] += l;
            } else {
                descent_km[b] += l;
            }
        }
    }
    let longest_flat_km = runs(n, 2, |i| ang50[i].abs() < BANDS[0])
        .iter()
        .map(|(a, b)| (samples[*b].s - samples[*a].s) / 1000.0)
        .fold(0.0, f64::max);

    let slope_raw: Vec<Option<f32>> = samples.iter().map(|s| s.slope).collect();
    let slope_m: Vec<Option<f32>> = (0..n).map(|i| median3(&slope_raw, i)).collect();
    let dense = |i: usize| {
        let s = &samples[i];
        s.cover.is_some_and(|c| c >= m.forest_cover_pct)
            && s.min_h.is_some_and(|h| h >= m.opening_h)
    };
    let passage = |kind: PassageKind, (a, b): (usize, usize), sign: f32| -> Passage {
        let idx = a..=b;
        let len = samples[b].s - samples[a].s + step;
        let mean = if samples[b].s > samples[a].s {
            angle((z[b] - z[a]).abs() / (samples[b].s - samples[a].s))
        } else {
            ang[a].abs()
        };
        Passage {
            kind,
            start_km: km_at(a),
            length_m: len,
            mean_deg: if kind == PassageKind::Sidehill {
                idx.clone().map(|i| ang[i].abs()).sum::<f32>() / (b - a + 1) as f32
            } else {
                mean
            },
            max_deg: idx
                .clone()
                .map(|i| (ang[i] * sign).max(0.0))
                .fold(0.0, f32::max)
                .max(if kind == PassageKind::Sidehill {
                    0.0
                } else {
                    mean
                }),
            terrain_max_deg: idx.clone().filter_map(|i| slope_m[i]).fold(0.0, f32::max),
            dense_forest_m: idx.clone().filter(|i| dense(*i)).map(seg).sum(),
            exposure_max: idx.clone().filter_map(|i| samples[i].exposure).max(),
            aspect_deg: circular_mean(idx.clone().filter_map(|i| samples[i].aspect)),
            z_start: samples[a].z,
            lon: samples[a].lon,
            lat: samples[a].lat,
        }
    };
    let min_len = (m.sustain_m / step).ceil() as usize;
    let gap = ((30.0 / step).round() as usize).max(1);
    let collect = |kind: PassageKind, sign: f32| -> Vec<Passage> {
        runs(n, gap, |i| ang[i] * sign >= BANDS[1])
            .into_iter()
            .filter(|(a, b)| b - a + 1 >= min_len.max(2))
            .map(|r| passage(kind, r, sign))
            .collect()
    };
    let climbs = collect(PassageKind::Climb, 1.0);
    let descents = collect(PassageKind::Descent, -1.0);
    let sidehills: Vec<Passage> = runs(n, gap, |i| {
        !coarse(i) && slope_m[i].is_some_and(|s| s >= m.sidehill_deg && s - ang[i].abs() >= 10.0)
    })
    .into_iter()
    .filter(|(a, b)| b - a + 1 >= min_len.max(2))
    .map(|r| passage(PassageKind::Sidehill, r, 1.0))
    .collect();
    let forest_descent_km = (0..n)
        .filter(|i| ang[*i] <= -BANDS[1] && dense(*i))
        .map(seg)
        .sum::<f64>()
        / 1000.0;

    // Distance along the track to the nearest exit.
    let mut to_exit = vec![f64::INFINITY; n];
    let mut last: Option<f64> = None;
    for i in 0..n {
        if samples[i].exit {
            last = Some(samples[i].s);
        }
        if let Some(l) = last {
            to_exit[i] = samples[i].s - l;
        }
    }
    last = None;
    for i in (0..n).rev() {
        if samples[i].exit {
            last = Some(samples[i].s);
        }
        if let Some(l) = last {
            to_exit[i] = to_exit[i].min(l - samples[i].s);
        }
    }
    let (mut max_exit, mut max_exit_at) = (0.0, 0.0);
    for (i, d) in to_exit.iter().enumerate() {
        if d.is_finite() && *d > max_exit {
            max_exit = *d;
            max_exit_at = km_at(i);
        }
    }

    let stretch = |(a, b): (usize, usize), label: String| Stretch {
        start_km: km_at(a),
        length_m: samples[b].s - samples[a].s + step,
        label,
    };
    let net_known = samples.iter().any(|s| s.no_network.is_some());
    let no_network_km = net_known.then(|| {
        (0..n)
            .filter(|i| samples[*i].no_network == Some(true))
            .map(seg)
            .sum::<f64>()
            / 1000.0
    });
    let mut no_network: Vec<Stretch> = runs(n, 1, |i| samples[i].no_network == Some(true))
        .into_iter()
        .map(|r| stretch(r, String::new()))
        .collect();
    no_network.sort_by(|a, b| b.length_m.total_cmp(&a.length_m));
    let network_unknown_km = if profile.inputs.network {
        (0..n)
            .filter(|i| samples[*i].no_network.is_none())
            .map(seg)
            .sum::<f64>()
            / 1000.0
    } else {
        length_m / 1000.0
    };

    // Gaps of up to 50 m inside a « très exposé » stretch are merged (cells along a
    // track a few metres off the path skied).
    let high_exposed: Vec<Stretch> = runs(n, 5, |i| samples[i].exposure == Some(3))
        .into_iter()
        .map(|r| stretch(r, "très exposé".to_owned()))
        .collect();
    let exposed: Vec<Stretch> = runs(n, 0, |i| samples[i].exposure.is_some_and(|c| c >= 2))
        .into_iter()
        .map(|(a, b)| {
            let high = (a..=b).any(|i| samples[i].exposure == Some(3));
            stretch(
                (a, b),
                if high { "très exposé" } else { "exposé" }.to_owned(),
            )
        })
        .collect();
    let exposed_km = (0..n)
        .filter(|i| samples[*i].exposure == Some(2))
        .map(seg)
        .sum::<f64>()
        / 1000.0;
    let high_exposed_km = (0..n)
        .filter(|i| samples[*i].exposure == Some(3))
        .map(seg)
        .sum::<f64>()
        / 1000.0;
    let unknown_exp: Vec<usize> = (0..n)
        .filter(|i| !profile.inputs.exposure || samples[*i].exposure.is_none())
        .collect();
    let exposure_unknown_km = unknown_exp.iter().map(|i| seg(*i)).sum::<f64>() / 1000.0;
    let exposure_unknown_max_terrain_deg = unknown_exp
        .iter()
        .filter_map(|i| slope_m[*i])
        .fold(0.0, f32::max);

    let zone_label = |i: usize| -> String {
        samples[i]
            .zone
            .as_ref()
            .map(|z| z.names.join(", "))
            .unwrap_or_default()
    };
    let zones: Vec<Stretch> = runs(n, 1, |i| samples[i].zone.as_ref().is_some_and(|z| z.any()))
        .into_iter()
        .map(|r| stretch(r, zone_label(r.0)))
        .collect();
    let zkm = |f: &dyn Fn(&crate::field::ZoneFlags) -> bool| {
        (0..n)
            .filter(|i| samples[*i].zone.as_ref().is_some_and(f))
            .map(seg)
            .sum::<f64>()
            / 1000.0
    };
    let forbidden_km = zkm(&|z| z.access_forbidden);
    let leave_routes_forbidden_km = zkm(&|z| z.leave_routes_forbidden);
    let avoid_km = zkm(&|z| z.avoid);
    let zones_unknown_km = (0..n)
        .filter(|i| !profile.inputs.zones || samples[*i].zone.is_none())
        .map(seg)
        .sum::<f64>()
        / 1000.0;

    let canopy = samples.iter().any(|s| s.cover.is_some());
    let open = |i: usize| samples[i].cover.is_some_and(|c| c < 10.0);
    let open_km = canopy.then(|| (0..n).filter(|i| open(*i)).map(seg).sum::<f64>() / 1000.0);
    let longest_open_km = canopy.then(|| {
        runs(n, 2, open)
            .iter()
            .map(|(a, b)| (samples[*b].s - samples[*a].s) / 1000.0)
            .fold(0.0, f64::max)
    });

    Metrics {
        direction,
        length_km: length_m / 1000.0,
        ascent_m: if has_z { ascent } else { 0.0 },
        descent_m: if has_z { descent } else { 0.0 },
        z_min: samples.iter().filter_map(|s| s.z).reduce(f32::min),
        z_max: samples.iter().filter_map(|s| s.z).reduce(f32::max),
        unknown_elevation_share: if length_m > 0.0 {
            unknown_z / length_m
        } else {
            1.0
        },
        munter_h: munter_s / 3600.0,
        coarse_share: if length_m > 0.0 {
            coarse_m / length_m
        } else {
            0.0
        },
        flat_km,
        longest_flat_km,
        climb_km,
        descent_km,
        max_climb_deg: ang.iter().copied().fold(0.0, f32::max),
        max_descent_deg: ang.iter().map(|a| -a).fold(0.0, f32::max),
        max_terrain_deg: slope_m.iter().flatten().copied().fold(0.0, f32::max),
        climbs,
        descents,
        sidehills,
        forest_descent_km,
        max_exit_km: max_exit / 1000.0,
        max_exit_at_km: max_exit_at,
        no_network_km,
        no_network,
        network_unknown_km,
        exposed,
        high_exposed,
        exposed_km,
        high_exposed_km,
        exposure_unknown_km,
        exposure_unknown_max_terrain_deg,
        zones,
        forbidden_km,
        leave_routes_forbidden_km,
        avoid_km,
        zones_unknown_km,
        open_km,
        longest_open_km,
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::profile::Inputs;

    /// Straight profile from a list of (length m, angle °) pieces.
    pub(crate) fn synthetic(pieces: &[(f64, f64)]) -> Profile {
        let mut samples = Vec::new();
        let (mut s, mut z) = (0.0, 1000.0);
        samples.push(Sample {
            s,
            z: Some(z as f32),
            slope: Some(0.0),
            ..Sample::default()
        });
        for (len, deg) in pieces {
            let steps = (len / 10.0).round() as usize;
            for _ in 0..steps {
                s += 10.0;
                z += 10.0 * deg.to_radians().tan();
                samples.push(Sample {
                    s,
                    x: s,
                    z: Some(z as f32),
                    slope: Some(deg.abs() as f32),
                    ..Sample::default()
                });
            }
        }
        samples.first_mut().unwrap().exit = true;
        samples.last_mut().unwrap().exit = true;
        Profile {
            step_m: 10.0,
            samples,
            inputs: Inputs::default(),
        }
    }

    const M: Measure = Measure {
        sustain_m: 30.0,
        forest_cover_pct: 50.0,
        opening_h: 2.0,
        sidehill_deg: 20.0,
        coarse_m: 60.0,
    };

    #[test]
    fn climb_then_descent_both_directions() {
        // 1 km flat, 300 m climbing at 18°, 2 km flat, 200 m descending at 22°.
        let p = synthetic(&[(1000.0, 0.0), (300.0, 18.0), (2000.0, 0.0), (200.0, -22.0)]);
        let f = measure(&p, Direction::Forward, &M);
        assert!((f.length_km - 3.5).abs() < 0.01);
        assert!((f.ascent_m - 300.0 * 18f64.to_radians().tan()).abs() < 2.0);
        assert_eq!(f.climbs.len(), 1);
        assert_eq!(f.descents.len(), 1);
        assert!((f.max_climb_deg - 18.0).abs() < 0.6, "{}", f.max_climb_deg);
        assert!(
            (f.max_descent_deg - 22.0).abs() < 0.6,
            "{}",
            f.max_descent_deg
        );
        assert!((f.climbs[0].start_km - 1.0).abs() < 0.05);
        let r = measure(&p, Direction::Reverse, &M);
        assert!((r.max_climb_deg - 22.0).abs() < 0.6);
        assert!((r.max_descent_deg - 18.0).abs() < 0.6);
        assert!(
            (r.climbs[0].start_km - 0.0).abs() < 0.05,
            "{}",
            r.climbs[0].start_km
        );
        assert!(f.longest_flat_km > 1.9 && f.flat_km > 2.9);
        // Exits at both ends: farthest point 1.75 km in the middle of 3.5 km.
        assert!((f.max_exit_km - 1.75).abs() < 0.02, "{}", f.max_exit_km);
        // Munter: 3.5 km + 97.5 m up / 4, 0.2 km + 80.8 m down / 10.
        assert!(f.munter_h > 1.0 && f.munter_h < 1.4, "{}", f.munter_h);
    }

    #[test]
    fn short_bumps_are_not_passages() {
        // A 10 m step at 30° inside a flat: shorter than the 30 m window.
        let p = synthetic(&[(500.0, 0.0), (10.0, 30.0), (500.0, 0.0)]);
        let f = measure(&p, Direction::Forward, &M);
        assert!(f.max_climb_deg < 15.0, "{}", f.max_climb_deg);
    }

    #[test]
    fn coarse_segments_are_interpolated() {
        // A straight 300 m segment across a 20 m deep gully with 22° sides.
        let mut p = synthetic(&[(100.0, 0.0), (50.0, -22.0), (50.0, 22.0), (100.0, 0.0)]);
        for s in &mut p.samples {
            s.seg = 0;
            s.seg_len = 300.0;
        }
        let window = Measure {
            coarse_m: f64::INFINITY,
            ..M
        };
        let f = measure(&p, Direction::Forward, &window);
        assert!(f.max_descent_deg > 18.0 && f.climbs.len() == 1);
        assert!(f.coarse_share == 0.0);
        let f = measure(&p, Direction::Forward, &M);
        assert!(f.max_descent_deg < 1.0 && f.climbs.is_empty() && f.descents.is_empty());
        assert!(f.ascent_m < 1.0, "{}", f.ascent_m);
        assert!(f.coarse_share > 0.99);
    }

    #[test]
    fn sidehill_detected_on_steep_terrain_with_flat_track() {
        let mut p = synthetic(&[(400.0, 0.0)]);
        for s in &mut p.samples[10..20] {
            s.slope = Some(27.0);
        }
        let f = measure(&p, Direction::Forward, &M);
        assert_eq!(f.sidehills.len(), 1);
        assert!(f.sidehills[0].terrain_max_deg >= 27.0);
        assert!((f.sidehills[0].length_m - 100.0).abs() < 11.0);
    }
}
