//! IGN elevation per site: slope and aspect.
//!
//! Owned ticket P5 (#18): download the IGN LiDAR HD terrain model at 5 m over a site,
//! compute slope and aspect, compare with the IGN `SLOPES.MOUNTAIN` layer.
//!
//! - [`dem`] — LiDAR HD tiles from the Géoplateforme WMS-R, mosaic, crop, block mean.
//! - [`slope`] — Horn slope and aspect.
//! - [`ign_slopes`] — comparison with the IGN slope map (WMTS).
//! - [`png`] — PNG decoder for WMTS tiles.
//!
//! GeoTIFF reading / writing and the `<layer>.meta.json` schema are in `skimap-common`
//! (`geotiff`, `meta`).
//!
//! [`build`] writes `data/out/<site>/terrain/`:
//! `dem_5m.tif`, `slope_5m.tif`, `aspect_5m.tif` (GeoTIFF Float32, Lambert-93
//! EPSG:2154, 5 m, `NaN` no-data), `slope_ign_comparison.json`, `terrain.meta.json`.
//!
//! Pipeline side: uses `skimap_common::download`.

pub mod dem;
pub mod error;
pub mod ign_slopes;
pub mod png;
pub mod slope;

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::LazyLock;
use std::time::Instant;

use serde::{Deserialize, Serialize};
use skimap_common::download::Downloader;
use skimap_common::layout::DataLayout;
use skimap_common::site::Site;
use tracing::info;

pub use dem::Extent;
pub use error::{Error, Result};
/// Float32 GeoTIFF reader / writer, moved to `skimap_common::geotiff`; re-exported for the
/// callers written before the move (`skimap-core` example `offpiste_bench`).
pub use skimap_common::geotiff;
use skimap_common::meta::{FileMeta, LayerMeta, SourceRef};

/// Layer name (output directory).
pub const LAYER: &str = "terrain";
/// WMTS zoom used for the comparison with the IGN slope map (≈ 3.4 m pixels at 45° N).
pub const COMPARISON_ZOOM: u8 = 15;

/// Reference of research report 07 for one site (`data/report07.json`).
#[derive(Debug, Clone, Deserialize)]
struct Report07Site {
    /// First and last tile, Lambert-93 kilometres, in x.
    tiles_x: [i64; 2],
    /// First and last tile, Lambert-93 kilometres, in y.
    tiles_y: [i64; 2],
    /// Steep-terrain share (≥ 30°) published by the report.
    steep30: f64,
    /// Flight and edition dates of the LiDAR HD tiles.
    lidar: String,
}

#[derive(Deserialize)]
struct Report07Table {
    sites: BTreeMap<String, Report07Site>,
}

static REPORT07: LazyLock<BTreeMap<String, Report07Site>> = LazyLock::new(|| {
    serde_json::from_str::<Report07Table>(include_str!("../data/report07.json"))
        .expect("data/report07.json")
        .sites
});

/// Steep-terrain share (≥ 30°) published by research report 07 and the Lambert-93 extent
/// of the tile mosaic it was computed on (5 m LiDAR HD, `numpy.gradient` central
/// differences, missing tiles excluded), from the table `data/report07.json`; `None` for
/// a site the report did not measure. Used as a cross-check.
pub fn report07_reference(site: &Site) -> Option<(Extent, f64)> {
    let r = REPORT07.get(site.id)?;
    let ([kx0, kx1], [ky0, ky1]) = (r.tiles_x, r.tiles_y);
    let mosaic = Extent {
        min_x: kx0 as f64 * 1000.0 - dem::TILE_SHIFT,
        min_y: (ky0 - 1) as f64 * 1000.0 + dem::TILE_SHIFT,
        max_x: (kx1 + 1) as f64 * 1000.0 - dem::TILE_SHIFT,
        max_y: ky1 as f64 * 1000.0 + dem::TILE_SHIFT,
    };
    Some((mosaic, r.steep30))
}

/// Cross-check against report 07.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Report07Check {
    /// Lambert-93 extent of the report's tile mosaic.
    pub area: Extent,
    /// Share ≥ 30° published by the report.
    pub report: f64,
    /// Share ≥ 30° of our slope over the same extent (all tiles, Horn operator).
    pub ours: f64,
}

