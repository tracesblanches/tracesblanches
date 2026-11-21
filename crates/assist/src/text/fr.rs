//! Texts of the assistant in French.

use chrono::{Datelike, NaiveDate, NaiveDateTime, Timelike};

use super::Texts;
use crate::crowd::{CrowdLevel, Population};
use crate::equipment::Kind as EquipmentKind;
use crate::inputs::LayerName;
use crate::snow_quality::{Quality, SlopeClass, Slot};
use crate::wax::SnowType;

/// French.
pub struct Fr;

const WEEKDAYS: [&str; 7] = [
    "lundi", "mardi", "mercredi", "jeudi", "vendredi", "samedi", "dimanche",
];
const MONTHS: [&str; 12] = [
    "janvier",
    "février",
    "mars",
    "avril",
    "mai",
    "juin",
    "juillet",
    "août",
    "septembre",
    "octobre",
    "novembre",
    "décembre",
];
const ASPECTS: [&str; 8] = [
    "nord",
    "nord-est",
    "est",
    "sud-est",
    "sud",
    "sud-ouest",
    "ouest",
    "nord-ouest",
];

impl Texts for Fr {
    fn num(&self, x: f64, decimals: usize) -> String {
        format!("{x:.decimals$}").replace('.', ",")
    }

    fn date(&self, d: NaiveDate) -> String {
        format!(
            "{} {} {} {}",
            WEEKDAYS[d.weekday().num_days_from_monday() as usize],
            d.day(),
            MONTHS[d.month0() as usize],
            d.year()
        )
    }

