//! Class layers as vector tiles: the slope classes of `terrain` (`slope_5m.tif`), the
//! avalanche exposure classes of `avalanche-exposure` (`exposure_5m.tif`) and the snow
//! classes of `snow` (`snow_depth.tif`, `class_groomed.tif`, `class_srn.tif`, 25 m) become
//! class polygons, cut into Mapbox Vector Tiles per zoom in a PMTiles archive of the pack.
//!
//! The classes and their thresholds are those of the source rasters (IGN slope classes
//! 30, 35, 40, 45°; exposure codes 1, 2, 3; snow classes of `skimap-snow`): nothing is
//! recomputed. Method, on the Lambert-93 grid of the source (5 m, 25 m for the snow,
//! which is not smoothed, [`ClassSpec::smoothing`]):
//!
//! 1. **Smoothing** ([`Smoothing::Fill`]). Each threshold level `class >= k` gains the
//!    cells of which at least 5 of the 9 cells of the 3 × 3 window are in it (cells
//!    without data count as outside); no cell of the source leaves it, so a band of
//!    steep cells one cell wide stays. The operation is monotone: the levels stay
//!    nested.
//! 2. **Small patches.** The 4-connected patches of a level smaller than
//!    [`Params::min_patch_m2`] ([`Params::min_inner_patch_m2`] from the second
//!    threshold up) are removed and its holes smaller than that are filled;
//!    the class of a cell is the highest level that keeps it. Removing a patch of a level
//!    leaves the lower levels: a small patch of 40° and more inside a 35° slope becomes
//!    35 to 40°, not flat. What disappears is measured per class (manifest
//!    `classes.levels`, crate documentation).
//! 3. **Shared boundaries.** The boundaries between the connected regions of the final
//!    classes are cut into chains between junctions; a chain runs through the midpoints
//!    of its cell edges (the staircase of the cells is halved) and is simplified once per
//!    zoom by Douglas-Peucker with its ends fixed. Two neighbouring polygons use the same
//!    simplified chain: no gap, no overlap, at every zoom.
//! 4. **Tiles.** Per zoom the polygons are cut to each tile with a buffer of
//!    [`BUFFER`] units of the 4096 extent, one feature per class and tile (attributes
//!    `class`, `label`). The tolerance of a zoom is [`Params::tolerance_px`] screen
//!    pixels at the next zoom (512-pixel tiles), never below
//!    [`Params::min_tolerance_m`]; below the highest zoom a ring smaller than
//!    [`Params::min_area_px`] square pixels of the next zoom is left out.

use std::collections::HashMap;
use std::path::Path;
use std::time::Instant;

use pmtiles::TileType;
use rayon::prelude::*;
use serde_json::json;
use skimap_common::crs::{Crs, transform_points};
use skimap_common::grid::Grid;

use crate::build::layers::{EntryParts, Outcome, copy_meta, layer_entry, read_meta, restricted};
use crate::build::raster::{
    Class, Read, exposure_classes, grid_bbox_wgs84, read_as, slope_classes, snow_depth_classes,
    snow_practice_classes, write_archive,
};
use crate::error::{Error, Result};
use crate::manifest::{
    ClassArea, ClassTilesInfo, LayerKind, LegendEntry, LevelCheck, Missing, ZoomValue,
};
use crate::mvt::{self, EXTENT, LayerWriter, Ring, Value};
use crate::tiles::TileData;

/// Buffer around each tile, in tile units (4096 per tile): the cut edges of a polygon lie
/// outside the drawn tile, so that the fill outline never shows a seam.
pub const BUFFER: f64 = 64.0;
/// A raw zone of a level at least this large is « significant »: the release-area sieve
/// of the avalanche exposure model (`gdal_sieve` at 1000 m², crates/avalanche).
pub const SIGNIFICANT_M2: f64 = 1000.0;
/// Attribute holding the class value.
pub const CLASS_PROPERTY: &str = "class";

/// Parameters of the vectorisation.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Params {
    /// Smoothing of each level.
    pub smoothing: Smoothing,
    /// Patches and holes of a level smaller than this are removed (m²).
    pub min_patch_m2: f64,
    /// Same for the levels from the second threshold up (slope 35° and more, « très
    /// exposé »): their patches and holes only move cells between two classes the styles
    /// draw, never to an undrawn one.
    pub min_inner_patch_m2: f64,
    /// Smallest simplification tolerance (m), at the highest zooms.
    pub min_tolerance_m: f64,
    /// Simplification tolerance of a zoom, in screen pixels at the next zoom (512-pixel
    /// tiles), the most magnified use of the tiles of a zoom below the highest.
    pub tolerance_px: f64,
    /// Below the highest zoom, rings smaller than this many square screen pixels of the
    /// next zoom are left out.
    pub min_area_px: f64,
    /// Lowest zoom of the tiles.
    pub minzoom: u8,
    /// Highest zoom of the tiles (the map overzooms beyond).
    pub maxzoom: u8,
}

/// Patches smaller than 200 m² (8 cells of 5 m) are removed: see the measurements of the
/// crate documentation (`docs/format.md`, class layers).
pub const MIN_PATCH_M2: f64 = 200.0;

/// Patches and holes of the inner levels smaller than 400 m² (16 cells) are removed:
/// measurements in `docs/format.md`.
pub const MIN_INNER_PATCH_M2: f64 = 400.0;

impl Default for Params {
    fn default() -> Self {
        Self {
            smoothing: Smoothing::Fill,
            min_patch_m2: MIN_PATCH_M2,
            min_inner_patch_m2: MIN_INNER_PATCH_M2,
            min_tolerance_m: 2.5,
            tolerance_px: 1.0,
            min_area_px: 1.0,
            minzoom: 10,
            maxzoom: 13,
        }
    }
}

/// Smoothing of a threshold level before the removal of small patches.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Smoothing {
    /// None.
    None,
    /// 3 × 3 majority: a cell is in the level when at least 5 of the 9 cells of its
    /// window are (thins the bands one cell wide).
    Majority,
    /// 3 × 3 majority that only adds: a cell of the level stays in it, a cell outside
    /// joins it when at least 5 of the 9 cells of its window are in it (fills the
    /// notches, never removes a cell of the source).
    Fill,
}

/// A class layer of the pipeline.
#[derive(Debug, Clone)]
pub struct ClassSpec {
    /// Pack layer identifier, also the vector tile layer name.
    pub id: &'static str,
    /// Display title, French.
    pub title: &'static str,
    /// Pipeline layer directory.
    pub origin: &'static str,
    /// GeoTIFF file in that directory.
    pub file: &'static str,
    /// `<couche>.meta.json`.
    pub meta: &'static str,
    /// Classes, increasing (the raster classification: bounds, labels, legend colours).
    pub classes: Vec<Class>,
    /// Value of the first class (`class` attribute = index + base): 0 for the slope
    /// (0 is below 30°), 1 for the exposure (codes 1, 2, 3 of the exposure layer).
    pub value_base: u8,
    /// First class index drawn (slope: 1, below 30° is not drawn; exposure: 0).
    pub drawn_from: usize,
    /// Labels of the levels `class >= k`, k = 1..classes.len().
    pub level_labels: &'static [&'static str],
    /// How the GeoTIFF is read.
    pub read: Read,
    /// Smoothing of this layer, instead of [`Params::smoothing`]: the snow classes are
    /// not smoothed (the smoothing only adds cells to the higher levels, the optimistic
    /// side for snow, where less snow is the cautious side).
    pub smoothing: Option<Smoothing>,
    /// Shown when the viewer opens.
    pub visible: bool,
    /// Notes, French.
    pub notes: &'static [&'static str],
}

