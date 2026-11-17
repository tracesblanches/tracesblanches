//! France-wide products of the CLI: the site registry (every census domain becomes a site,
//! `data/out/france/sites/sites.json`) and the catalogue of France
//! (`data/out/france/catalog.json` and `catalog.geojson`) read by the viewer, the app and
//! the server.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use skimap_common::crs;
use skimap_common::geom::{BBox, LonLat};
use skimap_common::layout::DataLayout;
use skimap_common::meta::{FileMeta, LayerMeta, SourceRef, now_rfc3339};
use skimap_common::site::{
    self, DOMAIN_MARGIN_M, IdInput, MAX_SITE_SIDE_M, MassifRecord, MassifStatus, Rect93, Registry,
    RegistryFile, SiteKind, SiteRecord, StatusRecord,
};
use skimap_snow::massifs::Massif;

use crate::build::Ctx;

/// Largest distance from a site outside every massif polygon to the nearest massif for it
/// to be attached to that massif (status « assumed »), metres: the snow map already gives
/// a cell outside every polygon the massif of the nearest cell; beyond it (Ardennes,
/// Morvan, 100 km and more) the site is outside the massifs.
pub const MASSIF_NEAREST_MAX_M: f64 = 10_000.0;
/// Spacing of the points sampled in an envelope to find its massifs, metres.
pub const MASSIF_SAMPLE_M: f64 = 250.0;
/// Smallest share of the sample points for a second massif to be listed.
pub const MASSIF_MIN_SHARE: f64 = 0.05;
/// Overlap of neighbouring sections, metres (each side).
pub const SECTION_OVERLAP_M: f64 = 1_000.0;
/// SRN areas of France outside groomed domains (G5, `skimap-srn`): play areas
/// (`kind: domaine`) and traverses (`traversee`), footprint MultiPolygon (traverse
/// corridor of 500 m), properties `id`, `name`, `commune {name, insee, departement}`,
/// `lon`, `lat`, `km_routes` (see `crates/srn/docs/srn.md`). Path under
/// `data/out/france/`.
pub const SRN_AREAS: &str = "srn/srn_areas.geojson";
/// Half width of the corridor of a traverse given as a line, metres (lines are not
/// expected in [`SRN_AREAS`], whose traverses already are corridors).
pub const TRAVERSE_HALF_WIDTH_M: f64 = 500.0;

/// Polygons of an envelope, Lambert-93: each polygon is its rings (outer first).
type Polys = Vec<Vec<Vec<(f64, f64)>>>;

fn inside(polys: &Polys, p: (f64, f64)) -> bool {
    polys.iter().any(|rings| {
        let mut c = false;
        for ring in rings {
            let n = ring.len();
            let mut j = n.wrapping_sub(1);
            for i in 0..n {
                let (xi, yi) = ring[i];
                let (xj, yj) = ring[j];
                if (yi > p.1) != (yj > p.1) && p.0 < (xj - xi) * (p.1 - yi) / (yj - yi) + xi {
                    c = !c;
                }
                j = i;
            }
        }
        c
    })
}

fn extent(polys: &Polys) -> Rect93 {
    let mut r = (
        f64::INFINITY,
        f64::INFINITY,
        f64::NEG_INFINITY,
        f64::NEG_INFINITY,
    );
    for p in polys.iter().flatten().flatten() {
        r = (r.0.min(p.0), r.1.min(p.1), r.2.max(p.0), r.3.max(p.1));
    }
    r
}

/// Polygons of a GeoJSON geometry in Lambert-93; a line becomes its corridor.
fn polygons_93(geometry: &Value, half_width: f64) -> Result<Polys> {
    let ring = |r: &Value| -> Result<Vec<(f64, f64)>> {
        r.as_array()
            .into_iter()
            .flatten()
            .map(|c| {
                let lon = c[0].as_f64().context("lon")?;
                let lat = c[1].as_f64().context("lat")?;
                Ok(crs::to_lambert93(LonLat::new(lon, lat))?)
            })
            .collect()
    };
    let poly = |p: &Value| -> Result<Vec<Vec<(f64, f64)>>> {
        p.as_array().into_iter().flatten().map(ring).collect()
    };
    let c = &geometry["coordinates"];
    Ok(match geometry["type"].as_str() {
        Some("Polygon") => vec![poly(c)?],
        Some("MultiPolygon") => c
            .as_array()
            .into_iter()
            .flatten()
            .map(poly)
            .collect::<Result<_>>()?,
        Some("LineString") => vec![vec![site::corridor(&ring(c)?, half_width)]],
        Some("MultiLineString") => c
            .as_array()
            .into_iter()
            .flatten()
            .map(|l| Ok(vec![site::corridor(&ring(l)?, half_width)]))
            .collect::<Result<_>>()?,
        other => bail!("géométrie {other:?} non prise en charge"),
    })
}

/// Massifs of an envelope: share of the points of a 250 m grid inside it per snowtools
/// massif, main first, those of at least 5 %; `fallback` (representative point) when no
/// grid point falls inside (tiny envelope).
fn massifs_of(polys: &Polys, fallback: (f64, f64), all: &[Massif]) -> Vec<MassifRecord> {
    massifs_within(polys, extent(polys), fallback, all)
}

/// [`massifs_of`] counting only the points inside `r` (a section).
fn massifs_within(
    polys: &Polys,
    r: Rect93,
    fallback: (f64, f64),
    all: &[Massif],
) -> Vec<MassifRecord> {
    let mut pts = Vec::new();
    let mut y = r.1 + MASSIF_SAMPLE_M / 2.0;
    while y < r.3 {
        let mut x = r.0 + MASSIF_SAMPLE_M / 2.0;
        while x < r.2 {
            if inside(polys, (x, y)) {
                pts.push((x, y));
            }
            x += MASSIF_SAMPLE_M;
        }
        y += MASSIF_SAMPLE_M;
    }
    if pts.is_empty() {
        pts.push(fallback);
    }
    let mut counts: BTreeMap<u16, (usize, String)> = BTreeMap::new();
    for p in &pts {
        if let Some(m) = skimap_snow::massifs::massif_at(all, p.0, p.1) {
            counts.entry(m.code).or_insert((0, m.name.clone())).0 += 1;
        }
    }
    let n = pts.len() as f64;
    let mut out: Vec<MassifRecord> = counts
        .into_iter()
        .map(|(code, (k, name))| MassifRecord {
            code,
            name,
            share: Some((k as f64 / n * 1000.0).round() / 1000.0),
        })
        .filter(|m| m.share.unwrap_or(0.0) >= MASSIF_MIN_SHARE)
        .collect();
    out.sort_by(|a, b| b.share.unwrap_or(0.0).total_cmp(&a.share.unwrap_or(0.0)));
    out
}

