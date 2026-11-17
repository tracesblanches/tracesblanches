//! Texts in French.

#[allow(unused_imports)]
use chrono::{NaiveDate, NaiveDateTime};

use super::Texts;

/// French.
pub struct Fr;

impl Texts for Fr {
    fn assist(&self) -> &'static dyn skimap_assist::text::Texts {
        skimap_assist::text::of(skimap_assist::text::Lang::Fr)
    }

    fn group_learned(&self, outings: u32) -> String {
        match outings {
            0 => "a priori, aucune sortie apprise".to_owned(),
            1 => "une sortie apprise".to_owned(),
            n => format!("{n} sorties apprises"),
        }
    }

    fn group_who(&self, adults: u8, children: &[u8]) -> String {
        let adults = match adults {
            1 => "1 adulte".to_owned(),
            n => format!("{n} adultes"),
        };
        let ages: Vec<String> = children.iter().map(u8::to_string).collect();
        match ages.split_last() {
            None => adults,
            Some((last, [])) => format!("{adults}, enfants de {last} ans"),
            Some((last, rest)) => format!("{adults}, enfants de {} et {last} ans", rest.join(", ")),
        }
    }

    fn duration(&self, s: f64) -> String {
        skimap_plan::pace::duration_fr(s)
    }

    fn clock5(&self, t: NaiveDateTime) -> String {
        skimap_plan::pace::clock_fr(t)
    }

    fn clock_day(&self, t: NaiveDateTime, day: NaiveDate) -> String {
        if t.date() > day {
            format!("{} le lendemain", self.clock5(t))
        } else {
            self.clock5(t)
        }
    }

    fn duration_range(&self, lo: &str, hi: &str) -> String {
        format!("{lo} à {hi}")
    }

    fn duration_about(&self, median: &str, lo: &str, hi: &str) -> String {
        format!("environ {median} ({lo} à {hi})")
    }

    fn back_slow_side(&self, median: &str, plan: &str, back: &str) -> String {
        format!("médiane {median}, prévu sur {plan} (côté lent), retour prévu vers {back}")
    }

    fn back_between(&self, lo: &str, hi: &str) -> String {
        format!("retour entre {lo} et {hi}")
    }

    fn back_around(&self, at: &str) -> String {
        format!("retour vers {at}")
    }

    fn model_time(&self, d: &str) -> String {
        format!("environ {d} (modèle)")
    }

    fn back_with_margin(&self, at: &str) -> String {
        format!("retour vers {at} avec la marge")
    }

    fn after_limit(&self, limit: NaiveDateTime, for_group: bool) -> String {
        format!(
            "Retour prévu après la limite de {} : boucle longue pour {}",
            self.assist().clock(limit),
            if for_group { "ce groupe" } else { "ce jour" }
        )
    }

    fn key_kind(&self, k: skimap_plan::plan::KeyKind) -> &'static str {
        use skimap_plan::plan::KeyKind;
        match k {
            KeyKind::Start => "départ",
            KeyKind::Turn => "carrefour",
            KeyKind::Junction => "carrefour évident",
            KeyKind::Refuge => "refuge",
            KeyKind::NoNetworkStart => "début de zone sans réseau",
            KeyKind::NoNetworkEnd => "fin de zone sans réseau",
            KeyKind::AvalancheStart => "début de zone d'avalanche connue",
            KeyKind::AvalancheEnd => "fin de zone d'avalanche connue",
            KeyKind::Finish => "arrivée",
        }
    }

    fn sms(
        &self,
        site: &str,
        date: NaiveDate,
        start: &str,
        departure: NaiveDateTime,
        label: &str,
        km: f64,
        trails: &[String],
        back: NaiveDateTime,
        limit: Option<NaiveDateTime>,
        alert: NaiveDateTime,
    ) -> String {
        let a = self.assist();
        format!(
            "Sortie ski de fond {site} le {} : départ {start} à {}, boucle {label} ({} km, pistes {}). Retour prévu vers {}{}. Sans nouvelles de moi à {}, appelle le 112 et donne ce plan.",
            date.format("%d/%m"),
            a.clock(departure),
            a.num(km, 1),
            trails.join(", "),
            a.clock(back),
            limit
                .map(|l| format!(", au plus tard {}", a.clock(l)))
                .unwrap_or_default(),
            a.clock(alert)
        )
    }

    fn default_group_name(&self) -> &'static str {
        "Famille avec enfants débutants"
    }

    fn solo_group_name(&self) -> &'static str {
        "Seul"
    }
}
