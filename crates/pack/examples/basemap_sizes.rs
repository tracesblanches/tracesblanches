//! P4 measurement: size of the basemaps and of the slope raster per site and zoom range.
//!
//! ```sh
//! cargo run --release -p skimap-pack --features pipeline --example basemap_sizes -- \
//!     [ign_maxzoom=17] [site...]
//! ```
//!
//! Downloads (shared downloader, cache) the Plan IGN tiles up to `ign_maxzoom`, extracts the
//! Protomaps build to zoom 15 by range requests, then writes one PMTiles per candidate
//! highest zoom into a temporary directory and prints the sizes.

use std::time::Instant;

use serde_json::json;
use skimap_common::config::Config;
use skimap_common::download::Downloader;
use skimap_common::site::{self, SITES};
use skimap_pack::TileArchive;
use skimap_pack::build::{osm, plan_ign, raster};
use skimap_pack::tiles::tiles_covering;

/// `warm <site> <maxzoom> <seconds>`: fills the downloader cache with the Plan IGN tiles of a
/// site for at most `seconds`, so that a long download runs as a series of short commands.
fn warm(mut args: impl Iterator<Item = String>) -> Result<(), Box<dyn std::error::Error>> {
    let s = site::site(&args.next().ok_or("warm <site> <maxzoom> <seconds>")?)?;
    let maxz: u8 = args.next().ok_or("maxzoom")?.parse()?;
    let budget = std::time::Duration::from_secs(args.next().ok_or("seconds")?.parse()?);
    let cfg = Config::from_env()?;
    let dl = Downloader::from_config(&cfg)?;
    let list = tiles_covering(s.bbox, 0, maxz);
    let t = Instant::now();
    let mut done = 0;
    for (z, x, y) in &list {
        if t.elapsed() > budget {
            break;
        }
        plan_ign::fetch_tile(&dl, *z, *x, *y)?;
        done += 1;
    }
    println!(
        "{}: {done}/{} tiles z0-{maxz} in cache after {:.0} s{}",
        s.id,
        list.len(),
        t.elapsed().as_secs_f64(),
        if done == list.len() {
            " (complete)"
        } else {
            ""
        }
    );
    Ok(())
}

