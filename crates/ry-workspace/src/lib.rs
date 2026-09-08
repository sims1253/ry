//! Filesystem-backed R package/workspace scope discovery shared by CLI and LSP.
//!
//! Package code is never loaded or evaluated. We parse project and installed
//! NAMESPACE files as R syntax, then turn proven imports/exports into opaque
//! checker bindings.

mod discovery;
pub use discovery::{
    DiscoveryLimits, DiscoveryResult, TruncationReport, discover_r_files, is_file_eligible,
    rbuildignore_pattern,
};
use discovery::{is_r_source_name, is_testthat_code_name};

pub mod packages;
mod serialized;

use serialized::serialized_inventory;

pub use packages::{
    NATIVE_REGISTRATION_SENTINEL, NATIVE_ROUTINE_PREFIX_SENTINEL, NamespaceMetadata,
    attached_packages, namespace_metadata,
};
pub use ry_core::FFI_PRIMITIVES;

use ry_core::SERIALIZED_BINDINGS_UNENUMERABLE;
use ry_core::SourceFile;
use ry_core::ast::{Expr, Stmt};
use ry_core::walk::{AstNode, Descend, Walk, walk_stmts};
use std::collections::{BTreeSet, HashMap, HashSet};
use std::ops::ControlFlow;
use std::path::{Path, PathBuf};

/// Inputs which describe the analysis environment without evaluating R code.
pub struct ResolutionEnvironment<'a> {
    pub files: Vec<&'a SourceFile>,
    pub user_stubs: &'a std::collections::BTreeMap<String, ry_typeshed::Typeshed>,
}

