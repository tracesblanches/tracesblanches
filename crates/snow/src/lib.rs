//! Estimated snow cover map.
//!
//! Owned tickets:
//! - P6 (#19): per pixel (massif, elevation, aspect, slope), snow depth interpolated from the
//!   Météo-France snowpack simulation (J-1); three classes per activity plus a range.
//! - P7 (#20): validation against Météo-France stations (Cervières 1614 m, Arvieux, Mouthe,
//!   La Boissaude) over past seasons.
//!
//! - [`massifs`] — SAFRAN massif contours (CNRM `snowtools`), rasterisation.
//! - [`sim`] — yearly simulation files, per-pixel interpolation and range.
//! - [`map`] — 25 m terrain (from the 5 m LiDAR HD of `skimap-terrain`) and projection.
//! - [`classes`] — three classes per practice.
//! - [`stations`], [`validate`] — station data and validation.
//!
//! [`build`] writes `data/out/<site>/snow/` (GeoTIFF Float32, Lambert-93 EPSG:2154, 25 m,
//! `NaN` no-data): `snow_depth.tif`, `snow_depth_low.tif`, `snow_depth_high.tif` (cm),
//! `class_groomed.tif`, `class_srn.tif` (0 insuffisant, 1 limite, 2 suffisant),
//! `massif.tif` (massif number) and `snow.meta.json`. [`validate::validate`] writes
//! `validation.json`.
//!
//! Not modelled (stated in the metadata): wind redistribution, forest interception and
//! shading, grooming compaction, artificial snow; the simulation is per massif and
//! altitude band, not a local measurement.

pub mod classes;
pub mod error;
pub mod map;
pub mod massifs;
pub mod sim;
pub mod stations;
pub mod validate;

use std::path::PathBuf;
use std::time::Instant;

use chrono::{Datelike, NaiveDate};
use serde::{Deserialize, Serialize};
use skimap_common::download::Downloader;
use skimap_common::geotiff;
use skimap_common::layout::DataLayout;
use skimap_common::meta::{FileMeta, LayerMeta, SourceRef};
use skimap_common::site::Site;
use skimap_terrain::Extent;
use tracing::info;

pub use error::{Error, Result};

/// Layer name (output directory).
pub const LAYER: &str = "snow";

/// Cells per massif in the map.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MassifCells {
    /// Massif number.
    pub code: u16,
    /// Massif name.
    pub name: String,
    /// Cells of the map attached to it.
    pub cells: usize,
}

/// Summary of a snow build.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SnowBuild {
    /// Site identifier.
    pub site: String,
    /// Simulation date (state at 06 UTC).
    pub date: NaiveDate,
    /// Map size, cells.
    pub size_cells: [usize; 2],
    /// Massifs of the map.
    pub massifs: Vec<MassifCells>,
    /// Cells outside every massif polygon given the nearest massif.
    pub cells_filled_by_nearest_massif: usize,
    /// Cells with an estimate.
    pub cells_estimated: usize,
    /// Mean, minimum and maximum estimated depth, cm.
    pub depth_mean_min_max_cm: [f32; 3],
    /// Share of estimated cells per class (0, 1, 2) and practice.
    pub class_shares: Vec<(String, [f64; 3])>,
    /// Files written and their sizes (bytes).
    pub files: Vec<(PathBuf, u64)>,
    /// Wall-clock timings, seconds.
    pub seconds: Timings,
}

/// Wall-clock timings of a build, seconds.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Timings {
    /// Massif contours (cache or download) and parse.
    pub massifs: f64,
    /// 25 m terrain: LiDAR HD tiles (cache or download), block mean, slope, massif raster.
    pub terrain: f64,
    /// Simulation file (revalidation or download), decompression and parse.
    pub simulation: f64,
    /// Per-pixel projection and range.
    pub estimate: f64,
    /// Classes and GeoTIFF / metadata writing.
    pub write: f64,
    /// Whole build.
    pub total: f64,
}

