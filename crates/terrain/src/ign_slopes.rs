//! Comparison of the computed slope with the IGN "Carte des pentes"
//! (`GEOGRAPHICALGRIDSYSTEMS.SLOPES.MOUNTAIN`, WMTS, free, no key).
//!
//! The IGN layer is a coloured classification computed from RGE ALTI 5 m: transparent
//! below 30°, then 30–35° yellow, 35–40° orange, 40–45° red, ≥ 45° pink (legend
//! `GEOGRAPHICALGRIDSYSTEMS.SLOPES.MOUNTAIN-legend.png`). Each WMTS pixel (Web Mercator,
//! zoom 15 ≈ 3.4 m on the ground at 45° N) is classified by its nearest legend colour and
//! compared with the class of our slope at the pixel centre (nearest 5 m cell).

use rayon::prelude::*;
use serde::{Deserialize, Serialize};
use skimap_common::crs::{Crs, tile_for, transform_points};
use skimap_common::download::{Downloader, Request};
use skimap_common::geom::{BBox, LonLat};
use skimap_common::grid::Grid;

use crate::dem::SOURCE;
use crate::error::Result;
use crate::png;

/// WMTS layer identifier.
pub const LAYER: &str = "GEOGRAPHICALGRIDSYSTEMS.SLOPES.MOUNTAIN";
/// Class edges of the IGN layer, degrees.
pub const CLASS_EDGES: [f32; 4] = [30.0, 35.0, 40.0, 45.0];
/// Legend colours of classes 1..=4 (measured on served tiles; the legend image is close).
pub const CLASS_COLOURS: [[u8; 3]; 4] =
    [[242, 229, 0], [243, 148, 25], [240, 0, 0], [200, 137, 187]];
/// Number of classes (0 = below 30°).
pub const N_CLASSES: usize = 5;
/// A served colour farther than this (RGB distance) from every legend colour is counted
/// as unclassified.
const MAX_COLOUR_DISTANCE: f64 = 60.0;

/// URL of a WMTS tile of the IGN slope map.
pub fn tile_url(z: u8, x: u32, y: u32) -> String {
    format!(
        "https://data.geopf.fr/wmts?SERVICE=WMTS&REQUEST=GetTile&VERSION=1.0.0&LAYER={LAYER}\
         &STYLE=normal&TILEMATRIXSET=PM_0_17&TILEMATRIX={z}&TILEROW={y}&TILECOL={x}&FORMAT=image/png"
    )
}

/// Class (0..=4) of a slope in degrees.
pub fn slope_class(slope: f32) -> usize {
    CLASS_EDGES.iter().filter(|e| slope >= **e).count()
}

/// Class of a served RGBA pixel, `None` if the colour is not in the legend.
pub fn colour_class(p: [u8; 4]) -> Option<usize> {
    if p[3] < 128 {
        return Some(0);
    }
    let (mut best, mut dist) = (0, f64::INFINITY);
    for (i, c) in CLASS_COLOURS.iter().enumerate() {
        let d = (0..3)
            .map(|k| (f64::from(p[k]) - f64::from(c[k])).powi(2))
            .sum::<f64>()
            .sqrt();
        if d < dist {
            (best, dist) = (i + 1, d);
        }
    }
    (dist <= MAX_COLOUR_DISTANCE).then_some(best)
}

/// Binary agreement on "slope ≥ 30°".
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Steep30 {
    /// Share of compared pixels ≥ 30° according to IGN.
    pub ign_fraction: f64,
    /// Share of compared pixels ≥ 30° according to our slope.
    pub ours_fraction: f64,
    /// Our ≥ 30° pixels that IGN also marks ≥ 30°.
    pub precision: f64,
    /// IGN ≥ 30° pixels that we also mark ≥ 30°.
    pub recall: f64,
    /// Harmonic mean of precision and recall.
    pub f1: f64,
    /// Intersection over union of the two ≥ 30° masks.
    pub iou: f64,
}

/// Result of the comparison over one area.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct SlopeComparison {
    /// WMTS zoom level used.
    pub zoom: u8,
    /// WMTS tiles read.
    pub tiles: usize,
    /// Pixels compared (inside the area, our slope defined, colour recognised).
    pub pixels: u64,
    /// Pixels skipped because our slope is undefined (outside LiDAR HD, borders).
    pub skipped_no_slope: u64,
    /// Pixels skipped because the colour is not a legend colour (anti-aliasing).
    pub unclassified_colour: u64,
    /// Class edges, degrees.
    pub class_edges_deg: [f32; 4],
    /// Confusion matrix `[ign class][our class]`, pixel counts.
    pub confusion: [[u64; N_CLASSES]; N_CLASSES],
    /// Share of pixels in the same class.
    pub exact_agreement: f64,
    /// Share of pixels at most one class apart.
    pub within_one_class: f64,
    /// Agreement on the ≥ 30° mask.
    pub steep30: Steep30,
    /// Our mean slope (degrees) over the pixels of each IGN class.
    pub our_mean_slope_by_ign_class: [f64; N_CLASSES],
}

