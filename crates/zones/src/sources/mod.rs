//! Fetching the zone sources through the shared downloader (pipeline side).
//!
//! | Module | Source | Downloader source |
//! |---|---|---|
//! | [`inpn`] | INPN protected areas, WFS Géoplateforme `patrinat_*` | `ign` |
//! | [`gpu`] | GPU easements `wfs_sup:assiette_sup_s` (AC1, AC2, AC4, AS1, A7) | `ign` |
//! | [`biodiv`] | Biodiv'sports sensitive areas, `period=ignore` | `biodiv-sports` |
//! | [`swiss`] | Swiss federal hunting reserves, cantons, communes (geo.admin) | `geo-admin` |
//! | [`jurisdiction`] | French communes and départements (geo.api.gouv.fr) | `geo-api` |

pub mod biodiv;
pub mod gpu;
pub mod inpn;
pub mod jurisdiction;
pub mod swiss;

use geo::{BoundingRect, MultiPolygon, Rect};
use geojson::{Feature, FeatureCollection, JsonObject};
use skimap_common::crs::Crs;
use skimap_common::download::{Downloader, Request};
use skimap_common::geom::BBox;

use crate::error::{Error, Result};
use crate::geom::{rects_overlap, reproject};
use crate::layer::{ZoneProps, multipolygon_from_geojson};

/// A zone as fetched: full geometry (not yet clipped), area not yet computed.
#[derive(Debug, Clone)]
pub struct RawZone {
    /// Properties (`area_ha` and `clipped` are filled by the build).
    pub props: ZoneProps,
    /// Geometry, WGS84.
    pub geometry: MultiPolygon<f64>,
}

/// Downloads a JSON document.
pub(crate) fn fetch_json(dl: &Downloader, source: &str, url: &str) -> Result<serde_json::Value> {
    Ok(dl.fetch(&Request::get(source, url))?.json()?)
}

/// Downloads a GeoJSON FeatureCollection.
pub(crate) fn fetch_fc(dl: &Downloader, source: &str, url: &str) -> Result<FeatureCollection> {
    let text = dl.fetch(&Request::get(source, url))?.text()?;
    text.parse::<FeatureCollection>()
        .map_err(|e| Error::data(url, format!("not a GeoJSON FeatureCollection: {e}")))
}

/// Géoplateforme WFS endpoint.
pub const GEOPF_WFS: &str = "https://data.geopf.fr/wfs/ows";

/// Page size of WFS requests.
const WFS_PAGE: usize = 1000;

/// URL of one WFS page: features of `typename` intersecting the box, asked in CRS:84
/// (lon, lat). The BBOX is given in `urn:ogc:def:crs:EPSG::4326`, hence lat,lon order.
pub fn wfs_url(typename: &str, b: BBox, start: usize) -> String {
    format!(
        "{GEOPF_WFS}?SERVICE=WFS&VERSION=2.0.0&REQUEST=GetFeature&TYPENAMES={typename}\
         &OUTPUTFORMAT=application/json&SRSNAME=CRS:84\
         &BBOX={},{},{},{},urn:ogc:def:crs:EPSG::4326&COUNT={WFS_PAGE}&STARTINDEX={start}",
        b.south, b.west, b.north, b.east
    )
}

/// All features of a WFS layer intersecting the box, with WGS84 geometries.
///
/// The server's `crs` member is checked: Web Mercator (EPSG:3857, the default of the
/// Géoplateforme) is reprojected; `CRS::84` and `EPSG::4326` are read as lon, lat (the
/// GPU layer labels CRS:84 output `EPSG::4326`), and the result is checked against the
/// site box to catch a swapped axis order.
pub fn wfs_features(
    dl: &Downloader,
    typename: &str,
    b: BBox,
) -> Result<Vec<(JsonObject, MultiPolygon<f64>)>> {
    let mut out = Vec::new();
    let mut start = 0;
    loop {
        let url = wfs_url(typename, b, start);
        let fc = fetch_fc(dl, "ign", &url)?;
        let crs = fc
            .foreign_members
            .as_ref()
            .and_then(|m| m.get("crs"))
            .and_then(|c| c.pointer("/properties/name"))
            .and_then(|n| n.as_str())
            .unwrap_or("")
            .to_owned();
        let from = if crs.contains("3857") {
            Some(Crs::WebMercator)
        } else if crs.is_empty() || crs.contains("CRS::84") || crs.contains("EPSG::4326") {
            None
        } else {
            return Err(Error::data(&url, format!("unexpected CRS `{crs}`")));
        };
        let n = fc.features.len();
        for f in fc.features {
            let (props, mut geom) = feature_parts(f, &url)?;
            if let Some(from) = from {
                geom = reproject(&geom, from, Crs::Wgs84)?;
            }
            check_lon_lat(&geom, b, &url)?;
            out.push((props, geom));
        }
        if n < WFS_PAGE {
            break;
        }
        start += n;
    }
    Ok(out)
}

/// Properties and polygonal geometry of a feature (features without geometry give an
/// empty multipolygon).
pub(crate) fn feature_parts(f: Feature, context: &str) -> Result<(JsonObject, MultiPolygon<f64>)> {
    let geom = match &f.geometry {
        Some(g) => {
            multipolygon_from_geojson(&g.value).map_err(|e| Error::data(context, e.to_string()))?
        }
        None => MultiPolygon::new(vec![]),
    };
    Ok((f.properties.unwrap_or_default(), geom))
}

/// Fails when a geometry that should touch the site box (it was selected by a box
/// filter) lies far from it: the sign of a swapped lat/lon axis order.
fn check_lon_lat(g: &MultiPolygon<f64>, b: BBox, context: &str) -> Result<()> {
    let Some(r) = g.bounding_rect() else {
        return Ok(());
    };
    let margin = 0.05;
    let site = Rect::new(
        geo::coord! { x: b.west - margin, y: b.south - margin },
        geo::coord! { x: b.east + margin, y: b.north + margin },
    );
    if !rects_overlap(r, site) {
        return Err(Error::data(
            context,
            format!(
                "geometry {:?}..{:?} does not touch the site box: axis order?",
                r.min(),
                r.max()
            ),
        ));
    }
    Ok(())
}

/// String property, `None` when missing, null or blank.
pub(crate) fn prop_str(p: &JsonObject, key: &str) -> Option<String> {
    p.get(key)
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wfs_url_uses_lat_lon_bbox_and_crs84_output() {
        let u = wfs_url("patrinat_apb:apb", BBox::new(46.54, 6.02, 46.68, 6.2), 0);
        assert!(
            u.contains("BBOX=46.54,6.02,46.68,6.2,urn:ogc:def:crs:EPSG::4326"),
            "{u}"
        );
        assert!(u.contains("SRSNAME=CRS:84"));
        assert!(u.contains("TYPENAMES=patrinat_apb:apb"));
        assert!(u.ends_with("STARTINDEX=0"));
    }

    #[test]
    fn swapped_axes_are_detected() {
        let b = BBox::new(46.54, 6.02, 46.68, 6.2);
        let ok = MultiPolygon::new(vec![crate::geom::bbox_polygon(
            BBox::new(46.6, 6.1, 46.61, 6.11),
            1,
        )]);
        assert!(check_lon_lat(&ok, b, "t").is_ok());
        let swapped = MultiPolygon::new(vec![crate::geom::bbox_polygon(
            BBox::new(6.1, 46.6, 6.11, 46.61),
            1,
        )]);
        assert!(check_lon_lat(&swapped, b, "t").is_err());
    }
}
