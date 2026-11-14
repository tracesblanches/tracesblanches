//! Errors of the refuges crate.

/// Result alias of this crate.
pub type Result<T, E = RefugesError> = std::result::Result<T, E>;

/// Errors of the huts and shelters layer.
#[derive(Debug, thiserror::Error)]
pub enum RefugesError {
    /// Error of the shared foundation (download, layout, CRS...).
    #[error(transparent)]
    Common(#[from] skimap_common::Error),

    /// Unexpected content of a downloaded document.
    #[error("unexpected content from {source_name}: {message}")]
    Format {
        /// Source name.
        source_name: String,
        /// Description.
        message: String,
    },
}

impl RefugesError {
    /// Builds a [`RefugesError::Format`].
    pub fn format(source_name: &str, message: impl Into<String>) -> Self {
        Self::Format {
            source_name: source_name.to_owned(),
            message: message.into(),
        }
    }
}
