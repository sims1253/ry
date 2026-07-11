//! Filesystem-backed R package scope discovery for CLI checks.
//!
//! Package code is never loaded or evaluated. We parse project and installed
//! NAMESPACE files as R syntax, then turn proven imports/exports into opaque
//! checker bindings.

use ry_checker::packages::NamespaceMetadata;
use ry_core::SourceFile;
use ry_core::ast::{Expr, Stmt};
use std::collections::{HashMap, HashSet};
use std::io::Read;
use std::path::{Path, PathBuf};

pub(crate) struct PackageScope {
    pub(crate) attached: HashSet<String>,
    pub(crate) bindings: HashMap<String, HashSet<String>>,
    pub(crate) imported_from: HashMap<String, HashMap<String, String>>,
    pub(crate) s3_methods: HashMap<String, HashSet<(String, String)>>,
    pub(crate) load_bindings: HashMap<String, HashMap<usize, HashSet<String>>>,
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
    configured_globals: &[String],
    files: impl IntoIterator<Item = &'a SourceFile>,
) -> PackageScope {
    let files: Vec<&SourceFile> = files.into_iter().collect();
    let library_roots = r_library_roots(all_paths);
    let preferred_version = current_r_minor_version(&library_roots);
    let mut namespace_cache: HashMap<PathBuf, NamespaceMetadata> = HashMap::new();
    let mut export_cache: HashMap<String, HashSet<String>> = HashMap::new();
    let mut dataset_cache: HashMap<PathBuf, HashSet<String>> = HashMap::new();
    let mut serialized_cache: HashMap<PathBuf, HashSet<String>> = HashMap::new();
    let mut attached = HashSet::new();
    let mut bindings = HashMap::new();
    let mut imported_from = HashMap::new();
    let mut s3_methods = HashMap::new();
    let mut load_bindings = HashMap::new();
    let project_attached: HashSet<String> = configured_packages
        .iter()
        .cloned()
        .chain(
            files
                .iter()
                .flat_map(|file| ry_checker::packages::attached_packages(file)),
        )
        .collect();

    for file in files {
        let mut file_attached: HashSet<String> = configured_packages.iter().cloned().collect();
        let mut file_bindings = HashSet::new();
        let mut file_s3_methods = HashSet::new();
        let mut file_imported_from = HashMap::new();
        let mut source_package = None;
        file_bindings.extend(configured_globals.iter().cloned());
        if let Some(root) = r_package_root(Path::new(&file.path)) {
            if let Some(package) = source_package_name(&root) {
                file_attached.insert(package.clone());
                source_package = Some(package);
            }
            let metadata = namespace_cache
                .entry(root.clone())
                .or_insert_with(|| read_namespace(&root.join("NAMESPACE")));
            file_bindings.extend(metadata.imported_bindings.iter().cloned());
            file_imported_from.extend(metadata.imported_from.clone());
            file_bindings.extend(metadata.s3_generics.iter().cloned());
            file_s3_methods.extend(metadata.s3_methods.iter().cloned());
            file_attached.extend(metadata.imported_packages.iter().cloned());
            if source_package_lazy_data(&root) {
                file_bindings.extend(
                    dataset_cache
                        .entry(root.clone())
                        .or_insert_with(|| source_package_datasets(&root))
                        .iter()
                        .cloned(),
                );
            }
            let sysdata = root.join("R/sysdata.rda");
            file_bindings.extend(
                serialized_cache
                    .entry(sysdata.clone())
                    .or_insert_with(|| serialized_bindings(&sysdata))
                    .iter()
                    .cloned(),
            );
            load_bindings.insert(
                file.path.clone(),
                loaded_serialized_bindings(file, &root, &project_attached, &mut serialized_cache),
            );
        }
        file_attached.extend(ry_checker::packages::attached_packages(file));
        for package in &file_attached {
            // The package currently being checked gets any shipped typeshed,
            // but its bindings come from this source tree. Reading exports
            // from a separately installed copy could mask a missing source
            // definition with stale metadata.
            if source_package.as_ref() == Some(package) {
                continue;
            }
            let exports = export_cache.entry(package.clone()).or_insert_with(|| {
                installed_package_exports(package, &library_roots, preferred_version.as_deref())
            });
            file_bindings.extend(exports.iter().cloned());
        }
        attached.extend(file_attached);
        bindings.insert(file.path.clone(), file_bindings);
        imported_from.insert(file.path.clone(), file_imported_from);
        s3_methods.insert(file.path.clone(), file_s3_methods);
    }
    PackageScope {
        attached,
        bindings,
        imported_from,
        s3_methods,
        load_bindings,
    }
}

