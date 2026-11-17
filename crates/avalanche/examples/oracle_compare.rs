//! Runs the Rust model on the inputs given to the reference implementation and compares
//! every stage with the reference run (P16 / P17):
//!
//! ```sh
//! cargo run --release -p skimap-avalanche --example oracle_compare -- \
//!     <site> <cell_m> <oracle_run_dir> [--isolate-flowpy]
//! ```
//!
//! Inputs: `data/out/<site>/avalanche-exposure/inputs/{dem,canopy}_<cell>m.tif` (written
//! by the `exposure_inputs` example). Writes
//! `data/out/<site>/avalanche-exposure/oracle_comparison_<cell>m.json`.

#![allow(
    clippy::disallowed_methods,
    reason = "developer tool, not a pipeline output"
)]
use std::path::PathBuf;
use std::time::Instant;

use skimap_avalanche::exposure::{self, Inputs, oracle, raster};
use skimap_common::config::Config;
use skimap_common::site;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.len() < 3 {
        return Err(
            "usage: oracle_compare <site> <cell_m> <oracle_run_dir> [--isolate-flowpy]".into(),
        );
    }
    let s = site::site(&args[0])?;
    let cell: u32 = args[1].parse()?;
    let run = oracle::OracleRun::open(PathBuf::from(&args[2]))?;
    let isolate = args.iter().any(|a| a == "--isolate-flowpy");
    let layout = Config::from_env()?.layout();
    let dir = layout.layer_dir(s, exposure::LAYER)?;
    let read = |name: &str| -> Result<raster::Raster<f32>, Box<dyn std::error::Error>> {
        Ok(raster::read(
            &dir.join("inputs").join(format!("{name}_{cell}m.tif")),
        )?)
    };
    let inputs = Inputs {
        dem: read("dem")?,
        canopy: read("canopy")?,
    };
    println!(
        "{} at {cell} m: {} x {} cells, {} threads",
        s.id,
        inputs.dem.w,
        inputs.dem.h,
        rayon::current_num_threads()
    );
    let t = Instant::now();
    let m = exposure::model(&inputs);
    println!("model {:.1} s: {:?}", t.elapsed().as_secs_f64(), m.timings);
    println!(
        "paths: frequent {} ({} visits), extreme {} ({} visits)",
        m.runout_frequent.paths,
        m.runout_frequent.visits,
        m.runout_extreme.paths,
        m.runout_extreme.visits
    );
    if let Some(p) = args.iter().position(|a| a == "--dump") {
        // Diagnostic rasters of the Rust run (Float32), for inspection next to the oracle.
        let d = PathBuf::from(&args[p + 1]);
        std::fs::create_dir_all(&d)?;
        for (name, r) in [
            ("frequent", &m.runout_frequent),
            ("extreme", &m.runout_extreme),
        ] {
            let counts = r
                .cell_counts
                .with_data(r.cell_counts.data.iter().map(|v| *v as f32).collect());
            raster::write_f32(&d.join(format!("{name}_cellCounts.tif")), &counts)?;
            raster::write_f32(&d.join(format!("{name}_zdelta.tif")), &r.zdelta)?;
        }
    }
    let t = Instant::now();
    let rep = oracle::compare(&m, &run, isolate)?;
    println!("comparison {:.1} s", t.elapsed().as_secs_f64());
    for l in rep.layers.iter().chain(&rep.flowpy_on_reference_inputs) {
        println!(
            "{:<58} both {:>9} mask {:.6} identical {:.6} within {:.0e}: {:.6} max {:.3e} mean {:.3e}",
            l.layer,
            l.cells_both,
            l.mask_agreement,
            l.identical,
            l.tolerance,
            l.within_tolerance,
            l.max_abs_diff,
            l.mean_abs_diff
        );
    }
    for c in &rep.classes {
        println!(
            "{:<50} cells {:>9} agreement {:.6} kappa {:.4} classes {:?}",
            c.layer, c.cells, c.agreement, c.kappa, c.classes
        );
        for row in &c.confusion {
            println!("    {row:?}");
        }
    }
    let out = dir.join(format!("oracle_comparison_{cell}m.json"));
    let body = serde_json::json!({
        "site": s.id,
        "cell_m": cell,
        "oracle_run": run.dir,
        "timings_s": m.timings,
        "paths": {"frequent": m.runout_frequent.paths, "extreme": m.runout_extreme.paths},
        "visits": {"frequent": m.runout_frequent.visits, "extreme": m.runout_extreme.visits},
        "threads": rayon::current_num_threads(),
        "report": rep,
    });
    std::fs::write(&out, serde_json::to_vec_pretty(&body)?)?;
    println!("{}", out.display());
    Ok(())
}
