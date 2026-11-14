//! Areas with no mobile network at all, not even for 112 (ARCEP).
//!
//! Owned ticket: P12 (#25). The 112 can be routed by any operator's antenna, so the
//! places where a call is impossible are those covered by none of the four networks:
//! the French part of the site box minus the union of the 2G/3G voice coverage of Orange,
//! SFR, Free and Bouygues (all levels: very good, good, limited). Recomputed at every
//! release of the maps (2G/3G voice: twice a year).
//!
//! Pipeline: [`build`] reads the ARCEP GeoPackages with `rusqlite` (R-tree spatial
//! index, own WKB parser, no GDAL), computes in Lambert-93 (EPSG:2154, the CRS of the
//! files) and writes `data/out/<site>/coverage/`:
//!
//! - `no_network.geojson`: one polygon per feature, WGS84, properties `kind`
//!   (`no_voice_network_any_operator`), `area_ha` (Lambert-93 planar), `release`;
//! - `analysed_area.geojson`: the French part of the box, where the answer is known
//!   (outside it, i.e. in Italy or Switzerland, nothing is known);
//! - `coverage.meta.json`: sources, licence, dates, per-operator shares, timings.

mod arcep;
mod error;
mod gpkg;
mod wkb;

use std::path::PathBuf;
use std::time::{Duration, Instant};

use geo::{Area, BooleanOps, BoundingRect, Densify, Euclidean, MultiPolygon, Polygon, Rect};
use geojson::{Feature, FeatureCollection, GeometryValue};
use rayon::prelude::*;
use serde::Serialize;
use skimap_common::crs::Crs;
use skimap_common::download::Downloader;
use skimap_common::layout::DataLayout;
use skimap_common::meta::{FileMeta, LayerMeta, SourceRef};
use skimap_common::output::{write_json, write_text};
use skimap_common::site::Site;
use skimap_zones::geom::{reproject, union_all};
use skimap_zones::layer::round_multipolygon;
use skimap_zones::sources::jurisdiction::france_part;

pub use arcep::{Operator, Release};
pub use error::{Error, Result};
pub use gpkg::GeoPackage;

/// Layer (directory) name.
pub const LAYER: &str = "coverage";

/// Options of [`build`].
#[derive(Debug, Clone)]
pub struct CoverageOptions {
    /// Release of the maps.
    pub release: Release,
    /// Directory holding already downloaded archives (e.g. `research/raw/arcep/dl`);
    /// missing archives are downloaded through the shared downloader.
    pub archive_dir: Option<PathBuf>,
}

impl Default for CoverageOptions {
    fn default() -> Self {
        Self {
            release: Release::LATEST_KNOWN,
            archive_dir: None,
        }
    }
}

/// Figures of one operator.
#[derive(Debug, Clone, Serialize)]
pub struct OperatorStats {
    /// Operator name.
    pub operator: String,
    /// GeoPackage features read (département × level).
    pub features: usize,
    /// Polygons whose box touches the analysed area.
    pub polygons_near: usize,
    /// Covered part of the analysed area, km².
    pub covered_km2: f64,
    /// Covered share of the analysed area, %.
    pub covered_pct: f64,
    /// Extraction time of the archive, s (`None`: extraction reused).
    pub extract_s: Option<f64>,
    /// Reading and parsing time, s.
    pub read_s: f64,
    /// Clipping and per-operator union time, s.
    pub geometry_s: f64,
}

/// Wall-clock times of a build, s.
#[derive(Debug, Clone, Serialize)]
pub struct Timings {
    /// French part of the box (commune outlines).
    pub analysed_area_s: f64,
    /// The four operators, in parallel (extraction, reading, clipping).
    pub operators_s: f64,
    /// Union of the four operators and difference with the area.
    pub union_difference_s: f64,
    /// Writing the outputs.
    pub write_s: f64,
    /// Whole build.
    pub total_s: f64,
}

