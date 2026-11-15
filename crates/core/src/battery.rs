//! Battery forecast model (C9, #41): consumption per mode (GPS, screen, voice) and cold
//! effect, from the T2 measurements.
//!
//! Until T2 (#44) measures the Pixel 8 by cold weather, the currents are those of report
//! 09 (`research/reports/09-batterie-android.md`, §5): the AOSP power profile of the
//! Pixel 8 (1 %/h = 45.8 mA; 8 Pro: 50.5 mA), deep sleep 10 mA, processor awake
//! +28.6 mA, GNSS +5 to +28 mA, screen +98 mA at minimum brightness, audio +75 mA while
//! playing. The mode currents below are the middle of the ranges of that table; they are
//! parameters ([`Currents`]), to be replaced by the T2 figures.
//!
//! Cold: the capacity that can be drawn falls with the battery temperature. Report 09
//! gives 70 % at −10 °C and 60 % at −20 °C for LFP cells (order of magnitude only: phones
//! use other chemistries); the curve is linear between 25 °C (100 %), 0 °C (85 %),
//! −10 °C (70 %) and −20 °C (60 %). The temperature is the battery's
//! (`BatteryManager.EXTRA_TEMPERATURE`), warmer than the air in an inside pocket.
//!
//! Emergency reserve: what a 112 call and the wait for the rescue need — 20 min of call
//! (radio and screen, [`Currents::call_ma`]) and 3 h of waiting with the screen off
//! ([`Currents::pause_ma`]) — never less than 10 % (the percentage misleads in the cold).
//!
//! When the outing has started, the drain measured by the phone (percentage over time)
//! is blended in: its weight grows with the time measured (full after 2 h).

/// Currents per mode, mA.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Currents {
    /// Phone in the pocket on the route: batched fixes, processor asleep between batches.
    pub pocket_ma: f64,
    /// Approaching a junction or off the route: a fix every second.
    pub precise_ma: f64,
    /// Screen on (interaction): screen at low brightness, fixes every second.
    pub screen_ma: f64,
    /// Stopped: GNSS off, significant motion armed.
    pub pause_ma: f64,
    /// Audio while an announcement plays.
    pub audio_ma: f64,
    /// Searching for a network where there is none (not published, report 09: to be
    /// measured), added per hour without network unless in airplane mode.
    pub no_network_ma: f64,
    /// An emergency call.
    pub call_ma: f64,
}

impl Default for Currents {
    fn default() -> Self {
        Self {
            pocket_ma: 31.0,
            precise_ma: 68.0,
            screen_ma: 230.0,
            pause_ma: 11.0,
            audio_ma: 75.0,
            no_network_ma: 25.0,
            call_ma: 400.0,
        }
    }
}

/// A phone.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Device {
    /// Rated capacity, mAh.
    pub capacity_mah: f64,
    /// Currents.
    pub currents: Currents,
}

impl Device {
    /// Pixel 8: 4 575 mAh (1 %/h = 45.8 mA, report 09).
    pub fn pixel8() -> Self {
        Self {
            capacity_mah: 4580.0,
            currents: Currents::default(),
        }
    }

    /// A phone of the given capacity with the Pixel 8 currents.
    pub fn with_capacity(capacity_mah: f64) -> Self {
        Self {
            capacity_mah: if capacity_mah > 500.0 {
                capacity_mah
            } else {
                4580.0
            },
            currents: Currents::default(),
        }
    }
}

/// How the outing uses the phone, per hour.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Usage {
    /// Share of the time stopped (pause, refuge), 0–1.
    pub paused_share: f64,
    /// Minutes of precise GPS per hour (junctions approached, deviations).
    pub precise_min_per_h: f64,
    /// Minutes of screen per hour.
    pub screen_min_per_h: f64,
    /// Seconds of announcements per hour.
    pub voice_s_per_h: f64,
    /// Share of the time without any network (ARCEP), 0–1.
    pub no_network_share: f64,
    /// Airplane mode (no network search).
    pub airplane: bool,
}

impl Default for Usage {
    /// Report 09 §5: 5 h outing, 1 h of pause, 10 junctions per hour (about 1 min of
    /// precise GPS each), 3 min of screen per hour, 20 announcements of 3 s per hour.
    fn default() -> Self {
        Self {
            paused_share: 0.2,
            precise_min_per_h: 10.0,
            screen_min_per_h: 3.0,
            voice_s_per_h: 60.0,
            no_network_share: 0.0,
            airplane: false,
        }
    }
}

/// Mean current of an outing, mA.
pub fn mean_current_ma(c: &Currents, u: &Usage) -> f64 {
    let paused = u.paused_share.clamp(0.0, 1.0);
    let moving = 1.0 - paused;
    let screen = (u.screen_min_per_h / 60.0).clamp(0.0, 1.0);
    let precise = (u.precise_min_per_h / 60.0).clamp(0.0, 1.0 - screen) * moving;
    let pocket = (moving - precise - screen * moving).max(0.0);
    let mut ma = pocket * c.pocket_ma
        + precise * c.precise_ma
        + screen * c.screen_ma
        + paused * (1.0 - screen) * c.pause_ma
        + (u.voice_s_per_h / 3600.0).clamp(0.0, 1.0) * c.audio_ma;
    if !u.airplane {
        ma += u.no_network_share.clamp(0.0, 1.0) * c.no_network_ma;
    }
    ma
}

