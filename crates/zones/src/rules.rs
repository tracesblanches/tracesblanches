//! Bivouac, fire and access rules entered by hand from the texts (P10, #23).
//!
//! The rules live in `crates/zones/rules/rules.json` (embedded with [`RuleSet::embedded`]),
//! each with its sources. A rule selects zones of the [`crate::layer::ZoneLayer`] by id or
//! by kind, has a topic ([`Topic`]), an effect ([`Effect`]), a period ([`Period`]), the
//! dates the text is in force and optional exceptions that have no geometry (cadastral
//! parcels).
//!
//! Source kinds ([`SourceKind`]) say how solid the rule is: a rule read only in a draft
//! act, a leaflet or a third-party page is marked as such, never presented as a signed
//! text. The first source of a rule is its primary source.

use std::collections::BTreeSet;
use std::fmt;
use std::str::FromStr;

use jiff::civil::Date;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use skimap_common::geom::LonLat;

use crate::error::{Error, Result};
use crate::layer::{BIODIV_ID_PREFIX, Zone, ZoneKind, ZoneLayer};

/// The rule file, embedded at compile time.
pub const RULES_JSON: &str = include_str!("../rules/rules.json");

/// What a rule is about.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Topic {
    /// Bivouac, camping (tent or not).
    Bivouac,
    /// Leaving the marked routes (ski, snowshoe, walking itineraries).
    LeaveRoutes,
    /// Fire; [`Rule::stoves`] says whether stoves are included.
    Fire,
    /// Entering the area at all (by day, by night, in a season).
    Access,
}

impl Topic {
    /// All topics, in display order.
    pub const ALL: [Topic; 4] = [
        Topic::Access,
        Topic::LeaveRoutes,
        Topic::Bivouac,
        Topic::Fire,
    ];
}

/// What a rule says.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Effect {
    /// Forbidden (with the conditions and exceptions of the rule).
    Forbidden,
    /// Tolerated under conditions ([`Rule::conditions`]).
    Tolerated,
    /// Allowed with the consent of the owner or of whoever has the use of the land.
    ConsentRequired,
    /// Not a legal ban: the area should be avoided (Biodiv'sports sensitive areas).
    AvoidRecommended,
    /// Explicitly allowed.
    Allowed,
}

/// Selects zones of the layer: exactly one of `zone` or `kind`; `practice` narrows a
/// kind selection to zones listing that practice (Biodiv'sports).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Selector {
    /// Zone identifier.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub zone: Option<String>,
    /// Zone kind.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kind: Option<ZoneKind>,
    /// Practice the zone must list.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub practice: Option<String>,
}

impl Selector {
    /// Whether the selector only selects zones of a personal-use source (Biodiv'sports:
    /// `kind: biodiv_sports` or a `biodiv:` zone), which a public build never has.
    pub fn personal(&self) -> bool {
        self.kind == Some(ZoneKind::BiodivSports)
            || self
                .zone
                .as_deref()
                .is_some_and(|z| z.starts_with(BIODIV_ID_PREFIX))
    }

    /// Whether the selector matches the zone.
    pub fn matches(&self, z: &Zone) -> bool {
        let base = match (&self.zone, self.kind) {
            (Some(id), None) => z.id() == id,
            (None, Some(k)) => z.kind() == k,
            _ => false,
        };
        base && self
            .practice
            .as_ref()
            .is_none_or(|p| z.props.practices.iter().any(|q| q == p))
    }
}

/// A day of the year, `MM-DD`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct MonthDay {
    /// Month, 1..=12.
    pub month: u32,
    /// Day of the month.
    pub day: u32,
}

impl MonthDay {
    /// Builds a month-day; 29 February is accepted.
    pub fn new(month: u32, day: u32) -> Result<Self> {
        // 2024 is a leap year: every MM-DD that exists in some year exists in it.
        if skimap_common::dates::ymd(2024, month, day).is_none() {
            return Err(Error::Rules(format!(
                "invalid month-day {month:02}-{day:02}"
            )));
        }
        Ok(Self { month, day })
    }

