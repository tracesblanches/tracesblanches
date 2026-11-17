//! `build-all --all` / `--domains a,b,c`: the `build-all` of many sites, in parallel with a
//! bound, resumable, with a log per site and a summary.
//!
//! - One downloader for every site: its per-source lock (one request in flight per
//!   source) and its rate limits are global, not per site.
//! - The planet extract and the census are read once; the stations layer of France (H1,
//!   H2) is refreshed once, before the sites (its due parts, every part with `--force`).
//! - A site whose last build succeeded with the same inputs (site record, `skimap`
//!   version, `personal_sources`) less than `max_age` ago is not rebuilt; a failed one is.
//! - Each site writes `data/out/<site>/build-all.log` (step lines) and `build-all.json`
//!   (inputs, steps, sizes); the run writes `data/out/france/build-all/summary.json` and
//!   the catalogue.

use std::collections::{BTreeMap, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use serde::Serialize;
use skimap_common::layout::DataLayout;
use skimap_common::meta::now_rfc3339;
use skimap_common::site::{Registry, Site};

use crate::build::{BuildOutcome, Ctx, France, Shared, StepRecord, StepState};
use crate::france::{self, BuildInputs, BuildRecord};

/// Options of a batch.
#[derive(Debug, Clone)]
pub struct BatchOptions {
    /// Sites built in parallel.
    pub jobs: usize,
    /// Rebuild even the sites that are up to date.
    pub force: bool,
    /// Age beyond which a successful build is redone.
    pub max_age: Duration,
    /// No site is started after this duration (the running ones finish).
    pub stop_after: Option<Duration>,
    /// Check the output fingerprints of the sites that look up to date: a site whose
    /// outputs no longer match its record (altered, removed, half written) is rebuilt
    /// (`build-all --stale`).
    pub verify: bool,
}

/// Whether every output recorded for the steps of `rec` is still on disk unchanged.
pub fn outputs_intact(layout: &DataLayout, site: &Site, rec: &BuildRecord) -> bool {
    let dir = layout.out_dir(site);
    rec.outcome
        .steps
        .iter()
        .all(|s| match (&s.outputs, crate::build::step_dir(&s.name)) {
            (Some(f), Some(d)) => crate::build::fingerprint(&dir.join(d)).as_ref() == Some(f),
            _ => true,
        })
}

/// Removes the temporary files (`*.tmp*`) left in a directory by an interrupted write.
pub fn remove_orphan_tmp(dir: &Path) -> usize {
    let mut n = 0;
    let Ok(rd) = std::fs::read_dir(dir) else {
        return 0;
    };
    for e in rd.flatten() {
        let p = e.path();
        if e.file_type().is_ok_and(|t| t.is_dir()) {
            n += remove_orphan_tmp(&p);
        } else if p
            .file_name()
            .and_then(|f| f.to_str())
            .is_some_and(|f| f.contains(".tmp"))
            && std::fs::remove_file(&p).is_ok()
        {
            n += 1;
        }
    }
    n
}

/// Box area from which a site is built alone among the big ones (9 sites of the registry
/// on 15/11/2026: the GTJ and Vercors traverse sections, Haute-Joux, Les Fourgs…).
pub const HEAVY_SITE_KM2: f64 = 300.0;

/// Area of the box of a site, km².
pub fn site_area_km2(site: &Site) -> f64 {
    let b = site.bbox;
    let lat = ((b.north + b.south) / 2.0).to_radians();
    (b.north - b.south) * 111.2 * (b.east - b.west) * 111.2 * lat.cos()
}

/// Lock file of a site build, in its output directory.
pub const LOCK_FILE: &str = ".build-all.lock";

/// Takes the build lock of a site (no two builds of one site at once, whatever the
/// process); released when the returned file is dropped (or the process dies).
pub fn lock_site(dir: &Path) -> Result<std::fs::File> {
    let p = dir.join(LOCK_FILE);
    let f = skimap_common::output::open_lock(&p)?;
    match f.try_lock() {
        Ok(()) => Ok(f),
        Err(std::fs::TryLockError::WouldBlock) => {
            anyhow::bail!(
                "{} : une autre construction de ce site est en cours",
                dir.display()
            )
        }
        Err(std::fs::TryLockError::Error(e)) => Err(e).with_context(|| format!("{}", p.display())),
    }
}

/// Downloads of the run, per source: files and bytes added to the cache.
#[derive(Debug, Clone, Default, Serialize, PartialEq)]
pub struct CacheDelta {
    /// New cache entries.
    pub files: i64,
    /// Bytes added.
    pub bytes: i64,
}

/// Cache inventory: `.body` files and bytes per source.
pub fn cache_inventory(root: &Path) -> BTreeMap<String, (i64, i64)> {
    let mut out = BTreeMap::new();
    let Ok(rd) = std::fs::read_dir(root) else {
        return out;
    };
    for e in rd.filter_map(|e| e.ok()) {
        if !e.file_type().is_ok_and(|t| t.is_dir()) {
            continue;
        }
        let (mut n, mut b) = (0i64, 0i64);
        if let Ok(files) = std::fs::read_dir(e.path()) {
            for f in files.filter_map(|f| f.ok()) {
                if f.path().extension().is_some_and(|x| x == "body") {
                    n += 1;
                    b += f.metadata().map_or(0, |m| m.len() as i64);
                }
            }
        }
        out.insert(e.file_name().to_string_lossy().into_owned(), (n, b));
    }
    out
}

/// One site of the summary.
#[derive(Debug, Clone, Serialize)]
pub struct SiteResult {
    /// Site.
    pub site: String,
    /// `built`, `failed`, `up_to_date`, `not_started` (stop time reached).
    pub result: String,
    /// Seconds.
    pub seconds: f64,
    /// Pack bytes.
    pub pack_bytes: Option<u64>,
    /// Export bytes.
    pub export_bytes: Option<u64>,
    /// Failed or blocked steps: `step : error`.
    pub failures: Vec<String>,
    /// Steps that did not apply.
    pub not_applicable: Vec<String>,
}

/// Summary of a batch.
#[derive(Debug, Clone, Serialize)]
pub struct BatchSummary {
    /// Start.
    pub started_at: String,
    /// End.
    pub finished_at: String,
    /// Wall-clock seconds.
    pub seconds: f64,
    /// Parallel sites.
    pub jobs: usize,
    /// Sites asked.
    pub sites: usize,
    /// Built without failure.
    pub built: usize,
    /// Built with at least one failed step.
    pub failed: usize,
    /// Already up to date.
    pub up_to_date: usize,
    /// Not started (stop time).
    pub not_started: usize,
    /// Failures grouped by cause (`step : error`, sites).
    pub failure_causes: BTreeMap<String, Vec<String>>,
    /// Seconds per step summed over the built sites, heaviest first.
    pub step_seconds: Vec<(String, f64)>,
    /// « sans objet » per step: sites.
    pub not_applicable: BTreeMap<String, usize>,
    /// Pack bytes of the built sites.
    pub pack_bytes: u64,
    /// Export bytes of the built sites.
    pub export_bytes: u64,
    /// Downloads of the run per source.
    pub downloads: BTreeMap<String, CacheDelta>,
    /// Peak resident memory of the process, MB (all parallel sites together).
    pub peak_rss_mb: Option<f64>,
    /// Refresh of the stations layer of France, once for the batch.
    pub stations: StepRecord,
    /// AROME weather of every site to build, once for the batch (public mode only).
    pub meteo: Option<StepRecord>,
    /// Per site.
    pub results: Vec<SiteResult>,
}

fn inputs(ctx: &Ctx, registry: &Registry, site: &Site) -> Result<BuildInputs> {
    Ok(BuildInputs {
        record: registry
            .record(site.id)
            .with_context(|| format!("{} absent du registre", site.id))?
            .clone(),
        version: env!("CARGO_PKG_VERSION").to_owned(),
        personal_sources: ctx.cfg.personal_sources,
        revision: france::BUILD_REVISION,
    })
}

/// Whether the last build of a site is successful, with the same inputs and recent.
pub fn up_to_date(
    rec: &BuildRecord,
    inputs: &BuildInputs,
    max_age: Duration,
    now: DateTime<Utc>,
) -> bool {
    let recent = DateTime::parse_from_rfc3339(&rec.finished_at)
        .is_ok_and(|t| (now - t.to_utc()).to_std().is_ok_and(|age| age < max_age));
    rec.ok && rec.inputs == *inputs && recent
}

fn first_line(s: &str, n: usize) -> String {
    let l = s.lines().next().unwrap_or("");
    let mut out: String = l.chars().take(n).collect();
    if l.chars().count() > n {
        out.push('…');
    }
    out
}

/// Cause of a failure, for grouping: the step and the error without its numbers (paths,
/// counts and coordinates differ between sites).
pub fn cause(step: &str, error: &str) -> String {
    let e: String = first_line(error, 160)
        .chars()
        .map(|c| if c.is_ascii_digit() { '#' } else { c })
        .collect();
    let mut s = String::new();
    for c in e.chars() {
        if !(c == '#' && s.ends_with('#')) {
            s.push(c);
        }
    }
    format!("{step} : {s}")
}

fn site_result(site: &str, o: &BuildOutcome) -> SiteResult {
    let failures = o
        .failed()
        .iter()
        .map(|s| format!("{} : {}", s.name, first_line(&s.line, 300)))
        .collect();
    let not_applicable = o
        .steps
        .iter()
        .filter(|s| s.state == StepState::NotApplicable)
        .map(|s| s.name.clone())
        .collect();
    SiteResult {
        site: site.to_owned(),
        result: if o.ok() { "built" } else { "failed" }.into(),
        seconds: o.seconds,
        pack_bytes: o.pack_bytes,
        export_bytes: o.export_bytes,
        failures,
        not_applicable,
    }
}

/// What a batch runs besides its bookkeeping: the stations layer of France, the AROME
/// weather of the sites to build and the build of one site ([`Pipeline`]; replaced in
/// tests).
pub trait Steps: Sync {
    /// Refreshes the stations layer of France (and the starts of the registry, X9); the
    /// census comes from `france` when there is none on disk yet.
    fn stations(&self, ctx: &Ctx, france: &France<'_>, force: bool) -> StepRecord;
    /// AROME weather of `sites`.
    fn meteo(&self, ctx: &Ctx, sites: &[&Site]) -> StepRecord;
    /// Builds one site and writes its record.
    fn site(
        &self,
        ctx: &Ctx,
        site: &'static Site,
        inp: BuildInputs,
        shared: Shared<'_>,
        opts: &BatchOptions,
    ) -> Result<BuildOutcome>;
    /// Builds one site on its own (`build-all <site>`: steps on stdout) and writes its
    /// record.
    fn single(
        &self,
        ctx: &Ctx,
        site: &'static Site,
        inp: BuildInputs,
        shared: Shared<'_>,
        force: bool,
        max_age: Duration,
    ) -> Result<BuildOutcome>;
}

/// The steps of the pipeline.
pub struct Pipeline;

impl Steps for Pipeline {
    fn stations(&self, ctx: &Ctx, france: &France<'_>, force: bool) -> StepRecord {
        stations_step(ctx, france, force)
    }

    fn meteo(&self, ctx: &Ctx, sites: &[&Site]) -> StepRecord {
        meteo_step(ctx, sites)
    }

    fn site(
        &self,
        ctx: &Ctx,
        site: &'static Site,
        inp: BuildInputs,
        shared: Shared<'_>,
        opts: &BatchOptions,
    ) -> Result<BuildOutcome> {
        build_one(ctx, site, inp, shared, opts)
    }

    fn single(
        &self,
        ctx: &Ctx,
        site: &'static Site,
        inp: BuildInputs,
        shared: Shared<'_>,
        force: bool,
        max_age: Duration,
    ) -> Result<BuildOutcome> {
        build_site(
            ctx,
            site,
            inp,
            shared,
            force,
            max_age,
            &mut std::io::stdout(),
        )
    }
}

/// Builds `sites`.
pub fn build_many(
    ctx: &Ctx,
    registry: &Registry,
    sites: &[&'static Site],
    opts: &BatchOptions,
) -> Result<BatchSummary> {
    build_many_with(ctx, registry, sites, opts, &Pipeline)
}

/// [`build_many`] with the given steps.
pub fn build_many_with(
    ctx: &Ctx,
    registry: &Registry,
    sites: &[&'static Site],
    opts: &BatchOptions,
    steps: &dyn Steps,
) -> Result<BatchSummary> {
    let t0 = Instant::now();
    let started_at = now_rfc3339();
    let cache_before = cache_inventory(&ctx.layout.cache_root());
    // The stations layer first: after new station cards it rewrites the starts of the
    // registry (X9), part of the inputs of each site. Read before it (#164), the inputs
    // of a pass were the old record while its sites were built and the next pass read
    // the new one: no record to take over, every step of the site redone.
    // Planet extract, census and SRN areas: read by the first step that needs them (a
    // site whose `pistes`, `domaines` and `srn` are taken over needs none).
    let france = France::new(ctx);
    let stations = steps.stations(ctx, &france, opts.force);
    let reread = reread(registry)?;
    let registry = reread.as_ref().unwrap_or(registry);
    let sites = sites
        .iter()
        .map(|s| Ok(registry.get(s.id)?))
        .collect::<Result<Vec<&'static Site>>>()?;
    let sites = &sites[..];
    let now = Utc::now();
    let mut results = Vec::new();
    let mut todo = VecDeque::new();
    for s in sites {
        let inp = inputs(ctx, registry, s)?;
        match france::read_record(&ctx.layout, s.id) {
            Some(r)
                if !opts.force
                    && up_to_date(&r, &inp, opts.max_age, now)
                    && (!opts.verify || outputs_intact(&ctx.layout, s, &r)) =>
            {
                println!("{:<34} à jour (construit le {})", s.id, r.finished_at);
                results.push(SiteResult {
                    site: s.id.to_owned(),
                    result: "up_to_date".into(),
                    seconds: 0.0,
                    pack_bytes: r.outcome.pack_bytes,
                    export_bytes: r.outcome.export_bytes,
                    failures: Vec::new(),
                    not_applicable: Vec::new(),
                });
            }
            _ => todo.push_back((*s, inp)),
        }
    }
    let n = todo.len();
    println!(
        "build-all : {} sites, {} à construire, {} à jour ; {} en parallèle",
        sites.len(),
        n,
        sites.len() - n,
        opts.jobs
    );
    let mut built: Vec<(String, BuildOutcome)> = Vec::new();
    let mut meteo = None;
    if n > 0 {
        let todo_sites: Vec<&Site> = todo.iter().map(|(s, _)| *s).collect();
        meteo = (!ctx.cfg.personal_sources).then(|| steps.meteo(ctx, &todo_sites));
        let shared = Shared {
            france: Some(&france),
            resume: None,
            journal: None,
        };
        let queue = Mutex::new(todo);
        let done = Mutex::new(Vec::new());
        let counter = Mutex::new(0usize);
        // Big sites one at a time: a GTJ section alone peaks at 5.6 GB (G2), several at
        // once with the other sites would exhaust the memory of the machine.
        let heavy = Mutex::new(());
        let deadline = opts.stop_after.map(|d| t0 + d);
        std::thread::scope(|scope| {
            for _ in 0..opts.jobs.max(1) {
                scope.spawn(|| {
                    loop {
                        if deadline.is_some_and(|d| Instant::now() >= d) {
                            break;
                        }
                        let Some((site, inp)) = queue.lock().expect("queue").pop_front() else {
                            break;
                        };
                        let _heavy = (site_area_km2(site) >= HEAVY_SITE_KM2)
                            .then(|| heavy.lock().unwrap_or_else(|p| p.into_inner()));
                        let r = steps.site(ctx, site, inp, shared, opts);
                        let k = {
                            let mut c = counter.lock().expect("counter");
                            *c += 1;
                            *c
                        };
                        match &r {
                            Ok(o) => println!(
                                "[{k}/{n}] {:<34} {} {:>7.1} s, pack {}{}",
                                site.id,
                                if o.ok() { "OK   " } else { "ÉCHEC" },
                                o.seconds,
                                o.pack_bytes.map_or("-".to_owned(), |b| format!(
                                    "{:.1} Mo",
                                    b as f64 / 1e6
                                )),
                                if o.ok() {
                                    String::new()
                                } else {
                                    format!(
                                        " ; {}",
                                        o.failed()
                                            .iter()
                                            .map(|s| format!(
                                                "{} : {}",
                                                s.name,
                                                first_line(&s.line, 120)
                                            ))
                                            .collect::<Vec<_>>()
                                            .join(" | ")
                                    )
                                }
                            ),
                            Err(e) => println!("[{k}/{n}] {:<34} ERREUR {e:#}", site.id),
                        }
                        done.lock().expect("done").push((site.id.to_owned(), r));
                    }
                });
            }
        });
        for (id, r) in done.into_inner().expect("done") {
            match r {
                Ok(o) => built.push((id, o)),
                Err(e) => results.push(SiteResult {
                    site: id,
                    result: "failed".into(),
                    seconds: 0.0,
                    pack_bytes: None,
                    export_bytes: None,
                    failures: vec![format!("build-all : {e:#}")],
                    not_applicable: Vec::new(),
                }),
            }
        }
    }
    let started: std::collections::BTreeSet<&str> = results
        .iter()
        .map(|r| r.site.as_str())
        .chain(built.iter().map(|(id, _)| id.as_str()))
        .collect();
    let not_started: Vec<String> = sites
        .iter()
        .filter(|s| !started.contains(s.id))
        .map(|s| s.id.to_owned())
        .collect();
    let mut step_seconds: BTreeMap<String, f64> = BTreeMap::new();
    let mut not_applicable: BTreeMap<String, usize> = BTreeMap::new();
    for (id, o) in &built {
        for s in &o.steps {
            *step_seconds.entry(s.name.clone()).or_insert(0.0) += s.seconds;
            if s.state == StepState::NotApplicable {
                *not_applicable.entry(s.name.clone()).or_insert(0) += 1;
            }
        }
        results.push(site_result(id, o));
    }
    for id in &not_started {
        results.push(SiteResult {
            site: id.clone(),
            result: "not_started".into(),
            seconds: 0.0,
            pack_bytes: None,
            export_bytes: None,
            failures: Vec::new(),
            not_applicable: Vec::new(),
        });
    }
    let mut failure_causes: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for (id, o) in &built {
        for s in o.failed() {
            failure_causes
                .entry(cause(&s.name, &s.line))
                .or_default()
                .push(id.clone());
        }
    }
    for r in results.iter().filter(|r| r.result == "failed") {
        if !built.iter().any(|(id, _)| *id == r.site) {
            for f in &r.failures {
                failure_causes
                    .entry(cause("build-all", f))
                    .or_default()
                    .push(r.site.clone());
            }
        }
    }
    if stations.state == StepState::Failed {
        failure_causes
            .entry(cause(&stations.name, &stations.line))
            .or_default()
            .push(skimap_common::layout::FRANCE.to_owned());
    }
    let mut step_seconds: Vec<(String, f64)> = step_seconds.into_iter().collect();
    step_seconds.sort_by(|a, b| b.1.total_cmp(&a.1));
    let cache_after = cache_inventory(&ctx.layout.cache_root());
    let downloads: BTreeMap<String, CacheDelta> = cache_after
        .iter()
        .filter_map(|(k, (n1, b1))| {
            let (n0, b0) = cache_before.get(k).copied().unwrap_or((0, 0));
            (n1 != &n0 || b1 != &b0).then(|| {
                (
                    k.clone(),
                    CacheDelta {
                        files: n1 - n0,
                        bytes: b1 - b0,
                    },
                )
            })
        })
        .collect();
    let count = |r: &str| results.iter().filter(|x| x.result == r).count();
    let summary = BatchSummary {
        started_at,
        finished_at: now_rfc3339(),
        seconds: t0.elapsed().as_secs_f64(),
        jobs: opts.jobs,
        sites: sites.len(),
        built: count("built"),
        failed: count("failed"),
        up_to_date: count("up_to_date"),
        not_started: count("not_started"),
        failure_causes,
        step_seconds,
        not_applicable,
        pack_bytes: built.iter().filter_map(|(_, o)| o.pack_bytes).sum(),
        export_bytes: built.iter().filter_map(|(_, o)| o.export_bytes).sum(),
        downloads,
        peak_rss_mb: crate::build::peak_rss_mb(),
        stations,
        meteo,
        results,
    };
    let dir = ctx.layout.france_dir().join("build-all");
    DataLayout::ensure(&dir)?;
    let stamp = Utc::now().format("%Y%m%dT%H%M%SZ");
    skimap_common::output::write_json(&dir.join(format!("summary_{stamp}.json")), &summary)?;
    skimap_common::output::write_json(&dir.join("summary.json"), &summary)?;
    // Every site of the registry, not only those of this batch.
    write_status(&ctx.layout, &registry.buildable())?;
    Ok(summary)
}

/// State of one step of a site in `status.json`.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct StepStatus {
    /// Step.
    pub step: String,
    /// `ok`, `failed`, `blocked`, `not_applicable`, `disabled`.
    pub state: StepState,
    /// Date of the outputs in use (the last successful run of the step), RFC 3339.
    pub updated_at: Option<String>,
    /// Last attempt, RFC 3339 (for a failed step: when it failed).
    pub tried_at: Option<String>,
    /// Error of a failed step.
    pub error: Option<String>,
    /// First failure of a step that has been failing since (kept from one `status.json`
    /// to the next), RFC 3339.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub failing_since: Option<String>,
    /// A step done (`ok`) whose outputs hold nothing: why ([`crate::build::empty_output`],
    /// read from the outputs on disk when the status is written).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub empty: Option<String>,
}

/// Days of failure after which a step is listed in `status.json` `alerts`.
pub const ALERT_AFTER_DAYS: i64 = 3;

/// One site in `status.json`.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct SiteStatus {
    /// Site.
    pub site: String,
    /// `ok` (every step done or not applicable), `failed` (at least one step failed: its
    /// previous outputs, if any, are kept and published with their date), `never_built`.
    pub state: String,
    /// Last run of the site, RFC 3339.
    pub built_at: Option<String>,
    /// Steps that are not done, with their cause.
    pub failed_steps: Vec<StepStatus>,
    /// Steps done that produced nothing, with the reason (B167).
    pub empty_steps: Vec<StepStatus>,
    /// Every step.
    pub steps: Vec<StepStatus>,
}

/// `data/out/france/status.json`: state of every site of the batch from its record
/// (`build-all.json`), for the scheduled catch-up and the server. The dates are the dates
/// of the data in use: a step that failed keeps the outputs of its last success, shown
/// with their date, without any other qualifier.
pub fn write_status(layout: &DataLayout, sites: &[&'static Site]) -> Result<PathBuf> {
    let path = layout.france_dir().join("status.json");
    // First failure of each (site, step) still failing, from the previous status.
    let mut since: BTreeMap<(String, String), String> = BTreeMap::new();
    if let Some(v) = std::fs::read(&path)
        .ok()
        .and_then(|b| serde_json::from_slice::<serde_json::Value>(&b).ok())
    {
        for site in v["by_site"].as_array().into_iter().flatten() {
            for st in site["failed_steps"].as_array().into_iter().flatten() {
                if let (Some(a), Some(b), Some(t)) = (
                    site["site"].as_str(),
                    st["step"].as_str(),
                    st["failing_since"].as_str(),
                ) {
                    since.insert((a.to_owned(), b.to_owned()), t.to_owned());
                }
            }
        }
    }
    let now = Utc::now();
    let mut alerts = Vec::new();
    let mut out = Vec::new();
    for s in sites {
        let Some(rec) = france::read_record(layout, s.id) else {
            out.push(SiteStatus {
                site: s.id.to_owned(),
                state: "never_built".into(),
                built_at: None,
                failed_steps: Vec::new(),
                empty_steps: Vec::new(),
                steps: Vec::new(),
            });
            continue;
        };
        let site_dir = layout.out_dir(s);
        let steps: Vec<StepStatus> = rec
            .outcome
            .steps
            .iter()
            .map(|st| {
                let failed = matches!(st.state, StepState::Failed | StepState::Blocked);
                let tried_at = st.at.clone().or_else(|| Some(rec.finished_at.clone()));
                let failing_since = failed.then(|| {
                    since
                        .get(&(s.id.to_owned(), st.name.clone()))
                        .cloned()
                        .or_else(|| tried_at.clone())
                        .unwrap_or_default()
                });
                if let Some(t) = failing_since
                    .as_deref()
                    .and_then(|t| DateTime::parse_from_rfc3339(t).ok())
                    && (now - t.to_utc()).num_days() >= ALERT_AFTER_DAYS
                {
                    alerts.push(format!(
                        "{} : {} en échec depuis le {}",
                        s.id,
                        st.name,
                        t.format("%d/%m/%Y")
                    ));
                }
                let empty = (st.state == StepState::Ok)
                    .then(|| crate::build::empty_output(&st.name, &site_dir, s))
                    .flatten();
                if let Some(why) = &empty
                    && st.name == "pistes"
                    && s.kind.has_groomed_trails()
                {
                    alerts.push(format!("{} : pistes : vide, {why}", s.id));
                }
                StepStatus {
                    step: st.name.clone(),
                    state: st.state,
                    updated_at: if failed { None } else { st.at.clone() },
                    tried_at,
                    error: failed.then(|| first_line(&st.line, 300)),
                    failing_since,
                    empty,
                }
            })
            .collect();
        let failed_steps: Vec<StepStatus> = steps
            .iter()
            .filter(|x| matches!(x.state, StepState::Failed | StepState::Blocked))
            .cloned()
            .collect();
        let empty_steps: Vec<StepStatus> = steps
            .iter()
            .filter(|x| x.empty.is_some())
            .cloned()
            .collect();
        out.push(SiteStatus {
            site: s.id.to_owned(),
            state: if rec.ok { "ok" } else { "failed" }.into(),
            built_at: Some(rec.finished_at.clone()),
            failed_steps,
            empty_steps,
            steps,
        });
    }
    let count = |k: &str| out.iter().filter(|x| x.state == k).count();
    let empty: Vec<String> = out
        .iter()
        .flat_map(|x| {
            x.empty_steps.iter().map(move |st| {
                format!(
                    "{} : {} : {}",
                    x.site,
                    st.step,
                    st.empty.as_deref().unwrap_or_default()
                )
            })
        })
        .collect();
    skimap_common::output::write_json(
        &path,
        &serde_json::json!({
            "generated_at": now_rfc3339(),
            "sites": out.len(),
            "ok": count("ok"),
            "failed": count("failed"),
            "never_built": count("never_built"),
            "empty": out.iter().filter(|x| !x.empty_steps.is_empty()).count(),
            "empty_steps": empty,
            "alert_after_days": ALERT_AFTER_DAYS,
            "alerts": alerts,
            "by_site": out,
        }),
    )?;
    Ok(path)
}

/// AROME weather (X17) of `sites` in one call: the run is downloaded and each file
/// decoded once for every point of every site (the `meteo` step of each site then finds
/// its layer at the latest run). A failure is recorded; the step of each site retries.
pub fn meteo_step(ctx: &Ctx, sites: &[&Site]) -> StepRecord {
    meteo_step_at(ctx, sites, None)
}

/// [`meteo_step`] at a given AROME run (`None`: the run of `SKIMAP_AROME_RUN`, else the
/// latest complete one).
pub fn meteo_step_at(ctx: &Ctx, sites: &[&Site], run: Option<DateTime<Utc>>) -> StepRecord {
    let t = Instant::now();
    let run = match run.map_or_else(|| ctx.arome_run(), |r| Ok(Some(r))) {
        Ok(r) => r,
        Err(e) => {
            return StepRecord {
                name: "meteo-arome".into(),
                state: StepState::Failed,
                seconds: 0.0,
                line: format!("{e:#}"),
                at: Some(now_rfc3339()),
                outputs: None,
                resumed: false,
            };
        }
    };
    let refreshed = match run {
        Some(run) => skimap_meteo::arome::refresh_run(sites, &ctx.layout, &ctx.dl, run),
        None => skimap_meteo::arome::refresh(sites, &ctx.layout, &ctx.dl),
    };
    let (state, line) = match refreshed {
        Ok(r) => {
            let written = r.sites.iter().filter(|s| s.status == "written").count();
            (
                StepState::Ok,
                format!(
                    "run AROME {} : {written} sites écrits, {} déjà à ce run",
                    r.run,
                    r.sites.len() - written
                ),
            )
        }
        Err(e) => (StepState::Failed, format!("{e:#}")),
    };
    let seconds = t.elapsed().as_secs_f64();
    match state {
        StepState::Ok => println!("météo AROME : {line} ({seconds:.1} s)"),
        _ => println!("météo AROME : ÉCHEC ({seconds:.1} s) : {line}"),
    }
    StepRecord {
        name: "meteo-arome".into(),
        state,
        seconds,
        line,
        at: Some(now_rfc3339()),
        outputs: None,
        resumed: false,
    }
}

/// Refreshes the stations layer of France once, printed and recorded like a step.
/// The registry as the stations step left it (starts, X9), read again from its file
/// (`None`: the evaluation sites, never rewritten).
fn reread(registry: &Registry) -> Result<Option<Registry>> {
    Ok(registry.source().map(Registry::read).transpose()?)
}

/// Whether the census of France is on disk (`domaines.geojson`, `domaines_ways.json`),
/// the input of the stations layer.
fn census_on_disk(layout: &DataLayout) -> bool {
    layout
        .france_layer_dir(skimap_osm::census::LAYER)
        .is_ok_and(|d| {
            d.join("domaines.geojson").is_file() && d.join("domaines_ways.json").is_file()
        })
}

fn stations_step(ctx: &Ctx, france: &France<'_>, force: bool) -> StepRecord {
    let t = Instant::now();
    // The stations layer reads the census on disk: made first in a new data directory
    // (the `domaines` step of a site would make it after the stations step).
    let census = if census_on_disk(&ctx.layout) {
        Ok(())
    } else {
        france.census().map(|_| ())
    };
    let (state, line) = match census.and_then(|()| ctx.stations(force)) {
        Ok(r) => (StepState::Ok, crate::build::describe_stations(&r)),
        Err(e) => (StepState::Failed, format!("{e:#}")),
    };
    let seconds = t.elapsed().as_secs_f64();
    match state {
        StepState::Ok => println!("stations : {line} ({seconds:.1} s)"),
        _ => println!("stations : ÉCHEC ({seconds:.1} s) : {line}"),
    }
    StepRecord {
        name: "stations".into(),
        state,
        seconds,
        line,
        at: Some(now_rfc3339()),
        outputs: None,
        resumed: false,
    }
}

/// Steps whose outputs read those of a step run on its own (CronJobs `skimap-job snow`,
/// `bera`, `status`, `refresh`), as declared in [`Ctx::build_all_with`]: the pack and
/// the export take every layer.
fn dependents(step: &str) -> &'static [&'static str] {
    match step {
        "snow" => &["offpiste-prepare", "build-pack", "export"],
        "offpiste-prepare" | "bera" | "status-poll" | "meteo" | "refuges" => {
            &["build-pack", "export"]
        }
        "build-pack" => &["export"],
        _ => &[],
    }
}

/// Records in `build-all.json` a step of a site run on its own, outside `build-all`
/// (#164): its outputs, line and date, so that the next catch-up takes it over instead
/// of doing it again (the snow of the morning CronJob was redone by the catch-up that
/// followed, with the off-piste rasters and the pack). The steps that read its outputs
/// and were not run after it are marked to be redone (state `blocked`); the next
/// catch-up runs those only. Nothing when the site has no record (never built by
/// `build-all`: the catch-up builds it whole).
pub fn record_standalone(layout: &DataLayout, site: &Site, step: &str, line: &str) -> Result<()> {
    let dir = layout.out_dir(site);
    let Some(mut rec) = france::read_record(layout, site.id) else {
        return Ok(());
    };
    let _lock = lock_site(&dir)?;
    let at = now_rfc3339();
    let outputs =
        crate::build::step_dir(step).and_then(|d| crate::build::fingerprint(&dir.join(d)));
    let Some(i) = rec.outcome.steps.iter().position(|s| s.name == step) else {
        return Ok(());
    };
    rec.outcome.steps[i] = StepRecord {
        name: step.to_owned(),
        state: StepState::Ok,
        seconds: 0.0,
        line: format!("{line} (fait seul, hors build-all)"),
        at: Some(at.clone()),
        outputs,
        resumed: false,
    };
    for d in dependents(step) {
        if let Some(s) = rec.outcome.steps.iter_mut().find(|s| s.name == *d)
            && s.state == StepState::Ok
        {
            s.state = StepState::Blocked;
            s.line = format!("à refaire : {step} refait seul le {at}");
            s.resumed = false;
        }
    }
    rec.ok = rec.outcome.ok();
    skimap_common::output::write_json(&france::record_path(layout, site.id), &rec)?;
    Ok(())
}

/// Builds one site under its lock: orphan temporary files removed, steps of the previous
/// (or interrupted) run taken over when still valid ([`crate::build::Resume`]), the record
/// written after every step.
fn build_site(
    ctx: &Ctx,
    site: &'static Site,
    inp: BuildInputs,
    shared: Shared<'_>,
    force: bool,
    max_age: Duration,
    log: &mut dyn std::io::Write,
) -> Result<BuildOutcome> {
    let dir = ctx.layout.out_dir(site);
    DataLayout::ensure(&dir)?;
    let _lock = lock_site(&dir)?;
    let removed = remove_orphan_tmp(&dir);
    if removed > 0 {
        tracing::warn!(site = site.id, removed, "orphan temporary files removed");
    }
    let previous =
        france::read_record(&ctx.layout, site.id).filter(|r| !force && r.inputs.resumable_by(&inp));
    let resume = previous.as_ref().map(|r| crate::build::Resume {
        previous: &r.outcome.steps,
        max_age,
        pack_bytes: r.outcome.pack_bytes,
        export_bytes: r.outcome.export_bytes,
    });
    let started_at = now_rfc3339();
    let record_path = france::record_path(&ctx.layout, site.id);
    let journal = |steps: &[StepRecord]| {
        let rec = BuildRecord {
            inputs: inp.clone(),
            started_at: started_at.clone(),
            finished_at: now_rfc3339(),
            ok: false,
            outcome: BuildOutcome {
                site: site.id.to_owned(),
                steps: steps.to_vec(),
                seconds: 0.0,
                pack_bytes: None,
                export_bytes: None,
            },
        };
        if let Err(e) = skimap_common::output::write_json(&record_path, &rec) {
            tracing::warn!(site = site.id, error = %e, "journal not written");
        }
    };
    let shared = Shared {
        resume,
        journal: Some(&journal),
        ..shared
    };
    let o = ctx.build_all_with(site, &shared, log);
    let rec = BuildRecord {
        inputs: inp.clone(),
        started_at: started_at.clone(),
        finished_at: now_rfc3339(),
        ok: o.ok(),
        outcome: o.clone(),
    };
    skimap_common::output::write_json(&record_path, &rec)?;
    Ok(o)
}

fn build_one(
    ctx: &Ctx,
    site: &'static Site,
    inp: BuildInputs,
    shared: Shared<'_>,
    opts: &BatchOptions,
) -> Result<BuildOutcome> {
    let dir = ctx.layout.out_dir(site);
    DataLayout::ensure(&dir)?;
    let log_path: PathBuf = dir.join(france::BUILD_LOG);
    let mut log = std::io::BufWriter::new(open_build_log(&log_path)?);
    build_site(ctx, site, inp, shared, opts.force, opts.max_age, &mut log)
}

/// Opens the log of a site build (`build-all.log`), written as the build goes.
/// A new inode (the previous log unlinked): a working copy linked to the previous log
/// keeps it (#164).
fn open_build_log(path: &Path) -> Result<std::fs::File> {
    Ok(skimap_common::output::create_log(path)?)
}

/// `build-all <site>`: the steps printed on stdout, the record written as in a batch
/// (`build-all.json`, read by the catalogue and by the up-to-date test of `--all`).
pub fn build_single(
    ctx: &Ctx,
    registry: &Registry,
    site: &'static Site,
    force: bool,
    max_age: Duration,
) -> Result<()> {
    let (stations, o) = build_single_with(ctx, registry, site, force, max_age, &Pipeline)?;
    o.into_result()?;
    if stations.state == StepState::Failed {
        anyhow::bail!("build-all : stations en échec : {}", stations.line);
    }
    Ok(())
}

/// [`build_single`] with the given steps: the stations step, then the site built with
/// its record as the stations step left the registry.
pub fn build_single_with(
    ctx: &Ctx,
    registry: &Registry,
    site: &'static Site,
    force: bool,
    max_age: Duration,
    steps: &dyn Steps,
) -> Result<(StepRecord, BuildOutcome)> {
    // As in a batch (#164): the stations step rewrites the starts of the registry (X9)
    // after new station cards. Run inside the build of the site, after its inputs were
    // read, it left the record of the site built with the old starts, and the next run,
    // reading the new ones, took nothing over.
    let france = France::new(ctx);
    let stations = steps.stations(ctx, &france, force);
    let reread = reread(registry)?;
    let registry = reread.as_ref().unwrap_or(registry);
    let site = registry.get(site.id)?;
    let inp = inputs(ctx, registry, site)?;
    let shared = Shared {
        france: Some(&france),
        ..Shared::default()
    };
    let o = steps.single(ctx, site, inp, shared, force, max_age)?;
    Ok((stations, o))
}

/// French text of `status.json` ([`write_status`]): counts, then every failed step and
/// every step done without any output, with its reason, then the alerts.
pub fn describe_status(path: &Path) -> Result<Vec<String>> {
    let v: serde_json::Value = serde_json::from_slice(
        &std::fs::read(path).with_context(|| format!("{}", path.display()))?,
    )
    .with_context(|| format!("{}", path.display()))?;
    let n = |k: &str| v[k].as_u64().unwrap_or(0);
    let mut l = vec![format!(
        "état : {} sites, {} sans échec, {} en échec, {} jamais construits, {} avec une étape vide ({})",
        n("sites"),
        n("ok"),
        n("failed"),
        n("never_built"),
        n("empty"),
        path.display()
    )];
    for site in v["by_site"].as_array().into_iter().flatten() {
        for st in site["failed_steps"].as_array().into_iter().flatten() {
            l.push(format!(
                "  échec  {} : {} : {}",
                site["site"].as_str().unwrap_or("?"),
                st["step"].as_str().unwrap_or("?"),
                st["error"].as_str().unwrap_or("")
            ));
        }
    }
    for e in v["empty_steps"].as_array().into_iter().flatten() {
        l.push(format!("  vide   {}", e.as_str().unwrap_or("?")));
    }
    for a in v["alerts"].as_array().into_iter().flatten() {
        l.push(format!("  alerte {}", a.as_str().unwrap_or("?")));
    }
    Ok(l)
}

/// French text of a summary.
pub fn describe(s: &BatchSummary) -> Vec<String> {
    let mut l = vec![format!(
        "build-all : {} sites en {:.0} s ({} en parallèle) : {} construits sans échec, {} avec échec, {} à jour, {} non commencés",
        s.sites, s.seconds, s.jobs, s.built, s.failed, s.up_to_date, s.not_started
    )];
    l.push(format!(
        "packs : {:.1} Mo ; exports : {:.1} Mo ; mémoire maximale du processus : {}",
        s.pack_bytes as f64 / 1e6,
        s.export_bytes as f64 / 1e6,
        s.peak_rss_mb
            .map_or("?".to_owned(), |m| format!("{m:.0} Mo"))
    ));
    l.push(format!(
        "stations (France, une fois) : {}{} ({:.1} s)",
        if s.stations.state == StepState::Failed {
            "ÉCHEC : "
        } else {
            ""
        },
        first_line(&s.stations.line, 300),
        s.stations.seconds
    ));
    l.push("temps par étape (somme sur les sites construits) :".into());
    for (k, v) in &s.step_seconds {
        l.push(format!("  {k:<19} {v:>9.1} s"));
    }
    if !s.not_applicable.is_empty() {
        l.push(format!(
            "sans objet : {}",
            s.not_applicable
                .iter()
                .map(|(k, v)| format!("{k} {v}"))
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }
    if !s.failure_causes.is_empty() {
        l.push("échecs par cause :".into());
        for (k, v) in &s.failure_causes {
            l.push(format!("  {} site(s) : {k} ({})", v.len(), v.join(", ")));
        }
    }
    let (files, bytes): (i64, i64) = s
        .downloads
        .values()
        .fold((0, 0), |(a, b), d| (a + d.files, b + d.bytes));
    l.push(format!(
        "téléchargements : {files} fichiers, {:.1} Mo ({})",
        bytes as f64 / 1e6,
        s.downloads
            .iter()
            .map(|(k, d)| format!("{k} {} / {:.1} Mo", d.files, d.bytes as f64 / 1e6))
            .collect::<Vec<_>>()
            .join(", ")
    ));
    l
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A new build log leaves a hard link to the previous one (working copy) intact
    /// (#164).
    #[test]
    fn build_log_keeps_hard_links() {
        use std::io::Write;
        let d = tempfile::tempdir().unwrap();
        let path = d.path().join(france::BUILD_LOG);
        let other = d.path().join("copy.log");
        std::fs::write(&path, b"previous build").unwrap();
        std::fs::hard_link(&path, &other).unwrap();
        let mut f = open_build_log(&path).unwrap();
        f.write_all(b"new build").unwrap();
        drop(f);
        assert_eq!(std::fs::read(&path).unwrap(), b"new build");
        assert_eq!(std::fs::read(&other).unwrap(), b"previous build");
    }

    #[test]
    fn causes_ignore_numbers() {
        assert_eq!(
            cause("terrain", "tile 0998-6426: HTTP 503\nmore"),
            "terrain : tile #-#: HTTP #"
        );
        assert_eq!(cause("snow", "x"), "snow : x");
    }

    /// Steps of a batch without the pipeline: the stations step adds a start to the site
    /// in the registry file when asked (as `france::refresh_registry_starts` does after
    /// new station cards); a site build records its inputs and whether the previous
    /// record could be taken over (as `build_site`).
    struct Fake {
        new_start: std::sync::atomic::AtomicBool,
        /// Per site build: site, starts of the site built, previous record taken over.
        builds: Mutex<Vec<(String, usize, bool)>>,
    }

    impl Steps for Fake {
        fn stations(&self, ctx: &Ctx, _france: &France<'_>, _force: bool) -> StepRecord {
            if self
                .new_start
                .swap(false, std::sync::atomic::Ordering::SeqCst)
            {
                let path = Registry::path(&ctx.layout).unwrap();
                let mut file: skimap_common::site::RegistryFile =
                    serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
                let at = file.sites[0].commune_center;
                file.sites[0].starts.push(skimap_common::site::Start {
                    id: "nouveau".into(),
                    name: "Nouveau départ".into(),
                    phrase: "le nouveau départ".into(),
                    phrase_en: None,
                    lon: at.lon,
                    lat: at.lat,
                    provenance: "test".into(),
                    access_m: None,
                });
                Registry::write(&path, &file).unwrap();
            }
            StepRecord {
                name: "stations".into(),
                state: StepState::Ok,
                seconds: 0.0,
                line: String::new(),
                at: Some(now_rfc3339()),
                outputs: None,
                resumed: false,
            }
        }

        fn meteo(&self, _ctx: &Ctx, _sites: &[&Site]) -> StepRecord {
            StepRecord {
                name: "meteo-arome".into(),
                state: StepState::Ok,
                seconds: 0.0,
                line: String::new(),
                at: Some(now_rfc3339()),
                outputs: None,
                resumed: false,
            }
        }

        fn site(
            &self,
            ctx: &Ctx,
            site: &'static Site,
            inp: BuildInputs,
            _shared: Shared<'_>,
            _opts: &BatchOptions,
        ) -> Result<BuildOutcome> {
            self.build(ctx, site, inp)
        }

        fn single(
            &self,
            ctx: &Ctx,
            site: &'static Site,
            inp: BuildInputs,
            _shared: Shared<'_>,
            _force: bool,
            _max_age: Duration,
        ) -> Result<BuildOutcome> {
            self.build(ctx, site, inp)
        }
    }

    impl Fake {
        fn new() -> Self {
            Self {
                new_start: false.into(),
                builds: Mutex::new(Vec::new()),
            }
        }

        /// A site build: its record.
        fn build(&self, ctx: &Ctx, site: &'static Site, inp: BuildInputs) -> Result<BuildOutcome> {
            let taken_over = france::read_record(&ctx.layout, site.id)
                .is_some_and(|r| r.inputs.resumable_by(&inp));
            self.builds
                .lock()
                .unwrap()
                .push((site.id.to_owned(), site.starts.len(), taken_over));
            let step = |name: &str| StepRecord {
                name: name.into(),
                state: StepState::Ok,
                seconds: 0.0,
                line: String::new(),
                at: Some(now_rfc3339()),
                outputs: None,
                resumed: taken_over,
            };
            let outcome = BuildOutcome {
                site: site.id.to_owned(),
                steps: vec![step("meteo"), step("build-pack"), step("export")],
                seconds: 0.0,
                pack_bytes: None,
                export_bytes: None,
            };
            let rec = BuildRecord {
                inputs: inp,
                started_at: now_rfc3339(),
                finished_at: now_rfc3339(),
                ok: true,
                outcome: outcome.clone(),
            };
            skimap_common::output::write_json(&france::record_path(&ctx.layout, site.id), &rec)?;
            Ok(outcome)
        }
    }

    /// A data directory whose registry holds one evaluation site.
    fn one_site_registry() -> (tempfile::TempDir, Ctx) {
        let t = tempfile::tempdir().unwrap();
        let cfg = skimap_common::config::Config::from_vars([(
            "SKIMAP_DATA_DIR".to_owned(),
            t.path().display().to_string(),
        )])
        .unwrap();
        let ctx = Ctx::new(&cfg).unwrap();
        let path = Registry::path(&ctx.layout).unwrap();
        let record = skimap_common::site::evaluation_records()[0].clone();
        Registry::write(
            &path,
            &skimap_common::site::RegistryFile {
                generated_at: now_rfc3339(),
                notes: Vec::new(),
                sites: vec![record],
            },
        )
        .unwrap();
        (t, ctx)
    }

    /// Three passes of the catch-up, `build` building the site of the registry it is
    /// given: the stations step finds a new start during the second; the third must take
    /// over the record of the second, written with the registry as the stations step left
    /// it.
    fn catch_up(ctx: &Ctx, steps: &Fake, build: &dyn Fn(&Registry, &'static Site)) {
        // A pass: a new process reads the registry, a new AROME run marks the pack of the
        // site to be redone.
        let pass = |new_start: bool| {
            let registry = Registry::load(&ctx.layout).unwrap();
            let site = registry.buildable()[0];
            if let Some(r) = france::read_record(&ctx.layout, site.id) {
                assert!(r.ok);
                record_standalone(&ctx.layout, site, "meteo", "run AROME suivant").unwrap();
            }
            steps
                .new_start
                .store(new_start, std::sync::atomic::Ordering::SeqCst);
            build(&registry, site);
            steps.builds.lock().unwrap().pop().unwrap()
        };
        let (id, starts0, _) = pass(false);
        // The stations step finds new starts for the site.
        let (_, starts1, _) = pass(true);
        let recorded = france::read_record(&ctx.layout, &id).unwrap().inputs.record;
        let on_disk = Registry::load(&ctx.layout)
            .unwrap()
            .record(&id)
            .unwrap()
            .clone();
        let (_, _, taken_over) = pass(false);
        assert!(
            taken_over,
            "the next pass rebuilt the site whole: recorded starts {:?}, registry starts {:?}",
            recorded.starts.len(),
            on_disk.starts.len()
        );
        assert_eq!(
            recorded, on_disk,
            "the pass records the registry as the stations step left it"
        );
        assert_eq!(starts1, starts0 + 1, "built with the new starts");
    }

    /// #164, catch-up of 17/11/2026: the inputs of each site (its registry record) were
    /// read before the stations step rewrote the starts of the registry, so the pass
    /// built and recorded the old record, and the next pass, reading the new one, found
    /// no record to take over and rebuilt the site whole, without any weather reason.
    #[test]
    fn inputs_are_read_after_the_stations_step_refreshed_the_registry() {
        let (_t, ctx) = one_site_registry();
        let opts = BatchOptions {
            jobs: 1,
            force: false,
            max_age: Duration::from_secs(86_400),
            stop_after: None,
            verify: false,
        };
        let steps = Fake::new();
        catch_up(&ctx, &steps, &|registry, site| {
            build_many_with(&ctx, registry, &[site], &opts, &steps).unwrap();
        });
    }

    /// #164: the same for `build-all <site>`, whose stations step ran inside the build of
    /// the site, after its inputs were read.
    #[test]
    fn single_site_inputs_are_read_after_the_stations_step_refreshed_the_registry() {
        let (_t, ctx) = one_site_registry();
        let steps = Fake::new();
        catch_up(&ctx, &steps, &|registry, site| {
            build_single_with(
                &ctx,
                registry,
                site,
                false,
                Duration::from_secs(86_400),
                &steps,
            )
            .unwrap();
        });
    }

    #[test]
    fn a_step_run_alone_is_recorded_and_what_reads_it_is_to_be_redone() {
        let t = tempfile::tempdir().unwrap();
        let layout = DataLayout::new(t.path());
        let site = skimap_common::site::site("cervieres").unwrap();
        let dir = layout.out_dir(site);
        std::fs::create_dir_all(dir.join("snow")).unwrap();
        std::fs::write(dir.join("snow/depth.tif"), b"day 1").unwrap();
        let step = |name: &str| StepRecord {
            name: name.into(),
            state: StepState::Ok,
            seconds: 1.0,
            line: String::new(),
            at: Some("2026-11-17T06:00:00Z".into()),
            outputs: crate::build::step_dir(name)
                .and_then(|d| crate::build::fingerprint(&dir.join(d))),
            resumed: false,
        };
        let names = [
            "terrain",
            "snow",
            "offpiste-prepare",
            "build-pack",
            "export",
        ];
        let rec = BuildRecord {
            inputs: BuildInputs {
                record: skimap_common::site::evaluation_records()[0].clone(),
                version: "0.1.0".into(),
                personal_sources: false,
                revision: france::BUILD_REVISION,
            },
            started_at: "2026-11-17T06:00:00Z".into(),
            finished_at: "2026-11-17T06:01:00Z".into(),
            ok: true,
            outcome: BuildOutcome {
                site: site.id.into(),
                steps: names.iter().map(|n| step(n)).collect(),
                seconds: 60.0,
                pack_bytes: None,
                export_bytes: None,
            },
        };
        skimap_common::output::write_json(&france::record_path(&layout, site.id), &rec).unwrap();
        assert!(outputs_intact(&layout, site, &rec));

        // The snow CronJob: snow, off-piste rasters, pack, each recorded after it ran.
        std::fs::write(dir.join("snow/depth.tif"), b"day 2").unwrap();
        record_standalone(&layout, site, "snow", "simulation du 2026-11-17").unwrap();
        let r = france::read_record(&layout, site.id).unwrap();
        let state =
            |r: &BuildRecord, n: &str| r.outcome.steps.iter().find(|s| s.name == n).unwrap().state;
        assert_eq!(state(&r, "snow"), StepState::Ok);
        assert_eq!(
            state(&r, "terrain"),
            StepState::Ok,
            "not a reader of the snow"
        );
        for n in ["offpiste-prepare", "build-pack", "export"] {
            assert_eq!(state(&r, n), StepState::Blocked, "{n}");
        }
        assert!(outputs_intact(&layout, site, &r), "new snow recorded");
        assert!(!r.ok);
        record_standalone(&layout, site, "offpiste-prepare", "x").unwrap();
        record_standalone(&layout, site, "build-pack", "x").unwrap();
        let r = france::read_record(&layout, site.id).unwrap();
        for n in ["snow", "offpiste-prepare", "build-pack"] {
            assert_eq!(state(&r, n), StepState::Ok, "{n}");
        }
        // The export, not run by the CronJob, is the only step the catch-up redoes.
        assert_eq!(state(&r, "export"), StepState::Blocked);
        assert!(!r.ok);

        // A site never built by build-all: nothing recorded.
        let other = skimap_common::site::site("chapelle-des-bois").unwrap();
        record_standalone(&layout, other, "snow", "x").unwrap();
        assert!(france::read_record(&layout, other.id).is_none());
    }

    #[test]
    fn freshness() {
        let rec = BuildRecord {
            inputs: BuildInputs {
                record: skimap_common::site::evaluation_records()[0].clone(),
                version: "0.1.0".into(),
                personal_sources: false,
                revision: france::BUILD_REVISION,
            },
            started_at: "2026-11-15T10:00:00Z".into(),
            finished_at: "2026-11-15T10:01:00Z".into(),
            ok: true,
            outcome: BuildOutcome {
                site: "cervieres".into(),
                steps: Vec::new(),
                seconds: 60.0,
                pack_bytes: Some(1),
                export_bytes: None,
            },
        };
        let now = DateTime::parse_from_rfc3339("2026-11-15T12:00:00Z")
            .unwrap()
            .to_utc();
        let day = Duration::from_secs(86_400);
        assert!(up_to_date(&rec, &rec.inputs, day, now));
        assert!(!up_to_date(
            &rec,
            &rec.inputs,
            Duration::from_secs(3600),
            now
        ));
        let mut other = rec.inputs.clone();
        other.personal_sources = true;
        assert!(!up_to_date(&rec, &other, day, now));
        let mut other = rec.inputs.clone();
        other.record.bbox.north += 0.01;
        assert!(!up_to_date(&rec, &other, day, now));
        let mut failed = rec.clone();
        failed.ok = false;
        assert!(!up_to_date(&failed, &rec.inputs, day, now));
        // A new revision of the steps: not up to date, but its steps can be taken over.
        let mut older = rec.inputs.clone();
        older.revision = 1;
        assert!(!up_to_date(
            &BuildRecord {
                inputs: older.clone(),
                ..rec.clone()
            },
            &rec.inputs,
            day,
            now
        ));
        assert!(older.resumable_by(&rec.inputs));
        assert!(!other.resumable_by(&rec.inputs), "another site record");
    }

    /// B167: a step that ends `ok` without producing anything is listed in `status.json`
    /// with its reason (a groomed station without pistes is an alert), never silently.
    #[test]
    fn status_lists_empty_steps() {
        let d = tempfile::tempdir().unwrap();
        let layout = DataLayout::new(d.path());
        let record = skimap_common::site::evaluation_records()
            .iter()
            .find(|r| r.kind.has_groomed_trails())
            .unwrap()
            .clone();
        let site = skimap_common::site::interned(&record).unwrap();
        let dir = layout.out_dir(site);
        let fc = |n: usize| {
            serde_json::json!({
                "type": "FeatureCollection",
                "features": (0..n).map(|_| serde_json::json!({
                    "type": "Feature", "properties": {},
                    "geometry": {"type": "Point", "coordinates": [6.0, 45.0]}
                })).collect::<Vec<_>>()
            })
            .to_string()
        };
        std::fs::create_dir_all(dir.join("pistes")).unwrap();
        std::fs::write(dir.join("pistes/pistes.geojson"), fc(0)).unwrap();
        std::fs::create_dir_all(dir.join("refuges")).unwrap();
        std::fs::write(dir.join("refuges/refuges_info.geojson"), fc(0)).unwrap();
        std::fs::write(dir.join("refuges/osm.geojson"), fc(2)).unwrap();
        std::fs::write(dir.join("refuges/camptocamp.geojson"), fc(0)).unwrap();
        let step = |name: &str| StepRecord {
            name: name.into(),
            state: StepState::Ok,
            seconds: 1.0,
            line: "0 ways".into(),
            at: Some("2026-11-17T10:00:00Z".into()),
            outputs: None,
            resumed: false,
        };
        let rec = BuildRecord {
            inputs: BuildInputs {
                record,
                version: "0.1.0".into(),
                personal_sources: false,
                revision: france::BUILD_REVISION,
            },
            started_at: "2026-11-17T10:00:00Z".into(),
            finished_at: "2026-11-17T10:01:00Z".into(),
            ok: true,
            outcome: BuildOutcome {
                site: site.id.into(),
                steps: vec![step("pistes"), step("refuges"), step("zones")],
                seconds: 60.0,
                pack_bytes: None,
                export_bytes: None,
            },
        };
        skimap_common::output::write_json(&france::record_path(&layout, site.id), &rec).unwrap();
        let path = write_status(&layout, &[site]).unwrap();
        let v: serde_json::Value = serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
        let s = &v["by_site"][0];
        let empty: Vec<&str> = s["empty_steps"]
            .as_array()
            .expect("empty_steps")
            .iter()
            .map(|x| x["step"].as_str().unwrap())
            .collect();
        // Pistes: no feature; zones: no file at all; refuges: 2 OSM points, not empty.
        assert_eq!(empty, ["pistes", "zones"], "{s}");
        assert!(
            s["empty_steps"][0]["empty"]
                .as_str()
                .unwrap()
                .contains("station damée"),
            "{s}"
        );
        assert_eq!(v["empty"], 1, "{v}");
        let alerts = v["alerts"].as_array().unwrap();
        assert!(
            alerts
                .iter()
                .any(|a| a.as_str().unwrap().contains("pistes : vide")),
            "{v}"
        );
    }

    #[test]
    fn inventory_counts_bodies() {
        let d = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(d.path().join("ign")).unwrap();
        std::fs::write(d.path().join("ign/a.body"), b"1234").unwrap();
        std::fs::write(d.path().join("ign/a.json"), b"{}").unwrap();
        let inv = cache_inventory(d.path());
        assert_eq!(inv["ign"], (1, 4));
    }
}
