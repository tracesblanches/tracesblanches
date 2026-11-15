//! Checks on the live sources and the real census (network, data directory): ignored by
//! default.
//!
//! ```sh
//! SKIMAP_DATA_DIR=$PWD/data cargo test -p skimap-stations --test real -- --ignored --nocapture
//! ```

use std::collections::BTreeSet;

use skimap_common::config::Config;
use skimap_common::download::Downloader;
use skimap_stations::{domains, titres};

fn setup() -> (Vec<domains::Domain>, Downloader) {
    let cfg = Config::from_env().unwrap();
    let dir = cfg.layout().france_layer_dir("domaines").unwrap();
    (
        domains::load(&dir).unwrap().0,
        Downloader::from_config(&cfg).unwrap(),
    )
}

/// Reads every issuer page, requires them all to parse, prints every title with its
/// members and domains, and checks the Jura: Chapelle-des-Bois is covered by the national
/// pass, the Montagnes du Jura pass and the Haut-Doubs site pass; every domain of the
/// Ain, the Doubs and the Jura is printed as member or not of the ENJ.
#[test]
#[ignore = "network and data directory"]
fn titles_from_live_sources() {
    let (ds, dl) = setup();
    let today = chrono::Utc::now().format("%Y-%m-%d").to_string();
    let read = titres::read_titles(&dl, &today);
    for d in &read.docs {
        if d.status != "ok" {
            println!("SOURCE {} : {} {:?}", d.id, d.status, d.error);
        }
    }
    assert!(read.missing.is_empty(), "{:?}", read.missing);
    let name = |id: &str| {
        ds.iter()
            .find(|d| d.id == id)
            .map(|d| d.name.clone())
            .unwrap_or_default()
    };
    let mut covered: Vec<(String, BTreeSet<String>)> = Vec::new();
    for t in &read.titres {
        let c = titres::coverage(t, &ds, &today);
        println!(
            "== {} : {} domaines, {} prix",
            t.id,
            c.domains.len(),
            t.prices.len()
        );
        for p in &t.prices {
            println!(
                "  prix {} {} {} € {} {:?}..{:?} ({})",
                p.product, p.category, p.eur, p.tier, p.from, p.until, p.season
            );
        }
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
                    "AUCUN".into()
                } else {
                    names.join(" ; ")
                }
            );
        }
        covered.push((t.id.clone(), c.domains));
    }
    for p in &read.site_pages {
        let m = titres::match_site_page(p, &ds, &today);
        println!(
            "== page {} [{}] -> {:?} ; {} prix ({:?}) ; {} téléphones",
            p.name,
            m.confidence,
            m.domains,
            p.prices.len(),
            p.season,
            p.phones.len()
        );
    }
    let has = |t: &str, d: &str| covered.iter().any(|(id, s)| id == t && s.contains(d));
    let cdb = "site-13681086";
    assert!(has("nordic-pass-national", cdb));
    assert!(has("enj-montagnes-du-jura-saison", cdb));
    assert!(
        covered.iter().any(|(id, s)| id.starts_with("enj-site-")
            && s.contains(cdb)
            && s.contains("site-2783351")),
        "Haut-Doubs site pass"
    );
    let enj: BTreeSet<String> = covered
        .iter()
        .find(|(id, _)| id == "enj-montagnes-du-jura-saison")
        .map(|(_, s)| s.clone())
        .unwrap();
    println!("== Jura : membres ENJ {}", enj.len());
    for d in ds
        .iter()
        .filter(|d| ["01", "25", "39"].contains(&d.departement.as_str()))
    {
        println!(
            "  {} {} {} : {}",
            d.departement,
            d.id,
            d.name,
            if enj.contains(&d.id) {
                "membre"
            } else {
                "HORS ENJ"
            }
        );
    }
    let all: BTreeSet<&String> = covered
        .iter()
        .filter(|(id, _)| !id.starts_with("enj-site-"))
        .flat_map(|(_, s)| s.iter())
        .collect();
    println!(
        "== domaines couverts par un titre de massif ou national : {} sur {}",
        all.len(),
        ds.len()
    );
}
