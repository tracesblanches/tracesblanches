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
use skimap_zones::layer::{ZoneKind, ZoneLayer};
use skimap_zones::local::{self, LocalStatus, OwnAct};
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
    /// Local rules not verified at the place (G4): « Règles locales non vérifiées : APPB
    /// X (arrêté de protection de biotope) : texte non lu ; arrêtés municipaux et
    /// préfectoraux : non recensés. »; empty outside every zone with an act of its own and
    /// every territory.
    pub local_rules: String,
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

/// Items of the local rules not verified at the place: the zones containing it whose own
/// act was not read or read in part ([`skimap_zones::local`]), even on the edge of the
/// territories of the layer (a sliver of a reserve along a border), then the municipal
/// decrees, never listed (French communes and départements are summed up by that line).
fn local_items(
    tx: &dyn text::Texts,
    layer: &ZoneLayer,
    rules: &RuleSet,
    ev: &zone_eval::Evaluation,
) -> Vec<String> {
    let mut items = Vec::new();
    let (mut fr, mut ch) = (false, false);
    for z in ev.zones.iter().filter_map(|h| layer.get(&h.id)) {
        match z.kind() {
            ZoneKind::Country => fr = true,
            ZoneKind::Canton => ch = true,
            _ => {}
        }
        let Some((act, status)) = local::local_status(rules, z) else {
            continue;
        };
        if matches!(act, OwnAct::ArretesMunicipaux | OwnAct::ArretesPrefectoraux) {
            continue;
        }
        let a = tx.local_act(act);
        match status {
            LocalStatus::Unverified => items.push(tx.local_unverified(&z.props.name, a)),
            LocalStatus::Partial => items.push(tx.local_partial(&z.props.name, a)),
            LocalStatus::Read => {}
        }
    }
    if fr {
        items.push(tx.local_municipal_fr().to_owned());
    }
    if ch {
        items.push(tx.local_municipal_ch().to_owned());
    }
    items
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
    let local_rules = tx.local_rules(&local_items(tx, layer, rules, &ev));
    let local = ev.local_time;
    Ok(PlaceRules {
        at_text: tx.rules_at(local),
        covered: ev.covered,
        topics,
        zones,
        rules: lines,
        local_rules,
        note: tx.rules_note().into(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn zone(
        id: &str,
        kind: ZoneKind,
        name: &str,
        b: skimap_common::geom::BBox,
    ) -> skimap_zones::Zone {
        skimap_zones::Zone::new(
            skimap_zones::ZoneProps {
                id: id.into(),
                kind,
                name: name.into(),
                source: "test".into(),
                url: None,
                months: None,
                practices: vec![],
                designated: None,
                area_ha: 1.0,
                clipped: false,
                note: None,
            },
            skimap_zones::geo::MultiPolygon::new(vec![skimap_zones::geom::bbox_polygon(b, 1)]),
        )
    }

    /// An APPB whose act was not read, a département and a cœur de parc: the line says
    /// which texts were not read, in both languages; nothing outside the territories.
    #[test]
    fn local_rules_not_verified_at_a_place() {
        use skimap_common::geom::BBox;
        let rules = RuleSet::embedded().unwrap();
        let big = BBox::new(44.0, 6.0, 46.0, 8.0);
        let small = BBox::new(45.0, 7.0, 45.1, 7.1);
        let layer = ZoneLayer {
            zones: vec![
                zone("country:fr", ZoneKind::Country, "France", big),
                zone("dept:73", ZoneKind::Department, "Savoie", big),
                zone("inpn:FR3800000", ZoneKind::Appb, "Tourbière", small),
                zone("inpn:FR3300001", ZoneKind::ParcNational, "Vanoise", small),
                zone("n2000", ZoneKind::Natura2000Sic, "Natura", small),
            ],
        };
        let at = DateTime::from_timestamp_millis(1_771_146_000_000).unwrap();
        let ev = zone_eval::evaluate(&layer, &rules, LonLat::new(7.05, 45.05), at);
        let fr = text::of(Lang::Fr);
        let line = fr.local_rules(&local_items(fr, &layer, &rules, &ev));
        assert_eq!(
            line,
            "Règles locales non vérifiées : Tourbière (arrêté de protection de biotope) : texte non lu ; \
             Vanoise (réglementation du cœur de parc national) : texte non lu ; \
             arrêtés municipaux et préfectoraux : non recensés."
        );
        let en = text::of(Lang::En);
        let line = en.local_rules(&local_items(en, &layer, &rules, &ev));
        assert!(
            line.starts_with(
                "Local rules not verified: Tourbière (biotope protection order): text not read; "
            ),
            "{line}"
        );
        // Outside every zone: nothing.
        let out = zone_eval::evaluate(&layer, &rules, LonLat::new(9.0, 47.0), at);
        assert!(local_items(fr, &layer, &rules, &out).is_empty());
        assert_eq!(fr.local_rules(&[]), "");
    }

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

    /// Every pack of the data directory: at an inner point of each zone whose own act was
    /// not read, the place says « non vérifiées » and names the zone; every covered place
    /// has the line.
    #[test]
    #[ignore = "needs data/out/<site>/pack"]
    fn every_pack_names_the_unread_acts() {
        let root = std::env::var("SKIMAP_DATA_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|_| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../data"))
            .join("out");
        let (mut packs, mut checked, mut uncovered) = (0usize, 0usize, 0usize);
        let t0 = std::time::Instant::now();
        for e in std::fs::read_dir(&root).unwrap() {
            let dir = e.unwrap().path().join("pack");
            if !dir.join("manifest.json").exists() {
                continue;
            }
            let d = dir.display().to_string();
            let Ok(l) = load(&d) else { continue };
            packs += 1;
            let (layer, rules) = (&l.0, &l.1);
            for z in &layer.zones {
                let Some((act, LocalStatus::Unverified)) = local::local_status(rules, z) else {
                    continue;
                };
                if act.is_territorial() {
                    continue;
                }
                let Some(p) = skimap_zones::fields::inner_point(z) else {
                    continue;
                };
                let r = place_rules(d.clone(), p.lon, p.lat, 1_771_146_000_000, Lang::Fr).unwrap();
                if !r.covered {
                    // On the edge of the territories (sliver along a border).
                    uncovered += 1;
                }
                assert!(
                    r.local_rules.starts_with("Règles locales non vérifiées")
                        && r.local_rules.contains(&z.props.name),
                    "{d} {}: {}",
                    z.id(),
                    r.local_rules
                );
                checked += 1;
            }
        }
        eprintln!(
            "{packs} packs, {checked} zones à acte non lu vérifiées ({uncovered} hors des territoires) en {:.2} s",
            t0.elapsed().as_secs_f64()
        );
        assert!(packs > 0);
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
