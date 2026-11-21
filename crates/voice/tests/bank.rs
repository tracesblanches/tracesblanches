//! One announcement per core event, and coverage of the vocabulary.

use skimap_voice::bank::{
    Briefing, Change, Crowd, Event, Glide, Item, Kick, Name, Side, Snow, ZoneRule,
};
use skimap_voice::clips::{DayMonth, Direction, Fixed};
use skimap_voice::names::{Colour, PisteName};
use skimap_voice::quantity::Clock;
use skimap_voice::vocabulary::{ClipKind, SiteNames, vocabulary};

fn greffier() -> PisteName {
    PisteName::new("Greffier", Some(Colour::Black))
}

fn names() -> SiteNames {
    SiteNames {
        site: "Chapelle-des-Bois".into(),
        pistes: vec![
            greffier(),
            PisteName::new("La Combette", Some(Colour::Blue)),
        ],
        places: vec![
            "Refuge du Pré à la Dame".into(),
            "Réserve naturelle du lac des Rousses".into(),
        ],
        dates: vec![DayMonth { day: 30, month: 6 }],
        status_pistes: 30,
    }
}

/// Every event of the core with its exact text.
fn cases() -> Vec<(Event, &'static str)> {
    let clock = |hour, minute| Clock { hour, minute };
    vec![
        (
            Event::Turn {
                metres: 213.0,
                direction: Direction::Left,
                piste: Some(greffier()),
            },
            "Dans deux cents mètres, tournez à gauche. Greffier, piste noire.",
        ),
        (
            Event::Turn {
                metres: 8.0,
                direction: Direction::SlightRight,
                piste: None,
            },
            "Prenez légèrement à droite.",
        ),
        (
            Event::Turn {
                metres: 4600.0,
                direction: Direction::Straight,
                piste: None,
            },
            "Dans cinq kilomètres. Continuez tout droit.",
        ),
        (
            Event::Fork {
                metres: Some(96.0),
                colour: Colour::Blue,
            },
            "Dans cent mètres. À la bifurcation, gardez la bleue.",
        ),
        (
            Event::Arrive { metres: None },
            "Vous arrivez à destination.",
        ),
        (
            Event::Remaining {
                metres: 4180.0,
                seconds: 55.0 * 60.0 + 40.0,
            },
            "Il reste quatre kilomètres. Environ cinquante-cinq minutes.",
        ),
        (
            Event::Remaining {
                metres: 9_400.0,
                seconds: 3.0 * 3600.0 + 59.0 * 60.0,
            },
            "Il reste neuf kilomètres. Environ quatre heures.",
        ),
        (
            Event::Remaining {
                metres: 40.0,
                seconds: 20.0,
            },
            "Il reste moins de cent mètres. Moins d'une minute.",
        ),
        (
            Event::Turnaround {
                seconds_left: 37.0 * 60.0,
                deadline: clock(15, 45),
            },
            "Demi-tour dans trente minutes, afin de rentrer avant la nuit.",
        ),
        (
            Event::Turnaround {
                seconds_left: 60.0,
                deadline: clock(15, 45),
            },
            "Faites demi-tour maintenant, afin de rentrer avant la nuit.",
        ),
        (
            Event::Turnaround {
                seconds_left: 4.0 * 3600.0,
                deadline: clock(15, 45),
            },
            "Heure limite de demi-tour. Quinze heures quarante-cinq.",
        ),
        (
            Event::TurnaroundDeadline {
                deadline: Clock::floor_quarter(16, 10),
            },
            "Heure limite de demi-tour. Seize heures.",
        ),
        (
            Event::TurnaroundAdvanced {
                deadline: clock(15, 15),
            },
            "Vous êtes en retard sur l'heure prévue. Le demi-tour est avancé. Heure limite de demi-tour. Quinze heures quinze.",
        ),
        (
            Event::Slope { degrees: 34.6 },
            "Attention, pente de trente-cinq degrés devant.",
        ),
        (
            Event::ProtectedZone {
                entered: false,
                zone: Some("Réserve naturelle du lac des Rousses".into()),
                rules: vec![ZoneRule::StayOnRoutes, ZoneRule::DogsOnLeash],
                until: Some(DayMonth { day: 30, month: 6 }),
            },
            "Attention, zone protégée devant. Réserve naturelle du lac des Rousses. Restez sur les itinéraires balisés. Chiens tenus en laisse. En vigueur jusqu'au trente juin.",
        ),
        (
            Event::ProtectedZone {
                entered: true,
                zone: None,
                rules: vec![ZoneRule::NoBivouac],
                until: None,
            },
            "Vous entrez dans une zone protégée. Bivouac interdit.",
        ),
        (
            Event::NoNetworkAhead {
                metres: 480.0,
                length_m: Some(2300.0),
            },
            "Dans cinq cents mètres. Début de zone sans aucun réseau, même pour les appels d'urgence. Sur deux kilomètres et demi.",
        ),
        (
            Event::NoNetworkEntered,
            "Vous entrez en zone sans aucun réseau. Les appels d'urgence ne passent pas.",
        ),
        (Event::NoNetworkExit, "Fin de la zone sans réseau."),
        (
            Event::OffRoute {
                side: Side::Right,
                metres: 120.0,
            },
            "Vous avez quitté l'itinéraire. Il est sur votre droite. À cent mètres.",
        ),
        (
            Event::OffRoute {
                side: Side::Behind,
                metres: 3100.0,
            },
            "Vous avez quitté l'itinéraire. Il est derrière vous. Il est à plus de deux kilomètres.",
        ),
        (
            Event::BackOnRoute {
                piste: Some(greffier()),
            },
            "Vous êtes de nouveau sur l'itinéraire. Greffier, piste noire.",
        ),
        (
            Event::Reassurance {
                piste: Some(greffier()),
            },
            "Toujours sur l'itinéraire. Greffier, piste noire.",
        ),
        (
            Event::Briefing(Briefing::Intro {
                site: "Chapelle-des-Bois".into(),
            }),
            "Point du jour. Chapelle-des-Bois.",
        ),
        (
            Event::Briefing(Briefing::Avalanche { level: None }),
            "Pas de bulletin d'avalanche pour ce massif.",
        ),
        (
            Event::Briefing(Briefing::Avalanche { level: Some(3) }),
            "Risque d'avalanche trois sur cinq, marqué.",
        ),
        (
            Event::Briefing(Briefing::PistesOpen {
                open: 21,
                total: 30,
            }),
            "Vingt et une pistes ouvertes.",
        ),
        (
            Event::Briefing(Briefing::PistesOpen {
                open: 30,
                total: 30,
            }),
            "Toutes les pistes sont ouvertes.",
        ),
        (
            Event::Briefing(Briefing::Recommended {
                name: Name::Piste(PisteName::new("La Combette", Some(Colour::Blue))),
            }),
            "Conseillé aujourd'hui. La Combette, piste bleue.",
        ),
        (
            Event::Briefing(Briefing::Weather {
                temperature_c: -7.6,
                wind_kmh: 37.0,
                fog: true,
                snowfall: false,
            }),
            "Température, moins huit degrés. Vent de quarante kilomètres heure. Brouillard prévu.",
        ),
        (
            Event::Briefing(Briefing::Sunset {
                at: Clock::nearest_quarter(17, 9),
            }),
            "Coucher du soleil. Dix-sept heures quinze.",
        ),
        (
            Event::Briefing(Briefing::BestTime { at: clock(10, 30) }),
            "Meilleur moment pour partir. Dix heures trente.",
        ),
        (
            Event::Briefing(Briefing::Snow(Snow::Hard)),
            "Neige dure et regelée.",
        ),
        (
            Event::Briefing(Briefing::Crowd(Crowd::High)),
            "Affluence forte.",
        ),
        (
            Event::Equipment {
                items: vec![Item::Headlamp, Item::TellSomeone],
            },
            "À emporter. Une frontale, le retour est proche de la nuit. Prévenez un proche de votre itinéraire et de votre heure de retour.",
        ),
        (
            Event::Wax {
                kick: Some(Kick::Violet),
                glide: Some(Glide::Cold),
            },
            "Fartage. Fart de retenue violet. Fart de glisse pour neige froide.",
        ),
        (
            Event::AvalancheSegment {
                bera_level: Some(3),
                piste: Some(PisteName::new("Le Cancel", Some(Colour::Red))),
            },
            "Attention, ce tronçon traverse une zone d'avalanche connue. Le Cancel, piste rouge. Risque d'avalanche trois sur cinq, marqué.",
        ),
        (Event::ConditionChange(Change::Fog), "Le brouillard arrive."),
        (
            Event::Battery {
                percent: 43.0,
                autonomy_s: 3.1 * 3600.0,
            },
            "Batterie à quarante pour cent. Autonomie restante. Environ trois heures.",
        ),
        (
            Event::BatteryLow,
            "Batterie faible. Gardez de la réserve pour un appel d'urgence.",
        ),
        (
            Event::Briefing(Briefing::AvalancheOffline),
            "Bulletin d'avalanche non disponible hors ligne. Consultez-le avant de partir.",
        ),
        (
            Event::Briefing(Briefing::AvalanchesPossible),
            "Des avalanches restent possibles sur les fortes pentes.",
        ),
        (
            Event::Briefing(Briefing::RouteAvalancheAreas),
            "Votre itinéraire traverse des zones d'avalanche connues.",
        ),
        (
            Event::Briefing(Briefing::RouteProtectedZone),
            "Votre itinéraire traverse une zone protégée.",
        ),
        (
            Event::Briefing(Briefing::RouteNoNetwork),
            "Une partie de l'itinéraire est sans aucun réseau, même pour les appels d'urgence.",
        ),
        (
            Event::Briefing(Briefing::FireStovesForbidden),
            "Feu interdit sur la commune, réchauds compris.",
        ),
        (
            Event::Briefing(Briefing::Snow(Snow::ColdDry)),
            "Neige froide et sèche.",
        ),
        (
            Event::LoopSummary {
                km: 12.8,
                seconds: 118.0 * 60.0,
                back: Clock {
                    hour: 12,
                    minute: 13,
                },
            },
            "Boucle de treize kilomètres, environ deux heures, retour vers midi quinze.",
        ),
        (
            Event::LoopSummary {
                km: 1.2,
                seconds: 25.0 * 60.0,
                back: Clock {
                    hour: 15,
                    minute: 50,
                },
            },
            "Boucle d'un kilomètre, environ vingt-cinq minutes, retour vers seize heures moins le quart.",
        ),
        (
            Event::Equipment {
                items: vec![Item::SunProtection, Item::FaceProtection, Item::Waxless],
            },
            "À emporter. Des lunettes de soleil et de la crème, le ciel sera dégagé. Des moufles et une protection du visage, il fera très froid. Des skis à écailles ou des peaux, le fart de retenue tiendra mal.",
        ),
    ]
}