#[derive(Default)]
struct Acc {
    tiles: usize,
    pixels: u64,
    skipped: u64,
    unclassified: u64,
    confusion: [[u64; N_CLASSES]; N_CLASSES],
    slope_sum: [f64; N_CLASSES],
}

impl Acc {
    fn merge(mut self, o: Acc) -> Acc {
        self.tiles += o.tiles;
        self.pixels += o.pixels;
        self.skipped += o.skipped;
        self.unclassified += o.unclassified;
        for i in 0..N_CLASSES {
            self.slope_sum[i] += o.slope_sum[i];
            for j in 0..N_CLASSES {
                self.confusion[i][j] += o.confusion[i][j];
            }
        }
        self
    }
}

/// Attempts on a 404: the WMTS answered 404 once for tile 15/16932/11583 (Chapelle-des-Bois)
/// and 200 to the five following identical requests, so a 404 is retried before failing.
const NOT_FOUND_ATTEMPTS: u32 = 4;

/// PNG bytes of one WMTS tile (cache, then network).
pub fn fetch_tile(dl: &Downloader, z: u8, x: u32, y: u32) -> Result<Vec<u8>> {
    let url = tile_url(z, x, y);
    let mut attempt = 0;
    loop {
        match dl.fetch(&Request::get(SOURCE, &url)) {
            Ok(f) => return Ok(f.bytes()?),
            Err(skimap_common::Error::Status { status: 404, .. })
                if attempt + 1 < NOT_FOUND_ATTEMPTS =>
            {
                tracing::warn!(%url, attempt, "WMTS 404, retrying");
                std::thread::sleep(std::time::Duration::from_secs(1 << attempt));
                attempt += 1;
            }
            Err(e) => return Err(e.into()),
        }
    }
}

/// Compares our slope grid (Lambert-93) with the IGN layer over `area` at zoom `z`.
pub fn compare(area: BBox, slope: &Grid<f32>, dl: &Downloader, z: u8) -> Result<SlopeComparison> {
    let (x0, y0) = tile_for(LonLat::new(area.west, area.north), z);
    let (x1, y1) = tile_for(LonLat::new(area.east, area.south), z);
    let tiles: Vec<(u32, u32)> = (y0..=y1)
        .flat_map(|y| (x0..=x1).map(move |x| (x, y)))
        .collect();
    let n = f64::from(1u32 << z) * 256.0;
    let acc = tiles
        .par_iter()
        .map(|&(tx, ty)| -> Result<Acc> {
            let img = png::decode(&fetch_tile(dl, z, tx, ty)?)?;
            let mut lonlat = Vec::with_capacity(img.width * img.height);
            for py in 0..img.height {
                for px in 0..img.width {
                    let gx = (f64::from(tx) * 256.0 + px as f64 + 0.5) / n;
                    let gy = (f64::from(ty) * 256.0 + py as f64 + 0.5) / n;
                    let lon = gx * 360.0 - 180.0;
                    let lat = (std::f64::consts::PI * (1.0 - 2.0 * gy))
                        .sinh()
                        .atan()
                        .to_degrees();
                    lonlat.push((lon, lat));
                }
            }
            let mut xy = lonlat.clone();
            transform_points(Crs::Wgs84, Crs::Lambert93, &mut xy)?;
            let mut a = Acc {
                tiles: 1,
                ..Acc::default()
            };
            for (i, (&(lon, lat), &(x, y))) in lonlat.iter().zip(&xy).enumerate() {
                if !area.contains(LonLat::new(lon, lat)) {
                    continue;
                }
                let s = match slope.value_at(x, y) {
                    Some(s) if s.is_finite() => *s,
                    _ => {
                        a.skipped += 1;
                        continue;
                    }
                };
                let p = img.pixel(i % img.width, i / img.width);
                let Some(ign) = colour_class(p) else {
                    a.unclassified += 1;
                    continue;
                };
                a.pixels += 1;
                a.confusion[ign][slope_class(s)] += 1;
                a.slope_sum[ign] += f64::from(s);
            }
            Ok(a)
        })
        .try_reduce(Acc::default, |a, b| Ok(a.merge(b)))?;
    Ok(summarise(z, acc))
}

