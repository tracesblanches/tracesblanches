//! Ready-made inputs of the assistant for a site (starts and proof outings from the site
//! table of `skimap-common`), shared by `skimap
//! assist` and `skimap build-all`: archived past days ([`history_paths`]), three suggestion
//! requests ([`suggestion_requests`]) and the outings of the core proof routes
//! ([`proof_outings`]).

use std::path::PathBuf;

use jiff::civil::{Date, Time};
use skimap_common::layout::DataLayout;
use skimap_common::site::Site;
use skimap_core::graph::Difficulty;
use skimap_core::routing::Style;

use crate::equipment::Outing;
use crate::error::{Error, Result};
use crate::inputs::Paths;
use crate::suggest::Request;

/// Inputs of past days archived in `assist/history/` (`skimap history`): every
/// `meteo_*.json` (oldest first), every `snow_<date>/` and `bera_<date>.json` (each day
/// takes its own). `with_forecast` adds the current forecast (`meteo-hourly`) and snow map.
pub fn history_paths(layout: &DataLayout, site: &Site, with_forecast: bool) -> Result<Paths> {
    let h = layout.layer_dir(site, "assist")?.join("history");
    let mut entries: Vec<PathBuf> = std::fs::read_dir(&h)
        .map_err(|source| Error::Io {
            path: h.clone(),
            source,
        })?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .collect();
    entries.sort();
    let mut paths = Paths::default();
    for e in entries {
        let n = e
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        if n.starts_with("meteo_") && n.ends_with(".json") && !n.ends_with(".meta.json") {
            paths.meteo.push(e);
        } else if n.starts_with("snow_") && e.is_dir() {
            paths.snow_dirs.push(e);
        } else if n.starts_with("bera_") && n.ends_with(".json") {
            paths.beras.push(e);
        }
    }
    if with_forecast {
        paths
            .meteo
            .push(layout.layer_dir(site, "meteo-hourly")?.join("hourly.json"));
        paths.snow_dirs.push(layout.layer_dir(site, "snow")?);
    }
    Ok(paths)
}

/// Three requests on `date`: « 2 h skating, back before 16 h », « 3 h classic » from the
/// main start, « 1 h 30 classic, green and blue » at 14:30 (from the second start of the
/// site table, when there is one).
pub fn suggestion_requests(site: &Site, date: Date) -> Vec<Request> {
    let at = |h: u32, m: u32| date.to_datetime(skimap_common::dates::hms(h, m, 0).unwrap());
    // Starts of the site table (`skimap_common::site::starts`): the main one, and the
    // second one for the easy request when the site has two; otherwise the site centre.
    let starts = skimap_common::site::starts(site);
    let (sa, sb) = (&starts[0], starts.get(1).unwrap_or(&starts[0]));
    let (a, an, b, bn) = (
        sa.position(),
        sa.phrase.as_str(),
        sb.position(),
        sb.phrase.as_str(),
    );
    #[allow(clippy::too_many_arguments)]
    let req =
        |label: &str, start, name: &str, t, min, style, back, max: Option<Difficulty>| Request {
            label: label.into(),
            start,
            start_name: name.into(),
            start_time: t,
            budget_min: min,
            style,
            return_before: back,
            max_difficulty: max,
            max_results: 3,
            lang: crate::text::Lang::Fr,
        };
    vec![
        req(
            "2 h, skating, retour avant 16 h",
            a,
            an,
            at(10, 0),
            120.0,
            Style::Skating,
            Time::new(16, 0, 0, 0).ok(),
            None,
        ),
        req(
            "3 h, classique",
            a,
            an,
            at(10, 0),
            180.0,
            Style::Classic,
            None,
            None,
        ),
        req(
            "1 h 30, classique, pistes vertes et bleues, départ 14 h 30",
            b,
            bn,
            at(14, 30),
            90.0,
            Style::Classic,
            None,
            Some(Difficulty::Easy),
        ),
    ]
}

