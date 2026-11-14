//! Recomputes the CLPA crossing figures of report 07 from the files the research kept
//! (`research/raw/`, not versioned), without network:
//!
//! ```sh
//! SKIMAP_RESEARCH_RAW=/path/to/skimap/research/raw cargo test -p skimap-avalanche -- --ignored
//! ```
//!
//! Same site rule as the report (vertex mean north of 44.82° N, the col d'Izoard).

use std::path::PathBuf;

use skimap_avalanche::historic::{self, pistes};

fn raw() -> PathBuf {
    PathBuf::from(
        std::env::var("SKIMAP_RESEARCH_RAW").expect("set SKIMAP_RESEARCH_RAW to research/raw"),
    )
}

#[test]
#[ignore = "needs research/raw (SKIMAP_RESEARCH_RAW)"]
fn report07_cervieres() {
    let base = raw().join("census_ava/clpa/ddt05/dataset/L_ALEA_AVALANCHE_005");
    let shp = std::fs::read(base.with_extension("shp")).unwrap();
    let dbf = std::fs::read(base.with_extension("dbf")).unwrap();
    let all = historic::read_ddt05(&shp, &dbf).unwrap();
    assert_eq!(all.len(), 5882, "records of the department");
    let cerv: Vec<_> = all.iter().filter(|e| e.attrs["INSEE"] == "05027").collect();
    assert_eq!(cerv.len(), 172);
    assert_eq!(cerv.iter().filter(|e| e.is_clpa()).count(), 171);

    let lines =
        pistes::from_overpass_json(&std::fs::read(raw().join("osm/cerv2.json")).unwrap()).unwrap();
    let north: Vec<_> = lines
        .into_iter()
        .filter(|l| l.network == pistes::Network::Nordic && l.vertex_mean().lat >= 44.82)
        .collect();
    let clpa: Vec<_> = all.iter().filter(|e| e.is_clpa()).collect();
    let every: Vec<_> = all.iter().collect();
    let c = historic::compute(&north, None, &clpa, &every).unwrap();
    let total: f64 = c.line_lengths_m.iter().sum();
    let inside: f64 = c.crossings.iter().map(|x| x.length_m).sum();
    let inside_any: f64 = c.line_inside_any_m.iter().sum();
    assert_eq!(c.lines.len(), 106);
    assert!((total / 1e3 - 34.5).abs() < 0.01, "total {total}");
    assert!((inside / 1e3 - 5.78).abs() < 0.005, "inside {inside}");
    assert!(
        (inside_any - inside).abs() < 1.0,
        "PPR adds nothing: {inside_any}"
    );
}
