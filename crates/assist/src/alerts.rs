//! Condition-change alerts during the outing (A8, #95): rising wind, fog, snowfall,
//! temperature drop, turnaround time moved earlier by a delay.
//!
//! [`Monitor::update`] is called with the time and the distance travelled along the
//! planned route (from the map matching of C2 on the phone, from a synthetic trace in the
//! replays). Rules (parameters in [`Params`]):
//!
//! - **weather**, on the forecast hours of the next 60 minutes at the forecast point
//!   nearest to the start, compared with the current hour and with the departure:
//!   gusts reaching 40 km/h and 20 km/h more than at the departure (wind rising); fog
//!   while the current hour has none; snowfall of 0.5 cm/h or more while the current hour
//!   is dry; wind chill 6 °C lower, or air 5 °C lower, within 3 hours (temperature drop).
//!   Each kind is said again only after 2 hours. Weather alerts start 10 minutes after the
//!   departure (the briefing gives the conditions known then) and only look at the hours
//!   before the planned return.
//! - **turnaround**: the observed pace is the elapsed time over the model time (C5) to the
//!   current distance. After 20 minutes of planned time, from 10 minutes and 10 % late,
//!   the latest turnaround time is
//!   recomputed at that pace — the furthest point from which the way back (same path
//!   reversed, model time × pace × 1.2, the C7 margin) still ends before the C7 deadline —
//!   and said (`Event::TurnaroundAdvanced`) when the loop can no longer be finished in
//!   time and the turnaround is at least 10 minutes earlier than at the planned pace; when
//!   it has passed: `Event::Turnaround` « Faites demi-tour maintenant » (not after the
//!   deadline itself).
//!
//! [`day_replay`] runs the weather rules over a whole day (an outing from 10 h, planned
//! return at 16 h) to replay the archived forecasts.

use std::borrow::Cow;
use std::collections::HashMap;

use chrono::{Duration, NaiveDateTime, Timelike};
use serde::Serialize;
use skimap_core::turnaround::{self, TurnaroundParams};
use skimap_voice::Event;
use skimap_voice::bank::Change;
use skimap_voice::quantity::Clock;

use crate::layers::{WeatherHour, WeatherPoint};
use crate::suggest::Profile;

/// Thresholds.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct Params {
    /// Gusts that make the wind rising, km/h.
    pub gust_kmh: f64,
    /// Rise of the gusts since the departure, km/h.
    pub gust_rise_kmh: f64,
    /// Snowfall per hour, cm.
    pub snow_cm_h: f64,
    /// Drop of the wind chill within 3 h, °C.
    pub chill_drop_c: f64,
    /// Drop of the air temperature within 3 h, °C.
    pub temp_drop_c: f64,
    /// Look-ahead of the weather, minutes.
    pub lookahead_min: i64,
    /// Silence after an alert of the same kind, minutes.
    pub repeat_min: i64,
    /// Delay before the turnaround is recomputed, minutes.
    pub delay_min: f64,
    /// Relative delay, fraction.
    pub delay_frac: f64,
    /// Turnaround moved earlier by at least, minutes.
    pub advance_min: i64,
}

impl Default for Params {
    fn default() -> Self {
        Self {
            gust_kmh: 40.0,
            gust_rise_kmh: 20.0,
            snow_cm_h: 0.5,
            chill_drop_c: 6.0,
            temp_drop_c: 5.0,
            lookahead_min: 60,
            repeat_min: 120,
            delay_min: 10.0,
            delay_frac: 0.10,
            advance_min: 10,
        }
    }
}

/// Kind of alert.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    /// Wind rising.
    WindRising,
    /// Fog coming.
    Fog,
    /// Snow starting.
    Snow,
    /// Temperature dropping.
    TemperatureDrop,
    /// Turnaround moved earlier.
    TurnaroundAdvanced,
    /// Turn around now.
    TurnaroundNow,
}

/// An alert.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Alert {
    /// Time, local.
    pub at: NaiveDateTime,
    /// Kind.
    pub kind: Kind,
    /// Text, French.
    pub text: String,
    /// Voice event.
    #[serde(skip)]
    pub event: Event,
    /// Spoken text (phrase bank).
    pub spoken: String,
}