/// Summary of a build.
#[derive(Debug, Clone, Serialize)]
pub struct CoverageReport {
    /// Site identifier.
    pub site: String,
    /// Release of the maps.
    pub release: String,
    /// Output directory.
    pub dir: PathBuf,
    /// French part of the site box, km² (Lambert-93 planar).
    pub analysed_km2: f64,
    /// Per-operator figures.
    pub operators: Vec<OperatorStats>,
    /// Covered by at least one operator, km².
    pub any_operator_km2: f64,
    /// Covered by at least one operator, %.
    pub any_operator_pct: f64,
    /// No network at all, km².
    pub no_network_km2: f64,
    /// No network at all, %.
    pub no_network_pct: f64,
    /// Number of no-network polygons.
    pub no_network_polygons: usize,
    /// Size of `no_network.geojson`, bytes.
    pub no_network_bytes: usize,
    /// Timings.
    pub timings: Timings,
}

fn secs(d: Duration) -> f64 {
    (d.as_secs_f64() * 1000.0).round() / 1000.0
}

fn km2(mp: &MultiPolygon<f64>) -> f64 {
    mp.unsigned_area() / 1e6
}

fn overlaps(a: &Rect<f64>, b: &Rect<f64>) -> bool {
    a.min().x <= b.max().x
        && b.min().x <= a.max().x
        && a.min().y <= b.max().y
        && b.min().y <= a.max().y
}

/// Coverage of one operator over the area (Lambert-93).
fn operator_coverage(
    dl: &Downloader,
    layout: &DataLayout,
    opts: &CoverageOptions,
    op: Operator,
    area: &MultiPolygon<f64>,
    rect: Rect<f64>,
) -> Result<(MultiPolygon<f64>, OperatorStats)> {
    let archive = arcep::archive(dl, opts.release, op, opts.archive_dir.as_deref())?;
    let dest = layout.cache_dir("arcep")?.join("gpkg");
    let (path, extract_t) = arcep::extract(&archive, &opts.release.gpkg_name(op), &dest)?;
    let t0 = Instant::now();
    let g = GeoPackage::open(&path)?;
    if g.epsg() != Some(2154) {
        return Err(Error::Gpkg {
            path,
            message: format!("expected EPSG:2154, found {:?}", g.epsg()),
        });
    }
    let feats = g.features_in(rect, "niveau")?;
    let read_t = t0.elapsed();
    let t1 = Instant::now();
    let mut near = 0;
    let pieces: Vec<MultiPolygon<f64>> = feats
        .par_iter()
        .map(|(_, mp)| {
            let polys: Vec<Polygon<f64>> =
                mp.0.iter()
                    .filter(|p| p.bounding_rect().is_some_and(|r| overlaps(&r, &rect)))
                    .cloned()
                    .collect();
            let n = polys.len();
            (n, MultiPolygon::new(polys).intersection(area))
        })
        .collect::<Vec<_>>()
        .into_iter()
        .map(|(n, mp)| {
            near += n;
            mp
        })
        .collect();
    let covered = union_all(pieces.iter());
    let stats = OperatorStats {
        operator: op.name().into(),
        features: feats.len(),
        polygons_near: near,
        covered_km2: km2(&covered),
        covered_pct: 0.0,
        extract_s: extract_t.map(secs),
        read_s: secs(read_t),
        geometry_s: secs(t1.elapsed()),
    };
    tracing::info!(
        operator = op.name(),
        features = feats.len(),
        near,
        km2 = stats.covered_km2,
        "operator coverage"
    );
    Ok((covered, stats))
}

fn feature(mp: &MultiPolygon<f64>, props: serde_json::Value) -> Feature {
    let props = match props {
        serde_json::Value::Object(m) => m,
        _ => unreachable!("object literal"),
    };
    Feature {
        geometry: Some(geojson::Geometry::new(GeometryValue::from(
            &round_multipolygon(mp, 1e6),
        ))),
        properties: Some(props),
        ..Default::default()
    }
}

fn to_geojson(fc: FeatureCollection) -> Result<String> {
    serde_json::to_string(&fc).map_err(|e| skimap_common::Error::json("coverage GeoJSON", e).into())
}

