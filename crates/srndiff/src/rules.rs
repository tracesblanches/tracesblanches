//! Readable rules: level of a direction of travel, ranked reasons.
//!
//! The levels follow the four SRN levels adopted by the alpine clubs in October 2025
//! (« SRN1 » to « SRN4 », skirandonneenordique.com, 24/11/2025), see `docs/methode.md`:
//!
//! | Level | Slopes (open terrain) | Forest | Day |
//! |---|---|---|---|
//! | facile (SRN1) | up to 15° | — | ≤ 10 km, 3–4 h, little climbing |
//! | moyen (SRN2) | 15–20° | sparse forest below 8° | 10–15 km, 4–6 h, ≤ 600 m |
//! | difficile (SRN3) | 20–25° | sparse forest up to 15° | 12–20 km, 5–8 h, ≤ 800 m |
//! | très difficile (SRN4) | up to 30° | — | raid, > 20 km, > 800 m, > 10 h |
//!
//! The level of a direction is the highest level implied by any factor: technique
//! (steepest sustained descent and climb, forest descents, side-hill traverses), effort
//! (distance, climbing, time), and floors (avalanche exposure, isolation, network, day
//! conditions). With the calibrated [`Params::default`], forest and side hill only feed the
//! advice. It is never « facile » when the track crosses terrain the exposure model
//! labels « exposé » or « très exposé ».

use serde::{Deserialize, Serialize};

use crate::analysis::{self, Direction, Measure, Metrics, Passage, PassageKind};
use crate::profile::Profile;
use crate::text::{self, Lang};

/// Difficulty level.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Level {
    /// « facile » (SRN1).
    Facile = 1,
    /// « moyen » (SRN2).
    Moyen = 2,
    /// « difficile » (SRN3).
    Difficile = 3,
    /// « très difficile » (SRN4).
    TresDifficile = 4,
}

impl Level {
    /// From 1–4 (clamped).
    pub fn from_index(i: u8) -> Level {
        match i {
            0 | 1 => Level::Facile,
            2 => Level::Moyen,
            3 => Level::Difficile,
            _ => Level::TresDifficile,
        }
    }

    /// 1–4.
    pub fn index(self) -> u8 {
        self as u8
    }

    /// French label.
    pub fn label(self) -> &'static str {
        self.label_in(Lang::Fr)
    }

    /// Label in `lang`: « facile », « easy »…
    pub fn label_in(self, lang: Lang) -> &'static str {
        text::of(lang).level(self)
    }

    /// One level more (capped).
    pub fn up(self) -> Level {
        Level::from_index(self.index() + 1)
    }
}

/// How the effort (distance, climbing, time) counts in the level.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Effort {
    /// Thresholds of the 2025 scale as published.
    Scale,
    /// Thresholds of the scale multiplied by a factor (calibration).
    Scaled(f64),
    /// Only multi-day outings (raid) raise the level.
    RaidOnly,
}

/// Rule parameters. [`Params::default`] holds the calibrated values (`docs/evaluation.md`).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Params {
    /// Window of the sustained angle of the track, metres.
    pub sustain_m: f64,
    /// Upper bounds of facile, moyen, difficile for open slopes, degrees (2025 scale).
    pub open_deg: [f32; 3],
    /// Slope at and above which a passage is outside SRN terrain, degrees.
    pub beyond_deg: f32,
    /// Upper bounds of facile (none) and moyen for descents in dense forest, degrees:
    /// below `forest_deg[0]` moyen, up to `forest_deg[1]` difficile, beyond très
    /// difficile (2025 scale: sparse forest below 8° SRN2, up to 15° SRN3).
    pub forest_deg: [f32; 2],
    /// Canopy cover from which a sample is in forest, percent; `None`: forest ignored.
    pub forest_cover_pct: Option<f32>,
    /// Minimum length in dense forest for a forest descent to count, metres.
    pub forest_min_m: f64,
    /// Side-hill traverse: difficile from this terrain slope, très difficile from the
    /// second; `None`: ignored.
    pub sidehill_deg: Option<[f32; 2]>,
    /// Effort rule.
    pub effort: Effort,
    /// Distance bounds of facile, moyen, difficile, km (2025 scale: 10, 15, 20).
    pub distance_km: [f64; 3],
    /// Ascent bounds, metres (2025 scale: « peu », 600, 800; « peu » = 300, deduced).
    pub ascent_m: [f64; 3],
    /// Munter time bounds, hours (2025 scale: 4, 6, 8).
    pub time_h: [f64; 3],
    /// Raid: longer than this, km, or longer than `raid_h` (2025 scale: 2 days or more,
    /// > 10 h a day).
    pub raid_km: f64,
    /// Raid duration, hours.
    pub raid_h: f64,
    /// Farther than this from any exit: at least moyen, km (deduced).
    pub remote_km: f64,
    /// Longer than this without any network: at least moyen, km (deduced; Norwegian
    /// scale: no mobile coverage is a « krevende » trait).
    pub no_network_km: f64,
    /// Exposure not evaluated over terrain at least this steep: at least moyen, degrees.
    pub unknown_exposure_deg: f32,
    /// Track segments longer than this are treated as approximate, metres.
    pub coarse_m: f64,
    /// Lowest level when the track crosses « très exposé » terrain (never below moyen:
    /// any exposed terrain already makes it at least moyen).
    pub high_exposure_floor: Level,
}