/// Monitor of an outing.
///
/// The route profile and the weather point are borrowed ([`Monitor::new`]) or owned
/// ([`Monitor::with`] with `Cow::Owned`, for a caller that computes routes on the fly);
/// [`Monitor::replace_route`] switches to another route from the same start without
/// forgetting which alerts were already said.
#[derive(Debug, Clone)]
pub struct Monitor<'a> {
    start: NaiveDateTime,
    profile: Cow<'a, Profile>,
    point: Cow<'a, WeatherPoint>,
    p: Params,
    deadline: Option<NaiveDateTime>,
    base_gust: f64,
    last: HashMap<Kind, NaiveDateTime>,
    planned_turn: Option<NaiveDateTime>,
    said_turn: Option<NaiveDateTime>,
    /// Lowest temperature and wind chill already announced.
    said_cold: (f64, f64),
}

fn hhmm(t: NaiveDateTime) -> String {
    format!("{} h {:02}", t.hour(), t.minute())
}

fn hour_at(point: &WeatherPoint, t: NaiveDateTime) -> Option<&WeatherHour> {
    let h0 =
        t - Duration::minutes(i64::from(t.minute())) - Duration::seconds(i64::from(t.second()));
    point.between(h0, h0 + Duration::hours(1)).first()
}

impl<'a> Monitor<'a> {
    /// Monitor of a route started at `start`, with the weather of `point`.
    pub fn new(
        start: NaiveDateTime,
        profile: &'a Profile,
        point: &'a WeatherPoint,
        p: Params,
    ) -> Self {
        Self::with(start, Cow::Borrowed(profile), Cow::Borrowed(point), p)
    }

    /// Monitor of a route started at `start`, with a borrowed or owned profile and point.
    pub fn with(
        start: NaiveDateTime,
        profile: Cow<'a, Profile>,
        point: Cow<'a, WeatherPoint>,
        p: Params,
    ) -> Self {
        let deadline =
            turnaround::deadline(start.date(), point.position, &TurnaroundParams::default())
                .map(|d| skimap_core::ephemeris::local_time(d.deadline));
        let base_gust = hour_at(&point, start)
            .and_then(|h| h.gust_kmh)
            .unwrap_or(0.0);
        let mut m = Self {
            start,
            profile,
            point,
            p,
            deadline,
            base_gust,
            last: HashMap::new(),
            planned_turn: None,
            said_turn: None,
            said_cold: (f64::MAX, f64::MAX),
        };
        m.planned_turn = m.latest_turn(start, 0.0, 1.0);
        m
    }

    /// Latest turnaround at a pace from the current position, `None` when the whole route
    /// can be finished before the deadline (or there is no deadline).
    fn latest_turn(&self, now: NaiveDateTime, along_m: f64, pace: f64) -> Option<NaiveDateTime> {
        let d = self.deadline?;
        let f = TurnaroundParams::default().eta_factor;
        let pr: &Profile = &self.profile;
        let i0 = pr.index_at(along_m);
        let secs = |s: f64| Duration::seconds(s.round() as i64);
        let finish = now + secs(pace * f * (pr.total_s() - pr.out_s[i0]));
        if finish <= d {
            return None;
        }
        let mut best = None;
        for i in i0..pr.out_s.len() {
            let ahead = pace * (pr.out_s[i] - pr.out_s[i0]);
            let back = pace * f * pr.back_s[i];
            if now + secs(ahead + back) <= d {
                best = Some(now + secs(ahead));
            } else {
                break;
            }
        }
        // Even turning now is too late: the latest turnaround has passed.
        Some(best.unwrap_or(now - Duration::minutes(1)))
    }

    fn may(&self, kind: Kind, now: NaiveDateTime) -> bool {
        self.last
            .get(&kind)
            .is_none_or(|t| (now - *t).num_minutes() >= self.p.repeat_min)
    }

