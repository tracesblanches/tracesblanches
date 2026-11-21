//! Zones and rules at a position and a date (C8, #40): active zones, applicable rules
//! (bivouac, leaving routes, fire, access) and one verdict per topic.
//!
//! Inputs are the site's `zones.geojson` ([`ZoneLayer`]) and `rules.json` ([`RuleSet`]),
//! both produced by `skimap-zones` (P9, P10). The evaluation:
//!
//! 1. finds the zones containing the position (boundary included);
//! 2. converts the instant to French / Swiss legal time (CET, CEST from the last Sunday
//!    of March to the last Sunday of October, 01:00 UTC) and takes the local date;
//! 3. for each rule whose selectors match a containing zone, checks that the text is in
//!    force on that date and that its period is active: calendar periods on the local
//!    date, night periods against sunrise and sunset at the rule's reference place,
//!    `zone_months` against the matched zone's sensitive months;
//! 4. reports exceptions without geometry (cadastral parcels) as notes when their period
//!    is active;
//! 5. gives one verdict per topic: the most restrictive active rule, in the order
//!    forbidden, forbidden in places (the rule only covers an unmapped part, such as
//!    200 m around woods), tolerated, consent required, avoid recommended, allowed, no
//!    rule.
//!
//! Outside every jurisdiction zone of the layer (`country`, `canton`), for instance across
//! the Italian border, nothing is known: [`Evaluation::covered`] is false.
//!
//! Sun times come from [`crate::ephemeris`] (sunrise equation, refraction −0.833°), within
//! about a minute of the NOAA algorithm.

use std::fmt::Write as _;

use chrono::{DateTime, Datelike, NaiveDate, NaiveDateTime, Utc};
use skimap_common::geom::LonLat;
use skimap_zones::layer::{Zone, ZoneKind, ZoneLayer};
use skimap_zones::rules::{Effect, Period, Rule, RuleSet, SourceKind, Topic};

/// Overall verdict of a topic, from the most to the least restrictive.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Verdict {
    /// Forbidden here and now.
    Forbidden,
    /// Forbidden in parts of this place that the map does not draw (near woods, private
    /// land crossed by trails): check on the ground.
    ForbiddenInPlaces,
    /// Tolerated under conditions.
    Tolerated,
    /// Allowed with the consent of the owner or occupant.
    ConsentRequired,
    /// Not forbidden, but the area should be avoided (wildlife).
    AvoidRecommended,
    /// Explicitly allowed.
    Allowed,
    /// No rule known.
    NoRule,
}

impl Verdict {
    fn of(rule: &Rule) -> Verdict {
        match rule.effect {
            Effect::Forbidden if rule.only_where.is_some() => Verdict::ForbiddenInPlaces,
            Effect::Forbidden => Verdict::Forbidden,
            Effect::Tolerated => Verdict::Tolerated,
            Effect::ConsentRequired => Verdict::ConsentRequired,
            Effect::AvoidRecommended => Verdict::AvoidRecommended,
            Effect::Allowed => Verdict::Allowed,
        }
    }

    /// French label.
    pub fn label_fr(self) -> &'static str {
        match self {
            Verdict::Forbidden => "interdit",
            Verdict::ForbiddenInPlaces => "interdit par endroits",
            Verdict::Tolerated => "toléré sous conditions",
            Verdict::ConsentRequired => "avec l'accord du propriétaire",
            Verdict::AvoidRecommended => "à éviter",
            Verdict::Allowed => "autorisé",
            Verdict::NoRule => "aucune règle connue",
        }
    }
}

/// Why a matching rule does not apply now.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Inactive {
    /// The text is not (or no longer) in force on that date.
    NotInForce,
    /// Outside the period of the year.
    OutOfPeriod,
    /// Night rule, and it is daytime at the reference place.
    Daytime,
    /// `zone_months` rule, and the matched zones are not sensitive this month.
    ZoneNotSensitive,
    /// Night rule whose reference place has no sunrise or sunset that day (polar).
    NoSunEvent,
}

/// A zone containing the position.
#[derive(Debug, Clone, PartialEq)]
pub struct ZoneHit {
    /// Zone identifier.
    pub id: String,
    /// Kind.
    pub kind: ZoneKind,
    /// Name.
    pub name: String,
    /// Whether the zone is sensitive in the local month (always true without season).
    pub sensitive_now: bool,
}

/// A rule matching a zone containing the position.
#[derive(Debug, Clone, PartialEq)]
pub struct RuleHit {
    /// Rule identifier.
    pub rule: String,
    /// Topic.
    pub topic: Topic,
    /// Verdict of this rule alone when active.
    pub verdict: Verdict,
    /// `None` when the rule applies now, otherwise why not.
    pub inactive: Option<Inactive>,
    /// Containing zones the rule matched.
    pub zones: Vec<String>,
    /// Summary, French.
    pub summary: String,
    /// Conditions, French.
    pub conditions: Option<String>,
    /// Unmapped part of the zone the rule is limited to.
    pub only_where: Option<String>,
    /// Fire rules: stoves included.
    pub stoves: Option<bool>,
    /// Period, French.
    pub period: String,
    /// Active exceptions (without geometry), French.
    pub exception_notes: Vec<String>,
    /// Primary source URL.
    pub source_url: String,
    /// Kind of the primary source (signed act, draft, leaflet...).
    pub source_kind: SourceKind,
}

