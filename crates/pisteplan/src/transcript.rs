//! Format of the transcription (phone side, no pipeline dependency): the pistes of the
//! official plan carried onto our own geometries, as GeoJSON WGS84 with typed properties.
//!
//! Files of `data/out/<site>/plan_pistes/`:
//! - `plan_pistes.geojson`: one `MultiLineString` per piste of the plan ([`PlanPiste`]);
//! - `plan_landmarks.geojson`: one `Point` per landmark read on the plan ([`PlanLandmark`]).

use serde::{Deserialize, Serialize};

/// What a line of the plan is, from the plan's legend.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PisteKind {
    /// Groomed cross-country ski piste.
    Ski,
    /// Link between two areas or between a village and the pistes.
    Liaison,
    /// Shared track (ski, walkers, snowshoes, fat bike).
    MultiActivity,
    /// Walkers' (and snowshoe) itinerary.
    Pedestrian,
    /// Snowshoe itinerary.
    Snowshoe,
}

/// Where the geometry of a transcribed piste comes from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GeometrySource {
    /// OSM ways (our pistes layer), matched to the plan's line.
    Osm,
    /// Line of the plan carried onto the map by the plan ↔ map transform: estimated trace,
    /// the piste is missing from OSM.
    EstimatedFromPlan,
}

/// Direction of travel shown by the arrows of the plan.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Direction {
    /// One-way, in the order of the geometry.
    Forward,
    /// One-way, against the order of the geometry.
    Backward,
    /// Arrows both ways or no arrow on a plan that draws arrows.
    Both,
}

/// Comparison of a piste of the plan with OSM.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OsmStatus {
    /// Same name and same colour in OSM.
    Match,
    /// Found in OSM with another colour (correction to propose).
    ColourDiffers,
    /// Found in OSM without colour or difficulty (addition of an attribute).
    ColourMissing,
    /// Drawn on the plan over OSM ways that carry no piste of this name (name to add).
    NameMissing,
    /// Absent from OSM (estimated trace, input of G1).
    Missing,
}

/// How a piste of the plan was matched.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MatchMethod {
    /// Certain labels and topology: same name, line of the plan's ink over the OSM
    /// geometry after registration (score = drawn share, above a validated threshold).
    Exact,
    /// Learnt model (score = model probability).
    Ml,
}

/// Properties of a feature of `plan_pistes.geojson`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PlanPiste {
    /// Name of the piste (as printed on the plan; for a plan read by OCR, the name of the
    /// matched OSM relation, the OCR reading being in `name_read`).
    pub name: String,
    /// Name as read on the plan (text layer or OCR).
    pub name_read: String,
    /// Number on the plan, if the plan numbers its itineraries.
    #[serde(default)]
    pub number: Option<String>,
    /// Difficulty colour of the plan (`green`, `blue`, `red`, `black`), if the ink is one.
    #[serde(default)]
    pub colour: Option<String>,
    /// Ink of the line on the plan.
    pub ink: String,
    /// Kind of line (legend).
    pub kind: PisteKind,
    /// Lengths printed on the plan, km (one per departure listing the piste).
    #[serde(default)]
    pub lengths_km_plan: Vec<f64>,
    /// Departures (boxes or sectors of the plan) listing the piste.
    #[serde(default)]
    pub departures: Vec<String>,
    /// Length of the geometry, m.
    pub length_m: f64,
    /// Whether the geometry is a closed loop.
    #[serde(rename = "loop")]
    pub is_loop: bool,
    /// Direction shown by the plan's arrows, when the plan draws arrows on this piste.
    #[serde(default)]
    pub direction: Option<Direction>,
    /// Matching method.
    pub method: MatchMethod,
    /// Matching score (0 to 1).
    pub score: f64,
    /// Origin of the geometry.
    pub geometry_source: GeometrySource,
    /// Comparison with OSM.
    pub osm_status: OsmStatus,
    /// Name in OSM of the matched relation or ways, when it differs or is the only one.
    #[serde(default)]
    pub osm_name: Option<String>,
    /// Colour in OSM.
    #[serde(default)]
    pub osm_colour: Option<String>,
    /// OSM piste relations matched.
    #[serde(default)]
    pub osm_relation_ids: Vec<i64>,
    /// OSM ways of the geometry.
    #[serde(default)]
    pub osm_way_ids: Vec<i64>,
    /// Plan identifier.
    pub plan: String,
}

