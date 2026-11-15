//! Relief threshold of the avalanche exposure step of `build-all`: the exposure model
//! (P16 to P18) runs only where an avalanche can start.
//!
//! Rule: the `terrain` layer has at least one 4-connected patch of slope ≥ 30° of at least
//! 1000 m².
//!
//! - 30°: slab avalanches start on slopes of about 30° and more; it is also the « pente
//!   ≥ 30° » share the terrain layer and research report 07 already publish. The model's
//!   typical scenario (AutoATES v3.0 release membership, Cauchy bell centred on 42.5°,
//!   threshold 0.25, `pra = (1 - m) m + m · mean`) needs at least 27.8° even on a cell
//!   whose wind-shelter and forest memberships are 1.
//! - 1000 m²: the sieve of the model (AutoATES `pra` sieve: smaller release areas are
//!   removed), 40 cells of 5 m.
//!
//! Below it the typical scenario of the model has hardly anything to release, and the
//! layer would cost the LiDAR HD canopy download of the whole site. Limit: the rare
//! scenario (α = 18°, release membership down to about 18° with threshold 0.05) is then
//! not computed either.

use anyhow::{Context, Result};
use skimap_common::layout::DataLayout;
use skimap_common::site::Site;

/// Slope threshold, degrees.
pub const RELEASE_SLOPE_DEG: f32 = 30.0;
/// Smallest patch, m² (AutoATES PRA sieve).
pub const MIN_PATCH_M2: f64 = 1000.0;

/// Steep terrain of a site.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Relief {
    /// Area of slope ≥ 30°, m².
    pub steep_m2: f64,
    /// Largest 4-connected patch of slope ≥ 30°, m².
    pub largest_patch_m2: f64,
    /// Patches of at least [`MIN_PATCH_M2`].
    pub patches: usize,
}

impl Relief {
    /// Whether the exposure model is run.
    pub fn avalanche_terrain(&self) -> bool {
        self.largest_patch_m2 >= MIN_PATCH_M2
    }

    /// French summary.
    pub fn describe(&self) -> String {
        format!(
            "pente ≥ {RELEASE_SLOPE_DEG}° sur {:.2} km², plus grande plaque {:.0} m², {} plaques ≥ {MIN_PATCH_M2:.0} m²",
            self.steep_m2 / 1e6,
            self.largest_patch_m2,
            self.patches
        )
    }
}

/// Steep patches of a slope raster (row-major, `NaN` no data), cell side `cell` metres.
pub fn steep_patches(slope: &[f32], width: usize, height: usize, cell: f64) -> Relief {
    let cell_m2 = cell * cell;
    let steep: Vec<bool> = slope.iter().map(|v| *v >= RELEASE_SLOPE_DEG).collect();
    let mut seen = vec![false; steep.len()];
    let mut stack = Vec::new();
    let (mut total, mut largest, mut patches) = (0usize, 0usize, 0usize);
    let min_cells = (MIN_PATCH_M2 / cell_m2).ceil() as usize;
    for start in 0..steep.len() {
        if !steep[start] || seen[start] {
            continue;
        }
        let mut n = 0usize;
        seen[start] = true;
        stack.push(start);
        while let Some(i) = stack.pop() {
            n += 1;
            let (x, y) = (i % width, i / width);
            let mut visit = |j: usize| {
                if steep[j] && !seen[j] {
                    seen[j] = true;
                    stack.push(j);
                }
            };
            if x > 0 {
                visit(i - 1);
            }
            if x + 1 < width {
                visit(i + 1);
            }
            if y > 0 {
                visit(i - width);
            }
            if y + 1 < height {
                visit(i + width);
            }
        }
        total += n;
        largest = largest.max(n);
        if n >= min_cells {
            patches += 1;
        }
    }
    Relief {
        steep_m2: total as f64 * cell_m2,
        largest_patch_m2: largest as f64 * cell_m2,
        patches,
    }
}

/// Steep relief of a site from its `terrain/slope_5m.tif`.
pub fn steep_relief(layout: &DataLayout, site: &Site) -> Result<Relief> {
    let path = layout
        .layer_dir(site, skimap_terrain::LAYER)?
        .join("slope_5m.tif");
    let g = skimap_common::geotiff::read_f32(&path)
        .with_context(|| format!("pente du site : {}", path.display()))?;
    Ok(steep_patches(
        g.data(),
        g.width(),
        g.height(),
        g.transform().pixel_width.abs(),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn patches_are_four_connected() {
        // 10 x 10 cells of 5 m; a 7 x 6 block of 35° (42 cells, 1050 m²) and two cells
        // touching only by a corner.
        let (w, h) = (10, 10);
        let mut s = vec![10.0_f32; w * h];
        for y in 0..6 {
            for x in 0..7 {
                s[y * w + x] = 35.0;
            }
        }
        s[8 * w + 8] = 31.0;
        s[9 * w + 9] = 31.0;
        s[9 * w] = f32::NAN;
        let r = steep_patches(&s, w, h, 5.0);
        assert_eq!(r.largest_patch_m2, 1050.0);
        assert_eq!(r.steep_m2, 1100.0);
        assert_eq!(r.patches, 1);
        assert!(r.avalanche_terrain());
        // 39 cells (975 m²): below the sieve.
        let mut s = vec![0.0_f32; w * h];
        for v in s.iter_mut().take(39) {
            *v = 40.0;
        }
        let r = steep_patches(&s, w, h, 5.0);
        assert_eq!(r.largest_patch_m2, 975.0);
        assert!(!r.avalanche_terrain());
        assert!(r.describe().contains("975 m²"));
    }
}