impl ClassSpec {
    /// Class value of a class index.
    pub fn value(&self, idx: usize) -> u8 {
        self.value_base + idx as u8
    }

    /// Legend: the drawn classes.
    pub fn legend(&self) -> Vec<LegendEntry> {
        self.classes
            .iter()
            .enumerate()
            .skip(self.drawn_from)
            .map(|(i, c)| LegendEntry {
                value: self.value(i),
                label: c.label.to_owned(),
                color: format!(
                    "#{:02x}{:02x}{:02x}{:02x}",
                    c.color[0], c.color[1], c.color[2], c.color[3]
                ),
            })
            .collect()
    }
}

/// The class layers, in display order.
pub fn catalog() -> Vec<ClassSpec> {
    vec![
        ClassSpec {
            id: "slope",
            title: "Pentes (LiDAR HD 5 m)",
            origin: "terrain",
            file: "slope_5m.tif",
            meta: "terrain.meta.json",
            classes: slope_classes(),
            value_base: 0,
            drawn_from: 1,
            level_labels: &["30° et plus", "35° et plus", "40° et plus", "45° et plus"],
            read: Read::Float32,
            smoothing: None,
            visible: true,
            notes: &[
                "Classes de la carte des pentes IGN (30, 35, 40, 45°) en polygones vectoriels ; moins de 30° : pas de polygone. Attribut `class` : 1 (30 à 35°) à 4 (45° et plus).",
            ],
        },
        ClassSpec {
            id: "avalanche-exposure",
            title: "Exposition aux avalanches (modèle et CLPA)",
            origin: "avalanche-exposure",
            file: "exposure_5m.tif",
            meta: "avalanche-exposure.meta.json",
            classes: exposure_classes(),
            value_base: 1,
            drawn_from: 0,
            level_labels: &["exposé ou très exposé", "très exposé"],
            read: Read::AvalancheClasses,
            smoothing: None,
            visible: false,
            notes: &[
                "Classes de la couche avalanche-exposure (modèle de type ATES et emprises CLPA fusionnées) en polygones vectoriels ; attribut `class` : 1 « terrain peu exposé (modèle) », 2 « exposé », 3 « très exposé », comme l'attribut `exposure` de avalanche-segments. Une sortie de modèle, jamais « sûr » ni « sans danger » ; consulter le BERA.",
            ],
        },
        snow(
            "snow-depth",
            "Neige estimée (hauteur)",
            "snow_depth.tif",
            snow_depth_classes(),
            &[
                "1 cm et plus",
                "10 cm et plus",
                "20 cm et plus",
                "40 cm et plus",
                "60 cm et plus",
                "100 cm et plus",
            ],
            &[
                "Valeur centrale de la simulation Météo-France projetée sur le relief (cellules de 25 m) en polygones vectoriels ; pas une mesure. Attribut `class` : 0 (moins de 1 cm) à 6 (100 cm et plus).",
            ],
        ),
        snow(
            "snow-groomed",
            "Neige pour piste damée (classes)",
            "class_groomed.tif",
            snow_practice_classes(),
            &["limite ou suffisant", "suffisant"],
            &[
                "Seuils proposés : moins de 20 cm insuffisant, 20 à 40 cm limite, 40 cm et plus suffisant. Attribut `class` : 0 insuffisant, 1 limite, 2 suffisant (polygones vectoriels des cellules de 25 m, sans lissage).",
            ],
        ),
        snow(
            "snow-srn",
            "Neige pour ski hors piste (classes)",
            "class_srn.tif",
            snow_practice_classes(),
            &["limite ou suffisant", "suffisant"],
            &[
                "Seuils proposés : moins de 30 cm insuffisant, 30 à 60 cm limite, 60 cm et plus suffisant. Attribut `class` : 0 insuffisant, 1 limite, 2 suffisant (polygones vectoriels des cellules de 25 m, sans lissage).",
            ],
        ),
    ]
}

/// A snow layer of `snow/` (25 m Float32 rasters): every class drawn, no smoothing, hidden
/// by default.
fn snow(
    id: &'static str,
    title: &'static str,
    file: &'static str,
    classes: Vec<Class>,
    level_labels: &'static [&'static str],
    notes: &'static [&'static str],
) -> ClassSpec {
    ClassSpec {
        id,
        title,
        origin: "snow",
        file,
        meta: "snow.meta.json",
        classes,
        value_base: 0,
        drawn_from: 0,
        level_labels,
        read: Read::Float32,
        smoothing: Some(Smoothing::None),
        visible: false,
        notes,
    }
}

/// No-data class index of a spec.
fn nodata(spec: &ClassSpec) -> u8 {
    spec.classes.len() as u8
}

fn classify(spec: &ClassSpec, v: f32) -> u8 {
    if v.is_nan() {
        return nodata(spec);
    }
    spec.classes
        .iter()
        .position(|c| v < c.upper)
        .unwrap_or(spec.classes.len() - 1) as u8
}

// ---------------------------------------------------------------------------------------
// Raster steps.

/// 3 × 3 binary majority (at least 5 of 9), cells outside the grid or without data
/// counting as outside; the result is restricted to `valid`.
pub fn majority3(mask: &[bool], valid: &[bool], w: usize, h: usize) -> Vec<bool> {
    let mut out = vec![false; w * h];
    out.par_chunks_mut(w).enumerate().for_each(|(y, row)| {
        let y0 = y.saturating_sub(1);
        let y1 = (y + 1).min(h - 1);
        // Column sums over the three rows.
        let col: Vec<u8> = (0..w)
            .map(|x| (y0..=y1).map(|yy| u8::from(mask[yy * w + x])).sum())
            .collect();
        for x in 0..w {
            let x0 = x.saturating_sub(1);
            let x1 = (x + 1).min(w - 1);
            let n: u8 = col[x0..=x1].iter().sum();
            row[x] = n >= 5 && valid[y * w + x];
        }
    });
    out
}

/// Calls `f` with the cells of every 4-connected component of the cells `member` holds,
/// neighbours joined when `same(a, b)`.
fn components(
    w: usize,
    h: usize,
    member: impl Fn(usize) -> bool,
    same: impl Fn(usize, usize) -> bool,
    mut f: impl FnMut(&[u32]),
) {
    let mut seen = vec![false; w * h];
    let mut comp: Vec<u32> = Vec::new();
    for start in 0..w * h {
        if seen[start] || !member(start) {
            continue;
        }
        comp.clear();
        seen[start] = true;
        comp.push(start as u32);
        let mut k = 0;
        while k < comp.len() {
            let i = comp[k] as usize;
            k += 1;
            let (x, y) = (i % w, i / w);
            let mut visit = |j: usize| {
                if !seen[j] && member(j) && same(i, j) {
                    seen[j] = true;
                    comp.push(j as u32);
                }
            };
            if x > 0 {
                visit(i - 1);
            }
            if x + 1 < w {
                visit(i + 1);
            }
            if y > 0 {
                visit(i - w);
            }
            if y + 1 < h {
                visit(i + w);
            }
        }
        f(&comp);
    }
}

