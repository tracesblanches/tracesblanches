//! Phrase bank: one announcement per event of the core (guidance, alerts, briefing).
//!
//! [`Event::announce`] turns an event into an [`Announcement`]: a list of clips with the
//! pause after each. The same announcement gives the text for the fallback voice
//! ([`Announcement::text`]) and the clips to concatenate ([`Announcement::segments`]).

use serde::{Deserialize, Serialize};

use crate::clips::{self, Clip, DayMonth, Direction, Fixed};
use crate::names::{Colour, PisteName, name_clip_id, name_clip_text};
use crate::quantity::{Clock, floor_duration, round_distance, round_duration};

/// Pause after a clip.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Pause {
    /// Inside one message ("…, tournez à gauche." | "Greffier, piste bleue."): 150 ms.
    Short,
    /// Between two messages: 400 ms.
    Long,
}

impl Pause {
    /// Silence in milliseconds.
    pub fn ms(self) -> u32 {
        match self {
            Pause::Short => 150,
            Pause::Long => 400,
        }
    }
}

/// One clip of an announcement and the pause after it (ignored after the last clip).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Segment {
    /// Clip id and text.
    pub clip: Clip,
    /// Pause after the clip.
    pub pause: Pause,
}

/// An announcement: clips to play in order.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Announcement {
    /// Clips with their pauses.
    pub segments: Vec<Segment>,
}

/// A clip whose text is longer than the chunk budget.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("clip `{id}` has {tokens} tokens, more than {max}: {text}")]
pub struct TooLong {
    /// Clip id.
    pub id: String,
    /// Clip text.
    pub text: String,
    /// Its token count.
    pub tokens: usize,
    /// Budget.
    pub max: usize,
}

impl Announcement {
    fn push(&mut self, clip: Clip, pause: Pause) {
        self.segments.push(Segment { clip, pause });
    }

    /// Appends `other` after a long pause.
    pub fn then(mut self, other: Announcement) -> Self {
        if let Some(last) = self.segments.last_mut() {
            last.pause = Pause::Long;
        }
        self.segments.extend(other.segments);
        self
    }

    /// Text for the fallback voice: the clip texts in order.
    pub fn text(&self) -> String {
        self.segments
            .iter()
            .map(|s| s.clip.text.as_str())
            .collect::<Vec<_>>()
            .join(" ")
    }

    /// Ids of the clips, in order.
    pub fn clip_ids(&self) -> Vec<&str> {
        self.segments.iter().map(|s| s.clip.id.as_str()).collect()
    }

    /// Text chunks of at most `max_tokens` tokens (`tokens` counts them, e.g.
    /// [`crate::tokenizer::PocketText::count`]), cut at sentence ends: every clip ends a
    /// sentence, and consecutive clips are grouped greedily, as pockettts.cpp does.
    pub fn chunks(
        &self,
        max_tokens: usize,
        tokens: impl Fn(&str) -> usize,
    ) -> Result<Vec<String>, TooLong> {
        let mut out: Vec<String> = Vec::new();
        let (mut cur, mut cur_n) = (String::new(), 0usize);
        for s in &self.segments {
            let n = tokens(&s.clip.text);
            if n > max_tokens {
                return Err(TooLong {
                    id: s.clip.id.clone(),
                    text: s.clip.text.clone(),
                    tokens: n,
                    max: max_tokens,
                });
            }
            if !cur.is_empty() && cur_n + n > max_tokens {
                out.push(std::mem::take(&mut cur));
                cur_n = 0;
            }
            if !cur.is_empty() {
                cur.push(' ');
            }
            cur.push_str(&s.clip.text);
            cur_n += n;
        }
        if !cur.is_empty() {
            out.push(cur);
        }
        Ok(out)
    }
}

/// A name that can be announced: a piste (with its colour) or any other place.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Name {
    /// A piste.
    Piste(PisteName),
    /// A refuge, place, zone or site.
    Place(String),
}

impl Name {
    fn clip(&self) -> Clip {
        match self {
            Name::Piste(p) => Clip {
                id: p.clip_id(),
                text: p.clip_text(),
            },
            Name::Place(n) => Clip {
                id: name_clip_id(n),
                text: name_clip_text(n),
            },
        }
    }
}

/// Side of the route when off it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Side {
    /// On the left.
    Left,
    /// On the right.
    Right,
    /// Behind.
    Behind,
    /// Ahead.
    Ahead,
}

