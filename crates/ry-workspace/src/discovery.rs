//! Bounded source discovery and package file eligibility.

use super::*;

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
    // `absolute` only joins the working directory, so a relative walk root
    // such as `.` still lines up with the config root.
    let path = std::path::absolute(path).unwrap_or_else(|_| path.to_path_buf());
    let root = std::path::absolute(root).unwrap_or_else(|_| root.to_path_buf());
    let relative = path.strip_prefix(&root).unwrap_or(&path);
    !excludes.matches(&relative.to_string_lossy().replace('\\', "/"))
}

/// Component depth of `path` relative to `root`: the number of components
/// below `root` (`root` itself is 0). `None` when `path` is not under `root`.
fn relative_depth(path: &Path, root: &Path) -> Option<usize> {
    path.strip_prefix(root)
        .ok()
        .map(|relative| relative.components().count())
}

/// Whether `path`'s directory sits at most `max_depth` components below
/// `root`: the walk's descent bound as a path property. A path outside
/// `root` passes.
fn is_within_depth(path: &Path, root: &Path, max_depth: usize) -> bool {
    let parent = path.parent().unwrap_or(path);
    relative_depth(parent, root).is_none_or(|depth| depth <= max_depth)
}

/// Per-file eligibility shared by the directory walk and open-buffer
/// admission: the reports opt-in, `exclude` patterns, depth, and size.
///
/// Depth counts from `depth_root`. `exclude_anchor` anchors the exclude
/// patterns, and `None` skips them. `content_len` is the byte length to
/// check against `index.max-file-bytes`, or `None` to skip the size gate.
/// Open buffers pass their text length because unsaved content never
/// touches disk. Indexed disk files pass `None` because the walk already
/// measured them.
pub fn is_file_eligible_with_limits(
    path: &Path,
    depth_root: &Path,
    exclude_anchor: Option<&Path>,
    excludes: &ry_config::Excludes,
    limits: &DiscoveryLimits,
    content_len: Option<u64>,
) -> bool {
    if !limits.admits_report(path) {
        return false;
    }
    if exclude_anchor.is_some_and(|anchor| !is_file_eligible_with_excludes(path, anchor, excludes))
    {
        return false;
    }
    if !is_within_depth(path, depth_root, limits.max_depth) {
        return false;
    }
    content_len.is_none_or(|len| len <= limits.max_file_bytes)
}

