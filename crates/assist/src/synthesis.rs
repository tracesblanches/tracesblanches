//! Plain-language summary of the day's risks and rules (A7, #94): avalanche bulletin or
//! its absence, weather, zones and rules active at the date, historic avalanche paths
//! and areas without network on the planned route. Never "safe".
//!
//! # The three sentences
//!
//! 1. **Avalanche**: the BERA level of that date with its altitude split and its most
//!    dangerous aspects, phrased with the voice bank (`clips::bera`); or « not available
//!    offline » (Queyras when the bulletin could not be read); or « no bulletin for this
//!    massif » (Jura), always followed by the reminder that avalanches remain possible on
//!    steep slopes and the official source to read. When a route is given, the length of
//!    it inside historic avalanche paths (CLPA) is added.
//! 2. **Weather** of the skiing window (09–16 h) at the forecast point nearest to the
//!    start: temperature range, wind chill below −10 °C, gusts from 40 km/h, fog, snow,
//!    rain, 0 °C isotherm below 2000 m, sky.
//! 3. **Rules and network**: rules active at noon (and at 21 h for night rules) on the
//!    route, or on the pistes of the site without route (core `zone_eval`, C8): access
//!    and leaving the marked routes always, fire when a protected area or a commune (not a
//!    general rule of the country, department or canton) sets it; bivouac rules are kept in
//!    the facts but not said (a day outing). Then the length of
//!    the route without any mobile network (P12), or the share of the site.
//!
//! The voice uses the phrase bank: BERA level, or « no bulletin » then « avalanches remain
//! possible on steep slopes », or « bulletin not available offline »; « your route crosses
//! known avalanche paths »; the weather; « your route crosses a protected area » (without
//! a route: the protected zones with their rules); « fire forbidden in the commune,
//! stoves included »; « part of the route has no network at all ».

use chrono::{Duration, NaiveDate};
use serde::Serialize;
use skimap_common::geom::LonLat;
use skimap_core::zone_eval::{self, Verdict};
use skimap_voice::Event;
use skimap_voice::bank::{Briefing as Line, ZoneRule};
use skimap_voice::clips::{self, DayMonth};
use skimap_zones::ZoneKind;
use skimap_zones::rules::{Period, Topic};

use crate::inputs::SiteData;
use crate::layers::{BeraStatus, densify};

/// An active rule found on the route.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ActiveRule {
    /// Rule identifier.
    pub rule: String,
    /// Topic.
    pub topic: Topic,
    /// Verdict label, French.
    pub verdict: &'static str,
    /// Verdict.
    #[serde(skip)]
    pub kind: Verdict,
    /// Set by a protected area (not a country, department, canton or commune rule).
    pub protected: bool,
    /// Names of the zones.
    pub zones: Vec<String>,
    /// Period, French.
    pub period: String,
    /// Active only at night.
    pub night_only: bool,
    /// Points of the route where it applies.
    pub points: usize,
    /// Summary of the rule.
    pub summary: String,
    /// Primary source.
    pub source_url: String,
}

/// Facts behind the sentences.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Facts {
    /// BERA status.
    pub bera_status: Option<BeraStatus>,
    /// BERA level.
    pub bera_level: Option<u8>,
    /// Name of the route, if any.
    pub route: Option<String>,
    /// Route length, km.
    pub route_km: Option<f64>,
    /// Route length inside historic avalanche paths, km.
    pub clpa_km: Option<f64>,
    /// Historic avalanche paths crossed.
    pub clpa_paths: usize,
    /// Route length without any mobile network, km (or share of the pistes sampled).
    pub no_network_km: Option<f64>,
    /// Points evaluated for the rules.
    pub points_evaluated: usize,
    /// Points outside the territories with known rules (Italy).
    pub points_not_covered: usize,
    /// Rules active.
    pub rules: Vec<ActiveRule>,
}

/// Summary of a day.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Synthesis {
    /// Date.
    pub date: NaiveDate,
    /// At most three sentences, French.
    pub sentences: Vec<String>,
    /// Voice events.
    pub voice: Vec<Event>,
    /// Facts.
    pub facts: Facts,
}