    /// The month-day of a date.
    pub fn of(date: Date) -> Self {
        Self {
            month: date.month() as u32,
            day: date.day() as u32,
        }
    }
}

impl fmt::Display for MonthDay {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:02}-{:02}", self.month, self.day)
    }
}

impl FromStr for MonthDay {
    type Err = Error;
    fn from_str(s: &str) -> Result<Self> {
        let bad = || Error::Rules(format!("invalid month-day `{s}` (expected MM-DD)"));
        let (m, d) = s.split_once('-').ok_or_else(bad)?;
        MonthDay::new(m.parse().map_err(|_| bad())?, d.parse().map_err(|_| bad())?)
    }
}

impl Serialize for MonthDay {
    fn serialize<S: Serializer>(&self, s: S) -> std::result::Result<S::Ok, S::Error> {
        s.collect_str(self)
    }
}

impl<'de> Deserialize<'de> for MonthDay {
    fn deserialize<D: Deserializer<'de>>(d: D) -> std::result::Result<Self, D::Error> {
        String::deserialize(d)?
            .parse()
            .map_err(serde::de::Error::custom)
    }
}

/// When a rule applies within the year (local legal time of the place).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum Period {
    /// All year.
    AllYear,
    /// Every year from `from` to `to`, both included; wraps over the new year when
    /// `from` is after `to` (15 December to 30 June).
    Yearly {
        /// First day.
        from: MonthDay,
        /// Last day.
        to: MonthDay,
    },
    /// At night, from sunset to sunrise at a reference place (APPB Haut-Doubs art. 6:
    /// "du coucher du soleil au lever du soleil à Pontarlier"). Evaluated by the core
    /// (C8), which computes the sun times.
    Night {
        /// Reference place of the sun times.
        reference: LonLat,
        /// Name of the reference place.
        place: String,
    },
    /// The sensitive months of the matched zone ([`crate::layer::ZoneProps::months`]).
    ZoneMonths,
}

impl Period {
    /// Whether a calendar period contains `date`. `Night` and `ZoneMonths` depend on the
    /// time or on the zone and return `None`.
    pub fn contains_date(&self, date: Date) -> Option<bool> {
        match self {
            Period::AllYear => Some(true),
            Period::Yearly { from, to } => {
                let d = MonthDay::of(date);
                Some(if from <= to {
                    *from <= d && d <= *to
                } else {
                    d >= *from || d <= *to
                })
            }
            Period::Night { .. } | Period::ZoneMonths => None,
        }
    }

    /// Short French description, for display.
    pub fn describe_fr(&self) -> String {
        match self {
            Period::AllYear => "toute l'année".to_owned(),
            Period::Yearly { from, to } => {
                format!("du {} au {}", fr_day(*from), fr_day(*to))
            }
            Period::Night { place, .. } => {
                format!("la nuit (du coucher au lever du soleil à {place})")
            }
            Period::ZoneMonths => "pendant les mois sensibles de la zone".to_owned(),
        }
    }
}

fn fr_day(md: MonthDay) -> String {
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
    let day = if md.day == 1 {
        "1er".to_owned()
    } else {
        md.day.to_string()
    };
    format!("{day} {}", MONTHS[(md.month - 1) as usize])
}

/// Dates during which the text is in force (both optional, both included).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InForce {
    /// First day in force.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub from: Option<Date>,
    /// Last day in force.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub until: Option<Date>,
}

impl InForce {
    /// Whether the text is in force on `date`.
    pub fn contains(&self, date: Date) -> bool {
        self.from.is_none_or(|f| date >= f) && self.until.is_none_or(|u| date <= u)
    }
}

/// An exception without geometry: the evaluation reports it as a note when its period
/// is active, without changing the verdict.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Exception {
    /// What the exception allows, in French.
    pub description: String,
    /// When it applies.
    pub period: Period,
    /// Cadastral parcels named by the text (`<commune> <section><number>`).
    #[serde(default)]
    pub parcels: Vec<String>,
    /// Why there is no geometry.
    #[serde(default)]
    pub geometry_note: Option<String>,
}

