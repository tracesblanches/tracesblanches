//! Export of a finished outing (#63): a GPX 1.1 file and a FIT activity file, shared by
//! the phone's share menu (Strava, Komoot, Garmin Connect... import them), with no
//! account and no key. The app writes the same outing to Health Connect from
//! [`ExportedOuting::points`].
//!
//! The trace is the one the location service recorded (`inputs.jsonl` of the outing,
//! [`crate::recap`]): every fix, its time, the altitude of the recap (barometer fused
//! with the relief, K8; the barometer alone only gives an altitude relative to the start,
//! so the points then carry none, the climb stays) and the heart rate the app read for
//! the hours skied (Health Connect, a watch), when there is one. The distance is the
//! recap's (GPS jumps and stops left out), cumulated point by point.
//!
//! The practice gives the sport: cross-country skiing on groomed trails, or nordic
//! backcountry skiing (FIT `cross_country_skiing` with the sub-sport `backcountry`); a
//! guided off-piste outing is always backcountry. FIT: Garmin FIT SDK, « FIT Protocol »
//! and the « Activity file » messages (file id, timer events, records, lap, session,
//! activity), little endian, CRC of the header and of the file.

use std::path::Path;

use jiff::Timestamp;
use skimap_common::geom::LonLat;

use crate::card::Lang;
use crate::recap::{self, AltitudeSource, JUMP_MS, MOVING_MS};
use crate::tracking::TrackingError;

/// A heart rate sample read for the outing: further than this from a point, the point
/// has none, ms.
const HEART_MATCH_MS: i64 = 15_000;

/// The practice of the outing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum ExportSport {
    /// Cross-country skiing on groomed trails (ski de fond).
    NordicSki,
    /// Nordic backcountry skiing, off the groomed trails (ski de randonnée nordique).
    BackcountrySki,
}

impl ExportSport {
    /// FIT `sport` and `sub_sport`: `cross_country_skiing` (12), `generic` (0) or
    /// `backcountry` (37).
    fn fit(self) -> (u8, u8) {
        match self {
            Self::NordicSki => (12, 0),
            Self::BackcountrySki => (12, 37),
        }
    }

    /// GPX `trk/type`.
    fn gpx(self) -> &'static str {
        match self {
            Self::NordicSki => "cross_country_skiing",
            Self::BackcountrySki => "backcountry_skiing",
        }
    }
}

/// A heart rate sample (Health Connect, from a watch): time, beats per minute.
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Record)]
pub struct HeartSample {
    /// Epoch milliseconds.
    pub t_ms: i64,
    /// Beats per minute.
    pub bpm: u32,
}

/// A point of the exported trace.
#[derive(Debug, Clone, Copy, PartialEq, uniffi::Record)]
pub struct ExportPoint {
    /// Epoch milliseconds.
    pub t_ms: i64,
    /// Latitude, WGS84.
    pub lat: f64,
    /// Longitude.
    pub lon: f64,
    /// Altitude, metres, when known.
    pub ele_m: Option<f64>,
    /// Distance skied from the start, metres.
    pub distance_m: f64,
    /// Heart rate, when a sample is near.
    pub heart_bpm: Option<u32>,
}

/// The outing exported: its files and what the app writes to Health Connect.
#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct ExportedOuting {
    /// Its directory (stable: the outing's id).
    pub dir: String,
    /// Loop, route or point.
    pub title: String,
    /// The practice written.
    pub sport: ExportSport,
    /// First fix, epoch ms.
    pub start_ms: i64,
    /// Last fix, epoch ms.
    pub end_ms: i64,
    /// Skied, metres (the recap's distance).
    pub distance_m: f64,
    /// Moving, seconds.
    pub moving_s: f64,
    /// Climb, metres, when the altitude is known.
    pub ascent_m: Option<f64>,
    /// Descent, metres.
    pub descent_m: Option<f64>,
    /// Mean and highest heart rate over the outing, when read.
    pub heart_avg: Option<u32>,
    /// Highest heart rate.
    pub heart_max: Option<u32>,
    /// Every point, in time order.
    pub points: Vec<ExportPoint>,
    /// The GPX 1.1 file written.
    pub gpx_path: String,
    /// The FIT activity file written.
    pub fit_path: String,
}

fn invalid(detail: impl std::fmt::Display) -> TrackingError {
    TrackingError::Invalid {
        detail: detail.to_string(),
    }
}

/// The instant `ms` after the epoch; the epoch for a time outside the years jiff represents.
fn utc(ms: i64) -> Timestamp {
    Timestamp::from_millisecond(ms).unwrap_or(Timestamp::UNIX_EPOCH)
}