impl Synthesis {
    /// The sentences as one paragraph.
    pub fn text(&self) -> String {
        self.sentences.join(" ")
    }
}

fn clause(text: &str) -> String {
    text.trim_end_matches('.').to_owned()
}

fn fmt_num(x: f64, decimals: usize) -> String {
    format!("{x:.decimals$}").replace('.', ",")
}

fn loc_fr(loc: &str) -> String {
    if let Some(a) = loc.strip_prefix('>') {
        format!("au-dessus de {a} m")
    } else if let Some(a) = loc.strip_prefix('<') {
        format!("en dessous de {a} m")
    } else {
        loc.to_owned()
    }
}

fn avalanche_sentence(data: &SiteData, date: NaiveDate, facts: &mut Facts) -> String {
    let bera = data.bera_for(date);
    let massif = data.site.primary_massif().name;
    let mut s = match bera {
        Some(b) if b.status == BeraStatus::Bulletin && b.level().is_some() => {
            let bl = b.bulletin.as_ref().expect("bulletin");
            let l = b.level().expect("level");
            facts.bera_level = Some(l);
            let mut s = clause(&clips::bera(l).text);
            let r = &bl.risk;
            if let (Some(r1), Some(r2), Some(loc2)) = (r.risk1, r.risk2, r.loc2.as_deref())
                && r1 != r2
            {
                s.push_str(&format!(" {} ({r1} sur 5 plus bas)", loc_fr(loc2)));
            }
            let aspects = bl.aspects.labels();
            if !aspects.is_empty() && aspects.len() < 8 {
                s.push_str(&format!(
                    ", pentes {} les plus exposées",
                    aspects.join(", ")
                ));
            }
            s.push_str(" : lisez le bulletin complet avant de partir");
            s
        }
        Some(b) if b.status == BeraStatus::NoBera => format!(
            "{} (Jura) : des avalanches restent possibles sur les pentes raides, le bulletin \
             du SLF couvre le Jura suisse voisin",
            clause(&clips::Fixed::NoBulletin.clip().text)
        ),
        Some(_) => format!(
            "Le BERA {massif} de ce jour n'est pas disponible hors ligne : consultez-le avant \
             de partir, le risque d'avalanche n'est jamais nul en montagne"
        ),
        None if !data.site.has_bera => format!(
            "{} : des avalanches restent possibles sur les pentes raides",
            clause(&clips::Fixed::NoBulletin.clip().text)
        ),
        None => format!(
            "Le BERA {massif} de ce jour n'est pas chargé : consultez-le avant de partir, le \
             risque d'avalanche n'est jamais nul en montagne"
        ),
    };
    facts.bera_status = bera.map(|b| b.status);
    if let (Some(route), Some(clpa)) = (&data.route, &data.clpa) {
        if clpa.covered {
            let pts = densify(&route.line, 20.0);
            let mut km = 0.0;
            let mut paths: Vec<&str> = Vec::new();
            for (p, len) in &pts {
                if let Some(a) = clpa.areas.find(*p) {
                    km += len / 1000.0;
                    if !paths.contains(&a.label.as_str()) {
                        paths.push(&a.label);
                    }
                }
            }
            facts.clpa_km = Some((km * 100.0).round() / 100.0);
            facts.clpa_paths = paths.len();
            if km >= 0.05 {
                s.push_str(&format!(
                    " ; votre itinéraire traverse {} km de {} zones d'avalanche connues (CLPA)",
                    fmt_num(km, 1),
                    paths.len()
                ));
            } else {
                s.push_str(
                    " ; votre itinéraire ne traverse aucune zone d'avalanche connue (CLPA), ce \
                     qui n'exclut pas une avalanche ailleurs",
                );
            }
        } else {
            s.push_str(" ; pas de carte des avalanches connues (CLPA) dans ce massif");
        }
    }
    s.push('.');
    s
}

