//! Opening of the off-piste layers of packs as the phone does it (E1 #125): the router
//! (`open_offpiste`: every raster) and the relief alone (`Relief::open`: altitude of a
//! trace in the recap), time and memory (resident set `VmRSS`, peak `VmHWM`), the
//! inflation into private storage done once before (kept while the pack does not change).
//!
//! `cargo run --release -p skimap-ffi --example offpiste_open_bench -- WORK WHAT PACK_DIR`
//! with `WHAT` = `router`, `relief`, `trace`, `trace-whole` or `compare` (every cell read in place equals the
//! raster read whole) (one per process: the peak is the process's).

use std::path::{Path, PathBuf};
use std::time::Instant;

fn status(key: &str) -> u64 {
    std::fs::read_to_string("/proc/self/status")
        .unwrap_or_default()
        .lines()
        .find_map(|l| l.strip_prefix(key))
        .and_then(|v| v.split_whitespace().next()?.parse().ok())
        .unwrap_or(0)
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args().skip(1);
    let work = PathBuf::from(args.next().ok_or("WORK missing")?);
    let what = args.next().ok_or("WHAT missing")?;
    let pack = args.next().ok_or("PACK_DIR missing")?;
    let p = skimap_pack::Pack::open(PathBuf::from(&pack))?;
    skimap_ffi::offpiste::inflate_offpiste(&p, &work)?;
    let (rss0, hwm0) = (status("VmRSS:"), status("VmHWM:"));
    let t = Instant::now();
    let label = match what.as_str() {
        "router" => {
            let r = skimap_ffi::offpiste::open_offpiste(
                pack.clone(),
                work.display().to_string(),
                skimap_ffi::Lang::Fr,
            )?;
            let ms = t.elapsed().as_secs_f64() * 1e3;
            let label = format!("router opened in {ms:.1} ms");
            drop(r);
            label
        }
        "relief" => {
            let r = skimap_ffi::offpiste::Relief::open(Path::new(&pack), &work)?;
            let ms = t.elapsed().as_secs_f64() * 1e3;
            let c = p.manifest().site.center;
            let z = r.at(skimap_common::geom::LonLat::new(c[0], c[1]));
            format!(
                "relief opened in {ms:.1} ms, altitude at the centre {z:?}, resident {} kB after",
                status("VmRSS:")
            )
        }
        "trace" | "trace-whole" => {
            // The recap's case: the altitude under every fix of a 3 h outing (one fix per
            // 2 s, 5 400 fixes along a loop of 6 km around the centre), read in place
            // (`trace`, the relief of the recap) or from the raster read whole
            // (`trace-whole`, as before #125).
            let c = p.manifest().site.center;
            let fixes: Vec<skimap_common::geom::LonLat> = (0..5_400)
                .map(|i| {
                    let a = f64::from(i) / 5_400.0 * std::f64::consts::TAU;
                    skimap_common::geom::LonLat::new(
                        c[0] + 0.0122 * a.cos(),
                        c[1] + 0.0086 * a.sin(),
                    )
                })
                .collect();
            let found = if what == "trace" {
                let r = skimap_ffi::offpiste::Relief::open(Path::new(&pack), &work)?;
                fixes.iter().filter_map(|f| r.at(*f)).count()
            } else {
                let dir = work.join("offpiste").join(&p.manifest().site.id);
                let g = skimap_core::offpiste::read_raster::<u16>(
                    &dir.join(skimap_core::offpiste::FILE_ELEVATION),
                )?;
                fixes
                    .iter()
                    .filter_map(|f| {
                        let (x, y) = skimap_common::crs::to_lambert93(*f).ok()?;
                        skimap_core::offpiste::elevation_m(*g.value_at(x, y)?)
                    })
                    .count()
            };
            format!(
                "{what}: {found} altitudes of {} fixes in {:.1} ms",
                fixes.len(),
                t.elapsed().as_secs_f64() * 1e3
            )
        }
        "compare" => {
            // Every cell of every inflated raster, read in place and read whole.
            let dir = work.join("offpiste").join(&p.manifest().site.id);
            let mut cells = 0usize;
            for e in std::fs::read_dir(&dir)?.flatten() {
                let path = e.path();
                if path.extension().is_none_or(|x| x != "skr") {
                    continue;
                }
                use skimap_core::offpiste::{RasterRows, read_raster};
                let n = if let Ok(g) = read_raster::<u16>(&path) {
                    let r = RasterRows::<u16>::open(&path)?;
                    for row in 0..g.height() {
                        for c in 0..g.width() {
                            assert_eq!(r.get(c, row), g.get(c, row).copied(), "{path:?}");
                        }
                    }
                    g.data().len()
                } else {
                    let g = read_raster::<u8>(&path)?;
                    let r = RasterRows::<u8>::open(&path)?;
                    for row in 0..g.height() {
                        for c in 0..g.width() {
                            assert_eq!(r.get(c, row), g.get(c, row).copied(), "{path:?}");
                        }
                    }
                    g.data().len()
                };
                cells += n;
            }
            format!("{cells} cells identical, read in place and read whole")
        }
        _ => return Err("WHAT: router, relief or compare".into()),
    };
    println!(
        "{}: {label}; resident before {rss0} kB, peak {hwm0} → {} kB (+{} kB)",
        p.manifest().site.id,
        status("VmHWM:"),
        status("VmHWM:").saturating_sub(hwm0)
    );
    Ok(())
}
