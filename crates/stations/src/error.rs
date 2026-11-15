//! Errors of the stations crate.

/// Result alias of this crate.
pub type Result<T, E = StationsError> = std::result::Result<T, E>;

/// Errors of the stations layer.
#[derive(Debug, thiserror::Error)]
pub enum StationsError {
    /// Error of the shared foundation (download, layout...).
    #[error(transparent)]
    Common(#[from] skimap_common::Error),

    /// Error of the local OSM extracts (X5).
    #[error(transparent)]
    Osm(#[from] skimap_osm::OsmError),

    /// Unexpected content of a downloaded or input document.
    #[error("unexpected content from {source_name}: {message}")]
    Format {
        /// Source name.
        source_name: String,
        /// Description.
        message: String,
    },

    /// A required input layer is missing.
    #[error("missing input: {0}")]
    Input(String),
}

impl StationsError {
    /// Builds a [`StationsError::Format`].
    pub fn format(source_name: &str, message: impl Into<String>) -> Self {
        Self::Format {
            source_name: source_name.to_owned(),
            message: message.into(),
        }
    }
}
