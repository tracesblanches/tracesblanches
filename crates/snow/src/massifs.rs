//! Massif contours of the Météo-France snowpack simulation (SAFRAN / S2M massifs).
//!
//! Source: `snowtools/DATA/massifs.shp` of the CNRM `snowtools` repository (Météo-France /
//! CNRS research unit that runs the simulation), CeCILL-C licence, Lambert-93. It is the
//! only source found with the Jura massifs (55 Haut-Doubs, 56 Haut-Jura): the
//! `Metadata_massif_DP_2024` GeoJSON used by report 04 (third-party copy) only has the BERA
//! massifs (Alps, Pyrenees, Corsica). The file is pinned to the commit that last changed it.
//!
//! Minimal ESRI shapefile (polygons) and dBASE readers are included: the workspace has no
//! shapefile crate.

use skimap_common::download::{Downloader, Request};
use skimap_common::grid::Grid;

use crate::error::{Error, Result};

/// Commit of `UMR-CNRM/snowtools` that last changed `massifs.shp` / `.dbf` (2023-01-22).
pub const SNOWTOOLS_COMMIT: &str = "c84a1609012cf88d47551e3e9c38ea758ee609b8";
/// Downloader source of GitHub raw files.
pub const SOURCE: &str = "github";

/// URL of a file of the massif shapefile (`ext` = `shp`, `dbf`, ...).
pub fn url(ext: &str) -> String {
    format!(
        "https://raw.githubusercontent.com/UMR-CNRM/snowtools/{SNOWTOOLS_COMMIT}/snowtools/DATA/massifs.{ext}"
    )
}

/// A massif polygon (one or several rings, even-odd rule), Lambert-93.
#[derive(Debug, Clone, PartialEq)]
pub struct Massif {
    /// Météo-France massif number.
    pub code: u16,
    /// Massif name.
    pub name: String,
    /// Rings of (x, y) vertices, closed or not.
    pub rings: Vec<Vec<(f64, f64)>>,
    /// Bounding box (min_x, min_y, max_x, max_y).
    pub bbox: (f64, f64, f64, f64),
}

impl Massif {
    /// Point-in-polygon, even-odd rule over all rings.
    pub fn contains(&self, x: f64, y: f64) -> bool {
        let (x0, y0, x1, y1) = self.bbox;
        if x < x0 || x > x1 || y < y0 || y > y1 {
            return false;
        }
        let mut inside = false;
        for ring in &self.rings {
            let n = ring.len();
            let mut j = n.wrapping_sub(1);
            for i in 0..n {
                let (xi, yi) = ring[i];
                let (xj, yj) = ring[j];
                if (yi > y) != (yj > y) && x < (xj - xi) * (y - yi) / (yj - yi) + xi {
                    inside = !inside;
                }
                j = i;
            }
        }
        inside
    }
}

fn le_i32(b: &[u8], o: usize) -> Result<i32> {
    b.get(o..o + 4)
        .map(|s| i32::from_le_bytes([s[0], s[1], s[2], s[3]]))
        .ok_or_else(|| Error::parse("shapefile", "truncated"))
}

fn be_i32(b: &[u8], o: usize) -> Result<i32> {
    b.get(o..o + 4)
        .map(|s| i32::from_be_bytes([s[0], s[1], s[2], s[3]]))
        .ok_or_else(|| Error::parse("shapefile", "truncated"))
}

fn le_f64(b: &[u8], o: usize) -> Result<f64> {
    b.get(o..o + 8)
        .map(|s| f64::from_le_bytes([s[0], s[1], s[2], s[3], s[4], s[5], s[6], s[7]]))
        .ok_or_else(|| Error::parse("shapefile", "truncated"))
}

/// Rings of one polygon record, Lambert-93 (x, y).
pub type Rings = Vec<Vec<(f64, f64)>>;

