//! Sun times and French / Swiss legal time, shared by the zone rules (C8, night periods)
//! and the turnaround time (C7, sunset and civil dusk).
//!
//! Sun times use the sunrise equation: solar declination and equation of time from the
//! mean anomaly, hour angle of the sun at a given altitude of its centre. Sunrise and
//! sunset take −0.833° (refraction 34′ plus the solar semi-diameter 16′); civil twilight
//! ends at −6°, nautical at −12°. Within about a minute of the NOAA algorithm (astral 3.2,
//! see the tests). Sea-level horizon: relief hides the sun earlier in a valley and the
//! light under forest fades before civil dusk.

use chrono::{DateTime, Datelike, Duration, NaiveDate, NaiveDateTime, TimeZone, Utc, Weekday};
use skimap_common::geom::LonLat;

/// Altitude of the sun's centre at sunrise and sunset, degrees.
pub const SUNSET_ALTITUDE_DEG: f64 = -0.833;
/// Altitude of the sun's centre at the end of civil twilight, degrees.
pub const CIVIL_ALTITUDE_DEG: f64 = -6.0;
/// Altitude of the sun's centre at the end of nautical twilight, degrees.
pub const NAUTICAL_ALTITUDE_DEG: f64 = -12.0;

/// Last Sunday of a month.
pub(crate) fn last_sunday(year: i32, month: u32) -> NaiveDate {
    let next = if month == 12 {
        NaiveDate::from_ymd_opt(year + 1, 1, 1)
    } else {
        NaiveDate::from_ymd_opt(year, month + 1, 1)
    }
    .expect("valid date");
    let mut d = next - Duration::days(1);
    while d.weekday() != Weekday::Sun {
        d -= Duration::days(1);
    }
    d
}

/// UTC offset of French and Swiss legal time at an instant: +2 h from the last Sunday of
/// March 01:00 UTC to the last Sunday of October 01:00 UTC, +1 h otherwise.
pub fn legal_offset_hours(at: DateTime<Utc>) -> i64 {
    let y = at.year();
    let start = Utc.from_utc_datetime(&last_sunday(y, 3).and_hms_opt(1, 0, 0).expect("time"));
    let end = Utc.from_utc_datetime(&last_sunday(y, 10).and_hms_opt(1, 0, 0).expect("time"));
    if at >= start && at < end { 2 } else { 1 }
}

/// Local legal time (CET / CEST) of an instant.
pub fn local_time(at: DateTime<Utc>) -> NaiveDateTime {
    at.naive_utc() + Duration::hours(legal_offset_hours(at))
}

/// Times (UTC) when the sun's centre crosses `altitude_deg` in the morning and in the
/// evening on a date at a place; `None` when it stays above or below all day.
pub fn sun_crossings(
    date: NaiveDate,
    place: LonLat,
    altitude_deg: f64,
) -> Option<(DateTime<Utc>, DateTime<Utc>)> {
    let rad = f64::to_radians;
    // Julian day of the date at 12:00 UTC (0001-01-01 is day 1 of the common era and
    // JD 1 721 425.5 at 00:00), then days since J2000.0 and mean solar noon at the
    // place (east longitudes are earlier in UTC).
    let jd_noon = date.num_days_from_ce() as f64 + 1_721_424.5 + 0.5;
    let n = (jd_noon - 2_451_545.0).round();
    let j_star = n - place.lon / 360.0;
    let m = (357.5291 + 0.985_600_28 * j_star).rem_euclid(360.0);
    let c = 1.9148 * rad(m).sin() + 0.02 * rad(2.0 * m).sin() + 0.0003 * rad(3.0 * m).sin();
    let lambda = (m + c + 180.0 + 102.9372).rem_euclid(360.0);
    let j_transit = 2_451_545.0 + j_star + 0.0053 * rad(m).sin() - 0.0069 * rad(2.0 * lambda).sin();
    let sin_d = rad(lambda).sin() * rad(23.4397).sin();
    let cos_d = sin_d.asin().cos();
    let cos_w =
        (rad(altitude_deg).sin() - rad(place.lat).sin() * sin_d) / (rad(place.lat).cos() * cos_d);
    if !(-1.0..=1.0).contains(&cos_w) {
        return None;
    }
    let w = cos_w.acos().to_degrees();
    let to_utc = |jd: f64| {
        let secs = ((jd - 2_440_587.5) * 86_400.0).round() as i64;
        DateTime::<Utc>::from_timestamp(secs, 0)
    };
    Some((
        to_utc(j_transit - w / 360.0)?,
        to_utc(j_transit + w / 360.0)?,
    ))
}