/// Removes the patches of `mask` smaller than `min` cells, then fills its holes (cells
/// with data outside the mask) smaller than `min`.
pub fn sieve(mask: &mut [bool], valid: &[bool], w: usize, h: usize, min: usize) {
    if min <= 1 {
        return;
    }
    for value in [true, false] {
        let mut flip: Vec<u32> = Vec::new();
        {
            let m = &*mask;
            components(
                w,
                h,
                |i| m[i] == value && valid[i],
                |_, _| true,
                |c| {
                    if c.len() < min {
                        flip.extend_from_slice(c);
                    }
                },
            );
        }
        for i in flip {
            mask[i as usize] = !value;
        }
    }
}

// ---------------------------------------------------------------------------------------
// Boundary chains between regions.

/// Label of the cells drawn by no polygon (below the first drawn class, no data, outside).
const BG: u32 = u32::MAX;

/// Directions in raster coordinates (y down): east, south, west, north.
const DX: [i64; 4] = [1, 0, -1, 0];
const DY: [i64; 4] = [0, 1, 0, -1];

/// Boundary chains of a label grid, between junctions (corners where three or four
/// edges meet), or closed loops without junction.
#[derive(Debug, Default)]
struct Chains {
    /// Corner coordinates `(x, y)` of every chain, one after the other.
    pts: Vec<[u32; 2]>,
    /// Start of each chain in `pts` (plus the end).
    start: Vec<usize>,
    /// Region on the left of the chain (screen left, y down) and on its right.
    left: Vec<u32>,
    right: Vec<u32>,
    /// Direction of the first and of the last edge.
    first_dir: Vec<u8>,
    last_dir: Vec<u8>,
    /// Closed loop without junction.
    closed: Vec<bool>,
}

impl Chains {
    fn len(&self) -> usize {
        self.left.len()
    }
    fn points(&self, i: usize) -> &[[u32; 2]] {
        &self.pts[self.start[i]..self.start[i + 1]]
    }
}

struct LabelGrid<'a> {
    lab: &'a [u32],
    w: usize,
    h: usize,
}

impl LabelGrid<'_> {
    fn at(&self, x: i64, y: i64) -> u32 {
        if x < 0 || y < 0 || x >= self.w as i64 || y >= self.h as i64 {
            BG
        } else {
            self.lab[y as usize * self.w + x as usize]
        }
    }
    /// Labels on the left and right of the edge leaving corner `(x, y)` in direction `d`.
    fn sides(&self, x: i64, y: i64, d: u8) -> (u32, u32) {
        match d {
            0 => (self.at(x, y - 1), self.at(x, y)),
            2 => (self.at(x - 1, y), self.at(x - 1, y - 1)),
            1 => (self.at(x, y), self.at(x - 1, y)),
            _ => (self.at(x - 1, y - 1), self.at(x, y - 1)),
        }
    }
    fn has_edge(&self, x: i64, y: i64, d: u8) -> bool {
        let (nx, ny) = (x + DX[d as usize], y + DY[d as usize]);
        if nx < 0 || ny < 0 || nx > self.w as i64 || ny > self.h as i64 {
            return false;
        }
        let (l, r) = self.sides(x, y, d);
        l != r
    }
    fn degree(&self, x: i64, y: i64) -> usize {
        (0..4).filter(|&d| self.has_edge(x, y, d)).count()
    }
    /// Index of the edge leaving `(x, y)` in direction `d` in the visited bitmaps:
    /// horizontal edges first (`w × (h + 1)`), then vertical ones (`(w + 1) × h`).
    fn edge_id(&self, x: i64, y: i64, d: u8) -> usize {
        let (w, h) = (self.w as i64, self.h as i64);
        let hz = |ex: i64, ey: i64| (ey * w + ex) as usize;
        let vt = |ex: i64, ey: i64| (w * (h + 1) + ey * (w + 1) + ex) as usize;
        match d {
            0 => hz(x, y),
            2 => hz(x - 1, y),
            1 => vt(x, y),
            _ => vt(x, y - 1),
        }
    }
}

fn extract_chains(g: &LabelGrid<'_>) -> Chains {
    let (w, h) = (g.w as i64, g.h as i64);
    let mut visited = vec![false; (g.w * (g.h + 1)) + (g.w + 1) * g.h];
    let mut c = Chains {
        start: vec![0],
        ..Chains::default()
    };
    let walk = |c: &mut Chains, visited: &mut [bool], x0: i64, y0: i64, d0: u8| {
        let (l, r) = g.sides(x0, y0, d0);
        c.left.push(l);
        c.right.push(r);
        c.first_dir.push(d0);
        c.pts.push([x0 as u32, y0 as u32]);
        let (mut x, mut y, mut d) = (x0, y0, d0);
        let closed;
        loop {
            visited[g.edge_id(x, y, d)] = true;
            x += DX[d as usize];
            y += DY[d as usize];
            c.pts.push([x as u32, y as u32]);
            if x == x0 && y == y0 {
                closed = g.degree(x, y) == 2;
                break;
            }
            if g.degree(x, y) != 2 {
                closed = false;
                break;
            }
            let back = (d + 2) % 4;
            d = (0..4)
                .find(|&nd| nd != back && g.has_edge(x, y, nd))
                .expect("a corner of degree 2 has a second edge");
        }
        c.last_dir.push(d);
        c.closed.push(closed);
        c.start.push(c.pts.len());
    };
    // Chains from the junctions.
    for y in 0..=h {
        for x in 0..=w {
            if g.degree(x, y) < 3 {
                continue;
            }
            for d in 0..4u8 {
                if g.has_edge(x, y, d) && !visited[g.edge_id(x, y, d)] {
                    walk(&mut c, &mut visited, x, y, d);
                }
            }
        }
    }
    // Closed loops without junction: each has a horizontal edge.
    for y in 0..=h {
        for x in 0..w {
            if g.has_edge(x, y, 0) && !visited[g.edge_id(x, y, 0)] {
                walk(&mut c, &mut visited, x, y, 0);
            }
        }
    }
    c
}

/// A chain used by a ring: index and whether it is followed backwards.
type Oriented = (u32, bool);

/// The rings of a region: chains followed with the region on their left, linked at the
/// junctions with the left turn first (diagonal cells of a region stay apart, as in its
/// 4-connected component).
#[derive(Debug, Clone)]
struct RegionRings {
    region: u32,
    rings: Vec<Vec<Oriented>>,
}

