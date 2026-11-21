//! Tracks of the outings on the phone (V13 #76, V15 #78): read from GPX (the traces of
//! the app, `trace.gpx`, and Strava's exports), TCX and FIT ([`crate::fit`]); summarised;
//! kept as an `outing.json` beside them; turned into what the calibration learns (C11,
//! [`OutingObs`]).

use std::path::Path;

use jiff::Timestamp;
use quick_xml::Reader;
use quick_xml::events::Event;
use serde::{Deserialize, Serialize};
use skimap_core::calibration::{OutingObs, TERRAINS, Terrain};
use skimap_core::eta::EtaModel;
use skimap_core::routing::Style;

/// A point of a track.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Point {
    /// Longitude, WGS84.
    pub lon: f64,
    /// Latitude.
    pub lat: f64,
    /// Elevation, m, when recorded.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ele: Option<f64>,
    /// Time, when recorded.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub t: Option<Timestamp>,
}

/// Errors reading a track.
#[derive(Debug, thiserror::Error)]
pub enum TrackError {
    /// Unreadable file.
    #[error("{0}")]
    Io(#[from] std::io::Error),
    /// Malformed content.
    #[error("{0}")]
    Format(String),
}

/// Mean Earth radius, m.
const R: f64 = 6_371_008.8;

/// Great-circle distance, m.
pub fn haversine(a: (f64, f64), b: (f64, f64)) -> f64 {
    let (la1, la2) = (a.1.to_radians(), b.1.to_radians());
    let dla = la2 - la1;
    let dlo = (b.0 - a.0).to_radians();
    let h = (dla / 2.0).sin().powi(2) + la1.cos() * la2.cos() * (dlo / 2.0).sin().powi(2);
    2.0 * R * h.sqrt().min(1.0).asin()
}

fn valid(lon: f64, lat: f64) -> bool {
    lon.is_finite()
        && lat.is_finite()
        && lon.abs() <= 180.0
        && lat.abs() <= 90.0
        && (lon, lat) != (0.0, 0.0)
}

/// Points of a GPX (track points, then route points if no track) or TCX document.
pub fn parse_xml(xml: &str) -> Result<Vec<Point>, TrackError> {
    let mut r = Reader::from_str(xml.trim_start_matches('\u{feff}').trim_start());
    r.config_mut().trim_text(true);
    let mut out = Vec::new();
    let mut cur: Option<Point> = None;
    // Text element being read: "ele", "time", "lat", "lon" (TCX), "alt" (TCX).
    let mut field: Option<&'static str> = None;
    let attr = |e: &quick_xml::events::BytesStart<'_>, k: &str| -> Option<f64> {
        e.attributes()
            .flatten()
            .find(|a| a.key.local_name().as_ref() == k)
            .and_then(|a| a.value.trim().parse().ok())
    };
    loop {
        match r.read_event() {
            Ok(Event::Empty(e)) => {
                // A self-closing point: no elevation, no time.
                if matches!(e.local_name().as_ref(), "trkpt" | "rtept")
                    && let (Some(lat), Some(lon)) = (attr(&e, "lat"), attr(&e, "lon"))
                    && valid(lon, lat)
                {
                    out.push(Point {
                        lon,
                        lat,
                        ele: None,
                        t: None,
                    });
                }
            }
            Ok(Event::Start(e)) => {
                let name = e.local_name();
                match name.as_ref() {
                    "trkpt" | "rtept" => {
                        cur = match (attr(&e, "lat"), attr(&e, "lon")) {
                            (Some(lat), Some(lon)) => Some(Point {
                                lon,
                                lat,
                                ele: None,
                                t: None,
                            }),
                            _ => None,
                        };
                    }
                    "Trackpoint" => {
                        cur = Some(Point {
                            lon: f64::NAN,
                            lat: f64::NAN,
                            ele: None,
                            t: None,
                        })
                    }
                    "ele" => field = Some("ele"),
                    "AltitudeMeters" => field = Some("ele"),
                    "time" | "Time" => field = Some("time"),
                    "LatitudeDegrees" => field = Some("lat"),
                    "LongitudeDegrees" => field = Some("lon"),
                    _ => {}
                }
            }
            Ok(Event::Text(t)) => {
                if let (Some(f), Some(p)) = (field, cur.as_mut()) {
                    let s = t.trim();
                    match f {
                        "ele" => p.ele = s.parse().ok().filter(|v: &f64| v.is_finite()),
                        "time" => p.t = s.parse::<Timestamp>().ok(),
                        "lat" => p.lat = s.parse().unwrap_or(f64::NAN),
                        "lon" => p.lon = s.parse().unwrap_or(f64::NAN),
                        _ => {}
                    }
                }
            }
            Ok(Event::End(e)) => {
                let name = e.local_name();
                match name.as_ref() {
                    "trkpt" | "rtept" | "Trackpoint" => {
                        if let Some(p) = cur.take()
                            && valid(p.lon, p.lat)
                        {
                            out.push(p);
                        }
                    }
                    _ => field = None,
                }
            }
            Ok(Event::Eof) => break,
            Err(e) => return Err(TrackError::Format(format!("XML: {e}"))),
            _ => {}
        }
    }
    Ok(out)
}

