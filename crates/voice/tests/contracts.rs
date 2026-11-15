//! Announcements and clips at their thresholds (Q4, #104: survivors of `cargo mutants -p
//! skimap-voice` in `bank.rs`, `clips.rs`, `quantity.rs`): pauses, clip lists, chunking
//! at the token limit, remaining distance and turnaround thresholds, all-open pistes,
//! calm wind, temperature unit, BERA level names, rounding grids.

use skimap_voice::bank::{Briefing, Event, Pause};
use skimap_voice::clips::bera;
use skimap_voice::quantity::{Clock, round_distance, round_duration};

fn text(e: &Event) -> String {
    e.announce().text()
}

#[test]
fn pauses_and_clip_ids() {
    assert_eq!((Pause::Short.ms(), Pause::Long.ms()), (150, 400));
    let a = Event::Remaining {
        metres: 1200.0,
        seconds: 600.0,
    }
    .announce();
    let ids = a.clip_ids();
    assert_eq!(ids.len(), a.segments.len());
    assert_eq!(
        ids,
        a.segments
            .iter()
            .map(|s| s.clip.id.as_str())
            .collect::<Vec<_>>()
    );
    assert!(ids.iter().all(|id| !id.is_empty()));
    assert!(ids[0].starts_with("remain"), "{ids:?}");
}

#[test]
fn chunks_fill_up_to_the_limit() {
    let a = Event::Remaining {
        metres: 1200.0,
        seconds: 600.0,
    }
    .announce();
    let n = a.segments.len();
    assert!(n >= 2);
    // Ten tokens per clip: a limit of exactly 10·n keeps one chunk; 10·n − 1 splits.
    let ten = |_: &str| 10usize;
    assert_eq!(a.chunks(10 * n, ten).unwrap().len(), 1);
    assert_eq!(a.chunks(10 * n - 1, ten).unwrap().len(), 2);
    // A clip of exactly the limit fits; one more token does not.
    assert_eq!(a.chunks(10, ten).unwrap().len(), n);
    assert!(a.chunks(9, ten).is_err());
    let joined = a.chunks(10 * n, ten).unwrap().join(" ");
    assert_eq!(joined, a.text());
}

#[test]
fn remaining_and_turnaround_thresholds() {
    let rem = |m: f64| {
        text(&Event::Remaining {
            metres: m,
            seconds: 600.0,
        })
    };
    // Below 75 m nothing is left to announce as a distance.
    assert_eq!(rem(74.9), rem(10.0));
    assert_ne!(rem(75.0), rem(74.9));
    assert!(rem(76.0).contains("cent mètres"), "{}", rem(76.0));
    let clock = Clock::nearest_quarter(16, 30);
    let turn = |s: f64| {
        text(&Event::Turnaround {
            seconds_left: s,
            deadline: clock,
        })
    };
    let now = turn(0.0);
    assert_eq!(turn(299.0), now);
    assert_ne!(turn(300.0), now);
    assert_eq!(turn(f64::NAN), now);
    // Countdowns below 135 minutes, then the deadline itself.
    let late = turn(135.0 * 60.0);
    assert!(late.contains("Seize heures trente"), "{late}");
    assert_ne!(turn(135.0 * 60.0 - 1.0), late);
}

#[test]
fn briefing_thresholds() {
    let b = |x: Briefing| Event::Briefing(x).announce().text();
    let all = b(Briefing::PistesOpen {
        open: 12,
        total: 12,
    });
    assert_ne!(
        all,
        b(Briefing::PistesOpen {
            open: 11,
            total: 12
        })
    );
    assert_eq!(
        all,
        b(Briefing::PistesOpen {
            open: 13,
            total: 12
        })
    );
    assert_ne!(b(Briefing::PistesOpen { open: 0, total: 0 }), all);
    let w = |kmh: f64| {
        b(Briefing::Weather {
            temperature_c: -3.0,
            wind_kmh: kmh,
            fog: false,
            snowfall: false,
        })
    };
    // Rounded to tens: 4 km/h is calm, 5 km/h is 10 km/h.
    assert_ne!(w(4.0), w(5.0));
    assert!(w(5.0).contains("dix kilomètres heure"), "{}", w(5.0));
    assert!(w(-3.0).contains("degrés"));
    let t = |c: f64| {
        b(Briefing::Weather {
            temperature_c: c,
            wind_kmh: 0.0,
            fog: false,
            snowfall: false,
        })
    };
    assert!(
        t(1.0).contains("un degré.") && t(2.0).contains("deux degrés."),
        "{}",
        t(1.0)
    );
    assert!(t(-1.0).contains("moins un degré."), "{}", t(-1.0));
    assert!(t(-2.0).contains("moins deux degrés."), "{}", t(-2.0));
}

#[test]
fn bera_level_names_and_rounding() {
    for (l, name) in [
        (1, "faible"),
        (2, "limité"),
        (3, "marqué"),
        (4, "fort"),
        (5, "très fort"),
    ] {
        let c = bera(l);
        assert_eq!(c.id, format!("bera.{l}"));
        assert!(c.text.ends_with(&format!(", {name}.")), "{}", c.text);
    }
    assert_eq!(round_distance(14.9), None);
    assert!(round_distance(15.0).is_some());
    assert_eq!(round_duration(29.9), None);
    assert!(round_duration(30.0).is_some());
    let q = |h, m| {
        let c = Clock::nearest_quarter(h, m);
        (c.hour, c.minute)
    };
    assert_eq!(q(16, 7), (16, 0));
    assert_eq!(q(16, 8), (16, 15));
    assert_eq!(q(23, 53), (0, 0));
}
