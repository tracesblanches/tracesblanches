//! Equipment list (A6, #93): avalanche kit, headlamp, layers, power bank, compass, each
//! item with its reason.
//!
//! Rules (each item says which fact triggered it):
//!
//! | Item | When |
//! |---|---|
//! | transceiver, shovel, probe | the route crosses terrain « exposé » or more of the exposure layer (P16–P18) on 50 m or more, or a historic avalanche path (CLPA, P20), or leaves the trails in a massif whose BERA of the day is 3 or more |
//! | headlamp | planned return (estimated time × 1.2, the C7 margin) less than an hour before sunset, or after it; or a start before sunrise |
//! | extra warm layer | wind chill of the outing hours at −10 °C or below; at −20 °C, mittens and face protection too |
//! | power bank, tell someone | 1 km or more, or a quarter of the route, without any mobile network (P12); telling someone also for an outing of more than 3 hours or off the trails |
//! | compass | fog forecast during the outing |
//! | sunglasses, sun cream | clear sky (cloud below 30 %) on snow |
//! | waxless or skin skis | grip wax unreliable that day (A2) |
//!
//! The voice says the items of the phrase bank (`Event::Equipment`); sunglasses and skis
//! have no phrase yet (requested).

use jiff::{
    SignedDuration,
    civil::{Date, DateTime},
};
use serde::Serialize;
use skimap_common::geom::LonLat;
use skimap_voice::Event;
use skimap_voice::bank::Item;

use crate::inputs::SiteData;
use crate::layers::densify;
use crate::text::Lang;

/// An outing.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Outing {
    /// Label.
    pub label: String,
    /// Start, local.
    pub start: DateTime,
    /// Estimated time (C5 model), minutes.
    pub duration_min: f64,
    /// Route.
    #[serde(skip)]
    pub route: Vec<LonLat>,
    /// Off the groomed trails (ski touring, SRN).
    pub offpiste: bool,
    /// Grip wax unreliable that day (A2).
    pub kick_unreliable: bool,
}

/// An item.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    /// Transceiver, shovel, probe.
    AvalancheKit,
    /// Headlamp.
    Headlamp,
    /// Extra warm layer.
    WarmLayer,
    /// Mittens and face protection.
    FaceProtection,
    /// Power bank.
    PowerBank,
    /// Tell someone the route and the return time.
    TellSomeone,
    /// Compass.
    Compass,
    /// Sunglasses and sun cream.
    SunProtection,
    /// Waxless or skin skis.
    WaxlessSkis,
}

impl Kind {
    /// French label.
    pub fn label_fr(self) -> &'static str {
        self.label(Lang::Fr)
    }

    /// Label in `lang`.
    pub fn label(self, lang: Lang) -> &'static str {
        crate::text::of(lang).eq_label(self)
    }

    /// Item of the phrase bank, when there is one.
    pub fn voice(self) -> Option<Item> {
        Some(match self {
            Kind::AvalancheKit => Item::AvalancheKit,
            Kind::Headlamp => Item::Headlamp,
            Kind::WarmLayer => Item::WarmLayer,
            Kind::PowerBank => Item::PowerBank,
            Kind::TellSomeone => Item::TellSomeone,
            Kind::Compass => Item::Compass,
            Kind::FaceProtection => Item::FaceProtection,
            Kind::SunProtection => Item::SunProtection,
            Kind::WaxlessSkis => Item::Waxless,
        })
    }
}

/// An item with its reason.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Entry {
    /// Item.
    pub kind: Kind,
    /// Label.
    pub label: &'static str,
    /// Reason, French.
    pub reason: String,
}

/// Facts used.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Facts {
    /// Route length, km.
    pub route_km: f64,
    /// Km at exposure « exposé » or more.
    pub exposed_km: f64,
    /// Km in historic avalanche paths.
    pub clpa_km: f64,
    /// Km without any network.
    pub no_network_km: f64,
    /// BERA level of the day.
    pub bera_level: Option<u8>,
    /// Planned return, local.
    pub back: DateTime,
    /// Sunset, local.
    pub sunset: Option<DateTime>,
    /// Lowest wind chill of the outing, °C.
    pub wind_chill_min_c: Option<f64>,
    /// Fog hours during the outing.
    pub fog_hours: usize,
    /// Mean cloud cover of the outing, %.
    pub cloud_mean_pct: Option<f64>,
}

/// The list of an outing.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Equipment {
    /// Outing.
    pub outing: Outing,
    /// Items.
    pub items: Vec<Entry>,
    /// Facts.
    pub facts: Facts,
    /// Text, French.
    pub text: String,
}

impl Equipment {
    /// Voice: « À emporter. … » with the items of the phrase bank.
    pub fn voice(&self) -> Event {
        Event::Equipment {
            items: self.items.iter().filter_map(|e| e.kind.voice()).collect(),
        }
    }
}

