//! Measurement for the « raster or vector » question of K15: turns a class raster of the
//! pipeline (slope in degrees, or avalanche exposure classes) into nested polygons
//! (`class >= k` for each threshold), after a majority filter and the removal of
//! patches and holes smaller than a minimum area, simplified, in WGS84 GeoJSON.
//! Prints the time, the counts and the sizes (plain and gzip) to compare with the
//! PMTiles rasters of the pack. Writes nothing in the repository.
//!
//! ```sh
//! cargo run --release -p skimap-styles --example vectorize_classes -- \
//!     data/out/<site>/terrain/slope_5m.tif 30,35,40,45 /tmp/slope.geojson
//! cargo run --release -p skimap-styles --example vectorize_classes -- \
//!     data/out/<site>/avalanche-exposure/exposure_5m.tif 2,3 /tmp/exposure.geojson
//! ```

#![allow(
    clippy::disallowed_methods,
    reason = "developer tool, not a pipeline output"
)]
use std::collections::VecDeque;
use std::io::Write;
use std::time::Instant;

use geo::{Area, Contains, Coord, LineString, Point, Polygon, Simplify};
use skimap_common::crs::{Crs, transform_points};
use skimap_common::geotiff::{read_f32, read_u8};

/// Majority filter radius (cells): 1 is 3 × 3.
const MODE_RADIUS: usize = 1;
/// Smallest patch or hole kept, in square metres.
const MIN_AREA_M2: f64 = 400.0;
/// Douglas-Peucker tolerance, metres.
const SIMPLIFY_M: f64 = 3.0;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().collect();
    let [_, input, thresholds, output] = args.as_slice() else {
        return Err("usage: vectorize_classes <raster.tif> <t1,t2,...> <out.geojson>".into());
    };
    let thresholds: Vec<f32> = thresholds
        .split(',')
        .map(str::parse)
        .collect::<Result<_, _>>()?;
    let t0 = Instant::now();
    // Values as f32 (UInt8 classes read as their numbers, 255 = no data).
    let (w, h, gt, crs, values): (usize, usize, _, Crs, Vec<f32>) = match read_f32(input.as_ref()) {
        Ok(g) => {
            let nd = g.nodata().copied();
            let v = g
                .data()
                .iter()
                .map(|&x| if Some(x) == nd || x.is_nan() { -1.0 } else { x })
                .collect();
            (g.width(), g.height(), *g.transform(), g.crs(), v)
        }
        Err(_) => {
            let g = read_u8(input.as_ref())?;
            let nd = g.nodata().copied();
            let v = g
                .data()
                .iter()
                .map(|&x| if Some(x) == nd { -1.0 } else { f32::from(x) })
                .collect();
            (g.width(), g.height(), *g.transform(), g.crs(), v)
        }
    };
    let cell = gt.to_gdal()[1].abs();
    let t_read = t0.elapsed();
    let class: Vec<u8> = values
        .iter()
        .map(|&v| thresholds.iter().filter(|&&t| v >= t).count() as u8)
        .collect();
    let class = mode_filter(&class, w, h, MODE_RADIUS);
    let min_cells = (MIN_AREA_M2 / (cell * cell)).ceil() as usize;
    let mut features = Vec::new();
    let (mut n_poly, mut n_vertices) = (0usize, 0usize);
    for (k, t) in thresholds.iter().enumerate() {
        let level = (k + 1) as u8;
        let mut mask: Vec<bool> = class.iter().map(|&c| c >= level).collect();
        drop_small(&mut mask, w, h, min_cells, true);
        drop_small(&mut mask, w, h, min_cells, false);
        let rings = trace(&mask, w, h);
        // Rings in world coordinates, simplified; outer rings counter-clockwise.
        let mut outers: Vec<(Polygon<f64>, Vec<LineString<f64>>)> = Vec::new();
        let mut holes = Vec::new();
        for r in rings {
            let pts: Vec<Coord<f64>> = r
                .iter()
                .map(|&(c, rw)| {
                    let (x, y) = gt.pixel_to_world(c as f64, rw as f64);
                    Coord { x, y }
                })
                .collect();
            let ls = LineString::new(pts).simplify(SIMPLIFY_M);
            if ls.0.len() < 4 {
                continue;
            }
            let poly = Polygon::new(ls.clone(), vec![]);
            if poly.signed_area() > 0.0 {
                outers.push((poly, Vec::new()));
            } else {
                holes.push(ls);
            }
        }
        for hole in holes {
            let p = Point::from(hole.0[0]);
            if let Some(o) = outers
                .iter_mut()
                .filter(|(o, _)| o.contains(&p))
                .min_by(|a, b| a.0.unsigned_area().total_cmp(&b.0.unsigned_area()))
            {
                o.1.push(hole);
            }
        }
        let mut polys = Vec::new();
        for (outer, hs) in outers {
            let mut rings = vec![outer.exterior().clone()];
            rings.extend(hs);
            let mut json_rings = Vec::new();
            for ring in rings {
                let mut pts: Vec<(f64, f64)> = ring.0.iter().map(|c| (c.x, c.y)).collect();
                transform_points(crs, Crs::Wgs84, &mut pts)?;
                n_vertices += pts.len();
                json_rings.push(
                    pts.iter()
                        .map(|(x, y)| serde_json::json!([round6(*x), round6(*y)]))
                        .collect::<Vec<_>>(),
                );
            }
            polys.push(json_rings);
            n_poly += 1;
        }
        features.push(serde_json::json!({
            "type": "Feature",
            "properties": {"class": level, "min": t},
            "geometry": {"type": "MultiPolygon", "coordinates": polys}
        }));
    }
    let fc = serde_json::json!({"type": "FeatureCollection", "features": features});
    let text = serde_json::to_string(&fc)?;
    std::fs::write(output, &text)?;
    let mut gz = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::new(6));
    gz.write_all(text.as_bytes())?;
    let gz = gz.finish()?;
    println!(
        "{input}: {w} x {h} cells of {cell} m, read {:.2} s, total {:.2} s; {} thresholds, {n_poly} polygons, \
         {n_vertices} vertices; GeoJSON {:.2} MB, gzip {:.2} MB",
        t_read.as_secs_f64(),
        t0.elapsed().as_secs_f64(),
        thresholds.len(),
        text.len() as f64 / 1e6,
        gz.len() as f64 / 1e6
    );
    Ok(())
}