/// Summary of an outing (`outing.json`), with its track thinned to one point every
/// [`THIN_M`] metres (for the challenges and the list).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Outing {
    /// Identifier (`strava-<id>`, or the directory of a plan).
    pub id: String,
    /// `skimap` (traced by the app) or `strava` (imported).
    pub source: String,
    /// Name.
    pub name: String,
    /// Activity type as the source says it (`Nordic Ski`, `nordic_skiing`).
    pub activity: String,
    /// Start, when the track has times.
    pub start: Option<Timestamp>,
    /// Elapsed time, s.
    pub elapsed_s: f64,
    /// Moving time, s (stops of 90 s or more removed).
    pub moving_s: f64,
    /// Distance, m.
    pub distance_m: f64,
    /// Climb, m.
    pub gain_m: f64,
    /// Thinned track, `[lon, lat]` rounded to 6 decimals.
    pub track: Vec<[f64; 2]>,
}

/// Spacing of the thinned track, m.
pub const THIN_M: f64 = 10.0;
/// A stop: at least this long below [`STOP_SPEED`] (the tracker's pause rule, J3).
pub const STOP_S: f64 = 90.0;
/// Speed under which the skier is stopped, m/s.
pub const STOP_SPEED: f64 = 0.4;
/// Length of the pieces over which a grade is measured, m (GPS elevation noise).
pub const GRADE_PIECE_M: f64 = 100.0;

/// The thinned track.
pub fn thin(points: &[Point]) -> Vec<[f64; 2]> {
    let mut out: Vec<[f64; 2]> = Vec::new();
    let r6 = |v: f64| (v * 1e6).round() / 1e6;
    for (i, p) in points.iter().enumerate() {
        let keep = match out.last() {
            None => true,
            Some(l) => haversine((l[0], l[1]), (p.lon, p.lat)) >= THIN_M || i + 1 == points.len(),
        };
        if keep {
            out.push([r6(p.lon), r6(p.lat)]);
        }
    }
    out
}

/// Moving pieces and stops of a timed track.
struct Segmentation {
    /// Moving steps: (distance m, dz m or 0, dt s, elevation).
    moving: Vec<(f64, f64, f64, Option<f64>)>,
    stops: Vec<f64>,
    elapsed: f64,
}

