//! The morning on the phone (J5 #114, K9 #59, C11 #130): briefing, best time, wax, loops
//! with the group's time range, equipment, then « Valider la journée » turns the loop
//! chosen into a day plan and its flat table (`plan.json`, `plan.trk`) that the tracking
//! session follows ([`crate::tracking`]).
//!
//! The assistant and the plan read the layers of a site in the pipeline layout
//! (`<root>/out/<site>/<layer>/<file>`, [`SiteData::load`], [`Context::load`]). A pack
//! records where each of its files comes from ([`skimap_pack::manifest::Layer::origin`]):
//! [`PackView`] rebuilds that layout from the pack with symbolic links (one per file, in
//! the app's private storage), so that the phone runs exactly the loaders of the pipeline
//! on the pack's files. Layers the pack carries in another format (snow and avalanche
//! exposure GeoTIFFs, tiled as PMTiles for the map) are not linked: the loaders report them
//! missing, as on a workstation without them.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use chrono::{NaiveDate, NaiveDateTime, NaiveTime, Timelike};
use skimap_assist::inputs::{Paths, SiteData};
use skimap_common::geom::LonLat;
use skimap_common::layout::DataLayout;
use skimap_common::site::Site;
use skimap_core::calibration::{GroupProfile, Level, OutingObs, ProfileModel, terrain_ratio};
use skimap_core::graph::{Difficulty, PisteGraph};
use skimap_core::routing::Style;
use skimap_pack::Pack;
use skimap_plan::course::{Course, PlanLeg};
use skimap_plan::pace::{self, PaceRoute, clock_fr, duration_fr};
use skimap_plan::plan::{self, Context, DayPlan, KeyKind, Morning, PlanRequest};

use crate::tracking::{TrackingError, epoch_ms};

/// Plan file written by [`DayPlanner::validate`].
pub const PLAN_FILE: &str = "plan.json";
/// Flat table written by [`DayPlanner::validate`].
pub const TRACK_FILE: &str = "plan.trk";
/// Pack the plan was made on (its directory), read by the tracking session.
pub const PACK_FILE: &str = "pack.txt";

impl From<skimap_plan::Error> for TrackingError {
    fn from(e: skimap_plan::Error) -> Self {
        TrackingError::Invalid {
            detail: e.to_string(),
        }
    }
}

impl From<skimap_pack::Error> for TrackingError {
    fn from(e: skimap_pack::Error) -> Self {
        TrackingError::Invalid {
            detail: e.to_string(),
        }
    }
}

fn io(path: &Path, e: std::io::Error) -> TrackingError {
    TrackingError::Invalid {
        detail: format!("{}: {e}", path.display()),
    }
}

/// The pipeline layout of a pack's files, as symbolic links.
#[derive(Debug, Clone)]
pub struct PackView {
    /// Root of the layout (`<root>/out/<site>/<layer>/`).
    pub layout: DataLayout,
    /// Links made.
    pub links: usize,
}