/// Sources of the snow layer.
pub fn sources() -> Vec<SourceRef> {
    vec![
        SourceRef::new(
            "Données de Modèle de Simulation Nivologique",
            sim::DATASET_URL,
            "Licence Ouverte 2.0 (Etalab)",
            "Météo-France – simulation nivologique",
        )
        .producer("Météo-France"),
        SourceRef::new(
            "Contours des massifs (snowtools, massifs.shp)",
            massifs::url("shp"),
            "CeCILL-C (dépôt UMR-CNRM/snowtools)",
            "Météo-France CNRM – snowtools",
        )
        .producer("Météo-France / CNRS, CNRM (CEN)")
        .edition(format!("commit {}", massifs::SNOWTOOLS_COMMIT)),
        SourceRef::new(
            "MNT LiDAR HD (5 m, agrégé à 25 m)",
            format!(
                "{}?LAYERS={}",
                skimap_terrain::dem::WMS_URL,
                skimap_terrain::dem::LAYER
            ),
            "Licence Ouverte 2.0 (Etalab)",
            "IGN – MNT LiDAR HD",
        )
        .producer("IGN"),
    ]
}

/// Notes of the metadata (method and limits), in French.
pub fn notes() -> Vec<String> {
    let mut v = vec![
        "Hauteur de neige naturelle estimée : simulation Météo-France du massif (état à 06 h UTC), interpolée par pixel de 25 m : linéaire en altitude entre bandes de 300 m, en orientation entre les deux octants voisins, en pente entre plat, 20° et 40° (au-delà de 40° : valeur à 40°).".to_owned(),
        "Relief : moyenne 5 × 5 du MNT LiDAR HD 5 m ; pente et orientation de Horn à 25 m. Pixel sans pente (bord, voisin sans donnée) traité comme plat.".to_owned(),
        "Massif : polygone snowtools contenant le centre du pixel ; un pixel hors de tout polygone reçoit le massif du pixel le plus proche.".to_owned(),
        format!(
            "Fourchette (snow_depth_low / snow_depth_high) : minimum et maximum de la même interpolation sur altitude ± {} m, pente ± {}° et orientation ± {}°. C'est la sensibilité au relief local, pas l'erreur du modèle (voir validation.json).",
            sim::RANGE_ALT_M, sim::RANGE_SLOPE_DEG, sim::RANGE_ASPECT_DEG
        ),
        "NON MODÉLISÉ : transport de neige par le vent (corniches, crêtes soufflées, accumulations), interception et ombre de la forêt, tassement par le damage, neige de culture. La simulation est une valeur de massif par bande d'altitude, pas une mesure locale.".to_owned(),
    ];
    for p in classes::PRACTICES {
        v.push(format!(
            "Classes {} ({}) : 0 {} < {} cm ≤ 1 {} < {} cm ≤ 2 {} ; seuils proposés, à valider sur le terrain (T5).",
            p.id,
            p.label,
            classes::CLASS_LABELS[0],
            p.thresholds_cm[0],
            classes::CLASS_LABELS[1],
            p.thresholds_cm[1],
            classes::CLASS_LABELS[2]
        ));
    }
    v
}

