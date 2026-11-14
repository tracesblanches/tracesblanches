//! Minimal ESRI shapefile reader: polygon geometries (`.shp`, types 5, 15, 25) and dBASE
//! attributes (`.dbf`, text fields). Only what the DDT 05 avalanche layer needs.
//!
//! Reference: ESRI Shapefile Technical Description (July 1998).

use std::collections::BTreeMap;

/// A polygon record: rings in file coordinates (outer rings clockwise, holes
/// counter-clockwise, as the format requires). `None` for a null shape.
pub type Rings = Vec<Vec<(f64, f64)>>;

fn le_i32(b: &[u8], at: usize) -> Result<i32, String> {
    b.get(at..at + 4)
        .map(|s| i32::from_le_bytes([s[0], s[1], s[2], s[3]]))
        .ok_or_else(|| format!("truncated at byte {at}"))
}

fn be_i32(b: &[u8], at: usize) -> Result<i32, String> {
    b.get(at..at + 4)
        .map(|s| i32::from_be_bytes([s[0], s[1], s[2], s[3]]))
        .ok_or_else(|| format!("truncated at byte {at}"))
}

fn le_f64(b: &[u8], at: usize) -> Result<f64, String> {
    b.get(at..at + 8)
        .map(|s| f64::from_le_bytes([s[0], s[1], s[2], s[3], s[4], s[5], s[6], s[7]]))
        .ok_or_else(|| format!("truncated at byte {at}"))
}

/// Reads the polygon records of a `.shp` file, in record order.
pub fn read_polygons(shp: &[u8]) -> Result<Vec<Option<Rings>>, String> {
    if shp.len() < 100 || be_i32(shp, 0)? != 9994 {
        return Err("not a shapefile (file code != 9994)".into());
    }
    let mut out = Vec::new();
    let mut pos = 100;
    while pos + 8 <= shp.len() {
        let content_len = usize::try_from(be_i32(shp, pos + 4)?)
            .map_err(|_| format!("negative record length at byte {pos}"))?
            * 2;
        let rec = shp
            .get(pos + 8..pos + 8 + content_len)
            .ok_or_else(|| format!("record at byte {pos} overflows the file"))?;
        pos += 8 + content_len;
        let shape_type = le_i32(rec, 0)?;
        match shape_type {
            0 => out.push(None),
            5 | 15 | 25 => {
                let n_parts = usize::try_from(le_i32(rec, 36)?).map_err(|_| "negative parts")?;
                let n_points = usize::try_from(le_i32(rec, 40)?).map_err(|_| "negative points")?;
                let mut parts = Vec::with_capacity(n_parts + 1);
                for i in 0..n_parts {
                    let p = usize::try_from(le_i32(rec, 44 + 4 * i)?)
                        .map_err(|_| "negative part index")?;
                    parts.push(p);
                }
                parts.push(n_points);
                let base = 44 + 4 * n_parts;
                let mut rings = Vec::with_capacity(n_parts);
                for w in parts.windows(2) {
                    if w[0] > w[1] || w[1] > n_points {
                        return Err(format!("bad part bounds {w:?}"));
                    }
                    let mut ring = Vec::with_capacity(w[1] - w[0]);
                    for i in w[0]..w[1] {
                        let at = base + 16 * i;
                        ring.push((le_f64(rec, at)?, le_f64(rec, at + 8)?));
                    }
                    rings.push(ring);
                }
                out.push(Some(rings));
            }
            t => return Err(format!("unsupported shape type {t}")),
        }
    }
    Ok(out)
}

