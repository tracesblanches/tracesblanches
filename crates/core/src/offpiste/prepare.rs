//! Pipeline side of C3 (#35), feature `pipeline`: builds the off-piste rasters of a site
//! from the terrain, zones, pistes and snow layers (moved from the `offpiste_bench`
//! example so that the pack, P14, can call it).
//!
//! - elevation and slope from `terrain/dem_5m.tif`, `terrain/slope_5m.tif` (Float32,
//!   Lambert-93, 5 m), quantised to decimetres (`u16`) and half degrees (`u8`, floor);
//! - zones of `zones/zones.geojson` rasterised by scanline at the cell centres (rings
//!   densified to 0.0005° then projected to Lambert-93, even-odd rule per polygon, holes
//!   included); cells whose centre lies within 1 m of a polygon edge are decided by
//!   `ZoneLayer::containing` itself (boundary included, as `zone_eval`); one class per
//!   distinct set of containing zones; each class gets a representative cell whose
//!   `ZoneLayer::containing` set is checked to be the class set, and 20 000 random cells
//!   are checked the same way;
//! - pistes of `pistes/graph_edges.geojson` rasterised by grid traversal (every cell a
//!   segment passes through): groomed nordic pistes (`kind = piste`, not `backcountry`,
//!   `uses` contains `nordic`) and other marked itineraries;
//! - snow classes of each snow layer (`class_srn.tif`, `class_groomed.tif`, 25 m), date
//!   taken from `snow.meta.json` (`valid_from`).

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::Instant;

use serde_json::{Value, json};
use skimap_common::crs::{self, Crs};
use skimap_common::geom::LonLat;
use skimap_common::geotiff;
use skimap_common::grid::{GeoTransform, Grid};
use skimap_common::layout::DataLayout;
use skimap_common::meta::{FileMeta, LayerMeta};
use skimap_common::site::Site;
use skimap_zones::layer::ZoneLayer;

use super::{
    ELEVATION_NODATA, LAYER, OffpisteGrid, PISTE_GROOMED, PISTE_MARKED, PISTE_NONE, SnowGrid,
    ZoneClass,
};
use crate::error::{Error, Result};
use crate::offpiste;

fn err(msg: impl Into<String>) -> Error {
    Error::format("offpiste prepare", msg)
}

pub(crate) fn same_grid<A, B>(a: &Grid<A>, b: &Grid<B>) -> bool {
    a.width() == b.width()
        && a.height() == b.height()
        && a.transform() == b.transform()
        && a.crs() == b.crs()
}

/// Densifies a WGS84 ring (at most 0.0005° per edge, about 40 to 55 m) and projects it to
/// Lambert-93: a long edge is straight in longitude / latitude (as for
/// `ZoneLayer::containing`), not in Lambert-93 (a 14 km edge along a parallel bulges by
/// several metres).
fn project_ring(points: &[(f64, f64)]) -> Result<Vec<(f64, f64)>> {
    let mut dense = Vec::with_capacity(points.len());
    for e in points.windows(2) {
        let ((x1, y1), (x2, y2)) = (e[0], e[1]);
        let n = (((x2 - x1).abs().max((y2 - y1).abs())) / 0.0005)
            .ceil()
            .max(1.0) as usize;
        for k in 0..n {
            let f = k as f64 / n as f64;
            dense.push((x1 + (x2 - x1) * f, y1 + (y2 - y1) * f));
        }
    }
    if let Some(last) = points.last() {
        dense.push(*last);
    }
    project(&dense)
}

fn project(points: &[(f64, f64)]) -> Result<Vec<(f64, f64)>> {
    let mut p = points.to_vec();
    crs::transform_points(Crs::Wgs84, Crs::Lambert93, &mut p)?;
    Ok(p)
}

