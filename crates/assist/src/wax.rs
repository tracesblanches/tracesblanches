//! Wax recommendation (A2, #89): classic (grip and glide) and skating (glide) from air and
//! snow temperature, humidity, snow type; manufacturer tables cited as source.
//!
//! # Sources (read on 15/11/2026)
//!
//! - **Swix**, product pages of the V-line hard grip waxes: each gives one range « in new,
//!   falling snow » and one « in old or transformed snow » (air temperature: the warm
//!   waxes go above 0 °C). V40 adds « Near 0 °C, dry air and fresh snow are required ».
//!   Klisters: range and snow type of each page (KX30 « coarse-grained snow/ice and hard
//!   tracks at freezing temperatures », KX40S « transformed and fine-grained snow… warmer
//!   than 0 °C », K21S « variable conditions around zero », K22 « coarse-grained, old snow,
//!   varying from ice/crust to wet snow », KX65 « damp to wet, granular and coarse snow »,
//!   KX75 « snow with high water content »). Glide: PS line ranges (air temperature: PS10
//!   goes to +10 °C). URLs in [`KICK`] and [`GLIDE`].
//! - **Toko**, grip wax list (Nordic GripWax and Klister ranges) and glide wax list (Base
//!   Performance Hot Wax, by **snow** temperature: the 2026-27 guide says « Base Performance
//!   Red … Schneetemperatur ca. −4 °C bis −12 °C »), guide p. 41: grip waxes for « dry,
//!   cold, and powdery snow and dry fresh snow », klisters for « wet, icy, and
//!   coarse-grained snow, wet fresh snow and old snow ». Used as the second brand and for
//!   the hard wax / klister rule.
//!
//! # Rules
//!
//! 1. Conditions of the skiing window (09–12 h and 12–16 h) at the forecast point: mean
//!    air temperature, its range, humidity when the weather layer has it, and the snow type
//!    from the snow quality model (A3, flat class): fresh → **new snow**; old dry or
//!    wind-blown → **old fine-grained snow**; hard or icy refrozen crust → **coarse frozen
//!    snow**; transformed → **transformed moist snow**; wet or slush → **wet snow**.
//! 2. Grip: new and old snow take the Swix hard wax whose column (new / old) contains the
//!    air temperature, the one whose range is centred closest; coarse frozen snow takes the
//!    ice klister (KX30), or the universal K21S when the air is above 0 °C; transformed moist
//!    snow takes KX40S (or K21S above +2 °C); wet snow takes KX65 (KX75 when slush or above
//!    +5 °C), K22 as the universal alternative. Above −2 °C
//!    the closest centre is V45's, so V40 (« near 0 °C, dry air and fresh snow are
//!    required ») is never given near 0 °C.
//! 3. **Skis à écailles ou peaux** when grip wax is unreliable: new snow between −1 and +1
//!    °C (the dry-to-moist change of the V50 / V55 pages), snow refrozen in the morning that
//!    turns wet in the window, air temperature crossing 0 °C in the window (from ≤ −2 to
//!    ≥ +1 °C), rain or wet falling snow.
//! 4. Glide (classic and skating): Swix PS by air temperature, Toko Base Performance by the
//!    estimated snow temperature: 0 °C when the surface is wet, otherwise
//!    `min(0, air) − 2 × (1 − cloud fraction)` (a clear sky cools the snow below the air).

use jiff::civil::Date;
use serde::Serialize;
use skimap_voice::bank::{Glide, Kick};

use crate::layers::WeatherPoint;
use crate::snow_quality::{DayQuality, Quality};
use crate::text::Lang;

/// Snow type for the grip wax.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SnowType {
    /// New, falling or fresh snow.
    New,
    /// Old fine-grained dry snow.
    Old,
    /// Coarse refrozen snow, crust, ice.
    CoarseFrozen,
    /// Transformed snow that softens, moist.
    TransformedMoist,
    /// Wet snow, slush.
    Wet,
}

impl SnowType {
    /// French label.
    pub fn label_fr(self) -> &'static str {
        self.label(Lang::Fr)
    }

    /// Label in `lang`.
    pub fn label(self, lang: Lang) -> &'static str {
        crate::text::of(lang).wax_snow_type(self)
    }

    /// Snow type of a surface quality (A3).
    pub fn of(q: Quality) -> Self {
        match q {
            Quality::Fresh => SnowType::New,
            Quality::OldDry | Quality::Windblown => SnowType::Old,
            Quality::Hard | Quality::Icy => SnowType::CoarseFrozen,
            Quality::Transformed => SnowType::TransformedMoist,
            Quality::Wet | Quality::Slush => SnowType::Wet,
        }
    }
}

/// A grip wax of a manufacturer table.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct GripWax {
    /// Brand.
    pub brand: &'static str,
    /// Product.
    pub product: &'static str,
    /// Colour family for the voice.
    pub colour: Kick,
    /// Klister (true) or hard wax.
    pub klister: bool,
    /// Range on new, falling snow (warm, cold), °C air.
    pub new_snow: Option<(f64, f64)>,
    /// Range on old or transformed snow (warm, cold), °C air.
    pub old_snow: Option<(f64, f64)>,
    /// Snow the product is made for, as published.
    pub snow: &'static str,
    /// Product page.
    pub url: &'static str,
}

macro_rules! swix {
    ($p:literal) => {
        concat!("https://swixsport.com/eu/ski-wax/", $p)
    };
}

