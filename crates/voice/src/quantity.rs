//! Natural rounding of distances, durations and clock times, and their French words.
//!
//! Every rounded value belongs to a finite grid, so that each one can be a pre-generated
//! clip: "deux cents mètres", "un kilomètre et demi", "environ une heure et quart".

use serde::{Deserialize, Serialize};

use crate::numbers::{Gender, cardinal};

/// Distance grid in metres, ascending: 20 to 70 m by 10, 100 to 250 m by 50, 300 to 900 m
/// by 100, 1 to 2.5 km by 500 m, 3 to 20 km by 1 km, 25 to 100 km by 5 km.
pub fn distance_grid() -> Vec<u32> {
    let mut g: Vec<u32> = (20..=70).step_by(10).collect();
    g.extend((100..=250).step_by(50));
    g.extend((300..=900).step_by(100));
    g.extend((1000..=2500).step_by(500));
    g.extend((3000..=20_000).step_by(1000));
    g.extend((25_000..=100_000).step_by(5000));
    g
}

/// Nearest grid value of `metres` (ties go up); `None` below 15 m ("maintenant") and the
/// last grid value above it.
pub fn round_distance(metres: f64) -> Option<u32> {
    if !metres.is_finite() || metres < 15.0 {
        return None;
    }
    let grid = distance_grid();
    let mut best = grid[0];
    for &g in &grid {
        if (f64::from(g) - metres).abs() <= (f64::from(best) - metres).abs() {
            best = g;
        }
    }
    Some(best)
}

/// "deux cents mètres", "un kilomètre", "un kilomètre et demi", "douze kilomètres".
pub fn distance_words(metres: u32) -> String {
    if metres < 1000 {
        return format!(
            "{} mètre{}",
            cardinal(metres, Gender::Masculine),
            plural(metres)
        );
    }
    let km = metres / 1000;
    let half = metres % 1000 == 500;
    let base = format!(
        "{} kilomètre{}",
        cardinal(km, Gender::Masculine),
        plural(km)
    );
    if half {
        format!("{base} et demi")
    } else {
        base
    }
}

fn plural(n: u32) -> &'static str {
    if n >= 2 { "s" } else { "" }
}

/// Duration grid in minutes, ascending: 1 to 10 by 1, 15 to 55 by 5, 60 to 180 by 15,
/// 210 to 720 by 30, 780 to 1440 by 60.
pub fn duration_grid() -> Vec<u32> {
    let mut g: Vec<u32> = (1..=10).collect();
    g.extend((15..=55).step_by(5));
    g.extend((60..=180).step_by(15));
    g.extend((210..=720).step_by(30));
    g.extend((780..=1440).step_by(60));
    g
}

/// Nearest grid value in minutes (ties go up); `None` below 30 s ("moins d'une minute").
pub fn round_duration(seconds: f64) -> Option<u32> {
    if !seconds.is_finite() || seconds < 30.0 {
        return None;
    }
    let minutes = seconds / 60.0;
    let grid = duration_grid();
    let mut best = grid[0];
    for &g in &grid {
        if (f64::from(g) - minutes).abs() <= (f64::from(best) - minutes).abs() {
            best = g;
        }
    }
    Some(best)
}

/// Largest grid value not above `seconds` (a countdown must never promise more time than
/// there is); `None` below one minute.
pub fn floor_duration(seconds: f64, grid: &[u32]) -> Option<u32> {
    if !seconds.is_finite() {
        return None;
    }
    let minutes = seconds / 60.0;
    grid.iter()
        .copied()
        .filter(|&g| f64::from(g) <= minutes)
        .max()
}

/// "une minute", "vingt-cinq minutes", "une heure et quart", "deux heures et demie",
/// "une heure trois quarts", "trois heures".
pub fn duration_words(minutes: u32) -> String {
    if minutes < 60 {
        return format!(
            "{} minute{}",
            cardinal(minutes, Gender::Feminine),
            plural(minutes)
        );
    }
    let h = minutes / 60;
    let m = minutes % 60;
    let hours = format!("{} heure{}", cardinal(h, Gender::Feminine), plural(h));
    match m {
        0 => hours,
        15 => format!("{hours} et quart"),
        30 => format!("{hours} et demie"),
        45 => format!("{hours} trois quarts"),
        _ => format!("{hours} {}", cardinal(m, Gender::Feminine)),
    }
}