impl RuleHit {
    /// Whether the rule applies now.
    pub fn active(&self) -> bool {
        self.inactive.is_none()
    }
}

/// Verdict of one topic.
#[derive(Debug, Clone, PartialEq)]
pub struct TopicVerdict {
    /// Topic.
    pub topic: Topic,
    /// Most restrictive active verdict.
    pub verdict: Verdict,
    /// Active rules giving this verdict.
    pub rules: Vec<String>,
    /// Fire: whether an active forbidding rule explicitly includes stoves.
    pub stoves_forbidden: bool,
}

/// Result of [`evaluate`].
#[derive(Debug, Clone, PartialEq)]
pub struct Evaluation {
    /// Position evaluated.
    pub position: LonLat,
    /// Local legal time (CET / CEST) of the instant.
    pub local_time: NaiveDateTime,
    /// Whether the position is inside a jurisdiction of the layer (France, a Swiss
    /// canton): outside, no rule is known.
    pub covered: bool,
    /// Zones containing the position.
    pub zones: Vec<ZoneHit>,
    /// Rules matching those zones, active or not.
    pub rules: Vec<RuleHit>,
    /// One verdict per topic ([`Topic::ALL`] order).
    pub verdicts: Vec<TopicVerdict>,
}

impl Evaluation {
    /// Verdict of a topic.
    pub fn verdict(&self, topic: Topic) -> &TopicVerdict {
        self.verdicts
            .iter()
            .find(|v| v.topic == topic)
            .expect("every topic has a verdict")
    }

    /// Active rules.
    pub fn active_rules(&self) -> impl Iterator<Item = &RuleHit> {
        self.rules.iter().filter(|r| r.active())
    }

    /// Whether a rule is active.
    pub fn is_active(&self, rule: &str) -> bool {
        self.rules.iter().any(|r| r.rule == rule && r.active())
    }

    /// Multi-line French description (for logs and the proof of C8).
    pub fn describe_fr(&self) -> String {
        let mut s = String::new();
        let _ = writeln!(
            s,
            "{:.5} N {:.5} E, {} (heure légale)",
            self.position.lat, self.position.lon, self.local_time
        );
        if !self.covered {
            let _ = writeln!(s, "  hors des territoires couverts : aucune règle connue");
        }
        for v in &self.verdicts {
            let topic = match v.topic {
                Topic::Access => "accès",
                Topic::LeaveRoutes => "sortie des itinéraires",
                Topic::Bivouac => "bivouac",
                Topic::Fire => "feu",
            };
            let stoves = if v.stoves_forbidden {
                " (réchauds compris)"
            } else {
                ""
            };
            let _ = writeln!(
                s,
                "  {topic} : {}{stoves} [{}]",
                v.verdict.label_fr(),
                v.rules.join(", ")
            );
        }
        let zones: Vec<_> = self
            .zones
            .iter()
            .map(|z| {
                if z.sensitive_now {
                    z.id.clone()
                } else {
                    format!("{} (hors saison)", z.id)
                }
            })
            .collect();
        let _ = writeln!(s, "  zones : {}", zones.join(", "));
        for r in self.rules.iter().filter(|r| !r.active()) {
            let _ = writeln!(
                s,
                "  inactive : {} ({:?})",
                r.rule,
                r.inactive.expect("inactive")
            );
        }
        for r in &self.rules {
            for n in &r.exception_notes {
                let _ = writeln!(s, "  exception ({}) : {n}", r.rule);
            }
        }
        s
    }
}

pub use crate::ephemeris::{legal_offset_hours, local_time, sun_times};

fn period_state(
    period: &Period,
    date: NaiveDate,
    at: DateTime<Utc>,
    zones: &[&Zone],
) -> Option<Inactive> {
    match period {
        Period::AllYear | Period::Yearly { .. } => {
            if period.contains_date(date) == Some(true) {
                None
            } else {
                Some(Inactive::OutOfPeriod)
            }
        }
        Period::Night { reference, .. } => match sun_times(date, *reference) {
            Some((rise, set)) if at >= rise && at < set => Some(Inactive::Daytime),
            Some(_) => None,
            None => Some(Inactive::NoSunEvent),
        },
        Period::ZoneMonths => {
            if zones.iter().any(|z| z.active_in_month(date.month())) {
                None
            } else {
                Some(Inactive::ZoneNotSensitive)
            }
        }
    }
}

