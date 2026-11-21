//! Property tests of the spoken quantities (Q3, #103): natural rounding of distances,
//! durations and clock times, French number words (parsed back by an independent reader),
//! announcements of any value (never a panic; in range, every phrase in the vocabulary).

use proptest::prelude::*;
use skimap_voice::bank::{Briefing, Event, Side};
use skimap_voice::clips::Direction;
use skimap_voice::numbers::{Gender, cardinal, signed};
use skimap_voice::quantity::{
    Clock, distance_grid, distance_words, duration_grid, duration_words, floor_duration,
    round_distance, round_duration,
};
use skimap_voice::vocabulary::{SiteNames, vocabulary};

/// Independent reader of French number words (traditional spelling, hyphens or spaces).
fn parse_french(s: &str) -> Option<u64> {
    let unit = |w: &str| -> Option<u64> {
        Some(match w {
            "zéro" => 0,
            "un" | "une" => 1,
            "deux" => 2,
            "trois" => 3,
            "quatre" => 4,
            "cinq" => 5,
            "six" => 6,
            "sept" => 7,
            "huit" => 8,
            "neuf" => 9,
            "dix" => 10,
            "onze" => 11,
            "douze" => 12,
            "treize" => 13,
            "quatorze" => 14,
            "quinze" => 15,
            "seize" => 16,
            "vingt" | "vingts" => 20,
            "trente" => 30,
            "quarante" => 40,
            "cinquante" => 50,
            "soixante" => 60,
            _ => return None,
        })
    };
    let mut total = 0u64;
    let mut group = 0u64; // below one thousand
    let mut small = 0u64; // below one hundred
    let words: Vec<&str> = s
        .split([' ', '-'])
        .filter(|w| !w.is_empty() && *w != "et")
        .collect();
    let mut i = 0;
    while i < words.len() {
        let w = words[i];
        match w {
            "cent" | "cents" => {
                group += if small == 0 { 100 } else { small * 100 };
                small = 0;
            }
            "mille" => {
                let g = group + small;
                total += if g == 0 { 1000 } else { g * 1000 };
                group = 0;
                small = 0;
            }
            "million" | "millions" => {
                total += (group + small) * 1_000_000;
                group = 0;
                small = 0;
            }
            "milliard" | "milliards" => {
                total += (group + small) * 1_000_000_000;
                group = 0;
                small = 0;
            }
            "quatre" if matches!(words.get(i + 1), Some(&"vingt" | &"vingts")) => {
                small += 80;
                i += 1;
            }
            _ => {
                let u = unit(w)?;
                small += u;
            }
        }
        i += 1;
    }
    Some(total + group + small)
}

fn names() -> SiteNames {
    SiteNames {
        site: "Cervières".into(),
        pistes: vec![],
        places: vec![],
        dates: vec![],
        status_pistes: 30,
    }
}

/// Any f64, with the special values.
fn any_f64() -> impl Strategy<Value = f64> {
    prop_oneof![
        4 => -1e4f64..1e5,
        1 => any::<f64>(),
        1 => prop_oneof![Just(f64::NAN), Just(f64::INFINITY), Just(f64::NEG_INFINITY), Just(-0.0), Just(f64::MAX), Just(f64::MIN)],
    ]
}

