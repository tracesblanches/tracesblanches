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
use jiff::Timestamp;
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
    /// Command run after each site built, with the directory of each of its packs
    /// appended (`skimap-server publish`): a pass killed later keeps the sites already
    /// published (#172).
    pub publish: Option<Vec<String>>,
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

/// Estimated peak memory of a site build per cell of its 5 m grid, bytes: the heaviest
/// step per cell, the avalanche exposure layer (`LAYER_PEAK_BYTES_PER_CELL`, steep-grid
/// bound). Measured alone with 3 threads on 18/11/2026 (#173), a 27.8-million-cell Jura
/// traverse / a 7.7-million-cell Vercors domain: exposure 49 / 85 B per cell, build-pack
/// 38 / 47, offpiste-prepare 29 / 30, terrain 14 / 15, snow 10 / 11, export 7 / 11.
pub const SITE_BYTES_PER_CELL: f64 = skimap_avalanche::exposure::LAYER_PEAK_BYTES_PER_CELL;

/// Least estimate of a site build, bytes: the coverage step peaks near 300 MiB whatever
/// the size of the site (ARCEP layers, 270 to 299 MiB measured).
pub const SITE_MIN_BYTES: u64 = 320 << 20;

/// Memory limit when neither the configuration (`SKIMAP_BUILD_MEMORY_MIB`) nor the cgroup
/// of the container (`/sys/fs/cgroup/memory.max`) gives one: the limit of the pipeline
/// pod.
pub const DEFAULT_MEMORY_LIMIT: u64 = 4 << 30;

/// Share of the memory limit kept out of the plan: the France inputs shared by the
/// sites (planet extract, census, SRN areas), the allocator and the estimate error.
pub const MEMORY_MARGIN: f64 = 0.25;

/// Cells of the 5 m terrain grid of a site (its box in Lambert-93, snapped outward);
/// `None` when the box cannot be projected.
pub fn grid_cells(site: &Site) -> Option<u64> {
    let e = skimap_terrain::Extent::from_bbox(site.bbox).ok()?;
    let c = skimap_terrain::dem::CELL;
    let w = ((e.max_x - e.min_x) / c).ceil() + 1.0;
    let h = ((e.max_y - e.min_y) / c).ceil() + 1.0;
    Some((w * h) as u64)
}

/// Estimated peak memory of the build of a site, bytes ([`SITE_BYTES_PER_CELL`] per cell
/// of its grid, at least [`SITE_MIN_BYTES`]).
pub fn site_memory(site: &Site) -> u64 {
    let cells = grid_cells(site).unwrap_or(0);
    ((cells as f64 * SITE_BYTES_PER_CELL) as u64).max(SITE_MIN_BYTES)
}

/// Limit of `/sys/fs/cgroup/memory.max` (cgroup v2), bytes; `None` for `max` or garbage.
pub fn parse_cgroup_memory_max(text: &str) -> Option<u64> {
    text.trim().parse().ok().filter(|n| *n > 0)
}

/// Memory limit of the batch, bytes, and where it comes from: the configuration, else
/// the cgroup of the container, else [`DEFAULT_MEMORY_LIMIT`].
pub fn memory_limit(configured_mib: Option<u64>) -> (u64, &'static str) {
    if let Some(m) = configured_mib {
        return (m << 20, "SKIMAP_BUILD_MEMORY_MIB");
    }
    match std::fs::read_to_string("/sys/fs/cgroup/memory.max")
        .ok()
        .as_deref()
        .and_then(parse_cgroup_memory_max)
    {
        Some(b) => (b, "cgroup"),
        None => (DEFAULT_MEMORY_LIMIT, "défaut"),
    }
}

/// Memory the sites built together may add up to under `limit` ([`MEMORY_MARGIN`]).
pub fn memory_budget(limit: u64) -> u64 {
    (limit as f64 * (1.0 - MEMORY_MARGIN)) as u64
}

/// Next site to start (#173): the first of `waiting` (estimated peaks, queue order)
/// that fits in `budget` beside the running sites (`running`: sum of their estimates);
/// a site alone above the budget starts only when no site runs. `None`: wait for a site
/// to end.
pub fn next_fitting(
    waiting: &[u64],
    running: u64,
    running_count: usize,
    budget: u64,
) -> Option<usize> {
    if running_count == 0 {
        return (!waiting.is_empty()).then_some(0);
    }
    waiting
        .iter()
        .position(|e| running.saturating_add(*e) <= budget)
}

/// Sites waiting and running in a batch, by estimated memory.
struct Plan<T> {
    waiting: Vec<(T, u64)>,
    running: u64,
    running_count: usize,
    /// Largest sum of the estimates of the sites running together.
    max_running: u64,
    /// Memory the running sites may add up to ([`memory_budget`]).
    budget: u64,
}

/// Takes the next site that fits ([`next_fitting`]), waiting for running sites to end
/// when none does; `None` when no site is left.
fn plan_next<T>(plan: &Mutex<Plan<T>>, ended: &std::sync::Condvar) -> Option<(T, u64)> {
    let mut p = plan.lock().unwrap_or_else(|p| p.into_inner());
    loop {
        if p.waiting.is_empty() {
            return None;
        }
        let est: Vec<u64> = p.waiting.iter().map(|w| w.1).collect();
        if let Some(i) = next_fitting(&est, p.running, p.running_count, p.budget) {
            let (t, m) = p.waiting.remove(i);
            p.running += m;
            p.running_count += 1;
            p.max_running = p.max_running.max(p.running);
            return Some((t, m));
        }
        p = ended.wait(p).unwrap_or_else(|p| p.into_inner());
    }
}

/// Lock file of a site build, in its output directory.
pub const LOCK_FILE: &str = ".build-all.lock";

/// Takes the build lock of a site (no two builds of one site at once, whatever the
/// process); released when the returned file is dropped (or the process dies).
pub fn lock_site(dir: &Path) -> Result<skimap_common::output::FileLock> {
    match skimap_common::output::FileLock::try_lock(&dir.join(LOCK_FILE))? {
        Some(lock) => Ok(lock),
        None => anyhow::bail!(
            "{} : une autre construction de ce site est en cours",
            dir.display()
        ),
    }
}

/// Attempt marker of a site build (#172), in its output directory.
pub const ATTEMPT_FILE: &str = "build-all.attempt.json";

/// Attempts of a site build killed with the process (#172).
#[derive(Debug, Clone, Default, PartialEq, Serialize, serde::Deserialize)]
pub struct Attempt {
    /// Start of an attempt not finished yet, RFC 3339.
    pub started_at: Option<String>,
    /// Attempts killed in a row.
    pub killed: u32,
    /// Start of the last attempt killed, RFC 3339.
    pub last_killed_at: Option<String>,
    /// Fingerprint of the inputs of the last attempt.
    pub inputs: String,
}

/// Attempts killed in a row after which a site is set aside (#172).
pub const SET_ASIDE_AFTER: u32 = 2;

/// Hours after which a site set aside is tried again with unchanged inputs (#172).
pub const RETRY_SET_ASIDE_HOURS: i64 = 24;

