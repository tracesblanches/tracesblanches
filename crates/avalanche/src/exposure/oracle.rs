//! Cell-by-cell comparison with a run of the reference implementation (P16): AutoATES
//! v3.0 + com4FlowPy driven by `crates/avalanche/oracle/autoates_oracle.py`.
//!
//! Reads the reference rasters of a run directory (`PRA/`, `FlowPy/<scenario>/res_*/`,
//! `ATES/`) and compares them with the Rust layers of the same inputs, stage by stage, so
//! that a difference can be traced to the stage that introduces it.

use std::path::{Path, PathBuf};

use serde::Serialize;

use super::raster::{Raster, read};
use super::{Model, class, exposure_classes, flowpy};
use crate::error::{Error, Result};

/// Agreement of a continuous layer.
#[derive(Debug, Clone, Serialize)]
pub struct LayerAgreement {
    /// Layer.
    pub layer: String,
    /// Cells with a value in both.
    pub cells_both: u64,
    /// Share of all cells where both have a value or both have none.
    pub mask_agreement: f64,
    /// Share of the common cells with exactly the same Float32 value.
    pub identical: f64,
    /// Share of the common cells within `tolerance`.
    pub within_tolerance: f64,
    /// Tolerance (absolute).
    pub tolerance: f64,
    /// Largest absolute difference.
    pub max_abs_diff: f64,
    /// Mean absolute difference.
    pub mean_abs_diff: f64,
}

/// Agreement of a class layer.
#[derive(Debug, Clone, Serialize)]
pub struct ClassAgreement {
    /// Layer.
    pub layer: String,
    /// Class codes, in the order of the matrix.
    pub classes: Vec<u8>,
    /// Cells valid in both.
    pub cells: u64,
    /// Share of identical classes.
    pub agreement: f64,
    /// Cohen's kappa.
    pub kappa: f64,
    /// Rows: Rust; columns: reference.
    pub confusion: Vec<Vec<u64>>,
}

/// Compares two Float32 rasters (`NaN` = no value).
pub fn compare_f32(
    layer: &str,
    ours: &Raster<f32>,
    theirs: &Raster<f32>,
    tol: f64,
) -> LayerAgreement {
    assert!(ours.same_grid(theirs), "grids of {layer}");
    let (mut both, mut mask_ok, mut same, mut within) = (0u64, 0u64, 0u64, 0u64);
    let (mut maxd, mut sumd) = (0f64, 0f64);
    for (a, b) in ours.data.iter().zip(&theirs.data) {
        if a.is_nan() == b.is_nan() {
            mask_ok += 1;
        }
        if a.is_nan() || b.is_nan() {
            continue;
        }
        both += 1;
        let d = f64::from((a - b).abs());
        if a.to_bits() == b.to_bits() || a == b {
            same += 1;
        }
        if d <= tol {
            within += 1;
        }
        maxd = maxd.max(d);
        sumd += d;
    }
    let share = |x: u64| {
        if both == 0 {
            0.0
        } else {
            x as f64 / both as f64
        }
    };
    LayerAgreement {
        layer: layer.into(),
        cells_both: both,
        mask_agreement: mask_ok as f64 / ours.data.len() as f64,
        identical: share(same),
        within_tolerance: share(within),
        tolerance: tol,
        max_abs_diff: maxd,
        mean_abs_diff: if both == 0 { 0.0 } else { sumd / both as f64 },
    }
}

/// Compares two class rasters on the cells where both are below `nodata`.
pub fn compare_classes(layer: &str, ours: &[u8], theirs: &[u8], classes: &[u8]) -> ClassAgreement {
    let k = classes.len();
    let pos = |c: u8| classes.iter().position(|x| *x == c);
    let mut m = vec![vec![0u64; k]; k];
    for (a, b) in ours.iter().zip(theirs) {
        if let (Some(i), Some(j)) = (pos(*a), pos(*b)) {
            m[i][j] += 1;
        }
    }
    let n: u64 = m.iter().flatten().sum();
    let diag: u64 = (0..k).map(|i| m[i][i]).sum();
    let po = if n == 0 { 0.0 } else { diag as f64 / n as f64 };
    let pe: f64 = (0..k)
        .map(|i| {
            let r: u64 = m[i].iter().sum();
            let c: u64 = (0..k).map(|j| m[j][i]).sum();
            r as f64 * c as f64
        })
        .sum::<f64>()
        / ((n as f64) * (n as f64)).max(1.0);
    ClassAgreement {
        layer: layer.into(),
        classes: classes.to_vec(),
        cells: n,
        agreement: po,
        kappa: if pe < 1.0 {
            (po - pe) / (1.0 - pe)
        } else {
            1.0
        },
        confusion: m,
    }
}