/// Exports the outing of `dir` (recorded by the location service) as GPX and FIT into
/// `out_dir`: the practice `sport` (an off-piste outing is backcountry), the heart rate
/// samples `heart` read for it (may be empty); `work_dir`: cache of the relief of the
/// pack (altitude, as the recap). Files `skimap-<date>-<heure>-<titre>.gpx` and `.fit`.
#[uniffi::export]
pub fn export_outing(
    dir: String,
    work_dir: String,
    out_dir: String,
    sport: ExportSport,
    heart: Vec<HeartSample>,
) -> Result<ExportedOuting, TrackingError> {
    let l = recap::load(Path::new(&dir), Lang::En)
        .filter(|l| l.fixes.len() >= 2)
        .ok_or_else(|| invalid(format!("{dir}: no recorded trace to export")))?;
    let sport = if l.summary.kind == "offpiste" {
        ExportSport::BackcountrySki
    } else {
        sport
    };
    let dem = l
        .pack_dir
        .as_deref()
        .and_then(|p| crate::offpiste::Relief::open(p, Path::new(&work_dir)).ok());
    let dem_fn = dem.as_ref().map(|r| move |p: LonLat| r.at(p));
    let alt = recap::altitude(
        Path::new(&dir),
        &l.fixes,
        dem_fn.as_ref().map(|f| f as &dyn Fn(LonLat) -> Option<f64>),
    );
    let absolute = alt
        .as_ref()
        .filter(|a| a.source != AltitudeSource::Barometer);
    let mut heart: Vec<HeartSample> = heart.into_iter().filter(|h| h.bpm > 0).collect();
    heart.sort_by_key(|h| h.t_ms);
    let points = points(&l.fixes, absolute.map(|a| a.at_fix.as_slice()), &heart);
    let (start_ms, end_ms) = (points[0].t_ms, points[points.len() - 1].t_ms);
    let during: Vec<u32> = heart
        .iter()
        .filter(|h| h.t_ms >= start_ms && h.t_ms <= end_ms)
        .map(|h| h.bpm.min(254))
        .collect();
    let heart_avg = (!during.is_empty()).then(|| {
        (during.iter().map(|&b| f64::from(b)).sum::<f64>() / during.len() as f64).round() as u32
    });
    let mut e = ExportedOuting {
        dir: dir.clone(),
        title: l.summary.title.clone(),
        sport,
        start_ms,
        end_ms,
        distance_m: points[points.len() - 1].distance_m,
        moving_s: l.summary.moving_s,
        ascent_m: alt.as_ref().map(|a| a.ascent_m),
        descent_m: alt.as_ref().map(|a| a.descent_m),
        heart_avg,
        heart_max: during.iter().copied().max(),
        points,
        gpx_path: String::new(),
        fit_path: String::new(),
    };
    let out = Path::new(&out_dir);
    std::fs::create_dir_all(out).map_err(invalid)?;
    let base = file_name(&e.title, start_ms);
    let gpx = out.join(format!("{base}.gpx"));
    let fit = out.join(format!("{base}.fit"));
    skimap_common::output::write_text(&gpx, &gpx_text(&e)).map_err(invalid)?;
    skimap_common::output::write_bytes(&fit, &fit_bytes(&e)).map_err(invalid)?;
    e.gpx_path = gpx.display().to_string();
    e.fit_path = fit.display().to_string();
    Ok(e)
}

/// The points of the fixes: altitude at each fix when absolute, distance cumulated as the
/// recap measures it, the nearest heart rate sample.
fn points(
    fixes: &[(i64, LonLat)],
    ele: Option<&[Option<f64>]>,
    heart: &[HeartSample],
) -> Vec<ExportPoint> {
    let mut order: Vec<usize> = (0..fixes.len()).collect();
    order.sort_by_key(|&i| fixes[i].0);
    let mut d = 0.0;
    let mut prev: Option<(i64, LonLat)> = None;
    order
        .into_iter()
        .map(|i| {
            let (t, p) = fixes[i];
            if let Some((t0, p0)) = prev {
                let dt = (t - t0) as f64 / 1000.0;
                if dt > 0.0 {
                    let step = p0.haversine_m(p);
                    let v = step / dt;
                    if (MOVING_MS..=JUMP_MS).contains(&v) {
                        d += step;
                    }
                }
            }
            prev = Some((t, p));
            ExportPoint {
                t_ms: t,
                lat: p.lat,
                lon: p.lon,
                ele_m: ele.and_then(|z| z.get(i).copied().flatten()),
                distance_m: d,
                heart_bpm: nearest(heart, t),
            }
        })
        .collect()
}

