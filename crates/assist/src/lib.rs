//! Assistant features built on the core and the pack data.
//!
//! Owned tickets, one module each so that agents can work in parallel:
//! - [`briefing`]: A1 (#88) daily briefing, where to ski today.
//! - [`wax`]: A2 (#89) wax recommendation.
//! - [`snow_quality`]: A3 (#90) snow quality per hour and per aspect.
//! - [`crowd`]: A4 (#91) crowd forecast.
//! - [`suggest`]: A5 (#92) route suggestion from available time, level and technique.
//! - [`equipment`]: A6 (#93) equipment list for the outing.
//! - [`synthesis`]: A7 (#94) plain-language summary of the day's risks and rules.
//! - [`alerts`]: A8 (#95) condition-change alerts during the outing.
//! - [`best_time`]: A9 (#96) best time to leave.
//! - [`recap`]: A10 (#97) outing recap.
//! - [`questions`]: A11 (#98) offline free-form questions, bounded intent classification.
//!
//! Texts in the language of the app: [`text`] (one module per language, VO6).
//!
//! Shared by the modules: [`layers`] (typed readers of the layer outputs), [`inputs`] (all
//! layers of a site), [`sun`] (sun position and irradiance), [`error`]. [`layer::build`]
//! writes the `assist` layer (`data/out/<site>/assist/`).
//!
//! Never phrase any output as "safe" ("sans danger"). Runs on the phone: no networking.

pub mod alerts;
pub mod best_time;
pub mod briefing;
pub mod crowd;
pub mod equipment;
pub mod error;
pub mod inputs;
pub mod layer;
pub mod layers;
pub mod presets;
pub mod questions;
pub mod recap;
pub mod snow_quality;
pub mod suggest;
pub mod sun;
pub mod synthesis;
pub mod text;
pub mod wax;

pub use layer::{Options, Output, build};
