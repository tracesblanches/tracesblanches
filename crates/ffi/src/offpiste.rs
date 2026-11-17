//! Off-piste SRN routing in the app (V1 #64, F16): a route from A to B off the trails
//! with the core's router (C3: slope limit, zones at the date, avalanche exposure, snow
//! of the pack), its difficulty, reasons and advice in both directions (SR1,
//! `skimap-srndiff`), its time at the group's pace (C11), then guidance along it by the
//! same location service as the day plan ([`crate::fog::GuidedSession`], line mode).
//!
//! The pack carries the off-piste rasters gzipped (`offpiste-*.skr.gz`); they are inflated
//! once into the app's private storage ([`inflate_offpiste`], kept while the pack does not
//! change) and read with `std` only. The request that dominates (C4: 10 × 10 km window at
//! 5 m) runs in about 0.2 s on a workstation; [`OffpisteRouteView::compute_ms`] gives the
//! time on the phone.
//!
//! Never « sans danger »: the text of a route always ends with « Indication calculée sur
//! le relief et les cartes, jamais une garantie » (srndiff), « terrain peu exposé » is a
//! model output and the BERA of the day stays the reference.

use std::io::Read as _;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use chrono::{DateTime, Utc};
use skimap_common::geom::LonLat;
use skimap_common::grid::Grid;
use skimap_core::calibration::{ProfileModel, TERRAINS, Terrain};
use skimap_core::eta::EtaModel;
use skimap_core::offpiste::{
    self, CostParams, ExposureFactors, OffpisteGrid, Overlay, Router, SnowGrid, ZonePolicy,
};
use skimap_core::routing::Style;
use skimap_pack::Pack;
use skimap_srndiff::field::{Areas, Classes, ElevationDm, SiteZones, SlopeHalfDeg};
use skimap_srndiff::{Layers, Params, assess, profile};
use skimap_zones::layer::ZoneLayer;
use skimap_zones::rules::RuleSet;

use crate::card::Lang;
use crate::fog::{FogTarget, GuidedKind, GuidedSpec, personal_time};
use crate::text;
use crate::tracking::TrackingError;

fn invalid(detail: impl std::fmt::Display) -> TrackingError {
    TrackingError::Invalid {
        detail: detail.to_string(),
    }
}

/// Inflates the off-piste layers of `pack` (`offpiste-*`, gzipped or not) into
/// `<work_dir>/offpiste/<site>/`, under their pipeline names (`elevation_5m.skr`…); kept
/// while the pack's file of each is the same (size and generation stamp). Returns the
/// directory.
pub fn inflate_offpiste(pack: &Pack, work_dir: &Path) -> Result<PathBuf, TrackingError> {
    let site = &pack.manifest().site.id;
    let out = work_dir.join("offpiste").join(site);
    std::fs::create_dir_all(&out).map_err(invalid)?;
    for l in pack.manifest().layers.iter() {
        if !l.id.starts_with("offpiste-") {
            continue;
        }
        let name = Path::new(&l.origin)
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .ok_or_else(|| invalid(format!("{} : origine sans nom", l.id)))?;
        // Snow: one file whatever its day (the router reads one snow grid).
        let name = if name.starts_with("snow_") {
            "snow.skr".to_owned()
        } else {
            name
        };
        let src = pack.path(&l.file).map_err(invalid)?;
        let stamp = format!("{} {}", l.bytes, l.generated_at.clone().unwrap_or_default());
        let dest = out.join(&name);
        let stamp_path = out.join(format!("{name}.stamp"));
        if dest.is_file() && std::fs::read_to_string(&stamp_path).ok().as_deref() == Some(&stamp) {
            continue;
        }
        let bytes = std::fs::read(&src).map_err(invalid)?;
        let data = if l.file.ends_with(".gz") {
            let mut v = Vec::new();
            flate2::read::GzDecoder::new(bytes.as_slice())
                .read_to_end(&mut v)
                .map_err(|e| invalid(format!("{}: {e}", src.display())))?;
            v
        } else {
            bytes
        };
        let tmp = out.join(format!("{name}.tmp"));
        std::fs::write(&tmp, data).map_err(invalid)?;
        std::fs::rename(&tmp, &dest).map_err(invalid)?;
        std::fs::write(&stamp_path, stamp).map_err(invalid)?;
    }
    Ok(out)
}

/// The relief of a pack (5 m elevation of the off-piste rasters), for the altitude of a
/// trace ([`crate::recap`]).
#[derive(Debug, Clone)]
pub struct Relief {
    grid: Grid<u16>,
}

