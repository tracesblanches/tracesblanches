//! Property tests of the projection of the snowpack simulation onto a pixel (Q3, #103):
//! the depth is a multilinear interpolation of the band table (exact at the nodes, within
//! the range of the values it interpolates, continuous across north), and the range
//! brackets every depth of its stencil.

use proptest::prelude::*;
use skimap_snow::classes::PRACTICES;
use skimap_snow::map::estimate_cell;
use skimap_snow::sim::{MassifDay, N_COMBOS, RANGE_ALT_M, RANGE_ASPECT_DEG, RANGE_SLOPE_DEG};

fn day() -> impl Strategy<Value = MassifDay> {
    (1usize..8, 300.0f32..1500.0).prop_flat_map(|(n, base)| {
        prop::collection::vec(prop::array::uniform17(0.0f32..400.0), n).prop_map(move |depth| {
            MassifDay {
                bands: (0..depth.len()).map(|k| base + 300.0 * k as f32).collect(),
                depth,
            }
        })
    })
}

/// Depth of a band at a lattice node (slope 0, 20 or 40°, octant k).
fn node(d: &MassifDay, b: usize, slope: u8, k: usize) -> f32 {
    match slope {
        0 => d.depth[b][0],
        20 => d.depth[b][1 + 2 * k],
        _ => d.depth[b][2 + 2 * k],
    }
}

fn close(a: f32, b: f32) -> bool {
    (a - b).abs() <= 1e-3 * a.abs().max(b.abs()).max(1.0)
}

proptest! {
    /// Exact at every node of the lattice (band altitude, flat / 20° / 40°, octant).
    #[test]
    fn exact_at_the_nodes(d in day(), b in any::<prop::sample::Index>(), k in 0usize..8, slope in prop_oneof![Just(0u8), Just(20u8), Just(40u8)]) {
        let b = b.index(d.bands.len());
        let alt = d.bands[b];
        let aspect = 45.0 * k as f32;
        let v = d.depth(alt, f32::from(slope), aspect);
        prop_assert!(close(v, node(&d, b, slope, k)), "{v} vs {}", node(&d, b, slope, k));
    }

    /// The depth lies between the smallest and largest table values it interpolates (the
    /// two bands around the altitude, all combinations), and is NaN only for a NaN altitude.
    #[test]
    fn within_the_interpolated_values(
        d in day(),
        alt in 0.0f32..4000.0,
        slope in prop_oneof![Just(f32::NAN), -5.0f32..70.0],
        aspect in prop_oneof![Just(f32::NAN), Just(-1.0f32), -720.0f32..720.0],
    ) {
        let v = d.depth(alt, slope, aspect);
        prop_assert!(v.is_finite());
        let i = d.bands.partition_point(|b| *b <= alt);
        let lo_b = i.saturating_sub(1);
        let hi_b = i.min(d.bands.len() - 1);
        let vals: Vec<f32> = [lo_b, hi_b].iter().flat_map(|&b| d.depth[b].iter().copied()).collect();
        let lo = vals.iter().copied().fold(f32::INFINITY, f32::min);
        let hi = vals.iter().copied().fold(f32::NEG_INFINITY, f32::max);
        prop_assert!(v >= lo - 1e-3 && v <= hi + 1e-3, "{v} not in [{lo}, {hi}]");
        prop_assert!(d.depth(f32::NAN, slope, aspect).is_nan());
    }

    /// Continuous: across north (359.99° vs 0°), along the altitude and the slope (no jump
    /// larger than the steepest table gradient allows).
    #[test]
    fn continuous(d in day(), alt in 0.0f32..4000.0, slope in 0.0f32..60.0, aspect in 0.0f32..360.0) {
        let range = d.depth.iter().flatten().fold(0.0f32, |m, v| m.max(*v));
        let north = (d.depth(alt, slope, 359.999) - d.depth(alt, slope, 0.0)).abs();
        prop_assert!(north <= 1e-4 * range + 1e-3, "jump of {north} cm across north");
        let eps = 0.01;
        let da = (d.depth(alt + eps, slope, aspect) - d.depth(alt, slope, aspect)).abs();
        prop_assert!(da <= range / 300.0 * eps * 1.01 + 1e-3, "altitude step {da}");
        let ds = (d.depth(alt, slope + eps, aspect) - d.depth(alt, slope, aspect)).abs();
        prop_assert!(ds <= range / 20.0 * eps * 1.01 + 1e-3, "slope step {ds}");
        // Aspect is periodic.
        prop_assert!(close(d.depth(alt, slope, aspect), d.depth(alt, slope, aspect + 360.0)));
    }

    /// The range brackets the depth at the point and at every point of the stencil
    /// (altitude ± 150 m, slope ± 10° above 0, aspect ± 45°, any aspect for a flat cell).
    /// Terrain cells are flat (aspect −1) only when their gradient is zero
    /// (`skimap_terrain::slope`), so a flat cell has a zero slope.
    #[test]
    fn range_brackets_the_stencil(
        d in day(),
        alt in 0.0f32..4000.0,
        (slope, aspect) in prop_oneof![Just((0.0f32, -1.0f32)), (0.0f32..60.0, 0.0f32..360.0)],
        samples in prop::collection::vec((-1.0f32..=1.0, -1.0f32..=1.0, -1.0f32..=1.0), 1..40),
    ) {
        let (lo, hi) = d.depth_range(alt, slope, aspect);
        let v = d.depth(alt, slope, aspect);
        prop_assert!(lo <= v + 1e-3 && v <= hi + 1e-3, "{v} not in [{lo}, {hi}]");
        for (ua, us, ux) in samples {
            let a = alt + ua * RANGE_ALT_M;
            let s = (slope + us * RANGE_SLOPE_DEG).max(0.0);
            let x = if aspect >= 0.0 { aspect + ux * RANGE_ASPECT_DEG } else { 180.0 + 180.0 * ux };
            let w = d.depth(a, s, x);
            prop_assert!(w >= lo - 1e-3 && w <= hi + 1e-3, "stencil point {w} not in [{lo}, {hi}]");
        }
        let e = estimate_cell(&d, alt, slope, aspect);
        prop_assert!(close(e.depth, v) && close(e.low, lo) && close(e.high, hi));
        // Undefined slope: treated as flat.
        let flat = estimate_cell(&d, alt, f32::NAN, aspect);
        prop_assert!(close(flat.depth, d.depth(alt, 0.0, -1.0)));
    }

    /// Classes grow with the depth, from 0 to 2, and NaN has no class.
    #[test]
    fn classes_are_monotonic(a in 0.0f32..300.0, b in 0.0f32..300.0) {
        let (lo, hi) = if a <= b { (a, b) } else { (b, a) };
        for p in PRACTICES {
            let (cl, ch) = (p.class(lo).expect("class"), p.class(hi).expect("class"));
            prop_assert!(cl <= ch && ch <= 2);
            prop_assert!(p.class(f32::NAN).is_none());
        }
    }
}

#[test]
fn seventeen_combinations() {
    assert_eq!(N_COMBOS, 17);
}
