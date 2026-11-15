//! Challenges computed on the phone from the outings (V13 #76): the whole GTJ, every piste
//! of a domain, kilometres skied. Nothing leaves the phone.
//!
//! Coverage of a reference (a route or the pistes of a domain): the reference lines are cut
//! into pieces of at most [`PIECE_M`] metres; a piece is skied when the middle of the piece
//! is within [`TOLERANCE_M`] metres of a segment of an outing's track (GPS error under
//! trees, both lanes of a wide piste). The progress is the length of the pieces skied over
//! the total length; the challenge is done at [`DONE_SHARE`] (variants of the route, short
//! GPS gaps), on the day of the outing that crossed it.

use std::collections::HashMap;

use chrono::{DateTime, Utc};
use serde::Serialize;

use crate::track::Outing;

/// Longest piece of a reference, m.
pub const PIECE_M: f64 = 50.0;
/// Distance from a track within which a piece counts as skied, m.
pub const TOLERANCE_M: f64 = 40.0;
/// Share of the length at which a challenge of coverage is done.
pub const DONE_SHARE: f64 = 0.95;
/// Cell of the spatial index, m.
const CELL_M: f64 = 100.0;

/// Progress of a challenge.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Progress {
    /// Metres done (skied, or kilometres of the distance challenge in metres).
    pub done_m: f64,
    /// Metres to do.
    pub total_m: f64,
    /// `done / total`, at most 1.
    pub share: f64,
    /// Outings that count.
    pub outings: u32,
    /// The day it was done, if done.
    pub done_on: Option<DateTime<Utc>>,
}

/// Local plane (equirectangular around a latitude): metres.
#[derive(Clone, Copy)]
struct Plane {
    kx: f64,
    ky: f64,
}

impl Plane {
    fn new(lat: f64) -> Self {
        let ky = 111_195.0;
        Self {
            kx: ky * lat.to_radians().cos(),
            ky,
        }
    }
    fn xy(&self, p: [f64; 2]) -> (f64, f64) {
        (p[0] * self.kx, p[1] * self.ky)
    }
}

fn seg_dist(p: (f64, f64), a: (f64, f64), b: (f64, f64)) -> f64 {
    let (dx, dy) = (b.0 - a.0, b.1 - a.1);
    let l2 = dx * dx + dy * dy;
    let t = if l2 > 0.0 {
        (((p.0 - a.0) * dx + (p.1 - a.1) * dy) / l2).clamp(0.0, 1.0)
    } else {
        0.0
    };
    ((a.0 + t * dx - p.0).powi(2) + (a.1 + t * dy - p.1).powi(2)).sqrt()
}

fn cell(x: f64, y: f64) -> (i64, i64) {
    ((x / CELL_M).floor() as i64, (y / CELL_M).floor() as i64)
}

/// A segment of an outing in the index: outing, ends in the plane.
type IndexedSegment = (usize, (f64, f64), (f64, f64));

