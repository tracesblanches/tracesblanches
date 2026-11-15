//! Golden fingerprints of the zone layers (P9, P10) of both sites, as versioned in the
//! fixtures (without Biodiv'sports) (Q2, #102): zones and areas by kind, rules that apply
//! and the zones they apply to.
//!
//! References: `fixtures/golden/zones/<site>.json`; update with
//! `SKIMAP_BLESS=1 cargo test -p skimap-zones --features pipeline --test golden` and
//! review the diff. The areas are those of research report 05 (APPB, sites, PNR) and
//! report 06 (French part of the boxes), checked with their own tolerances in
//! `tests/real_sites.rs`.
//!
//! Tolerances: counts and identifiers exact; areas are read from the layer (geodesic
//! area computed by the builder, rounded to 1e-4 ha), summed per kind: `Tol::FLOAT`.

#[path = "../../../fixtures/support/mod.rs"]
mod fixtures;

use std::collections::BTreeMap;

use fixtures::golden::{Golden, Tol};
use skimap_common::site::SITES;
use skimap_zones::{RuleSet, ZoneLayer};

#[test]
fn zone_layers() {
    for site in SITES {
        let read =
            |n: &str| std::fs::read_to_string(fixtures::layer_file(site.id, "zones", n)).unwrap();
        let l = ZoneLayer::from_geojson_str(&read("zones.geojson")).unwrap();
        let rules = RuleSet::from_json_str(&read("rules.json")).unwrap();
        let mut by_kind: BTreeMap<String, (usize, f64)> = BTreeMap::new();
        for z in &l.zones {
            let e = by_kind.entry(format!("{:?}", z.kind())).or_default();
            e.0 += 1;
            e.1 += z.props.area_ha;
        }
        let mut k = Golden::new(&format!("zones/{}", site.id));
        k.count("zones", l.zones.len())
            .count("rules", rules.rules.len());
        for (kind, (n, ha)) in by_kind {
            k.count(&format!("kind.{kind}.zones"), n).num(
                &format!("kind.{kind}.area_ha"),
                ha,
                Tol::FLOAT,
            );
        }
        for r in &rules.rules {
            let mut ids: Vec<&str> = r.matching_zones(&l).map(|z| z.id()).collect();
            if ids.is_empty() {
                continue;
            }
            ids.sort_unstable();
            k.text(&format!("rule.{}", r.id), ids.join(" "));
        }
        k.check();
    }
}
