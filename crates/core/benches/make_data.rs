//! Writes the inputs of the instruction-count benchmarks (Q5, #105) into `benches/data/`
//! at the repository root, from the real outputs of the pipeline (`data/out`):
//!
//! - `cervieres_offpiste/*.skr.gz`, `zone_classes.tsv`: the off-piste rasters of Cervières
//!   cropped around the Le Laus → Col d'Izoard proof route (C3) with a 600 m margin, and the
//!   snow of 15/02/2026 over the same area. Only publishable sources: the exposure is the
//!   model's alone (`avalanche-exposure/exposure_model_5m.tif`, before the fusion with the
//!   CLPA map), and the zone identifiers of sources whose licence is not verified
//!   ([`UNPUBLISHABLE_ZONE_PREFIXES`]) are left out of the class table (the router of the
//!   benchmarks uses an open zone policy: the cost does not depend on them);
//! - `cervieres_piste_graph.json.gz`, `chapelle-des-bois_piste_graph.json.gz`: the trail
//!   graphs of both sites with their elevation profiles, whole;
//! - `cervieres_avalanche_dem.tif`, `cervieres_avalanche_canopy.tif`: the crop of the 5 m
//!   inputs of the avalanche exposure model with the most « très exposé » cells of the site
//!   (the model's work grows with the release areas and their runouts);
//! - `bench_data.meta.json`: sources, licences, crops and sizes.
//!
//! ```sh
//! SKIMAP_MAKE_BENCH_DATA=1 cargo bench -p skimap-core --features pipeline --bench make_data
//! ```
//!
//! Without `SKIMAP_MAKE_BENCH_DATA` it does nothing (so that `cargo bench` never rewrites
//! the versioned inputs).

use std::io::Write;
use std::path::{Path, PathBuf};

use serde_json::json;
use skimap_common::geom::LonLat;
use skimap_common::grid::{GeoTransform, Grid};
use skimap_core::offpiste::{
    ELEVATION_NODATA, Encoding, FILE_ELEVATION, FILE_EXPOSURE, FILE_PISTES, FILE_SLOPE,
    FILE_ZONE_CLASSES, FILE_ZONES, OffpisteGrid, RasterValue, SnowGrid, Window, encode_raster_with,
    encode_zone_classes,
};

type Res<T> = Result<T, Box<dyn std::error::Error>>;

/// Le Laus (hamlet) and Col d'Izoard (2361 m), the C3 proof route.
pub const LAUS: LonLat = LonLat::new(6.72687, 44.8552163);
/// Col d'Izoard.
pub const IZOARD: LonLat = LonLat::new(6.734975, 44.8196595);
/// Margin of the off-piste crop around the route ends, cells (600 m).
const MARGIN_CELLS: usize = 120;
/// Zone identifier prefixes of sources that may not be published (F5, #11).
const UNPUBLISHABLE_ZONE_PREFIXES: &[&str] = &["biodiv:"];
/// Side of the avalanche crop, cells of 5 m (320 m).
const AVALANCHE_CELLS: usize = 64;

fn gz(bytes: &[u8]) -> Res<Vec<u8>> {
    let mut e = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::best());
    e.write_all(bytes)?;
    Ok(e.finish()?)
}

fn crop<T: Copy>(g: &Grid<T>, c0: usize, r0: usize, w: usize, h: usize) -> Res<Grid<T>> {
    let t = g.transform();
    let mut data = Vec::with_capacity(w * h);
    for r in r0..r0 + h {
        let row = r * g.width();
        data.extend_from_slice(&g.data()[row + c0..row + c0 + w]);
    }
    let (x, y) = t.pixel_to_world(c0 as f64, r0 as f64);
    let gt = GeoTransform {
        origin_x: x,
        origin_y: y,
        pixel_width: t.pixel_width,
        pixel_height: t.pixel_height,
    };
    Ok(Grid::from_vec(w, h, gt, g.crs(), data)?)
}

fn write(path: &Path, bytes: &[u8]) -> Res<u64> {
    std::fs::create_dir_all(path.parent().ok_or("no parent")?)?;
    std::fs::write(path, bytes)?;
    println!("{} ({} bytes)", path.display(), bytes.len());
    Ok(bytes.len() as u64)
}