fn mb(n: u64) -> f64 {
    n as f64 / 1e6
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_env_filter(std::env::var("RUST_LOG").unwrap_or_else(|_| "warn".to_owned()))
        .init();
    // usage: basemap_sizes [ign_maxzoom] [site...] | warm <site> <maxzoom> <seconds>
    let mut args = std::env::args().skip(1).peekable();
    if args.peek().map(String::as_str) == Some("warm") {
        args.next();
        return warm(args);
    }
    let ign_max: u8 = match args.peek().map(|a| a.parse::<u8>()) {
        Some(Ok(z)) => {
            args.next();
            z
        }
        _ => 17,
    };
    let ids: Vec<String> = args.collect();
    let sites: Vec<_> = if ids.is_empty() {
        SITES.iter().collect()
    } else {
        ids.iter()
            .map(|i| site::site(i))
            .collect::<Result<_, _>>()?
    };
    let cfg = Config::from_env()?;
    let layout = cfg.layout();
    let dl = Downloader::from_config(&cfg)?;
    let tmp = tempfile::tempdir()?;
    let mut report = Vec::new();
    for s in sites {
        println!("== {} ({})", s.name, s.id);
        // Plan IGN.
        let t = Instant::now();
        let tiles = plan_ign::fetch_tiles(&dl, s.bbox, 0, ign_max)?;
        println!(
            "plan-ign: {} tiles z0-{ign_max} in {:.1} s",
            tiles.len(),
            t.elapsed().as_secs_f64()
        );
        let meta = json!({"name": "Plan IGN v2", "vector_layers": []});
        let mut ign_rows = Vec::new();
        for maxz in 12..=ign_max {
            let sel: Vec<_> = tiles
                .iter()
                .filter(|((z, _, _), _)| *z <= maxz)
                .cloned()
                .collect();
            let raw: u64 = sel.iter().map(|(_, t)| t.len() as u64).sum();
            let p = tmp.path().join(format!("{}-ign-{maxz}.pmtiles", s.id));
            let bytes = plan_ign::write_pmtiles(&p, &sel, &meta, s.bbox, 0, maxz)?;
            let at_z = tiles.iter().filter(|((z, _, _), _)| *z == maxz).count();
            println!(
                "  plan-ign z0-{maxz:>2}: {:>6} tiles ({at_z:>5} at z{maxz}), MVT raw {:>7.2} MB, PMTiles gzip {:>7.2} MB",
                sel.len(),
                mb(raw),
                mb(bytes)
            );
            ign_rows.push(json!({"maxzoom": maxz, "tiles": sel.len(), "tiles_at_maxzoom": at_z, "raw_bytes": raw, "pmtiles_bytes": bytes}));
        }
        // OSM (Protomaps).
        let t = Instant::now();
        let build = osm::select_build(&dl, None)?;
        let ext = osm::cached_extract(s, &layout, &dl, &build, 15)?;
        println!(
            "osm: {} extract z0-15 ready in {:.1} s ({})",
            build.key,
            t.elapsed().as_secs_f64(),
            ext.display()
        );
        let a = TileArchive::open(&ext)?;
        let (minz, maxz_a) = a.zoom_range();
        let mut all = Vec::new();
        for (z, x, y) in tiles_covering(s.bbox, minz, maxz_a) {
            if let Some(t) = a.raw_tile(z, x, y)? {
                all.push(((z, x, y), t));
            }
        }
        let mut osm_rows = Vec::new();
        for maxz in 12..=maxz_a {
            let sel: Vec<_> = all
                .iter()
                .filter(|((z, _, _), _)| *z <= maxz)
                .cloned()
                .collect();
            let p = tmp.path().join(format!("{}-osm-{maxz}.pmtiles", s.id));
            let bytes = raster::write_archive(
                &p,
                pmtiles::TileType::Mvt,
                Some(a.tile_compression()),
                &sel,
                &json!({"name": "osm"}),
                s.bbox,
                minz,
                maxz,
            )?;
            println!(
                "  osm      z{minz}-{maxz:>2}: {:>6} tiles, PMTiles {:>7.2} MB",
                sel.len(),
                mb(bytes)
            );
            osm_rows.push(json!({"maxzoom": maxz, "tiles": sel.len(), "pmtiles_bytes": bytes}));
        }
        let ext_bytes = std::fs::metadata(&ext)?.len();
        println!(
            "  osm      CLI extract file: {:.2} MB, {} addressed tiles (header), {} tiles in the site box",
            mb(ext_bytes),
            a.addressed_tiles().unwrap_or(0),
            all.len()
        );
        // Slope raster.
        let slope_path = layout.layer_dir(s, "terrain")?.join("slope_5m.tif");
        let mut slope_rows = Vec::new();
        if slope_path.is_file() {
            let t = Instant::now();
            let grid = skimap_common::geotiff::read_f32(&slope_path)?;
            println!(
                "slope: read {} x {} in {:.1} s",
                grid.width(),
                grid.height(),
                t.elapsed().as_secs_f64()
            );
            for maxz in 14..=16u8 {
                let mut spec = raster::catalog()
                    .into_iter()
                    .find(|r| r.id == "slope")
                    .ok_or("slope spec")?;
                spec.maxzoom = maxz;
                let t = Instant::now();
                let tiles = raster::render(&grid, &spec)?;
                let t_render = t.elapsed().as_secs_f64();
                let png = raster::encode_tiles(&tiles, &spec.palette())?;
                let t_png = t.elapsed().as_secs_f64() - t_render;
                let p = tmp.path().join(format!("{}-slope-{maxz}.pmtiles", s.id));
                let bytes = raster::write_archive(
                    &p,
                    pmtiles::TileType::Png,
                    None,
                    &png,
                    &json!({}),
                    s.bbox,
                    spec.minzoom,
                    maxz,
                )?;
                println!(
                    "  slope    z{}-{maxz}: {:>5} tiles, {:>6.2} MB, render {t_render:.2} s, PNG {t_png:.2} s",
                    spec.minzoom,
                    png.len(),
                    mb(bytes)
                );
                slope_rows.push(json!({"maxzoom": maxz, "tiles": png.len(), "pmtiles_bytes": bytes, "render_s": t_render, "png_s": t_png}));
            }
        }
        report.push(json!({"site": s.id, "plan_ign": ign_rows, "osm_build": build.key, "osm": osm_rows, "slope": slope_rows}));
    }
    println!("{}", serde_json::to_string_pretty(&report)?);
    Ok(())
}
