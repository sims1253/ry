//! `ry.toml` project configuration.
//!
//! Discovered by walking up from the current directory (or the path
//! passed to `ry check`) until a `ry.toml` is found. Settings merge
//! with built-in defaults and CLI flags.
//!
//! Precedence (highest to lowest):
//! 1. CLI flags (`--error`, `--ignore`, etc.) override / extend the
//!    config file values.
//! 2. `ry.toml` config file.
//! 3. Built-in defaults.
//!
//! For list fields (`error`, `warn`, `ignore`, `typeshed`) CLI values are
//! APPENDED to the config file's lists rather than replacing them, matching
//! ty's `--error` / `--warn` / `--ignore` semantics. Scalar fields are
//! overridden only when the CLI explicitly sets them (detected via
//! clap's `ValueSource`), so a config `error-on-warning = true` still
//! takes effect when the user runs `ry check` with no flags. The `-v` and
//! `-q` counts add to the config's `verbose` and `quiet`.

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// Default output format when neither the config file nor the CLI
/// specifies one.
pub const DEFAULT_OUTPUT_FORMAT: &str = "full";

/// The on-disk filename ry looks for.
pub const CONFIG_FILENAME: &str = "ry.toml";

/// Default cap on decoded serialized R data (`.rda`/`.RData` inventories and
/// `load()` targets). Must sit above real package `R/sysdata.rda` sizes so
/// binding enumeration runs without per-project configuration (gt ships an
/// ~8 MB decoded sysdata); reading stops at the cap plus one overflow-detection
/// byte, and files past it degrade to a file-stem binding with a user-visible
/// note instead of unbounded decoding. Explicit `max-serialized-bytes` values,
/// including small ones, always win.
const DEFAULT_MAX_SERIALIZED_BYTES: u64 = 16 * 1024 * 1024;
const MAX_SERIALIZED_BYTES: u64 = 256 * 1024 * 1024;

/// Defaults for bounded directory discovery.
const DEFAULT_INDEX_MAX_FILES: u64 = 20_000;
const DEFAULT_INDEX_MAX_FILE_BYTES: u64 = 2 * 1024 * 1024;
const DEFAULT_INDEX_MAX_DEPTH: u64 = 64;

/// Bounded directory discovery limits. Applies to both CLI directory
/// discovery and LSP background indexing so the two modes discover
/// exactly the same file set.
///
/// Each key accepts a positive integer; zero is a configuration error
/// rather than an undocumented "unlimited" sentinel ([`Config::validate`]).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct IndexConfig {
    /// Maximum number of R source files discovered per root. Default: 20,000.
    #[serde(alias = "max-files")]
    pub max_files: u64,
    /// Maximum size in bytes of a single R file to include. Default: 2 MiB.
    #[serde(alias = "max-file-bytes")]
    pub max_file_bytes: u64,
    /// Maximum directory depth to descend from each root. Default: 64.
    #[serde(alias = "max-depth")]
    pub max_depth: u64,
}

impl Default for IndexConfig {
    fn default() -> Self {
        Self {
            max_files: DEFAULT_INDEX_MAX_FILES,
            max_file_bytes: DEFAULT_INDEX_MAX_FILE_BYTES,
            max_depth: DEFAULT_INDEX_MAX_DEPTH,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EnvironmentConfig {
    pub name: String,
    #[serde(default)]
    pub bindings: Vec<String>,
    #[serde(default)]
    pub paths: Vec<String>,
    /// `paths` compiled against the config root. Only [`Config::load_file`]
    /// fills it, so a `Config` from `Default` or plain deserialization has
    /// an empty scope that matches nothing.
    #[serde(skip)]
    pub scope: ScopedPaths,
}

/// Static readers for existing annotation conventions. Each reader is
/// disabled unless a project explicitly adopts it for named source paths.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct AnnotationsConfig {
    pub typehint: TypehintConfig,
}

/// The audited `typehint` 0.1.0 comment convention. Path patterns use the
/// same config-root-relative spelling as `exclude`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct TypehintConfig {
    pub adopt: bool,
    pub version: Option<String>,
    pub paths: Vec<String>,
    /// `paths` compiled against the config root. Only [`Config::load_file`]
    /// fills it, so a `Config` from `Default` or plain deserialization has
    /// an empty scope that matches nothing.
    #[serde(skip)]
    scope: ScopedPaths,
}

/// One ordered per-file rule policy. Later matching tables replace earlier
/// choices for codes they mention; unmatched rules retain the global policy.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct RuleOverrideConfig {
    pub paths: Vec<String>,
    pub error: Vec<String>,
    pub warn: Vec<String>,
    pub ignore: Vec<String>,
    /// `paths` compiled against the config root. Only [`Config::load_file`]
    /// fills it, so a `Config` from `Default` or plain deserialization has
    /// an empty scope that matches nothing.
    #[serde(skip)]
    pub scope: ScopedPaths,
}

/// A strict, reusable config-root-relative glob scope. Both the config root
/// and source use physical filesystem identity: existing paths resolve their
/// symlinks, while an unsaved path retains its unresolved normal suffix after
/// the deepest existing ancestor. A `..` after a missing ancestor is
/// ambiguous and does not match on Unix. Windows non-verbatim paths use
/// native normalization, which removes parent components before lookup.
/// `\` separates components only on Windows; on Unix it is a filename byte.
/// The default scope matches nothing.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ScopedPaths {
    root: Option<PathBuf>,
    patterns: Vec<glob::Pattern>,
}

impl ScopedPaths {
    /// Compile `patterns` against `root`. The error names the failing pattern.
    pub fn new<'a>(
        root: &Path,
        patterns: &'a [String],
    ) -> Result<Self, (&'a String, glob::PatternError)> {
        let patterns = patterns
            .iter()
            .map(|pattern| {
                let native = if cfg!(windows) {
                    pattern.replace('\\', "/")
                } else {
                    pattern.clone()
                };
                glob::Pattern::new(&native).map_err(|error| (pattern, error))
            })
            .collect::<Result<Vec<_>, _>>()?;
        let root = scoped_path_identity(root);
        Ok(Self { root, patterns })
    }

    pub fn is_empty(&self) -> bool {
        self.patterns.is_empty()
    }

    pub fn matches(&self, source: &Path) -> bool {
        let Some(root) = &self.root else {
            return false;
        };
        let Some(source) = scoped_path_identity(source) else {
            return false;
        };
        let Ok(relative) = source.strip_prefix(root) else {
            return false;
        };
        // A lossy replacement character can collide with a real Unicode
        // filename, so an unrepresentable native path cannot match a scope.
        let Some(relative) = relative.to_str() else {
            return false;
        };
        let relative = relative.replace(std::path::MAIN_SEPARATOR, "/");
        self.patterns.iter().any(|pattern| {
            pattern.matches_with(
                &relative,
                glob::MatchOptions {
                    require_literal_separator: true,
                    ..Default::default()
                },
            )
        })
    }
}

