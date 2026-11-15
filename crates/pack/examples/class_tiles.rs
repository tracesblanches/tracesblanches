//! Measurement of the class polygons (slope, avalanche exposure) of both sites for a
//! threshold of small patches: areas per class and level, significant zones, polygons,
//! vertices, size of the gzipped tiles, time. Writes nothing.
//!
//! ```sh
//! cargo run --release -p skimap-pack --features pipeline --example class_tiles -- \
//!     [patch=<m²>] [smoothing=none|majority|fill] [tol=<m>] [maxzoom=<z>] [minzoom=<z>]
//! ```

use std::io::Write;
use std::time::Instant;

use skimap_common::config::Config;
use skimap_common::site::SITES;
use skimap_pack::build::class_tiles::{Params, Smoothing, catalog, vectorize};
use skimap_pack::build::raster::read_as;

fn gz(b: &[u8]) -> usize {
    let mut z = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
    z.write_all(b).expect("gzip");
    z.finish().expect("gzip").len()
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut params = Params::default();
    for a in &args {
        let (k, v) = a.split_once('=').ok_or("arguments are key=value")?;
        match k {
            "patch" => params.min_patch_m2 = v.parse()?,
            "inner" => params.min_inner_patch_m2 = v.parse()?,
            "tol" => params.min_tolerance_m = v.parse()?,
            "tolpx" => params.tolerance_px = v.parse()?,
            "areapx" => params.min_area_px = v.parse()?,
            "maxzoom" => params.maxzoom = v.parse()?,
            "minzoom" => params.minzoom = v.parse()?,
            "smoothing" => {
                params.smoothing = match v {
                    "none" => Smoothing::None,
                    "majority" => Smoothing::Majority,
                    "fill" => Smoothing::Fill,
                    _ => return Err("smoothing=none|majority|fill".into()),
                }
            }
            _ => return Err(format!("unknown argument {k}").into()),
        }
    }
    let layout = Config::from_env()?.layout();
    println!("params {params:?}");
    for site in SITES.iter() {
        for spec in catalog() {
            let src = layout.out_dir(site).join(spec.origin).join(spec.file);
            let t = Instant::now();
            let grid = read_as(spec.read, &src)?;
            let read_s = t.elapsed().as_secs_f64();
            let t = Instant::now();
            let v = vectorize(&grid, &spec, &params)?;
            let secs = t.elapsed().as_secs_f64();
            let raw: usize = v.tiles.iter().map(|(_, b)| b.len()).sum();
            let zipped: usize = v.tiles.iter().map(|(_, b)| gz(b)).sum();
            let i = &v.info;
            println!(
                "== {} {} : read {read_s:.2} s, vectorise {secs:.2} s ; {} tiles, MVT {:.3} MB, gzip {:.3} MB ; {} polygons ; several exteriors {}",
                site.id,
                spec.id,
                i.tile_count,
                raw as f64 / 1e6,
                zipped as f64 / 1e6,
                i.polygons,
                i.regions_with_several_exteriors
            );
            let mut by_zoom: std::collections::BTreeMap<u8, (usize, usize)> = Default::default();
            for ((z, _, _), b) in &v.tiles {
                let e = by_zoom.entry(*z).or_default();
                e.0 += 1;
                e.1 += gz(b);
            }
            println!(
                "   gzip per zoom: {}",
                by_zoom
                    .iter()
                    .map(|(z, (n, b))| format!("z{z} {n} tiles {:.3} MB", *b as f64 / 1e6))
                    .collect::<Vec<_>>()
                    .join(", ")
            );
            for (step, s) in &v.timings {
                println!("   time {step:<30} {s:.2} s");
            }
            println!(
                "   vertices per zoom: {}",
                i.vertices
                    .iter()
                    .map(|z| format!("z{} {}", z.zoom, z.value))
                    .collect::<Vec<_>>()
                    .join(", ")
            );
            println!(
                "   tolerance per zoom (m): {}",
                i.tolerance_m
                    .iter()
                    .map(|z| format!("z{} {:.2}", z.zoom, z.value))
                    .collect::<Vec<_>>()
                    .join(", ")
            );
            for c in &i.classes {
                let per_zoom: Vec<String> = c
                    .vector_m2
                    .iter()
                    .map(|z| {
                        format!(
                            "z{} {:+.2} %",
                            z.zoom,
                            100.0 * (z.value - c.raster_m2) / c.raster_m2.max(1.0)
                        )
                    })
                    .collect();
                println!(
                    "   class {} {:<28} raster {:>10.4} km², filtered {:+.2} %, vector {}",
                    c.value,
                    c.label,
                    c.raster_m2 / 1e6,
                    100.0 * (c.filtered_m2 - c.raster_m2) / c.raster_m2.max(1.0),
                    per_zoom.join(" ")
                );
            }
            for l in &i.levels {
                println!(
                    "   level {:<22} raster {:>9.4} km², lost {:.4} km² ({:.2} %), gained {:.4} km² ({:.2} %), vector {:+.2} % ; significant zones {} : min coverage {:.3}, below half {} ; largest removed {:.0} m²",
                    l.label,
                    l.raster_m2 / 1e6,
                    l.lost_m2 / 1e6,
                    100.0 * l.lost_m2 / l.raster_m2.max(1.0),
                    l.gained_m2 / 1e6,
                    100.0 * l.gained_m2 / l.raster_m2.max(1.0),
                    100.0 * (l.vector_m2 - l.raster_m2) / l.raster_m2.max(1.0),
                    l.significant_zones,
                    l.significant_min_coverage,
                    l.significant_below_half,
                    l.largest_removed_m2
                );
            }
        }
    }
    Ok(())
}