/// Filesystem-derived state applied to a checker `Project`.
#[derive(Clone, Debug, Default)]
pub struct WorkspaceContext {
    pub attached_packages: HashSet<String>,
    pub bare_bindings: HashMap<String, HashSet<String>>,
    pub external_bindings: HashMap<String, HashSet<String>>,
    pub imported_bindings: HashMap<String, HashMap<String, String>>,
    pub s3_methods: HashMap<String, HashSet<(String, String)>>,
    pub load_bindings: HashMap<String, HashMap<usize, HashSet<String>>>,
    pub degraded_scopes: Vec<(PathBuf, &'static str)>,
}

#[derive(Debug, thiserror::Error)]
pub enum ResolveError {
    #[error("workspace root is not a directory: {0}")]
    InvalidRoot(PathBuf),
    #[error("invalid environment path pattern: {0}")]
    InvalidEnvironmentPattern(#[from] glob::PatternError),
}

/// Inventory of a directory of data files (`data/`, `R/sysdata.rda`).
/// `bindings` aggregates the per-file object names (or file-stem
/// fallbacks); `degraded` lists files that exceeded the byte cap.
#[derive(Clone, Default)]
struct DataInventory {
    bindings: HashSet<String>,
    degraded: Vec<PathBuf>,
}

/// A single file-stem binding, used as the conservative fallback when a
/// serialized workspace cannot be enumerated within the byte cap. The
/// bare file stem (`sysdata`) keeps unbound-variable analysis live and
/// only masks the single colliding name.
fn file_stem_binding(path: &Path) -> HashSet<String> {
    path.file_stem()
        .and_then(|stem| stem.to_str())
        .map(|stem| HashSet::from([stem.to_string()]))
        .unwrap_or_default()
}

/// Read R source as UTF-8, falling back to Latin-1 for invalid UTF-8.
/// Both frontends use this policy for on-disk source; open editor buffers
/// already arrive as Unicode through LSP.
pub fn read_r_source(path: &Path) -> std::io::Result<String> {
    let bytes = std::fs::read(path)?;
    Ok(String::from_utf8(bytes)
        .unwrap_or_else(|error| error.into_bytes().into_iter().map(char::from).collect()))
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

pub fn resolve_workspace_context<'a>(
    root: &Path,
    config: &ry_config::Config,
    environment: ResolutionEnvironment<'a>,
) -> Result<WorkspaceContext, ResolveError> {
    if !root.is_dir() {
        return Err(ResolveError::InvalidRoot(root.to_path_buf()));
    }
    let profiles = config
        .environments
        .iter()
        .map(|profile| {
            let patterns = profile
                .paths
                .iter()
                .map(|pattern| glob::Pattern::new(&pattern.replace('\\', "/")))
                .collect::<Result<Vec<_>, _>>()?;
            let anchor = profile.root.as_deref().unwrap_or(root);
            let anchor = anchor
                .canonicalize()
                .unwrap_or_else(|_| anchor.to_path_buf());
            Ok((profile, patterns, anchor))
        })
        .collect::<Result<Vec<_>, glob::PatternError>>()?;
    let files = environment.files;
    let user_stubs = environment.user_stubs;
    let all_paths: Vec<PathBuf> = files.iter().map(|file| PathBuf::from(&file.path)).collect();
    let configured_packages = &config.packages;
    let configured_globals = &config.globals;
    let max_serialized_bytes = config.max_serialized_bytes;
    let library_roots = r_library_roots(&all_paths);
    let preferred_version = current_r_minor_version(&library_roots);
    let mut namespace_cache: HashMap<PathBuf, NamespaceMetadata> = HashMap::new();
    let mut export_cache: HashMap<String, HashSet<String>> = HashMap::new();
    let mut dataset_cache: HashMap<PathBuf, DataInventory> = HashMap::new();
    let mut source_binding_cache: HashMap<PathBuf, SourceBindings> = HashMap::new();
    // A package root is visited once per file in it, so cache the
    // DESCRIPTION read like the sibling namespace/dataset caches.
    let mut description_cache: HashMap<PathBuf, DescriptionPackages> = HashMap::new();
    // The ancestor walk to the enclosing package root is identical for
    // every file in one directory; cache its result per directory so a
    // many-file directory does not repeat the same stat calls per file.
    let mut package_root_cache: HashMap<PathBuf, Option<PathBuf>> = HashMap::new();
    let mut attached = HashSet::new();
    let mut bare_attached = HashMap::new();
    let mut bindings = HashMap::new();
    let mut imported_from = HashMap::new();
    let mut s3_methods = HashMap::new();
    let mut load_bindings = HashMap::new();
    // A package root is visited once per file in it, so a single oversized
    // dataset would otherwise be reported once per file. Deduplicate on the
    // (path, reason) pair; the CLI prints one line per entry.
    let mut degraded: BTreeSet<(PathBuf, &'static str)> = BTreeSet::new();
    let project_attached: HashSet<String> = configured_packages
        .iter()
        .cloned()
        .chain(
            files
                .iter()
                .flat_map(|file| packages::attached_packages(file)),
        )
        .collect();

    for file in files {
        let mut file_attached: HashSet<String> = configured_packages.iter().cloned().collect();
        let mut file_bindings = HashSet::new();
        let mut file_s3_methods = HashSet::new();
        let mut file_imported_from = HashMap::new();
        let mut source_package = None;
        file_bindings.extend(configured_globals.iter().cloned());
        if !profiles.is_empty()
            && let Ok(path) = Path::new(&file.path)
                .canonicalize()
                .or_else(|_| std::path::absolute(&file.path))
        {
            for (profile, patterns, anchor) in &profiles {
                let Ok(relative) = path.strip_prefix(anchor) else {
                    continue;
                };
                let relative = relative.to_string_lossy().replace('\\', "/");
                if patterns.iter().any(|pattern| {
                    pattern.matches_with(
                        &relative,
                        glob::MatchOptions {
                            require_literal_separator: true,
                            ..Default::default()
                        },
                    )
                }) {
                    file_bindings.extend(profile.bindings.iter().cloned());
                }
            }
        }
        if let Some(root) = package_root_cache
            .entry(
                Path::new(&file.path)
                    .parent()
                    .unwrap_or_else(|| Path::new(""))
                    .to_path_buf(),
            )
            .or_insert_with(|| r_package_root(Path::new(&file.path)))
            .clone()
        {
            let source_bindings = source_binding_cache
                .entry(root.clone())
                .or_insert_with(|| source_package_namespace_bindings(&root))
                .clone();
            file_bindings.extend(source_bindings.bindings.iter().cloned());
            if let Some(package) = source_package_name(&root) {
                file_attached.insert(package.clone());
                source_package = Some(package);
            }
            let metadata = namespace_cache
                .entry(root.clone())
                .or_insert_with(|| read_namespace(&root.join("NAMESPACE")));
            // `useDynLib(pkg, .registration = TRUE)` binds every routine in
            // the package's `R_registerRoutines` table into the namespace.
            // ry does not read `src/`, so the witness set collected above --
            // names this package itself passes as an FFI entry point -- is
            // the evidence that a name is one of them. Without the
            // declaration the same names are ordinary unbound reads.
            if metadata.native_registration {
                file_bindings.extend(source_bindings.native_symbols.iter().cloned());
                file_bindings.extend(metadata.native_routines.iter().cloned());
            }
            file_bindings.extend(metadata.imported_bindings.iter().cloned());
            file_imported_from.extend(metadata.imported_from.clone());
            // Registering an S3 method does not install the generic as a
            // namespace binding. Real imports/dynamic bindings remain above.
            file_bindings.extend(metadata.native_routines.iter().cloned());
            file_bindings.extend(
                metadata
                    .native_routine_prefixes
                    .iter()
                    .map(|prefix| format!("{}{prefix}", packages::NATIVE_ROUTINE_PREFIX_SENTINEL)),
            );
            if metadata.native_registration {
                file_bindings.insert(packages::NATIVE_REGISTRATION_SENTINEL.to_string());
            }
            file_s3_methods.extend(metadata.s3_methods.iter().cloned());
            // `import(pkg)` puts pkg's exports in the package namespace, not
            // on the search path. Two execution contexts still resolve those
            // names: the package's own `R/` sources, and the testthat runner
            // files testthat sources into `env_clone(asNamespace(package))` —
            // that clone's parent chain includes the namespace's imports
            // environment (verified in R: names reachable through
            // `parent.env(asNamespace(pkg))` resolve from the clone).
            // `importFrom()` bindings above remain available wherever the
            // package context makes them meaningful.
            let relative = Path::new(&file.path).strip_prefix(&root).ok();
            if relative.is_some_and(is_package_r_file)
                || relative.is_some_and(is_testthat_runner_file)
            {
                file_attached.extend(metadata.imported_packages.iter().cloned());
                // Packages that rely on DESCRIPTION Depends may omit a
                // NAMESPACE (Quarto/Shiny projects commonly do). Depends are
                // attached before package code runs, unlike Imports.
                file_attached.extend(
                    description_cache
                        .entry(root.clone())
                        .or_insert_with(|| read_description_packages(&root))
                        .depends
                        .clone(),
                );
            }
            if source_package_lazy_data(&root) {
                let datasets = dataset_cache
                    .entry(root.clone())
                    .or_insert_with(|| source_package_datasets(&root, max_serialized_bytes))
                    .clone();
                file_bindings.extend(datasets.bindings.iter().cloned());
                for path in &datasets.degraded {
                    degraded.insert((path.clone(), "oversized dataset in data/"));
                }
            }
            let sysdata = root.join("R/sysdata.rda");
            let sysdata_inventory = serialized_inventory(&sysdata, max_serialized_bytes);
            file_bindings.extend(sysdata_inventory.bindings.iter().cloned());
            if sysdata_inventory.degraded {
                degraded.insert((sysdata, "oversized R/sysdata.rda"));
            }
            let loaded = loaded_serialized_bindings(
                file,
                &root,
                &project_attached,
                user_stubs,
                max_serialized_bytes,
            );
            for path in &loaded.degraded {
                degraded.insert((path.clone(), "oversized load() target"));
            }
            load_bindings.insert(file.path.clone(), loaded.per_span);

            if relative.is_some_and(is_test_or_script_file) {
                // Loading the package under test also attaches its Depends;
                // tests and user-facing package scripts additionally use
                // DESCRIPTION Suggests as their working set. Imports remain
                // excluded: they only provide bare names through explicit
                // NAMESPACE directives.
                let dependencies = description_cache
                    .entry(root.clone())
                    .or_insert_with(|| read_description_packages(&root))
                    .clone();
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
        file_attached.extend(packages::attached_packages(file));
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
                file_bindings.extend(typeshed.datasets.keys().cloned());
                file_bindings.extend(typeshed.globals.ambient_functions.iter().cloned());
            }
        }
        attached.extend(file_attached.iter().cloned());
        bare_attached.insert(file.path.clone(), file_attached);
        bindings.insert(file.path.clone(), file_bindings);
        imported_from.insert(file.path.clone(), file_imported_from);
        s3_methods.insert(file.path.clone(), file_s3_methods);
    }
    Ok(WorkspaceContext {
        attached_packages: attached,
        bare_bindings: bare_attached,
        external_bindings: bindings,
        imported_bindings: imported_from,
        s3_methods,
        load_bindings,
        degraded_scopes: degraded.into_iter().collect(),
    })
}

/// Whether a path relative to a package root is source code in `R/`.
fn is_package_r_file(path: &Path) -> bool {
    path.components()
        .next()
        .is_some_and(|component| component.as_os_str() == "R")
}

/// Whether a path relative to a package root is testthat runner code: the
/// `tests/testthat/` files testthat itself sources. testthat executes them in
/// the environment returned by its `test_env(package)`, a clone of the
/// package namespace whose parent chain includes the namespace's imports
/// environment — so names supplied by NAMESPACE `import(pkg)` resolve there
/// exactly as they do in `R/` sources. The classification mirrors
/// [`discovery::is_test_fixture`]'s documented-contract prefixes: the same file set
/// discovery treats as executable test code rather than data. Files at the
/// `tests/` root are excluded: `R CMD check` runs those in the global
/// environment after `library(package)`, where wholesale imports stay
/// namespace-internal and invisible.
fn is_testthat_runner_file(path: &Path) -> bool {
    let components: Vec<&str> = path
        .components()
        .filter_map(|component| component.as_os_str().to_str())
        .collect();
    matches!(components.as_slice(), ["tests", "testthat", file]
        if is_r_source_name(file) && is_testthat_code_name(file))
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

/// What a scan of a package's own `R/` sources establishes.
#[derive(Default, Clone)]
struct SourceBindings {
    /// Names bound dynamically (`assign`, `delayedAssign`,
    /// `makeActiveBinding`) into the package namespace.
    bindings: HashSet<String>,
    /// Names used as the entry-point argument of an FFI primitive somewhere
    /// in the package, which proves they are native routines rather than
    /// ordinary variables. Only meaningful when the NAMESPACE declares
    /// `useDynLib(..., .registration = TRUE)`; see [`resolve_workspace_context`].
    native_symbols: HashSet<String>,
}

fn source_package_namespace_bindings(root: &Path) -> SourceBindings {
    // R creates this binding while loading every package namespace. It is
    // present even when the DESCRIPTION omits a Package field.
    let mut found = source_package_dynamic_bindings(root);
    found.bindings.insert(".packageName".to_string());
    found
}

/// Collect the bindings introduced by R's literal-name namespace helpers.
/// These calls are deliberately collected from every source file below `R/`,
/// rather than only the files being checked: package load hooks commonly call
/// a helper defined in a different file. We never evaluate source, and only
/// retain literal names, so an unknown dynamic name cannot mask an unresolved
/// variable.
fn source_package_dynamic_bindings(root: &Path) -> SourceBindings {
    let mut found = SourceBindings::default();
    let mut paths = Vec::new();
    collect_r_source_files(&root.join("R"), &mut paths);
    let Ok(mut parser) = ry_core::RParser::new() else {
        return found;
    };
    for path in paths {
        let Ok(source) = read_r_source(&path) else {
            continue;
        };
        let Ok(file) = parser.parse(&path.to_string_lossy(), &source) else {
            continue;
        };
        collect_dynamic_bindings_stmts(&file.stmts, &mut found);
    }
    found
}

fn collect_r_source_files(directory: &Path, paths: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(directory) else {
        return;
    };
    for entry in entries.flatten() {
        // Skip symlinks to prevent infinite recursion through symlink loops.
        if entry.file_type().map(|ft| ft.is_symlink()).unwrap_or(false) {
            continue;
        }
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

/// Collect the dynamic-binding calls from one file's statements.
/// Walks every subtree including function bodies; the walker's
/// `fn_depth` (number of enclosing function bodies) decides whether a
/// bare two-argument `assign("x", v)` still targets the namespace:
/// only the file's top level does.
fn collect_dynamic_bindings_stmts(stmts: &[Stmt], found: &mut SourceBindings) {
    let _ = walk_stmts(stmts, Walk::ALL, |node: AstNode<'_>, fn_depth: usize| {
        let AstNode::Expr(Expr::Call { func, args, .. }) = node else {
            return ControlFlow::<(), Descend>::Continue(Descend::Into);
        };
        let Expr::Ident { name, .. } = func.as_ref() else {
            return ControlFlow::<(), Descend>::Continue(Descend::Into);
        };
        // `.Call(ffi_enquo, ...)` proves `ffi_enquo` names a native
        // routine, not a variable. rlang then passes the same symbol
        // as an ordinary value (`capture_arg = ffi_enquo`), which the
        // call-position rule alone cannot see. Record the witness so
        // every later use of the name resolves.
        if FFI_PRIMITIVES.contains(&name.as_str())
            && let Some(Expr::Ident { name: symbol, .. }) = args
                .first()
                .filter(|arg| arg.name.is_none())
                .map(|arg| &arg.value)
        {
            found.native_symbols.insert(symbol.clone());
        }
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
            "assign" | "makeActiveBinding" => args.get(2).is_some_and(|arg| arg.name.is_none()),
            "delayedAssign" => args.get(3).is_some_and(|arg| arg.name.is_none()),
            _ => false,
        };
        if matches!(
            name.as_str(),
            "assign" | "makeActiveBinding" | "delayedAssign"
        ) && (has_named_environment
            || has_positional_environment
            || (name == "assign" && fn_depth == 0))
            && let Some(Expr::String(binding, _)) = args.first().map(|argument| &argument.value)
        {
            found.bindings.insert(binding.clone());
        }
        ControlFlow::<(), Descend>::Continue(Descend::Into)
    });
}

#[derive(Default, Clone)]
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
        let Ok(source) = read_r_source(&path) else {
            continue;
        };
        let Ok(file) = parser.parse(&path.to_string_lossy(), &source) else {
            continue;
        };
        context.attached.extend(packages::attached_packages(&file));
        context
            .bindings
            .extend(file.stmts.iter().filter_map(|statement| match statement {
                Stmt::Assign {
                    target: Expr::Ident { name, .. },
                    ..
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
fn source_package_datasets(root: &Path, max_serialized_bytes: u64) -> DataInventory {
    let Ok(entries) = std::fs::read_dir(root.join("data")) else {
        return DataInventory::default();
    };
    let mut out = DataInventory::default();
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
                let inventory = serialized_inventory(&path, max_serialized_bytes);
                if inventory.bindings.is_empty() {
                    if let Some(stem) = path.file_stem().and_then(|stem| stem.to_str()) {
                        out.bindings.insert(stem.to_string());
                    }
                } else {
                    out.bindings.extend(inventory.bindings);
                }
                if inventory.degraded {
                    out.degraded.push(path);
                }
            }
            "rds" => {
                if let Some(stem) = path.file_stem().and_then(|stem| stem.to_str()) {
                    out.bindings.insert(stem.to_string());
                }
            }
            "r" => {
                let Ok(source) = read_r_source(&path) else {
                    continue;
                };
                let Ok(mut parser) = ry_core::RParser::new() else {
                    continue;
                };
                let Ok(file) = parser.parse(&path.to_string_lossy(), &source) else {
                    continue;
                };
                out.bindings
                    .extend(file.stmts.iter().filter_map(|statement| match statement {
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
    out
}

/// Per-file `load()` resolution result. `per_span` maps each `load()`
/// call's start span to the bindings it introduces; `degraded` lists any
/// target workspaces that exceeded the byte cap.
struct LoadedInventory {
    per_span: HashMap<usize, HashSet<String>>,
    degraded: Vec<PathBuf>,
}

fn loaded_serialized_bindings(
    file: &SourceFile,
    package_root: &Path,
    attached_packages: &HashSet<String>,
    user_stubs: &std::collections::BTreeMap<String, ry_typeshed::Typeshed>,
    max_serialized_bytes: u64,
) -> LoadedInventory {
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
    let mut out = LoadedInventory {
        per_span: HashMap::new(),
        degraded: Vec::new(),
    };
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
            let inventory = serialized_inventory(&path, max_serialized_bytes);
            if inventory.degraded {
                out.degraded.push(path);
            }
            out.per_span.insert(span.start, inventory.bindings);
        }
    }
    out
}

/// Parse an R NAMESPACE file with the regular R parser. This handles quoted
/// names, comments, and multiline directives without a second parser.
fn read_namespace(path: &Path) -> NamespaceMetadata {
    let Ok(src) = read_r_source(path) else {
        return NamespaceMetadata::default();
    };
    let Ok(mut parser) = ry_core::RParser::new() else {
        return NamespaceMetadata::default();
    };
    let Ok(file) = parser.parse(&path.to_string_lossy(), &src) else {
        return NamespaceMetadata::default();
    };
    packages::namespace_metadata(&file)
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
    // The renv probe walks each file's ancestor directories; files in
    // one directory share the walk, so probe each distinct directory
    // once instead of once per file.
    let mut seen_parents: HashSet<&Path> = HashSet::new();
    for path in all_paths {
        let start = if path.is_dir() {
            path.as_path()
        } else if let Some(parent) = path.parent() {
            parent
        } else {
            continue;
        };
        if !seen_parents.insert(start) {
            continue;
        }
        if let Some(renv) = start
            .ancestors()
            .map(|ancestor| ancestor.join("renv/library"))
            .find(|candidate| candidate.is_dir())
            && seen_renv.insert(renv.clone())
        {
            roots.push(LibraryRoot::nested(renv, 3));
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
        if path.is_dir()
            && let Some(found) =
                find_package_namespace(&path, package, depth - 1, preferred_version)
        {
            return Some(found);
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
mod dynamic_binding_tests {
    use super::*;

    fn collect_from(src: &str) -> SourceBindings {
        let mut parser = ry_core::RParser::new().unwrap();
        let file = parser.parse("dynamic_binding_test.R", src).unwrap();
        let mut found = SourceBindings::default();
        collect_dynamic_bindings_stmts(&file.stmts, &mut found);
        found
    }

    fn assert_exact(src: &str, expected: &[&str]) {
        let found = collect_from(src).bindings;
        let expected: HashSet<String> = expected.iter().map(|name| name.to_string()).collect();
        assert_eq!(found, expected, "bindings from `{src}`");
    }

    /// A bare two-argument `assign("x", v)` targets the package namespace
    /// only at the file top level (`fn_depth == 0`; braced blocks do not
    /// count). Inside a function body the same call binds in that call's
    /// execution environment and records nothing.
    #[test]
    fn bare_assign_records_only_at_top_level() {
        assert_exact("assign(\"top\", value)", &["top"]);
        assert_exact("{ assign(\"in_block\", value) }", &["in_block"]);
        assert_exact(
            "on_load <- function() assign(\"nested\", value)
assign(\"top\", value)",
            &["top"],
        );
    }

    /// Only `assign` has the top-level bare arm: bare
    /// `makeActiveBinding`/`delayedAssign` record nothing even at the
    /// file top level.
    #[test]
    fn bare_make_active_binding_and_delayed_assign_never_record() {
        assert_exact(
            "makeActiveBinding(\"active\", getter)
delayedAssign(\"later\", value)",
            &[],
        );
    }

    /// A named `envir`/`env`/`assign.env` argument records at any depth,
    /// whatever the environment expression is -- `asNamespace(...)`,
    /// `globalenv()`, or a namespace variable threaded through an
    /// `.onLoad` helper.
    #[test]
    fn named_environment_argument_records_inside_function_bodies() {
        assert_exact(
            "on_load <- function(libname, pkgname) {
  assign(\"ns_var\", 1, envir = asNamespace(\"pkg\"))
  assign(\"global_var\", 1, envir = globalenv())
  assign(\"env_alias\", 1, env = ns)
  makeActiveBinding(\"active\", getter, assign.env = ns)
}
",
            &["ns_var", "global_var", "env_alias", "active"],
        );
    }

    /// The environment passed positionally -- third argument of
    /// `assign`/`makeActiveBinding`, fourth of `delayedAssign` -- also
    /// records inside function bodies, matching `.onLoad` helpers that
    /// thread the namespace through positionally.
    #[test]
    fn positional_environment_argument_records_inside_function_bodies() {
        assert_exact(
            "on_load <- function(libname, pkgname) {
  assign(\"positional\", 1, ns)
  makeActiveBinding(\"lazy_active\", getter, ns)
  delayedAssign(\"lazy_later\", value, NULL, ns)
}
",
            &["positional", "lazy_active", "lazy_later"],
        );
    }

    /// A named third argument that is not an environment alias
    /// (`inherits = TRUE`) leaves the call bare for depth purposes:
    /// ignored inside a function body, recorded at the file top level by
    /// the `assign`-only arm.
    #[test]
    fn named_non_environment_argument_stays_depth_gated() {
        assert_exact("f <- function() assign(\"flag\", 1, inherits = TRUE)", &[]);
        assert_exact("assign(\"flag\", 1, inherits = TRUE)", &["flag"]);
    }

    /// Only a literal string target is statically knowable. An
    /// identifier target computes the binding name at runtime and
    /// records nothing -- even at the top level with an explicit
    /// environment, so an unknown dynamic name cannot mask an unresolved
    /// variable.
    #[test]
    fn non_literal_target_names_record_nothing() {
        assert_exact("assign(name_var, value)", &[]);
        assert_exact("assign(name_var, value, envir = ns)", &[]);
        assert_exact("f <- function() assign(name_var, value, envir = ns)", &[]);
    }

    /// `.Call(ffi_enquo, ...)` proves `ffi_enquo` names a native routine
    /// rather than a variable (rlang later passes the same symbol as an
    /// ordinary value). Every FFI primitive records its first argument
    /// when it is an unnamed symbol; a string entry point or a named
    /// first argument is not a symbol witness.
    #[test]
    fn ffi_primitives_record_unnamed_symbol_first_arguments() {
        assert_eq!(
            collect_from(".Call(ffi_enquo, quote(arg))").native_symbols,
            HashSet::from(["ffi_enquo".to_string()])
        );
        assert_eq!(
            collect_from(".External2(entry, x)").native_symbols,
            HashSet::from(["entry".to_string()])
        );
        assert!(
            collect_from(".Call(\"as_string\", x)")
                .native_symbols
                .is_empty()
        );
        assert!(
            collect_from(".Call(name = ffi_enquo, x)")
                .native_symbols
                .is_empty()
        );
    }
}

#[cfg(test)]
mod input_tests {
    use super::*;

    #[test]
    fn source_decoding_is_shared_and_read_errors_remain_errors() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("source.R");
        for bytes in ["café <- 1\n".as_bytes(), b"caf\xe9 <- 1\n"] {
            std::fs::write(&path, bytes).unwrap();
            assert_eq!(read_r_source(&path).unwrap(), "café <- 1\n");
        }
        assert!(read_r_source(&dir.path().join("missing.R")).is_err());
    }
}

#[cfg(test)]
mod s3_binding_provenance_tests {
    use super::*;

    #[test]
    fn registration_keeps_method_metadata_without_creating_generic_bindings() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir(dir.path().join("R")).unwrap();
        std::fs::write(
            dir.path().join("DESCRIPTION"),
            "Package: fixture\nVersion: 0.0.0\n",
        )
        .unwrap();
        let path = dir.path().join("R/code.R");
        for (namespace, source, expected_binding) in [
            (
                "S3method(\"+\", foo)\nS3method(custom, foo)\n",
                "x <- 1L",
                false,
            ),
            (
                "S3method(\"+\", foo)\nimportFrom(otherpkg, \"+\")\n",
                "x <- 1L",
                true,
            ),
            (
                "S3method(\"+\", foo)\n",
                "assign('+', function(...) 1L, envir = environment())",
                true,
            ),
        ] {
            std::fs::write(dir.path().join("NAMESPACE"), namespace).unwrap();
            std::fs::write(&path, source).unwrap();
            let path = path.to_str().unwrap();
            let file = ry_core::RParser::new()
                .unwrap()
                .parse(path, source)
                .unwrap();
            let context = resolve_workspace_context(
                dir.path(),
                &ry_config::Config::default(),
                ResolutionEnvironment {
                    files: vec![&file],
                    user_stubs: &Default::default(),
                },
            )
            .unwrap();
            assert_eq!(
                context.external_bindings[path].contains("+"),
                expected_binding
            );
            assert!(!context.external_bindings[path].contains("custom"));
            assert!(context.s3_methods[path].contains(&("+".into(), "foo".into())));
            if namespace.contains("importFrom") {
                assert_eq!(context.imported_bindings[path]["+"], "otherpkg");
            }
        }
    }
}
