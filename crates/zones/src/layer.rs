//! The zone layer (P9, #22): one GeoJSON layer per site holding every protected area,
//! easement, sensitive area and jurisdiction, with its sensitive months.
//!
//! Shared by the pipeline (writes `zones.geojson`) and the phone core (reads it, C8).
//! Coordinates are WGS84 (lon, lat) degrees.
//!
//! Feature properties (all features):
//!
//! | Property | Type | Meaning |
//! |---|---|---|
//! | `id` | string | stable identifier, `<namespace>:<source id>` (see [`Zone::id`]) |
//! | `kind` | string | [`ZoneKind`] in snake_case |
//! | `name` | string | official name |
//! | `source` | string | `inpn`, `gpu`, `biodiv-sports`, `geo-admin`, `geo-api` |
//! | `url` | string or null | record sheet, act or information page |
//! | `months` | array of 1..=12 or null | months when the area is sensitive (Biodiv'sports); null = no seasonal information (the area exists all year, its rules carry their own periods) |
//! | `practices` | array of strings | Biodiv'sports practices concerned (`terrestre`, `vertical`, ...) |
//! | `designated` | string or null | creation date of the area when known (ISO date) |
//! | `area_ha` | number | geodesic area of the geometry inside the site box, hectares |
//! | `clipped` | bool | whether the geometry was cut by the site box |
//! | `note` | string or null | remark of the pipeline (merges, caveats) |

use std::collections::BTreeSet;

use geo::{BoundingRect, Intersects, MultiPolygon, Point, Rect};
use geojson::{Feature, FeatureCollection, GeometryValue};
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use skimap_common::geom::LonLat;

use crate::error::{Error, Result};

/// Identifier prefix of the Biodiv'sports zones (`biodiv:<id>`): a personal-use source
/// (no licence found, docs/licences-donnees.md).
pub const BIODIV_ID_PREFIX: &str = "biodiv:";

/// Kind of a zone. The rules ([`crate::rules`]) select zones by id or by kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ZoneKind {
    /// Arrêté préfectoral de protection de biotope (INPN `patrinat_apb:apb`).
    Appb,
    /// Réserve naturelle nationale (INPN `patrinat_rnn:rnn`).
    ReserveNaturelleNationale,
    /// Réserve naturelle régionale (INPN `patrinat_rnr:rnr`).
    ReserveNaturelleRegionale,
    /// Parc naturel régional (INPN `patrinat_pnr:pnr`).
    ParcNaturelRegional,
    /// Parc national (INPN `patrinat_pn:parc_national`).
    ParcNational,
    /// Site classé (INPN `patrinat_sc:sc`, or GPU AC2 marked "Site Classé").
    SiteClasse,
    /// Site inscrit (GPU AC2 not matching any site classé). Only source: GPU.
    SiteInscrit,
    /// Site patrimonial remarquable (GPU AC4).
    Spr,
    /// Historic monument itself (GPU AC1 "Monument historique").
    MonumentHistorique,
    /// Surroundings of an historic monument (GPU AC1 "Périmètre des abords").
    MonumentAbords,
    /// Drinking water catchment protection perimeter (GPU AS1).
    CaptageProtection,
    /// Forêt de protection (GPU A7).
    ForetProtection,
    /// Natura 2000, habitats directive (INPN `patrinat_sic:sic`).
    Natura2000Sic,
    /// Natura 2000, birds directive (INPN `patrinat_zps:zps`).
    Natura2000Zps,
    /// Réserve biologique (INPN `patrinat_rb:rb`).
    ReserveBiologique,
    /// Sensitive area published by Biodiv'sports (seasonal, by practice).
    BiodivSports,
    /// Swiss federal hunting reserve (district franc fédéral), protected part.
    DistrictFranc,
    /// Part of a country inside the site box (`country:fr`).
    Country,
    /// French département inside the site box (`dept:25`).
    Department,
    /// Swiss canton inside the site box (`canton:vd`).
    Canton,
    /// Commune referenced by a rule (`commune:05027`, `commune:ch-5872`).
    Commune,
}

