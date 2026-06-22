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
//! For list fields (`error`, `warn`, `ignore`) CLI values are APPENDED
//! to the config file's lists rather than replacing them, matching ty's
//! `--error` / `--warn` / `--ignore` semantics. Scalar fields are
//! overridden only when the CLI explicitly sets them (detected via
//! clap's `ValueSource`), so a config `error-on-warning = true` still
//! takes effect when the user runs `ry check` with no flags.

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// Default output format when neither the config file nor the CLI
/// specifies one. Matches the CLI's pre-config default.
pub const DEFAULT_OUTPUT_FORMAT: &str = "concise";

/// The on-disk filename ry looks for.
pub const CONFIG_FILENAME: &str = "ry.toml";

/// Parsed contents of a `ry.toml` project config file.
///
/// The schema is intentionally minimal and conservative; we can add
/// fields later without breaking existing configs. Unknown fields are
/// rejected (`#[serde(deny_unknown_fields)]`) so typos surface
/// immediately rather than being silently ignored.
///
/// Field names use snake_case in Rust and kebab-case on disk; the
/// kebab-case aliases (e.g. `error-on-warning`, `output-format`) are
/// declared per-field via `#[serde(alias = "...")]`. The struct-level
/// `#[serde(default)]` lets users omit any subset of fields, with each
/// missing field falling back to its documented default.
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
    /// Exclude patterns (gitignore-style). Default: empty.
    pub exclude: Vec<String>,
    /// Output format. Default: "concise".
    #[serde(alias = "output-format", default = "default_output_format")]
    pub output_format: String,
    /// Verbosity count (cumulative with -v). Default: 0.
    pub verbose: u8,
    /// Quiet count (cumulative with -q). Default: 0.
    pub quiet: u8,
    /// Reserved for future use; accepted but currently ignored.
    #[serde(alias = "r-version")]
    pub r_version: Option<String>,
}

impl Default for Config {
    /// Built-in defaults. Mirrors `Config::defaults()` so callers can
    /// use either spelling interchangeably, and so a struct-literal
    /// `Config { ..Config::default() }` picks up the right output
    /// format rather than the empty string.
    fn default() -> Self {
        Self::defaults()
    }
}

/// serde default for `Config::output_format`. Kept as a free function
/// because `#[serde(default = "...")]` requires a path, not a closure.
/// Without this, the struct-level `#[serde(default)]` would fill a
/// missing `output-format` with the `String` default (empty string)
/// rather than `"concise"`.
fn default_output_format() -> String {
    DEFAULT_OUTPUT_FORMAT.to_string()
}

