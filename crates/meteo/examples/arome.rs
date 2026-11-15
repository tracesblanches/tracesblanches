//! Publishable hourly weather from the open AROME packages (X17, #151).
//!
//! - `cargo run -p skimap-meteo --example arome -- <site> [<site> …]`: refreshes the
//!   `meteo-hourly` layer of the sites from the latest complete run (no personal sources);
//! - `cargo run -p skimap-meteo --example arome -- --file <grib2> <lat> <lon>`: prints every
//!   used field of a local file at the nearest grid point (check against eccodes).

use skimap_common::config::Config;
use skimap_common::download::Downloader;
use skimap_common::site;
use skimap_meteo::arome;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.first().map(String::as_str) == Some("--file") {
        let [_, path, lat, lon] = args.as_slice() else {
            return Err("usage: arome --file <grib2> <lat> <lon>".into());
        };
        let t = std::time::Instant::now();
        let (fields, index) = arome::extract(path.as_ref(), &[(lat.parse()?, lon.parse()?)], None)?;
        println!(
            "grid point {:?}, index {:?}, {} fields in {:.2} s",
            index.grid_latlon[0],
            index.index[0],
            fields.len(),
            t.elapsed().as_secs_f64()
        );
        for f in fields {
            println!("{:?} {} {:?}", f.var, f.hour, f.values[0]);
        }
        return Ok(());
    }
    if args.is_empty() {
        return Err("usage: arome <site> [<site> …] | --file <grib2> <lat> <lon>".into());
    }
    let cfg = Config::from_env()?;
    let dl = Downloader::from_config(&cfg)?;
    let layout = cfg.layout();
    // Any site of the registry (evaluation sites when the census has not been run).
    let registry = site::Registry::load(&layout)?;
    let sites: Vec<&site::Site> = args
        .iter()
        .map(|a| registry.get(a))
        .collect::<Result<_, _>>()?;
    let t = std::time::Instant::now();
    let r = arome::refresh(&sites, &layout, &dl)?;
    println!(
        "run {} in {:.1} s: {}",
        r.run,
        t.elapsed().as_secs_f64(),
        serde_json::to_string(&r.sites)?
    );
    let t = std::time::Instant::now();
    let again = arome::refresh(&sites, &layout, &dl)?;
    println!(
        "second refresh in {:.2} s: {}",
        t.elapsed().as_secs_f64(),
        serde_json::to_string(&again.sites)?
    );
    for s in &sites {
        let path = layout.layer_dir(s, "meteo-hourly")?.join("hourly.json");
        let f: skimap_meteo::hourly::HourlyFile = serde_json::from_slice(&std::fs::read(&path)?)?;
        for p in &f.points {
            println!(
                "{} {} ({}) {:.4},{:.4} grid elevation {} m, {} hours {} .. {}",
                s.id,
                p.name,
                p.id,
                p.lat,
                p.lon,
                p.elevation_m,
                p.hours.len(),
                p.hours.first().map_or("?", |h| h.time.as_str()),
                p.hours.last().map_or("?", |h| h.time.as_str()),
            );
            for d in &p.days {
                println!(
                    "  {} snow {} cm, cloud 9-16h {:?} %, gust max {:?} km/h, wind chill min {:?} C",
                    d.date,
                    d.snowfall_cm,
                    d.daytime_cloud_cover_pct,
                    d.max_gust_kmh,
                    d.min_wind_chill_c
                );
            }
            for h in p.hours.iter().step_by(6) {
                println!(
                    "    {} T {:?} RH {:?} app {:?} chill {:?} wind {:?}/{:?} km/h dir {:?} snow {:?} cm precip {:?} mm cloud {:?} %",
                    h.time,
                    h.temperature_c,
                    h.relative_humidity_pct,
                    h.apparent_temperature_c,
                    h.wind_chill_c,
                    h.wind_kmh,
                    h.gust_kmh,
                    h.wind_direction_deg,
                    h.snowfall_cm,
                    h.precipitation_mm,
                    h.cloud_cover_pct
                );
            }
        }
    }
    Ok(())
}
