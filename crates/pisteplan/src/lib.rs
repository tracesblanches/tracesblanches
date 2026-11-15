//! Official piste plans transcribed onto our maps (P22, #121).
//!
//! Without features the crate is the phone-side reader of the transcription
//! ([`transcript`]) plus the plane-to-plane models ([`warp`]). The `pipeline` feature adds
//! the registration of a plan: download, rendering and text layer (poppler tools as the
//! input step), colour segmentation, skeleton graph, OCR (ocrs), matching to OSM
//! (labels, RANSAC, non-rigid ICP, junctions), thin-plate spline, transcription.

pub mod error;
pub mod linalg;
pub mod transcript;
pub mod warp;

#[cfg(feature = "pipeline")]
pub mod align;
#[cfg(feature = "pipeline")]
pub mod arrows;
#[cfg(feature = "pipeline")]
pub mod autospec;
#[cfg(feature = "pipeline")]
pub mod build;
#[cfg(feature = "pipeline")]
pub mod collect;
#[cfg(feature = "pipeline")]
pub mod image;
#[cfg(feature = "pipeline")]
pub mod input;
#[cfg(feature = "pipeline")]
pub mod labels;
#[cfg(feature = "pipeline")]
pub mod ocr;
#[cfg(feature = "pipeline")]
pub mod osmref;
#[cfg(feature = "pipeline")]
pub mod segment;
#[cfg(feature = "pipeline")]
pub mod skeleton;
#[cfg(feature = "pipeline")]
pub mod spec;
#[cfg(feature = "pipeline")]
pub mod transcribe;

pub use error::{Error, Result};
