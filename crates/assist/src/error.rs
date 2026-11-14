//! Errors of the assistant.

use std::path::PathBuf;

/// Result alias.
pub type Result<T> = std::result::Result<T, Error>;

/// An error of the assistant: a layer file that cannot be read or parsed.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// I/O error on a file.
    #[error("{path}: {source}")]
    Io {
        /// File.
        path: PathBuf,
        /// Cause.
        source: std::io::Error,
    },
    /// JSON that does not match the expected layer format.
    #[error("{path}: {message}")]
    Format {
        /// File.
        path: PathBuf,
        /// What is wrong.
        message: String,
    },
    /// Error of the common crate (GeoTIFF, projection, output).
    #[error(transparent)]
    Common(#[from] skimap_common::Error),
    /// A required input is missing.
    #[error("missing input: {0}")]
    Missing(String),
}

impl Error {
    /// Format error.
    pub fn format(path: impl Into<PathBuf>, message: impl std::fmt::Display) -> Self {
        Error::Format {
            path: path.into(),
            message: message.to_string(),
        }
    }
}
