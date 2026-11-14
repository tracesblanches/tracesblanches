//! Errors of the zones crate.

/// Result alias.
pub type Result<T, E = Error> = std::result::Result<T, E>;

/// Errors of the zones crate.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// Error of the shared foundation (I/O, HTTP, JSON, CRS).
    #[error(transparent)]
    Common(#[from] skimap_common::Error),

    /// Unexpected content in a source or in a layer.
    #[error("invalid data in {context}: {message}")]
    Data {
        /// What was being read.
        context: String,
        /// What is wrong.
        message: String,
    },

    /// The hand-entered rule file is inconsistent.
    #[error("invalid rules: {0}")]
    Rules(String),
}

impl Error {
    /// Builds an [`Error::Data`].
    pub fn data(context: impl Into<String>, message: impl Into<String>) -> Self {
        Self::Data {
            context: context.into(),
            message: message.into(),
        }
    }
}
