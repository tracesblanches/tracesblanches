//! The `avalanche-exposure` layer of a site: rasters, piste runs, CLPA validation,
//! summary and metadata.

use std::path::Path;
use std::time::Instant;

use serde::Serialize;
use serde_json::{Value, json};
use skimap_common::download::Downloader;
use skimap_common::geom::BBox;
use skimap_common::layout::DataLayout;
use skimap_common::meta::{FileMeta, LayerMeta, SourceRef};
use skimap_common::site::Site;
use tracing::info;

use super::raster::{self, Raster};
use super::validation::{self, ClpaValidation};
use super::{
    LAYER, LOW_MARGIN_M, LayerModel, Timings, class, flowpy, inputs_5m, layer_model, pra, segments,
};
use crate::error::{Error, Result};
use crate::historic::{
    self,
    pistes::{Network, PisteLine},
};

/// File names of the layer.
pub mod files {
    /// User classes 1–3 (UInt8, 255 = no data), on the 5 m terrain grid.
    pub const EXPOSURE: &str = "exposure_5m.tif";
    /// ATES-like classes 0–4 of the dev8 port (UInt8, 255 = no data), diagnostic.
    pub const ATES: &str = "ates_5m.tif";
    /// Model classes before the CLPA fusion (UInt8), written where the fusion changed cells.
    pub const EXPOSURE_MODEL: &str = "exposure_model_5m.tif";
    /// Canopy cover from the LiDAR HD MNH (Float32, %).
    pub const CANOPY: &str = "canopy_5m.tif";
    /// Runs of constant exposure along the piste network (GeoJSON, WGS84).
    pub const SEGMENTS: &str = "segments.geojson";
    /// Key figures.
    pub const SUMMARY: &str = "summary.json";
}

/// Disclaimers shown with the layer (French). Never « sûr », never « sans danger ».
pub const DISCLAIMERS: &[&str] = &[
    "Calcul automatique, non vérifié sur le terrain, indépendant de la neige du jour : consultez le BERA du massif avant de partir (il n'y a pas de BERA dans le Jura).",
    "« Terrain peu exposé (modèle) » : aucune avalanche modélisée, même dans le scénario rare (angle d'arrêt de 18°), n'atteint ce terrain à 10 m près, d'après le relief LiDAR et la forêt LiDAR de l'IGN. Ce n'est ni une garantie ni une absence de danger.",
    "Les pièges de terrain (ravines, ruisseaux, bords de route), les corniches, les avalanches venues de pentes hors de l'emprise calculée et les coupes de forêt postérieures au vol LiDAR ne sont pas pris en compte.",
    "Méthode : portage de la recherche AutoATES v3.0 (ISSW 2026), que ses auteurs présentent comme « a research/tutorial product, not an operational ATES map ».",
];

/// Length of the piste network per class, km.
#[derive(Debug, Clone, Default, Serialize)]
pub struct NetworkExposure {
    /// Lines.
    pub lines: usize,
    /// Total length, km.
    pub length_km: f64,
    /// « terrain peu exposé (modèle) », km.
    pub low_km: f64,
    /// « exposé », km.
    pub exposed_km: f64,
    /// « très exposé », km.
    pub high_km: f64,
    /// Without data, km.
    pub no_data_km: f64,
    /// Share of the length not « peu exposé » (exposé + très exposé + no data).
    pub not_low_share: f64,
}