    /// Alerts at `now`, having travelled `along_m` metres of the route.
    pub fn update(&mut self, now: NaiveDateTime, along_m: f64) -> Vec<Alert> {
        let mut out = Vec::new();
        let p = self.p;
        // The conditions known at the departure are the briefing's (A1, A7): weather alerts
        // start 10 minutes after it, and only look at hours before the planned return.
        let cur = (now >= self.start + Duration::minutes(10))
            .then(|| hour_at(&self.point, now).cloned())
            .flatten();
        let end = self.start
            + Duration::seconds(
                (self.profile.total_s() * TurnaroundParams::default().eta_factor) as i64,
            );
        let end = end.max(now + Duration::minutes(30));
        // Aggregates of the hours ahead (computed on the slices, without copying them: the
        // phone calls this once a minute during the outing).
        let (g_max, fog_ahead, s_ahead) = {
            let ahead = self
                .point
                .between(now, (now + Duration::minutes(p.lookahead_min)).min(end));
            (
                ahead.iter().filter_map(|h| h.gust_kmh).fold(0.0, f64::max),
                ahead.iter().any(|h| h.fog == Some(true)),
                ahead
                    .iter()
                    .filter_map(|h| h.snowfall_cm)
                    .fold(0.0, f64::max),
            )
        };
        let (chill_min, t_min) = {
            let three = self.point.between(now, (now + Duration::hours(3)).min(end));
            (
                three
                    .iter()
                    .filter_map(|h| h.wind_chill_c)
                    .fold(f64::MAX, f64::min),
                three
                    .iter()
                    .filter_map(|h| h.temperature_c)
                    .fold(f64::MAX, f64::min),
            )
        };
        let mut push = |m: &mut Self, kind: Kind, text: String, event: Event| {
            m.last.insert(kind, now);
            let spoken = event.announce().text();
            out.push(Alert {
                at: now,
                kind,
                text,
                event,
                spoken,
            });
        };
        if let Some(c) = &cur {
            let g_now = c.gust_kmh.unwrap_or(0.0);
            if g_max >= p.gust_kmh
                && g_max >= self.base_gust + p.gust_rise_kmh
                && g_now < p.gust_kmh
                && self.may(Kind::WindRising, now)
            {
                // Said again only if the gusts rise further.
                let before = std::mem::replace(&mut self.base_gust, g_max);
                push(
                    self,
                    Kind::WindRising,
                    format!(
                        "le vent se lève : rafales à {g_max:.0} km/h dans l'heure (avant : {before:.0})"
                    ),
                    Event::ConditionChange(Change::WindRising),
                );
            }
            if c.fog != Some(true) && fog_ahead && self.may(Kind::Fog, now) {
                push(
                    self,
                    Kind::Fog,
                    "brouillard prévu dans l'heure".into(),
                    Event::ConditionChange(Change::Fog),
                );
            }
            if c.snowfall_cm.unwrap_or(0.0) < 0.2
                && s_ahead >= p.snow_cm_h
                && self.may(Kind::Snow, now)
            {
                push(
                    self,
                    Kind::Snow,
                    format!("chute de neige dans l'heure ({s_ahead:.1} cm/h)").replace('.', ","),
                    Event::ConditionChange(Change::Snow),
                );
            }
            // A drop is said once: again only when it goes further than what was said.
            let chill_drop = c
                .wind_chill_c
                .is_some_and(|w| chill_min <= w.min(self.said_cold.1) - p.chill_drop_c);
            let temp_drop = c
                .temperature_c
                .is_some_and(|t| t_min <= t.min(self.said_cold.0) - p.temp_drop_c);
            if (chill_drop || temp_drop) && self.may(Kind::TemperatureDrop, now) {
                self.said_cold = (t_min, chill_min);
                push(
                    self,
                    Kind::TemperatureDrop,
                    format!(
                        "la température baisse : {:.0} °C, ressenti {:.0} °C d'ici 3 h",
                        t_min, chill_min
                    ),
                    Event::ConditionChange(Change::TemperatureDrop),
                );
            }
        }
        // Turnaround.
        let pr: &Profile = &self.profile;
        let i = pr.index_at(along_m);
        let planned = pr.out_s[i];
        let elapsed = (now - self.start).num_seconds() as f64;
        let late_s = elapsed - planned;
        let before_night = self.deadline.is_none_or(|d| now < d);
        if before_night
            && planned >= 1200.0
            && late_s >= p.delay_min * 60.0
            && late_s >= p.delay_frac * planned
        {
            let pace = elapsed / planned;
            if let Some(t) = self.latest_turn(now, along_m, pace) {
                if t <= now {
                    if self.may(Kind::TurnaroundNow, now) {
                        push(
                            self,
                            Kind::TurnaroundNow,
                            format!(
                                "en retard de {} min (allure × {:.2}) : faites demi-tour maintenant pour rentrer avant {}",
                                (late_s / 60.0).round(),
                                pace,
                                self.deadline.map(hhmm).unwrap_or_default()
                            )
                            .replace('.', ","),
                            Event::Turnaround {
                                seconds_left: 0.0,
                                deadline: Clock::floor_quarter(now.hour() as u8, now.minute() as u8),
                            },
                        );
                    }
                } else {
                    let earlier = match self.planned_turn {
                        Some(t0) => (t0 - t).num_minutes() >= p.advance_min,
                        None => true,
                    };
                    let new = self
                        .said_turn
                        .is_none_or(|s| (s - t).num_minutes() >= p.advance_min);
                    if earlier && new {
                        self.said_turn = Some(t);
                        let clock = Clock::floor_quarter(t.hour() as u8, t.minute() as u8);
                        push(
                            self,
                            Kind::TurnaroundAdvanced,
                            format!(
                                "en retard de {} min (allure × {:.2}) : la boucle ne finit plus avant {} ; demi-tour au plus tard à {}",
                                (late_s / 60.0).round(),
                                pace,
                                self.deadline.map(hhmm).unwrap_or_default(),
                                hhmm(t)
                            )
                            .replace('.', ","),
                            Event::TurnaroundAdvanced { deadline: clock },
                        );
                    }
                }
            }
        }
        out
    }

