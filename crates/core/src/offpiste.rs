//! Off-piste routing on a cost grid (C3, #35; Pixel benchmark C4, #36).
//!
//! # Data flow
//!
//! The pipeline prepares, once per site, compact rasters on the 5 m Lambert-93 grid of
//! `skimap-terrain` (`data/out/<site>/offpiste/`, written by [`prepare::prepare`], feature
//! `pipeline`; command `prepare` of the `offpiste_bench` example):
//!
//! | File | Type | Content |
//! |---|---|---|
//! | [`FILE_ELEVATION`] | `u16` | elevation, decimetres (IGN69); [`ELEVATION_NODATA`] |
//! | [`FILE_SLOPE`] | `u8` | slope, `floor(2 × degrees)` (half degrees); [`SLOPE_NODATA`] |
//! | [`FILE_ZONES`] | `u8` | zone class: index into [`FILE_ZONE_CLASSES`] |
//! | [`FILE_ZONE_CLASSES`] | text | per class: representative point and zone identifiers |
//! | [`FILE_PISTES`] | `u8` | [`PISTE_NONE`], [`PISTE_MARKED`], [`PISTE_GROOMED`] |
//! | `snow_<date>.skr` | `u8` | 25 m snow classes of one day ([`SnowGrid`]) |
//!
//! Every raster uses the `SKR1` format of [`encode_raster`]: a 72-byte little-endian
//! header (magic, value type, size, EPSG code, no-data value, GDAL geotransform) followed
//! by the row-major cells. Reading it needs only `std`, so the phone side carries no
//! GeoTIFF decoder and no HTTP stack.
//!
//! The rasters keep the *inputs* of the cost, not a frozen cost: the slope limit and the
//! weights are request parameters ([`CostParams`]), zone restrictions depend on the date
//! ([`zone_policy`]) and the snow changes every day.
//!
//! # Zones at a date
//!
//! A zone class is one distinct set of zones containing a cell centre. The rules of
//! [`crate::zone_eval::evaluate`] depend only on that set and on the instant, so
//! [`zone_policy`] evaluates each class once, at its representative point (a cell centre
//! whose containing set was checked to be the class set when the grid was prepared):
//!
//! - `access` forbidden: the class is blocked;
//! - `leave_routes` forbidden (APPB of the Risoux from 15/12 to 30/06, Swiss districts
//!   francs): blocked off the marked itineraries; cells of the piste network
//!   ([`PISTE_MARKED`], [`PISTE_GROOMED`]) stay open;
//! - `avoid_recommended` (Biodiv'sports wildlife areas in season): cost factor
//!   [`CostParams::avoid_factor`];
//! - "forbidden in places" (rules limited to an unmapped part of the zone, such as private
//!   land crossed by pistes): not blocked, reported in [`ClassPolicy::in_places`].
//!
//! # Cost model
//!
//! The cost of a move between two of the 8 neighbouring cells is a time in seconds
//! weighted by the cell factors (a pure travel time only where every factor is 1):
//!
//! ```text
//! uphill   (dz ≥ 0): (L / v_flat + dz / v_climb) × (f_a + f_b) / 2
//! downhill (dz < 0): L / v_flat × down(|dz| / L)  × (f_a + f_b) / 2
//! ```
//!
//! with `L` the horizontal length (5 m or 7.07 m), `dz` the elevation difference,
//! `v_flat` the speed on the flat, `v_climb` the climbing rate (Naismith-type rule) and
//! `down` a piecewise linear factor of the downhill angle ([`CostParams::downhill`]):
//! nordic skis glide on gentle descents and are slow and awkward on steep ones (no metal
//! edges, snowplough), hence the uphill / downhill asymmetry. `f` is the cell factor:
//!
//! - blocked (infinite) if the slope is at least [`CostParams::max_slope_deg`] (30° by
//!   default), if elevation or slope is missing, or by the zone policy;
//! - cross-slope penalty growing linearly from 1 at [`CostParams::steep_from_deg`] to
//!   [`CostParams::steep_factor_at_max`] just below the limit;
//! - snow class ([`CostParams::snow_factor`]; infinite blocks); off-piste cells use the
//!   SRN class, piste cells the groomed-piste class;
//! - piste network: [`CostParams::groomed_factor`], [`CostParams::marked_factor`];
//! - zone avoidance, and optional overlays ([`Overlay`]: forest, avalanche exposure),
//!   the typed hook for the layers built by other tickets.
//!
//! Diagonal moves are allowed only when both orthogonal neighbours are passable, so a
//! route never cuts the corner of a blocked cell.
//!
//! # Search
//!
//! A* over a rectangular [`Window`] of the grid (by default the box of the two ends
//! enlarged by [`CostParams::window_margin_m`]). The heuristic
//! `f_min × (d_min × octile / v_flat + max(0, z_goal − z) / v_climb)`, with `f_min` the
//! smallest cell factor of the window and `d_min` the smallest move factor, is a lower
//! bound of every remaining path and is consistent, so the first time the goal is popped
//! its cost is optimal (checked against Dijkstra in the tests). Memory: 9 bytes per window
//! cell (factor `f32`, cost `f32`, state `u8`) plus the open list.

#[cfg(feature = "pipeline")]
pub mod prepare;

use std::cmp::Reverse;
use std::collections::BinaryHeap;
use std::fmt;
use std::io;
use std::path::{Path, PathBuf};
use std::time::Instant;

use skimap_common::crs::{self, Crs};
use skimap_common::geom::LonLat;
use skimap_common::grid::{GeoTransform, Grid};
use skimap_zones::chrono::{DateTime, Utc};
use skimap_zones::layer::ZoneLayer;
use skimap_zones::rules::{RuleSet, Topic};

use crate::zone_eval::{self, Verdict};

/// Elevation raster file name.
pub const FILE_ELEVATION: &str = "elevation_5m.skr";
/// Slope raster file name.
pub const FILE_SLOPE: &str = "slope_5m.skr";
/// Zone class raster file name.
pub const FILE_ZONES: &str = "zones_5m.skr";
/// Zone class table file name.
pub const FILE_ZONE_CLASSES: &str = "zone_classes.tsv";
/// Piste raster file name.
pub const FILE_PISTES: &str = "pistes_5m.skr";
/// Layer directory name under `data/out/<site>/`.
pub const LAYER: &str = "offpiste";

/// No-data value of the elevation raster.
pub const ELEVATION_NODATA: u16 = u16::MAX;
/// No-data value of the slope raster.
pub const SLOPE_NODATA: u8 = u8::MAX;
/// Piste raster: not on the piste network.
pub const PISTE_NONE: u8 = 0;
/// Piste raster: marked itinerary, not groomed (snowshoe, walking, backcountry trail).
pub const PISTE_MARKED: u8 = 1;
/// Piste raster: groomed nordic piste.
pub const PISTE_GROOMED: u8 = 2;
/// Snow raster nibble: no data.
pub const SNOW_UNKNOWN: u8 = 0x0F;

/// Elevation of a raw elevation value, metres.
pub fn elevation_m(raw: u16) -> Option<f32> {
    (raw != ELEVATION_NODATA).then_some(f32::from(raw) * 0.1)
}

/// Raw elevation value of a height in metres (`None` for no data or out of range).
pub fn elevation_raw(m: f32) -> Option<u16> {
    let v = (m * 10.0).round();
    (m.is_finite() && (0.0..f32::from(ELEVATION_NODATA)).contains(&v)).then_some(v as u16)
}

/// Raw slope value of a slope in degrees: `floor(2 × degrees)`, so the raw value `q`
/// stands for a slope in `[q / 2, q / 2 + 0.5)`.
pub fn slope_raw(deg: f32) -> u8 {
    if deg.is_finite() && deg >= 0.0 {
        ((deg * 2.0).floor() as u32).min(180) as u8
    } else {
        SLOPE_NODATA
    }
}

// ---------------------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------------------

/// Errors of the off-piste module.
#[derive(Debug)]
pub enum Error {
    /// File system error.
    Io {
        /// File.
        path: PathBuf,
        /// Cause.
        source: io::Error,
    },
    /// Malformed or inconsistent data.
    Format(String),
    /// Coordinate transformation.
    Crs(String),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Io { path, source } => write!(f, "{}: {source}", path.display()),
            Error::Format(m) => write!(f, "off-piste data: {m}"),
            Error::Crs(m) => write!(f, "coordinates: {m}"),
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Error::Io { source, .. } => Some(source),
            _ => None,
        }
    }
}

/// Result of the module.
pub type Result<T> = std::result::Result<T, Error>;

fn io_err(path: &Path, source: io::Error) -> Error {
    Error::Io {
        path: path.to_owned(),
        source,
    }
}

/// End of a route request.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Endpoint {
    /// Start.
    From,
    /// Destination.
    To,
}

/// Why no route was returned.
#[derive(Debug, Clone, PartialEq)]
pub enum RouteError {
    /// The point is outside the prepared grid.
    OutsideGrid(Endpoint),
    /// No passable cell within the snapping radius.
    NoPassableCell {
        /// Which end.
        which: Endpoint,
        /// Radius searched, metres.
        radius_m: f64,
    },
    /// The two ends are not connected inside the window.
    NoRoute {
        /// Cells expanded before giving up.
        expanded: usize,
    },
    /// Coordinate transformation.
    Crs(String),
}

impl fmt::Display for RouteError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            RouteError::OutsideGrid(w) => write!(f, "{w:?} point outside the grid"),
            RouteError::NoPassableCell { which, radius_m } => {
                write!(
                    f,
                    "no passable cell within {radius_m} m of the {which:?} point"
                )
            }
            RouteError::NoRoute { expanded } => {
                write!(f, "no route ({expanded} cells explored)")
            }
            RouteError::Crs(m) => write!(f, "coordinates: {m}"),
        }
    }
}

impl std::error::Error for RouteError {}

// ---------------------------------------------------------------------------------------
// SKR1 raster files
// ---------------------------------------------------------------------------------------

/// Magic of the raster files.
pub const MAGIC: &[u8; 4] = b"SKR1";
/// Header size of the raster files, bytes.
pub const HEADER_LEN: usize = 72;

/// Cell type of a raster file.
pub trait RasterValue: Copy + PartialEq + Send + Sync + 'static {
    /// Type code in the header.
    const CODE: u8;
    /// Bytes per cell.
    const SIZE: usize;
    /// Appends the little-endian bytes.
    fn put(self, out: &mut Vec<u8>);
    /// Reads from `SIZE` little-endian bytes.
    fn get(b: &[u8]) -> Self;
    /// As `u32` (header no-data field).
    fn to_u32(self) -> u32;
    /// From the header no-data field.
    fn from_u32(v: u32) -> Option<Self>;
    /// `self − other`, wrapping (row-delta encoding).
    fn wrapping_sub(self, other: Self) -> Self;
    /// `self + other`, wrapping (row-delta decoding).
    fn wrapping_add(self, other: Self) -> Self;
}

