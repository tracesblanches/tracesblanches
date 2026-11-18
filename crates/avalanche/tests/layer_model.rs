//! The layer path of the exposure model ([`exposure::layer_model`], #173) keeps only the
//! bits of the PRA and runouts that the classifier reads: its classes and runout reach
//! must be those of the whole model, bit for bit.

use skimap_avalanche::exposure::{self, Inputs, raster::Raster};

#[test]
fn layer_model_matches_the_whole_model() {
    // Steps falling south (release zones above benches), corrugated across, a forest
    // band every third step: every vote, floor and class-0 term is exercised.
    let (w, h, cell) = (96usize, 768usize, 5.0f64);
    let (steep, flat) = (40f64.to_radians().tan(), 3f64.to_radians().tan());
    let mut row_z = vec![2500.0f64; h];
    for r in 1..h {
        let t = if r % 128 < 24 { steep } else { flat };
        row_z[r] = row_z[r - 1] - cell * t;
    }
    let dem: Vec<f32> = (0..w * h)
        .map(|i| {
            let (r, c) = (i / w, (i % w) as f64);
            (row_z[r] + 6.0 * (c / 9.0).sin()) as f32
        })
        .collect();
    let canopy: Vec<f32> = (0..w * h)
        .map(|i| match (i / w / 100) % 3 {
            2 => 70.0,
            1 => 25.0,
            _ => 5.0,
        })
        .collect();
    let raster = |data| Raster {
        w,
        h,
        cell,
        origin_x: 900_000.0,
        origin_y: 6_450_000.0,
        data,
    };
    let inputs = Inputs {
        dem: raster(dem),
        canopy: raster(canopy),
    };
    let whole = exposure::model(&inputs).layer();
    let layer = exposure::layer_model(&inputs);
    assert!(whole.paths.1 > 1000, "the grid must route paths");
    assert_eq!(layer.paths, whole.paths);
    assert_eq!(layer.ates_classes.data, whole.ates_classes.data);
    assert_eq!(layer.exposure.data, whole.exposure.data);
    assert_eq!(layer.reached_extreme, whole.reached_extreme);
    assert_eq!(layer.reached_frequent, whole.reached_frequent);
    let classes: std::collections::BTreeSet<u8> = layer.ates_classes.data.iter().copied().collect();
    assert!(classes.len() >= 4, "ATES classes {classes:?}");
}
