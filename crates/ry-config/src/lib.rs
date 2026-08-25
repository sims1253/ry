#![allow(clippy::collapsible_if)]

//! `ry` project configuration and diagnostic-filtering pipeline.
//!
//! This crate houses the types and logic that both the CLI (`ry check`)
//! and the LSP server (`ry server`) need:
//!
//! - [`Config`], [`EnvironmentConfig`], [`Excludes`]: parsing and
//!   representing `ry.toml`.
//! - [`Baseline`], [`load_baseline`], [`write_baseline_file`],
//!   [`subtract_baseline`]: the diagnostics-baseline machinery.
//! - [`build_filter`]: rule severity remapping from a [`Config`].

pub mod baseline;
pub mod config;

pub use baseline::*;
pub use config::*;