impl Default for Params {
    /// Calibrated on the published ratings (`docs/evaluation.md`, leave-one-route-out
    /// choice on 38 of 38 routes): slope over 200 m, open-terrain bounds 1.5° below the
    /// 2025 scale, forest and side hill not counted in the level (only in the advice),
    /// effort counted from twice the bounds of the scale, segments of more than 40 m
    /// approximate, « très exposé » floor moyen.
    fn default() -> Self {
        Self {
            sustain_m: 200.0,
            open_deg: [13.5, 18.5, 23.5],
            beyond_deg: 30.0,
            forest_deg: [8.0, 15.0],
            forest_cover_pct: None,
            forest_min_m: 30.0,
            sidehill_deg: None,
            effort: Effort::Scaled(2.0),
            distance_km: [10.0, 15.0, 20.0],
            ascent_m: [300.0, 600.0, 800.0],
            time_h: [4.0, 6.0, 8.0],
            raid_km: 30.0,
            raid_h: 10.0,
            remote_km: 5.0,
            no_network_km: 2.0,
            unknown_exposure_deg: 25.0,
            coarse_m: 40.0,
            high_exposure_floor: Level::Moyen,
        }
    }
}

impl Params {
    /// The 2025 scale read literally (no calibration): slope over 30 m, bounds 15, 20,
    /// 25°, forest from 50 % cover, side hill from 20°, effort as published, segments of
    /// more than 60 m approximate, « très exposé » floor difficile.
    pub fn scale() -> Self {
        Self {
            sustain_m: 30.0,
            open_deg: [15.0, 20.0, 25.0],
            forest_cover_pct: Some(50.0),
            sidehill_deg: Some([20.0, 25.0]),
            effort: Effort::Scale,
            coarse_m: 60.0,
            high_exposure_floor: Level::Difficile,
            ..Self::default()
        }
    }

    /// Measurement settings of these parameters.
    pub fn measure(&self) -> Measure {
        Measure {
            sustain_m: self.sustain_m,
            forest_cover_pct: self.forest_cover_pct.unwrap_or(f32::INFINITY),
            opening_h: 2.0,
            sidehill_deg: self.sidehill_deg.map_or(f32::INFINITY, |s| s[0]),
            coarse_m: self.coarse_m,
        }
    }
}

/// Conditions of the day, when known (all optional).
#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
pub struct Conditions {
    /// Fresh snow to break, cm.
    pub fresh_snow_cm: Option<f32>,
    /// Breakable crust.
    pub crust: bool,
    /// Refrozen, hard or icy snow (morning after a thaw).
    pub icy: bool,
}

/// Group of a reason.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Factor {
    /// Steep descent.
    Descent,
    /// Steep climb.
    Climb,
    /// Descent in dense forest.
    Forest,
    /// Side-hill traverse.
    Sidehill,
    /// Outside SRN terrain (30° or more).
    Beyond,
    /// Distance, climbing, time.
    Effort,
    /// Avalanche exposure.
    Avalanche,
    /// Far from an exit.
    Remote,
    /// No mobile network.
    Network,
    /// Protected zones.
    Zones,
    /// Conditions of the day.
    Conditions,
    /// Missing data.
    Data,
}

/// One reason of the level.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Reason {
    /// Level implied by this factor.
    pub level: Level,
    /// Group.
    pub factor: Factor,
    /// Text, in the language asked ([`rate_in`]).
    pub text: String,
    /// Where, km from the start of the direction.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub at_km: Option<f64>,
}

