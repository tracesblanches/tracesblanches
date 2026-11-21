//! Crowd forecast (A4, #91): school holidays (zones A, B, C, Wallonia-Brussels, neighbouring
//! Swiss cantons), weekends, public holidays, condition bonus.
//!
//! # Heuristic
//!
//! Each site has a catchment ([`catchment`]): for every population (French zone, Fédération
//! Wallonie-Bruxelles, canton of Vaud, canton of Neuchâtel) two weights, one for stays
//! during its school holidays and one for day trips on its weekends and public holidays.
//! The score of a day is
//!
//! ```text
//! score = 1 (locals, retired people)
//!       + Σ over populations of: holiday weight if the population is on school holiday,
//!                                otherwise weekend weight on a Saturday, a Sunday or one of
//!                                its public holidays, otherwise 0
//!       + condition bonus (fresh snow ≥ 10 cm over 48 h: +1; sunny: +0.5; both: +0.5 more)
//! ```
//!
//! A holiday period that starts on a Monday or ends on a Friday (Belgium, Switzerland) also
//! covers the adjacent weekend (departure and return weekends). Levels: score < 2 calm,
//! < 4 moderate, < 6 busy, ≥ 6 very busy.
//!
//! Weights (judgement, to be calibrated with attendance data, none available) come from the
//! table `data/catchments.json`: a profile per group of departments (southern Alps, French
//! Jura…), and for a department without a profile a default built from the holiday zone of
//! its académie (home zone: holidays 3, weekends 1.5; the two other zones: holidays 1.5;
//! Fédération Wallonie-Bruxelles: holidays 0.5). The opening dates of a domain, when it
//! publishes them, come from the site table (`local.season`).
//!
//! Limits: no Italian or Spanish calendar (border domains); Swiss skiers mostly stay in
//! Switzerland; no attendance data to calibrate. The conditions come later from the
//! weather layer (`meteo-hourly`): fresh snow = sum of `snowfall_cm` of the two previous
//! days, sunny = `daytime_cloud_cover_pct` < 30 %.
//!
//! # Sources (read on 14/11/2026)
//!
//! See [`SOURCES`]. Dates are stored as the days without school, weekends included: the
//! French calendar says « du samedi 19 décembre 2026 au lundi 4 janvier 2027 », the Monday
//! being the day classes resume, so the last day without school is Sunday 3 January.

use std::collections::BTreeMap;
use std::fmt;
use std::sync::LazyLock;

use jiff::civil;
use serde::Deserialize;
use skimap_common::site::Site;

/// A calendar date (proleptic Gregorian), without any time zone.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Date {
    /// Year.
    pub year: i32,
    /// Month, 1 to 12.
    pub month: u8,
    /// Day, 1 to 31.
    pub day: u8,
}

impl Date {
    /// Date from year, month and day (not validated; use [`Date::parse`] for input).
    pub const fn new(year: i32, month: u8, day: u8) -> Self {
        Self { year, month, day }
    }

    /// Parses `YYYY-MM-DD`; rejects impossible dates.
    pub fn parse(s: &str) -> Option<Self> {
        let mut it = s.split('-');
        let (y, m, d) = (it.next()?, it.next()?, it.next()?);
        if it.next().is_some() || y.len() != 4 || m.len() != 2 || d.len() != 2 {
            return None;
        }
        let date = Self::new(y.parse().ok()?, m.parse().ok()?, d.parse().ok()?);
        (Self::from_days(date.days()) == date).then_some(date)
    }

    /// Days since 1970-01-01 (H. Hinnant's `days_from_civil`).
    pub fn days(self) -> i64 {
        let y = i64::from(self.year) - i64::from(self.month <= 2);
        let era = y.div_euclid(400);
        let yoe = y - era * 400;
        let m = i64::from(self.month);
        let doy = (153 * (m + if m > 2 { -3 } else { 9 }) + 2) / 5 + i64::from(self.day) - 1;
        let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
        era * 146_097 + doe - 719_468
    }

    /// Date from days since 1970-01-01 (H. Hinnant's `civil_from_days`).
    pub fn from_days(z: i64) -> Self {
        let z = z + 719_468;
        let era = z.div_euclid(146_097);
        let doe = z - era * 146_097;
        let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
        let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
        let mp = (5 * doy + 2) / 153;
        let day = (doy - (153 * mp + 2) / 5 + 1) as u8;
        let month = if mp < 10 { mp + 3 } else { mp - 9 } as u8;
        let year = (yoe + era * 400 + i64::from(month <= 2)) as i32;
        Self { year, month, day }
    }

