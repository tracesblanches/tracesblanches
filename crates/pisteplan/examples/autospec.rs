//! Automatic reading sheets of the two sites compared with the hand-written ones:
//! `autospec [diag_dir] [site...]`. Prints resolution, label source, inks, line
//! parameters, map area and masks side by side; in `diag_dir` (a local directory, never
//! the outputs: the picture is the resort's) writes the plan with the detected boxes
//! (magenta), map area (blue) and hand-written map area and masks (green).
//!
//! `SKIMAP_PERSONAL_SOURCES=1 cargo run --release -p skimap-pisteplan --features pipeline --example autospec -- /tmp/diag`

use std::path::PathBuf;
use std::time::Instant;

use skimap_common::config::Config;
use skimap_common::download::Downloader;
use skimap_pisteplan::autospec::{self, de};
use skimap_pisteplan::build::ocr_page;
use skimap_pisteplan::image::{Rgb, lab};
use skimap_pisteplan::spec::{LabelSource, PageRect};
use skimap_pisteplan::{Error, input, ocr, spec};

fn rect(img: &mut Rgb, r: PageRect, k: f64, c: [u8; 3], w: f64) {
    let p = |x: f64, y: f64| [x / k, y / k];
    let (a, b, cc, d) = (p(r[0], r[1]), p(r[2], r[1]), p(r[2], r[3]), p(r[0], r[3]));
    img.polyline(&[a, b, cc, d, a], w, c);
}

fn overlap(a: PageRect, b: PageRect) -> f64 {
    let w = (a[2].min(b[2]) - a[0].max(b[0])).max(0.0);
    let h = (a[3].min(b[3]) - a[1].max(b[1])).max(0.0);
    w * h
}