/// Grip waxes (Swix V-line and klisters, Toko as second brand).
pub const KICK: &[GripWax] = &[
    GripWax {
        brand: "Swix",
        product: "V05 Polar",
        colour: Kick::Green,
        klister: false,
        new_snow: Some((-12.0, -25.0)),
        old_snow: Some((-15.0, -30.0)),
        snow: "new, falling snow / old or transformed snow",
        url: swix!("kick-wax/v05-polar-hardwax-25c-to-12c-43g--V0005-V"),
    },
    GripWax {
        brand: "Swix",
        product: "V20 Green",
        colour: Kick::Green,
        klister: false,
        new_snow: Some((-8.0, -15.0)),
        old_snow: Some((-10.0, -18.0)),
        snow: "new, falling snow / old or transformed snow",
        url: swix!("kick-wax/v20-green-hardwax-15c-to-8c-43g--V0020-V"),
    },
    GripWax {
        brand: "Swix",
        product: "V30 Blue",
        colour: Kick::Blue,
        klister: false,
        new_snow: Some((-2.0, -10.0)),
        old_snow: Some((-5.0, -15.0)),
        snow: "new, falling snow / old or transformed snow",
        url: swix!("kick-wax/v30-blue-hardwax-10c-to-2c-43g--V0030-V"),
    },
    GripWax {
        brand: "Swix",
        product: "V40 Blue Extra",
        colour: Kick::Blue,
        klister: false,
        new_snow: Some((-1.0, -7.0)),
        old_snow: Some((-3.0, -10.0)),
        snow: "near 0 °C, dry air and fresh snow are required",
        url: swix!("kick-wax/v40-blue-extra-hardwax-7c-to-1c-43g--V0040-V"),
    },
    GripWax {
        brand: "Swix",
        product: "V45 Violet Special",
        colour: Kick::Violet,
        klister: false,
        new_snow: Some((0.0, -3.0)),
        old_snow: Some((-2.0, -6.0)),
        snow: "new, falling snow / old or transformed snow",
        url: swix!("kick-wax/v45-violet-special-hardwax-3c-to-0c-43g--V0045-V"),
    },
    GripWax {
        brand: "Swix",
        product: "V50 Violet",
        colour: Kick::Violet,
        klister: false,
        new_snow: Some((0.0, 0.0)),
        old_snow: Some((-1.0, -3.0)),
        snow: "snow changes from dry to moist around 0 °C",
        url: swix!("kick-wax/v50-violet-hardwax-0c-43g--V0050-V"),
    },
    GripWax {
        brand: "Swix",
        product: "V55 Red Special",
        colour: Kick::Red,
        klister: false,
        new_snow: Some((1.0, 0.0)),
        old_snow: Some((0.0, -2.0)),
        snow: "moist new snow / older, coarse-grained snow",
        url: swix!("kick-wax/v55-red-special-hardwax-0c-to-1c-43g--V0055-V"),
    },
    GripWax {
        brand: "Swix",
        product: "V60 Red/Silver",
        colour: Kick::Red,
        klister: false,
        new_snow: Some((3.0, 0.0)),
        old_snow: Some((1.0, -1.0)),
        snow: "moist or wet new snow / older, coarse-grained snow",
        url: swix!("kick-wax/v60-red-silver-hardwax-0c-to-3c-43g--V0060-V"),
    },
    GripWax {
        brand: "Swix",
        product: "KX30 Blue Ice Klister",
        colour: Kick::Klister,
        klister: true,
        new_snow: None,
        old_snow: Some((0.0, -12.0)),
        snow: "coarse-grained snow/ice and hard tracks at freezing temperatures",
        url: swix!("klister/kx30-blue-ice-klister-12c-to-0c-55g--KX30-V"),
    },
    GripWax {
        brand: "Swix",
        product: "KX40S Silver Klister",
        colour: Kick::Klister,
        klister: true,
        new_snow: None,
        old_snow: Some((2.0, -4.0)),
        snow: "transformed and fine-grained snow, warmer than 0 °C and slightly wet",
        url: swix!("klister/kx40s-silver-klister-4c-to-2c-55g--KX40S-V"),
    },
    GripWax {
        brand: "Swix",
        product: "K21S Universal Silver Klister",
        colour: Kick::Klister,
        klister: true,
        new_snow: None,
        old_snow: Some((3.0, -5.0)),
        snow: "variable conditions around zero degrees, mixed snow",
        url: swix!("klister/k21s-universal-silver-klister-5c-to-3c-55g--K21S-V"),
    },
    GripWax {
        brand: "Swix",
        product: "K22 Universal VM Klister",
        colour: Kick::Klister,
        klister: true,
        new_snow: None,
        old_snow: Some((10.0, -3.0)),
        snow: "coarse-grained, old snow, varying from ice/crust to wet snow",
        url: swix!("klister/k22-universal-vm-klister-3c-to-10c-55g--K22-V"),
    },
    GripWax {
        brand: "Swix",
        product: "KX65 Red Klister",
        colour: Kick::Klister,
        klister: true,
        new_snow: None,
        old_snow: Some((5.0, 1.0)),
        snow: "damp to wet, granular and coarse snow",
        url: swix!("klister/kx65-red-klister-1c-to-5c-55g--KX65-V"),
    },
    GripWax {
        brand: "Swix",
        product: "KX75 Red Extra Wet Klister",
        colour: Kick::Klister,
        klister: true,
        new_snow: None,
        old_snow: Some((15.0, 2.0)),
        snow: "snow with a high water content",
        url: swix!("klister/kx75-red-extra-wet-klister-2c-to-15c-55g--KX75-V"),
    },
    GripWax {
        brand: "Toko",
        product: "Nordic GripWax X-Cold",
        colour: Kick::Green,
        klister: false,
        new_snow: Some((-12.0, -30.0)),
        old_snow: Some((-12.0, -30.0)),
        snow: "extremely cold conditions",
        url: TOKO_GRIP,
    },
    GripWax {
        brand: "Toko",
        product: "Nordic GripWax blue",
        colour: Kick::Blue,
        klister: false,
        new_snow: Some((-7.0, -30.0)),
        old_snow: Some((-7.0, -30.0)),
        snow: "cold, dry conditions",
        url: TOKO_GRIP,
    },
    GripWax {
        brand: "Toko",
        product: "Nordic GripWax red",
        colour: Kick::Red,
        klister: false,
        new_snow: Some((-2.0, -10.0)),
        old_snow: Some((-2.0, -10.0)),
        snow: "",
        url: TOKO_GRIP,
    },
    GripWax {
        brand: "Toko",
        product: "Nordic GripWax yellow",
        colour: Kick::Red,
        klister: false,
        new_snow: Some((0.0, -2.0)),
        old_snow: Some((0.0, -2.0)),
        snow: "",
        url: TOKO_GRIP,
    },
    GripWax {
        brand: "Toko",
        product: "Nordic Klister blue",
        colour: Kick::Klister,
        klister: true,
        new_snow: None,
        old_snow: Some((-7.0, -30.0)),
        snow: "icy snow",
        url: TOKO_GRIP,
    },
    GripWax {
        brand: "Toko",
        product: "Nordic Klister red",
        colour: Kick::Klister,
        klister: true,
        new_snow: None,
        old_snow: Some((-1.0, -8.0)),
        snow: "damp to frozen snow, wide range",
        url: TOKO_GRIP,
    },
    GripWax {
        brand: "Toko",
        product: "Nordic Klister yellow",
        colour: Kick::Klister,
        klister: true,
        new_snow: None,
        old_snow: Some((0.0, -2.0)),
        snow: "wet to moist snow",
        url: TOKO_GRIP,
    },
];