impl PackView {
    /// Links every file of `pack` (layers, their metadata, the voice layer) under `root`
    /// at the place it comes from in the pipeline (`out/<site>/<origin>`). `root` is
    /// emptied first: a view is rebuilt from the pack it shows.
    pub fn build(pack: &Pack, root: &Path) -> Result<Self, TrackingError> {
        let site = &pack.manifest().site.id;
        let base = root.join("out").join(site);
        if base.exists() {
            std::fs::remove_dir_all(&base).map_err(|e| io(&base, e))?;
        }
        let mut links = 0;
        let mut link = |target: PathBuf, at: PathBuf| -> Result<(), TrackingError> {
            if let Some(dir) = at.parent() {
                std::fs::create_dir_all(dir).map_err(|e| io(dir, e))?;
            }
            if at.symlink_metadata().is_ok() {
                return Ok(());
            }
            symlink(&target, &at).map_err(|e| io(&at, e))?;
            links += 1;
            Ok(())
        };
        let ext = |p: &str| Path::new(p).extension().map(|e| e.to_ascii_lowercase());
        for l in &pack.manifest().layers {
            skimap_pack::reader::check_relative(&l.origin)?;
            // A file converted for the phone (GeoTIFF → PMTiles) is not what the loaders
            // read: left out, reported missing by them.
            if ext(&l.file) != ext(&l.origin) {
                continue;
            }
            let at = base.join(&l.origin);
            link(pack.path(&l.file)?, at.clone())?;
            if let (Some(meta), Some(dir)) = (&l.meta, at.parent()) {
                let src = pack.path(meta)?;
                if let Some(name) = src.file_name() {
                    link(src.clone(), dir.join(name))?;
                }
            }
        }
        let voice = pack.root().join("voice");
        if voice.join("manifest.json").is_file() {
            link(voice, base.join("voice"))?;
        }
        Ok(Self {
            layout: DataLayout::new(root),
            links,
        })
    }
}

#[cfg(unix)]
fn symlink(target: &Path, at: &Path) -> std::io::Result<()> {
    std::os::unix::fs::symlink(target, at)
}

#[cfg(not(unix))]
fn symlink(target: &Path, at: &Path) -> std::io::Result<()> {
    if target.is_dir() {
        Err(std::io::Error::other("directory links need unix"))
    } else {
        std::fs::copy(target, at).map(|_| ())
    }
}

/// A start proposed for the site (the start points of the site's ready-made requests).
#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct DayStart {
    /// Name.
    pub name: String,
    /// Longitude.
    pub lon: f64,
    /// Latitude.
    pub lat: f64,
}

/// What the skier asks in the morning.
#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct DayRequest {
    /// Label (« 3 h, classique »).
    pub label: String,
    /// Start (and finish).
    pub start: DayStart,
    /// Day, `YYYY-MM-DD`.
    pub date: String,
    /// Start time `HH:MM`; `None`: the best time of the day.
    pub start_time: Option<String>,
    /// Earliest start tried for the best time, `HH:MM`.
    pub earliest: String,
    /// Time available, minutes (model time).
    pub budget_min: f64,
    /// Skating (else classic).
    pub skating: bool,
    /// Back before, `HH:MM`.
    pub return_before: Option<String>,
    /// Hardest difficulty wanted: `novice` (green), `easy` (blue), `intermediate` (red),
    /// `advanced` (black); `None`: any.
    pub max_difficulty: Option<String>,
}

/// A group (C11) as the app edits it.
#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct GroupSpec {
    /// Name (« seul », « en famille »).
    pub name: String,
    /// Adults.
    pub adults: u8,
    /// Ages of the children.
    pub children: Vec<u8>,
    /// Skating (else classic).
    pub skating: bool,
    /// `beginner`, `intermediate` or `expert`.
    pub level: String,
}

/// A saved group and what it learned.
#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct GroupView {
    /// The group.
    pub spec: GroupSpec,
    /// Outings learned.
    pub outings: u32,
    /// « a priori, aucune sortie apprise », « 3 sorties apprises ».
    pub learned: String,
    /// « 2 adultes, enfants de 6 et 9 ans ».
    pub label: String,
    /// The model, JSON (`ProfileModel` of `skimap_core::calibration`), stored by the app.
    pub model_json: String,
}

