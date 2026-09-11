//! Bounded source discovery and package file eligibility.

use super::*;

/// Return whether `path` is eligible to participate in analysis under `config`.
///
/// Matching is always rooted at the configuration/workspace root and uses
/// forward slashes, so callers cannot accidentally give indexing and
/// publication different exclude semantics.
pub fn is_file_eligible(path: &Path, root: &Path, config: &ry_config::Config) -> bool {
    let excludes = ry_config::Excludes::from_config(config);
    is_file_eligible_with_excludes(path, root, &excludes)
}

/// Check file eligibility with an already-compiled exclude matcher.
/// Directory walkers should build this once per owning configuration.
fn is_file_eligible_with_excludes(
    path: &Path,
    root: &Path,
    excludes: &ry_config::Excludes,
) -> bool {
    if excludes.is_empty() {
        return true;
    }
    // Match the workspace entry name, not a canonicalized symlink target: an
    // explicit exclude for `linked.R` must exclude that entry regardless of
    // where it points.
    let relative = path.strip_prefix(root).unwrap_or(path);
    !excludes.matches(&relative.to_string_lossy().replace('\\', "/"))
}

// ─────────────────────────────────────────────────────────────────────────
// Shared, bounded directory discovery (#48)
// ─────────────────────────────────────────────────────────────────────────

/// Bounded directory discovery limits derived from `[index]` in `ry.toml`.
/// Applied identically to CLI directory discovery and LSP background
/// indexing so the two modes discover exactly the same file set.
#[derive(Clone, Copy, Debug)]
pub struct DiscoveryLimits {
    /// Maximum number of R source files discovered per root.
    pub max_files: usize,
    /// Maximum size in bytes of a single R file to include.
    pub max_file_bytes: u64,
    /// Maximum directory depth to descend from each root.
    pub max_depth: usize,
}

impl DiscoveryLimits {
    pub fn from_config(config: &ry_config::Config) -> Self {
        Self {
            max_files: config.index.max_files as usize,
            max_file_bytes: config.index.max_file_bytes,
            max_depth: config.index.max_depth as usize,
        }
    }
}

/// Structured report when a discovery cap is hit.
/// A cap hit is never silent: the caller emits a tracing event,
/// LSP warning, or CLI warning based on this report.
#[derive(Clone, Debug, Default)]
pub struct TruncationReport {
    /// `true` when `max-files` stopped discovery before exhausting the tree.
    pub max_files_hit: bool,
    /// Files omitted because they exceeded `max-file-bytes` (path, size).
    pub oversized_files: Vec<(PathBuf, u64)>,
    /// Directories whose contents were pruned by `max-depth`.
    pub depth_pruned_dirs: Vec<PathBuf>,
}

impl TruncationReport {
    /// Returns `true` when any cap was hit.
    pub fn any_hit(&self) -> bool {
        self.max_files_hit || !self.oversized_files.is_empty() || !self.depth_pruned_dirs.is_empty()
    }
}

