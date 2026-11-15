//! Profile along a track: regular samples (every [`STEP_M`] metres along the line, in the
//! order of the track) with the terrain, the forest, the avalanche exposure, the network
//! and the zones read under each sample.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use skimap_common::crs;
use skimap_common::geom::LonLat;

use crate::field::{Areas, Field, ZoneFlags, ZoneSource};
use crate::{Error, Result};

/// Sampling step along the track, metres (two 5 m cells).
pub const STEP_M: f64 = 10.0;
/// Canopy height from which a 5 m cell counts as tree, metres (same rule as the
/// avalanche crate, `skimap_avalanche::exposure::forest::TREE_HEIGHT_M`).
pub const TREE_HEIGHT_M: f32 = 3.0;
/// Radius around an exit point (road, village) within which a sample is an exit, metres.
pub const EXIT_RADIUS_M: f64 = 60.0;

/// One sample of the profile.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Sample {
    /// Distance from the start of the track along the line, metres.
    pub s: f64,
    /// Longitude, degrees.
    pub lon: f64,
    /// Latitude, degrees.
    pub lat: f64,
    /// Lambert-93 X, metres.
    pub x: f64,
    /// Lambert-93 Y, metres.
    pub y: f64,
    /// Elevation, metres.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub z: Option<f32>,
    /// Terrain slope (fall line), degrees.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub slope: Option<f32>,
    /// Aspect: direction the slope faces, degrees from north, -1 flat.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub aspect: Option<f32>,
    /// Canopy height under the sample, metres.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub canopy_h: Option<f32>,
    /// Canopy cover of the 3 × 3 cells (15 m) around the sample, percent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cover: Option<f32>,
    /// Lowest canopy height of the 5 × 5 cells (25 m) around the sample, metres: a low
    /// value means an opening (track, road, clearing) next to the sample.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub min_h: Option<f32>,
    /// Avalanche exposure class: 1 « terrain peu exposé (modèle) », 2 « exposé »,
    /// 3 « très exposé ».
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub exposure: Option<u8>,
    /// Piste network (0 none, 1 marked, 2 groomed), when known.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub piste: Option<u8>,
    /// Inside a zone without any mobile network (even 112); `None` = not analysed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub no_network: Option<bool>,
    /// Zone restrictions at the date; `None` = no zone data here.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub zone: Option<ZoneFlags>,
    /// Exit: start or end of the track, or within [`EXIT_RADIUS_M`] of a road.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub exit: bool,
    /// Index of the segment of the original track holding the sample.
    #[serde(default)]
    pub seg: u32,
    /// Length of that segment, metres: a long segment is a straight line drawn between
    /// two distant points, not the path actually skied.
    #[serde(default)]
    pub seg_len: f32,
}

/// Which inputs were available when the profile was built (`false`: the factor is
/// reported as not evaluated, never as absent).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Inputs {
    /// Canopy height.
    pub canopy: bool,
    /// Avalanche exposure.
    pub exposure: bool,
    /// Zones without network.
    pub network: bool,
    /// Zones and rules.
    pub zones: bool,
    /// Exits other than the two ends (roads).
    pub roads: bool,
}

/// A sampled track.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Profile {
    /// Sampling step, metres.
    pub step_m: f64,
    /// Samples in the order of the track.
    pub samples: Vec<Sample>,
    /// Inputs used.
    pub inputs: Inputs,
}

/// Layers read under the samples. Only the elevation is required.
#[derive(Default, Clone, Copy)]
pub struct Layers<'a> {
    /// Elevation, metres.
    pub elevation: Option<&'a dyn Field>,
    /// Terrain slope, degrees; computed from the elevation (Horn, 5 m) when absent.
    pub slope: Option<&'a dyn Field>,
    /// Aspect, degrees; computed from the elevation when absent.
    pub aspect: Option<&'a dyn Field>,
    /// Canopy height (LiDAR HD MNH), metres.
    pub canopy_height: Option<&'a dyn Field>,
    /// Avalanche exposure classes 1–3.
    pub exposure: Option<&'a dyn Field>,
    /// Piste network classes.
    pub pistes: Option<&'a dyn Field>,
    /// Zones without any mobile network.
    pub no_network: Option<&'a Areas>,
    /// Zone rules at the date of the outing.
    pub zones: Option<&'a dyn ZoneSource>,
}

