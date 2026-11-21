//! Synthetic weather sets of the assistant tests (X15, #149), in the format of the
//! `meteo-hourly` layer (P19, `hourly.json`), versioned in `crates/assist/tests/data/weather/`
//! because the real forecasts (Open-Meteo) are personal use only and never versioned.
//!
//! Each set has four days of hours (two days before the day studied, the day, the day
//! after) at two forecast points, built from a few rules (a daily temperature cycle, the
//! events of the day at given hours) so that each value is known when reading the test:
//!
//! | Set | Site, day | Weather |
//! |---|---|---|
//! | `froid-sec` | Cervières, Tue 12/01/2027 | −17 to −9 °C, clear, light wind, very cold wind chill |
//! | `vent-fort` | Cervières, Sat 16/01/2027 | 10 cm overnight, gusts of 75 km/h from 11 h, cold front at 14 h (−5 → −12 °C) |
//! | `brouillard` | Cervières, Wed 20/01/2027 | around 0 °C, overcast, fog from 12 h to 16 h |
//! | `neige-fraiche` | Cervières, Mon 25/01/2027 | 19 cm from 16 h the day before to 7 h, snow again 13–15 h, sunny the day after |
//! | `regel` | Cervières, Sat 20/03/2027 | rain on a mild day before, clear night at −8 °C, clear day up to +5 °C |
//! | `redoux` | Chapelle-des-Bois, Wed 24/02/2027 | +3 to +8 °C, overcast, rain 1.5 mm/h 10–16 h, school holidays |
//! | `novembre` | Cervières, Sun 15/11/2026 | mild and dry, the snow map of the fixtures (no snow) |
//!
//! `SKIMAP_BLESS=1 cargo test -p skimap-assist --test weather` rewrites the files from these
//! rules; otherwise the test checks that the files are what the rules give.

#![allow(dead_code)]

use jiff::{
    SignedDuration,
    civil::{Date, DateTime},
};
use serde_json::{Value, json};

/// A forecast point of a set.
#[derive(Debug, Clone, Copy)]
pub struct Point {
    /// Identifier.
    pub id: &'static str,
    /// Name.
    pub name: &'static str,
    /// Latitude.
    pub lat: f64,
    /// Longitude.
    pub lon: f64,
    /// Elevation, m.
    pub elevation_m: f64,
    /// Offset of the temperature from the first point, °C.
    pub dt: f64,
    /// Offset of the wind and gusts, km/h.
    pub dw: f64,
}

/// One hour of a set.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Hour {
    /// Air temperature, °C.
    pub t: f64,
    /// Mean wind, km/h.
    pub wind: f64,
    /// Gusts, km/h.
    pub gust: f64,
    /// Cloud cover, %.
    pub cloud: f64,
    /// Snowfall, cm.
    pub snow: f64,
    /// Precipitation (rain and snow water), mm.
    pub precip: f64,
    /// Fog.
    pub fog: bool,
    /// Relative humidity, %.
    pub rh: f64,
    /// 0 °C isotherm, m.
    pub freezing: f64,
}

/// A set: name, site, day studied, points.
#[derive(Debug, Clone, Copy)]
pub struct Set {
    /// File stem.
    pub name: &'static str,
    /// Site identifier.
    pub site: &'static str,
    /// Day studied.
    pub day: (i32, u32, u32),
    /// Points.
    pub points: [Point; 2],
}

impl Set {
    /// The day studied.
    pub fn date(&self) -> Date {
        skimap_common::dates::ymd(self.day.0, self.day.1, self.day.2).expect("date")
    }

    /// First hour of the set: 00 h two days before.
    pub fn start(&self) -> DateTime {
        (self.date() - SignedDuration::from_hours(24 * 2)).at(0, 0, 0, 0)
    }
}

const LE_LAUS: Point = Point {
    id: "le-laus",
    name: "Le Laus",
    lat: 44.8552,
    lon: 6.7269,
    elevation_m: 1750.0,
    dt: 0.0,
    dw: 0.0,
};

const LES_FONTS: Point = Point {
    id: "les-fonts",
    name: "Les Fonts",
    lat: 44.8448,
    lon: 6.8161,
    elevation_m: 2040.0,
    dt: -2.0,
    dw: 5.0,
};

const CHAPELLE: Point = Point {
    id: "chapelle-des-bois",
    name: "Chapelle-des-Bois",
    lat: 46.6032,
    lon: 6.1125,
    elevation_m: 1090.0,
    dt: 0.0,
    dw: 0.0,
};

const RISOUX: Point = Point {
    id: "risoux",
    name: "Risoux",
    lat: 46.5700,
    lon: 6.1000,
    elevation_m: 1250.0,
    dt: -1.0,
    dw: 5.0,
};

