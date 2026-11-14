//! Geographic primitives in WGS84 (EPSG:4326), degrees.
//!
//! Coordinates are always (longitude, latitude) in code, as in GeoJSON and `geo-types`.
//! [`BBox`] stores south / west / north / east explicitly to avoid axis-order mistakes;
//! [`BBox::overpass`] gives the `s,w,n,e` order expected by Overpass.

use serde::{Deserialize, Serialize};

/// Mean Earth radius used by [`LonLat::haversine_m`] (IUGG, metres).
pub const EARTH_RADIUS_M: f64 = 6_371_008.8;

/// A WGS84 position in degrees.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct LonLat {
    /// Longitude, degrees east.
    pub lon: f64,
    /// Latitude, degrees north.
    pub lat: f64,
}

impl LonLat {
    /// Creates a position from longitude and latitude in degrees.
    pub const fn new(lon: f64, lat: f64) -> Self {
        Self { lon, lat }
    }

    /// Great-circle distance in metres (haversine, spherical Earth).
    pub fn haversine_m(self, other: LonLat) -> f64 {
        let (phi1, phi2) = (self.lat.to_radians(), other.lat.to_radians());
        let dphi = phi2 - phi1;
        let dlambda = (other.lon - self.lon).to_radians();
        let a =
            (dphi / 2.0).sin().powi(2) + phi1.cos() * phi2.cos() * (dlambda / 2.0).sin().powi(2);
        2.0 * EARTH_RADIUS_M * a.sqrt().asin()
    }
}

impl From<LonLat> for geo_types::Coord<f64> {
    fn from(p: LonLat) -> Self {
        geo_types::coord! { x: p.lon, y: p.lat }
    }
}

impl From<LonLat> for geo_types::Point<f64> {
    fn from(p: LonLat) -> Self {
        geo_types::Point::new(p.lon, p.lat)
    }
}

impl From<geo_types::Coord<f64>> for LonLat {
    fn from(c: geo_types::Coord<f64>) -> Self {
        Self::new(c.x, c.y)
    }
}

/// A WGS84 bounding box in degrees.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct BBox {
    /// Southern latitude.
    pub south: f64,
    /// Western longitude.
    pub west: f64,
    /// Northern latitude.
    pub north: f64,
    /// Eastern longitude.
    pub east: f64,
}

impl BBox {
    /// Creates a box from south, west, north, east (Overpass order).
    pub const fn new(south: f64, west: f64, north: f64, east: f64) -> Self {
        Self {
            south,
            west,
            north,
            east,
        }
    }

    /// South-west corner.
    pub const fn south_west(&self) -> LonLat {
        LonLat::new(self.west, self.south)
    }

    /// North-east corner.
    pub const fn north_east(&self) -> LonLat {
        LonLat::new(self.east, self.north)
    }

    /// Centre of the box (arithmetic mean of the bounds).
    pub fn center(&self) -> LonLat {
        LonLat::new(
            (self.west + self.east) / 2.0,
            (self.south + self.north) / 2.0,
        )
    }

    /// Whether the position lies inside the box (bounds included).
    pub fn contains(&self, p: LonLat) -> bool {
        p.lon >= self.west && p.lon <= self.east && p.lat >= self.south && p.lat <= self.north
    }

    /// `s,w,n,e` string for Overpass `(bbox)` filters.
    pub fn overpass(&self) -> String {
        format!("{},{},{},{}", self.south, self.west, self.north, self.east)
    }

    /// `minLon,minLat,maxLon,maxLat` string (WFS, OGC API, most REST APIs).
    pub fn lon_lat_order(&self) -> String {
        format!("{},{},{},{}", self.west, self.south, self.east, self.north)
    }

    /// Approximate width and height in metres, measured along the centre lines.
    pub fn size_m(&self) -> (f64, f64) {
        let c = self.center();
        let width = LonLat::new(self.west, c.lat).haversine_m(LonLat::new(self.east, c.lat));
        let height = LonLat::new(c.lon, self.south).haversine_m(LonLat::new(c.lon, self.north));
        (width, height)
    }
}

impl From<BBox> for geo_types::Rect<f64> {
    fn from(b: BBox) -> Self {
        geo_types::Rect::new(
            geo_types::coord! { x: b.west, y: b.south },
            geo_types::coord! { x: b.east, y: b.north },
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use approx::assert_relative_eq;

    #[test]
    fn haversine_one_degree_of_latitude() {
        // One degree of arc on the sphere of radius EARTH_RADIUS_M.
        let d = LonLat::new(6.0, 45.0).haversine_m(LonLat::new(6.0, 46.0));
        assert_relative_eq!(d, EARTH_RADIUS_M * 1f64.to_radians(), max_relative = 1e-12);
    }

    #[test]
    fn bbox_strings_and_contains() {
        let b = BBox::new(44.77, 6.66, 44.92, 6.86);
        assert_eq!(b.overpass(), "44.77,6.66,44.92,6.86");
        assert_eq!(b.lon_lat_order(), "6.66,44.77,6.86,44.92");
        assert!(b.contains(LonLat::new(6.7746, 44.8586)));
        assert!(!b.contains(LonLat::new(6.1118, 46.5952)));
        let r: geo_types::Rect<f64> = b.into();
        assert_eq!(r.min().x, 6.66);
        assert_eq!(r.max().y, 44.92);
    }

    #[test]
    fn bbox_size() {
        // Cervières: 0.20° of longitude at ~44.85° N ≈ 15.8 km, 0.15° of latitude ≈ 16.7 km.
        let (w, h) = BBox::new(44.77, 6.66, 44.92, 6.86).size_m();
        assert!((15_500.0..16_000.0).contains(&w), "width {w}");
        assert!((16_500.0..16_800.0).contains(&h), "height {h}");
    }
}
