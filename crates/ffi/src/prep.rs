//! Preparation mode and multi-day ski mode (E2 #126), battery forecast (C9 #41, K8 #58).
//!
//! Preparation (phone on charge and on Wi-Fi, a WorkManager job with constraints): for
//! each of the next days that the pack's hourly forecast covers (J+1 to J+3 when present),
//! the morning of the area (loops with the group's time, C11) and the day plan of the best
//! loop with its plans B, written ready to follow (`plan.json`, `plan.trk`, `pack.txt`:
//! the location service opens them as a validated day). A day without forecast in the pack
//! is listed as such, never planned on another day's weather. The clips the announcements
//! of a prepared plan need and the pack lacks are listed (said by the phone's voice).
//!
//! Ski mode (several days, possibly without network): nothing heavy runs; the prepared
//! plans are read back ([`prep_read`]) with the date of their preparation and the date of
//! the last update of every information of the pack (« mis à jour le 15/11 à 10 h 05 »),
//! never « périmé »: the skier judges.
//!
//! Battery: [`battery_forecast`] wraps `skimap_core::battery` for the screens: « batterie
//! suffisante jusqu'à 16 h 40 », the emergency reserve kept, a warning when the planned
//! outing goes beyond.

use std::path::{Path, PathBuf};
use std::time::Instant;

use chrono::{DateTime, NaiveDate, NaiveDateTime, Utc};
use serde::{Deserialize, Serialize};
use skimap_assist::inputs::{Paths, SiteData};
use skimap_core::battery::{self, Device, Measured, Usage};
use skimap_core::calibration::ProfileModel;
use skimap_pack::Pack;
use skimap_plan::pace;
use skimap_plan::plan::{self, Context};

use crate::card::Lang;
use crate::text;
use crate::tracking::{TrackingError, epoch_ms};
use crate::tracking_day::{PACK_FILE, PLAN_FILE, PackView, TRACK_FILE};

/// Summary of a prepared day, next to its plan.
pub const PREPARED_FILE: &str = "prepared.json";

fn invalid(detail: impl std::fmt::Display) -> TrackingError {
    TrackingError::Invalid {
        detail: detail.to_string(),
    }
}

fn local(ms: i64) -> NaiveDateTime {
    skimap_core::ephemeris::local_time(
        DateTime::<Utc>::from_timestamp_millis(ms).unwrap_or_default(),
    )
}

/// « mis à jour le 15/11 à 10 h 05 » (local time) of an epoch millisecond, in `lang`.
#[uniffi::export]
pub fn updated_text(ms: i64, lang: Lang) -> String {
    text::of(lang).updated(local(ms))
}

/// A loop of a prepared day.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, uniffi::Record)]
pub struct PreparedLoop {
    /// Label.
    pub label: String,
    /// Length, km.
    pub length_km: f64,
    /// « environ 2 h 10 (1 h 50 à 2 h 40) au rythme du groupe » or « … au modèle ».
    pub time_text: String,
}

/// A prepared day.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, uniffi::Record)]
pub struct PreparedDay {
    /// Site.
    pub site_id: String,
    /// `YYYY-MM-DD`.
    pub date: String,
    /// « dimanche 16/11 ».
    pub day_text: String,
    /// Start of the loops.
    pub start_name: String,
    /// Briefing lines (A1) of that day's forecast.
    pub briefing: Vec<String>,
    /// Best time to leave (A9).
    pub best_time: Option<String>,
    /// Wax (A2).
    pub wax: Option<String>,
    /// Loops, best first.
    pub loops: Vec<PreparedLoop>,
    /// Directory of the plan of the best loop, ready to follow.
    pub plan_dir: Option<String>,
    /// Expected return of that plan, « 12 h 40 ».
    pub expected_back: Option<String>,
    /// Plans B of that plan.
    pub plans_b: Vec<String>,
    /// Clips its announcements need that the pack lacks (said by the phone's voice).
    pub missing_clips: Vec<String>,
    /// Group the times are for.
    pub group: Option<String>,
    /// Prepared at, epoch ms.
    pub prepared_ms: i64,
    /// « mis à jour le 15/11 à 14 h 05 ».
    pub prepared_text: String,
    /// Never « sans danger ».
    pub never_safe: String,
}

