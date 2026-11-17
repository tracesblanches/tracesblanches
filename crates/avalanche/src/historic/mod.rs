//! Historic avalanches (P20, #99): CLPA footprints republished by the DDT 05 under the
//! Licence Ouverte, and the piste and itinerary sections that cross them.
//!
//! Source: « DDT05 - Risque naturel : aléa Avalanche dans les Hautes-Alpes (shp) »
//! (layer `L_ALEA_AVALANCHE_005`, Lambert-93), Licence Ouverte 2.0 on data.gouv.fr. The
//! CLPA itself is restricted by INRAE to personal use; whether the DDT may republish it
//! under the Licence Ouverte is not verified (report 07). Only records whose `SOURCE`
//! contains `CLPA` (`CLPA`, `PPR-CLPA`) are historic avalanche footprints; the other
//! records (PPR hazard zones) are counted for comparison only.
//!
//! The Jura has no CLPA: for sites outside the Hautes-Alpes the layer is empty with a
//! notice.
//!
//! **Personal use only.** INRAE restricts the CLPA to personal use and the DDT 05's right
//! to republish it is not confirmed (docs/licences-donnees.md: « Usage personnel
//! seulement, tant qu'il n'y a pas d'accord écrit : […] CLPA et EPA d'INRAE »). The
//! footprints are therefore read only with `personal_sources`
//! (`skimap_common::config::Config::personal_sources`); without it the layer is written
//! empty with a notice, so that a previous personal build never lingers, and with it the
//! DDT 05 source is marked `personal_use_only`.
//!
//! Pistes: the `pistes` layer of the site (`skimap_osm::extract`, P2), which must be built
//! first; [`build_from_dump`] reads a saved Overpass extract instead (report 07).
//!
//! Output (`data/out/<site>/avalanche-historic/`):
//! - `emprises.geojson`: CLPA footprints intersecting the site box or in the site commune,
//!   WGS84 polygons;
//!   properties `ddt05_index` (record index in the shapefile), `source`, `risque`,
//!   `information`, `moa`, `ref_saisie`, `insee`, `commune`, `area_m2` (Lambert-93).
//! - `crossings.geojson`: sections of pistes / itineraries inside a footprint, WGS84
//!   lines; properties `id`, `osm_way_id`, `name`, `relations`, `network`, `length_m`
//!   (Lambert-93), `ddt05_indices`, `risque`, `alert_level`, `alert`.
//! - `summary.json`: [`Summary`], totals per network and per piste, comparison with
//!   report 07, BERA of the day if `bera/bera.json` exists.
//! - `avalanche-historic.meta.json`.
//!
//! Lengths are Euclidean in Lambert-93 (scale factor within 2·10⁻⁴ of 1 between 44° and
//! 47° N), as in the reference computation of report 07.

pub mod clip;
pub mod pistes;
pub mod shapefile;

use std::collections::{BTreeMap, BTreeSet};
use std::io::Read;
use std::path::Path;

use serde::Serialize;
use serde_json::{Value, json};
use skimap_common::crs::{self, Crs};
use skimap_common::download::{Downloader, Request};
use skimap_common::geom::LonLat;
use skimap_common::layout::DataLayout;
use skimap_common::meta::{FileMeta, LayerMeta, SourceRef, now_rfc3339, unix_to_rfc3339};
use skimap_common::output::write_json;
use skimap_common::site::Site;
use skimap_meteo::bera::{BeraFile, risk_label};

use crate::error::{Error, Result};
use clip::{Poly, inside_runs};
use pistes::{Network, PisteLine};

/// Layer name.
pub const LAYER: &str = "avalanche-historic";
/// Department covered by the DDT 05 layer.
pub const DDT05_DEPARTMENT: &str = "05";
/// Download URL of the zipped shapefile (ATOM service of the ministry, linked from
/// data.gouv.fr).
pub const DDT05_ZIP_URL: &str = "https://atom.geo-ide.developpement-durable.gouv.fr/atomArchive/GetResource?id=7b862177-e717-4fe1-8271-ff1b82cf3d2c&dataType=dataset";
/// data.gouv.fr page of the dataset.
pub const DDT05_DATASET_URL: &str = "https://www.data.gouv.fr/datasets/ddt05-risque-naturel-alea-avalanche-dans-les-hautes-alpes-shp";
/// Downloader source of the ministry ATOM service.
pub const SOURCE_GEOIDE: &str = "geoide";
/// Public Overpass endpoint (source of the research extracts read by [`build_from_dump`]).
pub const OVERPASS_URL: &str = "https://overpass-api.de/api/interpreter";
/// Layer of the OSM crate holding the piste lines (`skimap_osm::extract::LAYER`).
pub const PISTES_LAYER: &str = "pistes";
/// File of [`PISTES_LAYER`] read by [`build`].
pub const PISTES_FILE: &str = "pistes.geojson";
/// Footprints smaller than this (m²) are counted as slivers in the summary (digitising
/// artefacts: about 6 % of the CLPA records of a Queyras commune are below it); they are
/// kept.
pub const SLIVER_AREA_M2: f64 = 100.0;
/// Base name of the layer inside the zip archive.
const SHP_BASE: &str = "L_ALEA_AVALANCHE_005";

