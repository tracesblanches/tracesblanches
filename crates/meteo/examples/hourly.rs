//! Builds the hourly weather of a site:
//! `SKIMAP_PERSONAL_SOURCES=1 cargo run -p skimap-meteo --example hourly -- <site>`.
//!
//! `SKIMAP_PERSONAL_SOURCES` stands for the `personal_sources` option until it exists in
//! `skimap_common::config::Config`.

use skimap_common::config::Config;
use skimap_common::download::Downloader;
use skimap_common::site;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let site = site::site(&std::env::args().nth(1).ok_or("usage: hourly <site>")?)?;
    let personal = std::env::var("SKIMAP_PERSONAL_SOURCES").is_ok_and(|v| v == "1");
    let cfg = Config::from_env()?;
    let dl = Downloader::from_config(&cfg)?;
    let f = skimap_meteo::hourly::build(site, &cfg.layout(), &dl, personal)?;
    for p in &f.points {
        println!(
            "{} ({}) {:.4},{:.4} elevation {} m, {} hours",
            p.name,
            p.id,
            p.lat,
            p.lon,
            p.elevation_m,
            p.hours.len()
        );
        for d in &p.days {
            println!(
                "  {} snow {} cm, cloud 9-16h {:?} %, gust max {:?} km/h, wind chill min {:?} C, fog 9-16h {} h, 0C min {:?} m",
                d.date,
                d.snowfall_cm,
                d.daytime_cloud_cover_pct,
                d.max_gust_kmh,
                d.min_wind_chill_c,
                d.daytime_fog_hours,
                d.min_freezing_level_m
            );
        }
        for h in p.hours.iter().step_by(6).take(5) {
            println!(
                "    {} T {:?} apparent {:?} chill {:?} wind {:?}/{:?} km/h vis {:?} m fog {:?} iso0 {:?} m snow {:?} cm cloud {:?} %",
                h.time,
                h.temperature_c,
                h.apparent_temperature_c,
                h.wind_chill_c,
                h.wind_kmh,
                h.gust_kmh,
                h.visibility_m,
                h.fog,
                h.freezing_level_m,
                h.snowfall_cm,
                h.cloud_cover_pct
            );
        }
    }
    Ok(())
}
