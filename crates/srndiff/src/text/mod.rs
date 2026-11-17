//! Texts of this crate in the language of the app (VO6, #129): one module per language
//! ([`fr`], [`en`]), each implementing every text of [`Texts`]; [`of`] gives the texts of
//! a language. A text missing in one language does not compile.
//!
//! The level, its reasons, the advice and the text of both directions reach the app (the
//! off-piste route); the calibration and evaluation reports of the pipeline stay in French
//! ([`crate::rules::rate`], [`crate::report::assess`], [`crate::report::Report::text_fr`]).
//!
//! Short, concrete sentences; never « sans danger » / « safe ».

pub use skimap_common::Lang;

use crate::rules::Level;

pub mod en;
pub mod fr;

/// The texts of [`Lang::Fr`] or [`Lang::En`].
pub fn of(lang: Lang) -> &'static dyn Texts {
    match lang {
        Lang::Fr => &fr::Fr,
        Lang::En => &en::En,
    }
}

/// Every text of this crate, in one language.
pub trait Texts: Sync {
    // ---- Numbers.
    /// The decimal separator of the language on a whole text (French: a point between two
    /// digits becomes a comma).
    fn finish(&self, text: String) -> String;
    /// A length in words: metres below 1 km, before [`Texts::finish`].
    fn length(&self, m: f64) -> String;
    /// A walking time: « 2 h 05 », « 2 h 05 min ».
    fn hours(&self, h: f64) -> String;
    /// A position along the track: « km 3.2 ».
    fn km(&self, v: f64) -> String;

    // ---- Levels.
    /// « facile », « moyen », « difficile », « très difficile ».
    fn level(&self, l: Level) -> &'static str;

    // ---- Reasons of the level.
    /// Steepest descent over the sustain window, at `at_km`, with its passage above 8°.
    fn steepest_descent(&self, deg: f32, sustain_m: f64, at_km: f64, length_m: f64) -> String;
    /// Gentle descents, at most `deg`.
    fn gentle_descents(&self, deg: f32) -> String;
    /// No notable descent.
    fn no_descent(&self) -> String;
    /// Steepest climb over the sustain window, at `at_km`, with its passage above 8°.
    fn steepest_climb(&self, deg: f32, sustain_m: f64, at_km: f64, length_m: f64) -> String;
    /// Descent in dense forest without an opening.
    fn forest_descent(&self, deg: f32, dense_m: f64, at_km: f64) -> String;
    /// Side-hill traverse.
    fn sidehill(&self, deg: f32, length_m: f64, at_km: f64) -> String;
    /// A passage at `deg` or more: outside SRN terrain.
    fn beyond(&self, deg: f32) -> String;
    /// Distance, climbing, walking time, raid.
    fn effort(&self, length_km: f64, ascent_m: f64, time: &str, raid: bool) -> String;
    /// Length of « très exposé » terrain, first at.
    fn high_exposed(&self, length: &str, first_at_km: Option<f64>) -> String;
    /// Length of « exposé » terrain in `n` passages.
    fn exposed(&self, length: &str, n: usize) -> String;
    /// Relief and exposure unknown on a share of the track.
    fn relief_and_exposure_unknown(&self, share_pct: f64) -> String;
    /// Exposure not evaluated on a length with slopes up to `deg`.
    fn exposure_unknown(&self, length: &str, deg: f32) -> String;
    /// Farthest from a road or an end of the track.
    fn remote(&self, km: f64, at_km: f64) -> String;
    /// A length without any mobile network.
    fn no_network(&self, length: &str, at_km: f64) -> String;
    /// A length in a zone forbidden at the date.
    fn forbidden(&self, length: &str) -> String;
    /// Fresh snow to break.
    fn fresh_snow(&self, cm: f32) -> String;
    /// Hard or refrozen snow (`icy`), else a crust.
    fn icy(&self, icy: bool) -> String;
    /// Approximate track.
    fn coarse(&self, share_pct: f64, coarse_m: f64) -> String;
    /// Relief unknown on a share of the track.
    fn relief_unknown(&self, share_pct: f64) -> String;