/// Computes the no-network areas of `site` and writes the `coverage` layer.
pub fn build(
    site: &Site,
    layout: &DataLayout,
    dl: &Downloader,
    opts: &CoverageOptions,
) -> Result<CoverageReport> {
    let t_start = Instant::now();
    let release = opts.release;

    // French part of the box, densified (~50 m) so that the box edges, straight in
    // longitude / latitude, stay exact once projected.
    let fr = france_part(dl, site.bbox)?;
    let fr = Euclidean.densify(&fr, 0.0005);
    let area = reproject(&fr, Crs::Wgs84, Crs::Lambert93)?;
    let rect = area
        .bounding_rect()
        .ok_or_else(|| Error::Wkb("empty French part of the site box".into()))?;
    let analysed_km2 = km2(&area);
    let t_area = t_start.elapsed();

    let t1 = Instant::now();
    let results: Vec<(MultiPolygon<f64>, OperatorStats)> = Operator::ALL
        .par_iter()
        .map(|&op| operator_coverage(dl, layout, opts, op, &area, rect))
        .collect::<Result<_>>()?;
    let t_ops = t1.elapsed();

    let t2 = Instant::now();
    let any = union_all(results.iter().map(|r| &r.0));
    let none = area.difference(&any);
    let t_union = t2.elapsed();

    let t3 = Instant::now();
    let mut operators: Vec<OperatorStats> = results.into_iter().map(|r| r.1).collect();
    for o in &mut operators {
        o.covered_pct = round2(o.covered_km2 / analysed_km2 * 100.0);
        o.covered_km2 = round2(o.covered_km2);
    }
    let any_km2 = km2(&any);
    let none_km2 = km2(&none);

    let none_wgs = reproject(&none, Crs::Lambert93, Crs::Wgs84)?;
    let features: Vec<Feature> = none
        .0
        .iter()
        .zip(&none_wgs.0)
        .map(|(l93, wgs)| {
            feature(
                &MultiPolygon::new(vec![wgs.clone()]),
                serde_json::json!({
                    "kind": "no_voice_network_any_operator",
                    "area_ha": round2(l93.unsigned_area() / 1e4),
                    "release": release.tag(),
                }),
            )
        })
        .collect();
    let n_polys = features.len();
    let none_json = to_geojson(FeatureCollection::new(features))?;
    let area_wgs = reproject(&area, Crs::Lambert93, Crs::Wgs84)?;
    let area_json = to_geojson(FeatureCollection::new([feature(
        &area_wgs,
        serde_json::json!({ "kind": "analysed_area", "area_km2": round2(analysed_km2) }),
    )]))?;
    let dir = layout.layer_dir(site, LAYER)?;
    write_text(&dir.join("no_network.geojson"), &none_json)?;
    write_text(&dir.join("analysed_area.geojson"), &area_json)?;
    let t_write = t3.elapsed();

    let report = CoverageReport {
        site: site.id.into(),
        release: release.tag(),
        dir: dir.clone(),
        analysed_km2: round2(analysed_km2),
        operators,
        any_operator_km2: round2(any_km2),
        any_operator_pct: round2(any_km2 / analysed_km2 * 100.0),
        no_network_km2: round2(none_km2),
        no_network_pct: round2(none_km2 / analysed_km2 * 100.0),
        no_network_polygons: n_polys,
        no_network_bytes: none_json.len(),
        timings: Timings {
            analysed_area_s: secs(t_area),
            operators_s: secs(t_ops),
            union_difference_s: secs(t_union),
            write_s: secs(t_write),
            total_s: secs(t_start.elapsed()),
        },
    };
    layer_meta(site, &report, opts).write(&dir)?;
    write_json(&dir.join("build_report.json"), &report)?;
    Ok(report)
}

fn round2(v: f64) -> f64 {
    (v * 100.0).round() / 100.0
}