fn weather_sentence(data: &SiteData, date: NaiveDate) -> (String, Option<Event>) {
    let start = data
        .route
        .as_ref()
        .and_then(|r| r.line.first().copied())
        .unwrap_or(data.site.bbox.center());
    let Some((p, _)) = data.weather.as_ref().and_then(|w| w.nearest(start)) else {
        return ("Pas de météo hors ligne pour ce jour.".into(), None);
    };
    if !p.covers(date, 9, 16) {
        return (
            format!(
                "Pas de prévision météo hors ligne pour ce jour au point {}.",
                p.name
            ),
            None,
        );
    }
    let w = p.summary(date, 9, 16);
    let mut parts = Vec::new();
    if let (Some(a), Some(b)) = (w.temperature_min_c, w.temperature_max_c) {
        if a.round() == b.round() {
            parts.push(format!("{a:.0} °C"));
        } else {
            parts.push(format!("de {a:.0} à {b:.0} °C"));
        }
    }
    if let Some(c) = w.wind_chill_min_c.filter(|c| *c <= -10.0) {
        parts.push(format!("ressenti {c:.0} °C"));
    }
    match w.gust_max_kmh {
        Some(g) if g >= 40.0 => parts.push(format!("rafales jusqu'à {g:.0} km/h")),
        Some(g) if g >= 25.0 => parts.push(format!("vent modéré (rafales {g:.0} km/h)")),
        Some(_) => parts.push("vent faible".into()),
        None => {}
    }
    if w.fog_hours > 0 {
        parts.push(format!("brouillard possible ({} h)", w.fog_hours));
    }
    if w.snowfall_cm >= 0.5 {
        parts.push(format!("{} cm de neige", fmt_num(w.snowfall_cm, 0)));
    }
    if w.rain_mm >= 0.5 {
        parts.push(format!("pluie ({} mm)", fmt_num(w.rain_mm, 1)));
    }
    if let Some(f) = w.freezing_level_min_m.filter(|f| *f < 2000.0) {
        parts.push(format!(
            "isotherme 0 °C vers {} m",
            (f / 100.0).round() * 100.0
        ));
    }
    match w.cloud_mean_pct {
        Some(c) if c < 30.0 => parts.push("ciel dégagé".into()),
        Some(c) if c > 80.0 => parts.push("ciel couvert".into()),
        _ => {}
    }
    let ev = w.temperature_mean_c.map(|t| {
        Event::Briefing(Line::Weather {
            temperature_c: t,
            wind_kmh: w.wind_max_kmh.unwrap_or(0.0),
            fog: w.fog_hours >= 2,
            snowfall: w.snowfall_cm >= 1.0,
        })
    });
    (
        format!(
            "Météo de 9 h à 16 h ({}, {} m) : {}.",
            p.name,
            p.elevation_m.round(),
            parts.join(", ")
        ),
        ev,
    )
}

