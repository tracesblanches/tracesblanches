//! Error type shared by the workspace.
//!
//! Library crates return [`Result`]; binaries may wrap it in `anyhow`. Messages never
//! contain secrets: URLs are always the public form (see [`crate::config::Secret`]).

use std::path::PathBuf;
use std::time::Duration;

/// Result alias using the shared [`Error`].
pub type Result<T, E = Error> = std::result::Result<T, E>;

/// Errors of the shared foundation.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// File system error, with the path involved.
    #[error("I/O error on {path}: {source}")]
    Io {
        /// Path being read or written.
        path: PathBuf,
        /// Underlying error.
        #[source]
        source: std::io::Error,
    },

    /// JSON (de)serialisation error.
    #[error("JSON error in {context}: {source}")]
    Json {
        /// What was being parsed or written.
        context: String,
        /// Underlying error.
        #[source]
        source: serde_json::Error,
    },

    /// Invalid configuration value (environment variable, rate-limit string, ...).
    #[error("configuration error: {0}")]
    Config(String),

    /// Request to a personal-use-only source while `personal_sources` is off
    /// ([`crate::download::SourcePolicy::personal_use_only`]).
    #[error(
        "source `{0}` is personal use only: refused without personal_sources (SKIMAP_PERSONAL_SOURCES=1)"
    )]
    PersonalSourceRefused(String),

    /// Unknown site identifier.
    #[error("unknown site `{0}`")]
    UnknownSite(String),

    /// Coordinate reference system error (definition or transformation).
    #[error("CRS error: {0}")]
    Crs(String),

    /// Raster grid error (shape mismatch, out of bounds, ...).
    #[error("grid error: {0}")]
    Grid(String),

    /// Malformed or unsupported GeoTIFF.
    #[error("GeoTIFF error ({context}): {message}")]
    Tiff {
        /// What was being read or written.
        context: String,
        /// Description.
        message: String,
    },

    /// Transport-level HTTP failure (connection, TLS, timeout) after all retries.
    #[error("HTTP request to {url} failed: {message}")]
    Http {
        /// Public URL (without secrets).
        url: String,
        /// Error description.
        message: String,
    },

    /// The server answered with a non-success status after all retries.
    #[error("HTTP status {status} for {url}")]
    Status {
        /// Public URL (without secrets).
        url: String,
        /// HTTP status code.
        status: u16,
    },

    /// The per-source rate limit would require waiting longer than allowed.
    #[error("rate limit of source `{source_name}` reached; next request allowed in {wait:?}")]
    RateLimited {
        /// Source name.
        source_name: String,
        /// Time until the next request is allowed.
        wait: Duration,
    },

    /// The server kept answering a body that is not what was asked for (an error page or
    /// another service's output with status 200, a raster without georeferencing), after
    /// every reload of [`crate::download::Downloader::fetch_checked`].
    #[error("invalid answer from {url} after {attempts} attempts: {message}")]
    Invalid {
        /// Public URL (without secrets).
        url: String,
        /// Attempts made.
        attempts: u32,
        /// Why the last body was refused.
        message: String,
    },

    /// Offline (cache-only) request for an entry that is not cached.
    #[error("not in cache (offline mode): {url}")]
    NotCached {
        /// Public URL (without secrets).
        url: String,
    },
}

impl Error {
    /// Builds an [`Error::Io`] from a path and an I/O error.
    pub fn io(path: impl Into<PathBuf>, source: std::io::Error) -> Self {
        Self::Io {
            path: path.into(),
            source,
        }
    }

    /// Builds an [`Error::Tiff`] from a context and a message.
    pub fn tiff(context: impl Into<String>, message: impl std::fmt::Display) -> Self {
        Self::Tiff {
            context: context.into(),
            message: message.to_string(),
        }
    }

    /// Builds an [`Error::Json`] from a context and a serde_json error.
    pub fn json(context: impl Into<String>, source: serde_json::Error) -> Self {
        Self::Json {
            context: context.into(),
            source,
        }
    }
}