impl Config {
    /// Built-in defaults. Equivalent to `Config::default()` but named
    /// for symmetry with the spec and for callers that want to be
    /// explicit about "no config file present".
    pub fn defaults() -> Self {
        Self {
            error_on_warning: false,
            exit_zero: false,
            error: Vec::new(),
            warn: Vec::new(),
            ignore: Vec::new(),
            exclude: Vec::new(),
            output_format: DEFAULT_OUTPUT_FORMAT.to_string(),
            verbose: 0,
            quiet: 0,
            r_version: None,
        }
    }

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
        let cfg: Self = toml::from_str(&text).map_err(|source| ConfigError::Parse {
            path: path.to_path_buf(),
            source,
        })?;
        Ok(cfg)
    }

    /// Walk up from `start` looking for a `ry.toml`. Stops at the first
    /// ancestor directory that contains one.
    ///
    /// `start` is resolved to an absolute path first; if `start` is a
    /// file rather than a directory, discovery begins from its parent.
    /// Symlinks are not followed during the upward walk (we use plain
    /// `Path::parent` iteration rather than `canonicalize`, so a
    /// symlinked `ry.toml` in a real directory is still found but the
    /// walk itself does not chase symlinks).
    ///
    /// Returns `Ok(Some((path, config)))` on the first match,
    /// `Ok(None)` if the filesystem root is reached without finding a
    /// `ry.toml`. Permission errors during `load_from_dir` propagate as
    /// `Err`; transient "does not exist" results from probing each
    /// ancestor are folded into the walk.
    pub fn discover(start: &Path) -> Result<Option<(PathBuf, Self)>, ConfigError> {
        // Resolve to an absolute path. We avoid `canonicalize` here so
        // that the walk does not silently resolve symlinked parents;
        // `std::env::current_dir` gives us the anchor for relative
        // inputs without dereferencing the path's own components.
        let abs = if start.is_absolute() {
            start.to_path_buf()
        } else {
            std::env::current_dir()
                .unwrap_or_else(|_| PathBuf::from("."))
                .join(start)
        };
        let mut dir: &Path = if abs.is_file() {
            abs.parent().unwrap_or(Path::new("."))
        } else {
            abs.as_path()
        };
        loop {
            if let Some(cfg) = Self::load_from_dir(dir)? {
                return Ok(Some((dir.join(CONFIG_FILENAME), cfg)));
            }
            match dir.parent() {
                Some(parent) => dir = parent,
                None => return Ok(None),
            }
        }
    }

    /// Merge CLI overrides into this config, returning a new `Config`.
    ///
    /// List fields (`error`, `warn`, `ignore`) have the CLI values
    /// APPENDED to the config file's values. Scalar boolean / string
    /// fields are overridden only when the CLI passes `Some(...)`,
    /// meaning "the user explicitly set this on the command line". A
    /// `None` means "the CLI did not touch this; keep the config value".
    ///
    /// `verbose` and `quiet` are additive: the CLI count is added on
    /// top of the config count, so `verbose = 1` in `ry.toml` plus a
    /// single `-v` flag yields a final count of 2.
    #[allow(clippy::too_many_arguments)]
    pub fn merge_cli(
        self,
        cli_errors: Vec<String>,
        cli_warns: Vec<String>,
        cli_ignores: Vec<String>,
        cli_error_on_warning: Option<bool>,
        cli_exit_zero: Option<bool>,
        cli_output_format: Option<String>,
        cli_verbose: u8,
        cli_quiet: u8,
    ) -> Self {
        let mut errors = self.error;
        errors.extend(cli_errors);
        let mut warns = self.warn;
        warns.extend(cli_warns);
        let mut ignores = self.ignore;
        ignores.extend(cli_ignores);

        let output_format = cli_output_format.unwrap_or(self.output_format);

        Self {
            error_on_warning: cli_error_on_warning.unwrap_or(self.error_on_warning),
            exit_zero: cli_exit_zero.unwrap_or(self.exit_zero),
            error: errors,
            warn: warns,
            ignore: ignores,
            exclude: self.exclude,
            output_format,
            // Saturating add so a config value of 255 plus a CLI flag
            // stays within u8 rather than panicking on overflow.
            verbose: self.verbose.saturating_add(cli_verbose),
            quiet: self.quiet.saturating_add(cli_quiet),
            r_version: self.r_version,
        }
    }
}

/// A compiled set of `exclude` patterns. Patterns are gitignore-style
/// globs matched against the path of each candidate file relative to
/// the directory containing the originating `ry.toml`.
///
/// Constructed once from a `Config` and reused across all path checks
/// so we pay pattern-compilation cost only once per run. The forward
/// slash is used as the path separator on every platform, matching the
/// glob crate's expectations and the documented `ry.toml` schema.
#[derive(Debug, Clone, Default)]
pub struct Excludes {
    patterns: Vec<glob::Pattern>,
}

impl Excludes {
    /// Compile the `exclude` field of a `Config` into a reusable
    /// matcher. Patterns that fail to compile are skipped with a
    /// `tracing::warn!`; an invalid pattern in a config file should
    /// not crash the run, but should be surfaced for the user to fix.
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

    /// Returns true if `relative_path` matches any of the compiled
    /// exclude patterns. The path must be expressed relative to the
    /// directory that owns the `ry.toml`, using forward slashes as
    /// separators.
    pub fn matches(&self, relative_path: &str) -> bool {
        self.patterns.iter().any(|p| p.matches(relative_path))
    }