/// How solid a source is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SourceKind {
    /// Code or statute (Légifrance, Fedlex, cantonal law).
    Law,
    /// Signed act (arrêté préfectoral or municipal) read in full.
    SignedAct,
    /// Draft of the act submitted to consultation; the signed text was not read.
    DraftAct,
    /// Official summary of the decision (motivations, motif de la décision).
    DecisionSummary,
    /// Official leaflet.
    Leaflet,
    /// Official web page (prefecture, canton, commune, park).
    OfficialPage,
    /// Third-party page (association, tourist office).
    ThirdPartyPage,
    /// Web service publishing the data (Biodiv'sports).
    DataService,
}

/// A source of a rule.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Source {
    /// Public URL.
    pub url: String,
    /// Title of the document.
    pub title: String,
    /// How solid it is.
    pub kind: SourceKind,
    /// Date of the document, ISO, when known.
    #[serde(default)]
    pub date: Option<String>,
    /// Article or section of the document.
    #[serde(default)]
    pub article: Option<String>,
}

/// Host of the Biodiv'sports pages and API (a personal-use source).
const BIODIV_HOST: &str = "https://biodiv-sports.fr";

impl Source {
    /// Whether the source is a personal-use one: Biodiv'sports by its URL, or a title
    /// naming one of [`skimap_common::licence::UNAUTHORISED_SOURCES`].
    pub fn personal(&self) -> bool {
        let title = self.title.to_lowercase();
        self.url.starts_with(BIODIV_HOST)
            || skimap_common::licence::UNAUTHORISED_SOURCES
                .iter()
                .any(|u| title.contains(u))
    }
}

/// One rule.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Rule {
    /// Stable identifier, `[a-z0-9-]+`.
    pub id: String,
    /// Topic.
    pub topic: Topic,
    /// Effect.
    pub effect: Effect,
    /// Zones the rule applies to (any of the selectors).
    pub applies_to: Vec<Selector>,
    /// Period within the year.
    pub period: Period,
    /// Dates the text is in force.
    #[serde(default)]
    pub in_force: InForce,
    /// What the rule says, in French, for display.
    pub summary: String,
    /// Conditions of the effect (tolerances, consent, hours), in French.
    #[serde(default)]
    pub conditions: Option<String>,
    /// The rule only applies to a part of the zone that has no geometry in the layer
    /// (e.g. "à moins de 200 m des bois"): the verdict says "forbidden in places".
    #[serde(default)]
    pub only_where: Option<String>,
    /// Our reading when the text is ambiguous (e.g. bivouac vs camping in R111-33).
    #[serde(default)]
    pub interpretation: Option<String>,
    /// Fire rules: whether stoves are included (`None`: the text does not say).
    #[serde(default)]
    pub stoves: Option<bool>,
    /// Exceptions without geometry.
    #[serde(default)]
    pub exceptions: Vec<Exception>,
    /// What was actually read, and what was not.
    pub verification: String,
    /// Sources, primary first.
    pub sources: Vec<Source>,
}

impl Rule {
    /// The primary source.
    pub fn primary_source(&self) -> &Source {
        &self.sources[0]
    }

    /// Zones of the layer the rule applies to.
    pub fn matching_zones<'a>(&'a self, layer: &'a ZoneLayer) -> impl Iterator<Item = &'a Zone> {
        layer
            .zones
            .iter()
            .filter(|z| self.applies_to.iter().any(|s| s.matches(z)))
    }

    /// Whether the rule applies to the zone.
    pub fn applies_to_zone(&self, z: &Zone) -> bool {
        self.applies_to.iter().any(|s| s.matches(z))
    }
}