/// Whether the walk from `walk_root` would land the on-disk file `path`,
/// decided without walking. It applies the rules of [`discover_recursive`]
/// to the file and to each ancestor below `walk_root`: symlinks, `exclude`
/// patterns (a bare `exclude = ["vendor"]` prunes the directory but never
/// matches `vendor/defs.R` itself), pruned directory shapes, depth,
/// `.Rbuildignore` with the `include_build_ignored` rescue, test fixtures,
/// and the size cap. As in [`discover_r_files`], `exclude` anchors at
/// `exclude_anchor` and the rescue at `exclude_anchor.unwrap_or(walk_root)`.
///
/// `index.max-files` is not a path property, so callers enforce it when
/// they commit. The checks read the filesystem one entry at a time, so a
/// concurrent edit can change the answer. Callers re-validate under the
/// state lock, and a rescan fixes anything left over.
pub fn is_single_file_walk_admitted(
    path: &Path,
    walk_root: &Path,
    exclude_anchor: Option<&Path>,
    excludes: &ry_config::Excludes,
    include_build_ignored: &[String],
    limits: &DiscoveryLimits,
    check_test_fixtures: bool,
) -> bool {
    if !is_source_path(path) || !limits.admits_report(path) {
        return false;
    }
    // The walk never follows symlinks; classify the entry itself.
    if std::fs::symlink_metadata(path).is_ok_and(|metadata| metadata.is_symlink()) {
        return false;
    }
    // No walk reaches outside its root. Without this check the ancestor
    // loop below would climb past the root.
    if !walk_root.as_os_str().is_empty() && path.strip_prefix(walk_root).is_err() {
        return false;
    }
    // An empty `walk_root` (the server fallback with no root) has no walk
    // to agree with, so only the file checks below apply.
    if !walk_root.as_os_str().is_empty() {
        // The outermost package level, as in `discover_r_files`: the nearest
        // DESCRIPTION at or above the walk root. It can sit above the folder
        // when the config was inherited.
        let mut package_root = walk_root
            .ancestors()
            .find(|ancestor| ancestor.join("DESCRIPTION").is_file())
            .map(Path::to_path_buf);
        let mut buildignore = package_root
            .as_deref()
            .map(read_rbuildignore)
            .unwrap_or_default();
        let includes = compile_build_ignored_includes(
            exclude_anchor.unwrap_or(walk_root),
            include_build_ignored,
        );
        let has_excludes = !excludes.is_empty();
        // Visit ancestors top-down, from the walk root's child to the
        // file's parent. Once an ancestor is build-ignored, everything below
        // it is, as with the walk's `inherited_buildignore`.
        let mut chain = Vec::new();
        let mut dir = path.parent();
        while let Some(current) = dir {
            if current == walk_root {
                break;
            }
            chain.push(current);
            dir = current.parent();
        }
        let mut inherited_buildignore = false;
        for dir in chain.into_iter().rev() {
            // The walk never descends into a symlinked directory.
            if std::fs::symlink_metadata(dir).is_ok_and(|metadata| metadata.is_symlink()) {
                return false;
            }
            // The walk also prunes directories that match `exclude`.
            if has_excludes
                && let Some(anchor) = exclude_anchor
                && !is_file_eligible_with_excludes(dir, anchor, excludes)
            {
                return false;
            }
            // Pruned shapes use the current package level, not the
            // innermost. Above a nested DESCRIPTION there is no package yet,
            // so `renv/` there is kept.
            let pruned = dir
                .file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| is_pruned_directory_name(name, package_root.is_some()))
                || package_root
                    .as_deref()
                    .is_some_and(|root| is_excluded_package_directory(root, dir));
            if pruned {
                return false;
            }
            // The walk never enters a directory more than `max_depth`
            // components below its root.
            if relative_depth(dir, walk_root).is_some_and(|depth| depth > limits.max_depth) {
                return false;
            }
            if package_root
                .as_deref()
                .is_some_and(|root| is_rbuildignored(root, dir, &buildignore))
            {
                inherited_buildignore = true;
            }
            if inherited_buildignore && includes.is_empty() {
                return false;
            }
            // A nested package swaps the pattern set. The inherited flag
            // keeps the outer verdict, as in the walk.
            if dir.join("DESCRIPTION").is_file() {
                package_root = Some(dir.to_path_buf());
                buildignore = read_rbuildignore(dir);
            }
        }
        // The walk's file rule: build-ignored by itself or by an ancestor,
        // and rescued only by an explicit include.
        let build_ignored = inherited_buildignore
            || package_root
                .as_deref()
                .is_some_and(|root| is_rbuildignored(root, path, &buildignore));
        if build_ignored && !includes.matches(path) {
            return false;
        }
    }
    if !check_test_fixtures && is_test_fixture(path) {
        return false;
    }
    // `None` skips the length gate. The check below measures the file on
    // disk, as the walk does.
    if !is_file_eligible_with_limits(path, walk_root, exclude_anchor, excludes, limits, None) {
        return false;
    }
    // The walk descends a directory named like a source (`pkg.R/`) instead
    // of landing it, and such a directory passes every check above.
    // Requiring a regular file also refuses a FIFO named `x.R`, which the
    // walk would land but nothing can read as a source. An entry that
    // cannot be measured passes, as in the walk. The caller's own read
    // decides a missing or unreadable file.
    if std::fs::metadata(path)
        .map(|metadata| !metadata.is_file() || metadata.len() > limits.max_file_bytes)
        .unwrap_or(false)
    {
        return false;
    }
    true
}

/// Compile `include_build_ignored` against `anchor`, the walk's
/// `exclude_root.unwrap_or(walk_root)`. Config loading rejects invalid
/// patterns.
fn compile_build_ignored_includes(anchor: &Path, patterns: &[String]) -> ry_config::ScopedPaths {
    ry_config::ScopedPaths::new(anchor, patterns).unwrap_or_default()
}

/// Bounded directory discovery limits from `[index]` in `ry.toml`, shared
/// by CLI discovery and LSP indexing.
#[derive(Clone, Copy, Debug)]
pub struct DiscoveryLimits {
    /// Maximum number of R source files discovered per root.
    pub max_files: usize,
    /// Maximum size in bytes of a single R file to include.
    pub max_file_bytes: u64,
    /// Maximum directory depth to descend from each root.
    pub max_depth: usize,
    /// Whether `.Rmd`/`.qmd` reports are analyzed (`reports.enabled`).
    pub reports_enabled: bool,
}

