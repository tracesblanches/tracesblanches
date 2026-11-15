//! Automatic labelling: every collected plan (catalogue of `collect_plans`) is read with
//! an automatic sheet and registered; the exact matching labels the candidate pairs
//! (legend piste ↔ OSM piste) written to `data/out/france/plan_training/plans/<id>.json`
//! (usage personnel). Resumable: a plan already written is skipped. The two sites' plans
//! are read the same way (test only, never trained on).
//!
//! `SKIMAP_PERSONAL_SOURCES=1 cargo run --release -p skimap-pisteplan --features pipeline --example train_pairs [jobs]`

use std::path::{Path, PathBuf};
use std::time::Instant;

use serde_json::json;
use skimap_common::config::Config;
use skimap_common::geom::BBox;
use skimap_pisteplan::autospec::register_auto;
use skimap_pisteplan::collect::{self, Collected, Domain};
use skimap_pisteplan::spec::PlanSource;
use skimap_pisteplan::transcribe::{self, InkDistance};
use skimap_pisteplan::{Error, learn, ocr, spec};

fn grown(b: BBox, km: f64) -> BBox {
    let dlat = km / 111.0;
    let dlon = km / (111.0 * b.center().lat.to_radians().cos());
    BBox::new(b.south - dlat, b.west - dlon, b.north + dlat, b.east + dlon)
}

fn union(bs: &[BBox]) -> Option<BBox> {
    let first = *bs.first()?;
    Some(bs.iter().fold(first, |a, b| {
        BBox::new(
            a.south.min(b.south),
            a.west.min(b.west),
            a.north.max(b.north),
            a.east.max(b.east),
        )
    }))
}

struct Job {
    src: PlanSource,
    hint: BBox,
    format: String,
    test: bool,
}

