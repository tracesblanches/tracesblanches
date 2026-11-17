//! Advice and text for both directions of travel, in the language asked ([`assess_in`]).

use std::fmt::Write as _;

use serde::{Deserialize, Serialize};

use crate::analysis::{self, Direction, Measure, Metrics, Passage};
use crate::profile::Profile;
use crate::rules::{self, Conditions, Factor, Level, Params, Rating};
use crate::text::{self, Lang, Texts};

/// Topic of an advice.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Topic {
    /// Skins or fish scales.
    Grip,
    /// Downhill and uphill technique.
    Technique,
    /// Glide on long flats.
    Glide,
    /// Passages to avoid.
    Avoid,
    /// Turning back.
    TurnBack,
    /// Equipment and precautions.
    Equipment,
    /// Snow and weather by aspect and altitude.
    Snow,
}

/// One piece of advice.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Advice {
    /// Topic.
    pub topic: Topic,
    /// Text, in the language asked ([`advise_in`]).
    pub text: String,
}

/// One direction: measurements, level, advice.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DirectionReport {
    /// Measurements.
    pub metrics: Metrics,
    /// Level and reasons.
    pub rating: Rating,
    /// Advice.
    pub advice: Vec<Advice>,
}

/// Both directions.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Report {
    /// In the order of the track.
    pub forward: DirectionReport,
    /// Reversed.
    pub reverse: DirectionReport,
}

/// Fixed sentence closing every text: the level is a computed indication.
pub const DISCLAIMER: &str = "Indication calculée sur le relief et les cartes, jamais une garantie : la neige, la météo et le bulletin d'avalanche décident sur place.";

/// The direction a slope faces, from its aspect: north, east, south or west.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Facing {
    East,
    North,
    South,
    West,
}

fn facing(aspect: f32) -> Option<Facing> {
    let a = aspect.rem_euclid(360.0);
    if !(0.0..360.0).contains(&a) {
        return None;
    }
    Some(if !(45.0..315.0).contains(&a) {
        Facing::North
    } else if a < 135.0 {
        Facing::East
    } else if a < 225.0 {
        Facing::South
    } else {
        Facing::West
    })
}

fn list_passages(tx: &dyn Texts, ps: &[&Passage]) -> String {
    ps.iter()
        .take(4)
        .map(|p| tx.passage(p.start_km, p.max_deg, p.length_m))
        .collect::<Vec<_>>()
        .join(", ")
}

/// Advice for one direction, in French.
pub fn advise(m: &Metrics, r: &Rating, other: &Rating, c: Option<&Conditions>) -> Vec<Advice> {
    advise_in(m, r, other, c, Lang::Fr)
}

