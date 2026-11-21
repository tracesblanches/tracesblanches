//! `skimap plan`: day plans of a site (J1, J2) for the requests of `skimap_plan::presets`,
//! `data/out/<site>/plan/`. Not in the pack: the plan is computed in the morning (on the
//! phone eventually, E2).

use std::path::PathBuf;
use std::time::Instant;

use anyhow::{Result, bail};
use jiff::civil::Date;
use skimap_common::layout::DataLayout;
use skimap_common::site::Site;
use skimap_plan::plan::{self, DayPlan};
use skimap_plan::presets;

/// Options of `skimap plan`.
#[derive(Debug, Clone, Default)]
pub struct PlanArgs {
    /// Day of the plans (default: the archived day of the site, `presets::past_day`).
    pub date: Option<Date>,
    /// Only this request of the site, numbered from 1 (default: all).
    pub request: Option<usize>,
}

/// What a run produced.
#[derive(Debug, Default)]
pub struct PlanRun {
    /// One plan per request with at least one loop.
    pub plans: Vec<DayPlan>,
    /// Files written.
    pub files: Vec<PathBuf>,
    /// Lines to print: per request, timings and counts, then the plan text.
    pub lines: Vec<String>,
}

/// Builds and writes the first loop's plan of each request of the site on the day.
pub fn plan(layout: &DataLayout, site: &Site, a: &PlanArgs) -> Result<PlanRun> {
    let date = a.date.unwrap_or_else(|| presets::past_day(site));
    let requests = presets::requests(site, date);
    let chosen: Vec<_> = match a.request {
        None => requests.into_iter().collect(),
        Some(n) if (1..=requests.len()).contains(&n) => {
            requests.into_iter().skip(n - 1).take(1).collect()
        }
        Some(n) => bail!(
            "--request {n} : {} demandes pour {} (1 à {})",
            requests.len(),
            site.id,
            requests.len()
        ),
    };
    let t0 = Instant::now();
    let (mut data, ctx) = presets::load(layout, site)?;
    let load = t0.elapsed().as_secs_f64();
    let mut run = PlanRun::default();
    for req in chosen {
        let t1 = Instant::now();
        let m = plan::morning(&data, &req);
        let morning = t1.elapsed().as_secs_f64();
        if m.suggestions.suggestions.is_empty() {
            run.lines.push(format!(
                "{} {} {} : aucune boucle ({})",
                site.id,
                date,
                req.label,
                m.notes.join(" ; ")
            ));
            continue;
        }
        let t2 = Instant::now();
        let p = plan::build(&mut data, &m, 0, &ctx)?;
        let build = t2.elapsed().as_secs_f64();
        let files = plan::write(layout, site, &p, &data)?;
        run.lines.push(format!(
            "{} {} {} : lecture des couches {load:.2} s, matin (A1, A2, A9, A5, A6) {morning:.3} s, plan {build:.3} s ; {} plans B, {} annonces prévues, {} clips dont {} absents du pack",
            site.id,
            date,
            req.label,
            p.plans_b.len(),
            p.announcements.len(),
            p.clips.required.len(),
            p.clips.missing.len(),
        ));
        run.lines.push(p.text_fr());
        run.lines
            .extend(files.iter().map(|f| format!("écrit {}", f.display())));
        run.files.extend(files);
        run.plans.push(p);
    }
    Ok(run)
}

#[cfg(test)]
mod tests {
    use clap::Parser;

    use crate::{Cli, Command, run};

    #[test]
    fn plan_subcommand_parses() {
        let cli = Cli::try_parse_from([
            "skimap",
            "plan",
            "cervieres",
            "--date",
            "2026-01-09",
            "--request",
            "2",
        ])
        .unwrap();
        assert!(matches!(
            cli.command,
            Command::Plan { ref site, date: Some(ref d), request: Some(2) }
                if site == "cervieres" && d == "2026-01-09"
        ));
        Cli::try_parse_from(["skimap", "plan", "chapelle-des-bois"]).unwrap();
        assert!(Cli::try_parse_from(["skimap", "plan", "cervieres", "--request", "x"]).is_err());
    }

    #[test]
    fn plan_checks_the_request_before_reading_layers() {
        let dir = tempfile::tempdir().unwrap();
        let args = |r: &str| {
            Cli::try_parse_from([
                "skimap",
                "--data-dir",
                dir.path().to_str().unwrap(),
                "plan",
                "cervieres",
                "--request",
                r,
            ])
            .unwrap()
        };
        let e = format!("{:#}", run(args("3")).unwrap_err());
        assert!(e.contains("--request 3 : 2 demandes pour cervieres"), "{e}");
        let e = format!("{:#}", run(args("0")).unwrap_err());
        assert!(e.contains("--request 0"), "{e}");
        // A valid request without data: the missing layer is said.
        assert!(run(args("1")).is_err());
    }
}
