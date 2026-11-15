//! Golden fingerprints of the pistes layer (P2) of both sites, rebuilt from the cut
//! OpenSnowMap extract of the fixtures (Q2, #102): key figures of `pistes.meta.json`,
//! kilometres by use, difficulty and grooming, the graph and its one-way traps.
//!
//! References: `fixtures/golden/osm/<site>.json`; update with
//! `SKIMAP_BLESS=1 cargo test -p skimap-osm --test golden` and review the diff.
//!
//! Tolerances: the extract is versioned and the classification deterministic: counts are
//! exact, lengths `Tol::FLOAT` (haversine sums, floating-point noise only). The figures
//! that the research reports give (crates/osm/docs/pistes.md: 66.7 km of which 61.6 km
//! nordic at Cervières, 270.5 km of tagged nordic ways at Chapelle-des-Bois, report 02:
//! ≈ 270 km) are asserted in `tests/fixtures.rs`.

#[path = "../../../fixtures/support/mod.rs"]
mod fixtures;

use std::collections::BTreeMap;

use fixtures::golden::{Golden, Tol};
use skimap_common::layout::DataLayout;
use skimap_common::meta::SourceRef;
use skimap_common::site::SITES;
use skimap_osm::extract::{self, Planet};
use skimap_osm::pistes::Piste;

fn planet() -> Planet {
    let data = skimap_osm::parse::read_file(
        &fixtures::input("osm/planet_pistes_sites.osm.gz"),
        &extract::france_aoi(),
    )
    .unwrap();
    let set = skimap_osm::pistes::classify(&data);
    Planet {
        data,
        set,
        source: SourceRef::new("fixtures", extract::PLANET_URL, "ODbL", "OSM"),
        valid_at: None,
    }
}

fn add_km(map: &mut BTreeMap<String, f64>, key: String, p: &Piste) {
    *map.entry(key).or_default() += p.length_m / 1000.0;
}

#[test]
fn pistes_layers() {
    let planet = planet();
    let tmp = tempfile::tempdir().unwrap();
    let layout = DataLayout::new(tmp.path());
    for site in SITES {
        let o = extract::build_from(site, &layout, &planet).unwrap();
        let s = &o.stats;
        let mut k = Golden::new(&format!("osm/{}", site.id));
        k.count("ways", s.ways)
            .count("ways_tagged", s.ways_tagged)
            .count("ways_relation_only", s.ways_relation_only)
            .count("ways_oneway", s.ways_oneway)
            .count("ways_with_grooming", s.ways_with_grooming)
            .count("ways_difficulty_from_colour", s.ways_difficulty_from_colour)
            .count(
                "ways_difficulty_from_relation_tag",
                s.ways_difficulty_from_relation_tag,
            )
            .count("ways_difficulty_from_way", s.ways_difficulty_from_way)
            .count("ways_outside_box", s.ways_outside_box)
            .count("relations", s.relations)
            .num("km_total", s.km_total, Tol::FLOAT)
            .num("km_nordic", s.km_nordic, Tol::FLOAT)
            .num("km_nordic_tagged", s.km_nordic_tagged, Tol::FLOAT)
            .num("km_hike_only", s.km_hike_only, Tol::FLOAT)
            .num("km_outside_box", s.km_outside_box, Tol::FLOAT)
            .count("graph.nodes", s.graph_nodes)
            .count("graph.edges", s.graph_edges)
            .count("graph.connectors", s.graph_connectors)
            .count("graph.components", s.graph_components)
            .text("graph.oneway_sinks", format!("{:?}", s.oneway_sinks))
            .text("graph.oneway_sources", format!("{:?}", s.oneway_sources))
            .count("reentries", o.reentries.len());
        // Whole layer (outside paths included), by use, difficulty and grooming.
        let mut km: BTreeMap<String, f64> = BTreeMap::new();
        for p in &o.set.pistes {
            for u in &p.uses {
                add_km(&mut km, format!("km_by_use.{u:?}"), p);
            }
            let d = p.difficulty.map_or("none".into(), |d| format!("{d:?}"));
            add_km(&mut km, format!("km_by_difficulty.{d}"), p);
            let g = &p.grooming;
            let groom = match (g.classic, g.skating, g.backcountry, g.scooter) {
                (true, true, _, _) => "classic+skating",
                (true, false, _, _) => "classic",
                (false, true, _, _) => "skating",
                (_, _, true, _) => "backcountry",
                (_, _, _, true) => "scooter",
                _ => "unknown",
            };
            add_km(&mut km, format!("km_by_grooming.{groom}"), p);
            add_km(&mut km, format!("km_by_oneway.{:?}", p.oneway), p);
        }
        for (key, v) in km {
            k.num(&key, v, Tol::FLOAT);
        }
        k.check();
    }
}
