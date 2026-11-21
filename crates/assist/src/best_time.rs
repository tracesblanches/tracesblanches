//! Best time to leave (A9, #96): slot combining weather, snow quality (A3), light, crowd
//! (A4) and ETA.
//!
//! Every start time from 08 h (or 30 min after sunrise) every 30 minutes is tried, as long
//! as the outing (its estimated time × 1.2, the C7 margin) ends before the C7 deadline
//! (light limit minus 30 min). Each start gets points over the hours of the outing
//! (judgement, documented here and to be tuned with T5):
//!
//! | Factor | Points |
//! |---|---|
//! | snow surface (A3, mean score of the slope classes asked, flat by default) | mean score − 2 (fresh or transformed +1, old dry +0.5, hard or wet −1, icy or slush −2) |
//! | gusts ≥ 40 km/h | −1 × share of the hours |
//! | fog | −0.5 × share |
//! | snowfall ≥ 1 cm/h | −0.3 × share |
//! | rain | −1 × share |
//! | wind chill ≤ −15 °C | −0.5 × share |
//! | sun (cloud < 30 % with the sun up) | +0.2 × share |
//! | crowd (A4) on a busy or very busy day: share of the outing between 10 h 30 and 14 h (assumed peak, no attendance data) | −0.5 or −1 × share |
//! | return less than 30 min before the deadline | −0.5 |
//!
//! The best start is the highest score, the earliest on a tie.

use jiff::{
    SignedDuration,
    civil::{Date, DateTime, Time},
};
use serde::Serialize;
use skimap_core::turnaround::{self, TurnaroundParams};
use skimap_voice::Event;
use skimap_voice::bank::Briefing as Line;
use skimap_voice::quantity::Clock;

use crate::briefing::{Factor, Reason};
use crate::crowd::CrowdLevel;
use crate::layers::WeatherPoint;
use crate::snow_quality::DayQuality;
use crate::text::Lang;

/// A request.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Request {
    /// Date.
    pub date: Date,
    /// Estimated time of the outing (C5 model time), minutes.
    pub duration_min: f64,
    /// Earliest start.
    pub earliest: Time,
    /// Slope classes of the outing (A3 codes); empty: flat (groomed trails).
    pub classes: Vec<String>,
}

/// A start time and its points.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Start {
    /// Start, local.
    pub start: DateTime,
    /// Planned return (duration × 1.2), local.
    pub back: DateTime,
    /// Score.
    pub score: f64,
    /// Reasons.
    pub reasons: Vec<Reason>,
}

/// Best time of a day.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct BestTime {
    /// Request.
    pub request: Request,
    /// Forecast point.
    pub point: String,
    /// C7 deadline, local.
    pub deadline: Option<DateTime>,
    /// Best start.
    pub best: Option<Start>,
    /// Every start tried.
    pub starts: Vec<Start>,
    /// Text, in the language asked ([`best_time_in`]).
    pub text: String,
}

impl BestTime {
    /// Voice line « Meilleur moment pour partir. Dix heures trente. ».
    pub fn voice(&self) -> Option<Event> {
        self.best.as_ref().map(|b| {
            Event::Briefing(Line::BestTime {
                at: Clock::nearest_quarter(b.start.hour() as u8, b.start.minute() as u8),
            })
        })
    }
}

fn push(out: &mut Vec<Reason>, factor: Factor, points: f64, text: String) {
    out.push(Reason {
        factor,
        points,
        text,
    });
}

/// Best time to leave at a point, with its snow quality of the day and the crowd level;
/// text in French ([`best_time_in`]).
pub fn best_time(
    point: &WeatherPoint,
    quality: Option<&DayQuality>,
    crowd: Option<CrowdLevel>,
    req: &Request,
) -> BestTime {
    best_time_in(point, quality, crowd, req, Lang::Fr)
}