/// Paths pruned during discovery. A directory entry covers its whole subtree.
#[derive(Clone, Debug, Default)]
pub struct SkippedPaths {
    pub entries: Vec<(PathBuf, &'static str)>,
    /// Additional entries omitted when the report reached `index.max-files`.
    pub omitted: usize,
}

impl SkippedPaths {
    fn record(&mut self, path: &Path, directory: bool, reason: &'static str, limit: usize) {
        if !directory && !is_source_path(path) {
            return;
        }
        if self.entries.len() < limit {
            self.entries.push((path.to_path_buf(), reason));
        } else {
            self.omitted += 1;
        }
    }
}

struct BuildIgnoredIncludes {
    root: PathBuf,
    patterns: Vec<glob::Pattern>,
}

impl BuildIgnoredIncludes {
    fn matches(&self, path: &Path) -> bool {
        let Ok(path) = std::path::absolute(path) else {
            return false;
        };
        let Ok(relative) = path.strip_prefix(&self.root) else {
            return false;
        };
        let relative = relative.to_string_lossy().replace('\\', "/");
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

fn is_source_path(path: &Path) -> bool {
    matches!(
        path.extension().and_then(|e| e.to_str()),
        Some("R" | "r" | "S" | "s" | "q")
    )
}

/// Result of a bounded directory discovery.
#[derive(Clone, Debug, Default)]
pub struct DiscoveryResult {
    /// Discovered R source file paths (sorted and deduplicated).
    pub files: Vec<PathBuf>,
    /// Structured cap report. Empty when no limit was reached.
    pub truncated: TruncationReport,
    pub skipped: SkippedPaths,
}

/// Discover all eligible R source files under `walk_root`, applying the
/// same eligibility, extension, hidden-directory, symlink, exclude, and
/// test-fixture rules to both CLI and LSP (#48).
///
/// `exclude_root` anchors the compiled `exclude` patterns from `config`.
/// It should be the directory containing the originating `ry.toml`. When
/// `None`, exclude patterns are not applied (matching a missing config).
///
/// Caps (`index.max-files`, `index.max-file-bytes`, `index.max-depth`)
/// bound discovery. A cap hit populates [`TruncationReport`] so the
/// caller can surface a visible warning.
pub fn discover_r_files(
    walk_root: &Path,
    exclude_root: Option<&Path>,
    config: &ry_config::Config,
    check_test_fixtures: bool,
) -> DiscoveryResult {
    // A single file passed directly is always included regardless of
    // package rules: it is the explicit subject of the analysis.
    if walk_root.is_file() {
        return DiscoveryResult {
            files: vec![walk_root.to_path_buf()],
            ..Default::default()
        };
    }
    let limits = DiscoveryLimits::from_config(config);
    let excludes = ry_config::Excludes::from_config(config);
    let has_excludes = !excludes.is_empty();
    let mut files = Vec::new();
    let mut truncated = TruncationReport::default();
    let mut skipped = SkippedPaths::default();
    let include_root = exclude_root.unwrap_or(walk_root);
    let includes = BuildIgnoredIncludes {
        root: std::path::absolute(include_root).unwrap_or_else(|_| include_root.to_path_buf()),
        patterns: config
            .include_build_ignored
            .iter()
            .filter_map(|pattern| glob::Pattern::new(&pattern.replace('\\', "/")).ok())
            .collect(),
    };
    let package_root = walk_root
        .ancestors()
        .find(|ancestor| ancestor.join("DESCRIPTION").is_file())
        .map(Path::to_path_buf);
    let buildignore = package_root
        .as_deref()
        .map(read_rbuildignore)
        .unwrap_or_default();
    discover_recursive(
        walk_root,
        &mut files,
        &mut truncated,
        &mut skipped,
        &includes,
        false,
        package_root.as_deref(),
        &buildignore,
        check_test_fixtures,
        0,
        &limits,
        &excludes,
        has_excludes,
        exclude_root,
    );
    files.sort();
    files.dedup();
    skipped.entries.sort_by(|a, b| a.0.cmp(&b.0));
    DiscoveryResult {
        files,
        truncated,
        skipped,
    }
}

/// Whether `path` is test data under a package's `tests/` tree rather than
/// code the package test runner executes. Testthat only sources runner
/// files at `tests/` root and files with its executable prefixes directly
/// under `tests/testthat/`; deeper R files are data consumed by tests.
///
/// A two-segment `tests/<file>` path is code only for R source names, a
/// three-segment `tests/testthat/<file>` path only for R source names
/// with a testthat executable prefix, and every other `tests/` path is a
/// fixture.
pub(super) fn is_test_fixture(path: &Path) -> bool {
    let Some(root) = path
        .parent()
        .and_then(|parent| parent.ancestors().find(|p| p.join("DESCRIPTION").is_file()))
    else {
        return false;
    };
    let Ok(relative) = path.strip_prefix(root) else {
        return false;
    };
    let components: Vec<&str> = relative
        .components()
        .filter_map(|component| component.as_os_str().to_str())
        .collect();
    if components.first() != Some(&"tests") {
        return false;
    }
    match components.as_slice() {
        [_, file] => !is_r_source_name(file),
        [_, "testthat", file] => !(is_r_source_name(file) && is_testthat_code_name(file)),
        _ => true,
    }
}

/// Whether `name` uses the conventional `.R`/`.r` spelling. testthat and
/// `R CMD check` execute only `.R`/`.r` under `tests/`, so the historical
/// S-dialect spellings (`.S`/`.s`/`.q`) stay discoverable as R source
/// outside `tests/` but classify as fixtures inside it.
pub(super) fn is_r_source_name(name: &str) -> bool {
    std::path::Path::new(name)
        .extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| matches!(extension, "R" | "r"))
}

/// Whether a `tests/testthat/` file name is runner code under testthat's
/// documented contract: `test-`/`test_` test files plus `helper`, `setup`,
/// and `teardown` prefixes. A name merely starting with the letters
/// "test" (`testing.R`, `testthat.R`) is data. testthat's implementation
/// regex `^test.*\.[rR]$` is broader than its docs and would execute a
/// lookalike; ry follows the documented contract, so a lookalike is
/// skipped unless `check_test_fixtures` is enabled.
pub(super) fn is_testthat_code_name(name: &str) -> bool {
    let stem = std::path::Path::new(name)
        .file_stem()
        .and_then(|stem| stem.to_str())
        .unwrap_or(name);
    ["test-", "test_", "helper", "setup", "teardown"]
        .iter()
        .any(|prefix| stem.starts_with(prefix))
}

#[allow(clippy::too_many_arguments)]
fn discover_recursive(
    dir: &Path,
    out: &mut Vec<PathBuf>,
    truncated: &mut TruncationReport,
    skipped: &mut SkippedPaths,
    includes: &BuildIgnoredIncludes,
    inherited_buildignore: bool,
    package_root: Option<&Path>,
    buildignore: &[glob::Pattern],
    check_test_fixtures: bool,
    depth: usize,
    limits: &DiscoveryLimits,
    excludes: &ry_config::Excludes,
    has_excludes: bool,
    exclude_root: Option<&Path>,
) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        // Skip symlinks and entries whose type cannot be classified;
        // following either could make recursive discovery escape the
        // requested tree.
        let Ok(file_type) = entry.file_type() else {
            continue;
        };
        let path = entry.path();
        let directory = file_type.is_dir();
        if file_type.is_symlink() {
            skipped.record(&path, false, "symlink", limits.max_files);
            continue;
        }
        // Apply ry.toml exclude patterns (relative to the config root).
        if has_excludes
            && let Some(anchor) = exclude_root
            && !is_file_eligible_with_excludes(&path, anchor, excludes)
        {
            skipped.record(&path, directory, "ry.toml exclude", limits.max_files);
            continue;
        }
        // Apply .Rbuildignore patterns (relative to the package root).
        let build_ignored = inherited_buildignore
            || package_root.is_some_and(|root| {
                path.ancestors()
                    .take_while(|p| *p != root)
                    .any(|ancestor| is_rbuildignored(root, ancestor, buildignore))
            });
        if build_ignored
            && ((directory && includes.patterns.is_empty())
                || (!directory && !includes.matches(&path)))
        {
            skipped.record(&path, directory, ".Rbuildignore", limits.max_files);
            continue;
        }
        if path.is_dir() {
            if let Some(name) = path.file_name().and_then(|n| n.to_str())
                && (name.starts_with('.')
                    || name == "target"
                    || name == "node_modules"
                    || (name == "renv" && package_root.is_some())
                    || name.ends_with(".Rcheck"))
            {
                skipped.record(
                    &path,
                    true,
                    "hidden or generated directory",
                    limits.max_files,
                );
                continue;
            }
            if package_root.is_some_and(|root| is_excluded_package_directory(root, &path)) {
                skipped.record(&path, true, "package support directory", limits.max_files);
                continue;
            }
            // depth cap prunes further descent.
            if depth >= limits.max_depth {
                truncated.depth_pruned_dirs.push(path);
                continue;
            }
            let (nested_package_root, nested_buildignore) = if path.join("DESCRIPTION").is_file() {
                (Some(path.clone()), read_rbuildignore(&path))
            } else {
                (package_root.map(Path::to_path_buf), buildignore.to_vec())
            };
            discover_recursive(
                &path,
                out,
                truncated,
                skipped,
                includes,
                build_ignored,
                nested_package_root.as_deref(),
                &nested_buildignore,
                check_test_fixtures,
                depth + 1,
                limits,
                excludes,
                has_excludes,
                exclude_root,
            );
        } else if is_source_path(&path) {
            if !check_test_fixtures && is_test_fixture(&path) {
                skipped.record(&path, false, "test fixture", limits.max_files);
                continue;
            }
            // max-files cap.
            if out.len() >= limits.max_files {
                truncated.max_files_hit = true;
                break;
            }
            // max-file-bytes cap.
            if let Ok(metadata) = std::fs::metadata(&path) {
                let size = metadata.len();
                if size > limits.max_file_bytes {
                    truncated.oversized_files.push((path, size));
                    continue;
                }
            }
            out.push(path);
        }
    }
}