/// One record of the DDT 05 avalanche layer.
#[derive(Debug, Clone, PartialEq)]
pub struct Emprise {
    /// Record index in the shapefile (stable identifier within one edition).
    pub index: usize,
    /// Attributes (`SOURCE`, `RISQUE`, `INFORMATIO`, `MOA`, `REF_SAISIE`, `INSEE`,
    /// `COMMUNE`).
    pub attrs: BTreeMap<String, String>,
    /// Geometry, Lambert-93.
    pub poly: Poly,
}

impl Emprise {
    fn attr(&self, k: &str) -> &str {
        self.attrs.get(k).map(String::as_str).unwrap_or("")
    }

    /// Whether the record is a CLPA footprint (historic avalanche).
    pub fn is_clpa(&self) -> bool {
        self.attr("SOURCE").contains("CLPA")
    }
}

/// Reads all records of the DDT 05 layer from the zipped shapefile.
pub fn read_ddt05_zip(zip_bytes: &[u8]) -> Result<Vec<Emprise>> {
    let mut zip = zip::ZipArchive::new(std::io::Cursor::new(zip_bytes))
        .map_err(|e| Error::format("DDT05 zip", e.to_string()))?;
    let mut read = |ext: &str| -> Result<Vec<u8>> {
        let name = (0..zip.len())
            .filter_map(|i| zip.name_for_index(i)?.ok().map(|n| n.into_owned()))
            .find(|n| n.ends_with(&format!("{SHP_BASE}.{ext}")))
            .ok_or_else(|| Error::format("DDT05 zip", format!("no {SHP_BASE}.{ext}")))?;
        let mut f = zip
            .by_name(&name)
            .map_err(|e| Error::format("DDT05 zip", e.to_string()))?;
        let mut buf = Vec::new();
        f.read_to_end(&mut buf)
            .map_err(|e| Error::format("DDT05 zip", e.to_string()))?;
        Ok(buf)
    };
    let shp = read("shp")?;
    let dbf = read("dbf")?;
    let prj = String::from_utf8_lossy(&read("prj")?).into_owned();
    if !prj.contains("Lambert_93") && !prj.contains("Lambert-93") {
        return Err(Error::format(
            "DDT05 zip",
            format!("unexpected projection {prj}"),
        ));
    }
    read_ddt05(&shp, &dbf)
}

/// Reads all records of the DDT 05 layer from its `.shp` and `.dbf` (Lambert-93).
pub fn read_ddt05(shp: &[u8], dbf: &[u8]) -> Result<Vec<Emprise>> {
    let geoms = shapefile::read_polygons(shp).map_err(|e| Error::format("DDT05 shp", e))?;
    let rows = shapefile::read_dbf(dbf).map_err(|e| Error::format("DDT05 dbf", e))?;
    if geoms.len() != rows.len() {
        return Err(Error::format(
            "DDT05",
            format!("{} shapes but {} attribute rows", geoms.len(), rows.len()),
        ));
    }
    Ok(geoms
        .into_iter()
        .zip(rows)
        .enumerate()
        .filter_map(|(index, (g, attrs))| {
            let rings = g?;
            (!attrs.is_empty()).then(|| Emprise {
                index,
                attrs,
                poly: Poly::new(rings),
            })
        })
        .collect())
}

/// Request of the DDT 05 archive (static dataset, cached 30 days by the `geoide` policy).
pub fn ddt05_request() -> Request {
    Request::get(SOURCE_GEOIDE, DDT05_ZIP_URL)
}

/// Request of the commune outline (geo.api.gouv.fr).
pub fn commune_request(site: &Site) -> Request {
    Request::get(
        "geo-api",
        &format!(
            "https://geo.api.gouv.fr/communes/{}?format=geojson&geometry=contour",
            site.insee
        ),
    )
}

