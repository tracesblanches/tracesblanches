//! GeoPackage geometry blobs: the `GP` header and the WKB body (polygons and
//! multipolygons, 2D, Z, M or ZM; ISO and EWKB type codes).

use geo::{Coord, LineString, MultiPolygon, Polygon, Rect};

use crate::error::{Error, Result};

/// Envelope stored in a GeoPackage header, when present.
pub type Envelope = Option<Rect<f64>>;

/// Splits a GeoPackage geometry blob into its envelope and WKB body.
///
/// Header (GeoPackage 1.2, §2.1.3): `GP`, version, flags (bit 0 byte order, bits 1–3
/// envelope kind, bit 4 empty), srs_id, envelope.
pub fn split_gpkg(blob: &[u8]) -> Result<(Envelope, &[u8])> {
    if blob.len() < 8 || &blob[0..2] != b"GP" {
        return Err(Error::Wkb("not a GeoPackage geometry (no GP magic)".into()));
    }
    let flags = blob[3];
    let little = flags & 1 == 1;
    let env_len = match (flags >> 1) & 0b111 {
        0 => 0,
        1 => 32,
        2 | 3 => 48,
        4 => 64,
        k => return Err(Error::Wkb(format!("invalid envelope indicator {k}"))),
    };
    let body = blob
        .get(8 + env_len..)
        .ok_or_else(|| Error::Wkb("truncated GeoPackage header".into()))?;
    let env = if env_len >= 32 {
        let mut r = Reader::new(&blob[8..8 + env_len], little);
        let (minx, maxx, miny, maxy) = (r.f64()?, r.f64()?, r.f64()?, r.f64()?);
        Some(Rect::new(
            Coord { x: minx, y: miny },
            Coord { x: maxx, y: maxy },
        ))
    } else {
        None
    };
    Ok((env, body))
}

struct Reader<'a> {
    buf: &'a [u8],
    pos: usize,
    little: bool,
}

impl<'a> Reader<'a> {
    fn new(buf: &'a [u8], little: bool) -> Self {
        Self {
            buf,
            pos: 0,
            little,
        }
    }

    fn take<const N: usize>(&mut self) -> Result<[u8; N]> {
        let end = self.pos + N;
        let s = self
            .buf
            .get(self.pos..end)
            .ok_or_else(|| Error::Wkb(format!("truncated WKB at byte {}", self.pos)))?;
        self.pos = end;
        Ok(s.try_into().expect("slice of length N"))
    }

    fn u8(&mut self) -> Result<u8> {
        Ok(self.take::<1>()?[0])
    }

    fn u32(&mut self) -> Result<u32> {
        let b = self.take::<4>()?;
        Ok(if self.little {
            u32::from_le_bytes(b)
        } else {
            u32::from_be_bytes(b)
        })
    }

    fn f64(&mut self) -> Result<f64> {
        let b = self.take::<8>()?;
        Ok(if self.little {
            f64::from_le_bytes(b)
        } else {
            f64::from_be_bytes(b)
        })
    }
}

/// Geometry type and extra ordinates per point (Z, M).
fn header(r: &mut Reader<'_>) -> Result<(u32, usize)> {
    let order = r.u8()?;
    r.little = match order {
        0 => false,
        1 => true,
        o => return Err(Error::Wkb(format!("invalid byte order {o}"))),
    };
    let raw = r.u32()?;
    // EWKB flags.
    let (mut extra, mut code) = (0usize, raw);
    if raw & 0x8000_0000 != 0 {
        extra += 1;
    }
    if raw & 0x4000_0000 != 0 {
        extra += 1;
    }
    if raw & 0x2000_0000 != 0 {
        r.u32()?; // SRID
    }
    code &= 0x0FFF_FFFF;
    // ISO codes: 1000 Z, 2000 M, 3000 ZM.
    match code / 1000 {
        0 => {}
        1 | 2 => extra += 1,
        3 => extra += 2,
        _ => return Err(Error::Wkb(format!("invalid geometry type {raw}"))),
    }
    Ok((code % 1000, extra))
}

fn ring(r: &mut Reader<'_>, extra: usize) -> Result<LineString<f64>> {
    let n = r.u32()? as usize;
    if n > r.buf.len() / 16 {
        return Err(Error::Wkb(format!("ring of {n} points exceeds the blob")));
    }
    let mut pts = Vec::with_capacity(n);
    for _ in 0..n {
        let (x, y) = (r.f64()?, r.f64()?);
        for _ in 0..extra {
            r.f64()?;
        }
        pts.push(Coord { x, y });
    }
    Ok(LineString::new(pts))
}

fn polygon_body(r: &mut Reader<'_>, extra: usize) -> Result<Polygon<f64>> {
    let n = r.u32()? as usize;
    if n == 0 {
        return Ok(Polygon::new(LineString::new(vec![]), vec![]));
    }
    let exterior = ring(r, extra)?;
    let mut holes = Vec::with_capacity(n - 1);
    for _ in 1..n {
        holes.push(ring(r, extra)?);
    }
    Ok(Polygon::new(exterior, holes))
}

/// Parses a WKB polygon or multipolygon.
pub fn parse_wkb(buf: &[u8]) -> Result<MultiPolygon<f64>> {
    let mut r = Reader::new(buf, true);
    let (kind, extra) = header(&mut r)?;
    match kind {
        3 => Ok(MultiPolygon::new(vec![polygon_body(&mut r, extra)?])),
        6 => {
            let n = r.u32()? as usize;
            let mut polys = Vec::with_capacity(n.min(buf.len() / 9));
            for _ in 0..n {
                let (k, e) = header(&mut r)?;
                if k != 3 {
                    return Err(Error::Wkb(format!("multipolygon member of type {k}")));
                }
                polys.push(polygon_body(&mut r, e)?);
            }
            Ok(MultiPolygon::new(polys))
        }
        k => Err(Error::Wkb(format!(
            "expected a (multi)polygon, got WKB type {k}"
        ))),
    }
}

