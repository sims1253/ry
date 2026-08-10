//! Filesystem-backed R package/workspace scope discovery shared by CLI and LSP.
//!
//! Package code is never loaded or evaluated. We parse project and installed
//! NAMESPACE files as R syntax, then turn proven imports/exports into opaque
//! checker bindings.

use ry_checker::SERIALIZED_BINDINGS_UNENUMERABLE;
use ry_checker::packages::NamespaceMetadata;
use ry_core::SourceFile;
use ry_core::ast::{Expr, Stmt};
use std::collections::{BTreeSet, HashMap, HashSet};
use std::io::Read;
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
    /// Per-file native routine names proven by `useDynLib` metadata.
    pub native_registrations: HashMap<String, HashSet<String>>,
    pub s3_methods: HashMap<String, HashSet<(String, String)>>,
    pub load_bindings: HashMap<String, HashMap<usize, HashSet<String>>>,
    pub degraded_scopes: Vec<(PathBuf, &'static str)>,
}

#[derive(Debug, thiserror::Error)]
pub enum ResolveError {
    #[error("workspace root is not a directory: {0}")]
    InvalidRoot(PathBuf),
}

/// Inventory of a serialized R data file (`.rda`/`.rdata`). `bindings`
/// are the enumerated object names, or a single file-stem fallback when
/// the decoded payload exceeds the byte cap. `degraded` is true when
/// enumeration was skipped, which means the binding set is an
/// approximation rather than the real object names.
#[derive(Clone)]
struct SerializedInventory {
    bindings: HashSet<String>,
    degraded: bool,
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
/// serialized workspace cannot be enumerated. A package's `sysdata.rda`
/// over the byte cap used to disable RY010 for every file in the package
/// (via [`SERIALIZED_BINDINGS_UNENUMERABLE`]); reducing it to its stem
/// (`sysdata`) keeps unbound-variable analysis live and only masks the
/// single colliding name.
fn file_stem_binding(path: &Path) -> HashSet<String> {
    path.file_stem()
        .and_then(|stem| stem.to_str())
        .map(|stem| HashSet::from([stem.to_string()]))
        .unwrap_or_default()
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
    let mut serialized_cache: HashMap<PathBuf, SerializedInventory> = HashMap::new();
    let mut source_binding_cache: HashMap<PathBuf, SourceBindings> = HashMap::new();
    let mut attached = HashSet::new();
    let mut bare_attached = HashMap::new();
    let mut bindings = HashMap::new();
    let mut imported_from = HashMap::new();
    let mut native_registrations = HashMap::new();
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
                .flat_map(|file| ry_checker::packages::attached_packages(file)),
        )
        .collect();

