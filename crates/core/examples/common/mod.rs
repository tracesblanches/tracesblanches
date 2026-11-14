//! Shared by the proof examples: real place pairs per site, seeded generator.

// Each example uses a subset of this module.
#![allow(dead_code)]

use skimap_common::geom::LonLat;

/// A named place.
#[derive(Debug, Clone, Copy)]
pub struct Place {
    /// Label.
    pub label: &'static str,
    /// Position (WGS84).
    pub pos: LonLat,
    /// Name in `refuges/refuges_info.geojson` (P11), whose `alt` checks the terrain model.
    pub refuge: Option<&'static str>,
}

const fn place(label: &'static str, lon: f64, lat: f64, refuge: Option<&'static str>) -> Place {
    Place {
        label,
        pos: LonLat::new(lon, lat),
        refuge,
    }
}

/// Real place pairs. Sources: refuges layer (P11: refuges.info) for huts and fountains,
/// `common::site` for the commune centre, OSM piste ends for Le Laus (the « Liaison Le
/// Laus - Les Fonts » and the Le Cancel trails end 18 m apart, more than the 10 m
/// connectors of P2: two components).
pub fn pairs(site: &str) -> Vec<(Place, Place)> {
    let fonts = place(
        "Refuge des Fonts de Cervières",
        6.8157,
        44.84465,
        Some("Refuge des Fonts de Cervières"),
    );
    let gaillard = place(
        "Chalet Gaillard",
        6.12167,
        46.56318,
        Some("Chalet Gaillard"),
    );
    match site {
        "cervieres" => vec![
            (
                place(
                    "Le Laus (départ de la « Liaison Le Laus - Les Fonts »)",
                    6.725_006,
                    44.858_940,
                    None,
                ),
                fonts,
            ),
            (
                place("Le Laus (pistes du Cancel)", 6.725_143, 44.859_064, None),
                place(
                    "Brunissard, fontaine du camping (par le col d'Izoard)",
                    6.7172,
                    44.7994,
                    Some("Fontaine de Brunissard (entrée camping Izoard)"),
                ),
            ),
        ],
        "chapelle-des-bois" => vec![
            (
                place(
                    "Chapelle-des-Bois (centre de la commune)",
                    6.1118,
                    46.5952,
                    None,
                ),
                gaillard,
            ),
            (
                gaillard,
                place(
                    "Refuge des Prés d'Haut",
                    6.11442,
                    46.64122,
                    Some("Refuge des Prés d'Haut"),
                ),
            ),
            (
                place(
                    "Chalet des Ministres",
                    6.09467,
                    46.54512,
                    Some("Chalet des Ministres"),
                ),
                place(
                    "Abri de la Citadelle",
                    6.18374,
                    46.63679,
                    Some("Abri de la Citadelle"),
                ),
            ),
        ],
        _ => Vec::new(),
    }
}

/// SplitMix64 (deterministic, seeded).
pub struct SplitMix(pub u64);

impl SplitMix {
    /// Next 64-bit value.
    pub fn next_u64(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    /// Uniform in [0, 1).
    pub fn unit(&mut self) -> f64 {
        (self.next_u64() >> 11) as f64 / (1u64 << 53) as f64
    }
}
