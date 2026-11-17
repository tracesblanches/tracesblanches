//! The rules of a place at a date in the app (V6 #69, F11): a touch on the map gives the
//! zones containing the place and their rules (bivouac, access, leaving the marked
//! routes, fire) at that instant, each with its period and its source; the rules not in
//! force at that instant are listed with why (out of period, daytime for a night rule,
//! zone out of its sensitive months).
//!
//! The evaluation is the core's (`skimap_core::zone_eval`, C8) on the pack's zone layer
//! and rules (P9, P10). The zone layer of a pack is read once and kept.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, OnceLock};

use chrono::{DateTime, Utc};
use skimap_common::geom::LonLat;
use skimap_core::zone_eval::{self, Verdict};
use skimap_pack::Pack;
use skimap_zones::layer::ZoneLayer;
use skimap_zones::rules::{RuleSet, Topic};

use crate::PackError;
use crate::card::Lang;
use crate::text;

/// The verdict of one topic at the place.
#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct TopicLine {
    /// `bivouac`, `access`, `leave_routes`, `fire`.
    pub topic: String,
    /// « Bivouac ».
    pub title: String,
    /// « interdit », « toléré sous conditions », « aucune règle connue ».
    pub verdict: String,
    /// Forbidden or forbidden in places (shown on the error colour).
    pub restrictive: bool,
}

/// One rule of the place.
#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct RuleLine {
    /// Topic title.
    pub topic: String,
    /// Verdict of the rule.
    pub verdict: String,
    /// In force at that instant.
    pub active: bool,
    /// Why not, when not in force.
    pub inactive: Option<String>,
    /// What it says.
    pub summary: String,
    /// When it applies, French.
    pub period: String,
    /// Conditions.
    pub conditions: Option<String>,
    /// Only a part of the zone (unmapped).
    pub only_where: Option<String>,
    /// Stoves included in a fire ban.
    pub stoves: Option<bool>,
    /// Title of the main source.
    pub source_title: String,
    /// Its address.
    pub source_url: String,
    /// « arrêté signé », « projet d'arrêté »…
    pub source_kind: String,
}

/// The rules of a place at an instant.
#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct PlaceRules {
    /// « le 15/02 à 9 h 00 ».
    pub at_text: String,
    /// The place is in a territory the rules cover (France, Swiss cantons of the packs).
    pub covered: bool,
    /// Verdict per topic, bivouac first.
    pub topics: Vec<TopicLine>,
    /// Zones containing the place: « APPB du Risoux (hors saison) ».
    pub zones: Vec<String>,
    /// Rules, in force first.
    pub rules: Vec<RuleLine>,
    /// What this is not (never a permission in itself).
    pub note: String,
}

type Loaded = Arc<(ZoneLayer, RuleSet)>;

fn cache() -> &'static Mutex<HashMap<PathBuf, Loaded>> {
    static C: OnceLock<Mutex<HashMap<PathBuf, Loaded>>> = OnceLock::new();
    C.get_or_init(|| Mutex::new(HashMap::new()))
}

fn load(pack_dir: &str) -> Result<Loaded, PackError> {
    let key = PathBuf::from(pack_dir);
    if let Some(l) = cache()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .get(&key)
    {
        return Ok(l.clone());
    }
    let pack = Pack::open(key.clone())?;
    let read = |id: &str| -> Result<String, PackError> {
        let l = pack.layer(id)?;
        std::fs::read_to_string(pack.path(&l.file)?).map_err(|e| PackError::Invalid {
            detail: format!("{id}: {e}"),
        })
    };
    let bad = |e: skimap_zones::Error| PackError::Invalid {
        detail: e.to_string(),
    };
    let layer = ZoneLayer::from_geojson_str(&read("zones")?).map_err(bad)?;
    let rules = RuleSet::from_json_str(&read("zones-rules")?).map_err(bad)?;
    let l = Arc::new((layer, rules));
    cache()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .insert(key, l.clone());
    Ok(l)
}

fn topic_key(t: Topic) -> &'static str {
    match t {
        Topic::Bivouac => "bivouac",
        Topic::Access => "access",
        Topic::LeaveRoutes => "leave_routes",
        Topic::Fire => "fire",
    }
}

