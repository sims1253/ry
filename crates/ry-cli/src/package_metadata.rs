//! Filesystem-backed R package scope discovery for CLI checks.
//!
//! Package code is never loaded or evaluated. We parse project and installed
//! NAMESPACE files as R syntax, then turn proven imports/exports into opaque
//! checker bindings.

use ry_checker::SERIALIZED_BINDINGS_UNENUMERABLE;
use ry_checker::packages::NamespaceMetadata;
use ry_core::SourceFile;
use ry_core::ast::{Expr, Stmt};
use std::collections::{HashMap, HashSet};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock};

static MAX_SERIALIZED_BYTES: AtomicU64 = AtomicU64::new(2 * 1024 * 1024);

pub(crate) fn set_max_serialized_bytes(cap: u64) {
    MAX_SERIALIZED_BYTES.store(cap, Ordering::Relaxed);
}

/// A user-declared environment profile from `ry.toml` `[[environments]]`:
/// ambient bindings injected into files whose path matches.
struct EnvironmentBindings {
    bindings: Vec<String>,
    paths: Vec<String>,
}

static ENVIRONMENTS: OnceLock<Mutex<Vec<EnvironmentBindings>>> = OnceLock::new();
pub(crate) fn set_environments(profiles: &[crate::config::EnvironmentConfig]) {
    let profiles = profiles
        .iter()
        .map(|p| EnvironmentBindings {
            bindings: p.bindings.clone(),
            paths: p.paths.clone(),
        })
        .collect();
    *ENVIRONMENTS
        .get_or_init(|| Mutex::new(Vec::new()))
        .lock()
        .unwrap() = profiles;
}