/// Advice for one direction, in `lang`.
pub fn advise_in(
    m: &Metrics,
    r: &Rating,
    other: &Rating,
    c: Option<&Conditions>,
    lang: Lang,
) -> Vec<Advice> {
    let tx = text::of(lang);
    let mut out = Vec::new();
    let mut add = |topic: Topic, text: String| {
        out.push(Advice {
            topic,
            text: tx.finish(text),
        })
    };
    let slippery = c.is_some_and(|c| c.icy || c.fresh_snow_cm.is_some_and(|s| s >= 20.0));

    // Grip.
    let skin_from = if slippery { 8.0 } else { 12.0 };
    // A climb counts for the skins when it is steep over at least 60 m (a shorter step is
    // climbed in herringbone or side steps).
    let steep_climbs: Vec<&Passage> = m
        .climbs
        .iter()
        .filter(|p| p.max_deg >= skin_from && p.length_m >= 60.0)
        .collect();
    if !steep_climbs.is_empty() {
        let longest = steep_climbs
            .iter()
            .max_by(|a, b| a.length_m.total_cmp(&b.length_m))
            .expect("non-empty");
        let total: f64 = steep_climbs.iter().map(|p| p.length_m).sum();
        add(
            Topic::Grip,
            tx.grip_skins(
                steep_climbs.len(),
                f64::from(skin_from),
                &tx.length(total),
                longest.length_m,
                longest.max_deg,
                longest.start_km,
            ),
        );
    } else if m.climbs.is_empty() {
        add(Topic::Grip, tx.grip_gentle());
    } else {
        add(Topic::Grip, tx.grip_scales(f64::from(skin_from)));
    }
    if slippery {
        add(Topic::Grip, tx.grip_slippery());
    }
    add(Topic::Grip, tx.grip_keep_skins());

    // Technique downhill.
    add(Topic::Technique, tx.technique(m.max_descent_deg));
    let hard_desc: Vec<&Passage> = m.descents.iter().filter(|p| p.max_deg >= 15.0).collect();
    if !hard_desc.is_empty() {
        add(
            Topic::Technique,
            tx.steep_descents(&list_passages(tx, &hard_desc)),
        );
    }
    let forest: Vec<&Passage> = m
        .descents
        .iter()
        .filter(|p| p.dense_forest_m >= 30.0)
        .collect();
    if !forest.is_empty() {
        add(
            Topic::Technique,
            tx.forest_descents(&list_passages(tx, &forest)),
        );
    }
    let conv: Vec<&Passage> = m.climbs.iter().filter(|p| p.max_deg >= 15.0).collect();
    if !conv.is_empty() {
        add(Topic::Technique, tx.kick_turns(&list_passages(tx, &conv)));
    }
    if !m.sidehills.is_empty() {
        add(
            Topic::Technique,
            tx.sidehills(
                &m.sidehills
                    .iter()
                    .take(4)
                    .map(|p| tx.sidehill_passage(p.start_km, p.terrain_max_deg, p.length_m))
                    .collect::<Vec<_>>()
                    .join(", "),
            ),
        );
    }

    // Glide.
    if m.flat_km >= 3.0 && m.flat_km / m.length_km.max(0.1) >= 0.4 {
        add(Topic::Glide, tx.glide(m.flat_km, m.longest_flat_km));
    }

    // Passages to avoid.
    let beyond: Vec<&Passage> = m
        .descents
        .iter()
        .chain(&m.climbs)
        .filter(|p| p.max_deg >= 30.0)
        .collect();
    if !beyond.is_empty() {
        add(
            Topic::Avoid,
            tx.avoid_beyond(
                &beyond
                    .iter()
                    .take(4)
                    .map(|p| tx.km(p.start_km))
                    .collect::<Vec<_>>()
                    .join(", "),
            ),
        );
    }
    // The four longest « très exposé » stretches, in the order of travel.
    let mut longest: Vec<&crate::analysis::Stretch> = m.high_exposed.iter().collect();
    longest.sort_by(|a, b| b.length_m.total_cmp(&a.length_m));
    longest.truncate(4);
    longest.sort_by(|a, b| a.start_km.total_cmp(&b.start_km));
    let high: Vec<String> = longest
        .iter()
        .map(|s| tx.stretch(s.start_km, &tx.length(s.length_m)))
        .collect();
    if !high.is_empty() {
        add(Topic::Avoid, tx.avoid_high_exposed(&high.join(", ")));
    }
    if m.forbidden_km > 0.0 {
        add(
            Topic::Avoid,
            tx.avoid_forbidden(&tx.length(m.forbidden_km * 1000.0)),
        );
    }
    if m.leave_routes_forbidden_km > 0.0 {
        add(
            Topic::Avoid,
            tx.avoid_leave_routes(
                &tx.length(m.leave_routes_forbidden_km * 1000.0),
                m.zones
                    .iter()
                    .find(|z| !z.label.is_empty())
                    .map(|z| z.label.as_str()),
            ),
        );
    }
    if m.avoid_km > 0.0 {
        add(
            Topic::Avoid,
            tx.avoid_quiet(&tx.length(m.avoid_km * 1000.0)),
        );
    }

    // Turning back.
    let key = r
        .reasons
        .iter()
        .find(|x| {
            x.level == r.level
                && x.at_km.is_some()
                && matches!(
                    x.factor,
                    Factor::Descent | Factor::Climb | Factor::Forest | Factor::Sidehill
                )
        })
        .and_then(|x| x.at_km);
    if let Some(at) = key
        && r.level > Level::Facile
        && at > 0.5 * m.length_km
    {
        add(Topic::TurnBack, tx.turn_back_hardest(at));
    }
    if m.max_exit_km >= 3.0 {
        add(
            Topic::TurnBack,
            tx.turn_back_far(m.max_exit_at_km, m.max_exit_km),
        );
    }
    if other.level != r.level {
        add(
            Topic::TurnBack,
            tx.other_direction(other.level < r.level, other.level),
        );
    }

    // Equipment.
    if m.exposed_km + m.high_exposed_km > 0.0 {
        add(Topic::Equipment, tx.equip_avalanche());
    }
    if let Some(s) = m.no_network.first()
        && s.length_m >= 500.0
    {
        add(
            Topic::Equipment,
            tx.equip_no_network(&tx.length(m.no_network_km.unwrap_or(0.0) * 1000.0)),
        );
    }
    if r.raid {
        add(
            Topic::Equipment,
            tx.equip_raid(&tx.hours(m.munter_h), (m.munter_h / 6.0).ceil().max(2.0)),
        );
    } else if m.munter_h >= 5.0 {
        add(Topic::Equipment, tx.equip_long_day(&tx.hours(m.munter_h)));
    }
    if let Some(o) = m.longest_open_km
        && o >= 2.0
    {
        add(Topic::Equipment, tx.equip_open(o));
    }

    // Snow by aspect.
    let steep_desc: Vec<&Passage> = m.descents.iter().filter(|p| p.max_deg >= 12.0).collect();
    let mut aspects: Vec<Facing> = steep_desc
        .iter()
        .filter_map(|p| p.aspect_deg.and_then(facing))
        .collect();
    aspects.sort_unstable();
    aspects.dedup();
    for a in aspects {
        match a {
            Facing::North => add(Topic::Snow, tx.snow_north()),
            Facing::South => add(Topic::Snow, tx.snow_south()),
            Facing::East | Facing::West => {}
        }
    }
    out
}