/// Polygons (rings) of a `.shp` file, in record order.
pub fn parse_shp(b: &[u8]) -> Result<Vec<Rings>> {
    if be_i32(b, 0)? != 9994 {
        return Err(Error::parse("shapefile", "bad file code"));
    }
    let shape_type = le_i32(b, 32)?;
    if shape_type != 5 {
        return Err(Error::parse(
            "shapefile",
            format!("shape type {shape_type}, expected polygon (5)"),
        ));
    }
    let mut out = Vec::new();
    let mut pos = 100;
    while pos + 8 <= b.len() {
        let len = usize::try_from(be_i32(b, pos + 4)?)
            .map_err(|_| Error::parse("shapefile", "negative record length"))?
            * 2;
        let c = pos + 8;
        let kind = le_i32(b, c)?;
        let mut rings = Vec::new();
        if kind == 5 {
            let nparts = usize::try_from(le_i32(b, c + 36)?)
                .map_err(|_| Error::parse("shapefile", "negative part count"))?;
            let npoints = usize::try_from(le_i32(b, c + 40)?)
                .map_err(|_| Error::parse("shapefile", "negative point count"))?;
            let mut parts = Vec::with_capacity(nparts + 1);
            for k in 0..nparts {
                parts.push(
                    usize::try_from(le_i32(b, c + 44 + 4 * k)?)
                        .map_err(|_| Error::parse("shapefile", "negative part index"))?,
                );
            }
            parts.push(npoints);
            let pts = c + 44 + 4 * nparts;
            for w in parts.windows(2) {
                let mut ring = Vec::with_capacity(w[1].saturating_sub(w[0]));
                for k in w[0]..w[1] {
                    ring.push((le_f64(b, pts + 16 * k)?, le_f64(b, pts + 16 * k + 8)?));
                }
                rings.push(ring);
            }
        } else if kind != 0 {
            return Err(Error::parse(
                "shapefile",
                format!("record shape type {kind}"),
            ));
        }
        out.push(rings);
        pos = c + len;
    }
    Ok(out)
}

/// Records of a dBASE III file as (field name, raw trimmed text) lists.
pub fn parse_dbf(b: &[u8]) -> Result<Vec<Vec<(String, String)>>> {
    let err = |m: &str| Error::parse("dbf", m);
    if b.len() < 32 {
        return Err(err("truncated header"));
    }
    let nrec = u32::from_le_bytes([b[4], b[5], b[6], b[7]]) as usize;
    let hlen = usize::from(u16::from_le_bytes([b[8], b[9]]));
    let rlen = usize::from(u16::from_le_bytes([b[10], b[11]]));
    let mut fields = Vec::new();
    let mut p = 32;
    while p + 32 <= hlen && b[p] != 0x0D {
        let name_raw = &b[p..p + 11];
        let end = name_raw.iter().position(|c| *c == 0).unwrap_or(11);
        let name = String::from_utf8_lossy(&name_raw[..end]).into_owned();
        let flen = usize::from(b[p + 16]);
        fields.push((name, flen));
        p += 32;
    }
    let mut out = Vec::with_capacity(nrec);
    for r in 0..nrec {
        let start = hlen + r * rlen;
        let rec = b
            .get(start..start + rlen)
            .ok_or_else(|| err("truncated record"))?;
        let mut off = 1; // deletion flag
        let mut vals = Vec::with_capacity(fields.len());
        for (name, flen) in &fields {
            let raw = rec
                .get(off..off + flen)
                .ok_or_else(|| err("truncated field"))?;
            vals.push((name.clone(), String::from_utf8_lossy(raw).trim().to_owned()));
            off += flen;
        }
        out.push(vals);
    }
    Ok(out)
}

/// Massifs from the bytes of `massifs.shp` and `massifs.dbf`.
pub fn from_bytes(shp: &[u8], dbf: &[u8]) -> Result<Vec<Massif>> {
    let polys = parse_shp(shp)?;
    let recs = parse_dbf(dbf)?;
    if polys.len() != recs.len() {
        return Err(Error::parse(
            "massifs",
            format!("{} shapes but {} records", polys.len(), recs.len()),
        ));
    }
    let field = |rec: &[(String, String)], name: &str| {
        rec.iter()
            .find(|(n, _)| n == name)
            .map(|(_, v)| v.clone())
            .ok_or_else(|| Error::parse("massifs", format!("missing field {name}")))
    };
    let mut out = Vec::new();
    for (rings, rec) in polys.into_iter().zip(recs) {
        let code: u16 = field(&rec, "code")?
            .parse()
            .map_err(|e| Error::parse("massifs code", e))?;
        let name = field(&rec, "title")?;
        let mut bbox = (
            f64::INFINITY,
            f64::INFINITY,
            f64::NEG_INFINITY,
            f64::NEG_INFINITY,
        );
        for &(x, y) in rings.iter().flatten() {
            bbox = (bbox.0.min(x), bbox.1.min(y), bbox.2.max(x), bbox.3.max(y));
        }
        out.push(Massif {
            code,
            name,
            rings,
            bbox,
        });
    }
    Ok(out)
}