/// Rule of a protected zone (C8).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ZoneRule {
    /// Stay on marked routes.
    StayOnRoutes,
    /// No entry.
    NoEntry,
    /// No bivouac.
    NoBivouac,
    /// No fire.
    NoFire,
    /// Dogs on a leash.
    DogsOnLeash,
    /// No dogs.
    NoDogs,
    /// Wildlife quiet zone.
    Quiet,
}

impl ZoneRule {
    fn fixed(self) -> Fixed {
        match self {
            ZoneRule::StayOnRoutes => Fixed::RuleStayOnRoutes,
            ZoneRule::NoEntry => Fixed::RuleNoEntry,
            ZoneRule::NoBivouac => Fixed::RuleNoBivouac,
            ZoneRule::NoFire => Fixed::RuleNoFire,
            ZoneRule::DogsOnLeash => Fixed::RuleDogsLeash,
            ZoneRule::NoDogs => Fixed::RuleNoDogs,
            ZoneRule::Quiet => Fixed::RuleQuiet,
        }
    }
}

/// Snow quality (A3).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Snow {
    /// Fresh snow.
    Fresh,
    /// Hard, refrozen.
    Hard,
    /// Transformed.
    Transformed,
    /// Wet, heavy.
    Wet,
    /// Wind-blown.
    Windblown,
    /// Icy patches.
    Icy,
}

/// Expected crowd (A4).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Crowd {
    /// Low.
    Low,
    /// Medium.
    Medium,
    /// High.
    High,
}

/// Equipment item with its reason (A6).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Item {
    /// Transceiver, shovel, probe: the route crosses avalanche terrain.
    AvalancheKit,
    /// Head torch: return close to nightfall.
    Headlamp,
    /// Extra warm layer: very cold.
    WarmLayer,
    /// Power bank: part of the route has no network.
    PowerBank,
    /// Tell someone the route and return time.
    TellSomeone,
    /// Compass: fog forecast.
    Compass,
}

/// Kick wax (A2, classic).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Kick {
    /// Green hard wax.
    Green,
    /// Blue hard wax.
    Blue,
    /// Violet hard wax.
    Violet,
    /// Red hard wax.
    Red,
    /// Klister.
    Klister,
    /// Skins or waxless skis.
    Skins,
}

/// Glide wax (A2).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Glide {
    /// Cold snow.
    Cold,
    /// Medium snow.
    Medium,
    /// Warm, wet snow.
    Warm,
}

/// Change of conditions during the outing (A8).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Change {
    /// Wind rising.
    WindRising,
    /// Fog coming.
    Fog,
    /// Snow starting.
    Snow,
    /// Temperature dropping fast.
    TemperatureDrop,
}

/// One line of the daily briefing (A1, A7).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Briefing {
    /// "Point du jour. Cervières."
    Intro {
        /// Site name.
        site: String,
    },
    /// BERA danger level, `None` when the massif has no bulletin.
    Avalanche {
        /// Level 1..=5.
        level: Option<u8>,
    },
    /// Open pistes.
    PistesOpen {
        /// Open pistes.
        open: u32,
        /// All pistes.
        total: u32,
    },
    /// Recommended piste or place.
    Recommended {
        /// What is recommended.
        name: Name,
    },
    /// Weather of the day.
    Weather {
        /// Air temperature, °C.
        temperature_c: f64,
        /// Mean wind, km/h.
        wind_kmh: f64,
        /// Fog forecast.
        fog: bool,
        /// Snowfall forecast.
        snowfall: bool,
    },
    /// Sunset time.
    Sunset {
        /// Local time.
        at: Clock,
    },
    /// Best start time (A9).
    BestTime {
        /// Local time.
        at: Clock,
    },
    /// Snow quality.
    Snow(Snow),
    /// Expected crowd.
    Crowd(Crowd),
}