/// The sample nearest to `t` within [`HEART_MATCH_MS`] (`heart` sorted by time).
fn nearest(heart: &[HeartSample], t: i64) -> Option<u32> {
    let i = heart.partition_point(|h| h.t_ms < t);
    [i.checked_sub(1), Some(i)]
        .into_iter()
        .flatten()
        .filter_map(|j| heart.get(j))
        .filter(|h| (h.t_ms - t).abs() <= HEART_MATCH_MS)
        .min_by_key(|h| (h.t_ms - t).abs())
        .map(|h| h.bpm.min(254))
}

/// `skimap-2026-02-15-0930-boucle-du-lac`: the local start and the title in ASCII.
fn file_name(title: &str, start_ms: i64) -> String {
    let local = skimap_core::ephemeris::local_time(utc(start_ms));
    let mut slug = String::new();
    for c in title.to_lowercase().chars() {
        let s = match c {
            'a'..='z' | '0'..='9' => c.to_string(),
            'à' | 'â' | 'ä' | 'á' | 'å' => "a".into(),
            'é' | 'è' | 'ê' | 'ë' => "e".into(),
            'î' | 'ï' | 'í' => "i".into(),
            'ô' | 'ö' | 'ó' => "o".into(),
            'ù' | 'û' | 'ü' | 'ú' => "u".into(),
            'ç' => "c".into(),
            'ñ' => "n".into(),
            'œ' => "oe".into(),
            'æ' => "ae".into(),
            _ => "-".into(),
        };
        if s == "-" && (slug.is_empty() || slug.ends_with('-')) {
            continue;
        }
        slug.push_str(&s);
        if slug.len() >= 40 {
            break;
        }
    }
    let slug = slug.trim_end_matches('-');
    let mut name = format!("skimap-{}", local.strftime("%Y-%m-%d-%H%M"));
    if !slug.is_empty() {
        name.push('-');
        name.push_str(slug);
    }
    name
}

fn xml_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

fn gpx_time(ms: i64) -> String {
    skimap_common::dates::utc_datetime(utc(ms))
        .strftime("%Y-%m-%dT%H:%M:%S%.3fZ")
        .to_string()
}

/// The GPX 1.1 document: one track, one segment; the heart rate in Garmin's
/// TrackPointExtension (v1), which every importer reads.
fn gpx_text(e: &ExportedOuting) -> String {
    let name = xml_escape(&e.title);
    let mut s = String::from(concat!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n",
        "<gpx version=\"1.1\" creator=\"skimap\" xmlns=\"http://www.topografix.com/GPX/1/1\"",
        " xmlns:gpxtpx=\"http://www.garmin.com/xmlschemas/TrackPointExtension/v1\"",
        " xmlns:xsi=\"http://www.w3.org/2001/XMLSchema-instance\"",
        " xsi:schemaLocation=\"http://www.topografix.com/GPX/1/1 http://www.topografix.com/GPX/1/1/gpx.xsd\">\n",
    ));
    s.push_str(&format!(
        "<metadata><name>{name}</name><time>{}</time></metadata>\n",
        gpx_time(e.start_ms)
    ));
    s.push_str(&format!(
        "<trk><name>{name}</name><type>{}</type><trkseg>\n",
        e.sport.gpx()
    ));
    for p in &e.points {
        s.push_str(&format!(
            "<trkpt lat=\"{:.7}\" lon=\"{:.7}\">",
            p.lat, p.lon
        ));
        if let Some(z) = p.ele_m {
            s.push_str(&format!("<ele>{z:.1}</ele>"));
        }
        s.push_str(&format!("<time>{}</time>", gpx_time(p.t_ms)));
        if let Some(hr) = p.heart_bpm {
            s.push_str(&format!(
                "<extensions><gpxtpx:TrackPointExtension><gpxtpx:hr>{hr}</gpxtpx:hr></gpxtpx:TrackPointExtension></extensions>"
            ));
        }
        s.push_str("</trkpt>\n");
    }
    s.push_str("</trkseg></trk>\n</gpx>\n");
    s
}

// ---------------------------------------------------------------------------------------
// FIT
// ---------------------------------------------------------------------------------------

/// Seconds from the Unix epoch to the FIT epoch (1989-12-31T00:00:00Z).
const FIT_EPOCH: i64 = 631_065_600;
/// Profile version written in the header (21.32).
const PROFILE: u16 = 2132;