/// Read an R `.Rbuildignore` file and translate its conservative regex
/// subset to glob patterns.
fn read_rbuildignore(root: &Path) -> Vec<glob::Pattern> {
    let Ok(contents) = std::fs::read_to_string(root.join(".Rbuildignore")) else {
        return Vec::new();
    };
    contents
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .filter_map(rbuildignore_pattern)
        .collect()
}

/// Translate the conservative regex subset used by conventional
/// `.Rbuildignore` files to the already-depended-on glob matcher.
/// Unsupported PCRE constructs are ignored, as required for patterns
/// our engine cannot compile.
pub fn rbuildignore_pattern(regex: &str) -> Option<glob::Pattern> {
    if regex.contains(['(', ')', '|', '{', '}', '+']) {
        return None;
    }
    let anchored_start = regex.starts_with('^');
    let trailing_backslashes = regex
        .strip_suffix('$')
        .map(|prefix| prefix.chars().rev().take_while(|&ch| ch == '\\').count())
        .unwrap_or(0);
    let anchored_end = regex.ends_with('$') && trailing_backslashes.is_multiple_of(2);
    let body = regex.strip_prefix('^').unwrap_or(regex);
    let body = if anchored_end {
        body.strip_suffix('$').unwrap_or(body)
    } else {
        body
    };
    let mut glob_str = String::new();
    if !anchored_start {
        glob_str.push('*');
    }
    let mut chars = body.chars().peekable();
    while let Some(ch) = chars.next() {
        match ch {
            '\\' => glob_str.push(chars.next()?),
            '.' if chars.peek() == Some(&'*') => {
                chars.next();
                glob_str.push('*');
            }
            '.' => glob_str.push('?'),
            '*' | '?' | '[' | ']' => glob_str.push(ch),
            ch => glob_str.push(ch),
        }
    }
    if !anchored_end {
        glob_str.push('*');
    }
    glob::Pattern::new(&glob_str).ok()
}

