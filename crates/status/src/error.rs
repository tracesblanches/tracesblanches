//! Error type of the crate.

/// Result alias.
pub type Result<T, E = Error> = std::result::Result<T, E>;

/// Errors of the trail status sources.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// Error of the shared foundation (download, I/O, JSON).
    #[error(transparent)]
    Common(#[from] skimap_common::Error),

    /// Content that does not match what the source documents.
    #[error("unexpected content from {source_name}: {message}")]
    Format {
        /// Source.
        source_name: String,
        /// What is wrong.
        message: String,
    },

    /// The sources of the site are personal-use only and `personal_sources` is off.
    #[error(
        "trail status sources of `{0}` are restricted to personal use; enable `personal_sources`"
    )]
    PersonalSourcesDisabled(String),

    /// The `pistes` layer, whose OSM ways give the geometry of the widget pistes, is
    /// missing.
    #[error("pistes layer missing ({0}): run `skimap pistes` first")]
    MissingPistes(std::path::PathBuf),

    /// No status source for the site.
    #[error("no trail status source for site `{0}`")]
    NoSource(String),
}

impl Error {
    pub(crate) fn format(source_name: impl Into<String>, message: impl Into<String>) -> Self {
        Self::Format {
            source_name: source_name.into(),
            message: message.into(),
        }
    }
}
