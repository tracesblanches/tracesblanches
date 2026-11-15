//! Finds and downloads the official plans of the census domains (Nordic France media
//! library, Espace Nordique Jurassien), then writes `data/out/france/plans/`.
//!
//! `SKIMAP_PERSONAL_SOURCES=1 cargo run --release -p skimap-pisteplan --features pipeline --example collect_plans`

use std::time::Instant;

use skimap_common::config::Config;
use skimap_pisteplan::Error;
use skimap_pisteplan::collect;

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
    let dl = collect::downloader(&cfg)?;
    let census = collect::load_census(&layout)?;
    let t = Instant::now();
    let (mut cands, rejected) = collect::discover_nordicfrance(&dl, &census)?;
    println!(
        "Nordic France: {} candidates, {} rejected ({:.0} s)",
        cands.len(),
        rejected.len(),
        t.elapsed().as_secs_f64()
    );
    let enj = collect::discover_enj(&dl, &census)?;
    println!(
        "ENJ: {} candidates ({:.0} s)",
        enj.len(),
        t.elapsed().as_secs_f64()
    );
    cands.extend(enj);
    let col = collect::fetch_all(&cands, &dl, &layout);
    for c in &col {
        println!(
            "{:28} {:10} {:>3} p {:>5} w {:22} {:?} {}{}",
            c.candidate.id,
            c.format.as_deref().unwrap_or("-"),
            c.pages.unwrap_or(0),
            c.words.unwrap_or(0),
            c.created
                .as_deref()
                .unwrap_or("-")
                .chars()
                .take(22)
                .collect::<String>(),
            c.candidate.domains,
            c.candidate.title,
            c.error
                .as_deref()
                .map(|e| format!(" ERROR {e}"))
                .or(c.duplicate_of.as_deref().map(|d| format!(" = {d}")))
                .unwrap_or_default()
        );
    }
    let stats = collect::write_catalogue(&layout, &col, &rejected, &census)?;
    println!(
        "{}\n({:.0} s)",
        serde_json::to_string_pretty(&stats)?,
        t.elapsed().as_secs_f64()
    );
    Ok(())
}