/// Reads the outline of a commune (GeoJSON Feature, Polygon or MultiPolygon) as a
/// polygon in WGS84 degrees.
pub fn read_commune(geojson: &[u8]) -> Result<Poly> {
    let v: Value =
        serde_json::from_slice(geojson).map_err(|e| Error::format("geo-api", e.to_string()))?;
    let g = &v["geometry"];
    let to_ring = |r: &Value| -> Vec<(f64, f64)> {
        r.as_array()
            .map(|pts| {
                pts.iter()
                    .filter_map(|p| Some((p[0].as_f64()?, p[1].as_f64()?)))
                    .collect()
            })
            .unwrap_or_default()
    };
    let rings: Vec<Vec<(f64, f64)>> = match g["type"].as_str() {
        Some("Polygon") => g["coordinates"]
            .as_array()
            .map(|a| a.iter().map(to_ring).collect())
            .unwrap_or_default(),
        Some("MultiPolygon") => g["coordinates"]
            .as_array()
            .map(|polys| {
                polys
                    .iter()
                    .filter_map(Value::as_array)
                    .flat_map(|p| p.iter().map(to_ring))
                    .collect()
            })
            .unwrap_or_default(),
        other => {
            return Err(Error::format(
                "geo-api",
                format!("unexpected geometry {other:?}"),
            ));
        }
    };
    if rings.is_empty() {
        return Err(Error::format("geo-api", "empty commune outline"));
    }
    Ok(Poly::new(rings))
}

/// A section of a piste inside one or more footprints.
#[derive(Debug, Clone, PartialEq)]
pub struct Crossing {
    /// `<osm way id>-<n>` (n-th run along the way).
    pub id: String,
    /// The line it belongs to.
    pub line: usize,
    /// Vertices, WGS84.
    pub coords: Vec<LonLat>,
    /// Length, metres (Lambert-93).
    pub length_m: f64,
    /// DDT 05 record indices of the footprints containing it.
    pub ddt05_indices: Vec<usize>,
}

/// Totals of one network.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct NetworkTotals {
    /// Network.
    pub network: Network,
    /// Number of lines (OSM ways) kept on the site.
    pub lines: usize,
    /// Total length, km.
    pub length_km: f64,
    /// Length inside a CLPA footprint, km.
    pub inside_clpa_km: f64,
    /// Length inside any record of the DDT 05 layer (CLPA and PPR), km.
    pub inside_any_ddt05_km: f64,
    /// Number of crossing sections.
    pub crossings: usize,
}

/// Totals of one piste name.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct PisteTotals {
    /// Name (`?` when the way has none).
    pub name: String,
    /// Network.
    pub network: Network,
    /// Length, km.
    pub length_km: f64,
    /// Length inside a CLPA footprint, km.
    pub inside_clpa_km: f64,
    /// Number of crossing sections.
    pub crossings: usize,
}

/// A value of report 07 next to the recomputed one.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Reconciliation {
    /// What is compared.
    pub item: String,
    /// Value in report 07.
    pub report07: f64,
    /// Recomputed value.
    pub recomputed: f64,
}

/// BERA of the day attached to the alerts.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct BeraContext {
    /// Massif name.
    pub massif: Option<String>,
    /// Maximum risk, if a bulletin was read.
    pub risk_max: Option<u8>,
    /// Validity of the bulletin.
    pub valid_until: Option<String>,
    /// Day of the product.
    pub date: String,
}

/// Content of `summary.json`.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Summary {
    /// Site identifier.
    pub site: String,
    /// Whether the DDT 05 layer covers the site.
    pub covered: bool,
    /// CLPA footprints of the layer: intersecting the site box or in the site commune.
    pub clpa_in_box: usize,
    /// CLPA footprints of the site commune (`INSEE`).
    pub clpa_in_commune: usize,
    /// Other records (PPR) of the site commune.
    pub other_in_commune: usize,
    /// Area of the CLPA footprints of the commune, km² (sum of the records).
    pub clpa_in_commune_km2: f64,
    /// CLPA footprints of the commune smaller than [`SLIVER_AREA_M2`].
    pub clpa_in_commune_slivers: usize,
    /// Piste source description.
    pub pistes_source: String,
    /// How lines were assigned to the site.
    pub site_filter: String,
    /// Totals per network.
    pub networks: Vec<NetworkTotals>,
    /// Totals per piste name, sorted by decreasing length inside footprints.
    pub pistes: Vec<PisteTotals>,
    /// Comparison with report 07 (sites the report measured, `data/report07.json`).
    pub reconciliation: Vec<Reconciliation>,
    /// BERA of the day, if `bera/bera.json` exists.
    pub bera: Option<BeraContext>,
    /// Message for the user (French).
    pub notice: String,
}

fn round(v: f64, digits: i32) -> f64 {
    let f = 10f64.powi(digits);
    // `+ 0.0` turns -0.0 into 0.0.
    (v * f).round() / f + 0.0
}

