//! Coordinate reference systems: WGS84, Lambert-93 and Web Mercator, in pure Rust
//! (`proj4rs`).
//!
//! The public API uses degrees for geographic coordinates and metres for projected ones,
//! always in (x, y) = (easting, northing) or (longitude, latitude) order.
//! RGF93 (the datum of Lambert-93) is treated as identical to WGS84, as in the EPSG:2154
//! definition (`towgs84=0,0,0`): the difference is below the metre.
//!
//! Tests check against PROJ 9.8.1 (pyproj 3.8.0) reference values to the millimetre.

use std::sync::OnceLock;

use proj4rs::Proj;
use proj4rs::transform::transform;
use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};
use crate::geom::{BBox, LonLat};

/// Supported coordinate reference systems.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Crs {
    /// WGS84 geographic, degrees (EPSG:4326, axis order lon, lat).
    Wgs84,
    /// RGF93 / Lambert-93, metres (EPSG:2154). IGN and ARCEP data.
    Lambert93,
    /// WGS84 / Pseudo-Mercator, metres (EPSG:3857). Web tiles.
    WebMercator,
}

const WGS84_DEF: &str = "+proj=longlat +datum=WGS84 +no_defs";
const LAMBERT93_DEF: &str = "+proj=lcc +lat_0=46.5 +lon_0=3 +lat_1=49 +lat_2=44 \
     +x_0=700000 +y_0=6600000 +ellps=GRS80 +towgs84=0,0,0,0,0,0,0 +units=m +no_defs";
const WEB_MERCATOR_DEF: &str = "+proj=merc +a=6378137 +b=6378137 +lat_ts=0 +lon_0=0 \
     +x_0=0 +y_0=0 +k=1 +units=m +nadgrids=@null +no_defs";

impl Crs {
    /// EPSG code.
    pub const fn epsg(self) -> u32 {
        match self {
            Crs::Wgs84 => 4326,
            Crs::Lambert93 => 2154,
            Crs::WebMercator => 3857,
        }
    }

    /// PROJ.4 definition string.
    pub const fn proj_string(self) -> &'static str {
        match self {
            Crs::Wgs84 => WGS84_DEF,
            Crs::Lambert93 => LAMBERT93_DEF,
            Crs::WebMercator => WEB_MERCATOR_DEF,
        }
    }

    /// Whether coordinates are geographic degrees.
    pub const fn is_geographic(self) -> bool {
        matches!(self, Crs::Wgs84)
    }

    fn proj(self) -> Result<&'static Proj> {
        static WGS84: OnceLock<std::result::Result<Proj, String>> = OnceLock::new();
        static L93: OnceLock<std::result::Result<Proj, String>> = OnceLock::new();
        static WM: OnceLock<std::result::Result<Proj, String>> = OnceLock::new();
        let cell = match self {
            Crs::Wgs84 => &WGS84,
            Crs::Lambert93 => &L93,
            Crs::WebMercator => &WM,
        };
        cell.get_or_init(|| Proj::from_proj_string(self.proj_string()).map_err(|e| e.to_string()))
            .as_ref()
            .map_err(|e| Error::Crs(format!("EPSG:{}: {e}", self.epsg())))
    }
}

/// Transforms one point from `from` to `to`. Geographic coordinates are in degrees.
pub fn transform_point(from: Crs, to: Crs, x: f64, y: f64) -> Result<(f64, f64)> {
    let mut pts = [(x, y)];
    transform_points(from, to, &mut pts)?;
    Ok(pts[0])
}

/// Transforms points in place from `from` to `to`. Geographic coordinates are in degrees.
pub fn transform_points(from: Crs, to: Crs, points: &mut [(f64, f64)]) -> Result<()> {
    if from == to {
        return Ok(());
    }
    let (src, dst) = (from.proj()?, to.proj()?);
    for p in points.iter_mut() {
        if !(p.0.is_finite() && p.1.is_finite()) {
            return Err(Error::Crs(format!("non-finite coordinate {p:?}")));
        }
        let mut q = if from.is_geographic() {
            (p.0.to_radians(), p.1.to_radians())
        } else {
            *p
        };
        transform(src, dst, &mut q).map_err(|e| {
            Error::Crs(format!(
                "EPSG:{} -> EPSG:{} {p:?}: {e}",
                from.epsg(),
                to.epsg()
            ))
        })?;
        *p = if to.is_geographic() {
            (q.0.to_degrees(), q.1.to_degrees())
        } else {
            q
        };
    }
    Ok(())
}