impl ZoneKind {
    /// snake_case name, as serialised.
    pub fn as_str(self) -> &'static str {
        match self {
            ZoneKind::Appb => "appb",
            ZoneKind::ReserveNaturelleNationale => "reserve_naturelle_nationale",
            ZoneKind::ReserveNaturelleRegionale => "reserve_naturelle_regionale",
            ZoneKind::ParcNaturelRegional => "parc_naturel_regional",
            ZoneKind::ParcNational => "parc_national",
            ZoneKind::SiteClasse => "site_classe",
            ZoneKind::SiteInscrit => "site_inscrit",
            ZoneKind::Spr => "spr",
            ZoneKind::MonumentHistorique => "monument_historique",
            ZoneKind::MonumentAbords => "monument_abords",
            ZoneKind::CaptageProtection => "captage_protection",
            ZoneKind::ForetProtection => "foret_protection",
            ZoneKind::Natura2000Sic => "natura2000_sic",
            ZoneKind::Natura2000Zps => "natura2000_zps",
            ZoneKind::ReserveBiologique => "reserve_biologique",
            ZoneKind::BiodivSports => "biodiv_sports",
            ZoneKind::DistrictFranc => "district_franc",
            ZoneKind::Country => "country",
            ZoneKind::Department => "department",
            ZoneKind::Canton => "canton",
            ZoneKind::Commune => "commune",
        }
    }
}

/// A set of months (1 = January ... 12 = December). Serialised as a sorted array.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Months(u16);

impl Months {
    /// Every month.
    pub const ALL: Months = Months(0x0FFF);

    /// From a list of month numbers; numbers outside 1..=12 are rejected.
    pub fn from_numbers(months: impl IntoIterator<Item = u32>) -> Result<Self> {
        let mut bits = 0u16;
        for m in months {
            if !(1..=12).contains(&m) {
                return Err(Error::data("months", format!("invalid month {m}")));
            }
            bits |= 1 << (m - 1);
        }
        Ok(Months(bits))
    }

    /// From twelve flags, January first (Biodiv'sports `period`).
    pub fn from_flags(flags: &[bool]) -> Result<Self> {
        if flags.len() != 12 {
            return Err(Error::data(
                "months",
                format!("expected 12 month flags, got {}", flags.len()),
            ));
        }
        Self::from_numbers(
            flags
                .iter()
                .enumerate()
                .filter(|(_, f)| **f)
                .map(|(i, _)| i as u32 + 1),
        )
    }

    /// Whether `month` (1..=12) is in the set.
    pub fn contains(self, month: u32) -> bool {
        (1..=12).contains(&month) && self.0 & (1 << (month - 1)) != 0
    }

    /// Month numbers in increasing order.
    pub fn numbers(self) -> Vec<u32> {
        (1..=12).filter(|m| self.contains(*m)).collect()
    }

    /// Whether no month is set.
    pub fn is_empty(self) -> bool {
        self.0 == 0
    }
}

impl Serialize for Months {
    fn serialize<S: Serializer>(&self, s: S) -> std::result::Result<S::Ok, S::Error> {
        self.numbers().serialize(s)
    }
}

impl<'de> Deserialize<'de> for Months {
    fn deserialize<D: Deserializer<'de>>(d: D) -> std::result::Result<Self, D::Error> {
        let v = Vec::<u32>::deserialize(d)?;
        Months::from_numbers(v).map_err(serde::de::Error::custom)
    }
}

/// Properties of a zone (the GeoJSON `properties` object).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ZoneProps {
    /// Stable identifier `<namespace>:<source id>`: `inpn:FR3800988`,
    /// `gpu:<idass>`, `biodiv:1183`, `ch-jbg:980`, `country:fr`, `dept:25`, `canton:vd`,
    /// `commune:05027` (INSEE), `commune:ch-5872` (Swiss BFS number).
    pub id: String,
    /// Kind of zone.
    pub kind: ZoneKind,
    /// Official name.
    pub name: String,
    /// Source name (downloader source of the data).
    pub source: String,
    /// Record sheet, act or information page.
    #[serde(default)]
    pub url: Option<String>,
    /// Sensitive months (Biodiv'sports); `None` when the source gives no season.
    #[serde(default)]
    pub months: Option<Months>,
    /// Biodiv'sports practices concerned.
    #[serde(default)]
    pub practices: Vec<String>,
    /// Creation date of the area, ISO date, when known.
    #[serde(default)]
    pub designated: Option<String>,
    /// Geodesic area inside the site box, hectares.
    pub area_ha: f64,
    /// Whether the geometry was cut by the site box.
    #[serde(default)]
    pub clipped: bool,
    /// Pipeline remark.
    #[serde(default)]
    pub note: Option<String>,
}

/// A zone: properties and WGS84 geometry.
#[derive(Debug, Clone, PartialEq)]
pub struct Zone {
    /// Properties.
    pub props: ZoneProps,
    /// Geometry, WGS84 (lon, lat).
    pub geometry: MultiPolygon<f64>,
    bounds: Option<Rect<f64>>,
}

impl Zone {
    /// Builds a zone and caches its bounding box.
    pub fn new(props: ZoneProps, geometry: MultiPolygon<f64>) -> Self {
        let bounds = geometry.bounding_rect();
        Self {
            props,
            geometry,
            bounds,
        }
    }

