//! VO3 run on the two sites: names from the research dumps, voice layer, 10 samples.
//!
//! ```sh
//! SKIMAP_DATA_DIR=~/projects/skimap/data cargo run --release -p skimap-voice \
//!     --example voice_pack -- ~/projects/skimap/research/raw [site...]
//! ```
//!
//! Names (until the osm / refuges / zones crates exist): nordic pistes of the Overpass
//! dumps (`osm/`), ski treks of the ENJ API (`enj/treks_cdb.json`, Chapelle-des-Bois),
//! huts and shelters (`dl/osm_*.json`, `dl/ri_*.json`), protected zones (`dl/*_patrinat_*`,
//! Biodiv'sports `srn/bsp_*`), whose seasonal end dates give the date clips.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use serde_json::Value;
use skimap_common::config::Config;
use skimap_common::geom::BBox;
use skimap_common::site::{self, Site};
use skimap_voice::bank::{
    Announcement, Briefing, Change, Event, Glide, Item, Kick, Name, Side, ZoneRule,
};
use skimap_voice::clips::{DayMonth, Direction};
use skimap_voice::names::{Colour, PisteName};
use skimap_voice::pack::{Source, VoiceManifest};
use skimap_voice::quantity::Clock;
use skimap_voice::{PackNames, VoiceConfig, assemble, build};

type Res<T> = Result<T, Box<dyn std::error::Error>>;

fn read(raw: &Path, rel: &str) -> Res<Value> {
    let p = raw.join(rel);
    let b = std::fs::read(&p).map_err(|e| format!("{}: {e}", p.display()))?;
    Ok(serde_json::from_slice(&b)?)
}

fn in_bbox(bb: &BBox, e: &Value) -> bool {
    if let Some(b) = e.get("bounds") {
        let g = |k: &str| b[k].as_f64().unwrap_or(f64::NAN);
        return g("minlat") <= bb.north
            && g("maxlat") >= bb.south
            && g("minlon") <= bb.east
            && g("maxlon") >= bb.west;
    }
    match (e["lat"].as_f64(), e["lon"].as_f64()) {
        (Some(lat), Some(lon)) => {
            lat >= bb.south && lat <= bb.north && lon >= bb.west && lon <= bb.east
        }
        // Ways of the hut dumps carry no geometry: the query was already the site box.
        _ => true,
    }
}

fn osm_pistes(raw: &Path, files: &[&str], bb: &BBox, out: &mut BTreeSet<PisteName>) -> Res<()> {
    for f in files {
        for e in read(raw, f)?["elements"].as_array().into_iter().flatten() {
            let t = &e["tags"];
            let nordic = t["piste:type"]
                .as_str()
                .is_some_and(|p| p.split(';').any(|x| x == "nordic"));
            let name = t["name"].as_str().or(t["piste:name"].as_str());
            if let (true, Some(n)) = (nordic && in_bbox(bb, e), name) {
                let c = Colour::from_osm(t["colour"].as_str(), t["piste:difficulty"].as_str());
                out.insert(PisteName::new(n.trim(), c));
            }
        }
    }
    Ok(())
}

fn places(raw: &Path, osm: &str, ri: &str, bb: &BBox, out: &mut BTreeSet<String>) -> Res<()> {
    for e in read(raw, osm)?["elements"].as_array().into_iter().flatten() {
        let t = &e["tags"];
        let hut = matches!(t["tourism"].as_str(), Some("alpine_hut" | "wilderness_hut"))
            || t["amenity"].as_str() == Some("shelter");
        if let (true, Some(n)) = (hut && in_bbox(bb, e), t["name"].as_str()) {
            out.insert(n.trim().to_owned());
        }
    }
    for f in read(raw, ri)?["features"].as_array().into_iter().flatten() {
        let p = &f["properties"];
        let kind = p["type"]["valeur"].as_str().unwrap_or("");
        if matches!(kind, "cabane non gardée" | "refuge gardé" | "gîte d'étape")
            && let Some(n) = p["nom"].as_str()
        {
            out.insert(n.trim().to_owned());
        }
    }
    Ok(())
}

