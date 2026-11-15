//! Prints the features of one layer of a vector archive, optionally filtered by an
//! attribute (exploration while writing styles), one line per feature and zoom.
//!
//! ```sh
//! cargo run --release -p skimap-styles --example dump_layer -- \
//!     data/out/cervieres/pack/basemap/osm.pmtiles pois kind=parking 14
//! ```

use skimap_pack::reader::TileArchive;
use skimap_styles::inventory::{for_each_tile, tile_value_text};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().collect();
    let [_, path, layer, rest @ ..] = args.as_slice() else {
        return Err("usage: dump_layer <archive> <layer> [key=value] [zoom]".into());
    };
    let filter = rest
        .first()
        .and_then(|f| f.split_once('='))
        .map(|(k, v)| (k.to_string(), v.to_string()));
    let zoom: Option<u8> = rest.get(1).map(|z| z.parse()).transpose()?;
    let archive = TileArchive::open(path)?;
    for_each_tile(&archive, |z, tile| {
        if zoom.is_some_and(|want| want != z) {
            return;
        }
        for l in tile.iter().filter(|l| &l.name == layer) {
            for f in &l.features {
                let attrs: Vec<(String, String)> = l
                    .attributes(f)
                    .map(|(k, v)| (k.to_string(), tile_value_text(v)))
                    .collect();
                if let Some((k, v)) = &filter
                    && !attrs
                        .iter()
                        .any(|(ak, av)| ak == k && av.trim_matches('"') == v)
                {
                    continue;
                }
                let text: Vec<String> = attrs
                    .iter()
                    .filter(|(k, _)| !k.starts_with("name:") && !k.starts_with("pgf:"))
                    .map(|(k, v)| format!("{k}={v}"))
                    .collect();
                println!("z{z} {} {}", f.geom.name(), text.join(" "));
            }
        }
    })?;
    Ok(())
}
