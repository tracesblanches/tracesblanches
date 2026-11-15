//! The `pistes` layer of the pack: the OSM ways of `pistes/pistes.geojson`, enriched way
//! by way (OSM way identifier) with
//!
//! - the **state** of the trail status layer (`status/status.geojson`, every source
//!   names the OSM ways it gives a state to: `osm_way_ids`): `state` (`open`, `closed`,
//!   `unknown`), `state_at` (date of the state as the source publishes it),
//!   `state_source`, `state_itineraries`. A way of several itineraries is `open` if one
//!   of them is (the section is groomed for it), else `closed` if one is, else
//!   `unknown`; only itineraries of the way's practice count (ski for a ski way). Without
//!   the status layer every way is `unknown`;
//! - the attributes transcribed from the official piste plan (`plan_pistes/`, P22):
//!   colour, direction, name, each with its source (`colour_source`, `oneway_source`,
//!   `name_source`: `osm` or `plan`). A plan colour fills a way OSM leaves without
//!   difficulty; an OSM colour is kept (the plan transcribes only a part of the pistes,
//!   P22: its colours on a way may miss a piste), a plan colour that OSM lacks is kept
//!   aside in `plan_colours`; a section shared by pistes of several colours keeps the
//!   rule of the pistes layer
//!   (`difficulty` the easiest, `difficulty_max` the hardest: a loop is skiable in full
//!   at its colour), so it is drawn once, in one colour. A direction read on the plan
//!   arrows makes a way one-way when OSM says nothing (the geometry is reversed for
//!   `backward`, as the pistes layer does for `oneway=-1`); an OSM direction is kept.
//!
//! Both sources are personal use only: they are joined only with `personal_sources`, and
//! their sources are then added to the layer (marked personal use only).

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::Path;

use serde::Serialize;
use serde_json::{Value, json};

use crate::build::layers::{read_meta, unauthorised};
use crate::error::{Error, Result};
use crate::manifest::Source;

/// One itinerary of the status layer, as seen from a way.
#[derive(Debug, Clone)]
struct StateRef {
    state: String,
    at: Option<String>,
    source: String,
    itinerary: String,
    practice: String,
}

/// Attributes of the plan for a way.
#[derive(Debug, Clone, Default)]
struct PlanRef {
    colours: BTreeSet<u8>,
    names: Vec<String>,
    direction: Option<String>,
}

/// What the enrichment did, for the layer notes and the checks.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct PistesReport {
    /// Ways of the layer.
    pub ways: u64,
    /// Ways per state.
    pub states: BTreeMap<String, u64>,
    /// Ways whose colour comes from the plan (OSM without difficulty).
    pub colour_from_plan: u64,
    /// Ways with a plan colour that OSM does not list (OSM kept, `plan_colours`).
    pub colour_plan_differs: u64,
    /// Ways whose plan colours are among the OSM colours.
    pub colour_plan_agrees: u64,
    /// Ways made one-way by the plan arrows.
    pub oneway_from_plan: u64,
    /// Ways whose OSM direction contradicts the plan (OSM kept).
    pub oneway_plan_conflicts: u64,
    /// Ways named from the plan.
    pub name_from_plan: u64,
    /// Sources added to the layer.
    #[serde(skip)]
    pub sources: Vec<Source>,
}

/// Colour rank of the pistes layer: novice (green) 0 … advanced (black) 3.
fn rank_of_colour(c: &str) -> Option<u8> {
    match c {
        "green" => Some(0),
        "blue" => Some(1),
        "red" => Some(2),
        "black" => Some(3),
        _ => None,
    }
}

const DIFFICULTY: [&str; 4] = ["novice", "easy", "intermediate", "advanced"];
const COLOUR: [&str; 4] = ["green", "blue", "red", "black"];

fn read_fc(path: &Path) -> Result<Option<Value>> {
    if !path.is_file() {
        return Ok(None);
    }
    let text = std::fs::read_to_string(path).map_err(|e| Error::io(path, e))?;
    Ok(Some(
        serde_json::from_str(&text).map_err(|e| Error::json(path.display().to_string(), e))?,
    ))
}

fn features(v: &Value) -> impl Iterator<Item = &Value> {
    v["features"].as_array().into_iter().flatten()
}