/// Downloads (or reads from cache) the massif contours.
pub fn fetch(dl: &Downloader) -> Result<Vec<Massif>> {
    let month = std::time::Duration::from_secs(30 * 86_400);
    let shp = dl
        .fetch(&Request::get(SOURCE, &url("shp")).max_age(month))?
        .bytes()?;
    let dbf = dl
        .fetch(&Request::get(SOURCE, &url("dbf")).max_age(month))?
        .bytes()?;
    from_bytes(&shp, &dbf)
}

/// Massif containing a Lambert-93 point.
pub fn massif_at(massifs: &[Massif], x: f64, y: f64) -> Option<&Massif> {
    massifs.iter().find(|m| m.contains(x, y))
}

/// Massif code of every cell centre of `like` (0 = no massif), by scanline.
pub fn rasterize(massifs: &[Massif], like: &Grid<f32>) -> Grid<u16> {
    let (w, h) = (like.width(), like.height());
    let gt = *like.transform();
    let (gx0, gy0, gx1, gy1) = like.bounds();
    let mut out = vec![0u16; w * h];
    for m in massifs {
        let (x0, y0, x1, y1) = m.bbox;
        if x1 < gx0 || x0 > gx1 || y1 < gy0 || y0 > gy1 {
            continue;
        }
        let mut xs = Vec::new();
        for r in 0..h {
            let (_, y) = gt.cell_center(0, r);
            if y < y0 || y > y1 {
                continue;
            }
            xs.clear();
            for ring in &m.rings {
                let n = ring.len();
                let mut j = n.wrapping_sub(1);
                for i in 0..n {
                    let (xi, yi) = ring[i];
                    let (xj, yj) = ring[j];
                    if (yi > y) != (yj > y) {
                        xs.push((xj - xi) * (y - yi) / (yj - yi) + xi);
                    }
                    j = i;
                }
            }
            xs.sort_by(f64::total_cmp);
            // Same rule as `contains`: a centre x is inside when an odd number of crossings
            // are > x, i.e. when a(2k) <= x < a(2k+1) for the sorted crossings.
            for pair in xs.as_chunks::<2>().0 {
                let (a, b) = (pair[0], pair[1]);
                let c_start = ((a - gt.origin_x) / gt.pixel_width - 0.5).ceil();
                let c_end = ((b - gt.origin_x) / gt.pixel_width - 0.5).ceil() - 1.0;
                let c_start = c_start.max(0.0) as usize;
                if c_end < 0.0 {
                    continue;
                }
                let c_end = (c_end as usize).min(w.saturating_sub(1));
                for c in c_start..=c_end {
                    if c < w && out[r * w + c] == 0 {
                        out[r * w + c] = m.code;
                    }
                }
            }
        }
    }
    Grid::from_vec(w, h, gt, like.crs(), out).expect("same shape")
}

/// Fills cells with a value (`valid`) but no massif (`0`) with the massif of the nearest
/// assigned cell (breadth-first, 8-neighbourhood); returns the number of cells filled.
pub fn fill_nearest(codes: &mut Grid<u16>, valid: impl Fn(usize) -> bool) -> usize {
    let (w, h) = (codes.width(), codes.height());
    let data = codes.data_mut();
    let mut queue: std::collections::VecDeque<usize> =
        (0..w * h).filter(|&i| data[i] != 0).collect();
    let mut filled = 0;
    while let Some(i) = queue.pop_front() {
        let (r, c) = (i / w, i % w);
        for rr in r.saturating_sub(1)..(r + 2).min(h) {
            for cc in c.saturating_sub(1)..(c + 2).min(w) {
                let j = rr * w + cc;
                if data[j] == 0 && valid(j) {
                    data[j] = data[i];
                    filled += 1;
                    queue.push_back(j);
                }
            }
        }
    }
    filled
}

#[cfg(test)]
mod tests {
    use super::*;
    use skimap_common::crs::Crs;
    use skimap_common::grid::GeoTransform;