/// Last day of the months that end a protection period ([bool; 12], wrapping).
fn period_ends(period: &Value) -> Vec<DayMonth> {
    let p: Vec<bool> = period
        .as_array()
        .map(|a| a.iter().map(|v| v.as_bool().unwrap_or(false)).collect())
        .unwrap_or_default();
    if p.len() != 12 || p.iter().all(|&x| x) {
        return Vec::new();
    }
    const LAST: [u8; 12] = [31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];
    (0..12)
        .filter(|&m| p[m] && !p[(m + 1) % 12])
        .map(|m| DayMonth {
            day: LAST[m],
            month: m as u8 + 1,
        })
        .collect()
}

fn zones(
    raw: &Path,
    prefix: &str,
    bsp: &[&str],
    names: &mut BTreeSet<String>,
    dates: &mut BTreeSet<DayMonth>,
) -> Res<()> {
    let dl = raw.join("dl");
    for entry in std::fs::read_dir(&dl)? {
        let f = entry?.file_name().to_string_lossy().into_owned();
        if f.starts_with(&format!("{prefix}_patrinat_")) && f.ends_with(".json") {
            for z in read(raw, &format!("dl/{f}"))?["features"]
                .as_array()
                .into_iter()
                .flatten()
            {
                if let Some(n) = z["properties"]["nom_site"].as_str() {
                    names.insert(n.trim().to_owned());
                }
            }
        }
    }
    for f in bsp {
        for z in read(raw, f)?["features"].as_array().into_iter().flatten() {
            let p = &z["properties"];
            if let Some(n) = p["name"].as_str() {
                names.insert(n.trim().to_owned());
            }
            dates.extend(period_ends(&p["period"]));
        }
    }
    Ok(())
}

fn pack_names(raw: &Path, s: &Site) -> Res<(PackNames, Vec<Source>)> {
    let mut pistes = BTreeSet::new();
    let mut place = BTreeSet::new();
    let mut dates = BTreeSet::new();
    let (osm, huts, ri, prefix, bsp): (&[&str], _, _, _, &[&str]) = match s.id {
        "cervieres" => (
            &["osm/cervieres.json", "osm/cerv2.json"],
            "dl/osm_cerv.json",
            "dl/ri_cerv.json",
            "cerv",
            &["srn/bsp_cerv_1.json", "srn/bsp_cerv_2,3.json"],
        ),
        _ => (
            &["osm/chapelle.json", "osm/cdb.json"],
            "dl/osm_cdb.json",
            "dl/ri_cdb.json",
            "cdb",
            &["srn/bsp_chap_1.json", "srn/bsp_chap_2,3.json"],
        ),
    };
    osm_pistes(raw, osm, &s.bbox, &mut pistes)?;
    let mut sources = vec![
        Source {
            name: "Noms des pistes nordiques (Overpass, piste:type=nordic)".into(),
            url: "https://overpass-api.de/api/interpreter".into(),
            licence: "ODbL".into(),
            attribution: "© les contributeurs d'OpenStreetMap".into(),
        },
        Source {
            name: "Noms des refuges et abris".into(),
            url: "https://www.refuges.info/api".into(),
            licence: "CC-BY-SA-2.0 (refuges.info), ODbL (OSM)".into(),
            attribution: "refuges.info, © les contributeurs d'OpenStreetMap".into(),
        },
        Source {
            name: "Noms et périodes des zones protégées (INPN, Biodiv'sports)".into(),
            url: "https://biodiv-sports.fr/api/v2/sensitivearea/ ; https://www.data.gouv.fr/datasets/inpn-donnees-du-programme-espaces-proteges".into(),
            licence: "à confirmer (L6, #11)".into(),
            attribution: "INPN (PatriNat), Biodiv'sports".into(),
        },
    ];
    if s.id == "chapelle-des-bois" {
        for t in read(raw, "enj/treks_cdb.json")?["results"]
            .as_array()
            .into_iter()
            .flatten()
        {
            if t["practice"].as_u64() == Some(2)
                && let Some(n) = t["name"].as_str()
            {
                let c = match t["difficulty"].as_u64() {
                    Some(1) => Some(Colour::Green),
                    Some(2) => Some(Colour::Blue),
                    Some(3) => Some(Colour::Red),
                    Some(4) => Some(Colour::Black),
                    _ => None,
                };
                pistes.insert(PisteName::new(n.trim(), c));
            }
        }
        sources.push(Source {
            name: "Itinéraires de ski de fond de l'Espace Nordique Jurassien (API Snowmap)".into(),
            url: "https://admin.snowmap.espacenordiquejurassien.com/api/v2/trek/".into(),
            licence: "à confirmer (L1, #6)".into(),
            attribution: "Espace Nordique Jurassien".into(),
        });
    }
    places(raw, huts, ri, &s.bbox, &mut place)?;
    zones(raw, prefix, bsp, &mut place, &mut dates)?;
    let status_pistes = pistes.len() as u32;
    Ok((
        PackNames {
            site: s.name.to_owned(),
            pistes: pistes.into_iter().collect(),
            places: place.into_iter().collect(),
            dates: dates.into_iter().collect(),
            status_pistes,
        },
        sources,
    ))
}