fn segment_distance(p: (f64, f64), a: (f64, f64), b: (f64, f64)) -> f64 {
    let (dx, dy) = (b.0 - a.0, b.1 - a.1);
    let l2 = dx * dx + dy * dy;
    let t = if l2 > 0.0 {
        (((p.0 - a.0) * dx + (p.1 - a.1) * dy) / l2).clamp(0.0, 1.0)
    } else {
        0.0
    };
    (p.0 - a.0 - t * dx).hypot(p.1 - a.1 - t * dy)
}

/// Nearest massif polygon to a point and its distance, metres.
fn nearest_massif(all: &[Massif], p: (f64, f64)) -> Option<(&Massif, f64)> {
    all.iter()
        .map(|m| {
            let d = m
                .rings
                .iter()
                .flat_map(|r| r.windows(2).map(|w| segment_distance(p, w[0], w[1])))
                .fold(f64::INFINITY, f64::min);
            (m, d)
        })
        .min_by(|a, b| a.1.total_cmp(&b.1))
}

/// Massifs of an envelope ([`massifs_of`]); when none covers it, the nearest one within
/// [`MASSIF_NEAREST_MAX_M`] of the representative point, as « assumed ».
fn attach_massifs(
    polys: &Polys,
    rep: (f64, f64),
    all: &[Massif],
) -> (Vec<MassifRecord>, MassifStatus, String) {
    let found = massifs_of(polys, rep, all);
    if !found.is_empty() {
        let note = massif_note(&found);
        return (found, MassifStatus::Verified, note);
    }
    match nearest_massif(all, rep) {
        Some((m, d)) if d <= MASSIF_NEAREST_MAX_M => (
            vec![MassifRecord {
                code: m.code,
                name: m.name.clone(),
                share: Some(0.0),
            }],
            MassifStatus::Assumed,
            format!(
                "aucun polygone snowtools ne couvre l'enveloppe ; massif le plus proche à {:.1} km du point représentatif",
                d / 1000.0
            ),
        ),
        Some((m, d)) => (
            Vec::new(),
            MassifStatus::Outside,
            format!(
                "aucun massif snowtools ne couvre l'enveloppe ; le plus proche ({} {}) est à {:.0} km",
                m.code,
                m.name,
                d / 1000.0
            ),
        ),
        None => (Vec::new(), MassifStatus::Outside, "aucun massif".into()),
    }
}

fn has_bera(massifs: &[MassifRecord]) -> bool {
    massifs
        .first()
        .is_some_and(|m| skimap_meteo::bera::massif(m.code).is_some())
}

fn massif_note(massifs: &[MassifRecord]) -> String {
    format!(
        "points tous les {MASSIF_SAMPLE_M:.0} m dans l'enveloppe : {}",
        massifs
            .iter()
            .map(|m| format!(
                "{} {} {:.1} %",
                m.code,
                m.name,
                m.share.unwrap_or(0.0) * 100.0
            ))
            .collect::<Vec<_>>()
            .join(", ")
    )
}

/// Box of the envelope's rectangle grown by `margin`, and its sections when it is larger
/// than [`MAX_SITE_SIDE_M`].
fn boxes(polys: &Polys, margin: f64) -> Result<(BBox, Vec<(BBox, Rect93)>)> {
    let r = site::grow(extent(polys), margin);
    let whole = site::bbox_of_lambert93(r)?;
    if (r.2 - r.0).max(r.3 - r.1) <= MAX_SITE_SIDE_M {
        return Ok((whole, Vec::new()));
    }
    // Sections cut on the envelope itself, then grown by the margin and the overlap.
    let rings: Vec<Vec<(f64, f64)>> = polys.iter().flatten().cloned().collect();
    let sections = site::split_extent(&rings, MAX_SITE_SIDE_M, margin + SECTION_OVERLAP_M)
        .into_iter()
        .map(|r| Ok((site::bbox_of_lambert93(r)?, r)))
        .collect::<Result<Vec<_>>>()?;
    Ok((whole, sections))
}

/// Adds a record, or its sections (each with the massifs of its part of the envelope)
/// after it.
fn add_sections(
    out: &mut Vec<SiteRecord>,
    mut parent: SiteRecord,
    sections: Vec<(BBox, Rect93)>,
    polys: &Polys,
    all: &[Massif],
) {
    if sections.is_empty() {
        out.push(parent);
        return;
    }
    let n = sections.len();
    let width = n.to_string().len();
    let mut children = Vec::new();
    for (k, (b, r)) in sections.into_iter().enumerate() {
        let mut c = parent.clone();
        let centre = ((r.0 + r.2) / 2.0, (r.1 + r.3) / 2.0);
        let found = massifs_within(polys, r, centre, all);
        if !found.is_empty() {
            c.has_bera = has_bera(&found);
            c.massif_status = MassifStatus::Verified;
            c.massif_note = Some(format!("tronçon : {}", massif_note(&found)));
            c.massifs = found;
        }
        c.id = format!("{}-{:0width$}", parent.id, k + 1);
        c.name = format!("{} ({}/{n})", parent.name, k + 1);
        c.parent = Some(parent.id.clone());
        c.bbox = b;
        c.envelope_bbox = None;
        c.local = None;
        children.push(c);
    }
    parent.sections = children.iter().map(|c| c.id.clone()).collect();
    out.push(parent);
    out.extend(children);
}

