//! Texts of the assistant in English.

use chrono::{Datelike, NaiveDate, NaiveDateTime, Timelike};

use super::Texts;
use crate::crowd::{CrowdLevel, Population};
use crate::equipment::Kind as EquipmentKind;
use crate::inputs::LayerName;
use crate::snow_quality::{Quality, SlopeClass, Slot};
use crate::wax::SnowType;

/// English.
pub struct En;

const WEEKDAYS: [&str; 7] = [
    "Monday",
    "Tuesday",
    "Wednesday",
    "Thursday",
    "Friday",
    "Saturday",
    "Sunday",
];
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
const ASPECTS: [&str; 8] = [
    "north",
    "north-east",
    "east",
    "south-east",
    "south",
    "south-west",
    "west",
    "north-west",
];

/// English names of the school and public holidays of the crowd calendars.
const HOLIDAYS: &[(&str, &str)] = &[
    ("2 janvier", "2 January"),
    ("Armistice", "Armistice Day"),
    ("Ascension", "Ascension Day"),
    ("Fête du travail", "Labour Day"),
    ("Instauration de la République", "Republic Day"),
    ("Jour de l'an", "New Year's Day"),
    ("Jour de l'Armistice", "Armistice Day"),
    ("Lundi de Pâques", "Easter Monday"),
    ("Lundi de Pentecôte", "Whit Monday"),
    ("Noël", "Christmas"),
    ("Nouvel an", "New Year's Day"),
    ("pont de l'Ascension", "Ascension long weekend"),
    ("relâches", "February break"),
    ("vacances d'automne", "autumn holidays"),
    ("vacances de détente", "February holidays"),
    ("vacances de la Toussaint", "All Saints' holidays"),
    ("vacances de Noël", "Christmas holidays"),
    ("vacances de Pâques", "Easter holidays"),
    ("vacances de printemps", "spring holidays"),
    ("vacances d'hiver", "winter holidays"),
    ("vacances du 1er mars", "1 March holidays"),
    ("Vendredi saint", "Good Friday"),
    ("Victoire 1945", "Victory in Europe Day"),
];

/// English of a holiday name, `None` when the table lacks it.
pub fn holiday_en(fr: &str) -> Option<&'static str> {
    HOLIDAYS.iter().find(|(f, _)| *f == fr).map(|(_, e)| *e)
}

impl Texts for En {
    fn num(&self, x: f64, decimals: usize) -> String {
        format!("{x:.decimals$}")
    }

    fn date(&self, d: NaiveDate) -> String {
        format!(
            "{} {} {} {}",
            WEEKDAYS[d.weekday().num_days_from_monday() as usize],
            d.day(),
            MONTHS[d.month0() as usize],
            d.year()
        )
    }

