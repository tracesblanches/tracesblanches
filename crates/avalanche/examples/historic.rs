//! Builds the historic avalanche layer of a site:
//! `cargo run -p skimap-avalanche --example historic -- <site> [overpass-dump.json]`.
//!
//! Without a dump the piste lines come from the site's `pistes` layer (OSM crate, P2),
//! which must be built first (`skimap pistes <site>`).

use std::time::Instant;

use skimap_common::config::Config;
use skimap_common::download::Downloader;
use skimap_common::site;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args().skip(1);
    let site = site::site(&args.next().ok_or("usage: historic <site> [dump.json]")?)?;
    let dump = args.next();
    let cfg = Config::from_env()?;
    let dl = Downloader::from_config(&cfg)?;
    let t = Instant::now();
    let s = match &dump {
        Some(p) => skimap_avalanche::historic::build_from_dump(
            site,
            &cfg.layout(),
            &dl,
            std::path::Path::new(p),
        )?,
        None => skimap_avalanche::historic::build(site, &cfg.layout(), &dl)?,
    };
    println!(
        "site={} covered={} pistes: {}",
        s.site, s.covered, s.pistes_source
    );
    println!(
        "CLPA in layer {}, in commune {} ({} below 100 m2) (+{} other records), commune CLPA area {} km2",
        s.clpa_in_box,
        s.clpa_in_commune,
        s.clpa_in_commune_slivers,
        s.other_in_commune,
        s.clpa_in_commune_km2
    );
    for n in &s.networks {
        println!(
            "network {:?}: {} lines, {} km, inside CLPA {} km ({} sections), inside any DDT05 {} km",
            n.network, n.lines, n.length_km, n.inside_clpa_km, n.crossings, n.inside_any_ddt05_km
        );
    }
    for p in s.pistes.iter().filter(|p| p.inside_clpa_km > 0.0) {
        println!(
            "  {:<32} {:?} {:>7.3} km, inside {:>6.3} km, {} sections",
            p.name, p.network, p.length_km, p.inside_clpa_km, p.crossings
        );
    }
    for r in &s.reconciliation {
        println!(
            "  report07 {:<30} {:>6.2} vs {:>7.3} (diff {:+.3})",
            r.item,
            r.report07,
            r.recomputed,
            r.recomputed - r.report07
        );
    }
    println!("bera: {:?}", s.bera);
    println!("notice: {}", s.notice);
    println!("elapsed {:.2?}", t.elapsed());
    Ok(())
}
