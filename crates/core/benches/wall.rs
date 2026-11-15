//! Wall-clock times of the dominating phone-side cases (Q5, #105), native, on the
//! workstation: the cases of the `dominant` benchmark (inputs of `benches/data`) and, when
//! `<data>/out/cervieres/offpiste` exists ($SKIMAP_DATA_DIR or ./data), the Laus → Izoard
//! request and the exhaustive search on the whole Cervières grid (3317 × 3480 cells).
//!
//! ```sh
//! cargo bench -p skimap-core --bench wall [-- <iterations>]
//! ```
//!
//! One warm-up run, then `iterations` (default 7) timed runs per case; prints the median
//! and the minimum. Instruction counts (`dominant`) are the regression check; these times
//! depend on the machine and its load.

mod cases;

use std::hint::black_box;
use std::path::PathBuf;
use std::time::Instant;

fn time<T>(name: &str, iters: usize, mut f: impl FnMut() -> T) {
    black_box(f());
    let mut t: Vec<f64> = (0..iters)
        .map(|_| {
            let t0 = Instant::now();
            black_box(f());
            t0.elapsed().as_secs_f64() * 1e3
        })
        .collect();
    t.sort_by(f64::total_cmp);
    println!(
        "{name:<44} median {:>9.2} ms   min {:>9.2} ms   ({iters} runs)",
        t[t.len() / 2],
        t[0]
    );
}

fn main() {
    // `cargo bench` passes `--bench`; a number is the iteration count.
    let iters = std::env::args()
        .skip(1)
        .find_map(|a| a.parse::<usize>().ok())
        .unwrap_or(7)
        .max(1);
    let o = cases::setup_offpiste();
    time("offpiste_route (crop, Laus → Izoard)", iters, || {
        cases::run_offpiste_route(&o)
    });
    time("offpiste_explore (crop, exhaustive)", iters, || {
        cases::run_offpiste_explore(&o)
    });
    let outing = cases::setup_outing();
    time(
        &format!("matching_outing ({} fixes)", outing.fixes.len()),
        iters,
        || cases::run_matching(&outing),
    );
    let n = cases::setup_network();
    time("routing_full_graph (Chapelle, 40 routes)", iters, || {
        cases::run_routing(&n)
    });
    let data = std::env::var_os("SKIMAP_DATA_DIR").map_or_else(
        || PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../data"),
        PathBuf::from,
    );
    match cases::setup_offpiste_full(&data) {
        Some(full) => {
            println!(
                "full grid: {} × {} cells ({})",
                full.grid.width(),
                full.grid.height(),
                data.display()
            );
            time(
                "offpiste_route (whole grid, Laus → Izoard)",
                iters,
                || cases::run_offpiste_route(&full),
            );
            time(
                "offpiste_explore (whole grid, exhaustive)",
                iters.min(3),
                || cases::run_offpiste_explore(&full),
            );
        }
        None => println!("full grid: {} not found, skipped", data.display()),
    }
}
