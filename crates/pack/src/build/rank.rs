//! The starts of a pack ranked by the planner (#166): the start proposed first by the
//! phone is the one from which the planner finds the best loops of the app's default
//! outing.
//!
//! The pipeline finds the starts of every domain from its OSM points and its network (X9,
//! `skimap_stations::departs`), main first, with an estimate of the network each serves.
//! The estimate is not the planner: a start on a large part of the network can still
//! give no loop of 2 h (trails that do not close, one-way stretches, a stem too long), a
//! start on a smaller part can give three. So, once the pack is written, the morning of
//! the phone (`skimap_plan::plan::morning_in`, the same code, on the same files: the
//! pack is read through [`crate::view::PackView`] as on the phone) is run from each start
//! of the site record, for the app's default outing (`skimap_plan::outing::Outing::DEFAULT`,
//! the one the morning and the evening preparation ask, #170) at [`START_TIME`], the
//! first day of the pack's forecast, and the starts of the record are reordered:
//!
//! 1. most real loops of the time asked (#150: a start from which the morning proposes
//!    three out-and-backs goes after one from which it proposes one real loop, and one
//!    from which it proposes only real loops shorter than the time, when no real loop
//!    fits it, goes after one with a real loop of the time);
//! 2. most real loops, the shorter ones included;
//! 3. most loops proposed (out-and-backs included: the morning proposes them only when no
//!    real loop fits, so between starts without real loop, the one with an out-and-back
//!    goes before the one with none);
//! 4. best loop score (`skimap_assist::suggest`: fit to the time, status, snow, weather);
//! 5. the registry order.
//!
//! What was found for each start is written beside the record
//! ([`crate::manifest::StartRanking`]); the registry keeps its starts and its order.

use std::cmp::Ordering;
use std::path::Path;
use std::time::Instant;

use jiff::civil::{Date, Time};
use rayon::prelude::*;
use skimap_assist::inputs::{Paths, SiteData};
use skimap_common::Lang;
use skimap_common::site::{Site, SiteRecord};
use skimap_core::routing::Style;
use skimap_plan::outing::{self, Outing};
use skimap_plan::plan::morning_in;

use crate::manifest::{StartRanking, StartScore};
use crate::reader::Pack;
use crate::view::PackView;

/// The outing the starts are ranked for: the app's default (#170).
pub const OUTING: Outing = Outing::DEFAULT;
/// Start time asked, local `HH:MM`: a fixed one, so that the light and the best time of
/// the day do not decide between starts.
pub const START_TIME: &str = "10:00";

fn hm(s: &str) -> Time {
    Time::strptime("%H:%M", s).expect("constant time")
}

/// Ranks the starts of `record` on the pack in `pack_dir` (module documentation), the
/// pack's view made under `work` (removed after); `None` when the record has no start.
pub fn rank(
    site: &Site,
    record: &mut SiteRecord,
    pack_dir: &Path,
    work: &Path,
    today: Date,
) -> Option<StartRanking> {
    if record.starts.is_empty() {
        return None;
    }
    let t0 = Instant::now();
    let pack = Pack::open(pack_dir);
    let date = pack
        .as_ref()
        .ok()
        .and_then(|p| p.forecast_days().first().cloned())
        .and_then(|d| Date::strptime("%Y-%m-%d", &d).ok())
        .unwrap_or(today);
    let data = pack
        .map_err(|e| e.to_string())
        .and_then(|p| PackView::build(&p, work).map_err(|e| e.to_string()))
        .and_then(|v| SiteData::load(&v.layout, site, &Paths::default()).map_err(|e| e.to_string()))
        .and_then(|d| {
            if d.graph.is_some() {
                Ok(d)
            } else {
                Err("pas de graphe des pistes dans le pack".to_owned())
            }
        });
    let mut ranking = StartRanking {
        budget_min: OUTING.budget_min,
        style: match OUTING.style {
            Style::Classic => "classic",
            Style::Skating => "skating",
        }
        .to_owned(),
        date: date.to_string(),
        start_time: START_TIME.to_owned(),
        starts: Vec::new(),
        note: None,
        compute_ms: 0,
    };
    match data {
        Ok(data) => {
            let mut scored: Vec<(StartScore, skimap_common::site::Start)> = record
                .starts
                .par_iter()
                .enumerate()
                .map(|(i, s)| (score(&data, s, i, date), s.clone()))
                .collect();
            scored.sort_by(|(a, _), (b, _)| order(a, b));
            let (scores, starts): (Vec<_>, Vec<_>) = scored.into_iter().unzip();
            record.starts = starts;
            ranking.starts = scores;
        }
        Err(e) => {
            ranking.note = Some(format!("ordre du registre gardé : {e}"));
        }
    }
    let _ = std::fs::remove_dir_all(work);
    ranking.compute_ms = u64::try_from(t0.elapsed().as_millis()).unwrap_or(u64::MAX);
    Some(ranking)
}