/// Whether a native path can safely identify a source named by `display`.
/// A replacement character in an otherwise valid Unicode filename may be
/// genuine, so inspect only those path components for another native entry
/// with the same lossy spelling. An unsaved file is allowed when no existing
/// entry could be confused with it.
pub fn unambiguous_native_display_path(native: &Path, display: &str) -> bool {
    if native.to_str() != Some(display) {
        return false;
    }
    let Ok(absolute) = std::path::absolute(native) else {
        return false;
    };
    for prefix in absolute.ancestors().collect::<Vec<_>>().into_iter().rev() {
        let Some(name) = prefix.file_name() else {
            continue;
        };
        let Some(name_text) = name.to_str() else {
            return false;
        };
        if !name_text.contains('\u{fffd}') {
            continue;
        }
        let Some(parent) = prefix.parent() else {
            return false;
        };
        let entries = match std::fs::read_dir(parent) {
            Ok(entries) => entries,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => break,
            Err(_) => return false,
        };
        let mut exact = false;
        let mut matching = 0;
        for entry in entries {
            let Ok(entry) = entry else {
                return false;
            };
            let entry_name = entry.file_name();
            if entry_name.to_string_lossy() == name_text {
                matching += 1;
                exact |= entry_name == name;
                if matching > 1 {
                    return false;
                }
            }
        }
        if matching != 0 && !exact {
            return false;
        }
    }
    true
}

#[cfg(all(test, unix))]
mod native_display_tests {
    use super::unambiguous_native_display_path;
    use std::os::unix::ffi::OsStringExt;

    #[test]
    fn genuine_unicode_replacement_is_allowed_until_native_collision() {
        let root = tempfile::tempdir().unwrap();
        let unicode = root.path().join("bad\u{fffd}.R");
        std::fs::write(&unicode, "x <- 1L").unwrap();
        let display = unicode.to_str().unwrap();
        assert!(unambiguous_native_display_path(&unicode, display));

        let raw = root
            .path()
            .join(std::ffi::OsString::from_vec(b"bad\xff.R".to_vec()));
        std::fs::write(&raw, "x <- 2L").unwrap();
        assert!(!unambiguous_native_display_path(&unicode, display));
        assert!(!unambiguous_native_display_path(
            &raw,
            &raw.to_string_lossy()
        ));

        std::fs::remove_file(&unicode).unwrap();
        assert!(!unambiguous_native_display_path(&unicode, display));
    }
}

/// Resolve the longest existing prefix with filesystem semantics. This
/// preserves the meaning of `symlink/..` and of final symlink files while
/// still matching unsaved files and virtual descendants under an existing
/// folder. Parent components that remain after platform normalization cannot
/// be resolved after a missing prefix and are declined.
pub fn scoped_path_identity(path: &Path) -> Option<PathBuf> {
    let absolute = std::path::absolute(path).ok()?;
    let mut suffix = Vec::new();
    for ancestor in absolute.ancestors() {
        match ancestor.canonicalize() {
            Ok(mut resolved) => {
                for component in suffix.into_iter().rev() {
                    match component {
                        std::path::Component::Normal(name) => resolved.push(name),
                        std::path::Component::CurDir => {}
                        _ => return None,
                    }
                }
                return Some(resolved);
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                // A dangling symlink exists but its physical target does not.
                // It is not an ordinary unsaved component to append.
                if ancestor
                    .symlink_metadata()
                    .is_ok_and(|metadata| metadata.file_type().is_symlink())
                {
                    return None;
                }
            }
            Err(_) => return None,
        }
        suffix.push(ancestor.components().next_back()?);
    }
    None
}

/// The typehint release whose `check_types()` semantics ry has audited.
pub const AUDITED_TYPEHINT_VERSION: &str = "0.1.0";

impl TypehintConfig {
    fn adoption_is_complete(&self) -> bool {
        self.adopt
            && self.version.as_deref() == Some(AUDITED_TYPEHINT_VERSION)
            && !self.paths.is_empty()
    }

    /// The audited, explicitly adopted source scope. Configs that were not
    /// loaded from a file have no root and adopt nothing.
    pub fn adopted_scope(&self) -> Option<ScopedPaths> {
        self.adoption_is_complete().then(|| self.scope.clone())
    }
}

/// Explicit report-input activation. Reports are never inferred from a file
/// extension alone because executable chunk options need a stated policy.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ReportsConfig {
    pub enabled: bool,
}