/// Toko grip wax list.
pub const TOKO_GRIP: &str = "https://www.toko.ch/en/wax-tools/all-grip-waxes/";
/// Toko glide wax list.
pub const TOKO_GLIDE: &str = "https://www.toko.ch/en/wax-tools/all-ski-waxes/";
/// Toko Wax & Tuning Guide 2026-27 (p. 22 snow temperature, p. 41 grip / klister).
pub const TOKO_GUIDE: &str =
    "https://www.toko.ch/fileadmin/Dateiliste/WaxTuningGuide_2026_27_DE-EN.pdf";

/// Temperature a glide range refers to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Basis {
    /// Air temperature.
    Air,
    /// Snow temperature.
    Snow,
}

/// A glide wax of a manufacturer table.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct GlideWax {
    /// Brand.
    pub brand: &'static str,
    /// Product.
    pub product: &'static str,
    /// Range (warm, cold), °C.
    pub range: (f64, f64),
    /// Temperature the range refers to.
    pub basis: Basis,
    /// Product page.
    pub url: &'static str,
}

/// Glide waxes (Swix PS by air temperature, Toko Base Performance by snow temperature).
pub const GLIDE: &[GlideWax] = &[
    GlideWax {
        brand: "Swix",
        product: "PS Polar",
        range: (-14.0, -32.0),
        basis: Basis::Air,
        url: swix!("glide-wax/solid/ps-polar-14c-32c-180g--PSP-18-V"),
    },
    GlideWax {
        brand: "Swix",
        product: "PS5 Turquoise",
        range: (-10.0, -18.0),
        basis: Basis::Air,
        url: swix!("glide-wax/solid/ps5-turquoise-10c-18c-900g--PS05-90-V"),
    },
    GlideWax {
        brand: "Swix",
        product: "PS6 Blue",
        range: (-6.0, -12.0),
        basis: Basis::Air,
        url: swix!("glide-wax/solid/ps6-blue-6c-12c-900g--PS06-90-V"),
    },
    GlideWax {
        brand: "Swix",
        product: "PS7 Violet",
        range: (-2.0, -8.0),
        basis: Basis::Air,
        url: swix!("glide-wax/solid/ps7-violet-2c-8c-900g--PS07-90-V"),
    },
    GlideWax {
        brand: "Swix",
        product: "PS8 Red",
        range: (4.0, -4.0),
        basis: Basis::Air,
        url: swix!("glide-wax/solid/ps8-red-4c-4c-900g--PS08-90-V"),
    },
    GlideWax {
        brand: "Swix",
        product: "PS10 Yellow",
        range: (10.0, 0.0),
        basis: Basis::Air,
        url: swix!("glide-wax/solid/ps10-yellow-0c-10c-180g--PS10-18-V"),
    },
    GlideWax {
        brand: "Toko",
        product: "Base Performance Hot Wax blue",
        range: (-10.0, -30.0),
        basis: Basis::Snow,
        url: TOKO_GLIDE,
    },
    GlideWax {
        brand: "Toko",
        product: "Base Performance Hot Wax red",
        range: (-4.0, -12.0),
        basis: Basis::Snow,
        url: TOKO_GLIDE,
    },
    GlideWax {
        brand: "Toko",
        product: "Base Performance Hot Wax yellow",
        range: (0.0, -6.0),
        basis: Basis::Snow,
        url: TOKO_GLIDE,
    },
];

fn contains(r: (f64, f64), t: f64) -> bool {
    // Ranges are written (warm, cold); a single value (V50 at 0 °C) covers ±0.5 °C.
    let (hi, lo) = if r.0 == r.1 {
        (r.0 + 0.5, r.1 - 0.5)
    } else {
        (r.0, r.1)
    };
    t <= hi && t >= lo
}

fn centre_distance(r: (f64, f64), t: f64) -> f64 {
    ((r.0 + r.1) / 2.0 - t).abs()
}

/// Conditions of a skiing window.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct WaxConditions {
    /// Local hours (from, to).
    pub hours: (u32, u32),
    /// Mean air temperature, °C.
    pub air_c: f64,
    /// Minimum air temperature, °C.
    pub air_min_c: f64,
    /// Maximum air temperature, °C.
    pub air_max_c: f64,
    /// Estimated snow surface temperature, °C.
    pub snow_c: f64,
    /// Mean relative humidity, % (when the weather layer has it).
    pub humidity_pct: Option<f64>,
    /// Snowfall in the window, cm.
    pub snowfall_cm: f64,
    /// Rain in the window, mm.
    pub rain_mm: f64,
    /// Snow surface at the start and at the end of the window (A3, flat).
    pub surface: (Quality, Quality),
    /// Snow type for the grip wax.
    pub snow_type: SnowType,
}