/// [`best_time`] with the texts in `lang` (the phone's language, VO6).
pub fn best_time_in(
    point: &WeatherPoint,
    quality: Option<&DayQuality>,
    crowd: Option<CrowdLevel>,
    req: &Request,
    lang: Lang,
) -> BestTime {
    let tx = crate::text::of(lang);
    let p = TurnaroundParams::default();
    let deadline = turnaround::deadline(req.date, point.position, &p)
        .map(|d| skimap_core::ephemeris::local_time(d.deadline));
    let sunrise = skimap_core::ephemeris::sun_times(req.date, point.position)
        .map(|(r, _)| skimap_core::ephemeris::local_time(r));
    let planned = SignedDuration::from_secs((req.duration_min * 60.0 * p.eta_factor) as i64);
    let mut t = req.date.to_datetime(req.earliest);
    if let Some(r) = sunrise {
        let after = r + SignedDuration::from_mins(30);
        while t < after {
            t += SignedDuration::from_mins(30);
        }
    }
    let classes: Vec<&crate::snow_quality::ClassDay> = match quality {
        Some(q) if q.enough_snow => {
            if req.classes.is_empty() {
                vec![&q.classes[0]]
            } else {
                q.classes
                    .iter()
                    .filter(|c| req.classes.contains(&c.code))
                    .collect()
            }
        }
        _ => Vec::new(),
    };
    let mut starts = Vec::new();
    // Starts that end before the deadline but have no hourly forecast.
    let mut unforecast = 0;
    while deadline.is_none_or(|d| t + planned <= d) && t.hour() < 18 {
        let back = t + planned;
        let hours = point.between(t - SignedDuration::from_mins(i64::from(t.minute())), back);
        let mut reasons = Vec::new();
        if hours.is_empty() {
            unforecast += 1;
            t += SignedDuration::from_mins(30);
            continue;
        }
        let n = hours.len() as f64;
        let share = |f: &dyn Fn(&crate::layers::WeatherHour) -> bool| {
            hours.iter().filter(|h| f(h)).count() as f64 / n
        };
        // Snow.
        let mut scores = Vec::new();
        for c in &classes {
            for h in &c.hours {
                if h.time + SignedDuration::from_hours(1) > t && h.time < back {
                    scores.push(h.score);
                }
            }
        }
        if !scores.is_empty() {
            let m = scores.iter().sum::<f64>() / scores.len() as f64;
            let what: Vec<String> = classes
                .iter()
                .map(|c| {
                    let qs: Vec<&str> = c
                        .hours
                        .iter()
                        .filter(|h| h.time + SignedDuration::from_hours(1) > t && h.time < back)
                        .map(|h| tx.quality(h.quality))
                        .collect();
                    let mut uniq: Vec<&str> = Vec::new();
                    for q in qs {
                        if !uniq.contains(&q) {
                            uniq.push(q);
                        }
                    }
                    tx.bt_surface(&tx.slope_class(&c.class), &uniq)
                })
                .collect();
            push(
                &mut reasons,
                Factor::SnowSurface,
                m - 2.0,
                what.join(tx.bt_sep()),
            );
        }
        let gust = share(&|h| h.gust_kmh.is_some_and(|g| g >= 40.0));
        if gust > 0.0 {
            let g = hours.iter().filter_map(|h| h.gust_kmh).fold(0.0, f64::max);
            push(&mut reasons, Factor::Wind, -gust, tx.bt_gusts(g));
        }
        let fog = share(&|h| h.fog == Some(true));
        if fog > 0.0 {
            push(
                &mut reasons,
                Factor::Fog,
                -0.5 * fog,
                tx.bt_fog((fog * n).round()),
            );
        }
        let snow = share(&|h| h.snowfall_cm.is_some_and(|s| s >= 1.0));
        if snow > 0.0 {
            push(
                &mut reasons,
                Factor::Snowfall,
                -0.3 * snow,
                tx.bt_snowfall().into(),
            );
        }
        let rain = share(&|h| {
            h.snowfall_cm.unwrap_or(0.0) <= 0.0
                && h.precipitation_mm.is_some_and(|p| p >= 0.2)
                && h.temperature_c.is_some_and(|t| t > 0.5)
        });
        if rain > 0.0 {
            push(&mut reasons, Factor::Rain, -rain, tx.bt_rain().into());
        }
        let cold = share(&|h| h.wind_chill_c.is_some_and(|c| c <= -15.0));
        if cold > 0.0 {
            let c = hours
                .iter()
                .filter_map(|h| h.wind_chill_c)
                .fold(0.0, f64::min);
            push(
                &mut reasons,
                Factor::Cold,
                -0.5 * cold,
                tx.bt_cold(&crate::briefing::degrees(c)),
            );
        }
        let sun = share(&|h| {
            h.cloud_cover_pct.is_some_and(|c| c < 30.0) && sunrise.is_some_and(|r| h.time >= r)
        });
        if sun > 0.0 {
            push(&mut reasons, Factor::Sun, 0.2 * sun, tx.bt_sun().into());
        }
        if let Some(level) = crowd.filter(|l| *l >= CrowdLevel::Busy) {
            let peak0 = req.date.at(10, 30, 0, 0);
            let peak1 = req.date.at(14, 0, 0, 0);
            let overlap = back
                .min(peak1)
                .duration_since(t.max(peak0))
                .as_mins()
                .max(0) as f64;
            let frac = overlap / planned.as_mins().max(1) as f64;
            if frac > 0.0 {
                let w = if level == CrowdLevel::VeryBusy {
                    1.0
                } else {
                    0.5
                };
                push(&mut reasons, Factor::Crowd, -w * frac, tx.bt_crowd(level));
            }
        }
        if let Some(d) = deadline
            && d.duration_since(back).as_mins() < 30
        {
            push(&mut reasons, Factor::Data, -0.5, tx.bt_near_deadline(d));
        }
        let score = reasons.iter().map(|r| r.points).sum();
        starts.push(Start {
            start: t,
            back,
            score,
            reasons,
        });
        t += SignedDuration::from_mins(30);
    }
    let best = starts
        .iter()
        .max_by(|a, b| a.score.total_cmp(&b.score).then(b.start.cmp(&a.start)))
        .cloned();
    let text = match &best {
        Some(b) => {
            let why: Vec<String> = b.reasons.iter().map(|r| r.text.clone()).collect();
            // What the best start two hours or more earlier, and later, would change.
            let worse = |o: &Start| {
                let lost: Vec<String> = o
                    .reasons
                    .iter()
                    .filter(|r| {
                        let mine = b.reasons.iter().find(|x| x.factor == r.factor);
                        r.points < 0.0 && mine.is_none_or(|x| x.points > r.points + 1e-9)
                            || r.factor == Factor::SnowSurface
                                && mine.is_some_and(|x| x.points > r.points + 1e-9)
                    })
                    .map(|r| r.text.clone())
                    .collect();
                (!lost.is_empty()).then(|| tx.bt_other_start(o.start, &lost))
            };
            let pick = |early: bool| {
                starts
                    .iter()
                    .filter(|s| {
                        let dm = s.start.duration_since(b.start).as_mins();
                        if early { dm <= -120 } else { dm >= 120 }
                    })
                    .max_by(|a, c| a.score.total_cmp(&c.score))
                    .and_then(worse)
            };
            let alts: Vec<String> = [pick(true), pick(false)].into_iter().flatten().collect();
            tx.bt_best(b.start, b.back, req.duration_min, &why, &alts)
        }
        None if unforecast > 0 => tx.bt_no_forecast().to_owned(),
        None => tx.bt_none(req.duration_min, deadline),
    };
    BestTime {
        request: req.clone(),
        point: point.id.clone(),
        deadline,
        best,
        starts,
        text,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::layers::WeatherHour;
    use jiff::civil::date;
    use jiff::civil::time;
    use skimap_common::geom::LonLat;

    fn point(gust_after: u32) -> WeatherPoint {
        let d0 = date(2027, 1, 12).at(0, 0, 0, 0);
        WeatherPoint {
            id: "p".into(),
            name: "P".into(),
            position: LonLat::new(6.7269, 44.8552),
            elevation_m: 1750.0,
            hours: (0..24)
                .map(|i| WeatherHour {
                    time: d0 + SignedDuration::from_hours(i),
                    temperature_c: Some(-5.0),
                    apparent_temperature_c: None,
                    wind_chill_c: Some(-8.0),
                    wind_kmh: Some(10.0),
                    gust_kmh: Some(if i as u32 >= gust_after { 60.0 } else { 15.0 }),
                    visibility_m: None,
                    fog: Some(false),
                    freezing_level_m: None,
                    snowfall_cm: Some(0.0),
                    precipitation_mm: Some(0.0),
                    cloud_cover_pct: Some(10.0),
                    relative_humidity_pct: None,
                    weather_code: None,
                })
                .collect(),
        }
    }

    #[test]
    fn leaves_before_the_wind_and_before_the_night() {
        let req = Request {
            date: date(2027, 1, 12),
            duration_min: 120.0,
            earliest: time(8, 0, 0, 0),
            classes: vec![],
        };
        // Gusts of 60 km/h from 12 h: the best start is the first one (30 min after
        // sunrise, ≈ 08:05 at sea level), before 10 h, back before the wind.
        let b = best_time(&point(12), None, None, &req);
        let best = b.best.clone().unwrap();
        assert_eq!(best.start, b.starts[0].start, "{}", b.text);
        assert!(
            best.start.hour() < 10 && best.back.hour() < 12,
            "{}",
            b.text
        );
        assert!(b.text.contains("rafales"), "{}", b.text);
        // Every start ends before the deadline.
        let d = b.deadline.unwrap();
        assert!(b.starts.iter().all(|s| s.back <= d));
        assert!(matches!(
            b.voice(),
            Some(Event::Briefing(Line::BestTime { .. }))
        ));
        // On a very busy day without wind, avoiding the assumed peak wins.
        let b = best_time(&point(24), None, Some(CrowdLevel::VeryBusy), &req);
        let s = b.best.unwrap().start;
        assert!(s.hour() < 9 || s.hour() >= 14, "{s}");
        // Too long for the day: no slot.
        let long = Request {
            duration_min: 600.0,
            ..req
        };
        assert!(best_time(&point(24), None, None, &long).best.is_none());
    }

    #[test]
    fn a_day_without_forecast_says_so() {
        // The point's hours are those of 12/01: on 14/01 there is no forecast, the outing
        // fits the day well; the text tells the missing forecast, not the deadline.
        let day = Request {
            date: date(2027, 1, 14),
            ..req(120.0)
        };
        for lang in [Lang::Fr, Lang::En] {
            let b = best_time_in(&point(24), None, None, &day, lang);
            assert!(b.best.is_none());
            assert!(
                !b.text.contains("limite") && !b.text.contains("deadline"),
                "{}",
                b.text
            );
            assert!(
                b.text.contains("prévision") || b.text.contains("forecast"),
                "{}",
                b.text
            );
        }
    }

    fn req(minutes: f64) -> Request {
        Request {
            date: date(2027, 1, 12),
            duration_min: minutes,
            earliest: time(8, 0, 0, 0),
            classes: vec![],
        }
    }

    /// The test point with every hour changed by `f(hour)`.
    fn point_with(f: impl Fn(u32, &mut WeatherHour)) -> WeatherPoint {
        let mut p = point(24);
        for h in &mut p.hours {
            f(h.time.hour() as u32, h);
        }
        p
    }

    fn start(b: &BestTime, h: u32, m: u32) -> &Start {
        b.starts
            .iter()
            .find(|s| s.start.hour() as u32 == h && s.start.minute() as u32 == m)
            .unwrap()
    }

    fn reason(s: &Start, f: Factor) -> Option<&Reason> {
        s.reasons.iter().find(|r| r.factor == f)
    }

    #[test]
    fn snowfall_rain_sun_thresholds() {
        // 1.5 cm/h from 12 to 14 h: an outing of 2 h (2 h 24 planned) from 11 h has 2 of its
        // 3 hours (11, 12 and 13 h) of snowfall.
        let p = point_with(|h, w| {
            w.cloud_cover_pct = Some(80.0);
            if (12..14).contains(&h) {
                w.snowfall_cm = Some(1.5);
            }
        });
        let b = best_time(&p, None, None, &req(120.0));
        let s = start(&b, 11, 0);
        let r = reason(s, Factor::Snowfall).unwrap();
        assert!((r.points + 0.3 * 2.0 / 3.0).abs() < 1e-12, "{r:?}");
        assert_eq!(r.text, "chute de neige");
        assert!(reason(start(&b, 9, 0), Factor::Snowfall).is_none());
        // Under 1 cm/h: not counted.
        let p = point_with(|_, w| {
            w.cloud_cover_pct = Some(80.0);
            w.snowfall_cm = Some(0.9);
        });
        assert!(
            best_time(&p, None, None, &req(120.0))
                .starts
                .iter()
                .all(|s| reason(s, Factor::Snowfall).is_none())
        );
        // Rain: precipitation without snowfall above +0.5 °C (0.5 °C itself is not).
        let rain = |t: f64| {
            let p = point_with(|_, w| {
                w.cloud_cover_pct = Some(80.0);
                w.temperature_c = Some(t);
                w.precipitation_mm = Some(0.2);
            });
            best_time(&p, None, None, &req(120.0))
        };
        let b = rain(0.6);
        assert_eq!(reason(&b.starts[0], Factor::Rain).unwrap().points, -1.0);
        assert!(reason(&rain(0.5).starts[0], Factor::Rain).is_none());
        // Sun: cloud cover under 30 % (30 itself is not), with the sun up.
        let sun = |c: f64| {
            let p = point_with(|_, w| w.cloud_cover_pct = Some(c));
            best_time(&p, None, None, &req(120.0))
        };
        let r = reason(&sun(29.0).starts[0], Factor::Sun).unwrap().clone();
        assert!((r.points - 0.2).abs() < 1e-12);
        assert!(reason(&sun(30.0).starts[0], Factor::Sun).is_none());
        // Wind chill down to −15 °C.
        let p = point_with(|h, w| {
            w.cloud_cover_pct = Some(80.0);
            w.wind_chill_c = Some(if h < 10 { -15.0 } else { -14.9 });
        });
        let b = best_time(&p, None, None, &req(120.0));
        let r = reason(&b.starts[0], Factor::Cold).unwrap();
        assert!(
            r.points < 0.0 && r.text == "ressenti jusqu'à -15 °C",
            "{r:?}"
        );
        assert!(reason(start(&b, 10, 0), Factor::Cold).is_none());
    }

    #[test]
    fn crowd_share_of_the_peak_and_the_deadline_margin() {
        let p = point_with(|_, w| w.cloud_cover_pct = Some(80.0));
        // A very busy day, 9 h to 11 h 24: 54 of 144 minutes in the peak (10 h 30 – 14 h).
        let b = best_time(&p, None, Some(CrowdLevel::VeryBusy), &req(120.0));
        let r = reason(start(&b, 9, 0), Factor::Crowd).unwrap();
        assert!((r.points + 54.0 / 144.0).abs() < 1e-12, "{r:?}");
        assert_eq!(
            r.text,
            "journée très chargé : pic supposé de 10 h 30 à 14 h"
        );
        // Busy: half of it; moderate: nothing.
        let b = best_time(&p, None, Some(CrowdLevel::Busy), &req(120.0));
        let r = reason(start(&b, 9, 0), Factor::Crowd).unwrap();
        assert!((r.points + 0.5 * 54.0 / 144.0).abs() < 1e-12, "{r:?}");
        let b = best_time(&p, None, Some(CrowdLevel::Moderate), &req(120.0));
        assert!(b.starts.iter().all(|s| reason(s, Factor::Crowd).is_none()));
        // Back 30 min before the deadline or more: no point; less: −0.5.
        let d = b.deadline.unwrap();
        let s0 = b.starts[2].start;
        // An outing that ends 30 min 20 s before the deadline from that start.
        let planned_s = d.duration_since(s0).as_secs() - 30 * 60 - 20;
        let b = best_time(&p, None, None, &req(planned_s as f64 / 72.0));
        let s = b.starts.iter().find(|s| s.start == s0).unwrap();
        assert_eq!(d.duration_since(s.back).as_mins(), 30);
        assert!(reason(s, Factor::Data).is_none(), "{s:?}");
        // The next one ends 20 s before the deadline: said.
        let next = b.starts.iter().find(|s| s.start > s0).unwrap();
        assert_eq!(reason(next, Factor::Data).unwrap().points, -0.5);
        let planned_s = d.duration_since(s0).as_secs() - 29 * 60;
        let b = best_time(&p, None, None, &req(planned_s as f64 / 72.0));
        let s = b.starts.iter().find(|s| s.start == s0).unwrap();
        assert_eq!(
            reason(s, Factor::Data).unwrap().text,
            format!(
                "retour à moins de 30 min de l'heure limite ({} h {:02})",
                d.hour(),
                d.minute()
            )
        );
    }

    #[test]
    fn hours_missing_in_the_morning() {
        // The forecast starts at 12 h: the starts before 9 h 36 have no hour of weather and
        // are skipped; the first one is 10 h (back at 12 h 24).
        let mut p = point(24);
        p.hours.retain(|h| h.time.hour() >= 12);
        let b = best_time(&p, None, None, &req(120.0));
        assert_eq!(b.starts[0].start.hour(), 10);
        assert_eq!(b.starts[0].start.minute(), 0);
        // No weather at all: no start, and the missing forecast said (not the deadline).
        p.hours.clear();
        let b = best_time(&p, None, None, &req(120.0));
        assert!(b.starts.is_empty() && b.best.is_none());
        assert_eq!(
            b.text,
            "Pas de meilleur moment : aucune prévision météo horaire pour ce jour dans le pack."
        );
        assert!(b.voice().is_none());
    }
}
