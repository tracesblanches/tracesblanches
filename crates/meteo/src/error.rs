//! Error type of the crate.

/// Result alias.
pub type Result<T, E = Error> = std::result::Result<T, E>;

/// Errors of the Météo-France and Open-Meteo clients.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// Error of the shared foundation (download, I/O, JSON).
    #[error(transparent)]
    Common(#[from] skimap_common::Error),

    /// Malformed XML.
    #[error("XML error in {context}: {message}")]
    Xml {
        /// What was being parsed.
        context: String,
        /// Parser message.
        message: String,
    },

    /// Well-formed answer whose content is not what the source documents.
    #[error("unexpected content from {source_name}: {message}")]
    Format {
        /// Source or URL (public form).
        source_name: String,
        /// What is wrong.
        message: String,
    },

    /// The weather of a site could not be made from a run that was read (#174): the
    /// reason, as reported for that site by [`crate::arome::refresh_run`].
    #[error("{0}")]
    Site(String),

    /// A personal-use-only source was requested while `personal_sources` is off.
    #[error("source `{0}` is restricted to personal use; enable `personal_sources` to use it")]
    PersonalSourcesDisabled(&'static str),
}

impl Error {
    pub(crate) fn format(source_name: impl Into<String>, message: impl Into<String>) -> Self {
        Self::Format {
            source_name: source_name.into(),
            message: message.into(),
        }
    }
}