/// Conditions of `date` from `h0` to `h1` (local hours) at a point, with the snow quality
/// of the day (A3); `None` without weather.
pub fn conditions(
    point: &WeatherPoint,
    quality: &DayQuality,
    h0: u32,
    h1: u32,
) -> Option<WaxConditions> {
    let date = quality.date;
    let s = point.summary(date, h0, h1);
    let air = s.temperature_mean_c?;
    let flat = &quality.classes[0].hours;
    let in_window: Vec<Quality> = flat
        .iter()
        .filter(|h| (h0..h1).contains(&(h.time.hour() as u32)))
        .map(|h| h.quality)
        .collect();
    let (first, last) = (*in_window.first()?, *in_window.last()?);
    // Snow type: the most frequent of the window, falling snow first.
    let mut counts: Vec<(Quality, usize)> = Vec::new();
    for q in &in_window {
        match counts.iter_mut().find(|(x, _)| x == q) {
            Some((_, n)) => *n += 1,
            None => counts.push((*q, 1)),
        }
    }
    let dominant = counts
        .iter()
        .max_by_key(|(_, n)| *n)
        .map(|(q, _)| *q)
        .unwrap_or(first);
    let snow_type = if s.snowfall_cm >= 0.5 {
        SnowType::New
    } else {
        SnowType::of(dominant)
    };
    let wet = dominant.is_wet();
    let cloud = s.cloud_mean_pct.unwrap_or(50.0) / 100.0;
    let snow_c = if wet {
        0.0
    } else {
        air.min(0.0) - 2.0 * (1.0 - cloud)
    };
    let round = |x: f64| (x * 10.0).round() / 10.0;
    Some(WaxConditions {
        hours: (h0, h1),
        air_c: round(air),
        air_min_c: round(s.temperature_min_c.unwrap_or(air)),
        air_max_c: round(s.temperature_max_c.unwrap_or(air)),
        snow_c: round(snow_c),
        humidity_pct: s.humidity_mean_pct.map(f64::round),
        snowfall_cm: round(s.snowfall_cm),
        rain_mm: round(s.rain_mm),
        surface: (first, last),
        snow_type,
    })
}

/// Recommendation for one window.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct WaxAdvice {
    /// Conditions.
    pub conditions: WaxConditions,
    /// Grip wax (Swix).
    pub kick: GripWax,
    /// Toko equivalent, if one fits.
    pub kick_alternative: Option<GripWax>,
    /// Grip wax reliable (false: take waxless or skin skis).
    pub kick_reliable: bool,
    /// Advice for waxless (« écailles ») or skin skis, with its reason.
    pub skins: Option<String>,
    /// Glide wax (Swix, air temperature).
    pub glide: GlideWax,
    /// Glide wax (Toko, snow temperature).
    pub glide_alternative: Option<GlideWax>,
    /// Reasons, in the language of [`advise_in`].
    pub reasons: Vec<String>,
}

impl WaxAdvice {
    /// Voice colour of the grip advice.
    pub fn voice_kick(&self) -> Kick {
        if self.kick_reliable {
            self.kick.colour
        } else {
            Kick::Skins
        }
    }

    /// Voice family of the glide wax.
    pub fn voice_glide(&self) -> Glide {
        match self.glide.product {
            "PS10 Yellow" => Glide::Warm,
            "PS8 Red" if self.conditions.air_c >= 0.0 || self.conditions.surface.1.is_wet() => {
                Glide::Warm
            }
            "PS8 Red" | "PS7 Violet" => Glide::Medium,
            _ => Glide::Cold,
        }
    }

    /// French text: « Classique : V30 Blue (Swix) … Skating : … ».
    pub fn text_fr(&self) -> String {
        self.text(Lang::Fr)
    }

    /// Text in `lang` (the skins advice stays in the language of [`advise_in`]).
    pub fn text(&self, lang: Lang) -> String {
        let tx = crate::text::of(lang);
        let c = &self.conditions;
        let alt = self.kick_alternative.map(|k| (k.product, k.brand));
        let kick = (self.kick.product, self.kick.brand);
        let kick = if self.kick_reliable {
            tx.wax_kick(kick, alt)
        } else {
            tx.wax_kick_uncertain(self.skins.as_deref(), kick, alt)
        };
        tx.wax_window(
            c.hours,
            &signed(c.air_c),
            &signed(c.snow_c),
            c.snow_type,
            &kick,
            (self.glide.product, self.glide.brand),
            self.glide_alternative.map(|g| (g.product, g.brand)),
        )
    }
}

/// « +2 », « −3 », « 0 ».
fn signed(t: f64) -> String {
    let r = t.round();
    if r == 0.0 {
        "0".into()
    } else {
        format!("{r:+.0}")
    }
}

fn pick_hard(brand: &str, t: f64, new: bool) -> Option<GripWax> {
    KICK.iter()
        .filter(|k| k.brand == brand && !k.klister)
        .filter_map(|k| {
            let r = if new { k.new_snow } else { k.old_snow }?;
            contains(r, t).then_some((k, centre_distance(r, t)))
        })
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .map(|(k, _)| *k)
}

fn klister(product: &str) -> GripWax {
    *KICK
        .iter()
        .find(|k| k.product == product)
        .expect("klister of the table")
}

fn pick_klister(brand: &str, t: f64) -> Option<GripWax> {
    KICK.iter()
        .filter(|k| k.brand == brand && k.klister)
        .filter_map(|k| {
            let r = k.old_snow?;
            contains(r, t).then_some((k, centre_distance(r, t)))
        })
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .map(|(k, _)| *k)
}

fn pick_glide(brand: &str, t: f64) -> Option<GlideWax> {
    GLIDE
        .iter()
        .filter(|g| g.brand == brand && contains(g.range, t))
        .min_by(|a, b| centre_distance(a.range, t).total_cmp(&centre_distance(b.range, t)))
        .copied()
}

/// Advice for one window, reasons in French ([`advise_in`]).
pub fn advise(c: &WaxConditions) -> WaxAdvice {
    advise_in(c, Lang::Fr)
}

