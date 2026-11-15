//! Recap of an outing and history of the outings (A10 #97): distance, time, estimated
//! against actual time, comparison with the previous outings, conditions met; written and
//! spoken. Altitude and climb of the trace from the barometer fused with the relief (K8
//! #58). Deleting an outing or the whole history.
//!
//! An outing is a directory under the app's `plans/`: a day plan (`plan.json`, K9) or a
//! guided outing (`guided.json`, [`crate::fog`]), with the inputs recorded by the location
//! service (`inputs.jsonl`) and, when the phone has a barometer, the pressure recorded in
//! batches ([`PRESSURE_FILE`], one line `epoch_ms hPa` per sample).
//!
//! Estimated time: the group's time for the day when the plan had one (C11), else the
//! model time (C5); the actual time is from the first to the last fix. Conditions: the
//! hourly forecast of the plan over the hours actually skied (« prévues », never
//! « constatées »: the phone measured none of them), and the altitude range measured.

use std::path::{Path, PathBuf};

use chrono::{DateTime, Datelike, NaiveDateTime, Utc};
use skimap_common::geom::LonLat;
use skimap_plan::pace::duration_fr;
use skimap_plan::plan::DayPlan;
use skimap_plan::tracker::Input;

use crate::card::Lang;
use crate::fog::{GuidedKind, GuidedSpec, distance_words};
use crate::tracking::{INPUTS_FILE, TrackEvent, TrackingError};
use crate::tracking_day::{PACK_FILE, PLAN_FILE};

/// Pressure recorded during the outing (barometer, batched): `epoch_ms hPa` per line.
pub const PRESSURE_FILE: &str = "pressure.txt";
/// Above this speed a step between two fixes is a GPS jump, not skiing, m/s.
const JUMP_MS: f64 = 15.0;
/// Moving above this, m/s.
const MOVING_MS: f64 = 0.6;
/// Climb counted past this hysteresis, metres.
const CLIMB_HYSTERESIS_M: f64 = 4.0;
/// Time constant of the barometer drift corrected by the relief, seconds.
const BARO_TAU_S: f64 = 900.0;

fn invalid(detail: impl std::fmt::Display) -> TrackingError {
    TrackingError::Invalid {
        detail: detail.to_string(),
    }
}

fn utc(ms: i64) -> DateTime<Utc> {
    DateTime::<Utc>::from_timestamp_millis(ms).unwrap_or_default()
}

/// The inputs recorded in `path` (a line cut by the end of the process is dropped).
pub fn read_inputs(path: &Path) -> Result<Vec<Input>, TrackingError> {
    let text = match std::fs::read_to_string(path) {
        Ok(t) => t,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(invalid(format!("{}: {e}", path.display()))),
    };
    Ok(text
        .lines()
        .filter_map(|l| serde_json::from_str(l).ok())
        .collect())
}

/// The input of an event (same conversion as the day plan's tracking).
pub fn event_input(e: &TrackEvent) -> Input {
    e.input()
}

fn xml_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

/// Writes the fixes of `log` as `trace.gpx` next to it, with the barometric altitude
/// when it was recorded; its path.
pub fn write_gpx(log: &Path, name: &str) -> Result<String, TrackingError> {
    let fixes = fixes(&read_inputs(log)?);
    let dir = log.parent().unwrap_or(Path::new("."));
    let alt = altitude(dir, &fixes, None);
    let mut s = String::from(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<gpx version=\"1.1\" creator=\"skimap\" xmlns=\"http://www.topografix.com/GPX/1/1\">\n<trk><name>",
    );
    s.push_str(&xml_escape(name));
    s.push_str("</name><type>nordic_skiing</type><trkseg>\n");
    for (i, (t, p)) in fixes.iter().enumerate() {
        let ele = alt
            .as_ref()
            .and_then(|a| a.at_fix.get(i).copied().flatten())
            .map(|z| format!("<ele>{z:.1}</ele>"))
            .unwrap_or_default();
        s.push_str(&format!(
            "<trkpt lat=\"{:.7}\" lon=\"{:.7}\">{ele}<time>{}</time></trkpt>\n",
            p.lat,
            p.lon,
            utc(*t).format("%Y-%m-%dT%H:%M:%S%.3fZ")
        ));
    }
    s.push_str("</trkseg></trk>\n</gpx>\n");
    let path = dir.join("trace.gpx");
    std::fs::write(&path, s).map_err(invalid)?;
    Ok(path.display().to_string())
}

