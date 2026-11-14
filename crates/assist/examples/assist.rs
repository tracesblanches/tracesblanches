//! Runs the assistant (A1, A2, A3, A7) on a site and writes `data/out/<site>/assist/`.
//! Reads the layer outputs only (no network).
//!
//! ```sh
//! cargo run --release -p skimap-assist --example assist -- cervieres 2026-11-15 \
//!     [--days 3] [--home 6.6426,44.8986] [--meteo a.json,b.json] [--snow DIR] \
//!     [--bera FILE] [--status DIR] [--route FILE] [--history past|with-forecast]
//! ```
//!
//! `--history` takes the files written by the `history` example for that date:
//! `data/out/<site>/assist/history/{meteo_*.json, snow_<date>/, bera_<date>.json}`;
//! `with-forecast` adds the current forecast layer last (a run that reaches today).

use std::path::PathBuf;

use chrono::NaiveDate;
use skimap_assist::inputs::Paths;
use skimap_common::geom::LonLat;
use skimap_common::layout::DataLayout;

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
    eprintln!("assist {} : {:.3} s", site.id, secs);
    Ok(())
}
