//! Sun position (elevation, azimuth) and clear-sky irradiance on a slope, for the snow
//! quality model (A3).
//!
//! The position uses the low-precision formulas of the Astronomical Almanac (mean
//! longitude and anomaly, ecliptic longitude, obliquity, right ascension, Greenwich mean
//! sidereal time), the same family as the sunrise equation of [`skimap_core::ephemeris`]
//! (which only gives crossing times, not a position). It agrees with astral 3.2 within
//! 0.01° and with `ephemeris::sun_crossings` (the sun is at the requested altitude at the
//! crossing times) within 0.3° (tests).
//! Sunrise, sunset and legal time come from `skimap_core::ephemeris`.
//!
//! Irradiance (W/m²), clear sky then cloud:
//! - direct normal: Meinel with the altitude correction of Laue (1970),
//!   `DNI = 1361 · ((1 − 0.14 h) · 0.7^(AM^0.678) + 0.14 h)`, `h` in km, air mass of
//!   Kasten & Young (1989);
//! - diffuse horizontal: 10 % of the direct normal (rule of thumb with Meinel);
//! - on a slope of angle `β`: direct × cos(incidence) + diffuse × (1 + cos β) / 2 +
//!   ground reflection (snow albedo 0.8) × global horizontal × (1 − cos β) / 2;
//! - cloud cover `N` (0..1): factor `1 − 0.75 N^3.4` (Kasten & Czeplak 1980).
//!
//! Not modelled: shading by the surrounding relief (valleys get the sun later than a free
//! horizon), forest shade.

use chrono::{DateTime, NaiveDateTime, Utc};
use skimap_common::geom::LonLat;

/// Solar constant, W/m².
pub const SOLAR_CONSTANT: f64 = 1361.0;
/// Albedo of the snow around a slope, for the reflected part.
pub const GROUND_ALBEDO: f64 = 0.8;

/// Sun position.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SunPosition {
    /// Elevation of the sun's centre above the horizon, degrees, without refraction.
    pub elevation_deg: f64,
    /// Azimuth, degrees from north, clockwise.
    pub azimuth_deg: f64,
}

