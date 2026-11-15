//! IGN LiDAR HD terrain model (MNT) at 5 m from the Géoplateforme WMS-R.
//!
//! The LiDAR HD MNT is distributed as 1 km × 1 km tiles at 50 cm, named after the
//! kilometre of their west edge and of their north edge in Lambert-93
//! (`LHD_FXX_0998_6426_MNT_O_0M50_LAMB93_IGN69`). The WMS serves each tile resampled to
//! 5 m with `WIDTH=200&HEIGHT=200`: exactly the request used by research report 07, so the
//! 143 tiles already in `research/raw/census_ava/ign/dem5/` are byte-identical to what
//! this module downloads (checked by the `research_tiles_check` example).
//!
//! The tile bounding box is shifted by a quarter metre (`997999.75 .. 998999.75`)
//! because the 50 cm pixels of the source tiles are centred on whole metres; the 5 m grid
//! keeps that origin. Outside the LiDAR HD coverage (Italy, Switzerland) the WMS returns
//! `-9999`, which becomes `NaN`.
//!
//! Licence: Licence Ouverte 2.0 (Etalab), IGN; some military areas (ZICAD) are masked.

use skimap_common::crs::{Crs, project_bbox};
use skimap_common::download::{CachePolicy, Downloader, Request};
use skimap_common::geom::BBox;
use skimap_common::grid::{GeoTransform, Grid};
use tracing::{debug, warn};

use crate::error::{Error, Result};
use skimap_common::geotiff::decode_f32;

/// Downloader source name of the LiDAR HD WMS-R. Its policy in
/// `skimap_common::download::known_sources` keeps tiles a year: LiDAR HD editions change
/// rarely and the WMS sends neither `ETag` nor `Last-Modified`, so a revalidation is a full
/// download (0.5 s per tile); force a refresh with `CachePolicy::Reload`.
pub const SOURCE: &str = "ign-lidar-wms";
/// WMS-R endpoint of the Géoplateforme (raw rasters).
pub const WMS_URL: &str = "https://data.geopf.fr/wms-r";
/// LiDAR HD terrain model layer (Lambert-93, IGN69 heights).
pub const LAYER: &str = "IGNF_LIDAR-HD_MNT_ELEVATION.ELEVATIONGRIDCOVERAGE.LAMB93";
/// Cell size of the downloaded terrain model, metres.
pub const CELL: f64 = 5.0;
/// Pixels per tile side (1 km at 5 m).
pub const TILE_PX: usize = 200;
/// Offset of the tile grid relative to whole kilometres, metres.
pub const TILE_SHIFT: f64 = 0.25;
/// Attempts when the WMS answers 200 with a service exception instead of an image.
const EXCEPTION_ATTEMPTS: u32 = 4;

/// An axis-aligned Lambert-93 extent in metres.
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Extent {
    /// West.
    pub min_x: f64,
    /// South.
    pub min_y: f64,
    /// East.
    pub max_x: f64,
    /// North.
    pub max_y: f64,
}

impl Extent {
    /// Lambert-93 extent of a WGS84 box (edges densified).
    pub fn from_bbox(b: BBox) -> Result<Self> {
        let (min_x, min_y, max_x, max_y) = project_bbox(b, Crs::Lambert93, 64)?;
        Ok(Self {
            min_x,
            min_y,
            max_x,
            max_y,
        })
    }

    /// Square extent of half-side `r` metres around a point.
    pub fn around(x: f64, y: f64, r: f64) -> Self {
        Self {
            min_x: x - r,
            min_y: y - r,
            max_x: x + r,
            max_y: y + r,
        }
    }

    /// Area in km².
    pub fn area_km2(&self) -> f64 {
        (self.max_x - self.min_x) * (self.max_y - self.min_y) / 1e6
    }
}

/// A LiDAR HD tile: kilometre index of its west edge and of its north edge.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct TileId {
    /// West edge, km (Lambert-93 X / 1000).
    pub kx: i64,
    /// North edge, km (Lambert-93 Y / 1000).
    pub ky: i64,
}

