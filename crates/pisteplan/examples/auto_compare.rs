//! Transcription with the automatic sheet compared with the hand-written one, on both
//! sites, without writing any output: `auto_compare [site...]`. Prints the pistes
//! transcribed by either, the differences, and the sheets' key figures.
//!
//! `SKIMAP_PERSONAL_SOURCES=1 cargo run --release -p skimap-pisteplan --features pipeline --example auto_compare`

use std::collections::BTreeMap;
use std::time::Instant;

use skimap_common::config::Config;
use skimap_common::download::Downloader;
use skimap_common::geom::BBox;
use skimap_pisteplan::autospec::register_auto;
use skimap_pisteplan::build::{Registration, register};
use skimap_pisteplan::transcribe::{self, Decision, InkDistance};
use skimap_pisteplan::{Error, ocr, spec};

fn decisions(reg: &Registration) -> (Vec<Decision>, f64) {
    let plan = transcribe::entries(reg);
    let dmap = InkDistance::new(reg);
    let (ds, v) = transcribe::decide(reg, &dmap, &plan);
    (ds, v.radius_pt)
}

fn key(d: &Decision) -> String {
    format!("{} [{}]", d.name, d.ink.as_deref().unwrap_or("?"))
}

/// The site's box grown by `km` (a hint of where the plan is, as the census gives one).
fn grown(b: BBox, km: f64) -> BBox {
    let dlat = km / 111.0;
    let dlon = km / (111.0 * b.center().lat.to_radians().cos());
    BBox::new(b.south - dlat, b.west - dlon, b.north + dlat, b.east + dlon)
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let sites: Vec<String> = std::env::args().skip(1).collect();
    let cfg = Config::from_env()?;
    if !cfg.personal_sources {
        return Err(Error::PersonalSourcesDisabled.into());
    }
    let layout = cfg.layout();
    let dl = Downloader::from_config(&cfg)?;
    let planet = skimap_osm::extract::load_planet(&dl)?;
    let engine = ocr::engine(&dl)?;
    for manual in spec::manual_plans() {
        if !sites.is_empty() && !sites.contains(&manual.site) {
            continue;
        }
        let site = skimap_common::site::site(&manual.site)?;
        let t = Instant::now();
        let mreg = register(manual.clone(), &layout, &dl, &planet, Some(&engine))?;
        let (mds, mr) = decisions(&mreg);
        let tm = t.elapsed().as_secs_f64();
        let t = Instant::now();
        let a = register_auto(
            &manual.plan_source(),
            grown(site.bbox, 10.0),
            &layout,
            &dl,
            &planet,
            &engine,
        )?;
        let (ads, ar) = decisions(&a.reg);
        let ta = t.elapsed().as_secs_f64();
        let mut rows: BTreeMap<String, (Option<f64>, Option<f64>)> = BTreeMap::new();
        for d in mds.iter().filter(|d| d.status.is_some()) {
            rows.entry(key(d)).or_default().0 = Some(d.score);
        }
        for d in ads.iter().filter(|d| d.status.is_some()) {
            rows.entry(key(d)).or_default().1 = Some(d.score);
        }
        println!(
            "\n== {}: manual {} transcribed of {} ({tm:.0} s, radius {mr} pt) | auto {} of {} ({ta:.0} s, radius {ar} pt)",
            manual.id,
            mds.iter().filter(|d| d.status.is_some()).count(),
            mds.len(),
            ads.iter().filter(|d| d.status.is_some()).count(),
            ads.len()
        );
        println!(
            "extent manual {:?}\n       auto   {:?} (hint {:?})",
            manual.extent, a.auto.spec.extent, a.hint
        );
        println!(
            "registration manual: {} RANSAC inliers, {} ICP pairs, {} ties | auto: {} inliers, {} pairs, {} ties",
            mreg.coarse.inliers.len(),
            mreg.pairs.len(),
            mreg.ties.len(),
            a.reg.coarse.inliers.len(),
            a.reg.pairs.len(),
            a.reg.ties.len()
        );
        for (k, (m, au)) in &rows {
            let tag = match (m, au) {
                (Some(_), Some(_)) => "both  ",
                (Some(_), None) => "MANUAL",
                (None, Some(_)) => "AUTO  ",
                _ => "",
            };
            println!(
                "  {tag} {k:45} manual {:>5} auto {:>5}",
                m.map_or("-".into(), |s| format!("{s:.2}")),
                au.map_or("-".into(), |s| format!("{s:.2}"))
            );
        }
        // Local diagnostic (`AUTO_DIAG=<dir>`, never published): for each piste only one
        // sheet transcribes, the plan around its OSM geometry (magenta), to check by eye.
        if let Ok(dir) = std::env::var("AUTO_DIAG") {
            let dir = std::path::PathBuf::from(dir);
            std::fs::create_dir_all(&dir)?;
            for (reg, ds, tag) in [(&a.reg, &ads, "auto"), (&mreg, &mds, "manual")] {
                for d in ds.iter().filter(|d| d.status.is_some()) {
                    let only = rows
                        .get(&key(d))
                        .is_some_and(|r| r.0.is_none() || r.1.is_none());
                    if !only {
                        continue;
                    }
                    let k = reg.input.pt_per_px;
                    let pts: Vec<Vec<[f64; 2]>> = d
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
                    let all: Vec<[f64; 2]> = pts.iter().flatten().copied().collect();
                    let x0 = all.iter().map(|p| p[0]).fold(f64::MAX, f64::min).max(0.0) as usize;
                    let y0 = all.iter().map(|p| p[1]).fold(f64::MAX, f64::min).max(0.0) as usize;
                    let x1 = (all.iter().map(|p| p[0]).fold(0.0, f64::max) as usize)
                        .min(reg.input.image.w);
                    let y1 = (all.iter().map(|p| p[1]).fold(0.0, f64::max) as usize)
                        .min(reg.input.image.h);
                    let pad = 120;
                    let (x0, y0) = (x0.saturating_sub(pad), y0.saturating_sub(pad));
                    let (x1, y1) = (
                        (x1 + pad).min(reg.input.image.w),
                        (y1 + pad).min(reg.input.image.h),
                    );
                    if x1 <= x0 || y1 <= y0 {
                        continue;
                    }
                    let mut img =
                        skimap_pisteplan::image::Rgb::new(x1 - x0, y1 - y0, [255, 255, 255]);
                    for y in y0..y1 {
                        for x in x0..x1 {
                            img.put((x - x0) as i64, (y - y0) as i64, reg.input.image.get(x, y));
                        }
                    }
                    for l in &pts {
                        let l: Vec<[f64; 2]> = l
                            .iter()
                            .map(|p| [p[0] - x0 as f64, p[1] - y0 as f64])
                            .collect();
                        img.polyline(&l, 1.5, [255, 0, 255]);
                    }
                    let name = format!(
                        "{}_{tag}_{}.png",
                        manual.site,
                        d.name.replace([' ', '/', '\''], "_")
                    );
                    img.write_png(&dir.join(name))?;
                }
            }
        }
        // Hybrids: the automatic sheet with one part taken from the hand-written one
        // (`AUTO_HYBRIDS=1`), to see which part makes a difference.
        if std::env::var("AUTO_HYBRIDS").is_ok() {
            let names: Vec<String> = ads.iter().filter(|d| d.status.is_some()).map(key).collect();
            for part in [
                "inks",
                "masks",
                "map_area",
                "extent",
                "line",
                "all_but_inks",
            ] {
                let mut s = a.auto.spec.clone();
                match part {
                    "inks" => s.inks = manual.inks.clone(),
                    "masks" => s.masks = manual.masks.clone(),
                    "map_area" => s.map_area = manual.map_area,
                    "extent" => s.extent = manual.extent,
                    "line" => s.line = manual.line,
                    _ => {
                        let inks = s.inks.clone();
                        s = manual.clone();
                        s.inks = inks;
                    }
                }
                let reg = register(s, &layout, &dl, &planet, Some(&engine))?;
                let (hds, _) = decisions(&reg);
                let hn: Vec<String> = hds.iter().filter(|d| d.status.is_some()).map(key).collect();
                println!(
                    "  hybrid manual {part:13}: {} transcribed; gained {:?}; lost {:?}",
                    hn.len(),
                    hn.iter().filter(|n| !names.contains(n)).collect::<Vec<_>>(),
                    names.iter().filter(|n| !hn.contains(n)).collect::<Vec<_>>()
                );
            }
        }
        // Decisions that differ, with the auto reason.
        for d in &ads {
            if d.status.is_none() && mds.iter().any(|m| m.status.is_some() && key(m) == key(d)) {
                println!("  auto refused {}: {}", key(d), d.reason);
            }
        }
    }
    Ok(())
}
