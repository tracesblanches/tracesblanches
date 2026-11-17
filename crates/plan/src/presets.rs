//! Ready-made requests on archived past days of a site (site table of `skimap-common`:
//! the two evaluation sites), shared by the example, the real-data tests and the scenario
//! bench (Q9).

use chrono::{NaiveDate, NaiveTime};
use skimap_assist::inputs::SiteData;
use skimap_common::layout::DataLayout;
use skimap_common::site::{PlanPreset, PresetStyle, Site};
use skimap_core::graph::Difficulty;
use skimap_core::routing::Style;

use crate::error::Result;
use crate::plan::{Context, PlanRequest};

/// Day archived in `assist/history/` used for the plans of a site: the `archived_day` of
/// the site table (`skimap_common::site::local`), or today (UTC) for a site without one.
pub fn past_day(site: &Site) -> NaiveDate {
    archived_day(site).unwrap_or_else(|| chrono::Utc::now().date_naive())
}

/// Archived day of a site in the site table, if any.
pub fn archived_day(site: &Site) -> Option<NaiveDate> {
    skimap_common::site::local(site.id).and_then(|l| l.archived_day)
}

/// Requests of a site on a day, from the site table (`local.plan_requests`: start, time
/// budget, technique, fixed start time, return time, green and blue only); a site without
/// requests in the table gets two shapes (2 h skating back before 16 h, 3 h classic) from
/// its main start
/// (`skimap_common::site::starts`). Without a fixed time, the start time is the best time
/// of A9 from 8 h.
pub fn requests(site: &Site, date: NaiveDate) -> Vec<PlanRequest> {
    let t = |h, m| NaiveTime::from_hms_opt(h, m, 0).expect("time");
    let hhmm = |s: &Option<String>| {
        s.as_deref()
            .map(|x| skimap_common::site::parse_hhmm(x).expect("site table time"))
    };
    let starts = skimap_common::site::starts(site);
    let generic = [
        PlanPreset {
            label: "2 h, skating, retour avant 16 h".into(),
            start: 0,
            start_time: None,
            budget_min: 120.0,
            style: PresetStyle::Skating,
            return_before: Some("16:00".into()),
            easy_only: false,
        },
        PlanPreset {
            label: "3 h, classique".into(),
            start: 0,
            start_time: Some("10:00".into()),
            budget_min: 180.0,
            style: PresetStyle::Classic,
            return_before: None,
            easy_only: false,
        },
    ];
    let presets = match skimap_common::site::local(site.id) {
        Some(l) if !l.plan_requests.is_empty() => &l.plan_requests[..],
        _ => &generic[..],
    };
    presets
        .iter()
        .map(|p| {
            let s = &starts[p.start.min(starts.len() - 1)];
            PlanRequest {
                label: p.label.clone(),
                start: s.position(),
                start_name: s.phrase.clone(),
                date,
                start_time: hhmm(&p.start_time),
                earliest: t(8, 0),
                budget_min: p.budget_min,
                style: match p.style {
                    PresetStyle::Classic => Style::Classic,
                    PresetStyle::Skating => Style::Skating,
                },
                return_before: hhmm(&p.return_before),
                max_difficulty: p.easy_only.then_some(Difficulty::Easy),
                max_results: 3,
            }
        })
        .collect()
}

/// A short loop for a family (C11): the start of the site's first request, 75 min of model
/// time from 9 h, classic, green and blue pistes, no return time.
pub fn family_request(site: &Site, date: NaiveDate) -> PlanRequest {
    let base = requests(site, date).remove(0);
    PlanRequest {
        label: "famille, 1 h 15 au modèle, classique, vertes et bleues, départ 9 h".into(),
        start_time: Some(NaiveTime::from_hms_opt(9, 0, 0).expect("time")),
        budget_min: 75.0,
        style: Style::Classic,
        return_before: None,
        max_difficulty: Some(Difficulty::Easy),
        ..base
    }
}

/// Layers of a site with the archived past days (`skimap_assist::presets::history_paths`)
/// and the plan's context (refuges, clips).
pub fn load(layout: &DataLayout, site: &Site) -> Result<(SiteData, Context)> {
    let paths = skimap_assist::presets::history_paths(layout, site, false)?;
    let data = SiteData::load(layout, site, &paths)?;
    let ctx = Context::load(layout, site)?;
    Ok((data, ctx))
}