#[test]
fn one_phrase_per_event() {
    for (e, want) in cases() {
        assert_eq!(e.announce().text(), want, "{e:?}");
    }
}

#[test]
fn vocabulary_covers_every_in_range_event() {
    let m = vocabulary(&names());
    // Everything in `cases` is covered, except the avalanche segment on "Le Cancel" (not
    // a name of this site): its text is still said.
    for (e, _) in cases() {
        let a = e.announce();
        let foreign = matches!(e, Event::AvalancheSegment { .. });
        assert_eq!(m.covers(&a), !foreign, "{e:?}");
    }
    // Sweeps over the value ranges.
    for d in (0..30_000).step_by(7) {
        let d = f64::from(d);
        for direction in Direction::ALL {
            let a = Event::Turn {
                metres: d.min(20_000.0),
                direction,
                piste: Some(greffier()),
            }
            .announce();
            assert!(m.covers(&a), "turn {d}");
        }
        for side in [Side::Left, Side::Right, Side::Behind, Side::Ahead] {
            assert!(
                m.covers(&Event::OffRoute { side, metres: d }.announce()),
                "off {d}"
            );
        }
        let a = Event::NoNetworkAhead {
            metres: d.min(20_000.0),
            length_m: Some(d.min(20_000.0)),
        }
        .announce();
        assert!(m.covers(&a), "no network {d}");
        let a = Event::Remaining {
            metres: d * 3.0,
            seconds: d * 2.0,
        }
        .announce();
        assert!(m.covers(&a), "remaining {d}");
    }
    for s in (0..6 * 3600).step_by(13) {
        let a = Event::Turnaround {
            seconds_left: f64::from(s),
            deadline: Clock::floor_quarter(14, 50),
        }
        .announce();
        assert!(m.covers(&a), "turnaround {s}");
    }
    for t in -35..=30 {
        for w in 0..=150 {
            let a = Event::Briefing(Briefing::Weather {
                temperature_c: f64::from(t),
                wind_kmh: f64::from(w),
                fog: false,
                snowfall: true,
            })
            .announce();
            assert!(m.covers(&a), "weather {t} {w}");
        }
    }
    for deg in 0..=45 {
        assert!(
            m.covers(
                &Event::Slope {
                    degrees: f64::from(deg)
                }
                .announce()
            )
        );
    }
    for h in 6..=21 {
        for min in 0..60 {
            let a = Event::Briefing(Briefing::Sunset {
                at: Clock::floor_quarter(h, min),
            })
            .announce();
            assert!(m.covers(&a));
        }
    }
    for p in 0..=100 {
        let a = Event::Battery {
            percent: f64::from(p),
            autonomy_s: f64::from(p) * 400.0,
        }
        .announce();
        assert!(m.covers(&a), "battery {p}");
    }
    for open in 0..=30 {
        let a = Event::Briefing(Briefing::PistesOpen { open, total: 30 }).announce();
        assert!(m.covers(&a));
    }
    // Out of range: correct text.
    let cold = Event::Briefing(Briefing::Weather {
        temperature_c: -41.0,
        wind_kmh: 0.0,
        fog: false,
        snowfall: false,
    });
    assert!(
        cold.announce()
            .text()
            .starts_with("Température, moins quarante et un degrés.")
    );
    assert!(!m.covers(&cold.announce()));
    let unknown = Event::Reassurance {
        piste: Some(PisteName::new("Ailleurs", None)),
    };
    assert!(!m.covers(&unknown.announce()));
}