/// A loop proposed in the morning.
#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct LoopView {
    /// Rank, from 1.
    pub rank: u32,
    /// Label (three main trails).
    pub label: String,
    /// Length, km.
    pub length_km: f64,
    /// Climb, m.
    pub gain_m: f64,
    /// Model time, minutes.
    pub model_min: f64,
    /// « 1 h 40 à 2 h 50 » (range) or « environ 2 h 10 (1 h 55 à 2 h 30) » for the group;
    /// the model time without a group.
    pub time_text: String,
    /// « retour entre 12 h 10 et 13 h 20 » / « retour vers 12 h 40 ».
    pub back_text: String,
    /// Whether a range is shown (little history, or a wide spread).
    pub range: bool,
    /// « Retour prévu après la limite de 17 h 05 » when the expected return (the group's
    /// median, else the model with its margin) is after the limit of the day (return asked,
    /// light minus 30 min): proposed all the same, the skier decides.
    pub warning: Option<String>,
    /// Km on open, closed and unknown trails.
    pub status_km: Vec<f64>,
    /// Reasons of the choice, French.
    pub reasons: Vec<String>,
    /// Equipment: « item — reason ».
    pub equipment: Vec<String>,
    /// Geometry, `[lon, lat, lon, lat, …]`.
    pub line: Vec<f64>,
}

/// The morning screen.
#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct MorningView {
    /// Day, `YYYY-MM-DD`.
    pub date: String,
    /// Start time used for the loops, « 9 h 30 ».
    pub start_time: String,
    /// Briefing lines (A1).
    pub briefing: Vec<String>,
    /// Best time to leave (A9).
    pub best_time: Option<String>,
    /// Wax (A2).
    pub wax: Option<String>,
    /// Loops, best first.
    pub loops: Vec<LoopView>,
    /// Missing layers and limits.
    pub notes: Vec<String>,
    /// The group the times are for, when one was chosen.
    pub group: Option<String>,
    /// Never « sans danger ».
    pub never_safe: String,
    /// Time to compute it, milliseconds.
    pub compute_ms: u32,
}

/// A key point of the validated plan.
#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct KeyPointView {
    /// « départ », « carrefour », « refuge », « zone sans réseau », « zone d'avalanche
    /// connue », « arrivée ».
    pub kind: String,
    /// Name.
    pub name: Option<String>,
    /// Km from the start.
    pub km: f64,
    /// Planned passage, « 10 h 25 ».
    pub at: String,
}

/// The validated day (K9, K10).
#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct DayPlanView {
    /// Directory of `plan.json` and `plan.trk`.
    pub plan_dir: String,
    /// Site.
    pub site_id: String,
    /// Label of the loop.
    pub label: String,
    /// Start name.
    pub start_name: String,
    /// Departure, « 9 h 30 », and epoch milliseconds.
    pub departure: String,
    /// Departure, epoch milliseconds.
    pub departure_ms: i64,
    /// Length, km.
    pub length_km: f64,
    /// Expected return, « 12 h 40 » (the group's median, else the model with its margin).
    pub expected_back: String,
    /// Expected return, epoch milliseconds.
    pub expected_back_ms: i64,
    /// Limit to be back (return asked, or light minus 30 min), « 16 h 30 ».
    pub limit: Option<String>,
    /// Limit, epoch milliseconds.
    pub limit_ms: Option<i64>,
    /// Latest turnaround, « 14 h 05 ».
    pub turnaround: Option<String>,
    /// Group time, French.
    pub pace_text: Option<String>,
    /// Key points.
    pub key_points: Vec<KeyPointView>,
    /// Plans B, French.
    pub plans_b: Vec<String>,
    /// Equipment: « item — reason ».
    pub equipment: Vec<String>,
    /// Text of the whole plan (`DayPlan::text_fr`).
    pub text: String,
    /// SMS to the contact (K10), French: route, return, what to do without news.
    pub sms: String,
    /// Geometry, `[lon, lat, …]`.
    pub line: Vec<f64>,
}

fn parse_time(s: &str) -> Result<NaiveTime, TrackingError> {
    NaiveTime::parse_from_str(s, "%H:%M").map_err(|e| TrackingError::Invalid {
        detail: format!("heure {s:?} : {e}"),
    })
}

