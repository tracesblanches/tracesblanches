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

/// Runs the assistant for a site and writes the `assist` layer.
pub fn build(site: &Site, layout: &DataLayout, opts: &Options) -> Result<Output> {
    let data = SiteData::load(layout, site, &opts.paths)?;
    let date = opts
        .date
        .unwrap_or_else(|| skimap_core::ephemeris::local_time(chrono::Utc::now()).date());
    let bopts = briefing::Options { home: opts.home };
    let brief = briefing::brief(&data, date, opts.extra_days, &bopts);
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
    write_meta(&dir, site, date, &data, &files)?;
    Ok(Output {
        site: site.id.to_owned(),
        date,
        briefing: brief,
        wax: waxes,
        synthesis: syntheses,
        files,
    })
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
    let mut sources: Vec<SourceRef> = LayerMeta::read(&path)
        .map(|m| m.sources)
        .unwrap_or_default();
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
    for s in data.sources.iter().chain(&charts) {
        if !sources.iter().any(|x| x.url == s.url && x.name == s.name) {
            sources.push(s.clone());
        }
    }
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
        .filter(|n| (n.ends_with(".json") || n.ends_with(".txt")) && !n.ends_with(".meta.json"))
        .collect();
    names.sort();
    let describe = |n: &str| {
        let d = if n.starts_with("briefing_") {
            "briefing du jour et des jours suivants : domaines et itinéraires classés, raisons (A1)"
        } else if n.starts_with("snow_quality_") {
            "qualité de neige par heure, par classe de pente et d'orientation, par point (A3)"
        } else if n.starts_with("wax_") {
            "fartage classique et skating par point, matin et après-midi, sources (A2)"
        } else if n.starts_with("synthesis_") {
            "synthèse en trois phrases des risques et règles du jour (A7)"
        } else {
            "fichier de l'assistant"
        };
        FileMeta::new(n, d)
    };
    LayerMeta {
        tickets: vec![
            "A1 (#88)".into(),
            "A2 (#89)".into(),
            "A3 (#90)".into(),
            "A7 (#94)".into(),
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