impl RasterValue for u8 {
    const CODE: u8 = 1;
    const SIZE: usize = 1;
    fn put(self, out: &mut Vec<u8>) {
        out.push(self);
    }
    fn get(b: &[u8]) -> Self {
        b[0]
    }
    fn to_u32(self) -> u32 {
        u32::from(self)
    }
    fn from_u32(v: u32) -> Option<Self> {
        u8::try_from(v).ok()
    }
    fn wrapping_sub(self, other: Self) -> Self {
        u8::wrapping_sub(self, other)
    }
    fn wrapping_add(self, other: Self) -> Self {
        u8::wrapping_add(self, other)
    }
}

impl RasterValue for u16 {
    const CODE: u8 = 2;
    const SIZE: usize = 2;
    fn put(self, out: &mut Vec<u8>) {
        out.extend_from_slice(&self.to_le_bytes());
    }
    fn get(b: &[u8]) -> Self {
        u16::from_le_bytes([b[0], b[1]])
    }
    fn to_u32(self) -> u32 {
        u32::from(self)
    }
    fn from_u32(v: u32) -> Option<Self> {
        u16::try_from(v).ok()
    }
    fn wrapping_sub(self, other: Self) -> Self {
        u16::wrapping_sub(self, other)
    }
    fn wrapping_add(self, other: Self) -> Self {
        u16::wrapping_add(self, other)
    }
}

/// Cell layout of a raster file (bit 7 of the type byte).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Encoding {
    /// Cells as they are, row-major, little-endian.
    Raw,
    /// Each row: first cell as is, then the wrapping difference with the cell to its left;
    /// for multi-byte cells, the row is stored byte plane by byte plane (all low bytes,
    /// then all high bytes). Lossless; the small differences of a terrain model compress
    /// about twice as well (gzip in the pack).
    RowDelta,
}

/// Flag of [`Encoding::RowDelta`] in the type byte. A reader that only knows
/// [`Encoding::Raw`] rejects such a file (unknown type) instead of misreading it.
pub const ROW_DELTA_FLAG: u8 = 0x80;

fn crs_of_epsg(epsg: u32) -> Result<Crs> {
    match epsg {
        2154 => Ok(Crs::Lambert93),
        4326 => Ok(Crs::Wgs84),
        3857 => Ok(Crs::WebMercator),
        e => Err(Error::Format(format!("unsupported EPSG:{e}"))),
    }
}

/// Encodes a raster with [`Encoding::Raw`] cells.
pub fn encode_raster<T: RasterValue>(grid: &Grid<T>, nodata: T) -> Result<Vec<u8>> {
    encode_raster_with(grid, nodata, Encoding::Raw)
}

/// Encodes a raster: header (magic `SKR1`, type byte = type code, plus
/// [`ROW_DELTA_FLAG`] for [`Encoding::RowDelta`], 3 reserved bytes, width `u32`, height
/// `u32`, EPSG `u32`, no-data `u32`, GDAL geotransform `6 × f64`), then the cells, row by
/// row, little-endian, laid out as `encoding` says.
pub fn encode_raster_with<T: RasterValue>(
    grid: &Grid<T>,
    nodata: T,
    encoding: Encoding,
) -> Result<Vec<u8>> {
    let (w, h) = (grid.width(), grid.height());
    let w32 = u32::try_from(w).map_err(|_| Error::Format("width too large".into()))?;
    let h32 = u32::try_from(h).map_err(|_| Error::Format("height too large".into()))?;
    let mut out = Vec::with_capacity(HEADER_LEN + w * h * T::SIZE);
    out.extend_from_slice(MAGIC);
    let code = match encoding {
        Encoding::Raw => T::CODE,
        Encoding::RowDelta => T::CODE | ROW_DELTA_FLAG,
    };
    out.extend_from_slice(&[code, 0, 0, 0]);
    out.extend_from_slice(&w32.to_le_bytes());
    out.extend_from_slice(&h32.to_le_bytes());
    out.extend_from_slice(&grid.crs().epsg().to_le_bytes());
    out.extend_from_slice(&nodata.to_u32().to_le_bytes());
    for v in grid.transform().to_gdal() {
        out.extend_from_slice(&v.to_le_bytes());
    }
    debug_assert_eq!(out.len(), HEADER_LEN);
    match encoding {
        Encoding::Raw => {
            for v in grid.data() {
                v.put(&mut out);
            }
        }
        Encoding::RowDelta => {
            let mut bytes = Vec::with_capacity(T::SIZE);
            // One row of planes, only when there are rows: a 4 × 10⁹ × 0 raster (valid,
            // empty) reserved 4 GB (found by fuzzing, Q3 #103).
            let mut planes = vec![Vec::with_capacity(if h > 0 { w } else { 0 }); T::SIZE];
            for row in grid.data().chunks_exact(w.max(1)) {
                let mut prev: Option<T> = None;
                for &v in row {
                    let d = prev.map_or(v, |p| v.wrapping_sub(p));
                    prev = Some(v);
                    bytes.clear();
                    d.put(&mut bytes);
                    for (k, b) in bytes.iter().enumerate() {
                        planes[k].push(*b);
                    }
                }
                for p in &mut planes {
                    out.append(p);
                }
            }
        }
    }
    Ok(out)
}

/// Decodes a raster written by [`encode_raster`]; the grid carries its no-data value.
pub fn decode_raster<T: RasterValue>(bytes: &[u8]) -> Result<Grid<T>> {
    if bytes.len() < HEADER_LEN || &bytes[0..4] != MAGIC {
        return Err(Error::Format("not an SKR1 raster".into()));
    }
    let encoding = if bytes[4] == T::CODE {
        Encoding::Raw
    } else if bytes[4] == T::CODE | ROW_DELTA_FLAG {
        Encoding::RowDelta
    } else {
        return Err(Error::Format(format!(
            "value type {} instead of {} (raw) or {} (row delta)",
            bytes[4],
            T::CODE,
            T::CODE | ROW_DELTA_FLAG
        )));
    };
    let u32_at =
        |o: usize| u32::from_le_bytes([bytes[o], bytes[o + 1], bytes[o + 2], bytes[o + 3]]);
    let f64_at = |o: usize| {
        let mut b = [0u8; 8];
        b.copy_from_slice(&bytes[o..o + 8]);
        f64::from_le_bytes(b)
    };
    let (w, h) = (u32_at(8) as usize, u32_at(12) as usize);
    let crs = crs_of_epsg(u32_at(16))?;
    let nodata = T::from_u32(u32_at(20))
        .ok_or_else(|| Error::Format("no-data value out of range".into()))?;
    let mut gt = [0.0; 6];
    for (k, v) in gt.iter_mut().enumerate() {
        *v = f64_at(24 + 8 * k);
    }
    let transform = GeoTransform::from_gdal(gt).map_err(|e| Error::Format(e.to_string()))?;
    // Both products checked: a header of 2^31 × 2^31 cells of 2 bytes overflowed the byte
    // count (found by fuzzing, Q3 #103).
    let n = w
        .checked_mul(h)
        .ok_or_else(|| Error::Format("size overflow".into()))?;
    let n_bytes = n
        .checked_mul(T::SIZE)
        .ok_or_else(|| Error::Format("size overflow".into()))?;
    let body = &bytes[HEADER_LEN..];
    if body.len() != n_bytes {
        return Err(Error::Format(format!(
            "{} data bytes for {w} x {h} cells of {} bytes",
            body.len(),
            T::SIZE
        )));
    }
    let data: Vec<T> = match encoding {
        Encoding::Raw => body.chunks_exact(T::SIZE).map(T::get).collect(),
        Encoding::RowDelta => {
            let mut data = Vec::with_capacity(n);
            let mut cell = vec![0u8; T::SIZE];
            for row in body.chunks_exact((w * T::SIZE).max(1)).take(h) {
                let mut prev: Option<T> = None;
                for c in 0..w {
                    for (k, b) in cell.iter_mut().enumerate() {
                        *b = row[k * w + c];
                    }
                    let d = T::get(&cell);
                    let v = prev.map_or(d, |p| p.wrapping_add(d));
                    data.push(v);
                    prev = Some(v);
                }
            }
            data
        }
    };
    Ok(Grid::from_vec(w, h, transform, crs, data)
        .map_err(|e| Error::Format(e.to_string()))?
        .with_nodata(nodata))
}

/// Writes a raster file with [`Encoding::RowDelta`] cells (the pack gzips them: Cervières
/// elevation 17.1 → 8.6 MB, slope 7.2 → 5.8 MB; the class rasters do not change);
/// returns its size in bytes.
pub fn write_raster<T: RasterValue>(path: &Path, grid: &Grid<T>, nodata: T) -> Result<u64> {
    write_raster_with(path, grid, nodata, Encoding::RowDelta)
}

/// Writes a raster file with the given cell layout; returns its size in bytes.
pub fn write_raster_with<T: RasterValue>(
    path: &Path,
    grid: &Grid<T>,
    nodata: T,
    encoding: Encoding,
) -> Result<u64> {
    let bytes = encode_raster_with(grid, nodata, encoding)?;
    std::fs::write(path, &bytes).map_err(|e| io_err(path, e))?;
    Ok(bytes.len() as u64)
}

/// Reads a raster file.
pub fn read_raster<T: RasterValue>(path: &Path) -> Result<Grid<T>> {
    let bytes = std::fs::read(path).map_err(|e| io_err(path, e))?;
    decode_raster(&bytes).map_err(|e| Error::Format(format!("{}: {e}", path.display())))
}

// ---------------------------------------------------------------------------------------
// Zone classes
// ---------------------------------------------------------------------------------------

/// A distinct set of zones containing cell centres.
#[derive(Debug, Clone, PartialEq)]
pub struct ZoneClass {
    /// Identifiers of the zones (`inpn:FR3801178`, `country:fr`...), sorted.
    pub zone_ids: Vec<String>,
    /// A cell centre whose containing zones are exactly [`ZoneClass::zone_ids`].
    pub representative: LonLat,
    /// Number of cells of the class in the prepared grid.
    pub cells: u64,
}

const CLASSES_HEADER: &str = "# skimap off-piste zone classes v1: class, lon, lat, cells, zone ids";