fn project(coords: &[LonLat]) -> Result<Vec<(f64, f64)>> {
    let mut pts: Vec<(f64, f64)> = coords.iter().map(|p| (p.lon, p.lat)).collect();
    crs::transform_points(Crs::Wgs84, Crs::Lambert93, &mut pts)?;
    Ok(pts)
}

fn unproject(pts: &[(f64, f64)]) -> Result<Vec<LonLat>> {
    let mut v = pts.to_vec();
    crs::transform_points(Crs::Lambert93, Crs::Wgs84, &mut v)?;
    Ok(v.into_iter().map(|(x, y)| LonLat::new(x, y)).collect())
}

/// Result of the crossing computation.
#[derive(Debug, Clone, PartialEq)]
pub struct Computed {
    /// Lines kept on the site.
    pub lines: Vec<PisteLine>,
    /// Length of each kept line, metres.
    pub line_lengths_m: Vec<f64>,
    /// Length of each kept line inside any DDT 05 record, metres.
    pub line_inside_any_m: Vec<f64>,
    /// Sections inside CLPA footprints.
    pub crossings: Vec<Crossing>,
}

/// Keeps the lines of the site and cuts them by the footprints.
///
/// `commune` (WGS84) keeps the lines whose vertex mean falls inside it; `None` keeps all.
/// `clpa` are the CLPA footprints, `all` every DDT 05 record (for the comparison total).
pub fn compute(
    lines: &[PisteLine],
    commune: Option<&Poly>,
    clpa: &[&Emprise],
    all: &[&Emprise],
) -> Result<Computed> {
    let kept: Vec<PisteLine> = lines
        .iter()
        .filter(|l| {
            commune.is_none_or(|c| {
                let m = l.vertex_mean();
                c.contains((m.lon, m.lat))
            })
        })
        .cloned()
        .collect();
    let clpa_polys: Vec<Poly> = clpa.iter().map(|e| e.poly.clone()).collect();
    let all_polys: Vec<Poly> = all.iter().map(|e| e.poly.clone()).collect();
    let mut out = Computed {
        lines: Vec::new(),
        line_lengths_m: Vec::new(),
        line_inside_any_m: Vec::new(),
        crossings: Vec::new(),
    };
    for (li, line) in kept.iter().enumerate() {
        let pts = project(&line.coords)?;
        out.line_lengths_m.push(clip::length(&pts));
        out.line_inside_any_m
            .push(inside_runs(&pts, &all_polys).iter().map(|r| r.length).sum());
        for (n, run) in inside_runs(&pts, &clpa_polys).into_iter().enumerate() {
            out.crossings.push(Crossing {
                id: format!(
                    "{}-{}",
                    line.osm_way_id
                        .map_or_else(|| format!("line{li}"), |w| w.to_string()),
                    n + 1
                ),
                line: li,
                coords: unproject(&run.coords)?,
                length_m: run.length,
                ddt05_indices: run.polygons.iter().map(|&i| clpa[i].index).collect(),
            });
        }
    }
    out.lines = kept;
    Ok(out)
}

/// Alert level of a crossing given the BERA maximum risk: `info` without bulletin or at
/// risk 1-2, `warning` at 3, `high` at 4-5.
pub fn alert_level(risk_max: Option<u8>) -> &'static str {
    match risk_max {
        Some(4..=5) => "high",
        Some(3) => "warning",
        _ => "info",
    }
}

/// Alert text of a crossing (French). Never says "safe".
pub fn alert_text(length_m: f64, bera: Option<&BeraContext>) -> String {
    let mut s = format!(
        "Ce tronçon traverse sur {:.0} m une zone d'avalanche connue (carte de localisation \
         des phénomènes d'avalanche, CLPA).",
        length_m
    );
    match bera {
        Some(BeraContext {
            massif: Some(m),
            risk_max: Some(r),
            valid_until,
            ..
        }) => s.push_str(&format!(
            " BERA {m} : risque {r} ({}), valable jusqu'au {}.",
            risk_label(*r).unwrap_or("?"),
            valid_until.as_deref().unwrap_or("?")
        )),
        _ => s.push_str(" Consultez le BERA du jour."),
    }
    s
}

fn bera_context(layout: &DataLayout, site: &Site) -> Result<Option<BeraContext>> {
    let path = layout
        .layer_dir(site, skimap_meteo::bera::LAYER)?
        .join("bera.json");
    if !path.exists() {
        return Ok(None);
    }
    let body = std::fs::read(&path).map_err(|e| skimap_common::Error::io(&path, e))?;
    let f: BeraFile = serde_json::from_slice(&body)
        .map_err(|e| skimap_common::Error::json(path.display().to_string(), e))?;
    if f.massif.is_none() {
        return Ok(None);
    }
    Ok(Some(BeraContext {
        massif: f.massif,
        risk_max: f.bulletin.as_ref().and_then(|b| b.risk.max),
        valid_until: f.bulletin.as_ref().and_then(|b| b.date_validity.clone()),
        date: f.date,
    }))
}