fn verdict_words(topic: Topic, v: Verdict, stoves: bool) -> Option<&'static str> {
    Some(match (topic, v) {
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

fn zone_rule(topic: Topic, v: Verdict) -> Option<ZoneRule> {
    Some(match (topic, v) {
        (Topic::Access, Verdict::Forbidden) => ZoneRule::NoEntry,
        (Topic::Access, Verdict::AvoidRecommended) => ZoneRule::Quiet,
        (Topic::LeaveRoutes, Verdict::Forbidden | Verdict::ForbiddenInPlaces) => {
            ZoneRule::StayOnRoutes
        }
        (Topic::Bivouac, Verdict::Forbidden) => ZoneRule::NoBivouac,
        (Topic::Fire, Verdict::Forbidden) => ZoneRule::NoFire,
        _ => return None,
    })
}

fn is_general(kind: ZoneKind) -> bool {
    matches!(
        kind,
        ZoneKind::Country | ZoneKind::Department | ZoneKind::Canton
    )
}

/// Points where the rules are evaluated: the route every 100 m, or every 10th vertex of
/// the site's nordic pistes.
fn rule_points(data: &SiteData) -> Vec<LonLat> {
    match &data.route {
        Some(r) => densify(&r.line, 100.0)
            .into_iter()
            .map(|(p, _)| p)
            .collect(),
        None => data
            .pistes
            .iter()
            .filter(|p| p.uses.iter().any(|u| u == "nordic"))
            .flat_map(|p| p.line.iter().step_by(10).copied())
            .collect(),
    }
}

fn rules_sentence(data: &SiteData, date: NaiveDate, facts: &mut Facts) -> (String, Vec<Event>) {
    let mut events = Vec::new();
    let place = if data.route.is_some() {
        "Sur votre itinéraire"
    } else {
        "Sur les pistes du site"
    };
    let mut parts: Vec<String> = Vec::new();
    if let Some((layer, rules)) = &data.zones {
        let pts = rule_points(data);
        facts.points_evaluated = pts.len();
        let noon = crate::sun::local_to_utc(date.and_hms_opt(12, 0, 0).expect("noon"));
        let night = noon + Duration::hours(9);
        let mut found: Vec<ActiveRule> = Vec::new();
        for p in &pts {
            for (at, is_night) in [(noon, false), (night, true)] {
                let ev = zone_eval::evaluate(layer, rules, *p, at);
                if !is_night && !ev.covered {
                    facts.points_not_covered += 1;
                }
                for r in ev.active_rules() {
                    let kinds: Vec<ZoneKind> = r
                        .zones
                        .iter()
                        .filter_map(|z| ev.zones.iter().find(|h| &h.id == z).map(|h| h.kind))
                        .collect();
                    let general = kinds.iter().all(|k| is_general(*k));
                    if matches!(r.topic, Topic::Bivouac | Topic::Fire) && general {
                        continue;
                    }
                    if verdict_words(r.topic, r.verdict, r.stoves == Some(true)).is_none() {
                        continue;
                    }
                    let names: Vec<String> = r
                        .zones
                        .iter()
                        .filter_map(|z| ev.zones.iter().find(|h| &h.id == z))
                        .filter(|h| !is_general(h.kind))
                        .map(|h| h.name.clone())
                        .collect();
                    match found.iter_mut().find(|f| f.rule == r.rule) {
                        Some(f) => {
                            if !is_night {
                                f.points += 1;
                                f.night_only = false;
                            }
                            for n in names {
                                if !f.zones.contains(&n) {
                                    f.zones.push(n);
                                }
                            }
                        }
                        None => found.push(ActiveRule {
                            rule: r.rule.clone(),
                            topic: r.topic,
                            verdict: verdict_words(r.topic, r.verdict, r.stoves == Some(true))
                                .expect("checked"),
                            kind: r.verdict,
                            protected: kinds
                                .iter()
                                .any(|k| !is_general(*k) && *k != ZoneKind::Commune),
                            zones: names,
                            period: r.period.clone(),
                            night_only: is_night,
                            points: usize::from(!is_night),
                            summary: r.summary.clone(),
                            source_url: r.source_url.clone(),
                        }),
                    }
                }
            }
        }
        // Same wording from several rules (two APPB): one item listing the zones.
        let mut items: Vec<(String, Vec<String>, Option<DayMonth>, bool)> = Vec::new();
        let order = |t: Topic| Topic::ALL.iter().position(|x| *x == t).unwrap_or(9);
        found.sort_by_key(|f| (f.night_only, order(f.topic), f.rule.clone()));
        for f in &found {
            let until = match rules.get(&f.rule).map(|r| &r.period) {
                Some(Period::Yearly { to, .. }) => Some(DayMonth {
                    day: to.day as u8,
                    month: to.month as u8,
                }),
                _ => None,
            };
            match items
                .iter_mut()
                .find(|i| i.0 == f.verdict && i.3 == f.night_only)
            {
                Some(i) => {
                    for z in &f.zones {
                        if !i.1.contains(z) {
                            i.1.push(z.clone());
                        }
                    }
                }
                None => items.push((f.verdict.to_owned(), f.zones.clone(), until, f.night_only)),
            }
            if !f.night_only
                && f.topic != Topic::Bivouac
                && f.protected
                && let Some(zr) = zone_rule(f.topic, f.kind)
            {
                events.push(Event::ProtectedZone {
                    entered: false,
                    zone: f.zones.first().cloned(),
                    rules: vec![zr],
                    until,
                });
            }
        }
        for (words, zones, until, night) in &items {
            if words.starts_with("bivouac") {
                continue;
            }
            let mut s = (*words).clone();
            if *night {
                s = format!("la nuit, {s}");
            }
            match zones.len() {
                0 => {}
                1 | 2 => s.push_str(&format!(" ({})", zones.join(", "))),
                n => s.push_str(&format!(
                    " ({}, {} et {} autres)",
                    zones[0],
                    zones[1],
                    n - 2
                )),
            }
            if let Some(u) = until {
                s.push_str(&format!(
                    " jusqu'au {} {}",
                    if u.day == 1 {
                        "1er".to_owned()
                    } else {
                        u.day.to_string()
                    },
                    skimap_voice::numbers::month_name(u.month)
                ));
            }
            parts.push(s);
        }
        facts.rules = found;
    }
    let rules_text = match (&data.zones, parts.is_empty()) {
        (None, _) => "règles des zones protégées non chargées".to_owned(),
        (Some(_), true) => "aucune règle d'accès connue active ce jour".to_owned(),
        (Some(_), false) => parts.join(", "),
    };
    let not_covered = if facts.points_not_covered > 0 {
        " (hors de France et de Suisse, aucune règle connue)"
    } else {
        ""
    };
    let network = match (&data.no_network, &data.route) {
        (Some(nn), Some(route)) => {
            let km: f64 = densify(&route.line, 50.0)
                .iter()
                .filter(|(p, _)| nn.find(*p).is_some())
                .map(|(_, l)| l / 1000.0)
                .sum();
            facts.no_network_km = Some((km * 100.0).round() / 100.0);
            if km >= 0.05 {
                format!(
                    " ; {} km sans aucun réseau mobile, même pour le 112",
                    fmt_num(km, 1)
                )
            } else {
                " ; réseau mobile annoncé sur tout l'itinéraire (cartes des opérateurs)".into()
            }
        }
        (Some(nn), None) => {
            let pts = rule_points(data);
            let n = pts.iter().filter(|p| nn.find(**p).is_some()).count();
            if pts.is_empty() {
                String::new()
            } else {
                format!(
                    " ; {} % des pistes sans aucun réseau mobile, même pour le 112",
                    (100.0 * n as f64 / pts.len() as f64).round()
                )
            }
        }
        (None, _) => String::new(),
    };
    (
        format!("{place} : {rules_text}{not_covered}{network}."),
        events,
    )
}

/// Summary of `date` for the site (and its planned route, if loaded).
pub fn synthesise(data: &SiteData, date: NaiveDate) -> Synthesis {
    let mut facts = Facts {
        bera_status: None,
        bera_level: None,
        route: data.route.as_ref().map(|r| r.name.clone()),
        route_km: data
            .route
            .as_ref()
            .map(|r| (crate::layers::line_length_m(&r.line) / 10.0).round() / 100.0),
        clpa_km: None,
        clpa_paths: 0,
        no_network_km: None,
        points_evaluated: 0,
        points_not_covered: 0,
        rules: Vec::new(),
    };
    let s1 = avalanche_sentence(data, date, &mut facts);
    let (s2, weather_ev) = weather_sentence(data, date);
    let (s3, zone_events) = rules_sentence(data, date, &mut facts);
    let mut voice = Vec::new();
    match (facts.bera_status, facts.bera_level) {
        (_, Some(l)) => voice.push(Event::Briefing(Line::Avalanche { level: Some(l) })),
        (Some(BeraStatus::NoBera), _) => {
            voice.push(Event::Briefing(Line::Avalanche { level: None }));
            voice.push(Event::Briefing(Line::AvalanchesPossible));
        }
        _ if !data.site.has_bera => {
            voice.push(Event::Briefing(Line::Avalanche { level: None }));
            voice.push(Event::Briefing(Line::AvalanchesPossible));
        }
        // A massif with a bulletin that could not be read offline.
        _ => voice.push(Event::Briefing(Line::AvalancheOffline)),
    }
    if facts.clpa_km.is_some_and(|km| km >= 0.05) {
        voice.push(Event::Briefing(Line::RouteAvalancheAreas));
    }
    voice.extend(weather_ev);
    if data.route.is_some() {
        // On a planned route: one sentence for the protected areas crossed, then the
        // commune fire rule and the stretch without network.
        if facts.rules.iter().any(|r| r.protected && !r.night_only) {
            voice.push(Event::Briefing(Line::RouteProtectedZone));
        }
    } else {
        voice.extend(zone_events);
    }
    if facts.rules.iter().any(|r| {
        r.topic == Topic::Fire && !r.protected && !r.night_only && r.verdict.contains("réchauds")
    }) {
        voice.push(Event::Briefing(Line::FireStovesForbidden));
    }
    if facts.no_network_km.is_some_and(|km| km >= 0.05) {
        voice.push(Event::Briefing(Line::RouteNoNetwork));
    }
    Synthesis {
        date,
        sentences: vec![s1, s2, s3],
        voice,
        facts,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::layers::{Bera, BeraAspects, BeraBulletin, BeraRisk};
    use skimap_common::site::{CERVIERES, CHAPELLE_DES_BOIS};

    fn empty(site: skimap_common::site::Site) -> SiteData {
        SiteData {
            site,
            weather: None,
            snows: vec![],
            beras: vec![],
            status: None,
            domains: vec![],
            pistes: vec![],
            clpa: None,
            no_network: None,
            zones: None,
            route: None,
            graph: None,
            exposure: None,
            missing: vec![],
            sources: vec![],
            files: vec![],
        }
    }

    fn sentences(s: &Synthesis) -> usize {
        s.text().matches(". ").count() + 1
    }

    #[test]
    fn bera_level_uses_the_voice_bank() {
        let mut d = empty(CERVIERES);
        d.beras = vec![Bera {
            site: "cervieres".into(),
            date: "2023-04-15".into(),
            massif: Some("Queyras".into()),
            status: BeraStatus::Bulletin,
            pdf_url: None,
            bulletin: Some(BeraBulletin {
                massif: "QUEYRAS".into(),
                date_validity: None,
                risk: BeraRisk {
                    risk1: Some(1),
                    loc1: Some("<2400".into()),
                    altitude: Some(2400),
                    risk2: Some(2),
                    loc2: Some(">2400".into()),
                    max: Some(2),
                    comment: None,
                },
                aspects: BeraAspects {
                    n: true,
                    ne: true,
                    e: true,
                    nw: true,
                    ..Default::default()
                },
                accidental: None,
                natural: None,
                snow_quality: None,
            }),
            notice: String::new(),
            links: vec![],
        }];
        let s = synthesise(&d, NaiveDate::from_ymd_opt(2023, 4, 15).unwrap());
        assert!(s.sentences[0].starts_with("Risque d'avalanche deux sur cinq, limité au-dessus de 2400 m (1 sur 5 plus bas), pentes nord, nord-est, est, nord-ouest les plus exposées"), "{}", s.sentences[0]);
        assert_eq!(s.sentences.len(), 3);
        assert_eq!(sentences(&s), 3, "{}", s.text());
        assert!(matches!(
            s.voice[0],
            Event::Briefing(Line::Avalanche { level: Some(2) })
        ));
        // Another date: the bulletin does not apply.
        let s = synthesise(&d, NaiveDate::from_ymd_opt(2023, 4, 16).unwrap());
        assert!(s.sentences[0].contains("pas chargé"), "{}", s.sentences[0]);
    }

    #[test]
    fn jura_without_bera_is_never_safe() {
        let d = empty(CHAPELLE_DES_BOIS);
        let s = synthesise(&d, NaiveDate::from_ymd_opt(2026, 2, 25).unwrap());
        assert!(s.sentences[0].starts_with("Pas de bulletin d'avalanche pour ce massif"));
        assert!(s.sentences[0].contains("restent possibles"));
        let all = s.text().to_lowercase();
        assert!(
            !all.contains("sans danger") && !all.contains(" sûr"),
            "{all}"
        );
        assert_eq!(sentences(&s), 3, "{}", s.text());
    }
}