/// What a preparation did.
#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct PrepReport {
    /// Days prepared.
    pub days: Vec<PreparedDay>,
    /// Days not prepared and why (« mardi 18/11 : aucune prévision dans le pack »).
    pub skipped: Vec<String>,
    /// Time of the whole preparation, milliseconds.
    pub compute_ms: u32,
}

/// Dates the pack's hourly forecast covers (at least 8 hours of the day).
pub fn forecast_days(pack: &Pack) -> Vec<NaiveDate> {
    let Ok(v) = pack.read_json("meteo-hourly") else {
        return Vec::new();
    };
    let mut count: std::collections::BTreeMap<NaiveDate, usize> = Default::default();
    for p in v["points"].as_array().into_iter().flatten().take(1) {
        for h in p["hours"].as_array().into_iter().flatten() {
            if let Some(d) = h["time"]
                .as_str()
                .and_then(|s| s.get(..10))
                .and_then(|s| NaiveDate::parse_from_str(s, "%Y-%m-%d").ok())
            {
                *count.entry(d).or_default() += 1;
            }
        }
    }
    count
        .into_iter()
        .filter(|(_, n)| *n >= 8)
        .map(|(d, _)| d)
        .collect()
}

fn time_text(s: &skimap_assist::suggest::Suggestion, t: &dyn text::Texts) -> String {
    match &s.group {
        Some(g) => t.time_group(g.median_min * 60.0, g.lo_min * 60.0, g.hi_min * 60.0),
        None => t.time_model(s.eta_min * 60.0),
    }
}

fn copy_plan(from: &Path, to: &Path) -> Result<(), TrackingError> {
    std::fs::create_dir_all(to).map_err(invalid)?;
    for f in [PLAN_FILE, TRACK_FILE, PACK_FILE] {
        std::fs::copy(from.join(f), to.join(f)).map_err(|e| invalid(format!("{f}: {e}")))?;
    }
    Ok(())
}

