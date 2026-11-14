//! Raster grid with a north-up geotransform (no rotation), as produced by IGN elevation
//! data (RGE ALTI, LiDAR HD) and used by slope, snow and cost grids.
//!
//! Conventions (same as GDAL): `(col, row)` with row 0 at the top; the geotransform maps
//! the *top-left corner* of a cell; cell centres are at `+0.5`. Data is row-major.

use serde::{Deserialize, Serialize};

use crate::crs::Crs;
use crate::error::{Error, Result};

/// Affine north-up geotransform (GDAL `[ox, pw, 0, oy, 0, ph]` with `ph < 0`).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct GeoTransform {
    /// X of the top-left corner of the grid.
    pub origin_x: f64,
    /// Y of the top-left corner of the grid.
    pub origin_y: f64,
    /// Cell width along X (positive).
    pub pixel_width: f64,
    /// Cell height along Y (negative for a north-up grid).
    pub pixel_height: f64,
}

impl GeoTransform {
    /// North-up transform from the top-left corner and a square cell size.
    pub fn north_up(origin_x: f64, origin_y: f64, cell_size: f64) -> Self {
        Self {
            origin_x,
            origin_y,
            pixel_width: cell_size,
            pixel_height: -cell_size,
        }
    }

    /// From a GDAL geotransform; rotated grids are rejected.
    pub fn from_gdal(gt: [f64; 6]) -> Result<Self> {
        if gt[2] != 0.0 || gt[4] != 0.0 {
            return Err(Error::Grid(format!(
                "rotated geotransform not supported: {gt:?}"
            )));
        }
        if gt[1] == 0.0 || gt[5] == 0.0 {
            return Err(Error::Grid(format!("zero cell size: {gt:?}")));
        }
        Ok(Self {
            origin_x: gt[0],
            origin_y: gt[3],
            pixel_width: gt[1],
            pixel_height: gt[5],
        })
    }

    /// As a GDAL geotransform.
    pub fn to_gdal(&self) -> [f64; 6] {
        [
            self.origin_x,
            self.pixel_width,
            0.0,
            self.origin_y,
            0.0,
            self.pixel_height,
        ]
    }

    /// World coordinates of a fractional pixel position (`(0, 0)` is the top-left corner).
    pub fn pixel_to_world(&self, col: f64, row: f64) -> (f64, f64) {
        (
            self.origin_x + col * self.pixel_width,
            self.origin_y + row * self.pixel_height,
        )
    }

    /// World coordinates of the centre of cell `(col, row)`.
    pub fn cell_center(&self, col: usize, row: usize) -> (f64, f64) {
        self.pixel_to_world(col as f64 + 0.5, row as f64 + 0.5)
    }

    /// Fractional pixel position of world coordinates.
    pub fn world_to_pixel(&self, x: f64, y: f64) -> (f64, f64) {
        (
            (x - self.origin_x) / self.pixel_width,
            (y - self.origin_y) / self.pixel_height,
        )
    }
}

/// A raster of `T` with its georeferencing.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Grid<T> {
    width: usize,
    height: usize,
    transform: GeoTransform,
    crs: Crs,
    nodata: Option<T>,
    data: Vec<T>,
}

impl<T: Clone> Grid<T> {
    /// Grid filled with `fill`.
    pub fn filled(width: usize, height: usize, transform: GeoTransform, crs: Crs, fill: T) -> Self {
        Self {
            width,
            height,
            transform,
            crs,
            nodata: None,
            data: vec![fill; width * height],
        }
    }
}

impl<T> Grid<T> {
    /// Grid from row-major data; `data.len()` must be `width * height`.
    pub fn from_vec(
        width: usize,
        height: usize,
        transform: GeoTransform,
        crs: Crs,
        data: Vec<T>,
    ) -> Result<Self> {
        if width.checked_mul(height) != Some(data.len()) {
            return Err(Error::Grid(format!(
                "data length {} does not match {width} x {height}",
                data.len()
            )));
        }
        Ok(Self {
            width,
            height,
            transform,
            crs,
            nodata: None,
            data,
        })
    }

    /// Sets the no-data value.
    pub fn with_nodata(mut self, nodata: T) -> Self {
        self.nodata = Some(nodata);
        self
    }

    /// Number of columns.
    pub fn width(&self) -> usize {
        self.width
    }

    /// Number of rows.
    pub fn height(&self) -> usize {
        self.height
    }

    /// Geotransform.
    pub fn transform(&self) -> &GeoTransform {
        &self.transform
    }

    /// Coordinate reference system of the geotransform.
    pub fn crs(&self) -> Crs {
        self.crs
    }

    /// No-data value, if any.
    pub fn nodata(&self) -> Option<&T> {
        self.nodata.as_ref()
    }

    /// Row-major cell values.
    pub fn data(&self) -> &[T] {
        &self.data
    }

    /// Mutable row-major cell values.
    pub fn data_mut(&mut self) -> &mut [T] {
        &mut self.data
    }

    /// Row-major index of `(col, row)`, if inside the grid.
    pub fn index(&self, col: usize, row: usize) -> Option<usize> {
        (col < self.width && row < self.height).then(|| row * self.width + col)
    }

    /// Value of cell `(col, row)`.
    pub fn get(&self, col: usize, row: usize) -> Option<&T> {
        self.index(col, row).map(|i| &self.data[i])
    }

    /// Mutable value of cell `(col, row)`.
    pub fn get_mut(&mut self, col: usize, row: usize) -> Option<&mut T> {
        self.index(col, row).map(|i| &mut self.data[i])
    }