    /// Returns true if there are no patterns, i.e. nothing can match.
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
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn defaults_match_expectations() {
        let d = Config::defaults();
        assert!(!d.error_on_warning);
        assert!(!d.exit_zero);
        assert!(d.error.is_empty());
        assert!(d.warn.is_empty());
        assert!(d.ignore.is_empty());
        assert!(d.exclude.is_empty());
        assert_eq!(d.output_format, DEFAULT_OUTPUT_FORMAT);
        assert_eq!(d.verbose, 0);
        assert_eq!(d.quiet, 0);
        assert!(d.r_version.is_none());
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
exclude = ["tests/fixtures/**", "**/_snapshots/**"]
output-format = "json"
verbose = 1
quiet = 2
r-version = "4.3"
"#;
        let cfg: Config = toml::from_str(toml).unwrap();
        assert!(cfg.error_on_warning);
        assert!(cfg.exit_zero);
        assert_eq!(cfg.error, vec!["RY001", "RY002"]);
        assert_eq!(cfg.warn, vec!["invalid-arithmetic"]);
        assert_eq!(cfg.ignore, vec!["RY010"]);
        assert_eq!(cfg.exclude, vec!["tests/fixtures/**", "**/_snapshots/**"]);
        assert_eq!(cfg.output_format, "json");
        assert_eq!(cfg.verbose, 1);
        assert_eq!(cfg.quiet, 2);
        assert_eq!(cfg.r_version.as_deref(), Some("4.3"));
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
    fn merge_cli_lists_append() {
        let cfg = Config {
            error: vec!["RY001".to_string()],
            warn: vec!["RY002".to_string()],
            ignore: vec!["RY010".to_string()],
            ..Config::defaults()
        };
        let merged = cfg.merge_cli(
            vec!["RY040".to_string()],
            vec![],
            vec!["RY050".to_string()],
            None,
            None,
            None,
            0,
            0,
        );
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
            ..Config::defaults()
        };
        let merged = cfg.merge_cli(
            Vec::new(),
            Vec::new(),
            Vec::new(),
            Some(true),
            Some(true),
            Some("json".to_string()),
            0,
            0,
        );
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
            ..Config::defaults()
        };
        let merged = cfg.merge_cli(Vec::new(), Vec::new(), Vec::new(), None, None, None, 0, 0);
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
            ..Config::defaults()
        };
        let merged = cfg.merge_cli(Vec::new(), Vec::new(), Vec::new(), None, None, None, 2, 3);
        assert_eq!(merged.verbose, 3);
        assert_eq!(merged.quiet, 4);
    }

    #[test]
    fn merge_cli_verbose_saturates() {
        let cfg = Config {
            verbose: 250,
            ..Config::defaults()
        };
        let merged = cfg.merge_cli(Vec::new(), Vec::new(), Vec::new(), None, None, None, 10, 0);
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
    fn load_from_dir_surfaces_parse_errors() {
        let tmp = TempDir::new().unwrap();
        fs::write(tmp.path().join(CONFIG_FILENAME), "not = valid = toml\n").unwrap();
        let err = Config::load_from_dir(tmp.path()).unwrap_err();
        assert!(matches!(err, ConfigError::Parse { .. }));
    }

    #[test]
    fn discover_returns_none_at_root() {
        // The filesystem root has no ry.toml; discovery from "/" must
        // terminate without looping and without erroring.
        let found = Config::discover(Path::new("/")).unwrap();
        assert!(found.is_none(), "expected no config at /, got {found:?}");
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
            ..Config::defaults()
        };
        let ex = Excludes::from_config(&cfg);
        assert!(!ex.is_empty());
        assert!(ex.matches("tests/fixtures/foo.R"));
        assert!(ex.matches("tests/fixtures/sub/bar.R"));
        assert!(ex.matches("pkg/_snapshots/x.R"));
        assert!(!ex.matches("src/good.R"));
    }

    #[test]
    fn excludes_literal_prefix_matches_descendants() {
        // A bare directory pattern should match anything beneath it;
        // the glob crate treats `tests/fixtures` as a literal, so we
        // additionally document that `**` is the recommended form.
        let ex = Excludes::from_config(&Config {
            exclude: vec!["tests/fixtures/**".to_string()],
            ..Config::defaults()
        });
        assert!(ex.matches("tests/fixtures/a.R"));
    }

    #[test]
    fn excludes_skips_invalid_patterns() {
        // An unbalanced `[` is not a valid glob; it must be skipped
        // rather than panic. The matcher ends up with zero patterns.
        let ex = Excludes::from_config(&Config {
            exclude: vec!["[unclosed".to_string()],
            ..Config::defaults()
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
}
