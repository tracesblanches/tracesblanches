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
    ($( $(#[$m:meta])* $v:ident = $key:literal => $text:literal, $en:literal, )*) => {
        /// Fixed phrases (no parameter).
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
        #[serde(rename_all = "snake_case")]
        pub enum Fixed { $( $(#[$m])* $v, )* }

        impl Fixed {
            /// Every fixed phrase.
            pub const ALL: &'static [Fixed] = &[ $( Fixed::$v, )* ];

            /// Clip of the phrase (French).
            pub fn clip(self) -> Clip {
                match self { $( Fixed::$v => Clip::new(concat!("fix.", $key), $text), )* }
            }

            /// English text of the phrase (VO6), same clip id.
            pub fn english(self) -> &'static str {
                match self { $( Fixed::$v => $en, )* }
            }
        }
    };
}

fixed_phrases! {
    /// Destination reached.
    Arrive = "arrive" => "Vous arrivez à destination.", "You have arrived at your destination.",
    /// Off the planned route.
    OffRoute = "off-route" => "Vous avez quitté l'itinéraire.", "You have left the route.",
    /// The route is on the left.
    RouteLeft = "route-left" => "Il est sur votre gauche.", "It is on your left.",
    /// The route is on the right.
    RouteRight = "route-right" => "Il est sur votre droite.", "It is on your right.",
    /// The route is behind.
    RouteBehind = "route-behind" => "Il est derrière vous.", "It is behind you.",
    /// The route is ahead.
    RouteAhead = "route-ahead" => "Il est devant vous.", "It is ahead of you.",
    /// More than the last grid distance away.
    RouteFar = "route-far" => "Il est à plus de deux kilomètres.", "It is more than two kilometres away.",
    /// Back on the route.
    BackOnRoute = "back-on-route" => "Vous êtes de nouveau sur l'itinéraire.", "You are back on the route.",
    /// Reassurance signal.
    Reassurance = "reassurance" => "Toujours sur l'itinéraire.", "Still on the route.",
    /// Turn back now.
    TurnaroundNow = "turnaround-now" => "Faites demi-tour maintenant, afin de rentrer avant la nuit.", "Turn back now, to be home before dark.",
    /// Label before a clock time.
    TurnaroundDeadline = "turnaround-deadline" => "Heure limite de demi-tour.", "Turnaround time.",
    /// Turnaround moved earlier because of a delay.
    TurnaroundLate = "turnaround-late" => "Vous êtes en retard sur l'heure prévue. Le demi-tour est avancé.", "You are behind schedule. The turnaround is brought forward.",
    /// Protected zone ahead.
    ZoneAhead = "zone-ahead" => "Attention, zone protégée devant.", "Caution, protected area ahead.",
    /// Protected zone entered.
    ZoneEntered = "zone-entered" => "Vous entrez dans une zone protégée.", "You are entering a protected area.",
    /// Another nordic area entered (H2 #133): its name follows.
    DomainEntered = "domain-entered" => "Vous entrez sur un autre domaine nordique.", "You are entering another nordic area.",
    /// Its own trail pass applies (H2 #133).
    DomainFee = "domain-fee" => "Sa redevance s'applique, sauf titre valable sur les deux domaines.", "Its own trail pass applies, unless your pass covers both areas.",
    /// Rule: stay on marked routes.
    RuleStayOnRoutes = "rule-stay" => "Restez sur les itinéraires balisés.", "Stay on the marked routes.",
    /// Rule: no entry.
    RuleNoEntry = "rule-no-entry" => "Accès interdit.", "No entry.",
    /// Rule: no bivouac.
    RuleNoBivouac = "rule-no-bivouac" => "Bivouac interdit.", "No camping.",
    /// Rule: no fire.
    RuleNoFire = "rule-no-fire" => "Feux interdits.", "No fires.",
    /// Rule: dogs on a leash.
    RuleDogsLeash = "rule-dogs-leash" => "Chiens tenus en laisse.", "Dogs must be kept on a lead.",
    /// Rule: no dogs.
    RuleNoDogs = "rule-no-dogs" => "Chiens interdits.", "No dogs allowed.",
    /// Rule: wildlife quiet zone.
    RuleQuiet = "rule-quiet" => "Zone de quiétude de la faune, restez discret.", "Wildlife quiet area, please keep quiet.",
    /// No network ahead (after a distance clip).
    NoNetworkAhead = "no-network-ahead" => "Début de zone sans aucun réseau, même pour les appels d'urgence.", "Start of an area with no network at all, not even for emergency calls.",
    /// No network zone entered.
    NoNetworkEntered = "no-network-entered" => "Vous entrez en zone sans aucun réseau. Les appels d'urgence ne passent pas.", "You are entering an area with no network at all. Emergency calls will not get through.",
    /// End of the no-network zone.
    NoNetworkExit = "no-network-exit" => "Fin de la zone sans réseau.", "End of the area without network.",
    /// Daily briefing opening (before the site name).
    BriefingIntro = "briefing-intro" => "Point du jour.", "Today's briefing.",
    /// No avalanche bulletin for the massif (Jura).
    NoBulletin = "no-bulletin" => "Pas de bulletin d'avalanche pour ce massif.", "No avalanche bulletin for this mountain range.",
    /// Every piste is open.
    AllOpen = "all-open" => "Toutes les pistes sont ouvertes.", "All trails are open.",
    /// Label before a recommended piste or place.
    Recommended = "recommended" => "Conseillé aujourd'hui.", "Recommended today.",
    /// Fog forecast.
    Fog = "fog" => "Brouillard prévu.", "Fog is forecast.",
    /// Snowfall forecast.
    Snowfall = "snowfall" => "Chutes de neige prévues.", "Snowfall is forecast.",
    /// Light wind.
    WindCalm = "wind-calm" => "Le vent sera faible.", "The wind will be light.",
    /// Label before the sunset time.
    Sunset = "sunset" => "Coucher du soleil.", "Sunset.",
    /// Label before the best start time.
    BestTime = "best-time" => "Meilleur moment pour partir.", "Best time to set off.",
    /// Snow: fresh.
    SnowFresh = "snow-fresh" => "Neige fraîche.", "Fresh snow.",
    /// Snow: hard, refrozen.
    SnowHard = "snow-hard" => "Neige dure et regelée.", "Hard, refrozen snow.",
    /// Snow: transformed (spring snow).
    SnowTransformed = "snow-transformed" => "Neige transformée.", "Spring snow.",
    /// Snow: wet, heavy.
    SnowWet = "snow-wet" => "Neige humide et lourde.", "Wet, heavy snow.",
    /// Snow: wind-blown.
    SnowWindblown = "snow-windblown" => "Neige soufflée par le vent.", "Wind-blown snow.",
    /// Snow: icy patches.
    SnowIcy = "snow-icy" => "Plaques de glace.", "Icy patches.",
    /// Crowd: low.
    CrowdLow = "crowd-low" => "Affluence faible.", "Few people expected.",
    /// Crowd: medium.
    CrowdMedium = "crowd-medium" => "Affluence moyenne.", "Moderately busy.",
    /// Crowd: high.
    CrowdHigh = "crowd-high" => "Affluence forte.", "Very busy.",
    /// Label before the equipment list.
    EquipmentIntro = "equipment-intro" => "À emporter.", "To take with you.",
    /// Avalanche transceiver, shovel, probe.
    ItemAvalancheKit = "item-avalanche-kit" => "DVA, pelle et sonde, l'itinéraire traverse du terrain avalancheux.", "Transceiver, shovel and probe, the route crosses avalanche terrain.",
    /// Head torch.
    ItemHeadlamp = "item-headlamp" => "Une frontale, le retour est proche de la nuit.", "A head torch, you will be back close to nightfall.",
    /// Extra warm layer.
    ItemWarmLayer = "item-warm-layer" => "Une couche chaude en plus, il fera très froid.", "An extra warm layer, it will be very cold.",
    /// Power bank.
    ItemPowerBank = "item-power-bank" => "Une batterie externe, une partie de l'itinéraire est sans réseau.", "A power bank, part of the route has no network.",
    /// Tell someone.
    ItemTellSomeone = "item-tell-someone" => "Prévenez un proche de votre itinéraire et de votre heure de retour.", "Tell someone your route and when you expect to be back.",
    /// Compass.
    ItemCompass = "item-compass" => "Une boussole, du brouillard est prévu.", "A compass, fog is forecast.",
    /// Label before the wax advice.
    WaxIntro = "wax-intro" => "Fartage.", "Waxing.",
    /// Kick wax: green.
    KickGreen = "kick-green" => "Fart de retenue vert.", "Green grip wax.",
    /// Kick wax: blue.
    KickBlue = "kick-blue" => "Fart de retenue bleu.", "Blue grip wax.",
    /// Kick wax: violet.
    KickViolet = "kick-violet" => "Fart de retenue violet.", "Violet grip wax.",
    /// Kick wax: red.
    KickRed = "kick-red" => "Fart de retenue rouge.", "Red grip wax.",
    /// Kick: klister.
    KickKlister = "kick-klister" => "Klister.", "Klister.",
    /// Kick: skins or waxless.
    KickSkins = "kick-skins" => "Prenez des skis à écailles ou des peaux.", "Take waxless skis or skins.",
    /// Glide wax: cold snow.
    GlideCold = "glide-cold" => "Fart de glisse pour neige froide.", "Glide wax for cold snow.",
    /// Glide wax: medium snow.
    GlideMedium = "glide-medium" => "Fart de glisse pour neige moyenne.", "Glide wax for medium snow.",
    /// Glide wax: warm, wet snow.
    GlideWarm = "glide-warm" => "Fart de glisse pour neige chaude et humide.", "Glide wax for warm, wet snow.",
    /// Segment through a known avalanche path (CLPA).
    AvalancheSegment = "avalanche-segment" => "Attention, ce tronçon traverse une zone d'avalanche connue.", "Caution, this section crosses a known avalanche path.",
    /// Wind rising.
    WindRising = "wind-rising" => "Le vent se lève.", "The wind is rising.",
    /// Fog coming.
    FogComing = "fog-coming" => "Le brouillard arrive.", "Fog is coming in.",
    /// Snow starting.
    SnowStarting = "snow-starting" => "Il commence à neiger.", "It is starting to snow.",
    /// Temperature dropping.
    TempDrop = "temp-drop" => "La température baisse fortement.", "The temperature is dropping sharply.",
    /// Label before the battery autonomy.
    Autonomy = "autonomy" => "Autonomie restante.", "Battery time remaining.",
    /// Battery low.
    BatteryLow = "battery-low" => "Batterie faible. Gardez de la réserve pour un appel d'urgence.", "Battery low. Keep some charge for an emergency call.",
    /// The BERA of the day could not be read offline (A7).
    AvalancheOffline = "avalanche-offline" => "Bulletin d'avalanche non disponible hors ligne. Consultez-le avant de partir.", "Avalanche bulletin not available offline. Check it before you set off.",
    /// Reminder after « no bulletin » or the BERA level (A7).
    AvalanchesPossible = "avalanches-possible" => "Des avalanches restent possibles sur les fortes pentes.", "Avalanches remain possible on steep slopes.",
    /// The planned route crosses historic avalanche paths (A7, P20).
    RouteAvalancheAreas = "route-avalanche-areas" => "Votre itinéraire traverse des zones d'avalanche connues.", "Your route crosses known avalanche paths.",
    /// The planned route crosses a protected area (A7, C8).
    RouteProtectedZone = "route-protected-zone" => "Votre itinéraire traverse une zone protégée.", "Your route crosses a protected area.",
    /// Part of the planned route has no network at all (A7, P12).
    RouteNoNetwork = "route-no-network" => "Une partie de l'itinéraire est sans aucun réseau, même pour les appels d'urgence.", "Part of the route has no network at all, not even for emergency calls.",
    /// Fire forbidden by the commune, stoves included (A7, P10).
    FireStovesForbidden = "fire-stoves-forbidden" => "Feu interdit sur la commune, réchauds compris.", "Fires are banned in this municipality, stoves included.",
    /// Cold, dry snow (A3).
    SnowColdDry = "snow-cold-dry" => "Neige froide et sèche.", "Cold, dry snow.",
    /// Sunglasses and sunscreen: clear sky (A6).
    ItemSunProtection = "item-sun-protection" => "Des lunettes de soleil et de la crème, le ciel sera dégagé.", "Sunglasses and sunscreen, the sky will be clear.",
    /// Mittens and face protection: very cold (A6).
    ItemFaceProtection = "item-face-protection" => "Des moufles et une protection du visage, il fera très froid.", "Mittens and face protection, it will be very cold.",
    /// Waxless skis or skins: grip wax unreliable (A6, A2).
    ItemWaxless = "item-waxless" => "Des skis à écailles ou des peaux, le fart de retenue tiendra mal.", "Waxless skis or skins, grip wax will not hold well.",
    /// A trail of the planned route is closed: switch to a detour (J2).
    PlanClosedAhead = "plan-closed-ahead" => "Piste fermée sur votre itinéraire. Nouvel itinéraire.", "Trail closed on your route. New route.",
    /// A trail of the planned route is closed and no other way is known (J2).
    PlanClosedNoDetour = "plan-closed-no-detour" => "Piste fermée sur votre itinéraire, aucun autre passage connu.", "Trail closed on your route, no other way known.",
    /// Conditions worsen: switch to the shortcut to the finish (J2).
    PlanShortcut = "plan-shortcut" => "Les conditions changent. Raccourci vers l'arrivée.", "Conditions are changing. Shortcut to the finish.",
    /// Late: switch to a shorter version of the loop (J2).
    PlanShortened = "plan-shortened" => "Vous êtes en retard sur l'heure prévue. Boucle raccourcie pour rentrer à l'heure.", "You are behind schedule. Loop shortened to get back on time.",
    /// Off the route for a while: new route computed (J2).
    PlanRerouted = "plan-rerouted" => "Nouvel itinéraire calculé.", "New route calculated.",
    /// The previous route is kept, the plan B refused (J2).
    PlanKept = "plan-kept" => "Itinéraire initial conservé.", "Original route kept.",
    /// No time left: the fastest way home does not start with a U-turn (J2).
    PlanHomeNow = "plan-home-now" => "Il faut rentrer maintenant par le plus court, afin d'arriver avant la nuit.", "Head back now by the shortest way, to be home before dark.",
    /// End of the outing (J3).
    PlanFinished = "plan-finished" => "Sortie terminée.", "Outing finished.",
    /// Proposal reason: the group moves slower than planned (C11).
    PlanPaceSlower = "plan-pace-slower" => "Vous allez moins vite que prévu.", "You are slower than planned.",
    /// Proposal reason: slower and more stops (C11).
    PlanPaceSlowerPauses = "plan-pace-slower-pauses" => "Vous allez moins vite que prévu, avec plus de pauses.", "You are slower than planned, with more breaks.",
    /// Proposal reason: more stops than planned (C11).
    PlanPacePauses = "plan-pace-pauses" => "Vous faites plus de pauses que prévu.", "You are taking more breaks than planned.",
    /// Proposal, first part (then `loop.back.*`): the short loop (C11).
    PlanProposeShort = "plan-propose-short" => "Je vous propose la boucle courte,", "I suggest the short loop,",
    /// Proposal, first part (then `loop.back.*`): the nearest exit (C11).
    PlanProposeExit = "plan-propose-exit" => "Je vous propose la sortie la plus proche,", "I suggest the nearest exit,",
    /// Proposal, first part (then `loop.back.*`): keep the loop, new return time (C11).
    PlanProposeKeep = "plan-propose-keep" => "Je vous propose de garder la boucle,", "I suggest keeping the loop,",
    /// A proposal was accepted (C11).
    PlanProposalAccepted = "plan-proposal-accepted" => "Proposition acceptée.", "Suggestion accepted.",
    /// The pace observed fits another group profile better: choose it? (X14, one gesture.)
    PlanProfileMismatch = "plan-profile-mismatch" => "Votre rythme ressemble à celui d'un autre profil. Voulez-vous le choisir ?", "Your pace is closer to another profile. Do you want to choose it?",
    /// « Ramène-moi » proposed: the return time calls for going home (#54).
    BackTime = "back-time" => "Il est temps de rentrer.", "Time to head back.",
    /// « Ramène-moi », to the car (one gesture to accept).
    BackToCar = "back-to-car" => "Être guidé jusqu'à la voiture ?", "Shall I guide you back to the car?",
    /// « Ramène-moi », to the start of the outing.
    BackToStart = "back-to-start" => "Être guidé jusqu'au départ ?", "Shall I guide you back to the start?",
    /// « Ramène-moi », to the end of the day plan.
    BackToFinish = "back-to-finish" => "Être guidé jusqu'à l'arrivée ?", "Shall I guide you to the finish?",
    /// « Ramène-moi », to the named place said just before.
    BackToThere = "back-to-there" => "Être guidé jusque-là ?", "Shall I guide you there?",
}

impl Fixed {
    /// Whether the phrase is the first part of a sentence that a value clip ends
    /// (« Je vous propose la boucle courte, » then « retour vers quinze heures quinze. »): it
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

/// "retour vers midi quinze." (last part of a loop summary): the time said as people
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
            let end = if f.continued() { "," } else { "." };
            // A question asks for one gesture (X14, « ramène-moi »).
            let asks = matches!(
                f,
                Fixed::PlanProfileMismatch
                    | Fixed::BackToCar
                    | Fixed::BackToStart
                    | Fixed::BackToFinish
                    | Fixed::BackToThere
            );
            let question = asks && c.text.ends_with(" ?");
            assert!(c.text.ends_with(end) || question, "{}", c.text);
            let en = f.english();
            let question = asks && en.ends_with('?') && !en.ends_with(" ?");
            assert!(en.ends_with(end) || question, "{en}");
            assert!(en.is_ascii(), "English text with French letters: {en}");
        }
    }
}