/// Text form of the class table: a header line, then one line per class:
/// `class<TAB>lon<TAB>lat<TAB>cells<TAB>id,id,...` (no ids: empty field).
pub fn encode_zone_classes(classes: &[ZoneClass]) -> Result<String> {
    let mut s = String::from(CLASSES_HEADER);
    s.push('\n');
    for (k, c) in classes.iter().enumerate() {
        if let Some(bad) = c
            .zone_ids
            .iter()
            .find(|id| id.contains([',', '\t', '\n', '\r']))
        {
            return Err(Error::Format(format!("zone id {bad:?} not encodable")));
        }
        s.push_str(&format!(
            "{k}\t{:.7}\t{:.7}\t{}\t{}\n",
            c.representative.lon,
            c.representative.lat,
            c.cells,
            c.zone_ids.join(",")
        ));
    }
    Ok(s)
}

/// Parses [`encode_zone_classes`].
pub fn parse_zone_classes(s: &str) -> Result<Vec<ZoneClass>> {
    let mut lines = s.lines();
    if lines.next() != Some(CLASSES_HEADER) {
        return Err(Error::Format("zone class table: bad header".into()));
    }
    let mut out = Vec::new();
    for (k, line) in lines.enumerate() {
        let f: Vec<&str> = line.split('\t').collect();
        let bad = || Error::Format(format!("zone class table line {}: {line:?}", k + 2));
        if f.len() != 5 || f[0].parse::<usize>().ok() != Some(k) {
            return Err(bad());
        }
        let lon: f64 = f[1].parse().map_err(|_| bad())?;
        let lat: f64 = f[2].parse().map_err(|_| bad())?;
        let cells: u64 = f[3].parse().map_err(|_| bad())?;
        let zone_ids = if f[4].is_empty() {
            Vec::new()
        } else {
            f[4].split(',').map(str::to_owned).collect()
        };
        out.push(ZoneClass {
            zone_ids,
            representative: LonLat::new(lon, lat),
            cells,
        });
    }
    if out.len() > 256 {
        return Err(Error::Format(format!(
            "{} zone classes (max 256)",
            out.len()
        )));
    }
    Ok(out)
}

// ---------------------------------------------------------------------------------------
// Prepared site grid
// ---------------------------------------------------------------------------------------

/// Static rasters of a site (all on the same 5 m grid).
#[derive(Debug, Clone)]
pub struct OffpisteGrid {
    /// Elevation, decimetres.
    pub elevation: Grid<u16>,
    /// Slope, half degrees (floor).
    pub slope: Grid<u8>,
    /// Zone class per cell.
    pub zones: Grid<u8>,
    /// Piste network per cell.
    pub pistes: Grid<u8>,
    /// Zone classes.
    pub classes: Vec<ZoneClass>,
}

impl OffpisteGrid {
    /// Assembles and checks the rasters (same shape and georeferencing, Lambert-93, every
    /// zone class index known).
    pub fn new(
        elevation: Grid<u16>,
        slope: Grid<u8>,
        zones: Grid<u8>,
        pistes: Grid<u8>,
        classes: Vec<ZoneClass>,
    ) -> Result<Self> {
        let same = |w: usize, h: usize, t: &GeoTransform, c: Crs| {
            w == elevation.width()
                && h == elevation.height()
                && t == elevation.transform()
                && c == elevation.crs()
        };
        if !same(
            slope.width(),
            slope.height(),
            slope.transform(),
            slope.crs(),
        ) || !same(
            zones.width(),
            zones.height(),
            zones.transform(),
            zones.crs(),
        ) || !same(
            pistes.width(),
            pistes.height(),
            pistes.transform(),
            pistes.crs(),
        ) {
            return Err(Error::Format("rasters do not share the same grid".into()));
        }
        if elevation.crs() != Crs::Lambert93 {
            return Err(Error::Format("grid is not Lambert-93".into()));
        }
        let t = elevation.transform();
        if (t.pixel_width + t.pixel_height).abs() > 1e-9 {
            return Err(Error::Format("cells are not square".into()));
        }
        if let Some(bad) = zones
            .data()
            .iter()
            .find(|c| usize::from(**c) >= classes.len())
        {
            return Err(Error::Format(format!(
                "zone class {bad} not in the table ({} classes)",
                classes.len()
            )));
        }
        Ok(Self {
            elevation,
            slope,
            zones,
            pistes,
            classes,
        })
    }

    /// Loads the files of a prepared directory (`data/out/<site>/offpiste`).
    pub fn load(dir: &Path) -> Result<Self> {
        let classes_path = dir.join(FILE_ZONE_CLASSES);
        let text = std::fs::read_to_string(&classes_path).map_err(|e| io_err(&classes_path, e))?;
        Self::new(
            read_raster(&dir.join(FILE_ELEVATION))?,
            read_raster(&dir.join(FILE_SLOPE))?,
            read_raster(&dir.join(FILE_ZONES))?,
            read_raster(&dir.join(FILE_PISTES))?,
            parse_zone_classes(&text)?,
        )
    }

    /// Writes the files to a directory; returns (file name, bytes) per file.
    pub fn write(&self, dir: &Path) -> Result<Vec<(String, u64)>> {
        std::fs::create_dir_all(dir).map_err(|e| io_err(dir, e))?;
        let mut out = vec![
            (
                FILE_ELEVATION.to_owned(),
                write_raster(&dir.join(FILE_ELEVATION), &self.elevation, ELEVATION_NODATA)?,
            ),
            (
                FILE_SLOPE.to_owned(),
                write_raster(&dir.join(FILE_SLOPE), &self.slope, SLOPE_NODATA)?,
            ),
            (
                FILE_ZONES.to_owned(),
                write_raster(&dir.join(FILE_ZONES), &self.zones, u8::MAX)?,
            ),
            (
                FILE_PISTES.to_owned(),
                write_raster(&dir.join(FILE_PISTES), &self.pistes, u8::MAX)?,
            ),
        ];
        let text = encode_zone_classes(&self.classes)?;
        let p = dir.join(FILE_ZONE_CLASSES);
        std::fs::write(&p, &text).map_err(|e| io_err(&p, e))?;
        out.push((FILE_ZONE_CLASSES.to_owned(), text.len() as u64));
        Ok(out)
    }

    /// Number of columns.
    pub fn width(&self) -> usize {
        self.elevation.width()
    }

    /// Number of rows.
    pub fn height(&self) -> usize {
        self.elevation.height()
    }

    /// Cell size, metres.
    pub fn cell_size(&self) -> f64 {
        self.elevation.transform().pixel_width
    }

    /// Cell containing a WGS84 position.
    pub fn cell_of(&self, p: LonLat) -> Result<Option<(usize, usize)>> {
        let (x, y) = crs::to_lambert93(p).map_err(|e| Error::Crs(e.to_string()))?;
        Ok(self.elevation.cell_at(x, y))
    }

    /// Lambert-93 centre of a cell.
    pub fn center_l93(&self, col: usize, row: usize) -> (f64, f64) {
        self.elevation.transform().cell_center(col, row)
    }

    /// WGS84 centre of a cell.
    pub fn center(&self, col: usize, row: usize) -> Result<LonLat> {
        let (x, y) = self.center_l93(col, row);
        crs::from_lambert93(x, y).map_err(|e| Error::Crs(e.to_string()))
    }
}

/// Snow classes of one day on the 25 m snow grid: low nibble = SRN class (0 insufficient,
/// 1 limit, 2 sufficient), high nibble = groomed-piste class; [`SNOW_UNKNOWN`] = no data.
#[derive(Debug, Clone)]
pub struct SnowGrid {
    /// Packed classes.
    pub grid: Grid<u8>,
}

impl SnowGrid {
    /// Packs one cell.
    pub fn pack(srn: Option<u8>, groomed: Option<u8>) -> u8 {
        let n = |c: Option<u8>| c.filter(|v| *v <= 2).unwrap_or(SNOW_UNKNOWN);
        n(srn) | (n(groomed) << 4)
    }

    /// SRN class nibble of a packed value.
    pub fn srn(v: u8) -> u8 {
        v & 0x0F
    }

    /// Groomed class nibble of a packed value.
    pub fn groomed(v: u8) -> u8 {
        v >> 4
    }

    /// Reads a snow file.
    pub fn load(path: &Path) -> Result<Self> {
        let grid: Grid<u8> = read_raster(path)?;
        if grid.crs() != Crs::Lambert93 {
            return Err(Error::Format("snow grid is not Lambert-93".into()));
        }
        Ok(Self { grid })
    }

    /// Writes a snow file.
    pub fn write(&self, path: &Path) -> Result<u64> {
        write_raster(path, &self.grid, SnowGrid::pack(None, None))
    }

    /// Packed value at a Lambert-93 point (unknown outside the grid).
    pub fn at(&self, x: f64, y: f64) -> u8 {
        self.grid
            .value_at(x, y)
            .copied()
            .unwrap_or(SnowGrid::pack(None, None))
    }
}

/// Avalanche exposure classes in the prepared directory (from the `avalanche-exposure`
/// layer, CLPA fusion included): 1 « terrain peu exposé (modèle) », 2 « exposé »,
/// 3 « très exposé », 255 no data.
pub const FILE_EXPOSURE: &str = "exposure_5m.skr";
/// No-data code of [`FILE_EXPOSURE`].
pub const EXPOSURE_NODATA: u8 = 255;

/// Cost factors of the avalanche exposure overlay, by class. Defaults: « très exposé »
/// blocked (the user may allow it, with a factor), « exposé » ×3, « terrain peu exposé
/// (modèle) » and no data ×1. « Peu exposé » is a model output, never « sûr ».
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ExposureFactors {
    /// Class 1, « terrain peu exposé (modèle) ».
    pub low: f32,
    /// Class 2, « exposé ».
    pub exposed: f32,
    /// Class 3, « très exposé » (`f32::INFINITY` blocks).
    pub high: f32,
    /// No data (outside the modelled area).
    pub nodata: f32,
}

impl Default for ExposureFactors {
    fn default() -> Self {
        Self {
            low: 1.0,
            exposed: 3.0,
            high: f32::INFINITY,
            nodata: 1.0,
        }
    }
}

impl ExposureFactors {
    /// The overlay of the router for an exposure grid.
    pub fn overlay(&self, grid: Grid<u8>) -> Overlay {
        let mut factor = [1.0f32; 256];
        factor[1] = self.low;
        factor[2] = self.exposed;
        factor[3] = self.high;
        factor[usize::from(EXPOSURE_NODATA)] = self.nodata;
        Overlay {
            kind: OverlayKind::AvalancheExposure,
            grid,
            factor,
        }
    }
}

