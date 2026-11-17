//! Builds the pistes layer (P2) of both sites and prints the figures compared with the
//! research reports 02, 04 and 07.
//!
//! ```sh
//! cargo run --release -p skimap-osm --example pistes
//! ```

use std::collections::BTreeSet;
use std::time::Instant;

use skimap_common::config::Config;
use skimap_common::download::Downloader;
/// Evaluation site of `data/sites.json`, test data.
static CERVIERES: std::sync::LazyLock<skimap_common::site::Site> =
    std::sync::LazyLock::new(|| *skimap_common::site::site("cervieres").expect("evaluation site"));
/// Evaluation site of `data/sites.json`, test data.
static CHAPELLE_DES_BOIS: std::sync::LazyLock<skimap_common::site::Site> =
    std::sync::LazyLock::new(|| {
        *skimap_common::site::site("chapelle-des-bois").expect("evaluation site")
    });
use skimap_osm::extract::{build_from, load_planet};
use skimap_osm::pistes::Use;

/// The 13 Chapelle-des-Bois trails tested by research report 02.
const CHAPELLE_13: [&str; 13] = [
    "La Beurrière",
    "La Loge",
    "Chaumoz",
    "Combe Verte",
    "Combe des Cives",
    "Jobez",
    "La Norbiére",
    "Nondance",
    "Chalet Pin",
    "Les Mortes",
    "Les Prés d'Haut",
    "Sylvain Guillaume",
    "Greffier",
];

fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .with_writer(std::io::stderr)
        .init();
    let cfg = Config::from_env()?;
    let layout = cfg.layout();
    let dl = Downloader::from_config(&cfg)?;
    let t = Instant::now();
    let planet = load_planet(&dl)?;
    println!(
        "planet read + classified in {:.2} s: {} pistes in the France area, OSM data of {:?}",
        t.elapsed().as_secs_f64(),
        planet.set.pistes.len(),
        planet.valid_at
    );
    for site in [&CERVIERES, &CHAPELLE_DES_BOIS] {
        let t = Instant::now();
        let out = build_from(site, &layout, &planet)?;
        println!(
            "\n== {} -> {} ({:.2} s)\n{}",
            site.name,
            out.dir.display(),
            t.elapsed().as_secs_f64(),
            serde_json::to_string_pretty(&out.stats)?
        );
        // The research reports count the ways with a vertex in the box: the outside
        // ways kept to close the network are left out.
        let s = &out.set.in_bbox(&site.bbox);
        for r in &out.reentries {
            println!(
                "outside path kept: {} -> {}, {:.0} m, ways {:?}",
                r.from_osm_node, r.to_osm_node, r.length_m, r.osm_way_ids
            );
        }
        if site.id == "cervieres" {
            // Report 04 (~25 km): ways tagged exactly `piste:type=nordic` (its Overpass
            // query did not match `nordic;hike`), no relation-only member, whole box.
            let exact = |id: i64| {
                planet
                    .data
                    .way(id)
                    .is_some_and(|w| w.tags.get("piste:type") == Some("nordic"))
            };
            let r04 = s.km(|p| exact(p.way_id));
            let n04 = s.pistes.iter().filter(|p| exact(p.way_id)).count();
            let tagged = s.km(|p| p.tagged && p.uses.contains(&Use::Nordic));
            println!(
                "Cervières: piste:type=nordic exactly, whole box: {n04} ways, {r04:.1} km \
                 (report 04: 102 ways, 24.9 km); any tagged nordic way {tagged:.1} km"
            );
            // Report 07 (34.5 km): nordic incl. relation members, mean latitude >= 44.82.
            let r07 = s.km(|p| p.is_nordic() && p.mean_lat() >= 44.82);
            // Report 02 (37.7 km = 34.5 nordic + 3.2 hike), same filter, hike included.
            let r02 = s.km(|p| p.mean_lat() >= 44.82);
            let r02_hike = s.km(|p| !p.is_nordic() && p.mean_lat() >= 44.82);
            let n02 = s.pistes.iter().filter(|p| p.mean_lat() >= 44.82).count();
            println!(
                "Cervières: \
                 nordic incl. relation members, north of 44.82 {r07:.1} km (report 07: 34.5 km); \
                 + hike {r02:.1} km in {n02} ways, of which hike only {r02_hike:.1} km \
                 (report 02: 37.7 km in 115 ways, 3.2 km hike)"
            );
            let names: BTreeSet<String> = s
                .relations
                .iter()
                .filter(|r| r.uses.contains(&Use::Nordic))
                .filter_map(|r| r.name.clone())
                .collect();
            println!("Cervières nordic relations: {names:?}");
        } else {
            let mut found = Vec::new();
            let mut ways = BTreeSet::new();
            for n in CHAPELLE_13 {
                let rels: Vec<usize> = s
                    .relations
                    .iter()
                    .enumerate()
                    .filter(|(_, r)| r.name.as_deref() == Some(n))
                    .map(|(i, _)| i)
                    .collect();
                if !rels.is_empty() {
                    found.push(n);
                }
                for p in &s.pistes {
                    if p.relations.iter().any(|i| rels.contains(i)) {
                        ways.insert(p.way_id);
                    }
                }
            }
            let km = s.km(|p| ways.contains(&p.way_id));
            let missing: Vec<&str> = CHAPELLE_13
                .iter()
                .filter(|n| !found.contains(n))
                .copied()
                .collect();
            println!(
                "Chapelle-des-Bois: {}/13 tested trails present as relations, missing {missing:?}; \
                 union of their ways {} ways, {km:.1} km in the box (report 02: 13/13, 65.7 km)",
                found.len(),
                ways.len()
            );
            let nordic_rel = s
                .relations
                .iter()
                .filter(|r| r.uses.contains(&Use::Nordic))
                .count();
            let hike_rel = s
                .relations
                .iter()
                .filter(|r| !r.uses.contains(&Use::Nordic))
                .count();
            let tagged_nordic = s
                .pistes
                .iter()
                .filter(|p| p.tagged && p.uses.contains(&Use::Nordic))
                .count();
            println!(
                "Chapelle-des-Bois: {nordic_rel} nordic relations, {hike_rel} other relations \
                 (report 02: 84 nordic + 3 hike); {tagged_nordic} ways tagged nordic, {:.1} km \
                 (report 02: 598 ways, ~270 km)",
                s.km(|p| p.tagged && p.uses.contains(&Use::Nordic))
            );
        }
    }
    Ok(())
}
