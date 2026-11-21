//! Texts in French.

#[allow(unused_imports)]
use chrono::{NaiveDate, NaiveDateTime, Timelike};
use skimap_core::routing::Style;

use super::Texts;
use crate::pace::{PaceReason, ProposalKind};
use crate::plan::KeyKind;
use crate::session::End;

/// French.
pub struct Fr;

fn hhmm(t: NaiveDateTime) -> String {
    format!("{} h {:02}", t.hour(), t.minute())
}

fn km(m: f64) -> String {
    format!("{:.1}", m / 1000.0).replace('.', ",")
}

/// Minutes of the plans B, absolute value: « 45 min », « 1 h 05 ».
fn minutes(s: f64) -> String {
    let m = (s / 60.0).round().abs() as i64;
    if m < 60 {
        format!("{m} min")
    } else {
        format!("{} h {:02}", m / 60, m % 60)
    }
}

impl Texts for Fr {
    fn assist(&self) -> &'static dyn skimap_assist::text::Texts {
        skimap_assist::text::of(skimap_assist::text::Lang::Fr)
    }

    fn never_safe(&self) -> &'static str {
        crate::plan::NEVER_SAFE
    }

    fn no_hourly_weather(&self) -> &'static str {
        "pas de météo horaire : ni meilleur moment, ni fartage, ni alertes de conditions"
    }

    fn no_best_time(&self, start: NaiveDateTime) -> String {
        format!(
            "pas de meilleur moment calculé : départ à {}",
            self.assist().clock(start)
        )
    }

    fn outing_label(
        &self,
        budget_min: u32,
        style: Style,
        easy_only: bool,
        return_before: Option<chrono::NaiveTime>,
    ) -> String {
        let (h, m) = (budget_min / 60, budget_min % 60);
        let mut s = match (h, m) {
            (0, m) => format!("{m} min"),
            (h, 0) => format!("{h} h"),
            (h, m) => format!("{h} h {m:02}"),
        };
        s.push_str(match style {
            Style::Skating => ", skating",
            Style::Classic => ", classique",
        });
        if easy_only {
            s.push_str(", vertes et bleues");
        }
        if let Some(t) = return_before {
            s.push_str(&format!(", retour avant {} h {:02}", t.hour(), t.minute()));
        }
        s
    }

    fn start_phrase<'a>(&self, fr: &'a str, _en: Option<&'a str>) -> &'a str {
        fr
    }

    fn p_title(&self, date: NaiveDate, label: &str, start: &str, at: NaiveDateTime) -> String {
        format!(
            "Plan du {} — {label} — départ {start} à {}",
            self.assist().date(date),
            hhmm(at)
        )
    }

    fn p_loop(
        &self,
        rank: usize,
        label: &str,
        km: f64,
        gain_m: f64,
        minutes: f64,
        style: Style,
        score: f64,
    ) -> String {
        format!(
            "Boucle n° {rank} : {label} — {} km, D+ {gain_m} m, environ {} min (rythme d'un skieur moyen, {}), score {}",
            format!("{km:.1}").replace('.', ","),
            minutes.round(),
            self.assist().style(style),
            format!("{score:.2}").replace('.', ",")
        )
    }

    fn p_trails(&self, names: &[String]) -> String {
        format!("Pistes : {}", names.join(", "))
    }

    fn p_arrival(
        &self,
        arrival: NaiveDateTime,
        with_margin: NaiveDateTime,
        deadline: Option<NaiveDateTime>,
        return_before: Option<NaiveDateTime>,
    ) -> String {
        format!(
            "Arrivée prévue {} au rythme du modèle, {} avec la marge de 20 %{}{}",
            hhmm(arrival),
            hhmm(with_margin),
            deadline
                .map(|d| format!(" ; limite de lumière moins 30 min : {}", hhmm(d)))
                .unwrap_or_default(),
            return_before
                .map(|d| format!(" ; retour demandé avant {}", hhmm(d)))
                .unwrap_or_default()
        )
    }

    fn p_turnaround(&self, at: Option<NaiveDateTime>, group: bool) -> String {
        match at {
            Some(x) if group => format!(
                "Demi-tour au plus tard à {} au rythme prévu du groupe",
                hhmm(x)
            ),
            Some(x) => format!("Demi-tour au plus tard à {} au rythme prévu", hhmm(x)),
            None => "La boucle finit avant la limite de lumière au rythme prévu : pas d'heure de demi-tour imposée".into(),
        }
    }

    fn p_reasons(&self) -> &'static str {
        "Raisons :"
    }

    fn p_reason(&self, text: &str, points: f64) -> String {
        format!("  - {text} [{points:+.2}]").replace('.', ",")
    }

    fn p_key_points(&self) -> &'static str {
        "Points clés (heure de passage au rythme du modèle) :"
    }

    fn p_key_kind(&self, k: KeyKind) -> &'static str {
        match k {
            KeyKind::Start => "départ",
            KeyKind::Turn => "carrefour annoncé",
            KeyKind::Junction => "carrefour évident (rien n'est dit)",
            KeyKind::Refuge => "refuge",
            KeyKind::NoNetworkStart => "début de zone sans réseau",
            KeyKind::NoNetworkEnd => "fin de zone sans réseau",
            KeyKind::AvalancheStart => "début de zone d'avalanche connue (CLPA)",
            KeyKind::AvalancheEnd => "fin de zone d'avalanche connue",
            KeyKind::DomainEntered => "entrée sur un autre domaine nordique (sa redevance)",
            KeyKind::Finish => "arrivée",
        }
    }

    fn p_key_point(&self, s_m: f64, at: NaiveDateTime, what: &str, name: Option<&str>) -> String {
        format!(
            "  {} km  {}  {what}{}",
            km(s_m),
            hhmm(at),
            name.map(|n| format!(" : {n}")).unwrap_or_default()
        )
    }

    fn p_announcements(&self) -> &'static str {
        "Annonces prévues :"
    }

    fn p_when(&self, s_m: Option<f64>, arrival: bool) -> String {
        match (s_m, arrival) {
            (Some(s), _) => format!("à {} km", km(s)),
            (None, false) => "au départ".to_owned(),
            (None, true) => "à l'arrivée".to_owned(),
        }
    }

    fn p_announcement(&self, when: &str, priority: &str, text: &str) -> String {
        format!("  {when} [{priority}] « {text} »")
    }

    fn p_plans_b(&self, n: usize) -> String {
        format!("Plans B ({n}) :")
    }

    fn p_plan_b(&self, text: &str, switch: &str) -> String {
        format!("  - {text} — annonce « {switch} »")
    }

    fn p_latest_exit(&self, s_m: f64, at: NaiveDateTime) -> String {
        format!(
            "  sortie du carrefour à {} km : y passer avant {} pour finir avant la limite",
            km(s_m),
            hhmm(at)
        )
    }

    fn p_morning(&self) -> &'static str {
        "Matin :"
    }

    fn p_morning_line(&self, part: &str, line: &str) -> String {
        format!("  [{part}] {line}")
    }

    fn p_morning_voice(&self, part: &str, spoken: &str) -> String {
        format!("  [{part}, voix] {spoken}")
    }

    fn p_equipment(&self) -> &'static str {
        "Équipement :"
    }

    fn p_item(&self, label: &str, reason: &str) -> String {
        format!("  - {label} : {reason}")
    }

    fn p_refuges(&self, within_m: f64, names: &[String]) -> String {
        format!(
            "Refuges à moins de {within_m} m de la boucle : {}",
            names.join(", ")
        )
    }

    fn p_unnamed_shelter(&self, kind: &str) -> String {
        format!("abri sans nom ({kind})")
    }

    fn p_clips(&self, required: usize, missing: &[String]) -> String {
        format!(
            "Clips : {required} nécessaires, {} absents du pack{}",
            missing.len(),
            if missing.is_empty() {
                String::new()
            } else {
                format!(" ({})", missing.join(", "))
            }
        )
    }

    fn p_clips_fallback(&self, missing: usize, required: usize) -> String {
        format!(
            "{missing} clips sur {required} absents du pack : ces annonces passent par la voix de secours du téléphone"
        )
    }

    fn p_limit(&self, note: &str) -> String {
        format!("Limite : {note}")
    }

    fn fb_unnamed_trail(&self) -> &'static str {
        "la piste sans nom"
    }

    fn fb_closed(&self, name: &str, span: Option<(f64, f64)>) -> String {
        format!(
            "Si {name} est fermée{}",
            span.map(|(a, b)| format!(" (de {} à {} km de la boucle)", km(a), km(b)))
                .unwrap_or_default()
        )
    }

    fn fb_at(&self, junction_s_m: Option<f64>) -> String {
        match junction_s_m {
            Some(s) => format!("au carrefour à {} km", km(s)),
            None => "au départ".to_owned(),
        }
    }

    fn fb_exit(&self, at: &str, length_m: f64, loop_m: f64, saves_s: f64, names: &str) -> String {
        format!(
            "Sortie {at} : {} km au total au lieu de {}, {} de moins au rythme d'un skieur moyen, par {names}",
            km(length_m),
            km(loop_m),
            minutes(saves_s),
        )
    }

    fn fb_no_detour(&self, closed: &str) -> String {
        format!("{closed} : aucun autre passage dans le graphe des pistes")
    }

    fn fb_detour(
        &self,
        closed: &str,
        at: &str,
        rejoin_m: Option<f64>,
        length_m: f64,
        saves_s: f64,
        names: &str,
    ) -> String {
        format!(
            "{closed} : contournement {at}{}, {} km au total, {} {} au rythme d'un skieur moyen, par {names}",
            match rejoin_m {
                Some(s) => format!(", retour sur la boucle à {} km", km(s)),
                None => ", puis directement vers l'arrivée".into(),
            },
            km(length_m),
            minutes(saves_s),
            if saves_s >= 0.0 {
                "de moins"
            } else {
                "de plus"
            },
        )
    }

    fn duration5(&self, s: f64) -> String {
        let m = ((s / 60.0 / 5.0).round() * 5.0) as i64;
        if m < 60 {
            format!("{m} min")
        } else {
            format!("{} h {:02}", m / 60, m % 60)
        }
    }

    fn clock5(&self, t: NaiveDateTime) -> String {
        let m = (t.hour() * 60 + t.minute()) as f64 + t.second() as f64 / 60.0;
        let m = ((m / 5.0).round() * 5.0) as u32 % (24 * 60);
        format!("{} h {:02}", m / 60, m % 60)
    }

    fn pc_learned(&self, outings: u32) -> String {
        match outings {
            0 => "a priori, aucune sortie apprise".to_owned(),
            1 => "une sortie apprise".to_owned(),
            n => format!("{n} sorties apprises"),
        }
    }

    fn pc_duration(&self, median: Option<f64>, lo: f64, hi: f64) -> String {
        match median {
            None => format!("{} à {}", self.duration5(lo), self.duration5(hi)),
            Some(m) => format!(
                "environ {} ({} à {})",
                self.duration5(m),
                self.duration5(lo),
                self.duration5(hi)
            ),
        }
    }

    fn pc_slow_side(&self, median: f64, planned: f64, chances: f64, back: NaiveDateTime) -> String {
        format!(
            "médiane {}, prévu sur {} (côté lent : {chances} chances sur 10 d'avoir fini), retour prévu vers {}",
            self.duration5(median),
            self.duration5(planned),
            self.clock5(back)
        )
    }

    fn pc_back_between(&self, lo: NaiveDateTime, hi: NaiveDateTime) -> String {
        format!("retour entre {} et {}", self.clock5(lo), self.clock5(hi))
    }

    fn pc_back_around(&self, t: NaiveDateTime) -> String {
        format!("retour vers {}", self.clock5(t))
    }

    fn pc_text(
        &self,
        group: &str,
        learned: &str,
        duration: &str,
        back: &str,
        moving_s: f64,
        stops_s: f64,
        c5_s: f64,
    ) -> String {
        format!(
            "Temps estimé pour {group} ({learned}) : {duration}, {back} ; en mouvement {} au rythme du groupe, arrêts {} ; skieur moyen sans arrêt : {}",
            self.duration5(moving_s),
            self.duration5(stops_s),
            self.duration5(c5_s)
        )
    }

    fn pr_why(&self, reason: PaceReason, slower_pct: f64) -> String {
        match reason {
            PaceReason::Slower => format!("Vous allez {slower_pct:.0} % moins vite que prévu."),
            PaceReason::SlowerAndPauses => {
                format!("Vous allez {slower_pct:.0} % moins vite que prévu, avec plus de pauses.")
            }
            PaceReason::Pauses => {
                format!(
                    "Vous faites plus de pauses que prévu ({slower_pct:.0} % moins vite au total)."
                )
            }
        }
    }

    fn pr_what(&self, kind: &ProposalKind) -> &'static str {
        match kind {
            ProposalKind::ShortLoop => "Je vous propose la boucle courte",
            ProposalKind::NearestExit => "Je vous propose la sortie la plus proche",
            ProposalKind::Keep => "Je vous propose de garder la boucle",
        }
    }

    fn pr_text(
        &self,
        why: &str,
        what: &str,
        back: (NaiveDateTime, NaiveDateTime, NaiveDateTime),
        saved_min: Option<f64>,
    ) -> String {
        let saved = saved_min
            .filter(|m| *m >= 5.0)
            .map(|m| format!(", {} de moins", self.duration5(m * 60.0)))
            .unwrap_or_default();
        format!(
            "{why} {what}, retour {} ({} à {}){saved}. Accepter ou refuser.",
            self.clock5(back.0),
            self.clock5(back.1),
            self.clock5(back.2)
        )
    }

    fn recap(
        &self,
        end: End,
        km: f64,
        moving_min: f64,
        paused_min: f64,
        done_pct: f64,
        route_km: f64,
        ratio: Option<f64>,
        switches: usize,
        spoken: usize,
        refuges: &[String],
    ) -> String {
        let comma = |x: String| x.replace('.', ",");
        format!(
            "{} : {} km en {moving_min} min de mouvement et {paused_min} min d'arrêt ; {done_pct} % de l'itinéraire suivi ({} km){} ; {switches} changement(s) d'itinéraire, {spoken} annonce(s){}",
            match end {
                End::Arrived => "Sortie terminée à l'arrivée",
                End::Abandoned => "Sortie interrompue loin de l'arrivée",
            },
            comma(format!("{km:.1}")),
            comma(format!("{route_km:.1}")),
            ratio
                .map(|p| format!(
                    ", temps réel / temps du modèle : {}",
                    comma(format!("{p:.2}"))
                ))
                .unwrap_or_default(),
            if refuges.is_empty() {
                String::new()
            } else {
                format!(", arrêts : {}", refuges.join(", "))
            }
        )
    }
}
