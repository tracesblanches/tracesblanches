//! Import of the history from the export archive of Strava (V15 #78), on the phone: the
//! user asks Strava for their archive (« Télécharger ou supprimer votre compte »), picks the
//! zip in the app, and this module reads it. Nothing is sent anywhere and the Strava API
//! is not called.
//!
//! The archive holds `activities.csv` (one row per activity: identifier, date, name,
//! type, ..., `Filename`) and the original files under `activities/` (`.gpx`, `.tcx`,
//! `.fit`, each possibly gzipped). Only skiing on snow without lifts is kept (Strava's
//! « Nordic Ski » and « Backcountry Ski »); every outing becomes a directory
//! `<out>/strava-<id>/` with `outing.json` (summary and thinned track, the challenges)
//! and `observation.json` (what the calibration C11 learns).

use std::collections::BTreeMap;
use std::io::Read;
use std::path::Path;

use serde::Serialize;
use skimap_core::routing::Style;

use crate::track::{self, OBSERVATION_FILE, OUTING_FILE, Point, TrackError};

/// Largest activity file read, decompressed (a 10-hour FIT at 1 Hz is about 1 MB).
pub const MAX_FILE_BYTES: u64 = 64 << 20;

/// What an import did.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct ImportReport {
    /// Outings imported now (identifiers, `strava-<id>`).
    pub imported: Vec<String>,
    /// Outings already on the phone (an import again).
    pub already: u32,
    /// Ski activities without a track (entered by hand).
    pub without_track: u32,
    /// Activities of other types, by type.
    pub other_types: BTreeMap<String, u32>,
    /// Outings learned by the calibration (with times and 10 min moving).
    pub observations: u32,
    /// Problems (first 20), each with the activity.
    pub errors: Vec<String>,
}

