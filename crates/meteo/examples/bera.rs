//! Builds the BERA product of a site: `cargo run -p skimap-meteo --example bera -- <site> [YYYY-MM-DD]`.
//!
//! Uses `METEOFRANCE_API_KEY` when set (today's bulletin from the API), otherwise the public
//! archive of the given day. Writes `data/out/<site>/bera/`.

use skimap_common::config::Config;
use skimap_common::download::Downloader;
use skimap_common::site;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args().skip(1);
    let site = site::site(&args.next().ok_or("usage: bera <site> [YYYY-MM-DD]")?)?;
    let date = match args.next() {
        Some(d) => chrono::NaiveDate::parse_from_str(&d, "%Y-%m-%d")?,
        None => chrono::Local::now().date_naive(),
    };
    let cfg = Config::from_env()?;
    let dl = Downloader::from_config(&cfg)?;
    let f = skimap_meteo::bera::build(
        site,
        &cfg.layout(),
        &dl,
        cfg.meteofrance_api_key.as_ref(),
        date,
    )?;
    println!(
        "site={} date={} status={:?} origin={:?}",
        f.site, f.date, f.status, f.origin
    );
    println!("xml_url={:?}\npdf_url={:?}", f.xml_url, f.pdf_url);
    println!("notice: {}", f.notice);
    if let Some(b) = &f.bulletin {
        println!(
            "massif {} ({}) risk1={:?} risk2={:?} alt={:?} max={:?} aspects N={} NE={} E={} SE={} S={} SW={} W={} NW={}",
            b.massif,
            b.massif_id,
            b.risk.risk1,
            b.risk.risk2,
            b.risk.altitude,
            b.risk.max,
            b.aspects.n,
            b.aspects.ne,
            b.aspects.e,
            b.aspects.se,
            b.aspects.s,
            b.aspects.sw,
            b.aspects.w,
            b.aspects.nw
        );
        println!("summary: {}", b.summary.as_deref().unwrap_or("-"));
        if let Some(sc) = &b.snow_cover {
            for l in &sc.levels {
                println!(
                    "  snow {} m: N {:?} cm, S {:?} cm",
                    l.altitude, l.north_cm, l.south_cm
                );
            }
        }
    }
    for l in &f.links {
        println!("link: {} <{}>", l.label, l.url);
    }
    println!(
        "written to {}",
        cfg.layout().layer_dir(site, "bera")?.display()
    );
    Ok(())
}
