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
//! - **turnaround of a group** (C11, [`Monitor::for_group`]): the same rules on the route
//!   at the group's pace ([`Profile::for_group`]: its moving pace per terrain and its
//!   stops), so the planned turnaround is the group's, and the delay is measured against
//!   the group's planned times, not the model's. Without a group ([`Monitor::new`]),
//!   nothing changes.
//!
//! [`day_replay`] runs the weather rules over a whole day (an outing from 10 h, planned
//! return at 16 h) to replay the archived forecasts.

use std::borrow::Cow;
use std::collections::HashMap;

use jiff::{SignedDuration, civil::DateTime};
use serde::Serialize;
use skimap_core::calibration::{PaceParams, ProfileModel};
use skimap_core::turnaround::{self, TurnaroundParams};
use skimap_voice::Event;
use skimap_voice::bank::Change;
use skimap_voice::quantity::Clock;

use crate::layers::{WeatherHour, WeatherPoint};
use crate::suggest::Profile;
use crate::text::Lang;

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
    pub at: DateTime,
    /// Kind.
    pub kind: Kind,
    /// Text, French.
    pub text: String,
    /// Voice event.
    #[serde(skip)]
    pub event: Event,
    /// Spoken text (phrase bank, in the language of the monitor).
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
    start: DateTime,
    profile: Cow<'a, Profile>,
    point: Cow<'a, WeatherPoint>,
    p: Params,
    deadline: Option<DateTime>,
    base_gust: f64,
    last: HashMap<Kind, DateTime>,
    planned_turn: Option<DateTime>,
    said_turn: Option<DateTime>,
    /// Lowest temperature and wind chill already announced.
    said_cold: (f64, f64),
    /// Language of the texts (VO6).
    lang: Lang,
}

fn hour_at(point: &WeatherPoint, t: DateTime) -> Option<&WeatherHour> {
    let h0 = t
        - SignedDuration::from_mins(i64::from(t.minute()))
        - SignedDuration::from_secs(i64::from(t.second()));
    point
        .between(h0, h0 + SignedDuration::from_hours(1))
        .first()
}

impl<'a> Monitor<'a> {
    /// Monitor of a route started at `start`, with the weather of `point`.
    pub fn new(start: DateTime, profile: &'a Profile, point: &'a WeatherPoint, p: Params) -> Self {
        Self::with(start, Cow::Borrowed(profile), Cow::Borrowed(point), p)
    }

    /// Monitor of a route started at `start` by a group (C11): the turnaround at the
    /// group's pace (`group`, prior or learned), on the route's model profile converted by
    /// [`Profile::for_group`]; `ratio`: the `skimap_core::calibration::terrain_ratio` of
    /// the route's time model.
    pub fn for_group(
        start: DateTime,
        profile: &Profile,
        point: &'a WeatherPoint,
        p: Params,
        group: &PaceParams,
        ratio: f64,
    ) -> Self {
        Self::with(
            start,
            Cow::Owned(profile.for_group(group, ratio)),
            Cow::Borrowed(point),
            p,
        )
    }

    /// Monitor of a route started at `start` by a group with what it learned (C11): as
    /// [`Monitor::for_group`], on the time the group is planned on (the slow side of its
    /// range with children, `ProfileModel::plan_quantile`) and with its light margin
    /// (`GroupProfile::turnaround_params`).
    pub fn for_model(
        start: DateTime,
        profile: &Profile,
        point: Cow<'a, WeatherPoint>,
        p: Params,
        model: &ProfileModel,
        ratio: f64,
    ) -> Self {
        Self::with_turnaround(
            start,
            Cow::Owned(profile.for_group_at(&model.params, ratio, model.plan_quantile())),
            point,
            p,
            &model.profile.turnaround_params(),
        )
    }

