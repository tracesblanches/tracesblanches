//! Runs the assistant (A1, A2, A3, A7) on a site and writes `data/out/<site>/assist/`.
//! Reads the layer outputs only (no network).
//!
//! ```sh
//! cargo run --release -p skimap-assist --example assist -- cervieres 2026-11-15 \
//!     [--days 3] [--home 6.6426,44.8986] [--meteo a.json,b.json] [--snow DIR] \
//!     [--bera FILE] [--status DIR] [--route FILE] [--history past|with-forecast] [--suggest] [--outings]
//! ```
//!
//! `--history` takes the files written by the `history` example for that date:
//! `data/out/<site>/assist/history/{meteo_*.json, snow_<date>/, bera_<date>.json}`;
//! `with-forecast` adds the current forecast layer last (a run that reaches today).

use std::path::PathBuf;

use chrono::{NaiveDate, NaiveTime};
use skimap_assist::equipment::Outing;
use skimap_assist::inputs::Paths;
use skimap_assist::suggest::Request;
use skimap_common::geom::LonLat;
use skimap_common::layout::DataLayout;
use skimap_core::graph::Difficulty;
use skimap_core::routing::Style;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let site = skimap_common::site::site(args.first().ok_or("usage: assist <site> [date] ...")?)?;
    let date = args
        .get(1)
        .filter(|a| !a.starts_with("--"))
        .map(|d| NaiveDate::parse_from_str(d, "%Y-%m-%d"))
        .transpose()?;
    let layout =
        DataLayout::new(std::env::var("SKIMAP_DATA_DIR").unwrap_or_else(|_| "data".into()));
    let mut opts = skimap_assist::Options {
        date,
        extra_days: 3,
        ..Default::default()
    };
    let mut paths = Paths::default();
    let mut i = 1 + usize::from(date.is_some());
    while i < args.len() {
        let v = args.get(i + 1).cloned().unwrap_or_default();
        match args[i].as_str() {
            "--days" => opts.extra_days = v.parse()?,
            "--home" => {
                let (lon, lat) = v.split_once(',').ok_or("--home lon,lat")?;
                opts.home = Some(LonLat::new(lon.parse()?, lat.parse()?));
            }
            "--meteo" => paths.meteo = v.split(',').map(PathBuf::from).collect(),
            "--snow" => paths.snow_dirs = v.split(',').map(PathBuf::from).collect(),
            "--bera" => paths.beras = v.split(',').map(PathBuf::from).collect(),
            "--status" => paths.status_dir = Some(v.into()),
            "--route" => paths.route = Some(v.into()),
            "--history" => {
                date.ok_or("--history needs a date")?;
                let h = layout.layer_dir(site, "assist")?.join("history");
                let mut meteo: Vec<PathBuf> = std::fs::read_dir(&h)?
                    .filter_map(|e| e.ok().map(|e| e.path()))
                    .filter(|p| {
                        p.file_name()
                            .is_some_and(|n| n.to_string_lossy().starts_with("meteo_"))
                            && p.extension().is_some_and(|e| e == "json")
                            && !p.to_string_lossy().ends_with(".meta.json")
                    })
                    .collect();
                meteo.sort();
                if v == "with-forecast" {
                    meteo.push(layout.layer_dir(site, "meteo-hourly")?.join("hourly.json"));
                }
                paths.meteo = meteo;
                // Every archived snow map and BERA: each day takes its own.
                let mut entries: Vec<PathBuf> = std::fs::read_dir(&h)?
                    .filter_map(|e| e.ok().map(|e| e.path()))
                    .collect();
                entries.sort();
                for e in entries {
                    let n = e
                        .file_name()
                        .map(|n| n.to_string_lossy().into_owned())
                        .unwrap_or_default();
                    if n.starts_with("snow_") && e.is_dir() {
                        paths.snow_dirs.push(e);
                    } else if n.starts_with("bera_") && n.ends_with(".json") {
                        paths.beras.push(e);
                    }
                }
                if v == "with-forecast" {
                    paths.snow_dirs.push(layout.layer_dir(site, "snow")?);
                }
            }
            "--suggest" => {
                // Three requests per site on the first day: « 2 h skating, back before
                // 16 h », « 3 h classic », « 1 h 30 classic, green and blue » in the
                // afternoon (from another start at Cervières).
                let d = date.ok_or("--suggest needs a date")?;
                let at = |h: u32, m: u32| d.and_hms_opt(h, m, 0).expect("time");
                let (a, an, b, bn) = match site.id {
                    "cervieres" => (
                        LonLat::new(6.7269, 44.8552),
                        "Le Laus",
                        LonLat::new(6.8161, 44.8448),
                        "Les Fonts",
                    ),
                    _ => (
                        LonLat::new(6.1125, 46.6032),
                        "le départ des pistes de Chapelle-des-Bois",
                        LonLat::new(6.1125, 46.6032),
                        "le départ des pistes de Chapelle-des-Bois",
                    ),
                };
                let req = |label: &str, start, name: &str, t, min, style, back, max| Request {
                    label: label.into(),
                    start,
                    start_name: name.into(),
                    start_time: t,
                    budget_min: min,
                    style,
                    return_before: back,
                    max_difficulty: max,
                    max_results: 3,
                };
                opts.requests = vec![
                    req(
                        "2 h, skating, retour avant 16 h",
                        a,
                        an,
                        at(10, 0),
                        120.0,
                        Style::Skating,
                        NaiveTime::from_hms_opt(16, 0, 0),
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
                ];
                i -= 1;
            }
            "--outings" => {
                // Typical outings on the core proof routes (A6), with the time core
                // computed for them (C5 classic time, C3 off-piste cost).
                let d = date.ok_or("--outings needs a date")?;
                let dir = layout.layer_dir(site, "routes")?;
                let list: &[(&str, &str, u32, u32, bool)] = match site.id {
                    "cervieres" => &[
                        (
                            "Le Laus → refuge des Fonts, pistes, classique",
                            "routes.geojson",
                            10,
                            0,
                            false,
                        ),
                        (
                            "Le Laus → col d'Izoard, hors piste, départ 13 h",
                            "offpiste_laus_izoard.geojson",
                            13,
                            0,
                            true,
                        ),
                    ],
                    _ => &[(
                        "Traversée du Risoux, hors piste, départ 10 h 30",
                        "offpiste_risoux_fevrier.geojson",
                        10,
                        30,
                        true,
                    )],
                };
                for (label, file, h, m, off) in list {
                    let v: serde_json::Value =
                        serde_json::from_str(&std::fs::read_to_string(dir.join(file))?)?;
                    let f = v["features"]
                        .as_array()
                        .and_then(|a| a.iter().find(|f| f["properties"]["kind"] == "route"))
                        .ok_or("no route feature")?;
                    let p = &f["properties"];
                    let secs = p["eta_classic_s"]
                        .as_f64()
                        .or_else(|| p["summary"]["cost_s"].as_f64())
                        .ok_or("no time on the route")?;
                    let route = skimap_assist::layers::line_coords(&f["geometry"]);
                    opts.outings.push(Outing {
                        label: (*label).into(),
                        start: d.and_hms_opt(*h, *m, 0).expect("time"),
                        duration_min: secs / 60.0,
                        route,
                        offpiste: *off,
                        kick_unreliable: false,
                    });
                }
                i -= 1;
            }
            other => return Err(format!("unknown option {other}").into()),
        }
        i += 2;
    }
    opts.paths = paths;
    let t = std::time::Instant::now();
    let out = skimap_assist::build(site, &layout, &opts)?;
    let secs = t.elapsed().as_secs_f64();
    for f in &out.files {
        println!("{}", f.display());
    }
    for d in &out.briefing.days {
        println!("--- {}", d.date);
        for l in &d.text {
            println!("{l}");
        }
        println!("Voix : {}", skimap_assist::briefing::spoken(d));
    }
    for w in &out.wax {
        println!("Fartage {} {} : {}", w.date, w.point, w.text);
    }
    for s in &out.synthesis {
        println!("Synthèse {} : {}", s.date, s.text());
    }
    for s in &out.suggestions {
        for l in &s.text {
            println!("{l}");
        }
    }
    for b in &out.best_times {
        println!("Départ {} {} : {}", b.request.date, b.point, b.text);
    }
    for e in &out.equipment {
        println!("Équipement : {}", e.text);
    }
    for r in &out.replays {
        println!(
            "Alertes {} ×{} : {}",
            r.route,
            r.slowdown,
            r.alerts
                .iter()
                .map(|a| format!("{} {}", a.at.format("%H:%M"), a.text))
                .collect::<Vec<_>>()
                .join(" | ")
        );
    }
    eprintln!("assist {} : {:.3} s", site.id, secs);
    Ok(())
}
