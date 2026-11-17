//! What the starts of `departs.json` serve (#166): for every census domain, the nordic km
//! of the part of the trail network each start is on ([`Reach`], the routing graph of the
//! domain ways), compared between two `departs.json` (before and after a change of the
//! rule). Prints the domains whose starts changed, then the counts: main start serving
//! less than N % of the nordic km of its domain, main start on a part shorter than a
//! default-outing loop ([`LOOP_KM`]) while the domain has a longer one, largest part
//! served by no start; and whether the SRN starts are the same.
//!
//! ```sh
//! SKIMAP_DATA_DIR=… cargo run --release -p skimap-stations --example departs_reach -- \
//!     <planet_pistes.osm.gz> <before/departs.json> <after/departs.json>
//! ```

use std::collections::HashMap;
use std::path::Path;

use skimap_common::layout::DataLayout;
use skimap_osm::extract::france_aoi;
use skimap_osm::parse::read_file;
use skimap_osm::pistes::{Piste, classify};
use skimap_stations::departs::{Depart, DepartsFile, LOOP_KM, Reach, Served, Way, serving};

/// Shares of the domain km below which a main start is counted.
const SHARES: [f64; 3] = [0.25, 0.5, 0.75];

/// One domain: identifier, nordic km, km of its largest part, what each start serves.
type Row = (String, f64, f64, Vec<Served>);

fn served(ways: &[Way], reach: &Reach, list: &[Depart]) -> Vec<Served> {
    list.iter()
        .filter_map(|d| serving(ways, reach, d.start.position()))
        .collect()
}

fn summary(label: &str, rows: &[Row]) {
    let mut below = [0usize; 3];
    let (mut short_main, mut main_unserved) = (Vec::new(), Vec::new());
    let mut quarter = Vec::new();
    for (id, domain_km, main_km, sv) in rows {
        let Some(first) = sv.first() else { continue };
        for (i, sh) in SHARES.iter().enumerate() {
            if first.km < sh * domain_km {
                below[i] += 1;
                if i == 0 {
                    quarter.push(id.as_str());
                }
            }
        }
        if first.km < LOOP_KM && *main_km >= LOOP_KM {
            short_main.push(id.as_str());
        }
        if !sv.iter().any(|x| x.main) {
            main_unserved.push(id.as_str());
        }
    }
    println!(
        "{label} : départ principal < 25 % des km nordiques du domaine : {} {quarter:?}, < 50 % : {}, < 75 % : {} ; départ principal sur une partie < {LOOP_KM} km alors que la plus grande fait plus : {} {short_main:?} ; plus grande partie sans départ : {} {main_unserved:?}",
        below[0],
        below[1],
        below[2],
        short_main.len(),
        main_unserved.len()
    );
}

fn describe(l: &[Depart], s: &[Served]) -> String {
    l.iter()
        .zip(s)
        .map(|(x, s)| {
            format!(
                "{} [{:?}, {:.1} km{}]",
                x.start.name,
                x.method,
                s.km,
                if s.main { ", plus grande partie" } else { "" }
            )
        })
        .collect::<Vec<_>>()
        .join(" | ")
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().collect();
    let usage = "usage: departs_reach <planet_pistes.osm.gz> <before.json> <after.json>";
    let planet = args.get(1).ok_or(usage)?;
    let before = DepartsFile::read(Path::new(args.get(2).ok_or(usage)?))?;
    let after = DepartsFile::read(Path::new(args.get(3).ok_or(usage)?))?;
    let layout =
        DataLayout::new(std::env::var("SKIMAP_DATA_DIR").unwrap_or_else(|_| "data".to_owned()));
    let census = layout.france_layer_dir(skimap_stations::CENSUS_LAYER)?;
    let (ds, domain_ways) = skimap_stations::domains::load(&census)?;
    let set = classify(&read_file(Path::new(planet), &france_aoi())?);
    let by_way: HashMap<i64, &Piste> = set.pistes.iter().map(|p| (p.way_id, p)).collect();

    let mut rows: [Vec<Row>; 2] = [Vec::new(), Vec::new()];
    let (mut changed, mut first_changed) = (0, 0);
    let empty = Vec::new();
    for d in &ds {
        let own: Vec<Piste> = domain_ways
            .get(&d.id)
            .into_iter()
            .flatten()
            .filter_map(|w| by_way.get(w).map(|p| (*p).clone()))
            .collect();
        let ways: Vec<Way> = own.iter().map(Way::of).collect();
        let reach = Reach::of(&set.subset(own));
        let main_km = reach.main().map_or(0.0, |m| reach.component_km[m]);
        let b = before.domains.get(&d.id).unwrap_or(&empty);
        let a = after.domains.get(&d.id).unwrap_or(&empty);
        let (sb, sa) = (served(&ways, &reach, b), served(&ways, &reach, a));
        rows[0].push((d.id.clone(), reach.total_km(), main_km, sb.clone()));
        rows[1].push((d.id.clone(), reach.total_km(), main_km, sa.clone()));
        let key = |l: &[Depart]| {
            l.iter()
                .map(|x| (x.start.name.clone(), x.start.lon, x.start.lat))
                .collect::<Vec<_>>()
        };
        if key(b) == key(a) {
            continue;
        }
        changed += 1;
        let pos = |l: &[Depart]| l.first().map(|x| (x.start.lon, x.start.lat));
        let same_first = pos(b) == pos(a);
        first_changed += usize::from(!same_first);
        println!(
            "{} ({}) : {:.1} km nordiques, plus grande partie {:.1} km{}\n  avant : {}\n  après : {}",
            d.id,
            d.name,
            reach.total_km(),
            main_km,
            if same_first {
                ""
            } else {
                " ; départ principal changé"
            },
            describe(b, &sb),
            describe(a, &sa)
        );
        for x in a {
            if let Some(r) = x.network.as_ref().and_then(|n| n.reason.as_deref()) {
                println!("    {} : {r}", x.start.id);
            }
        }
    }
    println!(
        "\n{} domaines, {changed} aux départs changés, dont {first_changed} au départ principal changé",
        ds.len()
    );
    summary("avant", &rows[0]);
    summary("après", &rows[1]);
    println!(
        "zones SRN : {} avant, {} après, départs identiques : {}",
        before.srn.len(),
        after.srn.len(),
        before.srn == after.srn
    );
    Ok(())
}
