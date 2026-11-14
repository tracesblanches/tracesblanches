//! Error type of the terrain crate.

use std::path::PathBuf;

/// Result alias of this crate.
pub type Result<T, E = Error> = std::result::Result<T, E>;

/// Terrain errors.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// Error from the shared foundation (download, CRS, grid, I/O).
    #[error(transparent)]
    Common(#[from] skimap_common::Error),

    /// File system error, with the path involved.
    #[error("I/O error on {path}: {source}")]
    Io {
        /// Path being read or written.
        path: PathBuf,
        /// Underlying error.
        #[source]
        source: std::io::Error,
    },

    /// Malformed or unsupported GeoTIFF.
    #[error("GeoTIFF error ({context}): {message}")]
    Tiff {
        /// What was being read or written.
        context: String,
        /// Description.
        message: String,
    },

    /// Malformed or unsupported PNG.
    #[error("PNG error: {0}")]
    Png(String),

    /// A source answered with something unusable (service exception, wrong size, ...).
    #[error("bad data from {source_name}: {message}")]
    BadData {
        /// Source or URL.
        source_name: String,
        /// Description.
        message: String,
    },
}

impl Error {
    /// Builds an [`Error::Io`].
    pub fn io(path: impl Into<PathBuf>, source: std::io::Error) -> Self {
        Self::Io {
            path: path.into(),
            source,
        }
    }

    /// Builds an [`Error::Tiff`].
    pub fn tiff(context: impl Into<String>, message: impl std::fmt::Display) -> Self {
        Self::Tiff {
            context: context.into(),
            message: message.to_string(),
        }
    }
}

impl From<serde_json::Error> for Error {
    fn from(e: serde_json::Error) -> Self {
        Self::Common(skimap_common::Error::json("terrain", e))
    }
}
