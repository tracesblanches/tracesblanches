//! Domains of the national census (P3, `data/out/france/domaines/`): identity, commune,
//! footprint, and the distance from a point to a footprint in metres.

use std::collections::BTreeMap;
use std::path::Path;

use serde_json::Value;
use skimap_common::geom::{BBox, EARTH_RADIUS_M, LonLat};

use crate::error::{Result, StationsError};

/// A census domain.
#[derive(Debug, Clone)]
pub struct Domain {
    /// Stable id (`site-<relation>` or `net-<way>`).
    pub id: String,
    /// Display name.
    pub name: String,
    /// Other names (OpenSkiMap areas).
    pub alt_names: Vec<String>,
    /// Commune of the representative point.
    pub commune: String,
    /// INSEE code of that commune.
    pub insee: String,
    /// Department code.
    pub departement: String,
    /// Representative point (on the network).
    pub point: LonLat,
    /// Nordic length, km.
    pub km_nordic: f64,
    /// OpenSkiMap areas overlapping the domain (id).
    pub openskimap: Vec<String>,
    /// OSM `site=piste` relation giving the identity, if any.
    pub site_relation: Option<i64>,
    /// Footprint polygons, rings of WGS84 points (first ring exterior, then holes).
    pub polygons: Vec<Vec<Vec<LonLat>>>,
    /// Bounding box of the footprint.
    pub bbox: BBox,
}

/// Local metric projection around a point (equirectangular), precise to well under 1 %
/// over the few kilometres the distances are used for.
#[derive(Debug, Clone, Copy)]
pub struct Local {
    lon0: f64,
    lat0: f64,
    kx: f64,
    ky: f64,
}

impl Local {
    /// Projection centred on `p`.
    pub fn new(p: LonLat) -> Self {
        let ky = EARTH_RADIUS_M * std::f64::consts::PI / 180.0;
        Self {
            lon0: p.lon,
            lat0: p.lat,
            kx: ky * p.lat.to_radians().cos(),
            ky,
        }
    }

    /// Metres east and north of the centre.
    pub fn xy(&self, p: LonLat) -> (f64, f64) {
        ((p.lon - self.lon0) * self.kx, (p.lat - self.lat0) * self.ky)
    }

    /// Point at `xy` metres east and north of the centre (inverse of [`Local::xy`]).
    pub fn lonlat(&self, xy: (f64, f64)) -> LonLat {
        LonLat::new(self.lon0 + xy.0 / self.kx, self.lat0 + xy.1 / self.ky)
    }
}

fn seg_dist(p: (f64, f64), a: (f64, f64), b: (f64, f64)) -> f64 {
    let (dx, dy) = (b.0 - a.0, b.1 - a.1);
    let l2 = dx * dx + dy * dy;
    let t = if l2 == 0.0 {
        0.0
    } else {
        (((p.0 - a.0) * dx + (p.1 - a.1) * dy) / l2).clamp(0.0, 1.0)
    };
    let (x, y) = (a.0 + t * dx - p.0, a.1 + t * dy - p.1);
    (x * x + y * y).sqrt()
}

fn in_ring(p: (f64, f64), ring: &[(f64, f64)]) -> bool {
    let mut inside = false;
    let n = ring.len();
    let mut j = n.wrapping_sub(1);
    for i in 0..n {
        let (xi, yi) = ring[i];
        let (xj, yj) = ring[j];
        if (yi > p.1) != (yj > p.1) && p.0 < (xj - xi) * (p.1 - yi) / (yj - yi) + xi {
            inside = !inside;
        }
        j = i;
    }
    inside
}

impl Domain {
    /// Distance from `p` to the footprint, m (0 inside).
    pub fn distance_m(&self, p: LonLat) -> f64 {
        let local = Local::new(p);
        let o = (0.0, 0.0);
        let mut best = f64::INFINITY;
        for poly in &self.polygons {
            let rings: Vec<Vec<(f64, f64)>> = poly
                .iter()
                .map(|r| r.iter().map(|q| local.xy(*q)).collect())
                .collect();
            if let Some((ext, holes)) = rings.split_first()
                && in_ring(o, ext)
                && !holes.iter().any(|h| in_ring(o, h))
            {
                return 0.0;
            }
            for r in &rings {
                for w in r.windows(2) {
                    best = best.min(seg_dist(o, w[0], w[1]));
                }
            }
        }
        best
    }

    /// Whether `p` may lie within `m` metres of the footprint (bounding-box test).
    pub fn near_bbox(&self, p: LonLat, m: f64) -> bool {
        let dlat = m / (EARTH_RADIUS_M * std::f64::consts::PI / 180.0);
        let dlon = dlat / self.bbox.south.to_radians().cos().max(0.2);
        p.lat >= self.bbox.south - dlat
            && p.lat <= self.bbox.north + dlat
            && p.lon >= self.bbox.west - dlon
            && p.lon <= self.bbox.east + dlon
    }
}

fn str_of(v: &Value, k: &str) -> String {
    v.get(k)
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_owned()
}

