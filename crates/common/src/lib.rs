//! Shared foundation of the skimap workspace.
//!
//! Owned tickets: F1 (#1) workspace foundation, P1 (#14) shared downloader.
//! Changes to this crate go through the orchestrator (see `docs/architecture.md`).
//!
//! - [`site`]: the two evaluation sites (Cervières, Chapelle-des-Bois).
//! - [`geom`]: WGS84 positions and bounding boxes.
//! - [`crs`]: WGS84 / Lambert-93 / Web Mercator conversions (pure Rust) and XYZ tiles.
//! - [`grid`]: georeferenced raster grid.
//! - [`layout`]: data directory layout (`data/cache`, `data/out`).
//! - [`config`]: configuration from the environment, secrets, rate-limit specs.
//! - [`error`]: shared error type.
//! - [`meta`]: `<layer>.meta.json` schema shared by every layer.
//! - [`output`]: atomic JSON / GeoJSON / bytes writers.
//! - `download` (feature `download`): cached, rate-limited HTTP downloader.
//! - `geotiff` (feature `geotiff`): single-band Float32 GeoTIFF reader and writer.

pub mod config;
pub mod crs;
#[cfg(feature = "download")]
pub mod download;
pub mod error;
pub mod geom;
#[cfg(feature = "geotiff")]
pub mod geotiff;
pub mod grid;
pub mod layout;
pub mod meta;
pub mod output;
pub mod site;

pub use error::{Error, Result};