/// The fixes of the inputs: (epoch ms, position).
pub fn fixes(inputs: &[Input]) -> Vec<(i64, LonLat)> {
    inputs
        .iter()
        .filter_map(|i| match i {
            Input::Fix { t, pos } => Some((t.timestamp_millis(), *pos)),
            _ => None,
        })
        .collect()
}

/// Distance, moving time and elapsed time of fixes: (metres, seconds, seconds).
pub fn measure(fixes: &[(i64, LonLat)]) -> (f64, f64, f64) {
    let (mut d, mut moving) = (0.0, 0.0);
    for w in fixes.windows(2) {
        let dt = (w[1].0 - w[0].0) as f64 / 1000.0;
        if dt <= 0.0 {
            continue;
        }
        let step = w[0].1.haversine_m(w[1].1);
        let v = step / dt;
        if v > JUMP_MS {
            continue;
        }
        if v >= MOVING_MS {
            d += step;
            moving += dt;
        }
    }
    let elapsed = match (fixes.first(), fixes.last()) {
        (Some(a), Some(b)) => (b.0 - a.0) as f64 / 1000.0,
        _ => 0.0,
    };
    (d, moving, elapsed)
}

// ---------------------------------------------------------------------------------------
// Altitude: barometer fused with the relief
// ---------------------------------------------------------------------------------------

/// Where the altitude of a trace comes from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum AltitudeSource {
    /// Barometer, its drift corrected by the relief under the GPS positions.
    BarometerAndRelief,
    /// Barometer alone (no relief): climb right, altitude relative to the start.
    Barometer,
    /// Relief under the GPS positions (no barometer).
    Relief,
}

/// Altitude along a trace.
#[derive(Debug, Clone, PartialEq)]
pub struct Altitude {
    /// Source.
    pub source: AltitudeSource,
    /// Altitude at each fix, metres (relative to the start for [`AltitudeSource::Barometer`]).
    pub at_fix: Vec<Option<f64>>,
    /// Climb, metres.
    pub ascent_m: f64,
    /// Descent, metres.
    pub descent_m: f64,
    /// Lowest and highest, metres.
    pub range_m: Option<(f64, f64)>,
}

/// Pressure samples of `dir`: (epoch ms, hPa), sorted.
pub fn read_pressure(dir: &Path) -> Vec<(i64, f64)> {
    let Ok(text) = std::fs::read_to_string(dir.join(PRESSURE_FILE)) else {
        return Vec::new();
    };
    let mut v: Vec<(i64, f64)> = text
        .lines()
        .filter_map(|l| {
            let mut it = l.split_whitespace();
            let t = it.next()?.parse().ok()?;
            let p: f64 = it.next()?.parse().ok()?;
            (300.0..1100.0).contains(&p).then_some((t, p))
        })
        .collect();
    v.sort_by_key(|x| x.0);
    v
}

/// Barometric altitude difference from `p0` to `p`, metres (international atmosphere).
pub fn baro_height(p: f64, p0: f64) -> f64 {
    44_330.8 * (1.0 - (p / p0).powf(0.190_263))
}

fn climb(z: &[f64]) -> (f64, f64) {
    let (mut up, mut down) = (0.0, 0.0);
    let Some(&first) = z.first() else {
        return (0.0, 0.0);
    };
    let mut anchor = first;
    for &v in z {
        if v - anchor >= CLIMB_HYSTERESIS_M {
            up += v - anchor;
            anchor = v;
        } else if anchor - v >= CLIMB_HYSTERESIS_M {
            down += anchor - v;
            anchor = v;
        }
    }
    (up, down)
}