/// WGS84 position to Lambert-93 (x, y) metres.
pub fn to_lambert93(p: LonLat) -> Result<(f64, f64)> {
    transform_point(Crs::Wgs84, Crs::Lambert93, p.lon, p.lat)
}

/// Lambert-93 (x, y) metres to WGS84 position.
pub fn from_lambert93(x: f64, y: f64) -> Result<LonLat> {
    transform_point(Crs::Lambert93, Crs::Wgs84, x, y).map(|(lon, lat)| LonLat::new(lon, lat))
}

/// WGS84 position to Web Mercator (x, y) metres.
pub fn to_web_mercator(p: LonLat) -> Result<(f64, f64)> {
    transform_point(Crs::Wgs84, Crs::WebMercator, p.lon, p.lat)
}

/// Web Mercator (x, y) metres to WGS84 position.
pub fn from_web_mercator(x: f64, y: f64) -> Result<LonLat> {
    transform_point(Crs::WebMercator, Crs::Wgs84, x, y).map(|(lon, lat)| LonLat::new(lon, lat))
}

/// Projected bounding box `(min_x, min_y, max_x, max_y)` of a WGS84 box in `to`.
///
/// The edges are densified (`steps` points per edge) because a lon/lat rectangle is not a
/// rectangle once projected in Lambert-93.
pub fn project_bbox(b: BBox, to: Crs, steps: usize) -> Result<(f64, f64, f64, f64)> {
    let n = steps.max(1);
    let mut pts = Vec::with_capacity(4 * (n + 1));
    for i in 0..=n {
        let t = i as f64 / n as f64;
        let lon = b.west + t * (b.east - b.west);
        let lat = b.south + t * (b.north - b.south);
        pts.extend([(lon, b.south), (lon, b.north), (b.west, lat), (b.east, lat)]);
    }
    transform_points(Crs::Wgs84, to, &mut pts)?;
    let init = (
        f64::INFINITY,
        f64::INFINITY,
        f64::NEG_INFINITY,
        f64::NEG_INFINITY,
    );
    Ok(pts.iter().fold(init, |(a, b, c, d), &(x, y)| {
        (a.min(x), b.min(y), c.max(x), d.max(y))
    }))
}

/// XYZ (slippy map) tile containing a position at zoom `z`.
pub fn tile_for(p: LonLat, z: u8) -> (u32, u32) {
    let n = f64::from(1u32 << z);
    let lat = p
        .lat
        .clamp(-85.051_128_779_806_59, 85.051_128_779_806_59)
        .to_radians();
    let x = ((p.lon + 180.0) / 360.0 * n).floor();
    let y = ((1.0 - lat.tan().asinh() / std::f64::consts::PI) / 2.0 * n).floor();
    let max = n - 1.0;
    (x.clamp(0.0, max) as u32, y.clamp(0.0, max) as u32)
}

