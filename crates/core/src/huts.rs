//! Refuges of the day plan (X21, #155): which features of the refuges.info and OSM layers
//! the plan uses (refuges gardés, cabanes non gardées and gîtes of refuges.info; alpine huts,
//! wilderness huts and shelters of OSM), the same hut found in both sources kept once (the
//! refuges.info one).
//!
//! The choice is a flag on each feature ([`PLAN_FLAG`]), written by the pack builder into
//! both layers of the pack ([`mark`]): the phone reads it as it is ([`marked`]) instead of
//! comparing every OSM hut with every refuges.info one when a pack opens. The two layers
//! stay two layers (licences, L3): the flag only says « used by the plan ».

use serde_json::Value;
use skimap_common::geom::LonLat;

/// Property of every feature of both layers: used by the plan (`true`) or not.
pub const PLAN_FLAG: &str = "plan_refuge";
/// refuges.info types (`type_label`) the plan uses.
pub const INFO_KINDS: &[&str] = &["cabane non gardée", "refuge gardé", "gîte d'étape"];
/// OSM kinds (`kind`) the plan uses.
pub const OSM_KINDS: &[&str] = &["alpine_hut", "wilderness_hut", "shelter"];
/// Two huts closer than this are the same place, metres.
pub const SAME_PLACE_M: f64 = 50.0;

/// Point of a GeoJSON feature.
pub fn point(f: &Value) -> Option<LonLat> {
    let c = f["geometry"]["coordinates"].as_array()?;
    Some(LonLat::new(c.first()?.as_f64()?, c.get(1)?.as_f64()?))
}

fn features(fc: &Value) -> &[Value] {
    fc["features"].as_array().map_or(&[], Vec::as_slice)
}

/// Whether every feature of a layer carries [`PLAN_FLAG`] (a layer of a pack built with it).
pub fn marked(fc: &Value) -> bool {
    features(fc)
        .iter()
        .all(|f| f["properties"][PLAN_FLAG].is_boolean())
}

/// Whether a feature carries [`PLAN_FLAG`] set to `true`.
pub fn flagged(f: &Value) -> bool {
    f["properties"][PLAN_FLAG].as_bool() == Some(true)
}

fn set(f: &mut Value, v: bool) {
    if !f["properties"].is_object() {
        f["properties"] = Value::Object(serde_json::Map::new());
    }
    f["properties"][PLAN_FLAG] = Value::Bool(v);
}

/// Sets [`PLAN_FLAG`] on every feature of the refuges.info layer `info` and of the OSM
/// layer `osm` (either may be `Value::Null`, a missing layer): a refuges.info feature of
/// [`INFO_KINDS`] with a point; an OSM feature of [`OSM_KINDS`] with a point and no hut
/// already kept (refuges.info first, then OSM in order) within [`SAME_PLACE_M`]. Returns
/// the number of features flagged in each.
pub fn mark(info: &mut Value, osm: &mut Value) -> (usize, usize) {
    let mut kept: Vec<LonLat> = Vec::new();
    let mut n_info = 0;
    if let Some(fs) = info["features"].as_array_mut() {
        for f in fs {
            let kind = f["properties"]["type_label"].as_str().unwrap_or_default();
            let p = point(f).filter(|_| INFO_KINDS.contains(&kind));
            if let Some(p) = p {
                kept.push(p);
                n_info += 1;
            }
            set(f, p.is_some());
        }
    }
    let mut n_osm = 0;
    if let Some(fs) = osm["features"].as_array_mut() {
        for f in fs {
            let kind = f["properties"]["kind"].as_str().unwrap_or_default();
            let p = point(f)
                .filter(|_| OSM_KINDS.contains(&kind))
                .filter(|p| !kept.iter().any(|k| k.haversine_m(*p) < SAME_PLACE_M));
            if let Some(p) = p {
                kept.push(p);
                n_osm += 1;
            }
            set(f, p.is_some());
        }
    }
    (n_info, n_osm)
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn feat(lon: f64, props: Value) -> Value {
        json!({"type": "Feature", "geometry": {"type": "Point", "coordinates": [lon, 45.0]},
               "properties": props})
    }

    #[test]
    fn same_hut_kept_once_and_other_kinds_left_out() {
        // 0.0003° of longitude at 45° N ≈ 24 m; 0.001° ≈ 79 m.
        let mut info = json!({"features": [
            feat(6.0, json!({"type_label": "refuge gardé"})),
            feat(6.1, json!({"type_label": "point d'eau"})),
        ]});
        let mut osm = json!({"features": [
            feat(6.0003, json!({"kind": "alpine_hut"})),
            feat(6.001, json!({"kind": "shelter"})),
            feat(6.0012, json!({"kind": "wilderness_hut"})),
            feat(6.2, json!({"kind": "viewpoint"})),
            json!({"type": "Feature", "geometry": null, "properties": {"kind": "shelter"}}),
        ]});
        assert!(!marked(&info) && !marked(&osm));
        assert_eq!(mark(&mut info, &mut osm), (1, 1));
        assert!(marked(&info) && marked(&osm));
        let flags = |fc: &Value| features(fc).iter().map(flagged).collect::<Vec<_>>();
        assert_eq!(flags(&info), [true, false]);
        // Near the refuges.info hut; kept; near the OSM shelter just kept; other kind; no point.
        assert_eq!(flags(&osm), [false, true, false, false, false]);
    }

    #[test]
    fn missing_layers_mark_nothing() {
        let mut osm = json!({"features": [feat(6.0, json!({"kind": "shelter"}))]});
        assert_eq!(mark(&mut Value::Null, &mut osm), (0, 1));
        assert!(marked(&Value::Null));
    }
}
