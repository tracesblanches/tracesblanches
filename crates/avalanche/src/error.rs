//! Error type of the crate.

/// Result alias.
pub type Result<T, E = Error> = std::result::Result<T, E>;

/// Errors of the avalanche layers.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// Error of the shared foundation (download, I/O, JSON, CRS).
    #[error(transparent)]
    Common(#[from] skimap_common::Error),

    /// A layer this one is built from has not been built yet.
    #[error("layer `{layer}` of `{site}` is missing ({path}); build it first")]
    MissingLayer {
        /// Missing layer.
        layer: String,
        /// Site identifier.
        site: String,
        /// File looked for.
        path: std::path::PathBuf,
    },

    /// Content that does not match what the source documents.
    #[error("unexpected content from {source_name}: {message}")]
    Format {
        /// Source.
        source_name: String,
        /// What is wrong.
        message: String,
    },
}

impl Error {
    pub(crate) fn format(source_name: impl Into<String>, message: impl Into<String>) -> Self {
        Self::Format {
            source_name: source_name.into(),
            message: message.into(),
        }
    }
}