fn difficulty(s: &str) -> Result<Difficulty, TrackingError> {
    Ok(match s {
        "novice" => Difficulty::Novice,
        "easy" => Difficulty::Easy,
        "intermediate" => Difficulty::Intermediate,
        "advanced" => Difficulty::Advanced,
        other => {
            return Err(TrackingError::Invalid {
                detail: format!("difficulté {other:?}"),
            });
        }
    })
}

fn style(skating: bool) -> Style {
    if skating {
        Style::Skating
    } else {
        Style::Classic
    }
}

impl DayRequest {
    fn to_plan(&self) -> Result<PlanRequest, TrackingError> {
        Ok(PlanRequest {
            label: self.label.clone(),
            start: LonLat::new(self.start.lon, self.start.lat),
            start_name: self.start.name.clone(),
            date: NaiveDate::parse_from_str(&self.date, "%Y-%m-%d").map_err(|e| {
                TrackingError::Invalid {
                    detail: format!("date {:?} : {e}", self.date),
                }
            })?,
            start_time: self.start_time.as_deref().map(parse_time).transpose()?,
            earliest: parse_time(&self.earliest)?,
            budget_min: self.budget_min,
            style: style(self.skating),
            return_before: self.return_before.as_deref().map(parse_time).transpose()?,
            max_difficulty: self.max_difficulty.as_deref().map(difficulty).transpose()?,
            max_results: 3,
        })
    }
}

impl GroupSpec {
    fn to_profile(&self) -> Result<GroupProfile, TrackingError> {
        let level = match self.level.as_str() {
            "beginner" => Level::Beginner,
            "intermediate" => Level::Intermediate,
            "expert" => Level::Expert,
            other => {
                return Err(TrackingError::Invalid {
                    detail: format!("niveau {other:?}"),
                });
            }
        };
        Ok(GroupProfile {
            name: self.name.clone(),
            adults: self.adults.max(1),
            children: self.children.clone(),
            style: style(self.skating),
            level,
        })
    }

    fn of(p: &GroupProfile) -> Self {
        Self {
            name: p.name.clone(),
            adults: p.adults,
            children: p.children.clone(),
            skating: p.style == Style::Skating,
            level: match p.level {
                Level::Beginner => "beginner",
                Level::Intermediate => "intermediate",
                Level::Expert => "expert",
            }
            .into(),
        }
    }
}

fn group_view(m: &ProfileModel) -> Result<GroupView, TrackingError> {
    let learned = match m.outings {
        0 => "a priori, aucune sortie apprise".to_owned(),
        1 => "une sortie apprise".to_owned(),
        n => format!("{n} sorties apprises"),
    };
    let p = &m.profile;
    let adults = match p.adults {
        1 => "1 adulte".to_owned(),
        n => format!("{n} adultes"),
    };
    let label = if p.children.is_empty() {
        adults
    } else {
        let ages: Vec<String> = p.children.iter().map(u8::to_string).collect();
        let ages = match ages.split_last() {
            Some((last, [])) => format!("{last} ans"),
            Some((last, rest)) => format!("{} et {last} ans", rest.join(", ")),
            None => String::new(),
        };
        format!("{adults}, enfants de {ages}")
    };
    Ok(GroupView {
        spec: GroupSpec::of(p),
        outings: m.outings,
        learned,
        label,
        model_json: serde_json::to_string(m).map_err(|e| TrackingError::Invalid {
            detail: e.to_string(),
        })?,
    })
}

fn model_of(json: &str) -> Result<ProfileModel, TrackingError> {
    serde_json::from_str(json).map_err(|e| TrackingError::Invalid {
        detail: format!("groupe : {e}"),
    })
}

/// A new group: its prior (C11).
#[uniffi::export]
pub fn group_new(spec: GroupSpec) -> Result<GroupView, TrackingError> {
    group_view(&ProfileModel::new(spec.to_profile()?))
}

/// A saved group read back.
#[uniffi::export]
pub fn group_read(model_json: String) -> Result<GroupView, TrackingError> {
    group_view(&model_of(&model_json)?)
}