/// The rule file.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RuleSet {
    /// Schema version of the file.
    pub schema_version: u32,
    /// Date of the last editorial review.
    pub reviewed: Date,
    /// Rules.
    pub rules: Vec<Rule>,
}

/// Current schema version.
pub const SCHEMA_VERSION: u32 = 1;

impl RuleSet {
    /// Parses and validates a rule file.
    pub fn from_json_str(s: &str) -> Result<Self> {
        let rs: RuleSet =
            serde_json::from_str(s).map_err(|e| Error::Rules(format!("rules.json: {e}")))?;
        rs.validate()?;
        Ok(rs)
    }

    /// The rules embedded in the crate.
    pub fn embedded() -> Result<Self> {
        Self::from_json_str(RULES_JSON)
    }

    /// The rules a public build may carry (`personal_sources` off): without the selectors
    /// of personal-use zones (Biodiv'sports), which such a build does not have, and without
    /// the sources of personal use; a rule left with no selector or no source is dropped
    /// (the Biodiv'sports sensitive areas, the APPB Haut-Doubs core zones whose contours
    /// are Biodiv'sports zone 686). It selects exactly the zones it did on a layer without
    /// personal zones.
    pub fn without_personal_sources(&self) -> Self {
        let rules = self
            .rules
            .iter()
            .filter_map(|r| {
                let mut r = r.clone();
                r.applies_to.retain(|s| !s.personal());
                r.sources.retain(|s| !s.personal());
                (!r.applies_to.is_empty() && !r.sources.is_empty()).then_some(r)
            })
            .collect();
        Self {
            schema_version: self.schema_version,
            reviewed: self.reviewed,
            rules,
        }
    }

    /// The rule file of a build: verbatim with `personal_sources`, otherwise
    /// [`Self::without_personal_sources`] written back as JSON (`rules.json` of the zones
    /// layer, `zones-rules` of a pack).
    pub fn file_for(text: &str, personal_sources: bool) -> Result<String> {
        if personal_sources {
            return Ok(text.to_owned());
        }
        let public = Self::from_json_str(text)?.without_personal_sources();
        public.validate()?;
        let mut out = serde_json::to_string_pretty(&public)
            .map_err(|e| Error::Rules(format!("rules.json: {e}")))?;
        out.push('\n');
        Ok(out)
    }

    /// Rule by identifier.
    pub fn get(&self, id: &str) -> Option<&Rule> {
        self.rules.iter().find(|r| r.id == id)
    }