fn coords_json(c: &[LonLat]) -> Value {
    Value::Array(
        c.iter()
            .map(|p| json!([round(p.lon, 7), round(p.lat, 7)]))
            .collect(),
    )
}

/// Ring in WGS84 rounded to 7 decimals, oriented counter-clockwise (`ccw`) or clockwise,
/// as RFC 7946 requires; orientation is checked after rounding because slivers of a few
/// cm² may flip.
fn ring_json(ring_l93: &[(f64, f64)], ccw: bool) -> Result<Value> {
    let mut pts: Vec<(f64, f64)> = unproject(ring_l93)?
        .into_iter()
        .map(|p| (round(p.lon, 7), round(p.lat, 7)))
        .collect();
    if (clip::signed_area(&pts) > 0.0) != ccw {
        pts.reverse();
    }
    Ok(Value::Array(
        pts.into_iter().map(|(x, y)| json!([x, y])).collect(),
    ))
}

fn feature_collection(features: Vec<Value>) -> Value {
    json!({ "type": "FeatureCollection", "features": features })
}

/// Builds the layer from given piste lines (the OSM crate layer or an Overpass dump) and
/// writes it. `pistes_source` describes the lines in the summary, `source` is their
/// provenance in the metadata.
pub fn build_with_pistes(
    site: &Site,
    layout: &DataLayout,
    dl: &Downloader,
    lines: &[PisteLine],
    pistes_source: &str,
    source: SourceRef,
    personal_sources: bool,
) -> Result<Summary> {
    let dir = layout.layer_dir(site, LAYER)?;
    DataLayout::ensure(&dir)?;
    let generated_at = now_rfc3339();
    let in_ddt05 = site.department == DDT05_DEPARTMENT;
    let covered = in_ddt05 && personal_sources;
    let bera = bera_context(layout, site)?;

    let mut sources = Vec::new();
    let commune = read_commune(&dl.fetch(&commune_request(site))?.bytes()?)?;
    sources.push(SourceRef::new(
        "Contour de la commune (geo.api.gouv.fr)",
        commune_request(site).url(),
        "ODbL 1.0 (jeu data.gouv.fr « Contours administratifs » servi par l'API, d'après Admin Express de l'IGN et OpenStreetMap)",
        "Contours administratifs (data.gouv.fr, ODbL) via geo.api.gouv.fr : IGN Admin Express, © contributeurs OpenStreetMap",
    ));
    let emprises = if covered {
        let zip = dl.fetch(&ddt05_request())?;
        sources.push(SourceRef::new(
            "DDT 05, aléa avalanche dans les Hautes-Alpes (CLPA republiée)",
            DDT05_DATASET_URL,
            "Licence Ouverte 2.0 (droit de republication de la CLPA par la DDT non vérifié) ; CLPA d'INRAE réservée à un usage personnel : usage personnel tant que la DDT 05 n'a pas confirmé (docs/licences-donnees.md)",
            "DDT des Hautes-Alpes ; CLPA INRAE (ex-Cemagref)",
        )
        .personal_use_only());
        read_ddt05_zip(&zip.bytes()?)?
    } else {
        Vec::new()
    };
    sources.push(source);

    let (bx0, by0, bx1, by1) = crs::project_bbox(site.bbox, Crs::Lambert93, 16)?;
    let in_box = |e: &&Emprise| {
        let b = e.poly.bbox;
        b.0 <= bx1 && bx0 <= b.2 && b.1 <= by1 && by0 <= b.3
    };
    // Records of the site: intersecting the site box or in the site commune (a commune can
    // extend beyond the box).
    let all_in_box: Vec<&Emprise> = emprises
        .iter()
        .filter(|e| in_box(e) || e.attr("INSEE") == site.insee)
        .collect();
    let clpa: Vec<&Emprise> = all_in_box.iter().copied().filter(|e| e.is_clpa()).collect();
    let commune_records: Vec<&Emprise> = emprises
        .iter()
        .filter(|e| e.attr("INSEE") == site.insee)
        .collect();
    let clpa_commune: Vec<&&Emprise> = commune_records.iter().filter(|e| e.is_clpa()).collect();

    let computed = compute(lines, Some(&commune), &clpa, &all_in_box)?;

    // Emprises layer.
    let mut features = Vec::new();
    for e in &clpa {
        let mut polys = Vec::new();
        for (outer, holes) in e.poly.parts() {
            let mut rings = vec![ring_json(&outer, true)?];
            for h in &holes {
                rings.push(ring_json(h, false)?);
            }
            polys.push(Value::Array(rings));
        }
        let geometry = if polys.len() == 1 {
            json!({ "type": "Polygon", "coordinates": polys.pop() })
        } else {
            json!({ "type": "MultiPolygon", "coordinates": polys })
        };
        features.push(json!({
            "type": "Feature",
            "geometry": geometry,
            "properties": {
                "ddt05_index": e.index,
                "source": e.attr("SOURCE"),
                "risque": e.attr("RISQUE"),
                "information": e.attr("INFORMATIO"),
                "moa": e.attr("MOA"),
                "ref_saisie": e.attr("REF_SAISIE"),
                "insee": e.attr("INSEE"),
                "commune": e.attr("COMMUNE"),
                "area_m2": round(e.poly.area(), 1),
            }
        }));
    }
    write_json(&dir.join("emprises.geojson"), &feature_collection(features))?;

    // Crossings layer.
    let by_index: BTreeMap<usize, &Emprise> = clpa.iter().map(|e| (e.index, *e)).collect();
    let mut features = Vec::new();
    for c in &computed.crossings {
        let l = &computed.lines[c.line];
        let risque: BTreeSet<&str> = c
            .ddt05_indices
            .iter()
            .filter_map(|i| by_index.get(i).map(|e| e.attr("RISQUE")))
            .collect();
        features.push(json!({
            "type": "Feature",
            "geometry": { "type": "LineString", "coordinates": coords_json(&c.coords) },
            "properties": {
                "id": c.id,
                "osm_way_id": l.osm_way_id,
                "name": l.name,
                "relations": l.relations,
                "network": l.network,
                "length_m": round(c.length_m, 1),
                "ddt05_indices": c.ddt05_indices,
                "risque": risque.into_iter().collect::<Vec<_>>(),
                "alert_level": alert_level(bera.as_ref().and_then(|b| b.risk_max)),
                "alert": alert_text(c.length_m, bera.as_ref()),
            }
        }));
    }
    write_json(
        &dir.join("crossings.geojson"),
        &feature_collection(features),
    )?;

    // Totals.
    let mut networks = Vec::new();
    for net in [Network::Nordic, Network::Hike] {
        let idx: Vec<usize> = (0..computed.lines.len())
            .filter(|&i| computed.lines[i].network == net)
            .collect();
        let cr: Vec<&Crossing> = computed
            .crossings
            .iter()
            .filter(|c| computed.lines[c.line].network == net)
            .collect();
        networks.push(NetworkTotals {
            network: net,
            lines: idx.len(),
            length_km: round(
                idx.iter().map(|&i| computed.line_lengths_m[i]).sum::<f64>() / 1e3,
                3,
            ),
            inside_clpa_km: round(cr.iter().map(|c| c.length_m).sum::<f64>() / 1e3, 3),
            inside_any_ddt05_km: round(
                idx.iter()
                    .map(|&i| computed.line_inside_any_m[i])
                    .sum::<f64>()
                    / 1e3,
                3,
            ),
            crossings: cr.len(),
        });
    }
    let mut per: BTreeMap<(String, Network), (f64, f64, usize)> = BTreeMap::new();
    for (i, l) in computed.lines.iter().enumerate() {
        let e = per
            .entry((l.name.clone().unwrap_or_else(|| "?".into()), l.network))
            .or_default();
        e.0 += computed.line_lengths_m[i];
    }
    for c in &computed.crossings {
        let l = &computed.lines[c.line];
        let e = per
            .entry((l.name.clone().unwrap_or_else(|| "?".into()), l.network))
            .or_default();
        e.1 += c.length_m;
        e.2 += 1;
    }
    let mut pistes: Vec<PisteTotals> = per
        .into_iter()
        .map(|((name, network), (len, inside, n))| PisteTotals {
            name,
            network,
            length_km: round(len / 1e3, 3),
            inside_clpa_km: round(inside / 1e3, 3),
            crossings: n,
        })
        .collect();
    pistes.sort_by(|a, b| {
        b.inside_clpa_km
            .total_cmp(&a.inside_clpa_km)
            .then(b.length_km.total_cmp(&a.length_km))
    });

    let nordic = &networks[0];
    // Comparison with report 07 (section B4) when the report measured the site.
    let reconciliation = if let Some(r) = crate::report07::for_site(site) {
        r.clpa_km
            .iter()
            .map(|(item, v)| Reconciliation {
                item: item.clone(),
                report07: *v,
                recomputed: match item.as_str() {
                    "total" => nordic.length_km,
                    "inside" => nordic.inside_clpa_km,
                    name => pistes
                        .iter()
                        .find(|p| p.name == name && p.network == Network::Nordic)
                        .map_or(0.0, |p| p.inside_clpa_km),
                },
            })
            .collect()
    } else {
        Vec::new()
    };

    let notice = if in_ddt05 && !covered {
        "Emprises CLPA non incluses : la CLPA d'INRAE est réservée à un usage personnel et \
         le droit de la DDT 05 de la republier n'est pas confirmé (option personal_sources \
         désactivée). L'absence de zone affichée ne veut pas dire absence de danger."
            .to_owned()
    } else if !covered {
        "Pas de carte de localisation des phénomènes d'avalanche (CLPA) pour ce site : la \
         CLPA ne couvre que les Alpes et les Pyrénées, le Jura n'en a pas. L'absence de zone \
         connue ne veut pas dire absence de danger."
            .to_owned()
    } else {
        format!(
            "{} tronçons du réseau nordique ({:.1} km sur {:.1} km) traversent une zone \
             d'avalanche connue (CLPA). Zones historiques seulement : une avalanche peut se \
             produire ailleurs. Consultez le BERA.",
            nordic.crossings, nordic.inside_clpa_km, nordic.length_km
        )
    };

    let summary = Summary {
        site: site.id.to_owned(),
        covered,
        clpa_in_box: clpa.len(),
        clpa_in_commune: clpa_commune.len(),
        other_in_commune: commune_records.len() - clpa_commune.len(),
        clpa_in_commune_km2: round(
            clpa_commune.iter().map(|e| e.poly.area()).sum::<f64>() / 1e6,
            3,
        ),
        clpa_in_commune_slivers: clpa_commune
            .iter()
            .filter(|e| e.poly.area() < SLIVER_AREA_M2)
            .count(),
        pistes_source: pistes_source.to_owned(),
        site_filter: format!(
            "lignes dont la moyenne des sommets est dans la commune INSEE {}",
            site.insee
        ),
        networks,
        pistes,
        reconciliation,
        bera,
        notice,
    };
    write_json(&dir.join("summary.json"), &summary)?;
    LayerMeta {
        tickets: vec!["P20 (#99)".to_owned()],
        generated_at,
        crs: Some("EPSG:4326 (WGS84) ; longueurs et surfaces en Lambert-93 (EPSG:2154)".into()),
        sources,
        files: vec![
            FileMeta::new(
                "emprises.geojson",
                "emprises CLPA (WGS84), propriétés de la couche DDT 05",
            ),
            FileMeta::new(
                "crossings.geojson",
                "tronçons de pistes dans une emprise (WGS84), longueur en m (Lambert-93), \
                 niveau et texte d'alerte",
            ),
            FileMeta::new(
                "summary.json",
                "totaux par réseau et par piste, comparaison au rapport 07, BERA",
            ),
        ],
        notes: vec![
            "Zones historiques connues seulement ; jamais une garantie d'absence de danger.".into(),
            "Longueurs euclidiennes en Lambert-93, comme le calcul de référence du rapport 07."
                .into(),
        ],
        ..LayerMeta::new(LAYER, site.id)
    }
    .write(&dir)?;
    Ok(summary)
}

