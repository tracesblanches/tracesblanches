//! Challenges on the real data of the two sites (V13 #76), in the `full_data_` category
//! (fixtures/support/mod.rs): the pipeline outputs of `$SKIMAP_DATA_DIR` (default `data/`
//! at the root of the repository), `cargo test -p skimap-collab --features phone --test
//! real -- --ignored full_data_ --nocapture`; the local CI runs them (`scripts/ci`).

#![cfg(feature = "phone")]

use std::path::PathBuf;

use skimap_collab::challenges::{coverage, geojson_lines};
use skimap_collab::track::{Outing, Point, thin};

/// `out/` of the complete local data: `$SKIMAP_DATA_DIR`, as the pipeline and the other
/// full-data tests read it (an agent worktree has no `data/` of its own).
fn data() -> PathBuf {
    std::env::var_os("SKIMAP_DATA_DIR")
        .map_or_else(
            || PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../data"),
            PathBuf::from,
        )
        .join("out")
}

fn read(path: &std::path::Path) -> String {
    std::fs::read_to_string(path).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

/// An outing that skied these lines, one after the other (the jumps between lines are
/// long segments, skipped by the index when over a kilometre).
fn skied(id: &str, lines: &[Vec<[f64; 2]>]) -> Outing {
    let track = lines
        .iter()
        .flat_map(|l| {
            let pts: Vec<Point> = l
                .iter()
                .map(|p| Point {
                    lon: p[0],
                    lat: p[1],
                    ele: None,
                    t: None,
                })
                .collect();
            thin(&pts)
        })
        .collect();
    Outing {
        id: id.into(),
        source: "test".into(),
        name: id.into(),
        activity: "nordic_skiing".into(),
        start: Some("2026-01-10T09:00:00Z".parse().unwrap()),
        elapsed_s: 0.0,
        moving_s: 0.0,
        distance_m: 0.0,
        gain_m: 0.0,
        track,
    }
}

#[test]
#[ignore = "full-data: data/out/<site>/pack"]
fn full_data_domain_challenge_on_both_sites() {
    for site in ["cervieres", "chapelle-des-bois"] {
        let layers = data().join(site).join("pack/layers");
        let pistes = read(&layers.join("pistes.geojson"));
        let lines = geojson_lines(&pistes, |_| true).unwrap();
        let routes = read(&layers.join("routes.geojson"));
        let route_lines = geojson_lines(&routes, |_| true).unwrap();
        let t = std::time::Instant::now();
        let none = coverage(&lines, &[]);
        let by_routes = coverage(&lines, &[skied("routes", &route_lines)]);
        let itself = coverage(&lines, &[skied("all", &lines)]);
        eprintln!(
            "{site}: {} lines, {:.1} km of pistes; the {} ready-made routes of the pack cover \
             {:.1} km ({:.0} %); skiing every piste: {:.1} %; three coverages in {:?}",
            lines.len(),
            none.total_m / 1000.0,
            route_lines.len(),
            by_routes.done_m / 1000.0,
            by_routes.share * 100.0,
            itself.share * 100.0,
            t.elapsed()
        );
        assert!(itself.share > 0.99, "{}", itself.share);
        assert!(by_routes.share > 0.05 && by_routes.share < 1.0);
    }
}

#[test]
#[ignore = "full-data: data/out/france/srn"]
fn full_data_gtj_challenge_from_the_srn_layer() {
    let srn = read(&data().join("france/srn/srn_routes.geojson"));
    let gtj = geojson_lines(&srn, |p| {
        p["name"]
            .as_str()
            .is_some_and(|n| n.contains("Grande traversée du Jura"))
    })
    .unwrap();
    let half: Vec<Vec<[f64; 2]>> = gtj.iter().map(|l| l[..l.len() / 2].to_vec()).collect();
    let p0 = coverage(&gtj, &[]);
    let p1 = coverage(&gtj, &[skied("half", &half)]);
    let p2 = coverage(&gtj, &[skied("all", &gtj)]);
    eprintln!(
        "GTJ SRN: {:.1} km; first half of the points skied: {:.0} %; all: {:.1} %, done on {:?}",
        p0.total_m / 1000.0,
        p1.share * 100.0,
        p2.share * 100.0,
        p2.done_on
    );
    assert!((p0.total_m / 1000.0 - 100.0).abs() < 5.0);
    assert!(p1.share > 0.3 && p1.share < 0.7);
    assert!(p2.share > 0.99 && p2.done_on.is_some());
}
