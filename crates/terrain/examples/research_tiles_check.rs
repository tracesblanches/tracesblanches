//! Checks that the LiDAR HD tiles of research report 07 (`research/raw/census_ava/ign/dem5`)
//! are byte-identical to the tiles fetched by [`skimap_terrain::dem::fetch_tile`].
//!
//! ```sh
//! cargo run --release -p skimap-terrain --example research_tiles_check -- \
//!     /path/to/research/raw/census_ava/ign/dem5
//! ```

use skimap_common::config::Config;
use skimap_common::download::{Downloader, Request};
use skimap_terrain::dem::{SOURCE, TileId};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let dir = std::path::PathBuf::from(
        std::env::args()
            .nth(1)
            .ok_or("usage: research_tiles_check <dem5 dir>")?,
    );
    let cfg = Config::from_env()?;
    let dl = Downloader::from_config(&cfg)?;
    let urls = std::fs::read_to_string(dir.join("urls.txt"))?;
    let (mut same, mut differ) = (0, 0);
    for line in urls.lines() {
        let mut it = line.split_whitespace();
        let (Some(prefix), Some(name), Some(url)) = (it.next(), it.next(), it.next()) else {
            continue;
        };
        let (kx, ky) = name.split_once('-').ok_or("bad tile name")?;
        let t = TileId {
            kx: kx.parse()?,
            ky: ky.parse()?,
        };
        assert_eq!(t.url(), url, "URL scheme differs for {name}");
        // Make sure the tile is in the cache (decoded and checked), then compare bytes.
        skimap_terrain::dem::fetch_tile(t, &dl)?;
        let ours = dl.fetch(&Request::get(SOURCE, &t.url()))?.bytes()?;
        let theirs = std::fs::read(dir.join(format!("{prefix}_{name}.tif")))?;
        if ours == theirs {
            same += 1;
        } else {
            differ += 1;
            println!("differs: {prefix}_{name}");
        }
    }
    println!("research tiles: {same} identical, {differ} different");
    Ok(())
}