/// Every set.
pub const SETS: [Set; 7] = [
    Set {
        name: "froid-sec",
        site: "cervieres",
        day: (2027, 1, 12),
        points: [LE_LAUS, LES_FONTS],
    },
    Set {
        name: "vent-fort",
        site: "cervieres",
        day: (2027, 1, 16),
        points: [LE_LAUS, LES_FONTS],
    },
    Set {
        name: "brouillard",
        site: "cervieres",
        day: (2027, 1, 20),
        points: [LE_LAUS, LES_FONTS],
    },
    Set {
        name: "neige-fraiche",
        site: "cervieres",
        day: (2027, 1, 25),
        points: [LE_LAUS, LES_FONTS],
    },
    Set {
        name: "regel",
        site: "cervieres",
        day: (2027, 3, 20),
        points: [LE_LAUS, LES_FONTS],
    },
    Set {
        name: "redoux",
        site: "chapelle-des-bois",
        day: (2027, 2, 24),
        points: [CHAPELLE, RISOUX],
    },
    Set {
        name: "novembre",
        site: "cervieres",
        day: (2026, 11, 15),
        points: [LE_LAUS, LES_FONTS],
    },
];

/// The set of a name.
pub fn set(name: &str) -> Set {
    *SETS.iter().find(|s| s.name == name).expect("set")
}

/// Daily cycle: `tmin` at 05 h, `tmax` at 14 h (cosine over 24 h, extremes at 02 h and
/// 14 h of the cosine, shifted so that the morning is the coldest).
pub fn cycle(hour: u32, tmin: f64, tmax: f64) -> f64 {
    let x = ((f64::from(hour) - 14.0) / 24.0 * std::f64::consts::TAU).cos();
    tmin + (tmax - tmin) * (x + 1.0) / 2.0
}

fn calm(t: f64) -> Hour {
    Hour {
        t,
        wind: 8.0,
        gust: 15.0,
        cloud: 50.0,
        snow: 0.0,
        precip: 0.0,
        fog: false,
        rh: 70.0,
        freezing: 1500.0,
    }
}

/// The weather of a set at a time, before the offsets of the point. `d`: days from the
/// day studied (−2 to 1).
pub fn weather(name: &str, d: i64, h: u32) -> Hour {
    match name {
        "froid-sec" => Hour {
            cloud: 5.0,
            rh: 55.0,
            freezing: 600.0,
            ..calm(cycle(h, -17.0, -9.0))
        },
        "vent-fort" => {
            // 0.8 cm/h from 20 h the day before to 07 h; gusts from 11 h; front at 14 h.
            let snowing = (d == -1 && h >= 20) || (d == 0 && h < 8);
            let windy = (d == 0 && h >= 11) || d > 0;
            let cold = (d == 0 && h >= 14) || d > 0;
            Hour {
                t: if cold { -12.0 } else { -5.0 },
                wind: if windy { 45.0 } else { 10.0 },
                gust: if windy { 75.0 } else { 20.0 },
                cloud: if snowing { 100.0 } else { 60.0 },
                snow: if snowing { 0.8 } else { 0.0 },
                precip: if snowing { 0.8 } else { 0.0 },
                rh: if snowing { 95.0 } else { 75.0 },
                freezing: 800.0,
                ..calm(-5.0)
            }
        }
        "brouillard" => {
            let fog = d == 0 && (12..16).contains(&h);
            Hour {
                cloud: 100.0,
                fog,
                rh: if fog { 100.0 } else { 90.0 },
                wind: 5.0,
                gust: 10.0,
                freezing: 1700.0,
                ..calm(cycle(h, -3.0, 1.0))
            }
        }
        "neige-fraiche" => {
            // 1.2 cm/h from 16 h the day before to 07 h (19.2 cm), 0.8 cm/h 13–15 h.
            let night = (d == -1 && h >= 16) || (d == 0 && h < 8);
            let day = d == 0 && (13..15).contains(&h);
            let snow = if night {
                1.2
            } else if day {
                0.8
            } else {
                0.0
            };
            Hour {
                snow,
                precip: snow,
                cloud: if snow > 0.0 { 100.0 } else { 10.0 },
                rh: if snow > 0.0 { 95.0 } else { 70.0 },
                freezing: 900.0,
                ..calm(cycle(h, -9.0, -4.0))
            }
        }
        "regel" => {
            // A mild day before with rain from 12 to 15 h, then clear.
            let rain = d == -1 && (12..15).contains(&h);
            Hour {
                cloud: if d == -1 && (10..16).contains(&h) {
                    90.0
                } else {
                    0.0
                },
                precip: if rain { 2.0 } else { 0.0 },
                rh: if rain { 95.0 } else { 60.0 },
                freezing: 2400.0,
                ..calm(if d < 0 {
                    cycle(h, -2.0, 8.0)
                } else {
                    cycle(h, -8.0, 5.0)
                })
            }
        }
        "redoux" => {
            let rain = (d == 0 && (10..16).contains(&h)) || (d == -1 && (14..18).contains(&h));
            Hour {
                wind: 20.0,
                gust: 35.0,
                cloud: 95.0,
                precip: if rain { 1.5 } else { 0.0 },
                rh: 95.0,
                freezing: 2600.0,
                ..calm(cycle(h, 3.0, 8.0))
            }
        }
        "novembre" => Hour {
            cloud: 40.0,
            freezing: 2900.0,
            ..calm(cycle(h, 0.0, 9.0))
        },
        _ => panic!("no set {name}"),
    }
}

