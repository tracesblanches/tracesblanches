//! Prints the inventory of a pack as JSON (exploration while writing styles).
//!
//! ```sh
//! cargo run --release -p skimap-styles --example inventory -- data/out/<site>/pack
//! cargo run --release -p skimap-styles --example inventory -- data/out/france-basemap
//! ```

use std::path::PathBuf;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let dir: PathBuf = std::env::args()
        .nth(1)
        .ok_or("usage: inventory <pack dir>")?
        .into();
    let inv = if dir.join(skimap_pack::france::DESCRIPTION_FILE).is_file() {
        skimap_styles::inventory::Inventory::from_france(&dir)?
    } else {
        skimap_styles::inventory::Inventory::from_pack(&dir)?
    };
    println!("{}", serde_json::to_string_pretty(&inv)?);
    Ok(())
}