fn network_exposure(ex: &[&segments::LineExposure<'_>]) -> NetworkExposure {
    let by = segments::length_by_class(ex.iter().flat_map(|l| l.runs.iter()));
    let total: f64 = by.iter().sum();
    NetworkExposure {
        lines: ex.len(),
        length_km: total / 1000.0,
        low_km: by[1] / 1000.0,
        exposed_km: by[2] / 1000.0,
        high_km: by[3] / 1000.0,
        no_data_km: by[0] / 1000.0,
        not_low_share: if total > 0.0 {
            1.0 - by[1] / total
        } else {
            0.0
        },
    }
}

/// Comparison with the geometric pre-filter of report 07 (section B3).
#[derive(Debug, Clone, Serialize)]
pub struct Report07 {
    /// Box of the report computation (south, west, north, east).
    pub bbox: [f64; 4],
    /// Report: share of the nordic piste points reachable at α = 18° (cone, no forest).
    pub reachable_alpha18: f64,
    /// Report: same at α = 30°.
    pub reachable_alpha30: f64,
    /// Ours: nordic lines whose vertex mean is in the box.
    pub ours: NetworkExposure,
}

/// Summary of a build.
#[derive(Debug, Clone, Serialize)]
pub struct ExposureSummary {
    /// Site.
    pub site: String,
    /// Grid size (columns, rows) and cell size, m.
    pub grid: (usize, usize, f64),
    /// Cells with a class.
    pub valid_cells: u64,
    /// Share of the valid cells per class (peu exposé, exposé, très exposé).
    pub class_shares: [f64; 3],
    /// Share of the valid cells per ATES-like class 0–4.
    pub ates_shares: [f64; 5],
    /// Release cells and paths routed (typical, infrequent).
    pub paths: (usize, usize),
    /// Every line of the pistes layer.
    pub all_lines: NetworkExposure,
    /// Nordic lines whose vertex mean is in the commune (rule of `historic`).
    pub nordic_in_commune: NetworkExposure,
    /// Comparison with report 07.
    pub report07: Option<Report07>,
    /// CLPA validation of the model alone (Hautes-Alpes only): the P18 measure.
    pub clpa_model: Option<ClpaValidation>,
    /// Same after fusing the CLPA footprints into the user classes (0 « peu exposé » by
    /// construction inside the footprints).
    pub clpa_fused: Option<ClpaValidation>,
    /// Cells raised from « peu exposé » to « exposé » by the CLPA fusion.
    pub clpa_cells_raised: u64,
    /// Model timings, s.
    pub timings: Timings,
    /// Input (MNH download or cache) and output timings, s.
    pub io_seconds: (f64, f64),
    /// Disclaimers.
    pub disclaimers: Vec<String>,
}

/// Report 07 pre-filter figures of a site: box and shares at α = 18° and 30°
/// (`data/report07.json`; `None` for a site the report did not measure).
pub fn report07_prefilter(site: &Site) -> Option<(BBox, f64, f64)> {
    crate::report07::for_site(site)
        .and_then(|r| r.prefilter)
        .map(|p| (p.bbox, p.alpha18, p.alpha30))
}

fn shares<const K: usize>(data: &[u8], codes: [u8; K]) -> (u64, [f64; K]) {
    let mut counts = [0u64; K];
    for v in data {
        if let Some(k) = codes.iter().position(|c| c == v) {
            counts[k] += 1;
        }
    }
    let n: u64 = counts.iter().sum();
    (
        n,
        counts.map(|c| if n == 0 { 0.0 } else { c as f64 / n as f64 }),
    )
}

fn read_pistes(layout: &DataLayout, site: &Site) -> Result<(Vec<PisteLine>, SourceRef)> {
    let dir = layout.layer_dir(site, historic::PISTES_LAYER)?;
    let path = dir.join(historic::PISTES_FILE);
    if !path.exists() {
        return Err(Error::MissingLayer {
            layer: historic::PISTES_LAYER.into(),
            site: site.id.into(),
            path,
        });
    }
    let body = std::fs::read(&path).map_err(|e| Error::io(&path, e))?;
    let lines = historic::pistes::from_pistes_geojson(&body)
        .map_err(|e| Error::format(path.display().to_string(), e.to_string()))?;
    let meta = LayerMeta::read(&dir.join(LayerMeta::file_name(historic::PISTES_LAYER)))?;
    let mut src =
        meta.sources.first().cloned().ok_or_else(|| {
            Error::format(historic::PISTES_LAYER, "pistes.meta.json lists no source")
        })?;
    src.name = format!(
        "Pistes et itinéraires OSM, couche pistes (P2) : {}",
        src.name
    );
    if let Some(d) = meta.valid_from {
        src.edition = Some(format!("données OSM du {d}"));
    }
    Ok((lines, src))
}

fn tif_meta(
    name: &str,
    description: &str,
    unit: &str,
    r: &Raster<u8>,
    bytes: u64,
    nodata: &str,
) -> FileMeta {
    FileMeta {
        unit: Some(unit.into()),
        format: Some("GeoTIFF UInt8, Deflate".into()),
        crs: Some("EPSG:2154".into()),
        cell_size: Some(r.cell),
        size_cells: Some([r.w, r.h]),
        nodata: Some(nodata.into()),
        bytes: Some(bytes),
        ..FileMeta::new(name, description)
    }
}

/// Builds the layer of a site: needs the `terrain` and `pistes` layers; downloads the
/// LiDAR HD MNH and, in the Hautes-Alpes with `personal_sources` of the environment
/// configuration (`SKIMAP_PERSONAL_SOURCES`, as the command line), the DDT 05 CLPA
/// through the shared downloader. See [`build_with`].
pub fn build(site: &Site, layout: &DataLayout, dl: &Downloader) -> Result<ExposureSummary> {
    let personal = skimap_common::config::Config::from_env()?.personal_sources;
    build_with(site, layout, dl, personal)
}

/// [`build`] with an explicit `personal_sources`. The CLPA is personal use only (INRAE;
/// the DDT 05's right to republish it is not confirmed, docs/licences-donnees.md):
/// without the option the layer is the model alone, no CLPA is read, fused nor
/// validated against; with it the CLPA footprints are fused (never « peu exposé » inside
/// a known footprint) and the layer is marked personal use only by its CLPA source.
pub fn build_with(
    site: &Site,
    layout: &DataLayout,
    dl: &Downloader,
    personal_sources: bool,
) -> Result<ExposureSummary> {
    let dir = layout.layer_dir(site, LAYER)?;
    DataLayout::ensure(&dir)?;
    let (lines, pistes_source) = read_pistes(layout, site)?;
    let t0 = Instant::now();
    let inputs = inputs_5m(site, layout, dl)?;
    let t_in = t0.elapsed().as_secs_f64();
    let m = layer_model(&inputs);
    info!(site = site.id, s = m.timings.total, "exposure model done");
    let t0 = Instant::now();
    let summary = write_layer(
        site,
        dl,
        &dir,
        &inputs.canopy,
        &m,
        &lines,
        pistes_source,
        t_in,
        personal_sources,
    )?;
    let mut summary = summary;
    summary.io_seconds.1 = t0.elapsed().as_secs_f64();
    skimap_common::output::write_json(&dir.join(files::SUMMARY), &summary)?;
    Ok(summary)
}

#[allow(clippy::too_many_arguments)]
fn write_layer(
    site: &Site,
    dl: &Downloader,
    dir: &Path,
    canopy: &Raster<f32>,
    m: &LayerModel,
    lines: &[PisteLine],
    pistes_source: SourceRef,
    t_in: f64,
    personal_sources: bool,
) -> Result<ExposureSummary> {
    // Validation of the model alone against the CLPA (P18), then the CLPA footprints are
    // fused into the user classes (never « peu exposé » inside a known footprint).
    let commune = historic::read_commune(&dl.fetch(&historic::commune_request(site))?.bytes()?)?;
    let in_commune = |l: &PisteLine| {
        let v = l.vertex_mean();
        commune.contains((v.lon, v.lat))
    };
    let nordic: Vec<&PisteLine> = lines
        .iter()
        .filter(|l| l.network == Network::Nordic && in_commune(l))
        .collect();
    let all: Vec<&PisteLine> = lines.iter().collect();
    let mut exposure = m.exposure.clone();
    let mut clpa_source = None;
    let footprints = if personal_sources {
        validation::clpa_footprints(site, dl)?
    } else {
        None
    };
    let (clpa_model, clpa_fused, clpa_raised) = match footprints {
        Some((polys, src)) => {
            clpa_source = Some(src.personal_use_only());
            // Footprint cells rasterized once for both validations and the fusion; the
            // validation of the model alone runs next to the fusion and its validation.
            let refs: Vec<_> = polys.iter().collect();
            let inside = validation::rasterize(&exposure, &refs);
            let (before, (raised, after)) = rayon::join(
                || validation::validate(m, &m.exposure, &polys, &inside, &nordic, &all),
                || {
                    let raised = validation::fuse_clpa(&mut exposure, &inside);
                    let after = validation::validate(m, &exposure, &polys, &inside, &nordic, &all);
                    (raised, after)
                },
            );
            (Some(before?), Some(after?), raised)
        }
        None => (None, None, 0),
    };

    // The model alone, kept for the diagnostic only when the fusion changed cells; a
    // previous fused build never lingers.
    let model_path = dir.join(files::EXPOSURE_MODEL);
    if clpa_raised == 0 && model_path.exists() {
        std::fs::remove_file(&model_path).map_err(|e| skimap_common::Error::io(&model_path, e))?;
    }
    // The rasters (Deflate, one thread each) and the piste runs are independent: written
    // in parallel, the bytes of every file are those of a sequential build.
    let ((exposure_bytes, model_bytes), (ates_bytes, (canopy_bytes, runs))) = rayon::join(
        || {
            rayon::join(
                || raster::write_u8(&dir.join(files::EXPOSURE), &exposure),
                || {
                    if clpa_raised > 0 {
                        raster::write_u8(&model_path, &m.exposure).map(Some)
                    } else {
                        Ok(None)
                    }
                },
            )
        },
        || {
            rayon::join(
                || raster::write_u8(&dir.join(files::ATES), &m.ates_classes),
                || {
                    rayon::join(
                        || raster::write_f32(&dir.join(files::CANOPY), canopy),
                        || -> Result<_> {
                            let ex = segments::lines(lines, &exposure)?;
                            let feats = segments::features(&ex)?;
                            skimap_common::output::write_geojson(
                                &dir.join(files::SEGMENTS),
                                &feats,
                            )?;
                            Ok((ex, feats.len()))
                        },
                    )
                },
            )
        },
    );
    let (ex, n_features) = runs?;

    let mut fmeta = Vec::new();
    let bytes = exposure_bytes?;
    fmeta.push(tif_meta(
        files::EXPOSURE,
        "Exposition aux avalanches (modèle) : 1 = terrain peu exposé (modèle), 2 = exposé, 3 = très exposé ; dans les Hautes-Alpes, les emprises CLPA sont au moins « exposé » ; même grille que terrain/dem_5m.tif (calque `Overlay` du routeur hors piste)",
        "classe",
        &exposure,
        bytes,
        "255",
    ));
    if let Some(bytes) = model_bytes? {
        fmeta.push(tif_meta(
            files::EXPOSURE_MODEL,
            "Diagnostic (validation P18) : classes du modèle seul, avant la fusion des emprises CLPA ; mêmes codes",
            "classe",
            &m.exposure,
            bytes,
            "255",
        ));
    }
    let bytes = ates_bytes?;
    fmeta.push(tif_meta(
        files::ATES,
        "Diagnostic, ne pas afficher : classes 0 à 4 du classifieur dev8 d'AutoATES v3.0 (portage), identiques à la référence ; la classe 0 peut tomber dans une zone d'écoulement modélisée",
        "classe",
        &m.ates_classes,
        bytes,
        "255",
    ));
    let bytes = canopy_bytes?;
    fmeta.push(FileMeta::geotiff_f32(
        files::CANOPY,
        "Couvert forestier : part des cellules de 5 m de hauteur de canopée ≥ 3 m dans la fenêtre 3 × 3 (MNH LiDAR HD)",
        "%",
        &canopy.to_grid(),
        bytes,
    ));

    // Piste runs.
    fmeta.push(
        FileMeta::new(
            files::SEGMENTS,
            "Tronçons des pistes et itinéraires de la couche pistes, coupés là où la classe d'exposition change (classe la plus haute à 10 m de la trace)",
        )
        .licence("ODbL 1.0 (géométrie OSM)")
        .count(n_features)
        .properties([
            ("osm_way_id", "identifiant du way OSM"),
            ("run", "rang du tronçon le long du way"),
            ("name", "nom de la piste (couche pistes)"),
            ("relations", "relations de piste contenant le way"),
            ("network", "nordic ou hike (règle du rapport 07)"),
            ("exposure", "1 = terrain peu exposé (modèle), 2 = exposé, 3 = très exposé, null = pas de donnée"),
            ("label", "libellé de la classe"),
            ("length_m", "longueur du tronçon, m (Lambert-93)"),
            ("way_length_m", "longueur du way, m"),
            ("way_max_exposure", "classe la plus haute le long du way"),
        ]),
    );

    let all_refs: Vec<&segments::LineExposure<'_>> = ex.iter().collect();
    let nordic_refs: Vec<&segments::LineExposure<'_>> = ex
        .iter()
        .filter(|l| l.line.network == Network::Nordic && in_commune(l.line))
        .collect();
    let report07 = report07_prefilter(site).map(|(b, a18, a30)| {
        let r: Vec<&segments::LineExposure<'_>> = ex
            .iter()
            .filter(|l| l.line.network == Network::Nordic && b.contains(l.line.vertex_mean()))
            .collect();
        Report07 {
            bbox: [b.south, b.west, b.north, b.east],
            reachable_alpha18: a18,
            reachable_alpha30: a30,
            ours: network_exposure(&r),
        }
    });

    let mut sources = vec![
        SourceRef::new(
            "MNT LiDAR HD (5 m, rééchantillonné par le WMS), via la couche terrain",
            format!(
                "{}?LAYERS={}",
                skimap_terrain::dem::WMS_URL,
                skimap_terrain::dem::LAYER
            ),
            "Licence Ouverte 2.0 (Etalab)",
            "IGN – MNT LiDAR HD",
        )
        .producer("IGN"),
        SourceRef::new(
            "MNH LiDAR HD (hauteur de canopée, 5 m, rééchantillonné par le WMS)",
            format!(
                "{}?LAYERS={}",
                skimap_terrain::dem::WMS_URL,
                super::forest::MNH_LAYER
            ),
            "Licence Ouverte 2.0 (Etalab)",
            "IGN – MNH LiDAR HD",
        )
        .producer("IGN")
        .note(match skimap_terrain::lidar_flights(site) {
            Some(l) => format!(
                "vols LiDAR HD : {l} (rapport 07) ; les coupes postérieures n'apparaissent pas"
            ),
            None => {
                "vols LiDAR HD propres à chaque dalle ; les coupes postérieures n'apparaissent pas"
                    .to_owned()
            }
        }),
        pistes_source,
        SourceRef::new(
            "Méthode : AutoATES v3.0, instantané ISSW 2026 (Sykes, Knies, Haegeli, Anthony-Malone, Statham), portage Rust",
            "https://github.com/AutoATES/autoATES-v3.0-issw/tree/5558c16469173cd14725576c1a5a60b7a8e14628",
            "MIT",
            "AutoATES v3.0 (MIT)",
        ),
        SourceRef::new(
            "Méthode : com4FlowPy (AvaFrame) / Flow-Py v1.0 (D'Amboise et al. 2022, GMD 15, 2423–2439), portage Rust",
            "https://github.com/OpenNHM/AvaFrame/tree/d0451ce8019bf1db6b7b55a9099314a482405d8e/avaframe/com4FlowPy",
            "EUPL-1.2 (AvaFrame) ; Flow-Py GPL-3.0 : effet du portage sur la licence du projet à décider (F2)",
            "AvaFrame com4FlowPy ; Flow-Py",
        ),
    ];

    if let Some(src) = clpa_source {
        sources.push(src);
    }
    let (valid, cs) = shares(&exposure.data, [class::LOW, class::EXPOSED, class::HIGH]);
    let (_, ates) = shares(&m.ates_classes.data, [0, 1, 2, 3, 4]);
    let summary = ExposureSummary {
        site: site.id.into(),
        grid: (exposure.w, exposure.h, exposure.cell),
        valid_cells: valid,
        class_shares: cs,
        ates_shares: ates,
        paths: m.paths,
        all_lines: network_exposure(&all_refs),
        nordic_in_commune: network_exposure(&nordic_refs),
        report07,
        clpa_model,
        clpa_fused,
        clpa_cells_raised: clpa_raised,
        timings: m.timings.clone(),
        io_seconds: (t_in, 0.0),
        disclaimers: DISCLAIMERS.iter().map(|s| (*s).to_owned()).collect(),
    };
    fmeta.push(FileMeta::new(files::SUMMARY, "Chiffres clés : parts des classes, longueurs de pistes par classe, validation CLPA, temps de calcul"));

    let mut meta = LayerMeta::new(LAYER, site.id);
    meta.tickets = vec!["P16 (#29)".into(), "P17 (#30)".into(), "P18 (#31)".into()];
    meta.validity = Some(
        "relief et forêt à la date des vols LiDAR HD ; indépendant de la neige et du jour".into(),
    );
    meta.crs = Some("EPSG:2154 (rasters), EPSG:4326 (GeoJSON)".into());
    meta.sources = sources;
    meta.files = fmeta;
    let mut notes: Vec<String> = DISCLAIMERS.iter().map(|s| (*s).to_owned()).collect();
    notes.extend([
        "Jamais « sûr » ni « sans danger » : les trois classes sont « terrain peu exposé (modèle) », « exposé », « très exposé ».".to_owned(),
        format!("Très exposé = classes ATES 3 et 4 du classifieur dev8 ; peu exposé = classes 0 et 1 à plus de {LOW_MARGIN_M} m de toute cellule atteinte par l'écoulement rare (α = 18°) et de toute zone de départ rare avant tamisage ; exposé = le reste."),
        if personal_sources {
            "Dans les Hautes-Alpes, les cellules dans une emprise CLPA (DDT 05) passent au moins en « exposé » : une avalanche connue n'est jamais affichée « peu exposé ». La validation P18 (clpa_model du résumé) mesure le modèle seul, avant cette fusion. CLPA à usage personnel : couche marquée usage personnel.".to_owned()
        } else {
            "Sortie du modèle seul : la CLPA (usage personnel, INRAE ; republication par la DDT 05 non confirmée) n'est ni lue ni fusionnée sans l'option personal_sources.".to_owned()
        },
        "Zones de départ : pente (Horn, comme GDAL), abri au vent (médiane des angles sur 60 m), forêt ; logique floue et seuils des valeurs ISSW 2026 d'AutoATES v3.0, sans rugosité ; tamis de 1000 m² (algorithme de gdal_sieve).".into(),
        "Écoulement : Flow-Py (com4FlowPy, moteur Numba porté opération par opération), α = 30° (fréquent) et 18° (rare), exposant 8, seuil de flux 0,003, z_delta ≤ 270 m, frottement et détraînement en forêt (forestDetrainment).".into(),
        "Forêt : cellule « arbre » si la hauteur de canopée du MNH LiDAR HD à 5 m est ≥ 3 m ; couvert = part des cellules « arbre » sur 3 × 3 cellules (15 m). Choix non calibrés.".into(),
        "Accord avec la référence Python (AutoATES v3.0 + AvaFrame, mêmes entrées, 5 m, site de validation de l'oracle, mesuré le 15/11/2026) : classes identiques sur les 10 087 708 cellules ; détail dans oracle_comparison_5m.json quand l'exemple oracle_compare a tourné.".into(),
    ]);
    meta.notes = notes;
    meta.parameters = json!({
        "pra": [pra::FREQUENT, pra::EXTREME],
        "flowpy": [flowpy::FREQUENT, flowpy::EXTREME],
        "low_margin_m": LOW_MARGIN_M,
        "segment_buffer_m": segments::BUFFER_M,
        "tree_height_m": super::forest::TREE_HEIGHT_M,
        "cover_window_cells": 2 * super::forest::COVER_RADIUS + 1,
        "class_codes": {"1": class::label(1), "2": class::label(2), "3": class::label(3), "255": "pas de donnée"},
    });
    meta.stats = serde_json::to_value(&summary).unwrap_or(Value::Null);
    meta.write(dir)?;
    Ok(summary)
}

/// Reads the user exposure classes of a site as a Lambert-93 `u8` grid aligned with the
/// 5 m terrain grid: codes 1 « terrain peu exposé (modèle) », 2 « exposé »,
/// 3 « très exposé », 255 no data. This is the form the off-piste router's
/// `skimap_core::offpiste::Overlay` (`OverlayKind::AvalancheExposure`) takes: its
/// `factor` table is indexed by these codes (255 should keep factor 1, or block, as the
/// caller decides).
pub fn read_exposure_grid(
    layout: &DataLayout,
    site: &Site,
) -> Result<skimap_common::grid::Grid<u8>> {
    let path = layout.layer_dir(site, LAYER)?.join(files::EXPOSURE);
    if !path.exists() {
        return Err(Error::MissingLayer {
            layer: LAYER.into(),
            site: site.id.into(),
            path,
        });
    }
    let g = skimap_common::geotiff::read_u8(&path)?;
    if g.crs() != skimap_common::crs::Crs::Lambert93 {
        return Err(Error::format(path.display().to_string(), "not Lambert-93"));
    }
    Ok(g.with_nodata(class::NODATA))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exposure_grid_round_trip_for_the_router_overlay() {
        let root =
            std::env::temp_dir().join(format!("skimap-avalanche-test-{}", std::process::id()));
        let layout = DataLayout::new(&root);
        let site = skimap_common::site::site("cervieres").expect("evaluation site");
        let dir = layout.layer_dir(site, LAYER).unwrap();
        DataLayout::ensure(&dir).unwrap();
        let r = Raster {
            w: 3,
            h: 2,
            cell: 5.0,
            origin_x: 988_709.75,
            origin_y: 6_431_980.25,
            data: vec![1u8, 2, 3, 255, 1, 2],
        };
        raster::write_u8(&dir.join(files::EXPOSURE), &r).unwrap();
        let g = read_exposure_grid(&layout, site).unwrap();
        assert_eq!(g.data(), &r.data[..]);
        assert_eq!(g.crs(), skimap_common::crs::Crs::Lambert93);
        assert_eq!(g.transform().origin_x, r.origin_x);
        assert_eq!(g.transform().pixel_width, 5.0);
        std::fs::remove_dir_all(&root).unwrap();
        for d in DISCLAIMERS {
            let l = d.to_lowercase();
            assert!(!l.contains(" sûr") && !l.contains("sans danger »"), "{d}");
        }
    }
}
