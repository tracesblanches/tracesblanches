//! Shared foundation of the skimap workspace.
//!
//! Owned tickets: F1 (#1) workspace foundation, P1 (#14) shared downloader.
//! Changes to this crate go through the orchestrator (see `docs/architecture.md`).
//!
//! - [`site`]: sites (registry of every domain of France, evaluation sites, data tables).
//! - [`geom`]: WGS84 positions and bounding boxes.
//! - [`crs`]: WGS84 / Lambert-93 / Web Mercator conversions (pure Rust) and XYZ tiles.
//! - [`grid`]: georeferenced raster grid.
//! - [`layout`]: data directory layout (`data/cache`, `data/out`).
//! - [`config`]: configuration from the environment, secrets, rate-limit specs.
//! - [`error`]: shared error type.
//! - [`meta`]: `<layer>.meta.json` schema shared by every layer.
//! - [`names`]: comparison of piste names across sources.
//! - [`output`]: atomic JSON / GeoJSON / bytes writers.
//! - `download` (feature `download`): cached, rate-limited HTTP downloader.
//! - `geotiff` (feature `geotiff`): single-band GeoTIFF: Float32 read and write, UInt8 read, lenient read of any sample type and CRS.

pub mod config;
pub mod crs;
#[cfg(feature = "download")]
pub mod download;
pub mod error;
pub mod geom;
#[cfg(feature = "geotiff")]
pub mod geotiff;
pub mod grid;
pub mod lang;
pub mod layout;
pub mod meta;
pub mod names;
pub mod output;
pub mod site;

pub use error::{Error, Result};
pub use lang::Lang;