impl Relief {
    /// Opens the relief of the pack in `pack_dir` (inflated into `work_dir`).
    pub fn open(pack_dir: &Path, work_dir: &Path) -> Result<Self, TrackingError> {
        let pack = Pack::open(pack_dir.to_path_buf()).map_err(invalid)?;
        let dir = inflate_offpiste(&pack, work_dir)?;
        let grid = offpiste::read_raster(&dir.join(offpiste::FILE_ELEVATION)).map_err(invalid)?;
        Ok(Self { grid })
    }

    /// Elevation at a position, metres.
    pub fn at(&self, p: LonLat) -> Option<f64> {
        let (x, y) = skimap_common::crs::to_lambert93(p).ok()?;
        let (c, r) = self.grid.cell_at(x, y)?;
        offpiste::elevation_m(*self.grid.get(c, r)?).map(f64::from)
    }
}

/// Everything a route reads, loaded once per pack.
struct Inputs {
    grid: OffpisteGrid,
    snow: Option<SnowGrid>,
    exposure: Option<Grid<u8>>,
    overlays: Vec<Overlay>,
    zones: Option<(ZoneLayer, RuleSet)>,
    network: Option<Areas>,
}

/// An off-piste route computed.
#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct OffpisteRouteView {
    /// « Le Laus → col d'Izoard ».
    pub label: String,
    /// Geometry, `[lon, lat, …]`.
    pub line: Vec<f64>,
    /// Length, km.
    pub length_km: f64,
    /// Climb and descent, m.
    pub ascent_m: f64,
    /// Descent, m.
    pub descent_m: f64,
    /// Steepest cell crossed, degrees (always below the limit of 30°).
    pub max_slope_deg: f64,
    /// Time: « environ 2 h 10 (1 h 50 à 2 h 40) au rythme du groupe » or « 2 h 10 au
    /// modèle ».
    pub time_text: String,
    /// Time, seconds (median).
    pub time_s: f64,
    /// Level A→B: « facile », « moyen », « difficile », « très difficile ».
    pub level: String,
    /// Level B→A.
    pub level_reverse: String,
    /// Reasons of the level A→B.
    pub reasons: Vec<String>,
    /// Advice A→B.
    pub advice: Vec<String>,
    /// Text of both directions (srndiff), ends with « jamais une garantie ».
    pub text: String,
    /// Zones where the route is restricted only in places, at the date.
    pub in_places: Vec<String>,
    /// Ends moved to the nearest passable cell, metres (A, B).
    pub snap_m: Vec<f64>,
    /// « Jamais une garantie » ([`text::Texts::never_guarantee`]).
    pub never_guarantee: String,
    /// Layers missing from the pack (« non évalué »).
    pub missing: Vec<String>,
    /// Time of the request on this phone, milliseconds.
    pub compute_ms: u32,
}

struct Last {
    view: OffpisteRouteView,
    model_s: [f64; TERRAINS],
    to: FogTarget,
    group_json: Option<String>,
}

/// The off-piste router of a pack.
#[derive(uniffi::Object)]
pub struct OffpistePlanner {
    pack_dir: PathBuf,
    site_id: String,
    inputs: Inputs,
    missing: Vec<String>,
    /// Language of the texts (VO6).
    lang: Lang,
    last: Mutex<Option<Last>>,
    load_ms: u32,
}

impl std::fmt::Debug for OffpistePlanner {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("OffpistePlanner")
            .field("pack_dir", &self.pack_dir)
            .finish_non_exhaustive()
    }
}

fn ms_since(t: Instant) -> u32 {
    u32::try_from(t.elapsed().as_millis()).unwrap_or(u32::MAX)
}