/// Whether `path` relative to `package_root` is excluded by
/// `.Rbuildignore`. Files under `R/` or `tests/` are never excluded
/// because they are always part of the package source.
fn is_rbuildignored(root: &Path, path: &Path, patterns: &[glob::Pattern]) -> bool {
    let Ok(relative) = path.strip_prefix(root) else {
        return false;
    };
    if relative.starts_with("R") || relative.starts_with("tests") {
        return false;
    }
    let relative = relative.to_string_lossy().replace('\\', "/");
    patterns.iter().any(|pattern| pattern.matches(&relative))
}

/// Whether a directory relative to a package root should be skipped
/// entirely (reverse-dependency check dirs, compiled source, snapshots).
fn is_excluded_package_directory(package_root: &Path, path: &Path) -> bool {
    let Ok(relative) = path.strip_prefix(package_root) else {
        return false;
    };
    let components: Vec<_> = relative
        .components()
        .filter_map(|component| component.as_os_str().to_str())
        .collect();
    matches!(components.as_slice(), ["revdep"] | ["src"])
        || matches!(components.as_slice(), ["tests", "testthat", "_snaps"])
}

/// Pins the recording conditions of [`collect_dynamic_bindings_stmts`].
/// The environment argument's *presence* is what records, never its
/// value: any `envir`/`env`/`assign.env` expression, or an unnamed
/// positional argument in the environment slot (third for
/// `assign`/`makeActiveBinding`, fourth for `delayedAssign`), makes the
/// target explicit. Bare `assign` records only via the `fn_depth == 0`
/// arm; bare `makeActiveBinding`/`delayedAssign` never do.
#[cfg(test)]
mod shared_tests {
    use super::*;

    /// Runner-code names under testthat's documented contract: both
    /// test-file spellings plus the helper/setup/teardown prefixes. A
    /// name merely starting with the letters "test" does not qualify.
    #[test]
    fn testthat_code_names_follow_the_documented_prefixes() {
        for name in [
            "test-that.R",
            "test_placeholder.r",
            "helper-values.R",
            "helper.R",
            "setup.R",
            "setup-db.R",
            "teardown.R",
            "teardown-cache.r",
        ] {
            assert!(is_testthat_code_name(name), "{name}");
        }
        for name in [
            "testing.R",
            "testthat.R",
            "test.R",
            "data.R",
            "snapshot.txt",
        ] {
            assert!(!is_testthat_code_name(name), "{name}");
        }
    }

    /// Runner classification accepts only the conventional `.R`/`.r`
    /// spellings, so historical S-dialect extensions never classify as
    /// runner code — including directly under `tests/`, where nothing
    /// executes them (fixtures are skipped unless
    /// `check_test_fixtures` is enabled).
    #[test]
    fn runner_classification_requires_r_extension() {
        for name in ["test-x.R", "helper.r", "setup.R"] {
            assert!(is_r_source_name(name), "{name}");
        }
        for name in ["test-x.S", "test-x.s", "test-x.q", "test-x.txt"] {
            assert!(!is_r_source_name(name), "{name}");
        }
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();
        std::fs::write(root.join("DESCRIPTION"), "Package: example\n").unwrap();
        for (relative, fixture) in [
            ("tests/testthat.R", false),
            ("tests/foo.R", false),
            ("tests/foo.r", false),
            ("tests/foo.S", true),
            ("tests/foo.s", true),
            ("tests/foo.q", true),
        ] {
            assert_eq!(is_test_fixture(&root.join(relative)), fixture, "{relative}");
        }
    }

