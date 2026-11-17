//! X20 (#154): builds the France basemap and prints its size, by highest zoom.
//!
//! ```sh
//! cargo run --release -p skimap-pack --features pipeline --example france_basemap -- \
//!     <out_dir> [maxzoom...]
//! ```
//!
//! Without zooms, builds [`skimap_pack::france::MAXZOOM`] into `<out_dir>` (kept while the
//! build in use is recent). With zooms, builds each into `<out_dir>/z<zoom>` and prints
//! the size of the archive, of the glyphs and the tiles per zoom.

use std::path::PathBuf;

use skimap_common::config::Config;
use skimap_common::download::Downloader;
use skimap_common::layout::DataLayout;
use skimap_pack::TileArchive;
use skimap_pack::build::france;
use skimap_pack::france::{MAXZOOM, TILES_FILE};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_env_filter(std::env::var("RUST_LOG").unwrap_or_else(|_| "info".to_owned()))
        .init();
    let mut args = std::env::args().skip(1);
    let out = PathBuf::from(args.next().ok_or("france_basemap <out_dir> [maxzoom...]")?);
    let zooms: Vec<u8> = args.map(|a| a.parse()).collect::<Result<_, _>>()?;
    let cfg = Config::from_env()?;
    let layout = DataLayout::new(&cfg.data_dir);
    let dl = Downloader::from_config(&cfg)?;
    let runs: Vec<(u8, PathBuf)> = if zooms.is_empty() {
        vec![(MAXZOOM, out)]
    } else {
        zooms
            .iter()
            .map(|z| (*z, out.join(format!("z{z}"))))
            .collect()
    };
    for (z, dir) in runs {
        let b = france::build(&layout, &dl, &dir, None, z)?;
        let tiles = b.basemap.tiles().map_or(0, |f| f.bytes);
        let archive = TileArchive::open(dir.join(TILES_FILE))?;
        println!(
            "z0-{z}: build {} ({}), {} tuiles, archive {:.2} Mo, total {:.2} Mo ({} fichiers), {:.1} s{}",
            b.basemap.build,
            b.basemap.data_date,
            archive.addressed_tiles().unwrap_or(0),
            tiles as f64 / 1e6,
            b.basemap.bytes() as f64 / 1e6,
            b.basemap.files.len(),
            b.seconds,
            if b.kept { " (conservé)" } else { "" }
        );
        if let Some(s) = b.extract {
            println!(
                "  extrait : {} tuiles, {} contenus, {} lectures, {:.2} Mo lus",
                s.tiles,
                s.contents,
                s.data_reads,
                s.data_bytes_read as f64 / 1e6
            );
        }
    }
    Ok(())
}
