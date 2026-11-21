//! Turnaround time before nightfall (C7, #39): ephemerides (sunset, twilight) plus
//! estimated remaining time.
//!
//! - The light limit is sunset or civil dusk ([`crate::ephemeris`], within about a
//!   minute of astral / NOAA) at the skier's position, minus a safety margin: the
//!   deadline to be back.
//! - The remaining time is the C5 estimate ([`crate::eta::EtaModel`]) of the way back,
//!   times a factor for stops, fatigue and cold (the model is not calibrated yet, C6).
//! - [`status`]: from where the skier is now, latest time to turn around and margin.
//! - [`out_and_back`]: on a planned route travelled out and back, the furthest point
//!   where the skier can still turn around, and when.
//!
//! The ephemerides assume a sea-level horizon: in a valley the sun goes behind the ridges
//! earlier, and under forest the light fades before civil dusk; the margin covers that.

use jiff::{SignedDuration, Timestamp, civil::Date};
use serde::{Deserialize, Serialize};
use skimap_common::geom::LonLat;

use crate::ephemeris::{civil_twilight, sun_times};
use crate::eta::EtaModel;
use crate::graph::PisteGraph;
use crate::routing::Route;

/// Light limit.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LightLimit {
    /// Sunset (sun's upper limb on a sea-level horizon).
    Sunset,
    /// End of civil twilight (sun's centre 6° below the horizon).
    CivilDusk,
}

/// User settings.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct TurnaroundParams {
    /// Light limit.
    pub limit: LightLimit,
    /// Safety margin before the light limit, minutes.
    pub margin_min: f64,
    /// Factor on the estimated time back (stops, fatigue, cold).
    pub eta_factor: f64,
}

impl Default for TurnaroundParams {
    /// Back 30 min before civil dusk (about sunset in winter) with 20 % on the estimate:
    /// assumptions, user settings.
    fn default() -> Self {
        Self {
            limit: LightLimit::CivilDusk,
            margin_min: 30.0,
            eta_factor: 1.2,
        }
    }
}

/// Deadline to be back on a date at a place.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct Deadline {
    /// Sunset (UTC).
    pub sunset: Timestamp,
    /// Civil dusk (UTC).
    pub civil_dusk: Timestamp,
    /// Light limit minus the margin (UTC).
    pub deadline: Timestamp,
}

/// Deadline for a date and place; `None` without sunset (polar day or night).
pub fn deadline(date: Date, place: LonLat, p: &TurnaroundParams) -> Option<Deadline> {
    let (_, sunset) = sun_times(date, place)?;
    let (_, civil_dusk) = civil_twilight(date, place)?;
    let limit = match p.limit {
        LightLimit::Sunset => sunset,
        LightLimit::CivilDusk => civil_dusk,
    };
    Some(Deadline {
        sunset,
        civil_dusk,
        deadline: limit - secs(p.margin_min * 60.0),
    })
}

fn secs(s: f64) -> SignedDuration {
    SignedDuration::from_millis((s * 1000.0).round() as i64)
}

/// Urgency of the turnaround.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Urgency {
    /// More than 30 min of margin.
    Ok,
    /// Less than 30 min of margin: turn around soon.
    Soon,
    /// No margin left: turn around now.
    Now,
}

/// Turnaround status from the current position.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct Status {
    /// Estimated time back, seconds, factor included.
    pub time_back_s: f64,
    /// Estimated arrival if the skier turns around now (UTC).
    pub arrival: Timestamp,
    /// Latest time to turn around (UTC): deadline minus the time back.
    pub latest_turnaround: Timestamp,
    /// Deadline minus arrival, seconds (negative: late).
    pub margin_s: f64,
    /// Urgency.
    pub urgency: Urgency,
}

/// Turnaround status at `now` given the model estimate of the way back
/// (`model_time_back_s`, without the factor).
pub fn status(
    now: Timestamp,
    model_time_back_s: f64,
    deadline: &Deadline,
    p: &TurnaroundParams,
) -> Status {
    let back = model_time_back_s * p.eta_factor;
    let arrival = now + secs(back);
    let margin_s = (deadline.deadline - arrival).as_millis() as i64 as f64 / 1000.0;
    Status {
        time_back_s: back,
        arrival,
        latest_turnaround: deadline.deadline - secs(back),
        margin_s,
        urgency: if margin_s <= 0.0 {
            Urgency::Now
        } else if margin_s < 1800.0 {
            Urgency::Soon
        } else {
            Urgency::Ok
        },
    }
}