fn ring(v: &Value) -> Option<Vec<LonLat>> {
    v.as_array()?
        .iter()
        .map(|c| {
            let c = c.as_array()?;
            Some(LonLat::new(c.first()?.as_f64()?, c.get(1)?.as_f64()?))
        })
        .collect()
}

/// Parses `domaines.geojson`.
pub fn parse(v: &Value) -> Result<Vec<Domain>> {
    let feats = v
        .get("features")
        .and_then(Value::as_array)
        .ok_or_else(|| StationsError::format("domaines.geojson", "no features"))?;
    let mut out = Vec::with_capacity(feats.len());
    for f in feats {
        let p = &f["properties"];
        let id = str_of(p, "id");
        let g = &f["geometry"];
        let polys: Vec<&Value> = match g.get("type").and_then(Value::as_str) {
            Some("MultiPolygon") => g["coordinates"].as_array().map(|a| a.iter().collect()),
            Some("Polygon") => Some(vec![&g["coordinates"]]),
            _ => None,
        }
        .ok_or_else(|| StationsError::format("domaines.geojson", format!("{id}: no polygon")))?;
        let mut polygons = Vec::new();
        let (mut s, mut w, mut n, mut e) = (90.0f64, 180.0f64, -90.0f64, -180.0f64);
        for poly in polys {
            let rings: Vec<Vec<LonLat>> = poly
                .as_array()
                .map(|rs| rs.iter().filter_map(ring).collect())
                .unwrap_or_default();
            for q in rings.iter().flatten() {
                s = s.min(q.lat);
                n = n.max(q.lat);
                w = w.min(q.lon);
                e = e.max(q.lon);
            }
            if !rings.is_empty() {
                polygons.push(rings);
            }
        }
        if polygons.is_empty() {
            return Err(StationsError::format(
                "domaines.geojson",
                format!("{id}: empty footprint"),
            ));
        }
        out.push(Domain {
            name: str_of(p, "name"),
            alt_names: p["alt_names"]
                .as_array()
                .map(|a| {
                    a.iter()
                        .filter_map(|x| x.as_str().map(str::to_owned))
                        .collect()
                })
                .unwrap_or_default(),
            commune: str_of(&p["commune"], "name"),
            insee: str_of(&p["commune"], "insee"),
            departement: str_of(&p["commune"], "departement"),
            point: LonLat::new(
                p["lon"].as_f64().unwrap_or_default(),
                p["lat"].as_f64().unwrap_or_default(),
            ),
            km_nordic: p["km_nordic"].as_f64().unwrap_or_default(),
            openskimap: p["openskimap"]
                .as_array()
                .map(|a| {
                    a.iter()
                        .filter_map(|x| x["id"].as_str().map(str::to_owned))
                        .collect()
                })
                .unwrap_or_default(),
            site_relation: p["site_relation"].as_i64(),
            polygons,
            bbox: BBox::new(s, w, n, e),
            id,
        });
    }
    Ok(out)
}

/// OSM ways of each domain (`domaines_ways.json`).
pub type DomainWays = BTreeMap<String, Vec<i64>>;

/// Reads the census layer (`domaines.geojson`) and the ways of each domain
/// (`domaines_ways.json`).
pub fn load(dir: &Path) -> Result<(Vec<Domain>, DomainWays)> {
    let read = |name: &str| -> Result<Value> {
        let path = dir.join(name);
        let b = std::fs::read(&path).map_err(|e| {
            StationsError::Input(format!(
                "{} ({e}) : lancer d'abord le recensement des domaines (P3)",
                path.display()
            ))
        })?;
        serde_json::from_slice(&b).map_err(|e| StationsError::format(name, e.to_string()))
    };
    let domains = parse(&read("domaines.geojson")?)?;
    let ways: BTreeMap<String, Vec<i64>> = serde_json::from_value(read("domaines_ways.json")?)
        .map_err(|e| StationsError::format("domaines_ways.json", e.to_string()))?;
    Ok((domains, ways))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn square() -> Domain {
        let v = json!({"features": [{"geometry": {"type": "MultiPolygon", "coordinates": [[[
            [6.0, 46.0], [6.01, 46.0], [6.01, 46.01], [6.0, 46.01], [6.0, 46.0]
        ]]]}, "properties": {"id": "site-1", "name": "Test", "lon": 6.005, "lat": 46.005,
            "commune": {"name": "C", "insee": "25000", "departement": "25"}}}]});
        parse(&v).unwrap().remove(0)
    }

    #[test]
    fn distances() {
        let d = square();
        assert_eq!(d.distance_m(LonLat::new(6.005, 46.005)), 0.0);
        // 0.01° of latitude north of the top edge: about 1112 m.
        let n = d.distance_m(LonLat::new(6.005, 46.02));
        assert!((n - 1112.0).abs() < 5.0, "{n}");
        // East of the right edge by 0.001° of longitude at 46°: about 77 m.
        let e = d.distance_m(LonLat::new(6.011, 46.005));
        assert!((e - 77.2).abs() < 1.0, "{e}");
        assert!(d.near_bbox(LonLat::new(6.011, 46.005), 100.0));
        assert!(!d.near_bbox(LonLat::new(6.1, 46.005), 100.0));
        assert_eq!(d.departement, "25");
    }
}
