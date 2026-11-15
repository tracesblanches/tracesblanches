//! Colour segmentation of the piste lines: each pixel of the map area takes the nearest
//! ink of the legend (CIELAB distance under the ink's threshold), masks are closed (the
//! white arrows and dashes of the lines are bridged) and connected components that are not
//! lines (text, icons, legend dashes) are dropped.

use crate::image::{Rgb, lab};
use crate::skeleton::{SkelGraph, thin, trace};
use crate::spec::PlanSpec;

/// Label of a pixel of no ink.
pub const NONE: u8 = 255;

/// Binary mask.
#[derive(Debug, Clone, PartialEq)]
pub struct Mask {
    /// Width.
    pub w: usize,
    /// Height.
    pub h: usize,
    /// One byte per pixel, 0 or 1.
    pub v: Vec<u8>,
}

impl Mask {
    /// Empty mask.
    pub fn new(w: usize, h: usize) -> Mask {
        Mask {
            w,
            h,
            v: vec![0; w * h],
        }
    }

    /// Value at (x, y), 0 outside.
    pub fn at(&self, x: i64, y: i64) -> u8 {
        if x < 0 || y < 0 || x as usize >= self.w || y as usize >= self.h {
            0
        } else {
            self.v[y as usize * self.w + x as usize]
        }
    }

    /// Number of set pixels.
    pub fn count(&self) -> usize {
        self.v.iter().filter(|&&b| b != 0).count()
    }

    fn morph(&self, r: i64, dilate: bool) -> Mask {
        // Square structuring element, separable: rows then columns.
        let (w, h) = (self.w, self.h);
        let pass = |src: &[u8], horizontal: bool| -> Vec<u8> {
            let mut out = vec![0u8; w * h];
            for y in 0..h {
                for x in 0..w {
                    let mut acc = !dilate;
                    for d in -r..=r {
                        let (xx, yy) = if horizontal {
                            (x as i64 + d, y as i64)
                        } else {
                            (x as i64, y as i64 + d)
                        };
                        let v = if xx < 0 || yy < 0 || xx as usize >= w || yy as usize >= h {
                            0
                        } else {
                            src[yy as usize * w + xx as usize]
                        };
                        if dilate && v != 0 {
                            acc = true;
                            break;
                        }
                        if !dilate && v == 0 {
                            acc = false;
                            break;
                        }
                    }
                    out[y * w + x] = u8::from(acc);
                }
            }
            out
        };
        let a = pass(&self.v, true);
        Mask {
            w,
            h,
            v: pass(&a, false),
        }
    }

    /// Morphological closing with a `(2r+1)²` square.
    pub fn close(&self, r: i64) -> Mask {
        self.morph(r, true).morph(r, false)
    }

    /// Fills the holes (4-connected background regions not touching the border) of at
    /// most `max_area` pixels: the white arrows drawn inside the lines.
    pub fn fill_holes(&self, max_area: usize) -> Mask {
        let (w, h) = (self.w, self.h);
        let mut out = self.clone();
        let mut seen = vec![false; w * h];
        let mut stack = Vec::new();
        let mut region = Vec::new();
        for start in 0..w * h {
            if self.v[start] != 0 || seen[start] {
                continue;
            }
            region.clear();
            let mut border = false;
            seen[start] = true;
            stack.push(start);
            while let Some(p) = stack.pop() {
                region.push(p);
                let (x, y) = (p % w, p / w);
                if x == 0 || y == 0 || x == w - 1 || y == h - 1 {
                    border = true;
                }
                let mut push = |q: usize| {
                    if self.v[q] == 0 && !seen[q] {
                        seen[q] = true;
                        stack.push(q);
                    }
                };
                if x > 0 {
                    push(p - 1);
                }
                if x + 1 < w {
                    push(p + 1);
                }
                if y > 0 {
                    push(p - w);
                }
                if y + 1 < h {
                    push(p + w);
                }
            }
            if !border && region.len() <= max_area {
                for &p in &region {
                    out.v[p] = 1;
                }
            }
        }
        out
    }

    /// Union with another mask.
    pub fn or(&mut self, other: &Mask) {
        for (a, b) in self.v.iter_mut().zip(&other.v) {
            *a |= *b;
        }
    }
}