/// Parsed contents of a `ry.toml` project config file. Missing fields take
/// their value from [`Config::default`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Config {
    /// Treat warnings as errors. Default: false.
    #[serde(alias = "error-on-warning")]
    pub error_on_warning: bool,
    /// Always exit 0 regardless of diagnostics. Default: false.
    #[serde(alias = "exit-zero")]
    pub exit_zero: bool,
    /// Rules to treat as errors. Default: empty.
    pub error: Vec<String>,
    /// Rules to treat as warnings. Default: empty.
    pub warn: Vec<String>,
    /// Rules to suppress. Default: empty.
    pub ignore: Vec<String>,
    /// Ordered config-root-relative rule severity overrides.
    #[serde(alias = "rule-overrides")]
    pub rule_overrides: Vec<RuleOverrideConfig>,
    /// Replace the default-enabled rule set. `Some([])` disables every rule;
    /// `None` retains the default-enabled set.
    pub select: Option<Vec<String>>,
    /// Add rules to the default or selected rule set.
    #[serde(alias = "extend-select")]
    pub extend_select: Vec<String>,
    /// Exclude patterns (gitignore-style). Default: empty.
    pub exclude: Vec<String>,
    /// Source globs to include despite `.Rbuildignore`, relative to `ry.toml`.
    #[serde(alias = "include-build-ignored")]
    pub include_build_ignored: Vec<String>,
    /// Check R fixture data nested under a package's `tests/` tree.
    /// Default: false.
    #[serde(alias = "check-test-fixtures")]
    pub check_test_fixtures: bool,
    /// Output format. Default: "full".
    #[serde(alias = "output-format")]
    pub output_format: String,
    /// Verbosity count (cumulative with -v). Default: 0.
    pub verbose: u8,
    /// Quiet count (cumulative with -q). Default: 0.
    pub quiet: u8,
    /// Packages treated as loaded for NSE gating, as if the script
    /// began with `library(<pkg>)` for each entry. Lets users declare
    /// their dependencies in `ry.toml` so dplyr-style NSE verbs resolve
    /// without an inline `library(dplyr)` in every file. Default: empty.
    pub packages: Vec<String>,
    /// Names supplied dynamically by the host application or by an
    /// unresolvable `load()`. These become opaque project bindings without
    /// suppressing diagnostics for other names.
    pub globals: Vec<String>,
    #[serde(alias = "max-serialized-bytes")]
    pub max_serialized_bytes: u64,
    pub environments: Vec<EnvironmentConfig>,
    /// Explicit adoption of source annotation conventions.
    pub annotations: AnnotationsConfig,
    /// Opt-in static analysis of ordinary R chunks in R Markdown and Quarto.
    pub reports: ReportsConfig,
    /// Runtime typeshed directories. Relative entries are anchored at the
    /// directory containing this configuration file.
    pub typeshed: Vec<PathBuf>,
    /// Baseline file. Relative paths are anchored at the config directory.
    pub baseline: Option<PathBuf>,
    /// Bounded directory discovery limits.
    pub index: IndexConfig,
    /// Absolute directory containing the loaded `ry.toml`; `None` for
    /// built-in defaults. Relative config paths are anchored here.
    #[serde(skip)]
    pub root: Option<PathBuf>,
}

impl Default for Config {
    /// Built-in defaults, as if no config file were present.
    fn default() -> Self {
        Self {
            error_on_warning: false,
            exit_zero: false,
            error: Vec::new(),
            warn: Vec::new(),
            ignore: Vec::new(),
            rule_overrides: Vec::new(),
            select: None,
            extend_select: Vec::new(),
            exclude: Vec::new(),
            include_build_ignored: Vec::new(),
            check_test_fixtures: false,
            output_format: DEFAULT_OUTPUT_FORMAT.to_string(),
            verbose: 0,
            quiet: 0,
            packages: Vec::new(),
            globals: Vec::new(),
            max_serialized_bytes: DEFAULT_MAX_SERIALIZED_BYTES,
            environments: Vec::new(),
            annotations: AnnotationsConfig::default(),
            reports: ReportsConfig::default(),
            typeshed: Vec::new(),
            baseline: None,
            index: IndexConfig::default(),
            root: None,
        }
    }
}

impl Config {
    /// Try to load a `ry.toml` from the given directory.
    ///
    /// Returns `Ok(Some(config))` if the file exists and parses
    /// successfully, `Ok(None)` if no `ry.toml` is present in `dir`,
    /// and `Err` on read or parse errors. The directory itself must
    /// exist; a missing directory is treated the same as a missing
    /// file (`Ok(None)`) so that discovery can probe ancestors without
    /// distinguishing "no config" from "no directory".
    pub fn load_from_dir(dir: &Path) -> Result<Option<Self>, ConfigError> {
        let path = dir.join(CONFIG_FILENAME);
        if !path.exists() {
            return Ok(None);
        }
        Self::load_file(&path).map(Some)
    }

    /// Read and parse a specific `ry.toml` file.
    pub fn load_file(path: &Path) -> Result<Self, ConfigError> {
        let text = std::fs::read_to_string(path).map_err(|source| ConfigError::Read {
            path: path.to_path_buf(),
            source,
        })?;
        let mut cfg: Self = toml::from_str(&text).map_err(|source| ConfigError::Parse {
            path: path.to_path_buf(),
            source,
        })?;
        let parent = path
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty());
        let root = std::path::absolute(parent.unwrap_or(Path::new("."))).map_err(|source| {
            ConfigError::Read {
                path: path.to_path_buf(),
                source,
            }
        })?;
        let scope = |key: String, patterns: &[String]| {
            ScopedPaths::new(&root, patterns).map_err(|(pattern, source)| {
                ConfigError::InvalidPattern {
                    path: path.to_path_buf(),
                    key,
                    pattern: pattern.clone(),
                    source,
                }
            })
        };
        for profile in &mut cfg.environments {
            profile.scope = scope(
                format!("environments `{}` paths", profile.name),
                &profile.paths,
            )?;
        }
        for (index, table) in cfg.rule_overrides.iter_mut().enumerate() {
            if table.paths.is_empty() {
                return Err(ConfigError::EmptyRuleOverridePaths {
                    path: path.to_path_buf(),
                    index: index + 1,
                });
            }
            table.scope = scope(format!("rule-overrides #{} paths", index + 1), &table.paths)?;
        }
        scope("include-build-ignored".into(), &cfg.include_build_ignored)?;
        cfg.annotations.typehint.scope = scope(
            "annotations.typehint paths".into(),
            &cfg.annotations.typehint.paths,
        )?;
        if cfg.annotations.typehint.adopt && !cfg.annotations.typehint.adoption_is_complete() {
            return Err(ConfigError::InvalidTypehintAdoption {
                path: path.to_path_buf(),
            });
        }
        for dir in &mut cfg.typeshed {
            if dir.is_relative() {
                *dir = root.join(&*dir);
            }
        }
        if let Some(baseline) = &mut cfg.baseline
            && baseline.is_relative()
        {
            *baseline = root.join(&*baseline);
        }
        cfg.validate_limits(path)?;
        cfg.root = Some(root);
        Ok(cfg)
    }

    /// Reject resource limits outside their documented ranges.
    fn validate_limits(&self, path: &Path) -> Result<(), ConfigError> {
        if !(1..=MAX_SERIALIZED_BYTES).contains(&self.max_serialized_bytes) {
            return Err(ConfigError::InvalidSerializedLimit {
                path: path.to_path_buf(),
            });
        }
        for (field, value) in [
            ("index.max-files", self.index.max_files),
            ("index.max-file-bytes", self.index.max_file_bytes),
            ("index.max-depth", self.index.max_depth),
        ] {
            if value == 0 {
                return Err(ConfigError::InvalidIndex {
                    path: path.to_path_buf(),
                    field,
                });
            }
        }
        Ok(())
    }

    /// Walk up from `start` looking for a `ry.toml`. Stops at the first
    /// ancestor directory that contains one; see [`config_candidates`]
    /// for the probe order.
    ///
    /// Returns `Ok(None)` if the filesystem root is reached without
    /// finding a `ry.toml`. Read and parse errors propagate as `Err`.
    pub fn discover(start: &Path) -> Result<Option<(PathBuf, Self)>, ConfigError> {
        for path in config_candidates(start) {
            if path.exists() {
                return Self::load_file(&path).map(|cfg| Some((path, cfg)));
            }
        }
        Ok(None)
    }
}