    /// testthat sources `tests/testthat/` runner files into the namespace
    /// clone, so `import(pkg)` names resolve there; the classification must
    /// be exactly the executable-code half of [`is_test_fixture`]'s
    /// three-segment arm. Everything else — `tests/` root scripts (run by
    /// `R CMD check` in the global environment after `library(package)`),
    /// fixture names, deeper paths, historical extensions — stays out.
    #[test]
    fn testthat_runner_files_are_exactly_the_executable_testthat_paths() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();
        std::fs::write(root.join("DESCRIPTION"), "Package: example\n").unwrap();
        for relative in [
            "tests/testthat/test-package.R",
            "tests/testthat/test_package.r",
            "tests/testthat/helper-values.R",
            "tests/testthat/setup-db.R",
            "tests/testthat/teardown.R",
        ] {
            assert!(is_testthat_runner_file(Path::new(relative)), "{relative}");
            assert!(!is_test_fixture(&root.join(relative)), "{relative}");
        }
        for relative in [
            "R/package.R",
            "tests/testthat.R",
            "tests/manual.R",
            "tests/testthat/data.R",
            "tests/testthat/testing.R",
            "tests/testthat/test-legacy.S",
            "tests/testthat/fixtures/input.R",
            "tests/testthat/_snaps/output.R",
            "vignettes/preprint.R",
        ] {
            assert!(!is_testthat_runner_file(Path::new(relative)), "{relative}");
        }
    }

    /// End-to-end pin of the `import(pkg)` extension: a wholesale import's
    /// package lands on the search path ry models for `R/` sources and for
    /// testthat runner files, but not for `tests/` root scripts or fixture
    /// files — those run where the imports environment is not on the
    /// parent chain. The runner file's own `library()` attachments still
    /// apply on top.
    #[test]
    fn wholesale_imports_reach_r_sources_and_testthat_runner_files() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();
        std::fs::write(root.join("DESCRIPTION"), "Package: example\n").unwrap();
        std::fs::write(root.join("NAMESPACE"), "import(rlang)\nexport(run)\n").unwrap();
        for directory in ["R", "tests", "tests/testthat"] {
            std::fs::create_dir_all(root.join(directory)).unwrap();
        }
        std::fs::write(root.join("R/run.R"), "").unwrap();
        std::fs::write(root.join("tests/testthat.R"), "").unwrap();
        std::fs::write(root.join("tests/testthat/test-run.R"), "").unwrap();
        std::fs::write(root.join("tests/testthat/data.R"), "").unwrap();

        let mut parser = ry_core::RParser::new().unwrap();
        let files: Vec<SourceFile> = [
            "R/run.R",
            "tests/testthat.R",
            "tests/testthat/test-run.R",
            "tests/testthat/data.R",
        ]
        .iter()
        .map(|relative| {
            let path = root.join(relative);
            let source = std::fs::read_to_string(&path).unwrap();
            let path = path.to_string_lossy().to_string();
            parser.parse(&path, &source).unwrap()
        })
        .collect();
        let environment = ResolutionEnvironment {
            files: files.iter().collect(),
            user_stubs: &std::collections::BTreeMap::new(),
        };
        let context =
            resolve_workspace_context(root, &ry_config::Config::default(), environment).unwrap();

        let attached_for = |relative: &str| {
            let path = root.join(relative);
            context
                .bare_bindings
                .get(&path.to_string_lossy().to_string())
                .unwrap_or_else(|| panic!("no bindings recorded for {relative}"))
        };
        assert!(
            attached_for("R/run.R").contains("rlang"),
            "R/ sources see import(rlang) names"
        );
        assert!(
            attached_for("tests/testthat/test-run.R").contains("rlang"),
            "testthat runner files execute in the namespace clone"
        );
        assert!(
            !attached_for("tests/testthat.R").contains("rlang"),
            "tests/ root scripts run in the global environment after library()"
        );
        assert!(
            !attached_for("tests/testthat/data.R").contains("rlang"),
            "fixture files are not sourced into the namespace clone"
        );
    }

    #[test]
    fn eligibility_is_rooted_and_separator_independent() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("vendor").join("influence.R");
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        std::fs::write(&file, "x <- 1\n").unwrap();
        let config = ry_config::Config {
            exclude: vec!["vendor/**".into()],
            ..Default::default()
        };
        assert!(!is_file_eligible(&file, dir.path(), &config));
        assert!(is_file_eligible(
            &dir.path().join("keep.R"),
            dir.path(),
            &config
        ));
    }

    #[cfg(unix)]
    #[test]
    fn eligibility_matches_a_symlink_entry_name_not_its_target() {
        use std::os::unix::fs::symlink;

        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("real.R");
        let link = dir.path().join("linked.R");
        std::fs::write(&target, "x <- 1\n").unwrap();
        symlink(&target, &link).unwrap();
        let config = ry_config::Config {
            exclude: vec!["linked.R".into()],
            ..Default::default()
        };

        assert!(!is_file_eligible(&link, dir.path(), &config));
        assert!(is_file_eligible(&target, dir.path(), &config));
    }

    #[test]
    fn discovery_skips_target_and_hidden_directories() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("keep.R"),
            "x <- 1
