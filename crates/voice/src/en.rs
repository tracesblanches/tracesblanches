//! English words of the voice (VO6): numbers, distances, durations, clock times and the
//! texts of the value clips, on the same grids and with the same ids as the French ones.
//!
//! British spelling (metres, kilometres), metric units, times on the 12-hour clock with
//! the part of the day ("four thirty in the afternoon"), all in words: the speech
//! synthesis reads them as written.

use crate::clips::{DayMonth, Direction};
use crate::names::Colour;
use crate::quantity::Clock;

const UNITS: [&str; 20] = [
    "zero",
    "one",
    "two",
    "three",
    "four",
    "five",
    "six",
    "seven",
    "eight",
    "nine",
    "ten",
    "eleven",
    "twelve",
    "thirteen",
    "fourteen",
    "fifteen",
    "sixteen",
    "seventeen",
    "eighteen",
    "nineteen",
];
const TENS: [&str; 10] = [
    "", "", "twenty", "thirty", "forty", "fifty", "sixty", "seventy", "eighty", "ninety",
];

fn below_hundred(n: u32) -> String {
    if n < 20 {
        return UNITS[n as usize].to_owned();
    }
    let (t, u) = (n / 10, n % 10);
    if u == 0 {
        TENS[t as usize].to_owned()
    } else {
        format!("{}-{}", TENS[t as usize], UNITS[u as usize])
    }
}

fn below_thousand(n: u32) -> String {
    let (h, r) = (n / 100, n % 100);
    match (h, r) {
        (0, r) => below_hundred(r),
        (h, 0) => format!("{} hundred", UNITS[h as usize]),
        (h, r) => format!("{} hundred and {}", UNITS[h as usize], below_hundred(r)),
    }
}

/// English words for any `n`: "twenty-one", "two hundred and five", "one thousand".
pub fn cardinal(n: u32) -> String {
    if n == 0 {
        return UNITS[0].to_owned();
    }
    let groups = [
        (n / 1_000_000_000, "billion"),
        ((n / 1_000_000) % 1000, "million"),
        ((n / 1000) % 1000, "thousand"),
    ];
    let mut parts: Vec<String> = groups
        .iter()
        .filter(|(g, _)| *g > 0)
        .map(|(g, w)| format!("{} {w}", below_thousand(*g)))
        .collect();
    let rest = n % 1000;
    if rest > 0 {
        if !parts.is_empty() && rest < 100 {
            parts.push(format!("and {}", below_hundred(rest)));
        } else {
            parts.push(below_thousand(rest));
        }
    }
    parts.join(" ")
}

/// Signed number: "minus eight".
pub fn signed(n: i32) -> String {
    if n < 0 {
        format!("minus {}", cardinal(n.unsigned_abs()))
    } else {
        cardinal(n.unsigned_abs())
    }
}

/// Ordinal of a day of the month: "first", "twenty-second", "thirtieth".
pub fn ordinal(n: u32) -> String {
    const IRREGULAR: [(u32, &str); 12] = [
        (1, "first"),
        (2, "second"),
        (3, "third"),
        (5, "fifth"),
        (8, "eighth"),
        (9, "ninth"),
        (12, "twelfth"),
        (20, "twentieth"),
        (30, "thirtieth"),
        (40, "fortieth"),
        (50, "fiftieth"),
        (60, "sixtieth"),
    ];
    if let Some((_, w)) = IRREGULAR.iter().find(|(k, _)| *k == n) {
        return (*w).to_owned();
    }
    if n > 20 && n < 100 && !n.is_multiple_of(10) {
        return format!("{}-{}", TENS[(n / 10) as usize], ordinal(n % 10));
    }
    format!("{}th", cardinal(n))
}

/// English month name, 1 = January.
pub fn month_name(month: u8) -> &'static str {
    const MONTHS: [&str; 12] = [
        "January",
        "February",
        "March",
        "April",
        "May",
        "June",
        "July",
        "August",
        "September",
        "October",
        "November",
        "December",
    ];
    MONTHS[usize::from(month.clamp(1, 12) - 1)]
}

fn plural(n: u32, one: &str, many: &str) -> String {
    if n == 1 {
        format!("one {one}")
    } else {
        format!("{} {many}", cardinal(n))
    }
}

/// "two hundred metres", "one kilometre", "one and a half kilometres", "twelve kilometres".
pub fn distance_words(metres: u32) -> String {
    if metres < 1000 {
        return plural(metres, "metre", "metres");
    }
    let km = metres / 1000;
    if metres % 1000 == 500 {
        format!("{} and a half kilometres", cardinal(km))
    } else {
        plural(km, "kilometre", "kilometres")
    }
}

/// "one minute", "twenty-five minutes", "one hour", "one and a half hours", "two hours
/// and fifteen minutes".
pub fn duration_words(minutes: u32) -> String {
    if minutes < 60 {
        return plural(minutes, "minute", "minutes");
    }
    let (h, m) = (minutes / 60, minutes % 60);
    match m {
        0 => plural(h, "hour", "hours"),
        30 => format!("{} and a half hours", cardinal(h)),
        m => format!(
            "{} and {}",
            plural(h, "hour", "hours"),
            plural(m, "minute", "minutes")
        ),
    }
}