/// Prepares the next `days` days after `today` (`YYYY-MM-DD`) on the pack in `pack_dir`:
/// mornings and plans written under `<out_dir>/<site>/<date>/`; `work_dir` receives the
/// pipeline view of the pack; `group_json`: the group (C11) the times are for; texts in
/// `lang`.
#[uniffi::export]
pub fn prep_run(
    pack_dir: String,
    work_dir: String,
    out_dir: String,
    group_json: Option<String>,
    today: String,
    days: u32,
    lang: Lang,
) -> Result<PrepReport, TrackingError> {
    let t0 = Instant::now();
    let tx = text::of(lang);
    let today = NaiveDate::parse_from_str(&today, "%Y-%m-%d").map_err(invalid)?;
    let pack = Pack::open(PathBuf::from(&pack_dir)).map_err(invalid)?;
    let site = skimap_common::site::site(&pack.manifest().site.id).map_err(|_| {
        TrackingError::Unsupported {
            site: pack.manifest().site.id.clone(),
        }
    })?;
    let covered = forecast_days(&pack);
    let view = PackView::build(&pack, Path::new(&work_dir))?;
    let mut data = SiteData::load(&view.layout, site, &Paths::default()).map_err(invalid)?;
    let ctx = Context::load(&view.layout, site)?;
    let model: Option<ProfileModel> = group_json
        .as_deref()
        .map(serde_json::from_str)
        .transpose()
        .map_err(invalid)?;
    let mut out = Vec::new();
    let mut skipped = Vec::new();
    for k in 1..=days {
        let date = today + chrono::Duration::days(i64::from(k));
        if !covered.contains(&date) {
            skipped.push(tx.skipped_no_forecast(&tx.day(date)));
            continue;
        }
        let Some(mut req) = skimap_plan::presets::requests(site, date)
            .into_iter()
            .next()
        else {
            skipped.push(tx.skipped_no_start(&tx.day(date)));
            continue;
        };
        req.date = date;
        let m = plan::morning_in(&data, &req, model.as_ref(), lang);
        let dir = Path::new(&out_dir).join(site.id).join(date.to_string());
        std::fs::create_dir_all(&dir).map_err(invalid)?;
        let mut day = PreparedDay {
            site_id: site.id.to_owned(),
            date: date.to_string(),
            day_text: tx.day(date),
            start_name: req.start_name.clone(),
            briefing: m.briefing.text.clone(),
            best_time: m.best_time.as_ref().map(|b| b.text.clone()),
            wax: m.wax.as_ref().map(|w| w.text.clone()),
            loops: m
                .suggestions
                .suggestions
                .iter()
                .map(|s| PreparedLoop {
                    label: s.label.clone(),
                    length_km: s.length_km,
                    time_text: time_text(s, tx),
                })
                .collect(),
            plan_dir: None,
            expected_back: None,
            plans_b: Vec::new(),
            missing_clips: Vec::new(),
            group: model
                .as_ref()
                .map(|md| tx.assist().group_label(&md.profile)),
            prepared_ms: Utc::now().timestamp_millis(),
            prepared_text: String::new(),
            never_safe: skimap_plan::text::of(lang).never_safe().into(),
        };
        day.prepared_text = tx.updated(local(day.prepared_ms));
        if !m.suggestions.suggestions.is_empty() {
            let mut p = plan::build(&mut data, &m, 0, &ctx)?;
            let g = data.graph.as_ref().ok_or_else(|| invalid("graphe"))?;
            if let (Some(md), None) = (&model, &m.group) {
                pace::with_pace(&mut p, g, md)?;
            }
            let track = plan::track(&p, g)?;
            let pdir = dir.join("plan");
            std::fs::create_dir_all(&pdir).map_err(invalid)?;
            std::fs::write(pdir.join(PLAN_FILE), p.to_json()?).map_err(invalid)?;
            std::fs::write(pdir.join(TRACK_FILE), track.to_bytes()).map_err(invalid)?;
            std::fs::write(pdir.join(PACK_FILE), &pack_dir).map_err(invalid)?;
            let back = p
                .pace
                .as_ref()
                .map_or(p.turnaround.arrival_with_margin, |x| x.planned);
            day.plan_dir = Some(pdir.display().to_string());
            day.expected_back = Some(tx.clock5(back));
            day.plans_b = p
                .plans_b
                .iter()
                .filter(|b| b.fallback.exists())
                .map(|b| b.fallback.text.clone())
                .collect();
            day.missing_clips = p.clips.missing.iter().cloned().collect();
            let _ = epoch_ms(back);
        }
        std::fs::write(
            dir.join(PREPARED_FILE),
            serde_json::to_string_pretty(&day).map_err(invalid)?,
        )
        .map_err(invalid)?;
        out.push(day);
    }
    Ok(PrepReport {
        days: out,
        skipped,
        compute_ms: u32::try_from(t0.elapsed().as_millis()).unwrap_or(u32::MAX),
    })
}

/// The age of one information of a pack.
#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct InfoAge {
    /// « Bulletin d'avalanche (BERA) ».
    pub label: String,
    /// Site.
    pub site_id: String,
    /// « mis à jour le 15/11 à 10 h 05 », or « date inconnue ».
    pub updated_text: String,
    /// Epoch ms, 0 when unknown.
    pub updated_ms: i64,
}

/// What ski mode shows: the prepared days and the age of every information.
#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct PrepState {
    /// Prepared days from `today` on, by date.
    pub days: Vec<PreparedDay>,
    /// Age of the informations of the packs.
    pub infos: Vec<InfoAge>,
}

/// Informations of a pack whose age ski mode shows, by layer id.
const INFOS: [&str; 9] = [
    "bera",
    "meteo-hourly",
    "status",
    "snow-depth",
    "offpiste-snow",
    "avalanche-exposure",
    "zones",
    "refuges",
    "pistes",
];

fn rfc3339_ms(s: &str) -> Option<i64> {
    DateTime::parse_from_rfc3339(s)
        .ok()
        .map(|d| d.timestamp_millis())
}

