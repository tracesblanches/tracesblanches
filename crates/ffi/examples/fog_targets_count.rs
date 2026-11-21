//! Counts the points `fog_targets` offers in packs, by kind (bearing in the fog, #162):
//! every pack must offer its refuges, whatever refuge layers it holds. Also counts the
//! targets whose label (name and kind) another target of the pack shares, which the list
//! « Aller vers » cannot tell apart (#181), and prints those labels; every label with
//! `FOG_LABELS=1`.
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
    let (mut total, mut same_total) = (0usize, 0usize);
    for dir in dirs {
        for lang in [Lang::Fr, Lang::En] {
            let targets = fog_targets(dir.clone(), None, None, lang, 0)?;
            let mut by_kind: BTreeMap<String, usize> = BTreeMap::new();
            let mut labels: BTreeMap<(String, String), usize> = BTreeMap::new();
            for t in &targets {
                *by_kind.entry(t.kind.clone()).or_default() += 1;
                *labels.entry((t.kind.clone(), t.name.clone())).or_default() += 1;
            }
            let same: Vec<String> = labels
                .iter()
                .filter(|(_, n)| **n > 1)
                .map(|((k, l), n)| format!("{n}× {k}:{l}"))
                .collect();
            let same_n: usize = labels.values().filter(|n| **n > 1).sum();
            total += targets.len();
            same_total += same_n;
            if lang == Lang::Fr {
                let refuges = by_kind.get("refuge").copied().unwrap_or(0);
                println!("{refuges:>4} refuges  {by_kind:?}  {dir}");
            }
            if std::env::var_os("FOG_LABELS").is_some() {
                for t in &targets {
                    println!("     {lang:?} {}: {}", t.kind, t.name);
                }
            }
            if !same.is_empty() {
                println!(
                    "     {lang:?}: {same_n} indiscernables : {}",
                    same.join(", ")
                );
            }
        }
    }
    println!("total {total} cibles (fr + en), {same_total} indiscernables");
    Ok(())
}