",
        )
        .unwrap();
        std::fs::create_dir_all(dir.path().join("target")).unwrap();
        std::fs::write(
            dir.path().join("target/skip.R"),
            "y <- 2
",
        )
        .unwrap();
        std::fs::create_dir_all(dir.path().join(".hidden")).unwrap();
        std::fs::write(
            dir.path().join(".hidden/secret.R"),
            "z <- 3
",
        )
        .unwrap();

        let result = discover_r_files(dir.path(), None, &ry_config::Config::default(), false);
        let names: Vec<String> = result
            .files
            .iter()
            .map(|p| p.file_name().unwrap().to_string_lossy().into_owned())
            .collect();
        assert!(names.contains(&"keep.R".to_string()));
        assert!(!names.contains(&"skip.R".to_string()), "target/ skipped");
        assert!(!names.contains(&"secret.R".to_string()), "hidden/ skipped");
    }

    #[test]
    fn discovery_max_files_cap_is_configurable_and_visible() {
        let dir = tempfile::tempdir().unwrap();
        for i in 0..5 {
            std::fs::write(
                dir.path().join(format!("file_{i}.R")),
                "x <- 1
",
            )
            .unwrap();
        }
        let config = ry_config::Config {
            index: ry_config::IndexConfig {
                max_files: 2,
                ..Default::default()
            },
            ..Default::default()
        };
        let result = discover_r_files(dir.path(), None, &config, false);
        assert_eq!(result.files.len(), 2, "only 2 files under cap");
        assert!(result.truncated.max_files_hit, "max-files cap reported");
    }

    #[test]
    fn discovery_max_file_bytes_cap_omits_oversized_files() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("small.R"),
            "x <- 1