/// Every `ry.toml` path [`Config::discover`] probes from `start`, nearest
/// first. A file `start` begins at its parent. A relative `start` joins
/// the working directory without `canonicalize` or folding `..`, so the
/// walk does not resolve symlinks and steps over dotted components as
/// `Path::parent` yields them.
pub fn config_candidates(start: &Path) -> Vec<PathBuf> {
    let abs = if start.is_absolute() {
        start.to_path_buf()
    } else {
        std::env::current_dir()
            .unwrap_or_else(|_| PathBuf::from("."))
            .join(start)
    };
    let first = if abs.is_file() {
        abs.parent().unwrap_or(Path::new("."))
    } else {
        abs.as_path()
    };
    first
        .ancestors()
        .map(|dir| dir.join(CONFIG_FILENAME))
        .collect()
}

/// CLI flag values merged into a loaded `ry.toml` by [`Config::merge_cli`].
#[derive(Debug, Default, Clone)]
pub struct CliOverrides {
    /// Rules to treat as errors (`--error`).
    pub error: Vec<String>,
    /// Rules to treat as warnings (`--warn`).
    pub warn: Vec<String>,
    /// Rules to disable (`--ignore`).
    pub ignore: Vec<String>,
    /// Extra typeshed directories (`--typeshed`).
    pub typeshed: Vec<PathBuf>,
    /// Baseline file (`--baseline`); wins over a config `baseline`.
    pub baseline: Option<PathBuf>,
    /// `--error-on-warning`, when the flag was passed explicitly.
    pub error_on_warning: Option<bool>,
    /// `--exit-zero`, when the flag was passed explicitly.
    pub exit_zero: Option<bool>,
    /// `--output-format`, when the flag was passed explicitly.
    pub output_format: Option<String>,
    /// `-v` count; added to the config's `verbose`.
    pub verbose: u8,
    /// `-q` count; added to the config's `quiet`.
    pub quiet: u8,
}

impl Config {
    /// Merge CLI overrides into this config, following the precedence in
    /// the module docs.
    pub fn merge_cli(mut self, cli: CliOverrides) -> Self {
        self.error.extend(cli.error);
        self.warn.extend(cli.warn);
        self.ignore.extend(cli.ignore);
        self.typeshed.extend(cli.typeshed);
        if let Some(baseline) = cli.baseline {
            self.baseline = Some(baseline);
        }
        if let Some(error_on_warning) = cli.error_on_warning {
            self.error_on_warning = error_on_warning;
        }
        if let Some(exit_zero) = cli.exit_zero {
            self.exit_zero = exit_zero;
        }
        if let Some(output_format) = cli.output_format {
            self.output_format = output_format;
        }
        self.verbose = self.verbose.saturating_add(cli.verbose);
        self.quiet = self.quiet.saturating_add(cli.quiet);
        self
    }
}

/// Compiled `exclude` globs, matched against paths relative to the
/// config root with `/` separators on every platform.
#[derive(Debug, Clone, Default)]
pub struct Excludes {
    patterns: Vec<glob::Pattern>,
}

impl Excludes {
    /// Compile `cfg.exclude`. An invalid pattern is skipped with a warning
    /// rather than failing the run.
    pub fn from_config(cfg: &Config) -> Self {
        let mut patterns = Vec::with_capacity(cfg.exclude.len());
        for raw in &cfg.exclude {
            match glob::Pattern::new(raw) {
                Ok(p) => patterns.push(p),
                Err(e) => {
                    tracing::warn!(pattern = %raw, error = %e, "skipping invalid exclude pattern");
                }
            }
        }
        Self { patterns }
    }

    pub fn matches(&self, relative_path: &str) -> bool {
        self.patterns.iter().any(|p| p.matches(relative_path))
    }

    pub fn is_empty(&self) -> bool {
        self.patterns.is_empty()
    }
}