/// Ages of the informations of the packs in `pack_dirs`, labels in `lang`.
pub fn info_ages(pack_dirs: &[String], lang: Lang) -> Vec<InfoAge> {
    let tx = text::of(lang);
    let updated =
        |ms: Option<i64>| ms.map_or_else(|| tx.date_unknown().to_owned(), |m| tx.updated(local(m)));
    let mut out = Vec::new();
    for d in pack_dirs {
        let Ok(pack) = Pack::open(PathBuf::from(d)) else {
            continue;
        };
        let site = pack.manifest().site.id.clone();
        for id in INFOS {
            let Ok(l) = pack.layer(id) else { continue };
            let ms = l.generated_at.as_deref().and_then(rfc3339_ms);
            out.push(InfoAge {
                label: tx.info_label(id).unwrap_or(id).to_owned(),
                site_id: site.clone(),
                updated_text: updated(ms),
                updated_ms: ms.unwrap_or(0),
            });
        }
        let ms = rfc3339_ms(&pack.manifest().generated_at);
        out.push(InfoAge {
            label: tx.info_label("pack").unwrap_or("pack").to_owned(),
            site_id: site,
            updated_text: updated(ms),
            updated_ms: ms.unwrap_or(0),
        });
    }
    out
}

/// Ski mode: the prepared days under `out_dir` from `today` on, and the age of every
/// information of the packs in `pack_dirs`. Reads files only.
#[uniffi::export]
pub fn prep_read(out_dir: String, pack_dirs: Vec<String>, today: String, lang: Lang) -> PrepState {
    let mut days: Vec<PreparedDay> = Vec::new();
    if let Ok(sites) = std::fs::read_dir(&out_dir) {
        for s in sites.filter_map(Result::ok) {
            let Ok(dates) = std::fs::read_dir(s.path()) else {
                continue;
            };
            for d in dates.filter_map(Result::ok) {
                let Ok(text) = std::fs::read_to_string(d.path().join(PREPARED_FILE)) else {
                    continue;
                };
                if let Ok(p) = serde_json::from_str::<PreparedDay>(&text)
                    && p.date >= today
                {
                    days.push(p);
                }
            }
        }
    }
    days.sort_by(|a, b| a.date.cmp(&b.date).then(a.site_id.cmp(&b.site_id)));
    PrepState {
        days,
        infos: info_ages(&pack_dirs, lang),
    }
}

/// Copies a prepared plan into `plan_dir` (a new outing directory), ready to follow.
#[uniffi::export]
pub fn prep_take(prepared_plan_dir: String, plan_dir: String) -> Result<(), TrackingError> {
    copy_plan(Path::new(&prepared_plan_dir), Path::new(&plan_dir))
}

/// A battery forecast for the screens.
#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct BatteryView {
    /// Drain, percentage points per hour.
    pub pct_per_h: f64,
    /// Emergency reserve, percentage points.
    pub reserve_pct: f64,
    /// Hours until the reserve.
    pub hours_to_reserve: f64,
    /// Battery enough until, epoch ms.
    pub enough_until_ms: i64,
    /// « Batterie suffisante jusqu'à 16 h 40, 15 % gardés pour un appel d'urgence ».
    pub text: String,
    /// The planned outing fits.
    pub enough: bool,
    /// « Batterie insuffisante pour la sortie prévue : … » when it does not.
    pub warning: Option<String>,
}

