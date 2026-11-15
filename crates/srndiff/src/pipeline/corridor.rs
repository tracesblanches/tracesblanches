//! Terrain, canopy and avalanche exposure along a narrow corridor around a track.
//!
//! The track is cut into chunks of [`Corridor::chunk_m`]. For each chunk, only the 1 km
//! LiDAR HD tiles within [`Corridor::exposure_margin_m`] of a sample are fetched (terrain
//! model and canopy height model, `skimap_terrain::dem::fetch_tile` and
//! `skimap_avalanche::exposure::forest::fetch_mnh`, shared downloader and cache), never
//! the whole bounding box. The avalanche exposure model of the avalanche crate
//! (`skimap_avalanche::exposure::model`, AutoATES v3.0 port, P17) runs on that mosaic,
//! unless the chunk lies in the exposure layer of a site (Cervières, Chapelle-des-Bois),
//! which also merges the CLPA footprints and is what the phone reads.
//!
//! Limits: release areas farther than the margin are not seen (a runout from more than
//! 1.5 km away needs a drop of about 500 m at α = 18°); the model is a model, never a
//! guarantee.

use std::collections::BTreeSet;
use std::time::Instant;

use serde::{Deserialize, Serialize};
use skimap_avalanche::exposure::{self, forest, raster::Raster};
use skimap_common::crs::Crs;
use skimap_common::download::Downloader;
use skimap_common::geom::LonLat;
use skimap_common::grid::{GeoTransform, Grid};
use skimap_common::layout::DataLayout;
use skimap_common::site::Site;
use skimap_terrain::dem::{self, Extent, TileId};
use skimap_zones::chrono::{DateTime, Utc};
use skimap_zones::layer::ZoneLayer;
use skimap_zones::rules::RuleSet;
use tracing::{debug, warn};

use crate::field::{Areas, Bilinear, Classes, Field, Nearest, SiteZones};
use crate::profile::{self, Inputs, Layers, Profile, STEP_M};
use crate::{Error, Result};

/// Corridor parameters.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Corridor {
    /// Chunk length along the track, metres.
    pub chunk_m: f64,
    /// Margin of the terrain fetched for the avalanche model, metres.
    pub exposure_margin_m: f64,
    /// Margin of the terrain and canopy when the exposure comes from a site layer, metres.
    pub near_margin_m: f64,
}

impl Default for Corridor {
    fn default() -> Self {
        Self {
            chunk_m: 5000.0,
            exposure_margin_m: 1500.0,
            near_margin_m: 50.0,
        }
    }
}

/// Layers of one evaluation site, read from `data/out/<site>/`.
pub struct SiteLayers {
    /// Site.
    pub site: &'static Site,
    /// Exposure classes (`offpiste/exposure_5m.skr`, CLPA merged).
    pub exposure: Option<Grid<u8>>,
    /// Zones without any network and the analysed area (`coverage`).
    pub no_network: Option<Areas>,
    /// Zones and rules (`zones`).
    pub zones: Option<(ZoneLayer, RuleSet)>,
}

impl SiteLayers {
    /// Reads what exists.
    pub fn load(layout: &DataLayout, site: &'static Site) -> Result<Self> {
        let dir = |l: &str| {
            layout
                .layer_dir(site, l)
                .map_err(|e| Error::Data(e.to_string()))
        };
        let exp_path = dir("offpiste")?.join(skimap_core::offpiste::FILE_EXPOSURE);
        let exposure = if exp_path.exists() {
            Some(
                skimap_core::offpiste::read_raster::<u8>(&exp_path)
                    .map_err(|e| Error::Data(e.to_string()))?,
            )
        } else {
            None
        };
        let cov = dir("coverage")?;
        let read = |p: std::path::PathBuf| std::fs::read_to_string(&p).ok();
        let no_network = match (
            read(cov.join("no_network.geojson")),
            read(cov.join("analysed_area.geojson")),
        ) {
            (Some(n), Some(a)) => Some(Areas::new(
                Areas::polygons_from_geojson(&n).map_err(Error::Data)?,
                Some(Areas::polygons_from_geojson(&a).map_err(Error::Data)?),
            )),
            _ => None,
        };
        let z = dir("zones")?;
        let zones = match (read(z.join("zones.geojson")), read(z.join("rules.json"))) {
            (Some(l), Some(r)) => Some((
                ZoneLayer::from_geojson_str(&l).map_err(|e| Error::Data(e.to_string()))?,
                RuleSet::from_json_str(&r).map_err(|e| Error::Data(e.to_string()))?,
            )),
            _ => None,
        };
        Ok(Self {
            site,
            exposure,
            no_network,
            zones,
        })
    }
}