/// Typical outings on the core proof routes (A6) on `date`, with the time core computed
/// for them (C5 classic time, C3 off-piste cost); a route file that does not exist is
/// skipped and named in the second list.
pub fn proof_outings(
    layout: &DataLayout,
    site: &Site,
    date: Date,
) -> Result<(Vec<Outing>, Vec<String>)> {
    let dir = layout.layer_dir(site, "routes")?;
    // Proof outings of the site table (`skimap_common::site::local`); none elsewhere.
    let list: Vec<(&str, &str, u32, u32, bool)> = skimap_common::site::local(site.id)
        .map(|l| {
            l.proof_outings
                .iter()
                .map(|o| {
                    let t =
                        skimap_common::site::parse_hhmm(&o.start_time).expect("site table time");
                    (
                        o.label.as_str(),
                        o.file.as_str(),
                        t.hour() as u32,
                        t.minute() as u32,
                        o.offpiste,
                    )
                })
                .collect()
        })
        .unwrap_or_default();
    let (mut outings, mut skipped) = (Vec::new(), Vec::new());
    for (label, file, h, m, off) in &list {
        let path = dir.join(file);
        if !path.is_file() {
            skipped.push(format!("{label} : {} absent", path.display()));
            continue;
        }
        let v: serde_json::Value = crate::layers::read_json(&path)?;
        let f = v["features"]
            .as_array()
            .and_then(|a| a.iter().find(|f| f["properties"]["kind"] == "route"))
            .ok_or_else(|| Error::format(&path, "no route feature"))?;
        let p = &f["properties"];
        let secs = p["eta_classic_s"]
            .as_f64()
            .or_else(|| p["summary"]["cost_s"].as_f64())
            .ok_or_else(|| Error::format(&path, "no time on the route"))?;
        outings.push(Outing {
            label: (*label).into(),
            start: date.to_datetime(skimap_common::dates::hms(*h, *m, 0).unwrap()),
            duration_min: secs / 60.0,
            route: crate::layers::line_coords(&f["geometry"]),
            offpiste: *off,
            kick_unreliable: false,
        });
    }
    Ok((outings, skipped))
}

#[cfg(test)]
mod tests {
    use super::*;
    /// Evaluation site of `data/sites.json`, test data.
    use jiff::civil::date;
    use skimap_common::geom::LonLat;
    static CERVIERES: std::sync::LazyLock<skimap_common::site::Site> =
        std::sync::LazyLock::new(|| {
            *skimap_common::site::site("cervieres").expect("evaluation site")
        });
    /// Evaluation site of `data/sites.json`, test data.
    static CHAPELLE_DES_BOIS: std::sync::LazyLock<skimap_common::site::Site> =
        std::sync::LazyLock::new(|| {
            *skimap_common::site::site("chapelle-des-bois").expect("evaluation site")
        });

    #[test]
    fn archived_days_by_kind() {
        let tmp = tempfile::tempdir().unwrap();
        let layout = DataLayout::new(tmp.path());
        let h = layout
            .layer_dir(&CERVIERES, "assist")
            .unwrap()
            .join("history");
        std::fs::create_dir_all(h.join("snow_2026-01-05")).unwrap();
        std::fs::create_dir_all(h.join("snow_2026-01-04")).unwrap();
        for f in [
            "meteo_2026-01-09_2026-01-10.json",
            "meteo_2026-01-04_2026-01-08.json",
            "meteo-hourly.meta.json",
            "meteo_x.meta.json",
            "meteo_notes.txt",
            "snow_2026-01-06.json",
            "bera_2026-01-05.json",
            "bera_2026-01-05.txt",
            "bera.meta.json",
            "autre.json",
        ] {
            std::fs::write(h.join(f), "{}").unwrap();
        }
        let p = history_paths(&layout, &CERVIERES, false).unwrap();
        let names = |v: &[PathBuf]| -> Vec<String> {
            v.iter()
                .map(|p| p.file_name().unwrap().to_string_lossy().into_owned())
                .collect()
        };
        assert_eq!(
            names(&p.meteo),
            [
                "meteo_2026-01-04_2026-01-08.json",
                "meteo_2026-01-09_2026-01-10.json"
            ]
        );
        assert_eq!(names(&p.snow_dirs), ["snow_2026-01-04", "snow_2026-01-05"]);
        assert_eq!(names(&p.beras), ["bera_2026-01-05.json"]);
        assert_eq!(p.status_dir, None);
        // With the forecast: the current layers last.
        let p = history_paths(&layout, &CERVIERES, true).unwrap();
        assert!(
            p.meteo
                .last()
                .unwrap()
                .ends_with("meteo-hourly/hourly.json")
        );
        assert!(p.snow_dirs.last().unwrap().ends_with("snow"));
        assert_eq!(p.meteo.len(), 3);
        // No archive: an error that names the directory.
        let e = history_paths(&layout, &CHAPELLE_DES_BOIS, false).unwrap_err();
        assert!(e.to_string().contains("history"), "{e}");
    }

