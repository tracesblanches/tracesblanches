//! Checks on the real census and on a real build (data directory): ignored by default.
//!
//! ```sh
//! SKIMAP_DATA_DIR=$PWD/data cargo test -p skimap-stations --test real -- --ignored --nocapture
//! ```

use std::collections::BTreeSet;

use skimap_common::config::Config;
use skimap_stations::{catalogue, domains, titres};

fn census() -> Vec<domains::Domain> {
    let cfg = Config::from_env().unwrap();
    let dir = cfg.layout().france_layer_dir("domaines").unwrap();
    domains::load(&dir).unwrap().0
}

/// Re-reads every cited source (network; personal use) and requires every quote to be
/// found and every PDF to be unchanged.
#[test]
#[ignore = "network"]
fn sources_still_say_the_same() {
    let cfg = Config::from_env().unwrap();
    let dl = skimap_common::download::Downloader::from_config(&cfg).unwrap();
    let (checks, _) = titres::check_sources(&dl);
    for c in &checks {
        println!("{} : {} {:?} {:?}", c.id, c.status, c.missing, c.error);
    }
    assert!(checks.iter().all(|c| c.status == "ok"), "{checks:#?}");
}

/// Every hand-checked domain id exists in the census; every Jura, Doubs or Ain domain is
/// either an ENJ member or listed as not a member (the Jura check of H2); prints the
/// coverage of each title for the review.
#[test]
#[ignore = "data directory"]
fn coverage_against_census() {
    let ds = census();
    let ids: BTreeSet<&str> = ds.iter().map(|d| d.id.as_str()).collect();
    for m in catalogue::national_members() {
        for d in m.domains {
            assert!(ids.contains(d), "{d} ({}) not in the census", m.label);
        }
    }
    for d in catalogue::JURA_NOT_MEMBERS {
        assert!(ids.contains(d), "{d} not in the census");
    }
    let enj: BTreeSet<&str> = catalogue::ENJ_MEMBERS
        .iter()
        .flat_map(|m| m.domains.iter().copied())
        .collect();
    for d in ds
        .iter()
        .filter(|d| ["01", "25", "39"].contains(&d.departement.as_str()))
    {
        let member = enj.contains(d.id.as_str());
        let not = catalogue::JURA_NOT_MEMBERS.contains(&d.id.as_str());
        assert!(
            member ^ not,
            "{} {} : membre ENJ {member}, hors ENJ {not}",
            d.id,
            d.name
        );
    }
    for max in [(0.35, 0.5), (0.5, 0.7), (0.7, 1.0), (1.0, 1.4)] {
        let cl = skimap_stations::pois::clusters_with(&ds, max);
        let area: f64 = cl
            .iter()
            .map(|(b, _)| (b.north - b.south) * (b.east - b.west))
            .sum();
        println!(
            "== groupes Overpass {max:?} : {} requêtes, {area:.2} degrés carrés",
            cl.len()
        );
    }
    let covs = titres::coverages(&ds);
    let name = |id: &str| {
        ds.iter()
            .find(|d| d.id == id)
            .map(|d| d.name.clone())
            .unwrap_or_default()
    };
    for c in &covs {
        println!("== {} : {} domaines", c.titre, c.domains.len());
        for m in &c.members {
            let names: Vec<String> = m
                .domains
                .iter()
                .map(|d| format!("{d} {}", name(d)))
                .collect();
            println!(
                "  [{}] {} -> {}",
                m.confidence,
                m.label,
                if names.is_empty() {
                    "AUCUN".to_owned()
                } else {
                    names.join(" ; ")
                }
            );
        }
    }
    let national = covs
        .iter()
        .find(|c| c.titre == "nordic-pass-national")
        .unwrap();
    let outside: Vec<String> = ds
        .iter()
        .filter(|d| !national.domains.contains(&d.id))
        .map(|d| {
            format!(
                "{} {} ({}, {} km)",
                d.id, d.name, d.departement, d.km_nordic
            )
        })
        .collect();
    println!(
        "== hors Nordic Pass national : {}\n  {}",
        outside.len(),
        outside.join("\n  ")
    );
}
