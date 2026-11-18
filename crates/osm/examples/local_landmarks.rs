//! Named landmarks of boxes from the local OSM index (X5, #139), in the shape of an
//! Overpass `out center tags` answer: the input of the plans of the pistes
//! (`skimap_pisteplan::osmref`), formerly asked to Overpass. One JSON line per box.
//!
//! ```sh
//! cargo run --release -p skimap-osm --example local_landmarks -- south,west,north,east ...
//! ```

use std::time::Instant;

use skimap_common::config::Config;
use skimap_common::download::Downloader;
use skimap_common::geom::BBox;
use skimap_osm::local;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .with_writer(std::io::stderr)
        .init();
    let cfg = Config::from_env()?;
    let layout = cfg.layout();
    let dl = Downloader::from_config(&cfg)?;
    let t = Instant::now();
    let idx = local::load_landmarks(&layout, &dl)?;
    eprintln!(
        "index {} : {} éléments, {} repères, chargé en {:.1} s ; données OSM du {}",
        idx.path.display(),
        idx.elements.len(),
        idx.elements
            .iter()
            .filter(|e| local::is_landmark(e.kind, |k| e.get(k)))
            .count(),
        t.elapsed().as_secs_f64(),
        idx.valid_at.as_deref().unwrap_or("?")
    );
    for arg in std::env::args().skip(1) {
        let v: Vec<f64> = arg.split(',').map(str::parse).collect::<Result<_, _>>()?;
        let [s, w, n, e] = v[..] else {
            return Err(format!("box {arg}: south,west,north,east expected").into());
        };
        let t = Instant::now();
        let ans = idx.query(&[BBox::new(s, w, n, e)], |e| {
            local::is_landmark(e.kind, |k| e.get(k))
        });
        eprintln!(
            "{arg} : {} repères en {:.3} s",
            ans["elements"].as_array().map_or(0, Vec::len),
            t.elapsed().as_secs_f64()
        );
        println!("{ans}");
    }
    Ok(())
}