    fn weekday(&self, monday0: usize) -> &'static str {
        WEEKDAYS[monday0 % 7]
    }

    fn clock(&self, t: NaiveDateTime) -> String {
        format!("{}:{:02}", t.hour(), t.minute())
    }

    fn hour(&self, t: NaiveDateTime) -> String {
        format!("{}:00", t.hour())
    }

    fn gusts_up_to(&self, kmh: f64) -> String {
        format!("gusts up to {kmh:.0} km/h")
    }

    fn wind_light(&self, g: f64) -> String {
        format!("light wind (gusts {g:.0} km/h)")
    }

    fn wind_moderate(&self, g: f64) -> String {
        format!("moderate wind (gusts {g:.0} km/h)")
    }

    fn very_cold(&self, chill: &str) -> String {
        format!("very cold, feels like {chill} °C")
    }

    fn cold(&self, chill: &str) -> String {
        format!("cold, feels like {chill} °C")
    }

    fn temperature(&self, mean: &str, chill: &str) -> String {
        format!("{mean} °C on average, feels like {chill} °C")
    }

    fn fog_hours(&self, hours: usize) -> String {
        format!("fog {hours} h out of 7")
    }

    fn day_snowfall(&self, cm: f64) -> String {
        format!("{} cm of snow during the day", self.num(cm, 0))
    }

    fn rain(&self, mm: f64) -> String {
        format!("rain ({} mm)", self.num(mm, 1))
    }

    fn sunny(&self) -> &'static str {
        "sunny"
    }

    fn snow_estimate(&self, depth: f64, map: NaiveDate, added: f64, share_pct: f64) -> String {
        let added = if added >= 0.5 {
            format!(", {} cm fallen since", self.num(added, 0))
        } else {
            String::new()
        };
        format!(
            "{} cm of estimated snow (median, map of {}{added}, {} % of the points ≥ 20 cm)",
            self.num(depth, 0),
            map.format("%d/%m"),
            share_pct
        )
    }

    fn snow_verdict(&self, base: &str, verdict: u8) -> String {
        match verdict {
            0 => format!("{base}: too little to groom"),
            1 => format!("{base}: borderline"),
            _ => format!("{base}: enough"),
        }
    }

    fn snow_unknown(&self) -> &'static str {
        "estimated snow unknown here (outside the site's map or no line): to be checked"
    }

    fn fresh_snow_24h(&self, cm: f64) -> String {
        format!("{} cm of fresh snow in 24 h", self.num(cm, 0))
    }

    fn surface(&self, q: Quality) -> String {
        format!("surface: {}", self.quality(q))
    }

    fn no_snow_map(&self) -> &'static str {
        "no snow map of this day or of the 4 days before"
    }

    fn weather_from(&self, point: &str, km: f64) -> String {
        format!("weather of the point {point}, {} km away", self.num(km, 0))
    }

    fn no_forecast(&self) -> &'static str {
        "no weather forecast for this day"
    }

    fn crowd(&self, level: CrowdLevel, reasons: &[String]) -> String {
        let why = if reasons.is_empty() {
            String::new()
        } else {
            format!(" ({})", reasons.join(", "))
        };
        format!("crowd level {}{why}", self.crowd_level(level))
    }

    fn as_the_crow_flies(&self, km: f64) -> String {
        format!("{} km as the crow flies", self.num(km, 0))
    }

    fn avalanche_km(&self, km: f64, level: Option<u8>) -> String {
        let km = self.num(km, 1);
        match level {
            Some(l) if l >= 3 => format!("{km} km in known avalanche paths, BERA {l} out of 5"),
            Some(l) => format!("{km} km in known avalanche paths (BERA {l} out of 5)"),
            None => format!("{km} km in known avalanche paths, BERA not available"),
        }
    }

    fn calendars_2026_27(&self) -> &'static str {
        "school calendars known for 2026-27 only: the crowd of this day counts weekends only"
    }

    fn status_unknown_day(&self) -> &'static str {
        "trail status unknown for this day"
    }

    fn status_last_known(&self, when: &str) -> String {
        format!("trail status: last known ({when})")
    }

    fn sources_disagree(&self, what: &str) -> String {
        format!("sources disagree: {what}")
    }

    fn open_without_snow(&self) -> &'static str {
        "open according to the source, but too little estimated snow: probably an off-season \
         status, to be checked"
    }

    fn open(&self) -> &'static str {
        "open"
    }

    fn closed(&self) -> &'static str {
        "closed"
    }

    fn status_unknown(&self) -> &'static str {
        "status unknown"
    }

    fn last_known_suffix(&self) -> &'static str {
        " (last known status)"
    }

    fn no_status_source(&self) -> &'static str {
        "no trail status source for this area"
    }

    fn only_itinerary_closed(&self, suffix: &str) -> String {
        format!("the only known route is closed{suffix}")
    }

    fn none_open(&self, n: usize, suffix: &str) -> String {
        format!("none of the {n} known routes is open{suffix}")
    }

    fn itineraries_open(
        &self,
        open: usize,
        known: usize,
        open_km: f64,
        total_km: f64,
        suffix: &str,
        no_snow: bool,
    ) -> String {
        format!(
            "{open} {} out of {known} ({} km out of {}){suffix}{}",
            if open == 1 {
                "route open"
            } else {
                "routes open"
            },
            self.num(open_km, 0),
            self.num(total_km, 0),
            if no_snow {
                " but too little estimated snow: to be checked"
            } else {
                ""
            }
        )
    }

    fn area_closed_dates(&self) -> &'static str {
        "area closed on this date (opening dates published by the town)"
    }

    fn nordic_km(&self, km: f64) -> String {
        format!("{} km of nordic trails (OSM)", self.num(km, 0))
    }

    fn candidate(&self, rank: usize, name: &str, score: f64, reasons: &[String]) -> String {
        format!(
            "{rank}. {name} (score {}): {}.",
            self.num(score, 2),
            reasons.join("; ")
        )
    }

    fn reason_points(&self, text: &str, points: f64) -> String {
        format!("{text} [{points:+.2}]")
    }

    fn briefing_title(&self, site: &str, date: NaiveDate) -> String {
        format!("{site} — {}", self.date(date))
    }

    fn briefing_bera(&self, level: u8) -> String {
        format!("BERA: avalanche danger {level} out of 5 (read the full bulletin).")
    }

    fn briefing_domains(&self) -> &'static str {
        "Areas:"
    }

    fn briefing_all_closed(&self, n: usize) -> String {
        format!("Routes: the {n} known routes are closed.")
    }

    fn briefing_itineraries(&self) -> &'static str {
        "Routes (the first 5 not closed):"
    }

    fn briefing_snow_quality(&self, point: &str, summary: &[String]) -> String {
        format!("Snow quality ({point}): {}", summary.join(" "))
    }

    fn briefing_sunset(&self, at: NaiveDateTime) -> String {
        format!("Sunset {}:{:02}.", at.hour(), at.minute())
    }

    fn briefing_data(&self, notes: &[String]) -> String {
        format!("Data: {}.", notes.join("; "))
    }

    fn crowd_level(&self, level: CrowdLevel) -> &'static str {
        match level {
            CrowdLevel::Calm => "calm",
            CrowdLevel::Moderate => "moderate",
            CrowdLevel::Busy => "busy",
            CrowdLevel::VeryBusy => "very busy",
        }
    }

    fn holiday(&self, fr: &'static str) -> &'static str {
        holiday_en(fr).unwrap_or(fr)
    }

    fn population(&self, p: Population) -> &'static str {
        match p {
            Population::FranceZoneA => "zone A",
            Population::FranceZoneB => "zone B",
            Population::FranceZoneC => "zone C",
            Population::Fwb => "Belgium (FWB)",
            Population::Vaud => "Vaud",
            Population::Neuchatel => "Neuchatel",
        }
    }

    fn france(&self) -> &'static str {
        "France"
    }

    fn holiday_for(&self, name: &str, who: &[&str]) -> String {
        format!("{name} ({})", who.join(", "))
    }

    fn crowd_fresh_snow(&self, cm: f64) -> String {
        format!("fresh snow ({cm:.0} cm)")
    }

    fn crowd_sun(&self) -> &'static str {
        "sun"
    }

    fn crowd_area_closed(&self) -> &'static str {
        "area closed (published opening dates)"
    }

    fn quality(&self, q: Quality) -> &'static str {
        match q {
            Quality::Fresh => "fresh snow",
            Quality::Windblown => "wind-blown fresh snow",
            Quality::OldDry => "old cold dry snow",
            Quality::Transformed => "transformed snow softening (corn)",
            Quality::Wet => "wet heavy snow",
            Quality::Slush => "slush",
            Quality::Hard => "hard refrozen snow",
            Quality::Icy => "thick refrozen crust, icy patches",
        }
    }

    fn aspect(&self, octant: usize) -> &'static str {
        ASPECTS[octant % 8]
    }

    fn slope_class(&self, c: &SlopeClass) -> String {
        match c.octant() {
            None => "flat".into(),
            Some(o) => format!("{}, {}° slope", self.aspect(o), c.slope_deg.round()),
        }
    }

    fn slot(&self, s: &Slot) -> String {
        format!("from {}:00 to {}:00", s.start.hour(), s.end.hour())
    }

    fn sq_no_snow(&self, cm: f64, elevation: f64) -> String {
        format!(
            "No or too little estimated snow at the point ({cm} cm, {elevation} m): snow \
             quality not assessed."
        )
    }

    fn sq_flat(&self, elevation: f64, q: Quality) -> String {
        format!(
            "On the flat ({elevation} m): {} between 10:00 and 15:00.",
            self.quality(q)
        )
    }

    fn sq_best(&self, class: &str, slot: &str, q: Quality) -> String {
        format!("Best slot: {class} {slot} ({}).", self.quality(q))
    }

    fn sq_aspect_at(&self, aspect: &str, at: NaiveDateTime) -> String {
        format!("{aspect} {}", self.hour(at))
    }

    fn sq_aspect_slope_from(&self, aspect: &str, slope: f64, at: NaiveDateTime) -> String {
        format!("{aspect} {slope}° from {}", self.hour(at))
    }

    fn sq_softens(&self, list: &[String]) -> String {
        format!("20° slopes: softening around {}.", list.join(", "))
    }

    fn sq_heavy(&self, list: &[String]) -> String {
        format!("Heavy snow: {}.", list.join(", "))
    }

    fn sq_never_softens(&self, list: &[&str]) -> String {
        format!("Does not soften (20°): {}.", list.join(", "))
    }

    fn sq_model(&self) -> &'static str {
        "Simple model (temperature, sun, cloud, wind): no shade of the relief, no grooming; \
         check on the ground."
    }

    fn group_label(&self, p: &skimap_core::calibration::GroupProfile) -> String {
        use skimap_core::calibration::Level;
        let level = match p.level {
            Level::Beginner => "beginner",
            Level::Intermediate => "intermediate",
            Level::Expert => "expert",
        };
        let mut ages: Vec<u8> = p.children.clone();
        ages.sort_unstable();
        let words: Vec<String> = ages.iter().map(u8::to_string).collect();
        let kids = match words.as_slice() {
            [] => String::new(),
            [one] => format!(", a child of {one}"),
            [rest @ .., last] => format!(", children of {} and {last}", rest.join(", ")),
        };
        format!(
            "“{}”: {} adult{}{kids}, {}, {level} level",
            p.name,
            p.adults,
            if p.adults > 1 { "s" } else { "" },
            self.style(p.style)
        )
    }

    fn style(&self, s: skimap_core::routing::Style) -> &'static str {
        match s {
            skimap_core::routing::Style::Classic => "classic",
            skimap_core::routing::Style::Skating => "skating",
        }
    }

    fn learned(&self, outings: u32) -> String {
        match outings {
            0 => "prior".to_owned(),
            1 => "one outing learned".to_owned(),
            n => format!("{n} outings learned"),
        }
    }

    fn hm(&self, seconds: f64) -> String {
        let m = (seconds / 60.0).round() as i64;
        if m < 60 {
            format!("{m} min")
        } else {
            format!("{} h {:02} min", m / 60, m % 60)
        }
    }

    fn s_no_graph(&self) -> &'static str {
        "no trail graph (routes layer)"
    }

    fn s_no_trail_near(&self, start: &str) -> String {
        format!("no trail within 300 m of {start}")
    }

    fn s_unnamed_loop(&self) -> &'static str {
        "unnamed loop"
    }

    fn s_time_group_slow(
        &self,
        median: &str,
        group: &str,
        lo: &str,
        hi: &str,
        learned: &str,
        plan: &str,
        style: &str,
        model: &str,
    ) -> String {
        format!(
            "about {median} for {group} ({lo} to {hi}, {learned}; planned on {plan}, the slow side: 8 chances out of 10 \
             to have finished; C5 model, {style}: {model})"
        )
    }

    fn s_time_group(
        &self,
        median: &str,
        group: &str,
        lo: &str,
        hi: &str,
        learned: &str,
        style: &str,
        model: &str,
    ) -> String {
        format!("about {median} for {group} ({lo} to {hi}, {learned}; C5 model, {style}: {model})")
    }

    fn s_time_model(&self, model: &str, style: &str) -> String {
        format!("about {model} (C5 model, {style})")
    }

    fn s_loop(&self, km: f64, gain: f64, time: &str, back: NaiveDateTime) -> String {
        format!(
            "{} km, {gain} m climb, {time}, back around {}",
            self.num(km, 1),
            self.clock(back)
        )
    }

    fn s_out_and_back(&self, pct: f64) -> String {
        format!("{pct} % of the route out and back on the same way")
    }

    fn s_eight(&self, km: f64) -> String {
        format!(
            "figure of eight: two loops from the start, back past it after {} km",
            self.num(km, 1)
        )
    }

    fn s_closed_km(&self, km: f64) -> String {
        format!("{} km on closed trails", self.num(km, 1))
    }

    fn s_open_km(&self, km: f64) -> String {
        format!("{} km on open trails", self.num(km, 1))
    }

    fn s_unknown_km(&self, km: f64) -> String {
        format!("{} km with no known status", self.num(km, 1))
    }

    fn s_status_unknown(&self) -> &'static str {
        "trail status unknown"
    }

    fn s_bera(&self, level: u8) -> String {
        format!("BERA {level} out of 5")
    }

    fn s_no_bera_range(&self) -> &'static str {
        "no BERA for this range"
    }

    fn s_bera_unavailable(&self) -> &'static str {
        "BERA not available"
    }

    fn s_exposed(&self, km: f64, why: &str) -> String {
        format!("{} km in avalanche terrain (model), {why}", self.num(km, 1))
    }

    fn s_harder(&self, km: f64) -> String {
        format!("{} km on trails harder than asked", self.num(km, 1))
    }

    fn s_trails(&self, names: &[String]) -> String {
        format!("trails: {}", names.join(", "))
    }

    fn s_if_all_goes_well(&self, median: &str, slow: &str) -> String {
        format!(
            "if all goes well: {median} at the group's median pace, but {slow} on the slow side \
             (beyond the time asked)"
        )
    }

    fn s_no_way_back(&self, n: usize) -> String {
        format!("{n} turning points with no way back to the start (one-way trails of the C1 graph)")
    }

    fn s_evaluated(
        &self,
        evaluated: usize,
        short: usize,
        long: usize,
        group: Option<&str>,
    ) -> String {
        format!(
            "{evaluated} loops evaluated, {short} too short and {long} too long \
             (outside 75 to 110 % of the time asked{})",
            group
                .map(|l| format!(", at the group's pace: {l}"))
                .unwrap_or_default()
        )
    }

    fn s_too_late(&self, n: usize, limit: Option<NaiveDateTime>) -> String {
        format!(
            "{n} loops set aside: back after {}",
            limit.map(|l| self.clock(l)).unwrap_or_default()
        )
    }

    fn s_none_fits(&self) -> &'static str {
        "no loop fits the time and the return time asked"
    }

    fn s_no_real_loop(
        &self,
        lo: &str,
        hi: &str,
        start: &str,
        one: bool,
        longest: Option<(f64, &str)>,
    ) -> String {
        format!(
            "no real loop found from {lo} to {hi} from {start}: {} partly out and back \
             on the same way, the only option of the network here{}",
            if one {
                "the loop proposed is"
            } else {
                "the loops proposed are"
            },
            longest
                .map(|(km, t)| format!(
                    "; the longest real loop found is {} km, about {t}",
                    self.num(km, 1)
                ))
                .unwrap_or_default()
        )
    }

    fn s_no_snow_all(&self) -> &'static str {
        "too little estimated snow on every loop: nothing is skiable this day"
    }

    fn s_closed_most(&self) -> &'static str {
        "trails closed on most of each loop"
    }

    fn s_title(
        &self,
        label: &str,
        start: &str,
        date: NaiveDate,
        at: NaiveDateTime,
        deadline: Option<NaiveDateTime>,
        group: Option<&str>,
    ) -> String {
        format!(
            "{label} from {start} on {} at {}{}{}",
            self.date(date),
            self.clock(at),
            deadline
                .map(|d| format!(" (C7 return limit: {})", self.clock(d)))
                .unwrap_or_default(),
            group
                .map(|l| format!(", group time: {l}"))
                .unwrap_or_default()
        )
    }

    fn s_line(
        &self,
        rank: usize,
        hopeful: bool,
        label: &str,
        score: f64,
        why: &[String],
    ) -> String {
        format!(
            "{rank}. {}{label} (score {}): {}.",
            if hopeful { "if all goes well: " } else { "" },
            self.num(score, 2),
            why.join("; ")
        )
    }

    fn s_limits(&self, notes: &[String]) -> String {
        format!("Limits: {}.", notes.join("; "))
    }

    fn s_unreachable(&self, nodes: usize, of: usize, km: f64, start: &str) -> String {
        format!(
            "{nodes} nodes out of {of} ({} km of trails) cannot be reached from {start}: separate \
             network or one-way trails in the trail graph (C1)",
            self.num(km, 1)
        )
    }

    fn bt_surface(&self, class: &str, qualities: &[&str]) -> String {
        format!("{class}: {}", qualities.join(" then "))
    }

    fn bt_sep(&self) -> &'static str {
        "; "
    }

    fn bt_gusts(&self, kmh: f64) -> String {
        format!("gusts up to {kmh:.0} km/h")
    }

    fn bt_fog(&self, hours: f64) -> String {
        format!("fog {hours} h")
    }

    fn bt_snowfall(&self) -> &'static str {
        "snowfall"
    }

    fn bt_rain(&self) -> &'static str {
        "rain"
    }

    fn bt_cold(&self, chill: &str) -> String {
        format!("feels like down to {chill} °C")
    }

    fn bt_sun(&self) -> &'static str {
        "sun"
    }

    fn bt_crowd(&self, level: CrowdLevel) -> String {
        format!(
            "{} day: assumed peak from 10:30 to 14:00",
            self.crowd_level(level)
        )
    }

    fn bt_near_deadline(&self, deadline: NaiveDateTime) -> String {
        format!(
            "back less than 30 min before the time limit ({})",
            self.clock(deadline)
        )
    }

    fn bt_other_start(&self, start: NaiveDateTime, lost: &[String]) -> String {
        format!("starting at {}: {}", self.clock(start), lost.join(", "))
    }

    fn bt_best(
        &self,
        start: NaiveDateTime,
        back: NaiveDateTime,
        minutes: f64,
        why: &[String],
        others: &[String],
    ) -> String {
        let vs = if others.is_empty() {
            String::new()
        } else {
            format!("; {}", others.join("; "))
        };
        format!(
            "Best start around {} (back around {}, {} min outing): {}{vs}.",
            self.clock(start),
            self.clock(back),
            minutes.round(),
            if why.is_empty() {
                "no time slot stands out".into()
            } else {
                why.join(", ")
            }
        )
    }

    fn bt_no_forecast(&self) -> &'static str {
        "No best time: no hourly weather forecast for this day in the pack."
    }

    fn bt_none(&self, minutes: f64, deadline: Option<NaiveDateTime>) -> String {
        format!(
            "No time slot: a {} min outing does not end before the time limit{}.",
            minutes.round(),
            deadline
                .map(|d| format!(" ({})", self.clock(d)))
                .unwrap_or_default()
        )
    }

    fn wax_snow_type(&self, t: SnowType) -> &'static str {
        match t {
            SnowType::New => "fresh snow",
            SnowType::Old => "old fine-grained snow",
            SnowType::CoarseFrozen => "coarse refrozen snow, crust or ice",
            SnowType::TransformedMoist => "moist transformed snow",
            SnowType::Wet => "wet snow",
        }
    }

    fn wax_window(
        &self,
        hours: (u32, u32),
        air: &str,
        snow: &str,
        snow_type: SnowType,
        kick: &str,
        glide: (&str, &str),
        glide_alt: Option<(&str, &str)>,
    ) -> String {
        let galt = glide_alt
            .map(|(p, b)| format!(" or {p} ({b})"))
            .unwrap_or_default();
        format!(
            "{}:00–{}:00, air {air} °C, snow ≈ {snow} °C, {}: classic, {kick}; glide \
             for classic and skating {} ({}){galt}.",
            hours.0,
            hours.1,
            self.wax_snow_type(snow_type),
            glide.0,
            glide.1
        )
    }

    fn wax_kick(&self, kick: (&str, &str), alt: Option<(&str, &str)>) -> String {
        let alt = alt
            .map(|(p, b)| format!(" or {p} ({b})"))
            .unwrap_or_default();
        format!("grip {} ({}){alt}", kick.0, kick.1)
    }

    fn wax_kick_uncertain(
        &self,
        skins: Option<&str>,
        kick: (&str, &str),
        alt: Option<(&str, &str)>,
    ) -> String {
        let alt = alt
            .map(|(p, b)| format!(" or {p} ({b})"))
            .unwrap_or_default();
        format!(
            "grip uncertain, {}; otherwise {} ({}){alt}",
            skins.unwrap_or("waxless or skin skis"),
            kick.0,
            kick.1
        )
    }

    fn wax_model(&self, snow_type: SnowType, mean: f64, min: f64, max: f64) -> String {
        format!(
            "{} (A3 snow quality model, flat), air {mean:+.1} °C on average ({min:+.1} to {max:+.1})",
            self.wax_snow_type(snow_type),
        )
    }

    fn wax_colder_than_all(&self) -> &'static str {
        "colder than every range: V05 Polar, at its limit"
    }

    fn wax_warmer_than_hard(&self) -> &'static str {
        "warmer than the hard waxes for this snow: universal klister"
    }

    fn wax_coarse_frozen(&self) -> &'static str {
        "coarse refrozen snow: klister (Toko, Swix)"
    }

    fn wax_fresh_near_zero(&self) -> &'static str {
        "fresh snow around 0 °C: grip wax clogs or slips"
    }

    fn wax_refrozen_then_wet(&self, first: Quality, last: Quality) -> String {
        format!(
            "{} at the start of the window, {} after: no wax holds the whole window",
            self.quality(first),
            self.quality(last)
        )
    }

    fn wax_crosses_zero(&self, min: f64, max: f64) -> String {
        format!("the temperature goes from {min:+.0} to {max:+.0} °C in the window")
    }

    fn wax_wet_falling(&self) -> &'static str {
        "rain or wet snow falling"
    }

    fn wax_skins(&self, why: &str) -> String {
        format!("waxless or skin skis advised ({why})")
    }

    fn wax_glide(&self, air: f64, snow: f64) -> String {
        format!("glide: air {air:+.1} °C (Swix tables), estimated snow {snow:+.1} °C (Toko tables)")
    }

    fn wax_humidity(&self, pct: f64) -> String {
        format!("humidity {pct:.0} %")
    }

    fn wax_humidity_unknown(&self) -> &'static str {
        "humidity unknown (not in the weather layer)"
    }

    fn wax_day(&self, morning: &str, afternoon: Option<&str>) -> String {
        match afternoon {
            Some(a) => format!("Morning {morning} Afternoon {a}"),
            None => format!("Morning {morning} Same wax in the afternoon."),
        }
    }

    fn eq_label(&self, k: EquipmentKind) -> &'static str {
        match k {
            EquipmentKind::AvalancheKit => "transceiver, shovel and probe",
            EquipmentKind::Headlamp => "headlamp",
            EquipmentKind::WarmLayer => "an extra warm layer",
            EquipmentKind::FaceProtection => "mittens and face protection",
            EquipmentKind::PowerBank => "power bank",
            EquipmentKind::TellSomeone => "tell someone close the route and the return time",
            EquipmentKind::Compass => "compass",
            EquipmentKind::SunProtection => "sunglasses and sunscreen",
            EquipmentKind::WaxlessSkis => "waxless or skin skis",
        }
    }

    fn eq_exposed(&self, km: f64) -> String {
        format!(
            "{} km in terrain exposed to avalanches (model)",
            self.num(km, 1)
        )
    }

    fn eq_clpa(&self, km: f64) -> String {
        format!("{} km in known avalanche paths (CLPA)", self.num(km, 1))
    }

    fn eq_offpiste_bera(&self, level: u8) -> String {
        format!("off-piste with a BERA of {level} out of 5")
    }

    fn eq_back_after_sunset(&self, back: NaiveDateTime, sunset: NaiveDateTime) -> String {
        format!(
            "back planned around {}, sunset at {}",
            self.clock(back),
            self.clock(sunset)
        )
    }

    fn eq_start_before_sunrise(&self, start: NaiveDateTime, sunrise: NaiveDateTime) -> String {
        format!(
            "start at {}, before sunrise ({})",
            self.clock(start),
            self.clock(sunrise)
        )
    }

    fn eq_chill(&self, chill: &str) -> String {
        format!("feels like down to {chill} °C")
    }

    fn eq_no_network(&self, km: f64, pct: f64) -> String {
        format!(
            "{} km without any mobile network, not even for 112 ({pct} % of the route)",
            self.num(km, 1)
        )
    }

    fn eq_offpiste(&self) -> &'static str {
        "off-piste outing"
    }

    fn eq_long(&self, minutes: f64) -> String {
        format!("outing of more than 3 h ({minutes} min estimated)")
    }

    fn eq_fog(&self, hours: usize) -> String {
        format!("fog forecast ({hours} h during the outing)")
    }

    fn eq_clear_sky(&self, cloud_pct: f64) -> String {
        format!("clear sky ({cloud_pct:.0} % cloud)")
    }

    fn eq_kick_unreliable(&self) -> &'static str {
        "grip wax uncertain that day (A2 wax)"
    }

    fn eq_nothing(&self, outing: &str) -> String {
        format!("{outing}: nothing special to add to the usual equipment.")
    }

    fn eq_take(&self, outing: &str, items: &[(&str, &str)]) -> String {
        format!(
            "{outing} — take: {}.",
            items
                .iter()
                .map(|(l, r)| format!("{l} ({r})"))
                .collect::<Vec<_>>()
                .join("; ")
        )
    }

    fn sy_altitude(&self, loc: &str) -> String {
        if let Some(a) = loc.strip_prefix('>') {
            format!("above {a} m")
        } else if let Some(a) = loc.strip_prefix('<') {
            format!("below {a} m")
        } else {
            loc.to_owned()
        }
    }

    fn sy_risk_other(&self, loc: &str, level: u8) -> String {
        format!(" {} ({level} out of 5 lower down)", self.sy_altitude(loc))
    }

    fn sy_aspects(&self, aspects: &[&str]) -> String {
        format!(", {} slopes the most exposed", aspects.join(", "))
    }

    fn sy_read_bulletin(&self) -> &'static str {
        ": read the whole bulletin before you go"
    }

    fn sy_no_bera_jura(&self, no_bulletin: &str) -> String {
        format!(
            "{no_bulletin} (Jura): avalanches remain possible on steep slopes, the SLF \
             bulletin covers the nearby Swiss Jura"
        )
    }

    fn sy_no_bera(&self, no_bulletin: &str) -> String {
        format!("{no_bulletin}: avalanches remain possible on steep slopes")
    }

    fn sy_bera_missing(&self, massif: Option<&str>, loaded: bool) -> String {
        let massif = massif.map_or_else(|| "massif".to_owned(), |m| format!("{m} massif"));
        if loaded {
            format!(
                "Today's BERA of the {massif} is not available offline: read it before you \
                 go, the avalanche risk is never zero in the mountains"
            )
        } else {
            format!(
                "Today's BERA of the {massif} is not loaded: read it before you go, the \
                 avalanche risk is never zero in the mountains"
            )
        }
    }

    fn sy_clpa(&self, km: f64, paths: usize) -> String {
        format!(
            "; your route crosses {} km of {paths} known avalanche paths (CLPA)",
            self.num(km, 1)
        )
    }

    fn sy_clpa_none(&self) -> &'static str {
        "; your route crosses no known avalanche path (CLPA), which does not rule out an \
         avalanche elsewhere"
    }

    fn sy_clpa_no_map(&self) -> &'static str {
        "; no map of the known avalanche paths (CLPA) in this massif"
    }

    fn sy_no_weather(&self) -> &'static str {
        "No offline weather for this day."
    }

    fn sy_no_forecast(&self, point: &str) -> String {
        format!("No offline weather forecast for this day at the point {point}.")
    }

    fn sy_temperature(&self, min: &str, max: Option<&str>) -> String {
        match max {
            None => format!("{min} °C"),
            Some(max) => format!("from {min} to {max} °C"),
        }
    }

    fn sy_chill(&self, chill: &str) -> String {
        format!("feels like {chill} °C")
    }

    fn sy_wind_light(&self) -> &'static str {
        "light wind"
    }

    fn sy_fog(&self, hours: usize) -> String {
        format!("fog possible ({hours} h)")
    }

    fn sy_snow(&self, cm: f64) -> String {
        format!("{} cm of snow", self.num(cm, 0))
    }

    fn sy_freezing(&self, m: f64) -> String {
        format!("freezing level around {m} m")
    }

    fn sy_clear(&self) -> &'static str {
        "clear sky"
    }

    fn sy_overcast(&self) -> &'static str {
        "overcast"
    }

    fn sy_weather(&self, point: &str, elevation: f64, parts: &[String]) -> String {
        format!(
            "Weather from 9:00 to 16:00 ({point}, {elevation} m): {}.",
            parts.join(", ")
        )
    }

    fn sy_verdict(
        &self,
        topic: skimap_zones::rules::Topic,
        verdict: skimap_core::zone_eval::Verdict,
        stoves: bool,
    ) -> Option<&'static str> {
        use skimap_core::zone_eval::Verdict;
        use skimap_zones::rules::Topic;
        Some(match (topic, verdict) {
            (Topic::Access, Verdict::Forbidden) => "no entry",
            (Topic::Access, Verdict::ForbiddenInPlaces) => "no entry in places",
            (Topic::Access, Verdict::AvoidRecommended) => "wildlife quiet zone to avoid",
            (Topic::LeaveRoutes, Verdict::Forbidden) => "leaving the marked routes forbidden",
            (Topic::LeaveRoutes, Verdict::ForbiddenInPlaces) => {
                "leaving the trails forbidden in places (private land)"
            }
            (Topic::LeaveRoutes, Verdict::AvoidRecommended) => "stay on the routes",
            (Topic::Bivouac, Verdict::Forbidden) => "bivouac forbidden",
            (Topic::Fire, Verdict::Forbidden) if stoves => "fire forbidden, stoves included",
            (Topic::Fire, Verdict::Forbidden) => "fire forbidden",
            _ => return None,
        })
    }

    fn sy_night(&self, words: &str) -> String {
        format!("at night, {words}")
    }

    fn sy_zones_more(&self, first: &str, second: &str, others: usize) -> String {
        format!(" ({first}, {second} and {others} others)")
    }

    fn sy_until(&self, day: u8, month: u8) -> String {
        format!(" until {day} {}", skimap_voice::en::month_name(month))
    }

    fn sy_rules_not_loaded(&self) -> &'static str {
        "rules of the protected zones not loaded"
    }

    fn sy_no_rule(&self) -> &'static str {
        "no known access rule active this day"
    }

    fn sy_not_covered(&self) -> &'static str {
        " (outside France and Switzerland, no known rule)"
    }

    fn sy_no_network_km(&self, km: f64) -> String {
        format!(
            "; {} km without any mobile network, not even for 112",
            self.num(km, 1)
        )
    }

    fn sy_network_everywhere(&self) -> &'static str {
        "; mobile network announced on the whole route (operator maps)"
    }

    fn sy_no_network_share(&self, pct: f64) -> String {
        format!("; {pct} % of the trails without any mobile network, not even for 112")
    }

    fn sy_rules(&self, route: bool, rules: &str, not_covered: &str, network: &str) -> String {
        let place = if route {
            "On your route"
        } else {
            "On the trails of the site"
        };
        format!("{place}: {rules}{not_covered}{network}.")
    }

    fn al_wind_rising(&self, gust_kmh: f64, before_kmh: f64) -> String {
        format!(
            "the wind is rising: gusts of {gust_kmh:.0} km/h within the hour (before: {before_kmh:.0})"
        )
    }

    fn al_fog(&self) -> &'static str {
        "fog forecast within the hour"
    }

    fn al_snow(&self, cm_h: f64) -> String {
        format!("snowfall within the hour ({cm_h:.1} cm/h)")
    }

    fn al_cold(&self, temperature: &str, chill: &str) -> String {
        format!("the temperature is dropping: {temperature} °C, feels like {chill} °C within 3 h")
    }

    fn al_turn_now(&self, late_min: f64, pace: f64, deadline: Option<NaiveDateTime>) -> String {
        format!(
            "{late_min} min late (pace × {pace:.2}): turn back now to be back before {}",
            deadline.map(|d| self.clock(d)).unwrap_or_default()
        )
    }

    fn al_turn_advanced(
        &self,
        late_min: f64,
        pace: f64,
        deadline: Option<NaiveDateTime>,
        turn: NaiveDateTime,
    ) -> String {
        format!(
            "{late_min} min late (pace × {pace:.2}): the loop no longer ends before {}; turn back at {} at the latest",
            deadline.map(|d| self.clock(d)).unwrap_or_default(),
            self.clock(turn)
        )
    }

    fn layer_name(&self, l: LayerName) -> &'static str {
        match l {
            LayerName::Weather => "hourly weather",
            LayerName::Snow => "snow map",
            LayerName::Bera => "BERA",
            LayerName::Status => "trail status",
            LayerName::Domains => "nordic areas",
            LayerName::Pistes => "trails",
            LayerName::Clpa => "historic avalanche paths",
            LayerName::NoNetwork => "zones without network",
            LayerName::Zones => "zones and rules",
            LayerName::Route => "planned route",
            LayerName::Graph => "trail graph",
            LayerName::Exposure => "avalanche exposure",
        }
    }

    fn layer_missing(&self, l: LayerName, why: &str) -> String {
        format!("{}: {why}", self.layer_name(l))
    }

    fn layer_personal(&self) -> &'static str {
        "personal-use-only source, not in the public pack"
    }
}