impl TileId {
    /// Top-left corner of the 5 m grid of this tile.
    pub fn origin(&self) -> (f64, f64) {
        (
            self.kx as f64 * 1000.0 - TILE_SHIFT,
            self.ky as f64 * 1000.0 + TILE_SHIFT,
        )
    }

    /// IGN name, e.g. `0998-6426`.
    pub fn name(&self) -> String {
        format!("{:04}-{:04}", self.kx, self.ky)
    }

    /// WMS GetMap URL of the tile resampled to 5 m.
    pub fn url(&self) -> String {
        let (x0, y1) = self.origin();
        let (x1, y0) = (x0 + 1000.0, y1 - 1000.0);
        format!(
            "{WMS_URL}?SERVICE=WMS&VERSION=1.3.0&EXCEPTIONS=text/xml&REQUEST=GetMap\
             &LAYERS={LAYER}&FORMAT=image/geotiff&STYLES=&CRS=EPSG:2154\
             &BBOX={x0},{y0},{x1},{y1}&WIDTH={TILE_PX}&HEIGHT={TILE_PX}\
             &FILENAME=LHD_FXX_{:04}_{:04}_MNT_O_0M50_LAMB93_IGN69.tif",
            self.kx, self.ky
        )
    }
}

/// Tiles whose interior intersects an extent (a tile only touching an edge is left
/// out), west to east then north to south.
pub fn tiles_for(e: &Extent) -> Vec<TileId> {
    // Tile kx spans X in [1000 kx - 0.25, 1000 (kx + 1) - 0.25]; tile ky spans Y in
    // [1000 (ky - 1) + 0.25, 1000 ky + 0.25].
    let kx0 = ((e.min_x + TILE_SHIFT) / 1000.0).floor() as i64;
    let kx1 = ((e.max_x + TILE_SHIFT) / 1000.0).ceil() as i64 - 1;
    let ky0 = ((e.min_y - TILE_SHIFT) / 1000.0).floor() as i64 + 1;
    let ky1 = ((e.max_y - TILE_SHIFT) / 1000.0).ceil() as i64;
    let mut out = Vec::new();
    for ky in (ky0..=ky1).rev() {
        for kx in kx0..=kx1 {
            out.push(TileId { kx, ky });
        }
    }
    out
}

/// Downloads (or reads from cache) one tile and decodes it.
pub fn fetch_tile(t: TileId, dl: &Downloader) -> Result<Grid<f32>> {
    let url = t.url();
    let mut policy = CachePolicy::Normal;
    let mut last_err = None;
    for attempt in 0..EXCEPTION_ATTEMPTS {
        let f = dl.fetch(&Request::get(SOURCE, &url).cache(policy))?;
        let bytes = f.bytes()?;
        match decode_f32(&bytes) {
            Ok(g) => {
                check_tile(t, &g)?;
                return Ok(g);
            }
            Err(e) if bytes.starts_with(b"II*\0") || bytes.starts_with(b"MM\0*") => {
                // A real TIFF we cannot read: reloading would not help.
                return Err(Error::BadData {
                    source_name: url,
                    message: e.to_string(),
                });
            }
            Err(e) => {
                // The WMS sometimes answers 200 with a ServiceException (`LayerNotDefined`)
                // that then succeeds on retry (report 07): do not keep it in the cache.
                let head = String::from_utf8_lossy(&bytes[..bytes.len().min(200)]).to_string();
                warn!(tile = %t.name(), attempt, %head, "WMS answer is not a GeoTIFF, reloading");
                last_err = Some(Error::BadData {
                    source_name: url.clone(),
                    message: format!("{e}; body starts with {head:?}"),
                });
                policy = CachePolicy::Reload;
                std::thread::sleep(std::time::Duration::from_secs(1 << attempt));
            }
        }
    }
    Err(last_err.expect("at least one attempt"))
}