/// A reference run directory.
#[derive(Debug, Clone)]
pub struct OracleRun {
    /// Run directory (`work_dir` of the oracle script).
    pub dir: PathBuf,
}

impl OracleRun {
    /// Opens a run directory.
    pub fn open(dir: impl Into<PathBuf>) -> Result<Self> {
        let dir = dir.into();
        if !dir.join("ATES").join("ATES_classification.tif").exists() {
            return Err(Error::format(
                dir.display().to_string(),
                "not a finished AutoATES v3.0 run (ATES/ATES_classification.tif missing)",
            ));
        }
        Ok(Self { dir })
    }

    fn read(&self, p: &Path) -> Result<Raster<f32>> {
        read(&self.dir.join(p))
    }

    /// PRA raster `pra_<scenario>_<name>.tif`.
    pub fn pra(&self, scenario: &str, name: &str) -> Result<Raster<f32>> {
        self.read(&PathBuf::from("PRA").join(format!("pra_{scenario}_{name}.tif")))
    }

    /// com4FlowPy output `<name>` of a scenario.
    pub fn flow(&self, scenario: &str, name: &str) -> Result<Raster<f32>> {
        let base = self.dir.join("FlowPy").join(scenario);
        let rd = std::fs::read_dir(&base).map_err(|e| Error::io(&base, e))?;
        for d in rd.flatten() {
            if d.file_name().to_string_lossy().starts_with("res_") {
                for f in std::fs::read_dir(d.path())
                    .map_err(|e| Error::io(d.path(), e))?
                    .flatten()
                {
                    let s = f.file_name().to_string_lossy().into_owned();
                    if s.ends_with(&format!("_{name}.tif")) {
                        return read(&f.path());
                    }
                }
            }
        }
        Err(Error::format(
            base.display().to_string(),
            format!("no {name} output"),
        ))
    }

    /// ATES raster (`merge_2` or `ATES_classification`).
    pub fn ates(&self, name: &str) -> Result<Raster<f32>> {
        self.read(&PathBuf::from("ATES").join(format!("{name}.tif")))
    }

    /// The reference inputs as warped by the reference (`PRA/autoATES_v3_DSM.tif`,
    /// `PRA/pra_extreme_forest_scale.tif`).
    pub fn dem(&self) -> Result<Raster<f32>> {
        self.read(Path::new("PRA/autoATES_v3_DSM.tif"))
    }
}

fn to_u8(r: &Raster<f32>) -> Vec<u8> {
    r.data
        .iter()
        .map(|v| if v.is_nan() { 255 } else { *v as u8 })
        .collect()
}

fn reached(counts: &Raster<f32>) -> Vec<bool> {
    counts
        .data
        .iter()
        .map(|v| v.is_finite() && *v > 0.0)
        .collect()
}

/// Flow-Py outputs of one scenario as the reference stores them (`NaN` where no path).
fn ours_flow(r: &flowpy::Runout) -> [(String, Raster<f32>); 4] {
    let reached = |i: usize| r.cell_counts.data[i] > 0;
    let n = r.cell_counts.data.len();
    let mask = |v: &Raster<f32>| {
        v.with_data(
            (0..n)
                .map(|i| if reached(i) { v.data[i] } else { f32::NAN })
                .collect(),
        )
    };
    [
        ("zdelta".into(), mask(&r.zdelta)),
        ("routFluxSum".into(), mask(&r.rout_flux_sum)),
        ("fpTravelAngleMax".into(), mask(&r.fp_travel_angle)),
        (
            "cellCounts".into(),
            r.cell_counts.with_data(
                (0..n)
                    .map(|i| {
                        if reached(i) {
                            r.cell_counts.data[i] as f32
                        } else {
                            f32::NAN
                        }
                    })
                    .collect(),
            ),
        ),
    ]
}

/// Full comparison report.
#[derive(Debug, Clone, Serialize)]
pub struct Report {
    /// Continuous layers.
    pub layers: Vec<LayerAgreement>,
    /// Class layers.
    pub classes: Vec<ClassAgreement>,
    /// Flow-Py alone: Rust runout on the reference's own release cells and forest index.
    pub flowpy_on_reference_inputs: Vec<LayerAgreement>,
}