/// The avalanche exposure overlay of a prepared directory ([`FILE_EXPOSURE`]), `None` if
/// the site has no exposure layer.
pub fn load_exposure_overlay(dir: &Path, factors: &ExposureFactors) -> Result<Option<Overlay>> {
    let p = dir.join(FILE_EXPOSURE);
    if !p.is_file() {
        return Ok(None);
    }
    Ok(Some(factors.overlay(read_raster(&p)?)))
}

/// Kind of an optional cost overlay.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OverlayKind {
    /// Forest cover (density classes).
    Forest,
    /// Avalanche exposure (ATES-like classes, P16 to P18).
    AvalancheExposure,
}

/// Optional cost layer: a Lambert-93 `u8` class raster of any resolution, sampled at the
/// cell centre; the cell factor is multiplied by `factor[class]` (`f32::INFINITY`
/// blocks). Cells outside the overlay get the factor 1.
#[derive(Debug, Clone)]
pub struct Overlay {
    /// What it represents.
    pub kind: OverlayKind,
    /// Classes.
    pub grid: Grid<u8>,
    /// Factor per class.
    pub factor: [f32; 256],
}

// ---------------------------------------------------------------------------------------
// Zone policy at a date
// ---------------------------------------------------------------------------------------

/// Restrictions of one zone class at an instant.
#[derive(Debug, Clone, PartialEq)]
pub struct ClassPolicy {
    /// Entering is forbidden (blocked even on pistes).
    pub blocked: bool,
    /// Leaving the marked itineraries is forbidden (blocked off the piste network).
    pub offpiste_blocked: bool,
    /// Visit to avoid (wildlife).
    pub avoid: bool,
    /// Whether the position is inside a jurisdiction of the zone layer.
    pub covered: bool,
    /// Active rules behind `blocked`, `offpiste_blocked` and `avoid`.
    pub rules: Vec<String>,
    /// Active "forbidden in places" rules of the access and leave-routes topics.
    pub in_places: Vec<String>,
}

impl ClassPolicy {
    /// No restriction.
    pub fn open() -> Self {
        Self {
            blocked: false,
            offpiste_blocked: false,
            avoid: false,
            covered: true,
            rules: Vec::new(),
            in_places: Vec::new(),
        }
    }
}

/// Restrictions of every zone class at an instant.
#[derive(Debug, Clone, PartialEq)]
pub struct ZonePolicy {
    /// Instant evaluated.
    pub at: DateTime<Utc>,
    /// Per class, same order as [`OffpisteGrid::classes`].
    pub classes: Vec<ClassPolicy>,
}

impl ZonePolicy {
    /// No restriction for `n` classes.
    pub fn open(n: usize, at: DateTime<Utc>) -> Self {
        Self {
            at,
            classes: vec![ClassPolicy::open(); n],
        }
    }
}

/// Evaluates every zone class at `at` with [`zone_eval::evaluate`] at its representative
/// point.
pub fn zone_policy(
    classes: &[ZoneClass],
    layer: &ZoneLayer,
    rules: &RuleSet,
    at: DateTime<Utc>,
) -> ZonePolicy {
    let classes = classes
        .iter()
        .map(|c| {
            let ev = zone_eval::evaluate(layer, rules, c.representative, at);
            let access = ev.verdict(Topic::Access);
            let leave = ev.verdict(Topic::LeaveRoutes);
            let mut p = ClassPolicy::open();
            p.covered = ev.covered;
            p.blocked = access.verdict == Verdict::Forbidden;
            p.offpiste_blocked = p.blocked || leave.verdict == Verdict::Forbidden;
            p.avoid = [access, leave]
                .iter()
                .any(|v| v.verdict == Verdict::AvoidRecommended);
            for v in [access, leave] {
                match v.verdict {
                    Verdict::Forbidden | Verdict::AvoidRecommended => {
                        p.rules.extend(v.rules.iter().cloned());
                    }
                    Verdict::ForbiddenInPlaces => p.in_places.extend(v.rules.iter().cloned()),
                    _ => {}
                }
            }
            p
        })
        .collect();
    ZonePolicy { at, classes }
}

// ---------------------------------------------------------------------------------------
// Cost parameters
// ---------------------------------------------------------------------------------------

/// Cost parameters of a request.
#[derive(Debug, Clone, PartialEq)]
pub struct CostParams {
    /// Cells whose slope is at least this are blocked, degrees. Exact for multiples of
    /// 0.5° (the slope is stored in half degrees); otherwise up to 0.5° conservative.
    pub max_slope_deg: f32,
    /// Speed on the flat, m/s (horizontal).
    pub flat_speed_mps: f32,
    /// Climbing rate, vertical m/s.
    pub climb_rate_mps: f32,
    /// Downhill time factor at downhill angles (degrees, factor), increasing angles,
    /// linear in between (in grade), constant beyond the last knot.
    pub downhill: Vec<(f32, f32)>,
    /// Cross-slope penalty starts at this slope, degrees.
    pub steep_from_deg: f32,
    /// Cross-slope factor just below [`CostParams::max_slope_deg`].
    pub steep_factor_at_max: f32,
    /// Factor per snow class: insufficient, limit, sufficient (`f32::INFINITY` blocks).
    pub snow_factor: [f32; 3],
    /// Factor where the snow is unknown (no snow layer, outside the snow map).
    pub snow_unknown_factor: f32,
    /// Factor on groomed nordic pistes.
    pub groomed_factor: f32,
    /// Factor on marked, ungroomed itineraries.
    pub marked_factor: f32,
    /// Factor in zones whose visit should be avoided.
    pub avoid_factor: f32,
    /// Ends are moved to the nearest passable cell within this radius, metres.
    pub snap_radius_m: f64,
    /// Margin of the search window around the two ends, metres.
    pub window_margin_m: f64,
}

impl Default for CostParams {
    fn default() -> Self {
        Self {
            max_slope_deg: 30.0,
            flat_speed_mps: 1.1,
            climb_rate_mps: 0.1,
            downhill: vec![
                (0.0, 1.0),
                (8.0, 0.6),
                (15.0, 1.0),
                (25.0, 2.5),
                (35.0, 5.0),
            ],
            steep_from_deg: 20.0,
            steep_factor_at_max: 2.0,
            snow_factor: [6.0, 1.5, 1.0],
            snow_unknown_factor: 2.0,
            groomed_factor: 0.6,
            marked_factor: 0.9,
            avoid_factor: 1.5,
            snap_radius_m: 250.0,
            window_margin_m: 2000.0,
        }
    }
}

impl CostParams {
    /// Whether a raw slope value is blocked.
    pub fn slope_blocked(&self, raw: u8) -> bool {
        raw == SLOPE_NODATA || (f32::from(raw) + 1.0) * 0.5 > self.max_slope_deg
    }

    /// Cell factor of a raw slope value (infinite when blocked).
    pub fn slope_factor(&self, raw: u8) -> f32 {
        if self.slope_blocked(raw) {
            return f32::INFINITY;
        }
        let s = f32::from(raw) * 0.5;
        if s <= self.steep_from_deg || self.max_slope_deg <= self.steep_from_deg {
            1.0
        } else {
            let t = ((s - self.steep_from_deg) / (self.max_slope_deg - self.steep_from_deg))
                .clamp(0.0, 1.0);
            1.0 + t * (self.steep_factor_at_max - 1.0)
        }
    }

    fn down_knots(&self) -> Vec<(f32, f32)> {
        self.downhill
            .iter()
            .map(|(deg, f)| (deg.to_radians().tan(), *f))
            .collect()
    }
}

fn down_factor(knots: &[(f32, f32)], grade: f32) -> f32 {
    match knots {
        [] => 1.0,
        [(g0, f0), ..] if grade <= *g0 => *f0,
        _ => {
            for w in knots.windows(2) {
                let ((g0, f0), (g1, f1)) = (w[0], w[1]);
                if grade <= g1 {
                    return f0 + (f1 - f0) * (grade - g0) / (g1 - g0);
                }
            }
            knots[knots.len() - 1].1
        }
    }
}

// ---------------------------------------------------------------------------------------
// Search
// ---------------------------------------------------------------------------------------

/// Rectangle of grid cells.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Window {
    /// First column.
    pub col0: usize,
    /// First row.
    pub row0: usize,
    /// Columns.
    pub width: usize,
    /// Rows.
    pub height: usize,
}

impl Window {
    /// The whole grid.
    pub fn full(g: &OffpisteGrid) -> Self {
        Self {
            col0: 0,
            row0: 0,
            width: g.width(),
            height: g.height(),
        }
    }

    /// Box of two cells enlarged by `margin` cells, clipped to the grid.
    pub fn around(g: &OffpisteGrid, a: (usize, usize), b: (usize, usize), margin: usize) -> Self {
        let c0 = a.0.min(b.0).saturating_sub(margin);
        let r0 = a.1.min(b.1).saturating_sub(margin);
        let c1 = (a.0.max(b.0) + margin + 1).min(g.width());
        let r1 = (a.1.max(b.1) + margin + 1).min(g.height());
        Self {
            col0: c0,
            row0: r0,
            width: c1 - c0,
            height: r1 - r0,
        }
    }

    /// Cells.
    pub fn cells(&self) -> usize {
        self.width * self.height
    }

    /// Whether a grid cell is inside.
    pub fn contains(&self, c: (usize, usize)) -> bool {
        c.0 >= self.col0
            && c.1 >= self.row0
            && c.0 < self.col0 + self.width
            && c.1 < self.row0 + self.height
    }
}

/// Counters and timings of a search.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct SearchStats {
    /// Cells of the window.
    pub window_cells: usize,
    /// Passable cells of the window.
    pub passable_cells: usize,
    /// Cells expanded (closed).
    pub expanded: usize,
    /// Pushes on the open list.
    pub pushed: usize,
    /// Largest open list length.
    pub max_open: usize,
    /// Bytes of the per-cell arrays and of the largest open list.
    pub bytes: usize,
    /// Time to build the cell factors, s.
    pub prepare_s: f64,
    /// Time of the search, s.
    pub search_s: f64,
}

/// A route on the grid.
#[derive(Debug, Clone, PartialEq)]
pub struct Route {
    /// Grid cells (column, row), from start to destination.
    pub cells: Vec<(usize, usize)>,
    /// WGS84 cell centres.
    pub points: Vec<LonLat>,
    /// Piste raster value of each cell.
    pub piste: Vec<u8>,
    /// Optimal cost, s: travel time of the model multiplied by the cell factors. It is a
    /// travel time only on sufficient snow, off the pistes, outside avoided zones and below
    /// the cross-slope penalty; the estimated time of a route is the job of C5 (`eta`).
    pub cost_s: f64,
    /// Horizontal length, m.
    pub length_m: f64,
    /// Cumulated ascent, m.
    pub ascent_m: f64,
    /// Cumulated descent, m.
    pub descent_m: f64,
    /// Largest stored slope along the route (lower bound of the half-degree bin), degrees.
    pub max_slope_deg: f32,
    /// Distance from the requested start to the start cell centre, m.
    pub from_snap_m: f64,
    /// Distance from the requested destination to the last cell centre, m.
    pub to_snap_m: f64,
    /// Zone classes crossed (sorted, unique).
    pub zone_classes: Vec<u8>,
    /// Search counters.
    pub stats: SearchStats,
}

