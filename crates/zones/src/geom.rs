//! Geometry helpers: site box polygon, clipping, geodesic areas.

use geo::orient::{Direction, Orient};
use geo::{BooleanOps, BoundingRect, Coord, GeodesicArea, LineString, MultiPolygon, Polygon, Rect};
use skimap_common::crs::{Crs, transform_points};
use skimap_common::geom::BBox;

use crate::error::Result;

/// Polygon of a WGS84 box, with `steps` points per edge (densified so that it stays
/// exact once projected, e.g. to Lambert-93). Counter-clockwise exterior.
pub fn bbox_polygon(b: BBox, steps: usize) -> Polygon<f64> {
    let n = steps.max(1);
    let mut ring = Vec::with_capacity(4 * n + 1);
    let lerp = |a: f64, b: f64, i: usize| a + (b - a) * i as f64 / n as f64;
    for i in 0..n {
        ring.push(Coord {
            x: lerp(b.west, b.east, i),
            y: b.south,
        });
    }
    for i in 0..n {
        ring.push(Coord {
            x: b.east,
            y: lerp(b.south, b.north, i),
        });
    }
    for i in 0..n {
        ring.push(Coord {
            x: lerp(b.east, b.west, i),
            y: b.north,
        });
    }
    for i in 0..n {
        ring.push(Coord {
            x: b.west,
            y: lerp(b.north, b.south, i),
        });
    }
    ring.push(ring[0]);
    Polygon::new(LineString::new(ring), vec![])
}

/// Whether two rectangles overlap (boundaries included).
pub fn rects_overlap(a: Rect<f64>, b: Rect<f64>) -> bool {
    a.min().x <= b.max().x
        && b.min().x <= a.max().x
        && a.min().y <= b.max().y
        && b.min().y <= a.max().y
}

/// Result of clipping a geometry to the site box.
#[derive(Debug, Clone)]
pub struct Clipped {
    /// Geometry inside the box.
    pub geometry: MultiPolygon<f64>,
    /// Whether some of the geometry was outside the box.
    pub clipped: bool,
}

/// Clips a WGS84 multipolygon to the site box after [`normalize`]. Returns `None` when
/// nothing is left. Only the parts whose bounding box touches the site box are kept
/// before the boolean operations, which keeps large sources (a 400 km² Biodiv'sports
/// area, a whole canton) cheap.
pub fn clip_to_bbox(mp: &MultiPolygon<f64>, b: BBox) -> Option<Clipped> {
    let rect: Rect<f64> = b.into();
    let bounds = mp.bounding_rect()?;
    if !rects_overlap(bounds, rect) {
        return None;
    }
    let near: Vec<Polygon<f64>> =
        mp.0.iter()
            .filter(|p| p.bounding_rect().is_some_and(|r| rects_overlap(r, rect)))
            .cloned()
            .collect();
    let dropped_parts = near.len() < mp.0.len();
    let mp = normalize(&MultiPolygon::new(near));
    let bounds = mp.bounding_rect()?;
    let inside = bounds.min().x >= rect.min().x
        && bounds.max().x <= rect.max().x
        && bounds.min().y >= rect.min().y
        && bounds.max().y <= rect.max().y;
    if inside {
        return Some(Clipped {
            geometry: mp,
            clipped: dropped_parts,
        });
    }
    let out = mp.intersection(&rect.to_polygon());
    if out.0.is_empty() || geodesic_area_m2(&out) < 1.0 {
        return None;
    }
    Some(Clipped {
        geometry: out,
        clipped: true,
    })
}

/// Reprojects every coordinate of a multipolygon with `skimap_common::crs`.
pub fn reproject(mp: &MultiPolygon<f64>, from: Crs, to: Crs) -> Result<MultiPolygon<f64>> {
    let ring = |ls: &LineString<f64>| -> Result<LineString<f64>> {
        let mut pts: Vec<(f64, f64)> = ls.0.iter().map(|c| (c.x, c.y)).collect();
        transform_points(from, to, &mut pts)?;
        Ok(LineString::new(
            pts.into_iter().map(|(x, y)| Coord { x, y }).collect(),
        ))
    };
    let mut out = Vec::with_capacity(mp.0.len());
    for p in &mp.0 {
        let interiors = p.interiors().iter().map(ring).collect::<Result<Vec<_>>>()?;
        out.push(Polygon::new(ring(p.exterior())?, interiors));
    }
    Ok(MultiPolygon::new(out))
}

/// Union of many multipolygons. Rings are first oriented consistently (exterior
/// counter-clockwise, holes clockwise), as required by [`geo::unary_union`].
pub fn union_all<'a>(parts: impl IntoIterator<Item = &'a MultiPolygon<f64>>) -> MultiPolygon<f64> {
    let oriented: Vec<Polygon<f64>> = parts
        .into_iter()
        .flat_map(|mp| mp.0.iter().map(|p| p.orient(Direction::Default)))
        .collect();
    if oriented.is_empty() {
        return MultiPolygon::new(vec![]);
    }
    geo::unary_union(&oriented)
}