",
        )
        .unwrap();
        std::fs::write(
            dir.path().join("big.R"),
            "y <- 2
",
        )
        .unwrap();
        // Set the big file's size via metadata — write a larger payload.
        std::fs::write(dir.path().join("big.R"), "y ".repeat(100)).unwrap();
        let config = ry_config::Config {
            index: ry_config::IndexConfig {
                max_file_bytes: 10,
                ..Default::default()
            },
            ..Default::default()
        };
        let result = discover_r_files(dir.path(), None, &config, false);
        let names: Vec<String> = result
            .files
            .iter()
            .map(|p| p.file_name().unwrap().to_string_lossy().into_owned())
            .collect();
        assert!(names.contains(&"small.R".to_string()));
        assert!(
            !names.contains(&"big.R".to_string()),
            "oversized file omitted"
        );
        assert!(
            result
                .truncated
                .oversized_files
                .iter()
                .any(|(p, _)| { p.file_name().unwrap() == "big.R" }),
            "oversized file reported in truncation"
        );
    }

    #[test]
    fn discovery_max_depth_cap_prunes_deep_directories() {
        let dir = tempfile::tempdir().unwrap();
        // Create a chain: a/b/c/deep.R
        let deep = dir.path().join("a/b/c/deep.R");
        std::fs::create_dir_all(deep.parent().unwrap()).unwrap();
        std::fs::write(
            &deep, "x <- 1
",
        )
        .unwrap();
        std::fs::write(
            dir.path().join("shallow.R"),
            "y <- 2
",
        )
        .unwrap();

        let config = ry_config::Config {
            index: ry_config::IndexConfig {
                max_depth: 1,
                ..Default::default()
            },
            ..Default::default()
        };
        let result = discover_r_files(dir.path(), None, &config, false);
        let names: Vec<String> = result
            .files
            .iter()
            .map(|p| p.file_name().unwrap().to_string_lossy().into_owned())
            .collect();
        assert!(
            names.contains(&"shallow.R".to_string()),
            "shallow file found"
        );
        assert!(!names.contains(&"deep.R".to_string()), "deep file pruned");
        assert!(
            !result.truncated.depth_pruned_dirs.is_empty(),
            "depth cap reported"
        );
    }

    #[test]
    fn discovery_includes_all_r_source_extensions() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();
        for extension in ["R", "r", "S", "s", "q"] {
            std::fs::write(root.join(format!("source.{extension}")), "value <- 1L\n").unwrap();
        }
        std::fs::write(root.join("source.txt"), "not R\n").unwrap();

        let mut paths = discover_r_files(root, None, &ry_config::Config::default(), false).files;
        paths.sort();

        let mut expected = ["R", "r", "S", "s", "q"]
            .map(|extension| root.join(format!("source.{extension}")))
            .into_iter()
            .collect::<Vec<_>>();
        expected.sort();
        assert_eq!(paths, expected);
    }

    #[test]
    fn package_scan_skips_test_fixtures_but_keeps_executable_test_code() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();
        std::fs::write(root.join("DESCRIPTION"), "Package: example\n").unwrap();
        for directory in [
            "R",
            "tests/testthat",
            "tests/testthat/fixtures",
            "tests/testthat/_snaps",
            "tests/manual",
            "revdep/other/R",
            "src/ratfor",
        ] {
            std::fs::create_dir_all(root.join(directory)).unwrap();
        }
        for file in [
            "R/package.R",
            "tests/testthat.R",
            "tests/testthat/test-package.R",
            "tests/testthat/helper-package.R",
            "tests/testthat/setup-package.R",
            "tests/testthat/teardown-package.R",
            "tests/testthat/fixtures/input.R",
            "tests/testthat/data.R",
            // Fixtures under the documented contract: a "test" prefix
            // lookalike, a runner spelling in a historical extension,
            // and a historical extension directly under tests/.
            "tests/testthat/testing.R",
            "tests/testthat/test-legacy.S",
            "tests/legacy.S",
            "tests/testthat/_snaps/output.R",
            "tests/manual/example.R",
            "revdep/other/R/other.R",
            "src/ratfor/program.r",
        ] {
            std::fs::write(root.join(file), "").unwrap();
        }

        let paths = discover_r_files(root, None, &ry_config::Config::default(), false).files;

        assert_eq!(
            paths,
            vec![
                root.join("R/package.R"),
                root.join("tests/testthat/helper-package.R"),
                root.join("tests/testthat/setup-package.R"),
                root.join("tests/testthat/teardown-package.R"),
                root.join("tests/testthat/test-package.R"),
                root.join("tests/testthat.R"),
            ]
        );
    }

    #[test]
    fn package_scan_can_opt_into_test_fixtures() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();
        std::fs::write(root.join("DESCRIPTION"), "Package: example\n").unwrap();
        std::fs::create_dir_all(root.join("tests/testthat/fixtures")).unwrap();
        let fixture = root.join("tests/testthat/fixtures/input.R");
        std::fs::write(&fixture, "missing_name\n").unwrap();

        let paths = discover_r_files(root, None, &ry_config::Config::default(), true).files;

        assert_eq!(paths, vec![fixture]);
    }

    #[test]
    fn package_scan_keeps_inst_sources() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();
        std::fs::write(root.join("DESCRIPTION"), "Package: example\n").unwrap();
        std::fs::create_dir_all(root.join("inst/resources")).unwrap();
        let installed = root.join("inst/resources/activate.R");
        std::fs::write(&installed, "missing_name\n").unwrap();

        let paths = discover_r_files(root, None, &ry_config::Config::default(), false).files;

        assert_eq!(paths, vec![installed]);
    }

    #[test]
    fn package_scan_skips_vendored_renv_bootstrap() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();
        std::fs::write(root.join("DESCRIPTION"), "Package: example\n").unwrap();
        std::fs::create_dir_all(root.join("R")).unwrap();
        std::fs::create_dir_all(root.join("renv")).unwrap();
        let source = root.join("R/package.R");
        std::fs::write(&source, "value <- 1L\n").unwrap();
        std::fs::write(root.join("renv/activate.R"), "bootstrap_missing\n").unwrap();

        let paths = discover_r_files(root, None, &ry_config::Config::default(), false).files;

        assert_eq!(paths, vec![source]);
    }

    #[test]
    fn explicitly_selected_file_is_not_package_excluded() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();
        std::fs::write(root.join("DESCRIPTION"), "Package: example\n").unwrap();
        std::fs::create_dir(root.join("src")).unwrap();
        let file = root.join("src/ratfor.r");
        std::fs::write(&file, "").unwrap();

        let paths = discover_r_files(&file, None, &ry_config::Config::default(), false).files;

        assert_eq!(paths, vec![file]);
    }

    #[test]
    fn explicitly_selected_q_file_is_collected() {
        let temp = tempfile::tempdir().unwrap();
        let file = temp.path().join("source.q");
        std::fs::write(&file, "value <- 1L\n").unwrap();

        let paths = discover_r_files(&file, None, &ry_config::Config::default(), false).files;

        assert_eq!(paths, vec![file]);
    }

    #[test]
    fn package_scan_honors_rbuildignore_except_r_and_tests() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();
        std::fs::write(root.join("DESCRIPTION"), "Package: example\n").unwrap();
        std::fs::write(root.join(".Rbuildignore"), "^ignored\\.R$\n^R/\n^tests/\n").unwrap();
        std::fs::create_dir_all(root.join("R")).unwrap();
        std::fs::create_dir_all(root.join("tests/testthat")).unwrap();
        for file in [
            "ignored.R",
            "kept.R",
            "R/package.R",
            "tests/testthat/test-package.R",
        ] {
            std::fs::write(root.join(file), "").unwrap();
        }

        let mut paths = discover_r_files(root, None, &ry_config::Config::default(), false).files;
        paths.sort();
        assert_eq!(
            paths,
            vec![
                root.join("R/package.R"),
                root.join("kept.R"),
                root.join("tests/testthat/test-package.R"),
            ]
        );
    }

    #[test]
    fn build_ignored_includes_are_narrow_and_explained() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::write(root.join("DESCRIPTION"), "Package: example\n").unwrap();
        std::fs::write(root.join(".Rbuildignore"), "^vignettes$\n").unwrap();
        for relative in [
            "R/main.R",
            "vignettes/keep.R",
            "vignettes/drop.R",
            "vignettes/nested/helper.R",
            "vignettes/nested/other.R",
            "vignettes/.hidden/keep.R",
        ] {
            let path = root.join(relative);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, "x <- 1L\n").unwrap();
        }
        let baseline = discover_r_files(root, Some(root), &ry_config::Config::default(), false);
        assert_eq!(baseline.files, vec![root.join("R/main.R")]);
        assert!(
            baseline
                .skipped
                .entries
                .contains(&(root.join("vignettes"), ".Rbuildignore"))
        );
        let config = ry_config::Config {
            include_build_ignored: vec![
                "vignettes/keep.R".into(),
                "vignettes/nested/*.R".into(),
                "vignettes/.hidden/keep.R".into(),
            ],
            exclude: vec!["vignettes/nested/other.R".into()],
            ..Default::default()
        };
        let selected = discover_r_files(root, Some(root), &config, false);
        assert_eq!(
            selected.files,
            vec![
                root.join("R/main.R"),
                root.join("vignettes/keep.R"),
                root.join("vignettes/nested/helper.R")
            ]
        );
        assert!(
            selected
                .skipped
                .entries
                .contains(&(root.join("vignettes/drop.R"), ".Rbuildignore"))
        );
        assert!(
            selected
                .skipped
                .entries
                .contains(&(root.join("vignettes/nested/other.R"), "ry.toml exclude"))
        );
    }

    #[test]
    fn include_does_not_clear_ignored_ancestor_at_a_nested_package() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::write(root.join("DESCRIPTION"), "Package: outer\n").unwrap();
        std::fs::write(root.join(".Rbuildignore"), "^examples$\n").unwrap();
        let nested = root.join("examples/package");
        std::fs::create_dir_all(nested.join("R")).unwrap();
        std::fs::write(nested.join("DESCRIPTION"), "Package: inner\n").unwrap();
        std::fs::write(nested.join("R/keep.R"), "x <- 1L\n").unwrap();
        std::fs::write(nested.join("R/drop.R"), "x <- 1L\n").unwrap();
        let config = ry_config::Config {
            include_build_ignored: vec!["examples/package/R/keep.R".into()],
            ..Default::default()
        };
        let result = discover_r_files(root, Some(root), &config, false);
        assert_eq!(result.files, vec![nested.join("R/keep.R")]);
    }

    #[test]
    fn skipped_path_report_is_bounded() {
        let dir = tempfile::tempdir().unwrap();
        for name in [".a", ".b", ".c"] {
            std::fs::create_dir(dir.path().join(name)).unwrap();
        }
        let mut config = ry_config::Config::default();
        config.index.max_files = 1;
        let result = discover_r_files(dir.path(), None, &config, false);
        assert_eq!(result.skipped.entries.len(), 1);
        assert_eq!(result.skipped.omitted, 2);
    }

    #[test]
    fn rbuildignore_trailing_dollar_respects_escape_parity() {
        assert!(rbuildignore_pattern("^file$").unwrap().matches("file"));
        assert!(!rbuildignore_pattern("^file$").unwrap().matches("filex"));
        assert!(rbuildignore_pattern(r"^file\$").unwrap().matches("file$"));
        assert!(rbuildignore_pattern(r"^file\\$").unwrap().matches(r"file\"));
        assert!(
            !rbuildignore_pattern(r"^file\\$")
                .unwrap()
                .matches(r"file\x")
        );
    }
}