fn segment(points: &[Point]) -> Segmentation {
    let mut moving = Vec::new();
    let mut stops = Vec::new();
    // A run of slow steps: its duration and its steps (moving again if shorter than STOP_S).
    let mut slow: Vec<(f64, f64, f64, Option<f64>)> = Vec::new();
    let mut slow_s = 0.0;
    let timed: Vec<&Point> = points.iter().filter(|p| p.t.is_some()).collect();
    let elapsed = match (timed.first(), timed.last()) {
        (Some(a), Some(b)) => {
            (b.t.unwrap_or_default() - a.t.unwrap_or_default())
                .as_millis()
                .max(0) as f64
                / 1000.0
        }
        _ => 0.0,
    };
    for w in timed.windows(2) {
        let (a, b) = (w[0], w[1]);
        let dt =
            (b.t.unwrap_or_default() - a.t.unwrap_or_default()).as_millis() as i64 as f64 / 1000.0;
        if dt <= 0.0 {
            continue;
        }
        let d = haversine((a.lon, a.lat), (b.lon, b.lat));
        let dz = match (a.ele, b.ele) {
            (Some(x), Some(y)) => y - x,
            _ => 0.0,
        };
        let step = (d, dz, dt, b.ele);
        if d / dt < STOP_SPEED {
            slow.push(step);
            slow_s += dt;
        } else {
            if slow_s >= STOP_S {
                stops.push(slow_s);
            } else {
                moving.append(&mut slow);
            }
            slow.clear();
            slow_s = 0.0;
            moving.push(step);
        }
    }
    if slow_s >= STOP_S {
        stops.push(slow_s);
    } else {
        moving.append(&mut slow);
    }
    Segmentation {
        moving,
        stops,
        elapsed,
    }
}

/// Summary of a track.
pub fn summarise(id: &str, source: &str, name: &str, activity: &str, points: &[Point]) -> Outing {
    let seg = segment(points);
    let distance_m: f64 = points
        .windows(2)
        .map(|w| haversine((w[0].lon, w[0].lat), (w[1].lon, w[1].lat)))
        .sum();
    let gain_m = gain(points);
    Outing {
        id: id.to_owned(),
        source: source.to_owned(),
        name: name.to_owned(),
        activity: activity.to_owned(),
        start: points.iter().find_map(|p| p.t),
        elapsed_s: seg.elapsed,
        moving_s: seg.moving.iter().map(|s| s.2).sum(),
        distance_m,
        gain_m,
        track: thin(points),
    }
}

/// Climb with a 3 m hysteresis (GPS and barometer noise).
fn gain(points: &[Point]) -> f64 {
    let mut total = 0.0;
    let mut base: Option<f64> = None;
    for e in points.iter().filter_map(|p| p.ele) {
        match base {
            None => base = Some(e),
            Some(b) if e > b + 3.0 => {
                total += e - b;
                base = Some(e);
            }
            Some(b) if e < b - 3.0 => base = Some(e),
            _ => {}
        }
    }
    total
}

/// What the calibration (C11) learns from a timed track: moving time and C5 model time
/// per terrain, over pieces of about [`GRADE_PIECE_M`]; the stops; the distance as the
/// progress (no planned route). `None` without times or with under 10 minutes moving.
pub fn observation(points: &[Point], style: Style, backcountry: bool) -> Option<OutingObs> {
    let seg = segment(points);
    let eta = EtaModel::new(style);
    let mut o = OutingObs {
        stops_s: seg.stops.clone(),
        elapsed_s: seg.elapsed,
        ..Default::default()
    };
    let (mut d, mut dz, mut dt, mut elev) = (0.0, 0.0, 0.0, None);
    let flush = |d: f64, dz: f64, dt: f64, elev: Option<f64>, o: &mut OutingObs| {
        if d <= 0.0 || dt <= 0.0 {
            return;
        }
        let grade = dz / d;
        let v = eta.speed_ms(grade, elev.unwrap_or(eta.default_elev_m), backcountry);
        let model = d.hypot(dz) / v;
        let k = Terrain::of_grade(grade) as usize;
        debug_assert!(k < TERRAINS);
        o.model_s[k] += model;
        o.moving_s[k] += dt;
        o.travelled_m += d;
        o.progress_m += d;
    };
    for &(sd, sdz, sdt, se) in &seg.moving {
        d += sd;
        dz += sdz;
        dt += sdt;
        elev = se.or(elev);
        if d >= GRADE_PIECE_M {
            flush(d, dz, dt, elev, &mut o);
            (d, dz, dt) = (0.0, 0.0, 0.0);
        }
    }
    flush(d, dz, dt, elev, &mut o);
    (o.moving_total_s() >= 600.0).then_some(o)
}