/// Level of one direction with its reasons.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Rating {
    /// Overall level.
    pub level: Level,
    /// Technical level (terrain only: slopes, forest, side hill).
    pub technical: Level,
    /// Effort level (distance, climbing, time) on the 2025 scale.
    pub effort: Level,
    /// A passage of 30° or more: outside SRN terrain.
    pub beyond_srn: bool,
    /// Multi-day outing (raid).
    pub raid: bool,
    /// Effort as counted in the level ([`Params::effort`]).
    pub effort_counted: Level,
    /// Floor set by the avalanche exposure (facile when none).
    pub avalanche_floor: Level,
    /// Floor set by isolation, network and missing exposure data.
    pub other_floor: Level,
    /// Reasons, most important first.
    pub reasons: Vec<Reason>,
}

fn band(v: f64, bounds: &[f64; 3]) -> Level {
    match bounds.iter().position(|b| v <= *b) {
        Some(i) => Level::from_index(i as u8 + 1),
        None => Level::TresDifficile,
    }
}

fn open_level(p: &Params, deg: f32) -> Level {
    match p.open_deg.iter().position(|b| deg < *b) {
        Some(i) => Level::from_index(i as u8 + 1),
        None => Level::TresDifficile,
    }
}

fn forest_level(p: &Params, deg: f32) -> Level {
    if deg < p.forest_deg[0] {
        Level::Moyen
    } else if deg <= p.forest_deg[1] {
        Level::Difficile
    } else {
        Level::TresDifficile
    }
}

/// French decimal separator: a point between two digits becomes a comma.
pub fn decimal_comma(s: &str) -> String {
    let c: Vec<char> = s.chars().collect();
    c.iter()
        .enumerate()
        .map(|(i, ch)| {
            let digit = |j: Option<&char>| j.is_some_and(|x| x.is_ascii_digit());
            if *ch == '.' && i > 0 && digit(c.get(i - 1)) && digit(c.get(i + 1)) {
                ','
            } else {
                *ch
            }
        })
        .collect()
}

/// A length in words: metres below 1 km (French).
pub fn length(m: f64) -> String {
    text::of(Lang::Fr).length(m)
}

fn steepest(ps: &[Passage]) -> Option<&Passage> {
    ps.iter().max_by(|a, b| a.max_deg.total_cmp(&b.max_deg))
}

/// Level and reasons of one direction, reasons in French (calibration and evaluation
/// reports of the pipeline).
pub fn rate(m: &Metrics, p: &Params, c: Option<&Conditions>) -> Rating {
    rate_in(m, p, c, Lang::Fr)
}