fn link_rings(c: &Chains) -> Result<Vec<RegionRings>> {
    // (region, start corner, chain, reversed)
    let key = |p: [u32; 2]| (u64::from(p[1]) << 32) | u64::from(p[0]);
    let mut entries: Vec<(u32, u64, u32, bool)> = Vec::with_capacity(2 * c.len());
    for i in 0..c.len() {
        let p = c.points(i);
        if c.left[i] != BG {
            entries.push((c.left[i], key(p[0]), i as u32, false));
        }
        if c.right[i] != BG {
            entries.push((c.right[i], key(p[p.len() - 1]), i as u32, true));
        }
    }
    entries.sort_unstable();
    let ends = |e: &(u32, u64, u32, bool)| -> (u64, u8, u8) {
        let i = e.2 as usize;
        let p = c.points(i);
        if e.3 {
            (key(p[0]), (c.last_dir[i] + 2) % 4, (c.first_dir[i] + 2) % 4)
        } else {
            (key(p[p.len() - 1]), c.first_dir[i], c.last_dir[i])
        }
    };
    let mut used = vec![false; entries.len()];
    let mut out: Vec<RegionRings> = Vec::new();
    let mut lo = 0;
    while lo < entries.len() {
        let region = entries[lo].0;
        let hi = lo + entries[lo..].partition_point(|e| e.0 == region);
        let mut rings = Vec::new();
        for first in lo..hi {
            if used[first] {
                continue;
            }
            used[first] = true;
            let mut ring = vec![(entries[first].2, entries[first].3)];
            let start_corner = entries[first].1;
            let (mut end, _, mut dir) = ends(&entries[first]);
            loop {
                // Candidates leaving the current corner (and the first chain when back at
                // the start).
                let a = lo + entries[lo..hi].partition_point(|e| e.1 < end);
                let b = lo + entries[lo..hi].partition_point(|e| e.1 <= end);
                let mut pick: Option<usize> = None;
                for want in [(dir + 3) % 4, dir, (dir + 1) % 4] {
                    pick = (a..b).find(|&k| {
                        (!used[k] || (k == first && end == start_corner))
                            && ends(&entries[k]).1 == want
                    });
                    if pick.is_some() {
                        break;
                    }
                }
                let Some(k) = pick else {
                    return Err(Error::Pack(format!(
                        "class polygons: ring of region {region} does not close"
                    )));
                };
                if k == first {
                    break;
                }
                used[k] = true;
                ring.push((entries[k].2, entries[k].3));
                (end, _, dir) = ends(&entries[k]);
            }
            rings.push(ring);
        }
        out.push(RegionRings { region, rings });
        lo = hi;
    }
    Ok(out)
}

/// Twice the signed area (raster coordinates, y down) swept by a chain followed forwards.
fn chain_cross(p: &[[u32; 2]]) -> i64 {
    p.windows(2)
        .map(|s| {
            let (x0, y0) = (i64::from(s[0][0]), i64::from(s[0][1]));
            let (x1, y1) = (i64::from(s[1][0]), i64::from(s[1][1]));
            x0 * y1 - x1 * y0
        })
        .sum()
}

// ---------------------------------------------------------------------------------------
// Simplification.

/// Douglas-Peucker of an open polyline, ends kept.
fn dp_open(p: &[[f64; 2]], tol: f64) -> Vec<[f64; 2]> {
    if p.len() <= 2 {
        return p.to_vec();
    }
    let mut keep = vec![false; p.len()];
    keep[0] = true;
    keep[p.len() - 1] = true;
    let mut stack = vec![(0usize, p.len() - 1)];
    let t2 = tol * tol;
    while let Some((a, b)) = stack.pop() {
        if b <= a + 1 {
            continue;
        }
        let (ax, ay) = (p[a][0], p[a][1]);
        let (dx, dy) = (p[b][0] - ax, p[b][1] - ay);
        let len2 = dx * dx + dy * dy;
        let mut best = (0.0, a);
        for (k, q) in p.iter().enumerate().take(b).skip(a + 1) {
            let (qx, qy) = (q[0] - ax, q[1] - ay);
            let d2 = if len2 == 0.0 {
                qx * qx + qy * qy
            } else {
                let cr = qx * dy - qy * dx;
                cr * cr / len2
            };
            if d2 > best.0 {
                best = (d2, k);
            }
        }
        if best.0 > t2 {
            keep[best.1] = true;
            stack.push((a, best.1));
            stack.push((best.1, b));
        }
    }
    p.iter()
        .zip(keep)
        .filter_map(|(q, k)| k.then_some(*q))
        .collect()
}

/// Douglas-Peucker of a closed loop (`p` without its repeated first point): split at
/// the first point and the point farthest from it. Returns the loop without repetition.
fn dp_closed(p: &[[f64; 2]], tol: f64) -> Vec<[f64; 2]> {
    if p.len() <= 3 {
        return p.to_vec();
    }
    let far = (1..p.len())
        .max_by(|&i, &j| {
            let d = |k: usize| (p[k][0] - p[0][0]).powi(2) + (p[k][1] - p[0][1]).powi(2);
            d(i).total_cmp(&d(j))
        })
        .unwrap_or(1);
    let mut a = dp_open(&p[..=far], tol);
    let mut second: Vec<[f64; 2]> = p[far..].to_vec();
    second.push(p[0]);
    let b = dp_open(&second, tol);
    a.pop();
    a.extend_from_slice(&b[..b.len() - 1]);
    a
}

/// Chain points drawn through the midpoints of its cell edges, ends kept at the corners
/// (junctions); closed loops: midpoints only.
fn midpoints(p: &[[u32; 2]], closed: bool) -> Vec<[f64; 2]> {
    let f = |q: [u32; 2]| [f64::from(q[0]), f64::from(q[1])];
    let mid = |a: [u32; 2], b: [u32; 2]| {
        [
            (f64::from(a[0]) + f64::from(b[0])) / 2.0,
            (f64::from(a[1]) + f64::from(b[1])) / 2.0,
        ]
    };
    let mut out = Vec::with_capacity(p.len() + 1);
    if !closed {
        out.push(f(p[0]));
    }
    for s in p.windows(2) {
        out.push(mid(s[0], s[1]));
    }
    if !closed {
        out.push(f(p[p.len() - 1]));
    }
    out
}

/// Shoelace area of a ring in world coordinates (y up: positive counter-clockwise).
fn area(r: &[[f64; 2]]) -> f64 {
    let n = r.len();
    let mut a = 0.0;
    for i in 0..n {
        let (p, q) = (r[i], r[(i + 1) % n]);
        a += p[0] * q[1] - q[0] * p[1];
    }
    a / 2.0
}

/// Ground metres of a screen pixel at map zoom `z + 1` (512-pixel tiles) at `lat`.
pub fn screen_pixel_m(z: u8, lat: f64) -> f64 {
    40_075_016.686 * lat.to_radians().cos() / (512.0 * f64::from(1u32 << (z + 1)))
}

// ---------------------------------------------------------------------------------------
// Tiles.

