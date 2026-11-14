//! Error type of the snow crate.

use std::path::PathBuf;

/// Result alias of this crate.
pub type Result<T, E = Error> = std::result::Result<T, E>;

/// Snow errors.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// Error from the shared foundation (download, CRS, grid, layout).
    #[error(transparent)]
    Common(#[from] skimap_common::Error),

    /// Error from the terrain crate (terrain model, GeoTIFF).
    #[error(transparent)]
    Terrain(#[from] skimap_terrain::Error),

    /// File system error, with the path involved.
    #[error("I/O error on {path}: {source}")]
    Io {
        /// Path being read or written.
        path: PathBuf,
        /// Underlying error.
        #[source]
        source: std::io::Error,
    },

    /// Malformed input file (CSV, shapefile, dBASE).
    #[error("cannot parse {what}: {message}")]
    Parse {
        /// What was being parsed.
        what: String,
        /// Description.
        message: String,
    },

    /// Requested data is not available (date not yet published, massif missing, ...).
    #[error("data not available: {0}")]
    Missing(String),
}

impl Error {
    /// Builds an [`Error::Io`].
    pub fn io(path: impl Into<PathBuf>, source: std::io::Error) -> Self {
        Self::Io {
            path: path.into(),
            source,
        }
    }

    /// Builds an [`Error::Parse`].
    pub fn parse(what: impl Into<String>, message: impl std::fmt::Display) -> Self {
        Self::Parse {
            what: what.into(),
            message: message.to_string(),
        }
    }
}

impl From<serde_json::Error> for Error {
    fn from(e: serde_json::Error) -> Self {
        Self::Common(skimap_common::Error::json("snow", e))
    }
}