/// Fuses the pressure of `dir` with the relief `dem` (altitude at a position) at the
/// fixes: the barometer gives the short-term changes, the relief under the GPS positions
/// the level and corrects the weather's drift (complementary filter, [`BARO_TAU_S`]).
pub fn altitude(
    dir: &Path,
    fixes: &[(i64, LonLat)],
    dem: Option<&dyn Fn(LonLat) -> Option<f64>>,
) -> Option<Altitude> {
    let pressure = read_pressure(dir);
    let relief: Vec<Option<f64>> = fixes.iter().map(|(_, p)| dem.and_then(|f| f(*p))).collect();
    let has_relief = relief.iter().any(Option::is_some);
    if pressure.len() < 2 {
        if !has_relief {
            return None;
        }
        // Relief alone, smoothed over 5 fixes (GPS noise on slopes).
        let z: Vec<f64> = relief.iter().flatten().copied().collect();
        let sm: Vec<f64> = (0..z.len())
            .map(|i| {
                let a = i.saturating_sub(2);
                let b = (i + 3).min(z.len());
                z[a..b].iter().sum::<f64>() / (b - a) as f64
            })
            .collect();
        let (ascent_m, descent_m) = climb(&sm);
        return Some(Altitude {
            source: AltitudeSource::Relief,
            at_fix: relief,
            ascent_m,
            descent_m,
            range_m: range(&sm),
        });
    }
    let p0 = pressure[0].1;
    // Barometric height at a time, interpolated.
    let baro_at = |t: i64| -> f64 {
        let i = pressure.partition_point(|x| x.0 < t);
        let p = match (i.checked_sub(1).map(|j| pressure[j]), pressure.get(i)) {
            (Some(a), Some(b)) if b.0 > a.0 => {
                a.1 + (b.1 - a.1) * (t - a.0) as f64 / (b.0 - a.0) as f64
            }
            (Some(a), _) => a.1,
            (None, Some(b)) => b.1,
            (None, None) => p0,
        };
        baro_height(p, p0)
    };
    // Climb from the barometer itself (all samples, between the fixes too).
    let baro_all: Vec<f64> = pressure.iter().map(|(_, p)| baro_height(*p, p0)).collect();
    let mut offset: Option<f64> = None;
    let mut last_t: Option<i64> = None;
    let mut at_fix = Vec::with_capacity(fixes.len());
    for ((t, _), r) in fixes.iter().zip(&relief) {
        let b = baro_at(*t);
        if let Some(r) = r {
            let target = r - b;
            offset = Some(match (offset, last_t) {
                (Some(o), Some(lt)) => {
                    let dt = ((t - lt) as f64 / 1000.0).max(0.0);
                    let k = 1.0 - (-dt / BARO_TAU_S).exp();
                    o + k * (target - o)
                }
                _ => target,
            });
            last_t = Some(*t);
        }
        at_fix.push(Some(b + offset.unwrap_or(0.0)));
    }
    // The drift corrected along the samples: the offset at the nearest fix.
    let corrected: Vec<f64> = if has_relief {
        let offs: Vec<(i64, f64)> = fixes
            .iter()
            .zip(&at_fix)
            .map(|((t, _), z)| (*t, z.unwrap_or(0.0) - baro_at(*t)))
            .collect();
        pressure
            .iter()
            .zip(&baro_all)
            .map(|((t, _), b)| {
                let i = offs.partition_point(|x| x.0 < *t).min(offs.len() - 1);
                b + offs[i].1
            })
            .collect()
    } else {
        baro_all
    };
    let (ascent_m, descent_m) = climb(&corrected);
    Some(Altitude {
        source: if has_relief {
            AltitudeSource::BarometerAndRelief
        } else {
            AltitudeSource::Barometer
        },
        at_fix,
        ascent_m,
        descent_m,
        range_m: range(&corrected),
    })
}

fn range(z: &[f64]) -> Option<(f64, f64)> {
    let lo = z.iter().copied().fold(f64::INFINITY, f64::min);
    let hi = z.iter().copied().fold(f64::NEG_INFINITY, f64::max);
    (lo.is_finite() && hi.is_finite()).then_some((lo, hi))
}