/// Level and reasons of one direction, reasons in `lang`.
pub fn rate_in(m: &Metrics, p: &Params, c: Option<&Conditions>, lang: Lang) -> Rating {
    let tx = text::of(lang);
    let mut reasons: Vec<Reason> = Vec::new();
    let mut push = |level: Level, factor: Factor, text: String, at_km: Option<f64>| {
        reasons.push(Reason {
            level,
            factor,
            text: tx.finish(text),
            at_km,
        })
    };
    let icy = c.is_some_and(|c| c.icy || c.crust);
    let mut beyond = false;

    // Technique: steepest descent, steepest climb, forest, side hill.
    let mut technical = Level::Facile;
    if let Some(d) = steepest(&m.descents) {
        let mut l = open_level(p, d.max_deg);
        if icy && d.max_deg >= 12.0 {
            l = l.up();
        }
        technical = technical.max(l);
        push(
            l,
            Factor::Descent,
            tx.steepest_descent(d.max_deg, p.sustain_m, d.start_km, d.length_m),
            Some(d.start_km),
        );
        if d.max_deg >= p.beyond_deg {
            beyond = true;
        }
    } else if m.max_descent_deg >= 3.0 {
        push(
            Level::Facile,
            Factor::Descent,
            tx.gentle_descents(m.max_descent_deg),
            None,
        );
    } else {
        push(Level::Facile, Factor::Descent, tx.no_descent(), None);
    }
    if let Some(cl) = steepest(&m.climbs) {
        let l = open_level(p, cl.max_deg);
        technical = technical.max(l);
        push(
            l,
            Factor::Climb,
            tx.steepest_climb(cl.max_deg, p.sustain_m, cl.start_km, cl.length_m),
            Some(cl.start_km),
        );
        if cl.max_deg >= p.beyond_deg {
            beyond = true;
        }
    }
    if p.forest_cover_pct.is_some() {
        let fd = m
            .descents
            .iter()
            .filter(|d| d.dense_forest_m >= p.forest_min_m)
            .max_by(|a, b| a.max_deg.total_cmp(&b.max_deg));
        if let Some(d) = fd {
            let mut l = forest_level(p, d.max_deg);
            if icy {
                l = l.up();
            }
            technical = technical.max(l);
            push(
                l,
                Factor::Forest,
                tx.forest_descent(d.max_deg, d.dense_forest_m, d.start_km),
                Some(d.start_km),
            );
        }
    }
    if let Some(sh) = p.sidehill_deg {
        let s = m
            .sidehills
            .iter()
            .max_by(|a, b| a.terrain_max_deg.total_cmp(&b.terrain_max_deg));
        if let Some(s) = s {
            let l = if s.terrain_max_deg >= sh[1] {
                Level::TresDifficile
            } else {
                Level::Difficile
            };
            technical = technical.max(l);
            push(
                l,
                Factor::Sidehill,
                tx.sidehill(s.terrain_max_deg, s.length_m, s.start_km),
                Some(s.start_km),
            );
            if s.terrain_max_deg >= p.beyond_deg {
                beyond = true;
            }
        }
    }
    if beyond {
        technical = Level::TresDifficile;
        push(
            Level::TresDifficile,
            Factor::Beyond,
            tx.beyond(p.beyond_deg),
            None,
        );
    }

    // Effort.
    let snow = c.and_then(|c| c.fresh_snow_cm).unwrap_or(0.0);
    let time_h = m.munter_h * if snow >= 20.0 { 1.3 } else { 1.0 };
    let raid = m.length_km > p.raid_km || time_h > p.raid_h;
    let scaled = |b: [f64; 3], f: f64| [b[0] * f, b[1] * f, b[2] * f];
    let effort_scale = band(m.length_km, &p.distance_km)
        .max(band(m.ascent_m, &p.ascent_m))
        .max(band(time_h, &p.time_h));
    let effort_counted = match p.effort {
        Effort::Scale => effort_scale,
        Effort::Scaled(f) => band(m.length_km, &scaled(p.distance_km, f))
            .max(band(m.ascent_m, &scaled(p.ascent_m, f)))
            .max(band(time_h, &scaled(p.time_h, f))),
        Effort::RaidOnly => Level::Facile,
    };
    let mut effort_counted = if raid {
        Level::TresDifficile
    } else {
        effort_counted
    };
    if snow >= 20.0 && !raid {
        effort_counted = effort_counted.max(effort_scale.min(Level::Difficile));
    }
    push(
        effort_counted,
        Factor::Effort,
        tx.effort(m.length_km, m.ascent_m, &tx.hours(time_h), raid),
        None,
    );

    // Floors.
    let mut floor = Level::Facile;
    let mut ava_floor = Level::Facile;
    if m.high_exposed_km > 0.0 {
        let l = p.high_exposure_floor.max(Level::Moyen);
        ava_floor = ava_floor.max(l);
        let at = m.high_exposed.first().map(|s| s.start_km);
        push(
            l,
            Factor::Avalanche,
            tx.high_exposed(&tx.length(m.high_exposed_km * 1000.0), at),
            at,
        );
    }
    if m.exposed_km > 0.0 {
        ava_floor = ava_floor.max(Level::Moyen);
        let at = m.exposed.first().map(|s| s.start_km);
        push(
            Level::Moyen,
            Factor::Avalanche,
            tx.exposed(&tx.length(m.exposed_km * 1000.0), m.exposed.len()),
            at,
        );
    }
    // Exposure unknown where the terrain is steep, or where the terrain itself is unknown
    // (outside the LiDAR HD cover): nothing allows « facile » there.
    let no_relief = m.unknown_elevation_share > 0.05;
    if m.exposure_unknown_km > 0.05
        && (m.exposure_unknown_max_terrain_deg >= p.unknown_exposure_deg || no_relief)
    {
        floor = floor.max(Level::Moyen);
        push(
            Level::Moyen,
            Factor::Data,
            if no_relief {
                tx.relief_and_exposure_unknown(100.0 * m.unknown_elevation_share)
            } else {
                tx.exposure_unknown(
                    &tx.length(m.exposure_unknown_km * 1000.0),
                    m.exposure_unknown_max_terrain_deg,
                )
            },
            None,
        );
    }
    if m.max_exit_km > p.remote_km {
        floor = floor.max(Level::Moyen);
        push(
            Level::Moyen,
            Factor::Remote,
            tx.remote(m.max_exit_km, m.max_exit_at_km),
            Some(m.max_exit_at_km),
        );
    }
    if let Some(s) = m.no_network.first()
        && s.length_m / 1000.0 > p.no_network_km
    {
        floor = floor.max(Level::Moyen);
        push(
            Level::Moyen,
            Factor::Network,
            tx.no_network(&tx.length(s.length_m), s.start_km),
            Some(s.start_km),
        );
    }
    if m.forbidden_km > 0.0 {
        push(
            Level::Facile,
            Factor::Zones,
            tx.forbidden(&tx.length(m.forbidden_km * 1000.0)),
            m.zones.first().map(|z| z.start_km),
        );
    }
    if let Some(c) = c {
        if snow >= 20.0 {
            push(
                effort_counted,
                Factor::Conditions,
                tx.fresh_snow(snow),
                None,
            );
        }
        if c.icy || c.crust {
            push(technical, Factor::Conditions, tx.icy(c.icy), None);
        }
    }
    if m.coarse_share > 0.3 {
        push(
            Level::Facile,
            Factor::Data,
            tx.coarse(100.0 * m.coarse_share, p.coarse_m),
            None,
        );
    }
    if no_relief && m.exposure_unknown_km <= 0.05 {
        push(
            Level::Facile,
            Factor::Data,
            tx.relief_unknown(100.0 * m.unknown_elevation_share),
            None,
        );
    }

    let level = technical.max(effort_counted).max(floor).max(ava_floor);
    reasons.sort_by(|a, b| {
        b.level
            .cmp(&a.level)
            .then_with(|| factor_rank(a.factor).cmp(&factor_rank(b.factor)))
    });
    Rating {
        level,
        technical,
        effort: if raid {
            Level::TresDifficile
        } else {
            effort_scale
        },
        beyond_srn: beyond,
        raid,
        effort_counted,
        avalanche_floor: ava_floor,
        other_floor: floor,
        reasons,
    }
}