/// Reads the records of a `.dbf` file as text (trimmed), field name → value. Deleted
/// records are kept (as empty maps) so that indices match the `.shp` records.
pub fn read_dbf(dbf: &[u8]) -> Result<Vec<BTreeMap<String, String>>, String> {
    if dbf.len() < 32 {
        return Err("dbf too short".into());
    }
    let n = u32::from_le_bytes([dbf[4], dbf[5], dbf[6], dbf[7]]) as usize;
    let header_len = u16::from_le_bytes([dbf[8], dbf[9]]) as usize;
    let record_len = u16::from_le_bytes([dbf[10], dbf[11]]) as usize;
    let mut fields = Vec::new();
    let mut at = 32;
    while at < header_len && dbf.get(at).is_some_and(|&b| b != 0x0D) {
        let d = dbf.get(at..at + 32).ok_or("truncated field descriptor")?;
        let name_end = d[..11].iter().position(|&b| b == 0).unwrap_or(11);
        let name = String::from_utf8_lossy(&d[..name_end]).into_owned();
        fields.push((name, d[16] as usize));
        at += 32;
    }
    let mut rows = Vec::with_capacity(n);
    for i in 0..n {
        let start = header_len + i * record_len;
        let r = dbf
            .get(start..start + record_len)
            .ok_or_else(|| format!("record {i} overflows the dbf"))?;
        let mut row = BTreeMap::new();
        if r[0] != b'*' {
            let mut p = 1;
            for (name, len) in &fields {
                let raw = r.get(p..p + len).ok_or("field overflows the record")?;
                row.insert(name.clone(), String::from_utf8_lossy(raw).trim().to_owned());
                p += len;
            }
        }
        rows.push(row);
    }
    Ok(rows)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn shp_with_square() -> Vec<u8> {
        // One PolygonZ-less polygon (type 5) with one ring, file header 100 bytes.
        let pts = [
            (0.0, 0.0),
            (0.0, 10.0),
            (10.0, 10.0),
            (10.0, 0.0),
            (0.0, 0.0),
        ];
        let mut rec = Vec::new();
        rec.extend(5i32.to_le_bytes());
        for v in [0.0f64, 0.0, 10.0, 10.0] {
            rec.extend(v.to_le_bytes());
        }
        rec.extend(1i32.to_le_bytes());
        rec.extend((pts.len() as i32).to_le_bytes());
        rec.extend(0i32.to_le_bytes());
        for (x, y) in pts {
            rec.extend(f64::to_le_bytes(x));
            rec.extend(f64::to_le_bytes(y));
        }
        let mut f = vec![0u8; 100];
        f[..4].copy_from_slice(&9994i32.to_be_bytes());
        f.extend(1i32.to_be_bytes());
        f.extend(((rec.len() / 2) as i32).to_be_bytes());
        f.extend(rec);
        // Null record.
        f.extend(2i32.to_be_bytes());
        f.extend(2i32.to_be_bytes());
        f.extend(0i32.to_le_bytes());
        f
    }

    #[test]
    fn polygons() {
        let p = read_polygons(&shp_with_square()).unwrap();
        assert_eq!(p.len(), 2);
        let rings = p[0].as_ref().unwrap();
        assert_eq!(rings.len(), 1);
        assert_eq!(rings[0][2], (10.0, 10.0));
        assert!(p[1].is_none());
        assert!(read_polygons(&[0u8; 50]).is_err());
    }

    #[test]
    fn dbf_text_fields() {
        let mut f = vec![0u8; 32];
        f[0] = 3;
        f[4..8].copy_from_slice(&2u32.to_le_bytes());
        let header_len: u16 = 32 + 2 * 32 + 1;
        f[8..10].copy_from_slice(&header_len.to_le_bytes());
        f[10..12].copy_from_slice(&(1u16 + 5 + 4).to_le_bytes());
        for (name, len) in [("INSEE", 5u8), ("SRC", 4u8)] {
            let mut d = [0u8; 32];
            d[..name.len()].copy_from_slice(name.as_bytes());
            d[11] = b'C';
            d[16] = len;
            f.extend(d);
        }
        f.push(0x0D);
        f.extend(b" 05027CLPA");
        f.extend(b"*05028PPR ");
        let rows = read_dbf(&f).unwrap();
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0]["INSEE"], "05027");
        assert_eq!(rows[0]["SRC"], "CLPA");
        assert!(rows[1].is_empty(), "deleted record");
    }
}