/// What building one profile cost.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct CorridorStats {
    /// Chunks.
    pub chunks: usize,
    /// Distinct terrain tiles read (km²).
    pub dem_tiles: usize,
    /// Distinct canopy tiles read.
    pub mnh_tiles: usize,
    /// Chunks whose exposure came from the model.
    pub model_chunks: usize,
    /// Cells given to the model.
    pub model_cells: usize,
    /// Seconds fetching and decoding tiles (cache or network).
    pub fetch_s: f64,
    /// Seconds in the avalanche model.
    pub model_s: f64,
    /// Seconds sampling the layers.
    pub sample_s: f64,
}

fn tile_extent(t: TileId) -> Extent {
    let (x0, y1) = t.origin();
    Extent {
        min_x: x0,
        min_y: y1 - 1000.0,
        max_x: x0 + 1000.0,
        max_y: y1,
    }
}

fn rect_dist(e: &Extent, x: f64, y: f64) -> f64 {
    let dx = (e.min_x - x).max(0.0).max(x - e.max_x);
    let dy = (e.min_y - y).max(0.0).max(y - e.max_y);
    dx.hypot(dy)
}

/// Tiles within `margin` of a point of `pts`.
fn tiles_near(pts: &[(f64, f64)], margin: f64) -> BTreeSet<TileId> {
    let mut out = BTreeSet::new();
    for &(x, y) in pts {
        for t in dem::tiles_for(&Extent::around(x, y, margin)) {
            if rect_dist(&tile_extent(t), x, y) <= margin {
                out.insert(t);
            }
        }
    }
    out
}

/// Mosaic of tiles (`NaN` where a tile is missing).
fn mosaic(
    tiles: &BTreeSet<TileId>,
    mut fetch: impl FnMut(TileId) -> Option<Grid<f32>>,
) -> Option<Grid<f32>> {
    let kx0 = tiles.iter().map(|t| t.kx).min()?;
    let kx1 = tiles.iter().map(|t| t.kx).max()?;
    let ky0 = tiles.iter().map(|t| t.ky).min()?;
    let ky1 = tiles.iter().map(|t| t.ky).max()?;
    let nx = usize::try_from(kx1 - kx0 + 1).ok()?;
    let ny = usize::try_from(ky1 - ky0 + 1).ok()?;
    let (w, h) = (nx * dem::TILE_PX, ny * dem::TILE_PX);
    let origin = TileId { kx: kx0, ky: ky1 }.origin();
    let gt = GeoTransform::north_up(origin.0, origin.1, dem::CELL);
    let mut g = Grid::filled(w, h, gt, Crs::Lambert93, f32::NAN);
    for t in tiles {
        let Some(tile) = fetch(*t) else { continue };
        if tile.width() != dem::TILE_PX || tile.height() != dem::TILE_PX {
            warn!(tile = %t.name(), "unexpected tile size, skipped");
            continue;
        }
        let c0 = usize::try_from(t.kx - kx0).ok()? * dem::TILE_PX;
        let r0 = usize::try_from(ky1 - t.ky).ok()? * dem::TILE_PX;
        let data = g.data_mut();
        for r in 0..dem::TILE_PX {
            let src = &tile.data()[r * dem::TILE_PX..(r + 1) * dem::TILE_PX];
            data[(r0 + r) * w + c0..(r0 + r) * w + c0 + dem::TILE_PX].copy_from_slice(src);
        }
    }
    Some(g)
}

/// Uses the first field that has a value.
struct FirstOf<'a>(Vec<&'a dyn Field>);

impl Field for FirstOf<'_> {
    fn at(&self, x: f64, y: f64) -> Option<f32> {
        self.0.iter().find_map(|f| f.at(x, y))
    }
}

fn raster_u8_to_grid(r: &Raster<u8>) -> Grid<u8> {
    Grid::from_vec(
        r.w,
        r.h,
        GeoTransform::north_up(r.origin_x, r.origin_y, r.cell),
        Crs::Lambert93,
        r.data.clone(),
    )
    .expect("raster size")
}

