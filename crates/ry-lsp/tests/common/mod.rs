//! Shared helpers for the cross-mode protocol gates.
//!
//! One copy of the `Published` normalization and the CLI/LSP comparison
//! plumbing used by both `protocol.rs` and `protocol_contract.rs`, so the
//! contract stays "LSP published diagnostics equal `ry check` run
//! independently in the same root".

// Each test binary uses a different subset of this module.
#![allow(dead_code)]

use ry_testkit::{ObservedPosition, PositionEncoding, normalize_path, normalize_position};
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::process::Command;

/// A diagnostic normalized to file-relative coordinates so CLI and LSP
/// output can be compared item by item.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct Published {
    pub path: String,
    pub code: String,
    pub severity: String,
    pub message: String,
    pub line: u32,
    pub byte_column: u32,
}

/// The workspace root (this crate's parent's parent), where `target/`
/// and the cargo invocations live.
pub fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .unwrap()
}

/// Build the production `ry` binary once per test process and return its
/// debug path.
pub fn ry_binary() -> PathBuf {
    static BINARY: std::sync::OnceLock<PathBuf> = std::sync::OnceLock::new();
    BINARY
        .get_or_init(|| {
            let status = Command::new(env!("CARGO"))
                .current_dir(workspace_root())
                .args(["build", "--quiet", "-p", "ry-cli"])
                .status()
                .expect("build the production ry binary for the protocol gate");
            assert!(status.success());
            workspace_root().join("target/debug/ry")
        })
        .clone()
}

/// Percent-encoded `file://` URI for `path` (the testkit encoder, which
/// the server itself accepts).
pub fn file_uri(path: &Path) -> String {
    ry_testkit::file_uri(path).unwrap()
}

/// Convert one `ry check --output-format json` entry into a `Published`
/// with byte columns, relative to `root`.
pub fn published_from_cli_value(value: &Value, root: &Path) -> Published {
    let path = value["path"].as_str().unwrap();
    let relative = normalize_path(Path::new(path), root);
    let relative = relative.strip_prefix("./").unwrap_or(&relative).to_string();
    let source = std::fs::read_to_string(root.join(&relative)).unwrap_or_default();
    let scalar = ObservedPosition {
        line: value["line"].as_u64().unwrap() as u32 - 1,
        character: value["column"].as_u64().unwrap() as u32 - 1,
        encoding: PositionEncoding::UnicodeScalar,
    };
    let position = normalize_position(&source, &scalar).unwrap();
    Published {
        path: relative,
        code: value["code"].as_str().unwrap_or("").to_string(),
        severity: value["severity"].as_str().unwrap_or("").to_string(),
        message: value["message"].as_str().unwrap_or("").to_string(),
        line: position.line,
        byte_column: position.character,
    }
}

/// Normalize an LSP `publishDiagnostics` message into sorted `Published`
/// entries so comparison is order-independent.
pub fn published_from_lsp(message: &Value, path: &Path, root: &Path) -> Vec<Published> {
    let relative = normalize_path(path, root);
    let source = std::fs::read_to_string(path).unwrap_or_default();
    let mut diags: Vec<_> = message
        .pointer("/params/diagnostics")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default()
        .iter()
        .map(|value| {
            let start = &value["range"]["start"];
            let position = normalize_position(
                &source,
                &ObservedPosition {
                    line: start["line"].as_u64().unwrap_or(0) as u32,
                    character: start["character"].as_u64().unwrap_or(0) as u32,
                    encoding: PositionEncoding::Utf16,
                },
            )
            .expect("diagnostic start position must normalize");
            Published {
                path: relative.clone(),
                code: value["code"].as_str().unwrap_or("").to_string(),
                severity: match value["severity"].as_u64() {
                    Some(1) => "error",
                    Some(2) => "warning",
                    Some(3) => "info",
                    Some(4) => "hint",
                    _ => "unknown",
                }
                .to_string(),
                message: value["message"].as_str().unwrap_or("").to_string(),
                line: position.line,
                byte_column: position.character,
            }
        })
        .collect();
    diags.sort();
    diags
}