/// The group with one more outing learned (`observation_json`: what the tracking session
/// gave at the end, [`crate::tracking::TrackingSession::observation`]).
#[uniffi::export]
pub fn group_learn(
    model_json: String,
    observation_json: String,
) -> Result<GroupView, TrackingError> {
    let mut m = model_of(&model_json)?;
    let o: OutingObs =
        serde_json::from_str(&observation_json).map_err(|e| TrackingError::Invalid {
            detail: format!("sortie : {e}"),
        })?;
    m.learn(&o);
    group_view(&m)
}

/// Starts of a site: the start points of its ready-made requests (`skimap_plan::presets`).
#[uniffi::export]
pub fn day_starts(site_id: String) -> Vec<DayStart> {
    let Ok(site) = skimap_common::site::site(&site_id) else {
        return Vec::new();
    };
    let date = skimap_plan::presets::past_day(site);
    let mut out: Vec<DayStart> = Vec::new();
    for r in skimap_plan::presets::requests(site, date) {
        if !out.iter().any(|s| s.name == r.start_name) {
            out.push(DayStart {
                name: r.start_name,
                lon: r.start.lon,
                lat: r.start.lat,
            });
        }
    }
    out
}

/// Sites the phone can plan a day on (the trail graph and the assistant's layers exist
/// for them).
#[uniffi::export]
pub fn day_sites() -> Vec<String> {
    skimap_common::site::SITES
        .iter()
        .map(|s| s.id.to_owned())
        .collect()
}

struct Loaded {
    data: SiteData,
    ctx: Context,
    morning: Option<Morning>,
}

/// The morning of a pack: its layers loaded once, the morning computed per request, a
/// loop validated into a plan.
#[derive(uniffi::Object)]
pub struct DayPlanner {
    pack_dir: PathBuf,
    site: &'static Site,
    loaded: Mutex<Loaded>,
    load_ms: u32,
}

impl std::fmt::Debug for DayPlanner {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DayPlanner")
            .field("pack_dir", &self.pack_dir)
            .field("site", &self.site.id)
            .finish_non_exhaustive()
    }
}

fn ms_since(t: Instant) -> u32 {
    u32::try_from(t.elapsed().as_millis()).unwrap_or(u32::MAX)
}

/// Opens the morning of the pack in `pack_dir`; `work_dir` (private storage) receives the
/// pipeline view of the pack ([`PackView`]).
#[uniffi::export]
pub fn open_day_planner(
    pack_dir: String,
    work_dir: String,
) -> Result<Arc<DayPlanner>, TrackingError> {
    let t0 = Instant::now();
    let pack = Pack::open(PathBuf::from(&pack_dir))?;
    let site = skimap_common::site::site(&pack.manifest().site.id).map_err(|_| {
        TrackingError::Unsupported {
            site: pack.manifest().site.id.clone(),
        }
    })?;
    let view = PackView::build(&pack, Path::new(&work_dir))?;
    let data = SiteData::load(&view.layout, site, &Paths::default()).map_err(|e| {
        TrackingError::Invalid {
            detail: e.to_string(),
        }
    })?;
    if data.graph.is_none() {
        return Err(TrackingError::Invalid {
            detail: "pas de graphe des pistes dans le pack".into(),
        });
    }
    let ctx = Context::load(&view.layout, site)?;
    Ok(Arc::new(DayPlanner {
        pack_dir: PathBuf::from(pack_dir),
        site,
        loaded: Mutex::new(Loaded {
            data,
            ctx,
            morning: None,
        }),
        load_ms: ms_since(t0),
    }))
}

fn hhmm(t: NaiveDateTime) -> String {
    format!("{} h {:02}", t.hour(), t.minute())
}

fn flat(line: &[LonLat]) -> Vec<f64> {
    line.iter().flat_map(|p| [p.lon, p.lat]).collect()
}

