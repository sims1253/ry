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
//!
//! Previously these lived in the `ry-cli` binary crate, where they were
//! unreachable from `ry-lsp`. Extracting them into a library crate lets
//! the LSP server honour the same configuration as the CLI.

pub mod config;
pub mod baseline;

pub use config::*;
pub use baseline::*;
