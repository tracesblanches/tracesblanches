//! The `assist` layer: [`build`] runs the briefing (A1), snow quality (A3), wax (A2) and
//! synthesis (A7) of a site for a date and the following days, and writes
//! `data/out/<site>/assist/`:
//!
//! | File | Content |
//! |---|---|
//! | `briefing_<date>.json` / `.txt` | ranked domains and itineraries per day, every reason (A1) |
//! | `snow_quality_<date>.json` / `.txt` | hourly surface per slope class and per forecast point, best slot and aspect (A3) |
//! | `wax_<date>.json` / `.txt` | grip and glide per point, morning and afternoon, with the manufacturer pages (A2) |
//! | `synthesis_<date>.json` / `.txt` | the three sentences of the day and their facts (A7) |
//! | `assist.meta.json` | sources of the inputs of every run, files, notes |
//!
//! `<date>` is the first day of the run. Every JSON file starts with the inputs actually
//! read (files, missing layers) so that a past day can be checked against its data.

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::path::PathBuf;

use chrono::NaiveDate;
use serde::Serialize;
use serde_json::{Value, json};
use skimap_common::geom::LonLat;
use skimap_common::layout::DataLayout;
use skimap_common::meta::{FileMeta, LayerMeta, SourceRef, now_rfc3339};
use skimap_common::output::{write_json, write_text};
use skimap_common::site::Site;

use crate::briefing::{self, SiteBriefing};
use crate::error::Result;
use crate::inputs::{Paths, SiteData};
use crate::snow_quality::{DayQuality, Quality};
use crate::synthesis::{self, Synthesis};
use crate::wax::{self, DayWax};
use crate::{alerts, best_time, equipment};

/// Layer name.
pub const LAYER: &str = "assist";

/// Options of a run.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Options {
    /// First day.
    pub date: Option<NaiveDate>,
    /// Days after the first one (the briefing covers `date` and the next `extra_days`).
    pub extra_days: u32,
    /// Home point, for the distances of the briefing.
    pub home: Option<LonLat>,
    /// Input files that replace the current layers (past days).
    pub paths: Paths,
    /// Route suggestion requests (A5), each on its own start day.
    pub requests: Vec<crate::suggest::Request>,
    /// Outings for the equipment list (A6), besides the best loop of each request.
    pub outings: Vec<equipment::Outing>,
}

/// A replay of the alerts (A8) on the best loop of a request, with a synthetic trace at
/// the model pace divided by `slowdown`.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Replay {
    /// Request.
    pub request: String,
    /// Loop.
    pub route: String,
    /// Start, local.
    pub start: chrono::NaiveDateTime,
    /// Slowdown of the trace (1: on time).
    pub slowdown: f64,
    /// C7 deadline.
    pub deadline: Option<chrono::NaiveDateTime>,
    /// Turnaround at the planned pace (`None`: the loop ends before the deadline).
    pub planned_turnaround: Option<chrono::NaiveDateTime>,
    /// Positions replayed.
    pub fixes: usize,
    /// Alerts.
    pub alerts: Vec<alerts::Alert>,
}

/// What a run produced.
#[derive(Debug, Clone, Serialize)]
pub struct Output {
    /// Site.
    pub site: String,
    /// First day.
    pub date: NaiveDate,
    /// Briefing.
    pub briefing: SiteBriefing,
    /// Wax per day and point.
    pub wax: Vec<DayWax>,
    /// Synthesis per day.
    pub synthesis: Vec<Synthesis>,
    /// Route suggestions (A5).
    pub suggestions: Vec<crate::suggest::Suggestions>,
    /// Best time to leave per day and point (A9).
    pub best_times: Vec<best_time::BestTime>,
    /// Equipment lists (A6).
    pub equipment: Vec<equipment::Equipment>,
    /// Alert replays (A8).
    pub replays: Vec<Replay>,
    /// Files written.
    pub files: Vec<PathBuf>,
}

fn header(data: &SiteData, date: NaiveDate) -> Value {
    json!({
        "site": data.site.id,
        "date": date.to_string(),
        "generated_at": now_rfc3339(),
        "inputs": data.files,
        "missing": data.missing,
        "never_safe": "Aucune sortie n'est « sans danger » : informations indicatives, lisez le BERA et les sources officielles.",
    })
}