/// Per-pixel ink index (into `spec.inks`), [`NONE`] elsewhere or off the map.
pub fn classify(spec: &PlanSpec, img: &Rgb, pt_per_px: f64) -> Vec<u8> {
    let refs: Vec<(usize, [f32; 3])> = spec
        .inks
        .iter()
        .enumerate()
        .flat_map(|(i, ink)| ink.refs.iter().map(move |&c| (i, lab(c))))
        .collect();
    // Bands of rows in parallel, each with its colour cache (pure per pixel: the result
    // does not depend on the split).
    use rayon::prelude::*;
    let mut out = vec![NONE; img.w * img.h];
    let band = 64;
    out.par_chunks_mut(img.w * band)
        .enumerate()
        .for_each(|(b, chunk)| {
            let mut cache = std::collections::HashMap::<[u8; 3], u8>::new();
            for (dy, row) in chunk.chunks_mut(img.w).enumerate() {
                let y = b * band + dy;
                let py = (y as f64 + 0.5) * pt_per_px;
                for (x, o) in row.iter_mut().enumerate() {
                    if !spec.on_map((x as f64 + 0.5) * pt_per_px, py) {
                        continue;
                    }
                    let c = img.get(x, y);
                    *o = *cache.entry(c).or_insert_with(|| {
                        let l = lab(c);
                        let mut best = (NONE, f32::MAX);
                        for &(i, r) in &refs {
                            let d = ((l[0] - r[0]).powi(2)
                                + (l[1] - r[1]).powi(2)
                                + (l[2] - r[2]).powi(2))
                            .sqrt();
                            if d <= spec.inks[i].max_de && d < best.1 {
                                best = (i as u8, d);
                            }
                        }
                        best.0
                    });
                }
            }
        });
    out
}

/// Mask of one ink.
pub fn ink_mask(labels: &[u8], w: usize, h: usize, ink: u8) -> Mask {
    Mask {
        w,
        h,
        v: labels.iter().map(|&l| u8::from(l == ink)).collect(),
    }
}

/// 8-connected components: label per pixel (0 = background) and pixel count per label.
pub fn components(m: &Mask) -> (Vec<u32>, Vec<usize>) {
    let mut lab = vec![0u32; m.w * m.h];
    let mut sizes = vec![0usize];
    let mut stack = Vec::new();
    for start in 0..m.v.len() {
        if m.v[start] == 0 || lab[start] != 0 {
            continue;
        }
        let id = sizes.len() as u32;
        let mut n = 0;
        lab[start] = id;
        stack.push(start);
        while let Some(p) = stack.pop() {
            n += 1;
            let (x, y) = ((p % m.w) as i64, (p / m.w) as i64);
            for dy in -1..=1 {
                for dx in -1..=1 {
                    let (xx, yy) = (x + dx, y + dy);
                    if m.at(xx, yy) != 0 {
                        let q = yy as usize * m.w + xx as usize;
                        if lab[q] == 0 {
                            lab[q] = id;
                            stack.push(q);
                        }
                    }
                }
            }
        }
        sizes.push(n);
    }
    (lab, sizes)
}

/// Parameters of the line extraction, in pixels of the rendering.
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize)]
pub struct LineParams {
    /// Closing radius.
    pub close_r: i64,
    /// Smallest skeleton length kept for a component.
    pub min_len_px: f64,
    /// Largest mean width (pixels / skeleton length) of a line component.
    pub max_width_px: f64,
    /// Smallest mean width of a line component (text strokes are thinner).
    pub min_width_px: f64,
    /// Largest hole filled inside a line (arrows), pixels.
    pub max_hole_px: usize,
    /// Spurs shorter than this are pruned from the skeleton.
    pub spur_px: f64,
}

/// Lines of one ink: closed mask, filtered components, skeleton graph.
pub fn ink_lines(labels: &[u8], w: usize, h: usize, ink: u8, p: &LineParams) -> (Mask, SkelGraph) {
    let mask = ink_mask(labels, w, h, ink)
        .close(p.close_r)
        .fill_holes(p.max_hole_px);
    lines_of_mask(&mask, p)
}

/// Lines of a closed mask: components filtered by length and width, skeleton graph.
pub fn lines_of_mask(mask: &Mask, p: &LineParams) -> (Mask, SkelGraph) {
    let (w, h) = (mask.w, mask.h);
    let skel = thin(mask);
    let (comp, sizes) = components(mask);
    // Skeleton length per component.
    let mut len = vec![0usize; sizes.len()];
    for (i, &s) in skel.v.iter().enumerate() {
        if s != 0 {
            len[comp[i] as usize] += 1;
        }
    }
    let keep: Vec<bool> = sizes
        .iter()
        .zip(&len)
        .map(|(&area, &l)| {
            let width = area as f64 / (l.max(1) as f64);
            l as f64 >= p.min_len_px && width <= p.max_width_px && width >= p.min_width_px
        })
        .collect();
    let mut kept = Mask::new(w, h);
    let mut kept_skel = Mask::new(w, h);
    for i in 0..mask.v.len() {
        if mask.v[i] != 0 && keep[comp[i] as usize] {
            kept.v[i] = 1;
            kept_skel.v[i] = skel.v[i];
        }
    }
    let g = trace(&kept_skel).pruned(p.spur_px);
    (kept, g)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn closing_bridges_a_gap_and_components_are_counted() {
        let mut m = Mask::new(20, 5);
        for x in 0..20 {
            if x != 9 && x != 10 {
                m.v[2 * 20 + x] = 1;
            }
        }
        assert_eq!(components(&m).1.len() - 1, 2);
        let c = m.close(1);
        assert_eq!(c.v[2 * 20 + 9], 1);
        assert_eq!(components(&c).1.len() - 1, 1);
    }
}