fn source_package_name(root: &Path) -> Option<String> {
    std::fs::read_to_string(root.join("DESCRIPTION"))
        .ok()?
        .lines()
        .find_map(|line| line.strip_prefix("Package:"))
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .map(str::to_string)
}

fn source_package_lazy_data(root: &Path) -> bool {
    std::fs::read_to_string(root.join("DESCRIPTION"))
        .ok()
        .and_then(|description| {
            description
                .lines()
                .find_map(|line| line.strip_prefix("LazyData:"))
                .map(str::trim)
                .map(str::to_ascii_lowercase)
        })
        .is_some_and(|value| matches!(value.as_str(), "true" | "yes"))
}

/// Dataset source files conventionally introduce the file stem as a package
/// binding (`data/example.rda` -> `example`). This inventory is static,
/// bounded to one directory, and cached indirectly by the per-run package
/// scope construction.
fn source_package_datasets(root: &Path) -> HashSet<String> {
    let Ok(entries) = std::fs::read_dir(root.join("data")) else {
        return HashSet::new();
    };
    let mut bindings = HashSet::new();
    for entry in entries.flatten() {
        let path = entry.path();
        let Some(extension) = path
            .extension()
            .and_then(|extension| extension.to_str())
            .map(str::to_ascii_lowercase)
        else {
            continue;
        };
        match extension.as_str() {
            "rda" | "rdata" => {
                let serialized = serialized_bindings(&path);
                if serialized.is_empty() {
                    if let Some(stem) = path.file_stem().and_then(|stem| stem.to_str()) {
                        bindings.insert(stem.to_string());
                    }
                } else {
                    bindings.extend(serialized);
                }
            }
            "rds" => {
                if let Some(stem) = path.file_stem().and_then(|stem| stem.to_str()) {
                    bindings.insert(stem.to_string());
                }
            }
            "r" => {
                let Ok(source) = std::fs::read_to_string(&path) else {
                    continue;
                };
                let Ok(mut parser) = ry_core::RParser::new() else {
                    continue;
                };
                let Ok(file) = parser.parse(&path.to_string_lossy(), &source) else {
                    continue;
                };
                bindings.extend(file.stmts.iter().filter_map(|statement| match statement {
                    Stmt::Assign {
                        target: Expr::Ident { name, .. },
                        ..
                    } => Some(name.clone()),
                    _ => None,
                }));
            }
            _ => {}
        }
    }
    bindings
}

/// Read only the top-level tags from an R serialization stream. `.rda`
/// workspaces are serialized pairlists whose tags are the binding names. The
/// parser's lazy mode skips vector payload allocation, and bzip2 streams are
/// decompressed in-process; no R runtime or project code is executed.
fn serialized_bindings(path: &Path) -> HashSet<String> {
    type CacheKey = (PathBuf, u64, u128);
    static CACHE: std::sync::OnceLock<std::sync::Mutex<HashMap<CacheKey, HashSet<String>>>> =
        std::sync::OnceLock::new();
    let Ok(metadata) = std::fs::metadata(path) else {
        return HashSet::new();
    };
    let modified = metadata
        .modified()
        .ok()
        .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|duration| duration.as_nanos())
        .unwrap_or_default();
    let key = (path.to_path_buf(), metadata.len(), modified);
    let cache = CACHE.get_or_init(|| std::sync::Mutex::new(HashMap::new()));
    if let Some(bindings) = cache.lock().expect("serialized cache poisoned").get(&key) {
        return bindings.clone();
    }
    let bindings = serialized_bindings_uncached(path);
    cache
        .lock()
        .expect("serialized cache poisoned")
        .insert(key, bindings.clone());
    bindings
}

