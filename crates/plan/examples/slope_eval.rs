//! K6 (#56): the terrain slope of the off-piste rasters (`offpiste/slope_5m.skr`) every
//! 5 m along the trails of the graph of every site of `SKIMAP_DATA_DIR/out/` that has
//! both: share of the points at 30° or more, histogram per 10°, runs of 30° or more (any,
//! 20, 50, 100 m and longer). `CAT=groomed|backcountry|hike` keeps one kind of trail. Why
//! the tracker never says the slope on a trail (`tracker::SlopeParams`): the bank of a
//! trail cut across a hillside is in the cells beside it.
//!
//! ```sh
//! SKIMAP_DATA_DIR=… cargo run --release -p skimap-plan --example slope_eval
//! ```
use skimap_common::geom::LonLat;
use skimap_core::graph::PisteGraph;
use skimap_core::offpiste;

fn main() {
    let root = std::env::var("SKIMAP_DATA_DIR").expect("SKIMAP_DATA_DIR");
    let out = std::path::Path::new(&root).join("out");
    let mut sites: Vec<_> = std::fs::read_dir(&out)
        .unwrap()
        .flatten()
        .map(|e| e.path())
        .collect();
    sites.sort();
    let (mut tot, mut steep) = (0usize, 0usize);
    for d in sites {
        let g = d.join("routes/piste_graph.json");
        let s = d.join("offpiste/slope_5m.skr");
        if !g.is_file() || !s.is_file() {
            continue;
        }
        let g = match PisteGraph::from_json(&std::fs::read_to_string(&g).unwrap()) {
            Ok(g) => g,
            Err(e) => {
                println!("{}: {e}", d.display());
                continue;
            }
        };
        let grid: skimap_common::grid::Grid<u8> = offpiste::read_raster(&s).unwrap();
        let at = |p: LonLat| -> Option<f64> {
            let (x, y) = skimap_common::crs::to_lambert93(p).ok()?;
            grid.value_at(x, y)
                .filter(|v| **v != offpiste::SLOPE_NODATA)
                .map(|v| f64::from(*v) / 2.0)
        };
        let (mut n, mut k, mut hist) = (0usize, 0usize, [0usize; 5]);
        let mut runs = [0usize; 4];
        let cat = std::env::var("CAT").unwrap_or_default();
        for e in 0..g.edges().len() {
            let a = &g.edge(e).attrs;
            let c = if a.backcountry {
                "backcountry"
            } else if a.hike_only() {
                "hike"
            } else {
                "groomed"
            };
            if !cat.is_empty() && c != cat {
                continue;
            }
            let len = g.length_m(e);
            let mut cur = 0.0f64;
            let mut s = 0.0;
            while s <= len {
                if let Some(v) = at(g.point_at(e, s)) {
                    n += 1;
                    hist[((v / 10.0) as usize).min(4)] += 1;
                    if v >= 30.0 {
                        k += 1;
                        cur += 5.0;
                    } else {
                        if cur > 0.0 {
                            for (i, m) in [0.0, 20.0, 50.0, 100.0].iter().enumerate() {
                                if cur >= *m {
                                    runs[i] += 1;
                                }
                            }
                        }
                        cur = 0.0;
                    }
                }
                s += 5.0;
            }
        }
        tot += n;
        steep += k;
        println!(
            "{:<40} {:>7} samples, >=30: {:>6} ({:.2} %), hist {:?}, runs any/20/50/100 m {:?}, km {:.0}",
            d.file_name().unwrap().to_string_lossy(),
            n,
            k,
            100.0 * k as f64 / n.max(1) as f64,
            hist,
            runs,
            g.total_length_m() / 1000.0
        );
    }
    println!(
        "total {tot} samples, >=30 {steep} ({:.2} %)",
        100.0 * steep as f64 / tot.max(1) as f64
    );
}