fn factor_rank(f: Factor) -> u8 {
    match f {
        Factor::Beyond => 0,
        Factor::Avalanche => 1,
        Factor::Descent => 2,
        Factor::Forest => 3,
        Factor::Sidehill => 4,
        Factor::Climb => 5,
        Factor::Conditions => 6,
        Factor::Effort => 7,
        Factor::Remote => 8,
        Factor::Network => 9,
        Factor::Zones => 10,
        Factor::Data => 11,
    }
}

/// `2 h 05` (French).
pub fn hours(h: f64) -> String {
    text::of(Lang::Fr).hours(h)
}

/// Measures and rates both directions, reasons in French.
pub fn rate_profile(
    profile: &Profile,
    p: &Params,
    c: Option<&Conditions>,
) -> [(Metrics, Rating); 2] {
    rate_profile_in(profile, p, c, Lang::Fr)
}

/// Measures and rates both directions, reasons in `lang`.
pub fn rate_profile_in(
    profile: &Profile,
    p: &Params,
    c: Option<&Conditions>,
    lang: Lang,
) -> [(Metrics, Rating); 2] {
    let ms = p.measure();
    [Direction::Forward, Direction::Reverse].map(|d| {
        let m = analysis::measure(profile, d, &ms);
        let r = rate_in(&m, p, c, lang);
        (m, r)
    })
}

