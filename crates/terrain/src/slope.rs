//! Slope and aspect with Horn's 3 × 3 operator (the default of `gdaldem slope/aspect`).
//!
//! For the window
//!
//! ```text
//! a b c
//! d e f
//! g h i
//! ```
//!
//! `dz/dx = ((c + 2f + i) - (a + 2d + g)) / (8 Δ)` (east positive) and
//! `dz/dy = ((a + 2b + c) - (g + 2h + i)) / (8 Δ)` (north positive).
//! Slope is `atan(√(dz/dx² + dz/dy²))` in degrees. Aspect is the direction the slope
//! faces (downhill), in degrees clockwise from north, in `[0, 360)`; `-1` on perfectly
//! flat cells (Météo-France convention for "flat"). Border cells and cells with a `NaN`
//! neighbour are `NaN`.

use rayon::prelude::*;
use skimap_common::grid::Grid;

/// Slope and aspect grids, same shape and georeferencing as the terrain model.
#[derive(Debug, Clone)]
pub struct SlopeAspect {
    /// Slope, degrees.
    pub slope: Grid<f32>,
    /// Aspect, degrees clockwise from north (direction faced), `-1` if flat.
    pub aspect: Grid<f32>,
}

/// Value marking a flat cell in the aspect grid.
pub const FLAT_ASPECT: f32 = -1.0;

/// Slope (degrees) and aspect (degrees, `-1` flat) from the Horn gradient.
pub fn slope_aspect_from_gradient(dzdx: f64, dzdy_north: f64) -> (f32, f32) {
    let slope = dzdx.hypot(dzdy_north).atan().to_degrees();
    let aspect = if dzdx == 0.0 && dzdy_north == 0.0 {
        f64::from(FLAT_ASPECT)
    } else {
        // Downhill direction is (-dzdx, -dzdy) in (east, north).
        let a = (-dzdx).atan2(-dzdy_north).to_degrees();
        if a < 0.0 { a + 360.0 } else { a }
    };
    // atan2 may return exactly 360 after the shift for -0.0 inputs.
    let aspect = if aspect >= 360.0 {
        aspect - 360.0
    } else {
        aspect
    };
    (slope as f32, aspect as f32)
}

/// Horn slope and aspect of a north-up terrain model with square cells.
pub fn horn(dem: &Grid<f32>) -> SlopeAspect {
    let (w, h) = (dem.width(), dem.height());
    let cell = dem.transform().pixel_width;
    let z = dem.data();
    let mut slope = vec![f32::NAN; w * h];
    let mut aspect = vec![f32::NAN; w * h];
    if w >= 3 && h >= 3 {
        slope
            .par_chunks_mut(w)
            .zip(aspect.par_chunks_mut(w))
            .enumerate()
            .filter(|(r, _)| *r > 0 && *r + 1 < h)
            .for_each(|(r, (srow, arow))| {
                let up = &z[(r - 1) * w..r * w];
                let mid = &z[r * w..(r + 1) * w];
                let down = &z[(r + 1) * w..(r + 2) * w];
                for c in 1..w - 1 {
                    let (a, b, cc) = (up[c - 1], up[c], up[c + 1]);
                    let (d, f) = (mid[c - 1], mid[c + 1]);
                    let (g, hh, i) = (down[c - 1], down[c], down[c + 1]);
                    let e = mid[c];
                    if ![a, b, cc, d, e, f, g, hh, i].iter().all(|v| v.is_finite()) {
                        continue;
                    }
                    let (a, b, cc) = (f64::from(a), f64::from(b), f64::from(cc));
                    let (d, f) = (f64::from(d), f64::from(f));
                    let (g, hh, i) = (f64::from(g), f64::from(hh), f64::from(i));
                    let dzdx = ((cc + 2.0 * f + i) - (a + 2.0 * d + g)) / (8.0 * cell);
                    let dzdy = ((a + 2.0 * b + cc) - (g + 2.0 * hh + i)) / (8.0 * cell);
                    let (s, asp) = slope_aspect_from_gradient(dzdx, dzdy);
                    srow[c] = s;
                    arow[c] = asp;
                }
            });
    }
    let mk = |data| {
        Grid::from_vec(w, h, *dem.transform(), dem.crs(), data).expect("same shape as the DEM")
    };
    SlopeAspect {
        slope: mk(slope),
        aspect: mk(aspect),
    }
}

