//! Inputs sampled along a track: rasters ([`Field`]), polygon sets ([`Areas`]), zone rules
//! ([`ZoneSource`]).
//!
//! Everything is read at Lambert-93 points (the 5 m grid of `skimap-terrain`). Adapters
//! cover the phone formats (`SKR1` rasters of `skimap_core::offpiste`: elevation in
//! decimetres, slope in half degrees, exposure classes) and the pipeline grids
//! (`Grid<f32>`).

use geo::{BoundingRect, Contains, MultiPolygon, Point, Rect};
use serde::{Deserialize, Serialize};
use skimap_common::geom::LonLat;
use skimap_common::grid::Grid;
use skimap_core::zone_eval;
use skimap_zones::jiff::Timestamp;
use skimap_zones::layer::ZoneLayer;
use skimap_zones::rules::{RuleSet, Topic};

/// A raster read at Lambert-93 points.
pub trait Field: Sync {
    /// Value at `(x, y)`, `None` outside the raster or on no data.
    fn at(&self, x: f64, y: f64) -> Option<f32>;
}

/// Float grid, nearest cell (`NaN` = no data).
pub struct Nearest<'a>(pub &'a Grid<f32>);

impl Field for Nearest<'_> {
    fn at(&self, x: f64, y: f64) -> Option<f32> {
        self.0.value_at(x, y).copied().filter(|v| v.is_finite())
    }
}

/// Float grid, bilinear interpolation (elevation).
pub struct Bilinear<'a>(pub &'a Grid<f32>);

impl Field for Bilinear<'_> {
    fn at(&self, x: f64, y: f64) -> Option<f32> {
        match self.0.sample_bilinear(x, y) {
            Some(v) if v.is_finite() => Some(v as f32),
            // Next to a no-data cell the bilinear sample is missing: fall back to the
            // nearest cell (track edges, LiDAR coverage edge).
            _ => self.0.value_at(x, y).copied().filter(|v| v.is_finite()),
        }
    }
}

/// `SKR1` elevation (`u16` decimetres, `skimap_core::offpiste::ELEVATION_NODATA`),
/// bilinear on the four surrounding cell centres.
pub struct ElevationDm<'a>(pub &'a Grid<u16>);

impl Field for ElevationDm<'_> {
    fn at(&self, x: f64, y: f64) -> Option<f32> {
        let g = self.0;
        let (c, r) = g.transform().world_to_pixel(x, y);
        let (fc, fr) = (c - 0.5, r - 0.5);
        let (c0, r0) = (fc.floor(), fr.floor());
        let (tx, ty) = ((fc - c0) as f32, (fr - r0) as f32);
        let get = |cc: f64, rr: f64| -> Option<f32> {
            if cc < 0.0 || rr < 0.0 {
                return None;
            }
            g.get(cc as usize, rr as usize)
                .and_then(|v| skimap_core::offpiste::elevation_m(*v))
        };
        match (
            get(c0, r0),
            get(c0 + 1.0, r0),
            get(c0, r0 + 1.0),
            get(c0 + 1.0, r0 + 1.0),
        ) {
            (Some(a), Some(b), Some(cv), Some(d)) => {
                Some((a * (1.0 - tx) + b * tx) * (1.0 - ty) + (cv * (1.0 - tx) + d * tx) * ty)
            }
            _ => g
                .value_at(x, y)
                .and_then(|v| skimap_core::offpiste::elevation_m(*v)),
        }
    }
}

/// `SKR1` slope (`u8` half degrees, `skimap_core::offpiste::SLOPE_NODATA`): the centre of
/// the half-degree bin.
pub struct SlopeHalfDeg<'a>(pub &'a Grid<u8>);

impl Field for SlopeHalfDeg<'_> {
    fn at(&self, x: f64, y: f64) -> Option<f32> {
        self.0
            .value_at(x, y)
            .filter(|v| **v != skimap_core::offpiste::SLOPE_NODATA)
            .map(|v| f32::from(*v) / 2.0 + 0.25)
    }
}

/// `u8` class raster (avalanche exposure 1–3), `nodata` excluded.
pub struct Classes<'a> {
    /// Grid.
    pub grid: &'a Grid<u8>,
    /// No-data code.
    pub nodata: u8,
}

