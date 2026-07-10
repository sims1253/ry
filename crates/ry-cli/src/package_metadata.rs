//! Filesystem-backed R package scope discovery for CLI checks.
//!
//! Package code is never loaded or evaluated. We parse project and installed
//! NAMESPACE files as R syntax, then turn proven imports/exports into opaque
//! checker bindings.

use ry_checker::packages::NamespaceMetadata;
use ry_core::SourceFile;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

pub(crate) struct PackageScope {
    pub(crate) attached: HashSet<String>,
    pub(crate) bindings: HashMap<String, HashSet<String>>,
}

struct LibraryRoot {
    path: PathBuf,
    max_depth: usize,
}

impl LibraryRoot {
    fn exact(path: impl Into<PathBuf>) -> Self {
        Self {
            path: path.into(),
            max_depth: 0,
        }
    }

    fn nested(path: impl Into<PathBuf>, max_depth: usize) -> Self {
        Self {
            path: path.into(),
            max_depth,
        }
    }
}

pub(crate) fn resolve<'a>(
    all_paths: &[PathBuf],
    configured_packages: &[String],
    files: impl IntoIterator<Item = &'a SourceFile>,
) -> PackageScope {
    let library_roots = r_library_roots(all_paths);
    let preferred_version = current_r_minor_version(&library_roots);
    let mut namespace_cache: HashMap<PathBuf, NamespaceMetadata> = HashMap::new();
    let mut export_cache: HashMap<String, HashSet<String>> = HashMap::new();
    let mut attached = HashSet::new();
    let mut bindings = HashMap::new();

    for file in files {
        let mut file_attached: HashSet<String> = configured_packages.iter().cloned().collect();
        let mut file_bindings = HashSet::new();
        if let Some(root) = r_package_root(Path::new(&file.path)) {
            let metadata = namespace_cache
                .entry(root.clone())
                .or_insert_with(|| read_namespace(&root.join("NAMESPACE")));
            file_bindings.extend(metadata.imported_bindings.iter().cloned());
            file_attached.extend(metadata.imported_packages.iter().cloned());
        }
        file_attached.extend(ry_checker::packages::attached_packages(file));
        for package in &file_attached {
            let exports = export_cache.entry(package.clone()).or_insert_with(|| {
                installed_package_exports(package, &library_roots, preferred_version.as_deref())
            });
            file_bindings.extend(exports.iter().cloned());
        }
        attached.extend(file_attached);
        bindings.insert(file.path.clone(), file_bindings);
    }
    PackageScope { attached, bindings }
}

/// Parse an R NAMESPACE file with the regular R parser. This handles quoted
/// names, comments, and multiline directives without a second parser.
fn read_namespace(path: &Path) -> NamespaceMetadata {
    let Ok(src) = std::fs::read_to_string(path) else {
        return NamespaceMetadata::default();
    };
    let Ok(mut parser) = ry_core::RParser::new() else {
        return NamespaceMetadata::default();
    };
    let Ok(file) = parser.parse(&path.to_string_lossy(), &src) else {
        return NamespaceMetadata::default();
    };
    ry_checker::packages::namespace_metadata(&file)
}

/// Find the nearest enclosing R package for a checked source path.
fn r_package_root(path: &Path) -> Option<PathBuf> {
    let start = if path.is_dir() { path } else { path.parent()? };
    start
        .ancestors()
        .find(|dir| dir.join("DESCRIPTION").is_file() && dir.join("NAMESPACE").is_file())
        .map(Path::to_path_buf)
}

/// Candidate R library roots that can be inspected without starting R.
/// The depth is only for layouts whose version/platform directories sit
/// between the root and the package directory.
fn r_library_roots(all_paths: &[PathBuf]) -> Vec<LibraryRoot> {
    let mut roots = Vec::new();
    let mut seen_renv = HashSet::new();
    for path in all_paths {
        let start = if path.is_dir() {
            path.as_path()
        } else if let Some(parent) = path.parent() {
            parent
        } else {
            continue;
        };
        if let Some(renv) = start
            .ancestors()
            .map(|ancestor| ancestor.join("renv/library"))
            .find(|candidate| candidate.is_dir())
        {
            if seen_renv.insert(renv.clone()) {
                roots.push(LibraryRoot::nested(renv, 3));
            }
        }
    }
    for key in ["R_LIBS", "R_LIBS_USER", "R_LIBS_SITE"] {
        if let Some(value) = std::env::var_os(key) {
            roots.extend(std::env::split_paths(&value).filter_map(library_root_from_env_path));
        }
    }
    if let Some(r_home) = std::env::var_os("R_HOME") {
        roots.push(LibraryRoot::exact(PathBuf::from(r_home).join("library")));
    }
    for path in [
        "/usr/local/lib/R/site-library",
        "/usr/local/lib64/R/site-library",
        "/usr/lib/R/site-library",
        "/usr/lib/R/library",
        "/usr/lib64/R/site-library",
        "/usr/lib64/R/library",
    ] {
        roots.push(LibraryRoot::exact(path));
    }
    roots.push(LibraryRoot::nested(
        "/Library/Frameworks/R.framework/Versions",
        3,
    ));
    if let Some(home) = std::env::var_os("HOME") {
        let home = PathBuf::from(home);
        roots.push(LibraryRoot::nested(home.join("R"), 2));
        roots.push(LibraryRoot::nested(home.join("Library/R"), 4));
    }
    for key in ["LOCALAPPDATA", "APPDATA"] {
        if let Some(root) = std::env::var_os(key) {
            roots.push(LibraryRoot::nested(
                PathBuf::from(root).join("R/win-library"),
                2,
            ));
        }
    }
    if let Some(profile) = std::env::var_os("USERPROFILE") {
        roots.push(LibraryRoot::nested(
            PathBuf::from(profile).join("Documents/R/win-library"),
            2,
        ));
    }
    for key in ["ProgramFiles", "ProgramFiles(x86)"] {
        if let Some(program_files) = std::env::var_os(key) {
            roots.push(LibraryRoot::nested(
                PathBuf::from(program_files).join("R"),
                2,
            ));
        }
    }
    let mut seen = HashSet::new();
    roots.retain(|root| seen.insert((root.path.clone(), root.max_depth)));
    roots
}

