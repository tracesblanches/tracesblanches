//! Texts in English.

use jiff::civil::{Date, DateTime};

use super::Texts;

/// English.
pub struct En;

impl Texts for En {
    fn assist(&self) -> &'static dyn skimap_assist::text::Texts {
        skimap_assist::text::of(skimap_assist::text::Lang::En)
    }

    fn group_learned(&self, outings: u32) -> String {
        match outings {
            0 => "prior, no outing learned".to_owned(),
            1 => "one outing learned".to_owned(),
            n => format!("{n} outings learned"),
        }
    }

    fn group_who(&self, adults: u8, children: &[u8]) -> String {
        let adults = match adults {
            1 => "1 adult".to_owned(),
            n => format!("{n} adults"),
        };
        let ages: Vec<String> = children.iter().map(u8::to_string).collect();
        match ages.split_last() {
            None => adults,
            Some((last, [])) => format!("{adults}, children of {last}"),
            Some((last, rest)) => format!("{adults}, children of {} and {last}", rest.join(", ")),
        }
    }

    fn duration(&self, s: f64) -> String {
        skimap_plan::text::of(skimap_plan::text::Lang::En).duration5(s)
    }

    fn clock5(&self, t: DateTime) -> String {
        skimap_plan::text::of(skimap_plan::text::Lang::En).clock5(t)
    }

    fn clock_day(&self, t: DateTime, day: Date) -> String {
        if t.date() > day {
            format!("{} the next day", self.clock5(t))
        } else {
            self.clock5(t)
        }
    }

    fn duration_range(&self, lo: &str, hi: &str) -> String {
        format!("{lo} to {hi}")
    }

    fn duration_about(&self, median: &str, lo: &str, hi: &str) -> String {
        format!("about {median} ({lo} to {hi})")
    }

    fn back_slow_side(&self, median: &str, plan: &str, back: &str) -> String {
        format!("median {median}, planned on {plan} (slow side), back around {back}")
    }

    fn back_between(&self, lo: &str, hi: &str) -> String {
        format!("back between {lo} and {hi}")
    }

    fn back_around(&self, at: &str) -> String {
        format!("back around {at}")
    }

    fn model_time(&self, d: &str) -> String {
        format!("about {d} (model)")
    }

    fn back_with_margin(&self, at: &str) -> String {
        format!("back around {at} with the margin")
    }

    fn after_limit(&self, limit: DateTime, for_group: bool) -> String {
        format!(
            "Expected back after the limit of {}: a long loop for {}",
            self.assist().clock(limit),
            if for_group { "this group" } else { "this day" }
        )
    }

    fn off_network_access(&self, metres: f64) -> String {
        let m = (metres / 10.0).round() * 10.0;
        format!(
            "Off-network access: {m} m on foot or on skis from the car park (or lodge) to the route, no trail, on top of the loop"
        )
    }

    fn key_kind(&self, k: skimap_plan::plan::KeyKind) -> &'static str {
        use skimap_plan::plan::KeyKind;
        match k {
            KeyKind::Start => "start",
            KeyKind::Turn => "junction",
            KeyKind::Junction => "obvious junction",
            KeyKind::Refuge => "hut",
            KeyKind::NoNetworkStart => "start of a zone without network",
            KeyKind::NoNetworkEnd => "end of a zone without network",
            KeyKind::AvalancheStart => "start of a known avalanche path",
            KeyKind::AvalancheEnd => "end of a known avalanche path",
            KeyKind::Finish => "finish",
        }
    }

    fn sms(
        &self,
        site: &str,
        date: Date,
        start: &str,
        departure: DateTime,
        label: &str,
        km: f64,
        trails: &[String],
        back: DateTime,
        limit: Option<DateTime>,
        alert: DateTime,
    ) -> String {
        let a = self.assist();
        format!(
            "Cross-country ski outing {site} on {}: start {start} at {}, loop {label} ({} km, trails {}). Back around {}{}. No news from me at {}: call 112 and give them this plan.",
            date.strftime("%d/%m"),
            a.clock(departure),
            a.num(km, 1),
            trails.join(", "),
            a.clock(back),
            limit
                .map(|l| format!(", {} at the latest", a.clock(l)))
                .unwrap_or_default(),
            a.clock(alert)
        )
    }

    fn default_group_name(&self) -> &'static str {
        "Family with beginner children"
    }

    fn solo_group_name(&self) -> &'static str {
        "Solo"
    }

    fn when(&self, t: DateTime) -> String {
        format!("{:02}/{:02} at {}", t.day(), t.month(), self.clock5(t))
    }

    fn delta(&self, actual_s: f64, estimated_s: f64) -> String {
        let d = actual_s - estimated_s;
        if d.abs() < 300.0 {
            "as planned".into()
        } else if d > 0.0 {
            format!("{} more than planned", self.duration(d))
        } else {
            format!("{} less than planned", self.duration(-d))
        }
    }

    fn recap_distance(&self, km: f64) -> String {
        format!("Distance: {} km", self.assist().num(km, 1))
    }

    fn recap_time(&self, elapsed_s: f64, moving_s: f64) -> String {
        format!(
            "Time: {}, {} moving",
            self.duration(elapsed_s),
            self.duration(moving_s)
        )
    }

    fn recap_speed(&self, kmh: f64) -> String {
        format!("Moving speed: {} km/h", self.assist().num(kmh, 1))
    }

    fn recap_estimate(&self, group: bool, estimated_s: f64, actual_s: f64) -> String {
        let who = if group {
            "at the pace of the group"
        } else {
            "by the model"
        };
        format!(
            "Estimated time {who}: {}, actual: {} ({})",
            self.duration(estimated_s),
            self.duration(actual_s),
            self.delta(actual_s, estimated_s)
        )
    }

    fn recap_gain_imported(&self, gain_m: f64) -> String {
        format!("Climb: {gain_m:.0} m up (Strava export)")
    }

    fn recap_elevation(
        &self,
        ascent_m: f64,
        descent_m: f64,
        source: crate::recap::AltitudeSource,
    ) -> String {
        use crate::recap::AltitudeSource;
        let src = match source {
            AltitudeSource::BarometerAndRelief => "barometer corrected by the relief",
            AltitudeSource::Barometer => "barometer",
            AltitudeSource::Relief => "relief under the trace",
        };
        format!("Climb: {ascent_m:.0} m up, {descent_m:.0} m down ({src})")
    }

    fn recap_same_route(&self, n: usize, when: &str, last_s: f64, diff_s: f64) -> String {
        let how = if diff_s.abs() < 300.0 {
            "about the same time".to_owned()
        } else if diff_s < 0.0 {
            format!("{} faster", self.duration(-diff_s))
        } else {
            format!("{} slower", self.duration(diff_s))
        };
        let nth = match (n % 10, n % 100) {
            (1, x) if x != 11 => "st",
            (2, x) if x != 12 => "nd",
            (3, x) if x != 13 => "rd",
            _ => "th",
        };
        format!(
            "{n}{nth} time on this route; last time ({when}): {}, today {how}.",
            self.duration(last_s)
        )
    }

    fn recap_speed_vs(&self, kmh: f64, mean_kmh: f64, n: usize) -> String {
        let s = if n > 1 { "s" } else { "" };
        format!(
            "Moving speed {} km/h, against {} km/h on average over {n} previous outing{s} here.",
            self.assist().num(kmh, 1),
            self.assist().num(mean_kmh, 1),
        )
    }

    fn recap_first(&self) -> &'static str {
        "First outing of this kind recorded: nothing to compare yet."
    }

    fn recap_spoken(
        &self,
        distance: &str,
        elapsed_s: f64,
        delta: Option<&str>,
        comparison: Option<&str>,
    ) -> String {
        format!(
            "Recap: {distance} in {}{}.{}",
            self.duration(elapsed_s),
            delta.map(|d| format!(", {d}")).unwrap_or_default(),
            comparison.map(|c| format!(" {c}")).unwrap_or_default()
        )
    }

    fn cond_temperature(&self, lo: f64, hi: f64) -> String {
        format!("Forecast temperature: {lo:.0} to {hi:.0} °C")
    }

    fn cond_gusts(&self, kmh: f64) -> String {
        format!("Forecast wind: gusts up to {kmh:.0} km/h")
    }

    fn cond_fog(&self, hours: usize) -> String {
        format!("Fog forecast for {hours} h")
    }

    fn cond_snow(&self, cm: f64) -> String {
        format!("Forecast snow: {} cm", self.assist().num(cm, 1))
    }

    fn cond_source(&self, point: &str) -> String {
        format!("Source: forecast of the pack at the point \"{point}\"")
    }

    fn cond_altitude(&self, lo: f64, hi: f64) -> String {
        format!("Elevation: from {lo:.0} to {hi:.0} m")
    }

    fn cond_none(&self) -> &'static str {
        "Conditions: no forecast in this outing."
    }

    fn not_an_outing(&self, dir: &str) -> String {
        format!("{dir}: not an outing")
    }

    fn rule_topic(&self, t: skimap_zones::rules::Topic) -> &'static str {
        use skimap_zones::rules::Topic;
        match t {
            Topic::Bivouac => "Bivouac",
            Topic::Access => "Access",
            Topic::LeaveRoutes => "Leaving the marked routes",
            Topic::Fire => "Fire",
        }
    }

    fn rule_verdict(&self, v: skimap_core::zone_eval::Verdict) -> &'static str {
        use skimap_core::zone_eval::Verdict;
        match v {
            Verdict::Forbidden => "forbidden",
            Verdict::ForbiddenInPlaces => "forbidden in places",
            Verdict::Tolerated => "tolerated under conditions",
            Verdict::ConsentRequired => "with the owner's consent",
            Verdict::AvoidRecommended => "to avoid",
            Verdict::Allowed => "allowed",
            Verdict::NoRule => "no known rule",
        }
    }

    fn stoves_included(&self) -> &'static str {
        " (stoves included)"
    }

    fn zone_off_season(&self, name: &str) -> String {
        format!("{name} (off season)")
    }

    fn rule_inactive(&self, i: skimap_core::zone_eval::Inactive) -> &'static str {
        use skimap_core::zone_eval::Inactive;
        match i {
            Inactive::NotInForce => "text not in force at this date",
            Inactive::OutOfPeriod => "outside its period",
            Inactive::Daytime => "night rule, it is daytime at this hour",
            Inactive::ZoneNotSensitive => "zone outside its sensitive months",
            Inactive::NoSunEvent => "sunset cannot be computed",
        }
    }

    fn source_kind(&self, k: skimap_zones::rules::SourceKind) -> &'static str {
        use skimap_zones::rules::SourceKind;
        match k {
            SourceKind::Law => "law or code",
            SourceKind::SignedAct => "signed order",
            SourceKind::DraftAct => "draft order (signed text not read)",
            SourceKind::DecisionSummary => "official summary of the decision",
            SourceKind::Leaflet => "official leaflet",
            SourceKind::OfficialPage => "official page",
            SourceKind::ThirdPartyPage => "third-party page",
            SourceKind::DataService => "data service",
        }
    }

    fn rules_at(&self, t: DateTime) -> String {
        format!("on {:02}/{:02} at {}", t.day(), t.month(), self.clock5(t))
    }

    fn rules_note(&self) -> &'static str {
        "Rules read in the texts cited, at this date and time; the official text and the signs on the ground prevail."
    }

    fn local_act(&self, a: skimap_zones::local::OwnAct) -> &'static str {
        use skimap_zones::local::OwnAct;
        match a {
            OwnAct::ArretePrefectoralBiotope => "biotope protection order",
            OwnAct::DecretReserveNationale => "nature reserve decree",
            OwnAct::DeliberationReserveRegionale => "regional nature reserve decision",
            OwnAct::ReglementationCoeurParc => "national park core regulation",
            OwnAct::ArreteReserveBiologique => "biological reserve order",
            OwnAct::ArreteCaptage => "water catchment protection order",
            OwnAct::ArretesMunicipaux => "municipal orders",
            OwnAct::ArretesPrefectoraux => "prefectoral orders",
            OwnAct::DroitCantonal => "cantonal law",
        }
    }

    fn local_unverified(&self, name: &str, act: &str) -> String {
        format!("{name} ({act}): text not read")
    }

    fn local_partial(&self, name: &str, act: &str) -> String {
        format!("{name} ({act}): read in part only")
    }

    fn local_municipal_fr(&self) -> &'static str {
        "municipal and prefectoral orders: not listed"
    }

    fn local_municipal_ch(&self) -> &'static str {
        "communal regulations: not listed"
    }

    fn local_rules(&self, items: &[String]) -> String {
        if items.is_empty() {
            return String::new();
        }
        format!("Local rules not verified: {}.", items.join("; "))
    }

    fn day(&self, d: Date) -> String {
        format!(
            "{} {:02}/{:02}",
            self.assist()
                .weekday(d.weekday().to_monday_zero_offset() as usize),
            d.day(),
            d.month()
        )
    }

    fn updated(&self, t: DateTime) -> String {
        format!(
            "updated {:02}/{:02} at {}",
            t.day(),
            t.month(),
            self.clock5(t)
        )
    }

    fn date_unknown(&self) -> &'static str {
        "date unknown"
    }

    fn skipped_no_forecast(&self, day: &str) -> String {
        format!("{day}: no forecast in the pack")
    }

    fn skipped_no_start(&self, day: &str) -> String {
        format!("{day}: no known start for this area")
    }

    fn time_group(&self, median_s: f64, lo_s: f64, hi_s: f64) -> String {
        format!(
            "about {} ({} to {}) at the pace of the group",
            self.duration(median_s),
            self.duration(lo_s),
            self.duration(hi_s)
        )
    }

    fn time_model(&self, s: f64) -> String {
        format!("about {} by the model", self.duration(s))
    }

    fn info_label(&self, id: &str) -> Option<&'static str> {
        Some(match id {
            "bera" => "Avalanche bulletin (BERA)",
            "meteo-hourly" => "Hourly weather forecast",
            "status" => "Trail status",
            "snow-depth" => "Estimated snow cover",
            "offpiste-snow" => "Off-piste snow",
            "avalanche-exposure" => "Avalanche exposure (model)",
            "zones" => "Zones and rules",
            "refuges" => "Huts",
            "pistes" => "Trails",
            "pack" => "Area pack",
            _ => return None,
        })
    }

    fn battery_day(&self, reserve_pct: f64, cold: Option<f64>) -> String {
        format!(
            "Battery enough for more than 24 h, {reserve_pct:.0} % kept for an emergency call{}",
            battery_cold(cold)
        )
    }

    fn battery_until(&self, until: &str, reserve_pct: f64, cold: Option<f64>) -> String {
        format!(
            "Battery enough until {until}, {reserve_pct:.0} % kept for an emergency call{}",
            battery_cold(cold)
        )
    }

    fn battery_short(&self, end: &str, reserve: &str) -> String {
        format!(
            "Battery too low for the planned outing (ends around {end}): it will reach the emergency reserve around {reserve}. Shorten the outing, take a power bank or switch to airplane mode."
        )
    }

    fn never_guarantee(&self) -> &'static str {
        "Computed from the relief and the maps, never a guarantee: the avalanche bulletin (BERA) of the day and your own judgement on the ground come first."
    }

    fn missing_layer(&self, id: &str) -> &'static str {
        match id {
            "snow" => "snow of the day",
            "exposure" => "avalanche exposure",
            "zones" => "zones and rules",
            _ => "network coverage",
        }
    }

    fn no_offpiste_relief(&self) -> &'static str {
        "no off-piste relief in this pack"
    }

    fn outside_relief(&self) -> &'static str {
        "point outside the relief of the pack"
    }

    fn no_passable(&self, radius_m: f64) -> String {
        format!(
            "no passable terrain within {radius_m:.0} m of an end (slope of 30° or more, zone closed at this date, very exposed)"
        )
    }

    fn no_offpiste_route(&self) -> &'static str {
        "no route under 30° outside the zones closed at this date"
    }

    fn arrival_range(&self, lo: DateTime, hi: DateTime) -> String {
        format!("{} to {}", self.clock5(lo), self.clock5(hi))
    }

    fn cause(&self, c: &skimap_plan::tracker::Cause) -> String {
        use skimap_plan::tracker::{Cause, Condition};
        match c {
            Cause::Closure { .. } => "trail closed on the route".into(),
            Cause::Late { limit } => {
                format!("late on the limit of {}", self.assist().clock(*limit))
            }
            Cause::Conditions(Condition::Fog) => "fog".into(),
            Cause::Conditions(Condition::WindRising) => "wind rising".into(),
            Cause::Conditions(Condition::Snow) => "snow".into(),
            Cause::Conditions(Condition::TemperatureDrop) => "cold".into(),
            Cause::Deviation => "off the route".into(),
            Cause::NoTime => "no time left to finish the loop before dark".into(),
            Cause::Pace => "proposal accepted".into(),
        }
    }

    fn back_proposal(&self, target: Option<&crate::tracking_back::Target>) -> String {
        use crate::tracking_back::{Target, TargetKind};
        let to = match target {
            Some(Target {
                kind: TargetKind::Destination,
                name: Some(n),
                ..
            }) => format!("to {n}"),
            Some(Target {
                kind: TargetKind::Destination,
                ..
            }) => "to the finish".into(),
            Some(Target {
                kind: TargetKind::Car,
                ..
            }) => "to the car".into(),
            _ => "to the start".into(),
        };
        format!("Time to head back. Be guided {to}?")
    }

    fn fog_kind(&self, kind: &str) -> &'static str {
        match kind {
            "refuge" => "Refuge",
            "parking" => "Car park",
            "start" => "Start",
            _ => "Point",
        }
    }

    fn fog_near(&self, what: &str, place: &str) -> String {
        format!("{what} near {place}")
    }

    fn fog_off(&self, what: &str, distance: &str, bearing_deg: f64, place: &str) -> String {
        let side = crate::fog::cardinal(bearing_deg, crate::card::Lang::En);
        format!("{what} {distance} {side} of {place}")
    }

    fn fog_rank(&self, label: &str, rank: usize) -> String {
        format!("{label} ({rank})")
    }

    fn all_domains(&self) -> &'static str {
        "All installed areas"
    }

    fn packs_note(&self, names: &[&str]) -> String {
        let s = if names.len() == 1 { "" } else { "s" };
        format!("{} pack{s}: {}.", names.len(), names.join(", "))
    }

    fn france_only_name(&self) -> &'static str {
        "France, no area installed"
    }

    fn france_only_note(&self, date: &str) -> String {
        format!("No pack installed: France basemap only, data of {date}.")
    }
}

fn battery_cold(cold: Option<f64>) -> String {
    cold.map(|t| format!(", battery at {t:.0} °C: keep the phone warm"))
        .unwrap_or_default()
}