    // ---- Advice.
    /// A passage in a list: « km 3.2 (16° over 120 m) ».
    fn passage(&self, at_km: f64, deg: f32, length_m: f64) -> String;
    /// A side-hill passage in a list.
    fn sidehill_passage(&self, at_km: f64, deg: f32, length_m: f64) -> String;
    /// A « très exposé » stretch in a list.
    fn stretch(&self, at_km: f64, length: &str) -> String;
    /// Short skins for `n` climbs of `from_deg` or more.
    #[allow(clippy::too_many_arguments)]
    fn grip_skins(
        &self,
        n: usize,
        from_deg: f64,
        total: &str,
        longest_m: f64,
        longest_deg: f32,
        longest_at_km: f64,
    ) -> String;
    /// Gentle climbs: scales.
    fn grip_gentle(&self) -> String;
    /// Climbs below `from_deg` or short: scales.
    fn grip_scales(&self, from_deg: f64) -> String;
    /// Slippery snow of the day.
    fn grip_slippery(&self) -> String;
    /// Always skins in the bag.
    fn grip_keep_skins(&self) -> String;
    /// Downhill technique for descents up to `deg`.
    fn technique(&self, deg: f32) -> String;
    /// Steep descents.
    fn steep_descents(&self, list: &str) -> String;
    /// Descents in dense forest.
    fn forest_descents(&self, list: &str) -> String;
    /// Kick turns or side steps.
    fn kick_turns(&self, list: &str) -> String;
    /// Steep slopes traversed.
    fn sidehills(&self, list: &str) -> String;
    /// Long flats.
    fn glide(&self, flat_km: f64, longest_km: f64) -> String;
    /// Climbs or descents of 30° or more.
    fn avoid_beyond(&self, list: &str) -> String;
    /// « Très exposé » terrain.
    fn avoid_high_exposed(&self, list: &str) -> String;
    /// Zone forbidden at the date.
    fn avoid_forbidden(&self, length: &str) -> String;
    /// Leaving the marked routes forbidden, `zone`: its name.
    fn avoid_leave_routes(&self, length: &str, zone: Option<&str>) -> String;
    /// Wildlife quiet zone.
    fn avoid_quiet(&self, length: &str) -> String;
    /// The hardest passage far from the start.
    fn turn_back_hardest(&self, at_km: f64) -> String;
    /// Farthest point from a road.
    fn turn_back_far(&self, at_km: f64, km: f64) -> String;
    /// The other direction is easier or harder, at `level`.
    fn other_direction(&self, easier: bool, level: Level) -> String;
    /// Avalanche terrain crossed: equipment.
    fn equip_avalanche(&self) -> String;
    /// No network on a length.
    fn equip_no_network(&self, length: &str) -> String;
    /// Several days.
    fn equip_raid(&self, time: &str, stages: f64) -> String;
    /// A long day.
    fn equip_long_day(&self, time: &str) -> String;
    /// Open terrain on `km` in a row.
    fn equip_open(&self, km: f64) -> String;
    /// Steep descents facing north.
    fn snow_north(&self) -> String;
    /// Steep descents facing south.
    fn snow_south(&self) -> String;

    // ---- The text of both directions.
    /// One direction: its level, technique, effort and figures.
    #[allow(clippy::too_many_arguments)]
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
    ) -> String;
    /// « Raisons : », « Raisons propres à ce sens : ».
    fn reasons_title(&self, forward: bool) -> &'static str;
    /// « Conseils : », « Conseils propres à ce sens : ».
    fn advice_title(&self, forward: bool) -> &'static str;
    /// The closing sentence: the level is a computed indication, never a guarantee.
    fn disclaimer(&self) -> &'static str;
}