/// Sutherland-Hodgman clip of a ring to an axis-aligned rectangle.
fn clip_ring(r: &[[f64; 2]], x0: f64, y0: f64, x1: f64, y1: f64) -> Vec<[f64; 2]> {
    let mut cur = r.to_vec();
    for edge in 0..4 {
        if cur.is_empty() {
            break;
        }
        let inside = |p: &[f64; 2]| match edge {
            0 => p[0] >= x0,
            1 => p[0] <= x1,
            2 => p[1] >= y0,
            _ => p[1] <= y1,
        };
        let cross = |a: &[f64; 2], b: &[f64; 2]| -> [f64; 2] {
            match edge {
                0 | 1 => {
                    let x = if edge == 0 { x0 } else { x1 };
                    let t = (x - a[0]) / (b[0] - a[0]);
                    [x, a[1] + t * (b[1] - a[1])]
                }
                _ => {
                    let y = if edge == 2 { y0 } else { y1 };
                    let t = (y - a[1]) / (b[1] - a[1]);
                    [a[0] + t * (b[0] - a[0]), y]
                }
            }
        };
        let mut next = Vec::with_capacity(cur.len() + 4);
        for i in 0..cur.len() {
            let a = &cur[i];
            let b = &cur[(i + 1) % cur.len()];
            match (inside(a), inside(b)) {
                (true, true) => next.push(*b),
                (true, false) => next.push(cross(a, b)),
                (false, true) => {
                    next.push(cross(a, b));
                    next.push(*b);
                }
                (false, false) => {}
            }
        }
        cur = next;
    }
    cur
}

/// Integer tile ring: rounded, consecutive duplicates removed.
fn quantize(r: &[[f64; 2]], ox: f64, oy: f64) -> Ring {
    let mut out: Ring = Vec::with_capacity(r.len());
    for p in r {
        let q = [(p[0] - ox).round() as i32, (p[1] - oy).round() as i32];
        if out.last() != Some(&q) {
            out.push(q);
        }
    }
    while out.len() > 1 && out.first() == out.last() {
        out.pop();
    }
    out
}

/// A polygon of one class at one zoom, in global tile units of that zoom.
struct ZoomPolygon {
    class: usize,
    /// Exterior first, then the holes; world orientation (exterior counter-clockwise).
    rings: Vec<Vec<[f64; 2]>>,
}

/// Cuts the polygons of a zoom into tiles; returns `(x, y) → rings per class`.
fn cut(polys: &[ZoomPolygon], n_classes: usize) -> HashMap<(u32, u32), Vec<Vec<Ring>>> {
    let e = f64::from(EXTENT);
    let mut tiles: HashMap<(u32, u32), Vec<Vec<Ring>>> = HashMap::new();
    for p in polys {
        let outer = &p.rings[0];
        let (mut x0, mut y0, mut x1, mut y1) = (f64::MAX, f64::MAX, f64::MIN, f64::MIN);
        for q in outer {
            x0 = x0.min(q[0]);
            y0 = y0.min(q[1]);
            x1 = x1.max(q[0]);
            y1 = y1.max(q[1]);
        }
        let t = |v: f64| ((v / e).floor().max(0.0)) as u32;
        for tx in t(x0 - BUFFER)..=t(x1 + BUFFER) {
            for ty in t(y0 - BUFFER)..=t(y1 + BUFFER) {
                let (ox, oy) = (f64::from(tx) * e, f64::from(ty) * e);
                let (cx0, cy0, cx1, cy1) =
                    (ox - BUFFER, oy - BUFFER, ox + e + BUFFER, oy + e + BUFFER);
                let mut rings: Vec<Ring> = Vec::new();
                for (k, r) in p.rings.iter().enumerate() {
                    let clipped = clip_ring(r, cx0, cy0, cx1, cy1);
                    // World orientation flips in tile coordinates (y down): reverse, so
                    // that exteriors have a positive area as MVT requires.
                    let mut q = quantize(&clipped, ox, oy);
                    q.reverse();
                    let a = mvt::ring_area(&q);
                    let ok = q.len() >= 3 && if k == 0 { a > 0.0 } else { a < 0.0 };
                    if !ok {
                        if k == 0 {
                            break;
                        }
                        continue;
                    }
                    rings.push(q);
                }
                if rings.is_empty() {
                    continue;
                }
                tiles
                    .entry((tx, ty))
                    .or_insert_with(|| vec![Vec::new(); n_classes])[p.class]
                    .extend(rings);
            }
        }
    }
    tiles
}

// ---------------------------------------------------------------------------------------
// The whole layer.

/// Result of the vectorisation of a class raster.
#[derive(Debug)]
pub struct Vectorized {
    /// Vector tiles (MVT, not compressed), sorted.
    pub tiles: Vec<TileData>,
    /// Measurements, as written in the manifest.
    pub info: ClassTilesInfo,
    /// Seconds per step.
    pub timings: Vec<(String, f64)>,
}

/// Mercator coordinates in `[0, 1]` (y down) of WGS84 points.
fn mercator(lon: f64, lat: f64) -> [f64; 2] {
    let lat = lat
        .clamp(-85.051_128_779_806_59, 85.051_128_779_806_59)
        .to_radians();
    [
        (lon + 180.0) / 360.0,
        (1.0 - lat.tan().asinh() / std::f64::consts::PI) / 2.0,
    ]
}