    /// Identifier.
    pub fn id(&self) -> &str {
        &self.props.id
    }

    /// Kind.
    pub fn kind(&self) -> ZoneKind {
        self.props.kind
    }

    /// Whether the position is inside the zone (boundary included).
    pub fn contains(&self, p: LonLat) -> bool {
        let Some(b) = self.bounds else {
            return false;
        };
        if p.lon < b.min().x || p.lon > b.max().x || p.lat < b.min().y || p.lat > b.max().y {
            return false;
        }
        self.geometry.intersects(&Point::new(p.lon, p.lat))
    }

    /// Whether the zone is sensitive in `month` (1..=12); zones without season always are.
    pub fn active_in_month(&self, month: u32) -> bool {
        self.props.months.is_none_or(|m| m.contains(month))
    }
}

/// All zones of a site.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ZoneLayer {
    /// Zones, in a stable order (kind, then id).
    pub zones: Vec<Zone>,
}

impl ZoneLayer {
    /// Parses a `zones.geojson` FeatureCollection.
    pub fn from_geojson_str(s: &str) -> Result<Self> {
        let fc: FeatureCollection = s
            .parse()
            .map_err(|e| Error::data("zones GeoJSON", format!("{e}")))?;
        let mut zones = Vec::with_capacity(fc.features.len());
        for f in fc.features {
            let props = f
                .properties
                .ok_or_else(|| Error::data("zones GeoJSON", "feature without properties"))?;
            let props: ZoneProps = serde_json::from_value(serde_json::Value::Object(props))
                .map_err(|e| Error::data("zones GeoJSON properties", e.to_string()))?;
            let geometry = match f.geometry {
                Some(g) => multipolygon_from_geojson(&g.value)?,
                None => MultiPolygon::new(vec![]),
            };
            zones.push(Zone::new(props, geometry));
        }
        let layer = ZoneLayer { zones };
        layer.check_unique_ids()?;
        Ok(layer)
    }

    /// Serialises as a GeoJSON FeatureCollection (coordinates rounded to 1e-6°, ~0.1 m).
    pub fn to_geojson_string(&self) -> Result<String> {
        let features = self
            .zones
            .iter()
            .map(|z| {
                let props = match serde_json::to_value(&z.props) {
                    Ok(serde_json::Value::Object(m)) => m,
                    _ => unreachable!("ZoneProps serialises to an object"),
                };
                Feature {
                    geometry: Some(geojson::Geometry::new(GeometryValue::from(
                        &round_multipolygon(&z.geometry, 1e6),
                    ))),
                    properties: Some(props),
                    ..Default::default()
                }
            })
            .collect::<Vec<_>>();
        serde_json::to_string(&FeatureCollection::new(features))
            .map_err(|e| Error::data("zones GeoJSON", e.to_string()))
    }

    /// Zone by identifier.
    pub fn get(&self, id: &str) -> Option<&Zone> {
        self.zones.iter().find(|z| z.id() == id)
    }

    /// Zones containing the position.
    pub fn containing(&self, p: LonLat) -> impl Iterator<Item = &Zone> {
        self.zones.iter().filter(move |z| z.contains(p))
    }

    /// Fails on duplicate identifiers.
    pub fn check_unique_ids(&self) -> Result<()> {
        let mut seen = BTreeSet::new();
        for z in &self.zones {
            if !seen.insert(z.id()) {
                return Err(Error::data(
                    "zones",
                    format!("duplicate zone id {}", z.id()),
                ));
            }
        }
        Ok(())
    }
}

/// Converts a GeoJSON geometry to a multipolygon. Polygons, multipolygons and geometry
/// collections of those are accepted (Biodiv'sports returns collections); other types
/// are an error.
pub fn multipolygon_from_geojson(g: &GeometryValue) -> Result<MultiPolygon<f64>> {
    let mut out = Vec::new();
    collect_polygons(g, &mut out)?;
    Ok(MultiPolygon::new(out))
}

fn collect_polygons(g: &GeometryValue, out: &mut Vec<geo::Polygon<f64>>) -> Result<()> {
    match g {
        GeometryValue::Polygon { .. } => {
            let p: geo::Polygon<f64> = g
                .try_into()
                .map_err(|e| Error::data("GeoJSON polygon", format!("{e}")))?;
            out.push(p);
        }
        GeometryValue::MultiPolygon { .. } => {
            let mp: MultiPolygon<f64> = g
                .try_into()
                .map_err(|e| Error::data("GeoJSON multipolygon", format!("{e}")))?;
            out.extend(mp.0);
        }
        GeometryValue::GeometryCollection { geometries } => {
            for sub in geometries {
                collect_polygons(&sub.value, out)?;
            }
        }
        other => {
            return Err(Error::data(
                "GeoJSON geometry",
                format!("expected a polygonal geometry, got {}", other.type_name()),
            ));
        }
    }
    Ok(())
}