/// Share of the rated capacity that can be drawn at a battery temperature (°C).
pub fn cold_factor(temp_c: f64) -> f64 {
    const KNOTS: [(f64, f64); 4] = [(-20.0, 0.60), (-10.0, 0.70), (0.0, 0.85), (25.0, 1.0)];
    if temp_c <= KNOTS[0].0 {
        return KNOTS[0].1;
    }
    for w in KNOTS.windows(2) {
        let ((t0, f0), (t1, f1)) = (w[0], w[1]);
        if temp_c <= t1 {
            return f0 + (f1 - f0) * (temp_c - t0) / (t1 - t0);
        }
    }
    1.0
}

/// Minutes of the emergency call kept in reserve.
pub const CALL_MIN: f64 = 20.0;
/// Hours of waiting for the rescue kept in reserve.
pub const WAIT_H: f64 = 3.0;
/// Reserve never below this share of the battery.
pub const MIN_RESERVE: f64 = 0.10;

/// The emergency reserve, share of the rated capacity (at the temperature: the cold
/// shrinks what can be drawn, so the same mAh are a larger share).
pub fn reserve_share(d: &Device, temp_c: f64) -> f64 {
    let mah = d.currents.call_ma * CALL_MIN / 60.0 + d.currents.pause_ma * WAIT_H;
    (mah / (d.capacity_mah * cold_factor(temp_c))).max(MIN_RESERVE)
}

/// Drain measured by the phone since the start of the outing.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Measured {
    /// Hours measured.
    pub hours: f64,
    /// Percentage points lost over them.
    pub pct_lost: f64,
}

/// The forecast.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Forecast {
    /// Mean drain, percentage points per hour (of the rated capacity).
    pub pct_per_h: f64,
    /// Emergency reserve, percentage points.
    pub reserve_pct: f64,
    /// Hours until the reserve is reached.
    pub hours_to_reserve: f64,
    /// The planned outing fits (with the reserve kept).
    pub enough: bool,
    /// Battery at the planned end, percentage (displayed, not corrected for the cold).
    pub pct_at_end: f64,
}

/// Forecast for an outing of `planned_h` hours from `level_pct` at `temp_c`.
pub fn forecast(
    d: &Device,
    u: &Usage,
    level_pct: f64,
    temp_c: f64,
    planned_h: f64,
    measured: Option<Measured>,
) -> Forecast {
    // Percentage points per hour of the model, larger in the cold (less to draw from).
    let model = mean_current_ma(&d.currents, u) / d.capacity_mah * 100.0 / cold_factor(temp_c);
    let pct_per_h = match measured {
        Some(m) if m.hours >= 0.25 && m.pct_lost >= 0.0 => {
            let w = (m.hours / 2.0).clamp(0.0, 1.0);
            (1.0 - w) * model + w * (m.pct_lost / m.hours)
        }
        _ => model,
    }
    .max(0.05);
    let reserve_pct = reserve_share(d, temp_c) * 100.0;
    let usable = (level_pct - reserve_pct).max(0.0);
    let hours_to_reserve = usable / pct_per_h;
    Forecast {
        pct_per_h,
        reserve_pct,
        hours_to_reserve,
        enough: hours_to_reserve >= planned_h,
        pct_at_end: (level_pct - pct_per_h * planned_h).max(0.0),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn report_09_outing_costs_a_few_percent() {
        // Report 09 §5: a 5 h outing costs about 4 to 9 % of the battery with the modes.
        let d = Device::pixel8();
        let f = forecast(&d, &Usage::default(), 100.0, 20.0, 5.0, None);
        let used = 100.0 - f.pct_at_end;
        assert!((4.0..=9.5).contains(&used), "{used}");
        assert!(f.enough);
        assert!(f.reserve_pct >= 10.0);
    }

    #[test]
    fn cold_and_screen_shorten_it() {
        let d = Device::pixel8();
        let warm = forecast(&d, &Usage::default(), 60.0, 20.0, 5.0, None);
        let cold = forecast(&d, &Usage::default(), 60.0, -15.0, 5.0, None);
        assert!(cold.hours_to_reserve < warm.hours_to_reserve);
        assert!((cold_factor(-10.0) - 0.70).abs() < 1e-9);
        assert!((cold_factor(-20.0) - 0.60).abs() < 1e-9);
        assert!((cold_factor(40.0) - 1.0).abs() < 1e-9);
        let screen = Usage {
            screen_min_per_h: 60.0,
            ..Usage::default()
        };
        let s = forecast(&d, &screen, 30.0, -10.0, 5.0, None);
        assert!(!s.enough, "{s:?}");
        // OsmAnd's 6 %/h « screen only » order of magnitude.
        assert!(s.pct_per_h > 4.0, "{}", s.pct_per_h);
    }

    #[test]
    fn measured_drain_takes_over() {
        let d = Device::pixel8();
        let m = Measured {
            hours: 2.0,
            pct_lost: 20.0,
        };
        let f = forecast(&d, &Usage::default(), 50.0, 10.0, 5.0, Some(m));
        assert!((f.pct_per_h - 10.0).abs() < 1e-9);
        assert!(!f.enough);
    }

    #[test]
    fn airplane_mode_saves_the_network_search() {
        let c = Currents::default();
        let blank = Usage {
            no_network_share: 1.0,
            ..Usage::default()
        };
        let plane = Usage {
            airplane: true,
            ..blank
        };
        assert!(mean_current_ma(&c, &plane) < mean_current_ma(&c, &blank));
    }
}
