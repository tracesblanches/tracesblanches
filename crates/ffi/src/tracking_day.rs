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

use chrono::{NaiveDate, NaiveDateTime, NaiveTime};
use skimap_assist::inputs::{Paths, SiteData};
use skimap_assist::suggest::GroupTime;
use skimap_common::geom::LonLat;
use skimap_common::site::Site;
use skimap_core::calibration::{GroupProfile, Level, OutingObs, ProfileModel};
use skimap_core::graph::{Difficulty, PisteGraph};
use skimap_core::routing::Style;
use skimap_pack::Pack;
use skimap_plan::course::Course;
use skimap_plan::outing::Outing;
use skimap_plan::pace;
use skimap_plan::plan::{self, Context, DayPlan, KeyKind, Morning, PlanRequest};

use crate::card::Lang;
use crate::text::{self, Texts};
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

impl From<skimap_common::Error> for TrackingError {
    fn from(e: skimap_common::Error) -> Self {
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

/// The pipeline layout of a pack's files, as symbolic links (in `skimap_pack`, where the
/// pack build also ranks the starts on the pack itself, #166).
pub use skimap_pack::view::PackView;

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

/// Shortest walk from the car park to the network said with the loops, metres (#168):
/// below, the size of a car park.
pub const ACCESS_SAID_M: f64 = 30.0;

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

/// The outing the skier asks (#170): what the morning's choices keep from one day to the
/// next, and what the evening preparation asks.
#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct DayOuting {
    /// Time available, minutes (model time).
    pub budget_min: f64,
    /// Skating (else classic).
    pub skating: bool,
    /// Back before, `HH:MM`.
    pub return_before: Option<String>,
    /// Green and blue pistes only.
    pub easy_only: bool,
}

impl DayOuting {
    /// The planner's outing.
    pub fn to_outing(&self) -> Result<Outing, TrackingError> {
        Ok(Outing {
            budget_min: self.budget_min,
            style: style(self.skating),
            return_before: self.return_before.as_deref().map(parse_time).transpose()?,
            easy_only: self.easy_only,
        })
    }

