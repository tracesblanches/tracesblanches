//! Raster helpers: a plain grid type for the algorithms and a UInt8 class GeoTIFF writer;
//! reading goes through `skimap_common::geotiff` (`read_any` for the GDAL / rasterio
//! outputs of the oracle, `read_u8` for the class rasters).
//!
//! The algorithms work on [`Raster`] (row-major, north-up, square cells, `NaN` = no data)
//! and never on a CRS: Lambert-93 is attached when writing.

use std::fs::File;
use std::io::{BufWriter, Write};
use std::path::Path;

use skimap_common::crs::Crs;
use skimap_common::grid::{GeoTransform, Grid};
use tiff::encoder::{Compression, DeflateLevel, TiffEncoder, colortype::Gray8};
use tiff::tags::Tag;

use crate::error::{Error, Result};

/// No-data value of the UInt8 class rasters.
pub const CLASS_NODATA: u8 = 255;

/// A north-up raster with square cells.
#[derive(Debug, Clone, PartialEq)]
pub struct Raster<T> {
    /// Columns.
    pub w: usize,
    /// Rows.
    pub h: usize,
    /// Cell size, metres.
    pub cell: f64,
    /// X of the west edge (CRS units).
    pub origin_x: f64,
    /// Y of the north edge (CRS units).
    pub origin_y: f64,
    /// Values, row-major from the north-west corner.
    pub data: Vec<T>,
}

impl<T: Clone> Raster<T> {
    /// A raster of the same geometry as `like`, filled with `v`.
    pub fn filled_like<U>(like: &Raster<U>, v: T) -> Self {
        Self {
            w: like.w,
            h: like.h,
            cell: like.cell,
            origin_x: like.origin_x,
            origin_y: like.origin_y,
            data: vec![v; like.w * like.h],
        }
    }

    /// Same geometry, new values.
    pub fn with_data<U>(&self, data: Vec<U>) -> Raster<U> {
        assert_eq!(data.len(), self.w * self.h, "raster size");
        Raster {
            w: self.w,
            h: self.h,
            cell: self.cell,
            origin_x: self.origin_x,
            origin_y: self.origin_y,
            data,
        }
    }

    /// Whether two rasters share the same grid (within a millimetre).
    pub fn same_grid<U>(&self, o: &Raster<U>) -> bool {
        self.w == o.w
            && self.h == o.h
            && (self.cell - o.cell).abs() < 1e-6
            && (self.origin_x - o.origin_x).abs() < 1e-3
            && (self.origin_y - o.origin_y).abs() < 1e-3
    }

    /// Centre of a cell (CRS units).
    pub fn center(&self, col: usize, row: usize) -> (f64, f64) {
        (
            self.origin_x + (col as f64 + 0.5) * self.cell,
            self.origin_y - (row as f64 + 0.5) * self.cell,
        )
    }

    /// Cell containing a point, if inside.
    pub fn cell_at(&self, x: f64, y: f64) -> Option<(usize, usize)> {
        let c = (x - self.origin_x) / self.cell;
        let r = (self.origin_y - y) / self.cell;
        if c < 0.0 || r < 0.0 {
            return None;
        }
        let (c, r) = (c as usize, r as usize);
        (c < self.w && r < self.h).then_some((c, r))
    }
}

impl Raster<f32> {
    /// From a Float32 grid of the terrain crate.
    pub fn from_grid(g: &Grid<f32>) -> Self {
        let gt = g.transform();
        Self {
            w: g.width(),
            h: g.height(),
            cell: gt.pixel_width,
            origin_x: gt.origin_x,
            origin_y: gt.origin_y,
            data: g.data().to_vec(),
        }
    }

    /// To a Lambert-93 Float32 grid (for the terrain crate GeoTIFF writer).
    pub fn to_grid(&self) -> Grid<f32> {
        let gt = GeoTransform::north_up(self.origin_x, self.origin_y, self.cell);
        Grid::from_vec(self.w, self.h, gt, Crs::Lambert93, self.data.clone())
            .expect("consistent raster")
    }

    /// Block mean by an integer factor (a coarse cell is the mean of its valid fine cells
    /// when at least half are valid), as `skimap_terrain::dem::block_mean`.
    pub fn block_mean(&self, factor: usize) -> Self {
        Self::from_grid(&skimap_terrain::dem::block_mean(&self.to_grid(), factor))
    }
}

/// Writes a Float32 raster as a Lambert-93 GeoTIFF (Deflate, `NaN` no-data).
pub fn write_f32(path: &Path, r: &Raster<f32>) -> Result<u64> {
    Ok(skimap_common::geotiff::write_f32(path, &r.to_grid())?)
}

/// GeoKeyDirectory for Lambert-93 (EPSG:2154), pixel-is-area.
fn lambert93_keys() -> Vec<u16> {
    vec![
        1, 1, 0, 4, // header: version 1.1.0, 4 keys
        1024, 0, 1, 1, // GTModelTypeGeoKey = projected
        1025, 0, 1, 1, // GTRasterTypeGeoKey = pixel is area
        3072, 0, 1, 2154, // ProjectedCSTypeGeoKey = EPSG:2154
        3076, 0, 1, 9001, // ProjLinearUnitsGeoKey = metre
    ]
}

