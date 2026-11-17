//! C1 pipeline side (`routing::build`) on a synthetic pistes layer and a planar terrain
//! model (Q4, #104: the build summary and `routes.meta.json` were only exercised through
//! the CLI chain test): summary figures, profiles from the terrain, files and sources of
//! the metadata.

#![cfg(feature = "pipeline")]
#![allow(
    clippy::disallowed_methods,
    reason = "tests write their inputs directly"
)]

use serde_json::json;
use skimap_common::crs::{Crs, to_lambert93};
use skimap_common::geom::LonLat;
use skimap_common::grid::{GeoTransform, Grid};
use skimap_common::layout::DataLayout;
use skimap_common::meta::{LayerMeta, SourceRef};
/// Evaluation site of `data/sites.json`, test data.
static CHAPELLE_DES_BOIS: std::sync::LazyLock<skimap_common::site::Site> =
    std::sync::LazyLock::new(|| {
        *skimap_common::site::site("chapelle-des-bois").expect("evaluation site")
    });

const M_LAT: f64 = 111_195.0;

fn at(x_m: f64, y_m: f64) -> LonLat {
    LonLat::new(
        6.1 + x_m / (M_LAT * 46.6f64.to_radians().cos()),
        46.6 + y_m / M_LAT,
    )
}

#[test]
fn graph_summary_profiles_and_metadata() {
    let tmp = tempfile::tempdir().unwrap();
    let layout = DataLayout::new(tmp.path());
    let site = &CHAPELLE_DES_BOIS;
    // Three 1 km edges eastwards and a 800 m one-way spur southwards.
    let p = |x: f64, y: f64| {
        let q = at(x, y);
        json!([q.lon, q.lat])
    };
    let node = |i: usize, x: f64, y: f64| {
        json!({"type": "Feature", "geometry": {"type": "Point", "coordinates": p(x, y)},
               "properties": {"node": i}})
    };
    let edge = |i: usize, from: usize, to: usize, a: (f64, f64), b: (f64, f64), oneway: bool| {
        json!({"type": "Feature", "geometry": {"type": "LineString", "coordinates": [p(a.0, a.1), p(b.0, b.1)]},
               "properties": {"edge": i, "from": from, "to": to, "kind": "piste", "uses": ["nordic"],
                              "oneway": oneway, "name": format!("e{i}")}})
    };
    let nodes = json!({"type": "FeatureCollection", "features": [
        node(0, 0.0, 0.0), node(1, 1000.0, 0.0), node(2, 2000.0, 0.0), node(3, 3000.0, 0.0),
        node(4, 1000.0, -800.0)]});
    let edges = json!({"type": "FeatureCollection", "features": [
        edge(0, 0, 1, (0.0, 0.0), (1000.0, 0.0), false),
        edge(1, 1, 2, (1000.0, 0.0), (2000.0, 0.0), false),
        edge(2, 2, 3, (2000.0, 0.0), (3000.0, 0.0), false),
        edge(3, 1, 4, (1000.0, 0.0), (1000.0, -800.0), true)]});
    let pistes = layout.layer_dir(site, "pistes").unwrap();
    std::fs::create_dir_all(&pistes).unwrap();
    std::fs::write(pistes.join("graph_edges.geojson"), edges.to_string()).unwrap();
    std::fs::write(pistes.join("graph_nodes.geojson"), nodes.to_string()).unwrap();
    let mut m = LayerMeta::new("pistes", site.id);
    m.valid_from = Some("2026-11-13T23:00:00Z".into());
    m.sources = vec![SourceRef::new(
        "OSM",
        "https://example.org/osm",
        "ODbL 1.0",
        "OSM",
    )];
    m.write(&pistes).unwrap();
    // Terrain: plane rising 5 cm per metre eastwards (Lambert-93 x), 5 m cells.
    let (x0, y0) = to_lambert93(at(-200.0, 300.0)).unwrap();
    let (x1, y1) = to_lambert93(at(3200.0, -1100.0)).unwrap();
    let (w, h) = (((x1 - x0) / 5.0) as usize, ((y0 - y1) / 5.0) as usize);
    let t = GeoTransform::north_up(x0.floor(), y0.ceil(), 5.0);
    let data: Vec<f32> = (0..w * h)
        .map(|i| (1000.0 + 0.05 * (i % w) as f64 * 5.0) as f32)
        .collect();
    let terrain = layout.layer_dir(site, "terrain").unwrap();
    std::fs::create_dir_all(&terrain).unwrap();
    skimap_common::geotiff::write_f32(
        &terrain.join("dem_5m.tif"),
        &Grid::from_vec(w, h, t, Crs::Lambert93, data).unwrap(),
    )
    .unwrap();
    let mut m = LayerMeta::new("terrain", site.id);
    m.sources = vec![
        SourceRef::new("LiDAR HD", "https://example.org/lidar", "LO 2.0", "IGN"),
        SourceRef::new("IGN slopes", "https://example.org/slopes", "?", "IGN"),
    ];
    m.write(&terrain).unwrap();
    // Proof routes already in the layer are listed by the metadata.
    let routes = layout.layer_dir(site, "routes").unwrap();
    std::fs::create_dir_all(&routes).unwrap();
    for f in [
        "routes.geojson",
        "matching_example.geojson",
        "offpiste_b.geojson",
        "offpiste_a.geojson",
    ] {
        std::fs::write(routes.join(f), "{}").unwrap();
    }

    let (g, b) = skimap_core::routing::build(site, &layout).unwrap();
    assert_eq!((b.nodes, b.edges, b.components), (5, 4, 1));
    assert!((b.length_km - g.total_length_m() / 1000.0).abs() < 1e-12);
    assert!((b.length_km - 3.8).abs() < 0.01, "{}", b.length_km);
    assert!((b.oneway_km - g.length_m(3) / 1000.0).abs() < 1e-12);
    assert!((b.oneway_km - 0.8).abs() < 0.01);
    assert_eq!(b.profiles.edges_with_profile, 4);
    // 5 cm per metre eastwards on 3 km (Lambert-93 x and the east direction differ by the
    // meridian convergence, below 1 % here).
    assert!(
        (b.gain_forward_m - 150.0).abs() < 2.0,
        "{}",
        b.gain_forward_m
    );
    let text = std::fs::read_to_string(routes.join("piste_graph.json")).unwrap();
    assert_eq!(b.graph_bytes, text.len() as u64);
    assert_eq!(b.graph_path, routes.join("piste_graph.json"));
    // Metadata: graph first, then the proof routes found, sorted.
    let meta = LayerMeta::read(&routes.join("routes.meta.json")).unwrap();
    let names: Vec<&str> = meta.files.iter().map(|f| f.name.as_str()).collect();
    assert_eq!(
        names,
        [
            "piste_graph.json",
            "routes.geojson",
            "matching_example.geojson",
            "offpiste_a.geojson",
            "offpiste_b.geojson"
        ]
    );
    assert_eq!(meta.files[0].bytes, Some(b.graph_bytes));
    assert_eq!(meta.files[0].count, Some(4));
    assert_eq!(meta.valid_from.as_deref(), Some("2026-11-13T23:00:00Z"));
    let sources: Vec<&str> = meta.sources.iter().map(|s| s.name.as_str()).collect();
    assert_eq!(sources, ["OSM", "LiDAR HD"]);
    assert_eq!(meta.stats["edges"], 4);
}