/// Summary of a terrain build.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TerrainBuild {
    /// Site identifier.
    pub site: String,
    /// Lambert-93 extent of the site box.
    pub extent: Extent,
    /// LiDAR HD tiles used.
    pub tiles: usize,
    /// Terrain model size, cells.
    pub size_cells: [usize; 2],
    /// Share of cells with an elevation (LiDAR HD covers France only).
    pub valid_fraction: f64,
    /// Elevation range, metres.
    pub elevation_range_m: [f32; 2],
    /// Share of valid slope cells ≥ 30° over the whole site box.
    pub steep30_fraction: f64,
    /// Comparison with IGN `SLOPES.MOUNTAIN`.
    pub ign_comparison: ign_slopes::SlopeComparison,
    /// Cross-check against research report 07.
    pub report07: Option<Report07Check>,
    /// Files written.
    pub files: Vec<PathBuf>,
    /// Seconds spent: terrain model (download or cache), slope, writing, comparison.
    pub seconds: Timings,
}

/// Wall-clock timings of a build, seconds.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Timings {
    /// Fetching (or reading from cache) and mosaicking the tiles.
    pub dem: f64,
    /// Slope and aspect.
    pub slope: f64,
    /// Writing the GeoTIFFs.
    pub write: f64,
    /// Comparison with the IGN slope map.
    pub comparison: f64,
}

/// Flight and edition dates of the LiDAR HD tiles of a site, when research report 07
/// read them (`data/report07.json`).
pub fn lidar_flights(site: &Site) -> Option<&'static str> {
    REPORT07.get(site.id).map(|r| r.lidar.as_str())
}

/// Sources of the terrain layer of a site.
pub fn sources(site: &Site) -> Vec<SourceRef> {
    let edition = lidar_flights(site).map_or_else(
        || "vol et édition propres à chaque dalle (métadonnées IGN LiDAR HD)".to_owned(),
        |l| format!("{l} (rapport 07)"),
    );
    vec![
        SourceRef::new("MNT LiDAR HD (5 m, rééchantillonné par le WMS)", format!("{}?LAYERS={}", dem::WMS_URL, dem::LAYER), "Licence Ouverte 2.0 (Etalab)", "IGN – MNT LiDAR HD")
.producer("IGN")
.edition(edition),
        SourceRef::new("Carte des pentes (SLOPES.MOUNTAIN), comparaison seulement", format!("https://data.geopf.fr/wmts?LAYER={}", ign_slopes::LAYER), "Licence Ouverte 2.0 (Etalab) : couche sans licence renseignée, les CGU de cartes.gouv.fr appliquent alors la Licence Ouverte (« A défaut, la Licence Ouverte / Open Licence – Etalab s'applique ») ; utilisée ici pour la comparaison seulement", "IGN – Carte des pentes")
.producer("IGN"),
    ]
}

