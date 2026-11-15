//! Sun position (elevation, azimuth) and clear-sky irradiance on a slope, for the snow
//! quality model (A3).
//!
//! The position of the sun is `skimap_core::ephemeris::sun_position` (Astronomical Almanac
//! formulas, within 0.01° of astral 3.2), shared with the core; sunrise, sunset and legal
//! time come from the same module.
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
pub use skimap_core::ephemeris::SunPosition;

/// Solar constant, W/m².
pub const SOLAR_CONSTANT: f64 = 1361.0;
/// Albedo of the snow around a slope, for the reflected part.
pub const GROUND_ALBEDO: f64 = 0.8;

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
    use chrono::TimeZone;

    fn utc(s: &str) -> DateTime<Utc> {
        Utc.from_utc_datetime(&NaiveDateTime::parse_from_str(s, "%Y-%m-%dT%H:%M:%S").unwrap())
    }

    #[test]
    fn irradiance_orders_aspects() {
        // 5 January, 11:00 UTC (around solar noon) at Le Laus, clear sky.
        let sun = skimap_core::ephemeris::sun_position(
            utc("2026-01-05T11:00:00"),
            skimap_common::geom::LonLat::new(6.7269, 44.8552),
        );
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