fn serialized_bindings_uncached(path: &Path) -> HashSet<String> {
    let Ok(bytes) = std::fs::read(path) else {
        return HashSet::new();
    };
    let bytes = if bytes.starts_with(b"BZh") {
        let mut decoded = Vec::new();
        let mut decoder = bzip2::read::BzDecoder::new(bytes.as_slice());
        if decoder.read_to_end(&mut decoded).is_err() {
            return HashSet::new();
        }
        decoded
    } else if bytes.starts_with(&[0x1f, 0x8b]) {
        let mut decoded = Vec::new();
        let mut decoder = flate2::read::GzDecoder::new(bytes.as_slice());
        if decoder.read_to_end(&mut decoded).is_err() {
            return HashSet::new();
        }
        decoded
    } else if bytes.starts_with(&[0xfd, b'7', b'z', b'X', b'Z', 0x00]) {
        let mut decoded = Vec::new();
        let mut decoder = xz2::read::XzDecoder::new(bytes.as_slice());
        if decoder.read_to_end(&mut decoded).is_err() {
            return HashSet::new();
        }
        decoded
    } else {
        bytes
    };
    let payload = bytes.strip_prefix(b"RDX2\n").unwrap_or(&bytes);
    let Ok(parsed) = rds2rust::read_rds_lazy(payload) else {
        return HashSet::new();
    };
    match parsed.object.into_concrete() {
        rds2rust::RObject::Pairlist(elements) => elements
            .into_iter()
            .filter_map(|element| element.tag.map(|tag| tag.to_string()))
            .collect(),
        _ => HashSet::new(),
    }
}

fn loaded_serialized_bindings(
    file: &SourceFile,
    package_root: &Path,
    attached_packages: &HashSet<String>,
    cache: &mut HashMap<PathBuf, HashSet<String>>,
) -> HashMap<usize, HashSet<String>> {
    fn resolve_path(
        expr: &Expr,
        file: &SourceFile,
        package_root: &Path,
        attached_packages: &HashSet<String>,
    ) -> Option<PathBuf> {
        let (path, source_relative_only) = match expr {
            Expr::String(path, _) => (path, false),
            Expr::Call { func, args, .. } => {
                let Expr::Ident { name, .. } = func.as_ref() else {
                    return None;
                };
                let signature = if let Some((package, function)) = name.rsplit_once("::") {
                    ry_typeshed::load_package(package.trim_end_matches(':'))
                        .and_then(|typeshed| typeshed.functions.get(function))
                } else {
                    attached_packages.iter().find_map(|package| {
                        ry_typeshed::load_package(package)
                            .and_then(|typeshed| typeshed.functions.get(name))
                            .filter(|signature| signature.source_relative_path_arg.is_some())
                    })
                }?;
                let index = signature.source_relative_path_arg?;
                let Expr::String(path, _) = &args.get(index)?.value else {
                    return None;
                };
                (path, true)
            }
            _ => return None,
        };
        let raw = PathBuf::from(path);
        if raw.is_absolute() {
            return Some(raw);
        }
        let file_parent = Path::new(&file.path).parent().unwrap_or(package_root);
        let beside_file = file_parent.join(&raw);
        if beside_file.is_file() {
            Some(beside_file)
        } else if source_relative_only {
            None
        } else {
            Some(package_root.join(raw))
        }
    }
    let mut bindings = HashMap::new();
    for statement in &file.stmts {
        let Stmt::Expr(Expr::Call {
            func, args, span, ..
        }) = statement
        else {
            continue;
        };
        if !matches!(func.as_ref(), Expr::Ident { name, .. } if name == "load") {
            continue;
        }
        if let Some(path) = args.first().and_then(|argument| {
            resolve_path(&argument.value, file, package_root, attached_packages)
        }) {
            let loaded = cache
                .entry(path.clone())
                .or_insert_with(|| serialized_bindings(&path))
                .clone();
            bindings.insert(span.start, loaded);
        }
    }
    bindings
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use std::sync::Arc;

    #[test]
    fn inventories_bzip2_rdata_pairlist_tags() {
        let object = rds2rust::RObject::Pairlist(vec![
            rds2rust::PairlistElement {
                tag: Some(Arc::from("alpha")),
                value: rds2rust::RObject::Null,
                tag_object: None,
            },
            rds2rust::PairlistElement {
                tag: Some(Arc::from("beta")),
                value: rds2rust::RObject::Null,
                tag_object: None,
            },
        ]);
        let gzip = rds2rust::write_rds(&object).unwrap();
        let mut serialization = Vec::new();
        flate2::read::GzDecoder::new(gzip.as_slice())
            .read_to_end(&mut serialization)
            .unwrap();
        let mut rdata = b"RDX2\n".to_vec();
        rdata.extend(serialization);
        let mut encoder = bzip2::write::BzEncoder::new(Vec::new(), bzip2::Compression::best());
        encoder.write_all(&rdata).unwrap();
        let compressed = encoder.finish().unwrap();
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("objects.rda");
        std::fs::write(&path, compressed).unwrap();

        assert_eq!(
            serialized_bindings(&path),
            HashSet::from(["alpha".to_string(), "beta".to_string()])
        );
    }
}