/// A clock time rounded to a quarter of an hour.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct Clock {
    /// Hour, 0..=23.
    pub hour: u8,
    /// Minute, 0, 15, 30 or 45.
    pub minute: u8,
}

impl Clock {
    /// Rounds down to the quarter of an hour: deadlines (turnaround, nightfall) are never
    /// announced later than they are.
    pub fn floor_quarter(hour: u8, minute: u8) -> Self {
        Self {
            hour: hour % 24,
            minute: (minute.min(59) / 15) * 15,
        }
    }

    /// Nearest quarter of an hour (for times that are not deadlines, e.g. sunset).
    pub fn nearest_quarter(hour: u8, minute: u8) -> Self {
        let total = u32::from(hour % 24) * 60 + u32::from(minute.min(59));
        let q = ((total + 7) / 15 * 15) % (24 * 60);
        Self {
            hour: (q / 60) as u8,
            minute: (q % 60) as u8,
        }
    }

    /// Every quarter of an hour between `from_hour` and `to_hour` inclusive (last one at
    /// `to_hour`:45).
    pub fn quarters(from_hour: u8, to_hour: u8) -> Vec<Self> {
        (from_hour..=to_hour)
            .flat_map(|h| [0, 15, 30, 45].map(|m| Self { hour: h, minute: m }))
            .collect()
    }

    /// "quinze heures quarante-cinq", "seize heures", "une heure trente".
    pub fn words(self) -> String {
        let h = u32::from(self.hour);
        let hours = format!("{} heure{}", cardinal(h, Gender::Feminine), plural(h));
        if self.minute == 0 {
            hours
        } else {
            format!(
                "{hours} {}",
                cardinal(u32::from(self.minute), Gender::Feminine)
            )
        }
    }

    /// The time as people say it: "midi quinze", "treize heures et demie", "seize heures
    /// moins le quart", "minuit", other minutes as [`Clock::words`] ("quinze heures dix").
    /// The quarter past is "quinze" and not "et quart": synthesised and transcribed over 20
    /// seeds, the 16 times "… heures et quart" are heard right 51 % of the time (the quarter
    /// dropped: « retour vers 15h »), "… heures quinze" 88 %, like "et demie" (88 %) and
    /// "moins le quart" (92 %) (docs/voix.md).
    pub fn spoken(self) -> String {
        let name = |h: u32| match h % 24 {
            0 => "minuit".to_owned(),
            12 => "midi".to_owned(),
            h => format!("{} heure{}", cardinal(h, Gender::Feminine), plural(h)),
        };
        let h = u32::from(self.hour);
        let noon_or_midnight = h % 12 == 0;
        match self.minute {
            0 => name(h),
            15 => format!("{} quinze", name(h)),
            30 if noon_or_midnight => format!("{} et demi", name(h)),
            30 => format!("{} et demie", name(h)),
            45 => format!("{} moins le quart", name(h + 1)),
            m => format!("{} {}", name(h), cardinal(u32::from(m), Gender::Feminine)),
        }
    }

    /// Stable key, e.g. `1545`.
    pub fn key(self) -> String {
        format!("{:02}{:02}", self.hour, self.minute)
    }
}