/// Direction compared with the OSM `piste:oneway` tag of the way.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OnewayStatus {
    /// Same one-way direction in OSM.
    Match,
    /// One-way on the plan, not tagged one-way in OSM (tag to add).
    MissingInOsm,
    /// One-way in OSM in the other direction (correction to check on the ground).
    Opposite,
    /// One-way in OSM, arrows both ways on the plan.
    PlanBothWays,
    /// Arrows both ways on the plan, not one-way in OSM.
    BothWays,
}

/// Properties of a feature of `plan_ways.geojson`: an OSM way of a transcribed piste
/// with arrows on the plan (geometry: the way, in its OSM order).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PlanWay {
    /// OSM way id.
    pub osm_way_id: i64,
    /// Transcribed pistes using the way.
    pub pistes: Vec<String>,
    /// Direction of travel shown by the arrows, relative to the way's order.
    pub direction: Direction,
    /// Arrows pointing along the way's order.
    pub arrows_forward: usize,
    /// Arrows pointing against it.
    pub arrows_backward: usize,
    /// OSM `piste:oneway` of the way (`forward`, `backward`, `no`, `unknown`).
    pub osm_oneway: String,
    /// Comparison with OSM.
    pub oneway_status: OnewayStatus,
    /// Plan identifier.
    pub plan: String,
}

/// Kind of a landmark read on the plan.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LandmarkKind {
    /// Village or hamlet.
    Place,
    /// Summit.
    Summit,
    /// Pass.
    Pass,
    /// Hut, refuge, shelter.
    Refuge,
    /// Departure of the pistes (box of the plan).
    Departure,
    /// Lake.
    Lake,
    /// Other named feature.
    Other,
}

/// Properties of a feature of `plan_landmarks.geojson`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PlanLandmark {
    /// Name on the plan.
    pub name: String,
    /// Kind.
    pub kind: LandmarkKind,
    /// Elevation printed on the plan, m.
    #[serde(default)]
    pub ele_plan: Option<f64>,
    /// OSM element the label was matched to (`node/123`, `way/45`).
    pub osm: String,
    /// OSM name.
    pub osm_name: String,
    /// Distance between the label's predicted position and the OSM feature, m (labels are
    /// written beside their feature: a few hundred metres is normal).
    pub residual_m: f64,
    /// Plan identifier.
    pub plan: String,
}

/// A GeoJSON feature with typed properties and raw geometry.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Feature<P> {
    /// Always `Feature`.
    #[serde(rename = "type")]
    pub kind: String,
    /// GeoJSON geometry (WGS84).
    pub geometry: serde_json::Value,
    /// Properties.
    pub properties: P,
}

#[derive(Deserialize)]
struct Collection<P> {
    features: Vec<Feature<P>>,
}

/// Reads `plan_pistes.geojson`.
pub fn read_pistes(geojson: &str) -> Result<Vec<Feature<PlanPiste>>, serde_json::Error> {
    serde_json::from_str::<Collection<PlanPiste>>(geojson).map(|c| c.features)
}

/// Reads `plan_landmarks.geojson`.
pub fn read_landmarks(geojson: &str) -> Result<Vec<Feature<PlanLandmark>>, serde_json::Error> {
    serde_json::from_str::<Collection<PlanLandmark>>(geojson).map(|c| c.features)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_a_transcribed_piste() {
        let s = r#"{"type":"FeatureCollection","features":[{"type":"Feature",
          "geometry":{"type":"MultiLineString","coordinates":[[[6.1,46.6],[6.11,46.61]]]},
          "properties":{"name":"Chalet Pin","name_read":"Chalet Pin","colour":"red","ink":"red",
          "kind":"ski","lengths_km_plan":[11.0],"departures":["Chapelle-des-Bois"],
          "length_m":10950.0,"loop":true,"direction":"forward","method":"exact","score":0.93,
          "geometry_source":"osm","osm_status":"match","osm_relation_ids":[1],
          "osm_way_ids":[2,3],"plan":"p"}}]}"#;
        let f = read_pistes(s).unwrap();
        assert_eq!(f.len(), 1);
        let p = &f[0].properties;
        assert_eq!(p.name, "Chalet Pin");
        assert!(p.is_loop);
        assert_eq!(p.direction, Some(Direction::Forward));
        assert_eq!(p.kind, PisteKind::Ski);
        assert_eq!(p.osm_status, OsmStatus::Match);
        assert_eq!(p.method, MatchMethod::Exact);
        assert_eq!(p.departures, vec!["Chapelle-des-Bois".to_owned()]);
        assert_eq!(p.number, None);
        // Round trip.
        let back: PlanPiste = serde_json::from_value(serde_json::to_value(p).unwrap()).unwrap();
        assert_eq!(&back, p);
    }
}