// ---------------------------------------------------------------------------------------
// Outings
// ---------------------------------------------------------------------------------------

/// One outing of the history.
#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct OutingSummary {
    /// Its directory.
    pub dir: String,
    /// `day` (day plan), `fog` (bearing), `offpiste`.
    pub kind: String,
    /// Site, when known.
    pub site_id: Option<String>,
    /// Loop, route or point.
    pub title: String,
    /// Day, `YYYY-MM-DD`.
    pub date: String,
    /// « 15/02 à 9 h 30 ».
    pub when: String,
    /// Start, epoch milliseconds (0: never started).
    pub start_ms: i64,
    /// Skied, km.
    pub distance_km: f64,
    /// Moving, seconds.
    pub moving_s: f64,
    /// First to last fix, seconds.
    pub elapsed_s: f64,
    /// Estimated (the group's time, else the model), seconds.
    pub estimated_s: Option<f64>,
}

/// The recap of an outing.
#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct OutingRecap {
    /// The outing.
    pub summary: OutingSummary,
    /// Lines: distance, time, estimated against actual, climb.
    pub figures: Vec<String>,
    /// Comparison with the previous outings.
    pub comparisons: Vec<String>,
    /// Conditions forecast over the hours skied, altitude range measured.
    pub conditions: Vec<String>,
    /// Climb, metres, and where it comes from.
    pub ascent_m: Option<f64>,
    /// Source of the altitude.
    pub altitude_source: Option<AltitudeSource>,
    /// Written recap.
    pub text: String,
    /// Spoken recap (short).
    pub spoken: String,
}

struct Loaded {
    summary: OutingSummary,
    plan: Option<DayPlan>,
    pack_dir: Option<PathBuf>,
    fixes: Vec<(i64, LonLat)>,
}

fn when_fr(ms: i64) -> String {
    let t = skimap_core::ephemeris::local_time(utc(ms));
    format!(
        "{:02}/{:02} à {}",
        t.day(),
        t.month(),
        skimap_plan::pace::clock_fr(t)
    )
}

fn load(dir: &Path) -> Option<Loaded> {
    let inputs = read_inputs(&dir.join(INPUTS_FILE)).ok()?;
    let fixes = fixes(&inputs);
    let (d, moving, elapsed) = measure(&fixes);
    let start_ms = fixes.first().map_or(0, |f| f.0);
    let (kind, site, title, date, plan, pack_dir, estimated) =
        if let Ok(spec) = GuidedSpec::read(dir) {
            let est = match spec.kind {
                GuidedKind::Line => {
                    let model = spec
                        .group_json
                        .as_deref()
                        .and_then(|j| serde_json::from_str(j).ok());
                    Some(crate::fog::personal_time(model.as_ref(), spec.model_s).0)
                }
                GuidedKind::Bearing => None,
            };
            let date = if start_ms > 0 {
                skimap_core::ephemeris::local_time(utc(start_ms))
                    .date()
                    .to_string()
            } else {
                spec.created_at.get(..10).unwrap_or_default().to_owned()
            };
            (
                match spec.kind {
                    GuidedKind::Bearing => "fog",
                    GuidedKind::Line => "offpiste",
                },
                spec.site_id.clone(),
                spec.label.clone(),
                date,
                None,
                spec.pack_dir.clone().map(PathBuf::from),
                est,
            )
        } else {
            let json = std::fs::read_to_string(dir.join(PLAN_FILE)).ok()?;
            let p = DayPlan::from_json(&json).ok()?;
            let est = p
                .pace
                .as_ref()
                .map(|x| x.prediction.median_s)
                .unwrap_or(p.route.model_min * 60.0);
            let pack = std::fs::read_to_string(dir.join(PACK_FILE))
                .ok()
                .map(|s| PathBuf::from(s.trim()));
            (
                "day",
                Some(p.site.clone()),
                p.route.label.clone(),
                p.date.to_string(),
                Some(p),
                pack,
                Some(est),
            )
        };
    Some(Loaded {
        summary: OutingSummary {
            dir: dir.display().to_string(),
            kind: kind.into(),
            site_id: site,
            title,
            date,
            when: if start_ms > 0 {
                when_fr(start_ms)
            } else {
                String::new()
            },
            start_ms,
            distance_km: d / 1000.0,
            moving_s: moving,
            elapsed_s: elapsed,
            estimated_s: estimated,
        },
        plan,
        pack_dir,
        fixes,
    })
}