fn round6(x: f64) -> f64 {
    (x * 1e6).round() / 1e6
}

/// Majority of the (2r+1)² window; ties keep the centre class.
fn mode_filter(c: &[u8], w: usize, h: usize, r: usize) -> Vec<u8> {
    let mut out = c.to_vec();
    for y in 0..h {
        for x in 0..w {
            let mut counts = [0u16; 8];
            for yy in y.saturating_sub(r)..(y + r + 1).min(h) {
                for xx in x.saturating_sub(r)..(x + r + 1).min(w) {
                    counts[c[yy * w + xx].min(7) as usize] += 1;
                }
            }
            let centre = c[y * w + x].min(7) as usize;
            let (best, n) = counts
                .iter()
                .enumerate()
                .max_by_key(|&(_, n)| *n)
                .unwrap_or((centre, &0));
            if *n > counts[centre] {
                out[y * w + x] = best as u8;
            }
        }
    }
    out
}

/// Flips 4-connected components of `value` smaller than `min` cells.
fn drop_small(mask: &mut [bool], w: usize, h: usize, min: usize, value: bool) {
    let mut seen = vec![false; mask.len()];
    let mut comp = Vec::new();
    let mut queue = VecDeque::new();
    for start in 0..mask.len() {
        if seen[start] || mask[start] != value {
            continue;
        }
        comp.clear();
        seen[start] = true;
        queue.push_back(start);
        while let Some(i) = queue.pop_front() {
            comp.push(i);
            let (x, y) = (i % w, i / w);
            let mut push = |j: usize| {
                if !seen[j] && mask[j] == value {
                    seen[j] = true;
                    queue.push_back(j);
                }
            };
            if x > 0 {
                push(i - 1);
            }
            if x + 1 < w {
                push(i + 1);
            }
            if y > 0 {
                push(i - w);
            }
            if y + 1 < h {
                push(i + w);
            }
        }
        if comp.len() < min {
            for &i in &comp {
                mask[i] = !value;
            }
        }
    }
}

/// Boundary rings of the mask along cell edges, as corner coordinates `(col, row)`.
/// Edges are directed with the mask on their left: outer rings come out
/// counter-clockwise in world coordinates (y up), holes clockwise. At a corner touched
/// by two diagonal cells the ring turns towards its own cell (left), so that diagonal
/// cells stay apart, as in the 4-connected components of [`drop_small`].
fn trace(mask: &[bool], w: usize, h: usize) -> Vec<Vec<(usize, usize)>> {
    let at = |x: isize, y: isize| {
        x >= 0
            && y >= 0
            && (x as usize) < w
            && (y as usize) < h
            && mask[y as usize * w + x as usize]
    };
    // Directed edges keyed by their start corner: (to corner, direction).
    let cols = w + 1;
    let mut next: Vec<Vec<(usize, u8)>> = vec![Vec::new(); cols * (h + 1)];
    let id = |x: usize, y: usize| y * cols + x;
    for y in 0..h as isize {
        for x in 0..w as isize {
            if !at(x, y) {
                continue;
            }
            let (ux, uy) = (x as usize, y as usize);
            // Direction codes: 0 east, 1 south, 2 west, 3 north (raster coordinates).
            if !at(x, y - 1) {
                next[id(ux + 1, uy)].push((id(ux, uy), 2));
            }
            if !at(x + 1, y) {
                next[id(ux + 1, uy + 1)].push((id(ux + 1, uy), 3));
            }
            if !at(x, y + 1) {
                next[id(ux, uy + 1)].push((id(ux + 1, uy + 1), 0));
            }
            if !at(x - 1, y) {
                next[id(ux, uy)].push((id(ux, uy + 1), 1));
            }
        }
    }
    let mut rings = Vec::new();
    for start in 0..next.len() {
        while let Some((mut to, mut dir)) = next[start].pop() {
            let mut ring = vec![(start % cols, start / cols)];
            while to != start {
                ring.push((to % cols, to / cols));
                let outs = &mut next[to];
                // Prefer the left turn (raster coordinates), then straight, then right.
                let pick = [(dir + 3) % 4, dir, (dir + 1) % 4]
                    .iter()
                    .find_map(|d| outs.iter().position(|&(_, od)| od == *d))
                    .unwrap_or(0);
                if outs.is_empty() {
                    break;
                }
                let (t, d) = outs.swap_remove(pick);
                to = t;
                dir = d;
            }
            ring.push(ring[0]);
            rings.push(ring);
        }
    }
    rings
}
