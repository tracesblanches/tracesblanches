//! Ready-made requests on archived past days of the two evaluation sites, shared by the
//! example, the real-data tests and the scenario bench (Q9).

use chrono::{NaiveDate, NaiveTime};
use skimap_assist::inputs::SiteData;
use skimap_common::geom::LonLat;
use skimap_common::layout::DataLayout;
use skimap_common::site::Site;
use skimap_core::graph::Difficulty;
use skimap_core::routing::Style;

use crate::error::Result;
use crate::plan::{Context, PlanRequest};

/// Day archived in `assist/history/` used for the plans of a site.
pub fn past_day(site: &Site) -> NaiveDate {
    match site.id {
        "cervieres" => NaiveDate::from_ymd_opt(2026, 1, 9).expect("date"),
        _ => NaiveDate::from_ymd_opt(2026, 2, 22).expect("date"),
    }
}

/// Requests of a site on a day: Cervières, 3 h classic from Le Laus; Chapelle-des-Bois,
/// 2 h skating back before 16 h from the start of the pistes; the start time is the best
/// time of A9 from 8 h.
pub fn requests(site: &Site, date: NaiveDate) -> Vec<PlanRequest> {
    let t = |h, m| NaiveTime::from_hms_opt(h, m, 0).expect("time");
    match site.id {
        "cervieres" => vec![
            PlanRequest {
                label: "3 h, classique".into(),
                start: LonLat::new(6.7269, 44.8552),
                start_name: "Le Laus".into(),
                date,
                start_time: None,
                earliest: t(8, 0),
                budget_min: 180.0,
                style: Style::Classic,
                return_before: None,
                max_difficulty: None,
                max_results: 3,
            },
            PlanRequest {
                label: "1 h 30, classique, vertes et bleues, départ 14 h 30".into(),
                start: LonLat::new(6.8161, 44.8448),
                start_name: "Les Fonts".into(),
                date,
                start_time: Some(t(14, 30)),
                earliest: t(8, 0),
                budget_min: 90.0,
                style: Style::Classic,
                return_before: None,
                max_difficulty: Some(Difficulty::Easy),
                max_results: 3,
            },
        ],
        _ => vec![
            PlanRequest {
                label: "2 h, skating, retour avant 16 h".into(),
                start: LonLat::new(6.1125, 46.6032),
                start_name: "le départ des pistes de Chapelle-des-Bois".into(),
                date,
                start_time: None,
                earliest: t(8, 0),
                budget_min: 120.0,
                style: Style::Skating,
                return_before: Some(t(16, 0)),
                max_difficulty: None,
                max_results: 3,
            },
            PlanRequest {
                label: "3 h, classique".into(),
                start: LonLat::new(6.1125, 46.6032),
                start_name: "le départ des pistes de Chapelle-des-Bois".into(),
                date,
                start_time: Some(t(10, 0)),
                earliest: t(8, 0),
                budget_min: 180.0,
                style: Style::Classic,
                return_before: None,
                max_difficulty: None,
                max_results: 3,
            },
        ],
    }
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