impl DiscoveryLimits {
    pub fn from_config(config: &ry_config::Config) -> Self {
        Self {
            max_files: config.index.max_files as usize,
            max_file_bytes: config.index.max_file_bytes,
            max_depth: config.index.max_depth as usize,
            reports_enabled: config.reports.enabled,
        }
    }

    /// False only for a report while reports are not opted in.
    pub fn admits_report(&self, path: &Path) -> bool {
        self.reports_enabled || !crate::reports::is_report_path(path)
    }
}

/// Which discovery caps were hit, for the caller to warn about.
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

/// Whether `path` carries an extension the directory walk admits:
/// conventional `.R`/`.r`, the historical S-dialect `.S`/`.s`/`.q`
/// spellings, and report extensions in any case. `reports.enabled` is
/// applied separately through [`DiscoveryLimits::admits_report`].
pub fn is_source_path(path: &Path) -> bool {
    crate::reports::is_report_path(path)
        || path
            .extension()
            .and_then(|ext| ext.to_str())
            .is_some_and(|ext| R_EXTENSIONS.contains(&ext))
}

const R_EXTENSIONS: [&str; 5] = ["R", "r", "S", "s", "q"];

/// Glob alternatives for every extension discovery admits: R, the
/// S-dialect spellings, and reports as case-insensitive character classes.
pub fn source_extension_globs() -> impl Iterator<Item = String> {
    R_EXTENSIONS
        .into_iter()
        .map(str::to_owned)
        .chain(crate::reports::REPORT_EXTENSIONS.iter().map(|extension| {
            extension
                .chars()
                .map(|letter| format!("[{}{letter}]", letter.to_ascii_uppercase()))
                .collect()
        }))
}

/// Result of a bounded directory discovery.
#[derive(Clone, Debug, Default)]
pub struct DiscoveryResult {
    /// Discovered R source file paths (sorted and deduplicated).
    pub files: Vec<PathBuf>,
    /// Structured cap report. Empty when no limit was reached.
    pub truncated: TruncationReport,
    pub skipped: SkippedPaths,
    /// Directories the walk could not read, with the error, including a
    /// nonexistent root. A tree that could not be walked must not look
    /// like a clean, empty check.
    pub read_errors: Vec<(PathBuf, String)>,
}