/// Opens the off-piste router of the pack in `pack_dir`; `work_dir` (private storage)
/// receives its inflated rasters; texts in `lang`.
#[uniffi::export]
pub fn open_offpiste(
    pack_dir: String,
    work_dir: String,
    lang: Lang,
) -> Result<Arc<OffpistePlanner>, TrackingError> {
    let t0 = Instant::now();
    let tx = text::of(lang);
    let pack = Pack::open(PathBuf::from(&pack_dir)).map_err(invalid)?;
    if pack.layer("offpiste-elevation").is_err() {
        return Err(invalid(tx.no_offpiste_relief()));
    }
    let dir = inflate_offpiste(&pack, Path::new(&work_dir))?;
    let grid = OffpisteGrid::load(&dir).map_err(invalid)?;
    let mut missing = Vec::new();
    let snow = SnowGrid::load(&dir.join("snow.skr")).ok();
    if snow.is_none() {
        missing.push(tx.missing_layer("snow").to_owned());
    }
    let exposure: Option<Grid<u8>> = offpiste::read_raster(&dir.join(offpiste::FILE_EXPOSURE)).ok();
    if exposure.is_none() {
        missing.push(tx.missing_layer("exposure").to_owned());
    }
    let overlays = exposure
        .iter()
        .map(|g| ExposureFactors::default().overlay(g.clone()))
        .collect();
    let text = |id: &str| -> Option<String> {
        let l = pack.layer(id).ok()?;
        std::fs::read_to_string(pack.path(&l.file).ok()?).ok()
    };
    let zones = match (text("zones"), text("zones-rules")) {
        (Some(z), Some(r)) => ZoneLayer::from_geojson_str(&z)
            .ok()
            .zip(RuleSet::from_json_str(&r).ok()),
        _ => None,
    };
    if zones.is_none() {
        missing.push(tx.missing_layer("zones").to_owned());
    }
    let network = text("no-network").and_then(|n| {
        let polys = Areas::polygons_from_geojson(&n).ok()?;
        let known = text("coverage-area").and_then(|a| Areas::polygons_from_geojson(&a).ok());
        Some(Areas::new(polys, known))
    });
    if network.is_none() {
        missing.push(tx.missing_layer("network").to_owned());
    }
    Ok(Arc::new(OffpistePlanner {
        pack_dir: PathBuf::from(pack_dir),
        site_id: pack.manifest().site.id.clone(),
        inputs: Inputs {
            grid,
            snow,
            exposure,
            overlays,
            zones,
            network,
        },
        missing,
        lang,
        last: Mutex::new(None),
        load_ms: ms_since(t0),
    }))
}

/// Model time (C5, off the trails) per terrain along a profile's samples.
fn model_times(samples: &[skimap_srndiff::Sample]) -> [f64; TERRAINS] {
    let eta = EtaModel::new(Style::Classic);
    let mut out = [0.0; TERRAINS];
    for w in samples.windows(2) {
        let d = w[1].s - w[0].s;
        if d <= 0.0 {
            continue;
        }
        let (Some(z0), Some(z1)) = (w[0].z, w[1].z) else {
            out[Terrain::Flat as usize] += d / eta.speed_ms(0.0, eta.default_elev_m, true);
            continue;
        };
        let grade = f64::from(z1 - z0) / d;
        let v = eta.speed_ms(grade, f64::from(z0), true).max(0.2);
        out[Terrain::of_grade(grade) as usize] += d / v;
    }
    out
}

#[uniffi::export]
impl OffpistePlanner {
    /// Site of the pack.
    pub fn site_id(&self) -> String {
        self.site_id.clone()
    }

    /// Time to load the rasters, milliseconds.
    pub fn load_ms(&self) -> u32 {
        self.load_ms
    }

    /// Layers the pack lacks (« non évalué » in the difficulty).
    pub fn missing(&self) -> Vec<String> {
        self.missing.clone()
    }