fn layer_meta(site: &Site, r: &CoverageReport, opts: &CoverageOptions) -> LayerMeta {
    let rel = opts.release;
    LayerMeta {
        tickets: vec!["P12 (#25)".into()],
        validity: Some(format!(
            "Cartes 2G/3G voix ARCEP {} (situation au {}). Publication semestrielle (T2 et T4) : régénérer à chaque nouvelle publication.",
            rel.tag(),
            rel.quarter_end()
        )),
        crs: Some("EPSG:4326 (longitude, latitude), coordonnées arrondies à 1e-6° ; calculs en Lambert-93 (EPSG:2154)".into()),
        files: vec![
            FileMeta::new("no_network.geojson", "zones sans aucun réseau voix, même pour le 112 (aucun des quatre opérateurs)").count(r.no_network_polygons).properties([("kind", "no_voice_network_any_operator"), ("area_ha", "surface du polygone, Lambert-93 plan, hectares"), ("release", "publication ARCEP, ex. 2026_T2")]),
            FileMeta::new("analysed_area.geojson", "partie française de l'emprise : hors de ce contour, la couverture est inconnue").count(1).properties([("kind", "analysed_area"), ("area_km2", "surface analysée, Lambert-93 plan")]),
            FileMeta::new("build_report.json", "chiffres et temps de calcul"),
        ],
        sources: vec![
            SourceRef::new("ARCEP, Mon Réseau Mobile, couvertures théoriques 2G/3G voix", Operator::ALL.iter().map(|o| rel.url(*o)).collect::<Vec<_>>().join(" "), "Licence Ouverte (data.gouv.fr, jeu « Mon Réseau Mobile », fr-lo)", "Arcep, Mon Réseau Mobile")
                .edition(rel.quarter_end())
                .note("simulations des opérateurs ; résolution non documentée ; Free : pas de carte 2G depuis 2024 T4"),
            SourceRef::new("geo.api.gouv.fr, contours des communes (partie française de l'emprise)", format!("{}/departements/<code>/communes?format=geojson&geometry=contour", skimap_zones::sources::jurisdiction::API), "non vérifiée (F5, #11)", "geo.api.gouv.fr (Etalab, IGN)"),
        ],
        notes: vec![
            "Le 112 peut passer par l'antenne de n'importe quel opérateur : seule l'union des quatre réseaux compte (réponse ministérielle QE 4594, JO du 20/01/2026).".into(),
            "Niveaux TBC, BC et CL tous comptés comme couverts ; CL = « à l'extérieur des bâtiments dans la plupart des cas ».".into(),
            "Cartes théoriques : une zone couverte peut ne pas l'être en forêt ou en fond de combe.".into(),
            "Hors de la partie française de l'emprise (Italie, Suisse), aucune information.".into(),
        ],
        stats: serde_json::to_value(r).expect("report serialises"),
        ..LayerMeta::new(LAYER, site.id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crate_name() {
        assert_eq!(env!("CARGO_PKG_NAME"), "skimap-coverage");
    }

    /// Union and difference on synthetic coverage: area 10 x 10 km, operator A covers
    /// the west half, operator B a 2 x 10 km strip overlapping it and a 1 x 1 km square
    /// in the east. No network: 10 x 10 − 5 x 10 − 1 x 1 = 49 km².
    #[test]
    fn union_and_difference() {
        let sq = |x: f64, y: f64, w: f64, h: f64| {
            MultiPolygon::new(vec![
                geo::Rect::new(
                    geo::coord! { x: x, y: y },
                    geo::coord! { x: x + w, y: y + h },
                )
                .to_polygon(),
            ])
        };
        let area = sq(0.0, 0.0, 10_000.0, 10_000.0);
        let a = sq(-1000.0, 0.0, 6000.0, 10_000.0).intersection(&area);
        let b = union_all([
            &sq(3000.0, 0.0, 2000.0, 10_000.0),
            &sq(8000.0, 8000.0, 1000.0, 1000.0),
        ])
        .intersection(&area);
        let any = union_all([&a, &b]);
        let none = area.difference(&any);
        assert!((km2(&any) - 51.0).abs() < 1e-9, "{}", km2(&any));
        assert!((km2(&none) - 49.0).abs() < 1e-9, "{}", km2(&none));
        assert_eq!(none.0.len(), 1);
        assert_eq!(none.0[0].interiors().len(), 1);
    }
}
