//! Texts in French.

use jiff::civil::{Date, DateTime};

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

    fn clock5(&self, t: DateTime) -> String {
        skimap_plan::pace::clock_fr(t)
    }

    fn clock_day(&self, t: DateTime, day: Date) -> String {
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

    fn after_limit(&self, limit: DateTime, for_group: bool) -> String {
        format!(
            "Retour prévu après la limite de {} : boucle longue pour {}",
            self.assist().clock(limit),
            if for_group { "ce groupe" } else { "ce jour" }
        )
    }

    fn off_network_access(&self, metres: f64) -> String {
        let m = (metres / 10.0).round() * 10.0;
        format!(
            "Accès hors réseau : {m} m à pied ou à ski du parking (ou de l'accueil) jusqu'à l'itinéraire, sans tracé, en plus de la boucle"
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
        date: Date,
        start: &str,
        departure: DateTime,
        label: &str,
        km: f64,
        trails: &[String],
        back: DateTime,
        limit: Option<DateTime>,
        alert: DateTime,
    ) -> String {
        let a = self.assist();
        format!(
            "Sortie ski de fond {site} le {} : départ {start} à {}, boucle {label} ({} km, pistes {}). Retour prévu vers {}{}. Sans nouvelles de moi à {}, appelle le 112 et donne ce plan.",
            date.strftime("%d/%m"),
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

    fn when(&self, t: DateTime) -> String {
        format!("{:02}/{:02} à {}", t.day(), t.month(), self.clock5(t))
    }

    fn delta(&self, actual_s: f64, estimated_s: f64) -> String {
        let d = actual_s - estimated_s;
        if d.abs() < 300.0 {
            "comme prévu".into()
        } else if d > 0.0 {
            format!("{} de plus que prévu", self.duration(d))
        } else {
            format!("{} de moins que prévu", self.duration(-d))
        }
    }

    fn recap_distance(&self, km: f64) -> String {
        format!("Distance : {} km", self.assist().num(km, 1))
    }

    fn recap_time(&self, elapsed_s: f64, moving_s: f64) -> String {
        format!(
            "Temps : {} dont {} en mouvement",
            self.duration(elapsed_s),
            self.duration(moving_s)
        )
    }

    fn recap_speed(&self, kmh: f64) -> String {
        format!("Vitesse en mouvement : {} km/h", self.assist().num(kmh, 1))
    }

    fn recap_estimate(&self, group: bool, estimated_s: f64, actual_s: f64) -> String {
        let who = if group {
            "au rythme du groupe"
        } else {
            "au modèle"
        };
        format!(
            "Temps estimé {who} : {}, réel : {} ({})",
            self.duration(estimated_s),
            self.duration(actual_s),
            self.delta(actual_s, estimated_s)
        )
    }

    fn recap_gain_imported(&self, gain_m: f64) -> String {
        format!("Dénivelé : {gain_m:.0} m en montée (export Strava)")
    }

    fn recap_elevation(
        &self,
        ascent_m: f64,
        descent_m: f64,
        source: crate::recap::AltitudeSource,
    ) -> String {
        use crate::recap::AltitudeSource;
        let src = match source {
            AltitudeSource::BarometerAndRelief => "baromètre corrigé par le relief",
            AltitudeSource::Barometer => "baromètre",
            AltitudeSource::Relief => "relief sous la trace",
        };
        format!("Dénivelé : {ascent_m:.0} m en montée, {descent_m:.0} m en descente ({src})")
    }

    fn recap_same_route(&self, n: usize, when: &str, last_s: f64, diff_s: f64) -> String {
        let how = if diff_s.abs() < 300.0 {
            "à peu près le même temps".to_owned()
        } else if diff_s < 0.0 {
            format!("{} plus vite", self.duration(-diff_s))
        } else {
            format!("{} plus lentement", self.duration(diff_s))
        };
        format!(
            "{n}e fois sur ce parcours ; la dernière fois ({when}) : {}, aujourd'hui {how}.",
            self.duration(last_s)
        )
    }

    fn recap_speed_vs(&self, kmh: f64, mean_kmh: f64, n: usize) -> String {
        let s = if n > 1 { "s" } else { "" };
        format!(
            "Vitesse en mouvement {} km/h, contre {} km/h en moyenne sur {n} sortie{s} précédente{s} ici.",
            self.assist().num(kmh, 1),
            self.assist().num(mean_kmh, 1),
        )
    }

    fn recap_first(&self) -> &'static str {
        "Première sortie enregistrée de ce genre : rien à comparer encore."
    }

    fn recap_spoken(
        &self,
        distance: &str,
        elapsed_s: f64,
        delta: Option<&str>,
        comparison: Option<&str>,
    ) -> String {
        format!(
            "Bilan : {distance} en {}{}.{}",
            self.duration(elapsed_s),
            delta.map(|d| format!(", {d}")).unwrap_or_default(),
            comparison.map(|c| format!(" {c}")).unwrap_or_default()
        )
    }

    fn cond_temperature(&self, lo: f64, hi: f64) -> String {
        format!("Température prévue : {lo:.0} à {hi:.0} °C")
    }

    fn cond_gusts(&self, kmh: f64) -> String {
        format!("Vent prévu : rafales jusqu'à {kmh:.0} km/h")
    }

    fn cond_fog(&self, hours: usize) -> String {
        format!("Brouillard prévu sur {hours} h")
    }

    fn cond_snow(&self, cm: f64) -> String {
        format!("Neige prévue : {} cm", self.assist().num(cm, 1))
    }

    fn cond_source(&self, point: &str) -> String {
        format!("Source : prévision du pack au point « {point} »")
    }

    fn cond_altitude(&self, lo: f64, hi: f64) -> String {
        format!("Altitude : de {lo:.0} à {hi:.0} m")
    }

    fn cond_none(&self) -> &'static str {
        "Conditions : aucune prévision dans cette sortie."
    }

    fn not_an_outing(&self, dir: &str) -> String {
        format!("{dir} : pas de sortie")
    }

    fn rule_topic(&self, t: skimap_zones::rules::Topic) -> &'static str {
        use skimap_zones::rules::Topic;
        match t {
            Topic::Bivouac => "Bivouac",
            Topic::Access => "Accès",
            Topic::LeaveRoutes => "Sortie des itinéraires balisés",
            Topic::Fire => "Feu",
        }
    }

    fn rule_verdict(&self, v: skimap_core::zone_eval::Verdict) -> &'static str {
        v.label_fr()
    }

    fn stoves_included(&self) -> &'static str {
        " (réchauds compris)"
    }

    fn zone_off_season(&self, name: &str) -> String {
        format!("{name} (hors saison)")
    }

    fn rule_inactive(&self, i: skimap_core::zone_eval::Inactive) -> &'static str {
        use skimap_core::zone_eval::Inactive;
        match i {
            Inactive::NotInForce => "texte pas en vigueur à cette date",
            Inactive::OutOfPeriod => "hors de sa période",
            Inactive::Daytime => "règle de nuit, il fait jour à cette heure",
            Inactive::ZoneNotSensitive => "zone hors de ses mois sensibles",
            Inactive::NoSunEvent => "coucher du soleil non calculable",
        }
    }

    fn source_kind(&self, k: skimap_zones::rules::SourceKind) -> &'static str {
        use skimap_zones::rules::SourceKind;
        match k {
            SourceKind::Law => "loi ou code",
            SourceKind::SignedAct => "arrêté signé",
            SourceKind::DraftAct => "projet d'arrêté (texte signé non lu)",
            SourceKind::DecisionSummary => "synthèse officielle de la décision",
            SourceKind::Leaflet => "plaquette officielle",
            SourceKind::OfficialPage => "page officielle",
            SourceKind::ThirdPartyPage => "page tierce",
            SourceKind::DataService => "service de données",
        }
    }

    fn rules_at(&self, t: DateTime) -> String {
        format!("le {:02}/{:02} à {}", t.day(), t.month(), self.clock5(t))
    }

    fn rules_note(&self) -> &'static str {
        "Règles relevées dans les textes cités, à cette date et cette heure ; le texte officiel et la signalisation sur place font foi."
    }

    fn local_act(&self, a: skimap_zones::local::OwnAct) -> &'static str {
        use skimap_zones::local::OwnAct;
        match a {
            OwnAct::ArretePrefectoralBiotope => "arrêté de protection de biotope",
            OwnAct::DecretReserveNationale => "décret de la réserve naturelle",
            OwnAct::DeliberationReserveRegionale => "délibération régionale de la réserve",
            OwnAct::ReglementationCoeurParc => "réglementation du cœur de parc national",
            OwnAct::ArreteReserveBiologique => "arrêté de la réserve biologique",
            OwnAct::ArreteCaptage => "arrêté de protection du captage",
            OwnAct::ArretesMunicipaux => "arrêtés municipaux",
            OwnAct::ArretesPrefectoraux => "arrêtés préfectoraux",
            OwnAct::DroitCantonal => "droit cantonal",
        }
    }

    fn local_unverified(&self, name: &str, act: &str) -> String {
        format!("{name} ({act}) : texte non lu")
    }

    fn local_partial(&self, name: &str, act: &str) -> String {
        format!("{name} ({act}) : lu en partie seulement")
    }

    fn local_municipal_fr(&self) -> &'static str {
        "arrêtés municipaux et préfectoraux : non recensés"
    }

    fn local_municipal_ch(&self) -> &'static str {
        "règlements communaux : non recensés"
    }

    fn local_rules(&self, items: &[String]) -> String {
        if items.is_empty() {
            return String::new();
        }
        format!("Règles locales non vérifiées : {}.", items.join(" ; "))
    }

    fn day(&self, d: Date) -> String {
        const DAYS: [&str; 7] = [
            "lundi", "mardi", "mercredi", "jeudi", "vendredi", "samedi", "dimanche",
        ];
        format!(
            "{} {:02}/{:02}",
            DAYS[d.weekday().to_monday_zero_offset() as usize],
            d.day(),
            d.month()
        )
    }

    fn updated(&self, t: DateTime) -> String {
        format!(
            "mis à jour le {:02}/{:02} à {}",
            t.day(),
            t.month(),
            self.clock5(t)
        )
    }

    fn date_unknown(&self) -> &'static str {
        "date inconnue"
    }

    fn skipped_no_forecast(&self, day: &str) -> String {
        format!("{day} : aucune prévision dans le pack")
    }

    fn skipped_no_start(&self, day: &str) -> String {
        format!("{day} : aucun départ connu pour ce domaine")
    }

    fn time_group(&self, median_s: f64, lo_s: f64, hi_s: f64) -> String {
        format!(
            "environ {} ({} à {}) au rythme du groupe",
            self.duration(median_s),
            self.duration(lo_s),
            self.duration(hi_s)
        )
    }

    fn time_model(&self, s: f64) -> String {
        format!("environ {} au modèle", self.duration(s))
    }

    fn info_label(&self, id: &str) -> Option<&'static str> {
        Some(match id {
            "bera" => "Bulletin d'avalanche (BERA)",
            "meteo-hourly" => "Prévisions météo heure par heure",
            "status" => "État des pistes",
            "snow-depth" => "Enneigement estimé",
            "offpiste-snow" => "Neige hors piste",
            "avalanche-exposure" => "Exposition aux avalanches (modèle)",
            "zones" => "Zones et règles",
            "refuges" => "Refuges",
            "pistes" => "Pistes",
            "pack" => "Pack du domaine",
            _ => return None,
        })
    }

    fn battery_day(&self, reserve_pct: f64, cold: Option<f64>) -> String {
        format!(
            "Batterie suffisante pour plus de 24 h, {reserve_pct:.0} % gardés pour un appel d'urgence{}",
            battery_cold(cold)
        )
    }

    fn battery_until(&self, until: &str, reserve_pct: f64, cold: Option<f64>) -> String {
        format!(
            "Batterie suffisante jusqu'à {until}, {reserve_pct:.0} % gardés pour un appel d'urgence{}",
            battery_cold(cold)
        )
    }

    fn battery_short(&self, end: &str, reserve: &str) -> String {
        format!(
            "Batterie insuffisante pour la sortie prévue (fin vers {end}) : elle atteindra la réserve d'urgence vers {reserve}. Raccourcis la sortie, emporte une batterie externe ou passe en mode avion."
        )
    }

    fn never_guarantee(&self) -> &'static str {
        "Indication calculée sur le relief et les cartes, jamais une garantie : le bulletin d'avalanche (BERA) du jour et ton jugement sur place priment."
    }

    fn missing_layer(&self, id: &str) -> &'static str {
        match id {
            "snow" => "neige du jour",
            "exposure" => "exposition aux avalanches",
            "zones" => "zones et règles",
            _ => "couverture réseau",
        }
    }

    fn no_offpiste_relief(&self) -> &'static str {
        "pas de relief hors piste dans ce pack"
    }

    fn outside_relief(&self) -> &'static str {
        "point hors du relief du pack"
    }

    fn no_passable(&self, radius_m: f64) -> String {
        format!(
            "aucun terrain praticable à moins de {radius_m:.0} m d'une extrémité (pente de 30° ou plus, zone interdite à cette date, très exposé)"
        )
    }

    fn no_offpiste_route(&self) -> &'static str {
        "aucun itinéraire sous 30° hors des zones interdites à cette date"
    }

    fn arrival_range(&self, lo: DateTime, hi: DateTime) -> String {
        format!("{} à {}", self.clock5(lo), self.clock5(hi))
    }

    fn cause(&self, c: &skimap_plan::tracker::Cause) -> String {
        use skimap_plan::tracker::{Cause, Condition};
        match c {
            Cause::Closure { .. } => "piste fermée sur l'itinéraire".into(),
            Cause::Late { limit } => {
                format!("retard sur la limite de {}", self.assist().clock(*limit))
            }
            Cause::Conditions(Condition::Fog) => "brouillard".into(),
            Cause::Conditions(Condition::WindRising) => "vent qui se lève".into(),
            Cause::Conditions(Condition::Snow) => "neige".into(),
            Cause::Conditions(Condition::TemperatureDrop) => "froid".into(),
            Cause::Deviation => "écart de l'itinéraire".into(),
            Cause::NoTime => "plus le temps de finir la boucle avant la nuit".into(),
            Cause::Pace => "proposition acceptée".into(),
        }
    }

    fn back_proposal(&self, target: Option<&crate::tracking_back::Target>) -> String {
        use crate::tracking_back::{Target, TargetKind};
        let to = match target {
            Some(Target {
                kind: TargetKind::Destination,
                name: Some(n),
                ..
            }) => format!("jusqu'à {n}"),
            Some(Target {
                kind: TargetKind::Destination,
                ..
            }) => "jusqu'à l'arrivée".into(),
            Some(Target {
                kind: TargetKind::Car,
                ..
            }) => "jusqu'à la voiture".into(),
            _ => "jusqu'au départ".into(),
        };
        format!("Il est temps de rentrer. Être guidé {to} ?")
    }

    fn all_domains(&self) -> &'static str {
        "Tous les domaines installés"
    }

    fn packs_note(&self, names: &[&str]) -> String {
        format!("{} packs : {}.", names.len(), names.join(", "))
    }

    fn france_only_name(&self) -> &'static str {
        "France, aucun domaine installé"
    }

    fn france_only_note(&self, date: &str) -> String {
        format!("Aucun pack installé : fond de France seul, données du {date}.")
    }
}

fn battery_cold(cold: Option<f64>) -> String {
    cold.map(|t| format!(", batterie à {t:.0} °C : garde le téléphone au chaud"))
        .unwrap_or_default()
}
