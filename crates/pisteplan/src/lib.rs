//! Official piste plans transcribed onto our maps (P22, #121).
//!
//! Without features the crate is the phone-side reader of the transcription
//! ([`transcript`]) plus the plane-to-plane models ([`warp`]), the boosted trees
//! ([`gbdt`]) and their evaluation by domain ([`evaluate`]). The `pipeline` feature adds
//! the registration of a plan: download, rendering and text layer (poppler tools as the
//! input step), reading sheet detected from the plan (`autospec`), colour segmentation,
//! skeleton graph, OCR (ocrs), matching to OSM (labels, RANSAC, non-rigid ICP,
//! junctions), thin-plate spline, transcription; the collector of the plans (`collect`)
//! and the learnt matching (`learn`).

// Unit tests write their inputs directly; the code itself goes through
// `skimap_common::output` (clippy.toml, #164).
#![cfg_attr(test, allow(clippy::disallowed_methods))]

pub mod error;
pub mod evaluate;
pub mod gbdt;
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
pub mod estimate;
#[cfg(feature = "pipeline")]
pub mod image;
#[cfg(feature = "pipeline")]
pub mod input;
#[cfg(feature = "pipeline")]
pub mod labels;
#[cfg(feature = "pipeline")]
pub mod learn;
#[cfg(feature = "pipeline")]
pub mod ocr;
#[cfg(feature = "pipeline")]
pub mod osmref;
#[cfg(feature = "pipeline")]
pub mod perspective;
#[cfg(feature = "pipeline")]
pub mod segment;
#[cfg(feature = "pipeline")]
pub mod skeleton;
#[cfg(feature = "pipeline")]
pub mod spec;
#[cfg(feature = "pipeline")]
pub mod transcribe;

pub use error::{Error, Result};
