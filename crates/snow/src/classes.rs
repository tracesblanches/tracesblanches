//! Three snow classes per practice, from the estimated natural snow depth.
//!
//! Thresholds are **proposals** to be validated in the field (T5, #47) and with the
//! trail managers; they apply to the simulated depth of natural snow on open ground:
//!
//! | Practice | 0 insuffisant | 1 limite | 2 suffisant |
//! |---|---|---|---|
//! | piste damée (fond classique / skating) | < 20 cm | 20–40 cm | ≥ 40 cm |
//! | SRN (ski de randonnée nordique hors piste) | < 30 cm | 30–60 cm | ≥ 60 cm |
//!
//! Reasoning: a groomer packs the natural snow to roughly half its depth and needs a
//! continuous base over meadows and tracks, so below ~20 cm grooming is unlikely and
//! between 20 and 40 cm the trail is fragile. Off-trail, the snow has to bury stones,
//! shrubs, stumps and walls on varied ground, so the thresholds are higher. Wind
//! redistribution, forest interception and artificial snow are not in the simulation.

use serde::{Deserialize, Serialize};

/// A practice and its two thresholds (cm).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Practice {
    /// Identifier (file names).
    pub id: &'static str,
    /// French label.
    pub label: &'static str,
    /// Lower bound of class 1 and of class 2, cm.
    pub thresholds_cm: [f32; 2],
}

/// Groomed cross-country trails.
pub const GROOMED: Practice = Practice {
    id: "groomed",
    label: "piste damée",
    thresholds_cm: [20.0, 40.0],
};

/// Nordic ski touring off trails (SRN).
pub const SRN: Practice = Practice {
    id: "srn",
    label: "ski de randonnée nordique (hors piste)",
    thresholds_cm: [30.0, 60.0],
};

/// The practices of the layer.
pub const PRACTICES: [Practice; 2] = [GROOMED, SRN];

/// French labels of the classes 0, 1, 2.
pub const CLASS_LABELS: [&str; 3] = ["insuffisant", "limite", "suffisant"];

impl Practice {
    /// Class 0, 1 or 2 of a depth (cm); `None` for `NaN`.
    pub fn class(&self, depth_cm: f32) -> Option<u8> {
        if depth_cm.is_nan() {
            None
        } else if depth_cm < self.thresholds_cm[0] {
            Some(0)
        } else if depth_cm < self.thresholds_cm[1] {
            Some(1)
        } else {
            Some(2)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classes() {
        assert_eq!(GROOMED.class(0.0), Some(0));
        assert_eq!(GROOMED.class(19.9), Some(0));
        assert_eq!(GROOMED.class(20.0), Some(1));
        assert_eq!(GROOMED.class(40.0), Some(2));
        assert_eq!(SRN.class(45.0), Some(1));
        assert_eq!(SRN.class(f32::NAN), None);
    }
}