    fn square_with_hole() -> Massif {
        Massif {
            code: 17,
            name: "Test".into(),
            rings: vec![
                vec![
                    (0.0, 0.0),
                    (0.0, 100.0),
                    (100.0, 100.0),
                    (100.0, 0.0),
                    (0.0, 0.0),
                ],
                vec![
                    (40.0, 40.0),
                    (60.0, 40.0),
                    (60.0, 60.0),
                    (40.0, 60.0),
                    (40.0, 40.0),
                ],
            ],
            bbox: (0.0, 0.0, 100.0, 100.0),
        }
    }

    #[test]
    fn point_in_polygon_with_hole() {
        let m = square_with_hole();
        assert!(m.contains(10.0, 10.0));
        assert!(!m.contains(50.0, 50.0));
        assert!(!m.contains(150.0, 50.0));
        assert!(m.contains(99.0, 50.0));
    }

    #[test]
    fn rasterize_matches_point_in_polygon() {
        let m = square_with_hole();
        let gt = GeoTransform::north_up(-12.0, 113.0, 7.0);
        let like = Grid::filled(20, 20, gt, Crs::Lambert93, 0.0f32);
        let r = rasterize(std::slice::from_ref(&m), &like);
        for row in 0..20 {
            for col in 0..20 {
                let (x, y) = gt.cell_center(col, row);
                let expected = if m.contains(x, y) { 17 } else { 0 };
                assert_eq!(
                    r.get(col, row).copied(),
                    Some(expected),
                    "({col},{row}) at ({x},{y})"
                );
            }
        }
    }

    #[test]
    fn nearest_fill() {
        let gt = GeoTransform::north_up(0.0, 0.0, 1.0);
        let mut g = Grid::from_vec(4, 1, gt, Crs::Lambert93, vec![55, 0, 0, 56]).unwrap();
        let n = fill_nearest(&mut g, |_| true);
        assert_eq!(n, 2);
        assert_eq!(g.data(), &[55, 55, 56, 56]);
        let mut g = Grid::from_vec(3, 1, gt, Crs::Lambert93, vec![55, 0, 0]).unwrap();
        fill_nearest(&mut g, |i| i != 2);
        assert_eq!(g.data(), &[55, 55, 0]);
    }

    #[test]
    fn dbf_and_shp_parsers() {
        // Hand-built dBASE: 2 fields (code N 3, title C 5), 1 record.
        let mut dbf = vec![0u8; 32];
        dbf[0] = 3;
        dbf[4] = 1;
        let hlen: u16 = 32 + 2 * 32 + 1;
        dbf[8..10].copy_from_slice(&hlen.to_le_bytes());
        dbf[10..12].copy_from_slice(&(1u16 + 3 + 5).to_le_bytes());
        for (name, t, len) in [("code", b'N', 3u8), ("title", b'C', 5u8)] {
            let mut f = vec![0u8; 32];
            f[..name.len()].copy_from_slice(name.as_bytes());
            f[11] = t;
            f[16] = len;
            dbf.extend(f);
        }
        dbf.push(0x0D);
        dbf.extend(b"  17Abc  ");
        let recs = parse_dbf(&dbf).unwrap();
        assert_eq!(
            recs,
            vec![vec![
                ("code".into(), "17".into()),
                ("title".into(), "Abc".into())
            ]]
        );

        // Hand-built shapefile: one polygon record with one triangular ring.
        let pts = [(0.0f64, 0.0f64), (0.0, 10.0), (10.0, 0.0), (0.0, 0.0)];
        let mut content = Vec::new();
        content.extend(5i32.to_le_bytes());
        content.extend([0u8; 32]);
        content.extend(1i32.to_le_bytes());
        content.extend((pts.len() as i32).to_le_bytes());
        content.extend(0i32.to_le_bytes());
        for (x, y) in pts {
            content.extend(x.to_le_bytes());
            content.extend(y.to_le_bytes());
        }
        let mut shp = vec![0u8; 100];
        shp[0..4].copy_from_slice(&9994i32.to_be_bytes());
        shp[32..36].copy_from_slice(&5i32.to_le_bytes());
        shp.extend(1i32.to_be_bytes());
        shp.extend(((content.len() / 2) as i32).to_be_bytes());
        shp.extend(content);
        let m = from_bytes(&shp, &dbf).unwrap();
        assert_eq!(m.len(), 1);
        assert_eq!(m[0].code, 17);
        assert_eq!(m[0].name, "Abc");
        assert!(m[0].contains(2.0, 2.0));
        assert!(!m[0].contains(8.0, 8.0));
    }
}
