//! Diagnostic of the line extraction: `inspect <site> <out_dir>` writes, in a local
//! directory (never in the outputs: the picture is the resort's), the plan faded with
//! the skeleton of each ink drawn over it.
//!
//! `SKIMAP_PERSONAL_SOURCES=1 cargo run --release -p skimap-pisteplan --features pipeline --example inspect -- chapelle-des-bois /tmp/x`

use std::path::PathBuf;
use std::time::Instant;

use skimap_common::config::Config;
use skimap_common::download::Downloader;
use skimap_pisteplan::{input, segment, spec};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().collect();
    let site = args.get(1).ok_or("usage: inspect <site> <out_dir>")?;
    let out = PathBuf::from(args.get(2).ok_or("usage: inspect <site> <out_dir>")?);
    std::fs::create_dir_all(&out)?;
    let cfg = Config::from_env()?;
    let dl = Downloader::from_config(&cfg)?;
    let spec = spec::plan_for(site).ok_or("no plan for this site")?;
    let t = Instant::now();
    let inp = input::load(spec, &cfg.layout(), &dl)?;
    println!(
        "loaded {}x{} in {:.2} s, {} text lines",
        inp.image.w,
        inp.image.h,
        t.elapsed().as_secs_f64(),
        inp.text.lines.len()
    );
    let t = Instant::now();
    let labels = segment::classify(spec, &inp.image, inp.pt_per_px);
    println!("classified in {:.2} s", t.elapsed().as_secs_f64());
    let (w, h) = (inp.image.w, inp.image.h);
    let mut img = inp.image.clone();
    img.fade(0.75);
    let mut cls = skimap_pisteplan::image::Rgb::new(w, h, [255, 255, 255]);
    for (i, l) in labels.iter().enumerate() {
        if *l != segment::NONE {
            let c = spec.inks[*l as usize].refs[0];
            cls.px[i * 3..i * 3 + 3].copy_from_slice(&c);
        }
    }
    cls.write_png(&out.join(format!("{site}_classes.png")))?;
    let scale = spec.dpi as f64 / 100.0;
    let params = spec.line;
    for (k, ink) in spec.inks.iter().enumerate() {
        let t = Instant::now();
        let (mask, g) = segment::ink_lines(&labels, w, h, k as u8, &params);
        let len: f64 = g.edges.iter().map(|e| e.len()).sum();
        let deg = g.degrees();
        println!(
            "{:7} mask {:8} px, {:5} nodes ({} junctions), {:5} edges, {:8.0} px of lines ({:.2} s)",
            ink.name,
            mask.count(),
            g.nodes.len(),
            deg.iter().filter(|&&d| d >= 3).count(),
            g.edges.len(),
            len,
            t.elapsed().as_secs_f64()
        );
        let c = if ink.name == "yellow" {
            [200, 160, 0]
        } else {
            ink.refs[0]
        };
        for e in &g.edges {
            img.polyline(&e.pts, 2.0 * scale, c);
        }
        for (n, p) in g.nodes.iter().enumerate() {
            if deg[n] >= 3 {
                img.disc(p[0], p[1], 3.0 * scale, [255, 0, 255]);
            }
        }
    }
    img.write_png(&out.join(format!("{site}_lines.png")))?;
    Ok(())
}
