//! Writes the reference routes of the challenges embedded in the app (V13 #76) from the
//! SRN layer of France (G5, `data/out/france/srn/srn_routes.geojson`): the Grande
//! Traversée du Jura à ski de randonnée nordique (DATAtourisme, Licence Ouverte 2.0),
//! simplified to 10 m (Douglas–Peucker), with its source and licence.
//!
//! ```sh
//! cargo run -p skimap-collab --features phone --example challenge_assets -- \
//!   data/out/france/srn/srn_routes.geojson android/community/src/main/assets/challenges.json
//! ```

#![allow(
    clippy::disallowed_methods,
    reason = "developer tool, not a pipeline output"
)]
use serde_json::{Value, json};

fn simplify(pts: &[[f64; 2]], tol_m: f64) -> Vec<[f64; 2]> {
    if pts.len() < 3 {
        return pts.to_vec();
    }
    let lat0 = pts[0][1].to_radians().cos();
    let xy = |p: [f64; 2]| (p[0] * 111_195.0 * lat0, p[1] * 111_195.0);
    let mut keep = vec![false; pts.len()];
    keep[0] = true;
    keep[pts.len() - 1] = true;
    let mut stack = vec![(0, pts.len() - 1)];
    while let Some((a, b)) = stack.pop() {
        let (pa, pb) = (xy(pts[a]), xy(pts[b]));
        let (dx, dy) = (pb.0 - pa.0, pb.1 - pa.1);
        let l = dx.hypot(dy).max(1e-9);
        let mut best = (0.0, a);
        for (i, &p) in pts.iter().enumerate().take(b).skip(a + 1) {
            let q = xy(p);
            let d = ((q.0 - pa.0) * dy - (q.1 - pa.1) * dx).abs() / l;
            if d > best.0 {
                best = (d, i);
            }
        }
        if best.0 > tol_m {
            keep[best.1] = true;
            stack.push((a, best.1));
            stack.push((best.1, b));
        }
    }
    pts.iter()
        .zip(keep)
        .filter(|(_, k)| *k)
        .map(|(p, _)| [(p[0] * 1e6).round() / 1e6, (p[1] * 1e6).round() / 1e6])
        .collect()
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let [input, output] = args.as_slice() else {
        eprintln!("usage: challenge_assets <srn_routes.geojson> <challenges.json>");
        std::process::exit(2);
    };
    let raw = std::fs::read_to_string(input).expect("read SRN routes");
    let v: Value = serde_json::from_str(&raw).expect("GeoJSON");
    let gtj = v["features"]
        .as_array()
        .expect("features")
        .iter()
        .find(|f| {
            f["properties"]["name"]
                .as_str()
                .is_some_and(|n| n.contains("Grande traversée du Jura") && n.contains("nordique"))
        })
        .expect("GTJ in the SRN routes");
    let p = &gtj["properties"];
    let coords: Vec<[f64; 2]> = gtj["geometry"]["coordinates"]
        .as_array()
        .expect("LineString")
        .iter()
        .map(|c| [c[0].as_f64().unwrap(), c[1].as_f64().unwrap()])
        .collect();
    let simple = simplify(&coords, 10.0);
    let out = json!({
        "format": 1,
        "generated_from": "data/out/france/srn/srn_routes.geojson (G5)",
        "routes": [{
            "id": "gtj-srn",
            "name": p["name"],
            "length_km": p["length_km"],
            "published_length_km": p["published_length_km"],
            "source": p["url"],
            "publisher": p["publisher"],
            "licence": p["licence"],
            "attribution": p["attribution"],
            "line": simple,
        }]
    });
    std::fs::write(output, serde_json::to_string(&out).unwrap()).expect("write");
    eprintln!(
        "GTJ: {} points → {} points, {} km",
        coords.len(),
        simple.len(),
        p["length_km"]
    );
}