    /// The route from `from` to `to` at `at_ms` (epoch ms: zones and their periods at that
    /// instant), with the time of the group `group_json` (C11) when given.
    pub fn route(
        &self,
        from: FogTarget,
        to: FogTarget,
        at_ms: i64,
        group_json: Option<String>,
    ) -> Result<OffpisteRouteView, TrackingError> {
        let t0 = Instant::now();
        let tx = text::of(self.lang);
        let i = &self.inputs;
        let at: DateTime<Utc> = DateTime::from_timestamp_millis(at_ms).unwrap_or_else(Utc::now);
        let policy = match &i.zones {
            Some((layer, rules)) => offpiste::zone_policy(&i.grid.classes, layer, rules, at),
            None => ZonePolicy::open(i.grid.classes.len(), at),
        };
        let params = CostParams::default();
        let router = Router {
            grid: &i.grid,
            snow: i.snow.as_ref(),
            overlays: &i.overlays,
            policy: &policy,
            params: &params,
        };
        let a = LonLat::new(from.lon, from.lat);
        let b = LonLat::new(to.lon, to.lat);
        let r = router.route(a, b).map_err(|e| {
            invalid(match e {
                offpiste::RouteError::OutsideGrid(_) => tx.outside_relief().to_owned(),
                offpiste::RouteError::NoPassableCell { radius_m, .. } => tx.no_passable(radius_m),
                offpiste::RouteError::NoRoute { .. } => tx.no_offpiste_route().to_owned(),
                offpiste::RouteError::Crs(m) => m,
            })
        })?;
        let in_places: Vec<String> = {
            let mut v: Vec<String> = r
                .zone_classes
                .iter()
                .filter_map(|c| policy.classes.get(usize::from(*c)))
                .flat_map(|p| p.in_places.iter().cloned())
                .collect();
            v.sort();
            v.dedup();
            v
        };
        // Difficulty (SR1) on the same rasters.
        let elev = ElevationDm(&i.grid.elevation);
        let slope = SlopeHalfDeg(&i.grid.slope);
        let exp = i.exposure.as_ref().map(|g| Classes {
            grid: g,
            nodata: offpiste::EXPOSURE_NODATA,
        });
        let pistes = Classes {
            grid: &i.grid.pistes,
            nodata: u8::MAX,
        };
        let zones = i
            .zones
            .as_ref()
            .map(|(layer, rules)| SiteZones { layer, rules, at });
        let layers = Layers {
            elevation: Some(&elev),
            slope: Some(&slope),
            exposure: exp.as_ref().map(|e| e as &dyn skimap_srndiff::field::Field),
            pistes: Some(&pistes),
            no_network: i.network.as_ref(),
            zones: zones
                .as_ref()
                .map(|z| z as &dyn skimap_srndiff::field::ZoneSource),
            ..Layers::default()
        };
        let prof = profile::build(&r.points, &layers, None).map_err(invalid)?;
        let rep = assess(&prof, &Params::default(), None);
        let label = format!("{} → {}", from.name, to.name);
        let model_s = model_times(&prof.samples);
        let model = group_json
            .as_deref()
            .and_then(|j| serde_json::from_str::<ProfileModel>(j).ok());
        let (med, lo, hi) = personal_time(model.as_ref(), model_s);
        let time_text = if model.is_some() {
            tx.time_group(med, lo, hi)
        } else {
            tx.time_model(med)
        };
        let view = OffpisteRouteView {
            label: label.clone(),
            line: r.points.iter().flat_map(|p| [p.lon, p.lat]).collect(),
            length_km: r.length_m / 1000.0,
            ascent_m: r.ascent_m.round(),
            descent_m: r.descent_m.round(),
            max_slope_deg: f64::from(r.max_slope_deg),
            time_text,
            time_s: med,
            level: rep.forward.rating.level.label().to_owned(),
            level_reverse: rep.reverse.rating.level.label().to_owned(),
            reasons: rep
                .forward
                .rating
                .reasons
                .iter()
                .map(|x| x.text.clone())
                .collect(),
            advice: rep.forward.advice.iter().map(|x| x.text.clone()).collect(),
            text: rep.text_fr(&label),
            in_places,
            snap_m: vec![r.from_snap_m, r.to_snap_m],
            never_guarantee: tx.never_guarantee().to_owned(),
            missing: self.missing.clone(),
            compute_ms: ms_since(t0),
        };
        *self
            .last
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(Last {
            view: view.clone(),
            model_s,
            to,
            group_json,
        });
        Ok(view)
    }

    /// Writes into `dir` the guided outing along the last route (followed by the location
    /// service like a day plan).
    pub fn guide(&self, dir: String, lang: Lang) -> Result<(), TrackingError> {
        let g = self
            .last
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let last = g.as_ref().ok_or(TrackingError::NoMorning)?;
        GuidedSpec {
            kind: GuidedKind::Line,
            label: match lang {
                Lang::Fr => format!("Hors piste {}", last.view.label),
                Lang::En => format!("Off-piste {}", last.view.label),
            },
            target: last.to.clone(),
            line: last
                .view
                .line
                .as_chunks::<2>()
                .0
                .iter()
                .map(|c| [c[0], c[1]])
                .collect(),
            model_s: last.model_s,
            group_json: last.group_json.clone(),
            period_s: crate::fog::DEFAULT_PERIOD_S,
            lang: match lang {
                Lang::Fr => "fr".into(),
                Lang::En => "en".into(),
            },
            site_id: Some(self.site_id.clone()),
            pack_dir: Some(self.pack_dir.display().to_string()),
            created_at: Utc::now().to_rfc3339(),
        }
        .write(Path::new(&dir))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn data() -> PathBuf {
        std::env::var("SKIMAP_DATA_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|_| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../data"))
    }

    fn target(name: &str, lon: f64, lat: f64) -> FogTarget {
        FogTarget {
            kind: "point".into(),
            name: name.into(),
            lon,
            lat,
            distance_m: None,
        }
    }