    #[test]
    fn requests_of_both_sites() {
        let d = date(2027, 1, 12);
        let c = suggestion_requests(&CERVIERES, d);
        assert_eq!(c.len(), 3);
        assert_eq!(c[0].start_name, "Le Laus");
        assert_eq!(c[2].start_name, "Les Fonts");
        assert_eq!(c[2].max_difficulty, Some(Difficulty::Easy));
        assert_eq!(c[0].return_before, Time::new(16, 0, 0, 0).ok());
        assert_eq!((c[1].budget_min, c[1].style), (180.0, Style::Classic));
        assert_eq!(c[2].start_time, d.at(14, 30, 0, 0));
        let ch = suggestion_requests(&CHAPELLE_DES_BOIS, d);
        assert!(ch.iter().all(|r| r.start == LonLat::new(6.1125, 46.6032)));
    }

    #[test]
    fn outings_of_the_proof_routes() {
        let tmp = tempfile::tempdir().unwrap();
        let layout = DataLayout::new(tmp.path());
        let dir = layout.layer_dir(&CERVIERES, "routes").unwrap();
        std::fs::create_dir_all(&dir).unwrap();
        let line = serde_json::json!({"type": "LineString", "coordinates": [[6.72, 44.85], [6.73, 44.85]]});
        // The pistes route with its classic time; the off-piste one with the cost only.
        std::fs::write(
            dir.join("routes.geojson"),
            serde_json::json!({"type": "FeatureCollection", "features": [
                {"type": "Feature", "geometry": line, "properties": {"kind": "start"}},
                {"type": "Feature", "geometry": line, "properties": {"kind": "route", "eta_classic_s": 8100.0}}
            ]})
            .to_string(),
        )
        .unwrap();
        let d = date(2027, 1, 12);
        let (o, skipped) = proof_outings(&layout, &CERVIERES, d).unwrap();
        assert_eq!(o.len(), 1);
        assert_eq!(o[0].duration_min, 135.0);
        assert_eq!(o[0].start, d.at(10, 0, 0, 0));
        assert!(!o[0].offpiste);
        assert_eq!(o[0].route.len(), 2);
        assert_eq!(o[0].label, "Le Laus → refuge des Fonts, pistes, classique");
        assert_eq!(skipped.len(), 1);
        assert!(skipped[0].starts_with("Le Laus → col d'Izoard, hors piste, départ 13 h : "));
        std::fs::write(
            dir.join("offpiste_laus_izoard.geojson"),
            serde_json::json!({"type": "FeatureCollection", "features": [
                {"type": "Feature", "geometry": line, "properties": {"kind": "route", "summary": {"cost_s": 3600.0}}}
            ]})
            .to_string(),
        )
        .unwrap();
        let (o, skipped) = proof_outings(&layout, &CERVIERES, d).unwrap();
        assert!(skipped.is_empty());
        assert_eq!(o.len(), 2);
        assert_eq!((o[1].duration_min, o[1].offpiste), (60.0, true));
        assert_eq!(o[1].start, d.at(13, 0, 0, 0));
        // A file without a route feature, or without a time: an error.
        std::fs::write(
            dir.join("offpiste_laus_izoard.geojson"),
            serde_json::json!({"type": "FeatureCollection", "features": [
                {"type": "Feature", "geometry": line, "properties": {"kind": "start"}}
            ]})
            .to_string(),
        )
        .unwrap();
        assert!(proof_outings(&layout, &CERVIERES, d).is_err());
        std::fs::write(
            dir.join("offpiste_laus_izoard.geojson"),
            serde_json::json!({"type": "FeatureCollection", "features": [
                {"type": "Feature", "geometry": line, "properties": {"kind": "route"}}
            ]})
            .to_string(),
        )
        .unwrap();
        assert!(proof_outings(&layout, &CERVIERES, d).is_err());
        // Chapelle-des-Bois: the Risoux crossing at 10 h 30, off the pistes.
        let cdir = layout.layer_dir(&CHAPELLE_DES_BOIS, "routes").unwrap();
        std::fs::create_dir_all(&cdir).unwrap();
        std::fs::write(
            cdir.join("offpiste_risoux_fevrier.geojson"),
            serde_json::json!({"type": "FeatureCollection", "features": [
                {"type": "Feature", "geometry": line, "properties": {"kind": "route", "eta_classic_s": 600.0}}
            ]})
            .to_string(),
        )
        .unwrap();
        let (o, _) = proof_outings(&layout, &CHAPELLE_DES_BOIS, d).unwrap();
        assert_eq!(o.len(), 1);
        assert_eq!((o[0].start, o[0].offpiste), (d.at(10, 30, 0, 0), true));
    }
}
