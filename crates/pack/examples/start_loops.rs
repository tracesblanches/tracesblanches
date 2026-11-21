//! Loops and out-and-backs proposed from every start of every pack (#150): the ranking
//! of the starts that the pack build writes (`skimap_pack::build::rank`, #166), run again
//! on the packs of the data directory, read-only (the view of each pack is made in a
//! temporary directory). A pack built before its site record was carried (#166) takes the
//! record of the registry of the data directory (`skimap departs` for its starts). For
//! each pack: the start ranked first and what the morning
//! proposes from it, then every start; at the end, the share of out-and-backs among the
//! loops proposed, from the first start and from all the starts.
//!
//! ```sh
//! SKIMAP_DATA_DIR=<data directory> \
//!   cargo run --release -p skimap-pack --features pipeline --example start_loops -- \
//!   [--packs <directory of <site>/pack>] [site …]
//! ```

use std::path::PathBuf;

use skimap_common::layout::DataLayout;
use skimap_common::site::Registry;
use skimap_pack::Pack;
use skimap_pack::build::rank;
use skimap_pack::manifest::StartScore;

fn data() -> PathBuf {
    let root = std::env::var("SKIMAP_DATA_DIR")
        .unwrap_or_else(|_| format!("{}/../../data", env!("CARGO_MANIFEST_DIR")));
    PathBuf::from(root)
}

/// Out-and-back share of each loop proposed from a start.
fn shares(s: &StartScore) -> Vec<f64> {
    s.lengths_km
        .iter()
        .zip(&s.network_km)
        .map(|(l, n)| if *l > 0.0 { 1.0 - n / l } else { 0.0 })
        .collect()
}

/// Counts: (loops, out-and-backs).
fn count(s: &StartScore) -> (usize, usize) {
    (s.loops, s.loops - s.real_loops)
}

fn main() {
    let mut args: Vec<String> = std::env::args().skip(1).collect();
    let packs_dir = match args.iter().position(|a| a == "--packs") {
        Some(i) => {
            let d = PathBuf::from(args.remove(i + 1));
            args.remove(i);
            d
        }
        None => data().join("out"),
    };
    let only = args;
    let registry = Registry::load(&DataLayout::new(data())).expect("registry");
    let today = jiff::ZonedDateTime::now().date();
    let mut dirs: Vec<PathBuf> = std::fs::read_dir(&packs_dir)
        .expect("out/ of the data directory")
        .flatten()
        .map(|e| e.path())
        .filter(|p| {
            only.is_empty()
                || p.file_name()
                    .is_some_and(|n| only.iter().any(|o| o.as_str() == n))
        })
        .collect();
    dirs.sort();
    // (loops, out-and-backs, packs) from the first start, from all the starts; packs
    // whose first start gives only out-and-backs while another gives a real loop.
    let (mut first, mut all) = ((0, 0), (0, 0));
    let (mut packs, mut with_loops, mut oab_first_real_elsewhere) = (0, 0, Vec::new());
    for dir in dirs {
        let pack_dir = dir.join("pack");
        let Ok(pack) = Pack::open(&pack_dir) else {
            continue;
        };
        let info = &pack.manifest().site;
        let Some(mut record) = info
            .record
            .clone()
            .or_else(|| registry.record(&info.id).cloned())
        else {
            continue;
        };
        let Ok(site) = skimap_common::site::interned(&record) else {
            continue;
        };
        packs += 1;
        let work = tempfile::tempdir().expect("temporary directory");
        let Some(r) = rank::rank(
            site,
            &mut record,
            &pack_dir,
            &work.path().join("view"),
            today,
        ) else {
            println!("{}: no start", site.id);
            continue;
        };
        if let Some(n) = &r.note {
            println!("{}: {n}", site.id);
            continue;
        }
        println!(
            "{} ({} starts, {} ms, {}):",
            site.id,
            r.starts.len(),
            r.compute_ms,
            r.date
        );
        for (i, s) in r.starts.iter().enumerate() {
            let (n, oab) = count(s);
            if i == 0 {
                first.0 += n;
                first.1 += oab;
                if n > 0 {
                    with_loops += 1;
                }
                if n > 0 && oab == n && r.starts.iter().any(|x| x.real_loops > 0) {
                    oab_first_real_elsewhere.push(site.id);
                }
            }
            all.0 += n;
            all.1 += oab;
            let sh: Vec<String> = shares(s)
                .iter()
                .zip(&s.lengths_km)
                .map(|(x, l)| format!("{l:.1} km {:.0} %", x * 100.0))
                .collect();
            println!(
                "  {}{} [registre {}] {} boucles, {} vraies, score {:?} : {}{}",
                if i == 0 { "* " } else { "  " },
                s.id,
                s.registry_rank,
                n,
                s.real_loops,
                s.best_score.map(|x| (x * 100.0).round() / 100.0),
                sh.join(", "),
                s.note
                    .as_ref()
                    .map(|n| format!(" ({n})"))
                    .unwrap_or_default()
            );
        }
    }
    let pct = |(n, o): (usize, usize)| {
        if n > 0 {
            100.0 * o as f64 / n as f64
        } else {
            0.0
        }
    };
    println!(
        "{packs} packs, {with_loops} with loops from the first start; out-and-backs: \
         first start {}/{} ({:.1} %), all starts {}/{} ({:.1} %)",
        first.1,
        first.0,
        pct(first),
        all.1,
        all.0,
        pct(all)
    );
    println!(
        "first start out-and-backs only while another start has a real loop: {} {:?}",
        oab_first_real_elsewhere.len(),
        oab_first_real_elsewhere
    );
}