/// Search options (tests and benchmark).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Heuristic {
    /// A* with the admissible heuristic of the module documentation.
    AStar,
    /// No heuristic (Dijkstra).
    Dijkstra,
}

const DIRS: [(i32, i32); 8] = [
    (1, 0),
    (-1, 0),
    (0, 1),
    (0, -1),
    (1, 1),
    (1, -1),
    (-1, 1),
    (-1, -1),
];
const STATE_NEW: u8 = 0x0F;
const STATE_START: u8 = 8;
const CLOSED: u8 = 0x80;

/// Router over a prepared grid with its daily snow, overlays and zone policy.
#[derive(Debug, Clone, Copy)]
pub struct Router<'a> {
    /// Static rasters.
    pub grid: &'a OffpisteGrid,
    /// Snow of the day, if any.
    pub snow: Option<&'a SnowGrid>,
    /// Optional overlays.
    pub overlays: &'a [Overlay],
    /// Zone restrictions at the date.
    pub policy: &'a ZonePolicy,
    /// Parameters.
    pub params: &'a CostParams,
}

/// Cell factors of a window.
struct Field {
    win: Window,
    factor: Vec<f32>,
    fmin: f32,
    passable: usize,
}

impl Router<'_> {
    /// Passable cells of a window, row-major (for no-go overlays and connectivity).
    pub fn passable(&self, win: Window) -> Vec<bool> {
        self.field(win)
            .factor
            .iter()
            .map(|f| f.is_finite())
            .collect()
    }

    /// Factor of one cell (infinite when blocked).
    pub fn cell_factor(&self, col: usize, row: usize) -> f32 {
        let w = Window {
            col0: col,
            row0: row,
            width: 1,
            height: 1,
        };
        self.field(w).factor[0]
    }

    fn field(&self, win: Window) -> Field {
        let g = self.grid;
        let p = self.params;
        let gw = g.width();
        let mut slope_lut = [0f32; 256];
        for (q, v) in slope_lut.iter_mut().enumerate() {
            *v = p.slope_factor(q as u8);
        }
        let snow_lut = |nibble: u8| -> f32 {
            match nibble {
                0..=2 => p.snow_factor[usize::from(nibble)],
                _ => p.snow_unknown_factor,
            }
        };
        let t = g.elevation.transform();
        // Snow and overlay cell of every window column / row.
        let xs: Vec<f64> = (0..win.width)
            .map(|c| t.cell_center(win.col0 + c, 0).0)
            .collect();
        let ys: Vec<f64> = (0..win.height)
            .map(|r| t.cell_center(0, win.row0 + r).1)
            .collect();
        let lookup = |grid: &Grid<u8>| -> (Vec<Option<usize>>, Vec<Option<usize>>) {
            let gt = grid.transform();
            let cols = xs
                .iter()
                .map(|x| {
                    let c = ((x - gt.origin_x) / gt.pixel_width).floor();
                    (c >= 0.0 && (c as usize) < grid.width()).then_some(c as usize)
                })
                .collect();
            let rows = ys
                .iter()
                .map(|y| {
                    let r = ((y - gt.origin_y) / gt.pixel_height).floor();
                    (r >= 0.0 && (r as usize) < grid.height()).then_some(r as usize)
                })
                .collect();
            (cols, rows)
        };
        let snow_idx = self.snow.map(|s| (lookup(&s.grid), s));
        let overlay_idx: Vec<_> = self.overlays.iter().map(|o| (lookup(&o.grid), o)).collect();
        let policy: Vec<(bool, bool, f32)> = self
            .policy
            .classes
            .iter()
            .map(|c| {
                (
                    c.blocked,
                    c.offpiste_blocked,
                    if c.avoid { p.avoid_factor } else { 1.0 },
                )
            })
            .collect();

        let mut factor = vec![f32::INFINITY; win.cells()];
        let mut fmin = f32::INFINITY;
        let mut passable = 0;
        for r in 0..win.height {
            let grow = (win.row0 + r) * gw;
            for c in 0..win.width {
                let gi = grow + win.col0 + c;
                if g.elevation.data()[gi] == ELEVATION_NODATA {
                    continue;
                }
                let mut f = slope_lut[usize::from(g.slope.data()[gi])];
                if !f.is_finite() {
                    continue;
                }
                let piste = g.pistes.data()[gi];
                let Some(&(blocked, off_blocked, avoid)) =
                    policy.get(usize::from(g.zones.data()[gi]))
                else {
                    continue;
                };
                if blocked || (off_blocked && piste == PISTE_NONE) {
                    continue;
                }
                f *= avoid;
                f *= match piste {
                    PISTE_GROOMED => p.groomed_factor,
                    PISTE_MARKED => p.marked_factor,
                    _ => 1.0,
                };
                f *= match &snow_idx {
                    Some(((cols, rows), s)) => match (cols[c], rows[r]) {
                        (Some(sc), Some(sr)) => {
                            let v = s.grid.data()[sr * s.grid.width() + sc];
                            snow_lut(if piste == PISTE_NONE {
                                SnowGrid::srn(v)
                            } else {
                                SnowGrid::groomed(v)
                            })
                        }
                        _ => p.snow_unknown_factor,
                    },
                    None => p.snow_unknown_factor,
                };
                for ((cols, rows), o) in &overlay_idx {
                    if let (Some(oc), Some(or)) = (cols[c], rows[r]) {
                        f *= o.factor[usize::from(o.grid.data()[or * o.grid.width() + oc])];
                    }
                }
                if f.is_finite() && f > 0.0 {
                    factor[r * win.width + c] = f;
                    fmin = fmin.min(f);
                    passable += 1;
                }
            }
        }
        Field {
            win,
            factor,
            fmin,
            passable,
        }
    }

    /// Nearest passable cell to a point within the snapping radius, with its distance.
    pub fn snap(
        &self,
        p: LonLat,
        which: Endpoint,
    ) -> std::result::Result<((usize, usize), f64), RouteError> {
        let g = self.grid;
        let (x, y) = crs::to_lambert93(p).map_err(|e| RouteError::Crs(e.to_string()))?;
        let (c0, r0) = g
            .elevation
            .cell_at(x, y)
            .ok_or(RouteError::OutsideGrid(which))?;
        let cell = g.cell_size();
        let rad = (self.params.snap_radius_m / cell).ceil() as usize;
        let win = Window::around(g, (c0, r0), (c0, r0), rad);
        let field = self.field(win);
        let mut best: Option<((usize, usize), f64)> = None;
        for r in 0..win.height {
            for c in 0..win.width {
                if !field.factor[r * win.width + c].is_finite() {
                    continue;
                }
                let (gc, gr) = (win.col0 + c, win.row0 + r);
                let (cx, cy) = g.center_l93(gc, gr);
                let d = ((cx - x).powi(2) + (cy - y).powi(2)).sqrt();
                if d <= self.params.snap_radius_m && best.is_none_or(|b| d < b.1) {
                    best = Some(((gc, gr), d));
                }
            }
        }
        best.ok_or(RouteError::NoPassableCell {
            which,
            radius_m: self.params.snap_radius_m,
        })
    }

    /// Route between two WGS84 positions: ends snapped, window around them.
    pub fn route(&self, from: LonLat, to: LonLat) -> std::result::Result<Route, RouteError> {
        let (a, da) = self.snap(from, Endpoint::From)?;
        let (b, db) = self.snap(to, Endpoint::To)?;
        let margin = (self.params.window_margin_m / self.grid.cell_size()).ceil() as usize;
        let win = Window::around(self.grid, a, b, margin);
        let mut r = self.route_cells(win, a, b, Heuristic::AStar)?;
        r.from_snap_m = da;
        r.to_snap_m = db;
        Ok(r)
    }

    /// Route between two cells inside a window.
    pub fn route_cells(
        &self,
        win: Window,
        start: (usize, usize),
        goal: (usize, usize),
        heuristic: Heuristic,
    ) -> std::result::Result<Route, RouteError> {
        if !win.contains(start) {
            return Err(RouteError::OutsideGrid(Endpoint::From));
        }
        if !win.contains(goal) {
            return Err(RouteError::OutsideGrid(Endpoint::To));
        }
        let t0 = Instant::now();
        let field = self.field(win);
        let prepare_s = t0.elapsed().as_secs_f64();
        let (s, gl) = (
            (start.1 - win.row0) * win.width + start.0 - win.col0,
            (goal.1 - win.row0) * win.width + goal.0 - win.col0,
        );
        if !field.factor[s].is_finite() {
            return Err(RouteError::NoPassableCell {
                which: Endpoint::From,
                radius_m: 0.0,
            });
        }
        if !field.factor[gl].is_finite() {
            return Err(RouteError::NoPassableCell {
                which: Endpoint::To,
                radius_m: 0.0,
            });
        }
        let t1 = Instant::now();
        let mut out = self.search(&field, s, Some(gl), heuristic);
        let search_s = t1.elapsed().as_secs_f64();
        out.stats.prepare_s = prepare_s;
        out.stats.search_s = search_s;
        let Some(cost) = out.goal_cost else {
            return Err(RouteError::NoRoute {
                expanded: out.stats.expanded,
            });
        };
        // Walk the parents back from the goal.
        let mut local = vec![gl];
        let mut cur = gl;
        while out.state[cur] & 0x0F != STATE_START {
            let (dc, dr) = DIRS[usize::from(out.state[cur] & 0x0F)];
            let c = (cur % win.width) as i64 - i64::from(dc);
            let r = (cur / win.width) as i64 - i64::from(dr);
            cur = r as usize * win.width + c as usize;
            local.push(cur);
        }
        local.reverse();
        self.make_route(win, &local, cost, out.stats)
    }

    /// Explores every cell reachable from `start` in the window (no destination): the
    /// worst case of a request whose destination is unreachable.
    pub fn explore(&self, win: Window, start: (usize, usize)) -> SearchStats {
        let t0 = Instant::now();
        let field = self.field(win);
        let prepare_s = t0.elapsed().as_secs_f64();
        let s = (start.1 - win.row0) * win.width + start.0 - win.col0;
        let t1 = Instant::now();
        let mut stats = if field.factor[s].is_finite() {
            self.search(&field, s, None, Heuristic::Dijkstra).stats
        } else {
            SearchStats::default()
        };
        stats.prepare_s = prepare_s;
        stats.search_s = t1.elapsed().as_secs_f64();
        stats.window_cells = win.cells();
        stats.passable_cells = field.passable;
        stats
    }

    fn search(
        &self,
        field: &Field,
        start: usize,
        goal: Option<usize>,
        heuristic: Heuristic,
    ) -> SearchOut {
        let g = self.grid;
        let p = self.params;
        let win = field.win;
        let (w, h) = (win.width, win.height);
        let gw = g.width();
        let elev = g.elevation.data();
        let cell = g.cell_size() as f32;
        let inv_v = 1.0 / p.flat_speed_mps;
        let inv_climb = 1.0 / p.climb_rate_mps;
        let knots = p.down_knots();
        let dmin = knots.iter().map(|k| k.1).fold(1.0f32, f32::min).max(0.0);
        let gidx = |i: usize| (win.row0 + i / w) * gw + win.col0 + i % w;
        // Raw elevations (decimetres): differences are taken on the integers, then scaled,
        // so that a 0.1 m step keeps its full precision (in f32, 2300 m has an ulp of
        // 2.4e-4 m; the difference of two such values lost up to 0.24 % of a 0.1 m step,
        // amplified by the downhill factor: found by the property test against an f64
        // Dijkstra, Q3 #103).
        let zr = |i: usize| i32::from(elev[gidx(i)]);
        let dz_m = |from: i32, to: i32| (to - from) as f32 * 0.1;

        let (h_lin, h_climb, goal_cr, z_goal) = match (heuristic, goal) {
            (Heuristic::AStar, Some(gl)) => (
                field.fmin * dmin * inv_v * cell,
                field.fmin * inv_climb,
                ((gl % w) as i64, (gl / w) as i64),
                zr(gl),
            ),
            _ => (0.0, 0.0, (0, 0), 0),
        };
        let hfun = |i: usize| -> f32 {
            if h_lin == 0.0 && h_climb == 0.0 {
                return 0.0;
            }
            let dx = ((i % w) as i64 - goal_cr.0).unsigned_abs() as f32;
            let dy = ((i / w) as i64 - goal_cr.1).unsigned_abs() as f32;
            let (big, small) = if dx > dy { (dx, dy) } else { (dy, dx) };
            let oct = big + (std::f32::consts::SQRT_2 - 1.0) * small;
            h_lin * oct + h_climb * dz_m(zr(i), z_goal).max(0.0)
        };

        let n = w * h;
        let mut cost = vec![f32::INFINITY; n];
        let mut state = vec![STATE_NEW; n];
        let mut open: BinaryHeap<Reverse<u64>> = BinaryHeap::new();
        let key = |f: f32, i: usize| (u64::from(f.to_bits()) << 32) | i as u64;
        cost[start] = 0.0;
        state[start] = STATE_START;
        open.push(Reverse(key(hfun(start), start)));
        let mut stats = SearchStats {
            window_cells: n,
            passable_cells: field.passable,
            pushed: 1,
            ..SearchStats::default()
        };
        let lens = [
            cell,
            cell,
            cell,
            cell,
            cell * std::f32::consts::SQRT_2,
            cell * std::f32::consts::SQRT_2,
            cell * std::f32::consts::SQRT_2,
            cell * std::f32::consts::SQRT_2,
        ];
        let mut goal_cost = None;
        while let Some(Reverse(k)) = open.pop() {
            let i = (k & 0xFFFF_FFFF) as usize;
            if state[i] & CLOSED != 0 {
                continue;
            }
            state[i] |= CLOSED;
            stats.expanded += 1;
            if Some(i) == goal {
                goal_cost = Some(f64::from(cost[i]));
                break;
            }
            let (ci, ri) = ((i % w) as i64, (i / w) as i64);
            let fi = field.factor[i];
            let zi = zr(i);
            let gi = cost[i];
            for (d, &(dc, dr)) in DIRS.iter().enumerate() {
                let (cj, rj) = (ci + i64::from(dc), ri + i64::from(dr));
                if cj < 0 || rj < 0 || cj >= w as i64 || rj >= h as i64 {
                    continue;
                }
                let j = rj as usize * w + cj as usize;
                let fj = field.factor[j];
                if !fj.is_finite() || state[j] & CLOSED != 0 {
                    continue;
                }
                if d >= 4 {
                    let a = ri as usize * w + cj as usize;
                    let b = rj as usize * w + ci as usize;
                    if !field.factor[a].is_finite() || !field.factor[b].is_finite() {
                        continue;
                    }
                }
                let len = lens[d];
                let dz = dz_m(zi, zr(j));
                let pace = if dz >= 0.0 {
                    len * inv_v + dz * inv_climb
                } else {
                    len * inv_v * down_factor(&knots, -dz / len)
                };
                let nc = gi + pace * 0.5 * (fi + fj);
                if nc < cost[j] {
                    cost[j] = nc;
                    state[j] = d as u8;
                    open.push(Reverse(key(nc + hfun(j), j)));
                    stats.pushed += 1;
                }
            }
            stats.max_open = stats.max_open.max(open.len());
        }
        stats.bytes = n * (4 + 4 + 1) + stats.max_open * 8;
        SearchOut {
            state,
            goal_cost,
            stats,
        }
    }

    fn make_route(
        &self,
        win: Window,
        local: &[usize],
        cost: f64,
        stats: SearchStats,
    ) -> std::result::Result<Route, RouteError> {
        let g = self.grid;
        let cells: Vec<(usize, usize)> = local
            .iter()
            .map(|i| (win.col0 + i % win.width, win.row0 + i / win.width))
            .collect();
        let mut pts: Vec<(f64, f64)> = cells.iter().map(|&(c, r)| g.center_l93(c, r)).collect();
        let mut length = 0.0;
        let (mut up, mut down) = (0.0, 0.0);
        for k in 1..cells.len() {
            let (a, b) = (pts[k - 1], pts[k]);
            length += ((a.0 - b.0).powi(2) + (a.1 - b.1).powi(2)).sqrt();
            let za = f64::from(
                *g.elevation
                    .get(cells[k - 1].0, cells[k - 1].1)
                    .unwrap_or(&0),
            );
            let zb = f64::from(*g.elevation.get(cells[k].0, cells[k].1).unwrap_or(&0));
            let dz = (zb - za) * 0.1;
            if dz > 0.0 {
                up += dz;
            } else {
                down -= dz;
            }
        }
        let piste: Vec<u8> = cells
            .iter()
            .map(|&(c, r)| *g.pistes.get(c, r).unwrap_or(&PISTE_NONE))
            .collect();
        let max_slope = cells
            .iter()
            .map(|&(c, r)| f32::from(*g.slope.get(c, r).unwrap_or(&0)) * 0.5)
            .fold(0.0, f32::max);
        let mut zone_classes: Vec<u8> = cells
            .iter()
            .map(|&(c, r)| *g.zones.get(c, r).unwrap_or(&0))
            .collect();
        zone_classes.sort_unstable();
        zone_classes.dedup();
        crs::transform_points(Crs::Lambert93, Crs::Wgs84, &mut pts)
            .map_err(|e| RouteError::Crs(e.to_string()))?;
        Ok(Route {
            cells,
            points: pts.into_iter().map(|(x, y)| LonLat::new(x, y)).collect(),
            piste,
            cost_s: cost,
            length_m: length,
            ascent_m: up,
            descent_m: down,
            max_slope_deg: max_slope,
            from_snap_m: 0.0,
            to_snap_m: 0.0,
            zone_classes,
            stats,
        })
    }
}