fn state_index(status: &Value) -> HashMap<i64, Vec<StateRef>> {
    let mut m: HashMap<i64, Vec<StateRef>> = HashMap::new();
    for f in features(status) {
        let p = &f["properties"];
        let r = StateRef {
            state: p["state"].as_str().unwrap_or("unknown").to_owned(),
            at: p["state_at"].as_str().map(str::to_owned),
            source: p["source"].as_str().unwrap_or("").to_owned(),
            itinerary: p["itinerary"].as_str().unwrap_or("").to_owned(),
            practice: p["osm_practice"].as_str().unwrap_or("any").to_owned(),
        };
        for id in p["osm_way_ids"].as_array().into_iter().flatten() {
            if let Some(id) = id.as_i64() {
                m.entry(id).or_default().push(r.clone());
            }
        }
    }
    m
}

fn plan_index(plan_pistes: &Value, plan_ways: Option<&Value>) -> HashMap<i64, PlanRef> {
    let mut m: HashMap<i64, PlanRef> = HashMap::new();
    for f in features(plan_pistes) {
        let p = &f["properties"];
        let colour = p["colour"].as_str().and_then(rank_of_colour);
        let name = p["name"].as_str().map(str::to_owned);
        for id in p["osm_way_ids"].as_array().into_iter().flatten() {
            if let Some(id) = id.as_i64() {
                let e = m.entry(id).or_default();
                e.colours.extend(colour);
                if let Some(n) = &name
                    && !e.names.contains(n)
                {
                    e.names.push(n.clone());
                }
            }
        }
    }
    for f in plan_ways.into_iter().flat_map(features) {
        let p = &f["properties"];
        if let (Some(id), Some(d)) = (p["osm_way_id"].as_i64(), p["direction"].as_str()) {
            m.entry(id).or_default().direction = Some(d.to_owned());
        }
    }
    m
}

/// Whether an itinerary of `practice` (`nordic`, `foot`, `any`) states the way.
fn counts_for(practice: &str, nordic: bool) -> bool {
    match practice {
        "nordic" => nordic,
        "foot" => !nordic,
        _ => true,
    }
}

fn reverse_line(g: &mut Value) {
    let kind = g["type"].as_str().map(str::to_owned);
    if let Some(c) = g.get_mut("coordinates").and_then(Value::as_array_mut) {
        match kind.as_deref() {
            Some("LineString") => c.reverse(),
            Some("MultiLineString") => {
                c.reverse();
                for l in c.iter_mut().filter_map(Value::as_array_mut) {
                    l.reverse();
                }
            }
            _ => {}
        }
    }
}

