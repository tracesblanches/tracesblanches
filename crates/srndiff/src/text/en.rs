//! The texts in English.

use super::Texts;
use crate::rules::Level;

/// English.
pub struct En;

fn where_(at: f64) -> String {
    format!("at km {at:.1}")
}

impl Texts for En {
    fn finish(&self, text: String) -> String {
        text
    }

    fn length(&self, m: f64) -> String {
        if m < 995.0 {
            format!("{:.0} m", (m / 10.0).round() * 10.0)
        } else {
            format!("{:.1} km", m / 1000.0)
        }
    }

    fn hours(&self, h: f64) -> String {
        let m = (h * 60.0).round() as i64;
        if m < 60 {
            format!("{m} min")
        } else {
            format!("{} h {:02} min", m / 60, m % 60)
        }
    }

    fn km(&self, v: f64) -> String {
        format!("km {v:.1}")
    }

    fn level(&self, l: Level) -> &'static str {
        match l {
            Level::Facile => "easy",
            Level::Moyen => "moderate",
            Level::Difficile => "difficult",
            Level::TresDifficile => "very difficult",
        }
    }

    fn steepest_descent(&self, deg: f32, sustain_m: f64, at_km: f64, length_m: f64) -> String {
        format!(
            "steepest descent: {:.0}° average over {:.0} m {} (a {:.0} m passage steeper than 8°)",
            deg,
            sustain_m,
            where_(at_km),
            length_m
        )
    }

    fn gentle_descents(&self, deg: f32) -> String {
        format!("gentle descents (at most {deg:.0}°)")
    }

    fn no_descent(&self) -> String {
        "no notable descent".to_owned()
    }

    fn steepest_climb(&self, deg: f32, sustain_m: f64, at_km: f64, length_m: f64) -> String {
        format!(
            "steepest climb: {:.0}° average over {:.0} m {} (a {:.0} m passage steeper than 8°)",
            deg,
            sustain_m,
            where_(at_km),
            length_m
        )
    }

    fn forest_descent(&self, deg: f32, dense_m: f64, at_km: f64) -> String {
        format!(
            "descent through dense forest with no opening: {:.0}° over {:.0} m {}",
            deg,
            dense_m,
            where_(at_km)
        )
    }

    fn sidehill(&self, deg: f32, length_m: f64, at_km: f64) -> String {
        format!(
            "side-hill traverse on a {:.0}° slope ({:.0} m) {}",
            deg,
            length_m,
            where_(at_km)
        )
    }

    fn beyond(&self, deg: f32) -> String {
        format!("a passage at {deg:.0}° or more: beyond Nordic backcountry ski terrain")
    }

    fn effort(&self, length_km: f64, ascent_m: f64, time: &str, raid: bool) -> String {
        format!(
            "{:.1} km, {:.0} m of climbing, about {} of actual travel (Munter){}",
            length_km,
            ascent_m,
            time,
            if raid {
                ": a multi-day trip (raid)"
            } else {
                ""
            }
        )
    }

    fn high_exposed(&self, length: &str, first_at_km: Option<f64>) -> String {
        format!(
            "crosses {} of terrain the model rates very exposed to avalanches{}",
            length,
            first_at_km
                .map(|a| format!(", first {}", where_(a)))
                .unwrap_or_default()
        )
    }

    fn exposed(&self, length: &str, n: usize) -> String {
        format!(
            "crosses {} of terrain the model rates exposed to avalanches, in {} passage{}",
            length,
            n,
            if n > 1 { "s" } else { "" }
        )
    }

    fn relief_and_exposure_unknown(&self, share_pct: f64) -> String {
        format!(
            "relief and avalanche exposure unknown on {share_pct:.0} % of the track (outside the IGN LiDAR HD relief): the level may be underestimated"
        )
    }

    fn exposure_unknown(&self, length: &str, deg: f32) -> String {
        format!("avalanche exposure not assessed on {length}, with slopes up to {deg:.0}°")
    }

    fn remote(&self, km: f64, at_km: f64) -> String {
        format!(
            "up to {:.1} km from a road or an end of the track ({})",
            km,
            where_(at_km)
        )
    }

    fn no_network(&self, length: &str, at_km: f64) -> String {
        format!(
            "{} in a row with no mobile network at all, not even for 112 ({})",
            length,
            where_(at_km)
        )
    }

    fn forbidden(&self, length: &str) -> String {
        format!("crosses {length} of an area closed to access on this date")
    }

    fn fresh_snow(&self, cm: f32) -> String {
        format!("{cm:.0} cm of fresh snow to break: more effort and time (≈ +30 %)")
    }

    fn icy(&self, icy: bool) -> String {
        if icy {
            "hard or refrozen snow: descents one level trickier"
        } else {
            "crust: descents one level trickier"
        }
        .to_owned()
    }

    fn coarse(&self, share_pct: f64, coarse_m: f64) -> String {
        format!(
            "approximate track: {share_pct:.0} % of its length in straight lines of more than {coarse_m:.0} m between two points; slopes averaged between the points, short steps may be missed"
        )
    }

    fn relief_unknown(&self, share_pct: f64) -> String {
        format!(
            "relief unknown on {share_pct:.0} % of the track (outside the IGN LiDAR HD relief): the level may be underestimated"
        )
    }

    fn passage(&self, at_km: f64, deg: f32, length_m: f64) -> String {
        format!("{} ({:.0}° over {:.0} m)", self.km(at_km), deg, length_m)
    }

    fn sidehill_passage(&self, at_km: f64, deg: f32, length_m: f64) -> String {
        format!(
            "{} ({:.0}° slope over {:.0} m)",
            self.km(at_km),
            deg,
            length_m
        )
    }

    fn stretch(&self, at_km: f64, length: &str) -> String {
        format!("{} ({})", self.km(at_km), length)
    }

    fn grip_skins(
        &self,
        n: usize,
        from_deg: f64,
        total: &str,
        longest_m: f64,
        longest_deg: f32,
        longest_at_km: f64,
    ) -> String {
        format!(
            "Short skins (or half skins) for {} climb{} of {:.0}° or more ({} in all); the longest: {:.0} m at {:.0}° at {}. Elsewhere, fish scales are enough.",
            n,
            if n > 1 { "s" } else { "" },
            from_deg,
            total,
            longest_m,
            longest_deg,
            self.km(longest_at_km)
        )
    }

    fn grip_gentle(&self) -> String {
        "Gentle climbs (below 8°): fish scales are enough.".to_owned()
    }

    fn grip_scales(&self, from_deg: f64) -> String {
        format!(
            "Climbs below {from_deg:.0}° or short: fish scales, firm kicks and poles, herringbone on the steps; skins help on hard snow, powder, or with a heavy pack or a pulk."
        )
    }

    fn grip_slippery(&self) -> String {
        "Slippery snow today (refrozen or powder): fish scales grip poorly, put the skins on from 8°.".to_owned()
    }

    fn grip_keep_skins(&self) -> String {
        "Always carry a pair of skins in the pack.".to_owned()
    }

    fn technique(&self, d: f32) -> String {
        if d < 8.0 {
            "Gentle descents: the snowplough is enough.".to_owned()
        } else if d < 15.0 {
            "Descents up to 15°: a controlled snowplough and stop.".to_owned()
        } else if d < 20.0 {
            "Descents of 15 to 20°: turns (stem or telemark) or traverses and kick turns."
                .to_owned()
        } else if d < 25.0 {
            "Descents of 20 to 25°: controlled side-slipping on the edges.".to_owned()
        } else {
            "Descents steeper than 25°: very good side-slip control, otherwise take the skis off and walk down.".to_owned()
        }
    }

    fn steep_descents(&self, list: &str) -> String {
        format!("Steep descents: {list}.")
    }

    fn forest_descents(&self, list: &str) -> String {
        format!(
            "Descent through dense forest: {list}. Slow down; keep the skins on to brake if the snow is hard."
        )
    }

    fn kick_turns(&self, list: &str) -> String {
        format!("Kick turns or side-stepping: {list}.")
    }

    fn sidehills(&self, list: &str) -> String {
        format!(
            "Steep slope traversed: {list}; easy on a path cut into the slope, but off the path or where snow fills it: edges set uphill, on foot if the snow is hard."
        )
    }

    fn glide(&self, flat_km: f64, longest_km: f64) -> String {
        format!(
            "Lots of flat ({flat_km:.1} km, {longest_km:.1} km in a row): skis waxed for glide, fish scales rather than skins, diagonal stride and double poling."
        )
    }

    fn avoid_beyond(&self, list: &str) -> String {
        format!(
            "Climb or descent of 30° or more, beyond SRN terrain: {list}; go around it or take the skis off."
        )
    }

    fn avoid_high_exposed(&self, list: &str) -> String {
        format!(
            "Terrain the model rates very exposed to avalanches: {list}; avoid it whenever the danger is not low."
        )
    }

    fn avoid_forbidden(&self, length: &str) -> String {
        format!("Area closed to access on this date over {length}: change the route.")
    }

    fn avoid_leave_routes(&self, length: &str, zone: Option<&str>) -> String {
        format!(
            "Leaving the marked routes is forbidden over {}{}: stay on the markings.",
            length,
            zone.map(|z| format!(" ({z})")).unwrap_or_default()
        )
    }

    fn avoid_quiet(&self, length: &str) -> String {
        format!("Wildlife quiet zone over {length}: avoid it in season.")
    }

    fn turn_back_hardest(&self, at_km: f64) -> String {
        format!(
            "The hardest passage is far from the start ({}): if you struggle before it, turn back.",
            self.km(at_km)
        )
    }

    fn turn_back_far(&self, at_km: f64, km: f64) -> String {
        format!(
            "Farthest point from a road: {} ({:.1} km). Set a turnaround time and keep to it.",
            self.km(at_km),
            km
        )
    }

    fn other_direction(&self, easier: bool, level: Level) -> String {
        format!(
            "The other way round, the track is {} ({}).",
            if easier { "easier" } else { "harder" },
            self.level(level)
        )
    }

    fn equip_avalanche(&self) -> String {
        "Avalanche terrain crossed: transceiver, shovel, probe and the avalanche bulletin (BERA) when there is one; the model does not replace it.".to_owned()
    }

    fn equip_no_network(&self, length: &str) -> String {
        format!(
            "No network over {length} (not even 112): tell someone close the route and the time you will be back."
        )
    }

    fn equip_raid(&self, time: &str, stages: f64) -> String {
        format!(
            "A multi-day trip: about {time} of actual travel, so {stages} stages of at most 6 h; plan lodging, a stove, repairs (binding, skins) and a headlamp."
        )
    }

    fn equip_long_day(&self, time: &str) -> String {
        format!("A long day (about {time}): headlamp, food and a spare warm layer.")
    }

    fn equip_open(&self, km: f64) -> String {
        format!(
            "Open terrain for {km:.1} km in a row: wind and whiteout possible; offline track and compass."
        )
    }

    fn snow_north(&self) -> String {
        "Steep north-facing descents: snow often cold, hard or powdery.".to_owned()
    }

    fn snow_south(&self) -> String {
        "Steep south-facing descents: refrozen in the morning, heavy snow in the afternoon; start early."
            .to_owned()
    }

    fn direction(
        &self,
        forward: bool,
        level: Level,
        beyond: bool,
        technical: Level,
        effort: Level,
        length_km: f64,
        ascent_m: f64,
        descent_m: f64,
        time: &str,
    ) -> String {
        let dir = if forward {
            "In the direction of the track"
        } else {
            "The other way round"
        };
        format!(
            "{dir}: {}{} — technique {}, effort {} on the 2025 scale; {:.1} km, +{:.0} m / −{:.0} m, about {}.",
            self.level(level).to_uppercase(),
            if beyond {
                " (beyond SRN in places)"
            } else {
                ""
            },
            self.level(technical),
            self.level(effort),
            length_km,
            ascent_m,
            descent_m,
            time
        )
    }

    fn reasons_title(&self, forward: bool) -> &'static str {
        if forward {
            "Reasons:"
        } else {
            "Reasons for this direction only:"
        }
    }

    fn advice_title(&self, forward: bool) -> &'static str {
        if forward {
            "Advice:"
        } else {
            "Advice for this direction only:"
        }
    }

    fn disclaimer(&self) -> &'static str {
        "An indication computed from the relief and the maps, never a guarantee: the snow, the weather and the avalanche bulletin decide on the spot."
    }
}