/// Errors of the archive itself.
#[derive(Debug, thiserror::Error)]
pub enum ImportError {
    /// Unreadable file.
    #[error("{0}")]
    Io(#[from] std::io::Error),
    /// Not a zip.
    #[error("not a zip archive: {0}")]
    Zip(String),
    /// No `activities.csv`.
    #[error("no activities.csv in the archive: not a Strava export")]
    NotStrava,
    /// Bad CSV.
    #[error("activities.csv: {0}")]
    Csv(String),
}

/// Whether a Strava activity type is skiing kept by skimap.
pub fn is_nordic(activity_type: &str) -> bool {
    let t = activity_type.to_lowercase();
    !t.contains("alpin")
        && [
            "nordic",
            "backcountry",
            "ski de fond",
            "ski nordique",
            "ski de randonnée",
            "ski de rando",
        ]
        .iter()
        .any(|k| t.contains(k))
}

fn column(headers: &csv::StringRecord, names: &[&str]) -> Option<usize> {
    headers.iter().position(|h| {
        let h = h.trim().to_lowercase();
        names.iter().any(|n| h == *n)
    })
}

fn read_entry<R: Read + std::io::Seek>(
    zip: &mut zip::ZipArchive<R>,
    name: &str,
) -> Result<Vec<u8>, String> {
    let f = zip.by_name(name).map_err(|e| format!("{name}: {e}"))?;
    let mut raw = Vec::new();
    f.take(MAX_FILE_BYTES + 1)
        .read_to_end(&mut raw)
        .map_err(|e| format!("{name}: {e}"))?;
    if name.ends_with(".gz") {
        let mut out = Vec::new();
        flate2::read::GzDecoder::new(&raw[..])
            .take(MAX_FILE_BYTES + 1)
            .read_to_end(&mut out)
            .map_err(|e| format!("{name}: gzip: {e}"))?;
        raw = out;
    }
    if raw.len() as u64 > MAX_FILE_BYTES {
        return Err(format!("{name}: over {MAX_FILE_BYTES} bytes"));
    }
    Ok(raw)
}

/// Points of an activity file by its name (`.gpx`, `.tcx`, `.fit`, optionally `.gz`).
pub fn parse_file(name: &str, data: &[u8]) -> Result<Vec<Point>, TrackError> {
    let base = name.strip_suffix(".gz").unwrap_or(name).to_lowercase();
    if base.ends_with(".fit") {
        crate::fit::parse(data)
    } else if base.ends_with(".gpx") || base.ends_with(".tcx") {
        track::parse_xml(&String::from_utf8_lossy(data))
    } else {
        Err(TrackError::Format(format!("{name}: unknown format")))
    }
}

/// Imports the archive at `zip_path` into `out_root`. `style`: the technique the model
/// times of the calibration are computed with (the group's).
pub fn import(zip_path: &Path, out_root: &Path, style: Style) -> Result<ImportReport, ImportError> {
    let file = std::fs::File::open(zip_path)?;
    let mut zip = zip::ZipArchive::new(std::io::BufReader::new(file))
        .map_err(|e| ImportError::Zip(e.to_string()))?;
    let csv_name = zip
        .file_names()
        .filter_map(|n| n.ok().map(|n| n.into_owned()))
        .filter(|n| n.rsplit('/').next() == Some("activities.csv"))
        .min_by_key(String::len)
        .ok_or(ImportError::NotStrava)?;
    let prefix = csv_name
        .strip_suffix("activities.csv")
        .unwrap_or("")
        .to_owned();
    let csv_bytes = read_entry(&mut zip, &csv_name).map_err(ImportError::Csv)?;
    let mut rdr = csv::ReaderBuilder::new()
        .flexible(true)
        .from_reader(&csv_bytes[..]);
    let headers = rdr
        .headers()
        .map_err(|e| ImportError::Csv(e.to_string()))?
        .clone();
    let id_col = column(&headers, &["activity id", "id de l'activité"])
        .ok_or_else(|| ImportError::Csv("no Activity ID column".into()))?;
    let type_col = column(&headers, &["activity type", "type d'activité"])
        .ok_or_else(|| ImportError::Csv("no Activity Type column".into()))?;
    let name_col = column(&headers, &["activity name", "nom de l'activité"]);
    let file_col = column(&headers, &["filename", "nom du fichier"])
        .ok_or_else(|| ImportError::Csv("no Filename column".into()))?;
    std::fs::create_dir_all(out_root)?;
    let mut rep = ImportReport::default();
    let error = |rep: &mut ImportReport, m: String| {
        if rep.errors.len() < 20 {
            rep.errors.push(m);
        }
    };
    for row in rdr.records() {
        let row = match row {
            Ok(r) => r,
            Err(e) => {
                error(&mut rep, format!("CSV: {e}"));
                continue;
            }
        };
        let get = |c: usize| row.get(c).unwrap_or("").trim();
        let kind = get(type_col);
        if !is_nordic(kind) {
            *rep.other_types.entry(kind.to_owned()).or_default() += 1;
            continue;
        }
        let sid = get(id_col);
        if sid.is_empty() || !sid.bytes().all(|b| b.is_ascii_digit()) {
            error(&mut rep, format!("activity id {sid:?}"));
            continue;
        }
        let id = format!("strava-{sid}");
        let dir = out_root.join(&id);
        if dir.join(OUTING_FILE).is_file() {
            rep.already += 1;
            continue;
        }
        let fname = get(file_col);
        if fname.is_empty() {
            rep.without_track += 1;
            continue;
        }
        let entry = format!("{prefix}{fname}");
        let points = match read_entry(&mut zip, &entry)
            .and_then(|d| parse_file(&entry, &d).map_err(|e| format!("{entry}: {e}")))
        {
            Ok(p) if p.len() >= 2 => p,
            Ok(_) => {
                rep.without_track += 1;
                continue;
            }
            Err(e) => {
                error(&mut rep, e);
                continue;
            }
        };
        let name = name_col.map(get).unwrap_or("");
        let outing = track::summarise(&id, "strava", name, kind, &points);
        let backcountry = kind.to_lowercase().contains("backcountry")
            || kind.to_lowercase().contains("randonnée");
        let obs = track::observation(&points, style, backcountry);
        std::fs::create_dir_all(&dir)?;
        if let Some(o) = &obs {
            let json = serde_json::to_string(o).map_err(std::io::Error::other)?;
            std::fs::write(dir.join(OBSERVATION_FILE), json)?;
            rep.observations += 1;
        }
        // outing.json last: its presence marks a complete import.
        let json = serde_json::to_string(&outing).map_err(std::io::Error::other)?;
        let tmp = dir.join(".outing.json.tmp");
        std::fs::write(&tmp, json)?;
        std::fs::rename(&tmp, dir.join(OUTING_FILE))?;
        rep.imported.push(id);
    }
    Ok(rep)
}