pub(crate) struct PackageScope {
    pub(crate) attached: HashSet<String>,
    pub(crate) bare_attached: HashMap<String, HashSet<String>>,
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
    user_stubs: &std::collections::BTreeMap<String, ry_typeshed::Typeshed>,
    files: impl IntoIterator<Item = &'a SourceFile>,
) -> PackageScope {
    let files: Vec<&SourceFile> = files.into_iter().collect();
    let library_roots = r_library_roots(all_paths);
    let preferred_version = current_r_minor_version(&library_roots);
    let mut namespace_cache: HashMap<PathBuf, NamespaceMetadata> = HashMap::new();
    let mut export_cache: HashMap<String, HashSet<String>> = HashMap::new();
    let mut dataset_cache: HashMap<PathBuf, HashSet<String>> = HashMap::new();
    let mut serialized_cache: HashMap<PathBuf, HashSet<String>> = HashMap::new();
    let mut source_binding_cache: HashMap<PathBuf, HashSet<String>> = HashMap::new();
    let mut attached = HashSet::new();
    let mut bare_attached = HashMap::new();
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
        for profile in ENVIRONMENTS
            .get_or_init(|| Mutex::new(Vec::new()))
            .lock()
            .unwrap()
            .iter()
        {
            if profile.paths.iter().any(|pattern| {
                file.path
                    .replace('\\', "/")
                    .contains(pattern.trim_end_matches("/**"))
            }) {
                file_bindings.extend(profile.bindings.iter().cloned());
            }
        }
        if let Some(root) = r_package_root(Path::new(&file.path)) {
            file_bindings.extend(
                source_binding_cache
                    .entry(root.clone())
                    .or_insert_with(|| source_package_namespace_bindings(&root))
                    .iter()
                    .cloned(),
            );
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
            file_bindings.extend(
                metadata
                    .native_routine_prefixes
                    .iter()
                    .map(|prefix| format!("\0useDynLib:{prefix}")),
            );
            file_s3_methods.extend(metadata.s3_methods.iter().cloned());
            // `import(pkg)` puts pkg's exports in the package namespace,
            // not on the search path used to run its tests and examples.
            // Keep wholesale imports confined to package implementation
            // files; `importFrom()` bindings above remain available wherever
            // the package context makes them meaningful.
            let relative = Path::new(&file.path).strip_prefix(&root).ok();
            if relative.is_some_and(is_package_r_file) {
                file_attached.extend(metadata.imported_packages.iter().cloned());
                // Packages that rely on DESCRIPTION Depends may omit a
                // NAMESPACE (Quarto/Shiny projects commonly do). Depends are
                // attached before package code runs, unlike Imports.
                file_attached.extend(read_description_packages(&root).depends);
            }
            if source_package_lazy_data(&root) {
                file_bindings.extend(
                    dataset_cache
                        .entry(root.clone())
                        .or_insert_with(|| {
                            source_package_datasets(
                                &root,
                                MAX_SERIALIZED_BYTES.load(Ordering::Relaxed),
                            )
                        })
                        .iter()
                        .cloned(),
                );
            }
            let sysdata = root.join("R/sysdata.rda");
            file_bindings.extend(
                serialized_cache
                    .entry(sysdata.clone())
                    .or_insert_with(|| {
                        serialized_bindings(&sysdata, MAX_SERIALIZED_BYTES.load(Ordering::Relaxed))
                    })
                    .iter()
                    .cloned(),
            );
            load_bindings.insert(
                file.path.clone(),
                loaded_serialized_bindings(
                    file,
                    &root,
                    &project_attached,
                    user_stubs,
                    MAX_SERIALIZED_BYTES.load(Ordering::Relaxed),
                    &mut serialized_cache,
                ),
            );

            if relative.is_some_and(is_test_or_script_file) {
                // Loading the package under test also attaches its Depends;
                // tests and user-facing package scripts additionally use
                // DESCRIPTION Suggests as their working set. Imports remain
                // excluded: they only provide bare names through explicit
                // NAMESPACE directives.
                let dependencies = read_description_packages(&root);
                let test_dependencies = dependencies
                    .depends
                    .into_iter()
                    .chain(dependencies.suggests)
                    .collect::<HashSet<_>>();
                for package in &test_dependencies {
                    // Without a stub, an attached test dependency can
                    // supply arbitrary exports. This is intentionally a
                    // file-local open search path, never a project-wide
                    // promotion.
                    if !user_stubs.contains_key(package)
                        && ry_typeshed::load_package(package).is_none()
                    {
                        file_bindings.insert(SERIALIZED_BINDINGS_UNENUMERABLE.to_string());
                    }
                }
                file_attached.extend(test_dependencies);
                file_attached.insert("testthat".to_string());
            }
            if relative.is_some_and(|path| path.starts_with("tests/testthat")) {
                let helpers = testthat_helper_context(&root);
                file_bindings.extend(helpers.bindings);
                file_attached.extend(helpers.attached);
            }
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
            if let Some(typeshed) = user_stubs
                .get(package)
                .or_else(|| ry_typeshed::load_package(package))
            {
                file_bindings.extend(typeshed.functions.keys().cloned());
                file_bindings.extend(typeshed.globals.ambient_functions.iter().cloned());
            }
        }
        attached.extend(file_attached.iter().cloned());
        bare_attached.insert(file.path.clone(), file_attached);
        bindings.insert(file.path.clone(), file_bindings);
        imported_from.insert(file.path.clone(), file_imported_from);
        s3_methods.insert(file.path.clone(), file_s3_methods);
    }
    PackageScope {
        attached,
        bare_attached,
        bindings,
        imported_from,
        s3_methods,
        load_bindings,
    }
}

/// Whether a path relative to a package root is source code in `R/`.
fn is_package_r_file(path: &Path) -> bool {
    path.components()
        .next()
        .is_some_and(|component| component.as_os_str() == "R")
}

/// Whether a path relative to a package root has the execution context used
/// for tests, installed scripts, demos, or vignettes.
fn is_test_or_script_file(path: &Path) -> bool {
    matches!(
        path.components()
            .next()
            .and_then(|component| component.as_os_str().to_str()),
        Some("tests" | "inst" | "demo" | "vignettes")
    )
}

/// Bindings introduced by R's literal-name namespace helpers. These calls are
/// deliberately collected from every source file below `R/`, rather than only
/// the files being checked: package load hooks commonly call a helper defined
/// in a different file. We never evaluate source, and only retain literal
/// names, so an unknown dynamic name cannot mask an unresolved variable.
fn source_package_namespace_bindings(root: &Path) -> HashSet<String> {
    // R creates this binding while loading every package namespace. It is
    // present even when the DESCRIPTION omits a Package field.
    let mut bindings = source_package_dynamic_bindings(root);
    bindings.insert(".packageName".to_string());
    bindings
}