/// Sunrise and sunset (UTC) on a date at a place, `None` during polar day or night.
pub fn sun_times(date: NaiveDate, place: LonLat) -> Option<(DateTime<Utc>, DateTime<Utc>)> {
    sun_crossings(date, place, SUNSET_ALTITUDE_DEG)
}

/// Civil dawn and civil dusk (UTC, sun's centre at −6°).
pub fn civil_twilight(date: NaiveDate, place: LonLat) -> Option<(DateTime<Utc>, DateTime<Utc>)> {
    sun_crossings(date, place, CIVIL_ALTITUDE_DEG)
}

/// Sun position.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SunPosition {
    /// Elevation of the sun's centre above the horizon, degrees, without refraction.
    pub elevation_deg: f64,
    /// Azimuth, degrees from north, clockwise.
    pub azimuth_deg: f64,
}

/// Position of the sun at an instant and a place: low-precision formulas of the
/// Astronomical Almanac (mean longitude and anomaly, ecliptic longitude, obliquity, right
/// ascension, Greenwich mean sidereal time), the same family as the sunrise equation of
/// [`sun_crossings`]. Within 0.01° of astral 3.2; at the times [`sun_crossings`] gives, the
/// sun is at the requested altitude within 0.3° (tests). Used by the snow quality model
/// of the assistant (A3).
pub fn sun_position(at: DateTime<Utc>, place: LonLat) -> SunPosition {
    let rad = f64::to_radians;
    // Days since J2000.0 (2000-01-01 12:00 UTC), fractional.
    let d = (at.timestamp() as f64) / 86_400.0 + 2_440_587.5 - 2_451_545.0;
    let l = 280.460 + 0.985_647_4 * d;
    let g = rad((357.528 + 0.985_600_3 * d).rem_euclid(360.0));
    let lambda = rad((l + 1.915 * g.sin() + 0.020 * (2.0 * g).sin()).rem_euclid(360.0));
    let eps = rad(23.439 - 0.000_000_4 * d);
    let decl = (lambda.sin() * eps.sin()).asin();
    let ra = (lambda.sin() * eps.cos()).atan2(lambda.cos());
    // Sidereal time at the place, then hour angle.
    let theta = rad((280.460_618_37 + 360.985_647_366_29 * d + place.lon).rem_euclid(360.0));
    let h = theta - ra;
    let phi = rad(place.lat);
    let elevation = (phi.sin() * decl.sin() + phi.cos() * decl.cos() * h.cos()).asin();
    // Azimuth from the south, westward positive, then from the north clockwise.
    let az_south = h.sin().atan2(h.cos() * phi.sin() - decl.tan() * phi.cos());
    SunPosition {
        elevation_deg: elevation.to_degrees(),
        azimuth_deg: (az_south.to_degrees() + 180.0).rem_euclid(360.0),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn last_sundays_including_december() {
        let d = |y, m, day| NaiveDate::from_ymd_opt(y, m, day).unwrap();
        assert_eq!(last_sunday(2027, 3), d(2027, 3, 28));
        assert_eq!(last_sunday(2027, 10), d(2027, 10, 31));
        // December: the next month is January of the next year.
        assert_eq!(last_sunday(2026, 12), d(2026, 12, 27));
        assert_eq!(last_sunday(2027, 12), d(2027, 12, 26));
    }

    /// astral 3.2 (`astral.sun.sunset`, `dusk(depression=6)`, observer at sea level),
    /// run in a virtual environment outside the repository (script in the C7 issue).
    /// Place, date, sunrise, sunset, civil dawn, civil dusk (UTC).
    type AstralRow = (
        &'static str,
        (i32, u32, u32),
        &'static str,
        &'static str,
        &'static str,
        &'static str,
    );

    const ASTRAL: &[AstralRow] = &[
        // place, date, sunrise, sunset, civil dawn, civil dusk (UTC)
        (
            "cervieres",
            (2026, 12, 21),
            "07:07:58",
            "15:54:21",
            "06:33:33",
            "16:28:47",
        ),
        (
            "cervieres",
            (2027, 1, 15),
            "07:07:47",
            "16:17:23",
            "06:34:29",
            "16:50:42",
        ),
        (
            "cervieres",
            (2027, 2, 15),
            "06:34:34",
            "17:00:31",
            "06:03:52",
            "17:31:15",
        ),
        (
            "cervieres",
            (2027, 3, 15),
            "05:46:30",
            "17:38:23",
            "05:16:48",
            "18:08:08",
        ),
        (
            "cervieres",
            (2027, 6, 21),
            "03:47:08",
            "19:22:36",
            "03:09:07",
            "20:00:38",
        ),
        (
            "chapelle",
            (2026, 12, 21),
            "07:17:03",
            "15:50:10",
            "06:41:11",
            "16:26:02",
        ),
        (
            "chapelle",
            (2027, 1, 15),
            "07:15:59",
            "16:14:05",
            "06:41:24",
            "16:48:42",
        ),
        (
            "chapelle",
            (2027, 2, 15),
            "06:40:08",
            "16:59:52",
            "06:08:26",
            "17:31:37",
        ),
        (
            "chapelle",
            (2027, 3, 15),
            "05:49:21",
            "17:40:28",
            "05:18:43",
            "18:11:10",
        ),
        (
            "chapelle",
            (2027, 6, 21),
            "03:42:27",
            "19:32:11",
            "03:02:19",
            "20:12:19",
        ),
    ];

    #[test]
    fn sunset_and_civil_dusk_match_astral_on_both_sites() {
        let mut worst = 0;
        for &(place, (y, m, d), rise, set, dawn, dusk) in ASTRAL {
            let p = match place {
                "cervieres" => LonLat::new(6.7232, 44.8667),
                _ => LonLat::new(6.1118, 46.5952),
            };
            let date = NaiveDate::from_ymd_opt(y, m, d).unwrap();
            let (r, s) = sun_times(date, p).unwrap();
            let (a, k) = civil_twilight(date, p).unwrap();
            for (got, want) in [(r, rise), (s, set), (a, dawn), (k, dusk)] {
                let want = Utc.from_utc_datetime(&date.and_time(want.parse().unwrap()));
                let diff = (got - want).num_seconds().abs();
                worst = worst.max(diff);
                println!("{place} {date}: {got} vs astral {want} ({diff} s)");
                assert!(diff <= 90, "{place} {date}: {got} vs {want} ({diff} s)");
            }
        }
        println!("worst difference: {worst} s");
    }

    #[test]
    fn civil_dusk_is_about_half_an_hour_after_sunset_in_winter() {
        let p = LonLat::new(6.1118, 46.5952);
        let date = NaiveDate::from_ymd_opt(2027, 1, 15).unwrap();
        let (_, set) = sun_times(date, p).unwrap();
        let (_, dusk) = civil_twilight(date, p).unwrap();
        let gap = (dusk - set).num_minutes();
        assert!((30..=40).contains(&gap), "{gap} min");
        // Nautical twilight later still.
        let (_, naut) = sun_crossings(date, p, NAUTICAL_ALTITUDE_DEG).unwrap();
        assert!(naut > dusk);
    }

    fn utc_at(s: &str) -> DateTime<Utc> {
        Utc.from_utc_datetime(&NaiveDateTime::parse_from_str(s, "%Y-%m-%dT%H:%M:%S").unwrap())
    }

    /// astral 3.2, `elevation(observer, t, with_refraction=False)` and `azimuth`, observer
    /// at sea level (Python oracle outside the repository).
    #[test]
    fn sun_position_matches_astral() {
        let cases = [
            (44.8552, 6.7269, "2026-01-05T11:00:00", 21.993, 170.423),
            (44.8552, 6.7269, "2023-04-15T07:30:00", 27.371, 104.315),
            (46.6032, 6.1125, "2026-02-25T14:00:00", 27.198, 217.055),
            (46.6032, 6.1125, "2026-06-21T05:00:00", 10.769, 67.160),
        ];
        for (lat, lon, t, el, az) in cases {
            let p = sun_position(utc_at(t), LonLat::new(lon, lat));
            assert!((p.elevation_deg - el).abs() < 0.02, "{t}: {p:?} vs {el}");
            assert!((p.azimuth_deg - az).abs() < 0.02, "{t}: {p:?} vs {az}");
        }
    }

    /// At the times `ephemeris::sun_crossings` gives, the sun is at that altitude.
    #[test]
    fn sun_position_at_crossing_times() {
        let place = LonLat::new(6.7269, 44.8552);
        for (y, m, d) in [(2026, 1, 5), (2026, 3, 20), (2026, 6, 21)] {
            let date = NaiveDate::from_ymd_opt(y, m, d).unwrap();
            for alt in [-0.833, 10.0, 20.0] {
                let Some((rise, set)) = sun_crossings(date, place, alt) else {
                    continue;
                };
                for t in [rise, set] {
                    let p = sun_position(t, place);
                    assert!(
                        (p.elevation_deg - alt).abs() < 0.3,
                        "{date} {alt}: {}",
                        p.elevation_deg
                    );
                }
                assert!(sun_position(rise, place).azimuth_deg < 180.0);
                assert!(sun_position(set, place).azimuth_deg > 180.0);
            }
        }
    }
}