fn with(mut head: Value, key: &str, v: impl Serialize) -> Result<Value> {
    head[key] = serde_json::to_value(v).map_err(|e| crate::error::Error::format(key, e))?;
    Ok(head)
}

fn letter(q: Quality) -> char {
    match q {
        Quality::Fresh => 'F',
        Quality::Windblown => 'V',
        Quality::OldDry => 'A',
        Quality::Transformed => 'T',
        Quality::Wet => 'H',
        Quality::Slush => 'S',
        Quality::Hard => 'D',
        Quality::Icy => 'G',
    }
}

/// Text table of a day of snow quality: one row per hour, one letter per class.
pub fn quality_table(q: &DayQuality) -> String {
    let mut s = String::new();
    let _ = writeln!(
        s,
        "{} — {} ({} m). F fraîche, V soufflée, A ancienne sèche, T transformée (décaille), H humide, S soupe, D dure regelée, G glace",
        briefing::date_fr(q.date),
        q.point,
        q.elevation_m.round()
    );
    let _ = write!(s, "heure ");
    for c in &q.classes {
        let _ = write!(s, "{:>5}", c.code);
    }
    let _ = writeln!(s);
    let n = q.classes[0].hours.len();
    for i in 0..n {
        let _ = write!(s, "{:>4} h", q.classes[0].hours[i].time.format("%H"));
        for c in &q.classes {
            let _ = write!(s, "{:>5}", letter(c.hours[i].quality));
        }
        let _ = writeln!(s);
    }
    for l in &q.summary {
        let _ = writeln!(s, "{l}");
    }
    s
}

fn turnaround_deadline(
    point: &crate::layers::WeatherPoint,
    date: NaiveDate,
) -> Option<chrono::NaiveDateTime> {
    skimap_core::turnaround::deadline(
        date,
        point.position,
        &skimap_core::turnaround::TurnaroundParams::default(),
    )
    .map(|d| skimap_core::ephemeris::local_time(d.deadline))
}