#[test]
fn clip_set_size_and_ids() {
    let m = vocabulary(&names());
    assert_eq!(m.count(ClipKind::Name), 5);
    let fixed = m.count(ClipKind::Fixed);
    let value = m.count(ClipKind::Value);
    assert!(fixed > 60 && value > 400, "{fixed} fixed, {value} values");
    for (id, (_, text)) in &m.clips {
        assert!(
            id.bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'.' || b == b'-'),
            "{id}"
        );
        assert!(!text.contains("  "), "{id}: {text}");
        // Loop summaries are one sentence in three clips: the first two continue it
        // (comma), the last two start in lower case; so do the proposals of C11 (« Je
        // propose la boucle courte, » then « retour vers… »).
        let continues = id.starts_with("loop.km.")
            || id.starts_with("loop.about.")
            || Fixed::ALL
                .iter()
                .any(|f| f.continued() && f.clip().id == *id);
        assert_eq!(text.ends_with(','), continues, "{id}: {text}");
        // A question asks for one gesture (X14): it ends with « ? ».
        assert!(
            continues || text.ends_with('.') || text.ends_with(" ?"),
            "{id}: {text}"
        );
        let lower = id.starts_with("loop.about.") || id.starts_with("loop.back.");
        assert_eq!(
            text.chars().next().is_some_and(char::is_uppercase),
            !lower,
            "{id}: {text}"
        );
    }
}
