//! Forest canopy cover from the IGN LiDAR HD canopy height model (MNH).
//!
//! The MNH (« modèle numérique de hauteur », canopy height above ground) is served by the
//! Géoplateforme WMS-R exactly like the terrain model: same 1 km tiles, same quarter-metre
//! shift, resampled to 5 m with `WIDTH=200` (layer
//! `IGNF_LIDAR-HD_MNH_ELEVATION.ELEVATIONGRIDCOVERAGE.LAMB93`, Licence Ouverte). The 5 m
//! value is close to the mean height of the 50 cm source pixels (measured on tile
//! 1003-6415, Queyras: correlation 0.94 with the 1 m block mean, 0.82 with the block
//! maximum).
//!
//! AutoATES v3.0 expects a canopy *cover* in percent (its reference input is a Sentinel-2
//! product at 10 to 20 m). We derive it from the MNH: a 5 m cell is « tree » when its
//! canopy height is at least [`TREE_HEIGHT_M`], and the cover of a cell is the share of
//! tree cells in the 3 × 3 window around it (15 m), in percent. Both choices are ours and
//! not calibrated; they are recorded in the layer metadata.

use skimap_common::download::{Downloader, Request};
use skimap_common::grid::{GeoTransform, Grid};
use skimap_terrain::dem::{self, Extent, TileId};
use tracing::debug;

use super::raster::Raster;
use crate::error::{Error, Result};

/// WMS-R layer of the LiDAR HD canopy height model.
pub const MNH_LAYER: &str = "IGNF_LIDAR-HD_MNH_ELEVATION.ELEVATIONGRIDCOVERAGE.LAMB93";
/// Canopy height from which a 5 m cell counts as tree, metres.
pub const TREE_HEIGHT_M: f32 = 3.0;
/// Half side of the cover window, cells (1 = 3 × 3 = 15 m at 5 m).
pub const COVER_RADIUS: usize = 1;

/// WMS GetMap URL of one MNH tile resampled to 5 m (same grid as the terrain model).
pub fn mnh_url(t: TileId) -> String {
    let (x0, y1) = t.origin();
    let (x1, y0) = (x0 + 1000.0, y1 - 1000.0);
    format!(
        "{}?SERVICE=WMS&VERSION=1.3.0&EXCEPTIONS=text/xml&REQUEST=GetMap\
         &LAYERS={MNH_LAYER}&FORMAT=image/geotiff&STYLES=&CRS=EPSG:2154\
         &BBOX={x0},{y0},{x1},{y1}&WIDTH={px}&HEIGHT={px}\
         &FILENAME=LHD_FXX_{:04}_{:04}_MNH_O_0M50_LAMB93_IGN69.tif",
        dem::WMS_URL,
        t.kx,
        t.ky,
        px = dem::TILE_PX,
    )
}

/// Downloads (or reads from cache) one MNH tile. Same WMS-R as the terrain model, same
/// unreliable answers (a 400, a GeoTIFF without `ModelPixelScaleTag` or with incomplete
/// geokeys, 15/11/2026): refused and fetched again, never kept in the cache
/// (`skimap_terrain::dem::fetch_tile`).
fn fetch_tile(t: TileId, dl: &Downloader) -> Result<Grid<f32>> {
    let url = mnh_url(t);
    // Cache lifetime: the source policy of the shared downloader (same source as the
    // terrain model tiles).
    dl.fetch_checked(
        &Request::get(dem::SOURCE, &url),
        dem::TILE_ATTEMPTS,
        dem::TILE_BACKOFF,
        &[400, 404],
        |f| {
            let bytes = f.bytes().map_err(|e| e.to_string())?;
            let g = skimap_common::geotiff::decode_f32(&bytes).map_err(|e| {
                let head = if bytes.starts_with(b"II*\0") || bytes.starts_with(b"MM\0*") {
                    "TIFF".to_owned()
                } else {
                    String::from_utf8_lossy(&bytes[..bytes.len().min(200)]).to_string()
                };
                format!("{e}; body starts with {head:?}")
            })?;
            let gt = g.transform();
            let (x0, y1) = t.origin();
            if g.width() != dem::TILE_PX
                || g.height() != dem::TILE_PX
                || (gt.origin_x - x0).abs() > 1e-6
                || (gt.origin_y - y1).abs() > 1e-6
            {
                return Err("unexpected MNH tile georeferencing".to_owned());
            }
            Ok(g)
        },
    )
    .map_err(|e| match e {
        skimap_common::Error::Invalid { url, message, .. } => Error::format(url, message),
        e => e.into(),
    })
}