/// An event of the core that is announced.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Event {
    /// Direction at the next junction, optionally with the piste taken.
    Turn {
        /// Distance to the junction, metres.
        metres: f64,
        /// Direction.
        direction: Direction,
        /// Piste taken after the junction.
        piste: Option<PisteName>,
    },
    /// Fork: keep the piste of this colour.
    Fork {
        /// Distance to the fork, metres (`None`: now).
        metres: Option<f64>,
        /// Colour to keep.
        colour: Colour,
    },
    /// Destination ahead or reached.
    Arrive {
        /// Distance to the destination (`None`: reached).
        metres: Option<f64>,
    },
    /// Remaining distance and time.
    Remaining {
        /// Remaining distance, metres.
        metres: f64,
        /// Remaining time, seconds.
        seconds: f64,
    },
    /// Turnaround countdown before nightfall (C7).
    Turnaround {
        /// Time left before the turnaround time, seconds (≤ 0: now).
        seconds_left: f64,
        /// Turnaround time.
        deadline: Clock,
    },
    /// Turnaround time (briefing).
    TurnaroundDeadline {
        /// Turnaround time.
        deadline: Clock,
    },
    /// Turnaround time moved earlier by a delay (A8).
    TurnaroundAdvanced {
        /// New turnaround time.
        deadline: Clock,
    },
    /// Slope steeper than 30° ahead.
    Slope {
        /// Slope, degrees.
        degrees: f64,
    },
    /// Protected zone ahead or entered, with its rules (C8).
    ProtectedZone {
        /// Entered (true) or ahead (false).
        entered: bool,
        /// Zone name.
        zone: Option<String>,
        /// Applicable rules.
        rules: Vec<ZoneRule>,
        /// End of the period, if seasonal.
        until: Option<DayMonth>,
    },
    /// Zone without any network ahead (P12).
    NoNetworkAhead {
        /// Distance to the zone, metres.
        metres: f64,
        /// Length of the zone along the route, metres.
        length_m: Option<f64>,
    },
    /// Zone without any network entered.
    NoNetworkEntered,
    /// End of the zone without network.
    NoNetworkExit,
    /// Off the route (C2), with the way back.
    OffRoute {
        /// Side of the route.
        side: Side,
        /// Distance to the route, metres.
        metres: f64,
    },
    /// Back on the route.
    BackOnRoute {
        /// Piste rejoined.
        piste: Option<PisteName>,
    },
    /// Reassurance: still on the route.
    Reassurance {
        /// Current piste.
        piste: Option<PisteName>,
    },
    /// Daily briefing line.
    Briefing(Briefing),
    /// Equipment list (A6).
    Equipment {
        /// Items with their reasons.
        items: Vec<Item>,
    },
    /// Wax advice (A2).
    Wax {
        /// Kick wax (classic), if any.
        kick: Option<Kick>,
        /// Glide wax.
        glide: Option<Glide>,
    },
    /// Segment through a historic avalanche path (P20), with the BERA level if any.
    AvalancheSegment {
        /// BERA level of the day.
        bera_level: Option<u8>,
        /// Piste of the segment.
        piste: Option<PisteName>,
    },
    /// Change of conditions (A8).
    ConditionChange(Change),
    /// Battery level and autonomy (C9).
    Battery {
        /// Battery level, percent.
        percent: f64,
        /// Estimated autonomy, seconds.
        autonomy_s: f64,
    },
    /// Battery low.
    BatteryLow,
}

fn piste_clip(p: &PisteName) -> Clip {
    Name::Piste(p.clone()).clip()
}

