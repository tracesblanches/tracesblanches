//! Advice and French text for both directions of travel.

use std::fmt::Write as _;

use serde::{Deserialize, Serialize};

use crate::analysis::{self, Direction, Measure, Metrics, Passage};
use crate::profile::Profile;
use crate::rules::{self, Conditions, Factor, Level, Params, Rating, hours};

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
    /// French text.
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

fn km(v: f64) -> String {
    format!("km {v:.1}")
}

fn facing(aspect: f32) -> Option<&'static str> {
    let a = aspect.rem_euclid(360.0);
    if !(0.0..360.0).contains(&a) {
        return None;
    }
    Some(if !(45.0..315.0).contains(&a) {
        "nord"
    } else if a < 135.0 {
        "est"
    } else if a < 225.0 {
        "sud"
    } else {
        "ouest"
    })
}

fn list_passages(ps: &[&Passage]) -> String {
    ps.iter()
        .take(4)
        .map(|p| {
            format!(
                "{} ({:.0}° sur {:.0} m)",
                km(p.start_km),
                p.max_deg,
                p.length_m
            )
        })
        .collect::<Vec<_>>()
        .join(", ")
}

/// Advice for one direction.
pub fn advise(m: &Metrics, r: &Rating, other: &Rating, c: Option<&Conditions>) -> Vec<Advice> {
    let mut out = Vec::new();
    let mut add = |topic: Topic, text: String| {
        out.push(Advice {
            topic,
            text: rules::decimal_comma(&text),
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
            format!(
                "Peaux courtes (ou demi-peaux) pour {} montée{} de {:.0}° ou plus ({} en tout) ; la plus longue : {:.0} m à {:.0}° au {}. Ailleurs, les écailles suffisent.",
                steep_climbs.len(),
                if steep_climbs.len() > 1 { "s" } else { "" },
                skin_from,
                rules::length(total),
                longest.length_m,
                longest.max_deg,
                km(longest.start_km)
            ),
        );
    } else if m.climbs.is_empty() {
        add(
            Topic::Grip,
            "Montées douces (moins de 8°) : les écailles suffisent.".to_owned(),
        );
    } else {
        add(
            Topic::Grip,
            format!(
                "Montées de moins de {skin_from:.0}° ou courtes : écailles, appuis francs et bâtons, pas de canard sur les ressauts ; peaux utiles sur neige dure, poudreuse ou avec un sac lourd ou une pulka."
            ),
        );
    }
    if slippery {
        add(
            Topic::Grip,
            "Neige du jour glissante (regel ou poudreuse) : les écailles tiennent mal, collez les peaux dès 8°.".to_owned(),
        );
    }
    add(
        Topic::Grip,
        "Gardez toujours une paire de peaux dans le sac.".to_owned(),
    );

    // Technique downhill.
    let d = m.max_descent_deg;
    let t = if d < 8.0 {
        "Descentes douces : le chasse-neige suffit.".to_owned()
    } else if d < 15.0 {
        "Descentes jusqu'à 15° : chasse-neige et arrêt maîtrisés.".to_owned()
    } else if d < 20.0 {
        "Descentes de 15 à 20° : virages (stem ou télémark) ou traversées et conversions."
            .to_owned()
    } else if d < 25.0 {
        "Descentes de 20 à 25° : dérapage latéral sur les carres maîtrisé.".to_owned()
    } else {
        "Descentes de plus de 25° : très bon contrôle en dérapage, sinon déchausser et descendre à pied.".to_owned()
    };
    add(Topic::Technique, t);
    let hard_desc: Vec<&Passage> = m.descents.iter().filter(|p| p.max_deg >= 15.0).collect();
    if !hard_desc.is_empty() {
        add(
            Topic::Technique,
            format!("Descentes raides : {}.", list_passages(&hard_desc)),
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
            format!(
                "Descente en forêt dense : {}. Vitesse réduite ; gardez les peaux pour freiner si la neige est dure.",
                list_passages(&forest)
            ),
        );
    }
    let conv: Vec<&Passage> = m.climbs.iter().filter(|p| p.max_deg >= 15.0).collect();
    if !conv.is_empty() {
        add(
            Topic::Technique,
            format!(
                "Conversions ou montée en escalier : {}.",
                list_passages(&conv)
            ),
        );
    }
    if !m.sidehills.is_empty() {
        let s: Vec<&Passage> = m.sidehills.iter().collect();
        add(
            Topic::Technique,
            format!(
                "Versant raide traversé : {} ; sans difficulté sur un chemin creusé dans la pente, mais en dévers hors chemin ou si la neige le comble : carres plantées côté amont, à pied si la neige est dure.",
                s.iter()
                    .take(4)
                    .map(|p| format!(
                        "{} (pente de {:.0}° sur {:.0} m)",
                        km(p.start_km),
                        p.terrain_max_deg,
                        p.length_m
                    ))
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
        );
    }

    // Glide.
    if m.flat_km >= 3.0 && m.flat_km / m.length_km.max(0.1) >= 0.4 {
        add(
            Topic::Glide,
            format!(
                "Beaucoup de plat ({:.1} km, dont {:.1} km d'affilée) : skis fartés en glisse, écailles plutôt que peaux, pas alternatif et poussée simultanée.",
                m.flat_km, m.longest_flat_km
            ),
        );
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
            format!(
                "Montée ou descente de 30° ou plus, hors terrain SRN : {} ; contournez-la ou déchaussez.",
                beyond
                    .iter()
                    .take(4)
                    .map(|p| km(p.start_km))
                    .collect::<Vec<_>>()
                    .join(", ")
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
        .map(|s| format!("{} ({})", km(s.start_km), rules::length(s.length_m)))
        .collect();
    if !high.is_empty() {
        add(
            Topic::Avoid,
            format!(
                "Terrain « très exposé » aux avalanches (modèle) : {} ; à éviter dès que le risque n'est pas faible.",
                high.join(", ")
            ),
        );
    }
    if m.forbidden_km > 0.0 {
        add(
            Topic::Avoid,
            format!(
                "Zone interdite d'accès à cette date sur {} : changez d'itinéraire.",
                rules::length(m.forbidden_km * 1000.0)
            ),
        );
    }
    if m.leave_routes_forbidden_km > 0.0 {
        add(
            Topic::Avoid,
            format!(
                "Sortie des itinéraires balisés interdite sur {}{} : restez sur le balisage.",
                rules::length(m.leave_routes_forbidden_km * 1000.0),
                m.zones
                    .iter()
                    .find(|z| !z.label.is_empty())
                    .map(|z| format!(" ({})", z.label))
                    .unwrap_or_default()
            ),
        );
    }
    if m.avoid_km > 0.0 {
        add(
            Topic::Avoid,
            format!(
                "Zone de quiétude de la faune sur {} : à éviter en saison.",
                rules::length(m.avoid_km * 1000.0)
            ),
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
        add(
            Topic::TurnBack,
            format!(
                "Le passage le plus difficile est loin du départ ({}) : si vous peinez avant, faites demi-tour.",
                km(at)
            ),
        );
    }
    if m.max_exit_km >= 3.0 {
        add(
            Topic::TurnBack,
            format!(
                "Point le plus éloigné d'une route : {} ({:.1} km). Fixez une heure de demi-tour et tenez-la.",
                km(m.max_exit_at_km),
                m.max_exit_km
            ),
        );
    }
    if other.level != r.level {
        add(
            Topic::TurnBack,
            format!(
                "Dans l'autre sens, le tracé est {} ({}).",
                if other.level < r.level {
                    "plus facile"
                } else {
                    "plus difficile"
                },
                other.level.label()
            ),
        );
    }

    // Equipment.
    if m.exposed_km + m.high_exposed_km > 0.0 {
        add(
            Topic::Equipment,
            "Terrain exposé aux avalanches traversé : DVA, pelle, sonde et lecture du bulletin d'avalanche (BERA) quand il existe ; le modèle ne le remplace pas.".to_owned(),
        );
    }
    if let Some(s) = m.no_network.first()
        && s.length_m >= 500.0
    {
        add(
            Topic::Equipment,
            format!(
                "Pas de réseau sur {} (même le 112) : prévenez un proche de l'itinéraire et de l'heure de retour.",
                rules::length(m.no_network_km.unwrap_or(0.0) * 1000.0)
            ),
        );
    }
    if r.raid {
        add(
            Topic::Equipment,
            format!(
                "Sortie de plusieurs jours : environ {} de marche effective, soit {} étapes d'au plus 6 h ; prévoir hébergement, réchaud, réparation (fixation, peaux) et frontale.",
                hours(m.munter_h),
                (m.munter_h / 6.0).ceil().max(2.0)
            ),
        );
    } else if m.munter_h >= 5.0 {
        add(
            Topic::Equipment,
            format!(
                "Longue journée (environ {}) : frontale, vivres et vêtement chaud de rechange.",
                hours(m.munter_h)
            ),
        );
    }
    if let Some(o) = m.longest_open_km
        && o >= 2.0
    {
        add(
            Topic::Equipment,
            format!(
                "Terrain découvert sur {o:.1} km d'affilée : vent et jour blanc possibles ; trace hors ligne et boussole."
            ),
        );
    }

    // Snow by aspect.
    let steep_desc: Vec<&Passage> = m.descents.iter().filter(|p| p.max_deg >= 12.0).collect();
    let mut aspects: Vec<&str> = steep_desc
        .iter()
        .filter_map(|p| p.aspect_deg.and_then(facing))
        .collect();
    aspects.sort_unstable();
    aspects.dedup();
    for a in aspects {
        match a {
            "nord" => add(
                Topic::Snow,
                "Descentes raides face nord : neige souvent froide, dure ou poudreuse.".to_owned(),
            ),
            "sud" => add(
                Topic::Snow,
                "Descentes raides face sud : regel le matin, neige lourde l'après-midi ; partez tôt.".to_owned(),
            ),
            _ => {}
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

/// Measures, rates and advises both directions: the level comes from the calibrated
/// measurement ([`Params::measure`]), the advice and the metrics of the report from
/// [`detail`].
pub fn assess(profile: &Profile, p: &Params, c: Option<&Conditions>) -> Report {
    let [(_, rf), (_, rr)] = rules::rate_profile(profile, p, c);
    let d = detail(p);
    let mf = analysis::measure(profile, Direction::Forward, &d);
    let mr = analysis::measure(profile, Direction::Reverse, &d);
    let af = advise(&mf, &rf, &rr, c);
    let ar = advise(&mr, &rr, &rf, c);
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

    /// French text of both directions; the other direction lists only the reasons and
    /// advice that differ from the direction of the track.
    pub fn text_fr(&self, name: &str) -> String {
        let mut s = String::new();
        let _ = writeln!(s, "{name}");
        for (i, d) in [&self.forward, &self.reverse].into_iter().enumerate() {
            let m = &d.metrics;
            let r = &d.rating;
            let dir = if i == 0 {
                "Dans le sens du tracé"
            } else {
                "Dans l'autre sens"
            };
            let line = format!(
                "{dir} : {}{} — technique {}, effort {} sur l'échelle 2025 ; {:.1} km, +{:.0} m / −{:.0} m, environ {}.",
                r.level.label().to_uppercase(),
                if r.beyond_srn {
                    " (hors SRN par endroits)"
                } else {
                    ""
                },
                r.technical.label(),
                r.effort.label(),
                m.length_km,
                m.ascent_m,
                m.descent_m,
                hours(m.munter_h)
            );
            let _ = writeln!(s, "{}", rules::decimal_comma(&line));
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
                let _ = writeln!(
                    s,
                    "Raisons{} :",
                    if i == 0 { "" } else { " propres à ce sens" }
                );
                for x in reasons {
                    let _ = writeln!(s, "  - [{}] {}", x.level.label(), x.text);
                }
            }
            if !advice.is_empty() {
                let _ = writeln!(
                    s,
                    "Conseils{} :",
                    if i == 0 { "" } else { " propres à ce sens" }
                );
                for a in advice {
                    let _ = writeln!(s, "  - {}", a.text);
                }
            }
        }
        let _ = write!(s, "{DISCLAIMER}");
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
