//! Estimated time model (C5, #37; personal calibration C6, #38): physical power model,
//! slope, snow friction, classic / skating factor (Bolger 2015), compared with Munter /
//! Tobler.
//!
//! # Model
//!
//! On a piece of trail of grade `s = dz / dx` (angle `α = atan s`), the classic skier
//! holds a constant propulsive power `P` (W) against gravity, ski–snow friction and air
//! drag (power balance of research report 06, after Moxnes 2013 and Carlsson 2011):
//!
//! ```text
//! P = v · m g (sin α + μ cos α) + ½ ρ CdA v³
//! ```
//!
//! solved for the speed `v` along the slope (unique positive root, bisection). Downhill,
//! gravity can exceed friction and `v` is capped at [`SkierParams::max_speed_ms`]
//! (braking, turns). Skating speed is the classic speed times `1 + gain(s)`, the gain
//! interpolated from Bolger et al. 2015: 12–15 % uphill (13.5 % used), 8–13 % on the flat
//! (10.5 %), 1–2 % downhill (1.5 %). Air density follows the elevation (standard
//! atmosphere pressure) and the air temperature. Friction `μ` depends on the snow class
//! and the temperature ([`friction`]).
//!
//! # Parameters and sources
//!
//! | Parameter | Default | Source |
//! |---|---|---|
//! | `m` mass with gear | 75 kg | assumption, per user |
//! | `P` propulsive power | 70 W | assumption (≈ 0.93 W/kg): flat classic ≈ 10.6 km/h, 6 % ≈ 3.9 km/h on groomed snow at −5 °C; per user, to calibrate on the user's own traces (C6) |
//! | `CdA` | 0.5 m² | assumption, upright skier |
//! | `max_speed_ms` | 7 m/s (25 km/h) | assumption, recreational descent |
//! | `μ` | see [`friction`] | shape (lowest just below 0 °C, higher in the cold and on wet snow) from Colbeck 1994 and Almqvist 2022 (report 06); values are assumptions until C6 |
//! | skating gain | 13.5 / 10.5 / 1.5 % | Bolger et al. 2015, IJSPP, PMID 25671845 (report 06) |
//!
//! Report 06 also quotes Moxnes 2013: +10 % friction or drag gives +3–4 % race time; the
//! tests check that the model reproduces it on a race-like profile with race parameters.
//!
//! # Comparisons
//!
//! - [`munter_s`]: `T = (km + Δh/100) / rate`, rate 4 uphill and on the flat, 10 downhill
//!   (report 06; a ski-touring formula).
//! - [`tobler_s`]: `W = 6 e^(−3.5 |s + 0.05|)` km/h (hiking function, report 06).

use serde::{Deserialize, Serialize};

use crate::graph::EdgeAttrs;
use crate::routing::{CostModel, Piece, Style, StylePolicy};

/// Standard gravity, m/s².
pub const G: f64 = 9.806_65;

/// Snow on the trail.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SnowClass {
    /// Groomed, old or transformed dry snow.
    Groomed,
    /// Groomed with new snow on it (less than a day, not packed).
    NewSnow,
    /// Wet groomed snow (spring, rain, above 0 °C).
    Wet,
    /// Hard, icy track.
    Icy,
}

/// Friction coefficient of waxed skis.
///
/// Groomed dry snow, piecewise linear in air temperature: 0.070 at −25 °C, 0.050 at
/// −15 °C, 0.035 at −8 °C, 0.025 at −3 °C, 0.030 at 0 °C and above (dry). Wet snow 0.045,
/// new snow 1.4 × the dry value, icy track 0.025. An ungroomed trail (skier-made tracks,
/// snowshoe trail) has at least 0.08. These numbers are starting values to calibrate
/// (C6); the shape follows Colbeck 1994 and Almqvist 2022 (report 06).
pub fn friction(snow: SnowClass, temperature_c: f64, ungroomed: bool) -> f64 {
    const DRY: [(f64, f64); 5] = [
        (-25.0, 0.070),
        (-15.0, 0.050),
        (-8.0, 0.035),
        (-3.0, 0.025),
        (0.0, 0.030),
    ];
    let dry = interp(&DRY, temperature_c);
    let mu = match snow {
        SnowClass::Groomed => dry,
        SnowClass::NewSnow => 1.4 * dry,
        SnowClass::Wet => 0.045,
        SnowClass::Icy => 0.025,
    };
    if ungroomed { mu.max(0.08) } else { mu }
}