/// Coverage of `lines` (`[lon, lat]`) by the outings.
pub fn coverage(lines: &[Vec<[f64; 2]>], outings: &[Outing]) -> Progress {
    let n_pts: usize = lines.iter().map(Vec::len).sum();
    if n_pts == 0 {
        return Progress {
            done_m: 0.0,
            total_m: 0.0,
            share: 0.0,
            outings: 0,
            done_on: None,
        };
    }
    let lat0 = lines.iter().flatten().map(|p| p[1]).sum::<f64>() / n_pts as f64;
    let plane = Plane::new(lat0);
    // Pieces: middle point and length.
    let mut pieces: Vec<((f64, f64), f64)> = Vec::new();
    for l in lines {
        for w in l.windows(2) {
            let (a, b) = (plane.xy(w[0]), plane.xy(w[1]));
            let len = ((b.0 - a.0).powi(2) + (b.1 - a.1).powi(2)).sqrt();
            let n = (len / PIECE_M).ceil().max(1.0);
            for k in 0..n as usize {
                let t = (k as f64 + 0.5) / n;
                pieces.push(((a.0 + t * (b.0 - a.0), a.1 + t * (b.1 - a.1)), len / n));
            }
        }
    }
    let total_m: f64 = pieces.iter().map(|p| p.1).sum();
    // Index of the outings' segments by cell (segment = outing index, a, b).
    let mut grid: HashMap<(i64, i64), Vec<IndexedSegment>> = HashMap::new();
    for (oi, o) in outings.iter().enumerate() {
        for w in o.track.windows(2) {
            let (a, b) = (plane.xy(w[0]), plane.xy(w[1]));
            let (c0, c1) = (
                cell(a.0.min(b.0), a.1.min(b.1)),
                cell(a.0.max(b.0), a.1.max(b.1)),
            );
            // A segment over a GPS gap of kilometres would cover everything between: skip.
            if (c1.0 - c0.0) > 10 || (c1.1 - c0.1) > 10 {
                continue;
            }
            for cx in c0.0..=c1.0 {
                for cy in c0.1..=c1.1 {
                    grid.entry((cx, cy)).or_default().push((oi, a, b));
                }
            }
        }
    }
    // For each piece, the earliest outing covering it.
    let start = |oi: usize| outings[oi].start.unwrap_or(DateTime::<Utc>::MIN_UTC);
    let mut covered: Vec<(DateTime<Utc>, f64, usize)> = Vec::new();
    for &(m, len) in &pieces {
        let c = cell(m.0, m.1);
        let mut best: Option<usize> = None;
        for dx in -1..=1 {
            for dy in -1..=1 {
                if let Some(v) = grid.get(&(c.0 + dx, c.1 + dy)) {
                    for &(oi, a, b) in v {
                        if best.is_some_and(|bi| start(bi) <= start(oi)) {
                            continue;
                        }
                        if seg_dist(m, a, b) <= TOLERANCE_M {
                            best = Some(oi);
                        }
                    }
                }
            }
        }
        if let Some(oi) = best {
            covered.push((start(oi), len, oi));
        }
    }
    covered.sort_by_key(|c| c.0);
    let mut done_m = 0.0;
    let mut done_on = None;
    let mut used = vec![false; outings.len()];
    for &(t, len, oi) in &covered {
        done_m += len;
        used[oi] = true;
        if done_on.is_none() && done_m >= DONE_SHARE * total_m {
            done_on = outings[oi].start.or(Some(t));
        }
    }
    Progress {
        done_m,
        total_m,
        share: (done_m / total_m).min(1.0),
        outings: used.iter().filter(|u| **u).count() as u32,
        done_on,
    }
}

/// Kilometres skied since `since` (all outings if `None`), towards `target_km`.
pub fn distance(outings: &[Outing], since: Option<DateTime<Utc>>, target_km: f64) -> Progress {
    let mut sorted: Vec<&Outing> = outings
        .iter()
        .filter(|o| since.is_none_or(|s| o.start.is_some_and(|t| t >= s)))
        .collect();
    sorted.sort_by_key(|o| o.start);
    let total_m = target_km * 1000.0;
    let mut done_m = 0.0;
    let mut done_on = None;
    for o in &sorted {
        done_m += o.distance_m;
        if done_on.is_none() && done_m >= total_m {
            done_on = o.start;
        }
    }
    Progress {
        done_m,
        total_m,
        share: if total_m > 0.0 {
            (done_m / total_m).min(1.0)
        } else {
            0.0
        },
        outings: sorted.len() as u32,
        done_on,
    }
}

