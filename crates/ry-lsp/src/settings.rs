//! Editor settings, in ruff-vscode's shape: per-folder settings plus a
//! global fallback, sent in `initializationOptions`. Zed cannot pull
//! `workspace/configuration`, so this channel must carry everything.
//! Unknown keys, such as the VS Code extension's `path`, are ignored.

use serde::Deserialize;

/// `{ settings: ISettings[], globalSettings: ISettings }`.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct ServerSettings {
    /// One entry per workspace folder, in order.
    pub settings: Vec<FolderSettings>,
    /// Global fallback used when no folder-specific entry matches.
    pub global_settings: FolderSettings,
}

/// Per-folder settings, mirroring ruff-vscode's `ISettings`. An unset
/// field falls back to `ry.toml`, then to the built-in default.
#[derive(Debug, Clone, Default, Deserialize)]
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
    /// `false` turns off diagnostics and inlay hints for the folder.
    pub enable: Option<bool>,
}

/// Matches ruff-vscode's `Lint` type.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct LintSettings {
    /// Replaces the default rule set. An empty list selects nothing.
    pub select: Option<Vec<String>>,
    /// Additional rules to enable on top of the defaults.
    pub extend_select: Option<Vec<String>>,
    pub ignore: Option<Vec<String>>,
    pub error: Option<Vec<String>>,
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
