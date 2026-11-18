//! Peak memory of the exposure model on a tall grid (#172): a Hauts-Plateaux zone of
//! 21.7 million cells needed 4.3 GB and killed the 4 GiB pipeline pod. The model must
//! keep its peak memory per cell bounded: the runout weights and workspaces follow a band
//! of rows, the classifier frees each layer once read, and the terrain layers of the PRA
//! step are not kept in the model. The layer build runs [`exposure::layer_model`], which
//! keeps of the PRA and of each runout only the bits the classifier reads (#173): this is
//! the figure `build-all` plans its sites with (`batch::BYTES_PER_CELL`).

use skimap_avalanche::exposure::{self, Inputs, raster::Raster};

/// A field of `/proc/self/status`, kB (`VmRSS`, `VmHWM`): the resident set the pod limit
/// counts. This file holds a single test, so the process is the model alone.
fn status_kb(field: &str) -> u64 {
    let s = std::fs::read_to_string("/proc/self/status").expect("/proc/self/status");
    s.lines()
        .find_map(|l| l.strip_prefix(field)?.strip_prefix(':'))
        .and_then(|v| v.trim().trim_end_matches(" kB").parse().ok())
        .expect("status field")
}

#[test]
fn model_peak_memory_per_cell_is_bounded() {
    // Steps falling south: every 128 rows, 24 rows at 40° (release zones) above a
    // 3° bench where the runout stops; corrugated across for gullies.
    let (w, h, cell) = (192usize, 2560usize, 5.0f64);
    let (steep, flat) = (40f64.to_radians().tan(), 3f64.to_radians().tan());
    let mut row_z = vec![3000.0f64; h];
    for r in 1..h {
        let t = if r % 128 < 24 { steep } else { flat };
        row_z[r] = row_z[r - 1] - cell * t;
    }
    let dem: Vec<f32> = (0..w * h)
        .map(|i| {
            let (r, c) = (i / w, (i % w) as f64);
            (row_z[r] + 6.0 * (c / 9.0).sin()) as f32
        })
        .collect();
    let canopy: Vec<f32> = (0..w * h)
        .map(|i| if (i / w / 200) % 3 == 2 { 70.0 } else { 5.0 })
        .collect();
    let raster = |data| Raster {
        w,
        h,
        cell,
        origin_x: 900_000.0,
        origin_y: 6_450_000.0,
        data,
    };
    let inputs = Inputs {
        dem: raster(dem),
        canopy: raster(canopy),
    };
    let pool = rayon::ThreadPoolBuilder::new()
        .num_threads(3)
        .build()
        .unwrap();
    let base = status_kb("VmHWM");
    let m = pool.install(|| exposure::layer_model(&inputs));
    let peak = (status_kb("VmHWM") - base) * 1024;
    let per_cell = peak as f64 / (w * h) as f64;
    eprintln!(
        "peak resident set added by the model: {:.1} MB, {per_cell:.1} B per cell, {} + {} paths",
        peak as f64 / 1e6,
        m.paths.0,
        m.paths.1
    );
    assert!(m.paths.1 > 10_000, "the grid must route many paths");
    // Measured on this grid (debug build, 3 threads): 228 B per cell before #172,
    // 133 B per cell after (whole model), 111 B per cell for the layer path (#173).
    assert!(
        per_cell < exposure::LAYER_PEAK_BYTES_PER_CELL,
        "{per_cell:.1} B per cell"
    );
}