    /// Day of the week, 0 = Monday … 6 = Sunday.
    pub fn weekday(self) -> u8 {
        // 1970-01-01 was a Thursday (3).
        (self.days() + 3).rem_euclid(7) as u8
    }

    /// Saturday or Sunday.
    pub fn is_weekend(self) -> bool {
        self.weekday() >= 5
    }

    /// The date `n` days later (or earlier when negative).
    pub fn add_days(self, n: i64) -> Self {
        Self::from_days(self.days() + n)
    }
}

impl fmt::Display for Date {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:04}-{:02}-{:02}", self.year, self.month, self.day)
    }
}

/// A population with its own school calendar and public holidays.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Population {
    /// France, zone A.
    FranceZoneA,
    /// France, zone B.
    FranceZoneB,
    /// France, zone C.
    FranceZoneC,
    /// Fédération Wallonie-Bruxelles (French-speaking Belgium).
    Fwb,
    /// Canton of Vaud.
    Vaud,
    /// Canton of Neuchâtel.
    Neuchatel,
}

impl Population {
    /// Short French label.
    pub fn label(self) -> &'static str {
        crate::text::of(crate::text::Lang::Fr).population(self)
    }

    fn is_french(self) -> bool {
        matches!(
            self,
            Population::FranceZoneA | Population::FranceZoneB | Population::FranceZoneC
        )
    }
}

/// A school holiday period: days without school, first and last included.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Period {
    /// Population.
    pub population: Population,
    /// Name.
    pub name: &'static str,
    /// First day without school.
    pub first: Date,
    /// Last day without school.
    pub last: Date,
}

const fn p(population: Population, name: &'static str, first: Date, last: Date) -> Period {
    Period {
        population,
        name,
        first,
        last,
    }
}

const fn d(year: i32, month: u8, day: u8) -> Date {
    Date::new(year, month, day)
}

use Population::{FranceZoneA as A, FranceZoneB as B, FranceZoneC as C, Fwb, Neuchatel, Vaud};

/// School holidays of the 2026-27 school year (see [`SOURCES`]).
pub const SCHOOL_HOLIDAYS_2026_27: &[Period] = &[
    // France (data.education.gouv.fr, fr-en-calendrier-scolaire; service-public.gouv.fr).
    p(
        A,
        "vacances de la Toussaint",
        d(2026, 10, 17),
        d(2026, 11, 1),
    ),
    p(
        B,
        "vacances de la Toussaint",
        d(2026, 10, 17),
        d(2026, 11, 1),
    ),
    p(
        C,
        "vacances de la Toussaint",
        d(2026, 10, 17),
        d(2026, 11, 1),
    ),
    p(A, "vacances de Noël", d(2026, 12, 19), d(2027, 1, 3)),
    p(B, "vacances de Noël", d(2026, 12, 19), d(2027, 1, 3)),
    p(C, "vacances de Noël", d(2026, 12, 19), d(2027, 1, 3)),
    p(A, "vacances d'hiver", d(2027, 2, 13), d(2027, 2, 28)),
    p(B, "vacances d'hiver", d(2027, 2, 20), d(2027, 3, 7)),
    p(C, "vacances d'hiver", d(2027, 2, 6), d(2027, 2, 21)),
    p(A, "vacances de printemps", d(2027, 4, 10), d(2027, 4, 25)),
    p(B, "vacances de printemps", d(2027, 4, 17), d(2027, 5, 2)),
    p(C, "vacances de printemps", d(2027, 4, 3), d(2027, 4, 18)),
    p(A, "pont de l'Ascension", d(2027, 5, 6), d(2027, 5, 9)),
    p(B, "pont de l'Ascension", d(2027, 5, 6), d(2027, 5, 9)),
    p(C, "pont de l'Ascension", d(2027, 5, 6), d(2027, 5, 9)),
    // Fédération Wallonie-Bruxelles (enseignement.be, calendrier scolaire 2026-2027).
    p(Fwb, "vacances d'automne", d(2026, 10, 19), d(2026, 11, 1)),
    p(Fwb, "vacances d'hiver", d(2026, 12, 21), d(2027, 1, 3)),
    p(Fwb, "vacances de détente", d(2027, 2, 22), d(2027, 3, 7)),
    p(Fwb, "vacances de printemps", d(2027, 4, 26), d(2027, 5, 9)),
    // Vaud (vd.ch, jours fériés et vacances scolaires 2026 et 2027).
    p(Vaud, "vacances d'automne", d(2026, 10, 10), d(2026, 10, 25)),
    p(Vaud, "vacances d'hiver", d(2026, 12, 24), d(2027, 1, 10)),
    p(Vaud, "relâches", d(2027, 2, 6), d(2027, 2, 14)),
    p(Vaud, "vacances de Pâques", d(2027, 3, 26), d(2027, 4, 11)),
    p(Vaud, "Ascension", d(2027, 5, 6), d(2027, 5, 9)),
    // Neuchâtel (ne.ch, calendrier et vacances scolaires, année 2026-2027).
    p(
        Neuchatel,
        "vacances d'automne",
        d(2026, 10, 5),
        d(2026, 10, 16),
    ),
    p(
        Neuchatel,
        "vacances d'hiver",
        d(2026, 12, 21),
        d(2027, 1, 1),
    ),
    p(
        Neuchatel,
        "vacances du 1er mars",
        d(2027, 3, 1),
        d(2027, 3, 5),
    ),
    p(
        Neuchatel,
        "vacances de printemps",
        d(2027, 3, 26),
        d(2027, 4, 9),
    ),
    p(Neuchatel, "Ascension", d(2027, 5, 6), d(2027, 5, 7)),
];