/// « 15 h 20 », « 1 h 10 le lendemain ».
fn clock_day(t: NaiveDateTime, day: NaiveDate) -> String {
    if t.date() > day {
        format!("{} le lendemain", clock_fr(t))
    } else {
        clock_fr(t)
    }
}

/// The group's time on a route of the morning (the text of `pace::plan_pace` without a
/// plan): (duration text, return text, range, median arrival).
fn group_time(
    g: &PisteGraph,
    legs: &[PlanLeg],
    eta: &skimap_core::eta::EtaModel,
    departure: NaiveDateTime,
    model: &ProfileModel,
) -> Option<(String, String, bool, NaiveDateTime)> {
    let c = Course::new(g, legs, eta).ok()?;
    let route = PaceRoute::of(&c, terrain_ratio(eta));
    let p = model.params.predict(route.total());
    let range = model.outings < pace::FIGURE_AFTER_OUTINGS || p.spread() > pace::FIGURE_SPREAD;
    let at = |s: f64| departure + chrono::Duration::milliseconds((s * 1000.0) as i64);
    let day = departure.date();
    Some(if range {
        (
            format!("{} à {}", duration_fr(p.lo_s), duration_fr(p.hi_s)),
            format!(
                "retour entre {} et {}",
                clock_day(at(p.lo_s), day),
                clock_day(at(p.hi_s), day)
            ),
            true,
            at(p.median_s),
        )
    } else {
        (
            format!(
                "environ {} ({} à {})",
                duration_fr(p.median_s),
                duration_fr(p.lo_s),
                duration_fr(p.hi_s)
            ),
            format!("retour vers {}", clock_day(at(p.median_s), day)),
            false,
            at(p.median_s),
        )
    })
}

/// The limit of a day (J1): the return asked, or light minus 30 min (C7), the earlier.
fn day_limit(req: &PlanRequest) -> Option<NaiveDateTime> {
    let tp = skimap_core::turnaround::TurnaroundParams::default();
    let dl = skimap_core::turnaround::deadline(req.date, req.start, &tp)
        .map(|d| skimap_core::ephemeris::local_time(d.deadline));
    let asked = req.return_before.map(|t| req.date.and_time(t));
    match (dl, asked) {
        (Some(a), Some(b)) => Some(a.min(b)),
        (a, b) => a.or(b),
    }
}

#[uniffi::export]
impl DayPlanner {
    /// Site of the pack.
    pub fn site_id(&self) -> String {
        self.site.id.to_owned()
    }

    /// Time to load the layers, milliseconds.
    pub fn load_ms(&self) -> u32 {
        self.load_ms
    }

    /// Layers the loaders could not read (snow and exposure GeoTIFFs are not in packs).
    pub fn missing(&self) -> Vec<String> {
        let l = self
            .loaded
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let mut v = l.data.missing.clone();
        v.extend(l.ctx.missing.iter().cloned());
        v
    }