/// Runs the assistant for a site and writes the `assist` layer.
pub fn build(site: &Site, layout: &DataLayout, opts: &Options) -> Result<Output> {
    let data = SiteData::load(layout, site, &opts.paths)?;
    let date = opts
        .date
        .unwrap_or_else(|| skimap_core::ephemeris::local_time(chrono::Utc::now()).date());
    let bopts = briefing::Options {
        home: opts.home,
        lang: crate::text::Lang::Fr,
    };
    let mut brief = briefing::brief(&data, date, opts.extra_days, &bopts);
    let mut waxes = Vec::new();
    for day in &brief.days {
        for q in &day.snow_quality {
            if let Some(p) = data.weather.as_ref().and_then(|w| w.point(&q.point))
                && let Some(w) = wax::day_wax(p, q)
            {
                waxes.push(w);
            }
        }
    }
    let syntheses: Vec<Synthesis> = brief
        .days
        .iter()
        .map(|d| synthesis::synthesise(&data, d.date))
        .collect();
    // A9: best time to leave for a 2-hour outing, per day and per forecast point; the
    // first point's goes into the briefing (text and voice, before the sunset line).
    let mut best_times: Vec<best_time::BestTime> = Vec::new();
    for day in brief.days.iter_mut() {
        let mut first: Option<best_time::BestTime> = None;
        for q in day.snow_quality.iter().filter(|q| q.enough_snow) {
            let Some(p) = data.weather.as_ref().and_then(|w| w.point(&q.point)) else {
                continue;
            };
            let req = best_time::Request {
                date: day.date,
                duration_min: 120.0,
                earliest: chrono::NaiveTime::from_hms_opt(8, 0, 0).expect("time"),
                classes: Vec::new(),
            };
            let bt = best_time::best_time(p, Some(q), day.crowd.as_ref().map(|c| c.level), &req);
            if first.is_none() {
                first = Some(bt.clone());
            }
            best_times.push(bt);
        }
        if let Some(bt) = first {
            day.text.push(format!(
                "Meilleur moment pour partir ({}) : {}",
                bt.point, bt.text
            ));
            if let Some(ev) = bt.voice() {
                let at = day
                    .voice
                    .iter()
                    .position(|e| {
                        matches!(
                            e,
                            skimap_voice::Event::Briefing(
                                skimap_voice::bank::Briefing::Sunset { .. }
                            )
                        )
                    })
                    .unwrap_or(day.voice.len());
                day.voice.insert(at, ev);
            }
        }
    }
    // A5, then A6 and A8 on the best loop of each request.
    let suggestions: Vec<crate::suggest::Suggestions> = opts
        .requests
        .iter()
        .map(|r| crate::suggest::suggest(&data, r))
        .collect();
    let kick_unreliable = |d: NaiveDate| {
        waxes
            .iter()
            .any(|w| w.date == d && (!w.morning.kick_reliable || !w.afternoon.kick_reliable))
    };
    let mut outings: Vec<equipment::Outing> = opts.outings.clone();
    for o in &mut outings {
        o.kick_unreliable |= kick_unreliable(o.start.date());
    }
    for s in &suggestions {
        if let Some(best) = s.suggestions.first() {
            outings.push(equipment::Outing {
                label: format!("{} : {}", s.request.label, best.label),
                start: s.request.start_time,
                duration_min: best.eta_min,
                route: best.line.clone(),
                offpiste: false,
                kick_unreliable: kick_unreliable(s.request.start_time.date()),
            });
        }
    }
    let lists: Vec<equipment::Equipment> = outings
        .iter()
        .map(|o| equipment::equipment(&data, o))
        .collect();
    let mut replays: Vec<Replay> = Vec::new();
    for s in &suggestions {
        let (Some(best), Some(point)) = (
            s.suggestions.first(),
            data.weather
                .as_ref()
                .and_then(|w| w.nearest(s.request.start))
                .map(|(p, _)| p),
        ) else {
            continue;
        };
        // At the requested start at three paces, then a late start (planned return 30 min
        // before the C7 deadline) at 1.5 times the model time.
        let late = turnaround_deadline(point, s.request.start_time.date()).map(|d| {
            d - chrono::Duration::seconds(
                (best.profile.total_s()
                    * skimap_core::turnaround::TurnaroundParams::default().eta_factor)
                    as i64,
            ) - chrono::Duration::minutes(30)
        });
        let mut cases = vec![
            (s.request.start_time, 1.0),
            (s.request.start_time, 1.5),
            (s.request.start_time, 2.0),
        ];
        if let Some(l) = late.filter(|l| *l > s.request.start_time) {
            cases.push((l, 1.5));
        }
        for (start, slowdown) in cases {
            let trace = alerts::synthetic_trace(&best.profile, slowdown, 300.0);
            let mut m =
                alerts::Monitor::new(start, &best.profile, point, alerts::Params::default());
            let planned_turnaround = m.planned_turnaround();
            let deadline = m.deadline();
            let found: Vec<alerts::Alert> = trace
                .iter()
                .flat_map(|(t, d)| m.update(start + chrono::Duration::seconds(*t as i64), *d))
                .collect();
            replays.push(Replay {
                request: s.request.label.clone(),
                route: best.label.clone(),
                start,
                slowdown,
                deadline,
                planned_turnaround,
                fixes: trace.len(),
                alerts: found,
            });
        }
    }

    let dir = layout.layer_dir(site, LAYER)?;
    DataLayout::ensure(&dir)?;
    let mut files = Vec::new();
    let stem = |kind: &str| format!("{kind}_{date}");
    let mut write = |name: String, body: Value, text: String| -> Result<()> {
        let j = dir.join(format!("{name}.json"));
        write_json(&j, &body)?;
        let t = dir.join(format!("{name}.txt"));
        write_text(&t, &text)?;
        files.push(j);
        files.push(t);
        Ok(())
    };

    // A1.
    let mut t = String::new();
    for d in &brief.days {
        for l in &d.text {
            let _ = writeln!(t, "{l}");
        }
        let _ = writeln!(t, "Voix : {}", briefing::spoken(d));
        let _ = writeln!(t);
    }
    write(
        stem("briefing"),
        with(header(&data, date), "briefing", &brief)?,
        t,
    )?;
    // A3.
    let qualities: Vec<&DayQuality> = brief.days.iter().flat_map(|d| &d.snow_quality).collect();
    let t = qualities
        .iter()
        .map(|q| quality_table(q))
        .collect::<Vec<_>>()
        .join("\n");
    let t = if t.is_empty() {
        "Pas de qualité de neige : la météo ne couvre aucun jour avec 24 h d'historique.\n".into()
    } else {
        t
    };
    write(
        stem("snow_quality"),
        with(
            with(
                header(&data, date),
                "params",
                crate::snow_quality::Params::default(),
            )?,
            "days",
            &qualities,
        )?,
        t,
    )?;
    // A2.
    let mut t = String::new();
    for w in &waxes {
        let _ = writeln!(
            t,
            "{} — {} : {}",
            briefing::date_fr(w.date),
            w.point,
            w.text
        );
        let _ = writeln!(t, "  Voix : {}", w.voice().announce().text());
        let _ = writeln!(t, "  Raisons : {}", w.morning.reasons.join(" ; "));
        let _ = writeln!(t, "  Sources : {}", w.sources.join(" "));
    }
    for d in &brief.days {
        for q in d.snow_quality.iter().filter(|q| !q.enough_snow) {
            let _ = writeln!(
                t,
                "{} — {} : pas ou trop peu de neige estimée ({} cm), pas de fartage.",
                briefing::date_fr(q.date),
                q.point,
                q.snow_cm.unwrap_or(0.0).round()
            );
        }
        if d.snow_quality.is_empty() {
            let _ = writeln!(
                t,
                "{} : pas de fartage, la météo ne couvre pas la journée avec 24 h d'historique.",
                briefing::date_fr(d.date)
            );
        }
    }
    write(stem("wax"), with(header(&data, date), "days", &waxes)?, t)?;
    // A7.
    let mut t = String::new();
    for s in &syntheses {
        let _ = writeln!(t, "{} — {}", briefing::date_fr(s.date), s.text());
        let spoken: Vec<String> = s.voice.iter().map(|e| e.announce().text()).collect();
        let _ = writeln!(t, "  Voix : {}", spoken.join(" "));
    }
    write(
        stem("synthesis"),
        with(header(&data, date), "days", &syntheses)?,
        t,
    )?;
    // A9.
    let mut t = String::new();
    for b in &best_times {
        let _ = writeln!(
            t,
            "{} — {} : {}",
            briefing::date_fr(b.request.date),
            b.point,
            b.text
        );
        if let Some(ev) = b.voice() {
            let _ = writeln!(t, "  Voix : {}", ev.announce().text());
        }
    }
    if best_times.is_empty() {
        t.push_str("Pas de créneau : aucun jour avec de la neige et la météo de la journée.\n");
    }
    write(
        stem("best_time"),
        with(header(&data, date), "days", &best_times)?,
        t,
    )?;
    // A6.
    if !lists.is_empty() {
        let mut t = String::new();
        for l in &lists {
            let _ = writeln!(t, "{}", l.text);
            let _ = writeln!(t, "  Voix : {}", l.voice().announce().text());
        }
        write(
            stem("equipment"),
            with(header(&data, date), "outings", &lists)?,
            t,
        )?;
    }
    // A8.
    let day_alerts: Vec<(NaiveDate, String, Vec<alerts::Alert>)> = brief
        .days
        .iter()
        .flat_map(|d| {
            data.weather
                .iter()
                .flat_map(|w| &w.points)
                .filter(move |p| p.covers(d.date, 9, 16))
                .map(move |p| {
                    (
                        d.date,
                        p.id.clone(),
                        alerts::day_replay(p, d.date, alerts::Params::default()),
                    )
                })
        })
        .collect();
    if !replays.is_empty() || !day_alerts.is_empty() {
        let mut t = String::new();
        for (d, p, a) in &day_alerts {
            let _ = writeln!(
                t,
                "Rejeu météo {} {} (sortie de 10 h, retour prévu à 16 h) : {}",
                d,
                p,
                if a.is_empty() {
                    "aucune alerte".to_owned()
                } else {
                    a.iter()
                        .map(|x| format!("{} {} ({})", x.at.format("%H:%M"), x.text, x.spoken))
                        .collect::<Vec<_>>()
                        .join(" ; ")
                }
            );
        }
        for r in &replays {
            let _ = writeln!(
                t,
                "{} ({}), départ {}, allure × {} ({} positions), heure limite {}, demi-tour prévu {} :",
                r.request,
                r.route,
                r.start.format("%d/%m %H:%M"),
                r.slowdown,
                r.fixes,
                r.deadline
                    .map(|d| d.format("%H:%M").to_string())
                    .unwrap_or_default(),
                r.planned_turnaround
                    .map(|d| d.format("%H:%M").to_string())
                    .unwrap_or_else(|| "aucun (boucle finie avant)".into())
            );
            if r.alerts.is_empty() {
                let _ = writeln!(t, "  aucune alerte");
            }
            for a in &r.alerts {
                let _ = writeln!(
                    t,
                    "  {} {:?} : {} — voix : {}",
                    a.at.format("%H:%M"),
                    a.kind,
                    a.text,
                    a.spoken
                );
            }
        }
        write(
            stem("alerts"),
            with(header(&data, date), "replays", &replays)?,
            t,
        )?;
    }
    // A5.
    if !suggestions.is_empty() {
        let mut t = String::new();
        for s in &suggestions {
            for l in &s.text {
                let _ = writeln!(t, "{l}");
            }
            let _ = writeln!(t);
        }
        write(
            stem("suggestions"),
            with(header(&data, date), "requests", &suggestions)?,
            t,
        )?;
        let feats: Vec<Value> = suggestions
            .iter()
            .flat_map(crate::suggest::features)
            .collect();
        let g = dir.join(format!("{}.geojson", stem("suggestions")));
        skimap_common::output::write_geojson(&g, &feats)?;
        files.push(g);
    }
    write_meta(&dir, site, date, &data, &files)?;
    Ok(Output {
        suggestions,
        best_times,
        equipment: lists,
        replays,
        site: site.id.to_owned(),
        date,
        briefing: brief,
        wax: waxes,
        synthesis: syntheses,
        files,
    })
}