/// A public holiday of a population.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PublicHoliday {
    /// Population (French holidays apply to the three zones).
    pub population: Population,
    /// Name.
    pub name: &'static str,
    /// Date.
    pub date: Date,
}

const fn h(population: Population, name: &'static str, date: Date) -> PublicHoliday {
    PublicHoliday {
        population,
        name,
        date,
    }
}

/// Public holidays from November 2026 to May 2027. French ones are listed once with
/// [`Population::FranceZoneA`] and apply to the three zones.
pub const PUBLIC_HOLIDAYS_2026_27: &[PublicHoliday] = &[
    h(A, "Armistice", d(2026, 11, 11)),
    h(A, "Noël", d(2026, 12, 25)),
    h(A, "Jour de l'an", d(2027, 1, 1)),
    h(A, "Lundi de Pâques", d(2027, 3, 29)),
    h(A, "Fête du travail", d(2027, 5, 1)),
    h(A, "Ascension", d(2027, 5, 6)),
    h(A, "Victoire 1945", d(2027, 5, 8)),
    h(A, "Lundi de Pentecôte", d(2027, 5, 17)),
    h(Fwb, "Jour de l'Armistice", d(2026, 11, 11)),
    h(Fwb, "Noël", d(2026, 12, 25)),
    h(Fwb, "Nouvel an", d(2027, 1, 1)),
    h(Fwb, "Lundi de Pâques", d(2027, 3, 29)),
    h(Fwb, "Fête du travail", d(2027, 5, 1)),
    h(Fwb, "Ascension", d(2027, 5, 6)),
    h(Fwb, "Lundi de Pentecôte", d(2027, 5, 17)),
    h(Vaud, "Noël", d(2026, 12, 25)),
    h(Vaud, "Nouvel an", d(2027, 1, 1)),
    h(Vaud, "2 janvier", d(2027, 1, 2)),
    h(Vaud, "Vendredi saint", d(2027, 3, 26)),
    h(Vaud, "Lundi de Pâques", d(2027, 3, 29)),
    h(Vaud, "Ascension", d(2027, 5, 6)),
    h(Vaud, "Lundi de Pentecôte", d(2027, 5, 17)),
    h(Neuchatel, "Noël", d(2026, 12, 25)),
    h(Neuchatel, "Nouvel an", d(2027, 1, 1)),
    h(Neuchatel, "Instauration de la République", d(2027, 3, 1)),
    h(Neuchatel, "Vendredi saint", d(2027, 3, 26)),
    h(Neuchatel, "Lundi de Pâques", d(2027, 3, 29)),
    h(Neuchatel, "Ascension", d(2027, 5, 6)),
    h(Neuchatel, "Lundi de Pentecôte", d(2027, 5, 17)),
];

/// An official source of the calendars.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Source {
    /// What it gives.
    pub name: &'static str,
    /// URL.
    pub url: &'static str,
}

/// Sources of the calendars, read on 14/11/2026.
pub const SOURCES: &[Source] = &[
    Source {
        name: "Calendrier scolaire 2026-2027, zones A, B, C (Ministère de l'Éducation \
               nationale, jeu de données fr-en-calendrier-scolaire)",
        url: "https://data.education.gouv.fr/explore/dataset/fr-en-calendrier-scolaire/",
    },
    Source {
        name: "Vacances scolaires 2026-2027 (service-public.gouv.fr, arrêté du 22/10/2025)",
        url: "https://www.service-public.gouv.fr/particuliers/actualites/A18563",
    },
    Source {
        name: "Calendrier scolaire 2026-2027, Fédération Wallonie-Bruxelles",
        url: "https://www.enseignement.be/calendrier-scolaire",
    },
    Source {
        name: "Jours fériés et vacances scolaires 2026 et 2027, État de Vaud",
        url: "https://www.vd.ch/formation/jours-feries-et-vacances-scolaires",
    },
    Source {
        name: "Calendrier et vacances scolaires, année 2026-2027, canton de Neuchâtel",
        url: "https://www.ne.ch/themes/scolarite-et-formation/calendrier-et-vacances-scolaires",
    },
];