    /// Monitor of a route started at `start`, with a borrowed or owned profile and point.
    pub fn with(
        start: DateTime,
        profile: Cow<'a, Profile>,
        point: Cow<'a, WeatherPoint>,
        p: Params,
    ) -> Self {
        Self::with_turnaround(start, profile, point, p, &TurnaroundParams::default())
    }

    /// [`Monitor::with`] with the light limit and margin `tp` (C7).
    pub fn with_turnaround(
        start: DateTime,
        profile: Cow<'a, Profile>,
        point: Cow<'a, WeatherPoint>,
        p: Params,
        tp: &TurnaroundParams,
    ) -> Self {
        let deadline = turnaround::deadline(start.date(), point.position, tp)
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
            lang: Lang::Fr,
        };
        m.planned_turn = m.latest_turn(start, 0.0, 1.0);
        m
    }

    /// The same monitor with its texts in `lang` (French by default, VO6).
    #[must_use]
    pub fn in_lang(mut self, lang: Lang) -> Self {
        self.lang = lang;
        self
    }

    /// Latest turnaround at a pace from the current position, `None` when the whole route
    /// can be finished before the deadline (or there is no deadline).
    fn latest_turn(&self, now: DateTime, along_m: f64, pace: f64) -> Option<DateTime> {
        let d = self.deadline?;
        let f = TurnaroundParams::default().eta_factor;
        let pr: &Profile = &self.profile;
        let i0 = pr.index_at(along_m);
        let secs = |s: f64| SignedDuration::from_secs(s.round() as i64);
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
        Some(best.unwrap_or(now - SignedDuration::from_mins(1)))
    }

    fn may(&self, kind: Kind, now: DateTime) -> bool {
        self.last
            .get(&kind)
            .is_none_or(|t| now.duration_since(*t).as_mins() >= self.p.repeat_min)
    }

    /// Alerts at `now`, having travelled `along_m` metres of the route.
    pub fn update(&mut self, now: DateTime, along_m: f64) -> Vec<Alert> {
        let mut out = Vec::new();
        let p = self.p;
        // The conditions known at the departure are the briefing's (A1, A7): weather alerts
        // start 10 minutes after it, and only look at hours before the planned return.
        let cur = (now >= self.start + SignedDuration::from_mins(10))
            .then(|| hour_at(&self.point, now).cloned())
            .flatten();
        let end = self.start
            + SignedDuration::from_secs(
                (self.profile.total_s() * TurnaroundParams::default().eta_factor) as i64,
            );
        let end = end.max(now + SignedDuration::from_mins(30));
        // Aggregates of the hours ahead (computed on the slices, without copying them: the
        // phone calls this once a minute during the outing).
        let (g_max, fog_ahead, s_ahead) = {
            let ahead = self.point.between(
                now,
                (now + SignedDuration::from_mins(p.lookahead_min)).min(end),
            );
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
            let three = self
                .point
                .between(now, (now + SignedDuration::from_hours(3)).min(end));
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
            let spoken = event.announce().in_lang(m.lang).text();
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
                    crate::text::of(self.lang).al_wind_rising(g_max, before),
                    Event::ConditionChange(Change::WindRising),
                );
            }
            if c.fog != Some(true) && fog_ahead && self.may(Kind::Fog, now) {
                push(
                    self,
                    Kind::Fog,
                    crate::text::of(self.lang).al_fog().into(),
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
                    crate::text::of(self.lang).al_snow(s_ahead),
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
                    crate::text::of(self.lang).al_cold(
                        &crate::briefing::degrees(t_min),
                        &crate::briefing::degrees(chill_min),
                    ),
                    Event::ConditionChange(Change::TemperatureDrop),
                );
            }
        }
        // Turnaround.
        let pr: &Profile = &self.profile;
        let i = pr.index_at(along_m);
        let planned = pr.out_s[i];
        let elapsed = now.duration_since(self.start).as_secs() as f64;
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
                            crate::text::of(self.lang).al_turn_now(
                                (late_s / 60.0).round(),
                                pace,
                                self.deadline,
                            ),
                            Event::Turnaround {
                                seconds_left: 0.0,
                                deadline: Clock::floor_quarter(
                                    now.hour() as u8,
                                    now.minute() as u8,
                                ),
                            },
                        );
                    }
                } else {
                    let earlier = match self.planned_turn {
                        Some(t0) => t0.duration_since(t).as_mins() >= p.advance_min,
                        None => true,
                    };
                    let new = self
                        .said_turn
                        .is_none_or(|s| s.duration_since(t).as_mins() >= p.advance_min);
                    if earlier && new {
                        self.said_turn = Some(t);
                        let clock = Clock::floor_quarter(t.hour() as u8, t.minute() as u8);
                        push(
                            self,
                            Kind::TurnaroundAdvanced,
                            crate::text::of(self.lang).al_turn_advanced(
                                (late_s / 60.0).round(),
                                pace,
                                self.deadline,
                                t,
                            ),
                            Event::TurnaroundAdvanced { deadline: clock },
                        );
                    }
                }
            }
        }
        out
    }

    /// Latest turnaround at the planned pace (`None`: the route ends before the deadline).
    pub fn planned_turnaround(&self) -> Option<DateTime> {
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
    pub fn deadline(&self) -> Option<DateTime> {
        self.deadline
    }
}