    /// Cell `(col, row)` containing world coordinates, if inside the grid.
    pub fn cell_at(&self, x: f64, y: f64) -> Option<(usize, usize)> {
        let (c, r) = self.transform.world_to_pixel(x, y);
        if c < 0.0 || r < 0.0 || !c.is_finite() || !r.is_finite() {
            return None;
        }
        let (c, r) = (c.floor() as usize, r.floor() as usize);
        (c < self.width && r < self.height).then_some((c, r))
    }

    /// Value of the cell containing world coordinates.
    pub fn value_at(&self, x: f64, y: f64) -> Option<&T> {
        self.cell_at(x, y).and_then(|(c, r)| self.get(c, r))
    }

    /// World bounds `(min_x, min_y, max_x, max_y)` of the whole grid.
    pub fn bounds(&self) -> (f64, f64, f64, f64) {
        let (x0, y0) = self.transform.pixel_to_world(0.0, 0.0);
        let (x1, y1) = self
            .transform
            .pixel_to_world(self.width as f64, self.height as f64);
        (x0.min(x1), y0.min(y1), x0.max(x1), y0.max(y1))
    }

    /// New grid of the same shape and georeferencing with `f` applied to every cell.
    pub fn map<U>(&self, f: impl FnMut(&T) -> U) -> Grid<U> {
        Grid {
            width: self.width,
            height: self.height,
            transform: self.transform,
            crs: self.crs,
            nodata: None,
            data: self.data.iter().map(f).collect(),
        }
    }
}

impl<T: Copy + PartialEq + Into<f64>> Grid<T> {
    /// Bilinear interpolation between cell centres; `None` outside the centre lattice or
    /// if one of the four neighbours is no-data.
    pub fn sample_bilinear(&self, x: f64, y: f64) -> Option<f64> {
        let (c, r) = self.transform.world_to_pixel(x, y);
        let (c, r) = (c - 0.5, r - 0.5);
        if !(c.is_finite() && r.is_finite()) || c < 0.0 || r < 0.0 {
            return None;
        }
        let (c0, r0) = (c.floor() as usize, r.floor() as usize);
        let (fc, fr) = (c - c0 as f64, r - r0 as f64);
        let c1 = if fc > 0.0 { c0 + 1 } else { c0 };
        let r1 = if fr > 0.0 { r0 + 1 } else { r0 };
        let v = |cc, rr| -> Option<f64> {
            let v = *self.get(cc, rr)?;
            if self.nodata == Some(v) {
                None
            } else {
                Some(v.into())
            }
        };
        let (v00, v10, v01, v11) = (v(c0, r0)?, v(c1, r0)?, v(c0, r1)?, v(c1, r1)?);
        let top = v00 + (v10 - v00) * fc;
        let bottom = v01 + (v11 - v01) * fc;
        Some(top + (bottom - top) * fr)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Grid<f32> {
        // 3 x 2 grid of 5 m cells, top-left corner at Lambert-93 (998000, 6425000).
        let gt = GeoTransform::north_up(998_000.0, 6_425_000.0, 5.0);
        Grid::from_vec(3, 2, gt, Crs::Lambert93, vec![0., 1., 2., 10., 11., 12.]).unwrap()
    }

    #[test]
    fn shape_checks() {
        let gt = GeoTransform::north_up(0.0, 0.0, 1.0);
        assert!(Grid::from_vec(2, 2, gt, Crs::Lambert93, vec![0u8; 3]).is_err());
        assert!(GeoTransform::from_gdal([0.0, 1.0, 0.1, 0.0, 0.0, -1.0]).is_err());
        let g = GeoTransform::from_gdal(gt.to_gdal()).unwrap();
        assert_eq!(g, gt);
    }

    #[test]
    fn world_pixel_round_trip() {
        let g = sample();
        assert_eq!(g.bounds(), (998_000.0, 6_424_990.0, 998_015.0, 6_425_000.0));
        assert_eq!(g.transform().cell_center(0, 0), (998_002.5, 6_424_997.5));
        assert_eq!(g.cell_at(998_012.0, 6_424_991.0), Some((2, 1)));
        assert_eq!(g.value_at(998_012.0, 6_424_991.0), Some(&12.0));
        assert_eq!(g.cell_at(997_999.0, 6_424_995.0), None);
        assert_eq!(g.cell_at(998_015.0, 6_424_995.0), None);
        assert_eq!(g.get(3, 0), None);
    }

    #[test]
    fn bilinear() {
        let g = sample();
        // Halfway between the centres of (0,0)=0 and (1,0)=1, a quarter down to row 1.
        let v = g.sample_bilinear(998_005.0, 6_424_997.5 - 1.25).unwrap();
        assert!((v - (0.5 + 0.25 * 10.0)).abs() < 1e-9, "{v}");
        // Exactly on a centre.
        assert_eq!(g.sample_bilinear(998_012.5, 6_424_992.5), Some(12.0));
        // Outside the centre lattice.
        assert_eq!(g.sample_bilinear(998_001.0, 6_424_999.0), None);
        let g = g.with_nodata(11.0);
        assert_eq!(g.sample_bilinear(998_005.0, 6_424_995.0), None);
    }

    #[test]
    fn map_keeps_georeferencing() {
        let g = sample().map(|v| *v > 5.0);
        assert_eq!(g.data(), &[false, false, false, true, true, true]);
        assert_eq!(g.crs(), Crs::Lambert93);
        assert_eq!(g.width(), 3);
    }
}