/// Weights of one population for one site.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Weight {
    /// Population.
    pub population: Population,
    /// Weight on its school holidays (stays).
    pub holidays: f64,
    /// Weight on its weekends and public holidays (day trips).
    pub weekends: f64,
}

const fn w(population: Population, holidays: f64, weekends: f64) -> Weight {
    Weight {
        population,
        holidays,
        weekends,
    }
}

/// Catchment of a site.
#[derive(Debug, Clone, PartialEq)]
pub struct Catchment {
    /// Weights per population.
    pub weights: Vec<Weight>,
    /// Opening dates of the domain, if published.
    pub season: Option<(Date, Date)>,
}

#[derive(Deserialize)]
#[serde(rename_all = "snake_case")]
enum PopulationRecord {
    ZoneA,
    ZoneB,
    ZoneC,
    Fwb,
    Vaud,
    Neuchatel,
}

impl From<&PopulationRecord> for Population {
    fn from(p: &PopulationRecord) -> Self {
        match p {
            PopulationRecord::ZoneA => A,
            PopulationRecord::ZoneB => B,
            PopulationRecord::ZoneC => C,
            PopulationRecord::Fwb => Fwb,
            PopulationRecord::Vaud => Vaud,
            PopulationRecord::Neuchatel => Neuchatel,
        }
    }
}

#[derive(Deserialize)]
struct WeightRecord {
    population: PopulationRecord,
    holidays: f64,
    weekends: f64,
}

#[derive(Deserialize)]
struct PairRecord {
    holidays: f64,
    weekends: f64,
}

#[derive(Deserialize)]
struct ProfileRecord {
    departments: Vec<String>,
    weights: Vec<WeightRecord>,
}

#[derive(Deserialize)]
struct DefaultRecord {
    home: PairRecord,
    other_zones: PairRecord,
    fwb: PairRecord,
}

#[derive(Deserialize)]
struct CatchmentTable {
    zones: BTreeMap<String, Vec<String>>,
    profiles: Vec<ProfileRecord>,
    default: DefaultRecord,
}

static TABLE: LazyLock<CatchmentTable> = LazyLock::new(|| {
    serde_json::from_str(include_str!("../data/catchments.json")).expect("data/catchments.json")
});

/// Holiday zone of a French department (académie of the department), `None` outside the
/// three zones (Corsica, overseas).
pub fn zone_of_department(department: &str) -> Option<Population> {
    TABLE
        .zones
        .iter()
        .find(|(_, deps)| deps.iter().any(|d| d == department))
        .and_then(|(z, _)| match z.as_str() {
            "A" => Some(A),
            "B" => Some(B),
            "C" => Some(C),
            _ => None,
        })
}

/// Catchment of a site: the profile of its department, or else the default built from the
/// holiday zone of its department; opening dates from the site table.
pub fn catchment(site: &Site) -> Catchment {
    let t = &*TABLE;
    let weights = match t
        .profiles
        .iter()
        .find(|p| p.departments.iter().any(|d| d == site.department))
    {
        Some(p) => p
            .weights
            .iter()
            .map(|r| w((&r.population).into(), r.holidays, r.weekends))
            .collect(),
        None => {
            let home = zone_of_department(site.department);
            let mut v = Vec::new();
            if let Some(h) = home {
                v.push(w(h, t.default.home.holidays, t.default.home.weekends));
            }
            for z in [A, B, C] {
                if Some(z) != home {
                    v.push(w(
                        z,
                        t.default.other_zones.holidays,
                        t.default.other_zones.weekends,
                    ));
                }
            }
            v.push(w(Fwb, t.default.fwb.holidays, t.default.fwb.weekends));
            v
        }
    };
    let date = |n: civil::Date| Date::new(i32::from(n.year()), n.month() as u8, n.day() as u8);
    let season = skimap_common::site::local(site.id)
        .and_then(|l| l.season.as_ref())
        .map(|s| (date(s.first), date(s.last)));
    Catchment { weights, season }
}