/// Enriches the pistes FeatureCollection `v` from the pipeline outputs `out`; joins the
/// personal-use sources only when `personal_sources` is set.
pub fn enrich(v: &mut Value, out: &Path, personal_sources: bool) -> Result<PistesReport> {
    let mut report = PistesReport::default();
    let mut states = HashMap::new();
    let mut plan = HashMap::new();
    if personal_sources {
        if let Some(st) = read_fc(&out.join("status").join("status.geojson"))? {
            states = state_index(&st);
            if let Some(m) = read_meta(&out.join("status").join("status.meta.json"))? {
                report.sources.extend(m.meta.sources);
            }
        }
        let dir = out.join("plan_pistes");
        if let Some(pp) = read_fc(&dir.join("plan_pistes.geojson"))? {
            let pw = read_fc(&dir.join("plan_ways.geojson"))?;
            plan = plan_index(&pp, pw.as_ref());
            if let Some(m) = read_meta(&dir.join("plan_pistes.meta.json"))? {
                report.sources.extend(m.meta.sources);
            }
        }
    }
    let Some(fs) = v.get_mut("features").and_then(Value::as_array_mut) else {
        return Err(Error::Pack(
            "pistes.geojson is not a FeatureCollection".into(),
        ));
    };
    for f in fs.iter_mut() {
        report.ways += 1;
        let id = f["properties"]["osm_way_id"].as_i64();
        let nordic = f["properties"]["uses"]
            .as_array()
            .is_some_and(|u| u.iter().any(|x| x == "nordic"));
        // State.
        let refs: Vec<&StateRef> = id
            .and_then(|i| states.get(&i))
            .into_iter()
            .flatten()
            .filter(|r| counts_for(&r.practice, nordic))
            .collect();
        let state = if refs.iter().any(|r| r.state == "open") {
            "open"
        } else if refs.iter().any(|r| r.state == "closed") {
            "closed"
        } else {
            "unknown"
        };
        let deciding: Vec<&&StateRef> = refs.iter().filter(|r| r.state == state).collect();
        let at = deciding.iter().filter_map(|r| r.at.clone()).max();
        let sources: BTreeSet<&str> = deciding.iter().map(|r| r.source.as_str()).collect();
        *report.states.entry(state.to_owned()).or_default() += 1;
        // Plan.
        let pr = id.and_then(|i| plan.get(&i)).cloned().unwrap_or_default();
        let mut reverse = false;
        let props = f
            .get_mut("properties")
            .and_then(Value::as_object_mut)
            .ok_or_else(|| Error::Pack("piste without properties".into()))?;
        props.insert("state".into(), json!(state));
        props.insert("state_at".into(), json!(at));
        props.insert(
            "state_source".into(),
            if sources.is_empty() {
                Value::Null
            } else {
                json!(sources.into_iter().collect::<Vec<_>>().join(","))
            },
        );
        props.insert(
            "state_itineraries".into(),
            json!(
                deciding
                    .iter()
                    .map(|r| r.itinerary.clone())
                    .collect::<Vec<_>>()
            ),
        );
        let osm_difficulty = props.get("difficulty").and_then(Value::as_str);
        let osm_colours: Vec<String> = props
            .get("colours")
            .and_then(Value::as_array)
            .map(|a| {
                a.iter()
                    .filter_map(|c| c.as_str().map(str::to_owned))
                    .collect()
            })
            .unwrap_or_default();
        let colour_source = match (pr.colours.first(), pr.colours.last(), osm_difficulty) {
            // OSM without colour: the plan's (easiest and hardest of its pistes).
            (Some(&easy), Some(&hard), None) => {
                report.colour_from_plan += 1;
                props.insert("difficulty".into(), json!(DIFFICULTY[usize::from(easy)]));
                props.insert(
                    "difficulty_max".into(),
                    json!(DIFFICULTY[usize::from(hard)]),
                );
                props.insert(
                    "colours".into(),
                    json!(
                        pr.colours
                            .iter()
                            .map(|c| COLOUR[usize::from(*c)])
                            .collect::<Vec<_>>()
                    ),
                );
                "plan"
            }
            // Both: OSM kept (the plan transcribes a part of the pistes only, so its
            // colours on a way can miss a piste of OSM); a plan colour OSM lacks is
            // counted and kept aside.
            (Some(_), Some(_), Some(_)) => {
                let plan: Vec<&str> = pr.colours.iter().map(|c| COLOUR[usize::from(*c)]).collect();
                if plan.iter().all(|c| osm_colours.iter().any(|o| o == c)) {
                    report.colour_plan_agrees += 1;
                } else {
                    report.colour_plan_differs += 1;
                    props.insert("plan_colours".into(), json!(plan));
                }
                "osm"
            }
            (_, _, Some(_)) => "osm",
            _ => "",
        };
        props.insert(
            "colour_source".into(),
            if colour_source.is_empty() {
                Value::Null
            } else {
                json!(colour_source)
            },
        );
        let osm_oneway = props.get("oneway").and_then(Value::as_bool) == Some(true);
        let oneway_source = match pr.direction.as_deref() {
            Some(d @ ("forward" | "backward")) if !osm_oneway => {
                report.oneway_from_plan += 1;
                props.insert("oneway".into(), json!(true));
                reverse = d == "backward";
                "plan"
            }
            Some("backward") if osm_oneway => {
                report.oneway_plan_conflicts += 1;
                "osm"
            }
            _ if osm_oneway => "osm",
            _ => "",
        };
        props.insert(
            "oneway_source".into(),
            if oneway_source.is_empty() {
                Value::Null
            } else {
                json!(oneway_source)
            },
        );
        if reverse {
            props.insert("geometry_reversed".into(), json!(true));
        }
        let has_name = props.get("name").and_then(Value::as_str).is_some();
        let name_source = if has_name {
            Value::from("osm")
        } else if let Some(n) = pr.names.first() {
            report.name_from_plan += 1;
            props.insert("name".into(), json!(n));
            Value::from("plan")
        } else {
            Value::Null
        };
        props.insert("name_source".into(), name_source);
        if !pr.names.is_empty() {
            props.insert("plan_pistes".into(), json!(pr.names));
        }
        if reverse && let Some(g) = f.get_mut("geometry") {
            reverse_line(g);
        }
    }
    for s in &mut report.sources {
        // Personal use only even if an older meta file does not say so.
        s.personal_use_only |= unauthorised(s);
    }
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn piste(
        id: i64,
        difficulty: Option<&str>,
        uses: &[&str],
        oneway: bool,
        name: Option<&str>,
    ) -> Value {
        json!({"type": "Feature", "geometry": {"type": "LineString", "coordinates": [[6.0, 46.0], [6.1, 46.1]]},
               "properties": {"osm_way_id": id, "difficulty": difficulty, "difficulty_max": difficulty,
                              "uses": uses, "oneway": oneway, "name": name}})
    }

    #[test]
    fn states_and_plan_join_by_way() {
        let dir = tempfile::tempdir().unwrap();
        let out = dir.path();
        std::fs::create_dir_all(out.join("status")).unwrap();
        std::fs::create_dir_all(out.join("plan_pistes")).unwrap();
        let status = json!({"type": "FeatureCollection", "features": [
            {"type": "Feature", "geometry": null, "properties": {"state": "closed", "state_at": "2026-04-06", "source": "nordicfrance",
              "itinerary": "nordicfrance:a", "osm_practice": "nordic", "osm_way_ids": [1, 2]}},
            {"type": "Feature", "geometry": null, "properties": {"state": "open", "state_at": "2026-11-14T15:15:07+01:00", "source": "enj",
              "itinerary": "enj:7", "osm_practice": "nordic", "osm_way_ids": [2]}},
            {"type": "Feature", "geometry": null, "properties": {"state": "open", "source": "nordicfrance",
              "itinerary": "nordicfrance:foot", "osm_practice": "foot", "osm_way_ids": [1, 4]}}
        ]});
        std::fs::write(out.join("status/status.geojson"), status.to_string()).unwrap();
        let plan = json!({"type": "FeatureCollection", "features": [
            {"type": "Feature", "geometry": null, "properties": {"colour": "black", "name": "Greffier", "osm_way_ids": [2, 3]}},
            {"type": "Feature", "geometry": null, "properties": {"colour": "green", "name": "Verte", "osm_way_ids": [3]}}
        ]});
        std::fs::write(
            out.join("plan_pistes/plan_pistes.geojson"),
            plan.to_string(),
        )
        .unwrap();
        let ways = json!({"type": "FeatureCollection", "features": [
            {"type": "Feature", "geometry": null, "properties": {"osm_way_id": 3, "direction": "backward"}}
        ]});
        std::fs::write(out.join("plan_pistes/plan_ways.geojson"), ways.to_string()).unwrap();
        let mut v = json!({"type": "FeatureCollection", "features": [
            piste(1, Some("easy"), &["nordic"], false, Some("A")),
            piste(2, Some("advanced"), &["nordic"], false, Some("B")),
            piste(3, None, &["nordic"], false, None),
            piste(4, None, &["hike"], false, None),
            piste(5, Some("easy"), &["nordic"], true, Some("E")),
        ]});
        // Without personal sources: every state unknown, nothing joined.
        let mut public = v.clone();
        let r = enrich(&mut public, out, false).unwrap();
        assert_eq!(r.states.get("unknown"), Some(&5));
        assert!(r.sources.is_empty());
        assert_eq!(
            public["features"][2]["properties"]["difficulty"],
            Value::Null
        );

        let r = enrich(&mut v, out, true).unwrap();
        let p = |i: usize| v["features"][i]["properties"].clone();
        // Way 1: ski itinerary closed; the foot itinerary does not count for a ski way.
        assert_eq!(p(0)["state"], "closed");
        assert_eq!(p(0)["state_at"], "2026-04-06");
        assert_eq!(p(0)["state_source"], "nordicfrance");
        // Way 2: one itinerary open, one closed: open, date and source of the open one.
        assert_eq!(p(1)["state"], "open");
        assert_eq!(p(1)["state_source"], "enj");
        assert_eq!(p(1)["state_itineraries"], json!(["enj:7"]));
        assert_eq!(p(1)["colour_source"], "osm");
        // Way 3: no OSM colour, shared by a black and a green plan piste: green (the
        // easiest), black kept as the hardest; name from the plan; reversed one-way.
        assert_eq!(p(2)["difficulty"], "novice");
        assert_eq!(p(2)["difficulty_max"], "advanced");
        assert_eq!(p(2)["colour_source"], "plan");
        assert_eq!(p(2)["name"], "Greffier");
        assert_eq!(p(2)["name_source"], "plan");
        assert_eq!(p(2)["oneway"], true);
        assert_eq!(p(2)["oneway_source"], "plan");
        assert_eq!(
            v["features"][2]["geometry"]["coordinates"][0],
            json!([6.1, 46.1])
        );
        assert_eq!(p(2)["state"], "unknown");
        // Way 4: walking way, foot itinerary open.
        assert_eq!(p(3)["state"], "open");
        // Way 5: OSM one-way.
        assert_eq!(p(4)["oneway_source"], "osm");
        assert_eq!(r.colour_from_plan, 1);
        assert_eq!(r.oneway_from_plan, 1);
        assert_eq!(r.name_from_plan, 1);
        assert_eq!(r.states.get("open"), Some(&2));
    }
}