/// The outings under `plans_root` that were started (at least two fixes), the latest
/// first.
#[uniffi::export]
pub fn outing_history(plans_root: String) -> Vec<OutingSummary> {
    let Ok(rd) = std::fs::read_dir(&plans_root) else {
        return Vec::new();
    };
    let mut v: Vec<OutingSummary> = rd
        .filter_map(Result::ok)
        .filter(|e| e.path().is_dir())
        .filter_map(|e| load(&e.path()))
        .filter(|l| l.fixes.len() >= 2)
        .map(|l| l.summary)
        .collect();
    v.sort_by_key(|o| std::cmp::Reverse(o.start_ms));
    v
}

fn km(x: f64, lang: Lang) -> String {
    let s = format!("{x:.1}");
    match lang {
        Lang::Fr => format!("{} km", s.replace('.', ",")),
        Lang::En => format!("{s} km"),
    }
}

fn speed_kmh(d_km: f64, s: f64) -> Option<f64> {
    (s > 60.0).then(|| d_km / (s / 3600.0))
}

/// « 10 min de plus », « 5 min de moins », « comme prévu » (within 5 min).
fn delta_fr(actual: f64, est: f64) -> String {
    let d = actual - est;
    if d.abs() < 300.0 {
        "comme prévu".into()
    } else if d > 0.0 {
        format!("{} de plus que prévu", duration_fr(d))
    } else {
        format!("{} de moins que prévu", duration_fr(-d))
    }
}