proptest! {
    /// Number words read back to the number, for every u32 and both genders; "un" / "une"
    /// agrees with the gender only in the last group.
    #[test]
    fn cardinal_reads_back(n in prop_oneof![0u32..2000, 0u32..1_000_000, any::<u32>()], fem in any::<bool>()) {
        let g = if fem { Gender::Feminine } else { Gender::Masculine };
        let w = cardinal(n, g);
        prop_assert_eq!(parse_french(&w), Some(u64::from(n)), "{} -> {:?}", n, w);
        prop_assert!(!w.contains("  ") && !w.starts_with(' ') && !w.ends_with(' '));
        if !fem {
            prop_assert!(!w.split([' ', '-']).any(|x| x == "une"), "{w}");
        }
        let s = signed(-(n.min(i32::MAX as u32) as i32), g);
        if n > 0 {
            prop_assert!(s.starts_with("moins "), "{s}");
        }
    }

    /// Distances: the nearest grid value (ties up), monotonic, `None` below 15 m.
    #[test]
    fn distance_rounding(a in 0.0f64..200_000.0, b in 0.0f64..200_000.0) {
        let grid = distance_grid();
        for m in [a, b] {
            match round_distance(m) {
                None => prop_assert!(m < 15.0),
                Some(g) => {
                    prop_assert!(m >= 15.0 && grid.contains(&g));
                    let err = (f64::from(g) - m).abs();
                    prop_assert!(grid.iter().all(|&x| (f64::from(x) - m).abs() >= err - 1e-9), "{m} -> {g}");
                    let words = distance_words(g);
                    prop_assert!(words.ends_with("mètres") || words.ends_with("mètre") || words.ends_with("kilomètres") || words.ends_with("kilomètre") || words.ends_with("et demi"), "{words}");
                }
            }
        }
        let (lo, hi) = if a <= b { (a, b) } else { (b, a) };
        prop_assert!(round_distance(lo).unwrap_or(0) <= round_distance(hi).unwrap_or(0));
    }

    /// Durations: nearest grid value in minutes (ties up), monotonic, `None` below 30 s; the
    /// floor never promises more time than there is and is the largest such value.
    #[test]
    fn duration_rounding(a in 0.0f64..200_000.0, b in 0.0f64..200_000.0) {
        let grid = duration_grid();
        for s in [a, b] {
            match round_duration(s) {
                None => prop_assert!(s < 30.0),
                Some(g) => {
                    prop_assert!(grid.contains(&g));
                    let err = (f64::from(g) - s / 60.0).abs();
                    prop_assert!(grid.iter().all(|&x| (f64::from(x) - s / 60.0).abs() >= err - 1e-9));
                    prop_assert!(!duration_words(g).is_empty());
                }
            }
            match floor_duration(s, &grid) {
                None => prop_assert!(s / 60.0 < f64::from(grid[0])),
                Some(f) => {
                    prop_assert!(f64::from(f) <= s / 60.0);
                    prop_assert!(!grid.iter().any(|&x| x > f && f64::from(x) <= s / 60.0));
                }
            }
        }
        let (lo, hi) = if a <= b { (a, b) } else { (b, a) };
        prop_assert!(round_duration(lo).unwrap_or(0) <= round_duration(hi).unwrap_or(0));
        prop_assert!(floor_duration(lo, &grid).unwrap_or(0) <= floor_duration(hi, &grid).unwrap_or(0));
    }

    /// Clock: the floor is never later than the time and less than a quarter earlier; the
    /// nearest quarter is within 7.5 minutes (around midnight too).
    #[test]
    fn clock_rounding(h in 0u8..24, m in 0u8..60) {
        let t = u32::from(h) * 60 + u32::from(m);
        let f = Clock::floor_quarter(h, m);
        let ft = u32::from(f.hour) * 60 + u32::from(f.minute);
        prop_assert!(ft <= t && t - ft < 15 && f.minute.is_multiple_of(15) && f.hour < 24);
        let n = Clock::nearest_quarter(h, m);
        let nt = u32::from(n.hour) * 60 + u32::from(n.minute);
        let d = (i64::from(nt) - i64::from(t)).rem_euclid(1440);
        let d = d.min(1440 - d);
        prop_assert!(d <= 7 && n.minute.is_multiple_of(15) && n.hour < 24, "{h}:{m} -> {n:?}");
        prop_assert!(!f.spoken().is_empty() && !f.words().is_empty());
        prop_assert_eq!(f.key().len(), 4);
    }

    /// Every announcement built from any value (NaN, infinities, huge or negative values
    /// from corrupt data) has a text, without panicking.
    #[test]
    fn announcements_never_panic(
        x in any_f64(),
        y in any_f64(),
        h in any::<u8>(),
        m in any::<u8>(),
        n in any::<u32>(),
    ) {
        let clock = Clock { hour: h, minute: m };
        let events = vec![
            Event::Turn { metres: x, direction: Direction::Left, piste: None },
            Event::Remaining { metres: x, seconds: y },
            Event::Turnaround { seconds_left: x, deadline: clock },
            Event::Slope { degrees: x },
            Event::OffRoute { side: Side::Left, metres: x },
            Event::NoNetworkAhead { metres: x, length_m: Some(y) },
            Event::LoopSummary { km: x, seconds: y, back: clock },
            Event::Battery { percent: x, autonomy_s: y },
            Event::Briefing(Briefing::Weather { temperature_c: x, wind_kmh: y, fog: true, snowfall: false }),
            Event::Briefing(Briefing::PistesOpen { open: n, total: n / 2 }),
            Event::Briefing(Briefing::Sunset { at: clock }),
        ];
        for e in events {
            let t = e.announce().text();
            prop_assert!(!t.is_empty(), "{e:?}");
        }
    }

    /// In the ranges of the grids, every phrase of the announcement is in the vocabulary.
    #[test]
    fn in_range_values_have_clips(
        metres in 0.0f64..20_000.0,
        seconds in 0.0f64..(24.0 * 3600.0),
        left in 0.0f64..(6.0 * 3600.0),
        degrees in 0.0f64..45.0,
        temperature in -35.0f64..30.0,
        wind in 0.0f64..150.0,
        percent in 0.0f64..=100.0,
    ) {
        let m = vocabulary(&names());
        let deadline = Clock::floor_quarter(14, 50);
        let events = vec![
            Event::Turn { metres, direction: Direction::Right, piste: None },
            Event::Remaining { metres, seconds },
            Event::Turnaround { seconds_left: left, deadline },
            Event::Slope { degrees },
            Event::OffRoute { side: Side::Behind, metres },
            Event::NoNetworkAhead { metres, length_m: Some(metres) },
            Event::Battery { percent, autonomy_s: seconds },
            Event::Briefing(Briefing::Weather { temperature_c: temperature, wind_kmh: wind, fog: false, snowfall: true }),
        ];
        for e in events {
            let a = e.announce();
            prop_assert!(m.covers(&a), "{e:?}: {}", a.text());
        }
    }
}