/// Builds the layer with the piste lines of the site's `pistes` layer (OSM crate, P2),
/// which must have been built (`skimap pistes <site>`); the CLPA only with
/// `personal_sources` of the environment configuration (`SKIMAP_PERSONAL_SOURCES`, the
/// configuration of the command line), see [`build_with`].
pub fn build(site: &Site, layout: &DataLayout, dl: &Downloader) -> Result<Summary> {
    let personal = skimap_common::config::Config::from_env()?.personal_sources;
    build_with(site, layout, dl, personal)
}

/// [`build`] with an explicit `personal_sources` (CLPA footprints read only with it).
pub fn build_with(
    site: &Site,
    layout: &DataLayout,
    dl: &Downloader,
    personal_sources: bool,
) -> Result<Summary> {
    let dir = layout.layer_dir(site, PISTES_LAYER)?;
    let path = dir.join(PISTES_FILE);
    if !path.exists() {
        return Err(Error::MissingLayer {
            layer: PISTES_LAYER.to_owned(),
            site: site.id.to_owned(),
            path,
        });
    }
    let body = std::fs::read(&path).map_err(|e| skimap_common::Error::io(&path, e))?;
    let lines = pistes::from_pistes_geojson(&body)
        .map_err(|e| Error::format(path.display().to_string(), e.to_string()))?;
    // Provenance of the OSM data: the sources of the pistes layer (OpenSnowMap extract).
    let meta = LayerMeta::read(&dir.join(LayerMeta::file_name(PISTES_LAYER)))?;
    let osm = meta.sources.first().ok_or_else(|| {
        Error::format(PISTES_LAYER, "pistes.meta.json lists no source".to_owned())
    })?;
    let data_date = meta.valid_from.clone().unwrap_or_else(|| "?".to_owned());
    let mut source = osm.clone();
    source.name = format!(
        "Pistes et itinéraires OSM, couche pistes (P2) : {}",
        osm.name
    );
    source.edition = Some(format!("données OSM du {data_date}"));
    let description = format!("couche pistes (P2), données OSM du {data_date}");
    build_with_pistes(
        site,
        layout,
        dl,
        &lines,
        &description,
        source,
        personal_sources,
    )
}