/// Snow and weather of the day, from the weather layer.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Conditions {
    /// Fresh snow over the previous 48 hours, cm.
    pub fresh_snow_cm: f64,
    /// Sunny day (daytime cloud cover below 30 %).
    pub sunny: bool,
}

/// Minimum fresh snow for the bonus, cm.
pub const FRESH_SNOW_CM: f64 = 10.0;

/// Crowd level.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum CrowdLevel {
    /// Calm.
    Calm = 1,
    /// Moderate.
    Moderate = 2,
    /// Busy.
    Busy = 3,
    /// Very busy.
    VeryBusy = 4,
}

impl CrowdLevel {
    /// Level of a score.
    pub fn from_score(score: f64) -> Self {
        if score < 2.0 {
            CrowdLevel::Calm
        } else if score < 4.0 {
            CrowdLevel::Moderate
        } else if score < 6.0 {
            CrowdLevel::Busy
        } else {
            CrowdLevel::VeryBusy
        }
    }

    /// French label.
    pub fn label(self) -> &'static str {
        self.label_in(crate::text::Lang::Fr)
    }

    /// Label in `lang`.
    pub fn label_in(self, lang: crate::text::Lang) -> &'static str {
        crate::text::of(lang).crowd_level(self)
    }
}

/// Forecast of one day at one site.
#[derive(Debug, Clone, PartialEq)]
pub struct DayForecast {
    /// Date.
    pub date: Date,
    /// Score (see the module documentation).
    pub score: f64,
    /// Level.
    pub level: CrowdLevel,
    /// Reasons, in French (« vacances d'hiver zone A », « dimanche »...).
    pub reasons: Vec<String>,
    /// Whether the domain is open that day, when the opening dates are published.
    pub open: Option<bool>,
}

/// Whether `population` has no school on `date`, with the name of the period. A period
/// starting on a Monday also covers the weekend before; one ending on a Friday, the weekend
/// after.
pub fn school_holiday(population: Population, date: Date) -> Option<&'static Period> {
    SCHOOL_HOLIDAYS_2026_27.iter().find(|p| {
        if p.population != population {
            return false;
        }
        if p.first <= date && date <= p.last {
            return true;
        }
        if !date.is_weekend() {
            return false;
        }
        let before = p.first.weekday() == 0 && date < p.first && p.first.days() - date.days() <= 2;
        let after = p.last.weekday() == 4 && date > p.last && date.days() - p.last.days() <= 2;
        before || after
    })
}

/// Public holiday of `population` on `date` (French holidays for the three zones).
pub fn public_holiday(population: Population, date: Date) -> Option<&'static PublicHoliday> {
    PUBLIC_HOLIDAYS_2026_27.iter().find(|h| {
        h.date == date
            && (h.population == population || (population.is_french() && h.population == A))
    })
}

/// Forecast of one day, reasons in French.
pub fn forecast_day(site: &Site, date: Date, conditions: Option<&Conditions>) -> DayForecast {
    forecast_day_in(site, date, conditions, crate::text::Lang::Fr)
}

/// Forecast of one day, reasons in `lang`.
pub fn forecast_day_in(
    site: &Site,
    date: Date,
    conditions: Option<&Conditions>,
    lang: crate::text::Lang,
) -> DayForecast {
    let t = crate::text::of(lang);
    let c = catchment(site);
    let mut score = 1.0;
    let mut reasons = Vec::new();
    if date.is_weekend() {
        reasons.push(t.weekday(usize::from(date.weekday())).to_owned());
    }
    // Reasons grouped by name: « vacances d'hiver (zone A, zone B) », « Noël (France,
    // Vaud) ».
    let mut publics: Vec<(&str, Vec<&str>)> = Vec::new();
    let mut schools: Vec<(&str, Vec<&str>)> = Vec::new();
    let push = |list: &mut Vec<(&'static str, Vec<&'static str>)>, name, who| match list
        .iter_mut()
        .find(|(n, _)| *n == name)
    {
        Some((_, v)) if !v.contains(&who) => v.push(who),
        Some(_) => {}
        None => list.push((name, vec![who])),
    };
    for wgt in &c.weights {
        let pop = wgt.population;
        let country = if pop.is_french() {
            t.france()
        } else {
            t.population(pop)
        };
        let public = public_holiday(pop, date);
        if let Some(h) = public {
            push(&mut publics, t.holiday(h.name), country);
        }
        if let Some(p) = school_holiday(pop, date) {
            score += wgt.holidays;
            push(&mut schools, t.holiday(p.name), t.population(pop));
        } else if public.is_some() || date.is_weekend() {
            score += wgt.weekends;
        }
    }
    for (name, who) in publics.into_iter().chain(schools) {
        reasons.push(t.holiday_for(name, &who));
    }
    if let Some(cond) = conditions {
        let fresh = cond.fresh_snow_cm >= FRESH_SNOW_CM;
        if fresh {
            score += 1.0;
            reasons.push(t.crowd_fresh_snow(cond.fresh_snow_cm));
        }
        if cond.sunny {
            score += 0.5;
            reasons.push(t.crowd_sun().to_owned());
        }
        if fresh && cond.sunny {
            score += 0.5;
        }
    }
    let open = c.season.map(|(a, b)| a <= date && date <= b);
    if open == Some(false) {
        reasons.push(t.crowd_area_closed().to_owned());
    }
    DayForecast {
        date,
        score,
        level: CrowdLevel::from_score(score),
        reasons,
        open,
    }
}

