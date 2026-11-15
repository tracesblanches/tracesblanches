//! Golden fingerprints of the slope and aspect operator (P5) on the 1 km terrain windows
//! of both sites (Q2, #102): histogram of the slope by 5° class, of the aspect by
//! 45° sector, steep share, mean slope.
//!
//! References: `fixtures/golden/terrain/<site>.json`; update with
//! `SKIMAP_BLESS=1 cargo test -p skimap-terrain --test golden` and review the diff.
//!
//! Tolerances: cell counts are exact (Horn on fixed 5 m cells; a class change of a single
//! cell is a change of the operator); the mean slope `Tol::FLOAT`. Research report 07
//! measured 38.1 % of the Cervières mosaic at 30° or more and 2.3 % at Chapelle-des-Bois
//! (`skimap_terrain::report07_reference`, whole mosaics, checked by `skimap terrain`);
//! the windows here were chosen for the widest spread of slopes, not to be typical.

#[path = "../../../fixtures/support/mod.rs"]
mod fixtures;

use fixtures::golden::{Golden, Tol};
use skimap_common::site::SITES;
use skimap_terrain::geotiff;
use skimap_terrain::slope::{FLAT_ASPECT, horn};

const SECTORS: [&str; 8] = ["N", "NE", "E", "SE", "S", "SW", "W", "NW"];

#[test]
fn slope_and_aspect_histograms() {
    for site in SITES {
        let dem =
            geotiff::read_f32(&fixtures::input(&format!("{}/dem_window_5m.tif", site.id))).unwrap();
        let sa = horn(&dem);
        let mut slope = [0u64; 18];
        let mut aspect = [0u64; 8];
        let (mut flat, mut steep30, mut valid, mut sum) = (0u64, 0u64, 0u64, 0.0f64);
        for (s, a) in sa.slope.data().iter().zip(sa.aspect.data()) {
            if !s.is_finite() {
                continue;
            }
            valid += 1;
            sum += f64::from(*s);
            slope[((*s / 5.0) as usize).min(17)] += 1;
            steep30 += u64::from(*s >= 30.0);
            if *a == FLAT_ASPECT {
                flat += 1;
            } else {
                aspect[(((*a + 22.5) / 45.0) as usize) % 8] += 1;
            }
        }
        let mut k = Golden::new(&format!("terrain/{}", site.id));
        k.count("cells_valid", valid)
            .count("cells_slope_ge_30", steep30)
            .count("cells_flat", flat)
            .num("slope_mean_deg", sum / valid as f64, Tol::FLOAT);
        for (i, n) in slope.iter().enumerate() {
            k.count(&format!("slope_{:02}_{:02}_deg", i * 5, i * 5 + 5), *n);
        }
        for (name, n) in SECTORS.iter().zip(aspect) {
            k.count(&format!("aspect_{name}"), n);
        }
        k.check();
    }
}