/// Census domains of `domaines.geojson` as registry records.
fn census_records(features: &[Value], massifs: &[Massif]) -> Result<Vec<SiteRecord>> {
    let evaluation: BTreeMap<&str, &SiteRecord> = site::evaluation_records()
        .iter()
        .filter_map(|r| r.census_id.as_deref().map(|c| (c, r)))
        .collect();
    let fixed: BTreeMap<&str, &str> = evaluation
        .iter()
        .map(|(c, r)| (*c, r.id.as_str()))
        .collect();
    let inputs: Vec<IdInput<'_>> = features
        .iter()
        .map(|f| {
            let p = &f["properties"];
            IdInput {
                census_id: p["id"].as_str().unwrap_or(""),
                name: p["name"].as_str().unwrap_or(""),
                km: p["km_nordic"].as_f64().unwrap_or(0.0),
            }
        })
        .collect();
    let ids = site::assign_ids(&inputs, &fixed);
    let mut out = Vec::new();
    for (f, id) in features.iter().zip(ids) {
        let p = &f["properties"];
        let census_id = p["id"].as_str().context("census id")?.to_owned();
        let polys = polygons_93(&f["geometry"], 0.0)?;
        let env = extent(&polys);
        let envelope_bbox = site::bbox_of_lambert93(env)?;
        let lon = p["lon"].as_f64().context("lon")?;
        let lat = p["lat"].as_f64().context("lat")?;
        let rep = crs::to_lambert93(LonLat::new(lon, lat))?;
        let (found, mstatus, mnote) = attach_massifs(&polys, rep, massifs);
        let status_source = site::status_source(&census_id).cloned();
        let status = StatusRecord {
            nordicfrance_station: status_source.as_ref().and_then(|s| s.nordicfrance_station),
            ..Default::default()
        };
        let km_nordic = p["km_nordic"].as_f64();
        let km_hike_only = p["km_hike_only"].as_f64();
        if let Some(eval) = evaluation.get(census_id.as_str()) {
            // Evaluation site: the table keeps its working box, commune and verified
            // massifs (P6); the census gives the rest.
            let mut r = (*eval).clone();
            r.census_id = Some(census_id);
            r.envelope_bbox = Some(envelope_bbox);
            r.km_nordic = km_nordic;
            r.km_hike_only = km_hike_only;
            r.status_source = status_source;
            out.push(r);
            continue;
        }
        let (bbox, sections) = boxes(&polys, DOMAIN_MARGIN_M)?;
        let rec = SiteRecord {
            id,
            name: p["name"].as_str().unwrap_or("").to_owned(),
            kind: SiteKind::NordicStation,
            parent: None,
            census_id: Some(census_id),
            department: p["commune"]["departement"]
                .as_str()
                .unwrap_or("")
                .to_owned(),
            insee: p["commune"]["insee"].as_str().unwrap_or("").to_owned(),
            commune: p["commune"]["name"].as_str().unwrap_or("").to_owned(),
            commune_center: LonLat::new(lon, lat),
            bbox,
            envelope_bbox: Some(envelope_bbox),
            massif_status: mstatus,
            massif_note: Some(mnote),
            has_bera: has_bera(&found),
            massifs: found,
            status,
            status_source,
            km_nordic,
            km_hike_only,
            km_routes: None,
            sections: Vec::new(),
            local: None,
            starts: Vec::new(),
        };
        add_sections(&mut out, rec, sections, &polys, massifs);
    }
    Ok(out)
}

/// Sites of the SRN areas of [`SRN_AREAS`] (ticket G5): identifier = the `id` of the
/// area, type SRN play area or traverse, working box = footprint + [`DOMAIN_MARGIN_M`],
/// split in sections when larger than [`MAX_SITE_SIDE_M`] (the GTJ, 100 km).
fn srn_records(features: &[Value], massifs: &[Massif]) -> Result<Vec<SiteRecord>> {
    let mut out = Vec::new();
    for f in features {
        let p = &f["properties"];
        let name = p["name"].as_str().context("name")?.to_owned();
        let id = p["id"]
            .as_str()
            .map_or_else(|| site::slug(&name), str::to_owned);
        let kind = match p["kind"].as_str() {
            Some("domaine") => SiteKind::SrnArea,
            Some("traversee") => SiteKind::Traverse,
            other => bail!("{id} : type SRN {other:?} (domaine, traversee)"),
        };
        let polys = polygons_93(&f["geometry"], TRAVERSE_HALF_WIDTH_M)?;
        let env = extent(&polys);
        let center = match (p["lon"].as_f64(), p["lat"].as_f64()) {
            (Some(lon), Some(lat)) => LonLat::new(lon, lat),
            _ => crs::from_lambert93((env.0 + env.2) / 2.0, (env.1 + env.3) / 2.0)?,
        };
        let rep = crs::to_lambert93(center)?;
        let (found, mstatus, mnote) = attach_massifs(&polys, rep, massifs);
        let (bbox, sections) = boxes(&polys, DOMAIN_MARGIN_M)?;
        let c = &p["commune"];
        let rec = SiteRecord {
            id,
            name,
            kind,
            parent: None,
            census_id: None,
            department: c["departement"].as_str().unwrap_or("").to_owned(),
            insee: c["insee"].as_str().unwrap_or("").to_owned(),
            commune: c["name"].as_str().unwrap_or("").to_owned(),
            commune_center: center,
            bbox,
            envelope_bbox: Some(site::bbox_of_lambert93(env)?),
            massif_status: mstatus,
            massif_note: Some(mnote),
            has_bera: has_bera(&found),
            massifs: found,
            status: StatusRecord::default(),
            status_source: None,
            km_nordic: None,
            km_hike_only: None,
            km_routes: p["km_routes"].as_f64(),
            sections: Vec::new(),
            local: None,
            starts: Vec::new(),
        };
        add_sections(&mut out, rec, sections, &polys, massifs);
    }
    Ok(out)
}

/// Sets the trail starts of the census sites from `stations/departs.json` (X9) when it
/// exists: a site takes the starts of its census domain, a section those inside its box
/// (the parent's main one if none). Returns the number of sites with starts.
pub fn apply_departs(layout: &DataLayout, records: &mut [SiteRecord]) -> Result<usize> {
    let path = layout
        .france_layer_dir(skimap_stations::LAYER)?
        .join(skimap_stations::departs::FILE);
    if !path.is_file() {
        return Ok(0);
    }
    let file = skimap_stations::departs::DepartsFile::read(&path)?;
    let mut n = 0;
    for r in records.iter_mut() {
        if matches!(r.kind, SiteKind::SrnArea | SiteKind::Traverse) {
            r.starts = srn_starts(&file, r);
            n += usize::from(!r.starts.is_empty());
            continue;
        }
        let Some(list) = r.census_id.as_ref().and_then(|c| file.domains.get(c)) else {
            continue;
        };
        let mut starts: Vec<site::Start> = list.iter().map(|d| d.start.clone()).collect();
        if r.parent.is_some() {
            let inside: Vec<site::Start> = starts
                .iter()
                .filter(|s| r.bbox.contains(s.position()))
                .cloned()
                .collect();
            if !inside.is_empty() {
                starts = inside;
            } else {
                starts.truncate(1);
            }
        }
        r.starts = starts;
        n += usize::from(!r.starts.is_empty());
    }
    Ok(n)
}

/// Minimum number of ENJ ski itineraries crossing the box of a domain for it to be polled
/// on the ENJ: one is enough, the poll attaches only the points inside the box.
pub const ENJ_MIN_ITINERARIES: usize = 1;