/// Equipment list of an outing.
pub fn equipment(data: &SiteData, o: &Outing) -> Equipment {
    equipment_in(data, o, Lang::Fr)
}

/// [`equipment`] with the texts in `lang` (the phone's language, VO6).
pub fn equipment_in(data: &SiteData, o: &Outing, lang: Lang) -> Equipment {
    let tx = crate::text::of(lang);
    let date: Date = o.start.date();
    let back = o.start
        + SignedDuration::from_secs(
            (o.duration_min
                * 60.0
                * skimap_core::turnaround::TurnaroundParams::default().eta_factor)
                as i64,
        );
    let (mut exposed, mut clpa, mut no_net, mut total) = (0.0, 0.0, 0.0, 0.0);
    for (p, len) in densify(&o.route, 10.0) {
        total += len;
        if data.exposure_at(p).is_some_and(|c| c >= 2) {
            exposed += len;
        }
        if data
            .clpa
            .as_ref()
            .is_some_and(|c| c.areas.find(p).is_some())
        {
            clpa += len;
        }
        if data
            .no_network
            .as_ref()
            .is_some_and(|n| n.find(p).is_some())
        {
            no_net += len;
        }
    }
    let (exposed, clpa, no_net, total) = (
        exposed / 1000.0,
        clpa / 1000.0,
        no_net / 1000.0,
        total / 1000.0,
    );
    let bera = data.bera_for(date).and_then(|b| b.level());
    let place = o.route.first().copied().unwrap_or(data.site.bbox.center());
    let suns = skimap_core::ephemeris::sun_times(date, place).map(|(r, s)| {
        (
            skimap_core::ephemeris::local_time(r),
            skimap_core::ephemeris::local_time(s),
        )
    });
    let w = data
        .weather
        .as_ref()
        .and_then(|w| w.nearest(place))
        .map(|(p, _)| {
            p.summary_between(
                o.start - SignedDuration::from_mins(i64::from(o.start.minute())),
                back,
            )
        });
    let mut items: Vec<Entry> = Vec::new();
    let mut add = |kind: Kind, reason: String| {
        items.push(Entry {
            kind,
            label: tx.eq_label(kind),
            reason,
        })
    };
    let mut why = Vec::new();
    if exposed >= 0.05 {
        why.push(tx.eq_exposed(exposed));
    }
    if clpa >= 0.05 {
        why.push(tx.eq_clpa(clpa));
    }
    if o.offpiste && bera.is_some_and(|l| l >= 3) {
        why.push(tx.eq_offpiste_bera(bera.unwrap_or(0)));
    }
    if !why.is_empty() {
        add(Kind::AvalancheKit, why.join(", "));
    }
    if let Some((rise, set)) = suns {
        if back + SignedDuration::from_hours(1) > set {
            add(Kind::Headlamp, tx.eq_back_after_sunset(back, set));
        } else if o.start < rise {
            add(Kind::Headlamp, tx.eq_start_before_sunrise(o.start, rise));
        }
    }
    if let Some(c) = w.as_ref().and_then(|w| w.wind_chill_min_c) {
        if c <= -10.0 {
            add(Kind::WarmLayer, tx.eq_chill(&crate::briefing::degrees(c)));
        }
        if c <= -20.0 {
            add(
                Kind::FaceProtection,
                tx.eq_chill(&crate::briefing::degrees(c)),
            );
        }
    }
    let long_no_net = no_net >= 1.0 || (total > 0.0 && no_net / total >= 0.25);
    if long_no_net {
        let r = tx.eq_no_network(no_net, (100.0 * no_net / total.max(1e-9)).round());
        add(Kind::PowerBank, r.clone());
        add(Kind::TellSomeone, r);
    } else if o.duration_min > 180.0 || o.offpiste {
        add(
            Kind::TellSomeone,
            if o.offpiste {
                tx.eq_offpiste().into()
            } else {
                tx.eq_long(o.duration_min.round())
            },
        );
    }
    if let Some(w) = &w
        && w.fog_hours > 0
    {
        add(Kind::Compass, tx.eq_fog(w.fog_hours));
    }
    if let Some(c) = w.as_ref().and_then(|w| w.cloud_mean_pct)
        && c < 30.0
    {
        add(Kind::SunProtection, tx.eq_clear_sky(c));
    }
    if o.kick_unreliable {
        add(Kind::WaxlessSkis, tx.eq_kick_unreliable().into());
    }
    let text = if items.is_empty() {
        tx.eq_nothing(&o.label)
    } else {
        let pairs: Vec<(&str, &str)> = items.iter().map(|e| (e.label, e.reason.as_str())).collect();
        tx.eq_take(&o.label, &pairs)
    };
    Equipment {
        outing: o.clone(),
        facts: Facts {
            route_km: (total * 100.0).round() / 100.0,
            exposed_km: (exposed * 100.0).round() / 100.0,
            clpa_km: (clpa * 100.0).round() / 100.0,
            no_network_km: (no_net * 100.0).round() / 100.0,
            bera_level: bera,
            back,
            sunset: suns.map(|s| s.1),
            wind_chill_min_c: w.as_ref().and_then(|w| w.wind_chill_min_c),
            fog_hours: w.as_ref().map_or(0, |w| w.fog_hours),
            cloud_mean_pct: w.as_ref().and_then(|w| w.cloud_mean_pct),
        },
        items,
        text,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::layers::{Area, Areas, Weather, WeatherHour, WeatherPoint};
    /// Evaluation site of `data/sites.json`, test data.
    use jiff::civil::date;
    static CERVIERES: std::sync::LazyLock<skimap_common::site::Site> =
        std::sync::LazyLock::new(|| {
            *skimap_common::site::site("cervieres").expect("evaluation site")
        });

    fn data(chill: f64, fog: bool) -> SiteData {
        let d0 = date(2027, 1, 12).at(0, 0, 0, 0);
        let hours = (0..24)
            .map(|i| WeatherHour {
                time: d0 + SignedDuration::from_hours(i),
                temperature_c: Some(-5.0),
                apparent_temperature_c: None,
                wind_chill_c: Some(chill),
                wind_kmh: Some(10.0),
                gust_kmh: Some(20.0),
                visibility_m: None,
                fog: Some(fog),
                freezing_level_m: None,
                snowfall_cm: Some(0.0),
                precipitation_mm: Some(0.0),
                cloud_cover_pct: Some(90.0),
                relative_humidity_pct: None,
                weather_code: None,
            })
            .collect();
        let sq = serde_json::json!({"type":"Polygon","coordinates":[[[6.70,44.84],[6.80,44.84],[6.80,44.86],[6.70,44.86],[6.70,44.84]]]});
        SiteData {
            site: *CERVIERES,
            weather: Some(Weather {
                points: vec![WeatherPoint {
                    id: "p".into(),
                    name: "P".into(),
                    position: LonLat::new(6.7269, 44.8552),
                    elevation_m: 1750.0,
                    hours,
                }],
                files: vec![],
            }),
            snows: vec![],
            beras: vec![],
            status: None,
            domains: vec![],
            pistes: vec![],
            clpa: None,
            no_network: Some(Areas {
                areas: vec![Area::new("x", crate::layers::multipolygon(&sq))],
            }),
            zones: None,
            route: None,
            graph: None,
            exposure: None,
            missing: vec![],
            sources: vec![],
            files: vec![],
        }
    }

    #[test]
    fn items_follow_their_facts() {
        let route = vec![LonLat::new(6.72, 44.85), LonLat::new(6.76, 44.85)];
        let o = Outing {
            label: "test".into(),
            start: date(2027, 1, 12).at(14, 0, 0, 0),
            duration_min: 120.0,
            route,
            offpiste: false,
            kick_unreliable: true,
        };
        let e = equipment(&data(-22.0, true), &o);
        let kinds: Vec<Kind> = e.items.iter().map(|i| i.kind).collect();
        // Back at 16:24, sunset ≈ 17:10: headlamp. −22 °C: layer and face. 3.2 km without
        // network: power bank and tell someone. Fog: compass. Grip unreliable: skis.
        for k in [
            Kind::Headlamp,
            Kind::WarmLayer,
            Kind::FaceProtection,
            Kind::PowerBank,
            Kind::TellSomeone,
            Kind::Compass,
            Kind::WaxlessSkis,
        ] {
            assert!(kinds.contains(&k), "{k:?} missing: {}", e.text);
        }
        assert!(!kinds.contains(&Kind::AvalancheKit));
        assert!(e.facts.no_network_km > 3.0);
        let Event::Equipment { items } = e.voice() else {
            panic!()
        };
        // Every item of the list has its phrase now (face protection, waxless skis too).
        assert_eq!(items.len(), 7);
        assert!(items.contains(&Item::FaceProtection) && items.contains(&Item::Waxless));
        // Morning, mild, no fog: no headlamp, no layer, no compass.
        let o2 = Outing {
            start: o.start - SignedDuration::from_hours(4),
            kick_unreliable: false,
            ..o
        };
        let e2 = equipment(&data(-5.0, false), &o2);
        let k2: Vec<Kind> = e2.items.iter().map(|i| i.kind).collect();
        assert!(
            !k2.contains(&Kind::Headlamp) && !k2.contains(&Kind::Compass),
            "{}",
            e2.text
        );
    }
}