fn library_root_from_env_path(path: PathBuf) -> Option<LibraryRoot> {
    let raw = path.to_string_lossy();
    let expanded = if raw == "~" {
        user_home()?
    } else if let Some(rest) = raw.strip_prefix("~/").or_else(|| raw.strip_prefix("~\\")) {
        user_home()?.join(rest)
    } else {
        path
    };

    let rendered = expanded.to_string_lossy();
    if let Some(placeholder) = rendered.find('%') {
        let prefix = rendered[..placeholder].trim_end_matches(['/', '\\']);
        if prefix.is_empty() {
            None
        } else {
            Some(LibraryRoot::nested(prefix, 3))
        }
    } else {
        Some(LibraryRoot::exact(expanded))
    }
}

fn user_home() -> Option<PathBuf> {
    std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from)
}

fn find_package_namespace(
    root: &Path,
    package: &str,
    depth: usize,
    preferred_version: Option<&str>,
) -> Option<PathBuf> {
    if package.is_empty()
        || package == "."
        || package == ".."
        || package
            .chars()
            .any(|c| matches!(c, '/' | '\\') || c == std::path::MAIN_SEPARATOR)
    {
        return None;
    }
    let direct = root.join(package).join("NAMESPACE");
    if direct.is_file() {
        return Some(direct);
    }
    if depth == 0 {
        return None;
    }
    let entries = std::fs::read_dir(root).ok()?;
    let mut directories: Vec<PathBuf> = entries
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.is_dir())
        .collect();
    directories.sort_by(|a, b| {
        let a_name = a.file_name().and_then(|name| name.to_str()).unwrap_or("");
        let b_name = b.file_name().and_then(|name| name.to_str()).unwrap_or("");
        let a_preferred =
            preferred_version.is_some_and(|version| directory_matches_r_version(a_name, version));
        let b_preferred =
            preferred_version.is_some_and(|version| directory_matches_r_version(b_name, version));
        b_preferred
            .cmp(&a_preferred)
            .then_with(|| b_name.cmp(a_name))
    });
    for path in directories {
        if path.is_dir() {
            if let Some(found) =
                find_package_namespace(&path, package, depth - 1, preferred_version)
            {
                return Some(found);
            }
        }
    }
    None
}

fn directory_matches_r_version(directory: &str, minor_version: &str) -> bool {
    let candidate = directory.strip_prefix("R-").unwrap_or(directory);
    candidate == minor_version
        || candidate.strip_prefix(minor_version).is_some_and(|suffix| {
            suffix.starts_with('.') || suffix.starts_with('-') || suffix.starts_with('_')
        })
}

fn installed_package_exports(
    package: &str,
    roots: &[LibraryRoot],
    preferred_version: Option<&str>,
) -> HashSet<String> {
    roots
        .iter()
        .find_map(|root| {
            find_package_namespace(&root.path, package, root.max_depth, preferred_version)
        })
        .map(|path| read_namespace(&path).exports)
        .unwrap_or_default()
}

fn current_r_minor_version(roots: &[LibraryRoot]) -> Option<String> {
    let namespace = roots
        .iter()
        .find_map(|root| find_package_namespace(&root.path, "base", root.max_depth, None))?;
    let description = std::fs::read_to_string(namespace.parent()?.join("DESCRIPTION")).ok()?;
    let version = description
        .lines()
        .find_map(|line| line.strip_prefix("Version:"))?
        .trim();
    let mut parts = version.split('.');
    Some(format!("{}.{}", parts.next()?, parts.next()?))
}
