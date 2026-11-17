//! Builds the packs of some sites, published then personal, and counts their entities by
//! source and licence group (#169, #171): every GeoJSON layer by the `provider` or
//! `source` of its features, every other file as one entity, the ign slope tiles, the
//! voice clips (with their names by kind) and the names of the site the voice may say in
//! a published pack (`skimap_voice::site_names`). Checks that a published pack has no
//! entity of a source without a reuse licence and that no file mixes ODbL and CC BY-SA
//! data.
//!
//! ```sh
//! SKIMAP_DATA_DIR=<working copy> cargo run --release -p skimap-pack --features pipeline \
//!     --example licence_census -- <site>...
//! ```
//!
//! Development build of each pack: no basemap, offline downloader; the ign slope tiles
//! with `SKIMAP_PACK_IGN_SLOPES=1` (from the download cache: only for sites whose tiles
//! were fetched). Writes the packs
//! into `<data>/out/<site>/pack/` of the working copy: never run it on `data/`.

use std::collections::BTreeMap;

use serde_json::Value;
use skimap_common::config::Config;
use skimap_common::download::Downloader;
use skimap_common::licence::{self, Group};
use skimap_common::site::Registry;
use skimap_pack::Pack;
use skimap_pack::build::layers::feature_group;
use skimap_pack::build::{BuildOptions, build, build_voice_pack};

/// Source of an entity of a layer: its `provider` (SRN) or `source` (zones), else the
/// layer.
fn source_of(layer: &str, props: &Value) -> String {
    props["provider"]
        .as_str()
        .or_else(|| props["source"].as_str())
        .map(str::to_owned)
        .unwrap_or_else(|| layer.to_owned())
}

