//! Roads near a track (exits), from the IGN BD TOPO (WFS of the Géoplateforme, layer
//! `BDTOPO_V3:troncon_de_route`, Licence Ouverte 2.0).
//!
//! A road is a section of nature « Type autoroutier », « Route à 1 chaussée », « Route à
//! 2 chaussées », « Bretelle » or « Rond-point », open to cars (`acces_vehicule_leger`
//! « Libre » or « A péage »), in service, and **not closed in winter**: the BD TOPO gives
//! the closure period (`periode_de_fermeture`, e.g. `[{"cycle": "Hiver"}]` on the road of
//! the col d'Izoard above Le Laus), and such sections are left out. Overpass was the first
//! choice (OSM roads) but stopped answering on 15/11/2026 and its tags rarely say whether
//! a pass road is closed in winter.
//!
//! The track is cut into pieces of about 5 km; one WFS request per piece, on its box
//! enlarged by about 200 m (filter `CQL_FILTER` on the nature and the box).

use skimap_common::crs;
use skimap_common::download::{CachePolicy, Downloader, Request};
use skimap_common::geom::LonLat;

use crate::{Error, Result};

/// WFS endpoint of the Géoplateforme.
pub const WFS_URL: &str = "https://data.geopf.fr/wfs/ows";
/// Layer of road sections.
pub const LAYER: &str = "BDTOPO_V3:troncon_de_route";
/// Downloader source (IGN Géoplateforme quota).
pub const SOURCE: &str = "ign";
const CACHE_DAYS: u64 = 90;
/// Margin of the box around each piece of track, degrees (≈ 200 m).
const MARGIN_DEG: f64 = 0.002;
/// Length of a piece of track per request, metres.
const PIECE_M: f64 = 5000.0;
/// Distance between the road points kept, metres.
const DENSIFY_M: f64 = 20.0;
const PAGE: usize = 5000;

/// Natures of the BD TOPO that are roads for a car.
pub const NATURES: [&str; 5] = [
    "Type autoroutier",
    "Route à 1 chaussée",
    "Route à 2 chaussées",
    "Bretelle",
    "Rond-point",
];

/// Boxes (south, west, north, east) of the pieces of a track.
pub fn boxes(track: &[LonLat]) -> Vec<(f64, f64, f64, f64)> {
    let mut out = Vec::new();
    let mut cur: Option<(f64, f64, f64, f64)> = None;
    let mut len = 0.0;
    let mut last: Option<LonLat> = None;
    for p in track {
        if let Some(l) = last {
            len += l.haversine_m(*p);
        }
        last = Some(*p);
        let b = cur.get_or_insert((p.lat, p.lon, p.lat, p.lon));
        b.0 = b.0.min(p.lat);
        b.1 = b.1.min(p.lon);
        b.2 = b.2.max(p.lat);
        b.3 = b.3.max(p.lon);
        if len >= PIECE_M {
            out.push(cur.take().expect("box"));
            cur = Some((p.lat, p.lon, p.lat, p.lon));
            len = 0.0;
        }
    }
    out.extend(cur);
    out.into_iter()
        .map(|(s, w, n, e)| {
            (
                s - MARGIN_DEG,
                w - MARGIN_DEG,
                n + MARGIN_DEG,
                e + MARGIN_DEG,
            )
        })
        .collect()
}

/// CQL filter of a box.
pub fn cql(b: (f64, f64, f64, f64)) -> String {
    let natures: Vec<String> = NATURES.iter().map(|n| format!("'{n}'")).collect();
    format!(
        "nature IN ({}) AND BBOX(geometrie,{:.5},{:.5},{:.5},{:.5},'EPSG:4326')",
        natures.join(","),
        b.1,
        b.0,
        b.3,
        b.2
    )
}