    /// The morning of a request, the loops' times for `group` (a saved model, JSON) when
    /// given.
    pub fn morning(
        &self,
        request: DayRequest,
        group_json: Option<String>,
    ) -> Result<MorningView, TrackingError> {
        let t0 = Instant::now();
        let req = request.to_plan()?;
        let model = group_json.as_deref().map(model_of).transpose()?;
        let mut l = self
            .loaded
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let m = plan::morning(&l.data, &req);
        let g = l
            .data
            .graph
            .as_ref()
            .ok_or_else(|| TrackingError::Invalid {
                detail: "graphe".into(),
            })?;
        let limit = day_limit(&req);
        let loops = m
            .suggestions
            .suggestions
            .iter()
            .enumerate()
            .map(|(i, s)| {
                let legs: Vec<PlanLeg> = s.legs.iter().map(|x| PlanLeg::from(*x)).collect();
                let (time_text, back_text, range, back) = model
                    .as_ref()
                    .and_then(|md| group_time(g, &legs, &s.eta, m.start_time, md))
                    .unwrap_or_else(|| {
                        (
                            format!("environ {} (modèle)", duration_fr(s.eta_min * 60.0)),
                            format!("retour vers {} avec la marge", clock_fr(s.arrival)),
                            false,
                            s.arrival,
                        )
                    });
                let warning = limit.filter(|l| back > *l).map(|l| {
                    format!(
                        "Retour prévu après la limite de {} : boucle longue pour {}",
                        hhmm(l),
                        if model.is_some() {
                            "ce groupe"
                        } else {
                            "ce jour"
                        }
                    )
                });
                LoopView {
                    rank: u32::try_from(i + 1).unwrap_or(u32::MAX),
                    label: s.label.clone(),
                    length_km: s.length_km,
                    gain_m: s.gain_m.round(),
                    model_min: s.eta_min.round(),
                    time_text,
                    back_text,
                    range,
                    warning,
                    status_km: vec![s.status_km.0, s.status_km.1, s.status_km.2],
                    reasons: s.reasons.iter().map(|r| r.text.clone()).collect(),
                    equipment: m
                        .equipment
                        .get(i)
                        .map(|e| {
                            e.items
                                .iter()
                                .map(|it| format!("{} — {}", it.label, it.reason))
                                .collect()
                        })
                        .unwrap_or_default(),
                    line: flat(&s.line),
                }
            })
            .collect();
        let view = MorningView {
            date: req.date.format("%Y-%m-%d").to_string(),
            start_time: hhmm(m.start_time),
            briefing: m.briefing.text.clone(),
            best_time: m.best_time.as_ref().map(|b| b.text.clone()),
            wax: m.wax.as_ref().map(|w| w.text.clone()),
            loops,
            notes: m.notes.clone(),
            group: model.map(|md| md.profile.label_fr()),
            never_safe: plan::NEVER_SAFE.into(),
            compute_ms: ms_since(t0),
        };
        l.morning = Some(m);
        Ok(view)
    }

    /// « Valider la journée »: builds the plan of loop `choice` (from 0) of the last
    /// morning, with the group's time when given, and writes `plan.json` and `plan.trk`
    /// into `plan_dir`.
    pub fn validate(
        &self,
        choice: u32,
        group_json: Option<String>,
        plan_dir: String,
    ) -> Result<DayPlanView, TrackingError> {
        let model = group_json.as_deref().map(model_of).transpose()?;
        let mut guard = self
            .loaded
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let l = &mut *guard;
        let m = l.morning.as_ref().ok_or(TrackingError::NoMorning)?;
        let mut p = plan::build(&mut l.data, m, choice as usize, &l.ctx)?;
        let g = l
            .data
            .graph
            .as_ref()
            .ok_or_else(|| TrackingError::Invalid {
                detail: "graphe".into(),
            })?;
        if let Some(md) = &model {
            pace::with_pace(&mut p, g, md)?;
        }
        let track = plan::track(&p, g)?;
        let dir = PathBuf::from(&plan_dir);
        std::fs::create_dir_all(&dir).map_err(|e| io(&dir, e))?;
        let write = |name: &str, bytes: &[u8]| -> Result<(), TrackingError> {
            let path = dir.join(name);
            let tmp = dir.join(format!("{name}.tmp"));
            std::fs::write(&tmp, bytes).map_err(|e| io(&tmp, e))?;
            std::fs::rename(&tmp, &path).map_err(|e| io(&path, e))
        };
        write(PLAN_FILE, p.to_json()?.as_bytes())?;
        write(TRACK_FILE, &track.to_bytes())?;
        write(PACK_FILE, self.pack_dir.display().to_string().as_bytes())?;
        Ok(plan_view(&p, &plan_dir, g))
    }
}

