//! P2 on the reference data set: the pistes layer of both sites rebuilt from the cut
//! OpenSnowMap extract of `fixtures/inputs/osm/` (every way around the two sites) is
//! byte-identical to the versioned layer (itself identical to the pipeline output,
//! checked by `skimap fixtures`), without network.

#[path = "../../../fixtures/support/mod.rs"]
mod fixtures;

use skimap_common::layout::DataLayout;
use skimap_common::meta::SourceRef;
use skimap_common::site::SITES;
use skimap_osm::extract::{self, PistesOutput, Planet};

/// The cut extract, read as `extract::load_planet` reads the full one.
fn planet() -> Planet {
    let data = skimap_osm::parse::read_file(
        &fixtures::input("osm/planet_pistes_sites.osm.gz"),
        &extract::france_aoi(),
    )
    .unwrap();
    let set = skimap_osm::pistes::classify(&data);
    let state = std::fs::read_to_string(fixtures::input("osm/planet_pistes-state.txt")).unwrap();
    Planet {
        data,
        set,
        source: SourceRef::new(
            "OpenSnowMap planet_pistes.osm.gz (fixtures)",
            extract::PLANET_URL,
            extract::LICENCE,
            extract::ATTRIBUTION,
        ),
        valid_at: Some(state.trim().to_owned()),
    }
}

fn build_both() -> (tempfile::TempDir, Vec<PistesOutput>) {
    let p = planet();
    let tmp = tempfile::tempdir().unwrap();
    let layout = DataLayout::new(tmp.path());
    let out = SITES
        .iter()
        .map(|s| extract::build_from(s, &layout, &p).unwrap())
        .collect();
    (tmp, out)
}

#[test]
fn pistes_layer_rebuilt_from_the_cut_extract() {
    let (_tmp, out) = build_both();
    for (site, o) in SITES.iter().zip(&out) {
        let got = std::fs::read(o.dir.join("pistes.geojson")).unwrap();
        let want =
            std::fs::read(fixtures::layer_file(site.id, "pistes", "pistes.geojson")).unwrap();
        assert!(
            got == want,
            "{}: pistes.geojson rebuilt from the fixtures differs from fixtures/data",
            site.id
        );
        let s = &o.stats;
        println!(
            "{}: {} ways, {} km ({} km nordic), graph {} nodes, {} edges, {} components",
            site.id,
            s.ways,
            s.km_total,
            s.km_nordic,
            s.graph_nodes,
            s.graph_edges,
            s.graph_components
        );
        assert!(s.graph_nodes > 0 && s.graph_edges >= s.graph_nodes - s.graph_components);
    }
}

/// Figures of `crates/osm/docs/pistes.md` and `connexite.md` (compared there with
/// research reports 02, 04 and 07) on the rebuilt layers.
#[test]
fn documented_figures() {
    let (_tmp, out) = build_both();
    let [cerv, chap] = [&out[0], &out[1]];
    // « L'emprise entière du site (Brunissard et Arvieux compris) contient 66,7 km de
    // pistes, dont 61,6 km nordiques. »
    assert_eq!((cerv.stats.km_total, cerv.stats.km_nordic), (66.72, 61.56));
    // « Cervières : 1 way de 1,5 km (« Souliers », qui relie deux morceaux du site). »
    assert_eq!(cerv.reentries.len(), 1);
    assert_eq!(cerv.reentries[0].osm_way_ids.len(), 1);
    assert!((cerv.reentries[0].length_m - 1500.0).abs() < 50.0);
    assert_eq!(cerv.stats.ways_oneway, 0);
    // « Chapelle-des-Bois : 6 chemins, 32 ways, 9,2 km » kept outside the box.
    assert_eq!(chap.reentries.len(), 6);
    assert_eq!(chap.stats.ways_outside_box, 32);
    assert!((chap.stats.km_outside_box - 9.2).abs() < 0.05);
    // « Chapelle-des-Bois : ways étiquetés nordic : 270,5 km » (report 02: ≈ 270 km).
    assert!((chap.stats.km_nordic_tagged - 270.5).abs() < 0.05);
    // connexite.md: node 1999008075 (loop « Elodie Bourgeois Pin ») is a one-way sink.
    assert!(chap.stats.oneway_sinks.contains(&1_999_008_075));
}
