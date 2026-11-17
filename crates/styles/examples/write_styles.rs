//! Writes the six styles into `styles/` (repository root), and with `--schema` the
//! snapshot of what the two packs offer (`styles/pack-schema.json`), read from
//! `data/out/<site>/pack`.
//!
//! ```sh
//! cargo run -p skimap-styles --example write_styles             # styles only
//! cargo run --release -p skimap-styles --example write_styles -- --schema
//! cargo run --release -p skimap-styles --example write_styles -- --france data/out/france-basemap
//! ```

#![allow(
    clippy::disallowed_methods,
    reason = "developer tool, not a pipeline output"
)]
use std::path::Path;

use skimap_styles::generate::{Kind, file_name, style_text};
use skimap_styles::palette::Theme;
use skimap_styles::{schema, styles_dir};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let dir = styles_dir();
    for kind in Kind::ALL {
        for theme in Theme::ALL {
            let path = dir.join(file_name(kind, theme));
            std::fs::write(&path, style_text(kind, theme))?;
            println!("{}", path.display());
        }
    }
    if std::env::args().any(|a| a == "--schema") {
        let data = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../data/out");
        // Same proof packs as the committed snapshot.
        let sites = schema::Snapshot::load()?.sites();
        let snapshot = schema::Snapshot::from_packs(&data, &sites)?;
        let path = dir.join(schema::FILE);
        std::fs::write(&path, snapshot.to_text())?;
        println!("{} ({} sites)", path.display(), snapshot.packs.len());
    }
    // `--france <dir>`: snapshot of the France basemap (X20), `styles/france-schema.json`.
    let args: Vec<String> = std::env::args().collect();
    if let Some(i) = args.iter().position(|a| a == "--france") {
        let src = args.get(i + 1).ok_or("--france <dir of france.json>")?;
        let inv = schema::france_from_dir(Path::new(src))?;
        let path = dir.join(schema::FRANCE_FILE);
        std::fs::write(&path, schema::inventory_text(&inv))?;
        println!("{}", path.display());
    }
    Ok(())
}
