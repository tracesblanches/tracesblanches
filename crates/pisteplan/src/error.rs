//! Error type of the crate.

use std::path::PathBuf;

/// Result alias.
pub type Result<T, E = Error> = std::result::Result<T, E>;

/// Errors of the piste plan registration.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// Error of the shared foundation (CRS, download, layout, output).
    #[error(transparent)]
    Common(#[from] skimap_common::Error),

    /// JSON (de)serialisation.
    #[error("JSON error in {context}: {source}")]
    Json {
        /// What was being read or written.
        context: String,
        /// Underlying error.
        #[source]
        source: serde_json::Error,
    },

    /// File system error.
    #[error("I/O error on {path}: {source}")]
    Io {
        /// Path involved.
        path: PathBuf,
        /// Underlying error.
        #[source]
        source: std::io::Error,
    },

    /// An external input tool (`pdftoppm`, `pdftotext`) failed or is missing.
    #[error("tool `{tool}` failed: {message}")]
    Tool {
        /// Tool name.
        tool: String,
        /// Description (exit status, stderr).
        message: String,
    },

    /// A fit could not be computed (too few or degenerate points).
    #[error("fit failed: {0}")]
    Fit(String),

    /// Unexpected or unusable data.
    #[error("data error: {0}")]
    Data(String),

    /// A personal-use source was requested without `personal_sources`.
    #[error(
        "the official piste plan belongs to the resort (personal use only): enable personal_sources (SKIMAP_PERSONAL_SOURCES=1)"
    )]
    PersonalSourcesDisabled,
}

impl Error {
    /// I/O error on `path`.
    pub fn io(path: impl Into<PathBuf>, source: std::io::Error) -> Self {
        Self::Io {
            path: path.into(),
            source,
        }
    }
}
