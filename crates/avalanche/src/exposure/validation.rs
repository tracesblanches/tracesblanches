//! Validation against known avalanches (P18, #31): the CLPA footprints republished by the
//! DDT 05 (module [`crate::historic`]).
//!
//! Target: no CLPA cell and no metre of piste inside a CLPA footprint classed « terrain peu
//! exposé (modèle) ». Measured on the cells whose centre is inside a footprint, and on
//! the piste sections inside a footprint (same sections as `historic`).

use serde::Serialize;

use super::raster::Raster;
use super::segments::{self, Run};
use super::{Model, class};
use crate::error::Result;
use crate::historic::pistes::PisteLine;
use crate::historic::{
    self,
    clip::{Poly, inside_runs},
};
use skimap_common::crs;
use skimap_common::download::Downloader;
use skimap_common::meta::SourceRef;
use skimap_common::site::Site;

/// Cells of `like` whose centre is inside any polygon (Lambert-93).
pub fn rasterize<T: Clone + Sync>(like: &Raster<T>, polys: &[&Poly]) -> Vec<bool> {
    use rayon::prelude::*;
    let cells: Vec<Vec<usize>> = polys
        .par_iter()
        .map(|p| {
            let (x0, y0, x1, y1) = p.bbox;
            let c0 = ((x0 - like.origin_x) / like.cell).floor().max(0.0) as usize;
            let c1 = (((x1 - like.origin_x) / like.cell).ceil().max(0.0) as usize).min(like.w);
            let r0 = ((like.origin_y - y1) / like.cell).floor().max(0.0) as usize;
            let r1 = (((like.origin_y - y0) / like.cell).ceil().max(0.0) as usize).min(like.h);
            let mut v = Vec::new();
            for r in r0..r1 {
                for c in c0..c1 {
                    if p.contains(like.center(c, r)) {
                        v.push(r * like.w + c);
                    }
                }
            }
            v
        })
        .collect();
    let mut m = vec![false; like.w * like.h];
    for i in cells.into_iter().flatten() {
        m[i] = true;
    }
    m
}

/// Shares of the footprint area per class.
#[derive(Debug, Clone, Serialize)]
pub struct AreaShares {
    /// Footprint cells with a class.
    pub cells: u64,
    /// Area, km².
    pub area_km2: f64,
    /// Footprint cells without data (outside the LiDAR HD, raster edge).
    pub cells_no_data: u64,
    /// Share « terrain peu exposé (modèle) » (target 0).
    pub low: f64,
    /// Share « exposé ».
    pub exposed: f64,
    /// Share « très exposé ».
    pub high: f64,
    /// Recall of the exposed classes: « exposé » + « très exposé ».
    pub recall_exposed: f64,
}

/// Lengths of the piste sections inside the footprints, per class.
#[derive(Debug, Clone, Serialize)]
pub struct LengthShares {
    /// Length inside the footprints, km.
    pub inside_km: f64,
    /// « terrain peu exposé (modèle) », km (target 0).
    pub low_km: f64,
    /// « exposé », km.
    pub exposed_km: f64,
    /// « très exposé », km.
    pub high_km: f64,
    /// Without data, km.
    pub no_data_km: f64,
}

/// CLPA validation of one site.
#[derive(Debug, Clone, Serialize)]
pub struct ClpaValidation {
    /// CLPA footprints used (intersecting the site box or in the commune).
    pub footprints: usize,
    /// Footprint area by exposure class.
    pub area: AreaShares,
    /// Footprint share of each ATES-like class 0–4 (dev8 port, identical to the
    /// reference on Cervières): shows how much known avalanche terrain the reference
    /// class 0 would cover.
    pub ates_shares: [f64; 5],
    /// Share of the footprint cells reached by the infrequent runout (α = 18°).
    pub reached_extreme: f64,
    /// Share reached by the typical runout (α = 30°).
    pub reached_frequent: f64,
    /// Nordic piste sections inside a footprint (commune rule of `historic`).
    pub nordic_crossings: LengthShares,
    /// Same for the whole network of the site layer (all lines, all networks).
    pub all_crossings: LengthShares,
}

fn lengths(runs: &[Run]) -> LengthShares {
    let by = segments::length_by_class(runs);
    LengthShares {
        inside_km: by.iter().sum::<f64>() / 1000.0,
        low_km: by[1] / 1000.0,
        exposed_km: by[2] / 1000.0,
        high_km: by[3] / 1000.0,
        no_data_km: by[0] / 1000.0,
    }
}

/// Exposure runs of the sections of `lines` inside `polys`.
pub fn crossing_runs(
    lines: &[&PisteLine],
    polys: &[Poly],
    classes: &Raster<u8>,
) -> Result<Vec<Run>> {
    let mut out = Vec::new();
    for l in lines {
        let mut pts = Vec::with_capacity(l.coords.len());
        for p in &l.coords {
            pts.push(crs::to_lambert93(*p)?);
        }
        for r in inside_runs(&pts, polys) {
            out.extend(segments::runs(&r.coords, classes));
        }
    }
    Ok(out)
}