/// Zones and rules at `position` at the instant `at`.
pub fn evaluate(
    layer: &ZoneLayer,
    rules: &RuleSet,
    position: LonLat,
    at: DateTime<Utc>,
) -> Evaluation {
    let local = local_time(at);
    let date = local.date();
    let containing: Vec<&Zone> = layer.containing(position).collect();
    let covered = containing
        .iter()
        .any(|z| matches!(z.kind(), ZoneKind::Country | ZoneKind::Canton));

    let zones = containing
        .iter()
        .map(|z| ZoneHit {
            id: z.id().to_owned(),
            kind: z.kind(),
            name: z.props.name.clone(),
            sensitive_now: z.active_in_month(date.month()),
        })
        .collect();

    let mut hits = Vec::new();
    for rule in &rules.rules {
        let matched: Vec<&Zone> = containing
            .iter()
            .copied()
            .filter(|z| rule.applies_to_zone(z))
            .collect();
        if matched.is_empty() {
            continue;
        }
        let inactive = if rule.in_force.contains(date) {
            period_state(&rule.period, date, at, &matched)
        } else {
            Some(Inactive::NotInForce)
        };
        let exception_notes = rule
            .exceptions
            .iter()
            .filter(|e| period_state(&e.period, date, at, &matched).is_none())
            .map(|e| {
                let mut n = e.description.clone();
                if !e.parcels.is_empty() {
                    let _ = write!(n, " Parcelles : {}.", e.parcels.join(", "));
                }
                if let Some(g) = &e.geometry_note {
                    let _ = write!(n, " {g}");
                }
                n
            })
            .collect();
        hits.push(RuleHit {
            rule: rule.id.clone(),
            topic: rule.topic,
            verdict: Verdict::of(rule),
            inactive,
            zones: matched.iter().map(|z| z.id().to_owned()).collect(),
            summary: rule.summary.clone(),
            conditions: rule.conditions.clone(),
            only_where: rule.only_where.clone(),
            stoves: rule.stoves,
            period: rule.period.describe_fr(),
            exception_notes,
            source_url: rule.primary_source().url.clone(),
            source_kind: rule.primary_source().kind,
        });
    }

    let verdicts = Topic::ALL
        .iter()
        .map(|&topic| {
            let active: Vec<&RuleHit> = hits
                .iter()
                .filter(|h| h.topic == topic && h.active())
                .collect();
            let verdict = active
                .iter()
                .map(|h| h.verdict)
                .min()
                .unwrap_or(Verdict::NoRule);
            TopicVerdict {
                topic,
                verdict,
                rules: active
                    .iter()
                    .filter(|h| h.verdict == verdict)
                    .map(|h| h.rule.clone())
                    .collect(),
                stoves_forbidden: topic == Topic::Fire
                    && active
                        .iter()
                        .any(|h| h.stoves == Some(true) && h.verdict == Verdict::Forbidden),
            }
        })
        .collect();

    Evaluation {
        position,
        local_time: local,
        covered,
        zones,
        rules: hits,
        verdicts,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ephemeris::last_sunday;
    use chrono::TimeZone;
    use skimap_common::geom::BBox;
    use skimap_zones::geo::MultiPolygon;
    use skimap_zones::geom::bbox_polygon;
    use skimap_zones::layer::{Months, ZoneProps};

    fn zone(id: &str, kind: ZoneKind, b: BBox) -> Zone {
        Zone::new(
            ZoneProps {
                id: id.into(),
                kind,
                name: id.into(),
                source: "test".into(),
                url: None,
                months: None,
                practices: vec![],
                designated: None,
                area_ha: 1.0,
                clipped: false,
                note: None,
            },
            MultiPolygon::new(vec![bbox_polygon(b, 1)]),
        )
    }

    fn utc(s: &str) -> DateTime<Utc> {
        s.parse().unwrap()
    }

    /// Synthetic Risoux: France, Jura, APPB Haut-Jura and the Biodiv'sports
    /// "Grand tétras - hiver" area (December to June, terrestrial practices).
    fn risoux() -> ZoneLayer {
        let mut grouse = zone(
            "biodiv:1183",
            ZoneKind::BiodivSports,
            BBox::new(46.5, 6.0, 46.6, 6.2),
        );
        grouse.props.months = Some(Months::from_numbers([12, 1, 2, 3, 4, 5, 6]).unwrap());
        grouse.props.practices = vec!["manifestation sportive".into(), "terrestre".into()];
        ZoneLayer {
            zones: vec![
                zone(
                    "country:fr",
                    ZoneKind::Country,
                    BBox::new(46.0, 5.0, 47.0, 7.0),
                ),
                zone(
                    "dept:39",
                    ZoneKind::Department,
                    BBox::new(46.0, 5.0, 46.59, 7.0),
                ),
                zone(
                    "inpn:FR3800988",
                    ZoneKind::Appb,
                    BBox::new(46.54, 6.07, 46.57, 6.14),
                ),
                grouse,
            ],
        }
    }

    const RISOUX: LonLat = LonLat::new(6.10, 46.56);

    #[test]
    fn risoux_february_vs_august() {
        let rules = RuleSet::embedded().unwrap();
        let layer = risoux();

        let feb = evaluate(&layer, &rules, RISOUX, utc("2027-02-10T10:00:00Z"));
        assert!(feb.covered);
        assert_eq!(feb.verdict(Topic::LeaveRoutes).verdict, Verdict::Forbidden);
        assert_eq!(
            feb.verdict(Topic::LeaveRoutes).rules,
            ["appb-haut-jura-leave-routes"]
        );
        assert_eq!(feb.verdict(Topic::Bivouac).verdict, Verdict::Forbidden);
        assert_eq!(
            feb.verdict(Topic::Bivouac).rules,
            ["appb-haut-jura-bivouac"]
        );
        assert_eq!(feb.verdict(Topic::Fire).verdict, Verdict::Forbidden);
        assert_eq!(
            feb.verdict(Topic::Access).verdict,
            Verdict::AvoidRecommended
        );
        assert!(feb.is_active("biodiv-sports-terrestre"));

        let aug = evaluate(&layer, &rules, RISOUX, utc("2027-08-10T10:00:00Z"));
        // Off-route ban and bivouac ban only from 15 December to 30 June; the ENJ rule
        // (private land crossed by trails) remains, as an unmapped "in places" ban.
        assert_eq!(
            aug.verdict(Topic::LeaveRoutes).verdict,
            Verdict::ForbiddenInPlaces
        );
        assert_eq!(
            aug.verdict(Topic::LeaveRoutes).rules,
            ["enj-private-land-leave-routes"]
        );
        assert!(!aug.is_active("appb-haut-jura-leave-routes"));
        assert_eq!(
            aug.verdict(Topic::Bivouac).verdict,
            Verdict::ForbiddenInPlaces
        );
        assert!(!aug.is_active("appb-haut-jura-bivouac"));
        assert!(aug.is_active("fr-r111-32-bivouac-consent"));
        // Fire outside the designated places stays forbidden all year in the APPB.
        assert_eq!(aug.verdict(Topic::Fire).verdict, Verdict::Forbidden);
        assert_eq!(aug.verdict(Topic::Fire).rules, ["appb-haut-jura-fire"]);
        // The grouse area is out of season.
        assert_eq!(aug.verdict(Topic::Access).verdict, Verdict::NoRule);
        assert!(
            !aug.zones
                .iter()
                .find(|z| z.id == "biodiv:1183")
                .unwrap()
                .sensitive_now
        );
        let inactive = aug
            .rules
            .iter()
            .find(|r| r.rule == "appb-haut-jura-bivouac")
            .unwrap();
        assert_eq!(inactive.inactive, Some(Inactive::OutOfPeriod));
    }

    #[test]
    fn season_boundaries_use_local_date() {
        let rules = RuleSet::embedded().unwrap();
        let layer = risoux();
        // 14 December 23:30 UTC is 15 December 00:30 in Paris: the ban has started.
        let e = evaluate(&layer, &rules, RISOUX, utc("2026-12-14T23:30:00Z"));
        assert_eq!(
            e.local_time.date(),
            NaiveDate::from_ymd_opt(2026, 12, 15).unwrap()
        );
        assert!(e.is_active("appb-haut-jura-bivouac"));
        let e = evaluate(&layer, &rules, RISOUX, utc("2026-12-14T22:30:00Z"));
        assert!(!e.is_active("appb-haut-jura-bivouac"));
        // 30 June 21:59 UTC is 23:59 CEST, still in; 22:00 UTC is 1 July.
        assert!(
            evaluate(&layer, &rules, RISOUX, utc("2027-06-30T21:59:00Z"))
                .is_active("appb-haut-jura-bivouac")
        );
        assert!(
            !evaluate(&layer, &rules, RISOUX, utc("2027-06-30T22:00:00Z"))
                .is_active("appb-haut-jura-bivouac")
        );
        // Before the APPB was signed.
        let e = evaluate(&layer, &rules, RISOUX, utc("2019-02-10T10:00:00Z"));
        let h = e
            .rules
            .iter()
            .find(|r| r.rule == "appb-haut-jura-bivouac")
            .unwrap();
        assert_eq!(h.inactive, Some(Inactive::NotInForce));
    }

    #[test]
    fn cervieres_village_spr_and_fire_with_stoves() {
        let rules = RuleSet::embedded().unwrap();
        let layer = ZoneLayer {
            zones: vec![
                zone(
                    "country:fr",
                    ZoneKind::Country,
                    BBox::new(44.0, 6.0, 45.0, 7.0),
                ),
                zone(
                    "commune:05027",
                    ZoneKind::Commune,
                    BBox::new(44.82, 6.70, 44.92, 6.86),
                ),
                zone(
                    "gpu:AC4-1",
                    ZoneKind::Spr,
                    BBox::new(44.86, 6.71, 44.87, 6.73),
                ),
            ],
        };
        let village = LonLat::new(6.7230, 44.8640);
        let e = evaluate(&layer, &rules, village, utc("2027-01-05T12:00:00Z"));
        assert_eq!(e.verdict(Topic::Bivouac).verdict, Verdict::Forbidden);
        assert_eq!(e.verdict(Topic::Bivouac).rules, ["fr-r111-33-bivouac"]);
        let fire = e.verdict(Topic::Fire);
        assert_eq!(fire.verdict, Verdict::Forbidden);
        assert_eq!(fire.rules, ["cervieres-fire"]);
        assert!(fire.stoves_forbidden);
        // Out of the SPR, bivouac needs the owner's consent; fire stays forbidden.
        let fonts = evaluate(
            &layer,
            &rules,
            LonLat::new(6.792, 44.842),
            utc("2027-01-05T12:00:00Z"),
        );
        assert_eq!(
            fonts.verdict(Topic::Bivouac).verdict,
            Verdict::ConsentRequired
        );
        assert!(fonts.verdict(Topic::Fire).stoves_forbidden);
        // Before the 2022 municipal ban, only L131-1 (near woods) remained.
        let old = evaluate(
            &layer,
            &rules,
            LonLat::new(6.792, 44.842),
            utc("2021-01-05T12:00:00Z"),
        );
        assert_eq!(old.verdict(Topic::Fire).verdict, Verdict::ForbiddenInPlaces);
        assert!(!old.verdict(Topic::Fire).stoves_forbidden);
    }

    #[test]
    fn chapelle_site_inscrit_and_italy_uncovered() {
        let rules = RuleSet::embedded().unwrap();
        let layer = ZoneLayer {
            zones: vec![
                zone(
                    "country:fr",
                    ZoneKind::Country,
                    BBox::new(46.5, 6.0, 46.7, 6.2),
                ),
                zone(
                    "dept:25",
                    ZoneKind::Department,
                    BBox::new(46.59, 6.0, 46.7, 6.2),
                ),
                zone(
                    "gpu:AC2-130009012-22545-1-1",
                    ZoneKind::SiteInscrit,
                    BBox::new(46.58, 6.08, 46.63, 6.15),
                ),
            ],
        };
        let e = evaluate(
            &layer,
            &rules,
            LonLat::new(6.087, 46.588),
            utc("2027-07-20T18:00:00Z"),
        );
        assert_eq!(e.verdict(Topic::Bivouac).verdict, Verdict::Forbidden);
        assert_eq!(e.verdict(Topic::Bivouac).rules, ["fr-r111-33-bivouac"]);
        let out = evaluate(
            &layer,
            &rules,
            LonLat::new(6.9, 44.85),
            utc("2027-07-20T18:00:00Z"),
        );
        assert!(!out.covered);
        assert!(out.verdicts.iter().all(|v| v.verdict == Verdict::NoRule));
    }

    #[test]
    fn haut_doubs_core_zone_night_and_season() {
        let rules = RuleSet::embedded().unwrap();
        let mut core = zone(
            "biodiv:686",
            ZoneKind::BiodivSports,
            BBox::new(46.72, 6.14, 46.79, 6.20),
        );
        core.props.months = Some(Months::from_numbers([12, 1, 2, 3, 4, 5, 6]).unwrap());
        core.props.practices = vec!["terrestre".into()];
        let layer = ZoneLayer {
            zones: vec![
                zone(
                    "country:fr",
                    ZoneKind::Country,
                    BBox::new(46.0, 6.0, 47.0, 7.0),
                ),
                zone(
                    "inpn:FR3801178",
                    ZoneKind::Appb,
                    BBox::new(46.6, 6.1, 46.8, 6.25),
                ),
                core,
            ],
        };
        let p = LonLat::new(6.17, 46.75);
        // July, noon: no seasonal ban, daytime.
        let noon = evaluate(&layer, &rules, p, utc("2027-07-15T10:00:00Z"));
        assert_eq!(noon.verdict(Topic::Access).verdict, Verdict::NoRule);
        assert_eq!(
            noon.rules
                .iter()
                .find(|r| r.rule == "appb-haut-doubs-core-night")
                .unwrap()
                .inactive,
            Some(Inactive::Daytime)
        );
        // July, 23:00 local (21:00 UTC; sunset at Pontarlier 19:25 UTC): night ban.
        let night = evaluate(&layer, &rules, p, utc("2027-07-15T21:00:00Z"));
        assert_eq!(night.verdict(Topic::Access).verdict, Verdict::Forbidden);
        assert_eq!(
            night.verdict(Topic::Access).rules,
            ["appb-haut-doubs-core-night"]
        );
        // 19:20 UTC is still day, 19:30 UTC night (oracle sunset 19:25:28 UTC).
        assert!(
            !evaluate(&layer, &rules, p, utc("2027-07-15T19:20:00Z"))
                .is_active("appb-haut-doubs-core-night")
        );
        assert!(
            evaluate(&layer, &rules, p, utc("2027-07-15T19:30:00Z"))
                .is_active("appb-haut-doubs-core-night")
        );
        // February noon: seasonal access ban; bivouac and fire banned all year.
        let feb = evaluate(&layer, &rules, p, utc("2027-02-15T11:00:00Z"));
        assert_eq!(
            feb.verdict(Topic::Access).rules,
            ["appb-haut-doubs-core-access"]
        );
        assert_eq!(feb.verdict(Topic::LeaveRoutes).verdict, Verdict::Forbidden);
        assert_eq!(
            feb.verdict(Topic::Bivouac).rules,
            ["appb-haut-doubs-core-bivouac"]
        );
        // Before the signature (31/12/2025): nothing from this APPB.
        let before = evaluate(&layer, &rules, p, utc("2025-02-15T11:00:00Z"));
        assert!(!before.is_active("appb-haut-doubs-core-access"));
        assert!(!before.is_active("appb-haut-doubs-leave-routes"));
    }

    #[test]
    fn night_rules_start_at_sunset() {
        let rules = RuleSet::embedded().unwrap();
        let mut core = zone(
            "biodiv:686",
            ZoneKind::BiodivSports,
            BBox::new(46.72, 6.14, 46.79, 6.20),
        );
        core.props.months = Some(Months::from_numbers([12, 1, 2, 3, 4, 5, 6]).unwrap());
        core.props.practices = vec!["terrestre".into()];
        let layer = ZoneLayer {
            zones: vec![
                zone(
                    "country:fr",
                    ZoneKind::Country,
                    BBox::new(46.0, 6.0, 47.0, 7.0),
                ),
                zone(
                    "inpn:FR3801178",
                    ZoneKind::Appb,
                    BBox::new(46.6, 6.1, 46.8, 6.25),
                ),
                core,
            ],
        };
        let id = "appb-haut-doubs-core-night";
        let rule = rules.rules.iter().find(|r| r.id == id).unwrap();
        let Period::Night { reference, .. } = &rule.period else {
            panic!("{id}: not a night rule")
        };
        let date = NaiveDate::from_ymd_opt(2027, 7, 15).unwrap();
        let (rise, set) = sun_times(date, *reference).unwrap();
        let p = LonLat::new(6.17, 46.75);
        let state = |at| {
            evaluate(&layer, &rules, p, at)
                .rules
                .iter()
                .find(|r| r.rule == id)
                .unwrap()
                .inactive
        };
        // Night from sunset (included) to sunrise (excluded).
        assert_eq!(state(set), None);
        assert_eq!(
            state(set - chrono::Duration::seconds(1)),
            Some(Inactive::Daytime)
        );
        assert_eq!(state(rise), Some(Inactive::Daytime));
        assert_eq!(state(rise - chrono::Duration::seconds(1)), None);
        let e = evaluate(&layer, &rules, p, set);
        assert!(e.active_rules().any(|r| r.rule == id));
        assert!(e.active_rules().all(|r| r.active()));
        assert!(e.active_rules().count() < e.rules.len());
    }

    #[test]
    fn paradis_parcel_exception_is_a_note() {
        let rules = RuleSet::embedded().unwrap();
        let layer = ZoneLayer {
            zones: vec![
                zone(
                    "country:fr",
                    ZoneKind::Country,
                    BBox::new(46.0, 6.0, 47.0, 7.0),
                ),
                zone(
                    "inpn:FR3800137",
                    ZoneKind::Appb,
                    BBox::new(46.64, 6.0, 46.69, 6.06),
                ),
            ],
        };
        let p = LonLat::new(6.03, 46.66);
        let aug = evaluate(&layer, &rules, p, utc("2027-08-10T10:00:00Z"));
        assert_eq!(aug.verdict(Topic::Bivouac).verdict, Verdict::Forbidden);
        let hit = aug
            .rules
            .iter()
            .find(|r| r.rule == "appb-paradis-bivouac")
            .unwrap();
        assert_eq!(hit.exception_notes.len(), 1);
        assert!(hit.exception_notes[0].contains("B 771"));
        // The parcel list is appended to the description of the exception.
        assert!(
            hit.exception_notes[0].contains(" Parcelles : "),
            "{}",
            hit.exception_notes[0]
        );
        let feb = evaluate(&layer, &rules, p, utc("2027-02-20T10:00:00Z"));
        let hit = feb
            .rules
            .iter()
            .find(|r| r.rule == "appb-paradis-bivouac")
            .unwrap();
        assert!(hit.exception_notes.is_empty());
        // Fire ban in woods from 15 February to 31 May.
        assert!(feb.is_active("appb-paradis-fire"));
        assert!(!aug.is_active("appb-paradis-fire"));
        assert!(feb.describe_fr().contains("bivouac : interdit"));
    }

    #[test]
    fn legal_time_transitions() {
        // 2027: summer time from 28 March 01:00 UTC to 31 October 01:00 UTC.
        assert_eq!(legal_offset_hours(utc("2027-03-28T00:59:59Z")), 1);
        assert_eq!(legal_offset_hours(utc("2027-03-28T01:00:00Z")), 2);
        assert_eq!(legal_offset_hours(utc("2027-10-31T00:59:59Z")), 2);
        assert_eq!(legal_offset_hours(utc("2027-10-31T01:00:00Z")), 1);
        assert_eq!(
            last_sunday(2026, 10),
            NaiveDate::from_ymd_opt(2026, 10, 25).unwrap()
        );
        assert_eq!(
            local_time(utc("2027-01-01T12:00:00Z")).to_string(),
            "2027-01-01 13:00:00"
        );
    }

    #[test]
    fn sun_times_match_reference() {
        // Reference: astral 3.2 (NOAA), Pontarlier 46.9035 N 6.3547 E, UTC.
        let pontarlier = LonLat::new(6.3547, 46.9035);
        let cases = [
            ((2027, 2, 15), "06:39:45", "16:58:20"),
            ((2027, 6, 21), "03:40:09", "19:32:32"),
            ((2026, 12, 21), "07:17:18", "15:47:58"),
            ((2027, 7, 15), "03:55:05", "19:25:28"),
        ];
        for ((y, m, d), rise, set) in cases {
            let date = NaiveDate::from_ymd_opt(y, m, d).unwrap();
            let (r, s) = sun_times(date, pontarlier).unwrap();
            for (got, want) in [(r, rise), (s, set)] {
                let want = Utc.from_utc_datetime(&date.and_time(want.parse().unwrap()));
                let diff = (got - want).num_seconds().abs();
                println!("{date}: {got} vs {want} ({diff} s)");
                assert!(diff <= 90, "{date}: {got} vs {want} ({diff} s)");
            }
        }
        // Polar night at 80° N in December.
        assert!(
            sun_times(
                NaiveDate::from_ymd_opt(2026, 12, 21).unwrap(),
                LonLat::new(15.0, 80.0)
            )
            .is_none()
        );
    }

    // ---- Real layers of both sites. ----
    //
    // By default on the versioned reference layers (`fixtures/data/out/<site>/zones/`,
    // without the Biodiv'sports areas: no licence found); the `full_data_` variants read
    // the complete local data (`$SKIMAP_DATA_DIR`, default `data/`) and also check the
    // Biodiv'sports areas: `cargo test -p skimap-core -- --ignored full_data_`.

    /// Where the real layers are read.
    #[derive(Clone, Copy, PartialEq)]
    enum Data {
        /// `fixtures/data` (no Biodiv'sports).
        Fixtures,
        /// Complete local data.
        Full,
    }

    /// Real zones and rules of `site`, and whether the layer was built with the personal
    /// sources (a source of `zones.meta.json` marked `personal_use_only`: Biodiv'sports);
    /// built without (the fixtures, the weekly build), it has no Biodiv'sports area.
    fn real(site: &str, data: Data) -> (ZoneLayer, RuleSet, bool) {
        let here = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
        let root = match data {
            Data::Fixtures => here.join("../../fixtures/data"),
            Data::Full => std::env::var_os("SKIMAP_DATA_DIR")
                .map(std::path::PathBuf::from)
                .unwrap_or_else(|| here.join("../../data")),
        };
        let dir = root.join("out").join(site).join("zones");
        let read = |name: &str| {
            let p = dir.join(name);
            std::fs::read_to_string(&p).unwrap_or_else(|e| {
                panic!(
                    "{}: {e} (fixtures: `skimap fixtures`; full data: `skimap zones {site}`)",
                    p.display()
                )
            })
        };
        let layer = ZoneLayer::from_geojson_str(&read("zones.geojson")).unwrap();
        let meta: serde_json::Value = serde_json::from_str(&read("zones.meta.json")).unwrap();
        let personal = meta["sources"]
            .as_array()
            .into_iter()
            .flatten()
            .any(|s| s["personal_use_only"] == true);
        assert!(
            data == Data::Full || !personal,
            "personal sources in the fixtures"
        );
        if !personal {
            assert!(
                layer
                    .zones
                    .iter()
                    .all(|z| z.kind() != ZoneKind::BiodivSports),
                "{site}: Biodiv'sports areas in a layer built without the personal sources"
            );
        }
        (
            layer,
            RuleSet::from_json_str(&read("rules.json")).unwrap(),
            personal,
        )
    }

    #[test]
    fn real_risoux_february_vs_august() {
        real_risoux_check(Data::Fixtures);
    }

    #[test]
    #[ignore = "full-data: data/out/chapelle-des-bois/zones"]
    fn full_data_real_risoux_february_vs_august() {
        real_risoux_check(Data::Full);
    }

    fn real_risoux_check(data: Data) {
        let (layer, rules, personal) = real("chapelle-des-bois", data);
        let feb = evaluate(&layer, &rules, RISOUX, utc("2027-02-10T10:00:00Z"));
        println!("{}", feb.describe_fr());
        let ids: Vec<_> = feb.zones.iter().map(|z| z.id.as_str()).collect();
        for id in ["inpn:FR3800988", "country:fr", "dept:39"] {
            assert!(ids.contains(&id), "{id} not in {ids:?}");
        }
        assert_eq!(
            feb.verdict(Topic::LeaveRoutes).rules,
            ["appb-haut-jura-leave-routes"]
        );
        assert_eq!(feb.verdict(Topic::Bivouac).verdict, Verdict::Forbidden);
        if personal {
            // Biodiv'sports « Grand tétras - hiver »: avoid the area in winter.
            assert!(ids.contains(&"biodiv:1183"), "{ids:?}");
            assert_eq!(
                feb.verdict(Topic::Access).verdict,
                Verdict::AvoidRecommended
            );
        } else {
            assert_eq!(feb.verdict(Topic::Access).verdict, Verdict::NoRule);
        }
        let aug = evaluate(&layer, &rules, RISOUX, utc("2027-08-10T10:00:00Z"));
        println!("{}", aug.describe_fr());
        assert!(!aug.is_active("appb-haut-jura-leave-routes"));
        assert!(!aug.is_active("appb-haut-jura-bivouac"));
        assert_eq!(
            aug.verdict(Topic::Bivouac).verdict,
            Verdict::ForbiddenInPlaces
        );
        assert_eq!(aug.verdict(Topic::Fire).rules, ["appb-haut-jura-fire"]);
        assert_eq!(aug.verdict(Topic::Access).verdict, Verdict::NoRule);
    }

    #[test]
    fn real_chapelle_site_inscrit_and_swiss_side() {
        chapelle_site_inscrit_and_swiss_side(Data::Fixtures);
    }

    #[test]
    #[ignore = "full-data: data/out/chapelle-des-bois/zones"]
    fn full_data_real_chapelle_site_inscrit_and_swiss_side() {
        chapelle_site_inscrit_and_swiss_side(Data::Full);
    }

    fn chapelle_site_inscrit_and_swiss_side(data: Data) {
        let (layer, rules, _) = real("chapelle-des-bois", data);
        // Lac des Mortes, in the site inscrit.
        let e = evaluate(
            &layer,
            &rules,
            LonLat::new(6.087, 46.588),
            utc("2027-01-10T10:00:00Z"),
        );
        println!("{}", e.describe_fr());
        assert!(e.zones.iter().any(|z| z.kind == ZoneKind::SiteInscrit));
        assert_eq!(e.verdict(Topic::Bivouac).verdict, Verdict::Forbidden);
        assert_eq!(e.verdict(Topic::Bivouac).rules, ["fr-r111-33-bivouac"]);
        // Swiss Risoud (Le Chenit, Vaud), outside the hunting reserve.
        let ch = evaluate(
            &layer,
            &rules,
            LonLat::new(6.15, 46.60),
            utc("2027-01-10T10:00:00Z"),
        );
        println!("{}", ch.describe_fr());
        assert!(ch.covered);
        assert_eq!(ch.verdict(Topic::Bivouac).rules, ["ch-chenit-camping"]);
        assert_eq!(ch.verdict(Topic::Access).verdict, Verdict::Allowed);
        assert!(ch.is_active("ch-vd-lccr-27-camping"));
        // Le Noirmont federal hunting reserve, south edge of the box.
        let jbg = layer
            .zones
            .iter()
            .find(|z| z.kind() == ZoneKind::DistrictFranc)
            .unwrap();
        let p = interior_point(jbg);
        let n = evaluate(&layer, &rules, p, utc("2027-01-10T10:00:00Z"));
        println!("{}", n.describe_fr());
        assert!(n.is_active("ch-odf-camping") && n.is_active("ch-odf-ski"));
        assert_eq!(n.verdict(Topic::LeaveRoutes).verdict, Verdict::Forbidden);
    }

    #[test]
    fn real_cervieres_village_and_casse_deserte() {
        cervieres_village_and_casse_deserte(Data::Fixtures);
    }

    #[test]
    #[ignore = "full-data: data/out/cervieres/zones"]
    fn full_data_real_cervieres_village_and_casse_deserte() {
        cervieres_village_and_casse_deserte(Data::Full);
    }

    fn cervieres_village_and_casse_deserte(data: Data) {
        let (layer, rules, _) = real("cervieres", data);
        let village = evaluate(
            &layer,
            &rules,
            LonLat::new(6.7230, 44.8640),
            utc("2027-01-05T12:00:00Z"),
        );
        println!("{}", village.describe_fr());
        assert!(village.zones.iter().any(|z| z.kind == ZoneKind::Spr));
        assert_eq!(village.verdict(Topic::Bivouac).verdict, Verdict::Forbidden);
        assert!(village.verdict(Topic::Fire).stoves_forbidden);
        assert_eq!(village.verdict(Topic::Fire).rules, ["cervieres-fire"]);
        // Casse Déserte (site classé, Arvieux).
        let casse = evaluate(
            &layer,
            &rules,
            LonLat::new(6.7381, 44.8119),
            utc("2027-07-05T19:00:00Z"),
        );
        println!("{}", casse.describe_fr());
        assert!(casse.zones.iter().any(|z| z.id == "inpn:FR3200828"));
        assert_eq!(casse.verdict(Topic::Bivouac).verdict, Verdict::Forbidden);
        assert!(casse.is_active("arvieux-2023-22-bivouac"));
        // Arvieux side outside the site classé: bivouac tolerated under conditions.
        let arvieux = evaluate(
            &layer,
            &rules,
            LonLat::new(6.75, 44.79),
            utc("2027-07-05T19:00:00Z"),
        );
        println!("{}", arvieux.describe_fr());
        assert_eq!(arvieux.verdict(Topic::Bivouac).verdict, Verdict::Tolerated);
        // Italy, east of the col de Bousson: no data.
        let italy = evaluate(
            &layer,
            &rules,
            LonLat::new(6.85, 44.90),
            utc("2027-01-05T12:00:00Z"),
        );
        println!("{}", italy.describe_fr());
        assert!(!italy.covered);
    }

    /// A point inside the first polygon of a zone (centre of a horizontal chord).
    fn interior_point(z: &Zone) -> LonLat {
        use skimap_zones::geo::{BoundingRect, Intersects, Line, Point};
        let poly = &z.geometry.0[0];
        let r = poly.bounding_rect().unwrap();
        let y = (r.min().y + r.max().y) / 2.0;
        let mut xs: Vec<f64> = poly
            .exterior()
            .lines()
            .filter_map(|l: Line<f64>| {
                let (a, b) = (l.start, l.end);
                if (a.y <= y) != (b.y <= y) {
                    Some(a.x + (y - a.y) * (b.x - a.x) / (b.y - a.y))
                } else {
                    None
                }
            })
            .collect();
        xs.sort_by(f64::total_cmp);
        let p = LonLat::new((xs[0] + xs[1]) / 2.0, y);
        assert!(z.geometry.intersects(&Point::new(p.lon, p.lat)));
        p
    }
}