impl Attempt {
    /// The attempt marker of the site whose output directory is `dir` (empty when none).
    pub fn read(dir: &Path) -> Self {
        std::fs::read(dir.join(ATTEMPT_FILE))
            .ok()
            .and_then(|b| serde_json::from_slice(&b).ok())
            .unwrap_or_default()
    }

    fn write(&self, dir: &Path) -> Result<()> {
        Ok(skimap_common::output::write_json(
            &dir.join(ATTEMPT_FILE),
            self,
        )?)
    }

    /// Whether the site is set aside: killed [`SET_ASIDE_AFTER`] times in a row with the
    /// inputs `inputs`, the last time less than [`RETRY_SET_ASIDE_HOURS`] ago.
    pub fn set_aside(&self, inputs: &str, now: Timestamp) -> bool {
        self.killed >= SET_ASIDE_AFTER
            && self.inputs == inputs
            && self
                .last_killed_at
                .as_deref()
                .and_then(|t| t.parse::<Timestamp>().ok())
                .is_some_and(|t| (now - t).as_hours() < RETRY_SET_ASIDE_HOURS)
    }
}

/// Line of a site set aside, for the summary and the alerts of `status.json`.
fn set_aside_line(a: &Attempt) -> String {
    format!(
        "mis de côté : {} tentatives tuées avec le processus (mémoire, délai), la dernière le {} ; réessayé quand ses entrées changent ou {RETRY_SET_ASIDE_HOURS} h après",
        a.killed,
        a.last_killed_at.as_deref().unwrap_or("?")
    )
}

/// Fingerprint of the inputs of a site build, for [`Attempt::inputs`].
fn inputs_fingerprint(inp: &BuildInputs) -> String {
    use sha2::{Digest, Sha256};
    hex::encode(Sha256::digest(
        serde_json::to_vec(inp).expect("inputs serialize"),
    ))
}

/// Counts the attempt of a site that a previous process started and never finished
/// (killed with it: OOM, deadline): the marker is there and nobody holds the site lock.
fn reckon_attempt(dir: &Path) -> Result<Attempt> {
    let mut a = Attempt::read(dir);
    if a.started_at.is_some() && lock_site(dir).is_ok() {
        a.killed += 1;
        a.last_killed_at = a.started_at.take();
        a.write(dir)?;
    }
    Ok(a)
}

/// Notes the start of an attempt before a site is built (#172).
fn begin_attempt(dir: &Path, inputs: &str) -> Result<()> {
    DataLayout::ensure(dir)?;
    let mut a = Attempt::read(dir);
    a.started_at = Some(now_rfc3339());
    a.inputs = inputs.to_owned();
    a.write(dir)
}

/// The attempt finished (whatever the outcome of its steps): the process was not killed.
fn end_attempt(dir: &Path) -> Result<()> {
    match std::fs::remove_file(dir.join(ATTEMPT_FILE)) {
        Err(e) if e.kind() != std::io::ErrorKind::NotFound => {
            Err(e).with_context(|| format!("{}", dir.join(ATTEMPT_FILE).display()))
        }
        _ => Ok(()),
    }
}

/// Pack directories of a site to publish: its pack, then its voice packs (VO6), those
/// holding a `manifest.json`.
pub fn pack_dirs(site_dir: &Path) -> Vec<PathBuf> {
    let mut v = vec![site_dir.join("pack")];
    if let Ok(rd) = std::fs::read_dir(site_dir.join("voice-pack")) {
        let mut voices: Vec<PathBuf> = rd.flatten().map(|e| e.path()).collect();
        voices.sort();
        v.extend(voices);
    }
    v.retain(|d| d.join("manifest.json").is_file());
    v
}