/// [`advise`] with the reasons in `lang` (the phone's language, VO6).
pub fn advise_in(c: &WaxConditions, lang: Lang) -> WaxAdvice {
    let tx = crate::text::of(lang);
    let t = c.air_c;
    let mut reasons = vec![tx.wax_model(c.snow_type, t, c.air_min_c, c.air_max_c)];
    let kick = match c.snow_type {
        SnowType::New | SnowType::Old => {
            let new = c.snow_type == SnowType::New;
            // V40 « near 0 °C, dry air and fresh snow are required »: above −2 °C the
            // closest centre is V45's (new snow, −3 to 0 °C) and V40's old-snow column
            // stops at −3 °C, so V40 is never given near 0 °C.
            match pick_hard("Swix", t, new) {
                Some(k) => k,
                None if t < -15.0 => {
                    reasons.push(tx.wax_colder_than_all().into());
                    *KICK.iter().find(|k| k.product == "V05 Polar").expect("V05")
                }
                None => {
                    reasons.push(tx.wax_warmer_than_hard().into());
                    klister("K21S Universal Silver Klister")
                }
            }
        }
        SnowType::CoarseFrozen => {
            reasons.push(tx.wax_coarse_frozen().into());
            if t <= 0.0 {
                klister("KX30 Blue Ice Klister")
            } else {
                klister("K21S Universal Silver Klister")
            }
        }
        SnowType::TransformedMoist => {
            if t <= 2.0 {
                klister("KX40S Silver Klister")
            } else {
                klister("K21S Universal Silver Klister")
            }
        }
        SnowType::Wet => {
            if c.surface.1 == Quality::Slush || t >= 5.0 {
                klister("KX75 Red Extra Wet Klister")
            } else if t >= 1.0 {
                klister("KX65 Red Klister")
            } else {
                klister("K22 Universal VM Klister")
            }
        }
    };
    let kick_alternative = if kick.klister {
        pick_klister("Toko", t)
    } else {
        pick_hard("Toko", t, c.snow_type == SnowType::New)
    };
    // When grip wax is unreliable.
    let refrozen_then_wet = matches!(c.surface.0, Quality::Hard | Quality::Icy | Quality::OldDry)
        && c.surface.1.is_wet();
    let skins = if c.snow_type == SnowType::New && (-1.0..=1.0).contains(&t) {
        Some(tx.wax_fresh_near_zero().to_owned())
    } else if refrozen_then_wet {
        Some(tx.wax_refrozen_then_wet(c.surface.0, c.surface.1))
    } else if c.air_min_c <= -2.0 && c.air_max_c >= 1.0 {
        Some(tx.wax_crosses_zero(c.air_min_c, c.air_max_c))
    } else if c.rain_mm >= 0.5 || (c.snowfall_cm >= 0.5 && t > 0.0) {
        Some(tx.wax_wet_falling().to_owned())
    } else {
        None
    }
    .map(|why| tx.wax_skins(&why));
    let glide = pick_glide("Swix", t).unwrap_or_else(|| {
        if t > 10.0 {
            GLIDE[5]
        } else if t < -32.0 {
            GLIDE[0]
        } else {
            GLIDE[4]
        }
    });
    let glide_alternative = pick_glide("Toko", c.snow_c);
    reasons.push(tx.wax_glide(t, c.snow_c));
    if let Some(h) = c.humidity_pct {
        reasons.push(tx.wax_humidity(h));
    } else {
        reasons.push(tx.wax_humidity_unknown().into());
    }
    WaxAdvice {
        conditions: c.clone(),
        kick_reliable: skins.is_none(),
        kick,
        kick_alternative,
        skins,
        glide,
        glide_alternative,
        reasons,
    }
}

/// Wax advice of a day: morning and afternoon.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct DayWax {
    /// Date.
    pub date: Date,
    /// Forecast point.
    pub point: String,
    /// 09–12 h.
    pub morning: WaxAdvice,
    /// 12–16 h.
    pub afternoon: WaxAdvice,
    /// Summary, in the language of [`day_wax_in`].
    pub text: String,
    /// Product pages cited.
    pub sources: Vec<String>,
}

impl DayWax {
    /// Voice event of the day (the morning grip, the glide of the day).
    pub fn voice(&self) -> skimap_voice::Event {
        skimap_voice::Event::Wax {
            kick: Some(self.morning.voice_kick()),
            glide: Some(self.morning.voice_glide()),
        }
    }
}

/// Wax advice of the day of `quality` at `point` (`None` without weather or without snow),
/// texts in French ([`day_wax_in`]).
pub fn day_wax(point: &WeatherPoint, quality: &DayQuality) -> Option<DayWax> {
    day_wax_in(point, quality, Lang::Fr)
}