/// Encodes a UInt8 class raster (Lambert-93, Deflate, no-data 255) into `w`.
pub fn encode_u8<W: Write + std::io::Seek>(w: W, r: &Raster<u8>) -> Result<()> {
    let e = |m: tiff::TiffError| Error::format("GeoTIFF UInt8", m.to_string());
    let width = u32::try_from(r.w).map_err(|x| Error::format("GeoTIFF UInt8", x.to_string()))?;
    let height = u32::try_from(r.h).map_err(|x| Error::format("GeoTIFF UInt8", x.to_string()))?;
    let mut enc = TiffEncoder::new(w)
        .map_err(e)?
        .with_compression(Compression::Deflate(DeflateLevel::Best));
    let mut img = enc.new_image::<Gray8>(width, height).map_err(e)?;
    let d = img.encoder();
    d.write_tag(Tag::ModelPixelScaleTag, &[r.cell, r.cell, 0.0][..])
        .map_err(e)?;
    d.write_tag(
        Tag::ModelTiepointTag,
        &[0.0, 0.0, 0.0, r.origin_x, r.origin_y, 0.0][..],
    )
    .map_err(e)?;
    d.write_tag(Tag::GeoKeyDirectoryTag, &lambert93_keys()[..])
        .map_err(e)?;
    d.write_tag(Tag::GdalNodata, "255").map_err(e)?;
    img.write_data(&r.data).map_err(e)?;
    Ok(())
}

/// Writes a UInt8 class raster; returns the file size in bytes.
pub fn write_u8(path: &Path, r: &Raster<u8>) -> Result<u64> {
    let f = File::create(path).map_err(|e| Error::io(path, e))?;
    let mut w = BufWriter::new(f);
    encode_u8(&mut w, r)?;
    w.flush().map_err(|e| Error::io(path, e))?;
    drop(w);
    Ok(std::fs::metadata(path)
        .map_err(|e| Error::io(path, e))?
        .len())
}

impl From<skimap_common::geotiff::AnyRaster> for Raster<f32> {
    fn from(a: skimap_common::geotiff::AnyRaster) -> Self {
        Raster {
            w: a.width,
            h: a.height,
            cell: a.transform.pixel_width,
            origin_x: a.transform.origin_x,
            origin_y: a.transform.origin_y,
            data: a.data,
        }
    }
}

/// Reads a single-band GeoTIFF of any common sample type and CRS (GDAL / rasterio outputs
/// of the oracle, our own files) with `skimap_common::geotiff::read_any`: values as `f32`,
/// no-data as `NaN`.
pub fn read(path: &Path) -> Result<Raster<f32>> {
    Ok(skimap_common::geotiff::read_any(path)?.into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn u8_round_trip() {
        let r = Raster {
            w: 3,
            h: 2,
            cell: 5.0,
            origin_x: 997_999.75,
            origin_y: 6_426_000.25,
            data: vec![1u8, 2, 3, 255, 0, 2],
        };
        let mut buf = Cursor::new(Vec::new());
        encode_u8(&mut buf, &r).unwrap();
        let any = skimap_common::geotiff::decode_any(buf.get_ref()).unwrap();
        assert_eq!(any.epsg, Some(2154));
        assert_eq!(any.crs, Some(Crs::Lambert93));
        let back = Raster::from(any);
        assert_eq!((back.w, back.h), (3, 2));
        assert_eq!(back.origin_x, 997_999.75);
        assert_eq!(back.origin_y, 6_426_000.25);
        assert_eq!(back.data[..3], [1.0, 2.0, 3.0]);
        assert!(back.data[3].is_nan());
        assert_eq!(back.data[4], 0.0);
        // The same file as classes: UInt8 with its no-data value.
        let g = skimap_common::geotiff::decode_u8(buf.get_ref()).unwrap();
        assert_eq!(g.data(), &[1u8, 2, 3, 255, 0, 2]);
        assert_eq!(g.nodata(), Some(&CLASS_NODATA));
        assert_eq!(g.crs(), Crs::Lambert93);
    }

    #[test]
    fn f32_round_trip_through_terrain_writer() {
        let r = Raster {
            w: 2,
            h: 2,
            cell: 10.0,
            origin_x: 1000.0,
            origin_y: 2000.0,
            data: vec![1.5f32, f32::NAN, -3.0, 4.0],
        };
        let mut buf = Cursor::new(Vec::new());
        skimap_common::geotiff::encode_f32(&mut buf, &r.to_grid()).unwrap();
        let back = Raster::from(skimap_common::geotiff::decode_any(buf.get_ref()).unwrap());
        assert!(back.same_grid(&r));
        assert_eq!(back.data[0], 1.5);
        assert!(back.data[1].is_nan());
        assert_eq!(back.cell_at(1005.0, 1995.0), Some((0, 0)));
        assert_eq!(back.cell_at(1015.0, 1985.0), Some((1, 1)));
        assert_eq!(back.cell_at(1025.0, 1985.0), None);
        assert_eq!(back.center(1, 0), (1015.0, 1995.0));
    }
}
