//! Polls the trail status of a site once:
//! `SKIMAP_PERSONAL_SOURCES=1 cargo run -p skimap-status --example poll -- <site>`.
//!
//! `SKIMAP_PERSONAL_SOURCES` stands for the `personal_sources` option until it exists in
//! `skimap_common::config::Config`.

use skimap_common::config::Config;
use skimap_common::download::Downloader;
use skimap_common::site;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let site = site::site(&std::env::args().nth(1).ok_or("usage: poll <site>")?)?;
    let personal = std::env::var("SKIMAP_PERSONAL_SOURCES").is_ok_and(|v| v == "1");
    let cfg = Config::from_env()?;
    let dl = Downloader::from_config(&cfg)?;
    let r = skimap_status::poll(site, &cfg.layout(), &dl, personal)?;
    println!("site={} polled_at={}", r.site, r.polled_at);
    for s in &r.sources {
        println!(
            "  {:<17} origin={:<12} changed={:<5} snapshot={:?}\n    {}\n    {}",
            s.source, s.origin, s.changed, s.snapshot, s.summary, s.url
        );
    }
    Ok(())
}