/// Samples every [`STEP_M`] metres along a WGS84 track (the last vertex is always a
/// sample). Only the geometry is filled.
pub fn resample(track: &[LonLat]) -> Result<Vec<Sample>> {
    if track.len() < 2 {
        return Err(Error::Track("a track needs at least two points".into()));
    }
    let mut pts = Vec::with_capacity(track.len());
    for p in track {
        let (x, y) = crs::to_lambert93(*p).map_err(|e| Error::Track(e.to_string()))?;
        if pts
            .last()
            .is_none_or(|(_, px, py): &(LonLat, f64, f64)| (x - px).hypot(y - py) > 1e-6)
        {
            pts.push((*p, x, y));
        }
    }
    if pts.len() < 2 {
        return Err(Error::Track("the track has zero length".into()));
    }
    let mut out = Vec::new();
    let mut s0 = 0.0;
    let mut next = 0.0;
    let mut last_len = 0.0;
    for (k, w) in pts.windows(2).enumerate() {
        let ((a, ax, ay), (b, bx, by)) = (w[0], w[1]);
        let len = (bx - ax).hypot(by - ay);
        last_len = len;
        while next <= s0 + len + 1e-9 {
            let t = ((next - s0) / len).clamp(0.0, 1.0);
            out.push(Sample {
                s: next,
                lon: a.lon + t * (b.lon - a.lon),
                lat: a.lat + t * (b.lat - a.lat),
                x: ax + t * (bx - ax),
                y: ay + t * (by - ay),
                seg: k as u32,
                seg_len: len as f32,
                ..Sample::default()
            });
            next += STEP_M;
        }
        s0 += len;
    }
    let (last, lx, ly) = *pts.last().expect("two points");
    if out.last().is_none_or(|l| s0 - l.s > 1e-6) {
        out.push(Sample {
            s: s0,
            lon: last.lon,
            lat: last.lat,
            x: lx,
            y: ly,
            seg: (pts.len() - 2) as u32,
            seg_len: last_len as f32,
            ..Sample::default()
        });
    }
    Ok(out)
}

/// Sets the segment fields of samples built before they existed (same track, same
/// sampling). Does nothing when the counts differ.
pub fn annotate_segments(track: &[LonLat], samples: &mut [Sample]) -> Result<()> {
    let fresh = resample(track)?;
    if fresh.len() == samples.len() {
        for (s, f) in samples.iter_mut().zip(fresh) {
            s.seg = f.seg;
            s.seg_len = f.seg_len;
        }
    }
    Ok(())
}

/// Horn slope and aspect from 8 elevation reads at ± `d` metres.
fn horn_at(f: &dyn Field, x: f64, y: f64, d: f64) -> Option<(f32, f32)> {
    let z = |dx: f64, dy: f64| f.at(x + dx, y + dy).map(f64::from);
    let (a, b, c) = (z(-d, d)?, z(0.0, d)?, z(d, d)?);
    let (dd, ff) = (z(-d, 0.0)?, z(d, 0.0)?);
    let (g, h, i) = (z(-d, -d)?, z(0.0, -d)?, z(d, -d)?);
    let dzdx = ((c + 2.0 * ff + i) - (a + 2.0 * dd + g)) / (8.0 * d);
    let dzdy = ((a + 2.0 * b + c) - (g + 2.0 * h + i)) / (8.0 * d);
    let slope = dzdx.hypot(dzdy).atan().to_degrees() as f32;
    let aspect = if dzdx == 0.0 && dzdy == 0.0 {
        -1.0
    } else {
        let a = (-dzdx).atan2(-dzdy).to_degrees();
        (if a < 0.0 { a + 360.0 } else { a }).rem_euclid(360.0) as f32
    };
    Some((slope, aspect))
}

/// Fills the layers under the samples (elevation, slope, aspect, forest, exposure,
/// network, zones). Can be called on consecutive slices with different layers (the
/// pipeline builds the terrain in corridor chunks).
pub fn fill(samples: &mut [Sample], layers: &Layers<'_>) {
    const CELL: f64 = 5.0;
    for s in samples.iter_mut() {
        let (x, y) = (s.x, s.y);
        if let Some(e) = layers.elevation {
            s.z = e.at(x, y);
            if (layers.slope.is_none() || layers.aspect.is_none())
                && let Some((sl, asp)) = horn_at(e, x, y, CELL)
            {
                if layers.slope.is_none() {
                    s.slope = Some(sl);
                }
                if layers.aspect.is_none() {
                    s.aspect = Some(asp);
                }
            }
        }
        if let Some(f) = layers.slope {
            s.slope = f.at(x, y);
        }
        if let Some(f) = layers.aspect {
            s.aspect = f.at(x, y);
        }
        if let Some(f) = layers.canopy_height {
            s.canopy_h = f.at(x, y);
            let (mut n, mut t) = (0u32, 0u32);
            let mut min_h: Option<f32> = None;
            for j in -2i32..=2 {
                for i in -2i32..=2 {
                    if let Some(h) = f.at(x + f64::from(i) * CELL, y + f64::from(j) * CELL) {
                        min_h = Some(min_h.map_or(h, |m| m.min(h)));
                        if i.abs() <= 1 && j.abs() <= 1 {
                            n += 1;
                            t += u32::from(h >= TREE_HEIGHT_M);
                        }
                    }
                }
            }
            s.cover = (n > 0).then(|| 100.0 * t as f32 / n as f32);
            s.min_h = min_h;
        }
        if let Some(f) = layers.exposure {
            s.exposure = f.at(x, y).map(|v| v.round() as u8);
        }
        if let Some(f) = layers.pistes {
            s.piste = f.at(x, y).map(|v| v.round() as u8);
        }
        if let Some(a) = layers.no_network {
            s.no_network = a.contains(LonLat::new(s.lon, s.lat));
        }
        if let Some(z) = layers.zones {
            s.zone = z.at(LonLat::new(s.lon, s.lat));
        }
    }
}

