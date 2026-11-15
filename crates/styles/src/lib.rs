//! MapLibre styles of the basemaps by practice (K15, #123): `fond` (groomed trails),
//! `srn` (nordic backcountry) and `guidage` (glance while moving), each in a light and a
//! dark theme, all drawn from the same tiles and layers of the offline pack.
//!
//! - [`mvt`]: minimal vector tile decoder (layers, attributes).
//! - [`inventory`]: what a pack offers to a style (tile layers and their attributes,
//!   GeoJSON layers, rasters, fonts, sprites); [`schema`]: its snapshot in
//!   `styles/pack-schema.json`.
//! - [`palette`], [`generate`]: the six styles; [`expr`]: the expression subset;
//!   [`validate`]: static checks; [`orphan`]: every layer draws something in the packs.
//!
//! The style files live in `styles/` at the repository root (see `styles/README.md`).

pub mod declared;
pub mod expr;
pub mod generate;
pub mod inventory;
pub mod mvt;
pub mod orphan;
pub mod palette;
pub mod schema;
pub mod validate;

use std::path::PathBuf;

/// The `styles/` directory at the repository root.
pub fn styles_dir() -> PathBuf {
    PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/../../styles"))
}

/// Errors of the crate.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// Error of the pack reader.
    #[error(transparent)]
    Pack(#[from] Box<skimap_pack::Error>),
    /// Undecodable tile.
    #[error("{path} tile {tile}: {msg}")]
    Tile {
        /// Archive.
        path: String,
        /// `z/x/y`.
        tile: String,
        /// Decoder message.
        msg: String,
    },
    /// File system error.
    #[error("I/O error on {0}: {1}")]
    Io(String, std::io::Error),
    /// Invalid JSON or unexpected content.
    #[error("{0}: {1}")]
    Json(String, String),
}

impl Error {
    pub(crate) fn pack(e: skimap_pack::Error) -> Self {
        Self::Pack(Box::new(e))
    }
}

/// Result of the crate.
pub type Result<T> = std::result::Result<T, Error>;