/// Replays a trace (time, distance along the route) and collects every alert.
pub fn replay(
    start: DateTime,
    profile: &Profile,
    point: &WeatherPoint,
    trace: &[(f64, f64)],
    p: Params,
) -> Vec<Alert> {
    replay_in(start, profile, point, trace, p, Lang::Fr)
}

/// [`replay`] with the texts in `lang` (VO6).
pub fn replay_in(
    start: DateTime,
    profile: &Profile,
    point: &WeatherPoint,
    trace: &[(f64, f64)],
    p: Params,
    lang: Lang,
) -> Vec<Alert> {
    let mut m = Monitor::new(start, profile, point, p).in_lang(lang);
    trace
        .iter()
        .flat_map(|(t_s, along)| m.update(start + SignedDuration::from_secs(*t_s as i64), *along))
        .collect()
}

/// Weather alerts of a day: an outing from 10 h of 5 hours model time (planned return at
/// 16 h with the 1.2 margin), at the planned pace, one update every 5 minutes.
pub fn day_replay(point: &WeatherPoint, date: jiff::civil::Date, p: Params) -> Vec<Alert> {
    day_replay_in(point, date, p, Lang::Fr)
}

/// [`day_replay`] with the texts in `lang` (VO6).
pub fn day_replay_in(
    point: &WeatherPoint,
    date: jiff::civil::Date,
    p: Params,
    lang: Lang,
) -> Vec<Alert> {
    let n = 300;
    let profile = Profile {
        dist_m: (0..=n).map(|i| f64::from(i) * 50.0).collect(),
        out_s: (0..=n).map(|i| f64::from(i) * 60.0).collect(),
        back_s: (0..=n).map(|i| f64::from(i) * 60.0).collect(),
    };
    let start = date.at(10, 0, 0, 0);
    let trace = synthetic_trace(&profile, 1.0, 300.0);
    replay_in(start, &profile, point, &trace, p, lang)
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

    use jiff::civil::date;
    use skimap_common::geom::LonLat;

    fn hhmm(t: DateTime) -> String {
        crate::text::of(Lang::Fr).clock(t)
    }

    fn point(f: impl Fn(u32) -> (f64, f64, bool, f64)) -> WeatherPoint {
        let d0 = date(2027, 1, 12).at(0, 0, 0, 0);
        WeatherPoint {
            id: "p".into(),
            name: "P".into(),
            position: LonLat::new(6.7269, 44.8552),
            elevation_m: 1750.0,
            hours: (0..24)
                .map(|i| {
                    let (t, gust, fog, snow) = f(i);
                    WeatherHour {
                        time: d0 + SignedDuration::from_hours(i64::from(i)),
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
        let start = date(2027, 1, 12).at(11, 0, 0, 0);
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
    fn turnaround_at_the_pace_of_a_group() {
        use skimap_core::calibration::{GroupProfile, Level, PaceParams, terrain_ratio};
        use skimap_core::eta::EtaModel;
        use skimap_core::routing::Style;
        let pt = point(|_| (-5.0, 15.0, false, 0.0));
        let start = date(2027, 1, 12).at(9, 0, 0, 0);
        let pr = profile();
        let ratio = terrain_ratio(&EtaModel::new(Style::Classic));
        // A family with children of 6 and 9: the 4 h model loop takes it about 2.5 times
        // longer with its stops.
        let family = PaceParams::prior(&GroupProfile::family(2, &[6, 9], Style::Classic));
        let fp = pr.for_group(&family, ratio);
        let median = family.predict(pr.model_by_terrain(ratio)).median_s;
        assert!(
            (fp.total_s() - median).abs() < 1e-6,
            "{} {median}",
            fp.total_s()
        );
        assert!(fp.total_s() > 2.0 * pr.total_s(), "{}", fp.total_s());
        assert_eq!(fp.dist_m, pr.dist_m);
        // The model loop ends before the deadline (no turnaround), the family's does not.
        assert!(
            Monitor::new(start, &pr, &pt, Params::default())
                .planned_turnaround()
                .is_none()
        );
        let m = Monitor::for_group(start, &pr, &pt, Params::default(), &family, ratio);
        let t = m.planned_turnaround().expect("a turnaround for the family");
        assert!(
            t > start + SignedDuration::from_hours(2) && t < start + SignedDuration::from_hours(6),
            "{t}"
        );
        // The family at its own pace: on time for itself, nothing said; judged against the
        // model, it would be told to turn around.
        let trace = synthetic_trace(&fp, 1.0, 300.0);
        let mut mg = Monitor::for_group(start, &pr, &pt, Params::default(), &family, ratio);
        let said: Vec<Alert> = trace
            .iter()
            .take_while(|(t_s, _)| start + SignedDuration::from_secs(*t_s as i64) < t)
            .flat_map(|(t_s, along)| {
                mg.update(start + SignedDuration::from_secs(*t_s as i64), *along)
            })
            .collect();
        assert!(said.is_empty(), "{said:#?}");
        let mut mm = Monitor::new(start, &pr, &pt, Params::default());
        let model: Vec<Alert> = trace
            .iter()
            .take_while(|(t_s, _)| start + SignedDuration::from_secs(*t_s as i64) < t)
            .flat_map(|(t_s, along)| {
                mm.update(start + SignedDuration::from_secs(*t_s as i64), *along)
            })
            .collect();
        assert!(
            model.iter().any(|a| a.kind == Kind::TurnaroundAdvanced),
            "{model:#?}"
        );
        // Slower than its own pace: the family's turnaround is advanced.
        let slow = synthetic_trace(&fp, 1.5, 300.0);
        let mut ms = Monitor::for_group(start, &pr, &pt, Params::default(), &family, ratio);
        let late: Vec<Alert> = slow
            .iter()
            .flat_map(|(t_s, along)| {
                ms.update(start + SignedDuration::from_secs(*t_s as i64), *along)
            })
            .collect();
        assert!(
            late.iter()
                .any(|a| matches!(a.kind, Kind::TurnaroundAdvanced | Kind::TurnaroundNow)),
            "{late:#?}"
        );
        // A lone adult of intermediate level: about the model, the same decisions as
        // without a group on an on-time trace.
        let adult = PaceParams::prior(&GroupProfile::solo(Style::Classic, Level::Intermediate));
        let ap = pr.for_group(&adult, ratio);
        assert!(
            (ap.total_s() / pr.total_s() - 1.0).abs() < 0.15,
            "{}",
            ap.total_s()
        );
    }

    #[test]
    fn delay_advances_the_turnaround() {
        let pt = point(|_| (-5.0, 15.0, false, 0.0));
        // Start 09:00, 4 h model loop: back at 13:48 at the planned pace, deadline ≈ 16:40.
        let start = date(2027, 1, 12).at(9, 0, 0, 0);
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
                .starts_with("Vous êtes en retard sur l'heure prévue. Le demi-tour est avancé.")
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

    fn day(h: u32, m: u32, s: u32) -> DateTime {
        date(2027, 1, 12).to_datetime(skimap_common::dates::hms(h, m, s).unwrap())
    }

    /// The latest turnaround on the flat route of [`profile`] (72 s per 100 m both ways),
    /// from `i0` at a pace: the last station `i` from which going on to it and back the
    /// same way (× 1.2) ends by the deadline.
    fn expected_turn(now: DateTime, d: DateTime, i0: usize, pace: f64) -> DateTime {
        let mut best = None;
        for i in i0..=200 {
            let ahead = (pace * 72.0 * (i - i0) as f64).round() as i64;
            let total =
                (pace * 72.0 * (i - i0) as f64 + pace * 1.2 * 72.0 * i as f64).round() as i64;
            if now + SignedDuration::from_secs(total) <= d {
                best = Some(now + SignedDuration::from_secs(ahead));
            } else {
                break;
            }
        }
        best.unwrap_or(now - SignedDuration::from_mins(1))
    }

    /// The C7 deadline of the test day at the test point.
    fn deadline_of_the_day() -> DateTime {
        let pt = point(|_| (-5.0, 15.0, false, 0.0));
        skimap_core::ephemeris::local_time(
            turnaround::deadline(
                day(0, 0, 0).date(),
                pt.position,
                &TurnaroundParams::default(),
            )
            .unwrap()
            .deadline,
        )
    }

    #[test]
    fn planned_turnaround_and_deadline_exactly() {
        let pt = point(|_| (-5.0, 15.0, false, 0.0));
        let pr = profile();
        let dl = deadline_of_the_day();
        // Leaving 4 h before the deadline, the 4 h loop (× 1.2) ends after it: the
        // turnaround is the last station from which the way back ends in time.
        let start = dl - SignedDuration::from_hours(4);
        let m = Monitor::new(start, &pr, &pt, Params::default());
        assert_eq!(m.deadline(), Some(dl));
        assert_eq!(m.profile().total_s(), pr.total_s());
        let t = m.planned_turnaround().unwrap();
        assert_eq!(t, expected_turn(start, dl, 0, 1.0));
        assert!(t > start + SignedDuration::from_hours(1) && t < dl, "{t}");
        // 5 h before: it ends in time, no turnaround.
        let m = Monitor::new(
            dl - SignedDuration::from_hours(5),
            &pr,
            &pt,
            Params::default(),
        );
        assert_eq!(m.planned_turnaround(), None);
        // A shorter route from the same start: recomputed.
        let mut m = Monitor::new(start, &pr, &pt, Params::default());
        let short = Profile {
            dist_m: pr.dist_m[..=50].to_vec(),
            out_s: pr.out_s[..=50].to_vec(),
            back_s: pr.back_s[..=50].to_vec(),
        };
        m.replace_route(Cow::Owned(short.clone()));
        assert_eq!(m.planned_turnaround(), None);
        assert_eq!(m.profile(), &short);
    }

    /// A flat route of 20 km, 60 s per 100 m both ways (3 h 20).
    fn profile60() -> Profile {
        let n = 200;
        Profile {
            dist_m: (0..=n).map(|i| i as f64 * 100.0).collect(),
            out_s: (0..=n).map(|i| i as f64 * 60.0).collect(),
            back_s: (0..=n).map(|i| i as f64 * 60.0).collect(),
        }
    }

    /// The alerts of one update `late_s` seconds late at a station of [`profile60`].
    fn turnaround_alerts(start: DateTime, late_s: i64, station: usize) -> Vec<Alert> {
        let pt = point(|_| (-5.0, 15.0, false, 0.0));
        let pr = profile60();
        let mut m = Monitor::new(start, &pr, &pt, Params::default());
        let now = start + SignedDuration::from_secs(60 * station as i64 + late_s);
        m.update(now, 100.0 * station as f64)
    }

    /// The latest turnaround on [`profile60`] from a station at a pace.
    fn expected_turn60(now: DateTime, d: DateTime, i0: usize, pace: f64) -> DateTime {
        let mut best = None;
        for i in i0..=200 {
            let ahead = (pace * 60.0 * (i - i0) as f64).round() as i64;
            let total =
                (pace * 60.0 * (i - i0) as f64 + pace * 1.2 * 60.0 * i as f64).round() as i64;
            if now + SignedDuration::from_secs(total) <= d {
                best = Some(now + SignedDuration::from_secs(ahead));
            } else {
                break;
            }
        }
        best.unwrap_or(now - SignedDuration::from_mins(1))
    }

    #[test]
    fn turnaround_thresholds() {
        let pt = point(|_| (-5.0, 15.0, false, 0.0));
        let pr = profile60();
        let dl = deadline_of_the_day();
        // 5 h before the deadline the 3 h 20 route (× 1.2 = 4 h) ends in time.
        let start = dl - SignedDuration::from_hours(5);
        let m = Monitor::new(start, &pr, &pt, Params::default());
        assert_eq!(m.planned_turnaround(), None);
        // 20 min of planned time (station 20), 10 min late: at pace 1.5 the route no
        // longer ends in time; the turnaround is said with its time.
        let a = turnaround_alerts(start, 600, 20);
        assert_eq!(a.len(), 1, "{a:#?}");
        assert_eq!(a[0].kind, Kind::TurnaroundAdvanced);
        let now = start + SignedDuration::from_secs(1800);
        let t = expected_turn60(now, dl, 20, 1.5);
        assert!(t > now);
        assert_eq!(
            a[0].text,
            format!(
                "en retard de 10 min (allure × 1,50) : la boucle ne finit plus avant {} ; demi-tour au plus tard à {}",
                hhmm(dl),
                hhmm(t)
            )
        );
        assert_eq!(
            a[0].event,
            Event::TurnaroundAdvanced {
                deadline: Clock::floor_quarter(t.hour() as u8, t.minute() as u8)
            }
        );
        // A second less late, or a station before 20 min of planned time: nothing.
        assert!(turnaround_alerts(start, 599, 20).is_empty());
        assert!(turnaround_alerts(start, 700, 19).is_empty());
        // Said once: on a route of the same pace both ways the latest turnaround does not
        // move with the delay, so 10 min later and later still nothing new is said.
        let mut m = Monitor::new(start, &pr, &pt, Params::default());
        assert_eq!(m.update(now, 2000.0).len(), 1);
        let later = start + SignedDuration::from_secs(60 * 25 + 900);
        assert!(m.update(later, 2500.0).is_empty());
        // Planned with a turnaround already (3 h 54 before the deadline): late, the same
        // turnaround, within 10 minutes of the planned one: not said.
        let start2 = dl - SignedDuration::from_mins(234);
        let m2 = Monitor::new(start2, &pr, &pt, Params::default());
        let t0 = m2.planned_turnaround().unwrap();
        assert_eq!(t0, expected_turn60(start2, dl, 0, 1.0));
        assert!(turnaround_alerts(start2, 600, 20).is_empty());
        // Late by 10 % of the planned time at least: 2 h 30 of planned time (station 150)
        // 15 min before the deadline: too late to go on; turn around now.
        let start3 = dl - SignedDuration::from_hours(3);
        let a = turnaround_alerts(start3, 900, 150);
        assert_eq!(a.len(), 1, "{a:#?}");
        assert_eq!(a[0].kind, Kind::TurnaroundNow);
        assert_eq!(
            a[0].text,
            format!(
                "en retard de 15 min (allure × 1,10) : faites demi-tour maintenant pour rentrer avant {}",
                hhmm(dl)
            )
        );
        assert!(turnaround_alerts(start3, 899, 150).is_empty());
        // Said once, not again within two hours.
        let mut m = Monitor::new(start3, &pr, &pt, Params::default());
        let now3 = start3 + SignedDuration::from_secs(9900);
        assert_eq!(m.update(now3, 15_000.0).len(), 1);
        assert!(
            m.update(now3 + SignedDuration::from_mins(5), 15_000.0)
                .is_empty()
        );
        // After the deadline itself: nothing more (A8 says it no longer).
        let mut m = Monitor::new(start3, &pr, &pt, Params::default());
        assert!(
            m.update(dl + SignedDuration::from_mins(1), 15_000.0)
                .is_empty()
        );
    }

    #[test]
    fn the_hour_of_an_instant_and_the_weather_ahead() {
        let pt = point(|h| (-5.0, 15.0, h == 11, 0.0));
        assert_eq!(hour_at(&pt, day(10, 30, 45)).unwrap().time, day(10, 0, 0));
        assert_eq!(hour_at(&pt, day(10, 59, 59)).unwrap().time, day(10, 0, 0));
        assert_eq!(hhmm(day(9, 5, 0)), "9 h 05");
        // Fog at 11 h: looked at only within the planned return (route time × 1.2), or
        // the next 30 minutes.
        let route = |minutes: f64| {
            let n = 100;
            Profile {
                dist_m: (0..=n).map(|i| f64::from(i) * 10.0).collect(),
                out_s: (0..=n).map(|i| f64::from(i) * minutes * 0.6).collect(),
                back_s: (0..=n).map(|i| f64::from(i) * minutes * 0.6).collect(),
            }
        };
        let fog_at = |minutes: f64, now: DateTime| {
            let pr = route(minutes);
            let mut m = Monitor::new(day(10, 0, 0), &pr, &pt, Params::default());
            m.update(now, 0.0).iter().any(|a| a.kind == Kind::Fog)
        };
        // 50.5 min × 1.2: back at 11 h 00 min 36 s, the 11 h hour is ahead.
        assert!(fog_at(50.5, day(10, 15, 0)));
        // 49 min × 1.2: back at 10 h 58, it is not.
        assert!(!fog_at(49.0, day(10, 15, 0)));
        // A 10 min route, at 10 h 40: the next 30 minutes reach 11 h 10.
        assert!(fog_at(10.0, day(10, 40, 0)));
        // Weather alerts start 10 minutes after the departure.
        assert!(!fog_at(60.0, day(10, 9, 0)));
        assert!(fog_at(60.0, day(10, 10, 0)));
    }

    #[test]
    fn weather_thresholds() {
        let at10 = day(10, 30, 0);
        let check = |f: &dyn Fn(u32) -> (f64, f64, bool, f64)| {
            let pt = point(f);
            let pr = profile();
            let mut m = Monitor::new(day(10, 0, 0), &pr, &pt, Params::default());
            m.update(at10, 0.0)
                .into_iter()
                .map(|a| a.kind)
                .collect::<Vec<_>>()
        };
        // Gusts reaching 40 km/h, 20 more than at the departure, while the current hour
        // is below 40.
        assert_eq!(
            check(&|h| (-5.0, if h >= 11 { 40.0 } else { 20.0 }, false, 0.0)),
            [Kind::WindRising]
        );
        assert!(check(&|h| (-5.0, if h >= 11 { 39.9 } else { 15.0 }, false, 0.0)).is_empty());
        assert!(
            check(&|h| (-5.0, if h >= 11 { 45.0 } else { 30.0 }, false, 0.0)).is_empty(),
            "only 15 more"
        );
        // Already 40 now: not « rising » (gusts at the departure counted at 10 h).
        assert!(
            check(&|h| (
                -5.0,
                match h {
                    11.. => 70.0,
                    _ => 40.0,
                },
                false,
                0.0
            ))
            .is_empty()
        );
        // Snow ahead while the current hour is dry (under 0.2 cm/h).
        assert_eq!(
            check(&|h| (-5.0, 15.0, false, if h >= 11 { 0.5 } else { 0.1 })),
            [Kind::Snow]
        );
        assert!(check(&|h| (-5.0, 15.0, false, if h >= 11 { 0.5 } else { 0.2 })).is_empty());
        assert!(check(&|h| (-5.0, 15.0, false, if h >= 11 { 0.4 } else { 0.0 })).is_empty());
        // A drop of the air of 5 °C (the wind chill follows, 3 under the air).
        assert_eq!(
            check(&|h| (if h >= 12 { -10.0 } else { -5.0 }, 15.0, false, 0.0)),
            [Kind::TemperatureDrop]
        );
        assert!(check(&|h| (if h >= 12 { -9.9 } else { -5.0 }, 15.0, false, 0.0)).is_empty());
    }

    #[test]
    fn a_drop_of_the_air_alone_or_of_the_wind_chill_alone() {
        // Without wind chill in the layer: the air alone.
        let mut pt = point(|h| (if h >= 12 { -11.0 } else { -5.0 }, 15.0, false, 0.0));
        for h in &mut pt.hours {
            h.wind_chill_c = None;
        }
        let pr = profile();
        let mut m = Monitor::new(day(10, 0, 0), &pr, &pt, Params::default());
        let a = m.update(day(10, 30, 0), 0.0);
        assert_eq!(a.len(), 1);
        assert_eq!(a[0].kind, Kind::TemperatureDrop);
        assert!(
            a[0].text.starts_with("la température baisse : -11 °C"),
            "{}",
            a[0].text
        );
        // The wind chill alone: −6 °C within 3 h, the air steady.
        let mut pt = point(|_| (-5.0, 15.0, false, 0.0));
        for h in &mut pt.hours {
            if h.time.hour() >= 12 {
                h.wind_chill_c = Some(-14.0);
            }
        }
        let mut m = Monitor::new(day(10, 0, 0), &pr, &pt, Params::default());
        let a = m.update(day(10, 30, 0), 0.0);
        assert_eq!(
            a.iter().map(|x| x.kind).collect::<Vec<_>>(),
            [Kind::TemperatureDrop]
        );
        assert_eq!(
            a[0].text,
            "la température baisse : -5 °C, ressenti -14 °C d'ici 3 h"
        );
        // Said once: the same drop again an hour later is not said.
        assert!(m.update(day(11, 30, 0), 0.0).is_empty());
    }

    #[test]
    fn synthetic_traces_follow_the_model_time() {
        let pr = Profile {
            dist_m: vec![0.0, 100.0, 100.0, 200.0],
            out_s: vec![0.0, 60.0, 60.0, 180.0],
            back_s: vec![0.0, 60.0, 60.0, 180.0],
        };
        let t = synthetic_trace(&pr, 2.0, 30.0);
        let expect: Vec<(f64, f64)> = vec![
            (0.0, 0.0),
            (30.0, 25.0),
            (60.0, 50.0),
            (90.0, 75.0),
            (120.0, 100.0),
            (150.0, 112.5),
            (180.0, 125.0),
            (210.0, 137.5),
            (240.0, 150.0),
            (270.0, 162.5),
            (300.0, 175.0),
            (330.0, 187.5),
            (360.0, 200.0),
        ];
        assert_eq!(t, expect);
        // On time: as long as the route.
        let t = synthetic_trace(&pr, 1.0, 60.0);
        assert_eq!(
            t,
            vec![(0.0, 0.0), (60.0, 100.0), (120.0, 150.0), (180.0, 200.0)]
        );
    }
}