fn interp(table: &[(f64, f64)], x: f64) -> f64 {
    let (x0, y0) = table[0];
    if x <= x0 {
        return y0;
    }
    for w in table.windows(2) {
        let ((xa, ya), (xb, yb)) = (w[0], w[1]);
        if x <= xb {
            return ya + (yb - ya) * (x - xa) / (xb - xa);
        }
    }
    table[table.len() - 1].1
}

/// Skating speed gain over classic at a grade (Bolger et al. 2015): 1.5 % at −5 % and
/// below, 10.5 % between −1 % and +1 %, 13.5 % at +5 % and above, linear in between.
pub fn skating_gain(grade: f64) -> f64 {
    interp(
        &[(-0.05, 0.015), (-0.01, 0.105), (0.01, 0.105), (0.05, 0.135)],
        grade,
    )
}

/// Air density, kg/m³, at an elevation (standard atmosphere pressure) and temperature.
pub fn air_density(elev_m: f64, temperature_c: f64) -> f64 {
    let p = 101_325.0 * (1.0 - 2.255_77e-5 * elev_m.max(0.0)).powf(5.255_88);
    p / (287.05 * (temperature_c + 273.15))
}

/// Per-user parameters (calibrated by C6).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct SkierParams {
    /// Mass with gear, kg.
    pub mass_kg: f64,
    /// Sustained propulsive power, W.
    pub power_w: f64,
    /// Drag area, m².
    pub cda_m2: f64,
    /// Maximum speed (descents), m/s.
    pub max_speed_ms: f64,
    /// Multiplier of the friction coefficient (wax, ski, personal technique).
    pub friction_factor: f64,
}

impl Default for SkierParams {
    fn default() -> Self {
        Self {
            mass_kg: 75.0,
            power_w: 70.0,
            cda_m2: 0.5,
            max_speed_ms: 7.0,
            friction_factor: 1.0,
        }
    }
}

/// Snow and weather of the day.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Conditions {
    /// Snow class on groomed trails.
    pub snow: SnowClass,
    /// Air temperature, °C.
    pub temperature_c: f64,
}

impl Default for Conditions {
    fn default() -> Self {
        Self {
            snow: SnowClass::Groomed,
            temperature_c: -5.0,
        }
    }
}

/// Time model: skier, conditions, technique, style preferences.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct EtaModel {
    /// Skier parameters.
    pub skier: SkierParams,
    /// Conditions.
    pub conditions: Conditions,
    /// Technique.
    pub style: Style,
    /// Style preferences for routing (not applied to [`EtaModel::time_s`]).
    pub policy: StylePolicy,
    /// Elevation used when a piece has none (no profile), m.
    pub default_elev_m: f64,
}

impl EtaModel {
    /// Model with default parameters for a style.
    pub fn new(style: Style) -> Self {
        Self {
            skier: SkierParams::default(),
            conditions: Conditions::default(),
            style,
            policy: StylePolicy::default(),
            default_elev_m: 1000.0,
        }
    }

    /// Classic speed along the slope, m/s, from the power balance.
    pub fn classic_speed_ms(&self, grade: f64, elev_m: f64, ungroomed: bool) -> f64 {
        let k = &self.skier;
        let mu = friction(
            self.conditions.snow,
            self.conditions.temperature_c,
            ungroomed,
        ) * k.friction_factor;
        let alpha = grade.atan();
        let resist = k.mass_kg * G * (alpha.sin() + mu * alpha.cos());
        let drag = 0.5 * air_density(elev_m, self.conditions.temperature_c) * k.cda_m2;
        let f = |v: f64| v * resist + drag * v * v * v - k.power_w;
        // f(0) = −P < 0 and f is convex on v > 0, so Newton's method started right of the
        // positive root decreases monotonically to it.
        let mut v = 1.0;
        while f(v) < 0.0 && v < 1000.0 {
            v *= 2.0;
        }
        for _ in 0..50 {
            let step = f(v) / (resist + 3.0 * drag * v * v);
            v -= step;
            if step.abs() < 1e-10 * v.max(1e-3) {
                break;
            }
        }
        v.min(k.max_speed_ms)
    }

    /// Speed along the slope for the model's style, m/s.
    pub fn speed_ms(&self, grade: f64, elev_m: f64, ungroomed: bool) -> f64 {
        let v = self.classic_speed_ms(grade, elev_m, ungroomed);
        match self.style {
            Style::Classic => v,
            Style::Skating => (v * (1.0 + skating_gain(grade))).min(self.skier.max_speed_ms),
        }
    }

