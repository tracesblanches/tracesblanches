//! Exposure along the piste network: each line is cut into runs of constant exposure
//! class.
//!
//! A point of a line takes the highest class of the cells whose centre lies within
//! [`BUFFER_M`] (the OSM tracks and the 5 m model are both uncertain by several metres:
//! the conservative side is the highest class nearby). Lines are walked in Lambert-93 in
//! steps of at most [`STEP_M`]; lengths are Euclidean in Lambert-93, as in `historic`.

use skimap_common::crs;
use skimap_common::geom::LonLat;

use super::class;
use super::raster::Raster;
use crate::error::Result;
use crate::historic::pistes::{Network, PisteLine};

/// Search radius around a track point, m.
pub const BUFFER_M: f64 = 10.0;
/// Sampling step along the lines, m.
pub const STEP_M: f64 = 1.0;

/// A run of constant exposure along one line.
#[derive(Debug, Clone, PartialEq)]
pub struct Run {
    /// Exposure class code ([`class`]).
    pub class: u8,
    /// Vertices, Lambert-93.
    pub coords: Vec<(f64, f64)>,
    /// Length, m.
    pub length_m: f64,
}

/// Highest class of the cells whose centre is within `radius` of `(x, y)`; `NODATA` when
/// none has a class.
pub fn class_near(r: &Raster<u8>, x: f64, y: f64, radius: f64) -> u8 {
    let c0 = ((x - radius - r.origin_x) / r.cell).floor().max(0.0) as usize;
    let c1 = (((x + radius - r.origin_x) / r.cell).floor() as i64).min(r.w as i64 - 1);
    let r0 = ((r.origin_y - (y + radius)) / r.cell).floor().max(0.0) as usize;
    let r1 = (((r.origin_y - (y - radius)) / r.cell).floor() as i64).min(r.h as i64 - 1);
    let mut best = class::NODATA;
    if c1 < 0 || r1 < 0 {
        return best;
    }
    for row in r0..=r1 as usize {
        for col in c0..=c1 as usize {
            let (cx, cy) = r.center(col, row);
            if (cx - x).powi(2) + (cy - y).powi(2) > radius * radius {
                continue;
            }
            let v = r.data[row * r.w + col];
            if v != class::NODATA && (best == class::NODATA || v > best) {
                best = v;
            }
        }
    }
    best
}

/// Runs of constant class along a Lambert-93 polyline.
pub fn runs(line: &[(f64, f64)], classes: &Raster<u8>) -> Vec<Run> {
    let mut out: Vec<Run> = Vec::new();
    for w in line.windows(2) {
        let (a, b) = (w[0], w[1]);
        let len = ((b.0 - a.0).powi(2) + (b.1 - a.1).powi(2)).sqrt();
        if len == 0.0 {
            continue;
        }
        let n = (len / STEP_M).ceil().max(1.0) as usize;
        for k in 0..n {
            let t0 = k as f64 / n as f64;
            let t1 = (k + 1) as f64 / n as f64;
            let p0 = (a.0 + (b.0 - a.0) * t0, a.1 + (b.1 - a.1) * t0);
            let p1 = (a.0 + (b.0 - a.0) * t1, a.1 + (b.1 - a.1) * t1);
            let mid = ((p0.0 + p1.0) / 2.0, (p0.1 + p1.1) / 2.0);
            let c = class_near(classes, mid.0, mid.1, BUFFER_M);
            let piece = len / n as f64;
            match out.last_mut() {
                Some(r) if r.class == c => {
                    r.coords.push(p1);
                    r.length_m += piece;
                }
                _ => out.push(Run {
                    class: c,
                    coords: vec![p0, p1],
                    length_m: piece,
                }),
            }
        }
    }
    out
}

/// Exposure of one piste line.
#[derive(Debug, Clone)]
pub struct LineExposure<'a> {
    /// The line.
    pub line: &'a PisteLine,
    /// Its runs.
    pub runs: Vec<Run>,
}

impl LineExposure<'_> {
    /// Length, m.
    pub fn length_m(&self) -> f64 {
        self.runs.iter().map(|r| r.length_m).sum()
    }
    /// Highest class along the line.
    pub fn max_class(&self) -> u8 {
        self.runs
            .iter()
            .map(|r| r.class)
            .filter(|c| *c != class::NODATA)
            .max()
            .unwrap_or(class::NODATA)
    }
}