/// File name of the summary beside a track.
pub const OUTING_FILE: &str = "outing.json";
/// File name of the calibration input beside a track.
pub const OBSERVATION_FILE: &str = "observation.json";

/// Reads `outing.json` in `dir`, or makes it from `trace.gpx` (an outing traced by the app)
/// and writes it. `None`: no outing in that directory.
pub fn load_or_summarise(dir: &Path) -> Option<Outing> {
    let f = dir.join(OUTING_FILE);
    if let Ok(s) = std::fs::read_to_string(&f)
        && let Ok(o) = serde_json::from_str::<Outing>(&s)
    {
        return Some(o);
    }
    let gpx = std::fs::read_to_string(dir.join("trace.gpx")).ok()?;
    let points = parse_xml(&gpx).ok()?;
    if points.len() < 2 {
        return None;
    }
    let id = dir.file_name()?.to_string_lossy().into_owned();
    let o = summarise(&id, "skimap", &id, "nordic_skiing", &points);
    if let Ok(json) = serde_json::to_string(&o) {
        let _ = skimap_common::output::write_text(&f, &json);
    }
    Some(o)
}

#[cfg(test)]
mod tests {
    use super::*;
    use jiff::tz::Offset;

    /// A timed track going north at `speed` m/s for `n` seconds, one point a second,
    /// climbing `grade`.
    pub(crate) fn line(n: usize, speed: f64, grade: f64) -> Vec<Point> {
        let t0: Timestamp = "2026-01-10T09:00:00Z".parse().unwrap();
        let dlat = speed / 111_195.0;
        (0..n)
            .map(|i| Point {
                lon: 6.77,
                lat: 44.85 + dlat * i as f64,
                ele: Some(1800.0 + grade * speed * i as f64),
                t: Some(t0 + jiff::SignedDuration::from_secs(i as i64)),
            })
            .collect()
    }

    #[test]
    fn gpx_of_the_app_and_tcx() {
        let gpx = r#"<?xml version="1.0"?><gpx xmlns="http://www.topografix.com/GPX/1/1"><trk><trkseg>
<trkpt lat="44.85" lon="6.77"><ele>1800.5</ele><time>2026-01-10T09:00:00.000Z</time></trkpt>
<trkpt lat="44.851" lon="6.771"><time>2026-01-10T09:01:00Z</time></trkpt>
<trkpt lat="0" lon="0"/>
</trkseg></trk></gpx>"#;
        let p = parse_xml(gpx).unwrap();
        assert_eq!(p.len(), 2);
        assert_eq!(p[0].ele, Some(1800.5));
        assert_eq!(
            p[1].t.unwrap().display_with_offset(Offset::UTC).to_string(),
            "2026-01-10T09:01:00+00:00"
        );
        let tcx = r#"
  <?xml version="1.0" encoding="UTF-8"?><TrainingCenterDatabase><Activities><Activity Sport="Other"><Lap><Track>
<Trackpoint><Time>2026-01-10T09:00:00Z</Time><Position><LatitudeDegrees>46.6</LatitudeDegrees><LongitudeDegrees>6.1</LongitudeDegrees></Position><AltitudeMeters>1090</AltitudeMeters></Trackpoint>
<Trackpoint><Time>2026-01-10T09:00:05Z</Time></Trackpoint>
<Trackpoint><Time>2026-01-10T09:00:10Z</Time><Position><LatitudeDegrees>46.601</LatitudeDegrees><LongitudeDegrees>6.1</LongitudeDegrees></Position></Trackpoint>
</Track></Lap></Activity></Activities></TrainingCenterDatabase>"#;
        let p = parse_xml(tcx).unwrap();
        assert_eq!(p.len(), 2, "the point without position skipped");
        assert_eq!((p[0].lat, p[0].ele), (46.6, Some(1090.0)));
    }