/// WGS84 bounds of the XYZ tile `(z, x, y)`.
pub fn tile_bounds(z: u8, x: u32, y: u32) -> BBox {
    let n = f64::from(1u32 << z);
    let lon = |x: f64| x / n * 360.0 - 180.0;
    let lat = |y: f64| {
        (std::f64::consts::PI * (1.0 - 2.0 * y / n))
            .sinh()
            .atan()
            .to_degrees()
    };
    BBox::new(
        lat(f64::from(y) + 1.0),
        lon(f64::from(x)),
        lat(f64::from(y)),
        lon(f64::from(x) + 1.0),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use approx::assert_abs_diff_eq;

    /// (name, lon, lat, Lambert-93 x, y, Web Mercator x, y), from PROJ 9.8.1 via pyproj 3.8.0
    /// (`Transformer.from_crs("EPSG:4326", "EPSG:2154" | "EPSG:3857", always_xy=True)`).
    const REFERENCE: &[(&str, f64, f64, f64, f64, f64, f64)] = &[
        (
            "origin",
            3.0,
            46.5,
            700000.0000,
            6600000.0000,
            333958.4724,
            5860839.8299,
        ),
        (
            "cervieres_centre",
            6.7746,
            44.8586,
            998073.3026,
            6424839.7605,
            754145.0223,
            5599288.3508,
        ),
        (
            "chapelle_centre",
            6.1118,
            46.5952,
            938164.8622,
            6615266.0624,
            680362.4638,
            5876248.9074,
        ),
        (
            "cerv_sw",
            6.66,
            44.77,
            989486.1064,
            6414583.3814,
            741387.8087,
            5585385.0785,
        ),
        (
            "cerv_ne",
            6.86,
            44.92,
            1004478.6354,
            6431977.2394,
            763651.7068,
            5608935.9038,
        ),
        (
            "chap_sw",
            6.02,
            46.54,
            931376.7356,
            6608867.3984,
            670143.3346,
            5867310.9407,
        ),
        (
            "chap_ne",
            6.2,
            46.68,
            944530.1405,
            6624945.8878,
            690180.8429,
            5889997.4518,
        ),
        (
            "paris",
            2.3522,
            48.8566,
            652469.0227,
            6862035.2594,
            261845.7062,
            6250564.3495,
        ),
    ];

    #[test]
    fn lambert93_matches_proj() {
        for &(name, lon, lat, x, y, _, _) in REFERENCE {
            let (px, py) = to_lambert93(LonLat::new(lon, lat)).unwrap();
            assert_abs_diff_eq!(px, x, epsilon = 1e-3);
            assert_abs_diff_eq!(py, y, epsilon = 1e-3);
            let back = from_lambert93(x, y).unwrap();
            assert!((back.lon - lon).abs() < 1e-8, "{name} lon {}", back.lon);
            assert!((back.lat - lat).abs() < 1e-8, "{name} lat {}", back.lat);
        }
    }

    #[test]
    fn web_mercator_matches_proj() {
        for &(name, lon, lat, _, _, x, y) in REFERENCE {
            let (px, py) = to_web_mercator(LonLat::new(lon, lat)).unwrap();
            assert_abs_diff_eq!(px, x, epsilon = 1e-3);
            assert_abs_diff_eq!(py, y, epsilon = 1e-3);
            let back = from_web_mercator(x, y).unwrap();
            assert!((back.lon - lon).abs() < 1e-8, "{name} lon {}", back.lon);
            assert!((back.lat - lat).abs() < 1e-8, "{name} lat {}", back.lat);
        }
    }

    #[test]
    fn lambert93_to_web_mercator_direct() {
        let (_, _, _, x, y, mx, my) = REFERENCE[1];
        let (px, py) = transform_point(Crs::Lambert93, Crs::WebMercator, x, y).unwrap();
        assert_abs_diff_eq!(px, mx, epsilon = 1e-3);
        assert_abs_diff_eq!(py, my, epsilon = 1e-3);
    }

    #[test]
    fn identity_and_non_finite() {
        assert_eq!(
            transform_point(Crs::Lambert93, Crs::Lambert93, 1.0, 2.0).unwrap(),
            (1.0, 2.0)
        );
        assert!(transform_point(Crs::Wgs84, Crs::Lambert93, f64::NAN, 45.0).is_err());
    }

    #[test]
    fn projected_bbox_contains_corners() {
        let b = BBox::new(44.77, 6.66, 44.92, 6.86);
        let (x0, y0, x1, y1) = project_bbox(b, Crs::Lambert93, 16).unwrap();
        for &(_, lon, lat, x, y, _, _) in &REFERENCE[3..5] {
            assert!(b.contains(LonLat::new(lon, lat)));
            assert!(
                x >= x0 - 1e-3 && x <= x1 + 1e-3 && y >= y0 - 1e-3 && y <= y1 + 1e-3,
                "{x} {y} in {x0} {y0} {x1} {y1}"
            );
        }
        assert!(x1 - x0 > 15_000.0 && y1 - y0 > 16_000.0);
    }

    #[test]
    fn tiles() {
        // Well-known: zoom 0 is a single tile; lon 0 / lat 0 at zoom 1 is tile (1, 1).
        assert_eq!(tile_for(LonLat::new(6.77, 44.86), 0), (0, 0));
        assert_eq!(tile_for(LonLat::new(0.0, 0.0), 1), (1, 1));
        let p = LonLat::new(6.7746, 44.8586);
        let (x, y) = tile_for(p, 15);
        let b = tile_bounds(15, x, y);
        assert!(b.contains(p), "{b:?}");
        // Tile edges agree with Web Mercator: width = 2πR / 2^z.
        let (ax, _) = to_web_mercator(b.south_west()).unwrap();
        let (bx, _) = to_web_mercator(b.north_east()).unwrap();
        assert_abs_diff_eq!(
            bx - ax,
            2.0 * std::f64::consts::PI * 6_378_137.0 / 32768.0,
            epsilon = 1e-6
        );
    }
}