/// Forecast of every day from `first` to `last` included, without conditions.
pub fn forecast_range(site: &Site, first: Date, last: Date) -> Vec<DayForecast> {
    (first.days()..=last.days())
        .map(|n| forecast_day(site, Date::from_days(n), None))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(id: &str) -> &'static Site {
        skimap_common::site::site(id).unwrap()
    }

    #[test]
    fn dates() {
        assert_eq!(Date::new(1970, 1, 1).days(), 0);
        assert_eq!(Date::new(2000, 3, 1).days(), 11_017);
        for n in [-1000, 0, 11_017, 20_000, 20_806, 21_000] {
            assert_eq!(Date::from_days(n).days(), n);
        }
        assert_eq!(
            Date::new(2026, 11, 14).weekday(),
            5,
            "14/11/2026 is a Saturday"
        );
        assert_eq!(Date::new(2026, 12, 19).weekday(), 5);
        assert_eq!(
            Date::new(2027, 2, 22).weekday(),
            0,
            "Monday 22 February 2027"
        );
        assert_eq!(Date::new(2027, 3, 28).weekday(), 6, "Easter Sunday 2027");
        assert_eq!(Date::parse("2027-02-29"), None);
        assert_eq!(Date::parse("2028-02-29"), Some(Date::new(2028, 2, 29)));
        assert_eq!(Date::parse("2027-1-01"), None);
        assert_eq!(Date::new(2026, 12, 31).add_days(1), Date::new(2027, 1, 1));
        assert_eq!(Date::new(2027, 3, 7).to_string(), "2027-03-07");
    }

    #[test]
    fn calendars_match_the_sources() {
        // Facts given with the ticket and checked against the official pages.
        let on = |p, y, m, d| school_holiday(p, Date::new(y, m, d)).is_some();
        // Zone A 13/02–01/03 (classes resume Monday 1 March).
        assert!(!on(A, 2027, 2, 12) && on(A, 2027, 2, 13) && on(A, 2027, 2, 28));
        assert!(!on(A, 2027, 3, 1));
        // Zone B 20/02–08/03, zone C 06/02–22/02.
        assert!(on(B, 2027, 2, 20) && on(B, 2027, 3, 7) && !on(B, 2027, 3, 8));
        assert!(on(C, 2027, 2, 6) && on(C, 2027, 2, 21) && !on(C, 2027, 2, 22));
        // Christmas 19/12/2026–04/01/2027.
        assert!(on(A, 2026, 12, 19) && on(C, 2027, 1, 3) && !on(B, 2027, 1, 4));
        // FWB congé de détente 22/02–07/03/2027, plus the weekend before.
        assert!(on(Fwb, 2027, 2, 22) && on(Fwb, 2027, 3, 7) && !on(Fwb, 2027, 3, 8));
        assert!(on(Fwb, 2027, 2, 20), "departure weekend");
        assert!(!on(Fwb, 2027, 2, 19));
        // Neuchâtel 1–5 March: return weekend included.
        assert!(on(Neuchatel, 2027, 3, 6) && on(Neuchatel, 2027, 2, 27));
        // Vaud relâches 6–14 February.
        assert!(on(Vaud, 2027, 2, 6) && on(Vaud, 2027, 2, 14) && !on(Vaud, 2027, 2, 15));
        // Every period is well ordered and inside the school year.
        for p in SCHOOL_HOLIDAYS_2026_27 {
            assert!(p.first <= p.last, "{p:?}");
            assert!(Date::new(2026, 9, 1) <= p.first && p.last <= Date::new(2027, 6, 30));
        }
        assert!(
            public_holiday(B, Date::new(2027, 3, 29)).is_some(),
            "Easter Monday, all zones"
        );
        assert!(public_holiday(Neuchatel, Date::new(2027, 3, 1)).is_some());
        assert!(public_holiday(Fwb, Date::new(2027, 3, 1)).is_none());
    }

    #[test]
    fn levels_on_key_days() {
        let lvl = |s, y, m, d| forecast_day(at(s), Date::new(y, m, d), None).level;
        use CrowdLevel::*;
        // Chapelle-des-Bois: the week of 22–28 February 2027 has zones A and B and the
        // Belgian congé de détente together (report 02: probable peak).
        for day in 22..=26 {
            assert_eq!(lvl("chapelle-des-bois", 2027, 2, day), VeryBusy, "{day}");
        }
        // 1–5 March: zone B, Belgium, Neuchâtel only.
        assert_eq!(lvl("chapelle-des-bois", 2027, 3, 3), Busy);
        // A weekday in January outside every holiday.
        assert_eq!(lvl("chapelle-des-bois", 2027, 1, 13), Calm);
        // An ordinary Sunday.
        assert_eq!(lvl("chapelle-des-bois", 2027, 1, 17), Moderate);
        // Christmas holidays everywhere.
        assert_eq!(lvl("chapelle-des-bois", 2026, 12, 29), VeryBusy);
        // Cervières: zone B is home; 22–26 February has A and B.
        assert_eq!(lvl("cervieres", 2027, 2, 24), VeryBusy);
        assert_eq!(lvl("cervieres", 2027, 3, 3), Busy);
        assert_eq!(lvl("cervieres", 2027, 2, 10), Moderate, "zone C only");
        assert_eq!(lvl("cervieres", 2027, 1, 13), Calm);
        // First week of January at Cervières (report 02): busy until Sunday 3, calm after.
        assert_eq!(lvl("cervieres", 2027, 1, 2), VeryBusy);
        assert_eq!(lvl("cervieres", 2027, 1, 5), Calm);
        let xmas = forecast_day(at("cervieres"), Date::new(2026, 12, 25), None);
        assert_eq!(
            xmas.reasons,
            [
                "Noël (France, Belgique (FWB), Vaud, Neuchâtel)",
                "vacances de Noël (zone B, zone A, zone C)",
                "vacances d'hiver (Belgique (FWB), Vaud, Neuchâtel)"
            ]
        );
        let sunday = forecast_day(at("cervieres"), Date::new(2027, 2, 21), None);
        assert_eq!(sunday.reasons[0], "dimanche");
    }

    #[test]
    fn any_site_has_a_catchment() {
        // A domain of a department without a profile: home zone, the two other zones,
        // Belgium; no published opening dates.
        let mut vosges = *at("cervieres");
        vosges.id = "un-domaine-des-vosges";
        vosges.department = "88";
        let c = catchment(&vosges);
        let pops: Vec<_> = c.weights.iter().map(|w| w.population).collect();
        assert_eq!(pops, [B, A, C, Fwb]);
        assert_eq!((c.weights[0].holidays, c.weights[0].weekends), (3.0, 1.5));
        assert_eq!((c.weights[1].holidays, c.weights[1].weekends), (1.5, 0.0));
        assert_eq!(c.season, None);
        let f = forecast_day(&vosges, Date::new(2027, 2, 24), None);
        assert_eq!(f.level, CrowdLevel::VeryBusy);
        // Every department of mainland France but Corsica has a zone.
        assert_eq!(zone_of_department("25"), Some(A));
        assert_eq!(zone_of_department("05"), Some(B));
        assert_eq!(zone_of_department("31"), Some(C));
        assert_eq!(zone_of_department("2A"), None);
        let all: usize = TABLE.zones.values().map(Vec::len).sum();
        assert_eq!(all, 94);
        // Same department, same catchment: the table has no site.
        let mut other = *at("chapelle-des-bois");
        other.id = "un-autre-domaine-du-doubs";
        assert_eq!(
            catchment(&other).weights,
            catchment(at("chapelle-des-bois")).weights
        );
    }

    #[test]
    fn condition_bonus_and_season() {
        let base = forecast_day(at("chapelle-des-bois"), Date::new(2027, 1, 17), None);
        let good = Conditions {
            fresh_snow_cm: 15.0,
            sunny: true,
        };
        let f = forecast_day(at("chapelle-des-bois"), Date::new(2027, 1, 17), Some(&good));
        assert_eq!(f.score, base.score + 2.0);
        assert_eq!(f.level, CrowdLevel::Busy);
        assert!(f.reasons.iter().any(|r| r.starts_with("neige fraîche")));
        let thin = Conditions {
            fresh_snow_cm: 5.0,
            sunny: false,
        };
        let f = forecast_day(at("chapelle-des-bois"), Date::new(2027, 1, 17), Some(&thin));
        assert_eq!(f.score, base.score);
        let closed = forecast_day(at("cervieres"), Date::new(2027, 4, 12), None);
        assert_eq!(closed.open, Some(false));
        assert_eq!(
            forecast_day(at("cervieres"), Date::new(2026, 12, 19), None).open,
            Some(true)
        );
        assert_eq!(base.open, None);
    }

    #[test]
    fn dates_across_centuries_and_bad_input() {
        // Leap years of the Gregorian calendar: 2000 (div. by 400) yes, 1900 and 2100 no.
        for (y, m, d) in [
            (1900, 2, 28),
            (1900, 3, 1),
            (2000, 2, 29),
            (2000, 3, 1),
            (2100, 2, 28),
            (2100, 3, 1),
            (2400, 2, 29),
            (1600, 2, 29),
            (1999, 12, 31),
        ] {
            let date = Date::new(y, m, d);
            assert_eq!(Date::from_days(date.days()), date, "{date}");
        }
        assert_eq!(Date::new(2100, 2, 28).add_days(1), Date::new(2100, 3, 1));
        assert_eq!(Date::new(2000, 2, 28).add_days(1), Date::new(2000, 2, 29));
        assert_eq!(Date::new(1900, 2, 28).add_days(1), Date::new(1900, 3, 1));
        assert_eq!(Date::parse("2100-02-29"), None);
        assert_eq!(Date::parse("2027-01-01-05"), None);
        assert_eq!(Date::parse("27-01-01"), None);
        assert_eq!(Date::parse("2027-01-1"), None);
        assert_eq!(Date::parse("2027-01"), None);
        assert_eq!(Date::parse("2027-13-01"), None);
        assert_eq!(Date::parse("2027-01-31"), Some(Date::new(2027, 1, 31)));
    }

    #[test]
    fn level_bounds_and_public_holidays() {
        assert_eq!(CrowdLevel::from_score(1.99), CrowdLevel::Calm);
        assert_eq!(CrowdLevel::from_score(2.0), CrowdLevel::Moderate);
        assert_eq!(CrowdLevel::from_score(3.99), CrowdLevel::Moderate);
        assert_eq!(CrowdLevel::from_score(4.0), CrowdLevel::Busy);
        assert_eq!(CrowdLevel::from_score(5.99), CrowdLevel::Busy);
        assert_eq!(CrowdLevel::from_score(6.0), CrowdLevel::VeryBusy);
        // French holidays are listed once (zone A) and apply to the three zones only.
        let armistice = Date::new(2026, 11, 11);
        assert_eq!(public_holiday(B, armistice).unwrap().name, "Armistice");
        assert_eq!(public_holiday(C, armistice).unwrap().name, "Armistice");
        assert_eq!(
            public_holiday(Fwb, armistice).unwrap().name,
            "Jour de l'Armistice"
        );
        assert!(public_holiday(Vaud, armistice).is_none());
        assert!(public_holiday(Neuchatel, armistice).is_none());
        // A public holiday of a population counts as its weekend: Vaud's 2 January.
        let f = forecast_day(at("chapelle-des-bois"), Date::new(2027, 1, 2), None);
        assert!(
            f.reasons.iter().any(|r| r.starts_with("2 janvier (Vaud)")),
            "{:?}",
            f.reasons
        );
    }

    /// Season 2026-27 for both sites; run with `-- --nocapture` to print the table.
    #[test]
    fn season_2026_27() {
        let (first, last) = (Date::new(2026, 12, 1), Date::new(2027, 4, 30));
        for site in ["cervieres", "chapelle-des-bois"] {
            let days = forecast_range(at(site), first, last);
            assert_eq!(days.len(), 151);
            let mut count = [0usize; 5];
            for f in &days {
                count[f.level as usize] += 1;
                println!(
                    "{site:<17} {} {:<8} {:>4.2} {:<11} {}{}",
                    f.date,
                    crate::text::of(crate::text::Lang::Fr).weekday(usize::from(f.date.weekday())),
                    f.score,
                    f.level.label(),
                    f.reasons.join(", "),
                    match f.open {
                        Some(false) => " [fermé]",
                        _ => "",
                    }
                );
            }
            println!(
                "{site}: calme {}, modéré {}, chargé {}, très chargé {}",
                count[1], count[2], count[3], count[4]
            );
            assert!(count[1] > 0 && count[4] > 0);
        }
    }
}
