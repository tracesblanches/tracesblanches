//! Exports the pack of one or both sites for Locus Map and OsmAnd into
//! `data/out/<site>/export/` (P21).
//!
//! ```sh
//! cargo run --release -p skimap-pack --features pipeline --example export_pack -- [site...]
//! ```

use std::time::Instant;

use skimap_common::config::Config;
use skimap_common::site::{self, SITES};
use skimap_pack::Pack;
use skimap_pack::build::pack_dir;
use skimap_pack::export::{EXPORT_LAYER, export};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let ids: Vec<String> = std::env::args().skip(1).collect();
    let sites: Vec<_> = if ids.is_empty() {
        SITES.iter().collect()
    } else {
        ids.iter()
            .map(|i| site::site(i))
            .collect::<Result<_, _>>()?
    };
    let layout = Config::from_env()?.layout();
    for s in sites {
        let t = Instant::now();
        let pack = Pack::open(pack_dir(s, &layout)?)?;
        let r = export(&pack, &layout.layer_dir(s, EXPORT_LAYER)?)?;
        println!(
            "== {} : {} ({:.1} s), total {:.2} MB",
            s.name,
            r.dir.display(),
            t.elapsed().as_secs_f64(),
            r.total_bytes as f64 / 1e6
        );
        for f in &r.files {
            println!(
                "  {:<36} {:>9.3} MB {:>7}",
                f.path,
                f.bytes as f64 / 1e6,
                f.count
            );
        }
        for (id, why) in &r.skipped {
            println!("  skipped {id}: {why}");
        }
    }
    Ok(())
}