/// Fraction of valid cells with slope ≥ `threshold` degrees inside `[x0, x1] × [y0, y1]`
/// (cell centres), and the number of valid cells.
pub fn fraction_at_least(
    slope: &Grid<f32>,
    threshold: f32,
    ext: (f64, f64, f64, f64),
) -> (f64, u64) {
    let (x0, y0, x1, y1) = ext;
    let (mut n, mut k) = (0u64, 0u64);
    for r in 0..slope.height() {
        for c in 0..slope.width() {
            let (x, y) = slope.transform().cell_center(c, r);
            if x < x0 || x > x1 || y < y0 || y > y1 {
                continue;
            }
            let v = slope.data()[r * slope.width() + c];
            if v.is_finite() {
                n += 1;
                if v >= threshold {
                    k += 1;
                }
            }
        }
    }
    (if n > 0 { k as f64 / n as f64 } else { f64::NAN }, n)
}

#[cfg(test)]
mod tests {
    use super::*;
    use approx::assert_abs_diff_eq;
    use skimap_common::crs::Crs;
    use skimap_common::grid::GeoTransform;

    /// Plane z = ax + by sampled on a 5 m grid (x east, y north).
    fn plane(a: f64, b: f64) -> Grid<f32> {
        let gt = GeoTransform::north_up(0.0, 50.0, 5.0);
        let mut data = Vec::new();
        for r in 0..10 {
            for c in 0..10 {
                let (x, y) = gt.cell_center(c, r);
                data.push((a * x + b * y) as f32);
            }
        }
        Grid::from_vec(10, 10, gt, Crs::Lambert93, data).unwrap()
    }

    #[test]
    fn inclined_planes() {
        // Rises to the east by tan(30°): faces west (270°), 30°.
        let t = 30f64.to_radians().tan();
        let sa = horn(&plane(t, 0.0));
        assert_abs_diff_eq!(sa.slope.get(4, 4).copied().unwrap(), 30.0, epsilon = 1e-3);
        assert_abs_diff_eq!(sa.aspect.get(4, 4).copied().unwrap(), 270.0, epsilon = 1e-3);
        // Rises to the north: faces south (180°).
        let sa = horn(&plane(0.0, 1.0));
        assert_abs_diff_eq!(sa.slope.get(5, 5).copied().unwrap(), 45.0, epsilon = 1e-3);
        assert_abs_diff_eq!(sa.aspect.get(5, 5).copied().unwrap(), 180.0, epsilon = 1e-3);
        // Rises to the south-west: faces north-east (45°).
        let sa = horn(&plane(-0.5, -0.5));
        assert_abs_diff_eq!(sa.aspect.get(5, 5).copied().unwrap(), 45.0, epsilon = 1e-3);
        let expected = (0.5f64.hypot(0.5)).atan().to_degrees() as f32;
        assert_abs_diff_eq!(
            sa.slope.get(5, 5).copied().unwrap(),
            expected,
            epsilon = 1e-3
        );
        // Rises to the south: faces north (0°).
        let sa = horn(&plane(0.0, -0.2));
        assert_abs_diff_eq!(sa.aspect.get(5, 5).copied().unwrap(), 0.0, epsilon = 1e-3);
    }

    #[test]
    fn borders_flat_and_nan() {
        let mut g = plane(0.0, 0.0);
        let sa = horn(&g);
        assert!(sa.slope.get(0, 0).unwrap().is_nan());
        assert!(sa.slope.get(9, 5).unwrap().is_nan());
        assert_eq!(sa.slope.get(5, 5).copied(), Some(0.0));
        assert_eq!(sa.aspect.get(5, 5).copied(), Some(FLAT_ASPECT));
        g.data_mut()[3 * 10 + 3] = f32::NAN;
        let sa = horn(&g);
        assert!(sa.slope.get(4, 4).unwrap().is_nan());
        assert!(sa.slope.get(2, 2).unwrap().is_nan());
        assert!(!sa.slope.get(5, 5).unwrap().is_nan());
    }

    #[test]
    fn steep_fraction() {
        let t = 35f64.to_radians().tan();
        let sa = horn(&plane(t, 0.0));
        let (f, n) = fraction_at_least(&sa.slope, 30.0, (0.0, 0.0, 50.0, 50.0));
        assert_eq!(n, 64);
        assert_eq!(f, 1.0);
    }
}