/// The view of a plan already written (the app reopened during the outing).
#[uniffi::export]
pub fn read_day_plan(plan_dir: String) -> Result<DayPlanView, TrackingError> {
    let dir = PathBuf::from(&plan_dir);
    let json = std::fs::read_to_string(dir.join(PLAN_FILE)).map_err(|e| io(&dir, e))?;
    let p = DayPlan::from_json(&json)?;
    let pack = std::fs::read_to_string(dir.join(PACK_FILE)).map_err(|e| io(&dir, e))?;
    let g = crate::tracking::load_graph(Path::new(pack.trim()))?;
    Ok(plan_view(&p, &plan_dir, &g))
}

fn kind_fr(k: KeyKind) -> &'static str {
    match k {
        KeyKind::Start => "départ",
        KeyKind::Turn => "carrefour",
        KeyKind::Junction => "carrefour évident",
        KeyKind::Refuge => "refuge",
        KeyKind::NoNetworkStart => "début de zone sans réseau",
        KeyKind::NoNetworkEnd => "fin de zone sans réseau",
        KeyKind::AvalancheStart => "début de zone d'avalanche connue",
        KeyKind::AvalancheEnd => "fin de zone d'avalanche connue",
        KeyKind::Finish => "arrivée",
    }
}

/// The SMS of the plan (K10): the contact is the timer, nothing is sent by the app.
pub fn sms_text(p: &DayPlan, back: NaiveDateTime) -> String {
    let limit = p.turnaround.limit.map(hhmm);
    let alert = limit
        .clone()
        .unwrap_or_else(|| hhmm(back + chrono::Duration::hours(1)));
    format!(
        "Sortie ski de fond {} le {} : départ {} à {}, boucle {} ({} km, pistes {}). Retour prévu vers {}{}. Sans nouvelles de moi à {}, appelle le 112 et donne ce plan.",
        p.site,
        p.date.format("%d/%m"),
        p.request.start_name,
        hhmm(p.departure),
        p.route.label,
        format!("{:.1}", p.route.length_km).replace('.', ","),
        p.route.names.join(", "),
        hhmm(back),
        limit
            .map(|l| format!(", au plus tard {l}"))
            .unwrap_or_default(),
        alert
    )
}

fn plan_view(p: &DayPlan, plan_dir: &str, g: &PisteGraph) -> DayPlanView {
    let back = p
        .pace
        .as_ref()
        .map_or(p.turnaround.arrival_with_margin, |x| x.arrival);
    let line = Course::new(g, &p.route.legs, &p.eta)
        .map(|c| flat(c.line().points()))
        .unwrap_or_default();
    DayPlanView {
        plan_dir: plan_dir.to_owned(),
        site_id: p.site.clone(),
        label: p.route.label.clone(),
        start_name: p.request.start_name.clone(),
        departure: hhmm(p.departure),
        departure_ms: epoch_ms(p.departure),
        length_km: p.route.length_km,
        expected_back: hhmm(back),
        expected_back_ms: epoch_ms(back),
        limit: p.turnaround.limit.map(hhmm),
        limit_ms: p.turnaround.limit.map(epoch_ms),
        turnaround: p.turnaround.latest_turnaround.map(hhmm),
        pace_text: p.pace.as_ref().map(|x| x.text.clone()),
        key_points: p
            .key_points
            .iter()
            .filter(|k| k.kind != KeyKind::Junction)
            .map(|k| KeyPointView {
                kind: kind_fr(k.kind).into(),
                name: k.name.clone(),
                km: (k.s / 100.0).round() / 10.0,
                at: hhmm(k.at),
            })
            .collect(),
        plans_b: p
            .plans_b
            .iter()
            .filter(|b| b.fallback.exists())
            .map(|b| b.fallback.text.clone())
            .collect(),
        equipment: p
            .equipment
            .iter()
            .map(|i| format!("{} — {}", i.label, i.reason))
            .collect(),
        text: p.text_fr(),
        sms: sms_text(p, back),
        line,
    }
}