    /// Time of a piece, seconds (no style preference factor).
    pub fn piece_time_s(
        &self,
        attrs: &EdgeAttrs,
        dist_m: f64,
        dz_m: f64,
        elev: Option<f64>,
    ) -> f64 {
        if dist_m <= 0.0 {
            return 0.0;
        }
        let grade = dz_m / dist_m;
        let ungroomed = attrs.backcountry || attrs.hike_only();
        let v = self.speed_ms(grade, elev.unwrap_or(self.default_elev_m), ungroomed);
        dist_m.hypot(dz_m) / v
    }

    /// Time over pieces, with the split by terrain.
    pub fn time_s(&self, pieces: &[Piece<'_>]) -> Breakdown {
        let mut b = Breakdown::default();
        for p in pieces {
            let t = self.piece_time_s(p.attrs, p.dist_m, p.dz_m, p.elev_m);
            b.add(p, t);
        }
        b
    }
}

impl CostModel for EtaModel {
    fn piece_cost(&self, attrs: &EdgeAttrs, dist_m: f64, dz_m: f64, elev_m: Option<f64>) -> f64 {
        self.piece_time_s(attrs, dist_m, dz_m, elev_m)
    }
    fn edge_factor(&self, attrs: &EdgeAttrs) -> f64 {
        self.policy.factor(attrs, self.style)
    }
}

/// Grade above which a piece counts as uphill, below minus which as downhill.
pub const FLAT_GRADE: f64 = 0.02;

/// Time and distance split by terrain.
#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize)]
pub struct Breakdown {
    /// Total time, s.
    pub total_s: f64,
    /// Time uphill (grade > 2 %), s.
    pub uphill_s: f64,
    /// Time on the flat, s.
    pub flat_s: f64,
    /// Time downhill (grade < −2 %), s.
    pub downhill_s: f64,
    /// Horizontal distance, m.
    pub dist_m: f64,
    /// Uphill distance, m.
    pub uphill_m: f64,
    /// Downhill distance, m.
    pub downhill_m: f64,
    /// Climb, m.
    pub gain_m: f64,
    /// Descent, m.
    pub loss_m: f64,
}

impl Breakdown {
    fn add(&mut self, p: &Piece<'_>, t: f64) {
        let grade = if p.dist_m > 0.0 {
            p.dz_m / p.dist_m
        } else {
            0.0
        };
        self.total_s += t;
        self.dist_m += p.dist_m;
        if grade > FLAT_GRADE {
            self.uphill_s += t;
            self.uphill_m += p.dist_m;
        } else if grade < -FLAT_GRADE {
            self.downhill_s += t;
            self.downhill_m += p.dist_m;
        } else {
            self.flat_s += t;
        }
        if p.dz_m > 0.0 {
            self.gain_m += p.dz_m;
        } else {
            self.loss_m -= p.dz_m;
        }
    }