fn check_tile(t: TileId, g: &Grid<f32>) -> Result<()> {
    let (x0, y1) = t.origin();
    let gt = g.transform();
    let ok = g.width() == TILE_PX
        && g.height() == TILE_PX
        && g.crs() == Crs::Lambert93
        && (gt.origin_x - x0).abs() < 1e-6
        && (gt.origin_y - y1).abs() < 1e-6
        && (gt.pixel_width - CELL).abs() < 1e-9;
    if ok {
        Ok(())
    } else {
        Err(Error::BadData {
            source_name: t.url(),
            message: format!(
                "unexpected georeferencing {}x{} {:?} {:?}",
                g.width(),
                g.height(),
                gt,
                g.crs()
            ),
        })
    }
}

/// 5 m terrain model covering `e`, cropped to the extent (snapped outward to cells).
/// Cells outside the LiDAR HD coverage are `NaN`.
pub fn fetch_dem(e: &Extent, dl: &Downloader) -> Result<Grid<f32>> {
    let tiles = tiles_for(e);
    let kx0 = tiles.iter().map(|t| t.kx).min().unwrap_or_default();
    let kx1 = tiles.iter().map(|t| t.kx).max().unwrap_or_default();
    let ky0 = tiles.iter().map(|t| t.ky).min().unwrap_or_default();
    let ky1 = tiles.iter().map(|t| t.ky).max().unwrap_or_default();
    let nx = usize::try_from(kx1 - kx0 + 1).unwrap_or(0);
    let ny = usize::try_from(ky1 - ky0 + 1).unwrap_or(0);
    let (w, h) = (nx * TILE_PX, ny * TILE_PX);
    let origin = TileId { kx: kx0, ky: ky1 }.origin();
    let gt = GeoTransform::north_up(origin.0, origin.1, CELL);
    let mut mosaic = Grid::filled(w, h, gt, Crs::Lambert93, f32::NAN);
    debug!(tiles = tiles.len(), w, h, "fetching LiDAR HD tiles");
    // Tiles fetched on several threads (the downloader bounds the requests in flight).
    let fetched =
        skimap_common::download::fetch_all(&tiles, skimap_common::download::FETCH_THREADS, |t| {
            fetch_tile(*t, dl)
        })?;
    for (t, tile) in tiles.iter().zip(fetched) {
        let c0 = usize::try_from(t.kx - kx0).unwrap_or(0) * TILE_PX;
        let r0 = usize::try_from(ky1 - t.ky).unwrap_or(0) * TILE_PX;
        let data = mosaic.data_mut();
        for r in 0..TILE_PX {
            let src = &tile.data()[r * TILE_PX..(r + 1) * TILE_PX];
            let dst = &mut data[(r0 + r) * w + c0..(r0 + r) * w + c0 + TILE_PX];
            for (d, s) in dst.iter_mut().zip(src) {
                // -9999 is already NaN; guard against other large negative fill values.
                *d = if s.is_finite() && *s > -1000.0 {
                    *s
                } else {
                    f32::NAN
                };
            }
        }
    }
    erode_valid(&mut mosaic, NODATA_MARGIN);
    Ok(crop(&mosaic, e))
}

/// Cells dropped next to no-data, see [`erode_valid`].
pub const NODATA_MARGIN: usize = 2;

/// Sets to `NaN` every valid cell within `margin` cells (8-neighbourhood) of a `NaN` cell.
///
/// The WMS resampling mixes the `-9999` fill value into the 5 m cells along the edge of
/// the LiDAR HD coverage: on Cervières, the first ring of valid cells next to no-data
/// deviates up to 2 650 m from its neighbours (e.g. 317 m instead of 2 252 m). The second
/// ring is close to the interior and the third is like the interior; two rings are
/// dropped.
pub fn erode_valid(g: &mut Grid<f32>, margin: usize) {
    let (w, h) = (g.width(), g.height());
    for _ in 0..margin {
        let nan: Vec<bool> = g.data().iter().map(|v| v.is_nan()).collect();
        let data = g.data_mut();
        for r in 0..h {
            for c in 0..w {
                if nan[r * w + c] {
                    continue;
                }
                let touches = (r.saturating_sub(1)..(r + 2).min(h))
                    .any(|rr| (c.saturating_sub(1)..(c + 2).min(w)).any(|cc| nan[rr * w + cc]));
                if touches {
                    data[r * w + c] = f32::NAN;
                }
            }
        }
    }
}