fn piste(n: &PackNames, name: &str, colour: Colour) -> Res<PisteName> {
    n.pistes
        .iter()
        .find(|p| p.name == name && p.colour == Some(colour))
        .cloned()
        .ok_or_else(|| format!("piste {name} ({colour:?}) not in the pack").into())
}

fn place(n: &PackNames, name: &str) -> Res<String> {
    n.places
        .iter()
        .find(|p| p.as_str() == name)
        .cloned()
        .ok_or_else(|| format!("place {name} not in the pack").into())
}

fn join(events: Vec<Event>) -> Announcement {
    events
        .iter()
        .map(Event::announce)
        .reduce(Announcement::then)
        .unwrap_or_default()
}

/// Ten announcements per site. Names are those of the pack; distances, times and
/// weather are illustrative values.
fn samples(s: &Site, n: &PackNames) -> Res<Vec<(&'static str, Announcement)>> {
    let c = |hour, minute| Clock { hour, minute };
    Ok(if s.id == "cervieres" {
        vec![
            (
                "01-direction",
                Event::Turn {
                    metres: 210.0,
                    direction: Direction::Left,
                    piste: Some(piste(n, "Le Cancel", Colour::Red)?),
                }
                .announce(),
            ),
            (
                "02-restant",
                Event::Remaining {
                    metres: 4180.0,
                    seconds: 55.0 * 60.0,
                }
                .announce(),
            ),
            (
                "03-demi-tour",
                Event::Turnaround {
                    seconds_left: 37.0 * 60.0,
                    deadline: c(15, 45),
                }
                .announce(),
            ),
            ("04-pente", Event::Slope { degrees: 34.4 }.announce()),
            (
                "05-zone-protegee",
                Event::ProtectedZone {
                    entered: false,
                    zone: Some(place(n, "Tétras lyre - hiver")?),
                    rules: vec![ZoneRule::Quiet, ZoneRule::StayOnRoutes],
                    until: Some(DayMonth { day: 30, month: 4 }),
                }
                .announce(),
            ),
            (
                "06-sans-reseau",
                Event::NoNetworkAhead {
                    metres: 480.0,
                    length_m: Some(2300.0),
                }
                .announce(),
            ),
            (
                "07-hors-itineraire",
                Event::OffRoute {
                    side: Side::Right,
                    metres: 120.0,
                }
                .announce(),
            ),
            (
                "08-rassurance",
                Event::Reassurance {
                    piste: Some(piste(n, "Le Gibera", Colour::Green)?),
                }
                .announce(),
            ),
            (
                "09-avalanche",
                Event::AvalancheSegment {
                    bera_level: Some(3),
                    piste: Some(piste(n, "Le Chemin du Sioux", Colour::Red)?),
                }
                .announce(),
            ),
            (
                "10-briefing",
                join(vec![
                    Event::Briefing(Briefing::Intro {
                        site: n.site.clone(),
                    }),
                    Event::Briefing(Briefing::Avalanche { level: Some(2) }),
                    Event::Briefing(Briefing::Weather {
                        temperature_c: -8.0,
                        wind_kmh: 35.0,
                        fog: false,
                        snowfall: false,
                    }),
                    Event::Briefing(Briefing::Recommended {
                        name: Name::Place(place(n, "Refuge des Fonts de Cervières")?),
                    }),
                    Event::Briefing(Briefing::Sunset { at: c(17, 15) }),
                    Event::TurnaroundDeadline {
                        deadline: c(15, 45),
                    },
                    Event::Equipment {
                        items: vec![Item::AvalancheKit, Item::Headlamp],
                    },
                    Event::Wax {
                        kick: Some(Kick::Violet),
                        glide: Some(Glide::Cold),
                    },
                ]),
            ),
        ]
    } else {
        vec![
            (
                "01-direction",
                Event::Turn {
                    metres: 290.0,
                    direction: Direction::Right,
                    piste: Some(piste(n, "Greffier", Colour::Black)?),
                }
                .announce(),
            ),
            (
                "02-bifurcation",
                Event::Fork {
                    metres: Some(100.0),
                    colour: Colour::Blue,
                }
                .announce(),
            ),
            (
                "03-restant",
                Event::Remaining {
                    metres: 9200.0,
                    seconds: 105.0 * 60.0,
                }
                .announce(),
            ),
            (
                "04-zone-protegee",
                Event::ProtectedZone {
                    entered: true,
                    zone: Some(place(n, "Grand tétras - hiver")?),
                    rules: vec![ZoneRule::Quiet, ZoneRule::StayOnRoutes],
                    until: Some(DayMonth { day: 30, month: 6 }),
                }
                .announce(),
            ),
            ("05-sans-reseau", Event::NoNetworkEntered.announce()),
            (
                "06-hors-itineraire",
                Event::OffRoute {
                    side: Side::Left,
                    metres: 60.0,
                }
                .announce(),
            ),
            (
                "07-demi-tour",
                Event::Turnaround {
                    seconds_left: 120.0,
                    deadline: c(16, 0),
                }
                .announce(),
            ),
            (
                "08-batterie",
                Event::Battery {
                    percent: 43.0,
                    autonomy_s: 3.0 * 3600.0,
                }
                .announce(),
            ),
            (
                "09-brouillard",
                join(vec![
                    Event::ConditionChange(Change::Fog),
                    Event::Reassurance {
                        piste: Some(piste(n, "La Thieulette", Colour::Blue)?),
                    },
                ]),
            ),
            (
                "10-briefing",
                join(vec![
                    Event::Briefing(Briefing::Intro {
                        site: n.site.clone(),
                    }),
                    Event::Briefing(Briefing::Avalanche { level: None }),
                    Event::Briefing(Briefing::PistesOpen {
                        open: 21,
                        total: n.status_pistes,
                    }),
                    Event::Briefing(Briefing::Weather {
                        temperature_c: -12.0,
                        wind_kmh: 20.0,
                        fog: true,
                        snowfall: false,
                    }),
                    Event::Briefing(Briefing::Recommended {
                        name: Name::Piste(piste(n, "La Combette", Colour::Blue)?),
                    }),
                    Event::Briefing(Briefing::Sunset { at: c(17, 0) }),
                    Event::Equipment {
                        items: vec![Item::WarmLayer, Item::Compass, Item::PowerBank],
                    },
                    Event::Wax {
                        kick: Some(Kick::Green),
                        glide: Some(Glide::Cold),
                    },
                ]),
            ),
        ]
    })
}