/// [`day_wax`] with the texts in `lang` (the phone's language, VO6).
pub fn day_wax_in(point: &WeatherPoint, quality: &DayQuality, lang: Lang) -> Option<DayWax> {
    if !quality.enough_snow {
        return None;
    }
    let morning = advise_in(&conditions(point, quality, 9, 12)?, lang);
    let afternoon = advise_in(&conditions(point, quality, 12, 16)?, lang);
    let differs = morning.kick.product != afternoon.kick.product
        || morning.kick_reliable != afternoon.kick_reliable
        || morning.glide.product != afternoon.glide.product;
    let text = crate::text::of(lang).wax_day(
        &morning.text(lang),
        differs.then(|| afternoon.text(lang)).as_deref(),
    );
    let mut sources: Vec<String> = [&morning, &afternoon]
        .iter()
        .flat_map(|a| {
            [
                Some(a.kick.url),
                a.kick_alternative.map(|k| k.url),
                Some(a.glide.url),
            ]
            .into_iter()
            .chain([a.glide_alternative.map(|g| g.url)])
            .flatten()
        })
        .map(str::to_owned)
        .collect();
    sources.push(TOKO_GUIDE.to_owned());
    sources.sort();
    sources.dedup();
    Some(DayWax {
        date: quality.date,
        point: point.id.clone(),
        morning,
        afternoon,
        text,
        sources,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cond(t: f64, snow: SnowType, surface: (Quality, Quality)) -> WaxConditions {
        WaxConditions {
            hours: (9, 12),
            air_c: t,
            air_min_c: t - 1.0,
            air_max_c: t + 1.0,
            snow_c: t.min(0.0) - 1.0,
            humidity_pct: None,
            snowfall_cm: 0.0,
            rain_mm: 0.0,
            surface,
            snow_type: snow,
        }
    }

    #[test]
    fn hard_waxes_follow_the_swix_columns() {
        let dry = (Quality::OldDry, Quality::OldDry);
        let fresh = (Quality::Fresh, Quality::Fresh);
        // −6 °C new snow: V30 (−2..−10, centre −6), V40 is −1..−7 (centre −4).
        assert_eq!(
            advise(&cond(-6.0, SnowType::New, fresh)).kick.product,
            "V30 Blue"
        );
        // −4 °C new snow: V40.
        assert_eq!(
            advise(&cond(-4.0, SnowType::New, fresh)).kick.product,
            "V40 Blue Extra"
        );
        // −12 °C old snow: V20 (old −10..−18) or V30 (old −5..−15): V20 is closer.
        assert_eq!(
            advise(&cond(-12.0, SnowType::Old, dry)).kick.product,
            "V20 Green"
        );
        // −1.5 °C old snow: V40 old column is −3..−10, V50 old is −1..−3 (centre −2).
        let a = advise(&cond(-1.5, SnowType::Old, dry));
        assert_eq!(a.kick.product, "V50 Violet");
        assert_eq!(a.voice_kick(), Kick::Violet);
        // Toko equivalent on the same temperature.
        assert_eq!(a.kick_alternative.unwrap().product, "Nordic GripWax yellow");
        // New snow at −1.5 °C (humid or not): V45, the closest centre (V40 is never given
        // above −2 °C).
        let mut c = cond(-1.5, SnowType::New, fresh);
        c.air_min_c = -2.0;
        c.air_max_c = -1.0;
        c.humidity_pct = Some(90.0);
        let a = advise(&c);
        assert_eq!(a.kick.product, "V45 Violet Special", "{a:?}");
    }

    #[test]
    fn klisters_and_skins() {
        // Refrozen crust at −5 °C: ice klister.
        let a = advise(&cond(
            -5.0,
            SnowType::CoarseFrozen,
            (Quality::Hard, Quality::Hard),
        ));
        assert_eq!(a.kick.product, "KX30 Blue Ice Klister");
        assert!(a.kick_reliable);
        // Hard in the morning, transformed at noon: skins advised.
        let a = advise(&cond(
            1.0,
            SnowType::TransformedMoist,
            (Quality::Hard, Quality::Transformed),
        ));
        assert!(!a.kick_reliable);
        assert_eq!(a.voice_kick(), Kick::Skins);
        assert!(a.skins.unwrap().contains("écailles"));
        // New snow at 0 °C: skins.
        let a = advise(&cond(0.0, SnowType::New, (Quality::Fresh, Quality::Fresh)));
        assert!(!a.kick_reliable);
        // Slush at +6: KX75.
        let a = advise(&cond(6.0, SnowType::Wet, (Quality::Wet, Quality::Slush)));
        assert_eq!(a.kick.product, "KX75 Red Extra Wet Klister");
        assert_eq!(a.voice_glide(), Glide::Warm);
    }

    #[test]
    fn glide_by_air_and_snow_temperature() {
        let a = advise(&cond(
            -10.0,
            SnowType::Old,
            (Quality::OldDry, Quality::OldDry),
        ));
        // −10 °C air: PS6 (−6..−12, centre −9) over PS5 (−10..−18, centre −14).
        assert_eq!(a.glide.product, "PS6 Blue");
        assert_eq!(a.voice_glide(), Glide::Cold);
        // Snow −11 °C: Toko red (−4..−12, centre −8) vs blue (−10..−30, centre −20).
        assert_eq!(
            a.glide_alternative.unwrap().product,
            "Base Performance Hot Wax red"
        );
        let a = advise(&cond(
            -3.0,
            SnowType::Old,
            (Quality::OldDry, Quality::OldDry),
        ));
        assert_eq!(a.voice_glide(), Glide::Medium);
        // Every table entry has a URL on the manufacturer site.
        for k in KICK {
            assert!(
                k.url.starts_with("https://swixsport.com/")
                    || k.url.starts_with("https://www.toko.ch/")
            );
        }
    }

    /// The range written in a Swix product URL (`…-25c-to-12c-…`, `…-0c-43g…`), °C.
    fn url_range(url: &str) -> Option<(f64, f64)> {
        let name = url.rsplit('/').next()?;
        let parts: Vec<&str> = name.split('-').collect();
        let deg = |s: &str| s.strip_suffix('c')?.parse::<f64>().ok();
        let degs: Vec<f64> = parts.iter().filter_map(|p| deg(p)).collect();
        match degs.as_slice() {
            [a] => Some((*a, *a)),
            [a, b, ..] => Some((*a, *b)),
            [] => None,
        }
    }

    #[test]
    fn tables_are_the_published_ranges() {
        // The range in each Swix URL (both bounds without their sign for the hard waxes
        // and glide waxes: « 25c-to-12c » is −25 to −12) against the table.
        for k in KICK.iter().filter(|k| k.brand == "Swix") {
            let (a, b) = url_range(k.url).unwrap_or_else(|| panic!("{}", k.url));
            let (warm, cold) = if k.klister {
                k.old_snow.unwrap()
            } else {
                k.new_snow.unwrap()
            };
            let ok = |x: f64, y: f64| (x == warm && y == cold) || (x == cold && y == warm);
            assert!(
                ok(a, b) || ok(-a, -b) || ok(-a, b) || ok(a, -b),
                "{} {:?} vs {:?}",
                k.product,
                (a, b),
                (warm, cold)
            );
            // Below 0 °C, both written without sign: the table has the negative values.
            if k.url.contains("c-to-") && !k.klister && warm < 0.0 {
                assert!(warm < 0.0 && cold < warm, "{}", k.product);
            }
        }
        for g in GLIDE.iter().filter(|g| g.brand == "Swix") {
            let (a, b) = url_range(g.url).unwrap_or_else(|| panic!("{}", g.url));
            let (warm, cold) = g.range;
            assert!(
                [(a, b), (-a, -b), (a, -b), (-a, b)]
                    .iter()
                    .any(|(x, y)| (*x == warm && *y == cold) || (*x == cold && *y == warm)),
                "{} {:?} vs {:?}",
                g.product,
                (a, b),
                g.range
            );
        }
        // The columns the URLs do not carry: old snow of the hard waxes (Swix pages) and
        // the Toko lists, as read on 15/11/2026.
        let old: Vec<(&str, (f64, f64))> = KICK
            .iter()
            .filter(|k| !k.klister)
            .map(|k| (k.product, k.old_snow.unwrap()))
            .collect();
        assert_eq!(
            old,
            [
                ("V05 Polar", (-15.0, -30.0)),
                ("V20 Green", (-10.0, -18.0)),
                ("V30 Blue", (-5.0, -15.0)),
                ("V40 Blue Extra", (-3.0, -10.0)),
                ("V45 Violet Special", (-2.0, -6.0)),
                ("V50 Violet", (-1.0, -3.0)),
                ("V55 Red Special", (0.0, -2.0)),
                ("V60 Red/Silver", (1.0, -1.0)),
                ("Nordic GripWax X-Cold", (-12.0, -30.0)),
                ("Nordic GripWax blue", (-7.0, -30.0)),
                ("Nordic GripWax red", (-2.0, -10.0)),
                ("Nordic GripWax yellow", (0.0, -2.0)),
            ]
        );
        type Columns = (&'static str, Option<(f64, f64)>, Option<(f64, f64)>);
        let toko: Vec<Columns> = KICK
            .iter()
            .filter(|k| k.brand == "Toko")
            .map(|k| (k.product, k.new_snow, k.old_snow))
            .collect();
        assert_eq!(
            toko,
            [
                (
                    "Nordic GripWax X-Cold",
                    Some((-12.0, -30.0)),
                    Some((-12.0, -30.0))
                ),
                (
                    "Nordic GripWax blue",
                    Some((-7.0, -30.0)),
                    Some((-7.0, -30.0))
                ),
                (
                    "Nordic GripWax red",
                    Some((-2.0, -10.0)),
                    Some((-2.0, -10.0))
                ),
                (
                    "Nordic GripWax yellow",
                    Some((0.0, -2.0)),
                    Some((0.0, -2.0))
                ),
                ("Nordic Klister blue", None, Some((-7.0, -30.0))),
                ("Nordic Klister red", None, Some((-1.0, -8.0))),
                ("Nordic Klister yellow", None, Some((0.0, -2.0))),
            ]
        );
        let toko_glide: Vec<(&str, (f64, f64), Basis)> = GLIDE
            .iter()
            .filter(|g| g.brand == "Toko")
            .map(|g| (g.product, g.range, g.basis))
            .collect();
        assert_eq!(
            toko_glide,
            [
                ("Base Performance Hot Wax blue", (-10.0, -30.0), Basis::Snow),
                ("Base Performance Hot Wax red", (-4.0, -12.0), Basis::Snow),
                ("Base Performance Hot Wax yellow", (0.0, -6.0), Basis::Snow),
            ]
        );
        assert!(
            GLIDE
                .iter()
                .filter(|g| g.brand == "Swix")
                .all(|g| g.basis == Basis::Air)
        );
    }

    #[test]
    fn ranges_and_single_values() {
        // A single value (V50 at 0 °C) covers ±0.5 °C.
        assert!(contains((0.0, 0.0), 0.5) && contains((0.0, 0.0), -0.5));
        assert!(!contains((0.0, 0.0), 0.51) && !contains((0.0, 0.0), -0.51));
        assert!(contains((-2.0, -10.0), -2.0) && contains((-2.0, -10.0), -10.0));
        assert!(!contains((-2.0, -10.0), -1.99) && !contains((-2.0, -10.0), -10.01));
        assert_eq!(centre_distance((-2.0, -10.0), -6.0), 0.0);
        assert_eq!(centre_distance((-2.0, -10.0), -3.0), 3.0);
        assert_eq!(centre_distance((-2.0, -10.0), -9.0), 3.0);
        assert_eq!(signed(0.4), "0");
        assert_eq!(signed(-0.4), "0");
        assert_eq!(signed(1.6), "+2");
        assert_eq!(signed(-2.0), "-2");
    }

    #[test]
    fn grip_beyond_the_tables() {
        let dry = (Quality::OldDry, Quality::OldDry);
        let fresh = (Quality::Fresh, Quality::Fresh);
        // Colder than every range: V05 at the limit, said.
        for (t, s, q) in [(-31.0, SnowType::Old, dry), (-26.0, SnowType::New, fresh)] {
            let a = advise(&cond(t, s, q));
            assert_eq!(a.kick.product, "V05 Polar", "{t}");
            assert!(
                a.reasons
                    .iter()
                    .any(|r| r.starts_with("plus froid que toutes les plages"))
            );
        }
        // −20 °C old snow is in V05's column: no « limit » said.
        let a = advise(&cond(-20.0, SnowType::Old, dry));
        assert_eq!(a.kick.product, "V05 Polar");
        assert!(!a.reasons.iter().any(|r| r.starts_with("plus froid")));
        // Warmer than the hard waxes of old snow: the universal klister, said.
        let a = advise(&cond(2.0, SnowType::Old, dry));
        assert_eq!(a.kick.product, "K21S Universal Silver Klister");
        assert!(
            a.reasons
                .iter()
                .any(|r| r.starts_with("plus chaud que les farts durs"))
        );
        assert!(a.kick.klister);
        // The Toko alternative of a klister is a Toko klister.
        assert_eq!(a.kick_alternative, None, "no Toko klister contains +2 °C");
        let a = advise(&cond(
            -0.5,
            SnowType::CoarseFrozen,
            (Quality::Hard, Quality::Hard),
        ));
        assert_eq!(a.kick_alternative.unwrap().product, "Nordic Klister yellow");
        // New snow at +2 °C: V60 (new 0 to +3).
        assert_eq!(
            advise(&cond(2.0, SnowType::New, fresh)).kick.product,
            "V60 Red/Silver"
        );
    }

    #[test]
    fn klister_thresholds() {
        let hard = (Quality::Hard, Quality::Hard);
        let kick = |t: f64, s: SnowType, q: (Quality, Quality)| advise(&cond(t, s, q)).kick.product;
        assert_eq!(
            kick(0.0, SnowType::CoarseFrozen, hard),
            "KX30 Blue Ice Klister"
        );
        assert_eq!(
            kick(0.1, SnowType::CoarseFrozen, hard),
            "K21S Universal Silver Klister"
        );
        let tr = (Quality::Transformed, Quality::Transformed);
        assert_eq!(
            kick(2.0, SnowType::TransformedMoist, tr),
            "KX40S Silver Klister"
        );
        assert_eq!(
            kick(2.1, SnowType::TransformedMoist, tr),
            "K21S Universal Silver Klister"
        );
        let wet = (Quality::Wet, Quality::Wet);
        assert_eq!(kick(5.0, SnowType::Wet, wet), "KX75 Red Extra Wet Klister");
        assert_eq!(kick(4.9, SnowType::Wet, wet), "KX65 Red Klister");
        assert_eq!(kick(1.0, SnowType::Wet, wet), "KX65 Red Klister");
        assert_eq!(kick(0.9, SnowType::Wet, wet), "K22 Universal VM Klister");
        assert_eq!(
            kick(0.9, SnowType::Wet, (Quality::Wet, Quality::Slush)),
            "KX75 Red Extra Wet Klister"
        );
        let a = advise(&cond(-5.0, SnowType::CoarseFrozen, hard));
        assert!(a.reasons[1].starts_with("neige regelée à gros grains : klister"));
    }

    #[test]
    fn when_grip_wax_does_not_hold() {
        let dry = (Quality::OldDry, Quality::OldDry);
        let fresh = (Quality::Fresh, Quality::Fresh);
        let skins = |c: &WaxConditions| advise(c).skins;
        // New snow from −1 to +1 °C.
        assert!(skins(&cond(-1.0, SnowType::New, fresh)).is_some());
        assert!(skins(&cond(1.0, SnowType::New, fresh)).is_some());
        let mut c = cond(-1.1, SnowType::New, fresh);
        c.air_min_c = -1.5;
        assert!(skins(&c).is_none());
        // Old dry, hard or icy in the morning, wet later: none holds the window.
        for first in [Quality::OldDry, Quality::Hard, Quality::Icy] {
            let mut c = cond(0.5, SnowType::Old, (first, Quality::Transformed));
            c.air_min_c = 0.0;
            assert!(
                skins(&c)
                    .unwrap()
                    .contains("aucun fart ne tient tout le créneau"),
                "{first:?}"
            );
        }
        let mut c = cond(0.5, SnowType::Old, (Quality::Fresh, Quality::Transformed));
        c.air_min_c = 0.0;
        assert!(
            skins(&c).is_none(),
            "fresh snow then softening: not refrozen"
        );
        let mut c = cond(-0.5, SnowType::Old, (Quality::Hard, Quality::Hard));
        c.air_min_c = -0.6;
        assert!(skins(&c).is_none(), "refrozen all along");
        // The air crossing 0 °C, from −2 to +1.
        let mut c = cond(-0.5, SnowType::Old, dry);
        c.air_min_c = -2.0;
        c.air_max_c = 1.0;
        assert_eq!(
            skins(&c).unwrap(),
            "skis à écailles ou peaux conseillés (la température passe de -2 à +1 °C dans le créneau)"
        );
        c.air_min_c = -1.9;
        assert!(skins(&c).is_none());
        c.air_min_c = -2.0;
        c.air_max_c = 0.9;
        assert!(skins(&c).is_none());
        // Rain, or snow falling above 0 °C.
        let mut c = cond(-0.5, SnowType::Old, dry);
        c.air_min_c = -1.0;
        c.rain_mm = 0.5;
        assert!(
            skins(&c)
                .unwrap()
                .contains("pluie ou neige mouillée qui tombe")
        );
        c.rain_mm = 0.4;
        assert!(skins(&c).is_none());
        let mut c = cond(0.5, SnowType::Old, dry);
        c.air_min_c = 0.0;
        c.snowfall_cm = 0.5;
        assert!(skins(&c).is_some());
        c.air_c = 0.0;
        assert!(skins(&c).is_none(), "snow falling at 0 °C is not wet");
        c.air_c = 0.5;
        c.snowfall_cm = 0.4;
        assert!(skins(&c).is_none());
        // Unreliable grip is said in the text, with the wax all the same.
        let mut c = cond(0.0, SnowType::New, fresh);
        c.humidity_pct = Some(85.0);
        let a = advise(&c);
        let t = a.text_fr();
        assert!(t.contains("retenue incertaine, skis à écailles ou peaux conseillés (neige fraîche autour de 0 °C"), "{t}");
        assert!(a.reasons.contains(&"humidité 85 %".to_owned()));
        assert!(
            advise(&cond(-5.0, SnowType::Old, dry))
                .reasons
                .contains(&"humidité inconnue (absente de la couche météo)".to_owned())
        );
    }

    #[test]
    fn glide_beyond_the_tables_and_voice() {
        let dry = (Quality::OldDry, Quality::OldDry);
        let g = |t: f64| advise(&cond(t, SnowType::Old, dry)).glide.product;
        assert_eq!(g(11.0), "PS10 Yellow");
        assert_eq!(g(-40.0), "PS Polar");
        assert_eq!(g(-33.0), "PS Polar");
        assert_eq!(g(1.0), "PS8 Red");
        assert_eq!(g(3.0), "PS10 Yellow", "centre +5 closer than PS8's 0");
        // The voice of the glide family.
        let v = |t: f64, q: (Quality, Quality)| advise(&cond(t, SnowType::Old, q)).voice_glide();
        assert_eq!(v(6.0, dry), Glide::Warm);
        assert_eq!(v(0.0, dry), Glide::Warm, "PS8 at 0 °C");
        assert_eq!(v(-1.0, dry), Glide::Medium, "PS8 below 0 °C, dry");
        assert_eq!(
            v(-1.0, (Quality::OldDry, Quality::Transformed)),
            Glide::Warm,
            "PS8 below 0 °C, wet at the end"
        );
        assert_eq!(v(-5.0, dry), Glide::Medium, "PS7");
        assert_eq!(v(-9.0, dry), Glide::Cold, "PS6");
        // Toko glide by the snow temperature.
        let mut c = cond(-1.0, SnowType::Old, dry);
        c.snow_c = -11.0;
        assert_eq!(
            advise(&c).glide_alternative.unwrap().product,
            "Base Performance Hot Wax red"
        );
        c.snow_c = -31.0;
        assert_eq!(advise(&c).glide_alternative, None);
    }
}
