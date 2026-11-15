//! Writes the six styles into `styles/` (repository root), and with `--schema` the
//! snapshot of what the two packs offer (`styles/pack-schema.json`), read from
//! `data/out/<site>/pack`.
//!
//! ```sh
//! cargo run -p skimap-styles --example write_styles             # styles only
//! cargo run --release -p skimap-styles --example write_styles -- --schema
//! ```

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
        let snapshot = schema::Snapshot::from_packs(&data)?;
        let path = dir.join(schema::FILE);
        std::fs::write(&path, snapshot.to_text())?;
        println!("{} ({} sites)", path.display(), snapshot.packs.len());
    }
    Ok(())
}
