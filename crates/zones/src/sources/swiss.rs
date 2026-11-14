//! Swiss data from the federal geoportal (api3.geo.admin.ch).
//!
//! - Federal hunting reserves (districts francs fédéraux, layer
//!   `ch.bafu.bundesinventare-jagdbanngebiete`): only the parts with integral or partial
//!   protection are kept; the "wildlife damage perimeters" are hunting management areas
//!   with no rule for skiers.
//! - Cantons (`ch.swisstopo.swissboundaries3d-kanton-flaeche.fill`) and communes
//!   (`ch.swisstopo.swissboundaries3d-gemeinde-flaeche.fill`, current year only) for the
//!   cantonal and communal rules.

use geojson::GeometryValue;
use serde_json::Value;
use skimap_common::download::Downloader;
use skimap_common::geom::BBox;

use super::{RawZone, fetch_json};
use crate::error::{Error, Result};
use crate::layer::{ZoneKind, ZoneProps, multipolygon_from_geojson};

/// REST API root.
pub const API: &str = "https://api3.geo.admin.ch/rest/services/api/MapServer";
/// Federal hunting reserves.
pub const LAYER_DISTRICTS: &str = "ch.bafu.bundesinventare-jagdbanngebiete";
/// Cantons.
pub const LAYER_CANTONS: &str = "ch.swisstopo.swissboundaries3d-kanton-flaeche.fill";
/// Communes.
pub const LAYER_COMMUNES: &str = "ch.swisstopo.swissboundaries3d-gemeinde-flaeche.fill";

/// The identify service caps its answers; reaching the cap means results were lost.
const IDENTIFY_CAP: usize = 200;

/// URL of an identify request: features of `layer` intersecting the box, WGS84 GeoJSON.
pub fn identify_url(layer: &str, b: BBox) -> String {
    format!(
        "{API}/identify?geometry={},{},{},{}&geometryType=esriGeometryEnvelope\
         &layers=all:{layer}&sr=4326&tolerance=0&returnGeometry=true\
         &geometryFormat=geojson&lang=fr",
        b.west, b.south, b.east, b.north
    )
}

fn results(v: &Value, url: &str) -> Result<Vec<Value>> {
    let r = v
        .get("results")
        .and_then(|r| r.as_array())
        .ok_or_else(|| Error::data(url, "no `results` array"))?;
    Ok(r.clone())
}

fn geometry(r: &Value, url: &str) -> Result<geo::MultiPolygon<f64>> {
    let g = r
        .get("geometry")
        .ok_or_else(|| Error::data(url, "result without geometry"))?;
    let g: GeometryValue = serde_json::from_value(g.clone())
        .map_err(|e| Error::data(url, format!("geometry: {e}")))?;
    multipolygon_from_geojson(&g)
}

fn identify(dl: &Downloader, layer: &str, b: BBox) -> Result<(String, Vec<Value>)> {
    let url = identify_url(layer, b);
    let v = fetch_json(dl, "geo-admin", &url)?;
    let r = results(&v, &url)?;
    if r.len() >= IDENTIFY_CAP {
        return Err(Error::data(
            &url,
            format!("{} results: the identify cap is reached", r.len()),
        ));
    }
    Ok((url, r))
}

fn s(r: &Value, ptr: &str) -> Option<String> {
    match r.pointer(ptr)? {
        Value::String(s) => Some(s.trim().to_owned()),
        Value::Number(n) => Some(n.to_string()),
        _ => None,
    }
}

/// Protected parts of the federal hunting reserves intersecting the box.
pub fn districts(dl: &Downloader, b: BBox) -> Result<Vec<RawZone>> {
    let (url, res) = identify(dl, LAYER_DISTRICTS, b)?;
    let mut out = Vec::new();
    for r in &res {
        let typ_de = s(r, "/properties/typ_de").unwrap_or_default();
        if !typ_de.contains("Schutzbestimmungen") {
            continue;
        }
        let id = s(r, "/id").ok_or_else(|| Error::data(&url, "district without id"))?;
        let number = s(r, "/properties/objektnummer").unwrap_or_default();
        let name = s(r, "/properties/gebietsname").unwrap_or_default();
        let typ_fr = s(r, "/properties/typ_fr").unwrap_or_default();
        out.push(RawZone {
            props: ZoneProps {
                id: format!("ch-jbg:{id}"),
                kind: ZoneKind::DistrictFranc,
                name: format!("District franc fédéral n° {number} {name}"),
                source: "geo-admin".into(),
                url: s(r, "/properties/refobjblatt"),
                months: None,
                practices: vec![],
                designated: None,
                area_ha: 0.0,
                clipped: false,
                note: Some(typ_fr),
            },
            geometry: geometry(r, &url)?,
        });
    }
    tracing::info!(parts = out.len(), "Swiss hunting reserves");
    Ok(out)
}

/// Cantons intersecting the box (`canton:vd`).
pub fn cantons(dl: &Downloader, b: BBox) -> Result<Vec<RawZone>> {
    let (url, res) = identify(dl, LAYER_CANTONS, b)?;
    let mut out = Vec::new();
    for r in &res {
        let ak = s(r, "/properties/ak").ok_or_else(|| Error::data(&url, "canton without `ak`"))?;
        out.push(RawZone {
            props: ZoneProps {
                id: format!("canton:{}", ak.to_lowercase()),
                kind: ZoneKind::Canton,
                name: s(r, "/properties/name").unwrap_or_else(|| ak.clone()),
                source: "geo-admin".into(),
                url: None,
                months: None,
                practices: vec![],
                designated: None,
                area_ha: 0.0,
                clipped: false,
                note: None,
            },
            geometry: geometry(r, &url)?,
        });
    }
    Ok(out)
}

/// A Swiss commune by its BFS number (current boundaries), `commune:ch-<bfs>`.
pub fn commune(dl: &Downloader, bfs: u32) -> Result<Option<RawZone>> {
    let url = format!(
        "{API}/find?layer={LAYER_COMMUNES}&searchText={bfs}&searchField=gde_nr\
         &contains=false&returnGeometry=true&geometryFormat=geojson&sr=4326&lang=fr"
    );
    let v = fetch_json(dl, "geo-admin", &url)?;
    let res = results(&v, &url)?;
    let Some(r) = res
        .iter()
        .find(|r| r.pointer("/properties/is_current_jahr") == Some(&Value::Bool(true)))
    else {
        return Ok(None);
    };
    Ok(Some(RawZone {
        props: ZoneProps {
            id: format!("commune:ch-{bfs}"),
            kind: ZoneKind::Commune,
            name: s(r, "/properties/gemname").unwrap_or_else(|| bfs.to_string()),
            source: "geo-admin".into(),
            url: None,
            months: None,
            practices: vec![],
            designated: None,
            area_ha: 0.0,
            clipped: false,
            note: None,
        },
        geometry: geometry(r, &url)?,
    }))
}