/// Calls `set(index)` for every cell whose centre is inside the polygon (rings in
/// Lambert-93, even-odd rule).
fn fill_polygon(
    rings: &[Vec<(f64, f64)>],
    t: &GeoTransform,
    w: usize,
    h: usize,
    mut set: impl FnMut(usize),
) {
    let row_of = |y: f64| (y - t.origin_y) / t.pixel_height - 0.5;
    let col_of = |x: f64| (x - t.origin_x) / t.pixel_width - 0.5;
    let yc = |r: i64| t.origin_y + (r as f64 + 0.5) * t.pixel_height;
    let (mut ymin, mut ymax) = (f64::INFINITY, f64::NEG_INFINITY);
    for ring in rings {
        for p in ring {
            ymin = ymin.min(p.1);
            ymax = ymax.max(p.1);
        }
    }
    if !ymin.is_finite() {
        return;
    }
    let r_from = (row_of(ymax).floor() as i64).max(0);
    let r_to = (row_of(ymin).ceil() as i64).min(h as i64 - 1);
    if r_from > r_to {
        return;
    }
    let mut buckets: Vec<Vec<f64>> = vec![Vec::new(); (r_to - r_from + 1) as usize];
    for ring in rings {
        for e in ring.windows(2) {
            let ((x1, y1), (x2, y2)) = (e[0], e[1]);
            if y1 == y2 {
                continue;
            }
            let (ylo, yhi) = (y1.min(y2), y1.max(y2));
            let a = (row_of(yhi).floor() as i64).max(r_from);
            let b = (row_of(ylo).ceil() as i64).min(r_to);
            for r in a..=b {
                let y = yc(r);
                if y >= ylo && y < yhi {
                    buckets[(r - r_from) as usize].push(x1 + (y - y1) * (x2 - x1) / (y2 - y1));
                }
            }
        }
    }
    for (k, xs) in buckets.iter_mut().enumerate() {
        xs.sort_by(f64::total_cmp);
        let r = r_from as usize + k;
        for pair in xs.as_chunks::<2>().0 {
            let c0 = (col_of(pair[0]).ceil() as i64).max(0);
            let c1 = (col_of(pair[1]).floor() as i64).min(w as i64 - 1);
            for c in c0..=c1 {
                set(r * w + c as usize);
            }
        }
    }
}

/// Distance from a point to a segment.
pub fn segment_distance(p: (f64, f64), a: (f64, f64), b: (f64, f64)) -> f64 {
    let (vx, vy) = (b.0 - a.0, b.1 - a.1);
    let l2 = vx * vx + vy * vy;
    let u = if l2 > 0.0 {
        (((p.0 - a.0) * vx + (p.1 - a.1) * vy) / l2).clamp(0.0, 1.0)
    } else {
        0.0
    };
    ((a.0 + u * vx - p.0).powi(2) + (a.1 + u * vy - p.1).powi(2)).sqrt()
}

/// Visits every cell a segment passes through (fractional pixel coordinates).
fn traverse(p0: (f64, f64), p1: (f64, f64), mut visit: impl FnMut(i64, i64)) {
    let (mut cx, mut cy) = (p0.0.floor() as i64, p0.1.floor() as i64);
    let (ex, ey) = (p1.0.floor() as i64, p1.1.floor() as i64);
    let (dx, dy) = (p1.0 - p0.0, p1.1 - p0.1);
    let (sx, sy) = (dx.signum() as i64, dy.signum() as i64);
    let tdx = if dx != 0.0 {
        1.0 / dx.abs()
    } else {
        f64::INFINITY
    };
    let tdy = if dy != 0.0 {
        1.0 / dy.abs()
    } else {
        f64::INFINITY
    };
    let mut tmx = if dx > 0.0 {
        ((cx + 1) as f64 - p0.0) / dx
    } else if dx < 0.0 {
        (cx as f64 - p0.0) / dx
    } else {
        f64::INFINITY
    };
    let mut tmy = if dy > 0.0 {
        ((cy + 1) as f64 - p0.1) / dy
    } else if dy < 0.0 {
        (cy as f64 - p0.1) / dy
    } else {
        f64::INFINITY
    };
    visit(cx, cy);
    let n = (ex - cx).abs() + (ey - cy).abs();
    for _ in 0..n {
        if tmx < tmy {
            cx += sx;
            tmx += tdx;
        } else {
            cy += sy;
            tmy += tdy;
        }
        visit(cx, cy);
    }
}