/// Builds the snow layer of a site for `date` (state at 06 UTC), or for the latest date of
/// the current-year simulation file when `date` is `None` (J-1).
pub fn build(
    site: &Site,
    layout: &DataLayout,
    dl: &Downloader,
    date: Option<NaiveDate>,
) -> Result<SnowBuild> {
    let t_all = Instant::now();
    let mut t = Timings::default();
    let dir = layout.layer_dir(site, LAYER)?;
    DataLayout::ensure(&dir)?;

    let t0 = Instant::now();
    let all_massifs = massifs::fetch(dl)?;
    t.massifs = t0.elapsed().as_secs_f64();

    let t0 = Instant::now();
    let extent = Extent::from_bbox(site.bbox)?;
    let terrain = map::snow_terrain(&extent, dl, &all_massifs)?;
    let codes = terrain.massif_codes();
    t.terrain = t0.elapsed().as_secs_f64();
    info!(site = site.id, ?codes, s = t.terrain, "snow terrain ready");

    let t0 = Instant::now();
    let year = date.map_or_else(|| chrono::Utc::now().year(), |d| d.year());
    let current = year == chrono::Utc::now().year();
    let gz = sim::fetch_year(dl, year, current)?;
    let simulation = sim::Simulation::parse_gz(&gz, &codes, |d| date.is_none_or(|x| x == d))?;
    let date = match date {
        Some(d) => d,
        None => simulation.latest_common_date(&codes).ok_or_else(|| {
            Error::Missing(format!(
                "no simulation date for massifs {codes:?} in {year}"
            ))
        })?,
    };
    for c in &codes {
        if simulation.get(*c, date).is_none() {
            return Err(Error::Missing(format!(
                "simulation of massif {c} on {date}"
            )));
        }
    }
    t.simulation = t0.elapsed().as_secs_f64();

    let t0 = Instant::now();
    let maps = map::estimate(&terrain, |c| simulation.get(c, date));
    t.estimate = t0.elapsed().as_secs_f64();

    let t0 = Instant::now();
    let massif_grid = terrain
        .massif
        .map(|c| if *c == 0 { f32::NAN } else { f32::from(*c) });
    let class_grids: Vec<_> = classes::PRACTICES
        .iter()
        .map(|p| (p, map::classify(&maps.depth, p)))
        .collect();
    let mut outputs: Vec<(String, &skimap_common::grid::Grid<f32>, String, &str)> = vec![
        (
            "snow_depth.tif".into(),
            &maps.depth,
            "Hauteur de neige estimée (valeur centrale)".into(),
            "cm",
        ),
        (
            "snow_depth_low.tif".into(),
            &maps.low,
            "Borne basse de la fourchette".into(),
            "cm",
        ),
        (
            "snow_depth_high.tif".into(),
            &maps.high,
            "Borne haute de la fourchette".into(),
            "cm",
        ),
    ];
    for (p, g) in &class_grids {
        outputs.push((
            format!("class_{}.tif", p.id),
            g,
            format!(
                "Classe {} : 0 insuffisant (< {} cm), 1 limite, 2 suffisant (≥ {} cm)",
                p.label, p.thresholds_cm[0], p.thresholds_cm[1]
            ),
            "classe",
        ));
    }
    outputs.push((
        "massif.tif".into(),
        &massif_grid,
        "Numéro du massif Météo-France utilisé".into(),
        "numéro",
    ));
    let mut files = Vec::new();
    let mut fmeta = Vec::new();
    for (name, g, desc, unit) in outputs {
        let path = dir.join(&name);
        let bytes = geotiff::write_f32(&path, g)?;
        fmeta.push(FileMeta::geotiff_f32(name, desc, unit, g, bytes));
        files.push((path, bytes));
    }
    let meta = LayerMeta {
        tickets: vec!["P6 (#19)".into()],
        valid_from: Some(format!("{date}T06:00:00Z")),
        validity: Some(format!(
            "{date} 06:00 UTC (simulation du jour J, publiée J+1)"
        )),
        crs: Some("EPSG:2154 (Lambert-93)".into()),
        sources: sources(),
        files: fmeta,
        notes: notes(),
        ..LayerMeta::new(LAYER, site.id)
    };
    let meta_path = meta.write(&dir)?;
    let meta_bytes = std::fs::metadata(&meta_path)
        .map_err(|e| Error::io(&meta_path, e))?
        .len();
    files.push((meta_path, meta_bytes));
    t.write = t0.elapsed().as_secs_f64();

    let est: Vec<f32> = maps
        .depth
        .data()
        .iter()
        .copied()
        .filter(|v| v.is_finite())
        .collect();
    let n = est.len();
    let mean = est.iter().map(|v| f64::from(*v)).sum::<f64>() / n.max(1) as f64;
    let (lo, hi) = est
        .iter()
        .fold((f32::INFINITY, f32::NEG_INFINITY), |(a, b), v| {
            (a.min(*v), b.max(*v))
        });
    let class_shares = class_grids
        .iter()
        .map(|(p, g)| {
            let mut k = [0usize; 3];
            for v in g.data().iter().filter(|v| v.is_finite()) {
                k[*v as usize] += 1;
            }
            let tot = k.iter().sum::<usize>().max(1) as f64;
            (
                p.id.to_owned(),
                [k[0] as f64 / tot, k[1] as f64 / tot, k[2] as f64 / tot],
            )
        })
        .collect();
    let massifs = codes
        .iter()
        .map(|c| MassifCells {
            code: *c,
            name: all_massifs
                .iter()
                .find(|m| m.code == *c)
                .map(|m| m.name.clone())
                .unwrap_or_default(),
            cells: terrain.massif.data().iter().filter(|v| **v == *c).count(),
        })
        .collect();
    t.total = t_all.elapsed().as_secs_f64();
    Ok(SnowBuild {
        site: site.id.into(),
        date,
        size_cells: [maps.depth.width(), maps.depth.height()],
        massifs,
        cells_filled_by_nearest_massif: terrain.cells_filled_by_nearest_massif,
        cells_estimated: n,
        depth_mean_min_max_cm: [mean as f32, lo, hi],
        class_shares,
        files,
        seconds: t,
    })
}