    /// The C3 proof routes on the real packs (`docs/horspiste.md`): Le Laus → col
    /// d'Izoard (5.75 km, D+ 620 m with the exposure) and the Risoux in February.
    #[test]
    #[ignore = "needs data/out/<site>/pack of both sites"]
    fn real_packs_route_and_guide() {
        let cases = [
            ("cervieres", "offpiste_laus_izoard.geojson", 5.75, 620.0),
            (
                "chapelle-des-bois",
                "offpiste_risoux_fevrier.geojson",
                12.1,
                277.0,
            ),
        ];
        for (site, file, km, dplus) in cases {
            let pack = data().join("out").join(site).join("pack");
            let work = tempfile::tempdir().unwrap();
            let t = Instant::now();
            let p = open_offpiste(
                pack.display().to_string(),
                work.path().display().to_string(),
                Lang::Fr,
            )
            .unwrap();
            let open_ms = t.elapsed().as_millis();
            // Ends of the proof route.
            let v: serde_json::Value = serde_json::from_str(
                &std::fs::read_to_string(data().join("out").join(site).join("routes").join(file))
                    .unwrap(),
            )
            .unwrap();
            let f = v["features"]
                .as_array()
                .unwrap()
                .iter()
                .find(|f| f["properties"]["kind"] == "route")
                .unwrap();
            let c = f["geometry"]["coordinates"].as_array().unwrap();
            let pt = |x: &serde_json::Value| (x[0].as_f64().unwrap(), x[1].as_f64().unwrap());
            let (a, b) = (pt(&c[0]), pt(&c[c.len() - 1]));
            let fam = crate::tracking_day::group_default(crate::card::Lang::Fr).unwrap();
            let r = p
                .route(
                    target("A", a.0, a.1),
                    target("B", b.0, b.1),
                    1_771_146_000_000,
                    Some(fam.model_json),
                )
                .unwrap();
            eprintln!(
                "{site}: open {open_ms} ms, route {} ms, {:.2} km, D+ {} m, max {:.1}°, {} / {}, {}\n{}",
                r.compute_ms,
                r.length_km,
                r.ascent_m,
                r.max_slope_deg,
                r.level,
                r.level_reverse,
                r.time_text,
                r.text
            );
            // The proofs ran on the snow of their day; the pack carries the snow of the
            // day it was built (14/11 here): the Risoux route changes with it (C3 proof in
            // August, other snow: 10.2 km).
            assert!((r.length_km - km).abs() / km < 0.15, "{}", r.length_km);
            assert!((r.ascent_m - dplus).abs() / dplus < 0.25, "{}", r.ascent_m);
            // Never off the marked routes where the date forbids it, never in a forbidden
            // zone: every point of the line on an allowed cell.
            let g = &p.inputs.grid;
            let (layer, rules) = p.inputs.zones.as_ref().unwrap();
            let at = DateTime::from_timestamp_millis(1_771_146_000_000).unwrap();
            let policy = offpiste::zone_policy(&g.classes, layer, rules, at);
            let mut off_in_appb = 0;
            for c in r.line.as_chunks::<2>().0.iter() {
                let (col, row) = g.cell_of(LonLat::new(c[0], c[1])).unwrap().unwrap();
                let i = row * g.width() + col;
                let cp = &policy.classes[usize::from(g.zones.data()[i])];
                let off = g.pistes.data()[i] == offpiste::PISTE_NONE;
                assert!(!cp.blocked, "{site}: forbidden zone at {c:?}");
                if off && cp.offpiste_blocked {
                    off_in_appb += 1;
                }
            }
            assert_eq!(off_in_appb, 0, "{site}");
            assert!(r.max_slope_deg < 30.0);
            assert!(r.text.contains("jamais une garantie"));
            assert!(r.time_text.contains("au rythme du groupe"));
            assert_ne!(r.level, "facile");
            let dir = work.path().join("guided");
            p.guide(dir.display().to_string(), Lang::Fr).unwrap();
            let s = crate::fog::open_guided(dir.display().to_string()).unwrap();
            let fixes: Vec<(i64, f64, f64)> = r
                .line
                .as_chunks::<2>()
                .0
                .iter()
                .step_by(4)
                .enumerate()
                .map(|(k, c)| (1_771_146_000_000 + k as i64 * 15_000, c[0], c[1]))
                .collect();
            let o = s.push_fixes(crate::tracking::pack_fixes(&fixes)).unwrap();
            assert!(o.recap.is_some_and(|x| x.arrived), "{site}");
            // Second open: rasters already inflated.
            let t = Instant::now();
            let _ = open_offpiste(
                pack.display().to_string(),
                work.path().display().to_string(),
                Lang::Fr,
            )
            .unwrap();
            eprintln!("{site}: reopen {} ms", t.elapsed().as_millis());
        }
    }
}