/// A piste edge: raster value and Lambert-93 polyline.
pub type PisteLine = (u8, Vec<(f64, f64)>);

/// Piste edges of `graph_edges.geojson`, in Lambert-93.
pub fn load_pistes(path: &Path) -> Result<Vec<PisteLine>> {
    let text = std::fs::read_to_string(path).map_err(|e| Error::io(path, e))?;
    let v: Value =
        serde_json::from_str(&text).map_err(|e| Error::json(path.display().to_string(), e))?;
    let mut out = Vec::new();
    for f in v["features"]
        .as_array()
        .ok_or_else(|| err("pistes: no features"))?
    {
        let p = &f["properties"];
        let groomed = p["kind"] == "piste"
            && p["backcountry"] != true
            && p["uses"]
                .as_array()
                .is_some_and(|u| u.iter().any(|x| x == "nordic"));
        let coords: Vec<(f64, f64)> = f["geometry"]["coordinates"]
            .as_array()
            .ok_or_else(|| err("pistes: not a LineString"))?
            .iter()
            .map(|c| {
                (
                    c[0].as_f64().unwrap_or(f64::NAN),
                    c[1].as_f64().unwrap_or(f64::NAN),
                )
            })
            .collect();
        out.push((
            if groomed { PISTE_GROOMED } else { PISTE_MARKED },
            project(&coords)?,
        ));
    }
    Ok(out)
}

struct XorShift(u64);
impl XorShift {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
}

fn containing_ids(layer: &ZoneLayer, p: LonLat) -> Vec<String> {
    let mut v: Vec<String> = layer.containing(p).map(|z| z.id().to_owned()).collect();
    v.sort();
    v
}