/// Position of the sun at an instant and a place.
pub fn position(at: DateTime<Utc>, place: LonLat) -> SunPosition {
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

/// UTC instant of a French / Swiss legal local time (CET / CEST).
pub fn local_to_utc(local: NaiveDateTime) -> DateTime<Utc> {
    // The offset is taken one hour before so that the change of hour itself is handled
    // like the hour before it (one ambiguous hour a year).
    let guess = DateTime::<Utc>::from_naive_utc_and_offset(local - chrono::Duration::hours(1), Utc);
    let off = skimap_core::ephemeris::legal_offset_hours(guess);
    DateTime::<Utc>::from_naive_utc_and_offset(local - chrono::Duration::hours(off), Utc)
}

/// Air mass (Kasten & Young 1989) for an elevation in degrees.
pub fn air_mass(elevation_deg: f64) -> f64 {
    let e = elevation_deg.max(0.0);
    1.0 / (e.to_radians().sin() + 0.50572 * (e + 6.07995).powf(-1.6364))
}

/// Clear-sky direct normal irradiance (Meinel, Laue altitude correction), W/m².
pub fn direct_normal(elevation_deg: f64, altitude_m: f64) -> f64 {
    if elevation_deg <= 0.0 {
        return 0.0;
    }
    let h = (altitude_m / 1000.0).clamp(0.0, 5.0);
    let am = air_mass(elevation_deg);
    SOLAR_CONSTANT * ((1.0 - 0.14 * h) * 0.7_f64.powf(am.powf(0.678)) + 0.14 * h)
}

/// Cloud factor of Kasten & Czeplak (1980), `N` the cloud fraction 0..1.
pub fn cloud_factor(cloud_fraction: f64) -> f64 {
    1.0 - 0.75 * cloud_fraction.clamp(0.0, 1.0).powf(3.4)
}

/// Cosine of the incidence angle of the sun on a slope (`slope_deg`, facing `aspect_deg`
/// from north clockwise); negative when the sun is behind the slope.
pub fn incidence_cos(sun: SunPosition, slope_deg: f64, aspect_deg: f64) -> f64 {
    let (e, a) = (sun.elevation_deg.to_radians(), sun.azimuth_deg.to_radians());
    let (b, g) = (slope_deg.to_radians(), aspect_deg.to_radians());
    b.cos() * e.sin() + b.sin() * e.cos() * (a - g).cos()
}

/// Global irradiance on a slope with cloud cover, W/m².
pub fn global_on_slope(
    sun: SunPosition,
    slope_deg: f64,
    aspect_deg: f64,
    altitude_m: f64,
    cloud_fraction: f64,
) -> f64 {
    if sun.elevation_deg <= 0.0 {
        return 0.0;
    }
    let dni = direct_normal(sun.elevation_deg, altitude_m);
    let diffuse = 0.1 * dni;
    let gh = dni * sun.elevation_deg.to_radians().sin() + diffuse;
    let cb = slope_deg.to_radians().cos();
    let direct = dni * incidence_cos(sun, slope_deg, aspect_deg).max(0.0);
    let total = direct + diffuse * (1.0 + cb) / 2.0 + GROUND_ALBEDO * gh * (1.0 - cb) / 2.0;
    total * cloud_factor(cloud_fraction)
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{NaiveDate, TimeZone};

    fn utc(s: &str) -> DateTime<Utc> {
        Utc.from_utc_datetime(&NaiveDateTime::parse_from_str(s, "%Y-%m-%dT%H:%M:%S").unwrap())
    }

    /// astral 3.2, `elevation(observer, t, with_refraction=False)` and `azimuth`, observer
    /// at sea level (Python oracle outside the repository).
    #[test]
    fn matches_astral() {
        let cases = [
            (44.8552, 6.7269, "2026-01-05T11:00:00", 21.993, 170.423),
            (44.8552, 6.7269, "2023-04-15T07:30:00", 27.371, 104.315),
            (46.6032, 6.1125, "2026-02-25T14:00:00", 27.198, 217.055),
            (46.6032, 6.1125, "2026-06-21T05:00:00", 10.769, 67.160),
        ];
        for (lat, lon, t, el, az) in cases {
            let p = position(utc(t), LonLat::new(lon, lat));
            assert!((p.elevation_deg - el).abs() < 0.02, "{t}: {p:?} vs {el}");
            assert!((p.azimuth_deg - az).abs() < 0.02, "{t}: {p:?} vs {az}");
        }
    }

    /// At the times `ephemeris::sun_crossings` gives, the sun is at that altitude.
    #[test]
    fn consistent_with_core_ephemeris() {
        let place = LonLat::new(6.7269, 44.8552);
        for (y, m, d) in [(2026, 1, 5), (2026, 3, 20), (2026, 6, 21)] {
            let date = NaiveDate::from_ymd_opt(y, m, d).unwrap();
            for alt in [-0.833, 10.0, 20.0] {
                let Some((rise, set)) = skimap_core::ephemeris::sun_crossings(date, place, alt)
                else {
                    continue;
                };
                for t in [rise, set] {
                    let p = position(t, place);
                    assert!(
                        (p.elevation_deg - alt).abs() < 0.3,
                        "{date} {alt}: {}",
                        p.elevation_deg
                    );
                }
                assert!(position(rise, place).azimuth_deg < 180.0);
                assert!(position(set, place).azimuth_deg > 180.0);
            }
        }
    }

    #[test]
    fn irradiance_orders_aspects() {
        // 5 January, 11:00 UTC (around solar noon) at Le Laus, clear sky.
        let sun = position(utc("2026-01-05T11:00:00"), LonLat::new(6.7269, 44.8552));
        let g = |slope, aspect| global_on_slope(sun, slope, aspect, 1750.0, 0.0);
        let (south, flat, north) = (g(40.0, 180.0), g(0.0, 0.0), g(40.0, 0.0));
        assert!(south > 2.0 * flat, "{south} {flat}");
        assert!(flat > north, "{flat} {north}");
        // North 40° in January: no direct sun (sun at 22°), only diffuse and reflected.
        assert!(incidence_cos(sun, 40.0, 0.0) < 0.0);
        // Overcast: about a quarter of clear sky.
        let overcast = global_on_slope(sun, 0.0, 0.0, 1750.0, 1.0);
        assert!((overcast / flat - 0.25).abs() < 1e-9);
        // Clear-sky direct normal at 60° and 2000 m: about 1000 W/m².
        let dni = direct_normal(60.0, 2000.0);
        assert!((950.0..1100.0).contains(&dni), "{dni}");
    }

    #[test]
    fn legal_time_round_trip() {
        let winter =
            NaiveDateTime::parse_from_str("2026-01-05T12:00:00", "%Y-%m-%dT%H:%M:%S").unwrap();
        assert_eq!(local_to_utc(winter), utc("2026-01-05T11:00:00"));
        let summer =
            NaiveDateTime::parse_from_str("2026-04-15T12:00:00", "%Y-%m-%dT%H:%M:%S").unwrap();
        assert_eq!(local_to_utc(summer), utc("2026-04-15T10:00:00"));
    }
}
