//! Why a collected plan does not register: `diag_plan <catalogue id> [diag_dir]` prints the
//! automatic sheet (inks, legend lines, map area), the labels on the map and their
//! candidates among the landmarks of the hint extent; in `diag_dir` (local, never
//! published) writes the plan with the map area, masks and legend lines drawn over it.
//!
//! `SKIMAP_PERSONAL_SOURCES=1 cargo run --release -p skimap-pisteplan --features pipeline --example diag_plan -- nf-2805 /tmp/diag`

use skimap_common::config::Config;
use skimap_common::download::Request;
use skimap_common::geom::BBox;
use skimap_pisteplan::autospec::{self, choose_dpi, label_source};
use skimap_pisteplan::build::ocr_page;
use skimap_pisteplan::collect::{self, Collected};
use skimap_pisteplan::spec::{LabelSource, PageRect, PlanSource};
use skimap_pisteplan::{Error, align, input, ocr, osmref};

fn rect(img: &mut skimap_pisteplan::image::Rgb, r: PageRect, k: f64, c: [u8; 3], w: f64) {
    let p = |x: f64, y: f64| [x / k, y / k];
    let (a, b, cc, d) = (p(r[0], r[1]), p(r[2], r[1]), p(r[2], r[3]), p(r[0], r[3]));
    img.polyline(&[a, b, cc, d, a], w, c);
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().collect();
    let id = args.get(1).ok_or("usage: diag_plan <id> [dir]")?;
    let cfg = Config::from_env()?;
    if !cfg.personal_sources {
        return Err(Error::PersonalSourcesDisabled.into());
    }
    let layout = cfg.layout();
    let dl = collect::downloader(&cfg)?;
    let census = collect::load_census(&layout)?;
    let cat: Vec<Collected> = serde_json::from_slice(&std::fs::read(
        layout
            .france_layer_dir(collect::LAYER)?
            .join("catalogue.json"),
    )?)?;
    let c = cat
        .iter()
        .find(|c| c.candidate.id == *id)
        .ok_or("not in the catalogue")?;
    let boxes: Vec<BBox> = census
        .iter()
        .filter(|d| c.candidate.domains.contains(&d.id))
        .map(|d| d.bbox)
        .collect();
    let b = boxes.first().copied().ok_or("no domain")?;
    let km = 10.0;
    let (dlat, dlon) = (km / 111.0, km / (111.0 * b.center().lat.to_radians().cos()));
    let hint = boxes.iter().fold(
        BBox::new(b.south - dlat, b.west - dlon, b.north + dlat, b.east + dlon),
        |a, x| {
            BBox::new(
                a.south.min(x.south - dlat),
                a.west.min(x.west - dlon),
                a.north.max(x.north + dlat),
                a.east.max(x.east + dlon),
            )
        },
    );
    let src = PlanSource {
        site: c.candidate.domains[0].clone(),
        id: c.candidate.id.clone(),
        title: c.candidate.title.clone(),
        publisher: c.candidate.publisher.clone(),
        url: c.candidate.url.clone(),
        page_url: c.candidate.page_url.clone(),
        source: c.candidate.source.clone(),
        edition: String::new(),
        rights: String::new(),
    };
    let fetched = dl.fetch(&Request::get(&src.source, &src.url).max_age(input::PDF_MAX_AGE))?;
    let text = input::text_layer(&fetched, &layout)?;
    let probe = input::probe(&fetched.path, &text)?;
    let dpi = choose_dpi(&probe);
    let inp = input::load_fetched(fetched, dpi, &layout)?;
    let engine = ocr::engine(&dl)?;
    let lines = match label_source(&probe) {
        LabelSource::TextLayer => inp.text.lines.iter().map(|(l, _)| l.clone()).collect(),
        LabelSource::Ocr => ocr_page(&engine, &inp, false, &layout)?,
    };
    let auto = autospec::detect(&src, &probe, &inp.image, inp.pt_per_px, &lines, hint);
    let s = &auto.spec;
    println!(
        "{id} {} dpi {} labels {:?}, {} text lines, {} legend lines, line width {:.2} px",
        c.candidate.title,
        s.dpi,
        s.labels,
        lines.len(),
        auto.report.entries.len(),
        auto.report.line_width_px
    );
    for f in &auto.report.inks {
        println!(
            "  ink {:7} {:?} max_de {:.1} kind {:?} entries {} map px {}",
            f.ink.name, f.ink.refs, f.ink.max_de, f.ink.kind, f.entries, f.map_pixels
        );
    }
    println!(
        "  map area {:?} (page {:?}), {} masks, network {:?}",
        s.map_area.map(f64::round),
        probe.page_pt,
        s.masks.len(),
        auto.report.network.map(|r| r.map(f64::round))
    );
    let map_labels: Vec<_> = lines
        .iter()
        .filter(|l| {
            let c = l.center();
            s.on_map(c[0], c[1])
        })
        .cloned()
        .collect();
    let (lms, src_l) = osmref::landmarks_any(&dl, &hint)?;
    let (min_sim, ele) = match s.labels {
        LabelSource::TextLayer => (0.92, Some(30.0)),
        LabelSource::Ocr => (0.75, None),
    };
    let cands = align::label_candidates(&map_labels, &lms, min_sim, ele);
    println!(
        "  {} labels on the map, {} landmarks ({}), {} label candidates",
        map_labels.len(),
        lms.len(),
        src_l.name,
        cands.len()
    );
    for cnd in cands.iter().take(15) {
        println!("    {:30} ↔ {}", cnd.label, lms[cnd.landmark].name);
    }
    for (r, n) in [(45.0, "45°"), (180.0, "180°")] {
        match align::coarse(&cands, 15.0, r) {
            Some(co) => println!(
                "  RANSAC {n}: {} inliers, {:.1} m/pt",
                co.inliers.len(),
                co.scale
            ),
            None => println!("  RANSAC {n}: none"),
        }
    }
    if let Some(dir) = args.get(2) {
        let dir = std::path::PathBuf::from(dir);
        std::fs::create_dir_all(&dir)?;
        let k = inp.pt_per_px;
        let mut img = inp.image.clone();
        img.fade(0.4);
        rect(&mut img, s.map_area, k, [0, 0, 255], 4.0);
        for m in &s.masks {
            rect(&mut img, *m, k, [230, 0, 230], 3.0);
        }
        for e in &auto.report.entries {
            rect(&mut img, e.rect, k, [255, 120, 0], 2.0);
        }
        for l in &map_labels {
            rect(&mut img, l.rect, k, [0, 160, 0], 1.0);
        }
        img.write_png(&dir.join(format!("{id}.png")))?;
    }
    Ok(())
}