/// Discover the eligible R sources under `walk_root` with the rules the CLI
/// and the LSP share. `exclude_root` anchors the `exclude` patterns and is
/// the directory holding `ry.toml`. `None` skips them. A cap hit fills
/// [`TruncationReport`].
pub fn discover_r_files(
    walk_root: &Path,
    exclude_root: Option<&Path>,
    config: &ry_config::Config,
    check_test_fixtures: bool,
) -> DiscoveryResult {
    // A single file passed directly is included regardless of package
    // rules: it is the explicit subject of the analysis. Only a report
    // still needs the explicit opt-in.
    let limits = DiscoveryLimits::from_config(config);
    if walk_root.is_file() {
        let mut result = DiscoveryResult::default();
        if limits.admits_report(walk_root) {
            result.files.push(walk_root.to_path_buf());
        } else {
            result
                .skipped
                .record(walk_root, false, "reports.enabled = false", usize::MAX);
        }
        return result;
    }
    let excludes = ry_config::Excludes::from_config(config);
    let has_excludes = !excludes.is_empty();
    let mut files = Vec::new();
    let mut truncated = TruncationReport::default();
    let mut skipped = SkippedPaths::default();
    let mut read_errors = Vec::new();
    let include_root = exclude_root.unwrap_or(walk_root);
    let includes = compile_build_ignored_includes(include_root, &config.include_build_ignored);
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
        walk_root,
        &mut files,
        &mut truncated,
        &mut skipped,
        &mut read_errors,
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
    read_errors.sort();
    read_errors.dedup();
    DiscoveryResult {
        files,
        truncated,
        skipped,
        read_errors,
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
    walk_root: &Path,
    out: &mut Vec<PathBuf>,
    truncated: &mut TruncationReport,
    skipped: &mut SkippedPaths,
    read_errors: &mut Vec<(PathBuf, String)>,
    includes: &ry_config::ScopedPaths,
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
    // A failed read_dir — the root does not exist, or permissions (or a
    // race) denied the listing — is recorded rather than swallowed: the
    // caller decides whether an unwalkable tree fails the run.
    let entries = match std::fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(error) => {
            read_errors.push((dir.to_path_buf(), error.to_string()));
            return;
        }
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
                    .take_while(|p| *p != root && *p != walk_root)
                    .any(|ancestor| is_rbuildignored(root, ancestor, buildignore))
            });
        if build_ignored
            && ((directory && includes.is_empty()) || (!directory && !includes.matches(&path)))
        {
            skipped.record(&path, directory, ".Rbuildignore", limits.max_files);
            continue;
        }
        if path.is_dir() {
            // Fixed pruned-directory shapes, shared with the single-file
            // verdict so they cannot drift (`renv` only inside a package).
            if let Some(name) = path.file_name().and_then(|n| n.to_str())
                && is_pruned_directory_name(name, package_root.is_some())
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
            // Open-buffer admission applies the same bound through
            // `is_within_depth`.
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
                walk_root,
                out,
                truncated,
                skipped,
                read_errors,
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
            if !limits.admits_report(&path) {
                skipped.record(&path, false, "reports.enabled = false", limits.max_files);
                continue;
            }
            if !check_test_fixtures && is_test_fixture(&path) {
                skipped.record(&path, false, "test fixture", limits.max_files);
                continue;
            }
            if out.len() >= limits.max_files {
                truncated.max_files_hit = true;
                break;
            }
            // Only sizes strictly above the cap are omitted, as in
            // `is_file_eligible_with_limits`.
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

/// Translate the regex subset conventional `.Rbuildignore` files use to a
/// glob. A pattern with unsupported regex constructs is skipped.
fn rbuildignore_pattern(regex: &str) -> Option<glob::Pattern> {
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

/// Whether the walk prunes a directory by name: hidden or generated
/// directories, and `renv` only inside a package.
fn is_pruned_directory_name(name: &str, in_package: bool) -> bool {
    name.starts_with('.')
        || name == "target"
        || name == "node_modules"
        || (name == "renv" && in_package)
        || name.ends_with(".Rcheck")
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
        let context = resolve_workspace_context(
            root,
            &ry_config::Config::default(),
            files.iter().collect(),
            &BTreeMap::new(),
        )
        .unwrap();

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

    /// The shared exclude third of `is_file_eligible_with_limits`:
    /// matching is rooted at the configuration/workspace root and uses
    /// forward slashes, so indexing and publication cannot give the same
    /// patterns different exclude semantics.
    #[test]
    fn shared_policy_excludes_are_rooted_and_separator_independent() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("vendor").join("influence.R");
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        std::fs::write(&file, "x <- 1\n").unwrap();
        let config = ry_config::Config {
            exclude: vec!["vendor/**".into()],
            ..Default::default()
        };
        let limits = DiscoveryLimits::from_config(&config);
        let excludes = ry_config::Excludes::from_config(&config);
        let eligible = |path: &Path| {
            is_file_eligible_with_limits(
                path,
                dir.path(),
                Some(dir.path()),
                &excludes,
                &limits,
                None,
            )
        };
        assert!(!eligible(&file));
        assert!(eligible(&dir.path().join("keep.R")));
    }

    /// The shared caps half of `is_file_eligible_with_limits`: the
    /// size boundary is inclusive and the depth bound keeps files whose
    /// parent is exactly at `max_depth` — the same boundaries
    /// `discover_recursive` applies to the bytes and descent levels on
    /// disk, so buffer-text admission cannot disagree with the closed
    /// index by a byte or a level.
    #[test]
    fn shared_limits_match_the_walkers_boundaries() {
        let limits = DiscoveryLimits {
            max_files: 100,
            max_file_bytes: 10,
            max_depth: 1,
            reports_enabled: false,
        };
        let excludes = ry_config::Excludes::from_config(&ry_config::Config::default());
        let root = Path::new("/root");
        let eligible = |path: &Path, len: Option<u64>| {
            is_file_eligible_with_limits(path, root, Some(root), &excludes, &limits, len)
        };
        assert!(eligible(&root.join("at.R"), Some(10)));
        assert!(!eligible(&root.join("over.R"), Some(11)));
        assert!(eligible(&root.join("a").join("deep.R"), None));
        assert!(!eligible(&root.join("a").join("b").join("deeper.R"), None));
    }

    /// Depth is a path property relative to the given root, not a walk
    /// property: nested roots see fewer components, and paths outside the
    /// root carry no depth to judge.
    #[test]
    fn depth_counts_directory_components_relative_to_the_root() {
        assert!(is_within_depth(
            Path::new("/root/a/b/file.R"),
            Path::new("/root/a"),
            1
        ));
        assert!(!is_within_depth(
            Path::new("/root/a/b/file.R"),
            Path::new("/root/a"),
            0
        ));
        assert!(is_within_depth(
            Path::new("/elsewhere/file.R"),
            Path::new("/root"),
            0
        ));
    }

    /// Exclude matching reads the workspace entry name, not a
    /// canonicalized symlink target: an explicit exclude for `linked.R`
    /// excludes that entry regardless of where it points.
    #[cfg(unix)]
    #[test]
    fn shared_policy_matches_a_symlink_entry_name_not_its_target() {
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
        let limits = DiscoveryLimits::from_config(&config);
        let excludes = ry_config::Excludes::from_config(&config);
        let eligible = |path: &Path| {
            is_file_eligible_with_limits(
                path,
                dir.path(),
                Some(dir.path()),
                &excludes,
                &limits,
                None,
            )
        };

        assert!(!eligible(&link));
        assert!(eligible(&target));
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
    fn report_extensions_ignore_case_and_disabled_reports_are_explained() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();
        let names = ["a.Rmd", "b.RMD", "c.qmd", "d.QMD", "e.Qmd"];
        for name in names {
            std::fs::write(root.join(name), "```{r}\nx <- 1L\n```\n").unwrap();
        }
        let expected: Vec<_> = names.map(|name| root.join(name)).into();

        let disabled = discover_r_files(root, None, &ry_config::Config::default(), false);
        assert!(disabled.files.is_empty());
        let skipped: Vec<_> = disabled
            .skipped
            .entries
            .iter()
            .map(|(path, reason)| {
                assert_eq!(*reason, "reports.enabled = false");
                path.clone()
            })
            .collect();
        assert_eq!(skipped, expected);

        let mut config = ry_config::Config::default();
        config.reports.enabled = true;
        assert_eq!(discover_r_files(root, None, &config, false).files, expected);

        let globs: Vec<_> = source_extension_globs().collect();
        assert!(globs.contains(&"[Rr][Mm][Dd]".to_owned()), "{globs:?}");
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

    /// A nonexistent root is a discovery failure, not an empty file set:
    /// the failed `read_dir` is recorded so `ry check` can fail the run
    /// instead of reporting a clean, empty check.
    #[test]
    fn missing_root_is_a_read_error_not_an_empty_discovery() {
        let dir = tempfile::tempdir().unwrap();
        let missing = dir.path().join("nonexistent.R");
        let result = discover_r_files(&missing, None, &ry_config::Config::default(), false);
        assert!(result.files.is_empty());
        assert_eq!(result.read_errors.len(), 1, "{:?}", result.read_errors);
        assert_eq!(result.read_errors[0].0, missing);
        assert!(!result.read_errors[0].1.is_empty());
    }

    /// An existing but empty (or fully excluded) tree is intentional:
    /// `files` is empty and no read error is recorded, so the caller can
    /// keep its success-with-note behavior for genuinely empty inputs.
    #[test]
    fn readable_empty_tree_records_no_read_errors() {
        let dir = tempfile::tempdir().unwrap();
        let result = discover_r_files(dir.path(), None, &ry_config::Config::default(), false);
        assert!(result.files.is_empty());
        assert!(result.read_errors.is_empty());
    }

    /// A directory that exists but cannot be listed is reported as a read
    /// error rather than silently contributing nothing. Root (or an
    /// exotic filesystem) lists mode-0 directories anyway; without a
    /// real read failure there is no error path to pin.
    #[cfg(unix)]
    #[test]
    fn unreadable_directory_is_reported_as_a_read_error() {
        use std::os::unix::fs::PermissionsExt;

        let dir = tempfile::tempdir().unwrap();
        let locked = dir.path().join("locked");
        std::fs::create_dir(&locked).unwrap();
        std::fs::write(locked.join("secret.R"), "x <- 1\n").unwrap();
        std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o000)).unwrap();
        if std::fs::read_dir(&locked).is_ok() {
            return;
        }

        let result = discover_r_files(dir.path(), None, &ry_config::Config::default(), false);
        assert!(result.files.is_empty());
        assert_eq!(result.read_errors.len(), 1, "{:?}", result.read_errors);
        assert_eq!(result.read_errors[0].0, locked);
    }

    /// The single-file verdict agrees with the walk on every path in a
    /// mixed tree: directory-shape excludes, file-shape excludes,
    /// buildignore pruning with include rescue (including across a nested
    /// package boundary), pruned directory names, depth and size caps, and
    /// testthat runner classification. The count gate is not a path
    /// property, so the oracle walk runs with `max-files` far above the
    /// tree size and the verdict is compared against walk membership
    /// alone.
    #[test]
    fn single_file_verdict_agrees_with_the_walk_on_every_path() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::write(root.join("DESCRIPTION"), "Package: example\n").unwrap();
        std::fs::write(root.join(".Rbuildignore"), "^vignettes$\n^examples$\n").unwrap();
        std::fs::create_dir_all(root.join("examples/pkg")).unwrap();
        std::fs::write(root.join("examples/pkg/DESCRIPTION"), "Package: inner\n").unwrap();
        let config = ry_config::Config {
            exclude: vec!["vendor".into(), "gen/**".into(), "skip.R".into()],
            include_build_ignored: vec!["vignettes/keep.R".into(), "examples/pkg/R/k.R".into()],
            index: ry_config::IndexConfig {
                max_files: 1_000_000,
                max_file_bytes: 64,
                max_depth: 3,
            },
            ..Default::default()
        };
        // (relative path, whether the walk discovers it).
        let mut cases = vec![
            ("keep.R", true),
            ("skip.R", false),
            ("big.R", false),
            ("main.R", true),
            ("notes.txt", false),
            ("vendor/defs.R", false),
            ("vendor/nested/deep.R", false),
            ("gen/out.R", false),
            ("vignettes/drop.R", false),
            ("vignettes/keep.R", true),
            ("examples/pkg/R/k.R", true),
            ("examples/pkg/R/drop.R", false),
            ("a/shallow.R", true),
            ("a/b/mid.R", true),
            ("a/b/c/deep.R", true),
            ("a/b/c/d/deeper.R", false),
            (".hidden/secret.R", false),
            ("target/skip.R", false),
            ("tests/testthat.R", true),
            ("tests/testthat/test-x.R", true),
            ("tests/testthat/data.R", false),
            ("tests/testthat/_snaps/snap.R", false),
            ("tests/manual/m.R", false),
            ("revdep/other/R/x.R", false),
            ("renv/activate.R", false),
        ];
        for (relative, _) in &cases {
            let path = root.join(relative);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            let contents = if *relative == "big.R" {
                "z <- 2\n".repeat(30)
            } else {
                "x <- 1\n".to_string()
            };
            std::fs::write(&path, contents).unwrap();
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::symlink;
            symlink(root.join("keep.R"), root.join("link.R")).unwrap();
            cases.push(("link.R", false));
        }
        assert!(
            std::fs::metadata(root.join("big.R")).unwrap().len() > 64,
            "big.R must exceed the size cap"
        );
        let members: std::collections::HashSet<PathBuf> =
            discover_r_files(root, Some(root), &config, false)
                .files
                .into_iter()
                .collect();
        let excludes = ry_config::Excludes::from_config(&config);
        let limits = DiscoveryLimits::from_config(&config);
        for (relative, expected) in &cases {
            let path = root.join(relative);
            assert_eq!(
                members.contains(&path),
                *expected,
                "walk oracle disagrees with the case table for {relative}"
            );
            assert_eq!(
                is_single_file_walk_admitted(
                    &path,
                    root,
                    Some(root),
                    &excludes,
                    &config.include_build_ignored,
                    &limits,
                    false,
                ),
                *expected,
                "verdict disagrees with the walk for {relative}"
            );
        }
    }

    /// A directory named like an R source (`pkg.R/`) passes the
    /// extension, symlink, and size checks — directory entries are
    /// small — but the walk descends directories and never lands one
    /// in its file set, so the verdict must refuse the directory
    /// spelling while still admitting the real sources inside it
    ///.
    #[test]
    fn single_file_verdict_refuses_directory_named_like_a_source() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let directory = root.join("pkg.R");
        std::fs::create_dir(&directory).unwrap();
        std::fs::write(directory.join("inside.R"), "x <- 1\n").unwrap();
        let config = ry_config::Config::default();
        let excludes = ry_config::Excludes::from_config(&config);
        let limits = DiscoveryLimits::from_config(&config);
        assert!(
            !is_single_file_walk_admitted(
                &directory,
                root,
                Some(root),
                &excludes,
                &config.include_build_ignored,
                &limits,
                false,
            ),
            "a directory named pkg.R must be refused exactly like the walk refuses it"
        );
        // Walk parity: the directory's own spelling never enters the
        // file set; only the regular file inside it is discovered.
        let walk = discover_r_files(root, Some(root), &config, false);
        assert!(
            !walk.files.contains(&directory),
            "the walk never lands the directory spelling: {:?}",
            walk.files
        );
        assert!(
            walk.files.contains(&directory.join("inside.R")),
            "the regular file inside the directory stays discoverable: {:?}",
            walk.files
        );
        assert!(
            is_single_file_walk_admitted(
                &directory.join("inside.R"),
                root,
                Some(root),
                &excludes,
                &config.include_build_ignored,
                &limits,
                false,
            ),
            "the verdict keeps admitting the regular file inside"
        );
    }

    /// `renv/` is pruned only inside a package: without a
    /// DESCRIPTION above the walk root the walker keeps it — including a
    /// nested package below it — and the verdict must agree. The retired
    /// per-file check resolved the *nearest* package instead, so it pruned
    /// `renv/` as soon as anything nested held a DESCRIPTION.
    #[test]
    fn single_file_verdict_keeps_renv_without_a_package() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        for relative in ["renv/keep.R", "renv/pkg/R/k.R", "plain.R"] {
            let path = root.join(relative);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(&path, "x <- 1\n").unwrap();
        }
        std::fs::write(root.join("renv/pkg/DESCRIPTION"), "Package: inner\n").unwrap();
        let config = ry_config::Config::default();
        let members: std::collections::HashSet<PathBuf> =
            discover_r_files(root, Some(root), &config, false)
                .files
                .into_iter()
                .collect();
        assert_eq!(
            members.len(),
            3,
            "the walk keeps renv/ without a package: {members:?}"
        );
        let excludes = ry_config::Excludes::from_config(&config);
        let limits = DiscoveryLimits::from_config(&config);
        for relative in ["renv/keep.R", "renv/pkg/R/k.R", "plain.R"] {
            let path = root.join(relative);
            assert!(
                is_single_file_walk_admitted(
                    &path,
                    root,
                    Some(root),
                    &excludes,
                    &config.include_build_ignored,
                    &limits,
                    false,
                ),
                "verdict must keep {relative} like the walk does"
            );
        }
    }
}

