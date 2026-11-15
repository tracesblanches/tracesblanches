//! The track of a FIT activity file (Garmin, Suunto, Coros...: the originals Strava keeps
//! in its export). Only `record` messages (global number 20) are read: timestamp (253),
//! position (0, 1, semicircles) and altitude (2, or enhanced 78). Format: Garmin FIT SDK,
//! « FIT Protocol » (file header, definition and data messages, compressed timestamps).

use chrono::{DateTime, TimeZone, Utc};

use crate::track::{Point, TrackError};

/// Seconds from the Unix epoch to the FIT epoch (1989-12-31T00:00:00Z).
const FIT_EPOCH: i64 = 631_065_600;
const RECORD: u16 = 20;

#[derive(Clone)]
struct Def {
    big_endian: bool,
    global: u16,
    /// (field number, size).
    fields: Vec<(u8, u8)>,
    /// Bytes of developer fields after the fields.
    dev_bytes: usize,
}

fn err(m: &str) -> TrackError {
    TrackError::Format(format!("FIT: {m}"))
}

fn uint(b: &[u8], big: bool) -> u64 {
    let mut v = 0u64;
    if big {
        for &x in b {
            v = (v << 8) | u64::from(x);
        }
    } else {
        for &x in b.iter().rev() {
            v = (v << 8) | u64::from(x);
        }
    }
    v
}

fn time(fit_s: u32) -> Option<DateTime<Utc>> {
    Utc.timestamp_opt(FIT_EPOCH + i64::from(fit_s), 0).single()
}

/// Points of a FIT file.
pub fn parse(data: &[u8]) -> Result<Vec<Point>, TrackError> {
    if data.len() < 12 {
        return Err(err("too short"));
    }
    let header = usize::from(data[0]);
    if header < 12 || data.get(8..12) != Some(b".FIT") {
        return Err(err("no FIT header"));
    }
    let size = usize::try_from(u32::from_le_bytes([data[4], data[5], data[6], data[7]]))
        .map_err(|_| err("size"))?;
    let end = (header + size).min(data.len());
    let mut defs: [Option<Def>; 16] = Default::default();
    let mut i = header;
    let mut last_ts: Option<u32> = None;
    let mut out = Vec::new();
    while i < end {
        let h = data[i];
        i += 1;
        let (local, compressed_offset) = if h & 0x80 != 0 {
            (usize::from((h >> 5) & 0x03), Some(u32::from(h & 0x1F)))
        } else {
            (usize::from(h & 0x0F), None)
        };
        if compressed_offset.is_none() && h & 0x40 != 0 {
            // Definition message.
            let dev = h & 0x20 != 0;
            let b = data
                .get(i..i + 5)
                .ok_or_else(|| err("truncated definition"))?;
            let big = b[1] == 1;
            let global = u16::try_from(uint(&b[2..4], big)).unwrap_or(0);
            let n = usize::from(b[4]);
            i += 5;
            let f = data
                .get(i..i + 3 * n)
                .ok_or_else(|| err("truncated fields"))?;
            let fields = f.chunks(3).map(|c| (c[0], c[1])).collect();
            i += 3 * n;
            let mut dev_bytes = 0;
            if dev {
                let nd = usize::from(*data.get(i).ok_or_else(|| err("truncated"))?);
                i += 1;
                let d = data
                    .get(i..i + 3 * nd)
                    .ok_or_else(|| err("truncated dev fields"))?;
                dev_bytes = d.chunks(3).map(|c| usize::from(c[1])).sum();
                i += 3 * nd;
            }
            defs[local] = Some(Def {
                big_endian: big,
                global,
                fields,
                dev_bytes,
            });
            continue;
        }
        let def = defs[local]
            .clone()
            .ok_or_else(|| err("data before its definition"))?;
        let mut p = Point {
            lon: f64::NAN,
            lat: f64::NAN,
            ele: None,
            t: None,
        };
        let mut ts = None;
        let (mut lat, mut lon) = (None, None);
        for &(num, sz) in &def.fields {
            let sz = usize::from(sz);
            let b = data.get(i..i + sz).ok_or_else(|| err("truncated data"))?;
            i += sz;
            if def.global != RECORD {
                continue;
            }
            let v = uint(b, def.big_endian);
            match (num, sz) {
                (253, 4) if v != 0xFFFF_FFFF => ts = u32::try_from(v).ok(),
                (0, 4) if v != 0x7FFF_FFFF => lat = Some(v as u32 as i32),
                (1, 4) if v != 0x7FFF_FFFF => lon = Some(v as u32 as i32),
                (2, 2) if v != 0xFFFF && p.ele.is_none() => p.ele = Some(v as f64 / 5.0 - 500.0),
                (78, 4) if v != 0xFFFF_FFFF => p.ele = Some(v as f64 / 5.0 - 500.0),
                _ => {}
            }
        }
        i += def.dev_bytes;
        if let Some(off) = compressed_offset {
            // 5-bit offset from the last full timestamp, rolling over.
            ts = last_ts.map(|last| {
                let base = last & !0x1F;
                let t = base + off;
                if off >= (last & 0x1F) { t } else { t + 0x20 }
            });
        }
        if let Some(t) = ts {
            last_ts = Some(t);
        }
        if def.global != RECORD {
            continue;
        }
        let semi = 180.0 / 2_147_483_648.0;
        if let (Some(la), Some(lo)) = (lat, lon) {
            p.lat = f64::from(la) * semi;
            p.lon = f64::from(lo) * semi;
            p.t = ts.and_then(time);
            if p.lat.abs() <= 90.0 && p.lon.abs() <= 180.0 {
                out.push(p);
            }
        }
    }
    Ok(out)
}