/// CLPA footprints of a site (DDT 05 layer, records whose source contains `CLPA`,
/// intersecting the site box or in the commune), with their source; `None` outside the
/// Hautes-Alpes (no CLPA in the Jura).
pub fn clpa_footprints(site: &Site, dl: &Downloader) -> Result<Option<(Vec<Poly>, SourceRef)>> {
    if site.department != historic::DDT05_DEPARTMENT {
        return Ok(None);
    }
    let zip = dl.fetch(&historic::ddt05_request())?;
    let src = SourceRef::from_fetched(
        "DDT 05, aléa avalanche dans les Hautes-Alpes (CLPA republiée)",
        "Licence Ouverte 2.0 (droit de republication de la CLPA par la DDT non vérifié)",
        "DDT des Hautes-Alpes ; CLPA INRAE (ex-Cemagref)",
        &zip,
    );
    let emprises = historic::read_ddt05_zip(&zip.bytes()?)?;
    let (bx0, by0, bx1, by1) = crs::project_bbox(site.bbox, crs::Crs::Lambert93, 16)?;
    let polys = emprises
        .iter()
        .filter(|e| e.is_clpa())
        .filter(|e| {
            let b = e.poly.bbox;
            (b.0 <= bx1 && bx0 <= b.2 && b.1 <= by1 && by0 <= b.3)
                || e.attrs.get("INSEE").map(String::as_str) == Some(site.insee)
        })
        .map(|e| e.poly.clone())
        .collect();
    Ok(Some((polys, src)))
}

/// Raises every valid cell inside a CLPA footprint (`inside`, from [`rasterize`] on the
/// same grid) to at least « exposé »: a known avalanche footprint is never shown as
/// « terrain peu exposé (modèle) ». Returns the number of cells raised.
pub fn fuse_clpa(exposure: &mut Raster<u8>, inside: &[bool]) -> u64 {
    let mut n = 0;
    for (v, i) in exposure.data.iter_mut().zip(inside) {
        if *i && *v == class::LOW {
            *v = class::EXPOSED;
            n += 1;
        }
    }
    n
}

/// Validates exposure classes (`exposure`, from `model` or after [`fuse_clpa`]) against
/// CLPA footprints (`inside`: their cells, from [`rasterize`] on the same grid, computed
/// once for both validations and the fusion). `nordic` are the nordic lines of the site
/// (vertex mean in the commune, the rule of `historic` and report 07), `all` every line
/// of the pistes layer.
pub fn validate(
    model: &Model,
    exposure: &Raster<u8>,
    clpa: &[Poly],
    inside: &[bool],
    nordic: &[&PisteLine],
    all: &[&PisteLine],
) -> Result<ClpaValidation> {
    let ex = &exposure.data;
    let (mut n, mut nd, mut low, mut exp, mut high) = (0u64, 0u64, 0u64, 0u64, 0u64);
    let mut ates = [0u64; 5];
    let (mut re, mut rf) = (0u64, 0u64);
    for i in 0..ex.len() {
        if !inside[i] {
            continue;
        }
        match ex[i] {
            class::LOW => low += 1,
            class::EXPOSED => exp += 1,
            class::HIGH => high += 1,
            _ => {
                nd += 1;
                continue;
            }
        }
        n += 1;
        let a = model.ates.classes.data[i];
        if a < 5 {
            ates[a as usize] += 1;
        }
        re += u64::from(model.runout_extreme.reached(i));
        rf += u64::from(model.runout_frequent.reached(i));
    }
    let share = |x: u64| if n == 0 { 0.0 } else { x as f64 / n as f64 };
    let cell_area = exposure.cell * exposure.cell;
    Ok(ClpaValidation {
        footprints: clpa.len(),
        area: AreaShares {
            cells: n,
            area_km2: n as f64 * cell_area / 1e6,
            cells_no_data: nd,
            low: share(low),
            exposed: share(exp),
            high: share(high),
            recall_exposed: share(exp + high),
        },
        ates_shares: ates.map(share),
        reached_extreme: share(re),
        reached_frequent: share(rf),
        nordic_crossings: lengths(&crossing_runs(nordic, clpa, exposure)?),
        all_crossings: lengths(&crossing_runs(all, clpa, exposure)?),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rasterize_uses_cell_centres() {
        let like = Raster {
            w: 4,
            h: 4,
            cell: 10.0,
            origin_x: 0.0,
            origin_y: 40.0,
            data: vec![0u8; 16],
        };
        // Square 0..20 × 20..40: the four north-west cells.
        let p = Poly::new(vec![vec![
            (0.0, 20.0),
            (20.0, 20.0),
            (20.0, 40.0),
            (0.0, 40.0),
        ]]);
        let m = rasterize(&like, &[&p]);
        let idx: Vec<usize> = (0..16).filter(|i| m[*i]).collect();
        assert_eq!(idx, vec![0, 1, 4, 5]);
    }
}