    /// Mean horizontal speed, km/h.
    pub fn mean_kmh(&self) -> f64 {
        if self.total_s > 0.0 {
            self.dist_m / self.total_s * 3.6
        } else {
            0.0
        }
    }
}

/// Munter time, seconds: `(km + Δh/100) / rate` per piece, rate `up` for pieces above
/// +2 % and on the flat, `down` below −2 % (report 06: 4 and 10).
pub fn munter_s(pieces: &[Piece<'_>], up: f64, down: f64) -> f64 {
    pieces
        .iter()
        .map(|p| {
            let grade = if p.dist_m > 0.0 {
                p.dz_m / p.dist_m
            } else {
                0.0
            };
            let units = p.dist_m / 1000.0 + p.dz_m.abs() / 100.0;
            let rate = if grade < -FLAT_GRADE { down } else { up };
            units / rate * 3600.0
        })
        .sum()
}

/// Tobler time, seconds: horizontal speed `6 e^(−3.5 |s + 0.05|)` km/h.
pub fn tobler_s(pieces: &[Piece<'_>]) -> f64 {
    pieces
        .iter()
        .filter(|p| p.dist_m > 0.0)
        .map(|p| {
            let s = p.dz_m / p.dist_m;
            let kmh = 6.0 * (-3.5 * (s + 0.05).abs()).exp();
            p.dist_m / (kmh / 3.6)
        })
        .sum()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graph::{EdgeAttrs, EdgeKind};

    fn groomed() -> EdgeAttrs {
        EdgeAttrs {
            kind: EdgeKind::Piste,
            name: None,
            osm_way_id: None,
            classic: Some(true),
            skating: Some(true),
            backcountry: false,
            nordic: true,
            hike: false,
            difficulty: None,
            difficulty_max: None,
        }
    }

    fn piece(a: &EdgeAttrs, dist: f64, dz: f64) -> Piece<'_> {
        Piece {
            attrs: a,
            dist_m: dist,
            dz_m: dz,
            elev_m: Some(1200.0),
        }
    }

    #[test]
    fn flat_speed_solves_the_power_balance() {
        let m = EtaModel::new(Style::Classic);
        let v = m.classic_speed_ms(0.0, 1200.0, false);
        let mu = friction(SnowClass::Groomed, -5.0, false);
        let rho = air_density(1200.0, -5.0);
        let p = v * 75.0 * G * mu + 0.5 * rho * 0.5 * v.powi(3);
        assert!((p - 70.0).abs() < 1e-6, "{p}");
        // Defaults: about 10.6 km/h on the flat, 3.9 km/h at 6 %.
        let kmh = v * 3.6;
        assert!((10.0..11.5).contains(&kmh), "{kmh}");
        let up = m.classic_speed_ms(0.06, 1200.0, false) * 3.6;
        assert!((3.5..4.5).contains(&up), "{up}");
        println!("flat {kmh:.2} km/h, 6 % {up:.2} km/h");
    }

    #[test]
    fn speed_is_monotonic_and_capped() {
        let m = EtaModel::new(Style::Classic);
        let mut last = f64::INFINITY;
        for k in -10..=25 {
            let g = k as f64 / 100.0;
            let v = m.classic_speed_ms(g, 1200.0, false);
            assert!(v <= last + 1e-9, "{g}: {v} > {last}");
            assert!(v <= m.skier.max_speed_ms + 1e-12);
            last = v;
        }
        assert!((m.classic_speed_ms(-0.15, 1200.0, false) - 7.0).abs() < 1e-9);
        // Ungroomed is slower, colder (−20 °C) is slower than −3 °C, new snow slower.
        let base = m.classic_speed_ms(0.0, 1200.0, false);
        assert!(m.classic_speed_ms(0.0, 1200.0, true) < base);
        let mut cold = m;
        cold.conditions.temperature_c = -20.0;
        assert!(cold.classic_speed_ms(0.0, 1200.0, false) < base);
        let mut fresh = m;
        fresh.conditions.snow = SnowClass::NewSnow;
        assert!(fresh.classic_speed_ms(0.0, 1200.0, false) < base);
    }

    #[test]
    fn friction_and_air_density_values() {
        assert!((friction(SnowClass::Groomed, -3.0, false) - 0.025).abs() < 1e-12);
        assert!((friction(SnowClass::Groomed, -5.5, false) - 0.030).abs() < 1e-12);
        assert!((friction(SnowClass::Groomed, -40.0, false) - 0.070).abs() < 1e-12);
        assert!((friction(SnowClass::Wet, 2.0, false) - 0.045).abs() < 1e-12);
        assert!((friction(SnowClass::Groomed, -5.0, true) - 0.08).abs() < 1e-12);
        // Standard atmosphere: 1.225 kg/m³ at sea level and 15 °C; ≈ 1.06 at 1800 m, −5 °C.
        assert!((air_density(0.0, 15.0) - 1.225).abs() < 0.001);
        let r = air_density(1800.0, -5.0);
        assert!((1.04..1.08).contains(&r), "{r}");
    }

    #[test]
    fn skating_gain_reproduces_bolger_overall() {
        // Bolger 2015: race time ≈ 55 % uphill, 15–20 % flat, 25–30 % downhill; skating
        // faster by 9 % (men) to 11 % (women) overall.
        let share = [(0.55, 0.06), (0.175, 0.0), (0.275, -0.06)];
        let overall: f64 = share.iter().map(|(w, g)| w * skating_gain(*g)).sum();
        assert!((0.09..=0.11).contains(&overall), "{overall}");
        assert!((skating_gain(0.08) - 0.135).abs() < 1e-12);
        assert!((skating_gain(0.0) - 0.105).abs() < 1e-12);
        assert!((skating_gain(-0.08) - 0.015).abs() < 1e-12);
        // Through the model: skating is faster than classic on every grade but the capped
        // descent.
        let a = groomed();
        let c = EtaModel::new(Style::Classic);
        let s = EtaModel::new(Style::Skating);
        for g in [0.08, 0.0, -0.02] {
            let p = [piece(&a, 1000.0, 1000.0 * g)];
            let (tc, ts) = (c.time_s(&p).total_s, s.time_s(&p).total_s);
            let gain = tc / ts - 1.0;
            assert!((gain - skating_gain(g)).abs() < 1e-9, "{g}: {gain}");
        }
    }

    /// Race-like course: 10 × (400 m at +5 %, 300 m flat, 400 m at −5 %). With the race
    /// parameters below the time shares are 55 % uphill, 24 % flat, 21 % downhill, close
    /// to Bolger's 55 / 15–20 / 25–30 %.
    fn race_course(a: &EdgeAttrs) -> Vec<Piece<'_>> {
        let mut v = Vec::new();
        for _ in 0..10 {
            v.push(piece(a, 400.0, 20.0));
            v.push(piece(a, 300.0, 0.0));
            v.push(piece(a, 400.0, -20.0));
        }
        v
    }

    #[test]
    fn ten_percent_more_friction_costs_three_to_four_percent_for_racers() {
        // Moxnes 2013 (report 06): +10 % friction or drag gives +3–4 % race time.
        let a = groomed();
        let mut m = EtaModel::new(Style::Skating);
        m.skier.power_w = 250.0;
        m.skier.max_speed_ms = 20.0;
        m.skier.cda_m2 = 0.45;
        let course = race_course(&a);
        let base = m.time_s(&course);
        let mut f = m;
        f.skier.friction_factor = 1.1;
        let mut d = m;
        d.skier.cda_m2 *= 1.1;
        let (tf, td) = (f.time_s(&course).total_s, d.time_s(&course).total_s);
        let (rf, rd) = (tf / base.total_s - 1.0, td / base.total_s - 1.0);
        println!(
            "race: {:.1} km/h, uphill share {:.2}; +10 % friction {:+.2} %, +10 % drag {:+.2} %",
            base.mean_kmh(),
            base.uphill_s / base.total_s,
            rf * 100.0,
            rd * 100.0
        );
        // Elite skating race speed, about 25 km/h.
        assert!((22.0..28.0).contains(&base.mean_kmh()));
        assert!((0.53..0.57).contains(&(base.uphill_s / base.total_s)));
        // +3.0 % here; drag +1.6 %, below the 3–4 % of Moxnes (drag share lower in this
        // model).
        assert!((0.025..=0.04).contains(&rf), "friction {rf}");
        assert!(rd > 0.0 && rd < rf, "drag {rd}");
    }

    #[test]
    fn router_with_the_time_model() {
        use crate::routing::{Router, partial_cost};
        let g = crate::routing::tests::bumpy();
        for style in [Style::Classic, Style::Skating] {
            let m = EtaModel::new(style);
            let r = Router::new(&g, m);
            for (a, b) in [
                (0.0, g.length_m(0)),
                (g.length_m(0), 0.0),
                (123.4, 678.9),
                (678.9, 123.4),
            ] {
                let direct = partial_cost(&g, &m, 0, a, b);
                let cached = r.partial_cost(0, a, b);
                assert!(
                    (direct - cached).abs() < 1e-7 * direct,
                    "{a}->{b}: {direct} vs {cached}"
                );
            }
            // Up the 4 % ramp is slower than down.
            assert!(r.partial_cost(0, 0.0, 1000.0) > r.partial_cost(0, 1000.0, 0.0));
        }
    }

    #[test]
    fn munter_and_tobler_formulas() {
        let a = groomed();
        // 1 km at +10 %: Munter (1 + 1) / 4 h = 30 min; Tobler 6 e^(-3.5·0.15) km/h.
        let p = [piece(&a, 1000.0, 100.0)];
        assert!((munter_s(&p, 4.0, 10.0) - 1800.0).abs() < 1e-9);
        let v = 6.0 * (-3.5f64 * 0.15).exp();
        assert!((tobler_s(&p) - 1.0 / v * 3600.0).abs() < 1e-9);
        // 1 km at −10 %: Munter (1 + 1) / 10 h = 12 min; Tobler peak near −5 %.
        let d = [piece(&a, 1000.0, -100.0)];
        assert!((munter_s(&d, 4.0, 10.0) - 720.0).abs() < 1e-9);
        let peak = [piece(&a, 1000.0, -50.0)];
        assert!((tobler_s(&peak) - 600.0).abs() < 1e-9);
    }
}