fn source_package_dynamic_bindings(root: &Path) -> HashSet<String> {
    let mut bindings = HashSet::new();
    let mut paths = Vec::new();
    collect_r_source_files(&root.join("R"), &mut paths);
    let Ok(mut parser) = ry_core::RParser::new() else {
        return bindings;
    };
    for path in paths {
        let Ok(source) = std::fs::read_to_string(&path) else {
            continue;
        };
        let Ok(file) = parser.parse(&path.to_string_lossy(), &source) else {
            continue;
        };
        for statement in &file.stmts {
            collect_dynamic_bindings_stmt(statement, 0, &mut bindings);
        }
    }
    bindings
}

fn collect_r_source_files(directory: &Path, paths: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(directory) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_r_source_files(&path, paths);
        } else if matches!(
            path.extension().and_then(|extension| extension.to_str()),
            Some("R") | Some("r")
        ) {
            paths.push(path);
        }
    }
}

fn collect_dynamic_bindings_stmt(
    statement: &Stmt,
    function_depth: usize,
    bindings: &mut HashSet<String>,
) {
    match statement {
        Stmt::Assign { target, value, .. } => {
            collect_dynamic_bindings_expr(target, function_depth, bindings);
            collect_dynamic_bindings_expr(value, function_depth, bindings);
        }
        Stmt::Expr(expr) => collect_dynamic_bindings_expr(expr, function_depth, bindings),
        Stmt::If {
            cond, then, else_, ..
        } => {
            collect_dynamic_bindings_expr(cond, function_depth, bindings);
            for statement in then {
                collect_dynamic_bindings_stmt(statement, function_depth, bindings);
            }
            if let Some(else_) = else_ {
                for statement in else_ {
                    collect_dynamic_bindings_stmt(statement, function_depth, bindings);
                }
            }
        }
        Stmt::For { iter, body, .. } => {
            collect_dynamic_bindings_expr(iter, function_depth, bindings);
            for statement in body {
                collect_dynamic_bindings_stmt(statement, function_depth, bindings);
            }
        }
        Stmt::While { cond, body, .. } => {
            collect_dynamic_bindings_expr(cond, function_depth, bindings);
            for statement in body {
                collect_dynamic_bindings_stmt(statement, function_depth, bindings);
            }
        }
        Stmt::FunctionDef { body, .. } => {
            for statement in body {
                collect_dynamic_bindings_stmt(statement, function_depth + 1, bindings);
            }
        }
        Stmt::Return { value, .. } => {
            if let Some(value) = value {
                collect_dynamic_bindings_expr(value, function_depth, bindings);
            }
        }
    }
}