/// Marks the exits: both ends, and every sample within [`EXIT_RADIUS_M`] of one of the
/// `exits` points (Lambert-93; densify roads to at most 20 m between points).
pub fn mark_exits(samples: &mut [Sample], exits: &[(f64, f64)]) {
    const BIN: f64 = 100.0;
    let key = |x: f64, y: f64| ((x / BIN).floor() as i64, (y / BIN).floor() as i64);
    let mut bins: HashMap<(i64, i64), Vec<(f64, f64)>> = HashMap::new();
    for &(x, y) in exits {
        bins.entry(key(x, y)).or_default().push((x, y));
    }
    let n = samples.len();
    for (i, s) in samples.iter_mut().enumerate() {
        if i == 0 || i + 1 == n {
            s.exit = true;
            continue;
        }
        let (kx, ky) = key(s.x, s.y);
        s.exit = (-1..=1).any(|dx| {
            (-1..=1).any(|dy| {
                bins.get(&(kx + dx, ky + dy)).is_some_and(|v| {
                    v.iter()
                        .any(|(ex, ey)| (ex - s.x).hypot(ey - s.y) <= EXIT_RADIUS_M)
                })
            })
        });
    }
}

/// Builds a profile in one go from in-memory layers (the phone case).
pub fn build(
    track: &[LonLat],
    layers: &Layers<'_>,
    exits: Option<&[(f64, f64)]>,
) -> Result<Profile> {
    if layers.elevation.is_none() {
        return Err(Error::Track("an elevation layer is required".into()));
    }
    let mut samples = resample(track)?;
    fill(&mut samples, layers);
    mark_exits(&mut samples, exits.unwrap_or(&[]));
    Ok(Profile {
        step_m: STEP_M,
        samples,
        inputs: Inputs {
            canopy: layers.canopy_height.is_some(),
            exposure: layers.exposure.is_some(),
            network: layers.no_network.is_some(),
            zones: layers.zones.is_some(),
            roads: exits.is_some(),
        },
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Plane {
        z0: f32,
        gx: f32,
    }
    impl Field for Plane {
        fn at(&self, x: f64, _y: f64) -> Option<f32> {
            Some(self.z0 + self.gx * x as f32)
        }
    }

    fn track_east(m: f64) -> Vec<LonLat> {
        let (x, y) = crs::to_lambert93(LonLat::new(6.0, 46.5)).unwrap();
        vec![
            crs::from_lambert93(x, y).unwrap(),
            crs::from_lambert93(x + m, y).unwrap(),
        ]
    }

    #[test]
    fn resample_every_step_with_last_point() {
        let s = resample(&track_east(105.0)).unwrap();
        assert_eq!(s.len(), 12);
        assert!((s[11].s - 105.0).abs() < 1e-6);
        assert!((s[3].s - 30.0).abs() < 1e-9);
    }

    #[test]
    fn slope_and_aspect_from_elevation() {
        // z rises 0.2 m per metre eastwards: slope atan(0.2) = 11.31°, facing west.
        let p = Plane { z0: 0.0, gx: 0.2 };
        let prof = build(
            &track_east(50.0),
            &Layers {
                elevation: Some(&p),
                ..Layers::default()
            },
            None,
        )
        .unwrap();
        let s = &prof.samples[2];
        assert!((s.slope.unwrap() - 11.31).abs() < 0.01);
        assert!((s.aspect.unwrap() - 270.0).abs() < 0.01);
        assert!(prof.samples[0].exit && prof.samples.last().unwrap().exit);
        assert!(!prof.samples[2].exit);
    }

    #[test]
    fn exits_near_roads() {
        let mut s = resample(&track_east(300.0)).unwrap();
        let mid = (s[15].x, s[15].y + 50.0);
        mark_exits(&mut s, &[mid]);
        // 50 m beside sample 15: samples 12 to 18 are within 60 m (58.3 m for 12).
        assert!(s[15].exit && s[12].exit && s[18].exit);
        assert!(!s[11].exit && !s[19].exit && !s[5].exit);
    }
}
