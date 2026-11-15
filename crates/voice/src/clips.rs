//! The clip vocabulary: every clip is an id and the French text spoken for it.
//!
//! An announcement is a sequence of clips. Each clip is a complete spoken unit ending with
//! a period (the text-to-speech engine gives every clip a final intonation), so clips are
//! cut where a speaker pauses: "Dans deux cents mètres, tournez à gauche." | "Greffier,
//! piste bleue." Values are rounded onto finite grids (see [`crate::quantity`]), so the
//! whole vocabulary of a pack is finite: fixed phrases, value phrases, and the pack names.
//!
//! The functions of this module build the id and text of a clip for any value; the value
//! ranges pre-generated in a pack are listed in [`mod@crate::manifest`]. A value outside them
//! still has a correct text, and its id is simply absent from the pack, which sends the
//! announcement to the fallback voice.

use serde::{Deserialize, Serialize};

use crate::names::Colour;
use crate::numbers::{Gender, cardinal, day_of_month, month_name, signed};
use crate::quantity::{Clock, distance_words, duration_words};

/// One clip: stable id (also its file name) and text.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct Clip {
    /// Stable id, ASCII, `[a-z0-9.-]`, e.g. `turn.left.200`.
    pub id: String,
    /// French text, one or two sentences ending with a period.
    pub text: String,
}

impl Clip {
    fn new(id: impl Into<String>, text: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            text: text.into(),
        }
    }
}

/// Upper-cases the first letter.
pub(crate) fn capitalise(s: &str) -> String {
    let mut c = s.chars();
    match c.next() {
        Some(f) => f.to_uppercase().chain(c).collect(),
        None => String::new(),
    }
}

/// Turn direction at a junction.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Direction {
    /// Turn left.
    Left,
    /// Turn right.
    Right,
    /// Bear left.
    SlightLeft,
    /// Bear right.
    SlightRight,
    /// Keep left at a fork.
    KeepLeft,
    /// Keep right at a fork.
    KeepRight,
    /// Go straight on.
    Straight,
    /// Turn back (wrong way).
    UTurn,
}

impl Direction {
    /// All directions.
    pub const ALL: [Direction; 8] = [
        Direction::Left,
        Direction::Right,
        Direction::SlightLeft,
        Direction::SlightRight,
        Direction::KeepLeft,
        Direction::KeepRight,
        Direction::Straight,
        Direction::UTurn,
    ];

    fn key(self) -> &'static str {
        match self {
            Direction::Left => "left",
            Direction::Right => "right",
            Direction::SlightLeft => "slight-left",
            Direction::SlightRight => "slight-right",
            Direction::KeepLeft => "keep-left",
            Direction::KeepRight => "keep-right",
            Direction::Straight => "straight",
            Direction::UTurn => "u-turn",
        }
    }

    fn verb(self) -> &'static str {
        match self {
            Direction::Left => "tournez à gauche",
            Direction::Right => "tournez à droite",
            Direction::SlightLeft => "prenez légèrement à gauche",
            Direction::SlightRight => "prenez légèrement à droite",
            Direction::KeepLeft => "restez à gauche",
            Direction::KeepRight => "restez à droite",
            Direction::Straight => "continuez tout droit",
            Direction::UTurn => "faites demi-tour",
        }
    }
}

/// "Dans deux cents mètres, tournez à gauche." (`metres` on the distance grid) or, with
/// `None`, "Tournez à gauche." (now).
pub fn turn(direction: Direction, metres: Option<u32>) -> Clip {
    match metres {
        Some(m) => Clip::new(
            format!("turn.{}.{m}", direction.key()),
            format!("Dans {}, {}.", distance_words(m), direction.verb()),
        ),
        None => Clip::new(
            format!("turn.{}.now", direction.key()),
            format!("{}.", capitalise(direction.verb())),
        ),
    }
}

/// "À la bifurcation, gardez la bleue."
pub fn fork(colour: Colour) -> Clip {
    Clip::new(
        format!("fork.{}", colour.word()),
        format!("À la bifurcation, gardez la {}.", colour.word()),
    )
}

/// "Dans deux cents mètres."
pub fn ahead(metres: u32) -> Clip {
    Clip::new(
        format!("ahead.{metres}"),
        format!("Dans {}.", distance_words(metres)),
    )
}