/// Parity between [`is_single_file_walk_admitted`] and
/// [`discover_r_files`] for symlinked directories and walk-root
/// containment: the verdict must refuse exactly the spellings the
/// walk never emits.
#[cfg(test)]
#[cfg(unix)]
mod single_file_verdict_parity_tests {
    use super::*;

    #[test]
    fn symlinked_ancestors_and_outside_paths_match_the_walk() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::create_dir(root.join("real")).unwrap();
        std::fs::write(root.join("real").join("x.R"), "x <- 1L\n").unwrap();
        std::os::unix::fs::symlink(root.join("real"), root.join("link")).unwrap();

        let config = ry_config::Config::default();
        let excludes = ry_config::Excludes::from_config(&config);
        let limits = DiscoveryLimits::from_config(&config);
        let admit = |path: &std::path::Path| {
            is_single_file_walk_admitted(path, root, Some(root), &excludes, &[], &limits, true)
        };

        let real = root.join("real").join("x.R");
        let through_link = root.join("link").join("x.R");
        assert!(admit(&real), "the real spelling is walk-admitted");
        assert!(
            !admit(&through_link),
            "the through-link spelling is never emitted by the walk"
        );

        let walk = discover_r_files(root, Some(root), &config, true);
        assert!(
            walk.files.contains(&real),
            "the walk indexes the real spelling: {:?}",
            walk.files
        );
        assert!(
            !walk.files.iter().any(|f| f == &through_link),
            "the walk never emits the through-link spelling: {:?}",
            walk.files
        );

        // Walk-root containment: no walk rooted at `real/` reaches
        // outside it.
        let outside = root.join("y.R");
        std::fs::write(&outside, "y <- 1L\n").unwrap();
        assert!(
            !is_single_file_walk_admitted(
                &outside,
                &root.join("real"),
                Some(root),
                &excludes,
                &[],
                &limits,
                true
            ),
            "paths outside the walk root are undiscoverable"
        );
    }
}