impl Event {
    /// The announcement of this event.
    pub fn announce(&self) -> Announcement {
        use Pause::{Long, Short};
        let mut a = Announcement::default();
        match self {
            Event::Turn {
                metres,
                direction,
                piste,
            } => {
                match round_distance(*metres) {
                    Some(m) if m <= crate::manifest::TURN_MAX_M => {
                        a.push(clips::turn(*direction, Some(m)), Short)
                    }
                    Some(m) => {
                        a.push(clips::ahead(m), Short);
                        a.push(clips::turn(*direction, None), Short);
                    }
                    None => a.push(clips::turn(*direction, None), Short),
                }
                if let Some(p) = piste {
                    a.push(piste_clip(p), Short);
                }
            }
            Event::Fork { metres, colour } => {
                if let Some(m) = metres.and_then(round_distance) {
                    a.push(clips::ahead(m), Short);
                }
                a.push(clips::fork(*colour), Short);
            }
            Event::Arrive { metres } => {
                if let Some(m) = metres.and_then(round_distance) {
                    a.push(clips::ahead(m), Short);
                }
                a.push(Fixed::Arrive.clip(), Short);
            }
            Event::Remaining { metres, seconds } => {
                let m = if *metres < 75.0 {
                    None
                } else {
                    round_distance(*metres).map(|m| m.max(100))
                };
                a.push(clips::remain(m), Short);
                a.push(clips::about(round_duration(*seconds)), Short);
            }
            Event::Turnaround {
                seconds_left,
                deadline,
            } => {
                // Countdowns are rounded down (never more time than there is); beyond the
                // last countdown the deadline itself is announced.
                let s = *seconds_left;
                match floor_duration(s, &clips::TURNAROUND_MINUTES) {
                    _ if s.is_nan() || s < 5.0 * 60.0 => a.push(Fixed::TurnaroundNow.clip(), Short),
                    Some(m) if s < 135.0 * 60.0 => a.push(clips::turnaround_in(m), Short),
                    _ => {
                        a.push(Fixed::TurnaroundDeadline.clip(), Short);
                        a.push(clips::clock(*deadline), Short);
                    }
                }
            }
            Event::TurnaroundDeadline { deadline } => {
                a.push(Fixed::TurnaroundDeadline.clip(), Short);
                a.push(clips::clock(*deadline), Short);
            }
            Event::TurnaroundAdvanced { deadline } => {
                a.push(Fixed::TurnaroundLate.clip(), Long);
                a.push(Fixed::TurnaroundDeadline.clip(), Short);
                a.push(clips::clock(*deadline), Short);
            }
            Event::Slope { degrees } => {
                let d = if degrees.is_finite() {
                    degrees.round().max(30.0) as u32
                } else {
                    30
                };
                a.push(clips::slope(d), Short);
            }
            Event::ProtectedZone {
                entered,
                zone,
                rules,
                until,
            } => {
                let head = if *entered {
                    Fixed::ZoneEntered
                } else {
                    Fixed::ZoneAhead
                };
                a.push(head.clip(), Short);
                if let Some(z) = zone {
                    a.push(Name::Place(z.clone()).clip(), Long);
                }
                for r in rules {
                    a.push(r.fixed().clip(), Short);
                }
                if let Some(d) = until {
                    a.push(clips::until(*d), Short);
                }
            }
            Event::NoNetworkAhead { metres, length_m } => {
                if let Some(m) = round_distance(*metres) {
                    a.push(clips::ahead(m), Short);
                }
                a.push(Fixed::NoNetworkAhead.clip(), Short);
                if let Some(l) = length_m.and_then(round_distance) {
                    a.push(clips::over(l.max(100)), Short);
                }
            }
            Event::NoNetworkEntered => a.push(Fixed::NoNetworkEntered.clip(), Short),
            Event::NoNetworkExit => a.push(Fixed::NoNetworkExit.clip(), Short),
            Event::OffRoute { side, metres } => {
                a.push(Fixed::OffRoute.clip(), Long);
                let side = match side {
                    Side::Left => Fixed::RouteLeft,
                    Side::Right => Fixed::RouteRight,
                    Side::Behind => Fixed::RouteBehind,
                    Side::Ahead => Fixed::RouteAhead,
                };
                a.push(side.clip(), Short);
                match round_distance(*metres) {
                    Some(m) if m <= crate::manifest::AT_MAX_M => a.push(clips::at(m), Short),
                    Some(_) => a.push(Fixed::RouteFar.clip(), Short),
                    None => {}
                }
            }
            Event::BackOnRoute { piste } => {
                a.push(Fixed::BackOnRoute.clip(), Short);
                if let Some(p) = piste {
                    a.push(piste_clip(p), Short);
                }
            }
            Event::Reassurance { piste } => {
                a.push(Fixed::Reassurance.clip(), Short);
                if let Some(p) = piste {
                    a.push(piste_clip(p), Short);
                }
            }
            Event::Briefing(b) => briefing(&mut a, b),
            Event::Equipment { items } => {
                a.push(Fixed::EquipmentIntro.clip(), Short);
                for i in items {
                    let f = match i {
                        Item::AvalancheKit => Fixed::ItemAvalancheKit,
                        Item::Headlamp => Fixed::ItemHeadlamp,
                        Item::WarmLayer => Fixed::ItemWarmLayer,
                        Item::PowerBank => Fixed::ItemPowerBank,
                        Item::TellSomeone => Fixed::ItemTellSomeone,
                        Item::Compass => Fixed::ItemCompass,
                    };
                    a.push(f.clip(), Short);
                }
            }
            Event::Wax { kick, glide } => {
                a.push(Fixed::WaxIntro.clip(), Short);
                if let Some(k) = kick {
                    let f = match k {
                        Kick::Green => Fixed::KickGreen,
                        Kick::Blue => Fixed::KickBlue,
                        Kick::Violet => Fixed::KickViolet,
                        Kick::Red => Fixed::KickRed,
                        Kick::Klister => Fixed::KickKlister,
                        Kick::Skins => Fixed::KickSkins,
                    };
                    a.push(f.clip(), Short);
                }
                if let Some(g) = glide {
                    let f = match g {
                        Glide::Cold => Fixed::GlideCold,
                        Glide::Medium => Fixed::GlideMedium,
                        Glide::Warm => Fixed::GlideWarm,
                    };
                    a.push(f.clip(), Short);
                }
            }
            Event::AvalancheSegment { bera_level, piste } => {
                a.push(Fixed::AvalancheSegment.clip(), Short);
                if let Some(p) = piste {
                    a.push(piste_clip(p), Long);
                }
                if let Some(l) = bera_level {
                    a.push(clips::bera(*l), Short);
                }
            }
            Event::ConditionChange(c) => {
                let f = match c {
                    Change::WindRising => Fixed::WindRising,
                    Change::Fog => Fixed::FogComing,
                    Change::Snow => Fixed::SnowStarting,
                    Change::TemperatureDrop => Fixed::TempDrop,
                };
                a.push(f.clip(), Short);
            }
            Event::Battery {
                percent,
                autonomy_s,
            } => {
                // Rounded down: the battery is never announced fuller than it is.
                let p = (percent.clamp(0.0, 100.0) / 10.0).floor() as u32 * 10;
                a.push(clips::battery(p), Long);
                a.push(Fixed::Autonomy.clip(), Short);
                a.push(clips::about(round_duration(*autonomy_s)), Short);
            }
            Event::BatteryLow => a.push(Fixed::BatteryLow.clip(), Short),
        }
        a
    }
}

