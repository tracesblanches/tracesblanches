//! Errors of the OSM crate.

use std::path::PathBuf;

/// Result alias of this crate.
pub type Result<T, E = OsmError> = std::result::Result<T, E>;

/// Errors of the OSM extraction and domain census.
#[derive(Debug, thiserror::Error)]
pub enum OsmError {
    /// Error of the shared foundation (download, layout, CRS...).
    #[error(transparent)]
    Common(#[from] skimap_common::Error),

    /// File system error.
    #[error("I/O error on {path}: {source}")]
    Io {
        /// Path involved.
        path: PathBuf,
        /// Underlying error.
        #[source]
        source: std::io::Error,
    },

    /// Malformed OSM XML.
    #[error("malformed OSM XML {path}: {message}")]
    Xml {
        /// File being read (empty when reading from memory).
        path: String,
        /// Description, with the byte position.
        message: String,
    },

    /// Unexpected content of a downloaded document (JSON, CSV, HTML).
    #[error("unexpected content from {source_name}: {message}")]
    Format {
        /// Source name.
        source_name: String,
        /// Description.
        message: String,
    },
}

impl OsmError {
    /// Builds an [`OsmError::Io`].
    pub fn io(path: impl Into<PathBuf>, source: std::io::Error) -> Self {
        Self::Io {
            path: path.into(),
            source,
        }
    }

    /// Builds an [`OsmError::Format`].
    pub fn format(source_name: &str, message: impl Into<String>) -> Self {
        Self::Format {
            source_name: source_name.to_owned(),
            message: message.into(),
        }
    }
}