/// Vectorises a class raster (see the module documentation).
pub fn vectorize(grid: &Grid<f32>, spec: &ClassSpec, params: &Params) -> Result<Vectorized> {
    let mut timings = Vec::new();
    let t = Instant::now();
    let (w, h) = (grid.width(), grid.height());
    let gt = *grid.transform();
    let cell = gt.pixel_width.abs();
    let cell_m2 = cell * gt.pixel_height.abs();
    let smoothing = spec.smoothing.unwrap_or(params.smoothing);
    let n = spec.classes.len();
    let nd = nodata(spec);
    let raw: Vec<u8> = grid.data().par_iter().map(|&v| classify(spec, v)).collect();
    let valid: Vec<bool> = raw.iter().map(|&c| c != nd).collect();
    let min_cells = (params.min_patch_m2 / cell_m2).ceil() as usize;
    let min_inner_cells = (params.min_inner_patch_m2 / cell_m2).ceil() as usize;
    // Levels from the second threshold up (slope 35° and more, « très exposé »): a
    // removed patch stays in the level below, which the styles draw.
    let first_inner = 2;
    let sig_cells = (SIGNIFICANT_M2 / cell_m2).ceil() as usize;
    timings.push(("classes".to_owned(), t.elapsed().as_secs_f64()));

    // 1-2. Levels: majority, sieve; final class = highest level kept.
    let t = Instant::now();
    let levels: Vec<Vec<bool>> = (1..n)
        .into_par_iter()
        .map(|k| {
            let m: Vec<bool> = raw
                .iter()
                .map(|&c| c != nd && usize::from(c) >= k)
                .collect();
            let mut m = match smoothing {
                Smoothing::None => m,
                Smoothing::Majority => majority3(&m, &valid, w, h),
                Smoothing::Fill => {
                    let mj = majority3(&m, &valid, w, h);
                    m.iter().zip(mj).map(|(a, b)| *a || b).collect()
                }
            };
            let min = if k >= first_inner {
                min_inner_cells
            } else {
                min_cells
            };
            sieve(&mut m, &valid, w, h, min);
            m
        })
        .collect();
    let class: Vec<u8> = (0..w * h)
        .into_par_iter()
        .map(|i| {
            if !valid[i] {
                return nd;
            }
            (1..n).rev().find(|&k| levels[k - 1][i]).unwrap_or(0) as u8
        })
        .collect();
    drop(levels);
    timings.push((
        "smoothing and small patches".to_owned(),
        t.elapsed().as_secs_f64(),
    ));

    // Measurements on the cells: per class, per level, significant zones.
    let t = Instant::now();
    let mut raw_count = vec![0u64; n];
    let mut kept_count = vec![0u64; n];
    for i in 0..w * h {
        if raw[i] != nd {
            raw_count[raw[i] as usize] += 1;
            kept_count[class[i] as usize] += 1;
        }
    }
    let level_checks: Vec<LevelCheck> = (1..n)
        .into_par_iter()
        .map(|k| {
            let (mut raw_n, mut lost, mut gained) = (0u64, 0u64, 0u64);
            for i in 0..w * h {
                if raw[i] == nd {
                    continue;
                }
                let (a, b) = (usize::from(raw[i]) >= k, usize::from(class[i]) >= k);
                raw_n += u64::from(a);
                lost += u64::from(a && !b);
                gained += u64::from(!a && b);
            }
            let (mut zones, mut below_half, mut min_cov, mut largest_gone) =
                (0u64, 0u64, 1.0f64, 0u64);
            components(
                w,
                h,
                |i| raw[i] != nd && usize::from(raw[i]) >= k,
                |_, _| true,
                |c| {
                    let kept = c
                        .iter()
                        .filter(|&&i| usize::from(class[i as usize]) >= k)
                        .count();
                    let cov = kept as f64 / c.len() as f64;
                    if c.len() >= sig_cells {
                        zones += 1;
                        min_cov = min_cov.min(cov);
                        below_half += u64::from(cov < 0.5);
                    }
                    if kept == 0 {
                        largest_gone = largest_gone.max(c.len() as u64);
                    }
                },
            );
            LevelCheck {
                from_value: spec.value(k),
                label: spec
                    .level_labels
                    .get(k - 1)
                    .copied()
                    .unwrap_or("")
                    .to_owned(),
                raster_m2: raw_n as f64 * cell_m2,
                lost_m2: lost as f64 * cell_m2,
                gained_m2: gained as f64 * cell_m2,
                vector_m2: 0.0,
                significant_zones: zones,
                significant_min_coverage: if zones == 0 { 1.0 } else { min_cov },
                significant_below_half: below_half,
                largest_removed_m2: largest_gone as f64 * cell_m2,
            }
        })
        .collect();
    timings.push(("measurements".to_owned(), t.elapsed().as_secs_f64()));

    // 3. Regions (4-connected, same class) and their shared boundary chains.
    let t = Instant::now();
    let mut lab = vec![BG; w * h];
    let mut region_class: Vec<u8> = Vec::new();
    components(
        w,
        h,
        |i| class[i] != nd && usize::from(class[i]) >= spec.drawn_from,
        |a, b| class[a] == class[b],
        |c| {
            let id = region_class.len() as u32;
            region_class.push(class[c[0] as usize]);
            for &i in c {
                lab[i as usize] = id;
            }
        },
    );
    drop(class);
    let lg = LabelGrid { lab: &lab, w, h };
    let chains = extract_chains(&lg);
    let regions = link_rings(&chains)?;
    drop(lab);
    // Exterior and holes of each region (sign of the exact area in cell corners).
    let cross: Vec<i64> = (0..chains.len())
        .map(|i| chain_cross(chains.points(i)))
        .collect();
    let ring_cross = |r: &[Oriented]| -> i64 {
        r.iter()
            .map(|&(i, rev)| {
                if rev {
                    -cross[i as usize]
                } else {
                    cross[i as usize]
                }
            })
            .sum()
    };
    // Region on the left in raster coordinates (y down): the exterior has a negative
    // area there (counter-clockwise on the map).
    let mut polygons: Vec<(usize, Vec<Vec<Oriented>>)> = Vec::new();
    let mut multi_exterior = 0u64;
    for rr in &regions {
        let cls = usize::from(region_class[rr.region as usize]);
        let (mut ext, mut holes): (Vec<&Vec<Oriented>>, Vec<&Vec<Oriented>>) =
            (Vec::new(), Vec::new());
        for r in &rr.rings {
            if ring_cross(r) < 0 {
                ext.push(r);
            } else {
                holes.push(r);
            }
        }
        if ext.len() != 1 {
            multi_exterior += 1;
        }
        // A region has one exterior; should a self-touching boundary give several, the
        // holes go to the largest one (the others keep none).
        ext.sort_by_key(|r| ring_cross(r));
        for (j, e) in ext.iter().enumerate() {
            let mut rings = vec![(*e).clone()];
            if j == 0 {
                rings.extend(holes.iter().map(|r| (*r).clone()));
            }
            polygons.push((cls, rings));
        }
    }
    timings.push(("regions and chains".to_owned(), t.elapsed().as_secs_f64()));

    // 4. Per zoom: simplify the chains, build the rings, cut into tiles.
    let t = Instant::now();
    let bbox = grid_bbox_wgs84(grid)?;
    let lat = bbox.center().lat;
    let zooms: Vec<u8> = (params.minzoom..=params.maxzoom).collect();
    let world: Vec<Vec<[f64; 2]>> = (0..chains.len())
        .into_par_iter()
        .map(|i| {
            midpoints(chains.points(i), chains.closed[i])
                .into_iter()
                .map(|[c, r]| {
                    let (x, y) = gt.pixel_to_world(c, r);
                    [x, y]
                })
                .collect()
        })
        .collect();
    struct ZoomOut {
        zoom: u8,
        tol: f64,
        tiles: Vec<TileData>,
        area: Vec<f64>,
        vertices: u64,
        polygons: u64,
    }
    let per_zoom: Vec<ZoomOut> = zooms
        .par_iter()
        .map(|&z| -> Result<ZoomOut> {
            let px = screen_pixel_m(z, lat);
            let tol = (params.tolerance_px * px).max(params.min_tolerance_m);
            let min_area = if z < params.maxzoom {
                (params.min_area_px * px * px).max(params.min_tolerance_m.powi(2))
            } else {
                params.min_tolerance_m.powi(2)
            };
            // Simplified chains, world and Mercator (global tile units of the zoom).
            let simple: Vec<Option<Vec<[f64; 2]>>> = world
                .par_iter()
                .enumerate()
                .map(|(i, p)| {
                    let s = if chains.closed[i] {
                        dp_closed(p, tol)
                    } else {
                        dp_open(p, tol)
                    };
                    (!chains.closed[i] || s.len() >= 3).then_some(s)
                })
                .collect();
            let scale = f64::from(1u32 << z) * f64::from(EXTENT);
            let merc: Vec<Option<Vec<[f64; 2]>>> = simple
                .par_iter()
                .map(|s| -> Result<Option<Vec<[f64; 2]>>> {
                    let Some(s) = s else { return Ok(None) };
                    let mut pts: Vec<(f64, f64)> = s.iter().map(|p| (p[0], p[1])).collect();
                    transform_points(grid.crs(), Crs::Wgs84, &mut pts)?;
                    Ok(Some(
                        pts.into_iter()
                            .map(|(lon, la)| {
                                let m = mercator(lon, la);
                                [m[0] * scale, m[1] * scale]
                            })
                            .collect(),
                    ))
                })
                .collect::<Result<_>>()?;
            let ring = |r: &[Oriented], pts: &[Option<Vec<[f64; 2]>>]| -> Option<Vec<[f64; 2]>> {
                let mut out: Vec<[f64; 2]> = Vec::new();
                for &(i, rev) in r {
                    let p = pts[i as usize].as_ref()?;
                    let it: Box<dyn Iterator<Item = &[f64; 2]>> = if rev {
                        Box::new(p.iter().rev())
                    } else {
                        Box::new(p.iter())
                    };
                    for (k, q) in it.enumerate() {
                        if k == 0 && !out.is_empty() {
                            continue;
                        }
                        out.push(*q);
                    }
                }
                if out.len() > 1 && out.first() == out.last() {
                    out.pop();
                }
                (out.len() >= 3).then_some(out)
            };
            let mut area_by_class = vec![0.0; n];
            let mut zpolys: Vec<ZoomPolygon> = Vec::with_capacity(polygons.len());
            let mut vertices = 0u64;
            for (cls, rings) in &polygons {
                let Some(ext_w) = ring(&rings[0], &simple) else {
                    continue;
                };
                let a_ext = area(&ext_w);
                if a_ext < min_area {
                    continue;
                }
                let Some(ext_m) = ring(&rings[0], &merc) else {
                    continue;
                };
                let mut a = a_ext;
                let mut zr = vec![ext_m];
                for hole in &rings[1..] {
                    let (Some(hw), Some(hm)) = (ring(hole, &simple), ring(hole, &merc)) else {
                        continue;
                    };
                    let ah = area(&hw);
                    if -ah < min_area {
                        continue;
                    }
                    a += ah;
                    zr.push(hm);
                }
                vertices += zr.iter().map(|r| r.len() as u64).sum::<u64>();
                area_by_class[*cls] += a;
                zpolys.push(ZoomPolygon {
                    class: *cls,
                    rings: zr,
                });
            }
            let cutted = cut(&zpolys, n);
            let mut tiles: Vec<TileData> = Vec::with_capacity(cutted.len());
            for ((x, y), per_class) in cutted {
                let mut lw = LayerWriter::new(spec.id);
                for (cls, rings) in per_class.iter().enumerate() {
                    if rings.is_empty() {
                        continue;
                    }
                    lw.add_polygon(
                        &[
                            (CLASS_PROPERTY, Value::Int(i64::from(spec.value(cls)))),
                            ("label", Value::Str(spec.classes[cls].label.to_owned())),
                        ],
                        rings,
                    );
                }
                if let Some(b) = mvt::encode_tile(&[lw], EXTENT) {
                    tiles.push(((z, x, y), b));
                }
            }
            Ok(ZoomOut {
                zoom: z,
                tol,
                tiles,
                area: area_by_class,
                vertices,
                polygons: zpolys.len() as u64,
            })
        })
        .collect::<Result<_>>()?;
    timings.push((
        "simplification and tiles".to_owned(),
        t.elapsed().as_secs_f64(),
    ));

    let top = per_zoom
        .iter()
        .find(|z| z.zoom == params.maxzoom)
        .ok_or_else(|| Error::Pack("class tiles: no highest zoom".into()))?;
    let mut levels = level_checks;
    for l in &mut levels {
        let k = usize::from(l.from_value - spec.value_base);
        l.vector_m2 = top.area[k..].iter().sum();
    }
    let classes = (spec.drawn_from..n)
        .map(|c| ClassArea {
            value: spec.value(c),
            label: spec.classes[c].label.to_owned(),
            raster_m2: raw_count[c] as f64 * cell_m2,
            filtered_m2: kept_count[c] as f64 * cell_m2,
            vector_m2: per_zoom
                .iter()
                .map(|z| ZoomValue {
                    zoom: z.zoom,
                    value: z.area[c],
                })
                .collect(),
        })
        .collect();
    let mut tiles: Vec<TileData> = per_zoom
        .iter()
        .flat_map(|z| z.tiles.iter().cloned())
        .collect();
    tiles.sort_by_key(|t| t.0);
    let info = ClassTilesInfo {
        minzoom: params.minzoom,
        maxzoom: params.maxzoom,
        extent: EXTENT,
        tile_count: tiles.len() as u64,
        source_layer: spec.id.to_owned(),
        property: CLASS_PROPERTY.to_owned(),
        legend: spec.legend(),
        source: format!(
            "{}/{} : EPSG:{}, {w} × {h} cellules de {cell} m",
            spec.origin,
            spec.file,
            grid.crs().epsg()
        ),
        method: format!(
            "niveaux « classe ≥ k » lissés par 3 × 3 ({}), taches et trous de moins de {} m² supprimés au premier seuil et de moins de {} m² aux seuils suivants (4-connexité), frontières partagées entre régions par les milieux des côtés des cellules, Douglas-Peucker par zoom (moitié d'un pixel d'écran au zoom suivant, au moins {} m), découpe par tuile avec une marge de {BUFFER} unités sur {EXTENT}",
            match smoothing {
                Smoothing::None => "aucun",
                Smoothing::Majority => "majorité",
                Smoothing::Fill => "majorité qui ajoute seulement",
            },
            params.min_patch_m2,
            params.min_inner_patch_m2,
            params.min_tolerance_m
        ),
        min_patch_m2: params.min_patch_m2,
        min_inner_patch_m2: params.min_inner_patch_m2,
        significant_m2: SIGNIFICANT_M2,
        tolerance_m: per_zoom
            .iter()
            .map(|z| ZoomValue {
                zoom: z.zoom,
                value: z.tol,
            })
            .collect(),
        polygons: top.polygons,
        vertices: per_zoom
            .iter()
            .map(|z| ZoomValue {
                zoom: z.zoom,
                value: z.vertices as f64,
            })
            .collect(),
        regions_with_several_exteriors: multi_exterior,
        classes,
        levels,
    };
    Ok(Vectorized {
        tiles,
        info,
        timings,
    })
}