    fn of(o: &Outing) -> Self {
        Self {
            budget_min: o.budget_min,
            skating: o.style == Style::Skating,
            return_before: o.return_before.map(|t| t.format("%H:%M").to_string()),
            easy_only: o.easy_only,
        }
    }
}

/// The outing of the app until the skier chooses another (`Outing::DEFAULT` of
/// `skimap_plan::outing`: 2 h, classic), the same for the morning, the evening
/// preparation and the ranking of the starts of a pack (#170).
#[uniffi::export]
pub fn day_default_outing() -> DayOuting {
    DayOuting::of(&Outing::DEFAULT)
}

/// Label of an outing in `lang` (« 2 h, classique »).
#[uniffi::export]
pub fn day_outing_label(outing: DayOuting, lang: Lang) -> Result<String, TrackingError> {
    Ok(outing.to_outing()?.label(lang))
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
    /// « Si tout va bien » (C11): with children, a loop that fits the time asked at the
    /// group's median only (not on the slow side the others are planned on); listed after
    /// the others, never first.
    pub if_all_goes_well: bool,
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
    /// The briefing lines shown before « more »: the snow quality at each forecast point,
    /// then the sunset.
    pub briefing_short: Vec<String>,
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
                detail: format!("difficulty {other:?}"),
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

fn group_view(m: &ProfileModel, lang: Lang) -> Result<GroupView, TrackingError> {
    let t = text::of(lang);
    let learned = t.group_learned(m.outings);
    let p = &m.profile;
    let label = t.group_who(p.adults, &p.children);
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
pub fn group_new(spec: GroupSpec, lang: Lang) -> Result<GroupView, TrackingError> {
    group_view(&ProfileModel::new(spec.to_profile()?), lang)
}

/// A saved group read back.
#[uniffi::export]
pub fn group_read(model_json: String, lang: Lang) -> Result<GroupView, TrackingError> {
    group_view(&model_of(&model_json)?, lang)
}

/// The group with one more outing learned (`observation_json`: what the tracking session
/// gave at the end, [`crate::tracking::TrackingSession::observation`]).
#[uniffi::export]
pub fn group_learn(
    model_json: String,
    observation_json: String,
    lang: Lang,
) -> Result<GroupView, TrackingError> {
    let mut m = model_of(&model_json)?;
    let o: OutingObs =
        serde_json::from_str(&observation_json).map_err(|e| TrackingError::Invalid {
            detail: format!("outing: {e}"),
        })?;
    m.learn(&o);
    group_view(&m, lang)
}

/// The group the app offers first (C11): « Famille avec enfants débutants ».
#[uniffi::export]
pub fn group_default(lang: Lang) -> Result<GroupView, TrackingError> {
    let mut p = GroupProfile::app_default();
    text::of(lang).default_group_name().clone_into(&mut p.name);
    group_view(&ProfileModel::new(p), lang)
}

/// The group of one adult the app offers (intermediate, classic), named in `lang`.
#[uniffi::export]
pub fn group_solo(lang: Lang) -> Result<GroupView, TrackingError> {
    group_new(
        GroupSpec {
            name: text::of(lang).solo_group_name().to_owned(),
            adults: 1,
            children: Vec::new(),
            skating: false,
            level: "intermediate".into(),
        },
        lang,
    )
}

/// The site of a pack, read from its manifest (P163, #163): the record the pack carries
/// (kind, box, massifs, the starts found by the pipeline), so that any installed pack of
/// France plans offline; a pack written before the record knew only the evaluation sites.
pub fn pack_site(pack: &Pack) -> Result<&'static Site, TrackingError> {
    let info = &pack.manifest().site;
    match &info.record {
        Some(r) if r.id == info.id => {
            skimap_common::site::interned(r).map_err(|e| TrackingError::Invalid {
                detail: e.to_string(),
            })
        }
        Some(r) => Err(TrackingError::Invalid {
            detail: format!("manifest: site `{}`, record of `{}`", info.id, r.id),
        }),
        None => skimap_common::site::site(&info.id).map_err(|_| TrackingError::Unsupported {
            site: info.id.clone(),
        }),
    }
}

/// Whether the morning can open on this pack: its site is known offline ([`pack_site`])
/// and it carries the trail graph.
fn plannable(pack: &Pack) -> bool {
    pack.manifest().layer("piste-graph").is_some() && pack_site(pack).is_ok()
}

/// Starts of a site, main first (`skimap_common::site::starts`: the evaluation table,
/// else the starts the pipeline found for every domain, X9, carried by the pack), names
/// in `lang`.
pub fn site_starts(site: &Site, lang: Lang) -> Vec<DayStart> {
    let tx = skimap_plan::text::of(lang);
    let mut out: Vec<DayStart> = Vec::new();
    for s in skimap_common::site::starts(site) {
        let name = tx
            .start_phrase(&s.phrase, s.phrase_en.as_deref())
            .to_owned();
        if !out.iter().any(|o| o.name == name) {
            out.push(DayStart {
                name,
                lon: s.lon,
                lat: s.lat,
            });
        }
    }
    out
}

/// Starts of the site of the pack in `pack_dir` ([`site_starts`]), read offline from the
/// pack; empty when the pack cannot be read or does not carry its site.
#[uniffi::export]
pub fn day_pack_starts(pack_dir: String, lang: Lang) -> Vec<DayStart> {
    Pack::open(PathBuf::from(pack_dir))
        .ok()
        .and_then(|p| pack_site(&p).ok())
        .map_or_else(Vec::new, |s| site_starts(s, lang))
}

/// Starts of a site by identifier ([`site_starts`]): a site whose pack was opened in this
/// process ([`pack_site`]), else an evaluation site; empty otherwise. With the pack at
/// hand, [`day_pack_starts`] needs no earlier opening.
#[uniffi::export]
pub fn day_starts(site_id: String, lang: Lang) -> Vec<DayStart> {
    skimap_common::site::known(&site_id).map_or_else(Vec::new, |s| site_starts(s, lang))
}

/// Installed packs under `root` the phone can plan a day on (any site of France whose
/// pack carries its site and the trail graph), as [`crate::list_packs`] lists them.
#[uniffi::export]
pub fn day_packs(root: String) -> Vec<crate::info::PackEntry> {
    crate::info::list_packs(Path::new(&root))
        .into_iter()
        .filter(|e| {
            e.error.is_none() && Pack::open(PathBuf::from(&e.dir)).is_ok_and(|p| plannable(&p))
        })
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
    /// Neighbouring packs joined to the pack of the start (H2 #133).
    neighbours: Vec<Pack>,
    site: &'static Site,
    loaded: Mutex<Loaded>,
    load_ms: u32,
    lang: Lang,
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
/// pipeline view of the pack ([`PackView`]); texts in `lang` (the phone's).
#[uniffi::export]
pub fn open_day_planner(
    pack_dir: String,
    work_dir: String,
    lang: Lang,
) -> Result<Arc<DayPlanner>, TrackingError> {
    open_day_planner_with(pack_dir, Vec::new(), work_dir, lang)
}

/// Opens the morning of the pack in `pack_dir` joined with the installed packs among
/// `installed` whose box meets its box (H2 #133, [`crate::neighbours`]): a loop may cross
/// into the neighbouring areas. `installed` may hold every installed pack (the start's
/// included): the neighbours are chosen here.
#[uniffi::export]
pub fn open_day_planner_with(
    pack_dir: String,
    installed: Vec<String>,
    work_dir: String,
    lang: Lang,
) -> Result<Arc<DayPlanner>, TrackingError> {
    let t0 = Instant::now();
    let pack = Pack::open(PathBuf::from(&pack_dir))?;
    let site = pack_site(&pack)?;
    let others = crate::neighbours::neighbours(&pack, &installed);
    let view = crate::neighbours::build_view(&pack, &others, Path::new(&work_dir))?;
    let data = SiteData::load(&view.layout, site, &Paths::default()).map_err(|e| {
        TrackingError::Invalid {
            detail: e.to_string(),
        }
    })?;
    if data.graph.is_none() {
        return Err(TrackingError::NoTrailGraph);
    }
    let ctx = Context::load(&view.layout, site)?;
    Ok(Arc::new(DayPlanner {
        neighbours: others,
        pack_dir: PathBuf::from(pack_dir),
        site,
        loaded: Mutex::new(Loaded {
            data,
            ctx,
            morning: None,
        }),
        load_ms: ms_since(t0),
        lang,
    }))
}

fn flat(line: &[LonLat]) -> Vec<f64> {
    line.iter().flat_map(|p| [p.lon, p.lat]).collect()
}

/// The group's time on a loop of the morning (`Suggestion::group`, C11): (duration text,
/// return text, range, planned arrival). With children the loop is planned on the slow
/// side of the range: the median is said too.
fn group_time(
    t: &GroupTime,
    departure: NaiveDateTime,
    tx: &dyn Texts,
) -> (String, String, bool, NaiveDateTime) {
    let m = |min: f64| min * 60.0;
    let at = |min: f64| departure + chrono::Duration::milliseconds((min * 60_000.0) as i64);
    let day = departure.date();
    let spread = if t.median_min > 0.0 {
        (t.hi_min - t.lo_min) / (2.0 * t.median_min)
    } else {
        0.0
    };
    let range = t.outings < pace::FIGURE_AFTER_OUTINGS || spread > pace::FIGURE_SPREAD;
    let duration = if range {
        tx.duration_range(&tx.duration(m(t.lo_min)), &tx.duration(m(t.hi_min)))
    } else {
        tx.duration_about(
            &tx.duration(m(t.median_min)),
            &tx.duration(m(t.lo_min)),
            &tx.duration(m(t.hi_min)),
        )
    };
    let back = if t.plan_min > t.median_min + 1.0 {
        tx.back_slow_side(
            &tx.duration(m(t.median_min)),
            &tx.duration(m(t.plan_min)),
            &tx.clock_day(at(t.plan_min), day),
        )
    } else if range {
        tx.back_between(
            &tx.clock_day(at(t.lo_min), day),
            &tx.clock_day(at(t.hi_min), day),
        )
    } else {
        tx.back_around(&tx.clock_day(at(t.median_min), day))
    };
    (duration, back, range, at(t.plan_min))
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

    /// Sites of the neighbouring packs joined to the pack of the start (H2 #133).
    pub fn neighbour_sites(&self) -> Vec<String> {
        self.neighbours
            .iter()
            .map(|p| p.manifest().site.id.clone())
            .collect()
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
        let mut v = l.data.missing_in(self.lang);
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
        let tx = text::of(self.lang);
        let req = request.to_plan()?;
        // The walk from the car park to the network, when the start says it (#168).
        let access = skimap_common::site::starts(self.site)
            .iter()
            .find(|s| s.lon == request.start.lon && s.lat == request.start.lat)
            .and_then(|s| s.access_m)
            .filter(|m| *m >= ACCESS_SAID_M)
            .map(|m| tx.off_network_access(m));
        let model = group_json.as_deref().map(model_of).transpose()?;
        let mut l = self
            .loaded
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let m = plan::morning_in(&l.data, &req, model.as_ref(), self.lang);
        let limit = day_limit(&req);
        let loops = m
            .suggestions
            .suggestions
            .iter()
            .enumerate()
            .map(|(i, s)| {
                let (time_text, back_text, range, back) = s
                    .group
                    .as_ref()
                    .map(|t| group_time(t, m.start_time, tx))
                    .unwrap_or_else(|| {
                        (
                            tx.model_time(&tx.duration(s.eta_min * 60.0)),
                            tx.back_with_margin(&tx.clock5(s.arrival)),
                            false,
                            s.arrival,
                        )
                    });
                let warning = limit
                    .filter(|l| back > *l)
                    .map(|l| tx.after_limit(l, model.is_some()));
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
                    if_all_goes_well: s.if_all_goes_well,
                    status_km: vec![s.status_km.0, s.status_km.1, s.status_km.2],
                    reasons: access
                        .iter()
                        .cloned()
                        .chain(s.reasons.iter().map(|r| r.text.clone()))
                        .collect(),
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
        let briefing_short = briefing_short(&m, tx);
        let view = MorningView {
            date: req.date.format("%Y-%m-%d").to_string(),
            start_time: tx.assist().clock(m.start_time),
            briefing: m.briefing.text.clone(),
            briefing_short,
            best_time: m.best_time.as_ref().map(|b| b.text.clone()),
            wax: m.wax.as_ref().map(|w| w.text.clone()),
            loops,
            notes: m.notes.clone(),
            group: model.map(|md| tx.assist().group_label(&md.profile)),
            never_safe: skimap_plan::text::of(self.lang).never_safe().into(),
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
        // The morning's group gives its time to the plan (`plan::build`); a group given
        // now that is not the morning's (a morning computed without one, or with the
        // group before it learned its last outing) is applied here.
        if let Some(md) = &model
            && m.group.as_ref() != Some(md)
        {
            pace::with_pace(&mut p, g, md)?;
        }
        let track = plan::track(&p, g)?;
        let dir = PathBuf::from(&plan_dir);
        std::fs::create_dir_all(&dir).map_err(|e| io(&dir, e))?;
        let write = |name: &str, bytes: &[u8]| -> Result<(), TrackingError> {
            Ok(skimap_common::output::write_bytes(&dir.join(name), bytes)?)
        };
        write(PLAN_FILE, p.to_json()?.as_bytes())?;
        write(TRACK_FILE, &track.to_bytes())?;
        write(PACK_FILE, self.pack_dir.display().to_string().as_bytes())?;
        crate::neighbours::write_neighbours(&dir, &self.neighbours)?;
        Ok(plan_view(&p, &plan_dir, g, self.lang))
    }
}

/// The briefing lines shown first: the snow quality at each forecast point, then the
/// sunset (the same lines as in `briefing`).
fn briefing_short(m: &Morning, tx: &dyn Texts) -> Vec<String> {
    let a = tx.assist();
    let b = &m.briefing;
    b.snow_quality
        .iter()
        .map(|q| a.briefing_snow_quality(&q.point_name, &q.summary))
        .chain(b.sunset.map(|s| a.briefing_sunset(s)))
        .collect()
}

/// The view of a plan already written (the app reopened during the outing), texts in
/// `lang`.
#[uniffi::export]
pub fn read_day_plan(plan_dir: String, lang: Lang) -> Result<DayPlanView, TrackingError> {
    let dir = PathBuf::from(&plan_dir);
    let json = std::fs::read_to_string(dir.join(PLAN_FILE)).map_err(|e| io(&dir, e))?;
    let p = DayPlan::from_json(&json)?;
    let pack = std::fs::read_to_string(dir.join(PACK_FILE)).map_err(|e| io(&dir, e))?;
    let g = crate::tracking::load_graph(Path::new(pack.trim()))?;
    Ok(plan_view(&p, &plan_dir, &g, lang))
}

/// The SMS of the plan (K10): the contact is the timer, nothing is sent by the app.
pub fn sms_text(p: &DayPlan, back: NaiveDateTime, lang: Lang) -> String {
    let alert = p
        .turnaround
        .limit
        .unwrap_or(back + chrono::Duration::hours(1));
    text::of(lang).sms(
        &p.site,
        p.date,
        &p.request.start_name,
        p.departure,
        &p.route.label,
        p.route.length_km,
        &p.route.names,
        back,
        p.turnaround.limit,
        alert,
    )
}

fn plan_view(p: &DayPlan, plan_dir: &str, g: &PisteGraph, lang: Lang) -> DayPlanView {
    let tx = text::of(lang);
    let hhmm = |t: NaiveDateTime| tx.assist().clock(t);
    let back = p
        .pace
        .as_ref()
        .map_or(p.turnaround.arrival_with_margin, |x| x.planned);
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
                kind: tx.key_kind(k.kind).into(),
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
        text: p.text_in(lang),
        sms: sms_text(p, back, lang),
        line,
    }
}