/// Prepares the off-piste rasters of a site (pipeline side of C3): reads `terrain`,
/// `zones`, `pistes` and the given snow layer directories, writes
/// `data/out/<site>/offpiste/` and its `offpiste.meta.json`, and returns the metadata.
/// The zone rasterisation is checked against `ZoneLayer::containing` on 20 000 random
/// cells (`stats.zones.check_mismatches`).
pub fn prepare(s: &Site, layout: &DataLayout, snow_dirs: &[PathBuf]) -> Result<LayerMeta> {
    let t_all = Instant::now();
    let out = layout.layer_dir(s, LAYER)?;
    DataLayout::ensure(&out)?;
    let terrain = layout.layer_dir(s, "terrain")?;
    let t0 = Instant::now();
    let dem = geotiff::read_f32(&terrain.join("dem_5m.tif"))?;
    let slope = geotiff::read_f32(&terrain.join("slope_5m.tif"))?;
    if !same_grid(&dem, &slope) || dem.crs() != Crs::Lambert93 {
        return Err(err("dem and slope grids differ or are not Lambert-93"));
    }
    let (w, h) = (dem.width(), dem.height());
    let n = w * h;
    let t = *dem.transform();
    let read_s = t0.elapsed().as_secs_f64();

    let t0 = Instant::now();
    let mut elevation = dem.map(|v| offpiste::elevation_raw(*v).unwrap_or(ELEVATION_NODATA));
    let mut slope_q = slope.map(|v| offpiste::slope_raw(*v));
    let mut missing = 0usize;
    for i in 0..n {
        if elevation.data()[i] == ELEVATION_NODATA || slope_q.data()[i] == offpiste::SLOPE_NODATA {
            elevation.data_mut()[i] = ELEVATION_NODATA;
            slope_q.data_mut()[i] = offpiste::SLOPE_NODATA;
            missing += 1;
        }
    }
    let steep = slope_q
        .data()
        .iter()
        .filter(|q| **q != offpiste::SLOPE_NODATA && **q >= 60)
        .count();
    let quantise_s = t0.elapsed().as_secs_f64();

    // Zones.
    let t0 = Instant::now();
    let zdir = layout.layer_dir(s, "zones")?;
    let zpath = zdir.join("zones.geojson");
    let ztext = std::fs::read_to_string(&zpath).map_err(|e| Error::io(&zpath, e))?;
    let layer = ZoneLayer::from_geojson_str(&ztext).map_err(|e| err(e.to_string()))?;
    if layer.zones.len() > 128 {
        return Err(err(format!(
            "{} zones, at most 128 supported",
            layer.zones.len()
        )));
    }
    let mut mask = vec![0u128; n];
    let mut near: Vec<usize> = Vec::new();
    for (k, z) in layer.zones.iter().enumerate() {
        for poly in &z.geometry.0 {
            let mut rings = vec![project_ring(
                &poly
                    .exterior()
                    .coords()
                    .map(|c| (c.x, c.y))
                    .collect::<Vec<_>>(),
            )?];
            for hole in poly.interiors() {
                rings.push(project_ring(
                    &hole.coords().map(|c| (c.x, c.y)).collect::<Vec<_>>(),
                )?);
            }
            fill_polygon(&rings, &t, w, h, |i| mask[i] |= 1u128 << k);
            // Cells whose centre is within 1 m of an edge: decided below by
            // ZoneLayer::containing itself (boundary included), as zone_eval does.
            for ring in &rings {
                for e in ring.windows(2) {
                    let a = t.world_to_pixel(e[0].0, e[0].1);
                    let b = t.world_to_pixel(e[1].0, e[1].1);
                    traverse(a, b, |c, r| {
                        if c >= 0 && r >= 0 && (c as usize) < w && (r as usize) < h {
                            let ctr = t.cell_center(c as usize, r as usize);
                            if segment_distance(ctr, e[0], e[1]) < 1.0 {
                                near.push(r as usize * w + c as usize);
                            }
                        }
                    });
                }
            }
        }
    }
    near.sort_unstable();
    near.dedup();
    let index_of: HashMap<&str, usize> = layer
        .zones
        .iter()
        .enumerate()
        .map(|(k, z)| (z.id(), k))
        .collect();
    let mut near_changed = 0usize;
    for &i in &near {
        let (x, y) = t.cell_center(i % w, i / w);
        let p = crs::from_lambert93(x, y)?;
        let m = layer
            .containing(p)
            .fold(0u128, |m, z| m | (1u128 << index_of[z.id()]));
        if m != mask[i] {
            near_changed += 1;
            mask[i] = m;
        }
    }
    let mut class_of: HashMap<u128, u8> = HashMap::new();
    let mut masks: Vec<u128> = Vec::new();
    let mut zones_q = Grid::from_vec(w, h, t, Crs::Lambert93, vec![0u8; n])?;
    for (i, m) in mask.iter().enumerate() {
        let c = match class_of.get(m) {
            Some(c) => *c,
            None => {
                let c = u8::try_from(masks.len()).map_err(|_| err("more than 256 zone classes"))?;
                class_of.insert(*m, c);
                masks.push(*m);
                c
            }
        };
        zones_q.data_mut()[i] = c;
    }
    let ids_of = |m: u128| -> Vec<String> {
        let mut v: Vec<String> = (0..layer.zones.len())
            .filter(|k| m & (1u128 << k) != 0)
            .map(|k| layer.zones[k].id().to_owned())
            .collect();
        v.sort();
        v
    };
    let mut counts = vec![0u64; masks.len()];
    // Candidate representative cells: interior cells (8 neighbours of the same class)
    // first, then any cell.
    let mut interior: Vec<Vec<usize>> = vec![Vec::new(); masks.len()];
    let mut any: Vec<Vec<usize>> = vec![Vec::new(); masks.len()];
    for r in 0..h {
        for c in 0..w {
            let i = r * w + c;
            let k = usize::from(zones_q.data()[i]);
            counts[k] += 1;
            if any[k].len() < 64 {
                any[k].push(i);
            }
            if interior[k].len() < 64 && r > 0 && c > 0 && r + 1 < h && c + 1 < w {
                let same = [
                    i - w - 1,
                    i - w,
                    i - w + 1,
                    i - 1,
                    i + 1,
                    i + w - 1,
                    i + w,
                    i + w + 1,
                ]
                .iter()
                .all(|j| zones_q.data()[*j] == zones_q.data()[i]);
                if same && (counts[k] % 97 == 1 || interior[k].is_empty()) {
                    interior[k].push(i);
                }
            }
        }
    }
    let mut classes = Vec::with_capacity(masks.len());
    for (k, m) in masks.iter().enumerate() {
        let ids = ids_of(*m);
        let mut rep = None;
        for &i in interior[k].iter().chain(any[k].iter()) {
            let p =
                crs::from_lambert93(t.cell_center(i % w, i / w).0, t.cell_center(i % w, i / w).1)?;
            if containing_ids(&layer, p) == ids {
                rep = Some(p);
                break;
            }
        }
        if rep.is_none() {
            for &i in interior[k].iter().chain(any[k].iter()).take(5) {
                let (x, y) = t.cell_center(i % w, i / w);
                let p = crs::from_lambert93(x, y)?;
                eprintln!(
                    "class {k} ({} cells) cell {} {}: {:?} vs {:?}",
                    counts[k],
                    i % w,
                    i / w,
                    p,
                    containing_ids(&layer, p)
                );
            }
        }
        let rep =
            rep.ok_or_else(|| err(format!("zone class {k} {ids:?}: no representative cell")))?;
        classes.push(ZoneClass {
            zone_ids: ids,
            representative: rep,
            cells: counts[k],
        });
    }
    // Random check of the rasterisation against ZoneLayer::containing.
    let mut rng = XorShift(0x9E37_79B9_7F4A_7C15);
    let sample = 20_000usize;
    let mut mismatches = Vec::new();
    for _ in 0..sample {
        let i = (rng.next() % n as u64) as usize;
        let (x, y) = t.cell_center(i % w, i / w);
        let p = crs::from_lambert93(x, y)?;
        let expect = &classes[usize::from(zones_q.data()[i])].zone_ids;
        let got = containing_ids(&layer, p);
        if &got != expect {
            mismatches.push(json!({"cell": [i % w, i / w], "lon": p.lon, "lat": p.lat, "raster": expect, "containing": got}));
        }
    }
    let zones_s = t0.elapsed().as_secs_f64();
    drop(mask);

    // Pistes.
    let t0 = Instant::now();
    let edges = load_pistes(&layout.layer_dir(s, "pistes")?.join("graph_edges.geojson"))?;
    let mut pistes = Grid::from_vec(w, h, t, Crs::Lambert93, vec![PISTE_NONE; n])?;
    for (v, line) in &edges {
        for seg in line.windows(2) {
            let a = t.world_to_pixel(seg[0].0, seg[0].1);
            let b = t.world_to_pixel(seg[1].0, seg[1].1);
            traverse(a, b, |c, r| {
                if c >= 0 && r >= 0 && (c as usize) < w && (r as usize) < h {
                    let cell = &mut pistes.data_mut()[r as usize * w + c as usize];
                    *cell = (*cell).max(*v);
                }
            });
        }
    }
    let piste_counts =
        [PISTE_MARKED, PISTE_GROOMED].map(|v| pistes.data().iter().filter(|x| **x == v).count());
    let pistes_s = t0.elapsed().as_secs_f64();

    let t0 = Instant::now();
    let grid = OffpisteGrid::new(elevation, slope_q, zones_q, pistes, classes)?;
    let files = grid.write(&out)?;
    let write_s = t0.elapsed().as_secs_f64();

    // Snow layers.
    let mut snow_files = Vec::new();
    let mut snow_sources = Vec::new();
    for dir in snow_dirs {
        let snow_meta = LayerMeta::read(&dir.join("snow.meta.json"))?;
        let valid = snow_meta
            .valid_from
            .clone()
            .ok_or_else(|| err("snow.meta.json: no valid_from"))?;
        let date = valid
            .get(0..10)
            .ok_or_else(|| err("snow.meta.json: bad valid_from"))?;
        snow_sources.extend(snow_meta.sources);
        let srn = geotiff::read_f32(&dir.join("class_srn.tif"))?;
        let groomed = geotiff::read_f32(&dir.join("class_groomed.tif"))?;
        if !same_grid(&srn, &groomed) || srn.crs() != Crs::Lambert93 {
            return Err(err("snow class grids differ"));
        }
        let cls = |v: f32| v.is_finite().then_some(v as u8);
        let packed: Vec<u8> = srn
            .data()
            .iter()
            .zip(groomed.data())
            .map(|(a, b)| SnowGrid::pack(cls(*a), cls(*b)))
            .collect();
        let snow = SnowGrid {
            grid: Grid::from_vec(
                srn.width(),
                srn.height(),
                *srn.transform(),
                Crs::Lambert93,
                packed,
            )?,
        };
        let name = format!("snow_{date}.skr");
        let bytes = snow.write(&out.join(&name))?;
        let mut k = [0usize; 4];
        for v in snow.grid.data() {
            k[usize::from(SnowGrid::srn(*v).min(3))] += 1;
        }
        snow_files.push(json!({
            "name": name, "bytes": bytes, "source_dir": dir.display().to_string(),
            "valid_from": valid, "size_cells": [srn.width(), srn.height()], "cell_size": srn.transform().pixel_width,
            "srn_cells": {"insufficient": k[0], "limit": k[1], "sufficient": k[2], "unknown": k[3]},
        }));
    }

    // `<layer>.meta.json` (common schema): sources of the input layers.
    let mut meta = LayerMeta::new(LAYER, s.id);
    meta.tickets = vec!["C3 (#35)".into(), "C4 (#36)".into()];
    meta.validity = Some(
        "relief permanent ; zones et pistes à la date de leurs couches ; neige au jour de chaque fichier snow_<date>.skr"
            .into(),
    );
    meta.crs = Some(
        "EPSG:2154 (Lambert-93), grille 5 m de skimap-terrain ; neige : grille 25 m de skimap-snow"
            .into(),
    );
    for layer in ["terrain", "zones", "pistes"] {
        let m = LayerMeta::read(
            &layout
                .layer_dir(s, layer)?
                .join(LayerMeta::file_name(layer)),
        )?;
        // terrain: only the LiDAR HD model (the IGN slope map is a comparison only).
        let take = if layer == "terrain" {
            1
        } else {
            m.sources.len()
        };
        meta.sources.extend(m.sources.into_iter().take(take));
    }
    for src in snow_sources {
        if !meta
            .sources
            .iter()
            .any(|x| x.name == src.name && x.url == src.url)
        {
            meta.sources.push(src);
        }
    }
    let format = "SKR1 : en-tête de 72 octets little-endian (magic SKR1, type 1 = u8 / 2 = u16, 3 octets réservés, largeur u32, hauteur u32, EPSG u32, valeur absente u32, géotransformation GDAL 6 × f64), puis les cellules ligne par ligne (skimap_core::offpiste::encode_raster)";
    let describe = |name: &str| -> &'static str {
        match name {
            offpiste::FILE_ELEVATION => "altitude, décimètres (u16), 65535 = absente",
            offpiste::FILE_SLOPE => {
                "pente, floor(2 × degrés) (u8), 255 = absente ; bloquée si (q + 1) / 2 > pente maximale"
            }
            offpiste::FILE_ZONES => "classe de zones (u8), indice dans zone_classes.tsv",
            offpiste::FILE_ZONE_CLASSES => {
                "classe, lon, lat du point représentatif, nombre de cellules, identifiants des zones"
            }
            offpiste::FILE_PISTES => {
                "0 hors réseau, 1 itinéraire balisé non damé, 2 piste nordique damée"
            }
            _ => "",
        }
    };
    for (name, bytes) in &files {
        let mut f = FileMeta::new(name.clone(), describe(name));
        if name.ends_with(".skr") {
            f.format = Some(format.into());
            f.crs = Some("EPSG:2154".into());
            f.cell_size = Some(t.pixel_width);
            f.size_cells = Some([w, h]);
        }
        f.bytes = Some(*bytes);
        meta.files.push(f);
    }
    for sf in &snow_files {
        let mut f = FileMeta::new(
            sf["name"].as_str().unwrap_or_default(),
            "u8 : classe SRN (4 bits bas) | classe piste damée (4 bits hauts) ; 0 insuffisant, 1 limite, 2 suffisant, 15 inconnu",
        );
        f.format = Some(format.into());
        f.crs = Some("EPSG:2154".into());
        f.cell_size = sf["cell_size"].as_f64();
        f.bytes = sf["bytes"].as_u64();
        meta.files.push(f);
    }
    meta.valid_from = snow_files
        .iter()
        .filter_map(|f| f["valid_from"].as_str())
        .max()
        .map(str::to_owned);
    meta.stats = json!({
        "snow": snow_files,
        "grid": {"size_cells": [w, h], "cell_size": t.pixel_width, "origin": [t.origin_x, t.origin_y],
                 "cells_without_terrain": missing, "share_without_terrain": missing as f64 / n as f64,
                 "cells_slope_ge_30": steep, "share_slope_ge_30_of_valid": steep as f64 / (n - missing).max(1) as f64},
        "zones": {"zones": layer.zones.len(), "classes": grid.classes.len(),
                  "boundary_cells_decided_by_containing": near.len(), "boundary_cells_changed": near_changed,
                  "check_sample_cells": sample, "check_mismatches": mismatches.len(), "mismatch_examples": mismatches.iter().take(10).collect::<Vec<_>>()},
        "pistes": {"edges": edges.len(), "cells_marked": piste_counts[0], "cells_groomed": piste_counts[1]},
        "timings_s": {"read_geotiff": read_s, "quantise": quantise_s, "zones": zones_s, "pistes": pistes_s, "write": write_s, "total": t_all.elapsed().as_secs_f64()},
        "peak_rss_kib": proc_status("VmHWM:"),
    });
    meta.notes = vec![
        "Les rasters gardent les entrées du coût (altitude, pente, classes de zones, pistes, neige) : la pente maximale, les poids et la date sont des paramètres de la requête sur le téléphone.".into(),
        "Itinéraires balisés = graphe des pistes OSM (ski de fond, raquettes, randonnée) : approximation des itinéraires autorisés des arrêtés (annexe 5 de l'APPB Haut-Jura), non vérifiée.".into(),
        "Sans couche forêt ni exposition aux avalanches pour l'instant (crochet Overlay prévu).".into(),
    ];
    meta.write(&out)?;
    Ok(meta)
}

/// Value of a `/proc/self/status` line (Linux and Android), kB.
fn proc_status(key: &str) -> Option<u64> {
    let s = std::fs::read_to_string("/proc/self/status").ok()?;
    s.lines()
        .find(|l| l.starts_with(key))
        .and_then(|l| l.split_whitespace().nth(1))
        .and_then(|v| v.parse().ok())
}