/// Adds the sources of this run to those of the earlier runs: a source already listed
/// (same name and URL) takes the terms read now (its licence may have been checked
/// since, #171), a new one is appended.
fn merge_sources<'a>(sources: &mut Vec<SourceRef>, run: impl IntoIterator<Item = &'a SourceRef>) {
    for s in run {
        match sources
            .iter_mut()
            .find(|x| x.url == s.url && x.name == s.name)
        {
            Some(x) => *x = s.clone(),
            None => sources.push(s.clone()),
        }
    }
}

fn write_meta(
    dir: &std::path::Path,
    site: &Site,
    date: NaiveDate,
    data: &SiteData,
    files: &[PathBuf],
) -> Result<()> {
    let path = dir.join(LayerMeta::file_name(LAYER));
    // Runs accumulate: one entry per first day, with its inputs and sources.
    let mut runs: BTreeMap<String, Value> = LayerMeta::read(&path)
        .ok()
        .and_then(|m| serde_json::from_value(m.parameters["runs"].clone()).ok())
        .unwrap_or_default();
    let previous = LayerMeta::read(&path).ok();
    let mut sources: Vec<SourceRef> = previous
        .as_ref()
        .map(|m| m.sources.clone())
        .unwrap_or_default();
    // Licence of each file written by an earlier run (kept while it stays on disk).
    let earlier: BTreeMap<String, String> = previous
        .iter()
        .flat_map(|m| &m.files)
        .filter_map(|f| Some((f.name.clone(), f.licence.clone()?)))
        .collect();
    let charts = [
        SourceRef::new(
            "Swix, pages produits (farts de retenue V, klisters KX et K, glisse PS)",
            "https://swixsport.com/eu/category/ski-wax",
            "plages de température citées (faits), pages consultées le 15/11/2026",
            "Swix",
        ),
        SourceRef::new(
            "Toko, listes Grip Waxes et Ski Waxes, Wax & Tuning Guide 2026-27",
            wax::TOKO_GUIDE,
            "plages de température citées (faits), pages consultées le 15/11/2026",
            "Toko",
        ),
    ];
    merge_sources(&mut sources, data.sources.iter().chain(&charts));
    // The files of this run are made of the sources of this run only (#151): their
    // licence group, so that a file of a public run enters the public pack even when an
    // earlier run of the layer read a personal source (`skimap_pack` reads the licence
    // of each file).
    let run_licence = skimap_common::licence::group(data.sources.iter().chain(&charts).map(|s| {
        match skimap_common::licence::publishable(s) {
            true => skimap_common::licence::of_source(s),
            false => skimap_common::licence::Family::Personal,
        }
    }))
    .licence_fr();
    let this_run: Vec<String> = files
        .iter()
        .filter_map(|f| f.file_name())
        .map(|f| f.to_string_lossy().into_owned())
        .collect();
    runs.insert(
        date.to_string(),
        json!({
            "generated_at": now_rfc3339(),
            "inputs": data.files,
            "missing": data.missing,
            "files": files.iter().filter_map(|f| f.file_name()).map(|f| f.to_string_lossy().into_owned()).collect::<Vec<_>>(),
        }),
    );
    let mut names: Vec<String> = std::fs::read_dir(dir)
        .map_err(|source| crate::error::Error::Io {
            path: dir.to_owned(),
            source,
        })?
        .filter_map(|e| e.ok())
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|n| {
            (n.ends_with(".json") || n.ends_with(".txt") || n.ends_with(".geojson"))
                && !n.ends_with(".meta.json")
        })
        .collect();
    names.sort();
    let describe = |n: &str| {
        let d = if n.starts_with("briefing_") {
            "briefing du jour et des jours suivants : domaines et itinéraires classés, raisons (A1)"
        } else if n.starts_with("snow_quality_") {
            "qualité de neige par heure, par classe de pente et d'orientation, par point (A3)"
        } else if n.starts_with("wax_") {
            "fartage classique et skating par point, matin et après-midi, sources (A2)"
        } else if n.starts_with("best_time_") {
            "meilleur moment pour partir par jour et par point, créneaux essayés et leurs raisons (A9)"
        } else if n.starts_with("equipment_") {
            "liste d'équipement par sortie, chaque élément avec sa raison (A6)"
        } else if n.starts_with("alerts_") {
            "rejeux des alertes de changement de conditions et de demi-tour sur des traces synthétiques (A8)"
        } else if n.starts_with("suggestions_") {
            "boucles suggérées pour un temps, une technique et une heure de retour, raisons ; GeoJSON WGS84 pour le visualiseur (A5)"
        } else if n.starts_with("synthesis_") {
            "synthèse en trois phrases des risques et règles du jour (A7)"
        } else {
            "fichier de l'assistant"
        };
        let licence = if this_run.iter().any(|f| f == n) {
            Some(run_licence.to_owned())
        } else {
            earlier.get(n).cloned()
        };
        FileMeta {
            licence,
            ..FileMeta::new(n, d)
        }
    };
    LayerMeta {
        tickets: vec![
            "A1 (#88)".into(),
            "A2 (#89)".into(),
            "A3 (#90)".into(),
            "A5 (#92)".into(),
            "A6 (#93)".into(),
            "A7 (#94)".into(),
            "A8 (#95)".into(),
            "A9 (#96)".into(),
        ],
        valid_from: Some(date.to_string()),
        validity: Some("le jour de chaque fichier et les jours suivants qu'il couvre".into()),
        sources,
        files: names.iter().map(|n| describe(n)).collect(),
        notes: vec![
            "Heuristiques documentées dans crates/assist (README et modules) ; jamais « sans danger ».".into(),
            "Les entrées de chaque exécution (fichiers lus, couches absentes) sont dans parameters.runs et en tête de chaque JSON.".into(),
            "Open-Meteo, Nordic France, ENJ, mairie : sources à usage personnel (personal_sources) ; ces sorties en héritent.".into(),
        ],
        parameters: json!({ "runs": runs }),
        ..LayerMeta::new(LAYER, site.id)
    }
    .write(dir)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A source of an earlier run whose licence was checked since takes the terms read
    /// now (#171): the old text « non vérifiée » does not stay in the metadata.
    #[test]
    fn sources_of_earlier_runs_take_the_current_terms() {
        let src = |l: &str| SourceRef::new("geo.api.gouv.fr", "https://geo.api.gouv.fr", l, "x");
        let other = SourceRef::new("BERA", "https://b", "Licence Ouverte 2.0", "MF");
        let mut sources = vec![src("non vérifiée (F5, #11)"), other.clone()];
        merge_sources(&mut sources, &[src("ODbL 1.0")]);
        assert_eq!(sources, vec![src("ODbL 1.0"), other]);
    }
}