/// Builds the profile of a track along a corridor. `roads`: exit points (Lambert-93) or
/// `None` when unknown; `at`: instant for the zone rules.
pub fn build_profile(
    track: &[LonLat],
    dl: &Downloader,
    sites: &[SiteLayers],
    at: DateTime<Utc>,
    roads: Option<&[(f64, f64)]>,
    c: &Corridor,
) -> Result<(Profile, CorridorStats)> {
    let mut samples = profile::resample(track)?;
    let mut st = CorridorStats::default();
    let mut dem_seen = BTreeSet::new();
    let mut mnh_seen = BTreeSet::new();
    let per = ((c.chunk_m / STEP_M).round() as usize).max(2);
    let mut i0 = 0;
    while i0 < samples.len() {
        let i1 = (i0 + per).min(samples.len());
        st.chunks += 1;
        let pts: Vec<(f64, f64)> = samples[i0..i1].iter().map(|s| (s.x, s.y)).collect();
        // Exposure from a site layer when every sample of the chunk has a value there.
        let site_exp: Option<&Grid<u8>> = sites.iter().find_map(|s| {
            let g = s.exposure.as_ref()?;
            pts.iter()
                .all(|(x, y)| g.value_at(*x, *y).is_some_and(|v| *v != 255))
                .then_some(g)
        });
        let margin = if site_exp.is_some() {
            c.near_margin_m
        } else {
            c.exposure_margin_m
        };
        let dem_tiles = tiles_near(&pts, margin);
        // Same tiles for the canopy: the model needs it on the whole mosaic, the profile
        // only near the track.
        let mnh_tiles = &dem_tiles;
        let t0 = Instant::now();
        let mut dem_grid = mosaic(&dem_tiles, |t| match dem::fetch_tile(t, dl) {
            Ok(g) => Some(g),
            Err(e) => {
                warn!(tile = %t.name(), error = %e, "terrain tile unavailable");
                None
            }
        })
        .ok_or_else(|| Error::Data("empty corridor".into()))?;
        // Fill value of the WMS outside the LiDAR HD coverage, and its mixing along the
        // coverage edge (terrain crate).
        for v in dem_grid.data_mut() {
            if !(v.is_finite() && *v > -1000.0) {
                *v = f32::NAN;
            }
        }
        dem::erode_valid(&mut dem_grid, dem::NODATA_MARGIN);
        let mnh_grid = mosaic(mnh_tiles, |t| {
            match forest::fetch_mnh(&tile_extent(t), dl) {
                Ok(g) => Some(g),
                Err(e) => {
                    warn!(tile = %t.name(), error = %e, "canopy tile unavailable");
                    None
                }
            }
        })
        .ok_or_else(|| Error::Data("empty corridor".into()))?;
        st.fetch_s += t0.elapsed().as_secs_f64();
        dem_seen.extend(dem_tiles.iter().copied());
        mnh_seen.extend(mnh_tiles.iter().copied());

        let model_exp: Option<Grid<u8>> = if site_exp.is_none() {
            let t0 = Instant::now();
            let dem_r = Raster::from_grid(&dem_grid);
            let canopy = forest::canopy_cover(&Raster::from_grid(&mnh_grid), &dem_r);
            st.model_cells += dem_r.w * dem_r.h;
            let m = exposure::model(&exposure::Inputs { dem: dem_r, canopy });
            st.model_s += t0.elapsed().as_secs_f64();
            st.model_chunks += 1;
            debug!(cells = st.model_cells, s = st.model_s, "exposure model");
            Some(raster_u8_to_grid(&m.exposure))
        } else {
            None
        };
        let t0 = Instant::now();
        let elev = Bilinear(&dem_grid);
        let canopy = Nearest(&mnh_grid);
        let exp_site = site_exp.map(|g| Classes {
            grid: g,
            nodata: 255,
        });
        let exp_model = model_exp.as_ref().map(|g| Classes {
            grid: g,
            nodata: 255,
        });
        let mut exp_fields: Vec<&dyn Field> = Vec::new();
        if let Some(e) = &exp_site {
            exp_fields.push(e);
        }
        if let Some(e) = &exp_model {
            exp_fields.push(e);
        }
        let exp = FirstOf(exp_fields);
        profile::fill(
            &mut samples[i0..i1],
            &Layers {
                elevation: Some(&elev),
                canopy_height: Some(&canopy),
                exposure: Some(&exp),
                ..Layers::default()
            },
        );
        st.sample_s += t0.elapsed().as_secs_f64();
        i0 = i1;
    }
    // Network and zones of the sites, for the samples inside a site box.
    let t0 = Instant::now();
    let (mut net, mut zon) = (false, false);
    for s in sites {
        let inside: Vec<usize> = (0..samples.len())
            .filter(|i| {
                s.site
                    .bbox
                    .contains(LonLat::new(samples[*i].lon, samples[*i].lat))
            })
            .collect();
        if inside.is_empty() {
            continue;
        }
        let zs = s.zones.as_ref().map(|(l, r)| SiteZones {
            layer: l,
            rules: r,
            at,
        });
        net |= s.no_network.is_some();
        zon |= zs.is_some();
        let layers = Layers {
            no_network: s.no_network.as_ref(),
            zones: zs.as_ref().map(|z| z as &dyn crate::field::ZoneSource),
            ..Layers::default()
        };
        for i in inside {
            profile::fill(&mut samples[i..=i], &layers);
        }
    }
    profile::mark_exits(&mut samples, roads.unwrap_or(&[]));
    st.sample_s += t0.elapsed().as_secs_f64();
    st.dem_tiles = dem_seen.len();
    st.mnh_tiles = mnh_seen.len();
    Ok((
        Profile {
            step_m: STEP_M,
            samples,
            inputs: Inputs {
                canopy: true,
                exposure: true,
                network: net,
                zones: zon,
                roads: roads.is_some(),
            },
        },
        st,
    ))
}