/// Exposure of every line.
pub fn lines<'a>(lines: &'a [PisteLine], classes: &Raster<u8>) -> Result<Vec<LineExposure<'a>>> {
    let mut out = Vec::new();
    for l in lines {
        let mut pts = Vec::with_capacity(l.coords.len());
        for p in &l.coords {
            pts.push(crs::to_lambert93(*p)?);
        }
        out.push(LineExposure {
            line: l,
            runs: runs(&pts, classes),
        });
    }
    Ok(out)
}

/// Length (m) per class code, index = code (1–3), index 0 = no data.
pub fn length_by_class<'a>(runs: impl IntoIterator<Item = &'a Run>) -> [f64; 4] {
    let mut out = [0.0; 4];
    for r in runs {
        let i = if (1..=3).contains(&r.class) {
            r.class as usize
        } else {
            0
        };
        out[i] += r.length_m;
    }
    out
}

/// GeoJSON features (WGS84) of the runs of every line.
pub fn features(ex: &[LineExposure<'_>]) -> Result<Vec<serde_json::Value>> {
    let mut out = Vec::new();
    for le in ex {
        let way_len = le.length_m();
        let way_max = le.max_class();
        for (k, r) in le.runs.iter().enumerate() {
            let mut pts: Vec<LonLat> = Vec::with_capacity(r.coords.len());
            for (x, y) in &r.coords {
                pts.push(crs::from_lambert93(*x, *y)?);
            }
            out.push(skimap_common::output::feature(
                skimap_common::output::line_string(pts),
                serde_json::json!({
                    "osm_way_id": le.line.osm_way_id,
                    "run": k,
                    "name": le.line.name,
                    "relations": le.line.relations,
                    "network": match le.line.network { Network::Nordic => "nordic", Network::Hike => "hike" },
                    "exposure": if r.class == class::NODATA { serde_json::Value::Null } else { r.class.into() },
                    "label": class::label(r.class),
                    "length_m": (r.length_m * 10.0).round() / 10.0,
                    "way_length_m": (way_len * 10.0).round() / 10.0,
                    "way_max_exposure": if way_max == class::NODATA { serde_json::Value::Null } else { way_max.into() },
                }),
            ));
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn grid() -> Raster<u8> {
        // 10 × 4 cells of 5 m; columns 0–4 class 1, 5–7 class 3, 8–9 class 2.
        let mut data = Vec::new();
        for _ in 0..4 {
            for c in 0..10 {
                data.push(match c {
                    0..=4 => class::LOW,
                    5..=7 => class::HIGH,
                    _ => class::EXPOSED,
                });
            }
        }
        Raster {
            w: 10,
            h: 4,
            cell: 5.0,
            origin_x: 0.0,
            origin_y: 20.0,
            data,
        }
    }

    #[test]
    fn buffer_takes_the_highest_class_nearby() {
        let g = grid();
        // Centre of column 0: 2.5 m; column 5 starts at 25 m.
        assert_eq!(class_near(&g, 2.5, 10.0, BUFFER_M), class::LOW);
        // 12.5 m: column 4 centre is 22.5, column 5 centre 27.5 (15 m away): still low.
        assert_eq!(class_near(&g, 12.5, 10.0, BUFFER_M), class::LOW);
        // 18 m: column 5 centre is 9.5 m away: high.
        assert_eq!(class_near(&g, 18.0, 10.0, BUFFER_M), class::HIGH);
        assert_eq!(class_near(&g, -50.0, 10.0, BUFFER_M), class::NODATA);
    }

    #[test]
    fn runs_split_where_the_class_changes() {
        let g = grid();
        let r = runs(&[(0.0, 10.0), (50.0, 10.0)], &g);
        let classes: Vec<u8> = r.iter().map(|x| x.class).collect();
        assert_eq!(classes, vec![class::LOW, class::HIGH, class::EXPOSED]);
        // Low up to 17.5 m (column 5 centre 27.5 is then 10 m away), high up to 47.5 m
        // (column 7 centre 37.5), then exposed.
        assert!((r[0].length_m - 17.5).abs() <= 1.0, "{}", r[0].length_m);
        assert!((r[1].length_m - 30.0).abs() <= 1.0, "{}", r[1].length_m);
        let total: f64 = r.iter().map(|x| x.length_m).sum();
        assert!((total - 50.0).abs() < 1e-9);
        let by = length_by_class(&r);
        assert!((by[1] + by[2] + by[3] - 50.0).abs() < 1e-9);
        assert_eq!(by[0], 0.0);
    }
}