fn iou(a: PageRect, b: PageRect) -> f64 {
    let i = overlap(a, b);
    let area = |r: PageRect| (r[2] - r[0]) * (r[3] - r[1]);
    i / (area(a) + area(b) - i)
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().collect();
    let diag = args.get(1).map(PathBuf::from);
    let sites: Vec<String> = args.iter().skip(2).cloned().collect();
    let cfg = Config::from_env()?;
    if !cfg.personal_sources {
        return Err(Error::PersonalSourcesDisabled.into());
    }
    let layout = cfg.layout();
    let dl = Downloader::from_config(&cfg)?;
    let engine = ocr::engine(&dl)?;
    for manual in spec::manual_plans() {
        if !sites.is_empty() && !sites.contains(&manual.site) {
            continue;
        }
        let t = Instant::now();
        let fetched = input::fetch(&manual, &dl)?;
        let text0 = input::load_fetched(fetched.clone(), 50, &layout)?.text;
        let probe = input::probe(&fetched.path, &text0)?;
        let dpi = autospec::choose_dpi(&probe);
        let inp = input::load_fetched(fetched, dpi, &layout)?;
        let lines = match autospec::label_source(&probe) {
            LabelSource::TextLayer => inp.text.lines.iter().map(|(l, _)| l.clone()).collect(),
            LabelSource::Ocr => ocr_page(&engine, &inp, false, &layout)?,
        };
        let auto = autospec::detect(
            &manual.plan_source(),
            &probe,
            &inp.image,
            inp.pt_per_px,
            &lines,
            manual.extent,
        );
        let s = &auto.spec;
        println!("\n== {} ({:.1} s)", manual.id, t.elapsed().as_secs_f64());
        println!(
            "probe: pages {}, page {:?} pt, words {}, images {}, picture_only {}",
            probe.pages,
            probe.page_pt,
            probe.words,
            probe.images.len(),
            probe.picture_only()
        );
        println!("dpi: auto {} / manual {}", s.dpi, manual.dpi);
        println!("labels: auto {:?} / manual {:?}", s.labels, manual.labels);
        println!(
            "legend lines: {} (with dash colour {})",
            auto.report.entries.len(),
            auto.report
                .entries
                .iter()
                .filter(|e| e.dash.is_some())
                .count()
        );
        for f in &auto.report.inks {
            let m = manual.inks.iter().find(|i| i.name == f.ink.name);
            let dmin = |c: [u8; 3]| {
                m.map(|m| {
                    m.refs
                        .iter()
                        .map(|r| de(lab(*r), lab(c)))
                        .fold(f32::INFINITY, f32::min)
                })
            };
            println!(
                "  ink {:7} refs {:?} (ΔE to manual {:?}) legend {:?} (ΔE {:?}) max_de {:.1}/{:?} kind {:?}/{:?} diff {:?}/{:?} entries {} map px {}",
                f.ink.name,
                f.ink.refs,
                f.ink.refs.first().and_then(|c| dmin(*c)),
                f.legend_colours,
                f.legend_colours.first().and_then(|c| dmin(*c)),
                f.ink.max_de,
                m.map(|m| m.max_de),
                f.ink.kind,
                m.map(|m| m.kind),
                f.ink.difficulty,
                m.and_then(|m| m.difficulty.clone()),
                f.entries,
                f.map_pixels
            );
        }
        for m in &manual.inks {
            if !s.inks.iter().any(|i| i.name == m.name) {
                println!("  manual ink {} not found", m.name);
            }
        }
        println!(
            "line width {:.2} px; auto {:?}\n  manual {:?}",
            auto.report.line_width_px, s.line, manual.line
        );
        println!(
            "map area auto {:?} manual {:?} IoU {:.2}; network {:?}",
            s.map_area.map(|v| v.round()),
            manual.map_area.map(|v| v.round()),
            iou(s.map_area, manual.map_area),
            auto.report.network.map(|r| r.map(|v| v.round()))
        );
        let marea: f64 = manual
            .masks
            .iter()
            .map(|r| (r[2] - r[0]) * (r[3] - r[1]))
            .sum();
        let covered: f64 = manual
            .masks
            .iter()
            .map(|r| s.masks.iter().map(|b| overlap(*r, *b)).sum::<f64>())
            .sum();
        println!(
            "masks auto {} manual {}: manual mask area covered by auto masks {:.0} %",
            s.masks.len(),
            manual.masks.len(),
            100.0 * covered / marea.max(1.0)
        );
        for b in &auto.report.boxes {
            println!("  box {:?}", b.map(|v| v.round()));
        }
        if let Some(dir) = &diag {
            std::fs::create_dir_all(dir)?;
            let k = inp.pt_per_px;
            let mut img = inp.image.clone();
            img.fade(0.5);
            for m in &manual.masks {
                rect(&mut img, *m, k, [0, 170, 0], 6.0);
            }
            rect(&mut img, manual.map_area, k, [0, 170, 0], 10.0);
            for b in &auto.report.boxes {
                rect(&mut img, *b, k, [230, 0, 230], 3.0);
            }
            rect(&mut img, s.map_area, k, [0, 0, 255], 4.0);
            if let Some(n) = auto.report.network {
                rect(&mut img, n, k, [0, 200, 255], 2.0);
            }
            for e in &auto.report.entries {
                rect(&mut img, e.rect, k, [255, 120, 0], 2.0);
            }
            img.write_png(&dir.join(format!("{}_autospec.png", manual.site)))?;
            // Kept line pixels of each ink (auto sheet), and of the hand-written sheet.
            for (tag, sp) in [("auto", s), ("manual", &manual)] {
                let classes = skimap_pisteplan::segment::classify(sp, &inp.image, k);
                let mut out = Rgb::new(inp.image.w, inp.image.h, [255, 255, 255]);
                for (i, ink) in sp.inks.iter().enumerate() {
                    let (kept, _) = skimap_pisteplan::segment::ink_lines(
                        &classes,
                        inp.image.w,
                        inp.image.h,
                        i as u8,
                        &sp.line,
                    );
                    let c = if ink.name == "yellow" {
                        [200, 160, 0]
                    } else {
                        ink.refs[0]
                    };
                    for (j, v) in kept.v.iter().enumerate() {
                        if *v != 0 {
                            out.px[j * 3..j * 3 + 3].copy_from_slice(&c);
                        }
                    }
                }
                out.write_png(&dir.join(format!("{}_kept_{tag}.png", manual.site)))?;
            }
            std::fs::write(
                dir.join(format!("{}_autospec.json", manual.site)),
                serde_json::to_string_pretty(&auto)?,
            )?;
        }
    }
    Ok(())
}