fn encode(s: &str) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || b"-_.~".contains(&b) {
            out.push(char::from(b));
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

/// Whether a section is an open road in winter.
pub fn usable(props: &serde_json::Value) -> bool {
    let access = props["acces_vehicule_leger"].as_str().unwrap_or("Libre");
    let state = props["etat_de_l_objet"].as_str().unwrap_or("En service");
    let closed_in_winter = props["periode_de_fermeture"]
        .as_str()
        .is_some_and(|p| p.contains("Hiver") || p.contains("hiver"));
    matches!(access, "Libre" | "A péage" | "À péage") && state == "En service" && !closed_in_winter
}

/// Road points (Lambert-93, at most 20 m apart) near a track.
pub fn road_points(track: &[LonLat], dl: &Downloader) -> Result<Vec<(f64, f64)>> {
    let mut out = Vec::new();
    for b in boxes(track) {
        let mut start = 0;
        loop {
            let url = format!(
                "{WFS_URL}?SERVICE=WFS&VERSION=2.0.0&REQUEST=GetFeature&TYPENAMES={LAYER}\
                 &OUTPUTFORMAT=application/json&COUNT={PAGE}&STARTINDEX={start}&CQL_FILTER={}",
                encode(&cql(b))
            );
            let req = Request::get(SOURCE, &url)
                .max_age(std::time::Duration::from_secs(CACHE_DAYS * 86_400));
            // The WFS sometimes answers 200 with an error page (15/11/2026): read again
            // once, bypassing the cached body.
            let v: serde_json::Value = match dl.fetch(&req).and_then(|f| f.json()) {
                Ok(v) => v,
                Err(_) => dl
                    .fetch(&req.cache(CachePolicy::Reload))
                    .and_then(|f| f.json())
                    .map_err(|e| Error::Data(format!("BD TOPO roads: {e}")))?,
            };
            let feats = v["features"]
                .as_array()
                .ok_or_else(|| Error::Data("BD TOPO roads: no features".into()))?;
            for f in feats {
                if !usable(&f["properties"]) {
                    continue;
                }
                let lines: Vec<&serde_json::Value> = match f["geometry"]["type"].as_str() {
                    Some("LineString") => vec![&f["geometry"]["coordinates"]],
                    Some("MultiLineString") => f["geometry"]["coordinates"]
                        .as_array()
                        .map(|a| a.iter().collect())
                        .unwrap_or_default(),
                    _ => Vec::new(),
                };
                for l in lines {
                    let geom: Vec<(f64, f64)> = l
                        .as_array()
                        .into_iter()
                        .flatten()
                        .filter_map(|c| {
                            crs::to_lambert93(LonLat::new(c[0].as_f64()?, c[1].as_f64()?)).ok()
                        })
                        .collect();
                    for w in geom.windows(2) {
                        let ((ax, ay), (bx, by)) = (w[0], w[1]);
                        let k = ((bx - ax).hypot(by - ay) / DENSIFY_M).ceil().max(1.0) as usize;
                        for i in 0..k {
                            let t = i as f64 / k as f64;
                            out.push((ax + t * (bx - ax), ay + t * (by - ay)));
                        }
                    }
                    out.extend(geom.last());
                }
            }
            let matched = v["numberMatched"].as_u64().unwrap_or(0) as usize;
            start += feats.len();
            if feats.is_empty() || start >= matched {
                break;
            }
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn boxes_cut_the_track_and_add_a_margin() {
        let t: Vec<LonLat> = (0..=120)
            .map(|i| LonLat::new(6.0 + f64::from(i) * 0.001, 46.5))
            .collect();
        // 120 × 76.6 m ≈ 9.2 km: two pieces.
        let b = boxes(&t);
        assert_eq!(b.len(), 2);
        assert!((b[0].1 - (6.0 - MARGIN_DEG)).abs() < 1e-9);
        assert!(cql(b[0]).contains("BBOX(geometrie,5.99800,46.49800,"));
    }

    #[test]
    fn winter_closures_are_not_exits() {
        let open = serde_json::json!({"acces_vehicule_leger": "Libre", "etat_de_l_objet": "En service", "periode_de_fermeture": null});
        let winter = serde_json::json!({"acces_vehicule_leger": "Libre", "etat_de_l_objet": "En service", "periode_de_fermeture": "[{\"cycle\": \"Hiver\"}]"});
        let private = serde_json::json!({"acces_vehicule_leger": "Restreint aux ayants droit"});
        assert!(usable(&open));
        assert!(!usable(&winter));
        assert!(!usable(&private));
        assert_eq!(encode("a b'é"), "a%20b%27%C3%A9");
    }
}