fn r1(x: f64) -> f64 {
    (x * 10.0).round() / 10.0
}

/// Wind chill (JAG/TI, Environment Canada), °C, as the P19 layer computes it.
pub fn wind_chill(t: f64, wind_kmh: f64) -> f64 {
    if wind_kmh < 4.8 || t > 10.0 {
        return t;
    }
    let v = wind_kmh.powf(0.16);
    13.12 + 0.6215 * t - 11.37 * v + 0.3965 * t * v
}

/// The `hourly.json` of a set.
pub fn file(s: &Set) -> Value {
    let points: Vec<Value> = s
        .points
        .iter()
        .map(|p| {
            let hours: Vec<Value> = (0..96)
                .map(|i| {
                    let time = s.start() + SignedDuration::from_hours(i);
                    let d = (time.date() - s.date()).get_days();
                    let w = weather(s.name, i64::from(d), time.hour() as u32);
                    let t = w.t + p.dt;
                    let wind = w.wind + p.dw;
                    let gust = w.gust + p.dw;
                    let chill = wind_chill(t, wind);
                    json!({
                        "time": time.strftime("%Y-%m-%dT%H:%M").to_string(),
                        "temperature_c": r1(t),
                        "apparent_temperature_c": r1(chill - 1.0),
                        "wind_chill_c": r1(chill),
                        "wind_kmh": r1(wind),
                        "gust_kmh": r1(gust),
                        "wind_direction_deg": 270.0,
                        "visibility_m": if w.fog { 150.0 } else { 20000.0 },
                        "fog": w.fog,
                        "freezing_level_m": w.freezing,
                        "snowfall_cm": r1(w.snow),
                        "precipitation_mm": r1(w.precip),
                        "cloud_cover_pct": w.cloud,
                        "cloud_cover_low_pct": if w.fog { 100.0 } else { 0.0 },
                        "weather_code": if w.fog {
                            45
                        } else if w.snow > 0.0 {
                            73
                        } else if w.precip > 0.0 {
                            63
                        } else if w.cloud > 80.0 {
                            3
                        } else {
                            0
                        },
                        "relative_humidity_pct": w.rh,
                    })
                })
                .collect();
            json!({
                "id": p.id,
                "name": p.name,
                "lat": p.lat,
                "lon": p.lon,
                "elevation_m": p.elevation_m,
                "hours": hours,
            })
        })
        .collect();
    json!({
        "site": s.site,
        "generated_at": "synthétique (crates/assist/tests/support/weather_sets.rs)",
        "points": points,
    })
}

/// The text of the versioned file: one hour per line, so that a change of a rule reads
/// as the lines of the hours it changes.
pub fn text(s: &Set) -> String {
    let v = file(s);
    let mut out = String::from("{\n");
    for k in ["site", "generated_at"] {
        out.push_str(&format!("  {:?}: {},\n", k, v[k]));
    }
    out.push_str("  \"points\": [\n");
    let points = v["points"].as_array().expect("points");
    for (i, p) in points.iter().enumerate() {
        out.push_str("    {\n");
        for k in ["id", "name", "lat", "lon", "elevation_m"] {
            out.push_str(&format!("      {:?}: {},\n", k, p[k]));
        }
        out.push_str("      \"hours\": [\n");
        let hours = p["hours"].as_array().expect("hours");
        for (j, h) in hours.iter().enumerate() {
            let comma = if j + 1 < hours.len() { "," } else { "" };
            out.push_str(&format!("        {h}{comma}\n"));
        }
        out.push_str("      ]\n");
        out.push_str(if i + 1 < points.len() {
            "    },\n"
        } else {
            "    }\n"
        });
    }
    out.push_str("  ]\n}\n");
    out
}

/// Path of the versioned file of a set.
pub fn path(name: &str) -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/data/weather")
        .join(format!("{name}.json"))
}
