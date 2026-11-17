//! National rules and status of the local rules (G4, #120) on every site of the registry
//! whose zone layer is built: per site, the national rules that apply, the zones with an
//! act of their own and how far it was read; then the totals. Offline, read only.
//!
//! ```sh
//! SKIMAP_DATA_DIR=data/public cargo run --release -p skimap-zones --example local_rules
//! ```

use std::collections::BTreeMap;
use std::time::Instant;

use skimap_common::config::Config;
use skimap_common::site::Registry;
use skimap_zones::local::{LocalStatus, site_local_rules};
use skimap_zones::{RuleSet, ZoneLayer};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let t0 = Instant::now();
    let layout = Config::from_env()?.layout();
    let reg = Registry::load(&layout)?;
    let rules = RuleSet::embedded()?;
    let (mut sites, mut missing, mut without_national) = (0usize, 0usize, Vec::new());
    let mut by_act: BTreeMap<(&str, &str), usize> = BTreeMap::new();
    let mut by_status: BTreeMap<&str, usize> = BTreeMap::new();
    let mut national_count: BTreeMap<String, usize> = BTreeMap::new();
    for s in reg.all() {
        let path = layout.out_dir(s).join("zones").join("zones.geojson");
        let Ok(text) = std::fs::read_to_string(&path) else {
            missing += 1;
            continue;
        };
        let layer = ZoneLayer::from_geojson_str(&text)?;
        let l = site_local_rules(&layer, &rules);
        sites += 1;
        if l.national.is_empty() {
            without_national.push(s.id.to_owned());
        }
        for n in &l.national {
            *national_count.entry(n.clone()).or_default() += 1;
        }
        for z in &l.zones {
            *by_act
                .entry((z.act.as_str(), z.status.as_str()))
                .or_default() += 1;
        }
        *by_status.entry(l.status.as_str()).or_default() += 1;
        println!(
            "{:<60} national {:>2}  local lu {:>2} partiel {:>2} non vérifié {:>3}  municipal {:<10}  statut {}",
            s.id,
            l.national.len(),
            l.count(LocalStatus::Read),
            l.count(LocalStatus::Partial),
            l.count(LocalStatus::Unverified),
            l.municipal.map_or("-", LocalStatus::as_str),
            l.status.as_str()
        );
    }
    println!("\n{sites} sites avec couche de zones, {missing} sans couche");
    println!("sites sans règle nationale : {}", without_national.len());
    for id in &without_national {
        println!("  {id}");
    }
    println!("règles nationales (sites où elles s'appliquent) :");
    for (r, n) in &national_count {
        println!("  {r:<45} {n}");
    }
    println!("zones à acte propre (acte, statut) :");
    for ((a, st), n) in &by_act {
        println!("  {a:<32} {st:<11} {n}");
    }
    println!("statut des sites : {by_status:?}");
    println!("durée : {:.2} s", t0.elapsed().as_secs_f64());
    Ok(())
}
