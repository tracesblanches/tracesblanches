//! Errors of the voice pipeline.

use std::path::PathBuf;

/// Result alias.
pub type Result<T, E = Error> = std::result::Result<T, E>;

/// Errors of the clip generation (the phrase bank itself cannot fail).
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// Shared foundation error (paths, sites).
    #[error(transparent)]
    Common(#[from] skimap_common::Error),
    /// File system error.
    #[error("I/O error on {path}: {source}")]
    Io {
        /// Path involved.
        path: PathBuf,
        /// Underlying error.
        #[source]
        source: std::io::Error,
    },
    /// JSON error.
    #[error("JSON error in {path}: {source}")]
    Json {
        /// File involved.
        path: PathBuf,
        /// Underlying error.
        #[source]
        source: serde_json::Error,
    },
    /// Model metadata could not be read.
    #[error(transparent)]
    Gguf(#[from] crate::gguf::GgufError),
    /// pockettts.cpp failed or produced unexpected output.
    #[error("text-to-speech: {0}")]
    Tts(String),
    /// A clip failed a check (tokens, end of speech, silence).
    #[error("clip `{id}`: {message}")]
    Clip {
        /// Clip id.
        id: String,
        /// What is wrong.
        message: String,
    },
    /// ffmpeg failed.
    #[error("audio encoding: {0}")]
    Audio(String),
    /// Missing or invalid configuration.
    #[error("configuration: {0}")]
    Config(String),
}

impl Error {
    /// I/O error on a path.
    pub fn io(path: impl Into<PathBuf>, source: std::io::Error) -> Self {
        Self::Io {
            path: path.into(),
            source,
        }
    }
}
