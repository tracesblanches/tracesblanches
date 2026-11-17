//! The outing asked (#170): one description, shared by the morning of the app, the
//! evening preparation and the ranking of the starts of a pack.
//!
//! [`Outing::DEFAULT`] is the outing the app proposes until the skier chooses another
//! (2 h, classic, any piste, no return time); the app keeps the skier's last choice and
//! the preparation of the evening asks that choice, else the default, so the evening
//! prepares the loops the morning shows. [`Outing::request`] makes the planner's request
//! of an outing from a start: the start time is the best time of the day (A9) from
//! [`EARLIEST`] unless one is given (the ranking of the starts fixes one, so that the
//! light does not decide between starts).

use chrono::{NaiveDate, NaiveTime};
use skimap_common::Lang;
use skimap_common::geom::LonLat;
use skimap_core::graph::Difficulty;
use skimap_core::routing::Style;

use crate::plan::PlanRequest;

/// Earliest start the best time of the day may choose, on a day to come (the app moves
/// it later on the day itself).
pub const EARLIEST: NaiveTime = match NaiveTime::from_hms_opt(8, 0, 0) {
    Some(t) => t,
    None => panic!("8 h"),
};

/// Loops the morning proposes.
pub const MAX_RESULTS: usize = 3;

/// What the skier asks: time, technique, pistes, return time.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Outing {
    /// Time available, minutes (model time).
    pub budget_min: f64,
    /// Technique.
    pub style: Style,
    /// Back before (local), if any.
    pub return_before: Option<NaiveTime>,
    /// Green and blue pistes only.
    pub easy_only: bool,
}

impl Outing {
    /// The outing of the app until the skier chooses another: 2 h, classic, any piste,
    /// no return time.
    pub const DEFAULT: Outing = Outing {
        budget_min: 120.0,
        style: Style::Classic,
        return_before: None,
        easy_only: false,
    };

    /// Label in `lang` (« 2 h, classique »).
    pub fn label(&self, lang: Lang) -> String {
        let minutes = self.budget_min.round().clamp(0.0, f64::from(u32::MAX)) as u32;
        crate::text::of(lang).outing_label(minutes, self.style, self.easy_only, self.return_before)
    }

    /// The planner's request of this outing from `start` (named `start_name`) on `date`,
    /// at `start_time` or the best time of the day from `earliest`.
    pub fn request(
        &self,
        start: LonLat,
        start_name: String,
        date: NaiveDate,
        start_time: Option<NaiveTime>,
        earliest: NaiveTime,
        lang: Lang,
    ) -> PlanRequest {
        PlanRequest {
            label: self.label(lang),
            start,
            start_name,
            date,
            start_time,
            earliest,
            budget_min: self.budget_min,
            style: self.style,
            return_before: self.return_before,
            max_difficulty: self.easy_only.then_some(Difficulty::Easy),
            max_results: MAX_RESULTS,
        }
    }
}

impl Default for Outing {
    fn default() -> Self {
        Self::DEFAULT
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn labels_in_both_languages() {
        assert_eq!(Outing::DEFAULT.label(Lang::Fr), "2 h, classique");
        assert_eq!(Outing::DEFAULT.label(Lang::En), "2 h, classic");
        let o = Outing {
            budget_min: 90.0,
            style: Style::Skating,
            return_before: NaiveTime::from_hms_opt(16, 0, 0),
            easy_only: true,
        };
        assert_eq!(
            o.label(Lang::Fr),
            "1 h 30, skating, vertes et bleues, retour avant 16 h 00"
        );
        assert_eq!(
            o.label(Lang::En),
            "1 h 30 min, skating, green and blue, back before 16:00"
        );
        let short = Outing {
            budget_min: 45.0,
            ..Outing::DEFAULT
        };
        assert_eq!(short.label(Lang::Fr), "45 min, classique");
    }

    #[test]
    fn the_default_request_is_the_app_default() {
        let d = NaiveDate::from_ymd_opt(2026, 11, 18).unwrap();
        let r = Outing::DEFAULT.request(
            LonLat::new(6.0, 45.0),
            "le parking".into(),
            d,
            None,
            EARLIEST,
            Lang::Fr,
        );
        assert_eq!(r.budget_min, 120.0);
        assert_eq!(r.style, Style::Classic);
        assert_eq!(r.return_before, None);
        assert_eq!(r.max_difficulty, None);
        assert_eq!(r.start_time, None);
        assert_eq!(r.earliest, NaiveTime::from_hms_opt(8, 0, 0).unwrap());
        assert_eq!(r.max_results, 3);
    }
}