fn hour12(h: u32) -> u32 {
    match h % 12 {
        0 => 12,
        h => h,
    }
}

fn part_of_day(h: u32) -> &'static str {
    match h {
        0..=11 => "in the morning",
        12..=17 => "in the afternoon",
        _ => "in the evening",
    }
}

/// A clock time on the 12-hour clock with the part of the day: "four thirty in the
/// afternoon", "nine o'clock in the morning", "noon", "midnight", "twelve fifteen in the
/// afternoon".
pub fn clock_words(c: Clock) -> String {
    let h = u32::from(c.hour % 24);
    let m = u32::from(c.minute);
    match (h, m) {
        (12, 0) => "noon".to_owned(),
        (0, 0) => "midnight".to_owned(),
        (h, 0) => format!("{} o'clock {}", cardinal(hour12(h)), part_of_day(h)),
        (h, m) if m < 10 => format!(
            "{} oh {} {}",
            cardinal(hour12(h)),
            cardinal(m),
            part_of_day(h)
        ),
        (h, m) => format!(
            "{} {} {}",
            cardinal(hour12(h)),
            below_hundred(m),
            part_of_day(h)
        ),
    }
}

/// The time as people say it in a sentence, without the part of the day: "noon", "a
/// quarter past twelve", "half past one", "a quarter to four", "four o'clock".
pub fn clock_spoken(c: Clock) -> String {
    let h = u32::from(c.hour % 24);
    let name = |h: u32| cardinal(hour12(h));
    match c.minute {
        0 => match h {
            0 => "midnight".to_owned(),
            12 => "noon".to_owned(),
            h => format!("{} o'clock", name(h)),
        },
        15 => format!("a quarter past {}", name(h)),
        30 => format!("half past {}", name(h)),
        45 => format!("a quarter to {}", name(h + 1)),
        m => format!("{} {}", name(h), below_hundred(u32::from(m))),
    }
}

/// Colour of a trail: "green", "blue", "red", "black".
pub fn colour(c: Colour) -> &'static str {
    match c {
        Colour::Green => "green",
        Colour::Blue => "blue",
        Colour::Red => "red",
        Colour::Black => "black",
    }
}

/// The instruction of a direction.
pub fn verb(d: Direction) -> &'static str {
    match d {
        Direction::Left => "turn left",
        Direction::Right => "turn right",
        Direction::SlightLeft => "bear left",
        Direction::SlightRight => "bear right",
        Direction::KeepLeft => "keep left",
        Direction::KeepRight => "keep right",
        Direction::Straight => "continue straight ahead",
        Direction::UTurn => "turn back",
    }
}

/// Official English names of the European avalanche danger levels (EAWS).
pub fn bera_name(level: u8) -> &'static str {
    match level {
        1 => "low",
        2 => "moderate",
        3 => "considerable",
        4 => "high",
        _ => "very high",
    }
}

/// "Applies until the thirtieth of June."
pub fn until(d: DayMonth) -> String {
    format!(
        "Applies until the {} of {}.",
        ordinal(u32::from(d.day)),
        month_name(d.month)
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numbers() {
        assert_eq!(cardinal(0), "zero");
        assert_eq!(cardinal(21), "twenty-one");
        assert_eq!(cardinal(100), "one hundred");
        assert_eq!(cardinal(205), "two hundred and five");
        assert_eq!(cardinal(1000), "one thousand");
        assert_eq!(cardinal(1005), "one thousand and five");
        assert!(
            cardinal(4_294_967_295).starts_with("four billion two hundred and ninety-four million")
        );
        assert_eq!(signed(-8), "minus eight");
        assert_eq!(ordinal(1), "first");
        assert_eq!(ordinal(22), "twenty-second");
        assert_eq!(ordinal(30), "thirtieth");
        assert_eq!(ordinal(31), "thirty-first");
        assert_eq!(ordinal(14), "fourteenth");
    }

    #[test]
    fn quantities() {
        assert_eq!(distance_words(200), "two hundred metres");
        assert_eq!(distance_words(1000), "one kilometre");
        assert_eq!(distance_words(1500), "one and a half kilometres");
        assert_eq!(distance_words(12_000), "twelve kilometres");
        assert_eq!(duration_words(1), "one minute");
        assert_eq!(duration_words(90), "one and a half hours");
        assert_eq!(duration_words(135), "two hours and fifteen minutes");
        let c = |hour, minute| Clock { hour, minute };
        assert_eq!(clock_words(c(16, 30)), "four thirty in the afternoon");
        assert_eq!(clock_words(c(9, 0)), "nine o'clock in the morning");
        assert_eq!(clock_words(c(12, 0)), "noon");
        assert_eq!(clock_words(c(19, 5)), "seven oh five in the evening");
        assert_eq!(clock_spoken(c(12, 15)), "a quarter past twelve");
        assert_eq!(clock_spoken(c(13, 30)), "half past one");
        assert_eq!(clock_spoken(c(15, 45)), "a quarter to four");
        assert_eq!(clock_spoken(c(16, 0)), "four o'clock");
        assert_eq!(clock_spoken(c(11, 45)), "a quarter to twelve");
        assert_eq!(clock_spoken(c(12, 0)), "noon");
    }
}
