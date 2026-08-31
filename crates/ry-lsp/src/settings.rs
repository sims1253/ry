//! Server settings models for the LSP settings channel.
//!
//! These types mirror ruff-vscode's settings shape: an array of
//! per-folder settings plus a global fallback, all sent up-front in
//! `initializationOptions`. The array shape lets Zed (which does not
//! support `workspace/configuration` pull) receive full per-folder
//! settings, while VS Code can additionally use the pull mechanism.
//!
//! Only settings the server acts on are modeled. Editor-owned settings
//! (`path`, `importStrategy`, `addExecutableToTerminalPath`, `logLevel`,
//! `checkTestFixtures`) are resolved by the extension itself; the server
//! ignores unknown keys, so accepting them here would only pretend to
//! honor them.

use serde::{Deserialize, Serialize};

/// The server-level settings envelope sent by the client in
/// `initializationOptions`, matching ruff-vscode's shape:
/// `{ settings: ISettings[], globalSettings: ISettings }`.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct ServerSettings {
    /// Per-workspace-folder settings. Each entry corresponds to one
    /// workspace folder, in order. For single-root workspaces this
    /// array has one element.
    pub settings: Vec<FolderSettings>,
    /// Global fallback used when no folder-specific entry matches.
    #[serde(default)]
    pub global_settings: FolderSettings,
}

/// Per-folder settings, mirroring ruff-vscode's `ISettings`.
///
/// Every field is `Option<T>` so that "unset" (fall back to `ry.toml`
/// or built-in default) is distinguishable from "set to the default
/// value". This is the precedence rule:
/// editor setting > `ry.toml` > built-in default.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct FolderSettings {
    /// Path to a `ry.toml`, overriding automatic discovery.
    pub configuration: Option<String>,
    /// Lint rule selection and severity overrides.
    pub lint: LintSettings,
    /// Mirrors `--min-confidence`. One of "low", "medium", "high".
    pub min_confidence: Option<String>,
    /// Mirrors `--baseline`.
    pub baseline: Option<String>,
    /// Whether ry is enabled for this folder. When `false`, the server
    /// skips analysis and publishes no diagnostics for the folder.
    pub enable: Option<bool>,
}

/// Lint-specific settings, matching ruff-vscode's `Lint` type.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct LintSettings {
    /// Rules to select (replaces the default set). An explicitly empty
    /// list selects nothing, disabling default-enabled rules.
    pub select: Option<Vec<String>>,
    /// Additional rules to enable on top of the defaults.
    pub extend_select: Option<Vec<String>>,
    /// Rules to suppress.
    pub ignore: Option<Vec<String>>,
    /// Rules to treat as errors.
    pub error: Option<Vec<String>>,
    /// Rules to treat as warnings.
    pub warn: Option<Vec<String>>,
}