/// Errors that can occur while reading or parsing a `ry.toml`.
#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    /// The file could not be read from disk.
    #[error("config file {path} could not be read: {source}")]
    Read {
        path: PathBuf,
        source: std::io::Error,
    },
    /// The file was read but its contents were not valid TOML or did
    /// not match the `Config` schema (including unknown fields, which
    /// are rejected to catch typos early).
    #[error("config file {path} could not be parsed: {source}")]
    Parse {
        path: PathBuf,
        source: toml::de::Error,
    },
    /// A bounded discovery limit was set to zero, which is a
    /// configuration error.
    #[error(
        "config file {path} has invalid value for {field}: bounded discovery limits must be positive integers, zero is not permitted"
    )]
    InvalidIndex { path: PathBuf, field: &'static str },
    #[error(
        "config file {path} has invalid max-serialized-bytes: expected 1..=268435456 (256 MiB)"
    )]
    InvalidSerializedLimit { path: PathBuf },
    #[error("config file {path} has invalid {key} pattern `{pattern}`: {source}")]
    InvalidPattern {
        path: PathBuf,
        key: String,
        pattern: String,
        source: glob::PatternError,
    },
    #[error(
        "config file {path} must set typehint version = '0.1.0' and nonempty paths when adopt = true"
    )]
    InvalidTypehintAdoption { path: PathBuf },
    #[error("config file {path} has rule-overrides table #{index} without paths")]
    EmptyRuleOverridePaths { path: PathBuf, index: usize },
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn invalid_scoped_globs_are_configuration_errors() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("ry.toml");
        for (toml, key) in [
            ("include-build-ignored = ['[']\n", "include-build-ignored"),
            (
                "[[environments]]\nname = 'broken'\npaths = ['[']\n",
                "environments `broken` paths",
            ),
            (
                "[[rule-overrides]]\npaths = ['R/**', '[']\n",
                "rule-overrides #1 paths",
            ),
            (
                "[annotations.typehint]\npaths = ['[']\n",
                "annotations.typehint paths",
            ),
        ] {
            fs::write(&path, toml).unwrap();
            match Config::load_file(&path) {
                Err(ConfigError::InvalidPattern {
                    key: actual,
                    pattern,
                    ..
                }) => {
                    assert_eq!(actual, key);
                    assert_eq!(pattern, "[");
                }
                other => panic!("{toml}: {other:?}"),
            }
        }
    }

    #[test]
    fn typehint_adoption_requires_an_audited_version_and_path_scope() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("ry.toml");
        fs::write(
            &path,
            "[annotations.typehint]\nadopt = true\npaths = ['R/**']\n",
        )
        .unwrap();
        assert!(matches!(
            Config::load_file(&path),
            Err(ConfigError::InvalidTypehintAdoption { .. })
        ));
        fs::write(
            &path,
            "[annotations.typehint]\nadopt = true\nversion = '0.1.0'\npaths = ['R/**']\n",
        )
        .unwrap();
        let config = Config::load_file(&path).unwrap();
        assert!(config.annotations.typehint.adopt);
        assert_eq!(
            config.annotations.typehint.version.as_deref(),
            Some("0.1.0")
        );
        assert_eq!(config.root.as_deref(), Some(dir.path()));
        let scope = config.annotations.typehint.adopted_scope().unwrap();
        assert!(scope.matches(&dir.path().join("R/code.R")));
        assert!(scope.matches(&dir.path().join("R/sub/code.R")));
        assert!(!scope.matches(&dir.path().join("tests/code.R")));
        assert!(!scope.matches(&dir.path().join("R/../outside.R")));
        assert!(!scope.matches(&dir.path().join("../outside/R/code.R")));

        let mut disabled = config.annotations.typehint;
        disabled.adopt = false;
        assert!(disabled.adopted_scope().is_none());
        disabled.adopt = true;
        disabled.version = Some("future".into());
        assert!(disabled.adopted_scope().is_none());
    }

    #[test]
    fn defaults_match_expectations() {
        let d = Config::default();
        assert!(!d.error_on_warning);
        assert!(!d.exit_zero);
        assert!(d.error.is_empty());
        assert!(d.warn.is_empty());
        assert!(d.ignore.is_empty());
        assert!(d.select.is_none());
        assert!(d.extend_select.is_empty());
        assert!(d.exclude.is_empty());
        assert!(!d.check_test_fixtures);
        assert_eq!(d.output_format, DEFAULT_OUTPUT_FORMAT);
        assert_eq!(d.verbose, 0);
        assert_eq!(d.quiet, 0);
        assert!(d.packages.is_empty(), "packages defaults to empty");
        assert!(d.globals.is_empty(), "globals defaults to empty");
        assert!(d.baseline.is_none());
        // Default and derive(Default) must agree.
        assert_eq!(d, Config::default());
    }

    #[test]
    fn parse_minimal_config() {
        let cfg: Config = toml::from_str("error-on-warning = true\n").unwrap();
        assert!(cfg.error_on_warning);
        assert!(cfg.error.is_empty());
        assert_eq!(cfg.output_format, DEFAULT_OUTPUT_FORMAT);
    }

    #[test]
    fn parse_full_config() {
        let toml = r#"
error-on-warning = true
exit-zero = true
error = ["RY001", "RY002"]
warn = ["invalid-arithmetic"]
ignore = ["RY010"]
select = ["RY002"]
extend-select = ["RY003"]
exclude = ["tests/fixtures/**", "**/_snapshots/**"]
check-test-fixtures = true
output-format = "json"
verbose = 1
quiet = 2
packages = ["dplyr", "tidyverse"]
globals = ["runtime_data", "generated_lookup"]
typeshed = ["stubs"]
baseline = "diagnostics.json"
"#;
        let cfg: Config = toml::from_str(toml).unwrap();
        assert!(cfg.error_on_warning);
        assert!(cfg.exit_zero);
        assert_eq!(cfg.error, vec!["RY001", "RY002"]);
        assert_eq!(cfg.warn, vec!["invalid-arithmetic"]);
        assert_eq!(cfg.ignore, vec!["RY010"]);
        assert_eq!(cfg.select, Some(vec!["RY002".into()]));
        assert_eq!(cfg.extend_select, vec!["RY003"]);
        assert_eq!(cfg.exclude, vec!["tests/fixtures/**", "**/_snapshots/**"]);
        assert!(cfg.check_test_fixtures);
        assert_eq!(cfg.output_format, "json");
        assert_eq!(cfg.verbose, 1);
        assert_eq!(cfg.quiet, 2);
        assert_eq!(cfg.packages, vec!["dplyr", "tidyverse"]);
        assert_eq!(cfg.globals, vec!["runtime_data", "generated_lookup"]);
        assert_eq!(cfg.typeshed, vec![PathBuf::from("stubs")]);
        assert_eq!(cfg.baseline, Some(PathBuf::from("diagnostics.json")));
    }

    #[test]
    fn parse_environment_profiles_and_serialized_byte_cap() {
        let cfg: Config = toml::from_str(
            r#"
max-serialized-bytes = 4096

[[environments]]
name = "shiny-server"
bindings = ["input", "output", "session"]
paths = ["inst/shiny/**"]
"#,
        )
        .unwrap();
        assert_eq!(cfg.max_serialized_bytes, 4096);
        assert_eq!(cfg.environments.len(), 1);
        assert_eq!(cfg.environments[0].name, "shiny-server");
        assert_eq!(cfg.environments[0].bindings, ["input", "output", "session"]);
        assert_eq!(cfg.environments[0].paths, ["inst/shiny/**"]);
    }

    #[test]
    fn rule_override_paths_are_config_root_relative_and_component_bounded() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();
        let config_path = root.join("ry.toml");
        fs::write(
            &config_path,
            "[[rule-overrides]]\npaths = [\"R/*.R\"]\nerror = [\"RY040\"]\n",
        )
        .unwrap();
        let config = Config::load_file(&config_path).unwrap();
        assert_eq!(config.rule_overrides.len(), 1);
        fs::create_dir_all(root.join("R/sub")).unwrap();
        fs::create_dir_all(root.join("scripts")).unwrap();
        fs::write(root.join("R/a.R"), "x <- 1L\n").unwrap();
        let paths = &config.rule_overrides[0].scope;
        assert!(paths.matches(&root.join("R/a.R")));
        assert!(paths.matches(&root.join("R/sub/../a.R")));
        assert!(paths.matches(&root.join("R/sub/../unsaved.R")));
        let dotted_root =
            ScopedPaths::new(&root.join("scripts/.."), &config.rule_overrides[0].paths).unwrap();
        assert!(dotted_root.matches(&root.join("R/a.R")));
        assert!(!paths.matches(&root.join("R/deep/a.R")));
        assert!(!paths.matches(&root.join("else/R/a.R")));
        assert!(!paths.matches(&root.join("R/../else/a.R")));
        assert!(!paths.matches(&root.join("../outside/R/a.R")));
    }

    #[cfg(unix)]
    #[test]
    fn scoped_paths_resolve_symlink_parent_before_dotdot() {
        use std::os::unix::fs::symlink;

        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();
        fs::create_dir_all(root.join("else/deep")).unwrap();
        fs::create_dir_all(root.join("else/R")).unwrap();
        fs::create_dir_all(root.join("R")).unwrap();
        fs::write(root.join("else/R/a.R"), "x <- 1L\n").unwrap();
        fs::write(root.join("R/a.R"), "x <- 1L\n").unwrap();
        symlink(root.join("else/deep"), root.join("link")).unwrap();

        let r_only = ScopedPaths::new(root, &["R/**".into()]).unwrap();
        let elsewhere = ScopedPaths::new(root, &["else/R/**".into()]).unwrap();
        let through_link = root.join("link/../R/a.R");
        assert!(!r_only.matches(&through_link));
        assert!(elsewhere.matches(&through_link));
        assert!(!r_only.matches(&root.join("link/../R/unsaved.R")));
        assert!(elsewhere.matches(&root.join("link/../R/unsaved.R")));
    }

    #[cfg(unix)]
    #[test]
    fn scoped_paths_use_one_physical_identity_for_symlinked_root_and_file() {
        use std::os::unix::fs::symlink;

        let temp = tempfile::tempdir().unwrap();
        let real = temp.path().join("real");
        let alias = temp.path().join("alias");
        fs::create_dir_all(real.join("R/sub")).unwrap();
        fs::create_dir_all(real.join("else")).unwrap();
        fs::write(real.join("else/target.R"), "x <- 1L\n").unwrap();
        symlink(&real, &alias).unwrap();

        let in_r = ScopedPaths::new(&alias, &["R/**".into()]).unwrap();
        let elsewhere = ScopedPaths::new(&alias, &["else/**".into()]).unwrap();
        // A config reached through its alias still owns the real R tree.
        assert!(in_r.matches(&alias.join("R/unsaved.R")));
        assert!(in_r.matches(&alias.join("R/sub/../unsaved.R")));
        assert!(in_r.matches(&real.join("R/unsaved.R")));

        symlink(real.join("else/target.R"), real.join("R/a.R")).unwrap();
        for spelling in [alias.join("R/a.R"), alias.join("R/sub/../a.R")] {
            assert!(!in_r.matches(&spelling));
            assert!(elsewhere.matches(&spelling));
        }

        let outside = tempfile::tempdir().unwrap();
        fs::write(outside.path().join("target.R"), "x <- 1L\n").unwrap();
        symlink(outside.path().join("target.R"), real.join("R/external.R")).unwrap();
        assert!(!in_r.matches(&alias.join("R/external.R")));
        assert!(!elsewhere.matches(&alias.join("R/external.R")));
        symlink(outside.path().join("missing.R"), real.join("R/dangling.R")).unwrap();
        assert!(!in_r.matches(&alias.join("R/dangling.R")));
        assert!(!in_r.matches(&alias.join("R/a.R/child.R")));
    }

    // APFS requires valid UTF-8 filenames, so this raw-byte fixture cannot
    // be created on macOS.
    #[cfg(all(unix, not(target_os = "macos")))]
    #[test]
    fn scoped_paths_do_not_replace_non_utf8_filename_bytes() {
        use std::ffi::OsString;
        use std::os::unix::ffi::OsStringExt;

        let temp = tempfile::tempdir().unwrap();
        let raw = temp.path().join(OsString::from_vec(b"bad\xff.R".to_vec()));
        let unicode = temp.path().join("bad\u{fffd}.R");
        fs::write(&raw, "x <- 1L\n").unwrap();
        fs::write(&unicode, "x <- 1L\n").unwrap();
        let paths = ScopedPaths::new(temp.path(), &["bad\u{fffd}.R".into()]).unwrap();
        assert!(!paths.matches(&raw));
        assert!(paths.matches(&unicode));
    }

    #[cfg(unix)]
    #[test]
    fn scoped_paths_keep_unix_backslash_as_a_literal_filename_byte() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();
        fs::create_dir(root.join("back")).unwrap();
        let backslash = root.join("back\\slash.R");
        let slash = root.join("back/slash.R");
        fs::write(&backslash, "x <- 1L\n").unwrap();
        fs::write(&slash, "x <- 1L\n").unwrap();
        let config_path = root.join("ry.toml");
        fs::write(
            &config_path,
            "[[rule-overrides]]\npaths = ['back\\slash.R']\nerror = ['RY040']\n",
        )
        .unwrap();
        let config = Config::load_file(&config_path).unwrap();
        let paths = &config.rule_overrides[0].scope;
        assert!(paths.matches(&backslash));
        assert!(!paths.matches(&slash));
    }

    #[test]
    fn rule_override_without_paths_is_a_config_error() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("ry.toml");
        fs::write(&path, "[[rule-overrides]]\nerror = [\"RY040\"]\n").unwrap();
        assert!(matches!(
            Config::load_file(&path),
            Err(ConfigError::EmptyRuleOverridePaths { index: 1, .. })
        ));
    }

    #[test]
    fn default_serialized_byte_cap_enumerates_real_sysdata_inventories() {
        // gt's R/sysdata.rda decodes to ~8 MB, so the default cap must sit
        // above real package sysdata for tag enumeration to run without
        // per-project configuration. Explicit caps, including small ones,
        // always take precedence.
        assert_eq!(
            Config::default().max_serialized_bytes,
            16 * 1024 * 1024,
            "the default cap must cover real R/sysdata.rda sizes"
        );
    }

    #[test]
    fn parse_invalid_config_returns_error() {
        let res: Result<Config, _> = toml::from_str("error-on-warning = 'not a bool'\n");
        assert!(res.is_err());

        // Syntactically broken TOML.
        let res: Result<Config, _> = toml::from_str("error-on-warning = \n");
        assert!(res.is_err());
    }

    #[test]
    fn parse_unknown_field_returns_error() {
        // `deny_unknown_fields` should catch typos.
        let res: Result<Config, _> = toml::from_str("error-on-warning = true\nbogus = 5\n");
        assert!(res.is_err(), "unknown fields must be rejected, got {res:?}");
    }

    #[test]
    fn merge_cli_typeshed_and_baseline() {
        let cfg = Config {
            typeshed: vec![PathBuf::from("config-stubs")],
            baseline: Some(PathBuf::from("config-baseline.json")),
            ..Config::default()
        };
        let merged = cfg.merge_cli(CliOverrides {
            typeshed: vec![PathBuf::from("cli-stubs")],
            baseline: Some(PathBuf::from("cli-baseline.json")),
            ..CliOverrides::default()
        });
        // Typeshed appends config and CLI entries.
        assert_eq!(
            merged.typeshed,
            vec![PathBuf::from("config-stubs"), PathBuf::from("cli-stubs")]
        );
        // CLI baseline takes precedence over config baseline.
        assert_eq!(merged.baseline, Some(PathBuf::from("cli-baseline.json")));
    }

    #[test]
    fn merge_cli_lists_append() {
        let cfg = Config {
            error: vec!["RY001".to_string()],
            warn: vec!["RY002".to_string()],
            ignore: vec!["RY010".to_string()],
            ..Config::default()
        };
        let merged = cfg.merge_cli(CliOverrides {
            error: vec!["RY040".to_string()],
            ignore: vec!["RY050".to_string()],
            ..CliOverrides::default()
        });
        assert_eq!(merged.error, vec!["RY001", "RY040"]);
        assert_eq!(merged.warn, vec!["RY002"]);
        assert_eq!(merged.ignore, vec!["RY010", "RY050"]);
    }

    #[test]
    fn merge_cli_scalar_overrides() {
        let cfg = Config {
            error_on_warning: false,
            exit_zero: false,
            output_format: "concise".to_string(),
            ..Config::default()
        };
        let merged = cfg.merge_cli(CliOverrides {
            error_on_warning: Some(true),
            exit_zero: Some(true),
            output_format: Some("json".to_string()),
            ..CliOverrides::default()
        });
        assert!(merged.error_on_warning);
        assert!(merged.exit_zero);
        assert_eq!(merged.output_format, "json");
    }

    #[test]
    fn merge_cli_scalar_unsets_when_not_passed() {
        // Config sets true; CLI passes None for both booleans and the
        // output format. The config values must survive.
        let cfg = Config {
            error_on_warning: true,
            exit_zero: true,
            output_format: "json".to_string(),
            verbose: 2,
            quiet: 1,
            ..Config::default()
        };
        let merged = cfg.merge_cli(CliOverrides::default());
        assert!(merged.error_on_warning);
        assert!(merged.exit_zero);
        assert_eq!(merged.output_format, "json");
        assert_eq!(merged.verbose, 2);
        assert_eq!(merged.quiet, 1);
    }

    #[test]
    fn merge_cli_verbose_quiet_are_additive() {
        let cfg = Config {
            verbose: 1,
            quiet: 1,
            ..Config::default()
        };
        let merged = cfg.merge_cli(CliOverrides {
            verbose: 2,
            quiet: 3,
            ..CliOverrides::default()
        });
        assert_eq!(merged.verbose, 3);
        assert_eq!(merged.quiet, 4);
    }

    #[test]
    fn merge_cli_verbose_saturates() {
        let cfg = Config {
            verbose: 250,
            ..Config::default()
        };
        let merged = cfg.merge_cli(CliOverrides {
            verbose: 10,
            ..CliOverrides::default()
        });
        assert_eq!(merged.verbose, 255);
    }

    #[test]
    fn load_from_dir_returns_none_when_absent() {
        let tmp = TempDir::new().unwrap();
        assert!(Config::load_from_dir(tmp.path()).unwrap().is_none());
    }

    #[test]
    fn load_from_dir_reads_file() {
        let tmp = TempDir::new().unwrap();
        fs::write(
            tmp.path().join(CONFIG_FILENAME),
            "error-on-warning = true\n",
        )
        .unwrap();
        let cfg = Config::load_from_dir(tmp.path()).unwrap().unwrap();
        assert!(cfg.error_on_warning);
    }

    #[test]
    fn typeshed_paths_are_resolved_from_config_directory() {
        let tmp = TempDir::new().unwrap();
        fs::write(
            tmp.path().join(CONFIG_FILENAME),
            "typeshed = [\"relative-stubs\", \"/absolute-stubs\"]\n",
        )
        .unwrap();
        let cfg = Config::load_from_dir(tmp.path()).unwrap().unwrap();
        assert_eq!(cfg.typeshed[0], tmp.path().join("relative-stubs"));
        assert_eq!(cfg.typeshed[1], PathBuf::from("/absolute-stubs"));
    }

    #[test]
    fn load_from_dir_surfaces_parse_errors() {
        let tmp = TempDir::new().unwrap();
        fs::write(tmp.path().join(CONFIG_FILENAME), "not = valid = toml\n").unwrap();
        let err = Config::load_from_dir(tmp.path()).unwrap_err();
        assert!(matches!(err, ConfigError::Parse { .. }));
    }

    #[test]
    fn discover_returns_none_at_root() {
        // Discovery must terminate at the filesystem root without
        // looping and without erroring. Uses a TempDir so the test
        // does not depend on the host filesystem state.
        let tmp = TempDir::new().unwrap();
        let nested = tmp.path().join("a").join("b");
        fs::create_dir_all(&nested).unwrap();
        let found = Config::discover(&nested).unwrap();
        assert!(found.is_none(), "expected no config, got {found:?}");
    }

    #[test]
    fn discover_finds_in_ancestor() {
        // grandparent/ry.toml
        // grandparent/parent/child/  <- start here
        let grandparent = TempDir::new().unwrap();
        fs::write(
            grandparent.path().join(CONFIG_FILENAME),
            "error = [\"RY001\"]\n",
        )
        .unwrap();
        let parent = grandparent.path().join("parent");
        let child = parent.join("child");
        fs::create_dir_all(&child).unwrap();

        let (path, cfg) = Config::discover(&child)
            .unwrap()
            .expect("should find ry.toml");
        assert_eq!(path, grandparent.path().join(CONFIG_FILENAME));
        assert_eq!(cfg.error, vec!["RY001"]);
    }

    #[test]
    fn discover_stops_at_first_ancestor() {
        // Two ry.toml files at different levels; the nearer one wins.
        let root = TempDir::new().unwrap();
        fs::write(root.path().join(CONFIG_FILENAME), "error = [\"ROOT\"]\n").unwrap();
        let mid = root.path().join("mid");
        fs::create_dir_all(&mid).unwrap();
        fs::write(mid.join(CONFIG_FILENAME), "error = [\"MID\"]\n").unwrap();

        let (path, cfg) = Config::discover(&mid)
            .unwrap()
            .expect("should find nearest ry.toml");
        assert_eq!(path, mid.join(CONFIG_FILENAME));
        assert_eq!(cfg.error, vec!["MID"]);
    }

    #[test]
    fn discover_steps_over_dotdot_without_folding_it() {
        // From `sub/../project`, `Path::parent` visits `sub/..` and then
        // `sub` itself, which a folded `project` start never visits.
        let tmp = TempDir::new().unwrap();
        let base = tmp.path();
        fs::create_dir_all(base.join("sub")).unwrap();
        fs::create_dir_all(base.join("project/R")).unwrap();
        fs::write(base.join("project/R/use.R"), "x <- 1L\n").unwrap();
        fs::write(base.join("sub").join(CONFIG_FILENAME), "exit-zero = true\n").unwrap();
        let expected = base.join("sub").join(CONFIG_FILENAME);
        for start in [
            base.join("sub/../project"),
            base.join("sub/../project/R/use.R"),
        ] {
            let (path, _) = Config::discover(&start).unwrap().expect("sub/ry.toml");
            assert_eq!(path, expected, "{start:?}");
            let first = config_candidates(&start)
                .into_iter()
                .find(|candidate| candidate.is_file());
            assert_eq!(first, Some(expected.clone()), "{start:?}");
        }
    }

    #[test]
    fn discover_handles_file_start() {
        // If `start` is a file (e.g. a single .R path), discovery
        // should begin from its parent directory.
        let tmp = TempDir::new().unwrap();
        fs::write(tmp.path().join(CONFIG_FILENAME), "exit-zero = true\n").unwrap();
        let file = tmp.path().join("bad.R");
        fs::write(&file, "x <- 1\n").unwrap();

        let (_path, cfg) = Config::discover(&file)
            .unwrap()
            .expect("should find via parent");
        assert!(cfg.exit_zero);
    }

    #[test]
    fn excludes_compile_and_match() {
        let cfg = Config {
            exclude: vec![
                "tests/fixtures/**".to_string(),
                "**/_snapshots/**".to_string(),
            ],
            ..Config::default()
        };
        let ex = Excludes::from_config(&cfg);
        assert!(!ex.is_empty());
        assert!(ex.matches("tests/fixtures/foo.R"));
        assert!(ex.matches("tests/fixtures/sub/bar.R"));
        assert!(ex.matches("pkg/_snapshots/x.R"));
        assert!(!ex.matches("src/good.R"));
    }

    #[test]
    fn excludes_bare_directory_does_not_match_descendants() {
        // The glob crate treats `tests/fixtures` as a literal path, so
        // it does not exclude the directory's contents. `**` is the
        // required form. This test records that constraint.
        let ex = Excludes::from_config(&Config {
            exclude: vec!["tests/fixtures".to_string()],
            ..Config::default()
        });
        assert!(ex.matches("tests/fixtures"));
        assert!(!ex.matches("tests/fixtures/a.R"));
    }

    #[test]
    fn excludes_skips_invalid_patterns() {
        // An unbalanced `[` is not a valid glob; it must be skipped
        // rather than panic. The matcher ends up with zero patterns.
        let ex = Excludes::from_config(&Config {
            exclude: vec!["[unclosed".to_string()],
            ..Config::default()
        });
        assert!(ex.is_empty());
        assert!(!ex.matches("anything"));
    }

    #[test]
    fn excludes_empty_matches_nothing() {
        let ex = Excludes::default();
        assert!(ex.is_empty());
        assert!(!ex.matches("anything.R"));
    }

    #[test]
    fn explicit_empty_select_is_distinct_from_omitted_select() {
        let omitted: Config = toml::from_str("").unwrap();
        let empty: Config = toml::from_str("select = []\n").unwrap();
        assert_eq!(omitted.select, None);
        assert_eq!(empty.select, Some(Vec::new()));
    }

    #[test]
    fn index_defaults_match_spec() {
        let cfg = Config::default();
        assert_eq!(cfg.index.max_files, 20_000);
        assert_eq!(cfg.index.max_file_bytes, 2 * 1024 * 1024);
        assert_eq!(cfg.index.max_depth, 64);
    }

    #[test]
    fn index_config_parses_from_toml() {
        let cfg: Config =
            toml::from_str("[index]\nmax-files = 100\nmax-file-bytes = 4096\nmax-depth = 10\n")
                .unwrap();
        assert_eq!(cfg.index.max_files, 100);
        assert_eq!(cfg.index.max_file_bytes, 4096);
        assert_eq!(cfg.index.max_depth, 10);
    }

    #[test]
    fn resource_limits_are_validated_when_loading_config() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join(CONFIG_FILENAME);
        for field in ["max-files", "max-file-bytes", "max-depth"] {
            fs::write(&path, format!("[index]\n{field} = 0\n")).unwrap();
            let error = Config::load_file(&path).unwrap_err();
            assert!(
                matches!(error, ConfigError::InvalidIndex { .. }),
                "{error:?}"
            );
            assert!(error.to_string().contains(field));
        }
        for value in [
            0,
            1,
            DEFAULT_MAX_SERIALIZED_BYTES,
            MAX_SERIALIZED_BYTES,
            MAX_SERIALIZED_BYTES + 1,
            i64::MAX as u64,
        ] {
            fs::write(&path, format!("max-serialized-bytes = {value}\n")).unwrap();
            let result = Config::load_file(&path);
            if (1..=MAX_SERIALIZED_BYTES).contains(&value) {
                assert_eq!(result.unwrap().max_serialized_bytes, value);
            } else {
                assert!(
                    matches!(result, Err(ConfigError::InvalidSerializedLimit { .. })),
                    "{result:?}"
                );
            }
        }
    }
}