/// Builds the terrain layer of a site: terrain model, slope, aspect, IGN comparison.
pub fn build(site: &Site, layout: &DataLayout, dl: &Downloader) -> Result<TerrainBuild> {
    let dir = layout.layer_dir(site, LAYER)?;
    DataLayout::ensure(&dir)?;
    let extent = Extent::from_bbox(site.bbox)?;
    let mut t = Timings::default();

    let t0 = Instant::now();
    let tiles = dem::tiles_for(&extent).len();
    let dem = dem::fetch_dem(&extent, dl)?;
    t.dem = t0.elapsed().as_secs_f64();
    info!(
        site = site.id,
        tiles,
        w = dem.width(),
        h = dem.height(),
        s = t.dem,
        "terrain model ready"
    );

    let t0 = Instant::now();
    let sa = slope::horn(&dem);
    t.slope = t0.elapsed().as_secs_f64();

    let t0 = Instant::now();
    let mut files = Vec::new();
    let mut fmeta = Vec::new();
    for (name, grid, desc, unit) in [
        (
            "dem_5m.tif",
            &dem,
            "Altitude du terrain (MNT LiDAR HD), IGN69",
            "m",
        ),
        ("slope_5m.tif", &sa.slope, "Pente (Horn 3×3)", "degré"),
        (
            "aspect_5m.tif",
            &sa.aspect,
            "Orientation : direction vers laquelle la pente descend, degrés depuis le nord dans le sens horaire ; -1 = plat",
            "degré",
        ),
    ] {
        let path = dir.join(name);
        let bytes = geotiff::write_f32(&path, grid)?;
        fmeta.push(FileMeta::geotiff_f32(name, desc, unit, grid, bytes));
        files.push(path);
    }
    t.write = t0.elapsed().as_secs_f64();

    let valid = dem.data().iter().filter(|v| v.is_finite()).count();
    let (lo, hi) = dem
        .data()
        .iter()
        .filter(|v| v.is_finite())
        .fold((f32::INFINITY, f32::NEG_INFINITY), |(a, b), v| {
            (a.min(*v), b.max(*v))
        });
    let all = (extent.min_x, extent.min_y, extent.max_x, extent.max_y);
    let (steep, _) = slope::fraction_at_least(&sa.slope, 30.0, all);
    let report07 = match report07_reference(site) {
        Some((area, report)) => {
            let e = (area.min_x, area.min_y, area.max_x, area.max_y);
            let (ours, _) = slope::fraction_at_least(&sa.slope, 30.0, e);
            Some(Report07Check { area, report, ours })
        }
        None => None,
    };

    let t0 = Instant::now();
    let cmp = ign_slopes::compare(site.bbox, &sa.slope, dl, COMPARISON_ZOOM)?;
    t.comparison = t0.elapsed().as_secs_f64();
    let cmp_path = dir.join("slope_ign_comparison.json");
    skimap_common::output::write_json(&cmp_path, &cmp)?;
    files.push(cmp_path);

    let mut fmeta_cmp = FileMeta::new(
        "slope_ign_comparison.json",
        "comparaison de la pente à la carte des pentes IGN",
    );
    fmeta_cmp.format = Some("JSON".into());
    fmeta.push(fmeta_cmp);
    let meta = LayerMeta {
        tickets: vec!["P5 (#18)".into()],
        validity: Some("relief permanent (date des levés LiDAR HD, voir sources)".into()),
        crs: Some("EPSG:2154 (Lambert-93), altitudes IGN69".into()),
        sources: sources(site),
        files: fmeta,
        notes: vec![
            "Grille Lambert-93 de 5 m dont l'origine est décalée de 0,25 m (grille des dalles LiDAR HD).".into(),
            "Le WMS rééchantillonne les dalles de 50 cm à 5 m (écart moyen de 0,11 m à la moyenne par blocs de 10 × 10 sur la dalle 0998-6426).".into(),
            "Hors de France (Italie, Suisse) et dans les zones masquées (ZICAD), pas de donnée : NaN.".into(),
            "Pente et orientation : opérateur de Horn sur 3 × 3 cellules ; bordures et voisins NaN donnent NaN.".into(),
            format!(
                "Comparaison à la carte des pentes IGN (zoom {COMPARISON_ZOOM}) dans slope_ign_comparison.json : la carte IGN vient du RGE ALTI 5 m, la nôtre du LiDAR HD."
            ),
        ],
        ..LayerMeta::new(LAYER, site.id)
    };
    files.push(meta.write(&dir)?);

    Ok(TerrainBuild {
        site: site.id.into(),
        extent,
        tiles,
        size_cells: [dem.width(), dem.height()],
        valid_fraction: valid as f64 / dem.data().len() as f64,
        elevation_range_m: [lo, hi],
        steep30_fraction: steep,
        ign_comparison: cmp,
        report07,
        files,
        seconds: t,
    })
}

#[cfg(test)]
mod tests {
    use skimap_common::site::SITES;

    #[test]
    fn report07_areas_are_inside_sites() {
        for s in SITES.iter() {
            let (a, f) = super::report07_reference(s).unwrap();
            let e = super::Extent::from_bbox(s.bbox).unwrap();
            assert!(
                a.min_x > e.min_x && a.max_x < e.max_x && a.min_y > e.min_y && a.max_y < e.max_y
            );
            assert!(f > 0.0 && f < 1.0);
        }
    }
}