/// "À cent mètres." (distance to the route when off it)
pub fn at(metres: u32) -> Clip {
    Clip::new(
        format!("at.{metres}"),
        format!("À {}.", distance_words(metres)),
    )
}

/// "Sur deux kilomètres." (length of a zone)
pub fn over(metres: u32) -> Clip {
    Clip::new(
        format!("over.{metres}"),
        format!("Sur {}.", distance_words(metres)),
    )
}

/// "Il reste quatre kilomètres." or, with `None`, "Il reste moins de cent mètres."
pub fn remain(metres: Option<u32>) -> Clip {
    match metres {
        Some(m) => Clip::new(
            format!("remain.{m}"),
            format!("Il reste {}.", distance_words(m)),
        ),
        None => Clip::new("remain.lt100", "Il reste moins de cent mètres."),
    }
}

/// "Environ cinquante-cinq minutes." or, with `None`, "Moins d'une minute."
pub fn about(minutes: Option<u32>) -> Clip {
    match minutes {
        Some(m) => Clip::new(
            format!("about.{m}"),
            format!("Environ {}.", duration_words(m)),
        ),
        None => Clip::new("about.lt1", "Moins d'une minute."),
    }
}

/// "Quinze heures quarante-cinq."
pub fn clock(c: Clock) -> Clip {
    Clip::new(
        format!("clock.{}", c.key()),
        format!("{}.", capitalise(&c.words())),
    )
}

/// Countdowns announced before the turnaround time, in minutes.
pub const TURNAROUND_MINUTES: [u32; 9] = [5, 10, 15, 20, 30, 45, 60, 90, 120];

/// "Demi-tour dans trente minutes, afin de rentrer avant la nuit."
pub fn turnaround_in(minutes: u32) -> Clip {
    Clip::new(
        format!("turnaround.in.{minutes}"),
        format!(
            "Demi-tour dans {}, afin de rentrer avant la nuit.",
            duration_words(minutes)
        ),
    )
}

/// "Attention, pente de trente-cinq degrés devant."
pub fn slope(degrees: u32) -> Clip {
    Clip::new(
        format!("slope.{degrees}"),
        format!(
            "Attention, pente de {} degrés devant.",
            cardinal(degrees, Gender::Masculine)
        ),
    )
}

/// "Température, moins huit degrés."
pub fn temperature(celsius: i32) -> Clip {
    let id = if celsius < 0 {
        format!("temp.m{}", celsius.unsigned_abs())
    } else {
        format!("temp.{celsius}")
    };
    let unit = if celsius.unsigned_abs() >= 2 {
        "degrés"
    } else {
        "degré"
    };
    Clip::new(
        id,
        format!(
            "Température, {} {unit}.",
            signed(celsius, Gender::Masculine)
        ),
    )
}

/// "Vent de quarante kilomètres heure." (`kmh` a multiple of ten)
pub fn wind(kmh: u32) -> Clip {
    Clip::new(
        format!("wind.{kmh}"),
        format!(
            "Vent de {} kilomètres heure.",
            cardinal(kmh, Gender::Masculine)
        ),
    )
}

/// Avalanche danger level of the BERA (1 to 5), with its official name after the number:
/// "Risque d'avalanche deux sur cinq, limité." (the other order, "avalanche limité", was
/// heard "avalanche unité" on 6 seeds out of 6 by the speech recogniser, against 1 out of 6
/// in this order).
pub fn bera(level: u8) -> Clip {
    let name = match level {
        1 => "faible",
        2 => "limité",
        3 => "marqué",
        4 => "fort",
        _ => "très fort",
    };
    Clip::new(
        format!("bera.{level}"),
        format!(
            "Risque d'avalanche {} sur cinq, {}.",
            cardinal(u32::from(level), Gender::Masculine),
            name
        ),
    )
}

/// "Douze pistes ouvertes.", "Une piste ouverte.", "Aucune piste ouverte."
pub fn pistes_open(n: u32) -> Clip {
    let text = match n {
        0 => "Aucune piste ouverte.".to_owned(),
        1 => "Une piste ouverte.".to_owned(),
        n => format!(
            "{} pistes ouvertes.",
            capitalise(&cardinal(n, Gender::Feminine))
        ),
    };
    Clip::new(format!("open.{n}"), text)
}

