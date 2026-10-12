//! Server settings models for the LSP settings channel.
//!
//! These types mirror ruff-vscode's settings shape: an array of
//! per-folder settings plus a global fallback, all sent up-front in
//! `initializationOptions`. The array shape lets Zed (which does not
//! support `workspace/configuration` pull) receive full per-folder
//! settings, while VS Code can additionally use the pull mechanism.
//!
//! Only settings the server acts on are modeled. The VS Code extension
//! handles `path`, `importStrategy`, and `logLevel`; the server ignores
//! unknown keys.

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
    /// Mirrors `--min-confidence`: "low", "medium", or "high".
    pub min_confidence: Option<ry_checker::Confidence>,
    /// Mirrors `--baseline`.
    pub baseline: Option<String>,
    /// Whether ry is enabled for this folder. When `false`, the server
    /// analyzes the folder neither in the background nor on demand: it
    /// publishes no diagnostics and returns no inlay hints for it.
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

impl LintSettings {
    /// `config` with every rule list the editor set replacing its own.
    pub fn apply(&self, config: &ry_config::Config) -> ry_config::Config {
        let mut config = config.clone();
        let replace = |target: &mut Vec<String>, value: &Option<Vec<String>>| {
            if let Some(value) = value {
                target.clone_from(value);
            }
        };
        replace(&mut config.error, &self.error);
        replace(&mut config.warn, &self.warn);
        replace(&mut config.ignore, &self.ignore);
        replace(&mut config.extend_select, &self.extend_select);
        if self.select.is_some() {
            config.select.clone_from(&self.select);
        }
        config
    }
}