/// Publishes the packs of a site with `cmd` (one run per pack directory, appended to
/// it); the failures, `pack : error`.
fn publish_site(cmd: &[String], site_dir: &Path) -> Vec<String> {
    let Some((prog, args)) = cmd.split_first() else {
        return Vec::new();
    };
    let mut failures = Vec::new();
    for d in pack_dirs(site_dir) {
        match std::process::Command::new(prog).args(args).arg(&d).status() {
            Ok(s) if s.success() => {}
            Ok(s) => failures.push(format!("{} : {s}", d.display())),
            Err(e) => failures.push(format!("{} : {prog} : {e}", d.display())),
        }
    }
    failures
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
    /// `built`, `failed`, `up_to_date`, `not_started` (stop time reached), `set_aside`
    /// (its build killed the process, #172).
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
    /// Set aside: their build killed the process [`SET_ASIDE_AFTER`] times (#172).
    pub set_aside: usize,
    /// Sites whose packs were published as soon as built (`BatchOptions::publish`).
    pub published: usize,
    /// Publications that failed, `site : pack : error`.
    pub publish_failures: Vec<String>,
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

/// Whether the last build of a site is successful, with the same inputs, recent and
/// current ([`steps_current`]).
pub fn up_to_date(
    rec: &BuildRecord,
    inputs: &BuildInputs,
    site: &Site,
    max_age: Duration,
    now: Timestamp,
) -> bool {
    let recent = rec
        .finished_at
        .parse::<Timestamp>()
        .is_ok_and(|t| std::time::Duration::try_from(now - t).is_ok_and(|age| age < max_age));
    rec.ok && rec.inputs == *inputs && recent && steps_current(rec, site)
}

/// Whether the record holds every step of the pipeline (a build killed with the process
/// leaves its journal: the steps it reached, #174) and each step done was done with the
/// inputs of now ([`crate::build::step_inputs`]: the fields of `site` it reads and, for
/// the pack and the export, their code: a new code of a part of the pack makes every
/// pack again, #174).
pub fn steps_current(rec: &BuildRecord, site: &Site) -> bool {
    let rec = with_step_inputs(rec.clone());
    crate::build::missing_steps(&rec.outcome.steps).is_empty()
        && rec
            .outcome
            .steps
            .iter()
            .filter(|s| s.state == StepState::Ok)
            .all(|s| s.inputs.as_deref() == Some(&crate::build::step_inputs(&s.name, site)))
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
    let now = Timestamp::now();
    let mut results = Vec::new();
    let mut todo = VecDeque::new();
    let mut suspects = Vec::new();
    for s in sites {
        let inp = inputs(ctx, registry, s)?;
        match france::read_record(&ctx.layout, s.id) {
            Some(r)
                if !opts.force
                    && up_to_date(&r, &inp, s, opts.max_age, now)
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
            _ => {
                // A site whose build killed the process (#172): set aside after
                // SET_ASIDE_AFTER attempts, else built last and alone.
                let dir = ctx.layout.out_dir(s);
                let fp = inputs_fingerprint(&inp);
                let attempt = reckon_attempt(&dir)?;
                if attempt.set_aside(&fp, now) {
                    println!(
                        "{:<34} mis de côté : {} tentatives tuées avec le processus, la dernière le {}",
                        s.id,
                        attempt.killed,
                        attempt.last_killed_at.as_deref().unwrap_or("?")
                    );
                    results.push(SiteResult {
                        site: s.id.to_owned(),
                        result: "set_aside".into(),
                        seconds: 0.0,
                        pack_bytes: None,
                        export_bytes: None,
                        failures: vec![set_aside_line(&attempt)],
                        not_applicable: Vec::new(),
                    });
                } else if attempt.killed > 0 {
                    suspects.push((*s, inp, true, fp));
                } else {
                    todo.push_back((*s, inp, false, fp));
                }
            }
        }
    }
    todo.extend(suspects);
    let n = todo.len();
    println!(
        "build-all : {} sites, {} à construire, {} à jour, {} mis de côté ; {} en parallèle",
        sites.len(),
        n,
        results.iter().filter(|r| r.result == "up_to_date").count(),
        results.iter().filter(|r| r.result == "set_aside").count(),
        opts.jobs
    );
    let mut built: Vec<(String, BuildOutcome)> = Vec::new();
    let mut meteo = None;
    let mut published = 0;
    let mut publish_failures = Vec::new();
    if n > 0 {
        let todo_sites: Vec<&Site> = todo.iter().map(|(s, ..)| *s).collect();
        meteo = (!ctx.cfg.personal_sources).then(|| steps.meteo(ctx, &todo_sites));
        let shared = Shared {
            france: Some(&france),
            resume: None,
            journal: None,
        };
        // Sites started by memory (#173): never two sites whose estimated peaks add up
        // beyond the budget; a site above it alone.
        let (limit, limit_source) = memory_limit(ctx.cfg.build_memory_mib);
        let budget = memory_budget(limit);
        println!(
            "mémoire : limite {:.0} Mio ({limit_source}), {:.0} Mio planifiables",
            limit as f64 / 1048576.0,
            budget as f64 / 1048576.0
        );
        let waiting: Vec<_> = todo
            .into_iter()
            .map(|t| {
                let m = site_memory(t.0);
                if m > budget {
                    println!(
                        "{:<34} pic estimé {:.0} Mio au-dessus de la limite planifiable : construit seul",
                        t.0.id,
                        m as f64 / 1048576.0
                    );
                }
                (t, m)
            })
            .collect();
        let plan = Mutex::new(Plan {
            waiting,
            running: 0,
            running_count: 0,
            max_running: 0,
            budget,
        });
        let ended = std::sync::Condvar::new();
        let done = Mutex::new(Vec::new());
        let counter = Mutex::new(0usize);
        // A site whose build killed the process runs alone: if it kills it again, no
        // other site is blamed with it (#172).
        let solo = std::sync::RwLock::new(());
        let deadline = opts.stop_after.map(|d| t0 + d);
        std::thread::scope(|scope| {
            for _ in 0..opts.jobs.max(1) {
                scope.spawn(|| {
                    loop {
                        if deadline.is_some_and(|d| Instant::now() >= d) {
                            break;
                        }
                        let Some(((site, inp, suspect, fp), mem)) = plan_next(&plan, &ended) else {
                            break;
                        };
                        let (_solo_w, _solo_r) = if suspect {
                            (Some(solo.write().unwrap_or_else(|p| p.into_inner())), None)
                        } else {
                            (None, Some(solo.read().unwrap_or_else(|p| p.into_inner())))
                        };
                        let dir = ctx.layout.out_dir(site);
                        if let Err(e) = begin_attempt(&dir, &fp) {
                            tracing::warn!(site = site.id, error = %e, "attempt not noted");
                        }
                        let r = steps.site(ctx, site, inp, shared, opts);
                        if let Err(e) = end_attempt(&dir) {
                            tracing::warn!(site = site.id, error = %e, "attempt not cleared");
                        }
                        // Published at once (#172): a pass killed later keeps it.
                        let publish_failures = match (&r, &opts.publish) {
                            (Ok(_), Some(cmd)) => {
                                let f = publish_site(cmd, &dir);
                                for l in &f {
                                    println!("{:<34} publication en échec : {l}", site.id);
                                }
                                Some(f)
                            }
                            _ => None,
                        };
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
                        done.lock()
                            .expect("done")
                            .push((site.id.to_owned(), r, publish_failures));
                        drop((_solo_w, _solo_r));
                        {
                            let mut p = plan.lock().unwrap_or_else(|p| p.into_inner());
                            p.running -= mem;
                            p.running_count -= 1;
                        }
                        ended.notify_all();
                    }
                });
            }
        });
        let plan = plan.into_inner().unwrap_or_else(|p| p.into_inner());
        println!(
            "mémoire : somme maximale des pics estimés des sites construits ensemble {:.0} Mio sur {:.0}",
            plan.max_running as f64 / 1048576.0,
            plan.budget as f64 / 1048576.0
        );
        for (id, r, p) in done.into_inner().expect("done") {
            if let Some(p) = p {
                if p.is_empty() {
                    published += 1;
                }
                publish_failures.extend(p.into_iter().map(|l| format!("{id} : {l}")));
            }
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
        set_aside: count("set_aside"),
        published,
        publish_failures,
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
    let stamp = Timestamp::now().strftime("%Y%m%dT%H%M%SZ");
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
    /// previous outputs, if any, are kept and published with their date), `never_built`,
    /// `set_aside` (its build killed the process [`SET_ASIDE_AFTER`] times in a row: the
    /// other sites go first, it is tried again when its inputs change or once a day; its
    /// previous pack, if any, stays published, #172), `interrupted` (its last build was
    /// killed before it reached [`SiteStatus::missing_steps`]: the next catch-up builds
    /// it, #174).
    pub state: String,
    /// Steps of the pipeline the last build never reached (#174).
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub missing_steps: Vec<String>,
    /// Attempts of the site killed with the process in a row (#172).
    #[serde(skip_serializing_if = "is_zero")]
    pub killed_attempts: u32,
    /// Last run of the site, RFC 3339.
    pub built_at: Option<String>,
    /// Steps that are not done, with their cause.
    pub failed_steps: Vec<StepStatus>,
    /// Steps done that produced nothing, with the reason (B167).
    pub empty_steps: Vec<StepStatus>,
    /// Every step.
    pub steps: Vec<StepStatus>,
}

fn is_zero(n: &u32) -> bool {
    *n == 0
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
    let now = Timestamp::now();
    let mut alerts = Vec::new();
    let mut out = Vec::new();
    for s in sites {
        let attempt = Attempt::read(&layout.out_dir(s));
        let set_aside = attempt.killed >= SET_ASIDE_AFTER;
        if set_aside {
            alerts.push(format!("{} : {}", s.id, set_aside_line(&attempt)));
        }
        let Some(rec) = france::read_record(layout, s.id) else {
            out.push(SiteStatus {
                site: s.id.to_owned(),
                state: if set_aside {
                    "set_aside"
                } else {
                    "never_built"
                }
                .into(),
                killed_attempts: attempt.killed,
                missing_steps: Vec::new(),
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
                    .and_then(|t| t.parse::<Timestamp>().ok())
                    && ((now - t).as_hours() / 24) >= ALERT_AFTER_DAYS
                {
                    alerts.push(format!(
                        "{} : {} en échec depuis le {}",
                        s.id,
                        st.name,
                        t.strftime("%d/%m/%Y")
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
        let missing = crate::build::missing_steps(&rec.outcome.steps);
        out.push(SiteStatus {
            site: s.id.to_owned(),
            state: if set_aside {
                "set_aside"
            } else if !missing.is_empty() {
                "interrupted"
            } else if rec.ok {
                "ok"
            } else {
                "failed"
            }
            .into(),
            missing_steps: missing.iter().map(|m| (*m).to_owned()).collect(),
            killed_attempts: attempt.killed,
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
            "set_aside": count("set_aside"),
            "interrupted": count("interrupted"),
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
pub fn meteo_step_at(ctx: &Ctx, sites: &[&Site], run: Option<Timestamp>) -> StepRecord {
    meteo_refresh_at(ctx, sites, run).0
}

/// [`meteo_step_at`] with what was done for each site (`None` when the run itself could
/// not be read). A site whose weather cannot be made from the run fails alone (#174): the
/// step is failed, its line names those sites with the reason.
pub fn meteo_refresh_at(
    ctx: &Ctx,
    sites: &[&Site],
    run: Option<Timestamp>,
) -> (StepRecord, Option<skimap_meteo::arome::RefreshReport>) {
    let t = Instant::now();
    let run = match run.map_or_else(|| ctx.arome_run(), |r| Ok(Some(r))) {
        Ok(r) => r,
        Err(e) => {
            let rec = StepRecord {
                name: "meteo-arome".into(),
                state: StepState::Failed,
                seconds: 0.0,
                line: format!("{e:#}"),
                at: Some(now_rfc3339()),
                outputs: None,
                resumed: false,
                inputs: None,
            };
            return (rec, None);
        }
    };
    let refreshed = match run {
        Some(run) => skimap_meteo::arome::refresh_run(sites, &ctx.layout, &ctx.dl, run),
        None => skimap_meteo::arome::refresh(sites, &ctx.layout, &ctx.dl),
    };
    let (state, line, report) = match refreshed {
        Ok(r) => {
            let n = |k: &str| r.sites.iter().filter(|s| s.status == k).count();
            let failed: Vec<String> = r
                .sites
                .iter()
                .filter_map(|s| Some(format!("{} : {}", s.site, s.error.as_deref()?)))
                .collect();
            let mut line = format!(
                "run AROME {} : {} sites écrits, {} déjà à ce run",
                r.run,
                n("written"),
                n("unchanged")
            );
            if !failed.is_empty() {
                line.push_str(&format!(
                    ", {} sans météo : {}",
                    failed.len(),
                    failed.join(" ; ")
                ));
            }
            let state = if failed.is_empty() {
                StepState::Ok
            } else {
                StepState::Failed
            };
            (state, line, Some(r))
        }
        Err(e) => (StepState::Failed, format!("{e:#}"), None),
    };
    let seconds = t.elapsed().as_secs_f64();
    match state {
        StepState::Ok => println!("météo AROME : {line} ({seconds:.1} s)"),
        _ => println!("météo AROME : ÉCHEC ({seconds:.1} s) : {line}"),
    }
    let rec = StepRecord {
        name: "meteo-arome".into(),
        state,
        seconds,
        line,
        at: Some(now_rfc3339()),
        outputs: None,
        resumed: false,
        inputs: None,
    };
    (rec, report)
}

/// Records in `build-all.json` the weather of each site that `meteo-refresh` wrote (the
/// pack and the export to be redone, [`record_standalone`]) or could not make (the step
/// failed with its reason, [`record_standalone_failed`]: said in `status.json`, #174).
/// Sites at the run already: nothing.
pub fn record_meteo(
    layout: &DataLayout,
    sites: &[&Site],
    report: &skimap_meteo::arome::RefreshReport,
) -> Vec<String> {
    let mut not_recorded = Vec::new();
    for r in &report.sites {
        let Some(site) = sites.iter().find(|s| s.id == r.site) else {
            continue;
        };
        let done = match (r.status, r.error.as_deref()) {
            ("written", _) => record_standalone(
                layout,
                site,
                "meteo",
                &format!(
                    "AROME : run {}, {} points, {} heures",
                    report.run, r.points, r.hours
                ),
            ),
            (_, Some(e)) => {
                record_standalone_failed(layout, site, "meteo", &format!("AROME : {e}"))
            }
            _ => Ok(()),
        };
        if let Err(e) = done {
            not_recorded.push(format!("{} : {e:#}", r.site));
        }
    }
    not_recorded
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
        inputs: None,
    }
}

/// Steps whose outputs read those of a step run on its own (CronJobs `skimap-job snow`,
/// `bera`, `status`, `refresh`), as declared in [`Ctx::build_all_with`]: the pack and
/// the export take every layer.
fn dependents(step: &str) -> &'static [&'static str] {
    match step {
        "snow" => &["offpiste-prepare", "build-pack", "export"],
        "offpiste-prepare" | "bera" | "status-poll" | "meteo" => &["build-pack", "export"],
        // The voice says the names of the refuges (#129).
        "refuges" => &["voice", "voice-en", "build-pack", "export"],
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
/// `build-all`: the catch-up builds it whole). The record of a build killed before its
/// end (its journal) stays not done (#174).
pub fn record_standalone(layout: &DataLayout, site: &Site, step: &str, line: &str) -> Result<()> {
    record_alone(layout, site, step, Ok(line))
}

/// [`record_standalone`] of a step run on its own that failed (#174: the weather of a
/// site that cannot be made from the AROME run): the step is failed in `build-all.json`,
/// with `error`, and so in `status.json`; its previous outputs stay in use (the steps
/// reading them are not to be redone), and the next catch-up tries it again.
pub fn record_standalone_failed(
    layout: &DataLayout,
    site: &Site,
    step: &str,
    error: &str,
) -> Result<()> {
    record_alone(layout, site, step, Err(error))
}

fn record_alone(
    layout: &DataLayout,
    site: &Site,
    step: &str,
    outcome: std::result::Result<&str, &str>,
) -> Result<()> {
    let dir = layout.out_dir(site);
    let Some(mut rec) = france::read_record(layout, site.id) else {
        return Ok(());
    };
    let _lock = lock_site(&dir)?;
    let at = now_rfc3339();
    let Some(i) = rec.outcome.steps.iter().position(|s| s.name == step) else {
        return Ok(());
    };
    let inputs = Some(crate::build::step_inputs(step, site));
    match outcome {
        Ok(line) => {
            let outputs =
                crate::build::step_dir(step).and_then(|d| crate::build::fingerprint(&dir.join(d)));
            rec.outcome.steps[i] = StepRecord {
                name: step.to_owned(),
                state: StepState::Ok,
                seconds: 0.0,
                line: format!("{line} (fait seul, hors build-all)"),
                at: Some(at.clone()),
                outputs,
                resumed: false,
                inputs,
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
        }
        Err(error) => {
            rec.outcome.steps[i] = StepRecord {
                name: step.to_owned(),
                state: StepState::Failed,
                seconds: 0.0,
                line: format!("{error} (fait seul, hors build-all)"),
                at: Some(at),
                outputs: None,
                resumed: false,
                inputs,
            };
        }
    }
    rec.ok = rec.outcome.ok() && crate::build::missing_steps(&rec.outcome.steps).is_empty();
    skimap_common::output::write_json(&france::record_path(layout, site.id), &rec)?;
    Ok(())
}

/// `rec` with the fingerprint of the fields each step read ([`crate::build::step_inputs`])
/// where it lacks: a record written before the steps recorded it (#164) was built with
/// its site record whole, and before the code of a step was recorded (#174): the fields
/// only, so that a step whose code is versioned is run again.
fn with_step_inputs(mut rec: BuildRecord) -> BuildRecord {
    if rec.outcome.steps.iter().any(|s| s.inputs.is_none())
        && let Ok(site) = skimap_common::site::interned(&rec.inputs.record)
    {
        for s in rec.outcome.steps.iter_mut().filter(|s| s.inputs.is_none()) {
            s.inputs = Some(crate::build::step_inputs_of(&s.name, site, None));
        }
    }
    rec
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
    let previous = france::read_record(&ctx.layout, site.id)
        .filter(|r| !force && r.inputs.resumable_by(&inp))
        .map(with_step_inputs);
    let resume = previous.as_ref().map(|r| crate::build::Resume {
        previous: &r.outcome.steps,
        max_age,
        personal_sources: ctx.cfg.personal_sources,
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
        "état : {} sites, {} sans échec, {} en échec, {} interrompus, {} jamais construits, {} avec une étape vide ({})",
        n("sites"),
        n("ok"),
        n("failed"),
        n("interrupted"),
        n("never_built"),
        n("empty"),
        path.display()
    )];
    for site in v["by_site"].as_array().into_iter().flatten() {
        if let Some(m) = site["missing_steps"].as_array() {
            l.push(format!(
                "  interrompu {} : construction arrêtée avant {}",
                site["site"].as_str().unwrap_or("?"),
                m.iter()
                    .filter_map(|x| x.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            ));
        }
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
        "build-all : {} sites en {:.0} s ({} en parallèle) : {} construits sans échec, {} avec échec, {} à jour, {} non commencés, {} mis de côté (processus tué) ; {} publiés au fil des sites",
        s.sites,
        s.seconds,
        s.jobs,
        s.built,
        s.failed,
        s.up_to_date,
        s.not_started,
        s.set_aside,
        s.published
    )];
    for f in &s.publish_failures {
        l.push(format!("publication en échec : {f}"));
    }
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
    fn sites_are_started_by_memory() {
        // Budget 10: a running site of 6 leaves room for 4.
        assert_eq!(next_fitting(&[], 0, 0, 10), None);
        assert_eq!(next_fitting(&[7, 3], 0, 0, 10), Some(0));
        assert_eq!(next_fitting(&[7, 3], 6, 1, 10), Some(1));
        assert_eq!(next_fitting(&[7, 5], 6, 1, 10), None);
        assert_eq!(next_fitting(&[4], 6, 1, 10), Some(0));
        // A site above the budget waits for no site to run, then runs alone.
        assert_eq!(next_fitting(&[12, 1], 0, 0, 10), Some(0));
        assert_eq!(next_fitting(&[12], 1, 1, 10), None);
        assert_eq!(next_fitting(&[u64::MAX], 1, 1, 10), None);

        // Two workers over a queue: the sum of the running estimates never exceeds the
        // budget unless one site runs alone, and every site is built once.
        let sizes = [9u64, 2, 8, 3, 12, 1, 5, 5, 7, 2];
        let plan = Mutex::new(Plan {
            waiting: sizes.iter().enumerate().map(|(i, m)| (i, *m)).collect(),
            running: 0,
            running_count: 0,
            max_running: 0,
            budget: 10,
        });
        let ended = std::sync::Condvar::new();
        let seen = Mutex::new(Vec::new());
        let over = std::sync::atomic::AtomicBool::new(false);
        std::thread::scope(|scope| {
            for _ in 0..2 {
                scope.spawn(|| {
                    while let Some((i, m)) = plan_next(&plan, &ended) {
                        {
                            let p = plan.lock().unwrap();
                            if p.running > p.budget && p.running_count > 1 {
                                over.store(true, std::sync::atomic::Ordering::SeqCst);
                            }
                        }
                        std::thread::sleep(Duration::from_millis(2 * m));
                        seen.lock().unwrap().push(i);
                        let mut p = plan.lock().unwrap();
                        p.running -= m;
                        p.running_count -= 1;
                        drop(p);
                        ended.notify_all();
                    }
                });
            }
        });
        assert!(!over.load(std::sync::atomic::Ordering::SeqCst));
        let mut seen = seen.into_inner().unwrap();
        seen.sort_unstable();
        assert_eq!(seen, (0..sizes.len()).collect::<Vec<_>>());
        let p = plan.into_inner().unwrap();
        assert_eq!((p.running, p.running_count), (0, 0));
        // The largest site ran alone; nothing else went above the budget.
        assert_eq!(p.max_running, 12);

        assert_eq!(parse_cgroup_memory_max("4294967296\n"), Some(4 << 30));
        assert_eq!(parse_cgroup_memory_max("max\n"), None);
        assert_eq!(
            memory_limit(Some(2048)),
            (2 << 30, "SKIMAP_BUILD_MEMORY_MIB")
        );
        assert_eq!(memory_budget(4 << 30), 3 << 30);
        // The 5 m grid of the box, as the terrain step crops it (3317 × 3480 cells for
        // this evaluation site).
        let site = skimap_common::site::site("cervieres").expect("evaluation site");
        let cells = grid_cells(site).unwrap() as f64;
        assert!((cells / (3317.0 * 3480.0) - 1.0).abs() < 0.01, "{cells}");
        assert_eq!(site_memory(site), (cells * SITE_BYTES_PER_CELL) as u64);
    }

    /// Simulated pass over the sites of the registry of `SKIMAP_DATA_DIR`, in the order of
    /// `build-all --all`, 2 jobs, 4 GiB: the former rule (sites of 300 km² or more one at a
    /// time) against the memory plan. Site durations: a + b × cells fitted on the
    /// `build-all.json` of the sites built in the data directory.
    /// `cargo test -p skimap-cli --bin skimap simulated_pass -- --ignored --nocapture`
    #[test]
    #[ignore = "reads the registry of SKIMAP_DATA_DIR"]
    fn simulated_pass() {
        let cfg = skimap_common::config::Config::from_env().unwrap();
        let layout = cfg.layout();
        let registry = Registry::load(&layout).unwrap();
        let sites = registry.buildable();
        let cells: Vec<f64> = sites
            .iter()
            .map(|s| grid_cells(s).unwrap() as f64)
            .collect();
        let pts: Vec<(f64, f64)> = sites
            .iter()
            .zip(&cells)
            .filter_map(|(s, c)| Some((*c, france::read_record(&layout, s.id)?.outcome.seconds)))
            .collect();
        let n = pts.len() as f64;
        let (mx, my) = (
            pts.iter().map(|p| p.0).sum::<f64>() / n,
            pts.iter().map(|p| p.1).sum::<f64>() / n,
        );
        let b = pts.iter().map(|p| (p.0 - mx) * (p.1 - my)).sum::<f64>()
            / pts.iter().map(|p| (p.0 - mx).powi(2)).sum::<f64>();
        let a = my - b * mx;
        let secs: Vec<f64> = cells.iter().map(|c| (a + b * c).max(1.0)).collect();
        let mem: Vec<u64> = sites.iter().map(|s| site_memory(s)).collect();
        let budget = memory_budget(DEFAULT_MEMORY_LIMIT);
        let area = |s: &Site| {
            let b = s.bbox;
            let lat = ((b.north + b.south) / 2.0).to_radians();
            (b.north - b.south) * 111.2 * (b.east - b.west) * 111.2 * lat.cos()
        };
        let heavy: Vec<bool> = sites.iter().map(|s| area(s) >= 300.0).collect();
        // Event simulation: 2 workers; `pick` gives the next site among the waiting ones
        // and the running ones, or None (wait for the next end).
        type Pick<'a> = &'a dyn Fn(&[usize], &[usize]) -> Option<usize>;
        let simulate = |pick: Pick<'_>| {
            let mut waiting: Vec<usize> = (0..sites.len()).collect();
            let mut running: Vec<(f64, usize)> = Vec::new();
            let (mut now, mut max_sum, mut max_peak_sum) = (0.0f64, 0u64, 0u64);
            while !waiting.is_empty() || !running.is_empty() {
                while running.len() < 2 && !waiting.is_empty() {
                    let ids: Vec<usize> = running.iter().map(|r| r.1).collect();
                    let Some(k) = pick(&waiting, &ids) else { break };
                    let i = waiting.remove(k);
                    running.push((now + secs[i], i));
                    let sum: u64 = running.iter().map(|r| mem[r.1]).sum();
                    max_sum = max_sum.max(sum);
                    if running.len() > 1 {
                        max_peak_sum = max_peak_sum.max(sum);
                    }
                }
                running.sort_by(|x, y| x.0.total_cmp(&y.0));
                let (t, _) = running.remove(0);
                now = t;
            }
            (now, max_sum, max_peak_sum)
        };
        // Former rule: the head of the queue, held while another big site runs.
        let old = simulate(&|w, r| (!(heavy[w[0]] && r.iter().any(|i| heavy[*i]))).then_some(0));
        let new = simulate(&|w, r| {
            let est: Vec<u64> = w.iter().map(|i| mem[*i]).collect();
            next_fitting(&est, r.iter().map(|i| mem[*i]).sum(), r.len(), budget)
        });
        let gib = |b: u64| b as f64 / (1u64 << 30) as f64;
        println!(
            "{} sites, durée = {a:.1} s + {:.2} s par million de cellules ({} sites mesurés)",
            sites.len(),
            b * 1e6,
            pts.len()
        );
        println!(
            "au-dessus de {:.2} Gio planifiables : {}",
            gib(budget),
            mem.iter().filter(|m| **m > budget).count()
        );
        for (name, (t, max_sum, pair)) in [("ancienne règle (300 km²)", old), ("plan mémoire", new)]
        {
            println!(
                "{name:<26} {:.0} s ; somme estimée maximale {:.2} Gio (sites ensemble : {:.2} Gio)",
                t,
                gib(max_sum),
                gib(pair)
            );
        }
        println!("écart de durée : {:+.1} %", (new.0 / old.0 - 1.0) * 100.0);
        assert!(new.2 <= budget);
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
    /// record was written with the same site record (every step can then be taken over).
    struct Fake {
        new_start: std::sync::atomic::AtomicBool,
        /// Per site build: site, starts of the site built, previous record taken over.
        builds: Mutex<Vec<(String, usize, bool)>>,
        /// The build of this site kills the process (`abort`, as the OOM killer does).
        abort_on: Option<String>,
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
                inputs: None,
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
                inputs: None,
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
                abort_on: None,
            }
        }

        /// A site build: its record.
        fn build(&self, ctx: &Ctx, site: &'static Site, inp: BuildInputs) -> Result<BuildOutcome> {
            let dir = ctx.layout.out_dir(site);
            skimap_common::output::append_bytes(
                &ctx.layout.root().join("built.txt"),
                format!("{}\n", site.id).as_bytes(),
            )?;
            if self.abort_on.as_deref() == Some(site.id) {
                std::process::abort();
            }
            skimap_common::output::write_bytes(&dir.join("pack/manifest.json"), b"{}")?;
            let taken_over = france::read_record(&ctx.layout, site.id)
                .is_some_and(|r| r.inputs.resumable_by(&inp) && r.inputs.record == inp.record);
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
                inputs: Some(crate::build::step_inputs(name, site)),
            };
            // Every step of the pipeline, as a whole run records them.
            let outcome = BuildOutcome {
                site: site.id.to_owned(),
                steps: crate::build::READS.iter().map(|(n, _)| step(n)).collect(),
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
            publish: None,
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

    fn catch_up_opts() -> BatchOptions {
        BatchOptions {
            jobs: 1,
            force: false,
            max_age: Duration::from_secs(7 * 86_400),
            stop_after: None,
            verify: true,
            publish: None,
        }
    }

    /// #174, catch-up of 18/11/2026: a build killed with the process (OOM) leaves its
    /// journal, the steps done so far with `ok: false`; the snow CronJob then recorded its
    /// step in it with `ok` recomputed from those steps only, `true`, and every catch-up
    /// after found the site up to date with half its steps: its pack was never rebuilt
    /// (new AROME runs, new OSM basemap code).
    #[test]
    fn an_interrupted_build_is_never_up_to_date() {
        let (_t, ctx) = one_site_registry();
        let registry = Registry::load(&ctx.layout).unwrap();
        let site = registry.buildable()[0];
        let steps = Fake::new();
        let opts = catch_up_opts();
        build_many_with(&ctx, &registry, &[site], &opts, &steps).unwrap();
        // The next build is killed after `zones`: its journal stays.
        let path = france::record_path(&ctx.layout, site.id);
        let mut rec = france::read_record(&ctx.layout, site.id).unwrap();
        let k = rec
            .outcome
            .steps
            .iter()
            .position(|s| s.name == "zones")
            .unwrap();
        rec.outcome.steps.truncate(k + 1);
        rec.ok = false;
        skimap_common::output::write_json(&path, &rec).unwrap();
        // The snow CronJob.
        record_standalone(&ctx.layout, site, "snow", "simulation du jour").unwrap();
        let r = france::read_record(&ctx.layout, site.id).unwrap();
        assert!(!r.ok, "a journal of an interrupted build recorded as done");
        let registry = Registry::load(&ctx.layout).unwrap();
        let s = build_many_with(&ctx, &registry, &[site], &opts, &steps).unwrap();
        assert_eq!(s.up_to_date, 0, "interrupted build taken for up to date");
        assert_eq!(steps.builds.lock().unwrap().len(), 2);
    }

    /// #174: the code of a part of the pack changed (OSM basemap from zoom 12 only, #160):
    /// every pack is made again, though neither the site nor any layer changed. The
    /// records in production on 18/11/2026 hold, for each step, the fingerprint of the
    /// fields of the site it reads only.
    #[test]
    fn a_new_code_of_the_pack_makes_every_pack_again() {
        use sha2::{Digest, Sha256};
        let (_t, ctx) = one_site_registry();
        let registry = Registry::load(&ctx.layout).unwrap();
        let site = registry.buildable()[0];
        let steps = Fake::new();
        let opts = catch_up_opts();
        build_many_with(&ctx, &registry, &[site], &opts, &steps).unwrap();
        let fields_only = |step: &str| {
            let v = crate::build::view(site, crate::build::reads(step));
            hex::encode(Sha256::digest(serde_json::to_vec(&v).unwrap()))
        };
        let path = france::record_path(&ctx.layout, site.id);
        let mut rec = france::read_record(&ctx.layout, site.id).unwrap();
        for s in &mut rec.outcome.steps {
            s.inputs = Some(fields_only(&s.name));
        }
        skimap_common::output::write_json(&path, &rec).unwrap();
        let s = build_many_with(&ctx, &registry, &[site], &opts, &steps).unwrap();
        assert_eq!(
            s.up_to_date, 0,
            "pack made by another code taken for up to date"
        );
        assert_eq!(steps.builds.lock().unwrap().len(), 2);
        // The steps without a versioned code keep their fingerprint: nothing else is run
        // again for this.
        for (n, _) in crate::build::READS {
            if crate::build::step_code(n).is_none() {
                assert_eq!(crate::build::step_inputs(n, site), fields_only(n), "{n}");
            }
        }
        // Built again with the code of now: up to date.
        let s = build_many_with(&ctx, &registry, &[site], &opts, &steps).unwrap();
        assert_eq!(s.up_to_date, 1);
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
            inputs: None,
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
        let record = skimap_common::site::evaluation_records()[0].clone();
        let site = skimap_common::site::interned(&record).unwrap();
        let step = |name: &str| StepRecord {
            name: name.into(),
            state: StepState::Ok,
            seconds: 0.0,
            line: String::new(),
            at: Some("2026-11-15T10:00:00Z".into()),
            outputs: None,
            resumed: false,
            inputs: Some(crate::build::step_inputs(name, site)),
        };
        let rec = BuildRecord {
            inputs: BuildInputs {
                record,
                version: "0.1.0".into(),
                personal_sources: false,
                revision: france::BUILD_REVISION,
            },
            started_at: "2026-11-15T10:00:00Z".into(),
            finished_at: "2026-11-15T10:01:00Z".into(),
            ok: true,
            outcome: BuildOutcome {
                site: site.id.into(),
                steps: crate::build::READS.iter().map(|(n, _)| step(n)).collect(),
                seconds: 60.0,
                pack_bytes: Some(1),
                export_bytes: None,
            },
        };
        let now = "2026-11-15T12:00:00Z".parse::<Timestamp>().unwrap();
        let day = Duration::from_secs(86_400);
        assert!(up_to_date(&rec, &rec.inputs, site, day, now));
        assert!(!up_to_date(
            &rec,
            &rec.inputs,
            site,
            Duration::from_secs(3600),
            now
        ));
        let mut other = rec.inputs.clone();
        other.personal_sources = true;
        assert!(!up_to_date(&rec, &other, site, day, now));
        let mut other = rec.inputs.clone();
        other.record.bbox.north += 0.01;
        assert!(!up_to_date(&rec, &other, site, day, now));
        let mut failed = rec.clone();
        failed.ok = false;
        assert!(!up_to_date(&failed, &rec.inputs, site, day, now));
        // The journal of a build killed before its end, `ok` or not (#174).
        let mut interrupted = rec.clone();
        interrupted.outcome.steps.truncate(8);
        assert!(!up_to_date(&interrupted, &rec.inputs, site, day, now));
        // The pack made by another code (#174).
        let mut other_code = rec.clone();
        for s in &mut other_code.outcome.steps {
            if s.name == "build-pack" {
                s.inputs = Some(crate::build::step_inputs_of(&s.name, site, Some("ancien")));
            }
        }
        assert!(!up_to_date(&other_code, &rec.inputs, site, day, now));
        // A record written before the code of the pack was recorded: the fields only.
        let mut legacy = rec.clone();
        for s in &mut legacy.outcome.steps {
            s.inputs = None;
        }
        assert!(!up_to_date(&legacy, &rec.inputs, site, day, now));
        for s in &mut legacy.outcome.steps {
            if crate::build::step_code(&s.name).is_some() {
                s.inputs = Some(crate::build::step_inputs(&s.name, site));
            }
        }
        assert!(
            up_to_date(&legacy, &rec.inputs, site, day, now),
            "the other steps taken with the fields they read"
        );
        // A new revision of the steps: not up to date, but its steps can be taken over.
        let mut older = rec.inputs.clone();
        older.revision = 1;
        assert!(!up_to_date(
            &BuildRecord {
                inputs: older.clone(),
                ..rec.clone()
            },
            &rec.inputs,
            site,
            day,
            now
        ));
        assert!(older.resumable_by(&rec.inputs));
        // Another record of the site: each step decides on the fields it reads (#164).
        assert!(other.resumable_by(&rec.inputs));
        let mut another_site = rec.inputs.clone();
        another_site.record.id = "autre".into();
        assert!(!another_site.resumable_by(&rec.inputs), "another site");
        let mut public = rec.inputs.clone();
        public.personal_sources = !public.personal_sources;
        assert!(!public.resumable_by(&rec.inputs), "personal_sources");
    }

    /// #174: the weather of a site that cannot be made, recorded by `meteo-refresh`, is a
    /// failed step of the site in `status.json` with its reason; an interrupted build is
    /// said too.
    #[test]
    fn status_says_why_a_site_has_no_weather_and_an_interrupted_build() {
        let (_t, ctx) = one_site_registry();
        let registry = Registry::load(&ctx.layout).unwrap();
        let site = registry.buildable()[0];
        let steps = Fake::new();
        build_many_with(&ctx, &registry, &[site], &catch_up_opts(), &steps).unwrap();
        let report = skimap_meteo::arome::RefreshReport {
            run: "2026-11-18T09:00:00Z".into(),
            sites: vec![skimap_meteo::arome::SiteRefresh {
                site: site.id.into(),
                status: "failed",
                points: 0,
                hours: 0,
                error: Some("no value at x (1, 2): outside the AROME grid".into()),
            }],
        };
        assert!(record_meteo(&ctx.layout, &[site], &report).is_empty());
        let r = france::read_record(&ctx.layout, site.id).unwrap();
        assert!(!r.ok);
        let path = write_status(&ctx.layout, &[site]).unwrap();
        let v: serde_json::Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        let st = &v["by_site"][0];
        assert_eq!(st["state"], "failed");
        assert_eq!(st["failed_steps"][0]["step"], "meteo");
        assert!(
            st["failed_steps"][0]["error"]
                .as_str()
                .unwrap()
                .contains("outside the AROME grid"),
            "{st}"
        );
        // The failed step is tried again by the next catch-up; the pack keeps the weather
        // it has (not marked to redo).
        let state = |n: &str| r.outcome.steps.iter().find(|s| s.name == n).unwrap().state;
        assert_eq!(state("build-pack"), StepState::Ok);
        // The weather written at the next run: the pack and the export to redo.
        let mut report = report;
        report.sites[0].status = "written";
        report.sites[0].error = None;
        assert!(record_meteo(&ctx.layout, &[site], &report).is_empty());
        let r = france::read_record(&ctx.layout, site.id).unwrap();
        let state = |n: &str| r.outcome.steps.iter().find(|s| s.name == n).unwrap().state;
        assert_eq!(state("meteo"), StepState::Ok);
        assert_eq!(state("build-pack"), StepState::Blocked);
        // A build killed after `zones`.
        let mut rec = r.clone();
        rec.outcome.steps.truncate(8);
        rec.ok = true;
        skimap_common::output::write_json(&france::record_path(&ctx.layout, site.id), &rec)
            .unwrap();
        let path = write_status(&ctx.layout, &[site]).unwrap();
        let v: serde_json::Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        assert_eq!(v["by_site"][0]["state"], "interrupted");
        assert_eq!(v["interrupted"], 1);
        assert_eq!(v["by_site"][0]["missing_steps"][0], "avalanche-exposure");
        let lines = describe_status(&path).unwrap();
        assert!(lines.iter().any(|l| l.contains("interrompu")), "{lines:?}");
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
            inputs: None,
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

    /// Data directory of [`a_site_that_kills_the_process_does_not_block_the_others`]
    /// and site whose build kills the process; the child pass runs only when both are set.
    const CRASH_DIR: &str = "SKIMAP_TEST_CRASH_DIR";
    const CRASH_SITE: &str = "SKIMAP_TEST_CRASH_SITE";

    /// One pass of the catch-up in a child process (run by
    /// [`a_site_that_kills_the_process_does_not_block_the_others`]): every site of the
    /// registry, the build of `CRASH_SITE` aborts the process, each site built is
    /// published by a command that appends its pack directory to `published.txt`.
    #[test]
    fn crash_pass_child() {
        let (Ok(dir), Ok(poison)) = (std::env::var(CRASH_DIR), std::env::var(CRASH_SITE)) else {
            return;
        };
        let cfg =
            skimap_common::config::Config::from_vars([("SKIMAP_DATA_DIR".to_owned(), dir.clone())])
                .unwrap();
        let ctx = Ctx::new(&cfg).unwrap();
        let registry = Registry::load(&ctx.layout).unwrap();
        let opts = BatchOptions {
            jobs: 1,
            force: false,
            max_age: Duration::from_secs(86_400),
            stop_after: None,
            verify: false,
            publish: Some(vec![
                "sh".into(),
                "-c".into(),
                format!("printf '%s\\n' \"$0\" >> {dir}/published.txt"),
            ]),
        };
        let steps = Fake {
            abort_on: Some(poison),
            ..Fake::new()
        };
        build_many_with(&ctx, &registry, &registry.buildable(), &opts, &steps).unwrap();
    }

    /// #172, catch-up of 17/11/2026 on the server: the process was killed (OOMKilled) on one
    /// big site; nothing was published (the publication came at the end of the pass) and
    /// every following pass died on the same site. Each pass here is a child process
    /// whose build of the first site aborts.
    #[test]
    fn a_site_that_kills_the_process_does_not_block_the_others() {
        let t = tempfile::tempdir().unwrap();
        let cfg = skimap_common::config::Config::from_vars([(
            "SKIMAP_DATA_DIR".to_owned(),
            t.path().display().to_string(),
        )])
        .unwrap();
        let ctx = Ctx::new(&cfg).unwrap();
        // Four sites: the evaluation sites, and copies of the first under other names.
        let mut records: Vec<_> = skimap_common::site::evaluation_records()
            .iter()
            .filter(|r| r.buildable())
            .cloned()
            .collect();
        for k in 1..=4 - records.len().min(4) {
            let mut r = records[0].clone();
            r.id = format!("{}-copie-{k}", r.id);
            records.push(r);
        }
        assert_eq!(records.len(), 4);
        Registry::write(
            &Registry::path(&ctx.layout).unwrap(),
            &skimap_common::site::RegistryFile {
                generated_at: now_rfc3339(),
                notes: Vec::new(),
                sites: records.clone(),
            },
        )
        .unwrap();
        let poison = records[0].id.clone();
        let others: Vec<String> = records[1..].iter().map(|r| r.id.clone()).collect();
        let pass = || {
            std::process::Command::new(std::env::current_exe().unwrap())
                .args(["batch::tests::crash_pass_child", "--exact", "--nocapture"])
                .env(CRASH_DIR, t.path())
                .env(CRASH_SITE, &poison)
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .status()
                .unwrap()
                .success()
        };
        let read = |f: &str| std::fs::read_to_string(t.path().join(f)).unwrap_or_default();
        let published = |id: &str| {
            read("published.txt")
                .lines()
                .filter(|l| l.ends_with(&format!("/{id}/pack")))
                .count()
        };
        let builds = |id: &str| read("built.txt").lines().filter(|l| *l == id).count();

        assert!(!pass(), "the first pass is killed on the first site");
        assert!(!pass(), "the second pass is killed again, last");
        for id in &others {
            assert_eq!(
                published(id),
                1,
                "{id} built and published before the site that kills the process: {}",
                read("published.txt")
            );
        }
        assert_eq!(builds(&poison), 2);
        // Two attempts killed: set aside, the pass ends, the others are up to date.
        assert!(pass(), "the third pass leaves the killing site aside");
        assert_eq!(builds(&poison), 2, "not tried a third time the same day");
        let status: serde_json::Value =
            serde_json::from_str(&read("out/france/status.json")).unwrap();
        let site = status["by_site"]
            .as_array()
            .unwrap()
            .iter()
            .find(|s| s["site"] == poison.as_str())
            .unwrap();
        assert_eq!(site["state"], "set_aside", "{site}");
        assert_eq!(site["killed_attempts"], 2, "{site}");
        assert!(
            status["alerts"]
                .as_array()
                .unwrap()
                .iter()
                .any(|a| a.as_str().unwrap().starts_with(&format!("{poison} : "))),
            "{status}"
        );
        // A day later it is tried again, after the others (here up to date).
        let path = ctx
            .layout
            .out_dir(Registry::load(&ctx.layout).unwrap().get(&poison).unwrap())
            .join(ATTEMPT_FILE);
        let mut a: Attempt = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        a.last_killed_at = Some(
            (Timestamp::now() - jiff::SignedDuration::from_hours(25))
                .strftime("%Y-%m-%dT%H:%M:%SZ")
                .to_string(),
        );
        skimap_common::output::write_json(&path, &a).unwrap();
        assert!(!pass(), "tried again a day later");
        assert_eq!(builds(&poison), 3);
        for id in &others {
            assert_eq!(builds(id), 1, "{id} up to date, not rebuilt");
        }
    }
}
