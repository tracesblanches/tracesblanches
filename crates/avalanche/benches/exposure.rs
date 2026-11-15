//! Instruction counts of the avalanche exposure model (Q5, #105), the stage that dominates
//! `build-all cervieres` (81 s of 99 s on the workstation), under Valgrind (Callgrind,
//! through gungraun): the whole model (slope, wind shelter, both PRA scenarios, both runouts,
//! ATES-like classes, exposure classes) on the 320 m × 320 m crop of the real 5 m inputs of
//! Cervières with the most « très exposé » cells (`benches/data`, `make_data` of
//! skimap-core).
//!
//! The model runs on rayon: the global pool is built with one thread, the benchmark's own
//! (`use_current_thread`), so that the whole computation is counted on the main thread and
//! does not depend on the scheduling of a work-stealing pool.
//!
//! ```sh
//! cargo bench -p skimap-avalanche --bench exposure
//! ```

use std::hint::black_box;
use std::path::Path;

use gungraun::{library_benchmark, library_benchmark_group, main};
use skimap_avalanche::exposure::raster::Raster;
use skimap_avalanche::exposure::{Inputs, class, model};

fn input(name: &str) -> Raster<f32> {
    let p = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../benches/data")
        .join(name);
    let g = skimap_common::geotiff::read_f32(&p).unwrap_or_else(|e| panic!("{}: {e}", p.display()));
    Raster::from_grid(&g)
}

fn setup_steepest() -> Inputs {
    rayon::ThreadPoolBuilder::new()
        .num_threads(1)
        .use_current_thread()
        .build_global()
        .expect("one-thread global pool");
    Inputs {
        dem: input("cervieres_avalanche_dem.tif"),
        canopy: input("cervieres_avalanche_canopy.tif"),
    }
}

#[library_benchmark]
#[bench::cervieres_steepest(setup = setup_steepest)]
fn exposure_model(inputs: Inputs) -> usize {
    let m = model(black_box(&inputs));
    black_box(
        m.exposure
            .data
            .iter()
            .filter(|c| **c == class::HIGH)
            .count(),
    )
}

library_benchmark_group!(
    name = exposure;
    benchmarks = exposure_model
);

main!(library_benchmark_groups = exposure);
