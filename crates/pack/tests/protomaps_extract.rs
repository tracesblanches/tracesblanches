//! The Rust extract of the Protomaps planet build (range requests through the shared
//! downloader) against a reference extract of the same build made by the official
//! `pmtiles extract` (go-pmtiles): same tiles, byte for byte. Ignored by default: network
//! (or a filled cache) and the reference archives.
//!
//! ```sh
//! SKIMAP_PROTOMAPS_BUILD=20261114 SKIMAP_PMTILES_REFERENCE=<dir with <site>-osm.pmtiles> \
//!   cargo test --release -p skimap-pack --features pipeline --test protomaps_extract \
//!   -- --ignored --nocapture
//! ```

use std::path::PathBuf;
use std::time::Instant;

use skimap_common::config::Config;
use skimap_common::download::Downloader;
use skimap_common::site::SITES;
use skimap_pack::TileArchive;
use skimap_pack::build::extract::{HttpRanges, extract};
use skimap_pack::build::{OSM_MAXZOOM, osm};
use skimap_pack::tiles::tiles_covering;

#[test]
#[ignore = "network (Protomaps build) and reference extracts"]
fn rust_extract_matches_go_pmtiles() {
    let mut cfg = Config::from_env().unwrap();
    if std::env::var_os("SKIMAP_DATA_DIR").is_none() {
        cfg.data_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../data");
    }
    let dl = Downloader::from_config(&cfg).unwrap();
    let wanted = std::env::var("SKIMAP_PROTOMAPS_BUILD").ok();
    let build = osm::select_build(&dl, wanted.as_deref()).unwrap();
    let url = format!("{}{}", osm::BUILD_BASE, build.key);
    let reference = std::env::var_os("SKIMAP_PMTILES_REFERENCE").map(PathBuf::from);
    let dir = tempfile::tempdir().unwrap();
    for site in SITES.iter() {
        let out = dir.path().join(format!("{}.pmtiles", site.id));
        let t = Instant::now();
        let s = extract(
            &HttpRanges {
                dl: &dl,
                source: osm::SOURCE,
                url: &url,
            },
            site.bbox,
            (0, OSM_MAXZOOM),
            &out,
        )
        .unwrap();
        println!(
            "{} {}: {s:?} in {:.1} s",
            site.id,
            build.key,
            t.elapsed().as_secs_f64()
        );
        let ours = TileArchive::open(&out).unwrap();
        let Some(refdir) = &reference else { continue };
        let theirs = TileArchive::open(refdir.join(format!("{}-osm.pmtiles", site.id))).unwrap();
        assert_eq!(ours.zoom_range(), theirs.zoom_range());
        assert_eq!(ours.addressed_tiles(), theirs.addressed_tiles());
        assert_eq!(ours.metadata().unwrap(), theirs.metadata().unwrap());
        let (mut same, mut absent) = (0, 0);
        for (z, x, y) in tiles_covering(site.bbox, 0, OSM_MAXZOOM) {
            let (a, b) = (
                ours.raw_tile(z, x, y).unwrap(),
                theirs.raw_tile(z, x, y).unwrap(),
            );
            assert_eq!(a, b, "tile {z}/{x}/{y}");
            if a.is_some() {
                same += 1;
            } else {
                absent += 1;
            }
        }
        println!(
            "  {same} tiles identical to go-pmtiles, {absent} absent from both; {} vs {} bytes",
            std::fs::metadata(&out).unwrap().len(),
            std::fs::metadata(theirs.path()).unwrap().len()
        );
    }
}
