//! XYZ tile arithmetic (Web Mercator, 256-pixel tiles).

use skimap_common::crs::tile_for;
use skimap_common::geom::{BBox, LonLat};

/// Tile address `(z, x, y)`.
pub type TileKey = (u8, u32, u32);
/// A tile and its bytes.
pub type TileData = (TileKey, Vec<u8>);

/// Tile size of every raster of the packs.
pub const TILE_SIZE: u32 = 256;

/// Inclusive tile range `(x_min, y_min, x_max, y_max)` covering `bbox` at zoom `z`.
pub fn tile_range(bbox: BBox, z: u8) -> (u32, u32, u32, u32) {
    let (x0, y0) = tile_for(LonLat::new(bbox.west, bbox.north), z);
    let (x1, y1) = tile_for(LonLat::new(bbox.east, bbox.south), z);
    (x0, y0, x1, y1)
}

/// Every tile `(z, x, y)` covering `bbox` for `z` in `minzoom..=maxzoom`.
pub fn tiles_covering(bbox: BBox, minzoom: u8, maxzoom: u8) -> Vec<(u8, u32, u32)> {
    let mut out = Vec::new();
    for z in minzoom..=maxzoom {
        let (x0, y0, x1, y1) = tile_range(bbox, z);
        for x in x0..=x1 {
            for y in y0..=y1 {
                out.push((z, x, y));
            }
        }
    }
    out
}

/// Global pixel coordinates (fractional) of a position at zoom `z` (every `u8` zoom: the
/// zoom of a raster layer comes from the downloaded manifest; `2^z` as a float cannot
/// overflow like `1u32 << z` did, found by fuzzing, Q3 #103).
pub fn global_pixel(p: LonLat, z: u8) -> (f64, f64) {
    let n = f64::from(TILE_SIZE) * f64::from(z).exp2();
    let lat = p
        .lat
        .clamp(-85.051_128_779_806_59, 85.051_128_779_806_59)
        .to_radians();
    let x = (p.lon + 180.0) / 360.0 * n;
    let y = (1.0 - lat.tan().asinh() / std::f64::consts::PI) / 2.0 * n;
    (x, y)
}

/// Tile and pixel inside the tile containing a position at zoom `z`.
pub fn tile_pixel(p: LonLat, z: u8) -> (u32, u32, u32, u32) {
    let (gx, gy) = global_pixel(p, z);
    let max = f64::from(TILE_SIZE) * f64::from(z).exp2() - 1.0;
    let (gx, gy) = (
        gx.floor().clamp(0.0, max) as u64,
        gy.floor().clamp(0.0, max) as u64,
    );
    let ts = u64::from(TILE_SIZE);
    (
        (gx / ts) as u32,
        (gy / ts) as u32,
        (gx % ts) as u32,
        (gy % ts) as u32,
    )
}

/// Longitude of a global pixel column (fractional) at zoom `z`.
pub fn pixel_lon(gx: f64, z: u8) -> f64 {
    let n = f64::from(TILE_SIZE) * f64::from(z).exp2();
    gx / n * 360.0 - 180.0
}

/// Latitude of a global pixel row (fractional) at zoom `z`.
pub fn pixel_lat(gy: f64, z: u8) -> f64 {
    let n = f64::from(TILE_SIZE) * f64::from(z).exp2();
    (std::f64::consts::PI * (1.0 - 2.0 * gy / n))
        .sinh()
        .atan()
        .to_degrees()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pixel_round_trip_and_tile_agree_with_common() {
        let p = LonLat::new(6.7746, 44.8586);
        for z in [0u8, 10, 15, 17] {
            let (tx, ty, px, py) = tile_pixel(p, z);
            assert_eq!((tx, ty), tile_for(p, z));
            assert!(px < TILE_SIZE && py < TILE_SIZE);
            let (gx, gy) = global_pixel(p, z);
            assert!((pixel_lon(gx, z) - p.lon).abs() < 1e-9);
            assert!((pixel_lat(gy, z) - p.lat).abs() < 1e-9);
        }
    }

    #[test]
    fn range_covers_bbox() {
        let b = BBox::new(44.77, 6.66, 44.92, 6.86);
        let (x0, y0, x1, y1) = tile_range(b, 12);
        assert!(x0 <= x1 && y0 <= y1);
        assert_eq!(
            tiles_covering(b, 12, 12).len() as u32,
            (x1 - x0 + 1) * (y1 - y0 + 1)
        );
        assert_eq!(tiles_covering(b, 0, 0), vec![(0, 0, 0)]);
    }
}
