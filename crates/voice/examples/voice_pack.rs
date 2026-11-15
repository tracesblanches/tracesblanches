//! VO3 run on the two sites: names of the built layers with their provenance
//! (`skimap_voice::sources`), voice layer, samples.
//!
//! ```sh
//! SKIMAP_DATA_DIR=~/projects/skimap/data cargo run --release -p skimap-voice \
//!     --example voice_pack -- [--names] [site...]
//! ```
//!
//! Names: nordic pistes (`pistes`), huts and shelters (`refuges`), protected zones and the
//! end dates of their periods (`zones`), names read on the piste plan (`plan_pistes`);
//! those read only from personal-use sources (Biodiv'sports, piste plans) are listed in
//! `personal.json` and left out of a published pack. `--names`: print the names, no synthesis.
//! The samples only use public names (they are what a published pack says).

use skimap_common::config::Config;
use skimap_common::site::{self, Site};
use skimap_voice::bank::{
    Announcement, Briefing, Change, Event, Glide, Item, Kick, Name, Pause, Segment, Side, ZoneRule,
};
use skimap_voice::clips::{self, Direction, Fixed};
use skimap_voice::names::{Colour, PisteName};
use skimap_voice::pack::{PersonalClips, VoiceManifest};
use skimap_voice::quantity::Clock;
use skimap_voice::{PackNames, VoiceConfig, assemble, build_site, site_names};

type Res<T> = Result<T, Box<dyn std::error::Error>>;

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
                    zone: Some(place(n, "Rochebrune - Izoard - Vallée de la Cerveyrette")?),
                    rules: vec![ZoneRule::StayOnRoutes],
                    until: None,
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
                    zone: Some(place(n, "Massif du Risoux")?),
                    rules: vec![ZoneRule::StayOnRoutes],
                    until: None,
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

/// A proposal of C11 as the tracker says it (`skimap_plan::pace::proposal_announcement`):
/// the reason, « Je vous propose …, », « retour vers … ».
fn proposal(reason: Fixed, kind: Fixed, back: Clock) -> Announcement {
    Announcement {
        segments: vec![
            Segment {
                clip: reason.clip(),
                pause: Pause::Long,
            },
            Segment {
                clip: kind.clip(),
                pause: Pause::Short,
            },
            Segment {
                clip: clips::loop_back(back),
                pause: Pause::Long,
            },
        ],
    }
}

/// Proposals (C11) and the turnaround moved earlier, on both sites.
fn common_samples() -> Vec<(&'static str, Announcement)> {
    let c = |hour, minute| Clock { hour, minute };
    vec![
        (
            "11-proposition-courte",
            proposal(
                Fixed::PlanPaceSlowerPauses,
                Fixed::PlanProposeShort,
                c(15, 15),
            ),
        ),
        (
            "12-proposition-sortie",
            proposal(Fixed::PlanPaceSlower, Fixed::PlanProposeExit, c(14, 30)),
        ),
        (
            "13-proposition-garder",
            proposal(Fixed::PlanPacePauses, Fixed::PlanProposeKeep, c(16, 45)),
        ),
        (
            "14-proposition-acceptee",
            Announcement {
                segments: vec![Segment {
                    clip: Fixed::PlanProposalAccepted.clip(),
                    pause: Pause::Long,
                }],
            }
            .then(
                Event::Turn {
                    metres: 300.0,
                    direction: Direction::Straight,
                    piste: None,
                }
                .announce(),
            ),
        ),
        (
            "15-demi-tour-avance",
            Event::TurnaroundAdvanced {
                deadline: c(15, 15),
            }
            .announce(),
        ),
    ]
}

fn main() -> Res<()> {
    let mut ids: Vec<String> = std::env::args().skip(1).collect();
    let names_only = ids.iter().any(|a| a == "--names");
    ids.retain(|a| a != "--names");
    let layout = Config::from_env()?.layout();
    let sites: Vec<&Site> = if ids.is_empty() {
        site::SITES.iter().collect()
    } else {
        ids.iter()
            .map(|i| site::site(i))
            .collect::<Result<_, _>>()?
    };
    for s in sites {
        let names = site_names(&layout, s)?;
        let (p, q) = (&names.public, &names.personal);
        println!(
            "== {}: public {} pistes, {} places, {} dates; personal {} pistes, {} places, {} dates",
            s.id,
            p.pistes.len(),
            p.places.len(),
            p.dates.len(),
            q.pistes.len(),
            q.places.len(),
            q.dates.len()
        );
        if names_only {
            println!("{}", serde_json::to_string_pretty(&(p, q))?);
            for src in names.public_sources.iter().chain(&names.personal_sources) {
                println!(
                    "   source {} (personal {})",
                    src.name, src.personal_use_only
                );
            }
            continue;
        }
        let cfg = VoiceConfig::from_env()?;
        let b = build_site(s, &layout, &names, &cfg)?;
        println!("{}", serde_json::to_string_pretty(&b)?);
        let vm = VoiceManifest::read(&b.dir)?;
        let personal = PersonalClips::read(&b.dir)?.unwrap_or_default();
        let out = b.dir.join("samples");
        if out.exists() {
            std::fs::remove_dir_all(&out)?;
        }
        let mut index = Vec::new();
        let mut all = samples(s, &names.public)?;
        all.extend(common_samples());
        for (stem, a) in all {
            for id in a.clip_ids() {
                vm.get(id)
                    .ok_or_else(|| format!("{stem}: {id} not in the pack"))?;
                if personal.contains(id) {
                    return Err(format!("{stem}: {id} is a personal clip").into());
                }
            }
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