/// Builds the layer from a saved Overpass JSON dump (e.g. `research/raw/osm/cerv2.json`,
/// the extract of report 07).
pub fn build_from_dump(
    site: &Site,
    layout: &DataLayout,
    dl: &Downloader,
    dump: &Path,
) -> Result<Summary> {
    let personal = skimap_common::config::Config::from_env()?.personal_sources;
    build_from_dump_with(site, layout, dl, dump, personal)
}

/// [`build_from_dump`] with an explicit `personal_sources` (CLPA footprints read only with
/// it).
pub fn build_from_dump_with(
    site: &Site,
    layout: &DataLayout,
    dl: &Downloader,
    dump: &Path,
    personal_sources: bool,
) -> Result<Summary> {
    let body = std::fs::read(dump).map_err(|e| skimap_common::Error::io(dump, e))?;
    let lines = pistes::from_overpass_json(&body)
        .map_err(|e| Error::format("overpass dump", e.to_string()))?;
    let mut source = SourceRef::new(
        format!(
            "Pistes et itinéraires OSM (extrait Overpass {})",
            dump.display()
        ),
        OVERPASS_URL,
        "ODbL 1.0",
        "© les contributeurs d'OpenStreetMap",
    );
    source.edition = std::fs::metadata(dump)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .and_then(|d| unix_to_rfc3339(d.as_secs() as i64));
    build_with_pistes(
        site,
        layout,
        dl,
        &lines,
        &format!("extrait {}", dump.display()),
        source,
        personal_sources,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn alert_wording() {
        let b = BeraContext {
            massif: Some("Queyras".into()),
            risk_max: Some(3),
            valid_until: Some("2027-01-10T18:00:00".into()),
            date: "2027-01-10".into(),
        };
        let t = alert_text(1384.4, Some(&b));
        assert!(t.contains("1384 m"));
        assert!(t.contains("risque 3 (marqué)"));
        assert!(!t.to_lowercase().contains("sûr") && !t.contains("sans danger"));
        assert!(alert_text(10.0, None).contains("Consultez le BERA"));
        assert_eq!(alert_level(Some(3)), "warning");
        assert_eq!(alert_level(Some(5)), "high");
        assert_eq!(alert_level(None), "info");
    }

    #[test]
    fn compute_on_a_synthetic_case() {
        // A 1 km x 1 km footprint around (6.75 E, 44.85 N) and a north-south line through it.
        let (cx, cy) = crs::to_lambert93(LonLat::new(6.75, 44.85)).unwrap();
        let sq = vec![
            (cx - 500.0, cy - 500.0),
            (cx - 500.0, cy + 500.0),
            (cx + 500.0, cy + 500.0),
            (cx + 500.0, cy - 500.0),
            (cx - 500.0, cy - 500.0),
        ];
        let mut attrs = BTreeMap::new();
        attrs.insert("SOURCE".to_owned(), "CLPA".to_owned());
        let e = Emprise {
            index: 7,
            attrs,
            poly: Poly::new(vec![sq]),
        };
        let a = crs::from_lambert93(cx, cy - 2000.0).unwrap();
        let b = crs::from_lambert93(cx, cy + 2000.0).unwrap();
        let line = PisteLine {
            osm_way_id: Some(42),
            name: Some("Test".into()),
            relations: vec![],
            network: Network::Nordic,
            piste_type: "nordic".into(),
            coords: vec![a, b],
        };
        let c = compute(std::slice::from_ref(&line), None, &[&e], &[&e]).unwrap();
        assert_eq!(c.crossings.len(), 1);
        assert!((c.crossings[0].length_m - 1000.0).abs() < 1e-6);
        assert!((c.line_lengths_m[0] - 4000.0).abs() < 1e-6);
        assert_eq!(c.crossings[0].ddt05_indices, [7]);
        assert_eq!(c.crossings[0].id, "42-1");
        // A commune that does not contain the line removes it.
        let far = Poly::new(vec![vec![(0.0, 0.0), (0.0, 1.0), (1.0, 1.0), (1.0, 0.0)]]);
        assert!(
            compute(&[line], Some(&far), &[&e], &[&e])
                .unwrap()
                .lines
                .is_empty()
        );
    }
}
