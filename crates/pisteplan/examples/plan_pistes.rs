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
    for p in spec::PLANS {
        if !sites.is_empty() && !sites.iter().any(|s| s == p.site) {
            continue;
        }
        let t = Instant::now();
        let reg = register(p, &layout, &dl, &planet, Some(&engine))?;
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
            img.write_png(&std::path::Path::new(&dir).join(format!("{}_share.png", p.site)))?;
        }
    }
    Ok(())
}
