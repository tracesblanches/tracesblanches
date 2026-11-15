//! Errors of the pack crate.

use std::path::PathBuf;

/// Result alias of the crate.
pub type Result<T, E = Error> = std::result::Result<T, E>;

/// Errors while reading, building or exporting a pack.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// Error of the shared foundation (I/O, JSON, CRS, download...).
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
    #[error("JSON error in {context}: {source}")]
    Json {
        /// What was parsed or written.
        context: String,
        /// Underlying error.
        #[source]
        source: serde_json::Error,
    },

    /// Invalid or unsupported pack (format, version, missing layer, bad path).
    #[error("pack: {0}")]
    Pack(String),

    /// PMTiles archive error.
    #[error("PMTiles {path}: {message}")]
    PmTiles {
        /// Archive path.
        path: PathBuf,
        /// Error description.
        message: String,
    },

    /// PNG encoding or decoding error.
    #[error("PNG: {0}")]
    Png(String),

    /// GeoJSON error.
    #[error("GeoJSON {context}: {message}")]
    GeoJson {
        /// File or layer.
        context: String,
        /// Error description.
        message: String,
    },

    /// Terrain crate error (GeoTIFF, IGN slope tiles).
    #[cfg(feature = "pipeline")]
    #[error(transparent)]
    Terrain(#[from] skimap_terrain::Error),

    /// Zones crate error (layer, rules).
    #[cfg(feature = "pipeline")]
    #[error(transparent)]
    Zones(#[from] skimap_zones::Error),

    /// SQLite error (MBTiles, SQLiteDB export).
    #[cfg(feature = "pipeline")]
    #[error("SQLite {path}: {source}")]
    Sqlite {
        /// Database path.
        path: PathBuf,
        /// Underlying error.
        #[source]
        source: rusqlite::Error,
    },
}

impl Error {
    /// I/O error with its path.
    pub fn io(path: impl Into<PathBuf>, source: std::io::Error) -> Self {
        Self::Io {
            path: path.into(),
            source,
        }
    }

    /// JSON error with its context.
    pub fn json(context: impl Into<String>, source: serde_json::Error) -> Self {
        Self::Json {
            context: context.into(),
            source,
        }
    }

    /// PMTiles error with the archive path.
    pub fn pmtiles(path: impl Into<PathBuf>, message: impl std::fmt::Display) -> Self {
        Self::PmTiles {
            path: path.into(),
            message: message.to_string(),
        }
    }
}