/// Parses a GeoPackage geometry blob.
#[cfg(test)]
pub fn parse_gpkg(blob: &[u8]) -> Result<(Envelope, MultiPolygon<f64>)> {
    let (env, body) = split_gpkg(blob)?;
    Ok((env, parse_wkb(body)?))
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    fn push_ring(out: &mut Vec<u8>, pts: &[(f64, f64)], z: bool, little: bool) {
        let u32b = |v: u32| {
            if little {
                v.to_le_bytes()
            } else {
                v.to_be_bytes()
            }
        };
        let f64b = |v: f64| {
            if little {
                v.to_le_bytes()
            } else {
                v.to_be_bytes()
            }
        };
        out.extend(u32b(pts.len() as u32));
        for &(x, y) in pts {
            out.extend(f64b(x));
            out.extend(f64b(y));
            if z {
                out.extend(f64b(1234.5));
            }
        }
    }

    /// WKB polygon with one exterior ring and optional hole.
    pub(crate) fn wkb_polygon(
        ext: &[(f64, f64)],
        hole: Option<&[(f64, f64)]>,
        z: bool,
        little: bool,
    ) -> Vec<u8> {
        let mut out = vec![u8::from(little)];
        let t: u32 = if z { 1003 } else { 3 };
        out.extend(if little {
            t.to_le_bytes()
        } else {
            t.to_be_bytes()
        });
        let n = 1 + u32::from(hole.is_some());
        out.extend(if little {
            n.to_le_bytes()
        } else {
            n.to_be_bytes()
        });
        push_ring(&mut out, ext, z, little);
        if let Some(h) = hole {
            push_ring(&mut out, h, z, little);
        }
        out
    }

    /// WKB multipolygon of the given polygons (little endian).
    pub(crate) fn wkb_multi(polys: &[Vec<u8>]) -> Vec<u8> {
        let mut out = vec![1u8];
        out.extend(6u32.to_le_bytes());
        out.extend((polys.len() as u32).to_le_bytes());
        for p in polys {
            out.extend(p);
        }
        out
    }

    /// GeoPackage blob with an XY envelope.
    pub(crate) fn gpkg_blob(wkb: &[u8], env: (f64, f64, f64, f64)) -> Vec<u8> {
        let mut out = b"GP".to_vec();
        out.push(0);
        out.push(0b0000_0011); // little endian, envelope [minx, maxx, miny, maxy]
        out.extend(2154i32.to_le_bytes());
        for v in [env.0, env.1, env.2, env.3] {
            out.extend(v.to_le_bytes());
        }
        out.extend(wkb);
        out
    }

    const SQ: [(f64, f64); 5] = [
        (0.0, 0.0),
        (10.0, 0.0),
        (10.0, 10.0),
        (0.0, 10.0),
        (0.0, 0.0),
    ];
    const HOLE: [(f64, f64); 5] = [(2.0, 2.0), (2.0, 4.0), (4.0, 4.0), (4.0, 2.0), (2.0, 2.0)];

    #[test]
    fn polygon_with_hole_both_byte_orders_and_z() {
        for (z, little) in [(false, true), (false, false), (true, true), (true, false)] {
            let mp = parse_wkb(&wkb_polygon(&SQ, Some(&HOLE), z, little)).unwrap();
            assert_eq!(mp.0.len(), 1);
            assert_eq!(mp.0[0].exterior().0.len(), 5);
            assert_eq!(mp.0[0].interiors().len(), 1);
            assert_eq!(geo::Area::unsigned_area(&mp), 96.0);
        }
    }

    #[test]
    fn multipolygon_in_gpkg_blob() {
        let far: Vec<(f64, f64)> = SQ.iter().map(|(x, y)| (x + 100.0, y + 100.0)).collect();
        let wkb = wkb_multi(&[
            wkb_polygon(&SQ, None, false, true),
            wkb_polygon(&far, None, false, false),
        ]);
        let blob = gpkg_blob(&wkb, (0.0, 110.0, 0.0, 110.0));
        let (env, mp) = parse_gpkg(&blob).unwrap();
        assert_eq!(mp.0.len(), 2);
        let env = env.unwrap();
        assert_eq!(
            (env.min().x, env.max().x, env.min().y, env.max().y),
            (0.0, 110.0, 0.0, 110.0)
        );
        assert_eq!(geo::Area::unsigned_area(&mp), 200.0);
    }

    #[test]
    fn malformed_input_is_an_error() {
        assert!(parse_gpkg(b"XX").is_err());
        let wkb = wkb_polygon(&SQ, None, false, true);
        assert!(parse_wkb(&wkb[..wkb.len() - 3]).is_err());
        let mut point = vec![1u8];
        point.extend(1u32.to_le_bytes());
        point.extend(0f64.to_le_bytes());
        point.extend(0f64.to_le_bytes());
        assert!(parse_wkb(&point).is_err());
        // Huge announced ring size must not allocate.
        let mut bad = vec![1u8];
        bad.extend(3u32.to_le_bytes());
        bad.extend(1u32.to_le_bytes());
        bad.extend(u32::MAX.to_le_bytes());
        assert!(parse_wkb(&bad).is_err());
    }
}
