//! Checks the six styles against any packs (public packs of `build-all`, the test pack of
//! the app): each pack alone must offer every required source, its fonts and icons; the
//! layers of the optional sources it lacks are left out (as the app and the viewer do);
//! properties and values are looked up in the pack and the snapshot of the proof packs.
//! Prints, per pack and style, the sources left out and the layers that draw something.
//!
//! ```sh
//! cargo run --release -p skimap-styles --example check_packs -- <pack dir>...
//! ```
//!
//! Exit status 1 when a style is refused for a pack.

use std::path::Path;

use skimap_styles::generate::{Kind, file_name, style};
use skimap_styles::inventory::Inventory;
use skimap_styles::orphan;
use skimap_styles::palette::Theme;
use skimap_styles::schema::Snapshot;
use skimap_styles::validate::{unchecked_sources, validate_with};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let dirs: Vec<String> = std::env::args().skip(1).collect();
    if dirs.is_empty() {
        return Err("usage: check_packs <pack dir>...".into());
    }
    let snapshot = Snapshot::load()?;
    let mut refused = 0;
    for dir in &dirs {
        let inv = Inventory::from_pack(Path::new(dir))?;
        println!("== {} ({dir})", inv.site);
        for kind in Kind::ALL {
            for theme in Theme::ALL {
                let s = style(kind, theme);
                let name = file_name(kind, theme);
                let packs = std::slice::from_ref(&inv);
                let errors = validate_with(&s, packs, &snapshot.packs);
                let left_out = unchecked_sources(&s, packs);
                let counts = orphan::matches(&s, &[Path::new(dir)])?;
                let drawn = counts.values().filter(|n| **n > 0).count();
                let layers = s["layers"].as_array().map_or(0, Vec::len);
                println!(
                    "  {name:<20} {}  {drawn}/{layers} layers draw ; sources left out: {left_out:?}",
                    if errors.is_empty() {
                        "OK     "
                    } else {
                        "REFUSED"
                    },
                );
                for e in &errors {
                    println!("      {e}");
                }
                if !errors.is_empty() {
                    refused += 1;
                }
                if theme == Theme::Clair {
                    let mut ours: Vec<String> = counts
                        .iter()
                        .filter(|(id, n)| {
                            **n > 0 && !id.starts_with("ign") && !id.starts_with("osm")
                        })
                        .map(|(id, n)| format!("{id} {n}"))
                        .collect();
                    ours.sort();
                    println!("      {}", ours.join(", "));
                }
            }
        }
    }
    if refused > 0 {
        return Err(format!("{refused} style(s) refused").into());
    }
    Ok(())
}