impl Field for Classes<'_> {
    fn at(&self, x: f64, y: f64) -> Option<f32> {
        self.grid
            .value_at(x, y)
            .filter(|v| **v != self.nodata)
            .map(|v| f32::from(*v))
    }
}

/// A set of WGS84 polygons (zones without any mobile network, `coverage` layer) and the
/// area where the set is known (`analysed_area`): outside it the answer is unknown.
pub struct Areas {
    polys: Vec<(Rect<f64>, MultiPolygon<f64>)>,
    known: Option<Vec<(Rect<f64>, MultiPolygon<f64>)>>,
}

impl Areas {
    /// From polygons and, if known, the analysed area (`None`: known everywhere).
    pub fn new(polys: Vec<MultiPolygon<f64>>, known: Option<Vec<MultiPolygon<f64>>>) -> Self {
        let with_rect = |v: Vec<MultiPolygon<f64>>| {
            v.into_iter()
                .filter_map(|p| p.bounding_rect().map(|r| (r, p)))
                .collect::<Vec<_>>()
        };
        Self {
            polys: with_rect(polys),
            known: known.map(with_rect),
        }
    }

    /// Reads a GeoJSON FeatureCollection of Polygon / MultiPolygon features.
    pub fn polygons_from_geojson(text: &str) -> Result<Vec<MultiPolygon<f64>>, String> {
        let v: serde_json::Value = serde_json::from_str(text).map_err(|e| e.to_string())?;
        let mut out = Vec::new();
        for f in v["features"].as_array().ok_or("no features")? {
            let g = &f["geometry"];
            let polys = match g["type"].as_str() {
                Some("Polygon") => vec![g["coordinates"].clone()],
                Some("MultiPolygon") => g["coordinates"].as_array().cloned().unwrap_or_default(),
                _ => continue,
            };
            let mut mp = Vec::new();
            for p in polys {
                let rings: Vec<geo::LineString<f64>> = p
                    .as_array()
                    .ok_or("bad polygon")?
                    .iter()
                    .map(|ring| {
                        ring.as_array()
                            .map(|pts| {
                                pts.iter()
                                    .filter_map(|c| {
                                        Some(geo::coord! { x: c[0].as_f64()?, y: c[1].as_f64()? })
                                    })
                                    .collect::<Vec<_>>()
                                    .into()
                            })
                            .unwrap_or_else(|| geo::LineString::new(vec![]))
                    })
                    .collect();
                let mut it = rings.into_iter();
                if let Some(ext) = it.next() {
                    mp.push(geo::Polygon::new(ext, it.collect()));
                }
            }
            out.push(MultiPolygon(mp));
        }
        Ok(out)
    }

    fn hit(set: &[(Rect<f64>, MultiPolygon<f64>)], p: LonLat) -> bool {
        let pt = Point::new(p.lon, p.lat);
        set.iter().any(|(r, mp)| {
            p.lon >= r.min().x
                && p.lon <= r.max().x
                && p.lat >= r.min().y
                && p.lat <= r.max().y
                && mp.contains(&pt)
        })
    }

    /// Whether `p` is inside the set; `None` outside the known area.
    pub fn contains(&self, p: LonLat) -> Option<bool> {
        if let Some(k) = &self.known
            && !Self::hit(k, p)
        {
            return None;
        }
        Some(Self::hit(&self.polys, p))
    }
}

/// Zone restrictions at one point, at the date of the outing.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ZoneFlags {
    /// Access forbidden.
    pub access_forbidden: bool,
    /// Leaving the marked itineraries forbidden (APPB of the Risoux in winter, Swiss
    /// districts francs).
    pub leave_routes_forbidden: bool,
    /// Wildlife area to avoid (Biodiv'sports in season).
    pub avoid: bool,
    /// Names of the zones with an active rule.
    pub names: Vec<String>,
}

impl ZoneFlags {
    /// Whether any restriction applies.
    pub fn any(&self) -> bool {
        self.access_forbidden || self.leave_routes_forbidden || self.avoid
    }
}