/// Whether a passage is a descent.
pub fn is_descent(p: &Passage) -> bool {
    p.kind == PassageKind::Descent
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::analysis::tests::synthetic;

    /// Parameters of the rule mechanics tests (independent of the calibration).
    pub(crate) fn tp() -> Params {
        Params {
            effort: Effort::RaidOnly,
            forest_cover_pct: Some(70.0),
            ..Params::scale()
        }
    }

    fn level(pieces: &[(f64, f64)]) -> (Level, Level) {
        let p = synthetic(pieces);
        let [(_, f), (_, r)] = rate_profile(&p, &tp(), None);
        (f.level, r.level)
    }

    #[test]
    fn levels_follow_the_scale_bands() {
        assert_eq!(
            level(&[(2000.0, 0.0), (300.0, 6.0), (300.0, -10.0)]).0,
            Level::Facile
        );
        assert_eq!(level(&[(2000.0, 0.0), (200.0, -17.0)]).0, Level::Moyen);
        assert_eq!(level(&[(2000.0, 0.0), (200.0, -22.0)]).0, Level::Difficile);
        assert_eq!(
            level(&[(2000.0, 0.0), (200.0, -27.0)]).0,
            Level::TresDifficile
        );
    }

    #[test]
    fn direction_matters_for_one_way_tracks() {
        // Steep climb of 22°, gentle descents: the climb is the hardest part forward,
        // the 22° descent backwards. Both difficile on the scale (slopes, not direction).
        let (f, r) = level(&[(1000.0, 0.0), (150.0, 22.0), (2000.0, -4.0)]);
        assert_eq!(f, Level::Difficile);
        assert_eq!(r, Level::Difficile);
    }

    #[test]
    fn never_easy_through_exposed_terrain() {
        let mut p = synthetic(&[(2000.0, 0.0)]);
        p.inputs.exposure = true;
        for s in &mut p.samples {
            s.exposure = Some(1);
        }
        let [(_, f), _] = rate_profile(&p, &tp(), None);
        assert_eq!(f.level, Level::Facile);
        p.samples[50].exposure = Some(2);
        let [(_, f), (_, r)] = rate_profile(&p, &tp(), None);
        assert_eq!(f.level, Level::Moyen);
        assert_eq!(r.level, Level::Moyen);
        assert_eq!(f.reasons[0].factor, Factor::Avalanche);
        p.samples[60].exposure = Some(3);
        let [(_, f), _] = rate_profile(&p, &tp(), None);
        assert_eq!(f.level, Level::Difficile);
    }

    #[test]
    fn raid_and_conditions_raise_the_level() {
        let long = synthetic(&[(35000.0, 0.0)]);
        let [(m, f), _] = rate_profile(&long, &tp(), None);
        assert!(m.length_km > 30.0);
        assert!(f.raid && f.level == Level::TresDifficile);
        let p = synthetic(&[(2000.0, 0.0), (200.0, -13.0)]);
        let [(_, f), _] = rate_profile(&p, &tp(), None);
        assert_eq!(f.level, Level::Facile);
        let icy = Conditions {
            icy: true,
            ..Conditions::default()
        };
        let [(_, f), _] = rate_profile(&p, &tp(), Some(&icy));
        assert_eq!(f.level, Level::Moyen);
    }

    #[test]
    fn calibrated_defaults_keep_the_avalanche_floor() {
        let mut p = synthetic(&[(2000.0, 0.0)]);
        p.inputs.exposure = true;
        for s in &mut p.samples {
            s.exposure = Some(1);
        }
        p.samples[70].exposure = Some(3);
        let [(_, f), (_, r)] = rate_profile(&p, &Params::default(), None);
        assert_eq!(f.level, Level::Moyen);
        assert_eq!(r.level, Level::Moyen);
        assert_eq!(f.avalanche_floor, Level::Moyen);
        // Even a floor set below moyen by mistake keeps « never facile ».
        let low = Params {
            high_exposure_floor: Level::Facile,
            ..Params::default()
        };
        let [(_, f), _] = rate_profile(&p, &low, None);
        assert_eq!(f.level, Level::Moyen);
    }

    #[test]
    fn dense_forest_descent_is_harder_than_open_one() {
        // 200 m descending at 12°: facile in the open (below 15°), difficile in dense
        // forest without an opening (8 to 15°), facile again on a forest road (opening).
        let mut p = synthetic(&[(2000.0, 0.0), (200.0, -12.0), (500.0, 0.0)]);
        let [(_, f), _] = rate_profile(&p, &tp(), None);
        assert_eq!(f.technical, Level::Facile);
        for s in &mut p.samples {
            s.cover = Some(100.0);
            s.min_h = Some(12.0);
        }
        let [(_, f), (_, r)] = rate_profile(&p, &tp(), None);
        assert_eq!(f.technical, Level::Difficile);
        assert_eq!(f.reasons[0].factor, Factor::Forest);
        // Backwards it is a climb: the forest does not matter uphill.
        assert_eq!(r.technical, Level::Facile);
        for s in &mut p.samples {
            s.min_h = Some(0.5);
        }
        let [(_, f), _] = rate_profile(&p, &tp(), None);
        assert_eq!(f.technical, Level::Facile);
    }

    #[test]
    fn literal_scale_counts_the_effort() {
        let p = synthetic(&[(16000.0, 0.0)]);
        let [(_, f), _] = rate_profile(&p, &Params::scale(), None);
        assert_eq!(f.level, Level::Difficile);
        // Calibrated: effort counts only for a raid, but the middle of 16 km is 8 km from
        // either end: at least moyen for the isolation.
        let [(_, f), _] = rate_profile(&p, &tp(), None);
        assert_eq!(f.level, Level::Moyen);
        assert_eq!(f.reasons[0].factor, Factor::Remote);
    }
}