fn briefing(a: &mut Announcement, b: &Briefing) {
    use Pause::Short;
    match b {
        Briefing::Intro { site } => {
            a.push(Fixed::BriefingIntro.clip(), Short);
            a.push(Name::Place(site.clone()).clip(), Short);
        }
        Briefing::Avalanche { level } => match level {
            Some(l) => a.push(clips::bera(*l), Short),
            None => a.push(Fixed::NoBulletin.clip(), Short),
        },
        Briefing::PistesOpen { open, total } => {
            if open >= total && *total > 0 {
                a.push(Fixed::AllOpen.clip(), Short);
            } else {
                a.push(clips::pistes_open(*open), Short);
            }
        }
        Briefing::Recommended { name } => {
            a.push(Fixed::Recommended.clip(), Short);
            a.push(name.clip(), Short);
        }
        Briefing::Weather {
            temperature_c,
            wind_kmh,
            fog,
            snowfall,
        } => {
            a.push(clips::temperature(temperature_c.round() as i32), Short);
            let w = (wind_kmh / 10.0).round() as u32 * 10;
            if w < 10 {
                a.push(Fixed::WindCalm.clip(), Short);
            } else {
                a.push(clips::wind(w), Short);
            }
            if *fog {
                a.push(Fixed::Fog.clip(), Short);
            }
            if *snowfall {
                a.push(Fixed::Snowfall.clip(), Short);
            }
        }
        Briefing::Sunset { at } => {
            a.push(Fixed::Sunset.clip(), Short);
            a.push(clips::clock(*at), Short);
        }
        Briefing::BestTime { at } => {
            a.push(Fixed::BestTime.clip(), Short);
            a.push(clips::clock(*at), Short);
        }
        Briefing::Snow(s) => {
            let f = match s {
                Snow::Fresh => Fixed::SnowFresh,
                Snow::Hard => Fixed::SnowHard,
                Snow::Transformed => Fixed::SnowTransformed,
                Snow::Wet => Fixed::SnowWet,
                Snow::Windblown => Fixed::SnowWindblown,
                Snow::Icy => Fixed::SnowIcy,
            };
            a.push(f.clip(), Short);
        }
        Briefing::Crowd(c) => {
            let f = match c {
                Crowd::Low => Fixed::CrowdLow,
                Crowd::Medium => Fixed::CrowdMedium,
                Crowd::High => Fixed::CrowdHigh,
            };
            a.push(f.clip(), Short);
        }
    }
}
