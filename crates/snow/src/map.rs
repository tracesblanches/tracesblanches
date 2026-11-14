//! Snow terrain at 25 m (elevation, slope, aspect, massif) and per-pixel projection.
//!
//! The 25 m grid is the mean of 5 × 5 LiDAR HD cells (block mean), on a lattice shared by
//! every extent (edges at `25 k − 0.25` m in X and `25 k + 0.25` m in Y, the LiDAR HD tile
//! grid): a station patch and the site map give the same value for the same cell. Slope
//! and aspect are computed at 25 m with Horn's operator, so they describe slopes at the
//! scale of the simulation classes (0, 20, 40°) rather than 5 m micro-relief.

use rayon::prelude::*;
use skimap_common::download::Downloader;
use skimap_common::grid::Grid;
use skimap_terrain::dem::{self, Extent, TILE_SHIFT};
use skimap_terrain::slope;

use crate::classes::Practice;
use crate::error::Result;
use crate::massifs::{self, Massif};
use crate::sim::MassifDay;

/// Cell size of the snow map, metres.
pub const CELL: f64 = 25.0;
/// Block factor from the 5 m terrain model.
pub const FACTOR: usize = 5;

/// Extent snapped outward to the shared 25 m lattice.
pub fn snap(e: &Extent) -> Extent {
    let down = |v: f64, off: f64| ((v - off) / CELL).floor() * CELL + off;
    let up = |v: f64, off: f64| ((v - off) / CELL).ceil() * CELL + off;
    Extent {
        min_x: down(e.min_x, -TILE_SHIFT),
        max_x: up(e.max_x, -TILE_SHIFT),
        min_y: down(e.min_y, TILE_SHIFT),
        max_y: up(e.max_y, TILE_SHIFT),
    }
}

/// Terrain inputs of the snow map.
#[derive(Debug, Clone)]
pub struct SnowTerrain {
    /// Elevation, m (25 m block mean).
    pub dem: Grid<f32>,
    /// Slope, degrees.
    pub slope: Grid<f32>,
    /// Aspect, degrees (-1 flat).
    pub aspect: Grid<f32>,
    /// Massif code per cell (0 = none).
    pub massif: Grid<u16>,
    /// Cells with an elevation but outside every massif polygon, given the nearest massif.
    pub cells_filled_by_nearest_massif: usize,
}

/// Builds the 25 m terrain of an extent.
pub fn snow_terrain(e: &Extent, dl: &Downloader, massifs: &[Massif]) -> Result<SnowTerrain> {
    let e = snap(e);
    let dem5 = dem::fetch_dem(&e, dl)?;
    let dem25 = dem::block_mean(&dem5, FACTOR);
    let sa = slope::horn(&dem25);
    let mut codes = massifs::rasterize(massifs, &dem25);
    let valid = |i: usize| dem25.data()[i].is_finite();
    // Cells without elevation do not need a massif.
    for (i, c) in codes.data_mut().iter_mut().enumerate() {
        if !valid(i) {
            *c = 0;
        }
    }
    let filled = massifs::fill_nearest(&mut codes, valid);
    Ok(SnowTerrain {
        dem: dem25,
        slope: sa.slope,
        aspect: sa.aspect,
        massif: codes,
        cells_filled_by_nearest_massif: filled,
    })
}

/// Estimated depth (cm) and its range at one cell.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CellEstimate {
    /// Central estimate.
    pub depth: f32,
    /// Lower bound of the range.
    pub low: f32,
    /// Upper bound of the range.
    pub high: f32,
}

impl SnowTerrain {
    /// Terrain values of the cell containing a Lambert-93 point:
    /// (elevation, slope, aspect, massif).
    pub fn at(&self, x: f64, y: f64) -> Option<(f32, f32, f32, u16)> {
        let (c, r) = self.dem.cell_at(x, y)?;
        let i = self.dem.index(c, r)?;
        Some((
            self.dem.data()[i],
            self.slope.data()[i],
            self.aspect.data()[i],
            self.massif.data()[i],
        ))
    }

    /// Massif codes present.
    pub fn massif_codes(&self) -> Vec<u16> {
        let mut v: Vec<u16> = self
            .massif
            .data()
            .iter()
            .copied()
            .filter(|c| *c != 0)
            .collect();
        v.sort_unstable();
        v.dedup();
        v
    }
}

/// Estimate of one cell from its terrain values. Cells whose slope is undefined (map
/// border, next to no-data) are treated as flat.
pub fn estimate_cell(day: &MassifDay, alt: f32, slope: f32, aspect: f32) -> CellEstimate {
    let (s, a) = if slope.is_finite() {
        (slope, aspect)
    } else {
        (0.0, -1.0)
    };
    let depth = day.depth(alt, s, a);
    let (low, high) = day.depth_range(alt, s, a);
    CellEstimate { depth, low, high }
}

/// Snow maps of one day.
#[derive(Debug, Clone)]
pub struct SnowMaps {
    /// Central depth, cm.
    pub depth: Grid<f32>,
    /// Lower bound, cm.
    pub low: Grid<f32>,
    /// Upper bound, cm.
    pub high: Grid<f32>,
}

/// Projects the simulation of one day onto the terrain; `day_of(massif)` gives the table
/// of a massif (cells of a massif without table are `NaN`).
pub fn estimate<'a>(
    t: &SnowTerrain,
    day_of: impl Fn(u16) -> Option<&'a MassifDay> + Sync,
) -> SnowMaps {
    let n = t.dem.data().len();
    let mut out = vec![(f32::NAN, f32::NAN, f32::NAN); n];
    out.par_iter_mut().enumerate().for_each(|(i, o)| {
        let alt = t.dem.data()[i];
        let code = t.massif.data()[i];
        if !alt.is_finite() || code == 0 {
            return;
        }
        if let Some(day) = day_of(code) {
            let e = estimate_cell(day, alt, t.slope.data()[i], t.aspect.data()[i]);
            *o = (e.depth, e.low, e.high);
        }
    });
    let mk = |f: fn(&(f32, f32, f32)) -> f32| {
        Grid::from_vec(
            t.dem.width(),
            t.dem.height(),
            *t.dem.transform(),
            t.dem.crs(),
            out.iter().map(f).collect(),
        )
        .expect("same shape")
    };
    SnowMaps {
        depth: mk(|v| v.0),
        low: mk(|v| v.1),
        high: mk(|v| v.2),
    }
}

/// Class grid (0, 1, 2 as Float32, `NaN` no-data) of a practice.
pub fn classify(depth: &Grid<f32>, p: &Practice) -> Grid<f32> {
    depth.map(|d| p.class(*d).map_or(f32::NAN, f32::from))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn snapping_is_on_the_shared_lattice() {
        let e = snap(&Extent {
            min_x: 988_714.1,
            min_y: 6_414_583.4,
            max_x: 1_005_292.7,
            max_y: 6_431_977.2,
        });
        for (v, off) in [
            (e.min_x, -0.25),
            (e.max_x, -0.25),
            (e.min_y, 0.25),
            (e.max_y, 0.25),
        ] {
            let k = (v - off) / CELL;
            assert!((k - k.round()).abs() < 1e-9, "{v}");
        }
        assert!(e.min_x <= 988_714.1 && e.min_x > 988_714.1 - CELL);
        assert!(e.max_y >= 6_431_977.2 && e.max_y < 6_431_977.2 + CELL);
        // Snapping is idempotent.
        assert_eq!(snap(&e), e);
    }
}