/// Builds a class layer from the pipeline GeoTIFF, if present: `layers/<id>.pmtiles`.
pub fn include(
    spec: &ClassSpec,
    out: &Path,
    root: &Path,
    params: &Params,
    personal_sources: bool,
) -> Result<(Outcome, Vec<(String, f64)>)> {
    let dir = out.join(spec.origin);
    let src = dir.join(spec.file);
    if !src.is_file() {
        return Ok((
            Outcome::Missing(Missing {
                id: spec.id.to_owned(),
                reason: format!(
                    "{}/{} absent des sorties du pipeline",
                    spec.origin, spec.file
                ),
            }),
            Vec::new(),
        ));
    }
    let meta = read_meta(&dir.join(spec.meta))?;
    if meta.as_ref().is_some_and(|m| restricted(&m.meta.sources)) && !personal_sources {
        return Ok((
            Outcome::Missing(Missing {
                id: spec.id.to_owned(),
                reason: "source à usage personnel seulement : option personal_sources désactivée"
                    .to_owned(),
            }),
            Vec::new(),
        ));
    }
    let t = Instant::now();
    let grid = read_as(spec.read, &src)?;
    let read_s = t.elapsed().as_secs_f64();
    let v = vectorize(&grid, spec, params)?;
    let mut timings = vec![("read".to_owned(), read_s)];
    timings.extend(v.timings);
    let t = Instant::now();
    let bbox = grid_bbox_wgs84(&grid)?;
    let file = format!("layers/{}.pmtiles", spec.id);
    let attribution: Vec<String> = meta
        .as_ref()
        .map(|m| {
            m.meta
                .sources
                .iter()
                .map(|s| s.attribution.clone())
                .filter(|a| !a.is_empty())
                .collect()
        })
        .unwrap_or_default();
    let metadata = json!({
        "name": spec.title,
        "type": "overlay",
        "format": "pbf",
        "attribution": attribution.join(" ; "),
        "vector_layers": [{
            "id": spec.id,
            "fields": {"class": "Number", "label": "String"},
            "minzoom": params.minzoom,
            "maxzoom": params.maxzoom,
        }],
        "skimap:legend": spec.legend(),
    });
    let bytes = write_archive(
        &root.join(&file),
        TileType::Mvt,
        None,
        &v.tiles,
        &metadata,
        bbox,
        params.minzoom,
        params.maxzoom,
    )?;
    timings.push(("write".to_owned(), t.elapsed().as_secs_f64()));
    let meta_rel = copy_meta(&dir, spec.meta, root)?;
    let mut l = layer_entry(
        EntryParts {
            id: spec.id,
            title: spec.title,
            kind: LayerKind::Vector,
            format: "pmtiles".to_owned(),
            file,
            origin: format!("{}/{}", spec.origin, spec.file),
            meta_rel,
            bytes,
            visible: spec.visible,
            notes: spec.notes,
        },
        meta.as_ref(),
    );
    l.features = Some(v.info.polygons);
    l.geometry_types = vec!["Polygon".to_owned()];
    l.classes = Some(v.info);
    Ok((Outcome::Included(Box::new(l)), timings))
}