/// Base types (FIT Protocol, table 7).
const ENUM: u8 = 0x00;
const UINT8: u8 = 0x02;
const UINT16: u8 = 0x84;
const SINT32: u8 = 0x85;
const UINT32: u8 = 0x86;
const UINT32Z: u8 = 0x8C;

const NO_U8: u8 = 0xFF;
const NO_U16: u16 = 0xFFFF;
const NO_U32: u32 = 0xFFFF_FFFF;

/// Global message numbers.
const FILE_ID: u16 = 0;
const SESSION: u16 = 18;
const LAP: u16 = 19;
const RECORD: u16 = 20;
const EVENT: u16 = 21;
const ACTIVITY: u16 = 34;

/// FIT's CRC-16 (FIT Protocol, « CRC »).
fn fit_crc(bytes: &[u8]) -> u16 {
    const T: [u16; 16] = [
        0x0000, 0xCC01, 0xD801, 0x1400, 0xF001, 0x3C00, 0x2800, 0xE401, 0xA001, 0x6C00, 0x7800,
        0xB401, 0x5000, 0x9C01, 0x8801, 0x4400,
    ];
    let mut crc = 0u16;
    for &b in bytes {
        let tmp = T[usize::from(crc & 0xF)];
        crc = ((crc >> 4) & 0x0FFF) ^ tmp ^ T[usize::from(b & 0xF)];
        let tmp = T[usize::from(crc & 0xF)];
        crc = ((crc >> 4) & 0x0FFF) ^ tmp ^ T[usize::from((b >> 4) & 0xF)];
    }
    crc
}

/// FIT timestamp (seconds since the FIT epoch) of epoch milliseconds.
fn fit_time(ms: i64) -> u32 {
    u32::try_from(ms.div_euclid(1000) - FIT_EPOCH).unwrap_or(0)
}

/// Degrees to semicircles.
fn semicircles(deg: f64) -> i32 {
    (deg * (2f64.powi(31) / 180.0)).round() as i32
}

/// A value of a data message.
#[derive(Clone, Copy)]
enum V {
    U8(u8),
    U16(u16),
    I32(i32),
    U32(u32),
    /// `uint32z` (0 is invalid): a serial number.
    U32Z(u32),
}

impl V {
    fn base(self) -> (u8, u8) {
        match self {
            V::U8(_) => (1, UINT8),
            V::U16(_) => (2, UINT16),
            V::I32(_) => (4, SINT32),
            V::U32(_) => (4, UINT32),
            V::U32Z(_) => (4, UINT32Z),
        }
    }
}

/// The data records of a FIT file being written.
struct FitData {
    buf: Vec<u8>,
}

impl FitData {
    /// A definition message for the local type `local`: (field number, base type, size).
    fn define(&mut self, local: u8, global: u16, fields: &[(u8, u8, u8)]) {
        self.buf.push(0x40 | local);
        self.buf.push(0); // reserved
        self.buf.push(0); // little endian
        self.buf.extend_from_slice(&global.to_le_bytes());
        self.buf.push(fields.len() as u8);
        for &(num, base, size) in fields {
            self.buf.extend_from_slice(&[num, size, base]);
        }
    }

    /// A definition and one data message from (field number, value); `enums` are the
    /// field numbers whose `U8` value is a FIT enum.
    fn message(&mut self, local: u8, global: u16, fields: &[(u8, V)], enums: &[u8]) {
        let defs: Vec<(u8, u8, u8)> = fields
            .iter()
            .map(|&(n, v)| {
                let (size, base) = v.base();
                let base = if enums.contains(&n) { ENUM } else { base };
                (n, base, size)
            })
            .collect();
        self.define(local, global, &defs);
        self.data(local, fields);
    }

    fn data(&mut self, local: u8, fields: &[(u8, V)]) {
        self.buf.push(local);
        for &(_, v) in fields {
            match v {
                V::U8(x) => self.buf.push(x),
                V::U16(x) => self.buf.extend_from_slice(&x.to_le_bytes()),
                V::I32(x) => self.buf.extend_from_slice(&x.to_le_bytes()),
                V::U32(x) | V::U32Z(x) => self.buf.extend_from_slice(&x.to_le_bytes()),
            }
        }
    }
}

