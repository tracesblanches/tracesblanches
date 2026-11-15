//! Registration diagnostics: `register <diag_dir> [site...]` registers the plans and
//! prints the key figures; in `diag_dir` (a local directory, never the outputs: the
//! picture is the resort's) it writes the plan with the OSM pistes carried onto it.
//!
//! `SKIMAP_PERSONAL_SOURCES=1 cargo run --release -p skimap-pisteplan --features pipeline --example register -- /tmp/diag`

use std::path::PathBuf;
use std::time::Instant;

use skimap_common::config::Config;
use skimap_common::download::Downloader;
use skimap_pisteplan::build::{ErrStats, register};
use skimap_pisteplan::{ocr, spec};

fn colour(c: Option<&str>) -> [u8; 3] {
    match c {
        Some("green") => [0, 170, 0],
        Some("blue") => [0, 90, 255],
        Some("red") => [230, 0, 0],
        Some("black") => [0, 0, 0],
        _ => [150, 0, 150],
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .with_writer(std::io::stderr)
        .init();
    let args: Vec<String> = std::env::args().collect();
    let diag = PathBuf::from(args.get(1).ok_or("usage: register <diag_dir> [site...]")?);
    std::fs::create_dir_all(&diag)?;
    let sites: Vec<String> = if args.len() > 2 {
        args[2..].to_vec()
    } else {
        spec::PLANS.iter().map(|p| p.site.to_owned()).collect()
    };
    let cfg = Config::from_env()?;
    let dl = Downloader::from_config(&cfg)?;
    let t = Instant::now();
    let planet = skimap_osm::extract::load_planet(&dl)?;
    println!("planet loaded in {:.1} s", t.elapsed().as_secs_f64());
    let engine = ocr::engine(&dl)?;
    for site in sites {
        let spec = spec::plan_for(&site).ok_or("no plan")?;
        let t = Instant::now();
        let r = register(spec, &cfg.layout(), &dl, &planet, Some(&engine))?;
        println!(
            "\n== {} ({:.1} s) {:?}",
            spec.id,
            t.elapsed().as_secs_f64(),
            r.timings
        );
        println!(
            "lines {} (map {}), ocr {}, legend {}, osm ways {}, relations {}, landmarks {}, junctions {}",
            r.lines.len(),
            r.map_labels.len(),
            r.ocr_lines.len(),
            r.legend.len(),
            r.osm.ways.len(),
            r.osm.relations.len(),
            r.osm.landmarks.len(),
            r.osm.junctions.len()
        );
        for e in &r.legend {
            println!(
                "  legend {:40} ink {:7} {:5.1} km  [{}]",
                e.name,
                e.ink.map_or("?", |i| spec.inks[i].name),
                e.length_km.unwrap_or(f64::NAN),
                e.departure.as_deref().unwrap_or("-")
            );
        }
        println!(
            "candidates {}, RANSAC inliers {}, scale {:.2} m/pt",
            r.candidates.len(),
            r.coarse.inliers.len(),
            r.coarse.scale
        );
        for c in &r.coarse.inliers {
            println!(
                "  inlier {:35} -> {:30} {:6.0} m",
                c.label,
                r.osm.landmarks[c.landmark].name,
                skimap_pisteplan::warp::dist(
                    r.coarse.affine.apply(skimap_pisteplan::align::up(c.plan)),
                    c.map
                )
            );
        }
        for s in &r.steps {
            println!(
                "  icp r {:5.0} λ {:5.2} pairs {:5} median {:6.1} m",
                s.radius_m, s.lambda, s.pairs, s.median_m
            );
        }
        println!("ties {}", r.ties.len());
        for l in &r.loo {
            println!(
                "  LOO {:40} n {:4} median {:7.1} p90 {:7.1} max {:7.1}",
                l.model,
                l.n,
                l.median_m.unwrap_or(f64::NAN),
                l.p90_m.unwrap_or(f64::NAN),
                l.max_m.unwrap_or(f64::NAN)
            );
        }
        let h = ErrStats::of(&r.held_out_m);
        println!(
            "held-out dense warp: n {} median {:?} p90 {:?}",
            h.n, h.median_m, h.p90_m
        );
        // Diagnostic image.
        let k = r.input.pt_per_px;
        let mut img = r.input.image.clone();
        img.fade(0.7);
        for w in &r.osm.ways {
            let pts: Vec<[f64; 2]> =
                w.xy.iter()
                    .map(|&q| {
                        let p = r.warp.to_plan(q);
                        [p[0] / k, p[1] / k]
                    })
                    .collect();
            img.polyline(&pts, 1.5, colour(w.colour.as_deref()));
        }
        for t in &r.ties {
            img.disc(t.plan[0] / k, t.plan[1] / k, 5.0, [255, 0, 255]);
        }
        for c in &r.coarse.inliers {
            img.disc(c.plan[0] / k, c.plan[1] / k, 6.0, [0, 200, 200]);
        }
        img.write_png(&diag.join(format!("{site}_osm_on_plan.png")))?;
    }
    Ok(())
}