fn collect_dynamic_bindings_expr(
    expr: &Expr,
    function_depth: usize,
    bindings: &mut HashSet<String>,
) {
    match expr {
        Expr::Call { func, args, .. } => {
            if let Expr::Ident { name, .. } = func.as_ref() {
                let has_named_environment = args.iter().any(|argument| {
                    matches!(
                        argument.name.as_deref(),
                        Some("envir" | "env" | "assign.env")
                    )
                });
                // The environment parameter is commonly passed positionally
                // from .onLoad helpers (for example `assign("x", value,
                // env)`). Treat only its documented position as explicit;
                // a two-argument assign inside a function remains local.
                let has_positional_environment = match name.as_str() {
                    "assign" | "makeActiveBinding" => {
                        args.get(2).is_some_and(|arg| arg.name.is_none())
                    }
                    "delayedAssign" => args.get(3).is_some_and(|arg| arg.name.is_none()),
                    _ => false,
                };
                if matches!(
                    name.as_str(),
                    "assign" | "makeActiveBinding" | "delayedAssign"
                ) && (has_named_environment
                    || has_positional_environment
                    || (name == "assign" && function_depth == 0))
                    && let Some(Expr::String(binding, _)) =
                        args.first().map(|argument| &argument.value)
                {
                    bindings.insert(binding.clone());
                }
            }
            collect_dynamic_bindings_expr(func, function_depth, bindings);
            for argument in args {
                collect_dynamic_bindings_expr(&argument.value, function_depth, bindings);
            }
        }
        Expr::BinOp { lhs, rhs, .. } => {
            collect_dynamic_bindings_expr(lhs, function_depth, bindings);
            collect_dynamic_bindings_expr(rhs, function_depth, bindings);
        }
        Expr::UnaryOp { expr, .. } => collect_dynamic_bindings_expr(expr, function_depth, bindings),
        Expr::Index { base, args, .. } => {
            collect_dynamic_bindings_expr(base, function_depth, bindings);
            for argument in args {
                collect_dynamic_bindings_expr(&argument.value, function_depth, bindings);
            }
        }
        Expr::Function { body, .. } | Expr::Block { body, .. } => {
            let function_depth =
                function_depth + usize::from(matches!(expr, Expr::Function { .. }));
            for statement in body {
                collect_dynamic_bindings_stmt(statement, function_depth, bindings);
            }
        }
        Expr::If {
            cond, then, else_, ..
        } => {
            collect_dynamic_bindings_expr(cond, function_depth, bindings);
            collect_dynamic_bindings_expr(then, function_depth, bindings);
            if let Some(else_) = else_ {
                collect_dynamic_bindings_expr(else_, function_depth, bindings);
            }
        }
        Expr::Logical(_, _)
        | Expr::Integer(_, _)
        | Expr::Double(_, _)
        | Expr::String(_, _)
        | Expr::Null(_)
        | Expr::Na(_, _)
        | Expr::Ident { .. }
        | Expr::Unknown(_) => {}
    }
}

#[derive(Default)]
struct DescriptionPackages {
    depends: HashSet<String>,
    suggests: HashSet<String>,
}

fn read_description_packages(root: &Path) -> DescriptionPackages {
    let Ok(text) = std::fs::read_to_string(root.join("DESCRIPTION")) else {
        return DescriptionPackages::default();
    };
    let mut fields: HashMap<String, String> = HashMap::new();
    let mut current = None::<String>;
    for line in text.lines() {
        if line.starts_with([' ', '\t']) {
            if let Some(name) = &current {
                fields
                    .entry(name.clone())
                    .or_default()
                    .push_str(line.trim());
            }
        } else if let Some((name, value)) = line.split_once(':') {
            current = Some(name.to_string());
            fields.insert(name.to_string(), value.trim().to_string());
        }
    }
    let packages = |field: &str| {
        fields
            .get(field)
            .into_iter()
            .flat_map(|value| value.split(','))
            .filter_map(|entry| entry.split_whitespace().next())
            .filter(|name| !name.is_empty() && *name != "R")
            .map(str::to_string)
            .collect()
    };
    DescriptionPackages {
        depends: packages("Depends"),
        suggests: packages("Suggests"),
    }
}

#[derive(Default)]
struct TestthatHelperContext {
    bindings: HashSet<String>,
    attached: HashSet<String>,
}

