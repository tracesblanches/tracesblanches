//! Difficulty of a nordic backcountry ski (SRN) track (SR1, #134).
//!
//! From a track and rasters (elevation, slope, aspect, canopy height, avalanche exposure),
//! zones without network and protected zones:
//!
//! 1. [`profile`]: samples every 10 m along the track with what lies under them;
//! 2. [`analysis`]: measurements in one direction of travel (slope distribution, steepest
//!    passages and their length, forest descents, flats, distance to an exit, network,
//!    exposure, zones, altitude, aspect);
//! 3. [`rules`]: readable rules on the four SRN levels of the alpine clubs (2025):
//!    « facile », « moyen », « difficile », « très difficile », with ranked reasons;
//!    never « facile » across terrain the model labels exposed to avalanches;
//! 4. [`report`]: advice (skins or scales, technique, glide, passages to avoid, turning
//!    back, equipment) and French text, for both directions.
//!
//! The computation needs no pipeline feature and builds for Android. The `pipeline`
//! feature adds the calibration side: LiDAR HD terrain and canopy along a narrow corridor,
//! the avalanche exposure model on the corridor, published ratings, roads.

pub mod analysis;
pub mod field;
#[cfg(feature = "pipeline")]
pub mod pipeline;
pub mod profile;
pub mod report;
pub mod rules;

pub use analysis::{Direction, Metrics, Passage, PassageKind};
pub use profile::{Layers, Profile, Sample};
pub use report::{Advice, Report, assess};
pub use rules::{Conditions, Level, Params, Rating, Reason};

/// Errors of the crate.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// Invalid track.
    #[error("track: {0}")]
    Track(String),
    /// Data problem (pipeline).
    #[error("{0}")]
    Data(String),
}

/// Result alias.
pub type Result<T> = std::result::Result<T, Error>;