/// Geodesic area (WGS84 ellipsoid) of a lon/lat multipolygon, square metres.
///
/// Computed ring by ring (|exterior| − Σ|holes|) so that the result does not depend on
/// the ring orientation: `geo`'s polygon area of a clockwise exterior is the area of the
/// rest of the Earth, and both the sources (Biodiv'sports, geo.api) and boolean
/// operations can produce clockwise rings.
pub fn geodesic_area_m2(mp: &MultiPolygon<f64>) -> f64 {
    let ring = |ls: &LineString<f64>| {
        Polygon::new(ls.clone(), vec![])
            .geodesic_area_signed()
            .abs()
    };
    mp.0.iter()
        .map(|p| ring(p.exterior()) - p.interiors().iter().map(ring).sum::<f64>())
        .sum()
}

/// Valid, consistently oriented version of a multipolygon: overlapping parts are merged
/// (non-zero rule after orientation). Sources such as Biodiv'sports publish invalid
/// multipolygons whose parts overlap, which an even-odd boolean operation would punch
/// holes into.
pub fn normalize(mp: &MultiPolygon<f64>) -> MultiPolygon<f64> {
    union_all([mp])
}

/// Geodesic area in hectares, rounded to 0.0001 ha (1 m²).
pub fn area_ha(mp: &MultiPolygon<f64>) -> f64 {
    geodesic_area_m2(mp).round() / 10_000.0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bbox_polygon_is_closed_and_densified() {
        let b = BBox::new(46.54, 6.02, 46.68, 6.20);
        let p = bbox_polygon(b, 10);
        assert_eq!(p.exterior().0.len(), 41);
        assert_eq!(p.exterior().0.first(), p.exterior().0.last());
        // Geodesic area of the Chapelle-des-Bois box: 0.18° x 0.14° at 46.6° N.
        let a = geodesic_area_m2(&MultiPolygon::new(vec![p])) / 1e6;
        assert!((214.0..216.5).contains(&a), "{a} km2");
    }

    #[test]
    fn area_ignores_orientation_and_overlaps_are_merged() {
        let ccw = bbox_polygon(BBox::new(46.0, 6.0, 46.01, 6.01), 1);
        let cw = Polygon::new(
            LineString::new(ccw.exterior().0.iter().rev().copied().collect()),
            vec![],
        );
        let a = geodesic_area_m2(&MultiPolygon::new(vec![ccw.clone()]));
        let b = geodesic_area_m2(&MultiPolygon::new(vec![cw.clone()]));
        // 0.01° x 0.01° at 46° N: about 1.11 km x 0.77 km.
        assert!((850_000.0..870_000.0).contains(&a), "{a}");
        assert!((a - b).abs() < 1e-6 * a, "{a} vs {b}");
        // Two overlapping copies (an invalid multipolygon) count once after normalize.
        let twice = MultiPolygon::new(vec![ccw, cw]);
        let n = normalize(&twice);
        assert!((geodesic_area_m2(&n) - a).abs() < 1e-6 * a);
        let c = clip_to_bbox(&twice, BBox::new(45.0, 5.0, 47.0, 7.0)).unwrap();
        assert!((geodesic_area_m2(&c.geometry) - a).abs() < 1e-6 * a);
    }

    #[test]
    fn reprojection_to_lambert93() {
        let mp = MultiPolygon::new(vec![bbox_polygon(BBox::new(46.54, 6.02, 46.68, 6.20), 20)]);
        let l93 = reproject(&mp, Crs::Wgs84, Crs::Lambert93).unwrap();
        let planar = geo::Area::unsigned_area(&l93) / 1e6;
        let geodesic = geodesic_area_m2(&mp) / 1e6;
        // Lambert-93 scale factor is ~0.999 at 46.6° N: areas agree within 0.3 %.
        assert!(
            (planar / geodesic - 1.0).abs() < 3e-3,
            "{planar} vs {geodesic}"
        );
    }

    #[test]
    fn clipping() {
        let b = BBox::new(0.0, 0.0, 1.0, 1.0);
        let inside = MultiPolygon::new(vec![bbox_polygon(BBox::new(0.2, 0.2, 0.4, 0.4), 1)]);
        let c = clip_to_bbox(&inside, b).unwrap();
        assert!(!c.clipped);
        let half = MultiPolygon::new(vec![bbox_polygon(BBox::new(0.5, 0.5, 1.5, 1.5), 1)]);
        let c = clip_to_bbox(&half, b).unwrap();
        assert!(c.clipped);
        let ratio = geodesic_area_m2(&c.geometry) / geodesic_area_m2(&half);
        assert!((0.24..0.26).contains(&ratio), "{ratio}");
        let outside = MultiPolygon::new(vec![bbox_polygon(BBox::new(2.0, 2.0, 3.0, 3.0), 1)]);
        assert!(clip_to_bbox(&outside, b).is_none());
    }
}