fn testthat_helper_context(root: &Path) -> TestthatHelperContext {
    let directory = root.join("tests/testthat");
    let Ok(entries) = std::fs::read_dir(directory) else {
        return TestthatHelperContext::default();
    };
    let mut context = TestthatHelperContext::default();
    let Ok(mut parser) = ry_core::RParser::new() else {
        return context;
    };
    for path in entries.flatten().map(|entry| entry.path()) {
        let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
            continue;
        };
        if !(name.starts_with("helper") || name.starts_with("setup"))
            || !matches!(
                path.extension().and_then(|ext| ext.to_str()),
                Some("R") | Some("r")
            )
        {
            continue;
        }
        let Ok(source) = std::fs::read_to_string(&path) else {
            continue;
        };
        let Ok(file) = parser.parse(&path.to_string_lossy(), &source) else {
            continue;
        };
        context
            .attached
            .extend(ry_checker::packages::attached_packages(&file));
        context
            .bindings
            .extend(file.stmts.iter().filter_map(|statement| match statement {
                Stmt::Assign {
                    target: Expr::Ident { name, .. },
                    ..
                } => Some(name.clone()),
                Stmt::FunctionDef {
                    name: Some(name), ..
                } => Some(name.clone()),
                _ => None,
            }));
    }
    context
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
fn source_package_datasets(root: &Path, max_serialized_bytes: u64) -> HashSet<String> {
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
                let serialized = serialized_bindings(&path, max_serialized_bytes);
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
fn serialized_bindings(path: &Path, cap: u64) -> HashSet<String> {
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
    let bindings = serialized_bindings_uncached(path, cap);
    cache
        .lock()
        .expect("serialized cache poisoned")
        .insert(key, bindings.clone());
    bindings
}

fn serialized_bindings_uncached(path: &Path, cap: u64) -> HashSet<String> {
    fn unenumerable() -> HashSet<String> {
        HashSet::from([SERIALIZED_BINDINGS_UNENUMERABLE.to_string()])
    }

    let Ok(bytes) = std::fs::read(path) else {
        return HashSet::new();
    };
    let bytes = if bytes.starts_with(b"BZh") {
        let mut decoded = Vec::new();
        let decoder = bzip2::read::BzDecoder::new(bytes.as_slice());
        if decoder.take(cap + 1).read_to_end(&mut decoded).is_err() {
            return HashSet::new();
        }
        if decoded.len() as u64 > cap {
            return unenumerable();
        }
        decoded
    } else if bytes.starts_with(&[0x1f, 0x8b]) {
        let mut decoded = Vec::new();
        let decoder = flate2::read::GzDecoder::new(bytes.as_slice());
        if decoder.take(cap + 1).read_to_end(&mut decoded).is_err() {
            return HashSet::new();
        }
        if decoded.len() as u64 > cap {
            return unenumerable();
        }
        decoded
    } else if bytes.starts_with(&[0xfd, b'7', b'z', b'X', b'Z', 0x00]) {
        let mut decoded = Vec::new();
        let decoder = xz2::read::XzDecoder::new(bytes.as_slice());
        if decoder.take(cap + 1).read_to_end(&mut decoded).is_err() {
            return HashSet::new();
        }
        if decoded.len() as u64 > cap {
            return unenumerable();
        }
        decoded
    } else {
        if bytes.len() as u64 > cap {
            return unenumerable();
        }
        bytes
    };
    let payload = bytes
        .strip_prefix(b"RDX2\n")
        .or_else(|| bytes.strip_prefix(b"RDX3\n"))
        .unwrap_or(&bytes);
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
    user_stubs: &std::collections::BTreeMap<String, ry_typeshed::Typeshed>,
    max_serialized_bytes: u64,
    cache: &mut HashMap<PathBuf, HashSet<String>>,
) -> HashMap<usize, HashSet<String>> {
    fn resolve_path(
        expr: &Expr,
        file: &SourceFile,
        package_root: &Path,
        attached_packages: &HashSet<String>,
        user_stubs: &std::collections::BTreeMap<String, ry_typeshed::Typeshed>,
    ) -> Option<PathBuf> {
        let (path, source_relative_only) = match expr {
            Expr::String(path, _) => (path, false),
            Expr::Call { func, args, .. } => {
                let Expr::Ident { name, .. } = func.as_ref() else {
                    return None;
                };
                let signature = if let Some((package, function)) = name.rsplit_once("::") {
                    let package = package.trim_end_matches(':');
                    user_stubs
                        .get(package)
                        .or_else(|| ry_typeshed::load_package(package))
                        .and_then(|typeshed| typeshed.functions.get(function))
                } else {
                    attached_packages.iter().find_map(|package| {
                        user_stubs
                            .get(package)
                            .or_else(|| ry_typeshed::load_package(package))
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
            resolve_path(
                &argument.value,
                file,
                package_root,
                attached_packages,
                user_stubs,
            )
        }) {
            let loaded = cache
                .entry(path.clone())
                .or_insert_with(|| serialized_bindings(&path, max_serialized_bytes))
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
        .find(|dir| dir.join("DESCRIPTION").is_file())
        .map(Path::to_path_buf)
}

/// Candidate R library roots that can be inspected without starting R.
/// The depth is only for layouts whose version/platform directories sit
/// between the root and the package directory.
fn r_library_roots(all_paths: &[PathBuf]) -> Vec<LibraryRoot> {
    // Hermetic mode: resolve nothing from the machine's R installation.
    // The ecosystem regression harness sets this so committed snapshots
    // do not depend on which packages happen to be installed locally.
    if std::env::var_os("RY_NO_INSTALLED_LIBRARIES").is_some_and(|v| !v.is_empty() && v != "0") {
        return Vec::new();
    }
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

    fn write_oversized_rdata(path: &Path) {
        let object = rds2rust::RObject::Pairlist(vec![rds2rust::PairlistElement {
            tag: Some(Arc::from("small_tag")),
            value: rds2rust::RObject::Null,
            tag_object: None,
        }]);
        let gzip = rds2rust::write_rds(&object).unwrap();
        let mut serialization = Vec::new();
        flate2::read::GzDecoder::new(gzip.as_slice())
            .read_to_end(&mut serialization)
            .unwrap();
        let mut rdata = b"RDX2\n".to_vec();
        rdata.extend_from_slice(&serialization);
        rdata.resize(2 * 1024 * 1024 + 1, 0);
        let mut encoder = bzip2::write::BzEncoder::new(Vec::new(), bzip2::Compression::best());
        encoder.write_all(&rdata).unwrap();
        std::fs::write(path, encoder.finish().unwrap()).unwrap();
    }

    fn package_bindings(root: &Path, source: &str) -> HashSet<String> {
        let path = root.join("R/use.R");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, source).unwrap();
        let mut parser = ry_core::RParser::new().unwrap();
        let file = parser.parse(&path.to_string_lossy(), source).unwrap();
        resolve(
            std::slice::from_ref(&path),
            &[],
            &[],
            &std::collections::BTreeMap::new(),
            [&file],
        )
        .bindings
        .remove(&path.to_string_lossy().to_string())
        .unwrap_or_default()
    }

    #[test]
    fn collects_literal_assign_with_environment_at_function_depth() {
        let root = tempfile::tempdir().unwrap();
        std::fs::write(root.path().join("DESCRIPTION"), "Package: fixture\n").unwrap();
        let bindings = package_bindings(
            root.path(),
            "setup <- function(env) { assign(\"from_helper\", 1, envir = env) }\nuse <- from_helper\n",
        );
        assert!(bindings.contains("from_helper"));
    }

    #[test]
    fn ignores_dynamic_assign_names_and_function_local_assigns() {
        let root = tempfile::tempdir().unwrap();
        std::fs::write(root.path().join("DESCRIPTION"), "Package: fixture\n").unwrap();
        let bindings = package_bindings(
            root.path(),
            "setup <- function(env, names) { assign(names[[1]], 1, envir = env); assign(\"local_only\", 1) }\n",
        );
        assert!(!bindings.contains("local_only"));
        assert!(!bindings.contains("names"));
    }

    #[test]
    fn collects_active_and_delayed_literal_bindings() {
        let root = tempfile::tempdir().unwrap();
        std::fs::write(root.path().join("DESCRIPTION"), "Package: fixture\n").unwrap();
        let bindings = package_bindings(
            root.path(),
            "setup <- function(env) { makeActiveBinding(\"active\", function() 1, envir = env); delayedAssign(\"delayed\", 1, assign.env = env) }\n",
        );
        assert!(bindings.contains("active"));
        assert!(bindings.contains("delayed"));
    }

    #[test]
    fn collects_literal_bindings_with_positional_environment() {
        let root = tempfile::tempdir().unwrap();
        std::fs::write(root.path().join("DESCRIPTION"), "Package: fixture\n").unwrap();
        let bindings = package_bindings(
            root.path(),
            "setup <- function(env) { assign(\"assigned\", 1, env); makeActiveBinding(\"active\", function() 1, env); delayedAssign(\"delayed\", 1, parent.frame(), env) }\n",
        );
        assert!(bindings.contains("assigned"));
        assert!(bindings.contains("active"));
        assert!(bindings.contains("delayed"));
    }

    #[test]
    fn injects_package_name_only_for_package_roots() {
        let package = tempfile::tempdir().unwrap();
        std::fs::write(package.path().join("DESCRIPTION"), "Package: fixture\n").unwrap();
        assert!(source_package_namespace_bindings(package.path()).contains(".packageName"));

        assert!(
            r_package_root(Path::new("/nonexistent-ry-script-root/script.R")).is_none(),
            "a directory without DESCRIPTION must not receive package bindings"
        );
    }

    #[test]
    fn inventories_rdx2_and_rdx3_pairlist_tags() {
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
        let dir = tempfile::tempdir().unwrap();
        for header in [b"RDX2\n".as_slice(), b"RDX3\n".as_slice()] {
            let mut rdata = header.to_vec();
            rdata.extend_from_slice(&serialization);
            let mut encoder = bzip2::write::BzEncoder::new(Vec::new(), bzip2::Compression::best());
            encoder.write_all(&rdata).unwrap();
            let compressed = encoder.finish().unwrap();
            let path = dir
                .path()
                .join(format!("objects-{}.rda", header[3] as char));
            std::fs::write(&path, compressed).unwrap();

            assert_eq!(
                serialized_bindings(&path, 2 * 1024 * 1024),
                HashSet::from(["alpha".to_string(), "beta".to_string()])
            );
        }
    }

    #[test]
    fn oversized_serialized_workspace_is_unenumerable() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("oversized.rda");
        write_oversized_rdata(&path);

        assert_eq!(
            serialized_bindings(&path, 2 * 1024 * 1024),
            HashSet::from([SERIALIZED_BINDINGS_UNENUMERABLE.to_string()])
        );
    }

    #[test]
    fn oversized_sysdata_opens_the_package_file_scope() {
        let root = tempfile::tempdir().unwrap();
        std::fs::write(root.path().join("DESCRIPTION"), "Package: fixture\n").unwrap();
        let source_path = root.path().join("R/use.R");
        std::fs::create_dir_all(source_path.parent().unwrap()).unwrap();
        std::fs::write(&source_path, "arbitrary_sysdata_name\n").unwrap();
        write_oversized_rdata(&root.path().join("R/sysdata.rda"));

        let mut parser = ry_core::RParser::new().unwrap();
        let file = parser
            .parse(&source_path.to_string_lossy(), "arbitrary_sysdata_name\n")
            .unwrap();
        let scope = resolve(
            std::slice::from_ref(&source_path),
            &[],
            &[],
            &std::collections::BTreeMap::new(),
            [&file],
        );
        let mut project = ry_checker::Project::new();
        project.add_file(source_path.to_string_lossy().to_string(), file);
        project.set_external_bindings(scope.bindings);
        let diagnostics = project.check();

        assert!(
            diagnostics[0]
                .1
                .iter()
                .all(|diagnostic| diagnostic.code != "RY010"),
            "oversized sysdata should open the file scope: {diagnostics:?}"
        );
    }

    #[test]
    fn user_stub_override_drives_source_relative_load_resolution() {
        let dir = tempfile::tempdir().unwrap();
        let source_path = dir.path().join("script.R");
        let data_path = dir.path().join("objects.rda");
        std::fs::write(&data_path, "invalid fixture is enough for path resolution").unwrap();
        std::fs::write(
            dir.path().join("custom.json"),
            r#"{
                "schema_version": "1",
                "package": "custom",
                "version": "test",
                "functions": {
                    "fixture": {
                        "params": ["path"],
                        "return": {"mode": "character", "length": "1"},
                        "source_relative_path_arg": 0
                    }
                }
            }"#,
        )
        .unwrap();
        let user_stubs = ry_typeshed::load_stub_dir(dir.path()).unwrap();
        let mut parser = ry_core::RParser::new().unwrap();
        let file = parser
            .parse(
                &source_path.to_string_lossy(),
                "load(custom::fixture(\"objects.rda\"))\n",
            )
            .unwrap();
        let bindings = loaded_serialized_bindings(
            &file,
            dir.path(),
            &HashSet::new(),
            &user_stubs,
            2 * 1024 * 1024,
            &mut HashMap::new(),
        );
        assert_eq!(
            bindings.len(),
            1,
            "custom signature should resolve the path"
        );
    }
}
