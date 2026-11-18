//! Measures the tile-by-tile update of PMTiles archives (#160): for each pair `old new`,
//! the patch size against the file, the tiles changed by zoom, the time to rebuild the new
//! file from the old one and the proof that it is identical (SHA-256), as the phone does.
//!
//! ```sh
//! cargo run --release -p skimap-pack --example pmtiles_patch -- old1.pmtiles new1.pmtiles [old2 new2 ...]
//! ```

#![allow(
    clippy::disallowed_methods,
    reason = "measurement tool, writes nothing"
)]
use std::time::Instant;

use skimap_pack::patch;
use skimap_pack::update::sha256_hex;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    assert!(
        !args.is_empty() && args.len().is_multiple_of(2),
        "usage: pmtiles_patch <old> <new> [<old> <new> ...]"
    );
    for pair in args.chunks(2) {
        let old = std::fs::read(&pair[0]).expect("old archive");
        let new = std::fs::read(&pair[1]).expect("new archive");
        let t = Instant::now();
        let (p, d) = match patch::diff(&old, &new) {
            Ok(x) => x,
            Err(e) => {
                println!("{} -> {}: no patch ({e})", pair[0], pair[1]);
                continue;
            }
        };
        let diff_s = t.elapsed().as_secs_f64();
        // The phone: old file on disk, patch read from a stream, result hashed.
        let dir = std::env::temp_dir().join(format!("pmtiles_patch_{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("temp dir");
        let old_path = dir.join("old.pmtiles");
        std::fs::write(&old_path, &old).expect("old copy");
        let t = Instant::now();
        let mut f = std::fs::File::open(&old_path).expect("old copy");
        let mut out = Vec::with_capacity(new.len());
        patch::apply(
            &mut std::io::BufReader::new(p.as_slice()),
            &mut f,
            old.len() as u64,
            &mut out,
            new.len() as u64,
        )
        .expect("apply");
        let same = sha256_hex(&out) == sha256_hex(&new);
        let apply_s = t.elapsed().as_secs_f64();
        let _ = std::fs::remove_dir_all(&dir);
        let zooms: Vec<String> = d
            .by_zoom
            .iter()
            .filter(|(_, z)| z.added + z.changed + z.removed > 0)
            .map(|(k, z)| {
                format!(
                    "z{k}:+{}~{}-{}={}B",
                    z.added, z.changed, z.removed, z.new_bytes
                )
            })
            .collect();
        println!(
            "{} -> {}: file {} B, patch {} B ({:.1} %), tiles +{} ~{} -{} ={}, sent {} B, ops {}, diff {:.3} s, rebuild {:.3} s, identical {same} [{}]",
            pair[0],
            pair[1],
            new.len(),
            p.len(),
            100.0 * p.len() as f64 / new.len() as f64,
            d.tiles_added,
            d.tiles_changed,
            d.tiles_removed,
            d.tiles_kept,
            d.sent_bytes,
            d.ops,
            diff_s,
            apply_s,
            zooms.join(" ")
        );
        assert!(same, "rebuilt file differs");
    }
}