/// Altitude as FIT stores it: scale 5, offset 500 m.
fn fit_altitude(z: Option<f64>) -> (u16, u32) {
    match z {
        Some(z) => {
            let v = ((z + 500.0) * 5.0).round().max(0.0);
            (
                if v < f64::from(NO_U16) {
                    v as u16
                } else {
                    NO_U16
                },
                v as u32,
            )
        }
        None => (NO_U16, NO_U32),
    }
}

/// The FIT activity file of the outing.
fn fit_bytes(e: &ExportedOuting) -> Vec<u8> {
    let mut f = FitData { buf: Vec::new() };
    let (sport, sub_sport) = e.sport.fit();
    let start = fit_time(e.start_ms);
    let end = fit_time(e.end_ms);
    let first = e.points[0];
    let last = e.points[e.points.len() - 1];
    let elapsed_ms = u32::try_from(e.end_ms - e.start_ms).unwrap_or(0);
    let distance_cm = (e.distance_m * 100.0).round() as u32;
    let climb = |m: Option<f64>| m.map_or(NO_U16, |m| m.round().clamp(0.0, 65_534.0) as u16);
    let hr = |b: Option<u32>| b.map_or(NO_U8, |b| b.min(254) as u8);
    // File id: an activity (4) of a development manufacturer (255), its start as serial.
    f.message(
        0,
        FILE_ID,
        &[
            (0, V::U8(4)),
            (1, V::U16(255)),
            (2, V::U16(1)),
            (3, V::U32Z(start.max(1))),
            (4, V::U32(start)),
        ],
        &[0],
    );
    // Timer start (event timer 0, type start 0).
    f.message(
        1,
        EVENT,
        &[
            (253, V::U32(start)),
            (0, V::U8(0)),
            (1, V::U8(0)),
            (4, V::U8(0)),
        ],
        &[0, 1],
    );
    let record_fields = [
        (253, UINT32, 4),
        (0, SINT32, 4),
        (1, SINT32, 4),
        (5, UINT32, 4),
        (2, UINT16, 2),
        (78, UINT32, 4),
        (3, UINT8, 1),
    ];
    f.define(2, RECORD, &record_fields);
    for p in &e.points {
        let (alt, enhanced) = fit_altitude(p.ele_m);
        f.data(
            2,
            &[
                (253, V::U32(fit_time(p.t_ms))),
                (0, V::I32(semicircles(p.lat))),
                (1, V::I32(semicircles(p.lon))),
                (5, V::U32((p.distance_m * 100.0).round() as u32)),
                (2, V::U16(alt)),
                (78, V::U32(enhanced)),
                (3, V::U8(hr(p.heart_bpm))),
            ],
        );
    }
    // Timer stop (event timer 0, type stop_all 4).
    f.data(
        1,
        &[
            (253, V::U32(end)),
            (0, V::U8(0)),
            (1, V::U8(4)),
            (4, V::U8(0)),
        ],
    );
    // One lap: the whole outing (event lap 9, type stop 1).
    f.message(
        3,
        LAP,
        &[
            (253, V::U32(end)),
            (0, V::U8(9)),
            (1, V::U8(1)),
            (2, V::U32(start)),
            (3, V::I32(semicircles(first.lat))),
            (4, V::I32(semicircles(first.lon))),
            (5, V::I32(semicircles(last.lat))),
            (6, V::I32(semicircles(last.lon))),
            (7, V::U32(elapsed_ms)),
            (8, V::U32(elapsed_ms)),
            (9, V::U32(distance_cm)),
            (21, V::U16(climb(e.ascent_m))),
            (22, V::U16(climb(e.descent_m))),
            (15, V::U8(hr(e.heart_avg))),
            (16, V::U8(hr(e.heart_max))),
            (25, V::U8(sport)),
            (39, V::U8(sub_sport)),
            (254, V::U16(0)),
        ],
        &[0, 1, 25, 39],
    );
    // The session (event session 8, type stop 1, trigger activity_end 0).
    f.message(
        4,
        SESSION,
        &[
            (253, V::U32(end)),
            (0, V::U8(8)),
            (1, V::U8(1)),
            (2, V::U32(start)),
            (3, V::I32(semicircles(first.lat))),
            (4, V::I32(semicircles(first.lon))),
            (5, V::U8(sport)),
            (6, V::U8(sub_sport)),
            (7, V::U32(elapsed_ms)),
            (8, V::U32(elapsed_ms)),
            (9, V::U32(distance_cm)),
            (22, V::U16(climb(e.ascent_m))),
            (23, V::U16(climb(e.descent_m))),
            (16, V::U8(hr(e.heart_avg))),
            (17, V::U8(hr(e.heart_max))),
            (25, V::U16(0)),
            (26, V::U16(1)),
            (28, V::U8(0)),
            (254, V::U16(0)),
        ],
        &[0, 1, 5, 6, 28],
    );
    // The activity: one session, manual (0), event activity 26, type stop 1; local time
    // of the end (French legal time).
    let local_end = skimap_common::dates::utc(skimap_core::ephemeris::local_time(utc(e.end_ms)))
        .map_or(e.end_ms, Timestamp::as_millisecond);
    f.message(
        5,
        ACTIVITY,
        &[
            (253, V::U32(end)),
            (0, V::U32(elapsed_ms)),
            (1, V::U16(1)),
            (2, V::U8(0)),
            (3, V::U8(26)),
            (4, V::U8(1)),
            (5, V::U32(fit_time(local_end))),
        ],
        &[2, 3, 4],
    );
    let data = f.buf;
    let mut out = Vec::with_capacity(14 + data.len() + 2);
    out.push(14);
    out.push(0x20); // protocol 2.0
    out.extend_from_slice(&PROFILE.to_le_bytes());
    out.extend_from_slice(&(data.len() as u32).to_le_bytes());
    out.extend_from_slice(b".FIT");
    let header_crc = fit_crc(&out);
    out.extend_from_slice(&header_crc.to_le_bytes());
    out.extend_from_slice(&data);
    let crc = fit_crc(&out);
    out.extend_from_slice(&crc.to_le_bytes());
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fog::{FogTarget, GuidedKind, GuidedSpec, guided_bearing};
    use crate::tracking::INPUTS_FILE;
    use rustyfit::profile::mesgdef;
    use rustyfit::profile::typedef::MesgNum;
    use rustyfit::proto::Message;
    use skimap_plan::tracker::Input;

    /// 2026-02-15 09:00 UTC.
    const T0: i64 = 1_771_146_000_000;

    /// A guided outing in `dir` with `n` fixes every 5 s, 12 m apart northwards (2.4 m/s),
    /// a 60 s stop in the middle and one GPS jump.
    fn outing(dir: &Path, n: usize) -> Vec<(i64, LonLat)> {
        guided_bearing(
            dir.display().to_string(),
            FogTarget {
                kind: "refuge".into(),
                name: "Refuge d'été & <test>".into(),
                lon: 6.0,
                lat: 45.05,
                distance_m: None,
            },
            None,
            0.0,
            Lang::Fr,
            None,
            None,
        )
        .unwrap();
        let mut fixes = Vec::new();
        let mut north = 0.0;
        let mut t = T0;
        for i in 0..n {
            if i == n / 2 {
                t += 60_000;
            }
            // The jump: 1 km away and back.
            let jump = if i == n / 3 { 1000.0 } else { 0.0 };
            fixes.push((t + 123, LonLat::new(6.0, 45.0 + (north + jump) / 111_195.0)));
            north += 12.0;
            t += 5000;
        }
        let mut s = String::new();
        for (t, pos) in &fixes {
            let t = utc(*t);
            s.push_str(&serde_json::to_string(&Input::Fix { t, pos: *pos }).unwrap());
            s.push('\n');
        }
        std::fs::write(dir.join(INPUTS_FILE), s).unwrap();
        fixes
    }

    /// The messages of a FIT file decoded by rustyfit, an implementation of the protocol
    /// independent of ours (header and file CRCs checked, components expanded).
    fn decode(bytes: &[u8]) -> Vec<Message> {
        let fit = rustyfit::Decoder::new()
            .decode(bytes)
            .expect("FIT decoded, CRCs checked")
            .expect("one FIT sequence");
        fit.messages
    }

    fn of(msgs: &[Message], k: MesgNum) -> impl Iterator<Item = &Message> {
        msgs.iter().filter(move |m| m.num == k)
    }

    fn time_s(t: rustyfit::profile::typedef::DateTime) -> i64 {
        t.unix_timestamp().expect("valid FIT time")
    }

    #[test]
    fn fit_read_back_by_an_independent_decoder() {
        let root = tempfile::tempdir().unwrap();
        let dir = root.path().join("outing");
        let fixes = outing(&dir, 120);
        // A watch's heart rate every 10 s from before the start, one far after the end.
        let mut heart: Vec<HeartSample> = (0..80)
            .map(|i| HeartSample {
                t_ms: T0 - 20_000 + i * 10_000,
                bpm: 120 + (i % 30) as u32,
            })
            .collect();
        heart.push(HeartSample {
            t_ms: T0 + 3_600_000 * 5,
            bpm: 200,
        });
        let out = root.path().join("share");
        let e = export_outing(
            dir.display().to_string(),
            root.path().join("work").display().to_string(),
            out.display().to_string(),
            ExportSport::NordicSki,
            heart.clone(),
        )
        .unwrap();
        // The recap's distance: the stop and the GPS jump left out.
        let (d, moving, _) = recap::measure(&fixes);
        assert!((e.distance_m - d).abs() < 1e-6, "{} {d}", e.distance_m);
        assert!(
            (e.distance_m - (119.0 * 12.0 - 36.0)).abs() < 1.0,
            "{}",
            e.distance_m
        );
        assert_eq!(e.moving_s, moving);
        assert_eq!(e.points.len(), 120);
        assert_eq!((e.start_ms, e.end_ms), (fixes[0].0, fixes[119].0));
        assert_eq!(e.sport, ExportSport::NordicSki);
        // No barometer and no relief: no altitude, no climb.
        assert!(e.points.iter().all(|p| p.ele_m.is_none()));
        assert_eq!(e.ascent_m, None);
        // Heart rate of the hours skied only.
        let during: Vec<u32> = heart
            .iter()
            .filter(|h| h.t_ms >= e.start_ms && h.t_ms <= e.end_ms)
            .map(|h| h.bpm)
            .collect();
        assert_eq!(e.heart_max, during.iter().copied().max());
        assert!(e.heart_max.unwrap() < 200);
        assert!(e.points.iter().all(|p| p.heart_bpm.is_some()));
        assert!(
            e.fit_path
                .ends_with("skimap-2026-02-15-1000-cap-vers-refuge-d-ete-test.fit"),
            "{}",
            e.fit_path
        );

        let bytes = std::fs::read(&e.fit_path).unwrap();
        let msgs = decode(&bytes);
        let file_id = mesgdef::FileId::from(of(&msgs, MesgNum::FILE_ID).next().unwrap());
        assert_eq!(file_id.r#type.to_string(), "activity");
        let records: Vec<_> = of(&msgs, MesgNum::RECORD)
            .map(mesgdef::Record::from)
            .collect();
        assert_eq!(records.len(), e.points.len());
        for (r, p) in records.iter().zip(&e.points) {
            assert_eq!(time_s(r.timestamp), p.t_ms.div_euclid(1000));
            let lat = r.position_lat_degrees().unwrap();
            let lon = r.position_long_degrees().unwrap();
            assert!(
                (lat - p.lat).abs() < 1e-7 && (lon - p.lon).abs() < 1e-7,
                "{lat} {lon} {p:?}"
            );
            let d = r.distance_scaled().unwrap();
            assert!((d - p.distance_m).abs() < 0.006, "{p:?}");
            assert_eq!(u32::from(r.heart_rate), p.heart_bpm.unwrap());
            assert!(r.enhanced_altitude_scaled().is_none() && r.altitude_scaled().is_none());
        }
        let session = mesgdef::Session::from(of(&msgs, MesgNum::SESSION).next().unwrap());
        assert_eq!(session.sport.to_string(), "cross_country_skiing");
        assert_eq!(session.sub_sport.to_string(), "generic");
        let total = session.total_distance_scaled().unwrap();
        assert!((total - e.distance_m).abs() < 0.006);
        let elapsed = (e.end_ms - e.start_ms) as f64 / 1000.0;
        let total_elapsed = session.total_elapsed_time_scaled().unwrap();
        assert!((total_elapsed - elapsed).abs() < 1e-3);
        assert_eq!(time_s(session.start_time), e.start_ms.div_euclid(1000));
        assert_eq!(u32::from(session.avg_heart_rate), e.heart_avg.unwrap());
        assert_eq!(u32::from(session.max_heart_rate), e.heart_max.unwrap());
        assert_eq!(session.total_ascent, u16::MAX, "no climb written");
        let lap = mesgdef::Lap::from(of(&msgs, MesgNum::LAP).next().unwrap());
        assert!((lap.total_distance_scaled().unwrap() - e.distance_m).abs() < 0.006);
        let activity = mesgdef::Activity::from(of(&msgs, MesgNum::ACTIVITY).next().unwrap());
        assert_eq!(activity.num_sessions, 1);
        assert_eq!(of(&msgs, MesgNum::EVENT).count(), 2);

        // The GPX: the same points and times, read back.
        let gpx = std::fs::read_to_string(&e.gpx_path).unwrap();
        assert!(gpx.contains("<name>Cap vers Refuge d'été &amp; &lt;test&gt;</name>"));
        assert!(gpx.contains("<type>cross_country_skiing</type>"));
        let pts = skimap_collab::track::parse_xml(&gpx).unwrap();
        assert_eq!(pts.len(), e.points.len());
        for (g, p) in pts.iter().zip(&e.points) {
            assert_eq!(g.t.unwrap().as_millisecond(), p.t_ms);
            assert!((g.lat - p.lat).abs() < 1e-7 && (g.lon - p.lon).abs() < 1e-7);
        }
        assert_eq!(gpx.matches("<gpxtpx:hr>").count(), e.points.len());
    }

    #[test]
    fn altitude_climb_and_backcountry_written() {
        let root = tempfile::tempdir().unwrap();
        let dir = root.path().join("offpiste");
        let fixes = outing(&dir, 40);
        // A guided off-piste line: always backcountry, whatever the practice set.
        let mut spec = GuidedSpec::read(&dir).unwrap();
        spec.kind = GuidedKind::Line;
        std::fs::write(
            dir.join("guided.json"),
            serde_json::to_string(&spec).unwrap(),
        )
        .unwrap();
        let e = export_outing(
            dir.display().to_string(),
            root.path().join("work").display().to_string(),
            root.path().join("share").display().to_string(),
            ExportSport::NordicSki,
            Vec::new(),
        )
        .unwrap();
        assert_eq!(e.sport, ExportSport::BackcountrySki);
        assert!(e.points.iter().all(|p| p.heart_bpm.is_none()));
        assert_eq!(e.heart_avg, None);

        // Altitude climbing 2 m per point, as the relief gives it.
        let ele: Vec<Option<f64>> = (0..fixes.len())
            .map(|i| Some(1480.5 + 2.0 * i as f64))
            .collect();
        let mut with = e.clone();
        with.points = points(&fixes, Some(&ele), &[]);
        with.ascent_m = Some(78.0);
        with.descent_m = Some(0.0);
        let msgs = decode(&fit_bytes(&with));
        let records: Vec<&Message> = of(&msgs, MesgNum::RECORD).collect();
        assert_eq!(records.len(), with.points.len());
        for (m, p) in records.iter().zip(&with.points) {
            // `altitude` (2, for the older readers) written, and expanded into
            // `enhanced_altitude` (78) by the profile: both equal.
            let written = |num: u8| m.fields.iter().any(|f| f.num == num && !f.is_expanded);
            assert!(written(mesgdef::Record::ALTITUDE), "{p:?}");
            let r = mesgdef::Record::from(*m);
            let z = [
                r.altitude_scaled().unwrap(),
                r.enhanced_altitude_scaled().unwrap(),
            ];
            assert!(
                z.iter().all(|z| (z - p.ele_m.unwrap()).abs() < 0.1001),
                "{z:?} {p:?}"
            );
            assert_eq!(r.heart_rate, u8::MAX, "no heart rate written");
        }
        let session = mesgdef::Session::from(of(&msgs, MesgNum::SESSION).next().unwrap());
        assert_eq!(session.sport.to_string(), "cross_country_skiing");
        assert_eq!(session.sub_sport.to_string(), "backcountry");
        assert_eq!(session.total_ascent, 78);
        assert_eq!(session.total_descent, 0);
        let gpx = gpx_text(&with);
        assert!(gpx.contains("<type>backcountry_skiing</type>"));
        let pts = skimap_collab::track::parse_xml(&gpx).unwrap();
        assert_eq!(pts.len(), with.points.len());
        for (g, p) in pts.iter().zip(&with.points) {
            assert!((g.ele.unwrap() - p.ele_m.unwrap()).abs() < 0.051);
        }
    }

    #[test]
    fn no_trace_no_export() {
        let root = tempfile::tempdir().unwrap();
        let dir = root.path().join("o");
        outing(&dir, 1);
        let r = export_outing(
            dir.display().to_string(),
            String::new(),
            root.path().join("share").display().to_string(),
            ExportSport::NordicSki,
            Vec::new(),
        );
        assert!(r.is_err());
    }

    #[test]
    fn names_and_crc() {
        assert_eq!(file_name("", T0), "skimap-2026-02-15-1000");
        assert_eq!(
            file_name("Œuvre — Col  d'Izoard!", T0),
            "skimap-2026-02-15-1000-oeuvre-col-d-izoard"
        );
        // The check value of FIT's CRC-16 (CRC-16/ARC) on « 123456789 ».
        assert_eq!(fit_crc(b"123456789"), 0xBB3D);
    }
}
