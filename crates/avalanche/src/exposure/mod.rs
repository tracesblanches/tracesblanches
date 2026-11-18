//! Avalanche terrain exposure (P17, #30), validated against the CLPA (P18, #31).
//!
//! See the crate README for the method, the outputs and the measured agreement with the
//! reference implementation.

pub mod ates;
pub mod build;
pub mod flowpy;
pub mod forest;
pub mod oracle;
pub mod pra;
pub mod raster;
pub mod segments;
pub mod validation;

pub use build::{ExposureSummary, build, build_with, read_exposure_grid};

use std::path::{Path, PathBuf};

use skimap_common::download::Downloader;
use skimap_common::layout::DataLayout;
use skimap_common::site::Site;
use skimap_terrain::Extent;

use crate::error::{Error, Result};
use raster::Raster;

/// Layer name (output directory).
pub const LAYER: &str = "avalanche-exposure";

/// Model inputs on one grid: terrain model (m) and canopy cover (%).
#[derive(Debug, Clone)]
pub struct Inputs {
    /// Terrain model, m, `NaN` = no data.
    pub dem: Raster<f32>,
    /// Canopy cover, %, `NaN` = unknown.
    pub canopy: Raster<f32>,
}

/// Reads the 5 m terrain model written by the terrain crate.
pub fn read_dem(layout: &DataLayout, site: &Site) -> Result<Raster<f32>> {
    let path = layout
        .layer_dir(site, skimap_terrain::LAYER)?
        .join("dem_5m.tif");
    if !path.exists() {
        return Err(Error::MissingLayer {
            layer: skimap_terrain::LAYER.into(),
            site: site.id.into(),
            path,
        });
    }
    Ok(Raster::from_grid(&skimap_common::geotiff::read_f32(&path)?))
}

/// 5 m inputs of a site: the terrain crate DEM and the canopy cover derived from the
/// LiDAR HD MNH (downloaded through the shared downloader).
pub fn inputs_5m(site: &Site, layout: &DataLayout, dl: &Downloader) -> Result<Inputs> {
    let dem = read_dem(layout, site)?;
    let extent = Extent::from_bbox(site.bbox)?;
    let mnh = Raster::from_grid(&forest::fetch_mnh(&extent, dl)?);
    if !mnh.same_grid(&dem) {
        return Err(Error::format(
            "LiDAR HD MNH",
            "canopy height grid differs from the terrain model grid",
        ));
    }
    let canopy = forest::canopy_cover(&mnh, &dem);
    Ok(Inputs { dem, canopy })
}

/// Same inputs at `factor` times the cell size (block means).
pub fn resample(inputs: &Inputs, factor: usize) -> Inputs {
    Inputs {
        dem: inputs.dem.block_mean(factor),
        canopy: inputs.canopy.block_mean(factor),
    }
}

/// Exposure class codes of the user layer (`exposure_<cell>m.tif`, UInt8).
pub mod class {
    /// « terrain peu exposé (modèle) ».
    pub const LOW: u8 = 1;
    /// « exposé ».
    pub const EXPOSED: u8 = 2;
    /// « très exposé ».
    pub const HIGH: u8 = 3;
    /// No data (outside the LiDAR HD coverage, raster edge).
    pub const NODATA: u8 = super::raster::CLASS_NODATA;

    /// French label of a class code.
    pub fn label(c: u8) -> &'static str {
        match c {
            LOW => "terrain peu exposé (modèle)",
            EXPOSED => "exposé",
            HIGH => "très exposé",
            _ => "pas de donnée",
        }
    }
}

/// Margin around the modelled runout and release areas inside which a cell is never
/// « peu exposé », m (position uncertainty of the 5 m model and of OSM tracks).
pub const LOW_MARGIN_M: f64 = 10.0;

/// Three exposure classes from the ATES-like classes and the raw infrequent scenario:
///
/// - « très exposé » (3): ATES class 3 or 4;
/// - « terrain peu exposé (modèle) » (1): ATES class 0 or 1 **and** farther than
///   [`LOW_MARGIN_M`] from every cell reached by the infrequent (α = 18°) runout and from
///   every infrequent release cell (before the sieve);
/// - « exposé » (2): everything else.
///
/// The rule is applied after the minimum mapping unit, on the raw runout, so that no
/// modelled runout is ever labelled « peu exposé » (unlike the class 0 of the reference,
/// which can fall inside a runout zone, report 07).
pub fn exposure_classes(ates: &Raster<u8>, reached_ext: &[bool], pra_ext: &[u8]) -> Raster<u8> {
    exposure_classes_with_margin(ates, reached_ext, pra_ext, LOW_MARGIN_M)
}

