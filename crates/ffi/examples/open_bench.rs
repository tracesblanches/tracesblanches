//! Time of opening packs on this machine, as the phone does it (X21, #155): the map
//! (`open_packs` on the style `fond-clair`), the morning (`open_day_planner`: pipeline view,
//! layers, refuges and clips) and, inside the morning, the plan context alone
//! (`Context::load`: refuges of the plan and clips).
//!
//! `cargo run --release -p skimap-ffi --example open_bench -- WORK RUNS PACK_DIR...`
//! (best and median of `RUNS` openings per pack; `WORK` receives the pipeline views; sites
//! from the registry of `SKIMAP_DATA_DIR`).

use std::path::{Path, PathBuf};
use std::time::Instant;

use skimap_ffi::Lang;
use skimap_ffi::tracking_day::{PackView, open_day_planner};
use skimap_pack::Pack;
use skimap_plan::plan::Context;

type Res<T> = Result<T, Box<dyn std::error::Error>>;

fn stats(mut v: Vec<f64>) -> (f64, f64) {
    v.sort_by(f64::total_cmp);
    (v[0], v[v.len() / 2])
}

fn main() -> Res<()> {
    let mut args = std::env::args().skip(1);
    let work = PathBuf::from(args.next().ok_or("WORK missing")?);
    let runs: usize = args.next().ok_or("RUNS missing")?.parse()?;
    let packs: Vec<PathBuf> = args.map(PathBuf::from).collect();
    let registry =
        skimap_common::site::Registry::load(&skimap_common::config::Config::from_env()?.layout())?;
    let style = std::fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../styles/fond-clair.json"),
    )?;
    println!(
        "{:<70} {:>8} {:>8} {:>8} {:>8} {:>8} {:>8} {:>5}",
        "pack", "map", "map~", "morning", "morning~", "ctx", "ctx~", "ref"
    );
    let (mut tot_map, mut tot_morning, mut tot_ctx) = (0.0, 0.0, 0.0);
    for p in &packs {
        let dir = p.display().to_string();
        let w = work.join(p.parent().and_then(Path::file_name).unwrap_or_default());
        std::fs::create_dir_all(&w)?;
        let (mut map, mut morning, mut ctx) = (Vec::new(), Vec::new(), Vec::new());
        let mut refuges = 0;
        for _ in 0..runs {
            let t = Instant::now();
            skimap_ffi::open_packs(vec![dir.clone()], style.clone(), false, Lang::Fr)?;
            map.push(t.elapsed().as_secs_f64() * 1e3);
            let t = Instant::now();
            // The morning opens on the evaluation sites only (`site::site`).
            if open_day_planner(dir.clone(), w.display().to_string(), Lang::Fr).is_ok() {
                morning.push(t.elapsed().as_secs_f64() * 1e3);
            }
            let pack = Pack::open(p.clone())?;
            let site = registry.get(&pack.manifest().site.id)?;
            let view = PackView::build(&pack, &w)?;
            let t = Instant::now();
            let c = Context::load(&view.layout, site)?;
            ctx.push(t.elapsed().as_secs_f64() * 1e3);
            refuges = c.refuges.len();
        }
        let (m0, m1) = stats(map);
        let (d0, d1) = if morning.is_empty() {
            (f64::NAN, f64::NAN)
        } else {
            stats(morning)
        };
        let (c0, c1) = stats(ctx);
        tot_map += m1;
        if d1.is_finite() {
            tot_morning += d1;
        }
        tot_ctx += c1;
        println!(
            "{:<70} {m0:>8.2} {m1:>8.2} {d0:>8.2} {d1:>8.2} {c0:>8.3} {c1:>8.3} {refuges:>5}",
            p.parent()
                .and_then(Path::file_name)
                .unwrap_or_default()
                .to_string_lossy()
        );
    }
    println!(
        "TOTAL (sum of medians, ms) map {tot_map:.1}  morning (evaluation sites) {tot_morning:.1}  ctx {tot_ctx:.2}  ({} packs, {runs} runs)",
        packs.len()
    );
    Ok(())
}