/// Outcome of the detection of the ENJ domains (X12b, #146).
#[derive(Debug, Clone, Default, Serialize)]
pub struct EnjDetection {
    /// ENJ ski itineraries read (cross-country and multi-activity).
    pub itineraries: usize,
    /// Sites polled on the ENJ by their box: identifier → number of ski itineraries
    /// crossing it.
    pub sites: BTreeMap<String, usize>,
    /// Census domains research report 08 attaches to the ENJ but no ENJ ski itinerary
    /// crosses (checked by hand, never added).
    pub report08_not_detected: Vec<String>,
    /// Detected sites that report 08 does not attach to the ENJ.
    pub detected_not_in_report08: Vec<String>,
}

/// Box in which the ENJ itineraries of a record are looked for: the envelope of the domain
/// (the census hull, without the working margin), else its working box.
fn enj_box(r: &SiteRecord) -> BBox {
    r.envelope_bbox.unwrap_or(r.bbox)
}

/// Sets `status.enj_bbox` of every groomed station crossed by ENJ ski itineraries, from the
/// ENJ data themselves (every ski itinerary with its geometry, [`skimap_status::enj`]):
/// no table of ENJ sites. Sites with an ENJ city keep it. Pure, for tests.
pub fn apply_enj(
    records: &mut [SiteRecord],
    treks: &[skimap_status::enj::TrekGeometry],
) -> EnjDetection {
    let mut d = EnjDetection {
        itineraries: treks.len(),
        ..Default::default()
    };
    for r in records.iter_mut() {
        r.status.enj_bbox = None;
        if !r.kind.has_groomed_trails() || !r.buildable() {
            continue;
        }
        let b = enj_box(r);
        let n = treks
            .iter()
            .filter(|t| skimap_status::enj::crosses(&t.geometry, &b))
            .count();
        let in_report = r.status_source.as_ref().is_some_and(|s| s.kind == "enj");
        if n >= ENJ_MIN_ITINERARIES {
            if r.status.enj_city.is_none() {
                r.status.enj_bbox = Some(b);
            }
            d.sites.insert(r.id.clone(), n);
            if !in_report {
                d.detected_not_in_report08.push(r.id.clone());
            }
        } else if in_report {
            d.report08_not_detected.push(r.id.clone());
        }
    }
    d
}

/// Detects the ENJ domains ([`apply_enj`]) when the personal sources are enabled (the ENJ
/// displays no licence, ticket L1); otherwise no site is polled on the ENJ by its box.
pub fn detect_enj(ctx: &Ctx, records: &mut [SiteRecord]) -> Result<Option<EnjDetection>> {
    if !ctx.cfg.personal_sources {
        for r in records.iter_mut() {
            r.status.enj_bbox = None;
        }
        return Ok(None);
    }
    let treks = skimap_status::enj::fetch_ski_coverage(&ctx.dl)?;
    Ok(Some(apply_enj(records, &treks)))
}

/// Rewrites the registry with the ENJ domains detected from the ENJ data (X12b); written
/// only when a site changed. No registry: nothing to do.
pub fn refresh_registry_enj(ctx: &Ctx) -> Result<Option<EnjDetection>> {
    let path = Registry::path(&ctx.layout)?;
    if !path.is_file() {
        return Ok(None);
    }
    let body = std::fs::read(&path).with_context(|| format!("{}", path.display()))?;
    let mut file: RegistryFile =
        serde_json::from_slice(&body).with_context(|| format!("{}", path.display()))?;
    let before: Vec<Option<BBox>> = file.sites.iter().map(|r| r.status.enj_bbox).collect();
    let d = detect_enj(ctx, &mut file.sites)?;
    let after: Vec<Option<BBox>> = file.sites.iter().map(|r| r.status.enj_bbox).collect();
    if before != after {
        Registry::from_records(file.sites.clone())?;
        Registry::write(&path, &file)?;
    }
    Ok(d)
}

/// Starts of an SRN site or section (X9b) from the starts of its area in `departs.json`:
/// the best of the area, or of those inside the box of the section (the best of the area
/// when none is inside).
fn srn_starts(file: &skimap_stations::departs::DepartsFile, r: &SiteRecord) -> Vec<site::Start> {
    use skimap_stations::departs_srn::pick;
    let Some(list) = file.srn.get(r.parent.as_deref().unwrap_or(&r.id)) else {
        return Vec::new();
    };
    let mut picked = if r.parent.is_some() {
        pick(list, Some(&r.bbox))
    } else {
        Vec::new()
    };
    if picked.is_empty() {
        picked = pick(list, None);
        if r.parent.is_some() {
            picked.truncate(1);
        }
    }
    picked.into_iter().map(|d| d.start).collect()
}

/// Trail starts (X9) after a run of the stations layer: rebuilt when the station cards
/// ran (new OSM points) or when `departs.json` is missing, then written into the registry.
pub fn departs_after_stations(
    ctx: &Ctx,
    ran: &[String],
) -> Result<Option<skimap_stations::departs::DepartsOutput>> {
    let path = ctx
        .layout
        .france_layer_dir(skimap_stations::LAYER)?
        .join(skimap_stations::departs::FILE);
    if !ran.iter().any(|r| r == "fiches") && path.is_file() && !srn_newer(&ctx.layout, &path)? {
        return Ok(None);
    }
    let out = skimap_stations::departs::build(&ctx.layout, &ctx.dl)?;
    refresh_registry_starts(&ctx.layout)?;
    Ok(Some(out))
}

/// Whether the SRN areas or routes (`srn/`) were written after `departs` (X9b: their
/// starts are then out of date).
fn srn_newer(layout: &DataLayout, departs: &Path) -> Result<bool> {
    use skimap_stations::departs_srn::{AREAS_FILE, ROUTES_FILE, SRN_LAYER};
    let modified = |p: &Path| std::fs::metadata(p).and_then(|m| m.modified()).ok();
    let Some(at) = modified(departs) else {
        return Ok(true);
    };
    let dir = layout.france_layer_dir(SRN_LAYER)?;
    Ok([AREAS_FILE, ROUTES_FILE]
        .iter()
        .filter_map(|f| modified(&dir.join(f)))
        .any(|m| m > at))
}

/// Rewrites the registry file with the starts of `stations/departs.json` (X9), after the
/// stations layer ran; no registry: nothing to do.
pub fn refresh_registry_starts(layout: &DataLayout) -> Result<usize> {
    let path = Registry::path(layout)?;
    if !path.is_file() {
        return Ok(0);
    }
    let body = std::fs::read(&path).with_context(|| format!("{}", path.display()))?;
    let mut file: RegistryFile =
        serde_json::from_slice(&body).with_context(|| format!("{}", path.display()))?;
    let n = apply_departs(layout, &mut file.sites)?;
    Registry::from_records(file.sites.clone())?;
    Registry::write(&path, &file)?;
    Ok(n)
}