/// Writes a FIT file of these points (tests of the reader; same layout as a watch).
#[doc(hidden)]
pub fn write_for_tests(points: &[Point]) -> Vec<u8> {
    let mut body = Vec::new();
    // file_id (global 0): type = 4 (activity), little endian, local 0.
    body.extend_from_slice(&[0x40, 0, 0, 0, 0, 1, 0, 1, 0]);
    body.extend_from_slice(&[0x00, 4]);
    // record definition, local 1, big endian, with a developer field of 2 bytes.
    body.extend_from_slice(&[
        0x61, 0, 1, 0, 20, 4, 253, 4, 0x86, 0, 4, 0x85, 1, 4, 0x85, 78, 4, 0x86,
    ]);
    body.extend_from_slice(&[1, 0, 2, 0]);
    let semi = 2_147_483_648.0 / 180.0;
    let mut last = 0u32;
    for (k, p) in points.iter().enumerate() {
        let t = p.t.map_or(0, |t| (t.timestamp() - FIT_EPOCH) as u32);
        let ele = p
            .ele
            .map_or(0xFFFF_FFFF, |e| ((e + 500.0) * 5.0).round() as u32);
        let lat = (p.lat * semi).round() as i32 as u32;
        let lon = (p.lon * semi).round() as i32 as u32;
        if k % 2 == 1 && t > last && t - last < 31 {
            // Compressed timestamp header, local 2 (a definition without timestamp).
            if k == 1 {
                body.extend_from_slice(&[
                    0x42, 0, 0, 20, 0, 3, 0, 4, 0x85, 1, 4, 0x85, 78, 4, 0x86,
                ]);
            }
            body.push(0x80 | (2 << 5) | (t & 0x1F) as u8);
            for v in [lat, lon, ele] {
                body.extend_from_slice(&v.to_le_bytes());
            }
        } else {
            body.push(0x01);
            for v in [t, lat, lon, ele] {
                body.extend_from_slice(&v.to_be_bytes());
            }
            body.extend_from_slice(&[0xAB, 0xCD]);
        }
        last = t;
    }
    let mut out = vec![14, 0x10, 0, 0];
    out.extend_from_slice(&u32::try_from(body.len()).unwrap_or(0).to_le_bytes());
    out.extend_from_slice(b".FIT");
    out.extend_from_slice(&[0, 0]);
    out.extend_from_slice(&body);
    out.extend_from_slice(&[0, 0]);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn records_with_full_and_compressed_timestamps() {
        let t0: DateTime<Utc> = "2026-01-10T09:00:00Z".parse().unwrap();
        let pts: Vec<Point> = (0..50)
            .map(|i| Point {
                lon: 6.123 + f64::from(i) * 1e-4,
                lat: 46.6 - f64::from(i) * 1e-4,
                ele: Some(1090.0 + f64::from(i)),
                t: Some(t0 + chrono::Duration::seconds(i64::from(i) * 3)),
            })
            .collect();
        let fit = write_for_tests(&pts);
        let got = parse(&fit).unwrap();
        assert_eq!(got.len(), pts.len());
        for (a, b) in got.iter().zip(&pts) {
            assert!((a.lat - b.lat).abs() < 1e-6 && (a.lon - b.lon).abs() < 1e-6);
            assert!((a.ele.unwrap() - b.ele.unwrap()).abs() < 0.2);
            assert_eq!(a.t, b.t);
        }
        assert!(parse(b"not a fit file").is_err());
        assert!(parse(&fit[..40]).is_err(), "truncated");
    }
}