fn write_skr<T: RasterValue>(path: &Path, g: &Grid<T>, nodata: T) -> Res<u64> {
    write(
        path,
        &gz(&encode_raster_with(g, nodata, Encoding::RowDelta)?)?,
    )
}

fn main() -> Res<()> {
    if std::env::var_os("SKIMAP_MAKE_BENCH_DATA").is_none() {
        println!("make_data: set SKIMAP_MAKE_BENCH_DATA=1 to regenerate benches/data");
        return Ok(());
    }
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let data: PathBuf =
        std::env::var_os("SKIMAP_DATA_DIR").map_or(root.join("data"), PathBuf::from);
    let out = root.join("benches/data");
    let mut files = Vec::new();

    // Off-piste rasters around the Laus → Izoard route.
    let dir = data.join("out/cervieres/offpiste");
    let g = OffpisteGrid::load(&dir)?;
    let a = g.cell_of(LAUS)?.ok_or("Le Laus outside the grid")?;
    let b = g.cell_of(IZOARD)?.ok_or("Izoard outside the grid")?;
    let win = Window::around(&g, a, b, MARGIN_CELLS);
    let (c0, r0, w, h) = (win.col0, win.row0, win.width, win.height);
    let od = out.join("cervieres_offpiste");
    let e = crop(&g.elevation, c0, r0, w, h)?;
    files.push((
        FILE_ELEVATION,
        write_skr(
            &od.join(format!("{FILE_ELEVATION}.gz")),
            &e,
            ELEVATION_NODATA,
        )?,
    ));
    for (name, grid) in [
        (FILE_SLOPE, &g.slope),
        (FILE_ZONES, &g.zones),
        (FILE_PISTES, &g.pistes),
    ] {
        let c = crop(grid, c0, r0, w, h)?;
        let nd = *grid.nodata().unwrap_or(&u8::MAX);
        files.push((name, write_skr(&od.join(format!("{name}.gz")), &c, nd)?));
    }
    // The model's exposure, same grid as the off-piste rasters.
    let ex = skimap_common::geotiff::read_u8(
        &data.join("out/cervieres/avalanche-exposure/exposure_model_5m.tif"),
    )?;
    if ex.transform() != g.elevation.transform() || ex.width() != g.width() {
        return Err("exposure model and off-piste grids differ".into());
    }
    let exc = crop(&ex, c0, r0, w, h)?;
    files.push((
        FILE_EXPOSURE,
        write_skr(&od.join(format!("{FILE_EXPOSURE}.gz")), &exc, 255)?,
    ));
    let mut classes = g.classes.clone();
    for c in &mut classes {
        c.zone_ids.retain(|id| {
            !UNPUBLISHABLE_ZONE_PREFIXES
                .iter()
                .any(|p| id.starts_with(p))
        });
    }
    let classes = encode_zone_classes(&classes)?;
    files.push((
        FILE_ZONE_CLASSES,
        write(&od.join(FILE_ZONE_CLASSES), classes.as_bytes())?,
    ));
    // Snow (25 m) over the crop: the cells containing the crop corners and everything between.
    let snow = SnowGrid::load(&dir.join("snow_2026-02-15.skr"))?;
    let (x0, y0) = e.transform().pixel_to_world(0.0, 0.0);
    let (x1, y1) = e.transform().pixel_to_world(w as f64, h as f64);
    let (sc0, sr0) = snow
        .grid
        .cell_at(x0, y0)
        .ok_or("snow grid misses the crop")?;
    let (sc1, sr1) = snow
        .grid
        .cell_at(x1 - 1e-6, y1 + 1e-6)
        .ok_or("snow grid misses the crop")?;
    let sn = crop(&snow.grid, sc0, sr0, sc1 - sc0 + 1, sr1 - sr0 + 1)?;
    files.push((
        "snow_2026-02-15.skr",
        write_skr(
            &od.join("snow_2026-02-15.skr.gz"),
            &sn,
            SnowGrid::pack(None, None),
        )?,
    ));

    // Trail graphs, whole.
    for site in ["cervieres", "chapelle-des-bois"] {
        let g = std::fs::read(data.join(format!("out/{site}/routes/piste_graph.json")))?;
        let name = format!("{site}_piste_graph.json.gz");
        let n = write(&out.join(&name), &gz(&g)?)?;
        files.push((
            if site == "cervieres" {
                "cervieres_piste_graph"
            } else {
                "chapelle_piste_graph"
            },
            n,
        ));
    }

    // Avalanche model inputs below the Col d'Izoard.
    let inputs = data.join("out/cervieres/avalanche-exposure/inputs");
    let dem = skimap_common::geotiff::read_f32(&inputs.join("dem_5m.tif"))?;
    let canopy = skimap_common::geotiff::read_f32(&inputs.join("canopy_5m.tif"))?;
    // Window (stride half a window) with the most « très exposé » (3) cells of the layer.
    let exposure = skimap_common::geotiff::read_u8(
        &data.join("out/cervieres/avalanche-exposure/exposure_5m.tif"),
    )?;
    let (ew, eh) = (exposure.width(), exposure.height());
    let n = AVALANCHE_CELLS;
    let mut best = (0usize, 0usize, 0usize);
    for r0 in (0..eh.saturating_sub(n)).step_by(n / 2) {
        for c0 in (0..ew.saturating_sub(n)).step_by(n / 2) {
            let k = (r0..r0 + n)
                .map(|r| {
                    exposure.data()[r * ew + c0..r * ew + c0 + n]
                        .iter()
                        .filter(|v| **v == 3)
                        .count()
                })
                .sum::<usize>();
            if k > best.2 {
                best = (c0, r0, k);
            }
        }
    }
    let (ac0, ar0, high_cells) = best;
    if exposure.transform() != dem.transform() {
        return Err("exposure and DEM grids differ".into());
    }
    for (name, g) in [
        ("cervieres_avalanche_dem.tif", &dem),
        ("cervieres_avalanche_canopy.tif", &canopy),
    ] {
        let c = crop(g, ac0, ar0, AVALANCHE_CELLS, AVALANCHE_CELLS)?;
        let mut buf = std::io::Cursor::new(Vec::new());
        skimap_common::geotiff::encode_f32(&mut buf, &c)?;
        files.push((
            if name.contains("dem") {
                "avalanche_dem"
            } else {
                "avalanche_canopy"
            },
            write(&out.join(name), &buf.into_inner())?,
        ));
    }

    let meta = json!({
        "tickets": ["Q5 #105"],
        "generator": "cargo bench -p skimap-core --features pipeline --bench make_data (SKIMAP_MAKE_BENCH_DATA=1)",
        "offpiste": {
            "source": "data/out/cervieres/offpiste (C3 prepare): 5 m Lambert-93 rasters",
            "route": {"from": [LAUS.lon, LAUS.lat], "to": [IZOARD.lon, IZOARD.lat], "name": "Le Laus → Col d'Izoard"},
            "crop_cells": {"col0": c0, "row0": r0, "width": w, "height": h, "margin_cells": MARGIN_CELLS},
            "snow": "snow_2026-02-15.skr cropped (25 m)",
            "exposure": "avalanche-exposure/exposure_model_5m.tif cropped: the model alone, without the fusion of prepare (not versioned)",
            "zone_classes": "zone_classes.tsv of prepare, identifiers of unverified licence removed",
        },
        "graphs": "data/out/<site>/routes/piste_graph.json, whole (C1)",
        "avalanche": {
            "source": "data/out/cervieres/avalanche-exposure/inputs/{dem,canopy}_5m.tif (P17)",
            "selection": "window with the most « très exposé » cells of exposure_5m.tif (stride half a window)",
            "high_cells": high_cells,
            "crop_cells": {"col0": ac0, "row0": ar0, "width": AVALANCHE_CELLS, "height": AVALANCHE_CELLS},
        },
        "licences": {
            "terrain (elevation, slope, avalanche DEM)": "IGN LiDAR HD, Licence Ouverte 2.0 (Etalab)",
            "canopy": "IGN LiDAR HD MNH, Licence Ouverte 2.0 (Etalab)",
            "pistes, trail graphs": "© les contributeurs d'OpenStreetMap, ODbL 1.0",
            "zones": "identifiers only (INPN PatriNat, communes, départements, GPU, country)",
            "snow": "Météo-France, Licence Ouverte 2.0",
            "exposure": "model output from IGN LiDAR HD (AutoATES port, P17)",
        },
        "bytes": files.iter().map(|(n, b)| json!({"file": n, "bytes": b})).collect::<Vec<_>>(),
    });
    let text = serde_json::to_string_pretty(&meta)? + "\n";
    write(&out.join("bench_data.meta.json"), text.as_bytes())?;
    Ok(())
}