struct SearchOut {
    state: Vec<u8>,
    goal_cost: Option<f64>,
    stats: SearchStats,
}

#[cfg(test)]
mod tests {
    use super::*;
    use skimap_common::geom::BBox;
    use skimap_zones::geo::MultiPolygon;
    use skimap_zones::geom::bbox_polygon;
    use skimap_zones::layer::{Zone, ZoneKind, ZoneProps};

    const CELL: f64 = 5.0;
    // A Lambert-93 origin inside France (near Chapelle-des-Bois).
    const OX: f64 = 960_000.0;
    const OY: f64 = 6_620_000.0;

    fn grid_of<T: Copy>(w: usize, h: usize, f: impl Fn(usize, usize) -> T) -> Grid<T> {
        let data = (0..w * h).map(|i| f(i % w, i / w)).collect();
        Grid::from_vec(
            w,
            h,
            GeoTransform::north_up(OX, OY, CELL),
            Crs::Lambert93,
            data,
        )
        .unwrap()
    }

    fn class(ids: &[&str]) -> ZoneClass {
        ZoneClass {
            zone_ids: ids.iter().map(|s| (*s).to_owned()).collect(),
            representative: LonLat::new(6.1, 46.6),
            cells: 0,
        }
    }

    /// Synthetic site: elevation and slope functions, zone class function, pistes.
    fn site(
        w: usize,
        h: usize,
        elev: impl Fn(usize, usize) -> f32,
        slope: impl Fn(usize, usize) -> f32,
        zone: impl Fn(usize, usize) -> u8,
        piste: impl Fn(usize, usize) -> u8,
        nclasses: usize,
    ) -> OffpisteGrid {
        OffpisteGrid::new(
            grid_of(w, h, |c, r| {
                elevation_raw(elev(c, r)).unwrap_or(ELEVATION_NODATA)
            }),
            grid_of(w, h, |c, r| slope_raw(slope(c, r))),
            grid_of(w, h, zone),
            grid_of(w, h, piste),
            (0..nclasses).map(|_| class(&[])).collect(),
        )
        .unwrap()
    }

    fn at(s: &str) -> DateTime<Utc> {
        s.parse().unwrap()
    }

    fn flat(w: usize, h: usize) -> OffpisteGrid {
        site(w, h, |_, _| 1000.0, |_, _| 0.0, |_, _| 0, |_, _| 0, 1)
    }