fn read_features(path: &Path) -> Result<Vec<Value>> {
    let v: Value = serde_json::from_slice(
        &std::fs::read(path).with_context(|| format!("{}", path.display()))?,
    )
    .with_context(|| format!("{}", path.display()))?;
    Ok(v["features"].as_array().cloned().unwrap_or_default())
}

/// Summary of a registry build.
#[derive(Debug, Serialize)]
pub struct RegistrySummary {
    /// Registry file.
    pub path: PathBuf,
    /// Records (parents of sections included).
    pub sites: usize,
    /// Sites that are built.
    pub buildable: usize,
    /// Census domains.
    pub census_domains: usize,
    /// SRN areas and traverses.
    pub extra: usize,
    /// Sites per main massif status.
    pub by_massif_status: BTreeMap<String, usize>,
    /// Sites with a BERA.
    pub with_bera: usize,
    /// Sites with a usable trail status source (Nordic France widget, ENJ city or box).
    pub with_status_source: usize,
    /// Sites with trail starts found by the stations layer (X9).
    pub with_starts: usize,
    /// ENJ domains detected from the ENJ data (X12b); absent without personal sources.
    pub enj: Option<EnjDetection>,
}

/// Builds the registry from the census (`data/out/france/domaines/domaines.geojson`), the
/// snowtools massifs and the site tables; adds the SRN areas of [`SRN_AREAS`] when present.
pub fn build_registry(ctx: &Ctx) -> Result<RegistrySummary> {
    let census_path = ctx
        .layout
        .france_layer_dir(skimap_osm::census::LAYER)?
        .join("domaines.geojson");
    let features = read_features(&census_path)
        .context("recensement absent : lancer d'abord `skimap domaines`")?;
    let massifs = skimap_snow::massifs::fetch(&ctx.dl)?;
    let mut records = census_records(&features, &massifs)?;
    let census_domains = features.len();
    let extra_path = ctx.layout.france_dir().join(SRN_AREAS);
    let mut extra = 0;
    if extra_path.is_file() {
        let e = srn_records(&read_features(&extra_path)?, &massifs)?;
        extra = e.iter().filter(|r| r.parent.is_none()).count();
        records.extend(e);
    }
    let with_starts = apply_departs(&ctx.layout, &mut records)?;
    let enj = detect_enj(ctx, &mut records)?;
    let registry = Registry::from_records(records.clone())?;
    let path = Registry::path(&ctx.layout)?;
    let dir = path.parent().expect("registry dir").to_owned();
    DataLayout::ensure(&dir)?;
    let notes = vec![
        "Un site par domaine du recensement P3 (data/out/france/domaines/domaines.geojson) ; identifiant : nom en minuscules sans accents (site::slug), suffixe des chiffres de l'identifiant du recensement pour les homonymes sauf le plus grand.".to_owned(),
        format!("Emprise : enveloppe du domaine + {DOMAIN_MARGIN_M:.0} m (Lambert-93), sauf les sites d'évaluation (table data/sites.json de skimap-common). Au-delà de {MAX_SITE_SIDE_M:.0} m de côté, le site est découpé en tronçons recouvrants ({SECTION_OVERLAP_M:.0} m)."),
        format!("Massifs : polygones snowtools (massifs.shp), points tous les {MASSIF_SAMPLE_M:.0} m dans l'enveloppe, massifs d'au moins {:.0} % ; BERA si le massif principal est un massif du BERA.", MASSIF_MIN_SHARE * 100.0),
        "État des pistes : rapport 08 (data/status_sources.json de skimap-common) ; seuls les widgets Nordic France et l'ENJ sont lisibles par skimap-status (usage personnel). Domaines ENJ : ceux dont l'enveloppe est traversée par au moins un itinéraire de ski de l'ENJ (API Geotrek, pratiques ski de fond et multi-activités), détectés à l'écriture du registre et par `skimap enj-domaines` ; aucune liste à la main.".to_owned(),
    ];
    Registry::write(
        &path,
        &RegistryFile {
            generated_at: now_rfc3339(),
            notes: notes.clone(),
            sites: records.clone(),
        },
    )?;
    let mut meta = LayerMeta::new(site::REGISTRY_LAYER, skimap_common::layout::FRANCE);
    meta.tickets = vec!["G2 (#118)".into()];
    meta.crs = Some("EPSG:4326 (WGS84) ; emprises calculées en Lambert-93 (EPSG:2154)".into());
    meta.sources = vec![
        SourceRef::new(
            "Recensement des domaines de fond (P3), couche france/domaines",
            census_path.display().to_string(),
            "ODbL 1.0 (OSM, OpenSkiMap) ; communes : Licence Ouverte 2.0",
            "© les contributeurs d'OpenStreetMap, OpenSkiMap.org",
        ),
        SourceRef::new(
            "Contours des massifs (snowtools, massifs.shp)",
            skimap_snow::massifs::url("shp"),
            "CeCILL-C",
            "Météo-France / CNRS, CNRM (UMR-CNRM/snowtools)",
        ),
    ];
    meta.files = vec![FileMeta::new(
        site::REGISTRY_FILE,
        "sites : identifiant, nom, type, emprise, massifs, département, sources d'état, tronçons",
    )
    .count(records.len())];
    meta.notes = notes;
    meta.write(&dir)?;
    let mut by_status = BTreeMap::new();
    for r in registry.records() {
        *by_status
            .entry(format!("{:?}", r.massif_status).to_lowercase())
            .or_insert(0) += 1;
    }
    Ok(RegistrySummary {
        path,
        sites: records.len(),
        buildable: registry.buildable().len(),
        census_domains,
        extra,
        by_massif_status: by_status,
        with_bera: records.iter().filter(|r| r.has_bera).count(),
        with_status_source: records
            .iter()
            .filter(|r| {
                r.status.nordicfrance_station.is_some()
                    || r.status.enj_city.is_some()
                    || r.status.enj_bbox.is_some()
            })
            .count(),
        with_starts,
        enj,
    })
}

// --- SRN layer of a site ------------------------------------------------------------

