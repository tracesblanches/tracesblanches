//! `build-all --all` / `--domains a,b,c`: the `build-all` of many sites, in parallel with a
//! bound, resumable, with a log per site and a summary.
//!
//! - One downloader for every site: its per-source lock (one request in flight per
//!   source) and its rate limits are global, not per site.
//! - The planet extract and the census are read once.
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

use crate::build::{BuildOutcome, Ctx, Shared, StepState};
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

/// Builds `sites`.
pub fn build_many(
    ctx: &Ctx,
    registry: &Registry,
    sites: &[&'static Site],
    opts: &BatchOptions,
) -> Result<BatchSummary> {
    let t0 = Instant::now();
    let started_at = now_rfc3339();
    let cache_before = cache_inventory(&ctx.layout.cache_root());
    let now = Utc::now();
    let mut results = Vec::new();
    let mut todo = VecDeque::new();
    for s in sites {
        let inp = inputs(ctx, registry, s)?;
        match france::read_record(&ctx.layout, s.id) {
            Some(r) if !opts.force && up_to_date(&r, &inp, opts.max_age, now) => {
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
    if n > 0 {
        let t = Instant::now();
        let planet = skimap_osm::extract::load_planet(&ctx.dl)?;
        let census = ctx.census(&planet)?;
        println!(
            "extrait OSM du {} et recensement ({} domaines) lus en {:.1} s",
            planet.valid_at.as_deref().unwrap_or("?"),
            census.domains.len(),
            t.elapsed().as_secs_f64()
        );
        let shared = Shared {
            planet: Some(&planet),
            census: Some(&census),
        };
        let queue = Mutex::new(todo);
        let done = Mutex::new(Vec::new());
        let counter = Mutex::new(0usize);
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
                        let r = build_one(ctx, site, inp, shared);
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
        results,
    };
    let dir = ctx.layout.france_dir().join("build-all");
    DataLayout::ensure(&dir)?;
    let stamp = Utc::now().format("%Y%m%dT%H%M%SZ");
    skimap_common::output::write_json(&dir.join(format!("summary_{stamp}.json")), &summary)?;
    skimap_common::output::write_json(&dir.join("summary.json"), &summary)?;
    Ok(summary)
}

fn build_one(
    ctx: &Ctx,
    site: &'static Site,
    inp: BuildInputs,
    shared: Shared<'_>,
) -> Result<BuildOutcome> {
    let dir = ctx.layout.out_dir(site);
    DataLayout::ensure(&dir)?;
    let log_path: PathBuf = dir.join(france::BUILD_LOG);
    let mut log = std::io::BufWriter::new(
        std::fs::File::create(&log_path).with_context(|| format!("{}", log_path.display()))?,
    );
    let started_at = now_rfc3339();
    let o = ctx.build_all_with(site, &shared, &mut log);
    let rec = BuildRecord {
        inputs: inp,
        started_at,
        finished_at: now_rfc3339(),
        ok: o.ok(),
        outcome: o.clone(),
    };
    skimap_common::output::write_json(&france::record_path(&ctx.layout, site.id), &rec)?;
    Ok(o)
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

    #[test]
    fn causes_ignore_numbers() {
        assert_eq!(
            cause("terrain", "tile 0998-6426: HTTP 503\nmore"),
            "terrain : tile #-#: HTTP #"
        );
        assert_eq!(cause("snow", "x"), "snow : x");
    }

    #[test]
    fn freshness() {
        let rec = BuildRecord {
            inputs: BuildInputs {
                record: skimap_common::site::evaluation_records()[0].clone(),
                version: "0.1.0".into(),
                personal_sources: false,
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
