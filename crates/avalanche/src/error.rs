//! Error type of the crate.

/// Result alias.
pub type Result<T, E = Error> = std::result::Result<T, E>;

/// Errors of the avalanche layers.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// Error of the shared foundation (download, I/O, JSON, CRS).
    #[error(transparent)]
    Common(#[from] skimap_common::Error),

    /// Error of the Météo-France crate (meta files).
    #[error(transparent)]
    Meteo(#[from] skimap_meteo::Error),

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