/// Latest turnaround point on a route travelled out and back.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct OutAndBack {
    /// Furthest distance along the route where the skier can turn around, metres
    /// (the route length when the whole route fits).
    pub turn_at_m: f64,
    /// Position of that point.
    pub turn_pos: LonLat,
    /// Time the skier reaches it (UTC), factor included.
    pub turn_time: Timestamp,
    /// Arrival back at the start (UTC), factor included.
    pub arrival: Timestamp,
    /// Whether the whole route can be done out and back before the deadline.
    pub whole_route: bool,
    /// Arrival if the whole route is done out and back (UTC).
    pub whole_arrival: Timestamp,
}

/// Furthest turnaround point on `route` (out along it, back along the same trails in
/// reverse) for a start at `start`; `None` if even turning around at once misses the
/// deadline.
pub fn out_and_back(
    g: &PisteGraph,
    route: &Route,
    model: &EtaModel,
    start: Timestamp,
    deadline: &Deadline,
    p: &TurnaroundParams,
) -> Option<OutAndBack> {
    let pieces = route.pieces(g);
    // Cumulative distance, time out, time back from each station to the start.
    let mut dist = vec![0.0];
    let mut out = vec![0.0];
    let mut back = vec![0.0];
    for pc in &pieces {
        dist.push(dist.last().expect("seeded") + pc.dist_m);
        out.push(
            out.last().expect("seeded")
                + model.piece_time_s(pc.attrs, pc.dist_m, pc.dz_m, pc.elev_m),
        );
        back.push(
            back.last().expect("seeded")
                + model.piece_time_s(pc.attrs, pc.dist_m, -pc.dz_m, pc.elev_m),
        );
    }
    let budget = (deadline.deadline - start).as_millis() as i64 as f64 / 1000.0 / p.eta_factor;
    let total = |i: usize| out[i] + back[i];
    if budget < 0.0 {
        return None;
    }
    let n = dist.len() - 1;
    let whole_arrival = start + secs(total(n) * p.eta_factor);
    // Round-trip time is increasing along the route: last station within the budget,
    // then linear interpolation inside the next piece.
    let k = (0..=n)
        .take_while(|&i| total(i) <= budget)
        .last()
        .unwrap_or(0);
    let (turn_at, t_out, t_back) = if k == n {
        (dist[n], out[n], back[n])
    } else {
        let f = ((budget - total(k)) / (total(k + 1) - total(k))).clamp(0.0, 1.0);
        let lerp = |v: &[f64]| v[k] + (v[k + 1] - v[k]) * f;
        (lerp(&dist), lerp(&out), lerp(&back))
    };
    Some(OutAndBack {
        turn_at_m: turn_at,
        turn_pos: point_along(g, route, turn_at),
        turn_time: start + secs(t_out * p.eta_factor),
        arrival: start + secs((t_out + t_back) * p.eta_factor),
        whole_route: k == n,
        whole_arrival,
    })
}