/// The recap of the outing in `dir`, compared with the other outings under `plans_root`;
/// `work_dir`: cache of the relief of the pack (altitude, [`crate::offpiste`]).
#[uniffi::export]
pub fn outing_recap(
    dir: String,
    plans_root: String,
    work_dir: String,
) -> Result<OutingRecap, TrackingError> {
    let l = load(Path::new(&dir)).ok_or_else(|| invalid(format!("{dir} : pas de sortie")))?;
    let s = &l.summary;
    let lang = Lang::Fr;
    // Relief of the pack for the altitude.
    let dem = l
        .pack_dir
        .as_deref()
        .and_then(|p| crate::offpiste::Relief::open(p, Path::new(&work_dir)).ok());
    let dem_fn = dem.as_ref().map(|r| move |p: LonLat| r.at(p));
    let alt = altitude(
        Path::new(&dir),
        &l.fixes,
        dem_fn.as_ref().map(|f| f as &dyn Fn(LonLat) -> Option<f64>),
    );
    let mut figures = vec![
        format!("Distance : {}", km(s.distance_km, lang)),
        format!(
            "Temps : {} dont {} en mouvement",
            duration_fr(s.elapsed_s),
            duration_fr(s.moving_s)
        ),
    ];
    if let Some(v) = speed_kmh(s.distance_km, s.moving_s) {
        figures.push(format!(
            "Vitesse en mouvement : {} km/h",
            format!("{v:.1}").replace('.', ",")
        ));
    }
    let estimate_said = s.estimated_s.map(|e| {
        let who = if l.plan.as_ref().is_some_and(|p| p.pace.is_some()) {
            "au rythme du groupe"
        } else {
            "au modèle"
        };
        figures.push(format!(
            "Temps estimé {who} : {}, réel : {} ({})",
            duration_fr(e),
            duration_fr(s.elapsed_s),
            delta_fr(s.elapsed_s, e)
        ));
        delta_fr(s.elapsed_s, e)
    });
    if let Some(a) = &alt {
        let src = match a.source {
            AltitudeSource::BarometerAndRelief => "baromètre corrigé par le relief",
            AltitudeSource::Barometer => "baromètre",
            AltitudeSource::Relief => "relief sous la trace",
        };
        figures.push(format!(
            "Dénivelé : {:.0} m en montée, {:.0} m en descente ({src})",
            a.ascent_m, a.descent_m
        ));
    }
    // Comparisons with the previous outings.
    let history: Vec<OutingSummary> = outing_history(plans_root)
        .into_iter()
        .filter(|h| h.dir != s.dir && h.start_ms < s.start_ms)
        .collect();
    let mut comparisons = Vec::new();
    let same: Vec<&OutingSummary> = history.iter().filter(|h| h.title == s.title).collect();
    if let Some(last) = same.first() {
        let n = same.len() + 1;
        let d = s.elapsed_s - last.elapsed_s;
        let how = if d.abs() < 300.0 {
            "à peu près le même temps".to_owned()
        } else if d < 0.0 {
            format!("{} plus vite", duration_fr(-d))
        } else {
            format!("{} plus lentement", duration_fr(d))
        };
        comparisons.push(format!(
            "{n}e fois sur ce parcours ; la dernière fois ({}) : {}, aujourd'hui {how}.",
            last.when,
            duration_fr(last.elapsed_s)
        ));
    }
    let speeds: Vec<f64> = history
        .iter()
        .filter(|h| h.site_id == s.site_id)
        .filter_map(|h| speed_kmh(h.distance_km, h.moving_s))
        .collect();
    if let (Some(v), false) = (speed_kmh(s.distance_km, s.moving_s), speeds.is_empty()) {
        let mean = speeds.iter().sum::<f64>() / speeds.len() as f64;
        comparisons.push(format!(
            "Vitesse en mouvement {} km/h, contre {} km/h en moyenne sur {} sortie{} précédente{} ici.",
            format!("{v:.1}").replace('.', ","),
            format!("{mean:.1}").replace('.', ","),
            speeds.len(),
            if speeds.len() > 1 { "s" } else { "" },
            if speeds.len() > 1 { "s" } else { "" },
        ));
    }
    if comparisons.is_empty() {
        comparisons
            .push("Première sortie enregistrée de ce genre : rien à comparer encore.".into());
    }
    let conditions = conditions(&l, alt.as_ref());
    let mut text = format!("{} — {}\n", s.title, s.when);
    for f in figures.iter().chain(&comparisons).chain(&conditions) {
        text.push_str("• ");
        text.push_str(f);
        text.push('\n');
    }
    let spoken = format!(
        "Bilan : {} en {}{}.{}",
        distance_words(s.distance_km * 1000.0, lang),
        duration_fr(s.elapsed_s),
        estimate_said.map(|d| format!(", {d}")).unwrap_or_default(),
        comparisons
            .first()
            .filter(|_| !same.is_empty())
            .map(|c| format!(" {c}"))
            .unwrap_or_default()
    );
    Ok(OutingRecap {
        summary: s.clone(),
        figures,
        comparisons,
        conditions,
        ascent_m: alt.as_ref().map(|a| a.ascent_m),
        altitude_source: alt.as_ref().map(|a| a.source),
        text,
        spoken,
    })
}