/// Names of the OSM stations an SRN area cites (`stations[].name`).
fn station_names(props: &Value) -> usize {
    props["stations"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|s| s["name"].is_string())
        .count()
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let ids: Vec<String> = std::env::args().skip(1).collect();
    let cfg = Config::from_env()?;
    let layout = cfg.layout();
    let registry = Registry::load(&layout)?;
    let dl = Downloader::builder(layout.cache_root())
        .offline(true)
        .build()?;
    let mut failures = Vec::new();
    for id in &ids {
        let site = registry.get(id)?;
        for personal in [false, true] {
            let opts = BuildOptions {
                personal_sources: personal,
                plan_ign: false,
                osm: false,
                ign_slopes: std::env::var("SKIMAP_PACK_IGN_SLOPES").is_ok_and(|v| v == "1"),
                voice: true,
                offpiste: true,
                ..BuildOptions::default()
            };
            let r = build(site, &layout, &dl, &opts)?;
            let pack = Pack::open(&r.pack_dir)?;
            let m = pack.manifest();
            let mode = if personal { "personnel" } else { "public" };
            // Entities per source of the third-party layers.
            let mut counts: BTreeMap<String, usize> = BTreeMap::new();
            for l in &m.layers {
                // Sources of a published file: one licence group, nothing personal.
                if !personal {
                    let g = licence::group(l.sources.iter().map(licence::of_source));
                    if g == Group::Personal || l.personal_use_only {
                        failures.push(format!(
                            "{id} {mode}: {} sources {:?}",
                            l.id,
                            l.sources
                                .iter()
                                .map(|s| (s.name.as_str(), licence::of_source(s)))
                                .collect::<Vec<_>>()
                        ));
                    }
                }
                if l.format != "geojson" {
                    *counts.entry(format!("{} (fichier)", l.id)).or_default() += 1;
                    continue;
                }
                let fc: Value = serde_json::from_slice(&std::fs::read(r.pack_dir.join(&l.file))?)?;
                for f in fc["features"].as_array().into_iter().flatten() {
                    let p = &f["properties"];
                    *counts
                        .entry(format!("{} / {}", l.id, source_of(&l.id, p)))
                        .or_default() += 1;
                    let n = station_names(p);
                    if n > 0 {
                        *counts
                            .entry(format!("{} / stations[].name (OSM)", l.id))
                            .or_default() += n;
                    }
                    if n > 0 && l.id.ends_with(Group::ShareAlike.suffix()) {
                        failures.push(format!("{id} {mode}: OSM station names in {}", l.id));
                    }
                    if l.id.starts_with("srn-") {
                        let want = Group::ALL
                            .into_iter()
                            .find(|g| l.id.ends_with(g.suffix()) && !g.suffix().is_empty())
                            .unwrap_or(Group::Odbl);
                        let got = feature_group(p);
                        if got != want {
                            failures.push(format!("{id} {mode}: {} holds a {got:?} entity", l.id));
                        }
                        if !personal && got == Group::Personal {
                            failures.push(format!("{id} {mode}: personal entity in {}", l.id));
                        }
                    }
                }
            }
            // Voice: clips by kind (names by their first id segment), sources.
            if let Some(vm) = &m.voice.manifest {
                let v: Value = serde_json::from_slice(&std::fs::read(r.pack_dir.join(vm))?)?;
                for c in v["clips"].as_array().into_iter().flatten() {
                    if c["kind"] == "name" {
                        let id = c["id"].as_str().unwrap_or_default();
                        let k = id.split('.').next().unwrap_or_default();
                        *counts.entry(format!("voice / noms {k}")).or_default() += 1;
                    } else {
                        *counts
                            .entry("voice / clips fixes et valeurs".into())
                            .or_default() += 1;
                    }
                }
                for s in &m.voice.sources {
                    let f = licence::of_source(s);
                    if !personal && !f.public() {
                        failures.push(format!("{id} {mode}: voice source {} {f:?}", s.name));
                    }
                }
            }
            println!("== {id} ({mode}) : {} couches", m.layers.len());
            for (k, n) in &counts {
                println!("  {k:<60} {n:>5}");
            }
            for miss in &r.missing {
                println!("  absente {} : {}", miss.id, miss.reason);
            }
            // The French voice pack published apart (VO6): its clips by kind and sources.
            if layout
                .out_dir(site)
                .join("voice")
                .join("manifest.json")
                .is_file()
            {
                let vr = build_voice_pack(site, &layout, "voice", "fr", personal)?;
                let vp = Pack::open(&vr.pack_dir)?;
                let vm = vp.manifest();
                let mut names: BTreeMap<String, usize> = BTreeMap::new();
                if let Some(f) = &vm.voice.manifest {
                    let v: Value = serde_json::from_slice(&std::fs::read(vr.pack_dir.join(f))?)?;
                    for c in v["clips"].as_array().into_iter().flatten() {
                        let k = if c["kind"] == "name" {
                            c["id"]
                                .as_str()
                                .unwrap_or_default()
                                .split('.')
                                .next()
                                .unwrap_or_default()
                                .to_owned()
                        } else {
                            "fixes et valeurs".to_owned()
                        };
                        *names.entry(k).or_default() += 1;
                    }
                }
                println!(
                    "  pack de voix fr : {} clips {:?} ; sources {:?}",
                    vm.voice.clips,
                    names,
                    vm.voice
                        .sources
                        .iter()
                        .map(|s| s.name.as_str())
                        .collect::<Vec<_>>()
                );
                for s in &vm.voice.sources {
                    if !personal && !licence::of_source(s).public() {
                        failures.push(format!("{id} {mode}: voice pack source {}", s.name));
                    }
                }
            }
            if !personal {
                // Names a published voice may say, by kind, from the layers as built.
                match skimap_voice::site_names(&layout, site) {
                    Ok(n) => {
                        println!(
                            "  noms de la voix (public) : {} pistes, {} lieux, {} dates ; sources {:?}",
                            n.public.pistes.len(),
                            n.public.places.len(),
                            n.public.dates.len(),
                            n.public_sources
                                .iter()
                                .map(|s| s.name.as_str())
                                .collect::<Vec<_>>()
                        );
                        println!(
                            "  noms de la voix (personnel) : {} pistes, {} lieux, {} dates",
                            n.personal.pistes.len(),
                            n.personal.places.len(),
                            n.personal.dates.len()
                        );
                        for s in &n.public_sources {
                            if !licence::of_source(s).public() {
                                failures.push(format!(
                                    "{id}: public voice name source {} not public",
                                    s.name
                                ));
                            }
                        }
                    }
                    Err(e) => println!("  noms de la voix : {e}"),
                }
            }
        }
    }
    if failures.is_empty() {
        println!(
            "OK : aucune entité personnelle dans un pack public, aucun fichier ne mêle ODbL et CC BY-SA"
        );
        Ok(())
    } else {
        for f in &failures {
            println!("ÉCHEC {f}");
        }
        Err(format!("{} problèmes", failures.len()).into())
    }
}