/// Rounds coordinates to `1/scale` and drops consecutive duplicates created by rounding.
pub fn round_multipolygon(mp: &MultiPolygon<f64>, scale: f64) -> MultiPolygon<f64> {
    let round_ring = |ls: &geo::LineString<f64>| {
        let mut v: Vec<geo::Coord<f64>> = Vec::with_capacity(ls.0.len());
        for c in &ls.0 {
            let r =
                geo::coord! { x: (c.x * scale).round() / scale, y: (c.y * scale).round() / scale };
            if v.last() != Some(&r) {
                v.push(r);
            }
        }
        geo::LineString::new(v)
    };
    MultiPolygon::new(
        mp.0.iter()
            .map(|p| {
                geo::Polygon::new(
                    round_ring(p.exterior()),
                    p.interiors().iter().map(round_ring).collect(),
                )
            })
            .filter(|p| p.exterior().0.len() >= 4)
            .collect(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use geo::polygon;

    fn square(id: &str, kind: ZoneKind, x0: f64, y0: f64, d: f64) -> Zone {
        let p = polygon![(x: x0, y: y0), (x: x0 + d, y: y0), (x: x0 + d, y: y0 + d), (x: x0, y: y0 + d), (x: x0, y: y0)];
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
            MultiPolygon::new(vec![p]),
        )
    }

    #[test]
    fn months_round_trip() {
        let m = Months::from_flags(&[
            true, true, false, false, false, false, false, false, false, false, false, true,
        ])
        .unwrap();
        assert_eq!(m.numbers(), vec![1, 2, 12]);
        assert!(m.contains(12) && !m.contains(6) && !m.contains(13) && !m.contains(0));
        let json = serde_json::to_string(&m).unwrap();
        assert_eq!(json, "[1,2,12]");
        assert_eq!(serde_json::from_str::<Months>(&json).unwrap(), m);
        assert!(serde_json::from_str::<Months>("[13]").is_err());
        assert!(Months::from_flags(&[true; 11]).is_err());
    }

    #[test]
    fn geojson_round_trip_and_containment() {
        let mut z = square("biodiv:1", ZoneKind::BiodivSports, 6.0, 46.0, 0.1);
        z.props.months = Some(Months::from_numbers([12, 1, 2]).unwrap());
        z.props.practices = vec!["terrestre".into()];
        let layer = ZoneLayer {
            zones: vec![z, square("country:fr", ZoneKind::Country, 5.0, 45.0, 2.0)],
        };
        let s = layer.to_geojson_string().unwrap();
        let back = ZoneLayer::from_geojson_str(&s).unwrap();
        assert_eq!(back.zones.len(), 2);
        assert_eq!(back.zones[0].props, layer.zones[0].props);
        let ids: Vec<_> = back
            .containing(LonLat::new(6.05, 46.05))
            .map(|z| z.id())
            .collect();
        assert_eq!(ids, ["biodiv:1", "country:fr"]);
        // Boundary is inside.
        assert!(back.zones[0].contains(LonLat::new(6.0, 46.05)));
        assert!(!back.zones[0].contains(LonLat::new(6.2, 46.05)));
        assert!(back.zones[0].active_in_month(1));
        assert!(!back.zones[0].active_in_month(7));
        assert!(back.zones[1].active_in_month(7));
    }

    #[test]
    fn duplicate_ids_rejected() {
        let layer = ZoneLayer {
            zones: vec![
                square("a", ZoneKind::Appb, 0.0, 0.0, 1.0),
                square("a", ZoneKind::Appb, 0.0, 0.0, 1.0),
            ],
        };
        let s = layer.to_geojson_string().unwrap();
        assert!(ZoneLayer::from_geojson_str(&s).is_err());
    }

    #[test]
    fn geometry_collection_is_flattened() {
        let g: geojson::Geometry = r#"{"type":"GeometryCollection","geometries":[
            {"type":"Polygon","coordinates":[[[0,0],[1,0],[1,1],[0,0]]]},
            {"type":"MultiPolygon","coordinates":[[[[2,2],[3,2],[3,3],[2,2]]]]}]}"#
            .parse()
            .unwrap();
        assert_eq!(multipolygon_from_geojson(&g.value).unwrap().0.len(), 2);
        let line: geojson::Geometry = r#"{"type":"LineString","coordinates":[[0,0],[1,1]]}"#
            .parse()
            .unwrap();
        assert!(multipolygon_from_geojson(&line.value).is_err());
    }
}
