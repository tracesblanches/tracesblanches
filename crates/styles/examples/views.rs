//! Views for the screenshots of a pack: the z13 tile with most terrain of 35° and more
//! (steep zone), the busiest junction of the piste graph (intersection), the extent of the
//! pistes (domain).
//!
//! ```sh
//! cargo run --release -p skimap-styles --example views -- data/out/cervieres/pack
//! ```

use skimap_common::geom::BBox;
use skimap_pack::reader::Pack;
use skimap_pack::tiles::{pixel_lat, pixel_lon, tiles_covering};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let dir = std::env::args().nth(1).ok_or("usage: views <pack dir>")?;
    let pack = Pack::open(&dir)?;
    let [w, s, e, n] = pack.manifest().site.bbox;
    // Steep zone: classes 2.. (35° and more) of the slope raster, at z13.
    let slope = pack.tiles("slope")?;
    let mut best = (0usize, 0u32, 0u32);
    for (z, x, y) in tiles_covering(BBox::new(s, w, n, e), 13, 13) {
        let Some(bytes) = slope.tile(z, x, y)? else {
            continue;
        };
        let img = skimap_pack::png::decode_indexed(&bytes)?;
        let steep = img
            .indices
            .iter()
            .filter(|&&i| (2..=4).contains(&i))
            .count();
        if steep > best.0 {
            best = (steep, x, y);
        }
    }
    let (_, x, y) = best;
    let lon = pixel_lon((f64::from(x) + 0.5) * 256.0, 13);
    let lat = pixel_lat((f64::from(y) + 0.5) * 256.0, 13);
    println!("steep {lat:.5} {lon:.5} ({} px >= 35°)", best.0);
    let read = |id: &str| -> Result<serde_json::Value, Box<dyn std::error::Error>> {
        Ok(serde_json::from_slice(
            &pack.read_file(&pack.layer(id)?.file)?,
        )?)
    };
    // Junction: node of highest degree.
    let nodes = read("pistes-graph-nodes")?;
    let mut top: Option<(u64, &serde_json::Value)> = None;
    for f in nodes["features"].as_array().into_iter().flatten() {
        let deg = f["properties"]["degree"].as_u64().unwrap_or(0);
        if top.is_none_or(|(d, _)| deg > d) {
            top = Some((deg, &f["geometry"]["coordinates"]));
        }
    }
    if let Some((deg, p)) = top {
        println!("junction {} {} (degree {deg})", p[1], p[0]);
    }
    // Domain: extent of the pistes.
    let pistes = read("pistes")?;
    let (mut x0, mut y0, mut x1, mut y1) = (180.0f64, 90.0f64, -180.0f64, -90.0f64);
    for f in pistes["features"].as_array().into_iter().flatten() {
        for p in f["geometry"]["coordinates"]
            .as_array()
            .into_iter()
            .flatten()
        {
            let (x, y) = (p[0].as_f64().unwrap_or(x0), p[1].as_f64().unwrap_or(y0));
            x0 = x0.min(x);
            x1 = x1.max(x);
            y0 = y0.min(y);
            y1 = y1.max(y);
        }
    }
    println!(
        "domain {:.5} {:.5} (extent {x0:.4},{y0:.4} {x1:.4},{y1:.4})",
        (y0 + y1) / 2.0,
        (x0 + x1) / 2.0
    );
    Ok(())
}