fn conditions(l: &Loaded, alt: Option<&Altitude>) -> Vec<String> {
    let mut out = Vec::new();
    let (Some(a), Some(b)) = (l.fixes.first(), l.fixes.last()) else {
        return out;
    };
    let from: NaiveDateTime = skimap_core::ephemeris::local_time(utc(a.0));
    let to: NaiveDateTime = skimap_core::ephemeris::local_time(utc(b.0));
    if let Some(w) = l.plan.as_ref().and_then(|p| p.weather.as_ref()) {
        let hours: Vec<_> = w
            .hours
            .iter()
            .filter(|h| h.time + chrono::Duration::hours(1) > from && h.time <= to)
            .collect();
        let temps: Vec<f64> = hours.iter().filter_map(|h| h.temperature_c).collect();
        if let (Some(lo), Some(hi)) = (
            temps.iter().copied().reduce(f64::min),
            temps.iter().copied().reduce(f64::max),
        ) {
            out.push(format!("Température prévue : {lo:.0} à {hi:.0} °C"));
        }
        if let Some(g) = hours
            .iter()
            .filter_map(|h| h.gust_kmh.or(h.wind_kmh))
            .reduce(f64::max)
        {
            out.push(format!("Vent prévu : rafales jusqu'à {g:.0} km/h"));
        }
        let fog = hours.iter().filter(|h| h.fog == Some(true)).count();
        if fog > 0 {
            out.push(format!("Brouillard prévu sur {fog} h"));
        }
        let snow: f64 = hours.iter().filter_map(|h| h.snowfall_cm).sum();
        if snow >= 0.5 {
            out.push(format!(
                "Neige prévue : {} cm",
                format!("{snow:.1}").replace('.', ",")
            ));
        }
        if !hours.is_empty() {
            out.push(format!(
                "Source : prévision du pack au point « {} »",
                w.name
            ));
        }
    }
    if let Some((lo, hi)) = alt.and_then(|a| a.range_m)
        && alt.is_some_and(|a| a.source != AltitudeSource::Barometer)
    {
        out.push(format!("Altitude : de {lo:.0} à {hi:.0} m"));
    }
    if out.is_empty() {
        out.push("Conditions : aucune prévision dans cette sortie.".into());
    }
    out
}

/// Deletes the outing in `dir` (its trace, plan and recap); it must be under
/// `plans_root`.
#[uniffi::export]
pub fn outing_delete(dir: String, plans_root: String) -> Result<(), TrackingError> {
    let root = std::fs::canonicalize(&plans_root).map_err(invalid)?;
    let d = std::fs::canonicalize(&dir).map_err(invalid)?;
    if d.parent() != Some(root.as_path()) {
        return Err(invalid(format!(
            "{dir} n'est pas une sortie de {plans_root}"
        )));
    }
    std::fs::remove_dir_all(&d).map_err(invalid)
}