    #[test]
    fn summary_stops_and_thinning() {
        // 20 min at 3 m/s, a 5 min stop, 10 min at 3 m/s.
        let mut p = line(1200, 3.0, 0.0);
        let last = *p.last().unwrap();
        for i in 1..=300 {
            p.push(Point {
                t: Some(last.t.unwrap() + jiff::SignedDuration::from_secs(i)),
                ..last
            });
        }
        let after = *p.last().unwrap();
        for (i, q) in line(600, 3.0, 0.0).into_iter().enumerate().skip(1) {
            p.push(Point {
                lat: after.lat + (q.lat - 44.85),
                t: Some(after.t.unwrap() + jiff::SignedDuration::from_secs(i as i64)),
                ..q
            });
        }
        let o = summarise("x", "strava", "Sortie", "Nordic Ski", &p);
        assert!(
            (o.distance_m - 3.0 * 1798.0).abs() < 10.0,
            "{}",
            o.distance_m
        );
        assert!((o.elapsed_s - 2098.0).abs() < 1e-6);
        assert!((o.moving_s - 1798.0).abs() < 1.0, "{}", o.moving_s);
        // Points every 3 m: one kept every 4 (12 m).
        let n = o.track.len() as f64;
        assert!((n - o.distance_m / 12.0).abs() < 3.0, "{n}");
        let obs = observation(&p, Style::Classic, false).unwrap();
        assert_eq!(obs.stops_s, vec![300.0]);
        assert!((obs.moving_total_s() - 1798.0).abs() < 1.0);
        assert!(obs.moving_s[Terrain::Flat as usize] > 1700.0);
        // C5 model on the flat ≈ 10.6 km/h = 2.94 m/s: model time ≈ the real time at 3 m/s.
        let ratio = obs.model_total_s() / obs.moving_total_s();
        assert!((0.9..1.1).contains(&ratio), "{ratio}");
    }

    #[test]
    fn uphill_is_classed_up_and_short_outings_are_not_learned() {
        let p = line(1200, 1.2, 0.06);
        let obs = observation(&p, Style::Classic, false).unwrap();
        assert!(obs.moving_s[Terrain::Up as usize] > 1100.0);
        assert!(gain(&p) > 80.0);
        assert!(observation(&line(300, 3.0, 0.0), Style::Classic, false).is_none());
        let untimed: Vec<Point> = p.iter().map(|q| Point { t: None, ..*q }).collect();
        assert!(observation(&untimed, Style::Classic, false).is_none());
    }

    #[test]
    fn outing_json_written_beside_the_trace() {
        let dir = tempfile::tempdir().unwrap();
        let plan = dir.path().join("2026-01-10-1");
        std::fs::create_dir_all(&plan).unwrap();
        let mut gpx = String::from("<gpx><trk><trkseg>");
        for q in line(700, 3.0, 0.0) {
            gpx.push_str(&format!(
                "<trkpt lat=\"{}\" lon=\"{}\"><time>{}</time></trkpt>",
                q.lat,
                q.lon,
                q.t.unwrap().display_with_offset(Offset::UTC)
            ));
        }
        gpx.push_str("</trkseg></trk></gpx>");
        std::fs::write(plan.join("trace.gpx"), gpx).unwrap();
        let o = load_or_summarise(&plan).unwrap();
        assert_eq!(
            (o.id.as_str(), o.source.as_str()),
            ("2026-01-10-1", "skimap")
        );
        assert!(plan.join(OUTING_FILE).is_file());
        assert_eq!(load_or_summarise(&plan).unwrap(), o);
        assert!(load_or_summarise(dir.path()).is_none());
    }
}