fn geometry_box(g: &Value) -> Option<BBox> {
    fn walk(v: &Value, b: &mut BBox) {
        match v.as_array() {
            Some(a) if a.len() >= 2 && a[0].is_number() => {
                let (x, y) = (a[0].as_f64().unwrap_or(0.0), a[1].as_f64().unwrap_or(0.0));
                b.west = b.west.min(x);
                b.east = b.east.max(x);
                b.south = b.south.min(y);
                b.north = b.north.max(y);
            }
            Some(a) => a.iter().for_each(|x| walk(x, b)),
            None => {}
        }
    }
    let mut b = BBox::new(
        f64::INFINITY,
        f64::INFINITY,
        f64::NEG_INFINITY,
        f64::NEG_INFINITY,
    );
    walk(&g["coordinates"], &mut b);
    b.west.is_finite().then_some(b)
}

fn boxes_meet(a: &BBox, b: &BBox) -> bool {
    a.west <= b.east && b.west <= a.east && a.south <= b.north && b.south <= a.north
}

/// Writes `data/out/<site>/srn/`: the SRN areas and itineraries of the France layer
/// (`data/out/france/srn/`, G5) whose box meets the site box, with the France metadata
/// (site, counts). Returns the numbers of areas and itineraries written; nothing is
/// written (and a previous layer is removed) when none meets the box.
pub fn write_site_srn(
    layout: &DataLayout,
    site: &skimap_common::site::Site,
) -> Result<(usize, usize)> {
    let france = layout.france_layer_dir(skimap_srn::build::LAYER)?;
    let pick = |file: &str| -> Result<Vec<Value>> {
        Ok(read_features(&france.join(file))?
            .into_iter()
            .filter(|f| geometry_box(&f["geometry"]).is_some_and(|b| boxes_meet(&b, &site.bbox)))
            .collect())
    };
    let areas = pick(skimap_srn::build::AREAS_FILE)?;
    let routes = pick(skimap_srn::build::ROUTES_FILE)?;
    let dir = layout.layer_dir(site, skimap_srn::build::LAYER)?;
    if areas.is_empty() && routes.is_empty() {
        if dir.is_dir() {
            std::fs::remove_dir_all(&dir).with_context(|| format!("{}", dir.display()))?;
        }
        return Ok((0, 0));
    }
    DataLayout::ensure(&dir)?;
    skimap_common::output::write_geojson(&dir.join(skimap_srn::build::AREAS_FILE), &areas)?;
    skimap_common::output::write_geojson(&dir.join(skimap_srn::build::ROUTES_FILE), &routes)?;
    let mut meta = LayerMeta::read(&france.join(LayerMeta::file_name(skimap_srn::build::LAYER)))?;
    meta.site = site.id.to_owned();
    meta.generated_at = now_rfc3339();
    for f in &mut meta.files {
        if f.name == skimap_srn::build::AREAS_FILE {
            f.count = Some(areas.len());
        } else if f.name == skimap_srn::build::ROUTES_FILE {
            f.count = Some(routes.len());
        }
    }
    meta.files.retain(|f| {
        f.name == skimap_srn::build::AREAS_FILE || f.name == skimap_srn::build::ROUTES_FILE
    });
    meta.notes.push(format!(
        "Zones et itinéraires de la couche France (data/out/france/srn/) dont l'emprise rencontre celle du site ({}).",
        site.bbox.overpass()
    ));
    meta.write(&dir)?;
    Ok((areas.len(), routes.len()))
}

// --- Build records and catalogue ------------------------------------------------------

/// What makes a build of a site out of date: the site record, the pipeline version and
/// the personal-sources option.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BuildInputs {
    /// Site record.
    pub record: SiteRecord,
    /// `skimap` version.
    pub version: String,
    /// `personal_sources`.
    pub personal_sources: bool,
    /// [`BUILD_REVISION`] of the steps (0 for records written before it existed).
    #[serde(default)]
    pub revision: u32,
}

/// Revision of the steps of `build-all`, bumped when a step is added or starts to run in
/// a mode where it did not (a site built with another revision is not up to date; its
/// steps are still taken over when their outputs are intact, [`crate::build::Resume`]).
/// 2: `meteo` in public mode (AROME, X17).
pub const BUILD_REVISION: u32 = 2;

impl BuildInputs {
    /// Whether the steps of a build with `self` can be taken over by a build with `other`:
    /// same site and same `personal_sources` (the pipeline version and revision may
    /// differ: the fingerprints of the outputs decide; the site record may differ: each
    /// step is taken over only with the same fields of it, those it reads,
    /// [`crate::build::step_inputs`], #164).
    pub fn resumable_by(&self, other: &BuildInputs) -> bool {
        self.record.id == other.record.id && self.personal_sources == other.personal_sources
    }
}

/// `data/out/<site>/build-all.json`: the last `build-all` of a site.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BuildRecord {
    /// Inputs.
    pub inputs: BuildInputs,
    /// Start (RFC 3339).
    pub started_at: String,
    /// End (RFC 3339).
    pub finished_at: String,
    /// Whether every step succeeded or did not apply.
    pub ok: bool,
    /// Outcome.
    pub outcome: crate::build::BuildOutcome,
}

/// Build record file of a site.
pub const BUILD_RECORD: &str = "build-all.json";
/// Build log file of a site.
pub const BUILD_LOG: &str = "build-all.log";

/// Path of the build record of a site.
pub fn record_path(layout: &DataLayout, id: &str) -> PathBuf {
    layout.out_root().join(id).join(BUILD_RECORD)
}

/// Reads the build record of a site, if any.
pub fn read_record(layout: &DataLayout, id: &str) -> Option<BuildRecord> {
    let p = record_path(layout, id);
    serde_json::from_slice(&std::fs::read(p).ok()?).ok()
}

fn dir_bytes(dir: &Path) -> u64 {
    let Ok(rd) = std::fs::read_dir(dir) else {
        return 0;
    };
    rd.filter_map(|e| e.ok())
        .map(|e| {
            let p = e.path();
            match e.file_type() {
                Ok(t) if t.is_dir() => dir_bytes(&p),
                Ok(_) => e.metadata().map_or(0, |m| m.len()),
                Err(_) => 0,
            }
        })
        .sum()
}

/// Summary of a catalogue.
#[derive(Debug, Serialize)]
pub struct CatalogSummary {
    /// `catalog.json`.
    pub path: PathBuf,
    /// Sites listed.
    pub sites: usize,
    /// Sites per build state.
    pub by_state: BTreeMap<String, usize>,
    /// Total pack size, bytes.
    pub pack_bytes: u64,
}

fn bbox_json(b: BBox) -> Value {
    json!([b.west, b.south, b.east, b.north])
}

