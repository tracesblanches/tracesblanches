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

use chrono::{Duration, NaiveDate, NaiveDateTime, Timelike};
use serde::Serialize;
use skimap_common::geom::LonLat;
use skimap_voice::Event;
use skimap_voice::bank::Item;

use crate::inputs::SiteData;
use crate::layers::densify;

/// An outing.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Outing {
    /// Label.
    pub label: String,
    /// Start, local.
    pub start: NaiveDateTime,
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
        match self {
            Kind::AvalancheKit => "DVA, pelle et sonde",
            Kind::Headlamp => "frontale",
            Kind::WarmLayer => "couche chaude en plus",
            Kind::FaceProtection => "moufles et protection du visage",
            Kind::PowerBank => "batterie externe",
            Kind::TellSomeone => "prévenir un proche de l'itinéraire et de l'heure de retour",
            Kind::Compass => "boussole",
            Kind::SunProtection => "lunettes de soleil et crème",
            Kind::WaxlessSkis => "skis à écailles ou peaux",
        }
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
            Kind::FaceProtection | Kind::SunProtection | Kind::WaxlessSkis => return None,
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
    pub back: NaiveDateTime,
    /// Sunset, local.
    pub sunset: Option<NaiveDateTime>,
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

fn fmt(x: f64) -> String {
    format!("{x:.1}").replace('.', ",")
}

fn hhmm(t: NaiveDateTime) -> String {
    format!("{} h {:02}", t.hour(), t.minute())
}

/// Equipment list of an outing.
pub fn equipment(data: &SiteData, o: &Outing) -> Equipment {
    let date: NaiveDate = o.start.date();
    let back = o.start
        + Duration::seconds(
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
                o.start - Duration::minutes(i64::from(o.start.minute())),
                back,
            )
        });
    let mut items: Vec<Entry> = Vec::new();
    let mut add = |kind: Kind, reason: String| {
        items.push(Entry {
            kind,
            label: kind.label_fr(),
            reason,
        })
    };
    let mut why = Vec::new();
    if exposed >= 0.05 {
        why.push(format!(
            "{} km en terrain exposé aux avalanches (modèle)",
            fmt(exposed)
        ));
    }
    if clpa >= 0.05 {
        why.push(format!(
            "{} km dans des zones d'avalanche connues (CLPA)",
            fmt(clpa)
        ));
    }
    if o.offpiste && bera.is_some_and(|l| l >= 3) {
        why.push(format!(
            "hors piste avec un BERA à {} sur 5",
            bera.unwrap_or(0)
        ));
    }
    if !why.is_empty() {
        add(Kind::AvalancheKit, why.join(", "));
    }
    if let Some((rise, set)) = suns {
        if back + Duration::hours(1) > set {
            add(
                Kind::Headlamp,
                format!(
                    "retour prévu vers {}, coucher du soleil à {}",
                    hhmm(back),
                    hhmm(set)
                ),
            );
        } else if o.start < rise {
            add(
                Kind::Headlamp,
                format!(
                    "départ à {}, avant le lever du soleil ({})",
                    hhmm(o.start),
                    hhmm(rise)
                ),
            );
        }
    }
    if let Some(c) = w.as_ref().and_then(|w| w.wind_chill_min_c) {
        if c <= -10.0 {
            add(Kind::WarmLayer, format!("ressenti jusqu'à {c:.0} °C"));
        }
        if c <= -20.0 {
            add(Kind::FaceProtection, format!("ressenti jusqu'à {c:.0} °C"));
        }
    }
    let long_no_net = no_net >= 1.0 || (total > 0.0 && no_net / total >= 0.25);
    if long_no_net {
        let r = format!(
            "{} km sans aucun réseau mobile, même pour le 112 ({} % de l'itinéraire)",
            fmt(no_net),
            (100.0 * no_net / total.max(1e-9)).round()
        );
        add(Kind::PowerBank, r.clone());
        add(Kind::TellSomeone, r);
    } else if o.duration_min > 180.0 || o.offpiste {
        add(
            Kind::TellSomeone,
            if o.offpiste {
                "sortie hors des pistes".into()
            } else {
                format!(
                    "sortie de plus de 3 h ({} min estimées)",
                    o.duration_min.round()
                )
            },
        );
    }
    if let Some(w) = &w
        && w.fog_hours > 0
    {
        add(
            Kind::Compass,
            format!("brouillard prévu ({} h pendant la sortie)", w.fog_hours),
        );
    }
    if let Some(c) = w.as_ref().and_then(|w| w.cloud_mean_pct)
        && c < 30.0
    {
        add(
            Kind::SunProtection,
            format!("ciel dégagé ({c:.0} % de nuages)"),
        );
    }
    if o.kick_unreliable {
        add(
            Kind::WaxlessSkis,
            "retenue du fart incertaine ce jour-là (fartage A2)".into(),
        );
    }
    let text = if items.is_empty() {
        format!(
            "{} : rien de particulier à ajouter à l'équipement habituel.",
            o.label
        )
    } else {
        format!(
            "{} — à emporter : {}.",
            o.label,
            items
                .iter()
                .map(|e| format!("{} ({})", e.label, e.reason))
                .collect::<Vec<_>>()
                .join(" ; ")
        )
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
    use skimap_common::site::CERVIERES;

    fn data(chill: f64, fog: bool) -> SiteData {
        let d0 = NaiveDate::from_ymd_opt(2027, 1, 12)
            .unwrap()
            .and_hms_opt(0, 0, 0)
            .unwrap();
        let hours = (0..24)
            .map(|i| WeatherHour {
                time: d0 + Duration::hours(i),
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
            site: CERVIERES,
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
            start: NaiveDate::from_ymd_opt(2027, 1, 12)
                .unwrap()
                .and_hms_opt(14, 0, 0)
                .unwrap(),
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
        assert_eq!(items.len(), 5);
        // Morning, mild, no fog: no headlamp, no layer, no compass.
        let o2 = Outing {
            start: o.start - Duration::hours(4),
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
