//! Unified diagnostics query for ry frontends.
//!
//! `check_project` encapsulates the `Project` coordination shared by
//! callers: parsed files, user stubs, and workspace context go in;
//! per-file diagnostics come out.

#![forbid(unsafe_code)]

pub mod check;

pub use check::{CheckInput, CheckOutput, check_project, check_project_with_scope_capture};