fn run_one(
    job: &Job,
    out: &Path,
    census: &[Domain],
    layout: &skimap_common::layout::DataLayout,
    dl: &skimap_common::download::Downloader,
    planet: &skimap_osm::extract::Planet,
    engine: &ocrs::OcrEngine,
) -> String {
    let path = out.join(format!("{}.json", job.src.id));
    if path.exists() {
        return format!("{}: déjà fait", job.src.id);
    }
    let t = Instant::now();
    let res = register_auto(&job.src, job.hint, layout, dl, planet, engine);
    let doc = match res {
        Ok(a) => {
            let reg = &a.reg;
            let t_reg = t.elapsed().as_secs_f64();
            let dmap = InkDistance::new(reg);
            let plan = transcribe::entries(reg);
            let (ds, v) = transcribe::decide(reg, &dmap, &plan);
            let t_exact = t.elapsed().as_secs_f64();
            let pairs = learn::pairs(reg, &dmap, &ds);
            let fit = |p: [f64; 2]| reg.warp.to_map(p);
            let drawn = skimap_pisteplan::build::extent_of(&reg.spec.map_area, &fit, 0.0).ok();
            let domains: Vec<&str> = census
                .iter()
                .filter(|d| {
                    let c = d.bbox.center();
                    drawn.is_some_and(|e| e.contains(c))
                })
                .map(|d| d.id.as_str())
                .collect();
            let ties: Vec<f64> = reg.ties.iter().map(|t| t.residual_m).collect();
            json!({
                "id": job.src.id, "site": job.src.site, "test": job.test, "format": job.format,
                "url": job.src.url, "title": job.src.title,
                "ok": true, "seconds": t.elapsed().as_secs_f64(),
                "drawn_extent": drawn, "domains_drawn": domains,
                "legend_lines": plan.len(),
                "exact": ds.iter().filter(|d| d.status.is_some()).count(),
                "radius_pt": v.radius_pt,
                "timings_s": {"register": t_reg, "exact": t_exact - t_reg, "pairs": t.elapsed().as_secs_f64() - t_exact, "steps": reg.timings},
                "ransac_inliers": reg.coarse.inliers.len(), "icp_pairs": reg.pairs.len(),
                "ties": reg.ties.len(), "tie_p90_m": skimap_pisteplan::warp::quantile(&ties, 0.9),
                "inks": a.auto.spec.inks.iter().map(|i| i.name.clone()).collect::<Vec<_>>(),
                "labels": format!("{:?}", a.auto.spec.labels),
                "pairs": pairs,
            })
        }
        Err(e) => json!({
            "id": job.src.id, "site": job.src.site, "test": job.test, "format": job.format,
            "url": job.src.url, "ok": false, "error": e.to_string(),
            "seconds": t.elapsed().as_secs_f64(),
        }),
    };
    let msg = format!(
        "{}: {} ({:.0} s)",
        job.src.id,
        if doc["ok"].as_bool() == Some(true) {
            format!(
                "{} lignes, {} exactes, {} paires",
                doc["legend_lines"],
                doc["exact"],
                doc["pairs"].as_array().map_or(0, Vec::len)
            )
        } else {
            format!("ÉCHEC {}", doc["error"])
        },
        t.elapsed().as_secs_f64()
    );
    let tmp = path.with_extension("part");
    if std::fs::write(&tmp, serde_json::to_vec(&doc).unwrap_or_default()).is_ok() {
        let _ = std::fs::rename(&tmp, &path);
    }
    msg
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let jobs_n: usize = std::env::args()
        .nth(1)
        .and_then(|s| s.parse().ok())
        .unwrap_or(2);
    let cfg = Config::from_env()?;
    if !cfg.personal_sources {
        return Err(Error::PersonalSourcesDisabled.into());
    }
    let layout = cfg.layout();
    let dl = collect::downloader(&cfg)?;
    let census = collect::load_census(&layout)?;
    let out: PathBuf = layout.france_layer_dir("plan_training")?.join("plans");
    std::fs::create_dir_all(&out)?;
    let cat_path = layout
        .france_layer_dir(collect::LAYER)?
        .join("catalogue.json");
    let cat: Vec<Collected> = serde_json::from_slice(&std::fs::read(&cat_path)?)?;
    let mut jobs: Vec<Job> = Vec::new();
    // The two sites (test only).
    for m in spec::manual_plans() {
        let site = skimap_common::site::site(&m.site)?;
        jobs.push(Job {
            src: m.plan_source(),
            hint: grown(site.bbox, 10.0),
            format: format!("{:?}", m.labels),
            test: true,
        });
    }
    for c in &cat {
        let fmt = c.format.clone().unwrap_or_default();
        if c.duplicate_of.is_some() || c.error.is_some() || !fmt.starts_with("pdf") {
            continue;
        }
        let boxes: Vec<BBox> = census
            .iter()
            .filter(|d| c.candidate.domains.contains(&d.id))
            .map(|d| d.bbox)
            .collect();
        let Some(b) = union(&boxes) else { continue };
        jobs.push(Job {
            src: PlanSource {
                site: c.candidate.domains[0].clone(),
                id: c.candidate.id.clone(),
                title: c.candidate.title.clone(),
                publisher: c.candidate.publisher.clone(),
                url: c.candidate.url.clone(),
                page_url: c.candidate.page_url.clone(),
                source: c.candidate.source.clone(),
                edition: c.created.clone().unwrap_or_default(),
                rights: "aucune licence publiée : usage personnel".into(),
            },
            hint: grown(b, 10.0),
            format: fmt,
            test: false,
        });
    }
    // Most likely to register first: the test sites, the ENJ plans (one cartographer, text
    // layer), the plans with a text layer, then the pictures.
    jobs.sort_by_key(|j| {
        (
            !j.test,
            !j.src.id.starts_with("enj-"),
            j.format != "pdf_text",
        )
    });
    // `TRAIN_ONLY=<id>,<id>`: only these plans (diagnostics, timing).
    if let Ok(only) = std::env::var("TRAIN_ONLY") {
        let only: Vec<&str> = only.split(',').collect();
        jobs.retain(|j| only.contains(&j.src.id.as_str()));
    }
    println!("{} plans to read ({} at a time)", jobs.len(), jobs_n);
    let planet = skimap_osm::extract::load_planet(&dl)?;
    let engine = ocr::engine(&dl)?;
    // `jobs_n` workers take the plans in turn; each registration still uses the global
    // rayon pool inside.
    let t = Instant::now();
    let next = std::sync::atomic::AtomicUsize::new(0);
    std::thread::scope(|s| {
        for _ in 0..jobs_n {
            s.spawn(|| {
                loop {
                    let i = next.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                    let Some(j) = jobs.get(i) else { break };
                    let m = run_one(j, &out, &census, &layout, &dl, &planet, &engine);
                    println!("[{:5.0} s] {m}", t.elapsed().as_secs_f64());
                }
            });
        }
    });
    Ok(())
}