/// Hours covered by the clock clips: deadlines and sunsets of a ski day.
pub const CLOCK_HOURS: (u8, u8) = (6, 21);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spoken_times() {
        let c = |hour, minute| Clock { hour, minute };
        assert_eq!(c(12, 15).spoken(), "midi quinze");
        assert_eq!(c(15, 15).spoken(), "quinze heures quinze");
        assert_eq!(c(12, 30).spoken(), "midi et demi");
        assert_eq!(c(11, 45).spoken(), "midi moins le quart");
        assert_eq!(c(13, 0).spoken(), "treize heures");
        assert_eq!(c(14, 30).spoken(), "quatorze heures et demie");
        assert_eq!(c(15, 45).spoken(), "seize heures moins le quart");
        assert_eq!(c(1, 15).spoken(), "une heure quinze");
        assert_eq!(c(23, 45).spoken(), "minuit moins le quart");
        assert_eq!(c(9, 10).spoken(), "neuf heures dix");
    }

    #[test]
    fn distance_rounding() {
        assert_eq!(round_distance(10.0), None);
        assert_eq!(round_distance(17.0), Some(20));
        assert_eq!(round_distance(46.0), Some(50));
        assert_eq!(round_distance(80.0), Some(70)); // nearer 70 than 100
        assert_eq!(round_distance(88.0), Some(100));
        assert_eq!(round_distance(213.0), Some(200));
        assert_eq!(round_distance(226.0), Some(250));
        assert_eq!(round_distance(640.0), Some(600));
        assert_eq!(round_distance(1260.0), Some(1500));
        assert_eq!(round_distance(4400.0), Some(4000));
        assert_eq!(round_distance(23_000.0), Some(25_000));
        assert_eq!(round_distance(500_000.0), Some(100_000));
        assert_eq!(round_distance(f64::NAN), None);
    }

    #[test]
    fn distance_text() {
        assert_eq!(distance_words(200), "deux cents mètres");
        assert_eq!(distance_words(20), "vingt mètres");
        assert_eq!(distance_words(1000), "un kilomètre");
        assert_eq!(distance_words(1500), "un kilomètre et demi");
        assert_eq!(distance_words(2500), "deux kilomètres et demi");
        assert_eq!(distance_words(12_000), "douze kilomètres");
        for g in distance_grid() {
            assert_eq!(round_distance(f64::from(g)), Some(g));
        }
    }

    #[test]
    fn duration_rounding_and_text() {
        assert_eq!(round_duration(20.0), None);
        assert_eq!(round_duration(50.0), Some(1));
        assert_eq!(round_duration(7.0 * 60.0 + 20.0), Some(7));
        assert_eq!(round_duration(57.0 * 60.0), Some(55));
        assert_eq!(round_duration(58.0 * 60.0), Some(60));
        assert_eq!(round_duration(100.0 * 60.0), Some(105));
        assert_eq!(round_duration(4.2 * 3600.0), Some(240));
        assert_eq!(duration_words(1), "une minute");
        assert_eq!(duration_words(21), "vingt et une minutes");
        assert_eq!(duration_words(60), "une heure");
        assert_eq!(duration_words(75), "une heure et quart");
        assert_eq!(duration_words(90), "une heure et demie");
        assert_eq!(duration_words(105), "une heure trois quarts");
        assert_eq!(duration_words(150), "deux heures et demie");
        assert_eq!(duration_words(720), "douze heures");
        for g in duration_grid() {
            assert_eq!(round_duration(f64::from(g) * 60.0), Some(g));
        }
    }

    #[test]
    fn floor_never_overstates() {
        let grid = [5, 10, 15, 20, 30, 45, 60, 90, 120];
        assert_eq!(floor_duration(4.0 * 60.0, &grid), None);
        assert_eq!(floor_duration(37.0 * 60.0, &grid), Some(30));
        assert_eq!(floor_duration(60.0 * 60.0, &grid), Some(60));
        assert_eq!(floor_duration(500.0 * 60.0, &grid), Some(120));
    }

    #[test]
    fn clock() {
        assert_eq!(
            Clock::floor_quarter(15, 59),
            Clock {
                hour: 15,
                minute: 45
            }
        );
        assert_eq!(
            Clock::nearest_quarter(16, 53),
            Clock {
                hour: 17,
                minute: 0
            }
        );
        assert_eq!(Clock::nearest_quarter(23, 55), Clock { hour: 0, minute: 0 });
        assert_eq!(
            Clock {
                hour: 15,
                minute: 45
            }
            .words(),
            "quinze heures quarante-cinq"
        );
        assert_eq!(
            Clock {
                hour: 16,
                minute: 0
            }
            .words(),
            "seize heures"
        );
        assert_eq!(
            Clock {
                hour: 1,
                minute: 30
            }
            .words(),
            "une heure trente"
        );
        assert_eq!(
            Clock {
                hour: 21,
                minute: 15
            }
            .words(),
            "vingt et une heures quinze"
        );
        assert_eq!(Clock { hour: 9, minute: 5 }.key(), "0905");
        assert_eq!(Clock::quarters(6, 21).len(), 64);
    }
}
