//! Errors of the day plan.

use std::path::PathBuf;

/// Result alias.
pub type Result<T> = std::result::Result<T, Error>;

/// An error of the plan: missing input, unreadable file, plan that does not fit its graph.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// I/O error on a file.
    #[error("{path}: {source}")]
    Io {
        /// File.
        path: PathBuf,
        /// Cause.
        source: std::io::Error,
    },
    /// JSON error.
    #[error("JSON {context}: {source}")]
    Json {
        /// What was read or written.
        context: String,
        /// Cause.
        source: serde_json::Error,
    },
    /// A required input is missing (no graph, no loop for the request...).
    #[error("missing input: {0}")]
    Missing(String),
    /// The plan was built on another trail graph than the one given to the tracker.
    #[error("plan built on graph {plan}, tracker given graph {given}")]
    GraphMismatch {
        /// Fingerprint stored in the plan.
        plan: String,
        /// Fingerprint of the graph given.
        given: String,
    },
    /// A plan that is not consistent (choice out of range, empty route, unknown edge).
    #[error("invalid plan: {0}")]
    Invalid(String),
    /// Error of the assistant (layers).
    #[error(transparent)]
    Assist(#[from] skimap_assist::error::Error),
    /// Error of the core (routing).
    #[error(transparent)]
    Core(#[from] skimap_core::error::Error),
    /// Error of the voice layer (manifest).
    #[error(transparent)]
    Voice(#[from] skimap_voice::Error),
    /// Error of the common crate (layout, output).
    #[error(transparent)]
    Common(#[from] skimap_common::Error),
}
