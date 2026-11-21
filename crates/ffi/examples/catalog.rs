//! Writes the catalogue resource of the Android app (`android/app/src/main/assets/
//! catalog.json`): the nordic areas of the census (P3, `data/out/france/domaines/
//! domaines.geojson`) reduced to points, and the département borders of metropolitan
//! France simplified to about 2 km, for the map of the area picker. Each area carries the
//! identifier of its site in the registry (`data/out/france/sites/sites.json`, #175).
//!
//! `cargo run -p skimap-ffi --example catalog -- [OUT]` (needs `SKIMAP_DATA_DIR` or `./data`;
//! downloads the borders once through the common downloader, source `etalab-contours`).

#![allow(
    clippy::disallowed_methods,
    reason = "developer tool, not a pipeline output"
)]
use std::collections::BTreeMap;
use std::path::PathBuf;

use geo::{Coord, LineString, Simplify};
use serde_json::{Value, json};
use skimap_common::config::Config;
use skimap_common::download::{Downloader, Request};
use skimap_common::output;
use skimap_common::site::Registry;

type Res<T> = Result<T, Box<dyn std::error::Error>>;

/// Etalab administrative borders (IGN Admin Express), départements simplified at 1 km.
const CONTOURS_URL: &str = "https://etalab-datasets.geo.data.gouv.fr/contours-administratifs/latest/geojson/departements-1000m.geojson";
/// Douglas-Peucker tolerance, degrees (about 2 km): borders for a phone-sized map.
const SIMPLIFY_DEG: f64 = 0.02;

fn round(v: f64, step: f64) -> f64 {
    (v / step).round() * step
}

fn main() -> Res<()> {
    let out = std::env::args_os().nth(1).map_or_else(
        || PathBuf::from("android/app/src/main/assets/catalog.json"),
        PathBuf::from,
    );
    let cfg = Config::from_env()?;
    let layout = cfg.layout();
    let census = layout
        .france_layer_dir("domaines")?
        .join("domaines.geojson");
    let fc: Value = serde_json::from_slice(&std::fs::read(&census)?)?;
    // Site of each census domain (#175): the registry record whose `census_id` is the
    // domain, the identifier of its pack and of its entry in the server catalog.
    let registry = Registry::load(&layout)?;
    let sites: BTreeMap<&str, &str> = registry
        .records()
        .iter()
        .filter_map(|r| Some((r.census_id.as_deref()?, r.id.as_str())))
        .collect();

    let mut domains = Vec::new();
    for f in fc["features"].as_array().ok_or("no features")? {
        let p = &f["properties"];
        let name = p["name"].as_str().unwrap_or_default();
        let commune = p["commune"]["name"].as_str().unwrap_or_default();
        let id = p["id"].as_str().ok_or("census id")?;
        let site = sites
            .get(id)
            .ok_or_else(|| format!("{id} ({name}): no site in the registry"))?;
        domains.push(json!({
            "id": id, "site": site, "name": name, "alt_names": p["alt_names"],
            "commune": commune, "dep": p["commune"]["departement"],
            "lon": round(p["lon"].as_f64().ok_or("lon")?, 1e-4),
            "lat": round(p["lat"].as_f64().ok_or("lat")?, 1e-4),
            "km": round(p["km_nordic"].as_f64().unwrap_or(0.0), 0.1),
        }));
    }

    let dl = Downloader::from_config(&cfg)?;
    let body = dl
        .fetch(
            &Request::get("etalab-contours", CONTOURS_URL)
                .max_age(std::time::Duration::from_secs(365 * 86_400)),
        )?
        .text()?;
    let deps: Value = serde_json::from_str(&body)?;
    let mut outline: Vec<Vec<f64>> = Vec::new();
    let mut points = 0usize;
    for f in deps["features"].as_array().ok_or("no département")? {
        let polys: Vec<&Value> = match f["geometry"]["type"].as_str() {
            Some("Polygon") => vec![&f["geometry"]["coordinates"]],
            Some("MultiPolygon") => f["geometry"]["coordinates"]
                .as_array()
                .map(|a| a.iter().collect())
                .unwrap_or_default(),
            _ => continue,
        };
        for poly in polys {
            // Exterior ring only: borders, no holes.
            let Some(ring) = poly.get(0).and_then(Value::as_array) else {
                continue;
            };
            let line: LineString<f64> = ring
                .iter()
                .filter_map(|c| {
                    Some(Coord {
                        x: c.get(0)?.as_f64()?,
                        y: c.get(1)?.as_f64()?,
                    })
                })
                .collect();
            // Metropolitan France only (overseas départements and collectivities are
            // outside the map and have no nordic area).
            if !line
                .0
                .iter()
                .all(|c| (-5.5..=10.0).contains(&c.x) && (41.0..=51.5).contains(&c.y))
            {
                continue;
            }
            let simple = line.simplify(SIMPLIFY_DEG);
            if simple.0.len() < 4 {
                continue; // islets
            }
            points += simple.0.len();
            outline.push(
                simple
                    .0
                    .iter()
                    .flat_map(|c| [round(c.x, 1e-3), round(c.y, 1e-3)])
                    .collect(),
            );
        }
    }

    let catalog = json!({
        "format": 1,
        "generated_at": chrono::Utc::now().format("%Y-%m-%d").to_string(),
        "attribution": [
            "Recensement des domaines nordiques skimap (OSM, OpenSkiMap, Nordic France)",
            "Contours : IGN Admin Express via Etalab (Licence Ouverte 2.0)"
        ],
        "domains": domains,
        "outline": outline,
    });
    let text = serde_json::to_string(&catalog)?;
    output::write_text(&out, &text)?;
    println!(
        "{}: {} areas, {} rings, {} points, {} bytes",
        out.display(),
        catalog["domains"].as_array().map_or(0, Vec::len),
        outline.len(),
        points,
        text.len()
    );
    Ok(())
}