/// The rules at (`lon`, `lat`) at `at_ms` (epoch milliseconds) on the zones of the pack
/// in `pack_dir`, texts in `lang` (the summaries, periods and conditions of the rules are
/// the sources' own words, not translated).
#[uniffi::export]
pub fn place_rules(
    pack_dir: String,
    lon: f64,
    lat: f64,
    at_ms: i64,
    lang: Lang,
) -> Result<PlaceRules, PackError> {
    let tx = text::of(lang);
    let l = load(&pack_dir)?;
    let (layer, rules) = (&l.0, &l.1);
    let at: DateTime<Utc> = DateTime::from_timestamp_millis(at_ms).unwrap_or_else(Utc::now);
    let ev = zone_eval::evaluate(layer, rules, LonLat::new(lon, lat), at);
    let order = [
        Topic::Bivouac,
        Topic::Access,
        Topic::LeaveRoutes,
        Topic::Fire,
    ];
    let topics = order
        .iter()
        .map(|t| {
            let v = ev.verdict(*t);
            let stoves = if v.stoves_forbidden {
                tx.stoves_included()
            } else {
                ""
            };
            TopicLine {
                topic: topic_key(*t).into(),
                title: tx.rule_topic(*t).into(),
                verdict: format!("{}{stoves}", tx.rule_verdict(v.verdict)),
                restrictive: matches!(v.verdict, Verdict::Forbidden | Verdict::ForbiddenInPlaces),
            }
        })
        .collect();
    let zones = ev
        .zones
        .iter()
        .map(|z| {
            if z.sensitive_now {
                z.name.clone()
            } else {
                tx.zone_off_season(&z.name)
            }
        })
        .collect();
    let mut lines: Vec<RuleLine> = ev
        .rules
        .iter()
        .map(|r| {
            let src = rules.get(&r.rule).map(|x| x.primary_source());
            RuleLine {
                topic: tx.rule_topic(r.topic).into(),
                verdict: tx.rule_verdict(r.verdict).into(),
                active: r.active(),
                inactive: r.inactive.map(|i| tx.rule_inactive(i).to_owned()),
                summary: r.summary.clone(),
                period: r.period.clone(),
                conditions: r.conditions.clone(),
                only_where: r.only_where.clone(),
                stoves: r.stoves,
                source_title: src.map(|s| s.title.clone()).unwrap_or_default(),
                source_url: r.source_url.clone(),
                source_kind: tx.source_kind(r.source_kind).into(),
            }
        })
        .collect();
    lines.sort_by_key(|r| !r.active);
    let local = ev.local_time;
    Ok(PlaceRules {
        at_text: tx.rules_at(local),
        covered: ev.covered,
        topics,
        zones,
        rules: lines,
        note: tx.rules_note().into(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pack(site: &str) -> String {
        std::env::var("SKIMAP_DATA_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|_| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../data"))
            .join("out")
            .join(site)
            .join("pack")
            .display()
            .to_string()
    }

    /// The Risoux APPB at Chapelle-des-Bois: leaving the marked routes forbidden from
    /// 15/12 to 30/06 (P10), not in August; a source on every rule.
    #[test]
    #[ignore = "needs data/out/chapelle-des-bois/pack"]
    fn risoux_in_february_and_in_august() {
        let p = pack("chapelle-des-bois");
        // A point of the August C3 proof skied off the marked routes inside the APPB.
        let v: serde_json::Value = serde_json::from_str(
            &std::fs::read_to_string(
                PathBuf::from(&p).join("../routes/offpiste_risoux_aout.geojson"),
            )
            .unwrap(),
        )
        .unwrap();
        let points: Vec<(f64, f64)> = v["features"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|f| f["properties"]["network"] == "offpiste")
            .flat_map(|f| f["geometry"]["coordinates"].as_array().unwrap().clone())
            .map(|c| (c[0].as_f64().unwrap(), c[1].as_f64().unwrap()))
            .collect();
        let leave_of = |r: &PlaceRules| {
            r.topics
                .iter()
                .find(|t| t.topic == "leave_routes")
                .unwrap()
                .verdict
                .clone()
        };
        let (lon, lat) = *points
            .iter()
            .find(|(lon, lat)| {
                leave_of(&place_rules(p.clone(), *lon, *lat, 1_771_146_000_000, Lang::Fr).unwrap())
                    == "interdit"
            })
            .expect("an off-piste point of August forbidden in February");
        let feb = place_rules(p.clone(), lon, lat, 1_771_146_000_000, Lang::Fr).unwrap();
        let aug = place_rules(p, lon, lat, 1_786_870_800_000, Lang::Fr).unwrap();
        for r in [&feb, &aug] {
            eprintln!("{} {:?}\n{:#?}", r.at_text, r.zones, r.topics);
            assert!(r.covered);
            assert!(r.rules.iter().all(|x| !x.source_url.is_empty()));
            assert_eq!(r.topics[0].title, "Bivouac");
        }
        eprintln!("point {lon} {lat}");
        assert_eq!(leave_of(&feb), "interdit");
        assert_ne!(leave_of(&aug), "interdit");
        assert!(aug.rules.iter().any(|r| !r.active && r.inactive.is_some()));
    }

    #[test]
    #[ignore = "needs data/out/cervieres/pack"]
    fn cervieres_bivouac_has_a_verdict_and_a_source() {
        let r = place_rules(pack("cervieres"), 6.75, 44.85, 1_771_146_000_000, Lang::Fr).unwrap();
        eprintln!("{r:#?}");
        assert!(r.covered);
        assert!(!r.zones.is_empty());
        assert!(
            r.rules
                .iter()
                .any(|x| x.topic == "Bivouac" && !x.source_title.is_empty())
        );
    }
}