/// Deletes every outing under `plans_root` except `keep` (the outing in progress);
/// returns how many were deleted.
#[uniffi::export]
pub fn outing_delete_all(plans_root: String, keep: Option<String>) -> Result<u32, TrackingError> {
    let keep = keep.and_then(|k| std::fs::canonicalize(k).ok());
    let mut n = 0;
    let Ok(rd) = std::fs::read_dir(&plans_root) else {
        return Ok(0);
    };
    for e in rd.filter_map(Result::ok) {
        let p = e.path();
        if !p.is_dir() || std::fs::canonicalize(&p).ok() == keep {
            continue;
        }
        std::fs::remove_dir_all(&p).map_err(invalid)?;
        n += 1;
    }
    Ok(n)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fog::{FogTarget, guided_bearing};

    const T0: i64 = 1_771_146_000_000;

    fn write_fixes(dir: &Path, start: i64, n: usize, step_m: f64, dt_s: i64) {
        let mut s = String::new();
        for i in 0..n {
            let t = utc(start + i as i64 * dt_s * 1000);
            let pos = LonLat::new(6.0, 45.0 + i as f64 * step_m / 111_195.0);
            s.push_str(&serde_json::to_string(&Input::Fix { t, pos }).unwrap());
            s.push('\n');
        }
        std::fs::write(dir.join(INPUTS_FILE), s).unwrap();
    }

    fn outing(root: &Path, name: &str, start: i64, n: usize) -> PathBuf {
        let d = root.join(name);
        guided_bearing(
            d.display().to_string(),
            FogTarget {
                kind: "refuge".into(),
                name: "Refuge test".into(),
                lon: 6.0,
                lat: 45.05,
                distance_m: None,
            },
            None,
            0.0,
            Lang::Fr,
            Some("cervieres".into()),
            None,
        )
        .unwrap();
        write_fixes(&d, start, n, 10.0, 5);
        d
    }

    #[test]
    fn measures_skip_jumps_and_stops() {
        let a = LonLat::new(6.0, 45.0);
        let f = vec![
            (0, a),
            (10_000, LonLat::new(6.0, 45.0 + 20.0 / 111_195.0)),
            // Stopped 60 s.
            (70_000, LonLat::new(6.0, 45.0 + 20.0 / 111_195.0)),
            // A GPS jump of 1 km in 10 s.
            (80_000, LonLat::new(6.0, 45.0 + 1020.0 / 111_195.0)),
        ];
        let (d, moving, elapsed) = measure(&f);
        assert!((d - 20.0).abs() < 0.5, "{d}");
        assert!((moving - 10.0).abs() < 1e-9);
        assert!((elapsed - 80.0).abs() < 1e-9);
    }

    #[test]
    fn barometer_fused_with_relief() {
        let dir = tempfile::tempdir().unwrap();
        // 20 min climbing 120 m: pressure drops ~14 hPa; the weather adds a drift of
        // +2 hPa (as if 17 m lower) that the relief corrects.
        let n = 241;
        let mut fixes = Vec::new();
        let mut lines = String::new();
        for i in 0..n {
            let t = T0 + i as i64 * 5000;
            let z = 1500.0 + 120.0 * i as f64 / (n - 1) as f64;
            fixes.push((t, LonLat::new(6.0, 45.0 + i as f64 * 5.0 / 111_195.0)));
            let p0 = 845.0;
            let p = p0 * (1.0 - (z - 1500.0) / 44_330.8).powf(1.0 / 0.190_263)
                + 2.0 * i as f64 / (n - 1) as f64;
            lines.push_str(&format!("{t} {p:.3}\n"));
        }
        std::fs::write(dir.path().join(PRESSURE_FILE), lines).unwrap();
        let dem = |p: LonLat| Some(1500.0 + (p.lat - 45.0) * 111_195.0 / 5.0 * 0.5);
        let a = altitude(dir.path(), &fixes, Some(&dem)).unwrap();
        assert_eq!(a.source, AltitudeSource::BarometerAndRelief);
        assert!((a.ascent_m - 120.0).abs() < 15.0, "{}", a.ascent_m);
        let last = a.at_fix.last().unwrap().unwrap();
        assert!((last - 1620.0).abs() < 15.0, "{last}");
        let b = altitude(dir.path(), &fixes, None).unwrap();
        assert_eq!(b.source, AltitudeSource::Barometer);
        // Without the relief the weather's drift eats part of the climb.
        assert!(b.ascent_m < a.ascent_m, "{} {}", b.ascent_m, a.ascent_m);
        assert!((baro_height(845.0, 845.0)).abs() < 1e-9);
    }

    #[test]
    fn history_recap_and_delete() {
        let root = tempfile::tempdir().unwrap();
        let first = outing(root.path(), "a", T0, 400);
        let second = outing(root.path(), "b", T0 + 86_400_000, 160);
        // Never started: not in the history.
        let _ = outing(root.path(), "c", T0 + 2 * 86_400_000, 1);
        let h = outing_history(root.path().display().to_string());
        assert_eq!(h.len(), 2);
        assert_eq!(h[0].dir, second.display().to_string());
        assert!(
            (h[1].distance_km - 3.99).abs() < 0.05,
            "{}",
            h[1].distance_km
        );
        let work = tempfile::tempdir().unwrap();
        let r = outing_recap(
            second.display().to_string(),
            root.path().display().to_string(),
            work.path().display().to_string(),
        )
        .unwrap();
        eprintln!("{}\n{}", r.text, r.spoken);
        assert!(r.text.contains("Distance : 1,6 km"), "{}", r.text);
        assert!(
            r.comparisons[0].starts_with("2e fois sur ce parcours"),
            "{:?}",
            r.comparisons
        );
        assert!(
            r.comparisons[0].contains("plus vite"),
            "{:?}",
            r.comparisons
        );
        assert!(
            r.spoken.starts_with("Bilan : 1,6 kilomètre"),
            "{}",
            r.spoken
        );
        outing_delete(
            first.display().to_string(),
            root.path().display().to_string(),
        )
        .unwrap();
        assert!(!first.exists());
        assert!(outing_delete("/tmp".into(), root.path().display().to_string()).is_err());
        let n = outing_delete_all(
            root.path().display().to_string(),
            Some(second.display().to_string()),
        )
        .unwrap();
        assert_eq!(n, 1);
        assert!(second.exists());
    }
}
