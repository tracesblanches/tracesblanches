//! Census of the French cross-country ski areas (P3) and domain layers of both sites,
//! with the figures compared with research report 07 and a sensitivity analysis.
//!
//! ```sh
//! cargo run --release -p skimap-osm --example domaines
//! SKIMAP_PERSONAL_SOURCES=1 cargo run --release -p skimap-osm --example domaines
//! ```

use std::time::Instant;

use skimap_common::config::Config;
use skimap_common::download::Downloader;
use skimap_common::site::{CERVIERES, CHAPELLE_DES_BOIS};
use skimap_osm::census::{CensusOptions, build_france_from, build_site_from};
use skimap_osm::domains::{Params, cluster};
use skimap_osm::extract::load_planet;
use skimap_osm::pistes::PisteSet;

/// Way of the « Izoard » piste of Cervières (relation 2704630), near Le Laus.
const CERVIERES_WAY: i64 = 201123151;
/// Way of the « Souliers » piste, member of the « Queyras - Arvieux » site.
const ARVIEUX_WAY: i64 = 276869463;

fn same_domain(set: &PisteSet, params: &Params, a: i64, b: i64) -> (bool, usize, usize) {
    let c = cluster(set, params);
    let idx = |w: i64| set.pistes.iter().position(|p| p.way_id == w).expect("way");
    let (da, db) = (c.piste_domain[idx(a)], c.piste_domain[idx(b)]);
    let nordic: Vec<f64> = c
        .domains
        .iter()
        .map(|d| d.km(set, |p| p.is_nordic()))
        .collect();
    (
        da == db,
        nordic.iter().filter(|k| **k > 0.0).count(),
        nordic.iter().filter(|k| **k >= 5.0).count(),
    )
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .with_writer(std::io::stderr)
        .init();
    let cfg = Config::from_env()?;
    let layout = cfg.layout();
    let dl = Downloader::from_config(&cfg)?;
    let personal = std::env::var("SKIMAP_PERSONAL_SOURCES").is_ok_and(|v| v == "1");
    let opts = CensusOptions {
        personal_sources: personal,
        ..Default::default()
    };
    let t = Instant::now();
    let planet = load_planet(&dl)?;
    let t_planet = t.elapsed().as_secs_f64();
    let census = build_france_from(&layout, &dl, &planet, &opts)?;
    let t_census = t.elapsed().as_secs_f64() - t_planet;
    println!(
        "planet {t_planet:.1} s, census {t_census:.1} s -> {}\n{}",
        census.dir.display(),
        serde_json::to_string_pretty(&census.stats)?
    );
    for site in [&CERVIERES, &CHAPELLE_DES_BOIS] {
        let dir = build_site_from(site, &layout, &planet, &census)?;
        println!("\n== {} -> {}", site.name, dir.display());
        for r in census.domains.iter().filter(|r| {
            r.footprint
                .as_ref()
                .is_some_and(|f| geo::Intersects::intersects(f, &geo_types::Rect::from(site.bbox)))
        }) {
            let osm: Vec<String> = r
                .openskimap
                .iter()
                .take(3)
                .map(|o| {
                    format!(
                        "{} {:.1} km",
                        o.name.as_deref().unwrap_or("(sans nom)"),
                        o.km
                    )
                })
                .collect();
            println!(
                "  {} « {} » ({:?}) {:.1} km nordic + {:.1} km hike, commune {:?}, OpenSkiMap: {}",
                r.id,
                r.name.as_deref().unwrap_or(""),
                r.name_source,
                r.km_nordic,
                r.km_hike_only,
                r.commune.as_ref().map(|c| c.name.as_str()),
                osm.join(", ")
            );
        }
    }

    println!(
        "\nSensitivity (clusters with nordic in the France area / with >= 5 km; Cervières with Arvieux?)"
    );
    let base = Params::default();
    for (label, p) in [
        ("default", base),
        (
            "gap 50 m",
            Params {
                gap_m: 50.0,
                ..base
            },
        ),
        (
            "gap 500 m (OpenSkiMap)",
            Params {
                gap_m: 500.0,
                ..base
            },
        ),
        (
            "liaison 4 km",
            Params {
                liaison_min_km: 4.0,
                ..base
            },
        ),
        (
            "liaison 8 km",
            Params {
                liaison_min_km: 8.0,
                ..base
            },
        ),
        (
            "liaison 15 km",
            Params {
                liaison_min_km: 15.0,
                ..base
            },
        ),
        (
            "core 2 km",
            Params {
                core_min_km: 2.0,
                ..base
            },
        ),
        (
            "core 10 km",
            Params {
                core_min_km: 10.0,
                ..base
            },
        ),
    ] {
        let (merged, n, n5) = same_domain(&planet.set, &p, CERVIERES_WAY, ARVIEUX_WAY);
        println!("  {label:24} {n:4} / {n5:4}  Cervières merged with Arvieux: {merged}");
    }
    Ok(())
}