/// Canopy height (m) at 5 m over `e`, on the grid of `skimap_terrain::dem::fetch_dem`
/// (same tiles, same crop). Fill values and the WMS mixing of the fill value along the
/// coverage edge (large negative heights) become `NaN`; small negative heights (LiDAR
/// noise, down to -0.05 m on the probed tiles) become 0.
pub fn fetch_mnh(e: &Extent, dl: &Downloader) -> Result<Grid<f32>> {
    let tiles = dem::tiles_for(e);
    let kx0 = tiles.iter().map(|t| t.kx).min().unwrap_or_default();
    let kx1 = tiles.iter().map(|t| t.kx).max().unwrap_or_default();
    let ky0 = tiles.iter().map(|t| t.ky).min().unwrap_or_default();
    let ky1 = tiles.iter().map(|t| t.ky).max().unwrap_or_default();
    let nx = usize::try_from(kx1 - kx0 + 1).unwrap_or(0);
    let ny = usize::try_from(ky1 - ky0 + 1).unwrap_or(0);
    let (w, h) = (nx * dem::TILE_PX, ny * dem::TILE_PX);
    let origin = TileId { kx: kx0, ky: ky1 }.origin();
    let gt = GeoTransform::north_up(origin.0, origin.1, dem::CELL);
    let mut mosaic = Grid::filled(w, h, gt, skimap_common::crs::Crs::Lambert93, f32::NAN);
    debug!(tiles = tiles.len(), "fetching LiDAR HD MNH tiles");
    // Tiles fetched on several threads (the downloader bounds the requests in flight).
    let fetched =
        skimap_common::download::fetch_all(&tiles, skimap_common::download::FETCH_THREADS, |t| {
            fetch_tile(*t, dl)
        })?;
    for (t, tile) in tiles.iter().zip(fetched) {
        let c0 = usize::try_from(t.kx - kx0).unwrap_or(0) * dem::TILE_PX;
        let r0 = usize::try_from(ky1 - t.ky).unwrap_or(0) * dem::TILE_PX;
        let data = mosaic.data_mut();
        for r in 0..dem::TILE_PX {
            let src = &tile.data()[r * dem::TILE_PX..(r + 1) * dem::TILE_PX];
            let dst = &mut data[(r0 + r) * w + c0..(r0 + r) * w + c0 + dem::TILE_PX];
            for (d, s) in dst.iter_mut().zip(src) {
                *d = if s.is_finite() && *s > -1.0 {
                    s.max(0.0)
                } else {
                    f32::NAN
                };
            }
        }
    }
    dem::erode_valid(&mut mosaic, dem::NODATA_MARGIN);
    Ok(dem::crop(&mosaic, e))
}

/// Canopy cover (%) from canopy height: share of cells at least [`TREE_HEIGHT_M`] high in
/// the `(2 r + 1)²` window ([`COVER_RADIUS`]), counting only cells with a height. `NaN`
/// where the height is unknown or where `dem` has no data.
pub fn canopy_cover(height: &Raster<f32>, dem: &Raster<f32>) -> Raster<f32> {
    assert!(height.same_grid(dem), "MNH and DEM grids differ");
    let (w, h) = (height.w, height.h);
    let r = COVER_RADIUS;
    let tree: Vec<u8> = height
        .data
        .iter()
        .map(|v| u8::from(v.is_finite() && *v >= TREE_HEIGHT_M))
        .collect();
    let mut out = vec![f32::NAN; w * h];
    for row in 0..h {
        for col in 0..w {
            let i = row * w + col;
            if !height.data[i].is_finite() || !dem.data[i].is_finite() {
                continue;
            }
            let (mut n, mut t) = (0u32, 0u32);
            for rr in row.saturating_sub(r)..(row + r + 1).min(h) {
                for cc in col.saturating_sub(r)..(col + r + 1).min(w) {
                    let j = rr * w + cc;
                    if height.data[j].is_finite() {
                        n += 1;
                        t += u32::from(tree[j]);
                    }
                }
            }
            out[i] = 100.0 * t as f32 / n as f32;
        }
    }
    height.with_data(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn url_matches_terrain_tile_request() {
        let t = TileId { kx: 998, ky: 6426 };
        let mnt = t.url();
        let mnh = mnh_url(t);
        assert_eq!(
            mnh,
            mnt.replace(dem::LAYER, MNH_LAYER)
                .replace("_MNT_O_", "_MNH_O_")
        );
    }

    #[test]
    fn cover_is_share_of_tall_cells() {
        let mut hgt = Raster {
            w: 4,
            h: 3,
            cell: 5.0,
            origin_x: 0.0,
            origin_y: 0.0,
            data: vec![0.0f32; 12],
        };
        hgt.data[5] = 10.0; // row 1, col 1
        hgt.data[6] = 2.9; // below the tree height
        hgt.data[3] = f32::NAN;
        let dem = hgt.with_data(vec![1000.0f32; 12]);
        let c = canopy_cover(&hgt, &dem);
        // Corner (0, 0): window of 4 cells, one tree.
        assert!((c.data[0] - 25.0).abs() < 1e-6);
        // Centre (1, 1): 9 cells, one tree.
        assert!((c.data[5] - 100.0 / 9.0).abs() < 1e-5);
        // Unknown height stays unknown; (2, 0) sees 5 known cells (one is NaN), 1 tree.
        assert!(c.data[3].is_nan());
        assert!((c.data[2] - 20.0).abs() < 1e-5);
        // Column 3 never sees the tree.
        assert_eq!(c.data[11], 0.0);
    }
}