#[cfg(test)]
mod tests {
    use super::*;
    use skimap_common::grid::GeoTransform;

    #[test]
    fn majority_is_monotone_and_drops_single_cells() {
        let (w, h) = (6, 5);
        let valid = vec![true; w * h];
        let mut m = vec![false; w * h];
        m[2 * w + 2] = true; // lone cell
        let out = majority3(&m, &valid, w, h);
        assert!(out.iter().all(|&b| !b));
        let full = majority3(&vec![true; w * h], &valid, w, h);
        // Corners see 4 cells: out; edges 6: in.
        assert!(!full[0] && full[1] && full[w + 1]);
    }

    #[test]
    fn sieve_removes_patches_and_fills_holes() {
        let (w, h) = (8, 8);
        let valid = vec![true; w * h];
        let mut m = vec![false; w * h];
        for y in 1..7 {
            for x in 1..7 {
                m[y * w + x] = true;
            }
        }
        m[3 * w + 3] = false; // hole of 1 cell
        m[7 * w + 7] = true; // patch of 1 cell
        sieve(&mut m, &valid, w, h, 2);
        assert!(m[3 * w + 3]);
        assert!(!m[7 * w + 7]);
        assert_eq!(m.iter().filter(|&&b| b).count(), 36);
    }

    #[test]
    fn simplification_keeps_ends() {
        let line = vec![[0.0, 0.0], [1.0, 0.1], [2.0, -0.1], [3.0, 0.0]];
        assert_eq!(dp_open(&line, 0.5), vec![[0.0, 0.0], [3.0, 0.0]]);
        assert_eq!(dp_open(&line, 0.01).len(), 4);
        let sq = vec![
            [0.0, 0.0],
            [5.0, 0.1],
            [10.0, 0.0],
            [10.0, 10.0],
            [0.0, 10.0],
        ];
        let s = dp_closed(&sq, 0.5);
        assert_eq!(s.len(), 4);
    }

    /// A synthetic slope: a 40° block with a 50° core and a 35° strip, on flat ground,
    /// one lone steep cell. The polygons must partition the cells: per class, the
    /// vector area equals the cell area after filtering, within the simplification.
    #[test]
    fn polygons_partition_the_classes() {
        let (w, h) = (120usize, 100usize);
        let gt = GeoTransform::north_up(990_000.0, 6_420_000.0, 5.0);
        let mut data = vec![10.0f32; w * h];
        for y in 20..80 {
            for x in 20..100 {
                data[y * w + x] = 42.0;
            }
        }
        for y in 40..60 {
            for x in 40..60 {
                data[y * w + x] = 50.0;
            }
        }
        for y in 85..95 {
            for x in 10..110 {
                data[y * w + x] = 33.0;
            }
        }
        data[5 * w + 5] = 60.0;
        data[..w].fill(f32::NAN);
        let grid = Grid::from_vec(w, h, gt, Crs::Lambert93, data).unwrap();
        let spec = catalog().into_iter().find(|s| s.id == "slope").unwrap();
        let params = Params {
            minzoom: 13,
            maxzoom: 15,
            ..Params::default()
        };
        let v = vectorize(&grid, &spec, &params).unwrap();
        assert!(!v.tiles.is_empty());
        let info = &v.info;
        assert_eq!(info.regions_with_several_exteriors, 0);
        for c in &info.classes {
            let top = c.vector_m2.last().unwrap().value;
            // Smoothing rounds the corners of the blocks; the lone cell is gone.
            let tol = 0.03 * c.filtered_m2.max(1.0) + 200.0;
            assert!(
                (top - c.filtered_m2).abs() <= tol,
                "class {}: vector {top} m², cells {} m²",
                c.value,
                c.filtered_m2
            );
        }
        // 45° and more: the 50° core only (400 cells = 10 000 m²).
        let c4 = info.classes.iter().find(|c| c.value == 4).unwrap();
        assert_eq!(c4.raster_m2, 10_025.0); // core and lone cell
        // Significant zones all kept, the lone cell (25 m²) removed.
        for l in &info.levels {
            assert!(l.significant_min_coverage > 0.9, "{l:?}");
        }
        assert_eq!(info.levels[0].largest_removed_m2, 25.0);
        // Tiles decode, rings oriented, classes as attributes.
        for (_, t) in &v.tiles {
            let layers = mvt::decode(t).unwrap();
            assert_eq!(layers[0].name, "slope");
            for f in &layers[0].features {
                let c = f.get("class").and_then(Value::as_i64).unwrap();
                assert!((1..=4).contains(&c));
                assert!(mvt::ring_area(&f.parts[0]) > 0.0);
            }
        }
    }
}