fn main() -> Res<()> {
    let mut args = std::env::args().skip(1);
    let raw = PathBuf::from(
        args.next()
            .ok_or("usage: voice_pack <research/raw> [site...]")?,
    );
    let ids: Vec<String> = args.collect();
    let layout = Config::from_env()?.layout();
    let cfg = VoiceConfig::from_env()?;
    let sites: Vec<&Site> = if ids.is_empty() {
        site::SITES.iter().collect()
    } else {
        ids.iter()
            .map(|i| site::site(i))
            .collect::<Result<_, _>>()?
    };
    for s in sites {
        let (names, sources) = pack_names(&raw, s)?;
        println!(
            "== {}: {} pistes, {} places, {} dates",
            s.id,
            names.pistes.len(),
            names.places.len(),
            names.dates.len()
        );
        let b = build(s, &layout, &names, &sources, &cfg)?;
        println!("{}", serde_json::to_string_pretty(&b)?);
        let vm = VoiceManifest::read(&b.dir)?;
        let out = b.dir.join("samples");
        let mut index = Vec::new();
        for (stem, a) in samples(s, &names)? {
            let r = assemble(&b.dir, &vm, &a, &cfg.ffmpeg, &out.join(stem))?;
            println!(
                "{:>5.2} s  {}  {}",
                r.duration_s,
                r.files[1].display(),
                r.text
            );
            index.push(r);
        }
        std::fs::write(out.join("samples.json"), serde_json::to_vec_pretty(&index)?)?;
    }
    Ok(())
}
