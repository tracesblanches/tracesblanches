//! The Rust model against the AutoATES v3.0 reference on the ISSW 2026 workshop data
//! (Connaught Creek, ALOS 21.34 m, Sentinel-2 canopy): an independent check of the port
//! on a site, a resolution and inputs that are not ours.
//!
//! The workshop data are not redistributable (ALOS, Sentinel-2: workshop and fair-use
//! research only) and are not in the repository. Run the workshop pipeline once with the
//! oracle environment (`scripts/run_pipeline.py` of
//! `AutoATES/issw2026-autoates-workshop`), then:
//!
//! ```sh
//! SKIMAP_AUTOATES_WORKSHOP_RUN=…/issw2026-autoates-workshop/outputs/connaught \
//!     cargo test --release -p skimap-avalanche --test oracle_connaught -- --ignored --nocapture
//! ```

use std::path::PathBuf;

use skimap_avalanche::exposure::{self, Inputs, oracle, raster};

#[test]
#[ignore = "needs the AutoATES workshop run (SKIMAP_AUTOATES_WORKSHOP_RUN)"]
fn connaught_matches_reference() {
    let dir = PathBuf::from(
        std::env::var("SKIMAP_AUTOATES_WORKSHOP_RUN")
            .expect("set SKIMAP_AUTOATES_WORKSHOP_RUN to outputs/connaught"),
    );
    let read = |f: &str| raster::read(&dir.join("PRA").join(f)).unwrap();
    let dem = read("autoATES_v3_DSM_00.tif");
    // The workshop canopy grid is offset by about 1 m from the DSM grid (AOI crop): the
    // reference PRA and runout use the two arrays cell for cell, so do we. (Its ATES
    // step resamples the canopy bilinearly onto the DSM grid; we do not: small canopy
    // differences in the ATES votes are expected here, not on our own grids, which
    // coincide.)
    let canopy = dem.with_data(read("autoATES_v3_FOREST_CANOPY_00.tif").data);
    let inputs = Inputs { dem, canopy };
    let m = exposure::model_autoates(&inputs);
    let run = oracle::OracleRun::open(&dir).unwrap();
    let rep = oracle::compare(&m, &exposure::terrain(&inputs.dem), &run, false).unwrap();
    for l in &rep.layers {
        println!(
            "{:<40} identical {:.6} within {:.0e} {:.6} max {:.3e}",
            l.layer, l.identical, l.tolerance, l.within_tolerance, l.max_abs_diff
        );
    }
    for c in &rep.classes {
        println!(
            "{:<50} agreement {:.6} kappa {:.4}",
            c.layer, c.agreement, c.kappa
        );
    }
    let get = |name: &str| rep.classes.iter().find(|c| c.layer == name).unwrap();
    assert_eq!(get("frequent PRA release (sieve)").agreement, 1.0);
    assert_eq!(get("extreme PRA release (sieve)").agreement, 1.0);
    assert!(get("ATES classes (final)").agreement > 0.999);
    assert!(get("exposure (3 classes, same rule on both sides)").agreement > 0.999);
}