/// [`exposure_classes`] with another margin (sensitivity studies).
pub fn exposure_classes_with_margin(
    ates: &Raster<u8>,
    reached_ext: &[bool],
    pra_ext: &[u8],
    margin_m: f64,
) -> Raster<u8> {
    let n = ates.w * ates.h;
    let mask: Vec<u8> = (0..n)
        .map(|i| u8::from(reached_ext[i] || pra_ext[i] != 0))
        .collect();
    let d = pra::distance_to(&ates.with_data(mask));
    ates.with_data(
        (0..n)
            .map(|i| match ates.data[i] {
                a if a > 4 => class::NODATA,
                3 | 4 => class::HIGH,
                0 | 1 if f64::from(d.data[i]) > margin_m => class::LOW,
                _ => class::EXPOSED,
            })
            .collect(),
    )
}

/// Wall-clock seconds of the model stages.
#[derive(Debug, Clone, Default, serde::Serialize)]
pub struct Timings {
    /// Slope and wind shelter.
    pub terrain: f64,
    /// Both PRA scenarios.
    pub pra: f64,
    /// Typical runout.
    pub runout_frequent: f64,
    /// Infrequent runout.
    pub runout_extreme: f64,
    /// ATES-like classes and the minimum mapping unit.
    pub ates: f64,
    /// The three exposure classes.
    pub classes: f64,
    /// Whole model.
    pub total: f64,
}

/// Terrain layers of the PRA step: Horn slope with edges (degrees) and wind shelter
/// (radians), as [`model_with`] computes them. Not kept in [`Model`]: only the PRA reads
/// them, and two Float32 grids alive through the runout cost 174 MB on a 21-million-cell
/// zone (#172); the diagnostics recompute them, bit for bit, with this function.
pub fn terrain(dem: &Raster<f32>) -> (Raster<f32>, Raster<f32>) {
    let slope = pra::gdal_slope(dem, true);
    let ws = pra::windshelter(
        dem,
        pra::FREQUENT.wind_radius_m,
        pra::FREQUENT.wind_probability,
    );
    (slope, ws)
}

/// All model layers of one grid that the layer and the classifier read.
#[derive(Debug, Clone)]
pub struct Model {
    /// Typical PRA.
    pub pra_frequent: pra::Pra,
    /// Infrequent PRA.
    pub pra_extreme: pra::Pra,
    /// Typical runout (α = 30°).
    pub runout_frequent: flowpy::Runout,
    /// Infrequent runout (α = 18°).
    pub runout_extreme: flowpy::Runout,
    /// ATES-like classes (dev8 port).
    pub ates: ates::Ates,
    /// User classes 1–3, 255 = no data.
    pub exposure: Raster<u8>,
    /// Stage timings.
    pub timings: Timings,
}

/// Forest structure index of the runout (`scale_forest`): canopy / 100 in Float32,
/// clipped to 0–1, no data → 0.
pub fn forest_index(canopy: &Raster<f32>) -> Raster<f32> {
    canopy.with_data(
        canopy
            .data
            .iter()
            .map(|v| {
                if v.is_nan() {
                    0.0
                } else {
                    (v / 100.0).clamp(0.0, 1.0)
                }
            })
            .collect(),
    )
}

/// Runs the whole model (AutoATES v3.0 ISSW defaults, ported) on one grid.
pub fn model(inputs: &Inputs) -> Model {
    model_with(inputs, &flowpy::FREQUENT, &flowpy::EXTREME)
}