/// Sub-grid covering `e`, snapped outward to whole cells and clamped to the grid.
pub fn crop(g: &Grid<f32>, e: &Extent) -> Grid<f32> {
    let gt = g.transform();
    let (c0, r0) = gt.world_to_pixel(e.min_x, e.max_y);
    let (c1, r1) = gt.world_to_pixel(e.max_x, e.min_y);
    let clamp = |v: f64, n: usize| (v.max(0.0) as usize).min(n);
    // Tolerance: an extent edge on a cell edge must not add a cell through rounding.
    const EPS: f64 = 1e-6;
    let (c0, r0) = (
        clamp((c0 + EPS).floor(), g.width()),
        clamp((r0 + EPS).floor(), g.height()),
    );
    let (c1, r1) = (
        clamp((c1 - EPS).ceil(), g.width()),
        clamp((r1 - EPS).ceil(), g.height()),
    );
    let (w, h) = (c1.saturating_sub(c0), r1.saturating_sub(r0));
    let (ox, oy) = gt.pixel_to_world(c0 as f64, r0 as f64);
    let ngt = GeoTransform::north_up(ox, oy, gt.pixel_width);
    let mut data = Vec::with_capacity(w * h);
    for r in r0..r1 {
        data.extend_from_slice(&g.data()[r * g.width() + c0..r * g.width() + c1]);
    }
    Grid::from_vec(w, h, ngt, g.crs(), data).expect("consistent crop")
}