    /// Latest turnaround at the planned pace (`None`: the route ends before the deadline).
    pub fn planned_turnaround(&self) -> Option<NaiveDateTime> {
        self.planned_turn
    }

    /// Follows another route from the same start (a plan B that shares the way travelled
    /// so far): the planned turnaround is recomputed on it, the alerts already said and
    /// the weather baselines are kept, so nothing is said twice because of the switch.
    pub fn replace_route(&mut self, profile: Cow<'a, Profile>) {
        self.profile = profile;
        self.planned_turn = self.latest_turn(self.start, 0.0, 1.0);
    }

    /// Profile of the route followed.
    pub fn profile(&self) -> &Profile {
        &self.profile
    }

    /// C7 deadline, local.
    pub fn deadline(&self) -> Option<NaiveDateTime> {
        self.deadline
    }
}

/// Replays a trace (time, distance along the route) and collects every alert.
pub fn replay(
    start: NaiveDateTime,
    profile: &Profile,
    point: &WeatherPoint,
    trace: &[(f64, f64)],
    p: Params,
) -> Vec<Alert> {
    let mut m = Monitor::new(start, profile, point, p);
    trace
        .iter()
        .flat_map(|(t_s, along)| m.update(start + Duration::seconds(*t_s as i64), *along))
        .collect()
}

/// Weather alerts of a day: an outing from 10 h of 5 hours model time (planned return at
/// 16 h with the 1.2 margin), at the planned pace, one update every 5 minutes.
pub fn day_replay(point: &WeatherPoint, date: chrono::NaiveDate, p: Params) -> Vec<Alert> {
    let n = 300;
    let profile = Profile {
        dist_m: (0..=n).map(|i| f64::from(i) * 50.0).collect(),
        out_s: (0..=n).map(|i| f64::from(i) * 60.0).collect(),
        back_s: (0..=n).map(|i| f64::from(i) * 60.0).collect(),
    };
    let start = date.and_hms_opt(10, 0, 0).expect("10 h");
    let trace = synthetic_trace(&profile, 1.0, 300.0);
    replay(start, &profile, point, &trace, p)
}

