//! Checks on the layer produced from the real sources (`--example srn` first):
//!
//! ```sh
//! cargo test -p skimap-srn --test real_layer -- --ignored
//! ```

use std::path::PathBuf;

use serde_json::Value;

fn layer(file: &str) -> Value {
    let root = std::env::var("SKIMAP_DATA_DIR").map_or_else(
        |_| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../data"),
        PathBuf::from,
    );
    let p = root.join("out/france/srn").join(file);
    serde_json::from_slice(&std::fs::read(&p).unwrap_or_else(|e| panic!("{}: {e}", p.display())))
        .unwrap()
}

fn features(v: &Value) -> Vec<&Value> {
    v["features"].as_array().unwrap().iter().collect()
}

fn area<'a>(areas: &'a [&'a Value], kind: &str, name_part: &str) -> &'a Value {
    areas
        .iter()
        .copied()
        .filter(|a| {
            a["properties"]["kind"] == kind
                && a["properties"]["name"]
                    .as_str()
                    .unwrap()
                    .to_lowercase()
                    .contains(&name_part.to_lowercase())
        })
        .max_by(|a, b| {
            let k = |x: &Value| x["properties"]["km_routes"].as_f64().unwrap();
            k(a).total_cmp(&k(b))
        })
        .unwrap_or_else(|| panic!("no {kind} named like {name_part}"))
}

#[test]
#[ignore = "needs data/out/france/srn (cargo run --release -p skimap-srn --example srn)"]
fn gtj_gtv_hauts_plateaux() {
    let v = layer("srn_areas.geojson");
    let areas = features(&v);
    // GTJ à ski de randonnée nordique: DATAtourisme publishes 100 km (tourDistance).
    let gtj = &area(&areas, "traversee", "ski de randonnée nordique")["properties"];
    assert_eq!(gtj["massif"], "Jura");
    let km = gtj["length_km"].as_f64().unwrap();
    assert!((95.0..=110.0).contains(&km), "GTJ {km} km");
    assert_eq!(gtj["published_length_km"].as_f64(), Some(100.0));
    // Traversée nordique des Hauts-Plateaux, Corrençon – Col de Rousset (Geotrek 36.4 km).
    let gtv = &area(&areas, "traversee", "Corrençon - Col de Rousset")["properties"];
    let km = gtv["length_km"].as_f64().unwrap();
    let published = gtv["published_length_km"].as_f64().unwrap();
    assert!(
        (km - published).abs() / published < 0.03,
        "{km} vs {published}"
    );
    // The plateau as a play area holding both traverses.
    let hp = &area(&areas, "domaine", "Hauts-Plateaux")["properties"];
    assert!(hp["routes"].as_array().unwrap().len() >= 2);
    // Two corridors of about 1 km over 78 km of itineraries, sharing their north end.
    let km2 = hp["area_km2"].as_f64().unwrap();
    assert!((35.0..80.0).contains(&km2), "{km2} km²");
    let crosses = hp["crosses"].as_array().unwrap();
    assert_eq!(
        crosses.len(),
        2,
        "both traverses cross the plateau: {crosses:?}"
    );
}

#[test]
#[ignore = "needs data/out/france/srn (cargo run --release -p skimap-srn --example srn)"]
fn every_area_is_french_and_documented() {
    let v = layer("srn_areas.geojson");
    let areas = features(&v);
    assert!(areas.len() >= 20, "{} areas", areas.len());
    let routes = layer("srn_routes.geojson");
    let ids: std::collections::BTreeSet<&str> = features(&routes)
        .iter()
        .map(|f| f["properties"]["id"].as_str().unwrap())
        .collect();
    for a in &areas {
        let p = &a["properties"];
        assert!(!p["commune"]["insee"].as_str().unwrap().is_empty());
        assert_ne!(p["massif"], "autre", "{}", p["name"]);
        assert!(p["km_routes"].as_f64().unwrap() > 0.0);
        assert!(!p["licences"].as_array().unwrap().is_empty());
        for r in p["routes"].as_array().unwrap() {
            assert!(
                ids.contains(r.as_str().unwrap()),
                "{r} missing from srn_routes"
            );
        }
    }
    let meta = layer("srn.meta.json");
    assert_eq!(meta["layer"], "srn");
    assert_eq!(meta["stats"]["areas"].as_u64(), Some(areas.len() as u64));
}

fn multipolygon(g: &Value) -> geo_types::MultiPolygon<f64> {
    let ring = |r: &Value| -> geo_types::LineString<f64> {
        r.as_array()
            .unwrap()
            .iter()
            .map(|c| geo_types::Coord {
                x: c[0].as_f64().unwrap(),
                y: c[1].as_f64().unwrap(),
            })
            .collect()
    };
    geo_types::MultiPolygon::new(
        g["coordinates"]
            .as_array()
            .unwrap()
            .iter()
            .map(|p| {
                let rings = p.as_array().unwrap();
                geo_types::Polygon::new(ring(&rings[0]), rings[1..].iter().map(ring).collect())
            })
            .collect(),
    )
}

#[test]
#[ignore = "needs data/out/france/srn (cargo run --release -p skimap-srn --example srn)"]
fn footprints_hold_their_itineraries() {
    use geo::Contains;
    let v = layer("srn_areas.geojson");
    let routes = layer("srn_routes.geojson");
    let by_id: std::collections::BTreeMap<&str, &Value> = features(&routes)
        .into_iter()
        .map(|f| (f["properties"]["id"].as_str().unwrap(), f))
        .collect();
    for a in features(&v) {
        let shape = multipolygon(&a["geometry"]);
        let (mut n, mut out) = (0, 0);
        for r in a["properties"]["routes"].as_array().unwrap() {
            let g = &by_id[r.as_str().unwrap()]["geometry"];
            let lines: Vec<&Value> = if g["type"] == "LineString" {
                vec![&g["coordinates"]]
            } else {
                g["coordinates"].as_array().unwrap().iter().collect()
            };
            for l in lines {
                for c in l.as_array().unwrap() {
                    n += 1;
                    let p = geo_types::Point::new(c[0].as_f64().unwrap(), c[1].as_f64().unwrap());
                    out += usize::from(!shape.contains(&p));
                }
            }
        }
        assert_eq!(
            out, 0,
            "{}: {out} of {n} points outside",
            a["properties"]["name"]
        );
    }
}
