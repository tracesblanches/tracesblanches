//! Instruction counts of the dominating phone-side cases (Q5, #105), under Valgrind
//! (Callgrind, through gungraun), on the real inputs of `benches/data` (`make_data`):
//!
//! - `offpiste_route`: off-piste A* across the Le Laus → Col d'Izoard crop of the Cervières
//!   grid (snow of 15/02/2026, avalanche exposure overlay); `offpiste_explore`: the
//!   exhaustive search of an unreachable destination over the same window;
//! - `matching_outing`: a whole outing matched fix by fix (route monitor, then Viterbi
//!   decoding): Le Laus → Refuge des Fonts and back on the Cervières graph, 1 fix per second,
//!   10 m Gauss–Markov noise and forest outliers;
//! - `routing_full_graph`: fastest routes (C5 time model) between 40 node pairs spread over
//!   the whole Chapelle-des-Bois graph (773 edges, the larger one), router precomputation
//!   included.
//!
//! ```sh
//! cargo install gungraun-runner --version 0.20.0 --locked   # once; needs valgrind
//! cargo bench -p skimap-core --bench dominant
//! benches/compare.sh <base revision>                        # regression check
//! ```
//!
//! The same cases are timed natively by the `wall` benchmark, with the full-size grid.

mod cases;

use std::hint::black_box;

use cases::{Network, Offpiste, Outing};
use gungraun::{library_benchmark, library_benchmark_group, main};

#[library_benchmark]
#[bench::laus_izoard(setup = cases::setup_offpiste)]
fn offpiste_route(o: Offpiste) -> f64 {
    black_box(cases::run_offpiste_route(&o))
}

#[library_benchmark]
#[bench::whole_crop(setup = cases::setup_offpiste)]
fn offpiste_explore(o: Offpiste) -> usize {
    black_box(cases::run_offpiste_explore(&o))
}

#[library_benchmark]
#[bench::laus_fonts_and_back(setup = cases::setup_outing)]
fn matching_outing(o: Outing) -> usize {
    black_box(cases::run_matching(&o))
}

#[library_benchmark]
#[bench::chapelle_40_pairs(setup = cases::setup_network)]
fn routing_full_graph(n: Network) -> f64 {
    black_box(cases::run_routing(&n))
}

library_benchmark_group!(
    name = offpiste;
    benchmarks = offpiste_route, offpiste_explore
);
library_benchmark_group!(
    name = trails;
    benchmarks = matching_outing, routing_full_graph
);

main!(library_benchmark_groups = offpiste, trails);