fn summarise(zoom: u8, a: Acc) -> SlopeComparison {
    let total = a.pixels as f64;
    let ratio = |k: u64, n: u64| if n > 0 { k as f64 / n as f64 } else { f64::NAN };
    let (mut exact, mut near) = (0u64, 0u64);
    let (mut tp, mut fp, mut fneg) = (0u64, 0u64, 0u64);
    let mut ign_rows = [0u64; N_CLASSES];
    for (i, (row, row_total)) in a.confusion.iter().zip(ign_rows.iter_mut()).enumerate() {
        for (j, &v) in row.iter().enumerate() {
            *row_total += v;
            if i == j {
                exact += v;
            }
            if i.abs_diff(j) <= 1 {
                near += v;
            }
            match (i > 0, j > 0) {
                (true, true) => tp += v,
                (false, true) => fp += v,
                (true, false) => fneg += v,
                _ => {}
            }
        }
    }
    let precision = ratio(tp, tp + fp);
    let recall = ratio(tp, tp + fneg);
    let mut mean = [f64::NAN; N_CLASSES];
    for i in 0..N_CLASSES {
        if ign_rows[i] > 0 {
            mean[i] = a.slope_sum[i] / ign_rows[i] as f64;
        }
    }
    SlopeComparison {
        zoom,
        tiles: a.tiles,
        pixels: a.pixels,
        skipped_no_slope: a.skipped,
        unclassified_colour: a.unclassified,
        class_edges_deg: CLASS_EDGES,
        confusion: a.confusion,
        exact_agreement: exact as f64 / total,
        within_one_class: near as f64 / total,
        steep30: Steep30 {
            ign_fraction: (tp + fneg) as f64 / total,
            ours_fraction: (tp + fp) as f64 / total,
            precision,
            recall,
            f1: 2.0 * precision * recall / (precision + recall),
            iou: ratio(tp, tp + fp + fneg),
        },
        our_mean_slope_by_ign_class: mean,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classes() {
        assert_eq!(slope_class(0.0), 0);
        assert_eq!(slope_class(29.99), 0);
        assert_eq!(slope_class(30.0), 1);
        assert_eq!(slope_class(39.9), 2);
        assert_eq!(slope_class(44.0), 3);
        assert_eq!(slope_class(70.0), 4);
        assert_eq!(colour_class([255, 255, 255, 0]), Some(0));
        assert_eq!(colour_class([242, 228, 0, 255]), Some(1));
        assert_eq!(colour_class([243, 147, 25, 255]), Some(2));
        assert_eq!(colour_class([232, 0, 0, 255]), Some(3));
        assert_eq!(colour_class([200, 137, 187, 255]), Some(4));
        assert_eq!(colour_class([0, 0, 255, 255]), None);
    }

    #[test]
    fn summary_metrics() {
        let mut a = Acc::default();
        // IGN class 0: 6 pixels, we say 0 (5) and 1 (1). IGN class 2: 4 pixels, we say 2 (2), 0 (1), 4 (1).
        a.confusion[0][0] = 5;
        a.confusion[0][1] = 1;
        a.confusion[2][2] = 2;
        a.confusion[2][0] = 1;
        a.confusion[2][4] = 1;
        a.pixels = 10;
        let s = summarise(15, a);
        assert!((s.exact_agreement - 0.7).abs() < 1e-12);
        assert!((s.within_one_class - 0.8).abs() < 1e-12);
        // tp = 3 (2 + 1), fp = 1, fn = 1.
        assert!((s.steep30.precision - 0.75).abs() < 1e-12);
        assert!((s.steep30.recall - 0.75).abs() < 1e-12);
        assert!((s.steep30.iou - 0.6).abs() < 1e-12);
        assert!((s.steep30.ign_fraction - 0.4).abs() < 1e-12);
    }

    #[test]
    fn url() {
        assert_eq!(
            tile_url(15, 17000, 11805),
            "https://data.geopf.fr/wmts?SERVICE=WMTS&REQUEST=GetTile&VERSION=1.0.0&LAYER=GEOGRAPHICALGRIDSYSTEMS.SLOPES.MOUNTAIN&STYLE=normal&TILEMATRIXSET=PM_0_17&TILEMATRIX=15&TILEROW=11805&TILECOL=17000&FORMAT=image/png"
        );
    }
}
