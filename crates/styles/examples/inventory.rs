//! Prints the inventory of a pack as JSON (exploration while writing styles).
//!
//! ```sh
//! cargo run --release -p skimap-styles --example inventory -- data/out/cervieres/pack
//! ```

use std::path::PathBuf;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let dir: PathBuf = std::env::args()
        .nth(1)
        .ok_or("usage: inventory <pack dir>")?
        .into();
    let inv = skimap_styles::inventory::Inventory::from_pack(&dir)?;
    println!("{}", serde_json::to_string_pretty(&inv)?);
    Ok(())
}
