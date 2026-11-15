//! Itineraries gathered from every source, in one model.

use serde::Serialize;
use skimap_common::geom::LonLat;

/// Provider of an itinerary.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Provider {
    /// OpenStreetMap, through the OpenSnowMap extract (ODbL).
    Osm,
    /// A Geotrek-admin instance (park, département).
    Geotrek,
    /// Camptocamp (CC BY-SA 3.0).
    Camptocamp,
    /// DATAtourisme (Licence Ouverte 2.0).
    Datatourisme,
}

impl Provider {
    /// Identifier used in the outputs.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Osm => "osm",
            Self::Geotrek => "geotrek",
            Self::Camptocamp => "camptocamp",
            Self::Datatourisme => "datatourisme",
        }
    }
}

/// A source document that says an itinerary exists, without a track: a point to
/// complete.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Hint {
    /// Identifier (`camptocamp:<id>`, `datatourisme:<uri path>`).
    pub id: String,
    /// Name.
    pub name: String,
    /// Provider.
    pub provider: Provider,
    /// Public page.
    pub url: String,
    /// Class the itinerary would have.
    pub activity: Activity,
    /// Position (start or main point), WGS84.
    #[serde(skip)]
    pub pos: LonLat,
    /// Why there is no track.
    pub reason: String,
}

/// What the itinerary is for, as published by its source.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Activity {
    /// Nordic backcountry skiing (SRN) explicitly: OSM `nordic` + `backcountry`, a Geotrek
    /// practice named nordic or SRN, a Camptocamp route that says so.
    Srn,
    /// Easy ski touring (Camptocamp ski rating 1.1 or 1.2, at most
    /// [`crate::camptocamp::EASY_MAX_ASCENT_M`] of ascent): slopes under 30°, SRN-compatible.
    EasySkiTouring,
    /// Snowshoe itinerary (OSM `hike` / `snowshoe`, Geotrek snowshoe practice): attached to
    /// an area, never seeds one.
    Snowshoe,
    /// Other ski touring (OSM `skitour`, Camptocamp 1.3 or more ascent): attached to an
    /// area, never seeds one.
    SkiTouring,
}

impl Activity {
    /// Whether the itinerary can seed an area.
    pub fn seeds(self) -> bool {
        matches!(self, Self::Srn | Self::EasySkiTouring)
    }

    /// Identifier used in the outputs.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Srn => "srn",
            Self::EasySkiTouring => "easy_ski_touring",
            Self::Snowshoe => "snowshoe",
            Self::SkiTouring => "ski_touring",
        }
    }
}

/// Shape of an itinerary.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Shape {
    /// Point to point (ends more than [`TRAVERSE_MIN_GAP_M`] apart).
    Traverse,
    /// Loop (ends within [`TRAVERSE_MIN_GAP_M`]).
    Loop,
    /// Out and back, as declared by the source.
    OutAndBack,
    /// Network of ways without a route relation (OSM).
    Network,
}

impl Shape {
    /// Identifier used in the outputs.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Traverse => "traverse",
            Self::Loop => "loop",
            Self::OutAndBack => "out_and_back",
            Self::Network => "network",
        }
    }
}

/// Ends of an itinerary farther apart than this make it a traverse, m.
pub const TRAVERSE_MIN_GAP_M: f64 = 2_000.0;

/// One itinerary.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Route {
    /// Stable identifier: `osm:relation/<id>`, `osm:way/<first id>`,
    /// `geotrek:<api host>:<id>`, `camptocamp:<id>`.
    pub id: String,
    /// Name as published.
    pub name: Option<String>,
    /// Provider.
    pub provider: Provider,
    /// Publisher (Geotrek instance name, "OpenStreetMap", "Camptocamp").
    pub publisher: String,
    /// Public page of the itinerary.
    pub url: String,
    /// Licence of the itinerary data.
    pub licence: String,
    /// Attribution to display.
    pub attribution: String,
    /// Activity.
    pub activity: Activity,
    /// Shape.
    pub shape: Shape,
    /// Polylines, WGS84.
    #[serde(skip)]
    pub lines: Vec<Vec<LonLat>>,
    /// Length of the polylines, km.
    pub length_km: f64,
    /// Length published by the source, km (Geotrek `length_2d`, Camptocamp
    /// `route_length`), to compare with [`Route::length_km`].
    pub published_length_km: Option<f64>,
    /// Ascent published by the source, m.
    pub ascent_m: Option<f64>,
    /// Difficulty as published (Geotrek difficulty label, Camptocamp ski rating).
    pub difficulty: Option<String>,
}

impl Route {
    /// Length of polylines, km (haversine).
    pub fn lines_length_km(lines: &[Vec<LonLat>]) -> f64 {
        lines
            .iter()
            .map(|l| l.windows(2).map(|w| w[0].haversine_m(w[1])).sum::<f64>())
            .sum::<f64>()
            / 1000.0
    }

    /// Shape from the ends of the polylines: loop when the first and last points of the
    /// longest chain are within [`TRAVERSE_MIN_GAP_M`].
    pub fn shape_from_ends(lines: &[Vec<LonLat>]) -> Shape {
        let first = lines.iter().find_map(|l| l.first().copied());
        let last = lines.iter().rev().find_map(|l| l.last().copied());
        match (first, last) {
            (Some(a), Some(b)) if a.haversine_m(b) > TRAVERSE_MIN_GAP_M => Shape::Traverse,
            _ => Shape::Loop,
        }
    }

    /// All vertices.
    pub fn points(&self) -> impl Iterator<Item = LonLat> + '_ {
        self.lines.iter().flatten().copied()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn length_and_shape() {
        let a = LonLat::new(6.0, 45.0);
        let b = LonLat::new(6.0, 45.1);
        let line = vec![a, b];
        let km = Route::lines_length_km(std::slice::from_ref(&line));
        assert!((km - 11.12).abs() < 0.02, "{km}");
        assert_eq!(Route::shape_from_ends(&[line]), Shape::Traverse);
        let c = LonLat::new(6.001, 45.0);
        assert_eq!(Route::shape_from_ends(&[vec![a, b, c]]), Shape::Loop);
        assert!(Activity::Srn.seeds() && Activity::EasySkiTouring.seeds());
        assert!(!Activity::Snowshoe.seeds() && !Activity::SkiTouring.seeds());
    }
}