/// "Batterie à quarante pour cent." (`percent` a multiple of ten, rounded down)
pub fn battery(percent: u32) -> Clip {
    Clip::new(
        format!("battery.{percent}"),
        format!(
            "Batterie à {} pour cent.",
            cardinal(percent, Gender::Masculine)
        ),
    )
}

/// A day of the year (end of a protection period).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct DayMonth {
    /// Day, 1..=31.
    pub day: u8,
    /// Month, 1..=12.
    pub month: u8,
}

/// "En vigueur jusqu'au trente juin."
pub fn until(d: DayMonth) -> Clip {
    Clip::new(
        format!("until.{:02}{:02}", d.month, d.day),
        format!(
            "En vigueur jusqu'au {} {}.",
            day_of_month(d.day),
            month_name(d.month)
        ),
    )
}

macro_rules! fixed_phrases {
    ($( $(#[$m:meta])* $v:ident = $key:literal => $text:literal, )*) => {
        /// Fixed phrases (no parameter).
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
        #[serde(rename_all = "snake_case")]
        pub enum Fixed { $( $(#[$m])* $v, )* }

        impl Fixed {
            /// Every fixed phrase.
            pub const ALL: &'static [Fixed] = &[ $( Fixed::$v, )* ];

            /// Clip of the phrase.
            pub fn clip(self) -> Clip {
                match self { $( Fixed::$v => Clip::new(concat!("fix.", $key), $text), )* }
            }
        }
    };
}

fixed_phrases! {
    /// Destination reached.
    Arrive = "arrive" => "Vous arrivez à destination.",
    /// Off the planned route.
    OffRoute = "off-route" => "Vous avez quitté l'itinéraire.",
    /// The route is on the left.
    RouteLeft = "route-left" => "Il est sur votre gauche.",
    /// The route is on the right.
    RouteRight = "route-right" => "Il est sur votre droite.",
    /// The route is behind.
    RouteBehind = "route-behind" => "Il est derrière vous.",
    /// The route is ahead.
    RouteAhead = "route-ahead" => "Il est devant vous.",
    /// More than the last grid distance away.
    RouteFar = "route-far" => "Il est à plus de deux kilomètres.",
    /// Back on the route.
    BackOnRoute = "back-on-route" => "Vous êtes de nouveau sur l'itinéraire.",
    /// Reassurance signal.
    Reassurance = "reassurance" => "Toujours sur l'itinéraire.",
    /// Turn back now.
    TurnaroundNow = "turnaround-now" => "Faites demi-tour maintenant, afin de rentrer avant la nuit.",
    /// Label before a clock time.
    TurnaroundDeadline = "turnaround-deadline" => "Heure limite de demi-tour.",
    /// Turnaround moved earlier because of a delay.
    TurnaroundLate = "turnaround-late" => "Vous êtes en retard sur l'heure prévue. Le demi-tour est avancé.",
    /// Protected zone ahead.
    ZoneAhead = "zone-ahead" => "Attention, zone protégée devant.",
    /// Protected zone entered.
    ZoneEntered = "zone-entered" => "Vous entrez dans une zone protégée.",
    /// Rule: stay on marked routes.
    RuleStayOnRoutes = "rule-stay" => "Restez sur les itinéraires balisés.",
    /// Rule: no entry.
    RuleNoEntry = "rule-no-entry" => "Accès interdit.",
    /// Rule: no bivouac.
    RuleNoBivouac = "rule-no-bivouac" => "Bivouac interdit.",
    /// Rule: no fire.
    RuleNoFire = "rule-no-fire" => "Feux interdits.",
    /// Rule: dogs on a leash.
    RuleDogsLeash = "rule-dogs-leash" => "Chiens tenus en laisse.",
    /// Rule: no dogs.
    RuleNoDogs = "rule-no-dogs" => "Chiens interdits.",
    /// Rule: wildlife quiet zone.
    RuleQuiet = "rule-quiet" => "Zone de quiétude de la faune, restez discret.",
    /// No network ahead (after a distance clip).
    NoNetworkAhead = "no-network-ahead" => "Début de zone sans aucun réseau, même pour les appels d'urgence.",
    /// No network zone entered.
    NoNetworkEntered = "no-network-entered" => "Vous entrez en zone sans aucun réseau. Les appels d'urgence ne passent pas.",
    /// End of the no-network zone.
    NoNetworkExit = "no-network-exit" => "Fin de la zone sans réseau.",
    /// Daily briefing opening (before the site name).
    BriefingIntro = "briefing-intro" => "Point du jour.",
    /// No avalanche bulletin for the massif (Jura).
    NoBulletin = "no-bulletin" => "Pas de bulletin d'avalanche pour ce massif.",
    /// Every piste is open.
    AllOpen = "all-open" => "Toutes les pistes sont ouvertes.",
    /// Label before a recommended piste or place.
    Recommended = "recommended" => "Conseillé aujourd'hui.",
    /// Fog forecast.
    Fog = "fog" => "Brouillard prévu.",
    /// Snowfall forecast.
    Snowfall = "snowfall" => "Chutes de neige prévues.",
    /// Light wind.
    WindCalm = "wind-calm" => "Le vent sera faible.",
    /// Label before the sunset time.
    Sunset = "sunset" => "Coucher du soleil.",
    /// Label before the best start time.
    BestTime = "best-time" => "Meilleur moment pour partir.",
    /// Snow: fresh.
    SnowFresh = "snow-fresh" => "Neige fraîche.",
    /// Snow: hard, refrozen.
    SnowHard = "snow-hard" => "Neige dure et regelée.",
    /// Snow: transformed (spring snow).
    SnowTransformed = "snow-transformed" => "Neige transformée.",
    /// Snow: wet, heavy.
    SnowWet = "snow-wet" => "Neige humide et lourde.",
    /// Snow: wind-blown.
    SnowWindblown = "snow-windblown" => "Neige soufflée par le vent.",
    /// Snow: icy patches.
    SnowIcy = "snow-icy" => "Plaques de glace.",
    /// Crowd: low.
    CrowdLow = "crowd-low" => "Affluence faible.",
    /// Crowd: medium.
    CrowdMedium = "crowd-medium" => "Affluence moyenne.",
    /// Crowd: high.
    CrowdHigh = "crowd-high" => "Affluence forte.",
    /// Label before the equipment list.
    EquipmentIntro = "equipment-intro" => "À emporter.",
    /// Avalanche transceiver, shovel, probe.
    ItemAvalancheKit = "item-avalanche-kit" => "DVA, pelle et sonde, l'itinéraire traverse du terrain avalancheux.",
    /// Head torch.
    ItemHeadlamp = "item-headlamp" => "Une frontale, le retour est proche de la nuit.",
    /// Extra warm layer.
    ItemWarmLayer = "item-warm-layer" => "Une couche chaude en plus, il fera très froid.",
    /// Power bank.
    ItemPowerBank = "item-power-bank" => "Une batterie externe, une partie de l'itinéraire est sans réseau.",
    /// Tell someone.
    ItemTellSomeone = "item-tell-someone" => "Prévenez un proche de votre itinéraire et de votre heure de retour.",
    /// Compass.
    ItemCompass = "item-compass" => "Une boussole, du brouillard est prévu.",
    /// Label before the wax advice.
    WaxIntro = "wax-intro" => "Fartage.",
    /// Kick wax: green.
    KickGreen = "kick-green" => "Fart de retenue vert.",
    /// Kick wax: blue.
    KickBlue = "kick-blue" => "Fart de retenue bleu.",
    /// Kick wax: violet.
    KickViolet = "kick-violet" => "Fart de retenue violet.",
    /// Kick wax: red.
    KickRed = "kick-red" => "Fart de retenue rouge.",
    /// Kick: klister.
    KickKlister = "kick-klister" => "Klister.",
    /// Kick: skins or waxless.
    KickSkins = "kick-skins" => "Prenez des skis à écailles ou des peaux.",
    /// Glide wax: cold snow.
    GlideCold = "glide-cold" => "Fart de glisse pour neige froide.",
    /// Glide wax: medium snow.
    GlideMedium = "glide-medium" => "Fart de glisse pour neige moyenne.",
    /// Glide wax: warm, wet snow.
    GlideWarm = "glide-warm" => "Fart de glisse pour neige chaude et humide.",
    /// Segment through a known avalanche path (CLPA).
    AvalancheSegment = "avalanche-segment" => "Attention, ce tronçon traverse une zone d'avalanche connue.",
    /// Wind rising.
    WindRising = "wind-rising" => "Le vent se lève.",
    /// Fog coming.
    FogComing = "fog-coming" => "Le brouillard arrive.",
    /// Snow starting.
    SnowStarting = "snow-starting" => "Il commence à neiger.",
    /// Temperature dropping.
    TempDrop = "temp-drop" => "La température baisse fortement.",
    /// Label before the battery autonomy.
    Autonomy = "autonomy" => "Autonomie restante.",
    /// Battery low.
    BatteryLow = "battery-low" => "Batterie faible. Gardez de la réserve pour un appel d'urgence.",
    /// The BERA of the day could not be read offline (A7).
    AvalancheOffline = "avalanche-offline" => "Bulletin d'avalanche non disponible hors ligne. Consultez-le avant de partir.",
    /// Reminder after « no bulletin » or the BERA level (A7).
    AvalanchesPossible = "avalanches-possible" => "Des avalanches restent possibles sur les fortes pentes.",
    /// The planned route crosses historic avalanche paths (A7, P20).
    RouteAvalancheAreas = "route-avalanche-areas" => "Votre itinéraire traverse des zones d'avalanche connues.",
    /// The planned route crosses a protected area (A7, C8).
    RouteProtectedZone = "route-protected-zone" => "Votre itinéraire traverse une zone protégée.",
    /// Part of the planned route has no network at all (A7, P12).
    RouteNoNetwork = "route-no-network" => "Une partie de l'itinéraire est sans aucun réseau, même pour les appels d'urgence.",
    /// Fire forbidden by the commune, stoves included (A7, P10).
    FireStovesForbidden = "fire-stoves-forbidden" => "Feu interdit sur la commune, réchauds compris.",
    /// Cold, dry snow (A3).
    SnowColdDry = "snow-cold-dry" => "Neige froide et sèche.",
    /// Sunglasses and sunscreen: clear sky (A6).
    ItemSunProtection = "item-sun-protection" => "Des lunettes de soleil et de la crème, le ciel sera dégagé.",
    /// Mittens and face protection: very cold (A6).
    ItemFaceProtection = "item-face-protection" => "Des moufles et une protection du visage, il fera très froid.",
    /// Waxless skis or skins: grip wax unreliable (A6, A2).
    ItemWaxless = "item-waxless" => "Des skis à écailles ou des peaux, le fart de retenue tiendra mal.",
    /// A trail of the planned route is closed: switch to a detour (J2).
    PlanClosedAhead = "plan-closed-ahead" => "Piste fermée sur votre itinéraire. Nouvel itinéraire.",
    /// A trail of the planned route is closed and no other way is known (J2).
    PlanClosedNoDetour = "plan-closed-no-detour" => "Piste fermée sur votre itinéraire, aucun autre passage connu.",
    /// Conditions worsen: switch to the shortcut to the finish (J2).
    PlanShortcut = "plan-shortcut" => "Les conditions changent. Raccourci vers l'arrivée.",
    /// Late: switch to a shorter version of the loop (J2).
    PlanShortened = "plan-shortened" => "Vous êtes en retard sur l'heure prévue. Boucle raccourcie pour rentrer à l'heure.",
    /// Off the route for a while: new route computed (J2).
    PlanRerouted = "plan-rerouted" => "Nouvel itinéraire calculé.",
    /// The previous route is kept, the plan B refused (J2).
    PlanKept = "plan-kept" => "Itinéraire initial conservé.",
    /// No time left: the fastest way home does not start with a U-turn (J2).
    PlanHomeNow = "plan-home-now" => "Il faut rentrer maintenant par le plus court, afin d'arriver avant la nuit.",
    /// End of the outing (J3).
    PlanFinished = "plan-finished" => "Sortie terminée.",
    /// Proposal reason: the group moves slower than planned (C11).
    PlanPaceSlower = "plan-pace-slower" => "Vous allez moins vite que prévu.",
    /// Proposal reason: slower and more stops (C11).
    PlanPaceSlowerPauses = "plan-pace-slower-pauses" => "Vous allez moins vite que prévu, avec plus de pauses.",
    /// Proposal reason: more stops than planned (C11).
    PlanPacePauses = "plan-pace-pauses" => "Vous faites plus de pauses que prévu.",
    /// Proposal, first part (then `loop.back.*`): the short loop (C11).
    PlanProposeShort = "plan-propose-short" => "Je propose la boucle courte,",
    /// Proposal, first part (then `loop.back.*`): the nearest exit (C11).
    PlanProposeExit = "plan-propose-exit" => "Je propose la sortie la plus proche,",
    /// Proposal, first part (then `loop.back.*`): keep the loop, new return time (C11).
    PlanProposeKeep = "plan-propose-keep" => "Je propose de garder la boucle,",
    /// A proposal was accepted (C11).
    PlanProposalAccepted = "plan-proposal-accepted" => "Proposition acceptée.",
}

impl Fixed {
    /// Whether the phrase is the first part of a sentence that a value clip ends
    /// (« Je propose la boucle courte, » then « retour vers quinze heures et quart. »): it
    /// ends with a comma instead of a full stop.
    pub fn continued(self) -> bool {
        matches!(
            self,
            Fixed::PlanProposeShort | Fixed::PlanProposeExit | Fixed::PlanProposeKeep
        )
    }
}

/// Lengths of a suggested loop announced, kilometres.
pub const LOOP_KM: (u32, u32) = (1, 60);

/// "Boucle de treize kilomètres," (first part of a loop summary, A5).
pub fn loop_length(km: u32) -> Clip {
    Clip::new(
        format!("loop.km.{km}"),
        if km == 1 {
            "Boucle d'un kilomètre,".to_owned()
        } else {
            format!("Boucle de {} kilomètres,", cardinal(km, Gender::Masculine))
        },
    )
}

/// "environ deux heures," (second part of a loop summary).
pub fn loop_about(minutes: u32) -> Clip {
    Clip::new(
        format!("loop.about.{minutes}"),
        format!("environ {},", duration_words(minutes)),
    )
}

/// "retour vers midi et quart." (last part of a loop summary): the time said as people
/// say it, to the quarter hour.
pub fn loop_back(c: Clock) -> Clip {
    Clip::new(
        format!("loop.back.{}", c.key()),
        format!("retour vers {}.", c.spoken()),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn value_clips() {
        assert_eq!(
            turn(Direction::Left, Some(200)).text,
            "Dans deux cents mètres, tournez à gauche."
        );
        assert_eq!(turn(Direction::Left, Some(200)).id, "turn.left.200");
        assert_eq!(
            turn(Direction::Straight, None).text,
            "Continuez tout droit."
        );
        assert_eq!(remain(Some(4000)).text, "Il reste quatre kilomètres.");
        assert_eq!(about(Some(60)).text, "Environ une heure.");
        assert_eq!(about(None).text, "Moins d'une minute.");
        assert_eq!(
            clock(Clock {
                hour: 16,
                minute: 30
            })
            .text,
            "Seize heures trente."
        );
        assert_eq!(
            turnaround_in(30).text,
            "Demi-tour dans trente minutes, afin de rentrer avant la nuit."
        );
        assert_eq!(
            slope(31).text,
            "Attention, pente de trente et un degrés devant."
        );
        assert_eq!(temperature(-8).text, "Température, moins huit degrés.");
        assert_eq!(temperature(-8).id, "temp.m8");
        assert_eq!(temperature(1).text, "Température, un degré.");
        assert_eq!(temperature(0).text, "Température, zéro degré.");
        assert_eq!(wind(40).text, "Vent de quarante kilomètres heure.");
        assert_eq!(bera(3).text, "Risque d'avalanche trois sur cinq, marqué.");
        assert_eq!(pistes_open(21).text, "Vingt et une pistes ouvertes.");
        assert_eq!(pistes_open(1).text, "Une piste ouverte.");
        assert_eq!(battery(40).text, "Batterie à quarante pour cent.");
        assert_eq!(
            until(DayMonth { day: 1, month: 7 }).text,
            "En vigueur jusqu'au premier juillet."
        );
        assert_eq!(
            fork(Colour::Blue).text,
            "À la bifurcation, gardez la bleue."
        );
    }

    #[test]
    fn fixed_ids_are_unique_and_safe() {
        let mut ids: Vec<String> = Fixed::ALL.iter().map(|f| f.clip().id).collect();
        let n = ids.len();
        ids.sort();
        ids.dedup();
        assert_eq!(ids.len(), n);
        for f in Fixed::ALL {
            let c = f.clip();
            assert!(
                c.id.bytes().all(|b| b.is_ascii_lowercase()
                    || b.is_ascii_digit()
                    || b == b'.'
                    || b == b'-')
            );
            let end = if f.continued() { ',' } else { '.' };
            assert!(c.text.ends_with(end), "{}", c.text);
        }
    }
}