/// Writes `data/out/france/catalog.json` and `catalog.geojson`: every site of the
/// registry (`domains[]`: stations, SRN areas, traverse sections) with its box
/// `[west, south, east, north]`, centre `[lon, lat]`, km, commune `{name, insee,
/// departement}`, massifs, status source, build state and pack (`dir` relative to
/// `data/out/`, `bytes`, `version`, `generated_at`), as read by `tools/viewer`.
pub fn write_catalog(layout: &DataLayout, registry: &Registry) -> Result<CatalogSummary> {
    let mut entries = Vec::new();
    let mut features = Vec::new();
    let mut by_state: BTreeMap<String, usize> = BTreeMap::new();
    let mut total = 0u64;
    for r in registry.records() {
        let pack_dir = layout
            .out_root()
            .join(&r.id)
            .join(skimap_pack::build::PACK_LAYER);
        let manifest: Option<Value> = std::fs::read(pack_dir.join("manifest.json"))
            .ok()
            .and_then(|b| serde_json::from_slice(&b).ok());
        let rec = read_record(layout, &r.id);
        let state = if !r.buildable() {
            "sections"
        } else {
            match (&rec, &manifest) {
                (Some(b), _) if b.inputs.record != *r => "stale",
                (Some(b), Some(_)) if b.ok => "built",
                (Some(_), Some(_)) => "built_with_failures",
                (Some(_), None) => "failed",
                (None, Some(_)) => "built_outside_build_all",
                (None, None) => "not_built",
            }
        };
        *by_state.entry(state.to_owned()).or_insert(0) += 1;
        let pack = manifest.as_ref().map(|m| {
            let bytes = m["total_bytes"]
                .as_u64()
                .unwrap_or_else(|| dir_bytes(&pack_dir));
            total += bytes;
            json!({
                "built": true,
                "dir": format!("{}/{}/", r.id, skimap_pack::build::PACK_LAYER),
                "version": m["pack_version"],
                "generated_at": m["generated_at"],
                "bytes": bytes,
                "layers": m["layers"].as_array().map_or(0, Vec::len),
                "personal_sources": m["personal_sources"],
            })
        });
        let failed: Vec<Value> = rec
            .as_ref()
            .map(|b| {
                b.outcome
                    .failed()
                    .iter()
                    .map(|s| json!({"step": s.name, "error": s.line}))
                    .collect()
            })
            .unwrap_or_default();
        let not_applicable: Vec<Value> = rec
            .as_ref()
            .map(|b| {
                b.outcome
                    .steps
                    .iter()
                    .filter(|s| s.state == crate::build::StepState::NotApplicable)
                    .map(|s| json!({"step": s.name, "why": s.line}))
                    .collect()
            })
            .unwrap_or_default();
        let center = r.commune_center;
        let entry = json!({
            "id": r.id,
            "name": r.name,
            "kind": r.kind,
            "parent": r.parent,
            "sections": r.sections,
            "census_id": r.census_id,
            "department": r.department,
            "commune": {"name": r.commune, "insee": r.insee, "departement": r.department},
            "center": [center.lon, center.lat],
            "bbox": bbox_json(r.bbox),
            "envelope_bbox": r.envelope_bbox.map(bbox_json),
            "km_nordic": r.km_nordic,
            "km_routes": r.km_routes,
            "km_hike_only": r.km_hike_only,
            "massifs": r.massifs.iter().map(|m| json!({"code": m.code, "name": m.name})).collect::<Vec<_>>(),
            "has_bera": r.has_bera,
            "status_source": r.status_source.as_ref().map(|s| json!({"kind": s.kind, "url": s.url})),
            "build": {
                "state": state,
                "finished_at": rec.as_ref().map(|b| b.finished_at.clone()),
                "seconds": rec.as_ref().map(|b| b.outcome.seconds),
                "failed": failed,
                "not_applicable": not_applicable,
            },
            "pack": pack,
        });
        let mut props = entry.clone();
        if let Some(o) = props.as_object_mut() {
            o.remove("bbox");
        }
        let b = r.bbox;
        features.push(json!({
            "type": "Feature",
            "geometry": {"type": "Polygon", "coordinates": [[
                [b.west, b.south], [b.east, b.south], [b.east, b.north], [b.west, b.north], [b.west, b.south]
            ]]},
            "properties": props,
        }));
        entries.push(entry);
    }
    let dir = layout.france_dir();
    DataLayout::ensure(&dir)?;
    let path = dir.join("catalog.json");
    skimap_common::output::write_json(
        &path,
        &json!({
            "format": "skimap-france-catalog",
            "format_version": 1,
            "generated_at": now_rfc3339(),
            "registry": Registry::path(layout)?.display().to_string(),
            "domains": entries,
            "by_state": by_state,
            "pack_bytes": total,
        }),
    )?;
    skimap_common::output::write_geojson(&dir.join("catalog.geojson"), &features)?;
    Ok(CatalogSummary {
        path,
        sites: registry.records().len(),
        by_state,
        pack_bytes: total,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn square(x0: f64, y0: f64, s: f64) -> Polys {
        vec![vec![vec![
            (x0, y0),
            (x0 + s, y0),
            (x0 + s, y0 + s),
            (x0, y0 + s),
        ]]]
    }

    #[test]
    fn feature_boxes() {
        let g = json!({"type": "MultiLineString", "coordinates": [[[6.0, 46.0], [6.2, 46.1]], [[5.9, 46.3], [6.0, 46.2]]]});
        let b = geometry_box(&g).unwrap();
        assert_eq!(b, BBox::new(46.0, 5.9, 46.3, 6.2));
        assert!(boxes_meet(&b, &BBox::new(46.25, 6.1, 46.5, 6.4)));
        assert!(!boxes_meet(&b, &BBox::new(46.4, 6.1, 46.5, 6.4)));
        assert!(geometry_box(&json!({"type": "Point", "coordinates": []})).is_none());
    }

    #[test]
    fn enj_domains_detected_from_the_itineraries() {
        let trek = |id: i64, b: &BBox, inside: bool| {
            let c = b.center();
            let lat = if inside { c.lat } else { b.north + 0.005 };
            skimap_status::enj::TrekGeometry {
                id,
                name: format!("t{id}"),
                practice: Some("Ski de fond".into()),
                geometry: json!({"type": "LineString",
                    "coordinates": [[b.west - 0.01, lat, 1000.0], [b.east + 0.01, lat, 1000.0]]}),
            }
        };
        let mut recs: Vec<SiteRecord> = site::evaluation_records().to_vec();
        assert!(recs.len() >= 2);
        // Test data: no ENJ city, one record crossed, one crossed only within 500 m of its
        // box (what `in_bbox=` of the API would still select), one an SRN area.
        for r in &mut recs {
            r.status.enj_city = None;
            r.status.enj_bbox = Some(BBox::new(0.0, 0.0, 1.0, 1.0));
            r.kind = SiteKind::NordicStation;
            r.sections.clear();
            r.status_source = None;
        }
        recs[1].status_source = Some(site::StatusSourceInfo {
            kind: "enj".into(),
            name: String::new(),
            url: None,
            nordicfrance_station: None,
            authorisation_required: String::new(),
            evidence: String::new(),
        });
        let b0 = enj_box(&recs[0]);
        let b1 = enj_box(&recs[1]);
        let treks = vec![trek(1, &b0, true), trek(2, &b0, true), trek(3, &b1, false)];
        let d = apply_enj(&mut recs, &treks);
        assert_eq!(d.itineraries, 3);
        assert_eq!(recs[0].status.enj_bbox, Some(b0));
        assert_eq!(d.sites.get(&recs[0].id), Some(&2));
        assert_eq!(recs[1].status.enj_bbox, None, "not crossed: not polled");
        assert_eq!(d.report08_not_detected, [recs[1].id.clone()]);
        assert_eq!(d.detected_not_in_report08, [recs[0].id.clone()]);
        // A site with an ENJ city keeps it (no box), an SRN area is never polled.
        recs[0].status.enj_city = Some(14);
        recs[1].kind = SiteKind::SrnArea;
        let treks = vec![trek(1, &b0, true), trek(3, &b1, true)];
        let d = apply_enj(&mut recs, &treks);
        assert_eq!(recs[0].status.enj_bbox, None);
        assert!(d.sites.contains_key(&recs[0].id));
        assert_eq!(recs[1].status.enj_bbox, None);
        assert!(!d.sites.contains_key(&recs[1].id));
        let site = recs[0].to_site();
        assert_eq!(
            skimap_status::enj::area(&site),
            Some(skimap_status::enj::EnjArea::City(14))
        );
        recs[0].status.enj_city = None;
        recs[0].status.enj_bbox = Some(b0);
        let site = recs[0].to_site();
        assert_eq!(
            skimap_status::enj::area(&site),
            Some(skimap_status::enj::EnjArea::BBox(b0))
        );
        assert!(!site.status.is_empty());
    }

    #[test]
    fn point_in_envelope() {
        let p = square(0.0, 0.0, 10.0);
        assert!(inside(&p, (5.0, 5.0)));
        assert!(!inside(&p, (15.0, 5.0)));
        // A hole.
        let mut h = p.clone();
        h[0].push(vec![(4.0, 4.0), (6.0, 4.0), (6.0, 6.0), (4.0, 6.0)]);
        assert!(!inside(&h, (5.0, 5.0)));
        assert!(inside(&h, (2.0, 2.0)));
    }

    #[test]
    fn domain_box_and_sections() {
        // Lambert-93 around Chapelle-des-Bois: a 7 km domain is one site, a 60 km area is
        // split into sections of at most 25 km.
        let (x, y) = crs::to_lambert93(LonLat::new(6.11, 46.60)).unwrap();
        let (b, s) = boxes(&square(x, y, 7_000.0), DOMAIN_MARGIN_M).unwrap();
        assert!(s.is_empty());
        let (w, h) = b.size_m();
        assert!(
            (10_900.0..11_600.0).contains(&w) && (10_900.0..11_600.0).contains(&h),
            "{w} {h}"
        );
        let (_, s) = boxes(&square(x, y, 60_000.0), 0.0).unwrap();
        assert!(s.len() >= 9, "{}", s.len());
        for (b, _) in &s {
            let (w, h) = b.size_m();
            assert!(
                w <= MAX_SITE_SIDE_M * 1.05 && h <= MAX_SITE_SIDE_M * 1.05,
                "{w} {h}"
            );
        }
        let mut out = Vec::new();
        let mut rec = site::evaluation_records()[1].clone();
        rec.id = "grand".into();
        rec.name = "Grand".into();
        let polys = square(x, y, 60_000.0);
        add_sections(&mut out, rec, s.clone(), &polys, &[]);
        assert_eq!(out.len(), s.len() + 1);
        assert_eq!(out[0].sections.len(), s.len());
        assert_eq!(out[1].parent.as_deref(), Some("grand"));
        assert!(out[1].id.starts_with("grand-"));
        assert!(
            out[1].name.ends_with(&format!("/{})", s.len())),
            "{}",
            out[1].name
        );
        Registry::from_records(out).unwrap();
    }

    #[test]
    fn srn_sites_and_sections_take_their_starts() {
        use skimap_stations::departs::{Depart, DepartsFile, Method};
        // The stations crate names the SRN files without depending on skimap-srn.
        assert_eq!(
            skimap_stations::departs_srn::SRN_LAYER,
            skimap_srn::build::LAYER
        );
        assert_eq!(
            skimap_stations::departs_srn::AREAS_FILE,
            skimap_srn::build::AREAS_FILE
        );
        assert_eq!(
            skimap_stations::departs_srn::ROUTES_FILE,
            skimap_srn::build::ROUTES_FILE
        );
        let d = |lon: f64, score: f64| Depart {
            start: site::Start {
                id: String::new(),
                name: format!("{lon}"),
                phrase: String::new(),
                phrase_en: None,
                lon,
                lat: 46.0,
                provenance: String::new(),
                access_m: None,
            },
            method: Method::Points,
            score,
            points: Vec::new(),
            network: None,
        };
        let file = DepartsFile {
            generated_at: String::new(),
            osm_valid_at: None,
            notes: Vec::new(),
            domains: BTreeMap::new(),
            srn: BTreeMap::from([(
                "srn-traversee-x".to_owned(),
                vec![d(6.0, 9.0), d(6.1, 8.0), d(6.5, 2.0)],
            )]),
        };
        let mut parent = site::evaluation_records()[1].clone();
        parent.id = "srn-traversee-x".into();
        parent.kind = SiteKind::Traverse;
        parent.census_id = None;
        let lon = |v: Vec<site::Start>| v.iter().map(|s| s.lon).collect::<Vec<_>>();
        assert_eq!(lon(srn_starts(&file, &parent)), vec![6.0, 6.1]);
        let mut section = parent.clone();
        section.id = "srn-traversee-x-2".into();
        section.parent = Some(parent.id.clone());
        section.bbox = BBox::new(45.9, 6.4, 46.1, 6.6);
        assert_eq!(lon(srn_starts(&file, &section)), vec![6.5]);
        // A section without a start inside: the best of the area.
        section.bbox = BBox::new(45.9, 7.0, 46.1, 7.2);
        assert_eq!(lon(srn_starts(&file, &section)), vec![6.0]);
        parent.id = "other".into();
        assert!(srn_starts(&file, &parent).is_empty());
    }
}