/// Block mean by an integer `factor` (e.g. 5 m to 25 m). A coarse cell is the mean of its
/// valid fine cells when at least half of them are valid, `NaN` otherwise.
pub fn block_mean(g: &Grid<f32>, factor: usize) -> Grid<f32> {
    let (w, h) = (g.width() / factor, g.height() / factor);
    let gt = g.transform();
    let ngt = GeoTransform::north_up(gt.origin_x, gt.origin_y, gt.pixel_width * factor as f64);
    let mut out = vec![f32::NAN; w * h];
    let need = (factor * factor).div_ceil(2);
    for (r, row) in out.chunks_mut(w.max(1)).enumerate().take(h) {
        for (c, cell) in row.iter_mut().enumerate() {
            let (mut sum, mut n) = (0.0f64, 0usize);
            for rr in r * factor..(r + 1) * factor {
                for cc in c * factor..(c + 1) * factor {
                    let v = g.data()[rr * g.width() + cc];
                    if v.is_finite() {
                        sum += f64::from(v);
                        n += 1;
                    }
                }
            }
            if n >= need {
                *cell = (sum / n as f64) as f32;
            }
        }
    }
    Grid::from_vec(w, h, ngt, g.crs(), out).expect("consistent block mean")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tile_url_matches_research_request() {
        // First line of research/raw/census_ava/ign/dem5/urls.txt (report 07).
        let expected = "https://data.geopf.fr/wms-r?SERVICE=WMS&VERSION=1.3.0&EXCEPTIONS=text/xml&REQUEST=GetMap&LAYERS=IGNF_LIDAR-HD_MNT_ELEVATION.ELEVATIONGRIDCOVERAGE.LAMB93&FORMAT=image/geotiff&STYLES=&CRS=EPSG:2154&BBOX=997999.75,6425000.25,998999.75,6426000.25&WIDTH=200&HEIGHT=200&FILENAME=LHD_FXX_0998_6426_MNT_O_0M50_LAMB93_IGN69.tif";
        assert_eq!(TileId { kx: 998, ky: 6426 }.url(), expected);
        assert_eq!(TileId { kx: 998, ky: 6426 }.name(), "0998-6426");
    }

    #[test]
    fn tiles_cover_extent() {
        let e = Extent {
            min_x: 998_100.0,
            min_y: 6_424_500.0,
            max_x: 999_900.0,
            max_y: 6_425_100.0,
        };
        let t = tiles_for(&e);
        // x: 998, 999 ; y (north edges): 6426 (covers 6425000..6426000), 6425.
        assert_eq!(
            t,
            vec![
                TileId { kx: 998, ky: 6426 },
                TileId { kx: 999, ky: 6426 },
                TileId { kx: 998, ky: 6425 },
                TileId { kx: 999, ky: 6425 },
            ]
        );
        // Edge just below a tile boundary (6425000.25 belongs to the upper tile).
        let e2 = Extent {
            min_x: 998_000.0,
            min_y: 6_425_000.3,
            max_x: 998_001.0,
            max_y: 6_425_001.0,
        };
        assert_eq!(tiles_for(&e2), vec![TileId { kx: 998, ky: 6426 }]);
        // Extent exactly on tile edges: the touching neighbours are not included.
        let e3 = Extent {
            min_x: 997_999.75,
            min_y: 6_425_000.25,
            max_x: 998_999.75,
            max_y: 6_426_000.25,
        };
        assert_eq!(tiles_for(&e3), vec![TileId { kx: 998, ky: 6426 }]);
    }

    #[test]
    fn crop_and_block_mean() {
        let gt = GeoTransform::north_up(0.0, 100.0, 5.0);
        let data: Vec<f32> = (0..400).map(|i| i as f32).collect();
        let g = Grid::from_vec(20, 20, gt, Crs::Lambert93, data).unwrap();
        let c = crop(
            &g,
            &Extent {
                min_x: 7.0,
                min_y: 40.0,
                max_x: 18.0,
                max_y: 93.0,
            },
        );
        // Columns 1..4 (x 5..20), rows 1..12 (y 95..35 -> 93 is in row 1, 40 in row 11).
        assert_eq!((c.width(), c.height()), (3, 11));
        assert_eq!(c.transform().origin_x, 5.0);
        assert_eq!(c.transform().origin_y, 95.0);
        assert_eq!(c.data()[0], 21.0);
        let mut g2 = g.clone();
        g2.data_mut()[0] = f32::NAN;
        let m = block_mean(&g2, 2);
        assert_eq!((m.width(), m.height()), (10, 10));
        assert_eq!(m.transform().pixel_width, 10.0);
        // Block (0,0) = {NaN, 1, 20, 21}: 3 valid of 4 -> mean 14.
        assert_eq!(m.data()[0], 14.0);
        assert_eq!(m.data()[1], (2.0 + 3.0 + 22.0 + 23.0) / 4.0);
        let mut g3 = g.clone();
        for i in [0, 1, 20] {
            g3.data_mut()[i] = f32::NAN;
        }
        assert!(block_mean(&g3, 2).data()[0].is_nan());
        // Extent exactly on cell edges (with floating noise): no extra cell.
        let c = crop(
            &g,
            &Extent {
                min_x: 10.000_000_001,
                min_y: 49.999_999_999,
                max_x: 24.999_999_999,
                max_y: 90.000_000_001,
            },
        );
        assert_eq!((c.width(), c.height()), (3, 8));
        assert_eq!(c.transform().origin_x, 10.0);
        assert_eq!(c.transform().origin_y, 90.0);
    }

    #[test]
    fn erosion_next_to_nodata() {
        let gt = GeoTransform::north_up(0.0, 0.0, 5.0);
        let mut g = Grid::filled(7, 7, gt, Crs::Lambert93, 1.0f32);
        g.data_mut()[3 * 7 + 3] = f32::NAN;
        let mut g1 = g.clone();
        erode_valid(&mut g1, 1);
        let nan1 = g1.data().iter().filter(|v| v.is_nan()).count();
        assert_eq!(nan1, 9);
        erode_valid(&mut g, 2);
        let nan2 = g.data().iter().filter(|v| v.is_nan()).count();
        assert_eq!(nan2, 25);
        assert_eq!(g.get(0, 0), Some(&1.0));
        assert!(g.get(1, 1).unwrap().is_nan());
    }
}
