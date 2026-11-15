//! Errors of the SRN crate.

use std::path::PathBuf;

/// Result alias of this crate.
pub type Result<T, E = SrnError> = std::result::Result<T, E>;

/// Errors of the SRN census.
#[derive(Debug, thiserror::Error)]
pub enum SrnError {
    /// Error of the shared foundation (download, layout, CRS...).
    #[error(transparent)]
    Common(#[from] skimap_common::Error),

    /// Error of the OSM crate (planet extract).
    #[error(transparent)]
    Osm(#[from] skimap_osm::OsmError),

    /// File system error.
    #[error("I/O error on {path}: {source}")]
    Io {
        /// Path involved.
        path: PathBuf,
        /// Underlying error.
        #[source]
        source: std::io::Error,
    },

    /// Unexpected content of a downloaded or local document.
    #[error("unexpected content from {source_name}: {message}")]
    Format {
        /// Source name.
        source_name: String,
        /// Description.
        message: String,
    },
}

impl SrnError {
    /// Builds an [`SrnError::Io`].
    pub fn io(path: impl Into<PathBuf>, source: std::io::Error) -> Self {
        Self::Io {
            path: path.into(),
            source,
        }
    }

    /// Builds an [`SrnError::Format`].
    pub fn format(source_name: &str, message: impl Into<String>) -> Self {
        Self::Format {
            source_name: source_name.to_owned(),
            message: message.into(),
        }
    }
}