    fn router<'a>(
        g: &'a OffpisteGrid,
        policy: &'a ZonePolicy,
        params: &'a CostParams,
    ) -> Router<'a> {
        Router {
            grid: g,
            snow: None,
            overlays: &[],
            policy,
            params,
        }
    }

    #[test]
    fn raster_round_trip() {
        let g = grid_of(7, 5, |c, r| (c * 1000 + r) as u16).with_nodata(ELEVATION_NODATA);
        let bytes = encode_raster(&g, ELEVATION_NODATA).unwrap();
        assert_eq!(bytes.len(), HEADER_LEN + 7 * 5 * 2);
        let back: Grid<u16> = decode_raster(&bytes).unwrap();
        assert_eq!(back, g);
        let g8 = grid_of(3, 4, |c, r| (c + 10 * r) as u8).with_nodata(255);
        let back8: Grid<u8> = decode_raster(&encode_raster(&g8, 255).unwrap()).unwrap();
        assert_eq!(back8, g8);
        // Wrong type, truncated data, bad magic.
        assert!(decode_raster::<u8>(&bytes).is_err());
        assert!(decode_raster::<u16>(&bytes[..bytes.len() - 1]).is_err());
        let mut bad = bytes.clone();
        bad[0] = b'X';
        assert!(decode_raster::<u16>(&bad).is_err());
        // Regressions found by fuzzing (Q3, #103): a NaN origin, and a size whose byte
        // count overflows (2^32 − 1 squared cells of 2 bytes) are format errors.
        let mut nan = bytes.clone();
        nan[24..32].copy_from_slice(&f64::NAN.to_le_bytes());
        assert!(decode_raster::<u16>(&nan).is_err());
        let mut huge = bytes[..HEADER_LEN].to_vec();
        huge[8..16].copy_from_slice(&[0xFF; 8]);
        assert!(decode_raster::<u16>(&huge).is_err());
    }

    #[test]
    fn exposure_overlay_defaults_and_file() {
        let f = ExposureFactors::default();
        let o = f.overlay(grid_of(2, 2, |c, r| {
            [1u8, 2, 3, EXPOSURE_NODATA][c + 2 * r]
        }));
        assert_eq!(o.kind, OverlayKind::AvalancheExposure);
        assert_eq!(o.factor[1], 1.0);
        assert_eq!(o.factor[2], 3.0);
        assert!(o.factor[3].is_infinite(), "très exposé blocked by default");
        assert_eq!(o.factor[usize::from(EXPOSURE_NODATA)], 1.0);
        assert_eq!(o.factor[0], 1.0);
        // The user may allow « très exposé ».
        let allowed = ExposureFactors { high: 6.0, ..f }.overlay(o.grid.clone());
        assert_eq!(allowed.factor[3], 6.0);
        // From a prepared directory: absent file, no overlay.
        let dir = std::env::temp_dir().join(format!("skimap-exposure-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        assert!(load_exposure_overlay(&dir, &f).unwrap().is_none());
        write_raster(&dir.join(FILE_EXPOSURE), &o.grid, EXPOSURE_NODATA).unwrap();
        let back = load_exposure_overlay(&dir, &f).unwrap().unwrap();
        std::fs::remove_dir_all(&dir).unwrap();
        assert_eq!(back.grid.data(), o.grid.data());
        assert!(back.factor[3].is_infinite());
    }

    #[test]
    fn row_delta_round_trip_and_layout() {
        // Elevation-like rows with no-data cells and a wrap (65535 then 3).
        let g = grid_of(6, 3, |c, r| match (c, r) {
            (2, 1) => ELEVATION_NODATA,
            (3, 1) => 3,
            _ => (20_000 + c * 7 + r * 1000) as u16,
        })
        .with_nodata(ELEVATION_NODATA);
        let b = encode_raster_with(&g, ELEVATION_NODATA, Encoding::RowDelta).unwrap();
        assert_eq!(b[4], 2 | ROW_DELTA_FLAG);
        assert_eq!(b.len(), HEADER_LEN + 6 * 3 * 2);
        // Row 0: first cell 20000 (0x4E20), then deltas of 7: low plane, then high plane.
        let row0 = &b[HEADER_LEN..HEADER_LEN + 12];
        assert_eq!(&row0[..6], &[0x20, 7, 7, 7, 7, 7]);
        assert_eq!(&row0[6..], &[0x4E, 0, 0, 0, 0, 0]);
        let back: Grid<u16> = decode_raster(&b).unwrap();
        assert_eq!(back, g);
        assert_eq!(back.nodata(), Some(&ELEVATION_NODATA));
        // u8 rasters too; raw files still read.
        let g8 = grid_of(5, 4, |c, r| ((c * 37 + r * 91) % 256) as u8).with_nodata(255);
        let b8 = encode_raster_with(&g8, 255, Encoding::RowDelta).unwrap();
        assert_eq!(b8[4], 1 | ROW_DELTA_FLAG);
        assert_eq!(decode_raster::<u8>(&b8).unwrap(), g8);
        assert_eq!(
            decode_raster::<u8>(&encode_raster(&g8, 255).unwrap()).unwrap(),
            g8
        );
        // Files are written row-delta.
        let p = std::env::temp_dir().join(format!("skimap-row-delta-{}.skr", std::process::id()));
        write_raster(&p, &g, ELEVATION_NODATA).unwrap();
        let (head, back) = (std::fs::read(&p).unwrap()[4], read_raster::<u16>(&p));
        std::fs::remove_file(&p).unwrap();
        assert_eq!(head, 2 | ROW_DELTA_FLAG);
        assert_eq!(back.unwrap(), g);
        // A raw-only reader (type byte check) rejects them: here, the wrong type width.
        assert!(decode_raster::<u8>(&b).is_err());
    }

    #[test]
    fn quantisation() {
        assert_eq!(elevation_raw(2361.04), Some(23610));
        assert_eq!(elevation_m(23610), Some(2361.0));
        assert_eq!(elevation_raw(f32::NAN), None);
        assert_eq!(elevation_raw(-3.0), None);
        assert_eq!(slope_raw(29.99), 59);
        assert_eq!(slope_raw(30.0), 60);
        assert_eq!(slope_raw(f32::NAN), SLOPE_NODATA);
        let p = CostParams::default();
        // Exact at 30°: [29.5, 30) passes, [30, 30.5) is blocked.
        assert!(!p.slope_blocked(59));
        assert!(p.slope_blocked(60));
        assert!(p.slope_blocked(SLOPE_NODATA));
        // A limit between bins blocks the whole bin containing it.
        let q = CostParams {
            max_slope_deg: 29.7,
            ..CostParams::default()
        };
        assert!(q.slope_blocked(59));
        assert!(!q.slope_blocked(58));
        assert_eq!(p.slope_factor(40), 1.0);
        assert!((p.slope_factor(50) - 1.5).abs() < 1e-6);
    }

    #[test]
    fn zone_class_table_round_trip() {
        let classes = vec![
            ZoneClass {
                zone_ids: vec![],
                representative: LonLat::new(6.7, 44.8),
                cells: 12,
            },
            ZoneClass {
                zone_ids: vec!["country:fr".into(), "inpn:FR3801178".into()],
                representative: LonLat::new(6.1612345, 46.6312345),
                cells: 3,
            },
        ];
        let s = encode_zone_classes(&classes).unwrap();
        assert_eq!(parse_zone_classes(&s).unwrap(), classes);
        assert!(parse_zone_classes("nope\n").is_err());
        let bad = vec![class(&["a,b"])];
        assert!(encode_zone_classes(&bad).is_err());
    }

    #[test]
    fn downhill_factor_is_piecewise_linear() {
        let p = CostParams::default();
        let k = p.down_knots();
        assert_eq!(down_factor(&k, 0.0), 1.0);
        assert!((down_factor(&k, 8f32.to_radians().tan()) - 0.6).abs() < 1e-6);
        assert!((down_factor(&k, 25f32.to_radians().tan()) - 2.5).abs() < 1e-6);
        assert_eq!(down_factor(&k, 2.0), 5.0);
    }

    #[test]
    fn straight_route_on_the_flat() {
        let g = flat(60, 40);
        let pol = ZonePolicy::open(1, at("2026-02-15T10:00:00Z"));
        let params = CostParams::default();
        let r = router(&g, &pol, &params)
            .route_cells(Window::full(&g), (5, 20), (55, 20), Heuristic::AStar)
            .unwrap();
        assert_eq!(r.cells.len(), 51);
        assert!((r.length_m - 250.0).abs() < 1e-9);
        // No snow layer: unknown snow factor.
        let expect = 250.0 / 1.1 * f64::from(params.snow_unknown_factor);
        assert!((r.cost_s - expect).abs() < 1e-3, "{} {expect}", r.cost_s);
        assert_eq!(r.ascent_m, 0.0);
    }

    #[test]
    fn steep_wall_is_crossed_through_the_gap_only() {
        // A 35° band across the grid with a gap at rows 30..33.
        let g = site(
            60,
            40,
            |_, _| 1000.0,
            |c, r| {
                if (28..32).contains(&c) && !(30..33).contains(&r) {
                    35.0
                } else {
                    5.0
                }
            },
            |_, _| 0,
            |_, _| 0,
            1,
        );
        let pol = ZonePolicy::open(1, at("2026-02-15T10:00:00Z"));
        let params = CostParams::default();
        let r = router(&g, &pol, &params)
            .route_cells(Window::full(&g), (5, 5), (55, 5), Heuristic::AStar)
            .unwrap();
        for &(c, rr) in &r.cells {
            assert!(!params.slope_blocked(*g.slope.get(c, rr).unwrap()));
        }
        assert!(
            r.cells
                .iter()
                .any(|&(c, rr)| c == 30 && (30..33).contains(&rr))
        );
        assert!(r.max_slope_deg < 30.0);
        // Raising the limit to 40° opens the wall: shorter route.
        let open = CostParams {
            max_slope_deg: 40.0,
            ..CostParams::default()
        };
        let r2 = router(&g, &pol, &open)
            .route_cells(Window::full(&g), (5, 5), (55, 5), Heuristic::AStar)
            .unwrap();
        assert!(r2.length_m < r.length_m);
    }

    #[test]
    fn no_corner_cutting_between_blocked_cells() {
        // Two blocked cells touching by a corner: the diagonal between them is refused.
        let g = site(
            3,
            3,
            |_, _| 1000.0,
            |c, r| {
                if (c, r) == (1, 0) || (c, r) == (0, 1) {
                    40.0
                } else {
                    0.0
                }
            },
            |_, _| 0,
            |_, _| 0,
            1,
        );
        let pol = ZonePolicy::open(1, at("2026-02-15T10:00:00Z"));
        let params = CostParams::default();
        let err = router(&g, &pol, &params)
            .route_cells(Window::full(&g), (0, 0), (2, 2), Heuristic::AStar)
            .unwrap_err();
        assert_eq!(err, RouteError::NoRoute { expanded: 1 });
    }

    #[test]
    fn uphill_costs_more_than_downhill() {
        // Inclined plane: 10 % grade along columns (5.7°).
        let g = site(
            80,
            5,
            |c, _| 1000.0 + c as f32 * 0.5,
            |_, _| 5.7,
            |_, _| 0,
            |_, _| 0,
            1,
        );
        let pol = ZonePolicy::open(1, at("2026-02-15T10:00:00Z"));
        let params = CostParams::default();
        let rt = router(&g, &pol, &params);
        let up = rt
            .route_cells(Window::full(&g), (0, 2), (79, 2), Heuristic::AStar)
            .unwrap();
        let down = rt
            .route_cells(Window::full(&g), (79, 2), (0, 2), Heuristic::AStar)
            .unwrap();
        assert!((up.ascent_m - 39.5).abs() < 1e-6);
        assert!((down.descent_m - 39.5).abs() < 1e-6);
        // Uphill: L/v + dz/vc; downhill: L/v × down(0.1) < L/v.
        let f = f64::from(params.snow_unknown_factor);
        let expect_up = (395.0 / 1.1 + 39.5 / 0.1) * f;
        assert!(
            (up.cost_s - expect_up).abs() / expect_up < 1e-4,
            "{}",
            up.cost_s
        );
        assert!(down.cost_s < 395.0 / 1.1 * f);
        assert!(up.cost_s > 2.0 * down.cost_s);
    }

    /// Pseudo-random rugged terrain; A* and Dijkstra must find the same optimal cost.
    #[test]
    fn astar_is_optimal() {
        let mut seed = 0x1234_5678u64;
        let mut rnd = move || {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            (seed % 1000) as f32 / 1000.0
        };
        let (w, h) = (120, 90);
        let elev: Vec<f32> = (0..w * h)
            .map(|i| {
                let (c, r) = ((i % w) as f32, (i / w) as f32);
                1500.0 + 40.0 * (c / 9.0).sin() * (r / 7.0).cos() + 3.0 * rnd()
            })
            .collect();
        let slopes: Vec<f32> = (0..w * h).map(|_| rnd() * 36.0).collect();
        let pistes: Vec<u8> = (0..w * h)
            .map(|i| {
                if i / w == 45 {
                    PISTE_GROOMED
                } else {
                    PISTE_NONE
                }
            })
            .collect();
        let g = site(
            w,
            h,
            |c, r| elev[r * w + c],
            |c, r| slopes[r * w + c],
            |c, _| u8::from(c > 60),
            |c, r| pistes[r * w + c],
            2,
        );
        let mut pol = ZonePolicy::open(2, at("2026-02-15T10:00:00Z"));
        pol.classes[1].avoid = true;
        let params = CostParams::default();
        let rt = router(&g, &pol, &params);
        let mut found = 0;
        for (a, b) in [
            ((2, 2), (117, 87)),
            ((110, 3), (4, 80)),
            ((60, 1), (60, 88)),
        ] {
            let x = rt.route_cells(Window::full(&g), a, b, Heuristic::AStar);
            let y = rt.route_cells(Window::full(&g), a, b, Heuristic::Dijkstra);
            match (x, y) {
                (Ok(x), Ok(y)) => {
                    assert!(
                        (x.cost_s - y.cost_s).abs() <= 1e-4 * y.cost_s,
                        "{} {}",
                        x.cost_s,
                        y.cost_s
                    );
                    assert!(x.stats.expanded <= y.stats.expanded);
                    found += 1;
                }
                (Err(_), Err(_)) => {}
                (x, y) => panic!("A* {x:?} vs Dijkstra {y:?}"),
            }
        }
        assert!(found >= 2, "terrain too blocked for the test");
    }

    #[test]
    fn forbidden_zone_is_crossed_on_the_piste_only() {
        // Class 1 = a band of columns 20..40 where leaving the routes is forbidden; a
        // marked itinerary runs along row 30.
        let g = site(
            60,
            40,
            |_, _| 1000.0,
            |_, _| 0.0,
            |c, _| u8::from((20..40).contains(&c)),
            |_, r| if r == 30 { PISTE_MARKED } else { PISTE_NONE },
            2,
        );
        let params = CostParams::default();
        let mut winter = ZonePolicy::open(2, at("2026-02-15T10:00:00Z"));
        winter.classes[1].offpiste_blocked = true;
        let r = router(&g, &winter, &params)
            .route_cells(Window::full(&g), (5, 5), (55, 5), Heuristic::AStar)
            .unwrap();
        for (k, &(c, rr)) in r.cells.iter().enumerate() {
            if (20..40).contains(&c) {
                assert_eq!(rr, 30);
                assert_eq!(r.piste[k], PISTE_MARKED);
            }
        }
        let summer = ZonePolicy::open(2, at("2026-08-15T10:00:00Z"));
        let r2 = router(&g, &summer, &params)
            .route_cells(Window::full(&g), (5, 5), (55, 5), Heuristic::AStar)
            .unwrap();
        assert!(r2.cells.iter().all(|&(_, rr)| rr == 5));
        // Access forbidden: blocked even on the piste.
        let mut closed = winter.clone();
        closed.classes[1].blocked = true;
        let err = router(&g, &closed, &params)
            .route_cells(Window::full(&g), (5, 5), (55, 5), Heuristic::AStar)
            .unwrap_err();
        assert!(matches!(err, RouteError::NoRoute { .. }));
    }

    #[test]
    fn snow_classes_and_overlay_hook() {
        let g = flat(60, 40);
        let pol = ZonePolicy::open(1, at("2026-02-15T10:00:00Z"));
        // 25 m snow grid aligned on the 5 m grid: insufficient snow on 25 m column 6
        // (5 m columns 30..35) except at 25 m row 6 (5 m rows 30..35).
        let snow = SnowGrid {
            grid: Grid::from_vec(
                12,
                8,
                GeoTransform::north_up(OX, OY, 25.0),
                Crs::Lambert93,
                (0..96)
                    .map(|i| {
                        let (c, r) = (i % 12, i / 12);
                        if c == 6 && r != 6 {
                            SnowGrid::pack(Some(0), Some(0))
                        } else {
                            SnowGrid::pack(Some(2), Some(2))
                        }
                    })
                    .collect(),
            )
            .unwrap(),
        };
        let params = CostParams {
            snow_factor: [f32::INFINITY, 1.5, 1.0],
            ..CostParams::default()
        };
        let mut rt = router(&g, &pol, &params);
        rt.snow = Some(&snow);
        assert!(!rt.cell_factor(31, 5).is_finite());
        assert_eq!(rt.cell_factor(31, 31), 1.0);
        let r = rt
            .route_cells(Window::full(&g), (5, 5), (55, 5), Heuristic::AStar)
            .unwrap();
        assert!(
            r.cells
                .iter()
                .filter(|&&(c, _)| (30..35).contains(&c))
                .all(|&(_, rr)| (30..35).contains(&rr))
        );
        // Overlay hook: avalanche exposure class 3 blocks the gap; no route left.
        let mut factor = [1.0f32; 256];
        factor[3] = f32::INFINITY;
        let ov = Overlay {
            kind: OverlayKind::AvalancheExposure,
            grid: Grid::from_vec(
                60,
                40,
                GeoTransform::north_up(OX, OY, CELL),
                Crs::Lambert93,
                (0..2400)
                    .map(|i| if (30..35).contains(&(i / 60)) { 3 } else { 0 })
                    .collect(),
            )
            .unwrap(),
            factor,
        };
        let overlays = [ov];
        rt.overlays = &overlays;
        assert!(matches!(
            rt.route_cells(Window::full(&g), (5, 5), (55, 5), Heuristic::AStar),
            Err(RouteError::NoRoute { .. })
        ));
    }

    #[test]
    fn ends_are_snapped_to_passable_cells() {
        // Left half blocked (no elevation).
        let g = site(
            60,
            40,
            |c, _| if c < 30 { f32::NAN } else { 1000.0 },
            |_, _| 0.0,
            |_, _| 0,
            |_, _| 0,
            1,
        );
        let pol = ZonePolicy::open(1, at("2026-02-15T10:00:00Z"));
        let params = CostParams::default();
        let rt = router(&g, &pol, &params);
        let p = g.center(20, 10).unwrap();
        let ((c, r), d) = rt.snap(p, Endpoint::From).unwrap();
        assert_eq!((c, r), (30, 10));
        assert!((d - 50.0).abs() < 0.01, "{d}");
        let far = CostParams {
            snap_radius_m: 20.0,
            ..CostParams::default()
        };
        assert!(matches!(
            router(&g, &pol, &far).snap(p, Endpoint::From),
            Err(RouteError::NoPassableCell { .. })
        ));
        let q = g.center(50, 30).unwrap();
        let route = rt.route(p, q).unwrap();
        assert_eq!(route.cells[0], (30, 10));
        assert_eq!(*route.cells.last().unwrap(), (50, 30));
        assert!(route.to_snap_m < 0.01);
    }

    fn zone(id: &str, kind: ZoneKind, b: BBox) -> Zone {
        Zone::new(
            ZoneProps {
                id: id.into(),
                kind,
                name: id.into(),
                source: "test".into(),
                url: None,
                months: None,
                practices: vec![],
                designated: None,
                area_ha: 1.0,
                clipped: false,
                note: None,
            },
            MultiPolygon::new(vec![bbox_polygon(b, 1)]),
        )
    }

    #[test]
    fn policy_follows_the_appb_season() {
        // The real rule file: APPB Haut-Doubs, leaving routes forbidden 15/12 to 30/06.
        let rules = RuleSet::embedded().unwrap();
        let layer = ZoneLayer {
            zones: vec![
                zone(
                    "country:fr",
                    ZoneKind::Country,
                    BBox::new(46.0, 5.0, 47.0, 7.0),
                ),
                zone(
                    "inpn:FR3801178",
                    ZoneKind::Appb,
                    BBox::new(46.6, 6.14, 46.66, 6.2),
                ),
            ],
        };
        let classes = vec![
            ZoneClass {
                zone_ids: vec!["country:fr".into()],
                representative: LonLat::new(6.10, 46.62),
                cells: 1,
            },
            ZoneClass {
                zone_ids: vec!["country:fr".into(), "inpn:FR3801178".into()],
                representative: LonLat::new(6.17, 46.63),
                cells: 1,
            },
        ];
        let feb = zone_policy(&classes, &layer, &rules, at("2026-02-15T10:00:00Z"));
        assert!(!feb.classes[0].offpiste_blocked);
        assert!(feb.classes[1].offpiste_blocked);
        assert!(!feb.classes[1].blocked);
        assert_eq!(
            feb.classes[1].rules,
            vec!["appb-haut-doubs-leave-routes".to_owned()]
        );
        let aug = zone_policy(&classes, &layer, &rules, at("2026-08-15T10:00:00Z"));
        assert!(!aug.classes[1].offpiste_blocked);
        assert!(aug.classes.iter().all(|c| c.covered));
    }
}