/// A trace along the route at the model speed divided by `slowdown` (1: on time),
/// one position every `dt_s` seconds: (time since start, distance).
pub fn synthetic_trace(profile: &Profile, slowdown: f64, dt_s: f64) -> Vec<(f64, f64)> {
    let mut out = Vec::new();
    let total = profile.total_s() * slowdown;
    let mut t = 0.0;
    while t <= total {
        let model = t / slowdown;
        let i = profile
            .out_s
            .partition_point(|x| *x <= model)
            .clamp(1, profile.out_s.len() - 1);
        let (t0, t1) = (profile.out_s[i - 1], profile.out_s[i]);
        let (d0, d1) = (profile.dist_m[i - 1], profile.dist_m[i]);
        let f = if t1 > t0 {
            ((model - t0) / (t1 - t0)).clamp(0.0, 1.0)
        } else {
            0.0
        };
        out.push((t, d0 + (d1 - d0) * f));
        t += dt_s;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::NaiveDate;
    use skimap_common::geom::LonLat;

    fn point(f: impl Fn(u32) -> (f64, f64, bool, f64)) -> WeatherPoint {
        let d0 = NaiveDate::from_ymd_opt(2027, 1, 12)
            .unwrap()
            .and_hms_opt(0, 0, 0)
            .unwrap();
        WeatherPoint {
            id: "p".into(),
            name: "P".into(),
            position: LonLat::new(6.7269, 44.8552),
            elevation_m: 1750.0,
            hours: (0..24)
                .map(|i| {
                    let (t, gust, fog, snow) = f(i);
                    WeatherHour {
                        time: d0 + Duration::hours(i64::from(i)),
                        temperature_c: Some(t),
                        apparent_temperature_c: None,
                        wind_chill_c: Some(t - 3.0),
                        wind_kmh: Some(gust / 2.0),
                        gust_kmh: Some(gust),
                        visibility_m: None,
                        fog: Some(fog),
                        freezing_level_m: None,
                        snowfall_cm: Some(snow),
                        precipitation_mm: Some(snow),
                        cloud_cover_pct: Some(50.0),
                        relative_humidity_pct: None,
                        weather_code: None,
                    }
                })
                .collect(),
        }
    }

    /// A flat route: 20 km in 4 h model time.
    fn profile() -> Profile {
        let n = 200;
        Profile {
            dist_m: (0..=n).map(|i| i as f64 * 100.0).collect(),
            out_s: (0..=n).map(|i| i as f64 * 72.0).collect(),
            back_s: (0..=n).map(|i| i as f64 * 72.0).collect(),
        }
    }

    #[test]
    fn weather_changes_fire_once() {
        // Wind from 13 h, fog from 14 h, snow and cold from 15 h (before the return at 15:48).
        let pt = point(|h| {
            (
                if h >= 15 { -10.0 } else { -2.0 },
                if h >= 13 { 55.0 } else { 15.0 },
                h >= 14,
                if h >= 15 { 1.0 } else { 0.0 },
            )
        });
        let start = NaiveDate::from_ymd_opt(2027, 1, 12)
            .unwrap()
            .and_hms_opt(11, 0, 0)
            .unwrap();
        let pr = profile();
        let trace = synthetic_trace(&pr, 1.0, 300.0);
        let alerts = replay(start, &pr, &pt, &trace, Params::default());
        let kinds: Vec<Kind> = alerts.iter().map(|a| a.kind).collect();
        for k in [
            Kind::WindRising,
            Kind::Fog,
            Kind::Snow,
            Kind::TemperatureDrop,
        ] {
            assert_eq!(
                kinds.iter().filter(|x| **x == k).count(),
                1,
                "{k:?}: {alerts:#?}"
            );
        }
        let wind = alerts.iter().find(|a| a.kind == Kind::WindRising).unwrap();
        assert!(wind.at.hour() == 12, "{}", wind.at);
        assert_eq!(wind.spoken, "Le vent se lève.");
        // On time, starting at 11 h, the 4 h 48 planned loop ends after the deadline
        // (≈ 16:40): a turnaround is planned, but no « advanced » alert at the planned pace.
        assert!(!kinds.contains(&Kind::TurnaroundAdvanced));
    }

    #[test]
    fn delay_advances_the_turnaround() {
        let pt = point(|_| (-5.0, 15.0, false, 0.0));
        // Start 09:00, 4 h model loop: back at 13:48 at the planned pace, deadline ≈ 16:40.
        let start = NaiveDate::from_ymd_opt(2027, 1, 12)
            .unwrap()
            .and_hms_opt(9, 0, 0)
            .unwrap();
        let pr = profile();
        let m = Monitor::new(start, &pr, &pt, Params::default());
        assert!(m.planned_turnaround().is_none());
        // Twice slower: the loop no longer ends in time, the turnaround is said, once
        // or a few times as it moves, never at the planned pace.
        let slow = synthetic_trace(&pr, 2.0, 300.0);
        let alerts = replay(start, &pr, &pt, &slow, Params::default());
        let adv: Vec<&Alert> = alerts
            .iter()
            .filter(|a| a.kind == Kind::TurnaroundAdvanced)
            .collect();
        assert!(!adv.is_empty(), "{alerts:#?}");
        assert!(
            adv[0]
                .spoken
                .starts_with("Vous êtes en retard sur l'horaire prévu.")
        );
        let on_time = replay(
            start,
            &pr,
            &pt,
            &synthetic_trace(&pr, 1.0, 300.0),
            Params::default(),
        );
        assert!(on_time.is_empty(), "{on_time:#?}");
        // Three times slower: at some point turning now is the only way back in time.
        let very = replay(
            start,
            &pr,
            &pt,
            &synthetic_trace(&pr, 3.0, 300.0),
            Params::default(),
        );
        assert!(
            very.iter().any(|a| a.kind == Kind::TurnaroundNow),
            "{very:#?}"
        );
    }
}