/// Same with other runout parameters (sensitivity studies; the layer uses [`model`]).
pub fn model_with(
    inputs: &Inputs,
    flow_frequent: &flowpy::FlowParams,
    flow_extreme: &flowpy::FlowParams,
) -> Model {
    use std::time::Instant;
    let t_all = Instant::now();
    let mut t = Timings::default();
    let t0 = Instant::now();
    let (slope, ws) = terrain(&inputs.dem);
    t.terrain = t0.elapsed().as_secs_f64();
    let t0 = Instant::now();
    let pf = pra::pra(&slope, &ws, &inputs.canopy, &pra::FREQUENT);
    let pe = pra::pra(&slope, &ws, &inputs.canopy, &pra::EXTREME);
    drop((slope, ws));
    t.pra = t0.elapsed().as_secs_f64();
    let fsi = forest_index(&inputs.canopy);
    let t0 = Instant::now();
    // One weight table for both scenarios when they spread alike (the defaults do).
    let mut wf = flowpy::Weights::new(&inputs.dem, &fsi, flow_frequent.exp);
    let mut we = (flow_extreme.exp.to_bits() != flow_frequent.exp.to_bits())
        .then(|| flowpy::Weights::new(&inputs.dem, &fsi, flow_extreme.exp));
    let rf = flowpy::run_with(&mut wf, &pf.release, flow_frequent);
    t.runout_frequent = t0.elapsed().as_secs_f64();
    let t0 = Instant::now();
    let re = flowpy::run_with(we.as_mut().unwrap_or(&mut wf), &pe.release, flow_extreme);
    t.runout_extreme = t0.elapsed().as_secs_f64();
    // The weight tables and the forest index are not used past the runout: freed before
    // the classifier allocates its own layers (#172).
    drop((wf, we));
    drop(fsi);
    let t0 = Instant::now();
    let a = ates::classify(&inputs.dem, &inputs.canopy, (&pf, &rf), (&pe, &re));
    t.ates = t0.elapsed().as_secs_f64();
    let t0 = Instant::now();
    let reached: Vec<bool> = (0..re.cell_counts.data.len())
        .map(|i| re.reached(i))
        .collect();
    let exposure = exposure_classes(&a.classes, &reached, &pe.binary.data);
    t.classes = t0.elapsed().as_secs_f64();
    t.total = t_all.elapsed().as_secs_f64();
    Model {
        pra_frequent: pf,
        pra_extreme: pe,
        runout_frequent: rf,
        runout_extreme: re,
        ates: a,
        exposure,
        timings: t,
    }
}

/// Writes the inputs as `dem_<cell>m.tif` and `canopy_<cell>m.tif` into `dir`.
pub fn write_inputs(dir: &Path, inputs: &Inputs) -> Result<Vec<PathBuf>> {
    DataLayout::ensure(dir)?;
    let tag = format!("{}m", inputs.dem.cell.round() as i64);
    let mut out = Vec::new();
    for (name, r) in [("dem", &inputs.dem), ("canopy", &inputs.canopy)] {
        let p = dir.join(format!("{name}_{tag}.tif"));
        raster::write_f32(&p, r)?;
        out.push(p);
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn three_classes_never_low_near_a_runout() {
        // 8 cells in a row at 5 m: ATES 0, 0, 0, 1, 1, 3, 4, invalid; the infrequent
        // runout reaches cell 0 only.
        let ates = Raster {
            w: 8,
            h: 1,
            cell: 5.0,
            origin_x: 0.0,
            origin_y: 0.0,
            data: vec![0u8, 0, 0, 1, 1, 3, 4, 255],
        };
        let mut reached = vec![false; 8];
        reached[0] = true;
        let e = exposure_classes(&ates, &reached, &[0u8; 8]);
        // Cells 1 and 2 are 5 and 10 m from the runout: within the 10 m margin.
        assert_eq!(
            e.data,
            vec![
                class::EXPOSED,
                class::EXPOSED,
                class::EXPOSED,
                class::LOW,
                class::LOW,
                class::HIGH,
                class::HIGH,
                class::NODATA
            ]
        );
        // A release cell (before the sieve) has the same effect.
        let mut pra = vec![0u8; 8];
        pra[4] = 1;
        let e = exposure_classes(&ates, &[false; 8], &pra);
        assert_eq!(
            &e.data[..5],
            &[
                class::LOW,
                class::LOW,
                class::EXPOSED,
                class::EXPOSED,
                class::EXPOSED
            ]
        );
        for c in [class::LOW, class::EXPOSED, class::HIGH, class::NODATA] {
            let l = class::label(c).to_lowercase();
            assert!(!l.contains("sûr") && !l.contains("sans danger"), "{l}");
        }
    }
}