    for file in files {
        let mut file_attached: HashSet<String> = configured_packages.iter().cloned().collect();
        let mut file_bindings = HashSet::new();
        let mut file_s3_methods = HashSet::new();
        let mut file_native_registrations = HashSet::new();
        let mut file_imported_from = HashMap::new();
        let mut source_package = None;
        file_bindings.extend(configured_globals.iter().cloned());
        for profile in &config.environments {
            if profile.paths.iter().any(|pattern| {
                file.path
                    .replace('\\', "/")
                    .contains(pattern.trim_end_matches("/**"))
            }) {
                file_bindings.extend(profile.bindings.iter().cloned());
            }
        }
        if let Some(root) = r_package_root(Path::new(&file.path)) {
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
                file_native_registrations.extend(source_bindings.native_symbols.iter().cloned());
                file_native_registrations.extend(metadata.native_routines.iter().cloned());
                file_bindings.extend(file_native_registrations.iter().cloned());
            }
            file_bindings.extend(metadata.imported_bindings.iter().cloned());
            file_imported_from.extend(metadata.imported_from.clone());
            file_bindings.extend(metadata.s3_generics.iter().cloned());
            file_bindings.extend(metadata.native_routines.iter().cloned());
            file_bindings.extend(metadata.native_routine_prefixes.iter().map(|prefix| {
                format!(
                    "{}{prefix}",
                    ry_checker::packages::NATIVE_ROUTINE_PREFIX_SENTINEL
                )
            }));
            if metadata.native_registration {
                file_bindings
                    .insert(ry_checker::packages::NATIVE_REGISTRATION_SENTINEL.to_string());
            }
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
            let sysdata_inventory = serialized_cache
                .entry(sysdata.clone())
                .or_insert_with(|| serialized_inventory(&sysdata, max_serialized_bytes))
                .clone();
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
                &mut serialized_cache,
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
                file_bindings.extend(typeshed.datasets.keys().cloned());
                file_bindings.extend(typeshed.globals.ambient_functions.iter().cloned());
            }
        }
        attached.extend(file_attached.iter().cloned());
        bare_attached.insert(file.path.clone(), file_attached);
        bindings.insert(file.path.clone(), file_bindings);
        imported_from.insert(file.path.clone(), file_imported_from);
        native_registrations.insert(file.path.clone(), file_native_registrations);
        s3_methods.insert(file.path.clone(), file_s3_methods);
    }
    Ok(WorkspaceContext {
        attached_packages: attached,
        bare_bindings: bare_attached,
        external_bindings: bindings,
        imported_bindings: imported_from,
        native_registrations,
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
/// What a scan of a package's own `R/` sources establishes.
#[derive(Default, Clone)]
struct SourceBindings {
    /// Names bound dynamically (`assign`, `delayedAssign`,
    /// `makeActiveBinding`) into the package namespace.
    bindings: HashSet<String>,
    /// Names used as the entry-point argument of an FFI primitive somewhere
    /// in the package, which proves they are native routines rather than
    /// ordinary variables. Only meaningful when the NAMESPACE declares
    /// `useDynLib(..., .registration = TRUE)`; see [`resolve`].
    native_symbols: HashSet<String>,
}

fn source_package_namespace_bindings(root: &Path) -> SourceBindings {
    // R creates this binding while loading every package namespace. It is
    // present even when the DESCRIPTION omits a Package field.
    let mut found = source_package_dynamic_bindings(root);
    found.bindings.insert(".packageName".to_string());
    found
}

fn source_package_dynamic_bindings(root: &Path) -> SourceBindings {
    let mut found = SourceBindings::default();
    let mut paths = Vec::new();
    collect_r_source_files(&root.join("R"), &mut paths);
    let Ok(mut parser) = ry_core::RParser::new() else {
        return found;
    };
    for path in paths {
        let Ok(source) = std::fs::read_to_string(&path) else {
            continue;
        };
        let Ok(file) = parser.parse(&path.to_string_lossy(), &source) else {
            continue;
        };
        for statement in &file.stmts {
            collect_dynamic_bindings_stmt(statement, 0, &mut found);
        }
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

fn collect_dynamic_bindings_stmt(
    statement: &Stmt,
    function_depth: usize,
    found: &mut SourceBindings,
) {
    match statement {
        Stmt::Assign { target, value, .. } => {
            collect_dynamic_bindings_expr(target, function_depth, found);
            collect_dynamic_bindings_expr(value, function_depth, found);
        }
        Stmt::Expr(expr) => collect_dynamic_bindings_expr(expr, function_depth, found),
        Stmt::If {
            cond, then, else_, ..
        } => {
            collect_dynamic_bindings_expr(cond, function_depth, found);
            for statement in then {
                collect_dynamic_bindings_stmt(statement, function_depth, found);
            }
            if let Some(else_) = else_ {
                for statement in else_ {
                    collect_dynamic_bindings_stmt(statement, function_depth, found);
                }
            }
        }
        Stmt::For { iter, body, .. } => {
            collect_dynamic_bindings_expr(iter, function_depth, found);
            for statement in body {
                collect_dynamic_bindings_stmt(statement, function_depth, found);
            }
        }
        Stmt::While { cond, body, .. } => {
            collect_dynamic_bindings_expr(cond, function_depth, found);
            for statement in body {
                collect_dynamic_bindings_stmt(statement, function_depth, found);
            }
        }
        Stmt::FunctionDef { body, .. } => {
            for statement in body {
                collect_dynamic_bindings_stmt(statement, function_depth + 1, found);
            }
        }
        Stmt::Return { value, .. } => {
            if let Some(value) = value {
                collect_dynamic_bindings_expr(value, function_depth, found);
            }
        }
    }
}

fn collect_dynamic_bindings_expr(expr: &Expr, function_depth: usize, found: &mut SourceBindings) {
    match expr {
        Expr::Call { func, args, .. } => {
            if let Expr::Ident { name, .. } = func.as_ref() {
                // `.Call(ffi_enquo, ...)` proves `ffi_enquo` names a native
                // routine, not a variable. rlang then passes the same symbol
                // as an ordinary value (`capture_arg = ffi_enquo`), which the
                // call-position rule alone cannot see. Record the witness so
                // every later use of the name resolves.
                if ry_checker::packages::FFI_PRIMITIVES.contains(&name.as_str())
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
                    found.bindings.insert(binding.clone());
                }
            }
            collect_dynamic_bindings_expr(func, function_depth, found);
            for argument in args {
                collect_dynamic_bindings_expr(&argument.value, function_depth, found);
            }
        }
        Expr::BinOp { lhs, rhs, .. } => {
            collect_dynamic_bindings_expr(lhs, function_depth, found);
            collect_dynamic_bindings_expr(rhs, function_depth, found);
        }
        Expr::UnaryOp { expr, .. } => collect_dynamic_bindings_expr(expr, function_depth, found),
        Expr::Index { base, args, .. } => {
            collect_dynamic_bindings_expr(base, function_depth, found);
            for argument in args {
                collect_dynamic_bindings_expr(&argument.value, function_depth, found);
            }
        }
        Expr::Function { body, .. } | Expr::Block { body, .. } => {
            let function_depth =
                function_depth + usize::from(matches!(expr, Expr::Function { .. }));
            for statement in body {
                collect_dynamic_bindings_stmt(statement, function_depth, found);
            }
        }
        Expr::If {
            cond, then, else_, ..
        } => {
            collect_dynamic_bindings_expr(cond, function_depth, found);
            collect_dynamic_bindings_expr(then, function_depth, found);
            if let Some(else_) = else_ {
                collect_dynamic_bindings_expr(else_, function_depth, found);
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
                let Ok(source) = std::fs::read_to_string(&path) else {
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

/// Read only the top-level tags from an R serialization stream. `.rda`
/// workspaces are serialized pairlists whose tags are the binding names. The
/// parser's lazy mode skips vector payload allocation, and bzip2 streams are
/// decompressed in-process; no R runtime or project code is executed.
///
/// Returns the enumerated object names plus a `degraded` flag. When the
/// decoded payload exceeds the byte cap, enumeration is skipped and the
/// binding set is reduced to a single file-stem fallback ([`file_stem_binding`])
/// so unbound-variable analysis (RY010) stays live instead of being disabled
/// package-wide (the previous behavior via [`SERIALIZED_BINDINGS_UNENUMERABLE`]).
fn serialized_inventory(path: &Path, cap: u64) -> SerializedInventory {
    /// What the cached inventory was derived from. A mismatch on any field
    /// means the entry is stale. `cap` is part of it because raising
    /// `max-serialized-bytes` must re-enumerate a file that was previously
    /// reduced to its stem.
    type Stamp = (u64, u128, u64);
    static CACHE: std::sync::OnceLock<
        std::sync::Mutex<HashMap<PathBuf, (Stamp, SerializedInventory)>>,
    > = std::sync::OnceLock::new();
    let Ok(metadata) = std::fs::metadata(path) else {
        return SerializedInventory {
            bindings: HashSet::new(),
            degraded: false,
        };
    };
    let modified = metadata
        .modified()
        .ok()
        .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|duration| duration.as_nanos())
        .unwrap_or_default();
    let stamp: Stamp = (metadata.len(), modified, cap);
    let cache = CACHE.get_or_init(|| std::sync::Mutex::new(HashMap::new()));
    if let Some(inventory) = cache
        .lock()
        .expect("serialized cache poisoned")
        .get(path)
        .filter(|(cached, _)| *cached == stamp)
        .map(|(_, inventory)| inventory.clone())
    {
        return inventory;
    }
    let inventory = serialized_inventory_uncached(path, cap);
    // Keyed on the path, not on (path, stamp): a long-lived LSP session
    // re-checks the same data files after every edit, and keying on the
    // stamp would retain one binding set per historical version forever.
    // Replacing the entry bounds the cache by the number of distinct files.
    cache
        .lock()
        .expect("serialized cache poisoned")
        .insert(path.to_path_buf(), (stamp, inventory.clone()));
    inventory
}

fn serialized_inventory_uncached(path: &Path, cap: u64) -> SerializedInventory {
    // Decoded payload exceeded the byte cap: rather than disabling
    // unbound-variable analysis package-wide (the former
    // `SERIALIZED_BINDINGS_UNENUMERABLE` path), fall back to the file
    // stem as a single conservative binding. Callers flag the scope as
    // degraded so the user knows RY010 precision dropped for that file.
    let degraded = |path: &Path| SerializedInventory {
        bindings: file_stem_binding(path),
        degraded: true,
    };
    let empty = || SerializedInventory {
        bindings: HashSet::new(),
        degraded: false,
    };

    let read_cap = cap.saturating_add(1);
    let bytes = match std::fs::read(path) {
        Ok(bytes) => bytes,
        Err(_) => return empty(),
    };
    let bytes = if bytes.starts_with(b"BZh") {
        let mut decoded = Vec::new();
        let decoder = bzip2::read::BzDecoder::new(bytes.as_slice());
        if decoder.take(read_cap).read_to_end(&mut decoded).is_err() {
            return empty();
        }
        if decoded.len() as u64 > cap {
            return degraded(path);
        }
        decoded
    } else if bytes.starts_with(&[0x1f, 0x8b]) {
        let mut decoded = Vec::new();
        let decoder = flate2::read::GzDecoder::new(bytes.as_slice());
        if decoder.take(read_cap).read_to_end(&mut decoded).is_err() {
            return empty();
        }
        if decoded.len() as u64 > cap {
            return degraded(path);
        }
        decoded
    } else if bytes.starts_with(&[0xfd, b'7', b'z', b'X', b'Z', 0x00]) {
        let mut decoded = Vec::new();
        let decoder = xz2::read::XzDecoder::new(bytes.as_slice());
        if decoder.take(read_cap).read_to_end(&mut decoded).is_err() {
            return empty();
        }
        if decoded.len() as u64 > cap {
            return degraded(path);
        }
        decoded
    } else {
        // An uncompressed file already has a known size from `metadata.len()`
        // checked in the cached wrapper. Short-circuit before parsing if it
        // exceeds the cap — no need to read the whole payload.
        if bytes.len() as u64 > cap {
            return degraded(path);
        }
        bytes
    };
    let payload = bytes
        .strip_prefix(b"RDX2\n")
        .or_else(|| bytes.strip_prefix(b"RDX3\n"))
        .unwrap_or(&bytes);
    let Ok(parsed) = rds2rust::read_rds_lazy(payload) else {
        return empty();
    };
    let bindings = match parsed.object.into_concrete() {
        rds2rust::RObject::Pairlist(elements) => elements
            .into_iter()
            .filter_map(|element| element.tag.map(|tag| tag.to_string()))
            .collect(),
        _ => HashSet::new(),
    };
    SerializedInventory {
        bindings,
        degraded: false,
    }
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
    cache: &mut HashMap<PathBuf, SerializedInventory>,
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
            let inventory = cache
                .entry(path.clone())
                .or_insert_with(|| serialized_inventory(&path, max_serialized_bytes))
                .clone();
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
pub fn is_file_eligible_with_excludes(
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
// P36-W7 — Shared, bounded directory discovery (#48)
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

impl Default for DiscoveryLimits {
    fn default() -> Self {
        Self::from_config(&ry_config::Config::default())
    }
}

/// Structured report when a discovery cap is hit (P36-W7).
/// A cap hit is never silent: the caller emits a tracing event,
/// LSP warning, or CLI warning based on this report.
#[derive(Clone, Debug, Default)]
pub struct TruncationReport {
    /// The root at which the cap was hit.
    pub root: PathBuf,
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

    /// Total number of files omitted across all caps.
    pub fn omitted_count(&self) -> usize {
        // `max_files_hit` means we stopped, so the omitted count is
        // unbounded from the walker's perspective; the caller reports
        // the count it can determine. Here we count the known omissions.
        self.oversized_files.len()
    }
}

/// Result of a bounded directory discovery.
#[derive(Clone, Debug, Default)]
pub struct DiscoveryResult {
    /// Discovered R source file paths (sorted and deduplicated).
    pub files: Vec<PathBuf>,
    /// Structured cap report. Empty when no limit was reached.
    pub truncated: TruncationReport,
}

/// Discover all eligible R source files under `walk_root`, applying the
/// same eligibility, extension, hidden-directory, symlink, exclude, and
/// test-fixture rules to both CLI and LSP (P36-W7 / issue #48).
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
            truncated: TruncationReport {
                root: walk_root.parent().unwrap_or(walk_root).to_path_buf(),
                ..Default::default()
            },
        };
    }
    let limits = DiscoveryLimits::from_config(config);
    let excludes = ry_config::Excludes::from_config(config);
    let has_excludes = !excludes.is_empty();
    let mut files = Vec::new();
    let mut truncated = TruncationReport {
        root: walk_root.to_path_buf(),
        ..Default::default()
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
    DiscoveryResult { files, truncated }
}

#[allow(clippy::too_many_arguments)]
fn discover_recursive(
    dir: &Path,
    out: &mut Vec<PathBuf>,
    truncated: &mut TruncationReport,
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
        if file_type.is_symlink() {
            continue;
        }
        let path = entry.path();
        // Apply ry.toml exclude patterns (relative to the config root).
        if has_excludes
            && let Some(anchor) = exclude_root
            && !is_file_eligible_with_excludes(&path, anchor, excludes)
        {
            continue;
        }
        // Apply .Rbuildignore patterns (relative to the package root).
        if package_root.is_some_and(|root| is_rbuildignored(root, &path, buildignore)) {
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
                continue;
            }
            if package_root.is_some_and(|root| is_excluded_package_directory(root, &path)) {
                continue;
            }
            // P36-W7: depth cap prunes further descent.
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
                nested_package_root.as_deref(),
                &nested_buildignore,
                check_test_fixtures,
                depth + 1,
                limits,
                excludes,
                has_excludes,
                exclude_root,
            );
        } else if matches!(
            path.extension().and_then(|e| e.to_str()),
            Some("R") | Some("r") | Some("S") | Some("s") | Some("q")
        ) && (check_test_fixtures
            || ry_checker::project::package_file_kind(&path)
                != ry_checker::project::PackageFileKind::TestFixture)
        {
            // P36-W7: max-files cap.
            if out.len() >= limits.max_files {
                truncated.max_files_hit = true;
                continue;
            }
            // P36-W7: max-file-bytes cap.
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

#[cfg(test)]
mod shared_tests {
    use super::*;

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
        let dir = tempfile::tempdir().unwrap();
        for ext in &["R", "r", "S", "s", "q"] {
            std::fs::write(
                dir.path().join(format!("source.{ext}")),
                "value <- 1
",
            )
            .unwrap();
        }
        std::fs::write(
            dir.path().join("source.txt"),
            "not R
",
        )
        .unwrap();
        let result = discover_r_files(dir.path(), None, &ry_config::Config::default(), false);
        assert_eq!(
            result.files.len(),
            5,
            "R, r, S, s, q discovered; .txt excluded"
        );
    }
}