    fn weekday(&self, monday0: usize) -> &'static str {
        WEEKDAYS[monday0 % 7]
    }

    fn clock(&self, t: NaiveDateTime) -> String {
        format!("{} h {:02}", t.hour(), t.minute())
    }

    fn hour(&self, t: NaiveDateTime) -> String {
        format!("{} h", t.hour())
    }

    fn gusts_up_to(&self, kmh: f64) -> String {
        format!("rafales jusqu'à {kmh:.0} km/h")
    }

    fn wind_light(&self, g: f64) -> String {
        format!("vent faible (rafales {g:.0} km/h)")
    }

    fn wind_moderate(&self, g: f64) -> String {
        format!("vent modéré (rafales {g:.0} km/h)")
    }

    fn very_cold(&self, chill: &str) -> String {
        format!("très froid, ressenti {chill} °C")
    }

    fn cold(&self, chill: &str) -> String {
        format!("froid, ressenti {chill} °C")
    }

    fn temperature(&self, mean: &str, chill: &str) -> String {
        format!("{mean} °C en moyenne, ressenti {chill} °C")
    }

    fn fog_hours(&self, hours: usize) -> String {
        format!("brouillard {hours} h sur 7")
    }

    fn day_snowfall(&self, cm: f64) -> String {
        format!("{} cm de neige pendant la journée", self.num(cm, 0))
    }

    fn rain(&self, mm: f64) -> String {
        format!("pluie ({} mm)", self.num(mm, 1))
    }

    fn sunny(&self) -> &'static str {
        "ensoleillé"
    }

    fn snow_estimate(&self, depth: f64, map: NaiveDate, added: f64, share_pct: f64) -> String {
        let added = if added >= 0.5 {
            format!(" dont {} cm tombés depuis", self.num(added, 0))
        } else {
            String::new()
        };
        format!(
            "{} cm de neige estimée (médiane, carte du {}{added}, {} % des points ≥ 20 cm)",
            self.num(depth, 0),
            map.format("%d/%m"),
            share_pct
        )
    }

    fn snow_verdict(&self, base: &str, verdict: u8) -> String {
        match verdict {
            0 => format!("{base} : insuffisante pour damer"),
            1 => format!("{base} : limite"),
            _ => format!("{base} : suffisante"),
        }
    }

    fn snow_unknown(&self) -> &'static str {
        "neige estimée inconnue ici (hors de la carte du site ou sans tracé) : à vérifier"
    }

    fn fresh_snow_24h(&self, cm: f64) -> String {
        format!("{} cm de neige fraîche en 24 h", self.num(cm, 0))
    }

    fn surface(&self, q: Quality) -> String {
        format!("surface : {}", self.quality(q))
    }

    fn no_snow_map(&self) -> &'static str {
        "aucune carte d'enneigement de ce jour ou des 4 jours avant"
    }

    fn weather_from(&self, point: &str, km: f64) -> String {
        format!("météo du point {point} à {} km", self.num(km, 0))
    }

    fn no_forecast(&self) -> &'static str {
        "pas de prévision météo pour ce jour"
    }

    fn crowd(&self, level: CrowdLevel, reasons: &[String]) -> String {
        let why = if reasons.is_empty() {
            String::new()
        } else {
            format!(" ({})", reasons.join(", "))
        };
        format!("niveau d'affluence {}{why}", self.crowd_level(level))
    }

    fn as_the_crow_flies(&self, km: f64) -> String {
        format!("à {} km à vol d'oiseau", self.num(km, 0))
    }

    fn avalanche_km(&self, km: f64, level: Option<u8>) -> String {
        let km = self.num(km, 1);
        match level {
            Some(l) if l >= 3 => {
                format!("{km} km dans des zones d'avalanche connues, BERA {l} sur 5")
            }
            Some(l) => format!("{km} km dans des zones d'avalanche connues (BERA {l} sur 5)"),
            None => format!("{km} km dans des zones d'avalanche connues, BERA non disponible"),
        }
    }

    fn calendars_2026_27(&self) -> &'static str {
        "calendriers scolaires connus seulement pour 2026-27 : l'affluence de ce jour ne \
         compte que les week-ends"
    }

    fn status_unknown_day(&self) -> &'static str {
        "état des pistes inconnu pour ce jour"
    }

    fn status_last_known(&self, when: &str) -> String {
        format!("état des pistes : dernier connu ({when})")
    }

    fn sources_disagree(&self, what: &str) -> String {
        format!("sources en désaccord : {what}")
    }

    fn open_without_snow(&self) -> &'static str {
        "ouvert selon la source, mais neige estimée insuffisante : état sans doute hors \
         saison, à vérifier"
    }

    fn open(&self) -> &'static str {
        "ouvert"
    }

    fn closed(&self) -> &'static str {
        "fermé"
    }

    fn status_unknown(&self) -> &'static str {
        "état inconnu"
    }

    fn last_known_suffix(&self) -> &'static str {
        " (dernier état connu)"
    }

    fn no_status_source(&self) -> &'static str {
        "pas de source d'état des pistes pour ce domaine"
    }

    fn only_itinerary_closed(&self, suffix: &str) -> String {
        format!("le seul itinéraire connu est fermé{suffix}")
    }

    fn none_open(&self, n: usize, suffix: &str) -> String {
        format!("aucun des {n} itinéraires connus n'est ouvert{suffix}")
    }

    fn itineraries_open(
        &self,
        open: usize,
        known: usize,
        open_km: f64,
        total_km: f64,
        suffix: &str,
        no_snow: bool,
    ) -> String {
        format!(
            "{open} {} sur {known} ({} km sur {}){suffix}{}",
            if open == 1 {
                "itinéraire ouvert"
            } else {
                "itinéraires ouverts"
            },
            self.num(open_km, 0),
            self.num(total_km, 0),
            if no_snow {
                " mais neige estimée insuffisante : à vérifier"
            } else {
                ""
            }
        )
    }

    fn area_closed_dates(&self) -> &'static str {
        "domaine fermé à cette date (dates d'ouverture publiées par la commune)"
    }

    fn nordic_km(&self, km: f64) -> String {
        format!("{} km de pistes nordiques (OSM)", self.num(km, 0))
    }

    fn candidate(&self, rank: usize, name: &str, score: f64, reasons: &[String]) -> String {
        format!(
            "{rank}. {name} (score {}) : {}.",
            self.num(score, 2),
            reasons.join(" ; ")
        )
    }

    fn reason_points(&self, text: &str, points: f64) -> String {
        format!("{text} [{points:+.2}]").replace('.', ",")
    }

    fn briefing_title(&self, site: &str, date: NaiveDate) -> String {
        format!("{site} — {}", self.date(date))
    }

    fn briefing_bera(&self, level: u8) -> String {
        format!("BERA : risque {level} sur 5 (lisez le bulletin complet).")
    }

    fn briefing_domains(&self) -> &'static str {
        "Domaines :"
    }

    fn briefing_all_closed(&self, n: usize) -> String {
        format!("Itinéraires : les {n} itinéraires connus sont fermés.")
    }

    fn briefing_itineraries(&self) -> &'static str {
        "Itinéraires (les 5 premiers non fermés) :"
    }

    fn briefing_snow_quality(&self, point: &str, summary: &[String]) -> String {
        format!("Qualité de neige ({point}) : {}", summary.join(" "))
    }

    fn briefing_sunset(&self, at: NaiveDateTime) -> String {
        format!("Coucher du soleil {} h {:02}.", at.hour(), at.minute())
    }

    fn briefing_data(&self, notes: &[String]) -> String {
        format!("Données : {}.", notes.join(" ; "))
    }

    fn crowd_level(&self, level: CrowdLevel) -> &'static str {
        match level {
            CrowdLevel::Calm => "calme",
            CrowdLevel::Moderate => "modéré",
            CrowdLevel::Busy => "chargé",
            CrowdLevel::VeryBusy => "très chargé",
        }
    }

    fn holiday(&self, fr: &'static str) -> &'static str {
        fr
    }

    fn population(&self, p: Population) -> &'static str {
        match p {
            Population::FranceZoneA => "zone A",
            Population::FranceZoneB => "zone B",
            Population::FranceZoneC => "zone C",
            Population::Fwb => "Belgique (FWB)",
            Population::Vaud => "Vaud",
            Population::Neuchatel => "Neuchâtel",
        }
    }

    fn france(&self) -> &'static str {
        "France"
    }

    fn holiday_for(&self, name: &str, who: &[&str]) -> String {
        format!("{name} ({})", who.join(", "))
    }

    fn crowd_fresh_snow(&self, cm: f64) -> String {
        format!("neige fraîche ({cm:.0} cm)")
    }

    fn crowd_sun(&self) -> &'static str {
        "soleil"
    }

    fn crowd_area_closed(&self) -> &'static str {
        "domaine fermé (dates d'ouverture publiées)"
    }

    fn quality(&self, q: Quality) -> &'static str {
        match q {
            Quality::Fresh => "neige fraîche",
            Quality::Windblown => "neige fraîche soufflée par le vent",
            Quality::OldDry => "neige ancienne froide et sèche",
            Quality::Transformed => "neige transformée qui ramollit (décaille)",
            Quality::Wet => "neige humide et lourde",
            Quality::Slush => "neige soupe",
            Quality::Hard => "neige dure, regelée",
            Quality::Icy => "croûte de regel épaisse, plaques de glace",
        }
    }

    fn aspect(&self, octant: usize) -> &'static str {
        ASPECTS[octant % 8]
    }

    fn slope_class(&self, c: &SlopeClass) -> String {
        match c.octant() {
            None => "plat".into(),
            Some(o) => format!("{}, pente de {}°", self.aspect(o), c.slope_deg.round()),
        }
    }

    fn slot(&self, s: &Slot) -> String {
        format!("de {} h à {} h", s.start.hour(), s.end.hour())
    }

    fn sq_no_snow(&self, cm: f64, elevation: f64) -> String {
        format!(
            "Pas ou trop peu de neige estimée au point ({cm} cm, {elevation} m) : qualité de \
             neige non évaluée."
        )
    }

    fn sq_flat(&self, elevation: f64, q: Quality) -> String {
        format!(
            "Au plat ({elevation} m) : {} entre 10 h et 15 h.",
            self.quality(q)
        )
    }

    fn sq_best(&self, class: &str, slot: &str, q: Quality) -> String {
        format!("Meilleur créneau : {class} {slot} ({}).", self.quality(q))
    }

    fn sq_aspect_at(&self, aspect: &str, at: NaiveDateTime) -> String {
        format!("{aspect} {}", self.hour(at))
    }

    fn sq_aspect_slope_from(&self, aspect: &str, slope: f64, at: NaiveDateTime) -> String {
        format!("{aspect} {slope}° dès {}", self.hour(at))
    }

    fn sq_softens(&self, list: &[String]) -> String {
        format!("Pentes de 20° : ramollissement vers {}.", list.join(", "))
    }

    fn sq_heavy(&self, list: &[String]) -> String {
        format!("Neige lourde : {}.", list.join(", "))
    }

    fn sq_never_softens(&self, list: &[&str]) -> String {
        format!("Ne ramollit pas (20°) : {}.", list.join(", "))
    }

    fn sq_model(&self) -> &'static str {
        "Modèle simple (température, soleil, nuages, vent) : ni ombre du relief ni damage ; \
         à confirmer sur le terrain."
    }

    fn group_label(&self, p: &skimap_core::calibration::GroupProfile) -> String {
        use skimap_core::calibration::Level;
        let level = match p.level {
            Level::Beginner => "débutant",
            Level::Intermediate => "intermédiaire",
            Level::Expert => "confirmé",
        };
        let mut ages: Vec<u8> = p.children.clone();
        ages.sort_unstable();
        let words: Vec<String> = ages.iter().map(u8::to_string).collect();
        let kids = match words.as_slice() {
            [] => String::new(),
            [one] => format!(", un enfant de {one} ans"),
            [rest @ .., last] => format!(", enfants de {} et {last} ans", rest.join(", ")),
        };
        format!(
            "« {} » : {} adulte{}{kids}, {}, niveau {level}",
            p.name,
            p.adults,
            if p.adults > 1 { "s" } else { "" },
            self.style(p.style)
        )
    }

    fn style(&self, s: skimap_core::routing::Style) -> &'static str {
        match s {
            skimap_core::routing::Style::Classic => "classique",
            skimap_core::routing::Style::Skating => "skating",
        }
    }

    fn learned(&self, outings: u32) -> String {
        match outings {
            0 => "a priori".to_owned(),
            1 => "une sortie apprise".to_owned(),
            n => format!("{n} sorties apprises"),
        }
    }

    fn hm(&self, seconds: f64) -> String {
        let m = (seconds / 60.0).round() as i64;
        if m < 60 {
            format!("{m} min")
        } else {
            format!("{} h {:02}", m / 60, m % 60)
        }
    }

    fn s_no_graph(&self) -> &'static str {
        "pas de graphe des pistes (couche routes)"
    }

    fn s_no_trail_near(&self, start: &str) -> String {
        format!("pas de piste à moins de 300 m de {start}")
    }

    fn s_unnamed_loop(&self) -> &'static str {
        "boucle sans nom"
    }

    fn s_time_group_slow(
        &self,
        median: &str,
        group: &str,
        lo: &str,
        hi: &str,
        learned: &str,
        plan: &str,
        style: &str,
        model: &str,
    ) -> String {
        format!(
            "environ {median} pour {group} ({lo} à {hi}, {learned} ; prévu sur {plan}, le côté lent : 8 chances sur 10 \
             d'avoir fini ; modèle C5, {style} : {model})"
        )
    }

    fn s_time_group(
        &self,
        median: &str,
        group: &str,
        lo: &str,
        hi: &str,
        learned: &str,
        style: &str,
        model: &str,
    ) -> String {
        format!(
            "environ {median} pour {group} ({lo} à {hi}, {learned} ; modèle C5, {style} : {model})"
        )
    }

    fn s_time_model(&self, model: &str, style: &str) -> String {
        format!("environ {model} (modèle C5, {style})")
    }

    fn s_loop(&self, km: f64, gain: f64, time: &str, back: NaiveDateTime) -> String {
        format!(
            "{} km, D+ {gain} m, {time}, retour prévu vers {}",
            self.num(km, 1),
            self.clock(back)
        )
    }

    fn s_out_and_back(&self, pct: f64) -> String {
        format!("{pct} % du parcours en aller-retour sur le même chemin")
    }

    fn s_eight(&self, km: f64) -> String {
        format!(
            "en huit : deux boucles depuis le départ, on y repasse après {} km",
            self.num(km, 1)
        )
    }

    fn s_closed_km(&self, km: f64) -> String {
        format!("{} km sur des pistes fermées", self.num(km, 1))
    }

    fn s_open_km(&self, km: f64) -> String {
        format!("{} km sur des pistes ouvertes", self.num(km, 1))
    }

    fn s_unknown_km(&self, km: f64) -> String {
        format!("{} km sans état connu", self.num(km, 1))
    }

    fn s_status_unknown(&self) -> &'static str {
        "état des pistes inconnu"
    }

    fn s_bera(&self, level: u8) -> String {
        format!("BERA {level} sur 5")
    }

    fn s_no_bera_range(&self) -> &'static str {
        "pas de BERA dans ce massif"
    }

    fn s_bera_unavailable(&self) -> &'static str {
        "BERA non disponible"
    }

    fn s_exposed(&self, km: f64, why: &str) -> String {
        format!(
            "{} km en terrain exposé aux avalanches (modèle), {why}",
            self.num(km, 1)
        )
    }

    fn s_harder(&self, km: f64) -> String {
        format!(
            "{} km sur des pistes plus difficiles que demandé",
            self.num(km, 1)
        )
    }

    fn s_trails(&self, names: &[String]) -> String {
        format!("pistes : {}", names.join(", "))
    }

    fn s_if_all_goes_well(&self, median: &str, slow: &str) -> String {
        format!(
            "si tout va bien : {median} au rythme médian du groupe, mais {slow} sur le côté lent \
             (au-delà du temps demandé)"
        )
    }

    fn s_no_way_back(&self, n: usize) -> String {
        format!(
            "{n} points de demi-tour sans retour possible vers le départ (sens uniques \
             du graphe C1)"
        )
    }

    fn s_evaluated(
        &self,
        evaluated: usize,
        short: usize,
        long: usize,
        group: Option<&str>,
    ) -> String {
        format!(
            "{evaluated} boucles évaluées, {short} trop courtes et {long} trop longues \
             (hors de 75 à 110 % du temps demandé{})",
            group
                .map(|l| format!(", au rythme du groupe : {l}"))
                .unwrap_or_default()
        )
    }

    fn s_too_late(&self, n: usize, limit: Option<NaiveDateTime>) -> String {
        format!(
            "{n} boucles écartées : retour après {}",
            limit.map(|l| self.clock(l)).unwrap_or_default()
        )
    }

    fn s_none_fits(&self) -> &'static str {
        "aucune boucle ne tient dans le temps et l'heure de retour demandés"
    }

    fn s_no_real_loop(
        &self,
        lo: &str,
        hi: &str,
        start: &str,
        one: bool,
        longest: Option<(f64, &str)>,
    ) -> String {
        format!(
            "aucune vraie boucle trouvée de {lo} à {hi} depuis {start} : {} en partie en aller-retour \
             sur le même chemin, seule option du réseau ici{}",
            if one {
                "la boucle proposée est"
            } else {
                "les boucles proposées sont"
            },
            longest
                .map(|(km, t)| format!(
                    " ; la plus longue vraie boucle trouvée fait {} km, environ {t}",
                    self.num(km, 1)
                ))
                .unwrap_or_default()
        )
    }

    fn s_no_snow_all(&self) -> &'static str {
        "neige estimée insuffisante sur toutes les boucles : rien n'est skiable ce jour"
    }

    fn s_closed_most(&self) -> &'static str {
        "pistes fermées sur la plus grande partie de chaque boucle"
    }

    fn s_title(
        &self,
        label: &str,
        start: &str,
        date: NaiveDate,
        at: NaiveDateTime,
        deadline: Option<NaiveDateTime>,
        group: Option<&str>,
    ) -> String {
        format!(
            "{label} depuis {start} le {} à {}{}{}",
            self.date(date),
            self.clock(at),
            deadline
                .map(|d| format!(" (heure limite de retour C7 : {})", self.clock(d)))
                .unwrap_or_default(),
            group
                .map(|l| format!(", temps du groupe : {l}"))
                .unwrap_or_default()
        )
    }

    fn s_line(
        &self,
        rank: usize,
        hopeful: bool,
        label: &str,
        score: f64,
        why: &[String],
    ) -> String {
        format!(
            "{rank}. {}{label} (score {}) : {}.",
            if hopeful { "si tout va bien : " } else { "" },
            self.num(score, 2),
            why.join(" ; ")
        )
    }

    fn s_limits(&self, notes: &[String]) -> String {
        format!("Limites : {}.", notes.join(" ; "))
    }

    fn s_unreachable(&self, nodes: usize, of: usize, km: f64, start: &str) -> String {
        format!(
            "{nodes} nœuds sur {of} ({} km de pistes) ne sont pas atteignables depuis {start} : réseau \
             séparé ou sens uniques dans le graphe des pistes (C1)",
            self.num(km, 1)
        )
    }

    fn bt_surface(&self, class: &str, qualities: &[&str]) -> String {
        format!("{class} : {}", qualities.join(" puis "))
    }

    fn bt_sep(&self) -> &'static str {
        " ; "
    }

    fn bt_gusts(&self, kmh: f64) -> String {
        format!("rafales jusqu'à {kmh:.0} km/h")
    }

    fn bt_fog(&self, hours: f64) -> String {
        format!("brouillard {hours} h")
    }

    fn bt_snowfall(&self) -> &'static str {
        "chute de neige"
    }

    fn bt_rain(&self) -> &'static str {
        "pluie"
    }

    fn bt_cold(&self, chill: &str) -> String {
        format!("ressenti jusqu'à {chill} °C")
    }

    fn bt_sun(&self) -> &'static str {
        "soleil"
    }

    fn bt_crowd(&self, level: CrowdLevel) -> String {
        format!(
            "journée {} : pic supposé de 10 h 30 à 14 h",
            self.crowd_level(level)
        )
    }

    fn bt_near_deadline(&self, deadline: NaiveDateTime) -> String {
        format!(
            "retour à moins de 30 min de l'heure limite ({})",
            self.clock(deadline)
        )
    }

    fn bt_other_start(&self, start: NaiveDateTime, lost: &[String]) -> String {
        format!("départ à {} : {}", self.clock(start), lost.join(", "))
    }

    fn bt_best(
        &self,
        start: NaiveDateTime,
        back: NaiveDateTime,
        minutes: f64,
        why: &[String],
        others: &[String],
    ) -> String {
        let vs = if others.is_empty() {
            String::new()
        } else {
            format!(" ; {}", others.join(" ; "))
        };
        format!(
            "Meilleur départ vers {} (retour vers {}, sortie de {} min) : {}{vs}.",
            self.clock(start),
            self.clock(back),
            minutes.round(),
            if why.is_empty() {
                "rien ne distingue les créneaux".into()
            } else {
                why.join(", ")
            }
        )
    }

    fn bt_no_forecast(&self) -> &'static str {
        "Pas de meilleur moment : aucune prévision météo horaire pour ce jour dans le pack."
    }

    fn bt_none(&self, minutes: f64, deadline: Option<NaiveDateTime>) -> String {
        format!(
            "Pas de créneau : une sortie de {} min ne finit pas avant l'heure limite{}.",
            minutes.round(),
            deadline
                .map(|d| format!(" ({})", self.clock(d)))
                .unwrap_or_default()
        )
    }

    fn wax_snow_type(&self, t: SnowType) -> &'static str {
        match t {
            SnowType::New => "neige fraîche",
            SnowType::Old => "neige ancienne à grains fins",
            SnowType::CoarseFrozen => "neige regelée à gros grains, croûte ou glace",
            SnowType::TransformedMoist => "neige transformée humide",
            SnowType::Wet => "neige mouillée",
        }
    }

    fn wax_window(
        &self,
        hours: (u32, u32),
        air: &str,
        snow: &str,
        snow_type: SnowType,
        kick: &str,
        glide: (&str, &str),
        glide_alt: Option<(&str, &str)>,
    ) -> String {
        let galt = glide_alt
            .map(|(p, b)| format!(" ou {p} ({b})"))
            .unwrap_or_default();
        format!(
            "{} h–{} h, air {air} °C, neige ≈ {snow} °C, {} : classique, {kick} ; glisse \
             classique et skating {} ({}){galt}.",
            hours.0,
            hours.1,
            self.wax_snow_type(snow_type),
            glide.0,
            glide.1
        )
    }

    fn wax_kick(&self, kick: (&str, &str), alt: Option<(&str, &str)>) -> String {
        let alt = alt
            .map(|(p, b)| format!(" ou {p} ({b})"))
            .unwrap_or_default();
        format!("retenue {} ({}){alt}", kick.0, kick.1)
    }

    fn wax_kick_uncertain(
        &self,
        skins: Option<&str>,
        kick: (&str, &str),
        alt: Option<(&str, &str)>,
    ) -> String {
        let alt = alt
            .map(|(p, b)| format!(" ou {p} ({b})"))
            .unwrap_or_default();
        format!(
            "retenue incertaine, {} ; sinon {} ({}){alt}",
            skins.unwrap_or("skis à écailles ou peaux"),
            kick.0,
            kick.1
        )
    }

    fn wax_model(&self, snow_type: SnowType, mean: f64, min: f64, max: f64) -> String {
        format!(
            "{} (modèle de qualité de neige A3, plat), air {mean:+.1} °C en moyenne ({min:+.1} à {max:+.1})",
            self.wax_snow_type(snow_type),
        )
        .replace('.', ",")
    }

    fn wax_colder_than_all(&self) -> &'static str {
        "plus froid que toutes les plages : V05 Polar, à la limite"
    }

    fn wax_warmer_than_hard(&self) -> &'static str {
        "plus chaud que les farts durs pour cette neige : klister universel"
    }

    fn wax_coarse_frozen(&self) -> &'static str {
        "neige regelée à gros grains : klister (Toko, Swix)"
    }

    fn wax_fresh_near_zero(&self) -> &'static str {
        "neige fraîche autour de 0 °C : le fart de retenue botte ou glisse"
    }

    fn wax_refrozen_then_wet(&self, first: Quality, last: Quality) -> String {
        format!(
            "{} au début du créneau, {} ensuite : aucun fart ne tient tout le créneau",
            self.quality(first),
            self.quality(last)
        )
    }

    fn wax_crosses_zero(&self, min: f64, max: f64) -> String {
        format!("la température passe de {min:+.0} à {max:+.0} °C dans le créneau")
    }

    fn wax_wet_falling(&self) -> &'static str {
        "pluie ou neige mouillée qui tombe"
    }

    fn wax_skins(&self, why: &str) -> String {
        format!("skis à écailles ou peaux conseillés ({why})")
    }

    fn wax_glide(&self, air: f64, snow: f64) -> String {
        format!(
            "glisse : air {air:+.1} °C (tables Swix), neige estimée {snow:+.1} °C (tables Toko)"
        )
        .replace('.', ",")
    }

    fn wax_humidity(&self, pct: f64) -> String {
        format!("humidité {pct:.0} %")
    }

    fn wax_humidity_unknown(&self) -> &'static str {
        "humidité inconnue (absente de la couche météo)"
    }

    fn wax_day(&self, morning: &str, afternoon: Option<&str>) -> String {
        match afternoon {
            Some(a) => format!("Matin {morning} Après-midi {a}"),
            None => format!("Matin {morning} Même fartage l'après-midi."),
        }
    }

    fn eq_label(&self, k: EquipmentKind) -> &'static str {
        match k {
            EquipmentKind::AvalancheKit => "DVA, pelle et sonde",
            EquipmentKind::Headlamp => "frontale",
            EquipmentKind::WarmLayer => "couche chaude en plus",
            EquipmentKind::FaceProtection => "moufles et protection du visage",
            EquipmentKind::PowerBank => "batterie externe",
            EquipmentKind::TellSomeone => {
                "prévenir un proche de l'itinéraire et de l'heure de retour"
            }
            EquipmentKind::Compass => "boussole",
            EquipmentKind::SunProtection => "lunettes de soleil et crème",
            EquipmentKind::WaxlessSkis => "skis à écailles ou peaux",
        }
    }

    fn eq_exposed(&self, km: f64) -> String {
        format!(
            "{} km en terrain exposé aux avalanches (modèle)",
            self.num(km, 1)
        )
    }

    fn eq_clpa(&self, km: f64) -> String {
        format!(
            "{} km dans des zones d'avalanche connues (CLPA)",
            self.num(km, 1)
        )
    }

    fn eq_offpiste_bera(&self, level: u8) -> String {
        format!("hors piste avec un BERA à {level} sur 5")
    }

    fn eq_back_after_sunset(&self, back: NaiveDateTime, sunset: NaiveDateTime) -> String {
        format!(
            "retour prévu vers {}, coucher du soleil à {}",
            self.clock(back),
            self.clock(sunset)
        )
    }

    fn eq_start_before_sunrise(&self, start: NaiveDateTime, sunrise: NaiveDateTime) -> String {
        format!(
            "départ à {}, avant le lever du soleil ({})",
            self.clock(start),
            self.clock(sunrise)
        )
    }

    fn eq_chill(&self, chill: &str) -> String {
        format!("ressenti jusqu'à {chill} °C")
    }

    fn eq_no_network(&self, km: f64, pct: f64) -> String {
        format!(
            "{} km sans aucun réseau mobile, même pour le 112 ({pct} % de l'itinéraire)",
            self.num(km, 1)
        )
    }

    fn eq_offpiste(&self) -> &'static str {
        "sortie hors des pistes"
    }

    fn eq_long(&self, minutes: f64) -> String {
        format!("sortie de plus de 3 h ({minutes} min estimées)")
    }

    fn eq_fog(&self, hours: usize) -> String {
        format!("brouillard prévu ({hours} h pendant la sortie)")
    }

    fn eq_clear_sky(&self, cloud_pct: f64) -> String {
        format!("ciel dégagé ({cloud_pct:.0} % de nuages)")
    }

    fn eq_kick_unreliable(&self) -> &'static str {
        "retenue du fart incertaine ce jour-là (fartage A2)"
    }

    fn eq_nothing(&self, outing: &str) -> String {
        format!("{outing} : rien de particulier à ajouter à l'équipement habituel.")
    }

    fn eq_take(&self, outing: &str, items: &[(&str, &str)]) -> String {
        format!(
            "{outing} — à emporter : {}.",
            items
                .iter()
                .map(|(l, r)| format!("{l} ({r})"))
                .collect::<Vec<_>>()
                .join(" ; ")
        )
    }

    fn sy_altitude(&self, loc: &str) -> String {
        if let Some(a) = loc.strip_prefix('>') {
            format!("au-dessus de {a} m")
        } else if let Some(a) = loc.strip_prefix('<') {
            format!("en dessous de {a} m")
        } else {
            loc.to_owned()
        }
    }

    fn sy_risk_other(&self, loc: &str, level: u8) -> String {
        format!(" {} ({level} sur 5 plus bas)", self.sy_altitude(loc))
    }

    fn sy_aspects(&self, aspects: &[&str]) -> String {
        format!(", pentes {} les plus exposées", aspects.join(", "))
    }

    fn sy_read_bulletin(&self) -> &'static str {
        " : lisez le bulletin complet avant de partir"
    }

    fn sy_no_bera_jura(&self, no_bulletin: &str) -> String {
        format!(
            "{no_bulletin} (Jura) : des avalanches restent possibles sur les fortes pentes, le \
             bulletin du SLF couvre le Jura suisse voisin"
        )
    }

    fn sy_no_bera(&self, no_bulletin: &str) -> String {
        format!("{no_bulletin} : des avalanches restent possibles sur les fortes pentes")
    }

    fn sy_bera_missing(&self, massif: Option<&str>, loaded: bool) -> String {
        let massif = massif.unwrap_or("du massif");
        if loaded {
            format!(
                "Le BERA {massif} de ce jour n'est pas disponible hors ligne : consultez-le \
                 avant de partir, le risque d'avalanche n'est jamais nul en montagne"
            )
        } else {
            format!(
                "Le BERA {massif} de ce jour n'est pas chargé : consultez-le avant de partir, \
                 le risque d'avalanche n'est jamais nul en montagne"
            )
        }
    }

    fn sy_clpa(&self, km: f64, paths: usize) -> String {
        format!(
            " ; votre itinéraire traverse {} km de {paths} zones d'avalanche connues (CLPA)",
            self.num(km, 1)
        )
    }

    fn sy_clpa_none(&self) -> &'static str {
        " ; votre itinéraire ne traverse aucune zone d'avalanche connue (CLPA), ce qui \
         n'exclut pas une avalanche ailleurs"
    }

    fn sy_clpa_no_map(&self) -> &'static str {
        " ; pas de carte des avalanches connues (CLPA) dans ce massif"
    }

    fn sy_no_weather(&self) -> &'static str {
        "Pas de météo hors ligne pour ce jour."
    }

    fn sy_no_forecast(&self, point: &str) -> String {
        format!("Pas de prévision météo hors ligne pour ce jour au point {point}.")
    }

    fn sy_temperature(&self, min: &str, max: Option<&str>) -> String {
        match max {
            None => format!("{min} °C"),
            Some(max) => format!("de {min} à {max} °C"),
        }
    }

    fn sy_chill(&self, chill: &str) -> String {
        format!("ressenti {chill} °C")
    }

    fn sy_wind_light(&self) -> &'static str {
        "vent faible"
    }

    fn sy_fog(&self, hours: usize) -> String {
        format!("brouillard possible ({hours} h)")
    }

    fn sy_snow(&self, cm: f64) -> String {
        format!("{} cm de neige", self.num(cm, 0))
    }

    fn sy_freezing(&self, m: f64) -> String {
        format!("isotherme 0 °C vers {m} m")
    }

    fn sy_clear(&self) -> &'static str {
        "ciel dégagé"
    }

    fn sy_overcast(&self) -> &'static str {
        "ciel couvert"
    }

    fn sy_weather(&self, point: &str, elevation: f64, parts: &[String]) -> String {
        format!(
            "Météo de 9 h à 16 h ({point}, {elevation} m) : {}.",
            parts.join(", ")
        )
    }

    fn sy_verdict(
        &self,
        topic: skimap_zones::rules::Topic,
        verdict: skimap_core::zone_eval::Verdict,
        stoves: bool,
    ) -> Option<&'static str> {
        use skimap_core::zone_eval::Verdict;
        use skimap_zones::rules::Topic;
        Some(match (topic, verdict) {
            (Topic::Access, Verdict::Forbidden) => "accès interdit",
            (Topic::Access, Verdict::ForbiddenInPlaces) => "accès interdit par endroits",
            (Topic::Access, Verdict::AvoidRecommended) => "zone de quiétude de la faune à éviter",
            (Topic::LeaveRoutes, Verdict::Forbidden) => "sortie des itinéraires balisés interdite",
            (Topic::LeaveRoutes, Verdict::ForbiddenInPlaces) => {
                "sortie des pistes interdite par endroits (terrains privés)"
            }
            (Topic::LeaveRoutes, Verdict::AvoidRecommended) => "restez sur les itinéraires",
            (Topic::Bivouac, Verdict::Forbidden) => "bivouac interdit",
            (Topic::Fire, Verdict::Forbidden) if stoves => "feu interdit, réchauds compris",
            (Topic::Fire, Verdict::Forbidden) => "feu interdit",
            _ => return None,
        })
    }

    fn sy_night(&self, words: &str) -> String {
        format!("la nuit, {words}")
    }

    fn sy_zones_more(&self, first: &str, second: &str, others: usize) -> String {
        format!(" ({first}, {second} et {others} autres)")
    }

    fn sy_until(&self, day: u8, month: u8) -> String {
        format!(
            " jusqu'au {} {}",
            if day == 1 {
                "1er".to_owned()
            } else {
                day.to_string()
            },
            skimap_voice::numbers::month_name(month)
        )
    }

    fn sy_rules_not_loaded(&self) -> &'static str {
        "règles des zones protégées non chargées"
    }

    fn sy_no_rule(&self) -> &'static str {
        "aucune règle d'accès connue active ce jour"
    }

    fn sy_not_covered(&self) -> &'static str {
        " (hors de France et de Suisse, aucune règle connue)"
    }

    fn sy_no_network_km(&self, km: f64) -> String {
        format!(
            " ; {} km sans aucun réseau mobile, même pour le 112",
            self.num(km, 1)
        )
    }

    fn sy_network_everywhere(&self) -> &'static str {
        " ; réseau mobile annoncé sur tout l'itinéraire (cartes des opérateurs)"
    }

    fn sy_no_network_share(&self, pct: f64) -> String {
        format!(" ; {pct} % des pistes sans aucun réseau mobile, même pour le 112")
    }

    fn sy_rules(&self, route: bool, rules: &str, not_covered: &str, network: &str) -> String {
        let place = if route {
            "Sur votre itinéraire"
        } else {
            "Sur les pistes du site"
        };
        format!("{place} : {rules}{not_covered}{network}.")
    }

    fn al_wind_rising(&self, gust_kmh: f64, before_kmh: f64) -> String {
        format!(
            "le vent se lève : rafales à {gust_kmh:.0} km/h dans l'heure (avant : {before_kmh:.0})"
        )
    }

    fn al_fog(&self) -> &'static str {
        "brouillard prévu dans l'heure"
    }

    fn al_snow(&self, cm_h: f64) -> String {
        format!("chute de neige dans l'heure ({cm_h:.1} cm/h)").replace('.', ",")
    }

    fn al_cold(&self, temperature: &str, chill: &str) -> String {
        format!("la température baisse : {temperature} °C, ressenti {chill} °C d'ici 3 h")
    }

    fn al_turn_now(&self, late_min: f64, pace: f64, deadline: Option<NaiveDateTime>) -> String {
        format!(
            "en retard de {late_min} min (allure × {pace:.2}) : faites demi-tour maintenant pour rentrer avant {}",
            deadline.map(|d| self.clock(d)).unwrap_or_default()
        )
        .replace('.', ",")
    }

    fn al_turn_advanced(
        &self,
        late_min: f64,
        pace: f64,
        deadline: Option<NaiveDateTime>,
        turn: NaiveDateTime,
    ) -> String {
        format!(
            "en retard de {late_min} min (allure × {pace:.2}) : la boucle ne finit plus avant {} ; demi-tour au plus tard à {}",
            deadline.map(|d| self.clock(d)).unwrap_or_default(),
            self.clock(turn)
        )
        .replace('.', ",")
    }

    fn layer_name(&self, l: LayerName) -> &'static str {
        match l {
            LayerName::Weather => "météo horaire",
            LayerName::Snow => "carte d'enneigement",
            LayerName::Bera => "BERA",
            LayerName::Status => "état des pistes",
            LayerName::Domains => "domaines",
            LayerName::Pistes => "pistes",
            LayerName::Clpa => "avalanches historiques",
            LayerName::NoNetwork => "zones sans réseau",
            LayerName::Zones => "zones et règles",
            LayerName::Route => "itinéraire prévu",
            LayerName::Graph => "graphe des pistes",
            LayerName::Exposure => "exposition aux avalanches",
        }
    }

    fn layer_missing(&self, l: LayerName, why: &str) -> String {
        format!("{} : {why}", self.layer_name(l))
    }
}
