//! Error type of the core.

use std::path::PathBuf;

/// Result alias using the core [`Error`].
pub type Result<T, E = Error> = std::result::Result<T, E>;

/// Errors of the core algorithms and loaders.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// File system error, with the path involved.
    #[error("I/O error on {path}: {source}")]
    Io {
        /// Path being read or written.
        path: PathBuf,
        /// Underlying error.
        #[source]
        source: std::io::Error,
    },
    /// JSON (de)serialisation error.
    #[error("JSON error in {context}: {source}")]
    Json {
        /// What was being parsed or written.
        context: String,
        /// Underlying error.
        #[source]
        source: serde_json::Error,
    },
    /// Input that does not follow the expected schema.
    #[error("invalid input in {context}: {message}")]
    Format {
        /// What was being read.
        context: String,
        /// What is wrong.
        message: String,
    },
    /// No trail within the snapping distance of a point.
    #[error("no trail within {max_m} m of ({lon:.5}, {lat:.5})")]
    NoTrailNearby {
        /// Longitude of the point.
        lon: f64,
        /// Latitude of the point.
        lat: f64,
        /// Snapping distance, metres.
        max_m: f64,
    },
    /// The two points are not connected under the cost model (other component, one-way
    /// trails, forbidden edges).
    #[error("no route between the two points")]
    NoRoute,
    /// Shared foundation error (projections, grids).
    #[error(transparent)]
    Common(#[from] skimap_common::error::Error),
    /// Off-piste raster error (C3).
    #[error(transparent)]
    Offpiste(#[from] crate::offpiste::Error),
}

impl Error {
    /// Builds an [`Error::Io`].
    pub fn io(path: impl Into<PathBuf>, source: std::io::Error) -> Self {
        Self::Io {
            path: path.into(),
            source,
        }
    }

    /// Builds an [`Error::Json`].
    pub fn json(context: impl Into<String>, source: serde_json::Error) -> Self {
        Self::Json {
            context: context.into(),
            source,
        }
    }

    /// Builds an [`Error::Format`].
    pub fn format(context: impl Into<String>, message: impl Into<String>) -> Self {
        Self::Format {
            context: context.into(),
            message: message.into(),
        }
    }
}