/// Compares a Rust model with a reference run of the same inputs. With `isolate_flowpy`
/// the Rust Flow-Py also runs on the reference release cells (costly: two more runs).
pub fn compare(model: &Model, run: &OracleRun, isolate_flowpy: bool) -> Result<Report> {
    let mut layers = vec![
        compare_f32(
            "slope (PRA, edges)",
            &model.slope,
            &run.pra("frequent", "slope")?,
            1e-4,
        ),
        compare_f32(
            "windshelter",
            &model.windshelter,
            &run.pra("frequent", "windshelter")?,
            1e-6,
        ),
    ];
    let mut classes = Vec::new();
    let mut flowpy_iso = Vec::new();
    for (pra, rr, name) in [
        (&model.pra_frequent, &model.runout_frequent, "frequent"),
        (&model.pra_extreme, &model.runout_extreme, "extreme"),
    ] {
        layers.push(compare_f32(
            &format!("{name} PRA continuous"),
            &pra.continuous,
            &run.pra(name, "pra_continuous")?,
            1e-6,
        ));
        let bin = run.pra(name, "pra_binary")?;
        classes.push(compare_classes(
            &format!("{name} PRA binary"),
            &pra.binary.data,
            &to_u8(&bin),
            &[0, 1],
        ));
        let sieve = run.pra(name, "pra_sieve")?;
        classes.push(compare_classes(
            &format!("{name} PRA release (sieve)"),
            &pra.release.data,
            &to_u8(&sieve),
            &[0, 1],
        ));
        for (k, v) in ours_flow(rr) {
            layers.push(compare_f32(
                &format!("{name} runout {k}"),
                &v,
                &run.flow(name, &k)?,
                if k == "routFluxSum" { 1e-3 } else { 1e-4 },
            ));
        }
        if isolate_flowpy {
            let dem = run.dem()?;
            let release =
                sieve.with_data(to_u8(&sieve).iter().map(|v| u8::from(*v == 1)).collect());
            let fsi = run.pra(name, "forest_scale")?;
            let p = if name == "frequent" {
                flowpy::FREQUENT
            } else {
                flowpy::EXTREME
            };
            let r = flowpy::run(&dem, &release, &fsi, &p);
            for (k, v) in ours_flow(&r) {
                flowpy_iso.push(compare_f32(
                    &format!("{name} runout {k} (reference release and forest)"),
                    &v,
                    &run.flow(name, &k)?,
                    if k == "routFluxSum" { 1e-3 } else { 1e-4 },
                ));
            }
        }
    }
    let merge2 = to_u8(&run.ates("merge_2")?);
    let final_ = to_u8(&run.ates("ATES_classification")?);
    classes.push(compare_classes(
        "ATES classes before MMU (merge_2)",
        &model.ates.raw.data,
        &merge2,
        &[0, 1, 2, 3, 4],
    ));
    classes.push(compare_classes(
        "ATES classes (final)",
        &model.ates.classes.data,
        &final_,
        &[0, 1, 2, 3, 4],
    ));
    // The user classes, computed with the same rule on each side.
    let counts = run.flow("extreme", "cellCounts")?;
    let bin = to_u8(&run.pra("extreme", "pra_binary")?);
    let ref_ates = model.ates.classes.with_data(final_.clone());
    let ref_exp = exposure_classes(&ref_ates, &reached(&counts), &bin);
    classes.push(compare_classes(
        "exposure (3 classes, same rule on both sides)",
        &model.exposure.data,
        &ref_exp.data,
        &[class::LOW, class::EXPOSED, class::HIGH],
    ));
    Ok(Report {
        layers,
        classes,
        flowpy_on_reference_inputs: flowpy_iso,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn class_agreement_and_kappa() {
        let a = [0u8, 0, 1, 1, 2, 255];
        let b = [0u8, 1, 1, 1, 2, 2];
        let c = compare_classes("t", &a, &b, &[0, 1, 2]);
        assert_eq!(c.cells, 5);
        assert!((c.agreement - 0.8).abs() < 1e-12);
        assert_eq!(c.confusion[0], vec![1, 1, 0]);
        // po = 0.8; pe = (2·1 + 2·3 + 1·1) / 25 = 0.36; kappa = 0.44 / 0.64.
        assert!((c.kappa - 0.44 / 0.64).abs() < 1e-12);
        let r = Raster {
            w: 3,
            h: 1,
            cell: 5.0,
            origin_x: 0.0,
            origin_y: 0.0,
            data: vec![1.0f32, f32::NAN, 2.0],
        };
        let s = r.with_data(vec![1.0f32, 3.0, 2.5]);
        let l = compare_f32("x", &r, &s, 0.1);
        assert_eq!(l.cells_both, 2);
        assert!((l.identical - 0.5).abs() < 1e-12);
        assert!((l.max_abs_diff - 0.5).abs() < 1e-12);
        assert!((l.mask_agreement - 2.0 / 3.0).abs() < 1e-12);
    }
}
