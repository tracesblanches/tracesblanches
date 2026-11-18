//! Cost of adopting a Protomaps build by lowest zoom of the OSM basemap (#160): for each
//! site, the extracts of two builds (zooms 0 to 15, cached in
//! `data/cache/protomaps/extracts/`, read by ranges if absent), then for each lowest zoom
//! the pack archive of each build (as `osm::current_extract` writes it), its size, and the
//! tile patch from the old one to the new one (`patch::diff`, what the phone downloads),
//! with the bytes of the changed tiles by zoom. Variant `keep<z>`: the tiles below `z`
//! kept from the old build, those from `z` taken from the new one (option b of #160).
//!
//! ```sh
//! SKIMAP_DATA_DIR=<copy> cargo run --release -p skimap-pack --features pipeline \
//!     --example osm_zoom_cut -- <old AAAAMMJJ> <new AAAAMMJJ> <site>...
//! ```
//!
//! Writes only into the cache of the data directory (the extracts) and a temporary
//! directory.

use std::fs::File;
use std::path::Path;

use skimap_common::config::Config;
use skimap_common::download::Downloader;
use skimap_common::geom::BBox;
use skimap_common::site::Registry;
use skimap_pack::build::extract::Archive;
use skimap_pack::build::osm;
use skimap_pack::build::raster::write_archive;
use skimap_pack::patch;
use skimap_pack::reader::TileArchive;
use skimap_pack::tiles::tiles_covering;

const CUTS: [u8; 6] = [0, 7, 9, 10, 11, 12];

/// The pack archive of `full` (zooms 0-15) from zoom `cut`, as the pipeline writes it.
fn cut_archive(full: &Path, bbox: BBox, cut: u8, out: &Path) -> Vec<u8> {
    let f = File::open(full).expect("extract");
    Archive::open(&f)
        .expect("archive")
        .extract(bbox, (cut, 15), out)
        .expect("cut");
    std::fs::read(out).expect("cut archive")
}

/// Low zooms (below `cut`) of `old`, the others of `new`, metadata of `new`.
fn mixed(old: &Path, new: &Path, bbox: BBox, cut: u8, out: &Path) -> Vec<u8> {
    let (a, b) = (
        TileArchive::open(old).unwrap(),
        TileArchive::open(new).unwrap(),
    );
    let tiles: Vec<_> = tiles_covering(bbox, 0, 15)
        .into_iter()
        .filter_map(|(z, x, y)| {
            let from = if z < cut { &a } else { &b };
            from.raw_tile(z, x, y).unwrap().map(|t| ((z, x, y), t))
        })
        .collect();
    let meta: serde_json::Value = serde_json::from_str(&b.metadata().unwrap()).unwrap();
    write_archive(
        out,
        pmtiles::TileType::Mvt,
        Some(pmtiles::Compression::Gzip),
        &tiles,
        &meta,
        bbox,
        0,
        15,
    )
    .unwrap();
    std::fs::read(out).unwrap()
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_env_filter(std::env::var("RUST_LOG").unwrap_or_else(|_| "warn".to_owned()))
        .init();
    let args: Vec<String> = std::env::args().skip(1).collect();
    let [old_key, new_key, ids @ ..] = args.as_slice() else {
        return Err("usage: osm_zoom_cut <old AAAAMMJJ> <new AAAAMMJJ> <site>...".into());
    };
    let cfg = Config::from_env()?;
    let layout = cfg.layout();
    let dl = Downloader::from_config(&cfg)?;
    let registry = Registry::load(&layout)?;
    let (old_b, new_b) = (
        osm::select_build(&dl, Some(old_key))?,
        osm::select_build(&dl, Some(new_key))?,
    );
    let tmp = tempfile::tempdir()?;
    let mut totals = vec![(0u64, 0u64); CUTS.len()];
    let mut keep_total = (0u64, 0u64);
    for id in ids {
        let site = registry.get(id)?;
        let old = osm::cached_extract(site, &layout, &dl, &old_b, (0, 15))?;
        let new = osm::cached_extract(site, &layout, &dl, &new_b, (0, 15))?;
        println!(
            "== {id}: {} tiles at z15",
            tiles_covering(site.bbox, 15, 15).len()
        );
        for (i, &cut) in CUTS.iter().enumerate() {
            let a = cut_archive(&old, site.bbox, cut, &tmp.path().join("a.pmtiles"));
            let b = cut_archive(&new, site.bbox, cut, &tmp.path().join("b.pmtiles"));
            let (p, d) = patch::diff(&a, &b)?;
            let zooms: Vec<String> = d
                .by_zoom
                .iter()
                .filter(|(_, z)| z.added + z.changed + z.removed > 0)
                .map(|(k, z)| format!("z{k}:{}={}B", z.added + z.changed, z.new_bytes))
                .collect();
            println!(
                "  z{cut:>2}-15: archive {:>9} B, patch {:>9} B, tiles changed {:>2} [{}]",
                b.len(),
                p.len(),
                d.tiles_added + d.tiles_changed + d.tiles_removed,
                zooms.join(" ")
            );
            totals[i].0 += b.len() as u64;
            totals[i].1 += p.len() as u64;
        }
        let cut = skimap_pack::OSM_MINZOOM;
        let a = cut_archive(&old, site.bbox, 0, &tmp.path().join("a.pmtiles"));
        let b = mixed(&old, &new, site.bbox, cut, &tmp.path().join("m.pmtiles"));
        let (p, _) = patch::diff(&a, &b)?;
        println!(
            "  keep{cut} (z0-{} of {old_key}, z{cut}-15 of {new_key}): archive {:>9} B, patch {:>9} B",
            cut - 1,
            b.len(),
            p.len()
        );
        keep_total.0 += b.len() as u64;
        keep_total.1 += p.len() as u64;
    }
    println!("== {} sites", ids.len());
    for (i, &cut) in CUTS.iter().enumerate() {
        println!(
            "  z{cut:>2}-15: archives {:>10} B, patches {:>10} B",
            totals[i].0, totals[i].1
        );
    }
    println!(
        "  keep{}: archives {:>10} B, patches {:>10} B",
        skimap_pack::OSM_MINZOOM,
        keep_total.0,
        keep_total.1
    );
    Ok(())
}
