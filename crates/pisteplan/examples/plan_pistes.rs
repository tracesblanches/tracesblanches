//! Builds the `plan_pistes` layer of both sites (or those given) and prints the decisions.
//!
//! `SKIMAP_PERSONAL_SOURCES=1 cargo run --release -p skimap-pisteplan --features pipeline --example plan_pistes [site...]`

use std::time::Instant;

use skimap_common::config::Config;
use skimap_common::download::Downloader;
use skimap_pisteplan::build::{register, transcribe_and_write};
use skimap_pisteplan::{Error, ocr, spec};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .with_writer(std::io::stderr)
        .init();
    let cfg = Config::from_env()?;
    if !cfg.personal_sources {
        return Err(Error::PersonalSourcesDisabled.into());
    }
    let layout = cfg.layout();
    let dl = Downloader::from_config(&cfg)?;
    let sites: Vec<String> = std::env::args().skip(1).collect();
    let t = Instant::now();
    let planet = skimap_osm::extract::load_planet(&dl)?;
    let engine = ocr::engine(&dl)?;
    println!(
        "planet and OCR models loaded in {:.1} s",
        t.elapsed().as_secs_f64()
    );
    for p in spec::manual_plans() {
        if !sites.is_empty() && !sites.contains(&p.site) {
            continue;
        }
        let t = Instant::now();
        let reg = register(p, &layout, &dl, &planet, Some(&engine))?;
        let p = &reg.spec;
        let rep = transcribe_and_write(&reg, &layout, &planet.source)?;
        println!(
            "\n== {} -> {} ({:.1} s)",
            p.id,
            rep.dir.display(),
            t.elapsed().as_secs_f64()
        );
        for d in &rep.decisions {
            println!(
                "  {:38} {:7} {:16} score {:4.2} sim {:4.2} {}{}",
                d.entry.name_read,
                d.ink.as_deref().unwrap_or("?"),
                d.status
                    .map(|s| format!("{s:?}"))
                    .unwrap_or_else(|| "NON TRANSCRITE".into()),
                d.score,
                d.name_sim,
                if d.name != d.entry.name_read {
                    format!("→ « {} » ", d.name)
                } else {
                    String::new()
                },
                d.reason
            );
        }
        println!("{}", serde_json::to_string_pretty(&rep.stats)?);
        // Local diagnostic (`PLAN_DIAG_ML=<dir>`, never published): the plan around each
        // piste transcribed by the model, its OSM geometry in magenta, to check by eye.
        if let Ok(dir) = std::env::var("PLAN_DIAG_ML") {
            let dir = std::path::PathBuf::from(dir);
            std::fs::create_dir_all(&dir)?;
            let k = reg.input.pt_per_px;
            for d in rep
                .decisions
                .iter()
                .filter(|d| d.method == Some(skimap_pisteplan::transcript::MatchMethod::Ml))
            {
                let lines: Vec<Vec<[f64; 2]>> = d
                    .ways
                    .iter()
                    .map(|&w| {
                        reg.osm.ways[w]
                            .xy
                            .iter()
                            .map(|&q| {
                                let p = reg.warp.to_plan(q);
                                [p[0] / k, p[1] / k]
                            })
                            .collect()
                    })
                    .collect();
                let all: Vec<[f64; 2]> = lines.iter().flatten().copied().collect();
                let (w, h) = (reg.input.image.w, reg.input.image.h);
                let lo = |f: fn(&[f64; 2]) -> f64| all.iter().map(f).fold(f64::MAX, f64::min);
                let hi = |f: fn(&[f64; 2]) -> f64| all.iter().map(f).fold(0.0, f64::max);
                let x0 = (lo(|p| p[0]).max(0.0) as usize).saturating_sub(150);
                let y0 = (lo(|p| p[1]).max(0.0) as usize).saturating_sub(150);
                let x1 = ((hi(|p| p[0]) as usize) + 150).min(w);
                let y1 = ((hi(|p| p[1]) as usize) + 150).min(h);
                if x1 <= x0 || y1 <= y0 {
                    continue;
                }
                let mut img = skimap_pisteplan::image::Rgb::new(x1 - x0, y1 - y0, [255; 3]);
                for y in y0..y1 {
                    for x in x0..x1 {
                        img.put((x - x0) as i64, (y - y0) as i64, reg.input.image.get(x, y));
                    }
                }
                for l in &lines {
                    let l: Vec<[f64; 2]> = l
                        .iter()
                        .map(|q| [q[0] - x0 as f64, q[1] - y0 as f64])
                        .collect();
                    img.polyline(&l, 1.5, [255, 0, 255]);
                }
                let name = d.name.replace([' ', '/', '\''], "_");
                img.write_png(&dir.join(format!("{}_ml_{name}.png", p.site)))?;
            }
        }
        // Local diagnostic (never published: the picture is the resort's): samples of the
        // drawn-share test, ink found (green) or not (magenta).
        if let Ok(dir) = std::env::var("PLAN_DIAG") {
            let k = reg.input.pt_per_px;
            let mut img = reg.input.image.clone();
            img.fade(0.6);
            let only = std::env::var("PLAN_DIAG_ONLY").ok();
            let dmap = skimap_pisteplan::transcribe::InkDistance::new(&reg);
            let radius = rep.stats["validation"]["radius_pt"].as_f64().unwrap_or(3.0);
            for d in &rep.decisions {
                let Some(ink) = d.entry.ink else { continue };
                if only.as_deref().is_some_and(|o| o != d.entry.name_read) {
                    continue;
                }
                let lines: Vec<&[[f64; 2]]> = d
                    .ways
                    .iter()
                    .map(|&w| reg.osm.ways[w].xy.as_slice())
                    .collect();
                for (p, dist) in
                    skimap_pisteplan::transcribe::drawn_samples(&reg, &dmap, &lines, ink)
                {
                    let c = if dist <= radius {
                        [0, 160, 0]
                    } else {
                        [255, 0, 255]
                    };
                    img.disc(p[0] / k, p[1] / k, 2.5, c);
                }
            }
            // Arrows read on the plan: a blue tick from the centre along the direction.
            if p.arrows {
                let s = p.dpi as f64 / 100.0;
                let arrows = skimap_pisteplan::arrows::detect(
                    &reg,
                    &dmap,
                    (12.0 * s * s) as usize,
                    (120.0 * s * s) as usize,
                );
                for a in &arrows {
                    let c = [a.p[0] / k, a.p[1] / k];
                    let e = [c[0] + a.dir[0] * 14.0, c[1] + a.dir[1] * 14.0];
                    img.segment(c, e, 2.0, [0, 0, 255]);
                    img.disc(c[0], c[1], 3.0, [0, 0, 255]);
                }
            }
            img.write_png(&std::path::Path::new(&dir).join(format!("{}_share.png", p.site)))?;
        }
    }
    Ok(())
}