/// Order of two starts, best first (module documentation): most real loops of the time
/// asked, most real loops, most loops, best loop score, registry order.
pub fn order(a: &StartScore, b: &StartScore) -> Ordering {
    let best = |s: &StartScore| s.best_score.unwrap_or(f64::NEG_INFINITY);
    let of_the_time = |s: &StartScore| s.real_loops - s.shorter_loops.min(s.real_loops);
    of_the_time(b)
        .cmp(&of_the_time(a))
        .then(b.real_loops.cmp(&a.real_loops))
        .then(b.loops.cmp(&a.loops))
        .then(best(b).total_cmp(&best(a)))
        .then(a.registry_rank.cmp(&b.registry_rank))
}

/// The morning of the default request from `start`.
fn score(
    data: &SiteData,
    start: &skimap_common::site::Start,
    registry_rank: usize,
    date: Date,
) -> StartScore {
    let req = OUTING.request(
        start.position(),
        start.phrase.clone(),
        date,
        Some(hm(START_TIME)),
        outing::EARLIEST,
        Lang::Fr,
    );
    let m = morning_in(data, &req, None, Lang::Fr);
    let loops = &m.suggestions.suggestions;
    StartScore {
        id: start.id.clone(),
        registry_rank,
        loops: loops.len(),
        real_loops: loops.iter().filter(|l| l.real_loop).count(),
        shorter_loops: loops.iter().filter(|l| l.shorter).count(),
        best_score: loops.iter().map(|l| l.score).reduce(f64::max),
        lengths_km: loops.iter().map(|l| round2(l.length_km)).collect(),
        network_km: loops
            .iter()
            .map(|l| round2(l.length_km * (1.0 - l.out_and_back_share)))
            .collect(),
        note: loops.is_empty().then(|| m.notes.last().cloned()).flatten(),
    }
}

fn round2(x: f64) -> f64 {
    (x * 100.0).round() / 100.0
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(id: &str, rank: usize, loops: usize, real: usize, best: Option<f64>) -> StartScore {
        StartScore {
            id: id.into(),
            registry_rank: rank,
            loops,
            real_loops: real,
            shorter_loops: 0,
            best_score: best,
            lengths_km: Vec::new(),
            network_km: Vec::new(),
            note: None,
        }
    }

    fn ranked(mut v: Vec<StartScore>) -> Vec<String> {
        v.sort_by(order);
        v.into_iter().map(|x| x.id).collect()
    }

    #[test]
    fn the_start_with_loops_goes_before_the_registry_main_one() {
        // The registry's main start gives no loop, the second two: the second first.
        assert_eq!(
            ranked(vec![s("a", 0, 0, 0, None), s("b", 1, 2, 2, Some(-2.0))]),
            ["b", "a"]
        );
    }

    #[test]
    fn more_real_loops_then_loops_then_score_then_registry() {
        assert_eq!(
            ranked(vec![
                s("f", 5, 1, 0, Some(-1.0)),
                s("a", 0, 1, 1, Some(-1.0)),
                s("b", 1, 3, 0, Some(-3.0)),
                s("c", 2, 3, 2, Some(-2.5)),
                s("d", 3, 3, 2, Some(-1.5)),
                s("e", 4, 3, 2, Some(-1.5)),
            ]),
            ["d", "e", "c", "a", "b", "f"]
        );
    }

    #[test]
    fn a_real_loop_goes_before_out_and_backs() {
        // The registry's main start gives three out-and-backs, the second one real loop
        // (#150): the second first.
        assert_eq!(
            ranked(vec![
                s("a", 0, 3, 0, Some(-1.7)),
                s("b", 1, 1, 1, Some(-1.35))
            ]),
            ["b", "a"]
        );
    }

    #[test]
    fn a_real_loop_of_the_time_goes_before_shorter_ones() {
        // The registry's main start gives two real loops shorter than the time asked and
        // an out-and-back, the third one real loop of the time (#150): the third first.
        let mut a = s("a", 0, 3, 2, Some(-1.52));
        a.shorter_loops = 2;
        assert_eq!(ranked(vec![a, s("c", 2, 1, 1, Some(-1.71))]), ["c", "a"]);
    }

    #[test]
    fn equal_starts_keep_the_registry_order() {
        assert_eq!(
            ranked(vec![
                s("c", 2, 0, 0, None),
                s("a", 0, 0, 0, None),
                s("b", 1, 0, 0, None),
            ]),
            ["a", "b", "c"]
        );
    }
}
