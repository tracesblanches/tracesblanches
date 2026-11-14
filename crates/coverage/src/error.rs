//! Errors of the coverage crate.

use std::path::PathBuf;

/// Result alias.
pub type Result<T, E = Error> = std::result::Result<T, E>;

/// Errors of the coverage crate.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// Error of the shared foundation (I/O, HTTP, CRS).
    #[error(transparent)]
    Common(#[from] skimap_common::Error),

    /// Error of the zones crate (French part of the site box, metadata).
    #[error(transparent)]
    Zones(#[from] skimap_zones::Error),

    /// 7z archive error.
    #[error("7z archive {path}: {message}")]
    Archive {
        /// Archive path.
        path: PathBuf,
        /// What went wrong.
        message: String,
    },

    /// SQLite / GeoPackage error.
    #[error("GeoPackage {path}: {message}")]
    Gpkg {
        /// GeoPackage path.
        path: PathBuf,
        /// What went wrong.
        message: String,
    },

    /// Malformed geometry blob.
    #[error("invalid geometry: {0}")]
    Wkb(String),
}

impl Error {
    pub(crate) fn gpkg(path: &std::path::Path, e: impl std::fmt::Display) -> Self {
        Self::Gpkg {
            path: path.to_owned(),
            message: e.to_string(),
        }
    }

    pub(crate) fn archive(path: &std::path::Path, e: impl std::fmt::Display) -> Self {
        Self::Archive {
            path: path.to_owned(),
            message: e.to_string(),
        }
    }
}