/// Lines (`[lon, lat]`) of the `LineString` and `MultiLineString` features of a GeoJSON
/// feature collection, keeping those whose properties pass `keep`.
pub fn geojson_lines(
    json: &str,
    keep: impl Fn(&serde_json::Value) -> bool,
) -> Result<Vec<Vec<[f64; 2]>>, serde_json::Error> {
    let v: serde_json::Value = serde_json::from_str(json)?;
    let mut out = Vec::new();
    let line = |c: &serde_json::Value| -> Vec<[f64; 2]> {
        c.as_array()
            .map(|pts| {
                pts.iter()
                    .filter_map(|p| Some([p.get(0)?.as_f64()?, p.get(1)?.as_f64()?]))
                    .collect()
            })
            .unwrap_or_default()
    };
    for f in v["features"].as_array().into_iter().flatten() {
        if !keep(&f["properties"]) {
            continue;
        }
        let g = &f["geometry"];
        match g["type"].as_str() {
            Some("LineString") => out.push(line(&g["coordinates"])),
            Some("MultiLineString") => {
                for l in g["coordinates"].as_array().into_iter().flatten() {
                    out.push(line(l));
                }
            }
            _ => {}
        }
    }
    out.retain(|l| l.len() >= 2);
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn outing(id: &str, day: u32, track: Vec<[f64; 2]>) -> Outing {
        Outing {
            id: id.into(),
            source: "skimap".into(),
            name: id.into(),
            activity: "nordic_skiing".into(),
            start: Some(format!("2026-01-{day:02}T09:00:00Z").parse().unwrap()),
            elapsed_s: 0.0,
            moving_s: 0.0,
            distance_m: 10_000.0,
            gain_m: 0.0,
            track,
        }
    }

    /// A straight route of 10 km going east at 46.6° N, sampled every 10 m.
    fn route() -> Vec<[f64; 2]> {
        let kx = 111_195.0 * 46.6f64.to_radians().cos();
        (0..=1000)
            .map(|i| [6.0 + f64::from(i) * 10.0 / kx, 46.6])
            .collect()
    }

    #[test]
    fn route_done_in_three_outings_on_the_last_day() {
        let r = route();
        // Outings 25 m north of the route (GPS error), in three parts, the middle twice.
        let off = 25.0 / 111_195.0;
        let part = |a: usize, b: usize| {
            r[a..=b]
                .iter()
                .map(|p| [p[0], p[1] + off])
                .collect::<Vec<_>>()
        };
        let o1 = outing("a", 3, part(0, 400));
        let o2 = outing("b", 10, part(350, 700));
        let mut far = part(700, 1000);
        // A detour 200 m away does not count.
        for p in far.iter_mut().take(20) {
            p[1] += 200.0 / 111_195.0;
        }
        let o3 = outing("c", 20, far);
        let p = coverage(std::slice::from_ref(&r), &[o2.clone(), o1.clone()]);
        assert!((p.total_m - 10_000.0).abs() < 1.0, "{}", p.total_m);
        assert!((p.share - 0.70).abs() < 0.01, "{}", p.share);
        assert_eq!((p.outings, p.done_on), (2, None));
        let p = coverage(std::slice::from_ref(&r), &[o1, o2, o3.clone()]);
        assert!(
            p.share > 0.97 && p.share < 0.99,
            "detour of 200 m missing: {}",
            p.share
        );
        assert_eq!(p.done_on, o3.start);
        assert_eq!(p.outings, 3);
    }

    #[test]
    fn far_tracks_and_gps_gaps_do_not_count() {
        let r = route();
        let off = 60.0 / 111_195.0;
        let parallel: Vec<[f64; 2]> = r.iter().map(|p| [p[0], p[1] + off]).collect();
        assert_eq!(
            coverage(std::slice::from_ref(&r), &[outing("p", 1, parallel)]).share,
            0.0
        );
        // Two points 10 km apart along the route (a gap): skipped.
        let gap = vec![r[0], r[1000]];
        assert!(coverage(std::slice::from_ref(&r), &[outing("g", 1, gap)]).share < 0.02);
        assert_eq!(coverage(&[], &[]).total_m, 0.0);
    }

    #[test]
    fn kilometres() {
        let os: Vec<Outing> = (1..=12).map(|d| outing("x", d, vec![])).collect();
        let p = distance(&os, None, 100.0);
        assert_eq!((p.done_m, p.share, p.outings), (120_000.0, 1.0, 12));
        assert_eq!(p.done_on, os[9].start);
        let since = Some("2026-01-06T00:00:00Z".parse().unwrap());
        let p = distance(&os, since, 100.0);
        assert_eq!((p.outings, p.done_on), (7, None));
    }

    #[test]
    fn lines_of_a_feature_collection() {
        let j = r#"{"type":"FeatureCollection","features":[
          {"type":"Feature","properties":{"n":1},"geometry":{"type":"LineString","coordinates":[[6,46],[6.1,46.1]]}},
          {"type":"Feature","properties":{"n":2},"geometry":{"type":"MultiLineString","coordinates":[[[6,46],[6.1,46]],[[7,46]]]}},
          {"type":"Feature","properties":{"n":3},"geometry":{"type":"Point","coordinates":[6,46]}}]}"#;
        assert_eq!(geojson_lines(j, |_| true).unwrap().len(), 2);
        assert_eq!(geojson_lines(j, |p| p["n"] == 1).unwrap().len(), 1);
    }
}
