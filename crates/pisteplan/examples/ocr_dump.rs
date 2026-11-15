//! Prints the OCR lines of a plan: `ocr_dump <site>` (centre in page points, on the map
//! or not, text).

use skimap_common::config::Config;
use skimap_common::download::Downloader;
use skimap_pisteplan::{input, ocr, spec};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let site = std::env::args().nth(1).ok_or("usage: ocr_dump <site>")?;
    let cfg = Config::from_env()?;
    let dl = Downloader::from_config(&cfg)?;
    let spec = spec::plan_for(&site).ok_or("no plan")?;
    let inp = input::load(spec, &cfg.layout(), &dl)?;
    let e = ocr::engine(&dl)?;
    let t = std::time::Instant::now();
    let mode = std::env::args().nth(2).unwrap_or_default();
    let img = if mode.contains("tophat") {
        ocr::enhance_text(&inp.image, 6, 4.0)
    } else {
        inp.image.clone()
    };
    if let Some(p) = std::env::args().nth(3) {
        img.write_png(std::path::Path::new(&p))?;
    }
    let lines = if mode.contains("tiled") {
        ocr::read_lines_tiled(&e, &img, 800, 200, inp.pt_per_px)?
    } else {
        ocr::read_lines(&e, &img, [0, 0, img.w, img.h], inp.pt_per_px)?
    };
    eprintln!(
        "{} lines in {:.1} s",
        lines.len(),
        t.elapsed().as_secs_f64()
    );
    for l in lines {
        let c = l.center();
        println!(
            "{:6.1} {:6.1} {} | {}",
            c[0],
            c[1],
            spec.on_map(c[0], c[1]),
            l.text
        );
    }
    Ok(())
}
