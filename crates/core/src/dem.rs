//! Pipeline side (feature `pipeline`): elevation profiles of the trail graph from the
//! terrain model of `skimap-terrain` (P5 #18, `data/out/<site>/terrain/dem_5m.tif`,
//! Float32, Lambert-93 EPSG:2154, 5 m, `NaN` = no data), read with the shared GeoTIFF
//! helper `skimap_common::geotiff`.

use std::path::Path;

use skimap_common::crs::{Crs, to_lambert93};
use skimap_common::geom::LonLat;
use skimap_common::grid::Grid;

use crate::error::{Error, Result};
use crate::graph::{PisteGraph, Profile};

/// Default spacing of the profile stations, metres (two cells of the 5 m model).
pub const PROFILE_STEP_M: f64 = 10.0;

/// Reads `dem_5m.tif` (must be Lambert-93).
pub fn read_dem(path: &Path) -> Result<Grid<f32>> {
    let g = skimap_common::geotiff::read_f32(path)?;
    if g.crs() != Crs::Lambert93 {
        return Err(Error::format(
            path.display().to_string(),
            format!("expected EPSG:2154, got EPSG:{}", g.crs().epsg()),
        ));
    }
    Ok(g)
}

/// Bilinear terrain elevation at a WGS84 position (`None` outside the model or on
/// missing cells).
pub fn elevation(dem: &Grid<f32>, p: LonLat) -> Option<f64> {
    let (x, y) = to_lambert93(p).ok()?;
    dem.sample_bilinear(x, y).filter(|z| z.is_finite())
}

/// Coverage of the profiles attached by [`attach_profiles`].
#[derive(Debug, Clone, Copy, Default, PartialEq, serde::Serialize)]
pub struct ProfileStats {
    /// Edges with a profile.
    pub edges_with_profile: usize,
    /// Edges without any terrain value (outside France): no profile, flat.
    pub edges_without: usize,
    /// Length with a profile, metres.
    pub length_with_m: f64,
    /// Length without, metres.
    pub length_without_m: f64,
    /// Stations sampled.
    pub stations: usize,
    /// Stations without terrain value, interpolated along their edge.
    pub stations_filled: usize,
}

/// Samples a profile on every edge, one station every `step_m` metres plus both ends.
/// Missing values are interpolated along the edge (held constant before the first and
/// after the last valid value); an edge without any value gets no profile.
pub fn attach_profiles(
    graph: &mut PisteGraph,
    step_m: f64,
    source: &str,
    mut sample: impl FnMut(LonLat) -> Option<f64>,
) -> ProfileStats {
    let mut stats = ProfileStats::default();
    let mut profiles = Vec::with_capacity(graph.edges().len());
    for e in 0..graph.edges().len() {
        let len = graph.length_m(e);
        let n = (len / step_m).ceil().max(1.0) as usize;
        let mut dist = Vec::with_capacity(n + 1);
        let mut z: Vec<Option<f64>> = Vec::with_capacity(n + 1);
        for k in 0..=n {
            let d = if k == n { len } else { k as f64 * step_m };
            dist.push(d);
            z.push(sample(graph.point_at(e, d)));
        }
        stats.stations += z.len();
        let valid: Vec<usize> = (0..z.len()).filter(|&i| z[i].is_some()).collect();
        if valid.is_empty() {
            stats.edges_without += 1;
            stats.length_without_m += len;
            profiles.push(None);
            continue;
        }
        let filled = z.len() - valid.len();
        stats.stations_filled += filled;
        let mut elev = vec![0.0f32; z.len()];
        for i in 0..z.len() {
            let v = match z[i] {
                Some(v) => v,
                None => {
                    let j = valid.partition_point(|&k| k < i);
                    match (j.checked_sub(1).map(|j| valid[j]), valid.get(j).copied()) {
                        (Some(a), Some(b)) => {
                            let (za, zb) = (z[a].expect("valid"), z[b].expect("valid"));
                            za + (zb - za) * (dist[i] - dist[a]) / (dist[b] - dist[a])
                        }
                        (Some(a), None) => z[a].expect("valid"),
                        (None, Some(b)) => z[b].expect("valid"),
                        (None, None) => unreachable!("valid is not empty"),
                    }
                }
            };
            elev[i] = v as f32;
        }
        stats.edges_with_profile += 1;
        stats.length_with_m += len;
        profiles.push(Some(Profile {
            dist_m: dist.iter().map(|&d| d as f32).collect(),
            elev_m: elev,
            filled: filled as u32,
        }));
    }
    graph.set_profiles(profiles, source.to_owned());
    stats
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graph::tests::{toy, toy_plane};

    #[test]
    fn profiles_follow_a_synthetic_ramp_and_fill_gaps() {
        let mut g = toy();
        let plane = toy_plane();
        // Ramp: 5 % up towards +x; missing values west of x = 200 m on edge e0.
        let s = attach_profiles(&mut g, 10.0, "ramp", |p| {
            let [x, _] = plane.xy(p);
            (x >= 195.0).then_some(1000.0 + 0.05 * x)
        });
        assert_eq!(s.edges_with_profile, 5);
        assert!(s.stations_filled > 0);
        let p0 = g.edge(0).profile.as_ref().unwrap();
        // Before x = 200 m, held at the first valid value.
        assert!((p0.elev_m[0] - 1010.0).abs() < 0.01, "{}", p0.elev_m[0]);
        let (up, down) = p0.gain_loss();
        assert!((up - 40.0).abs() < 0.1 && down < 1e-6, "{up} {down}");
        // e1 (1000 m along +x): 50 m climb forward, 50 m descent backward.
        let (up, down) = g.gain_loss(crate::graph::HalfEdge {
            edge: 1,
            forward: false,
        });
        assert!(up < 1e-3 && (down - 50.0).abs() < 0.1, "{up} {down}");
        assert_eq!(g.data().elevation_source.as_deref(), Some("ramp"));
    }

    #[test]
    fn edge_without_values_has_no_profile() {
        let mut g = toy();
        let s = attach_profiles(&mut g, 10.0, "none", |_| None);
        assert_eq!(s.edges_with_profile, 0);
        assert_eq!(s.edges_without, 5);
        assert!(g.edges().iter().all(|e| e.profile.is_none()));
    }
}