fn point_along(g: &PisteGraph, r: &Route, d: f64) -> LonLat {
    let mut base = 0.0;
    for l in &r.legs {
        let len = l.length_m();
        if d <= base + len {
            let t = d - base;
            let on = if l.to_m >= l.from_m {
                l.from_m + t
            } else {
                l.from_m - t
            };
            return g.point_at(l.edge, on);
        }
        base += len;
    }
    r.end.pos
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graph::tests::{toy, toy_plane};
    use crate::graph::{PisteGraph, Profile};
    use crate::routing::{Router, Style};
    use jiff::civil::date;

    fn utc(s: &str) -> Timestamp {
        s.parse::<Timestamp>().unwrap()
    }

    #[test]
    fn deadline_matches_astral_at_both_sites() {
        // astral 3.2 (see `ephemeris` tests): civil dusk and sunset, UTC.
        let cases = [
            (
                LonLat::new(6.1118, 46.5952),
                (2027, 1, 15),
                "2027-01-15T16:14:05Z",
                "2027-01-15T16:48:42Z",
            ),
            (
                LonLat::new(6.7232, 44.8667),
                (2026, 12, 21),
                "2026-12-21T15:54:21Z",
                "2026-12-21T16:28:47Z",
            ),
        ];
        for (place, (y, m, d), sunset, dusk) in cases {
            let date = date(y, m, d);
            let p = TurnaroundParams::default();
            let dl = deadline(date, place, &p).unwrap();
            let want = utc(dusk) - SignedDuration::from_mins(30);
            let diff = (dl.deadline - want).as_secs().abs();
            assert!(diff <= 90, "{date}: {} vs {want}", dl.deadline);
            assert!((dl.sunset - utc(sunset)).as_secs().abs() <= 90);
            let sunset_limit = TurnaroundParams {
                limit: LightLimit::Sunset,
                margin_min: 0.0,
                ..p
            };
            let dl2 = deadline(date, place, &sunset_limit).unwrap();
            assert_eq!(dl2.deadline, dl.sunset);
        }
    }

    #[test]
    fn status_and_urgency() {
        let date = date(2027, 1, 15);
        let p = TurnaroundParams::default();
        let dl = deadline(date, LonLat::new(6.1118, 46.5952), &p).unwrap();
        // One hour back by the model, 1.2 × = 72 min.
        let now = dl.deadline - SignedDuration::from_mins(120);
        let s = status(now, 3600.0, &dl, &p);
        assert_eq!(s.time_back_s, 4320.0);
        assert_eq!(
            s.latest_turnaround,
            dl.deadline - SignedDuration::from_mins(72)
        );
        assert!((s.margin_s - 48.0 * 60.0).abs() < 1e-6);
        assert_eq!(s.urgency, Urgency::Ok);
        assert_eq!(
            status(dl.deadline - SignedDuration::from_mins(90), 3600.0, &dl, &p).urgency,
            Urgency::Soon
        );
        let late = status(dl.deadline - SignedDuration::from_mins(60), 3600.0, &dl, &p);
        assert_eq!(late.urgency, Urgency::Now);
        assert!((late.margin_s + 12.0 * 60.0).abs() < 1e-6);
    }

    fn ramp_graph() -> PisteGraph {
        // Toy graph with e0 climbing 40 m over 1000 m (4 %).
        let mut data = toy().data().clone();
        data.edges[0].profile = Some(Profile {
            dist_m: (0..=100).map(|k| k as f32 * 10.0).collect(),
            elev_m: (0..=100).map(|k| 1000.0 + 0.4 * k as f32).collect(),
            filled: 0,
        });
        PisteGraph::new(data).unwrap()
    }

    #[test]
    fn out_and_back_turns_where_the_round_trip_fits() {
        let g = ramp_graph();
        let model = EtaModel::new(Style::Classic);
        let r = Router::new(&g, model)
            .route(
                toy_plane().lonlat([0.0, 0.0]),
                toy_plane().lonlat([1000.0, -500.0]),
                5.0,
            )
            .unwrap();
        let pieces = r.pieces(&g);
        let t_out: f64 = model.time_s(&pieces).total_s;
        let back_pieces: Vec<_> = pieces
            .iter()
            .map(|p| crate::routing::Piece {
                dz_m: -p.dz_m,
                ..*p
            })
            .collect();
        let t_back: f64 = model.time_s(&back_pieces).total_s;
        let p = TurnaroundParams {
            eta_factor: 1.0,
            ..TurnaroundParams::default()
        };
        let date = date(2027, 1, 15);
        let dl = deadline(date, LonLat::new(6.1, 46.6), &p).unwrap();
        // Enough time: whole route.
        let start = dl.deadline - secs(t_out + t_back + 600.0);
        let ob = out_and_back(&g, &r, &model, start, &dl, &p).unwrap();
        assert!(ob.whole_route);
        assert!((ob.turn_at_m - r.length_m()).abs() < 1e-6);
        assert!(
            (ob.arrival - (dl.deadline - SignedDuration::from_secs(600)))
                .as_secs()
                .abs()
                <= 1
        );
        // Half the round trip: turn before the end, arrive exactly at the deadline.
        let start = dl.deadline - secs(0.5 * (t_out + t_back));
        let ob = out_and_back(&g, &r, &model, start, &dl, &p).unwrap();
        assert!(!ob.whole_route);
        assert!(ob.turn_at_m > 0.0 && ob.turn_at_m < r.length_m());
        assert!(
            (ob.arrival - dl.deadline).as_millis().abs() <= 5,
            "{:?}",
            ob.arrival
        );
        // The reported position is the point at that distance.
        assert!(ob.turn_pos.haversine_m(g.point_at(0, ob.turn_at_m)) < 0.5);
        // Too late to start.
        assert!(
            out_and_back(
                &g,
                &r,
                &model,
                dl.deadline + SignedDuration::from_mins(1),
                &dl,
                &p
            )
            .is_none()
        );
        // Larger factor: turns earlier.
        let cautious = TurnaroundParams {
            eta_factor: 1.5,
            ..p
        };
        let ob2 = out_and_back(&g, &r, &model, start, &dl, &cautious).unwrap();
        assert!(ob2.turn_at_m < ob.turn_at_m);
    }
}