    /// Checks the consistency of the file; see the error messages for the checks.
    pub fn validate(&self) -> Result<()> {
        let fail = |id: &str, msg: &str| Err(Error::Rules(format!("rule `{id}`: {msg}")));
        if self.schema_version != SCHEMA_VERSION {
            return Err(Error::Rules(format!(
                "schema_version {} (expected {SCHEMA_VERSION})",
                self.schema_version
            )));
        }
        let mut ids = BTreeSet::new();
        for r in &self.rules {
            let id = r.id.as_str();
            let id_ok = !id.is_empty()
                && id
                    .bytes()
                    .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-');
            if !id_ok {
                return fail(id, "identifier must match [a-z0-9-]+");
            }
            if !ids.insert(id) {
                return fail(id, "duplicate identifier");
            }
            if r.sources.is_empty() {
                return fail(id, "no source");
            }
            for s in &r.sources {
                if !s.url.starts_with("https://") || s.url.contains(char::is_whitespace) {
                    return fail(
                        id,
                        &format!("source URL must be https without spaces: {}", s.url),
                    );
                }
                if s.title.trim().is_empty() {
                    return fail(id, "source without title");
                }
            }
            if r.summary.trim().is_empty() || r.verification.trim().is_empty() {
                return fail(id, "empty summary or verification");
            }
            if r.applies_to.is_empty() {
                return fail(id, "applies_to is empty");
            }
            for s in &r.applies_to {
                if s.zone.is_some() == s.kind.is_some() {
                    return fail(id, "a selector needs exactly one of `zone` or `kind`");
                }
                if s.practice.is_some() && s.kind != Some(ZoneKind::BiodivSports) {
                    return fail(id, "`practice` only narrows `kind: biodiv_sports`");
                }
            }
            if r.period == Period::ZoneMonths
                && !r
                    .applies_to
                    .iter()
                    .all(|s| s.kind == Some(ZoneKind::BiodivSports))
            {
                return fail(id, "zone_months period needs biodiv_sports selectors");
            }
            if r.stoves.is_some() && r.topic != Topic::Fire {
                return fail(id, "`stoves` is only meaningful for fire rules");
            }
            if let (Some(f), Some(u)) = (r.in_force.from, r.in_force.until)
                && f > u
            {
                return fail(id, "in_force.from is after in_force.until");
            }
            for e in &r.exceptions {
                if e.period == Period::ZoneMonths {
                    return fail(id, "an exception cannot use zone_months");
                }
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A public rule file names no personal-use source: no Biodiv'sports zone, kind,
    /// page or API, no other unauthorised source; it still parses, and keeps every rule
    /// that selects other zones (the national ones, the APPB Haut-Jura).
    #[test]
    fn public_rules_carry_no_personal_source() {
        let all = RuleSet::embedded().unwrap();
        assert!(
            RULES_JSON.contains("biodiv-sports"),
            "the test needs a personal rule"
        );
        let text = RuleSet::file_for(RULES_JSON, false).unwrap();
        let low = text.to_lowercase();
        for bad in [
            "biodiv-sports",
            "biodiv_sports",
            "biodiv'sports",
            "\"biodiv:",
        ] {
            assert!(!low.contains(bad), "public rules contain {bad}");
        }
        for u in skimap_common::licence::UNAUTHORISED_SOURCES {
            assert!(!low.contains(u), "public rules contain {u}");
        }
        let public = RuleSet::from_json_str(&text).unwrap();
        let dropped: Vec<&str> = all
            .rules
            .iter()
            .filter(|r| public.get(&r.id).is_none())
            .map(|r| r.id.as_str())
            .collect();
        assert!(!dropped.is_empty());
        for id in &dropped {
            let r = all.get(id).unwrap();
            assert!(
                r.applies_to.iter().all(Selector::personal)
                    || r.sources.iter().all(Source::personal),
                "{id} dropped"
            );
        }
        assert!(public.get("appb-haut-jura-leave-routes").is_some());
        assert_eq!(all.rules.len(), public.rules.len() + dropped.len());
        // With the option, the file is the embedded one, byte for byte.
        assert_eq!(RuleSet::file_for(RULES_JSON, true).unwrap(), RULES_JSON);
    }

    fn d(y: i32, m: u32, day: u32) -> Date {
        skimap_common::dates::ymd(y, m, day).unwrap()
    }

    #[test]
    fn month_day_parsing() {
        assert_eq!(
            "12-15".parse::<MonthDay>().unwrap(),
            MonthDay::new(12, 15).unwrap()
        );
        assert!("02-29".parse::<MonthDay>().is_ok());
        for bad in ["02-30", "13-01", "1215", "aa-bb", "00-10"] {
            assert!(bad.parse::<MonthDay>().is_err(), "{bad}");
        }
        assert_eq!(MonthDay::new(6, 30).unwrap().to_string(), "06-30");
    }

    #[test]
    fn yearly_period_wraps_over_new_year() {
        let p = Period::Yearly {
            from: MonthDay::new(12, 15).unwrap(),
            to: MonthDay::new(6, 30).unwrap(),
        };
        assert_eq!(p.contains_date(d(2027, 2, 10)), Some(true));
        assert_eq!(p.contains_date(d(2026, 12, 15)), Some(true));
        assert_eq!(p.contains_date(d(2026, 12, 14)), Some(false));
        assert_eq!(p.contains_date(d(2027, 6, 30)), Some(true));
        assert_eq!(p.contains_date(d(2027, 7, 1)), Some(false));
        assert_eq!(p.contains_date(d(2027, 8, 15)), Some(false));
        let summer = Period::Yearly {
            from: MonthDay::new(7, 1).unwrap(),
            to: MonthDay::new(9, 30).unwrap(),
        };
        assert_eq!(summer.contains_date(d(2027, 8, 15)), Some(true));
        assert_eq!(summer.contains_date(d(2027, 10, 1)), Some(false));
        assert_eq!(Period::AllYear.contains_date(d(2027, 1, 1)), Some(true));
        assert_eq!(Period::ZoneMonths.contains_date(d(2027, 1, 1)), None);
        assert_eq!(p.describe_fr(), "du 15 décembre au 30 juin");
        assert_eq!(summer.describe_fr(), "du 1er juillet au 30 septembre");
    }

    #[test]
    fn in_force_bounds() {
        let f = InForce {
            from: Some(d(2025, 12, 31)),
            until: None,
        };
        assert!(!f.contains(d(2025, 12, 30)));
        assert!(f.contains(d(2025, 12, 31)));
        assert!(InForce::default().contains(d(1900, 1, 1)));
    }

    #[test]
    fn embedded_rules_are_valid_and_sourced() {
        let rs = RuleSet::embedded().unwrap();
        assert!(rs.rules.len() >= 20, "{} rules", rs.rules.len());
        for r in &rs.rules {
            assert!(r.primary_source().url.starts_with("https://"), "{}", r.id);
        }
        // Rules required by the ticket.
        for id in [
            "appb-haut-jura-bivouac",
            "appb-haut-jura-leave-routes",
            "appb-haut-doubs-leave-routes",
            "appb-haut-doubs-core-night",
            "appb-haut-doubs-core-access",
            "appb-haut-doubs-core-bivouac",
            "appb-paradis-bivouac",
            "cervieres-fire",
            "arvieux-2023-22-bivouac",
            "fr-r111-33-bivouac",
            "ch-odf-camping",
        ] {
            assert!(rs.get(id).is_some(), "missing rule {id}");
        }
        let fire = rs.get("cervieres-fire").unwrap();
        assert_eq!(fire.stoves, Some(true));
        let paradis = rs.get("appb-paradis-bivouac").unwrap();
        assert_eq!(paradis.exceptions[0].parcels.len(), 2);
        assert_eq!(paradis.primary_source().kind, SourceKind::SignedAct);
        // Rules read only in a draft are marked as such.
        let hd = rs.get("appb-haut-doubs-leave-routes").unwrap();
        assert_eq!(hd.primary_source().kind, SourceKind::DraftAct);
    }

    #[test]
    fn validation_catches_mistakes() {
        let base = RuleSet::embedded().unwrap();
        let mut rs = base.clone();
        rs.rules[1].id = rs.rules[0].id.clone();
        assert!(rs.validate().is_err(), "duplicate id");

        let mut rs = base.clone();
        rs.rules[0].sources.clear();
        assert!(rs.validate().is_err(), "no source");

        let mut rs = base.clone();
        rs.rules[0].sources[0].url = "http://example.org".into();
        assert!(rs.validate().is_err(), "not https");

        let mut rs = base.clone();
        rs.rules[0].applies_to = vec![Selector {
            zone: Some("x".into()),
            kind: Some(ZoneKind::Appb),
            practice: None,
        }];
        assert!(rs.validate().is_err(), "zone and kind");

        let mut rs = base.clone();
        let i = rs
            .rules
            .iter()
            .position(|r| r.topic != Topic::Fire)
            .unwrap();
        rs.rules[i].stoves = Some(true);
        assert!(rs.validate().is_err(), "stoves on non-fire rule");

        let mut rs = base;
        rs.schema_version = 99;
        assert!(rs.validate().is_err(), "schema version");

        assert!(
            RuleSet::from_json_str(
                r#"{"schema_version":1,"reviewed":"2026-11-14","rules":[],"x":1}"#
            )
            .is_err(),
            "unknown field"
        );
    }
}
