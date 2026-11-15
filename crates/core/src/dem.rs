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
        assert!((s.length_without_m - g.total_length_m()).abs() < 1e-6);
        assert_eq!(s.length_with_m, 0.0);
        assert_eq!(s.stations_filled, 0);
    }

    /// Every statistic, the stations and the linear interpolation of a gap between two
    /// valid stations (Q4: mutants of the station count, the distances and the filling).
    #[test]
    fn statistics_stations_and_gap_interpolation() {
        let mut g = toy();
        let plane = toy_plane();
        let lengths: Vec<f64> = (0..g.edges().len()).map(|e| g.length_m(e)).collect();
        // Edge e4 (y = −500 m) has no value at all; on e0 (y = 0, x from 0 to 1000 m) the
        // values between x = 300 and 600 m are missing: interpolated linearly.
        let s = attach_profiles(&mut g, 100.0, "gap", |p| {
            let [x, y] = plane.xy(p);
            if (y + 500.0).abs() < 1.0 && x > 999.0 {
                return None;
            }
            let gap = y.abs() < 1.0 && x > 250.0 && x < 650.0;
            (!gap).then_some(2000.0 + x * x / 1000.0)
        });
        // Stations: ceil(len / 100) + 1 per edge (both ends included).
        let stations: usize = lengths
            .iter()
            .map(|l| (l / 100.0).ceil().max(1.0) as usize + 1)
            .sum();
        assert_eq!(s.stations, stations);
        assert_eq!(s.edges_with_profile, 4);
        assert_eq!(s.edges_without, 1);
        assert!((s.length_without_m - lengths[4]).abs() < 1e-6, "{s:?}");
        assert!((s.length_with_m + s.length_without_m - g.total_length_m()).abs() < 1e-6);
        // e0: stations at 0, 100, ..., 1000 m; 300, 400, 500, 600 m are filled; e2 and
        // e3 end on e4: their last two stations (500 m and the end, 500.0x m long) too.
        assert_eq!(s.stations_filled, 8, "{s:?}");
        let p0 = g.edge(0).profile.as_ref().unwrap();
        assert_eq!(p0.filled, 4);
        // 1000.0x m long: stations 0, 100, ..., 1000 m and the end.
        assert_eq!(p0.dist_m.len(), 12);
        assert!((p0.dist_m[3] - 300.0).abs() < 0.5 && (p0.dist_m[11] - 1000.0).abs() < 0.5);
        // Linear between 200 m (2040 m) and 700 m (2490 m): 400 m → 2220 m.
        let z = |d: f64| 2000.0 + d * d / 1000.0;
        let lin = z(200.0) + (z(700.0) - z(200.0)) * (400.0 - 200.0) / 500.0;
        assert!(
            (f64::from(p0.elev_m[4]) - lin).abs() < 0.05,
            "{}",
            p0.elev_m[4]
        );
        assert!((f64::from(p0.elev_m[7]) - z(700.0)).abs() < 0.05);
        assert!(g.edge(4).profile.is_none());
        assert_eq!(g.data().elevation_source.as_deref(), Some("gap"));
    }

    /// A trailing gap is held at the last valid value; a single-station edge shorter than
    /// the step gets its two ends.
    #[test]
    fn trailing_gap_and_short_step() {
        let mut g = toy();
        let plane = toy_plane();
        attach_profiles(&mut g, 300.0, "tail", |p| {
            let [x, y] = plane.xy(p);
            (!(y.abs() < 1.0 && x > 650.0 && x < 1001.0)).then_some(100.0 + x / 10.0)
        });
        let p0 = g.edge(0).profile.as_ref().unwrap();
        // Stations 0, 300, 600, 900, 1000 m: the last two held at 600 m's 160 m.
        assert_eq!(p0.dist_m.len(), 5);
        assert!((p0.elev_m[3] - 160.0).abs() < 0.01 && (p0.elev_m[4] - 160.0).abs() < 0.01);
        let mut g = toy();
        let s = attach_profiles(&mut g, 5000.0, "long step", |_| Some(1.0));
        assert_eq!(s.stations, 2 * g.edges().len());
    }

    #[test]
    fn terrain_model_must_be_lambert93_and_is_sampled_bilinearly() {
        use skimap_common::grid::{GeoTransform, Grid};
        let dir = tempfile::tempdir().unwrap();
        // 4 x 3 cells of 5 m around Lambert-93 (998000, 6425000), z = x offset in metres.
        let t = GeoTransform::north_up(998_000.0, 6_425_000.0, 5.0);
        let data: Vec<f32> = (0..12).map(|i| (i % 4) as f32 * 5.0).collect();
        let g = Grid::from_vec(4, 3, t, Crs::Lambert93, data.clone()).unwrap();
        let p = dir.path().join("l93.tif");
        skimap_common::geotiff::write_f32(&p, &g).unwrap();
        let dem = read_dem(&p).unwrap();
        // Centre of cell (1, 1) and halfway to (2, 1).
        let at = |x: f64, y: f64| {
            crate::dem::elevation(&dem, skimap_common::crs::from_lambert93(x, y).unwrap())
        };
        assert!((at(998_007.5, 6_424_992.5).unwrap() - 5.0).abs() < 1e-3);
        assert!((at(998_010.0, 6_424_992.5).unwrap() - 7.5).abs() < 1e-3);
        assert!(at(990_000.0, 6_424_992.5).is_none());
        let g = Grid::from_vec(
            4,
            3,
            GeoTransform::north_up(6.0, 45.0, 0.001),
            Crs::Wgs84,
            data,
        )
        .unwrap();
        let p = dir.path().join("wgs84.tif");
        skimap_common::geotiff::write_f32(&p, &g).unwrap();
        assert!(read_dem(&p).is_err());
    }
}
