//! Counts the points `fog_targets` offers in packs, by kind (bearing in the fog, #162):
//! every pack must offer its refuges, whatever refuge layers it holds.
//!
//! ```sh
//! cargo run --release -p skimap-ffi --example fog_targets_count -- <pack dir>...
//! ```

use std::collections::BTreeMap;

use skimap_ffi::card::Lang;
use skimap_ffi::fog::fog_targets;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let dirs: Vec<String> = std::env::args().skip(1).collect();
    if dirs.is_empty() {
        return Err("usage: fog_targets_count <pack dir>...".into());
    }
    for dir in dirs {
        let targets = fog_targets(dir.clone(), None, None, Lang::Fr, 0)?;
        let mut by_kind: BTreeMap<String, usize> = BTreeMap::new();
        for t in &targets {
            *by_kind.entry(t.kind.clone()).or_default() += 1;
        }
        let refuges = by_kind.get("refuge").copied().unwrap_or(0);
        println!("{refuges:>4} refuges  {by_kind:?}  {dir}");
    }
    Ok(())
}