/// Measurement of the passages quoted in the advice: steepness over 50 m (short pitches
/// matter for skins and technique), forest from 70 % cover, side hill from 25°,
/// independent of the calibrated window of the level.
pub fn detail(p: &Params) -> Measure {
    Measure {
        sustain_m: 50.0,
        forest_cover_pct: 70.0,
        opening_h: 2.0,
        sidehill_deg: 25.0,
        coarse_m: p.coarse_m,
    }
}

/// Measures, rates and advises both directions in French: the level comes from the
/// calibrated measurement ([`Params::measure`]), the advice and the metrics of the report
/// from [`detail`].
pub fn assess(profile: &Profile, p: &Params, c: Option<&Conditions>) -> Report {
    assess_in(profile, p, c, Lang::Fr)
}

/// [`assess`] with the reasons and the advice in `lang`.
pub fn assess_in(profile: &Profile, p: &Params, c: Option<&Conditions>, lang: Lang) -> Report {
    let [(_, rf), (_, rr)] = rules::rate_profile_in(profile, p, c, lang);
    let d = detail(p);
    let mf = analysis::measure(profile, Direction::Forward, &d);
    let mr = analysis::measure(profile, Direction::Reverse, &d);
    let af = advise_in(&mf, &rf, &rr, c, lang);
    let ar = advise_in(&mr, &rr, &rf, c, lang);
    Report {
        forward: DirectionReport {
            metrics: mf,
            rating: rf,
            advice: af,
        },
        reverse: DirectionReport {
            metrics: mr,
            rating: rr,
            advice: ar,
        },
    }
}

impl Report {
    /// Hardest direction level.
    pub fn level(&self) -> Level {
        self.forward.rating.level.max(self.reverse.rating.level)
    }

    /// French text of both directions ([`Report::text`]).
    pub fn text_fr(&self, name: &str) -> String {
        self.text(Lang::Fr, name)
    }

    /// Text of both directions in `lang` (that of [`assess_in`]); the other direction
    /// lists only the reasons and advice that differ from the direction of the track.
    pub fn text(&self, lang: Lang, name: &str) -> String {
        let tx = text::of(lang);
        let mut s = String::new();
        let _ = writeln!(s, "{name}");
        for (i, d) in [&self.forward, &self.reverse].into_iter().enumerate() {
            let m = &d.metrics;
            let r = &d.rating;
            let line = tx.direction(
                i == 0,
                r.level,
                r.beyond_srn,
                r.technical,
                r.effort,
                m.length_km,
                m.ascent_m,
                m.descent_m,
                &tx.hours(m.munter_h),
            );
            let _ = writeln!(s, "{}", tx.finish(line));
            let fwd = &self.forward;
            let reasons: Vec<_> = r
                .reasons
                .iter()
                .filter(|x| i == 0 || !fwd.rating.reasons.iter().any(|y| y.text == x.text))
                .take(6)
                .collect();
            let advice: Vec<_> = d
                .advice
                .iter()
                .filter(|a| i == 0 || !fwd.advice.iter().any(|b| b.text == a.text))
                .collect();
            if !reasons.is_empty() {
                let _ = writeln!(s, "{}", tx.reasons_title(i == 0));
                for x in reasons {
                    let _ = writeln!(s, "  - [{}] {}", tx.level(x.level), x.text);
                }
            }
            if !advice.is_empty() {
                let _ = writeln!(s, "{}", tx.advice_title(i == 0));
                for a in advice {
                    let _ = writeln!(s, "  - {}", a.text);
                }
            }
        }
        let _ = write!(s, "{}", tx.disclaimer());
        s
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::analysis::tests::synthetic;

    #[test]
    fn text_has_both_directions_and_never_says_safe() {
        let p = synthetic(&[(1000.0, 0.0), (300.0, 16.0), (4000.0, 0.0), (400.0, -9.0)]);
        let r = assess(&p, &crate::rules::tests::tp(), None);
        let t = r.text_fr("Essai");
        assert!(t.contains("Dans le sens du tracé : MOYEN"), "{t}");
        assert!(t.contains("Dans l'autre sens"), "{t}");
        assert!(t.contains("Peaux courtes"), "{t}");
        assert!(t.contains("Conversions"), "{t}");
        for bad in ["sans danger", "sûr ", "sécurisé"] {
            assert!(!t.contains(bad), "{bad} in {t}");
        }
        assert!(t.ends_with(DISCLAIMER));
    }

    #[test]
    fn long_flat_gets_glide_advice() {
        let p = synthetic(&[(8000.0, 0.0), (200.0, 5.0)]);
        let r = assess(&p, &crate::rules::tests::tp(), None);
        assert!(r.forward.advice.iter().any(|a| a.topic == Topic::Glide));
        assert_eq!(r.level(), Level::Facile);
    }
}