/// Zone rules read at a point.
pub trait ZoneSource: Sync {
    /// Flags at `p`, `None` when the point is outside every jurisdiction known.
    fn at(&self, p: LonLat) -> Option<ZoneFlags>;
}

/// Zones and rules of a site (`zones` layer) at one instant, through
/// `skimap_core::zone_eval` (C8).
pub struct SiteZones<'a> {
    /// Zones of the site.
    pub layer: &'a ZoneLayer,
    /// Rules.
    pub rules: &'a RuleSet,
    /// Instant of the outing.
    pub at: Timestamp,
}

impl ZoneSource for SiteZones<'_> {
    fn at(&self, p: LonLat) -> Option<ZoneFlags> {
        let ev = zone_eval::evaluate(self.layer, self.rules, p, self.at);
        if !ev.covered {
            return None;
        }
        use zone_eval::Verdict;
        let v = |t: Topic| ev.verdict(t).verdict;
        let access_forbidden = v(Topic::Access) == Verdict::Forbidden;
        let leave_routes_forbidden = v(Topic::LeaveRoutes) == Verdict::Forbidden;
        let avoid = [Topic::Access, Topic::LeaveRoutes]
            .iter()
            .any(|t| v(*t) == Verdict::AvoidRecommended);
        let mut names: Vec<String> = Vec::new();
        if access_forbidden || leave_routes_forbidden || avoid {
            for z in &ev.zones {
                // Only the zones carrying the restriction (not the country or the
                // département, whose active rules concern bivouac or fire).
                let active = ev.rules.iter().any(|r| {
                    r.active()
                        && matches!(r.topic, Topic::Access | Topic::LeaveRoutes)
                        && matches!(r.verdict, Verdict::Forbidden | Verdict::AvoidRecommended)
                        && r.zones.iter().any(|id| id == &z.id)
                });
                if active && !z.name.is_empty() && !names.contains(&z.name) {
                    names.push(z.name.clone());
                }
            }
        }
        Some(ZoneFlags {
            access_forbidden,
            leave_routes_forbidden,
            avoid,
            names,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use skimap_common::crs::Crs;
    use skimap_common::grid::GeoTransform;

    #[test]
    fn skr1_adapters_decode_units() {
        let gt = GeoTransform::north_up(0.0, 10.0, 5.0);
        let elev = Grid::from_vec(
            2,
            2,
            gt,
            Crs::Lambert93,
            vec![10000u16, 10010, 10020, 10030],
        )
        .unwrap();
        // Centre of the 2 × 2 block: mean of the four centres = 1001.5 m.
        let z = ElevationDm(&elev).at(5.0, 5.0).unwrap();
        assert!((z - 1001.5).abs() < 1e-3, "{z}");
        let slope = Grid::from_vec(1, 1, gt, Crs::Lambert93, vec![41u8]).unwrap();
        assert_eq!(SlopeHalfDeg(&slope).at(2.0, 8.0), Some(20.75));
        let nod = Grid::from_vec(1, 1, gt, Crs::Lambert93, vec![255u8]).unwrap();
        assert_eq!(SlopeHalfDeg(&nod).at(2.0, 8.0), None);
        let ex = Classes {
            grid: &nod,
            nodata: 255,
        };
        assert_eq!(ex.at(2.0, 8.0), None);
    }

    #[test]
    fn areas_known_and_unknown() {
        let text = r#"{"type":"FeatureCollection","features":[
          {"type":"Feature","properties":{},"geometry":{"type":"Polygon","coordinates":[[[0,0],[1,0],[1,1],[0,1],[0,0]]]}}]}"#;
        let known = r#"{"type":"FeatureCollection","features":[
          {"type":"Feature","properties":{},"geometry":{"type":"Polygon","coordinates":[[[0,0],[3,0],[3,3],[0,3],[0,0]]]}}]}"#;
        let a = Areas::new(
            Areas::polygons_from_geojson(text).unwrap(),
            Some(Areas::polygons_from_geojson(known).unwrap()),
        );
        assert_eq!(a.contains(LonLat::new(0.5, 0.5)), Some(true));
        assert_eq!(a.contains(LonLat::new(2.0, 2.0)), Some(false));
        assert_eq!(a.contains(LonLat::new(5.0, 5.0)), None);
    }
}