/// Battery forecast: `level_pct` now, battery `temp_c`, `capacity_mah` (0: Pixel 8), the
/// outing planned to end at `planned_end_ms` (0: none), screen minutes per hour expected,
/// share of the outing without network, airplane mode, and the drain measured since the
/// start of the outing (`measured_h` hours, `measured_pct` points; 0: none).
#[uniffi::export]
#[allow(clippy::too_many_arguments)]
pub fn battery_forecast(
    now_ms: i64,
    level_pct: f64,
    temp_c: f64,
    capacity_mah: f64,
    planned_end_ms: i64,
    screen_min_per_h: f64,
    no_network_share: f64,
    airplane: bool,
    measured_h: f64,
    measured_pct: f64,
    lang: Lang,
) -> BatteryView {
    let tx = text::of(lang);
    let d = Device::with_capacity(capacity_mah);
    let u = Usage {
        screen_min_per_h,
        no_network_share,
        airplane,
        ..Usage::default()
    };
    let planned_h = if planned_end_ms > now_ms {
        (planned_end_ms - now_ms) as f64 / 3_600_000.0
    } else {
        0.0
    };
    let measured = (measured_h > 0.0).then_some(Measured {
        hours: measured_h,
        pct_lost: measured_pct,
    });
    let f = battery::forecast(&d, &u, level_pct, temp_c, planned_h, measured);
    let until = now_ms + (f.hours_to_reserve.min(72.0) * 3_600_000.0) as i64;
    let clock = |ms: i64| tx.clock5(local(ms));
    let cold = (temp_c < 0.0).then_some(temp_c);
    let text = if f.hours_to_reserve >= 24.0 {
        tx.battery_day(f.reserve_pct, cold)
    } else {
        tx.battery_until(&clock(until), f.reserve_pct, cold)
    };
    let warning = (planned_h > 0.0 && !f.enough)
        .then(|| tx.battery_short(&clock(planned_end_ms), &clock(until)));
    BatteryView {
        pct_per_h: f.pct_per_h,
        reserve_pct: f.reserve_pct,
        hours_to_reserve: f.hours_to_reserve,
        enough_until_ms: until,
        text,
        enough: f.enough || planned_h == 0.0,
        warning,
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

    #[test]
    fn battery_texts() {
        let now = 1_771_146_000_000; // 09:00 UTC = 10:00 Paris
        let ok = battery_forecast(
            now,
            80.0,
            15.0,
            0.0,
            now + 5 * 3_600_000,
            3.0,
            0.0,
            false,
            0.0,
            0.0,
            Lang::Fr,
        );
        assert!(ok.enough && ok.warning.is_none());
        assert!(ok.text.starts_with("Batterie suffisante"), "{}", ok.text);
        assert!(ok.text.contains("appel d'urgence"));
        let low = battery_forecast(
            now,
            18.0,
            -12.0,
            0.0,
            now + 6 * 3_600_000,
            20.0,
            0.5,
            false,
            1.0,
            9.0,
            Lang::Fr,
        );
        eprintln!("{} / {:?}", low.text, low.warning);
        assert!(!low.enough);
        assert!(
            low.warning
                .as_deref()
                .is_some_and(|w| w.contains("insuffisante"))
        );
        assert!(low.text.contains("au chaud"));
    }

    /// Cervières pack: J+1 and J+2 are in its forecast, J+3 is not; each prepared day has
    /// a plan the location service opens.
    #[test]
    #[ignore = "needs data/out/cervieres/pack"]
    fn cervieres_three_days() {
        let pack = data().join("out/cervieres/pack");
        let work = tempfile::tempdir().unwrap();
        let out = tempfile::tempdir().unwrap();
        let p = Pack::open(pack.clone()).unwrap();
        let days = forecast_days(&p);
        eprintln!("forecast days {days:?}");
        let today = days[0].to_string();
        let fam = crate::tracking_day::group_default(crate::card::Lang::Fr).unwrap();
        let r = prep_run(
            pack.display().to_string(),
            work.path().display().to_string(),
            out.path().display().to_string(),
            Some(fam.model_json),
            today.clone(),
            3,
            Lang::Fr,
        )
        .unwrap();
        eprintln!("{} ms, skipped {:?}", r.compute_ms, r.skipped);
        for d in &r.days {
            eprintln!(
                "{} {:?} {:?} back {:?} missing clips {}",
                d.day_text,
                d.loops.first(),
                d.best_time,
                d.expected_back,
                d.missing_clips.len()
            );
        }
        assert_eq!(r.days.len() + r.skipped.len(), 3);
        assert_eq!(r.days.len(), days.len() - 1);
        assert!(r.skipped.iter().all(|s| s.contains("aucune prévision")));
        let d = &r.days[0];
        assert!(d.loops[0].time_text.contains("au rythme du groupe"));
        let plans = tempfile::tempdir().unwrap();
        let taken = plans.path().join("p1");
        prep_take(d.plan_dir.clone().unwrap(), taken.display().to_string()).unwrap();
        let v =
            crate::tracking_day::read_day_plan(taken.display().to_string(), crate::card::Lang::Fr)
                .unwrap();
        assert!(v.pace_text.is_some());
        let s =
            crate::tracking::open_tracking(taken.display().to_string(), None, "fr".into()).unwrap();
        assert_eq!(s.replayed(), 0);
        let st = prep_read(
            out.path().display().to_string(),
            vec![pack.display().to_string()],
            today,
            Lang::Fr,
        );
        assert_eq!(st.days.len(), r.days.len());
        assert!(st.infos.iter().any(
            |i| i.label.starts_with("Bulletin") && i.updated_text.starts_with("mis à jour le")
        ));
        assert!(st.infos.iter().all(|i| !i.updated_text.contains("périmé")));
    }
}
