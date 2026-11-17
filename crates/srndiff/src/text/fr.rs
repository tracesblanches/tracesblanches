//! The texts in French.

use super::Texts;
use crate::rules::{Level, decimal_comma};

/// French.
pub struct Fr;

fn where_(at: f64) -> String {
    format!("au km {at:.1}")
}

impl Texts for Fr {
    fn finish(&self, text: String) -> String {
        decimal_comma(&text)
    }

    fn length(&self, m: f64) -> String {
        if m < 995.0 {
            format!("{:.0} m", (m / 10.0).round() * 10.0)
        } else {
            format!("{:.1} km", m / 1000.0)
        }
    }

    fn hours(&self, h: f64) -> String {
        let m = (h * 60.0).round() as i64;
        format!("{} h {:02}", m / 60, m % 60)
    }

    fn km(&self, v: f64) -> String {
        format!("km {v:.1}")
    }

    fn level(&self, l: Level) -> &'static str {
        match l {
            Level::Facile => "facile",
            Level::Moyen => "moyen",
            Level::Difficile => "difficile",
            Level::TresDifficile => "très difficile",
        }
    }

    fn steepest_descent(&self, deg: f32, sustain_m: f64, at_km: f64, length_m: f64) -> String {
        format!(
            "descente la plus raide : {:.0}° en moyenne sur {:.0} m {} (passage de {:.0} m à plus de 8°)",
            deg,
            sustain_m,
            where_(at_km),
            length_m
        )
    }

    fn gentle_descents(&self, deg: f32) -> String {
        format!("descentes douces (au plus {deg:.0}°)")
    }

    fn no_descent(&self) -> String {
        "aucune descente notable".to_owned()
    }

    fn steepest_climb(&self, deg: f32, sustain_m: f64, at_km: f64, length_m: f64) -> String {
        format!(
            "montée la plus raide : {:.0}° en moyenne sur {:.0} m {} (passage de {:.0} m à plus de 8°)",
            deg,
            sustain_m,
            where_(at_km),
            length_m
        )
    }

    fn forest_descent(&self, deg: f32, dense_m: f64, at_km: f64) -> String {
        format!(
            "descente en forêt dense sans trouée : {:.0}° sur {:.0} m {}",
            deg,
            dense_m,
            where_(at_km)
        )
    }

    fn sidehill(&self, deg: f32, length_m: f64, at_km: f64) -> String {
        format!(
            "traversée en dévers sur pente de {:.0}° ({:.0} m) {}",
            deg,
            length_m,
            where_(at_km)
        )
    }

    fn beyond(&self, deg: f32) -> String {
        format!("passage à {deg:.0}° ou plus : hors du terrain du ski de randonnée nordique")
    }

    fn effort(&self, length_km: f64, ascent_m: f64, time: &str, raid: bool) -> String {
        format!(
            "{:.1} km, {:.0} m de montée, environ {} de marche effective (Munter){}",
            length_km,
            ascent_m,
            time,
            if raid {
                " : sortie de plusieurs jours (raid)"
            } else {
                ""
            }
        )
    }

    fn high_exposed(&self, length: &str, first_at_km: Option<f64>) -> String {
        format!(
            "traverse {} de terrain « très exposé » aux avalanches (modèle){}",
            length,
            first_at_km
                .map(|a| format!(", d'abord {}", where_(a)))
                .unwrap_or_default()
        )
    }

    fn exposed(&self, length: &str, n: usize) -> String {
        format!(
            "traverse {} de terrain « exposé » aux avalanches (modèle), en {} passage{}",
            length,
            n,
            if n > 1 { "s" } else { "" }
        )
    }

    fn relief_and_exposure_unknown(&self, share_pct: f64) -> String {
        format!(
            "relief et exposition aux avalanches inconnus sur {share_pct:.0} % du tracé (hors du relief LiDAR HD de l'IGN) : le niveau peut être sous-estimé"
        )
    }

    fn exposure_unknown(&self, length: &str, deg: f32) -> String {
        format!(
            "exposition aux avalanches non évaluée sur {length}, avec des pentes jusqu'à {deg:.0}°"
        )
    }

    fn remote(&self, km: f64, at_km: f64) -> String {
        format!(
            "jusqu'à {:.1} km d'une route ou d'une extrémité du tracé ({})",
            km,
            where_(at_km)
        )
    }

    fn no_network(&self, length: &str, at_km: f64) -> String {
        format!(
            "{} d'affilée sans aucun réseau mobile, même pour le 112 ({})",
            length,
            where_(at_km)
        )
    }

    fn forbidden(&self, length: &str) -> String {
        format!("traverse {length} d'une zone dont l'accès est interdit à cette date")
    }

    fn fresh_snow(&self, cm: f32) -> String {
        format!("{cm:.0} cm de neige fraîche à tracer : effort et temps accrus (≈ +30 %)")
    }

    fn icy(&self, icy: bool) -> String {
        if icy {
            "neige dure ou regelée : descentes plus délicates d'un niveau"
        } else {
            "croûte : descentes plus délicates d'un niveau"
        }
        .to_owned()
    }

    fn coarse(&self, share_pct: f64, coarse_m: f64) -> String {
        format!(
            "tracé approximatif : {share_pct:.0} % de sa longueur en lignes droites de plus de {coarse_m:.0} m entre deux points ; pentes moyennées entre les points, ressauts courts possibles non vus"
        )
    }

    fn relief_unknown(&self, share_pct: f64) -> String {
        format!(
            "relief inconnu sur {share_pct:.0} % du tracé (hors du relief LiDAR HD de l'IGN) : niveau sous-estimé possible"
        )
    }

    fn passage(&self, at_km: f64, deg: f32, length_m: f64) -> String {
        format!("{} ({:.0}° sur {:.0} m)", self.km(at_km), deg, length_m)
    }

    fn sidehill_passage(&self, at_km: f64, deg: f32, length_m: f64) -> String {
        format!(
            "{} (pente de {:.0}° sur {:.0} m)",
            self.km(at_km),
            deg,
            length_m
        )
    }

    fn stretch(&self, at_km: f64, length: &str) -> String {
        format!("{} ({})", self.km(at_km), length)
    }

    fn grip_skins(
        &self,
        n: usize,
        from_deg: f64,
        total: &str,
        longest_m: f64,
        longest_deg: f32,
        longest_at_km: f64,
    ) -> String {
        format!(
            "Peaux courtes (ou demi-peaux) pour {} montée{} de {:.0}° ou plus ({} en tout) ; la plus longue : {:.0} m à {:.0}° au {}. Ailleurs, les écailles suffisent.",
            n,
            if n > 1 { "s" } else { "" },
            from_deg,
            total,
            longest_m,
            longest_deg,
            self.km(longest_at_km)
        )
    }

    fn grip_gentle(&self) -> String {
        "Montées douces (moins de 8°) : les écailles suffisent.".to_owned()
    }

    fn grip_scales(&self, from_deg: f64) -> String {
        format!(
            "Montées de moins de {from_deg:.0}° ou courtes : écailles, appuis francs et bâtons, pas de canard sur les ressauts ; peaux utiles sur neige dure, poudreuse ou avec un sac lourd ou une pulka."
        )
    }

    fn grip_slippery(&self) -> String {
        "Neige du jour glissante (regel ou poudreuse) : les écailles tiennent mal, collez les peaux dès 8°.".to_owned()
    }

    fn grip_keep_skins(&self) -> String {
        "Gardez toujours une paire de peaux dans le sac.".to_owned()
    }

    fn technique(&self, d: f32) -> String {
        if d < 8.0 {
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
        }
    }

    fn steep_descents(&self, list: &str) -> String {
        format!("Descentes raides : {list}.")
    }

    fn forest_descents(&self, list: &str) -> String {
        format!(
            "Descente en forêt dense : {list}. Vitesse réduite ; gardez les peaux pour freiner si la neige est dure."
        )
    }

    fn kick_turns(&self, list: &str) -> String {
        format!("Conversions ou montée en escalier : {list}.")
    }

    fn sidehills(&self, list: &str) -> String {
        format!(
            "Versant raide traversé : {list} ; sans difficulté sur un chemin creusé dans la pente, mais en dévers hors chemin ou si la neige le comble : carres plantées côté amont, à pied si la neige est dure."
        )
    }

    fn glide(&self, flat_km: f64, longest_km: f64) -> String {
        format!(
            "Beaucoup de plat ({flat_km:.1} km, dont {longest_km:.1} km d'affilée) : skis fartés en glisse, écailles plutôt que peaux, pas alternatif et poussée simultanée."
        )
    }

    fn avoid_beyond(&self, list: &str) -> String {
        format!(
            "Montée ou descente de 30° ou plus, hors terrain SRN : {list} ; contournez-la ou déchaussez."
        )
    }

    fn avoid_high_exposed(&self, list: &str) -> String {
        format!(
            "Terrain « très exposé » aux avalanches (modèle) : {list} ; à éviter dès que le risque n'est pas faible."
        )
    }

    fn avoid_forbidden(&self, length: &str) -> String {
        format!("Zone interdite d'accès à cette date sur {length} : changez d'itinéraire.")
    }

    fn avoid_leave_routes(&self, length: &str, zone: Option<&str>) -> String {
        format!(
            "Sortie des itinéraires balisés interdite sur {}{} : restez sur le balisage.",
            length,
            zone.map(|z| format!(" ({z})")).unwrap_or_default()
        )
    }

    fn avoid_quiet(&self, length: &str) -> String {
        format!("Zone de quiétude de la faune sur {length} : à éviter en saison.")
    }

    fn turn_back_hardest(&self, at_km: f64) -> String {
        format!(
            "Le passage le plus difficile est loin du départ ({}) : si vous peinez avant, faites demi-tour.",
            self.km(at_km)
        )
    }

    fn turn_back_far(&self, at_km: f64, km: f64) -> String {
        format!(
            "Point le plus éloigné d'une route : {} ({:.1} km). Fixez une heure de demi-tour et tenez-la.",
            self.km(at_km),
            km
        )
    }

    fn other_direction(&self, easier: bool, level: Level) -> String {
        format!(
            "Dans l'autre sens, le tracé est {} ({}).",
            if easier {
                "plus facile"
            } else {
                "plus difficile"
            },
            self.level(level)
        )
    }

    fn equip_avalanche(&self) -> String {
        "Terrain exposé aux avalanches traversé : DVA, pelle, sonde et lecture du bulletin d'avalanche (BERA) quand il existe ; le modèle ne le remplace pas.".to_owned()
    }

    fn equip_no_network(&self, length: &str) -> String {
        format!(
            "Pas de réseau sur {length} (même le 112) : prévenez un proche de l'itinéraire et de l'heure de retour."
        )
    }

    fn equip_raid(&self, time: &str, stages: f64) -> String {
        format!(
            "Sortie de plusieurs jours : environ {time} de marche effective, soit {stages} étapes d'au plus 6 h ; prévoir hébergement, réchaud, réparation (fixation, peaux) et frontale."
        )
    }

    fn equip_long_day(&self, time: &str) -> String {
        format!("Longue journée (environ {time}) : frontale, vivres et vêtement chaud de rechange.")
    }

    fn equip_open(&self, km: f64) -> String {
        format!(
            "Terrain découvert sur {km:.1} km d'affilée : vent et jour blanc possibles ; trace hors ligne et boussole."
        )
    }

    fn snow_north(&self) -> String {
        "Descentes raides face nord : neige souvent froide, dure ou poudreuse.".to_owned()
    }

    fn snow_south(&self) -> String {
        "Descentes raides face sud : regel le matin, neige lourde l'après-midi ; partez tôt."
            .to_owned()
    }

    fn direction(
        &self,
        forward: bool,
        level: Level,
        beyond: bool,
        technical: Level,
        effort: Level,
        length_km: f64,
        ascent_m: f64,
        descent_m: f64,
        time: &str,
    ) -> String {
        let dir = if forward {
            "Dans le sens du tracé"
        } else {
            "Dans l'autre sens"
        };
        format!(
            "{dir} : {}{} — technique {}, effort {} sur l'échelle 2025 ; {:.1} km, +{:.0} m / −{:.0} m, environ {}.",
            self.level(level).to_uppercase(),
            if beyond {
                " (hors SRN par endroits)"
            } else {
                ""
            },
            self.level(technical),
            self.level(effort),
            length_km,
            ascent_m,
            descent_m,
            time
        )
    }

    fn reasons_title(&self, forward: bool) -> &'static str {
        if forward {
            "Raisons :"
        } else {
            "Raisons propres à ce sens :"
        }
    }

    fn advice_title(&self, forward: bool) -> &'static str {
        if forward {
            "Conseils :"
        } else {
            "Conseils propres à ce sens :"
        }
    }

    fn disclaimer(&self) -> &'static str {
        crate::report::DISCLAIMER
    }
}
